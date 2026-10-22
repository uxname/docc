//! The history of a project for a program that shows it itself: docc log
//! (the commits along the first-parent chain with the files each one changed
//! and how many blocks) and docc diff (the blocks of two revisions lined up).
//! Both follow blocks through history::ops, the one alignment of docc.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::config::Config;
use crate::engine;
use crate::history::{self, Applied, Leaf, Options, Replay, cat_files};
use crate::render::{self, CBlock, CPage, Section};

/// LOG_SCHEMA_ID and DIFF_SCHEMA_ID are the "$schema" of the outputs and the
/// "$id" of their JSON Schemas (`docc schema log|diff`).
pub const LOG_SCHEMA_ID: &str = "urn:docc:schema:log:1";
pub const DIFF_SCHEMA_ID: &str = "urn:docc:schema:diff:1";

type Error = Box<dyn std::error::Error + Send + Sync>;

/// git runs git in dir: its standard output, or the first line of its error.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").args(args).current_dir(dir).output().map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("git failed").trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// resolve is the full hash of a revision: its commit, or its tree for a
/// tree (the empty tree of a root commit).
fn resolve(dir: &Path, rev: &str) -> Result<String, String> {
    if rev.is_empty() || rev.starts_with('-') {
        return Err(format!("{rev:?} is not a Git revision"));
    }
    ["commit", "tree"]
        .iter()
        .find_map(|t| git(dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{{t}}}")]).ok())
        .map(|s| s.trim().to_string())
        .ok_or_else(|| format!("no such Git revision {rev:?}"))
}

/// stem is the document id of a path: the file name without .json.
fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".json").unwrap_or(name).to_string()
}

fn status_of(s: u8) -> &'static str {
    match s {
        b'A' | b'C' => "A",
        b'D' => "D",
        b'R' => "R",
        _ => "M",
    }
}

#[derive(Serialize)]
pub struct LogOut {
    #[serde(rename = "$schema")]
    schema: &'static str,
    pub commits: Vec<LogCommit>,
}

#[derive(Serialize)]
pub struct Person {
    name: String,
    email: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogCommit {
    pub sha: String,
    parents: Vec<String>,
    author: Person,
    committer: Person,
    pub date: String,
    pub subject: String,
    body: String,
    comments_only: bool,
    pub files: Vec<LogFile>,
}

/// Counts are the blocks a change added, changed and removed.
#[derive(Serialize, Default, Clone, Copy)]
pub struct Counts {
    pub add: usize,
    pub ch: usize,
    pub rm: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFile {
    pub status: &'static str,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    pub blocks: Counts,
    comments_only: bool,
    /// the new version is not a JSON object
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    broken: bool,
    /// the document is hidden from the readers ("hidden": true)
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
}

/// block_leaf reports whether a leaf counts as a block of the page: the
/// body and the free fields, not the title and metadata, and not empty.
fn block_leaf(ptr: &str, text: &str) -> bool {
    let top = ptr[1.min(ptr.len())..].split('/').next().unwrap_or("").replace("~1", "/").replace("~0", "~");
    !text.is_empty() && render::block_field(&top)
}

/// counts are the block counts of the last step of st when it was commit c.
fn counts(st: &Replay, c: usize) -> Counts {
    let mut n = Counts::default();
    let Some(ch) = st.doc.changes.last().filter(|ch| ch.c == c) else {
        return n;
    };
    if ch.created {
        n.add = st.leaves.iter().filter(|l| block_leaf(&l.ptr, &l.text)).count();
        return n;
    }
    for v in &ch.changed {
        match (block_leaf(&v[0], &v[1]), block_leaf(&v[0], &v[2])) {
            (true, true) => n.ch += 1,
            (false, true) => n.add += 1,
            (true, false) => n.rm += 1,
            _ => {}
        }
    }
    n.add += ch.added.iter().filter(|v| block_leaf(&v[0], &v[1])).count();
    n.rm += ch.removed.iter().filter(|v| block_leaf(&v[0], &v[1])).count();
    n
}

fn title_of(st: &Replay) -> Option<String> {
    st.leaves.iter().find(|l| l.ptr == "/title").map(|l| l.text.clone())
}

/// comments_only reports whether the only change between two versions of a
/// JSON file is inside its top-level "$comments".
fn comments_only(old: &[u8], new: &[u8]) -> bool {
    let (Ok(Value::Object(mut a)), Ok(Value::Object(mut b))) =
        (serde_json::from_slice::<Value>(old), serde_json::from_slice::<Value>(new))
    else {
        return false;
    };
    let (ca, cb) = (a.remove("$comments"), b.remove("$comments"));
    a == b && ca != cb
}

/// log is the history of the src folder of cfg along the first-parent chain
/// of rev, newest first, stopping before since. One git log and one cat-file
/// read it; with since, the files it touches start from their version there.
pub fn log(cfg: &Config, rev: &str, since: &str) -> Result<LogOut, String> {
    let (dir, prefix) = history::locate_git(&cfg.abs(&cfg.src))?;
    let head = resolve(&dir, rev)?;
    let mut revs = vec![head.clone()];
    let mut base = String::new();
    if !since.is_empty() {
        base = resolve(&dir, since)?;
        let chain = git(&dir, &["rev-list", "--first-parent", &head])?;
        if !chain.lines().any(|l| l == base) {
            return Err(format!("--since {since:?} is not on the first-parent history of {rev:?}"));
        }
        revs.push(format!("^{base}"));
    }
    let revs: Vec<&str> = revs.iter().map(String::as_str).collect();
    let (metas, entries) = history::read_log(&dir, &revs, prefix.as_bytes()).map_err(|e| format!("git log: {e}"))?;
    // the version at since of every file the range finds there
    let mut seeds: Vec<(&str, &[u8])> = Vec::new();
    if !base.is_empty() {
        let mut seen = std::collections::HashSet::new();
        for e in &entries {
            let (path, git_path) = if e.status == b'R' { (&e.old, &e.git_old) } else { (&e.path, &e.git_path) };
            if seen.insert(path.as_str()) && e.status != b'A' && path.ends_with(".json") {
                seeds.push((path, git_path));
            }
            seen.insert(e.path.as_str());
        }
    }
    let mut req = Vec::new();
    for (_, gp) in &seeds {
        req.extend_from_slice(format!("{base}:").as_bytes());
        req.extend_from_slice(gp);
        req.push(b'\n');
    }
    for e in entries.iter().filter(|e| e.status != b'D' && e.path.ends_with(".json")) {
        req.extend_from_slice(format!("{}:", metas[e.c].hash).as_bytes());
        req.extend_from_slice(&e.git_path);
        req.push(b'\n');
    }
    let mut blobs = if req.is_empty() { Vec::new() } else { cat_files(&dir, req).map_err(|e| e.to_string())? }.into_iter();
    let o = Options::default();
    let mut state: HashMap<String, Replay> = HashMap::new();
    let mut raw: HashMap<String, Vec<u8>> = HashMap::new();
    for (path, _) in &seeds {
        if let Some(data) = blobs.next().flatten() {
            let e = history::Entry { c: usize::MAX, status: b'A', old: String::new(), path: path.to_string(), git_path: Vec::new(), git_old: Vec::new() };
            history::apply(&mut state, &e, Some(&data), &o);
            raw.insert(path.to_string(), data);
        }
    }
    let mut commits: Vec<LogCommit> = metas
        .into_iter()
        .map(|m| LogCommit {
            sha: m.hash,
            parents: m.parents,
            author: Person { name: m.author.0, email: m.author.1 },
            committer: Person { name: m.committer.0, email: m.committer.1 },
            date: m.date,
            subject: m.subject,
            body: m.body,
            comments_only: false,
            files: Vec::new(),
        })
        .collect();
    for e in &entries {
        let mut f = LogFile {
            status: status_of(e.status),
            path: e.path.clone(),
            old_path: Some(e.old.clone()).filter(|o| !o.is_empty() && e.status == b'R'),
            id: None,
            title: None,
            blocks: Counts::default(),
            comments_only: false,
            broken: false,
            hidden: false,
        };
        if e.path.ends_with(".json") {
            f.id = Some(stem(&e.path));
            if e.status == b'D' {
                raw.remove(&e.path);
                if let Applied::Gone(Some(st)) = history::apply(&mut state, e, None, &o) {
                    f.blocks.rm = st.leaves.iter().filter(|l| block_leaf(&l.ptr, &l.text)).count();
                    (f.title, f.hidden) = (title_of(&st), st.hidden);
                }
            } else {
                let data = blobs.next().flatten().unwrap_or_default();
                let before = if e.status == b'R' { raw.remove(&e.old) } else { raw.get(&e.path).cloned() };
                f.comments_only = e.status == b'M' && before.is_some_and(|b| comments_only(&b, &data));
                f.broken = matches!(history::apply(&mut state, e, Some(&data), &o), Applied::Broken);
                if let Some(st) = state.get(&e.path) {
                    if !f.broken {
                        f.blocks = counts(st, e.c);
                    }
                    (f.title, f.hidden) = (title_of(st), st.hidden);
                }
                raw.insert(e.path.clone(), data);
            }
        }
        commits[e.c].files.push(f);
    }
    for c in &mut commits {
        c.comments_only = !c.files.is_empty() && c.files.iter().all(|f| f.comments_only);
    }
    commits.reverse();
    Ok(LogOut { schema: LOG_SCHEMA_ID, commits })
}

#[derive(Serialize)]
pub struct DiffOut {
    #[serde(rename = "$schema")]
    schema: &'static str,
    base: String,
    head: String,
    pub files: Vec<DiffFile>,
}

#[derive(Serialize, Default)]
pub struct Sides<T> {
    old: T,
    new: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub status: &'static str,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_path: Option<String>,
    id: String,
    title: String,
    sections: Sides<Vec<Section>>,
    pub blocks: Vec<Op>,
    /// a side of the file is in a revision that does not compile (errors of
    /// groups 1xx-2xx, or a broken docc.json): its blocks are not known
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    broken: bool,
}

#[derive(Serialize)]
pub struct Op {
    pub op: &'static str,
    old: Option<CBlock>,
    new: Option<CBlock>,
}

/// Side is one revision of a diff: its pages by path relative to src, and
/// for each the parent array of its block pointers (history::flatten).
#[derive(Default)]
struct Side {
    pages: HashMap<String, (CPage, HashMap<String, String>)>,
    broken: bool,
}

impl Side {
    /// load compiles a revision as docc compile --rev does; a revision
    /// without the src folder has no documents.
    fn load(cfg: Result<Config, Error>) -> Side {
        let broken = Side { broken: true, ..Default::default() };
        let Ok(cfg) = cfg else { return broken };
        if cfg.source.entry(&cfg.abs(&cfg.src)) != Some(crate::source::Entry::Dir) {
            return Side::default();
        }
        let Ok(mut p) = engine::load(cfg) else { return broken };
        p.validate();
        if engine::structural(&p.diags()) {
            return broken;
        }
        let out = render::Renderer::new(&p, false).compile();
        let mut side = Side::default();
        for pg in out.pages {
            let parents = p
                .docs
                .iter()
                .find(|d| d.rel == pg.path)
                .and_then(|d| d.root())
                .map(|r| history::flatten(r).into_iter().map(|l| (l.ptr, l.parent)).collect())
                .unwrap_or_default();
            side.pages.insert(pg.path.clone(), (pg, parents));
        }
        side
    }

    /// blocks are the properties of a page, then its blocks, with their leaves.
    fn blocks(&self, path: &str) -> (Vec<CBlock>, Vec<Leaf>, Vec<Section>) {
        let Some((pg, parents)) = self.pages.get(path) else {
            return Default::default();
        };
        let props = pg.props.iter().map(|p| CBlock {
            p: p.p.clone(),
            s: p.s.clone(),
            h: p.h.clone(),
            sec: None,
            i: None,
            k: Some(p.k.clone()),
        });
        let bs: Vec<CBlock> = props.chain(pg.blocks.iter().cloned()).collect();
        // the footnotes block has no source: its HTML stands for its text
        let leaves = bs
            .iter()
            .map(|b| Leaf {
                ptr: b.p.clone(),
                parent: parents.get(&b.p).cloned().unwrap_or_default(),
                text: if b.p.is_empty() { b.h.clone() } else { b.s.clone() },
            })
            .collect();
        (bs, leaves, pg.sections.clone())
    }
}

/// diff lines up the documents of two revisions; load_cfg loads the config of
/// a revision. Without path: the changed documents with their changed blocks;
/// with path (of either side): that document with every block.
pub fn diff(
    load_cfg: impl Fn(&str) -> Result<Config, Error>,
    is_rev_error: impl Fn(&Error) -> bool,
    base: &str,
    head: &str,
    path: &str,
) -> Result<DiffOut, Error> {
    let usage = |msg: String| -> Error { Box::new(engine::UsageError { msg }) };
    let hc = match load_cfg(head) {
        Err(e) if is_rev_error(&e) => return Err(e),
        c => c,
    };
    let bc = match load_cfg(base) {
        Err(e) if is_rev_error(&e) => return Err(e),
        c => c,
    };
    let cfg = match (&hc, &bc) {
        (Ok(c), _) | (_, Ok(c)) => c,
        _ => return hc.map(|_| unreachable!()), // neither docc.json reads: nothing tells where src is
    };
    let (dir, prefix): (PathBuf, String) = history::locate_git(&cfg.abs(&cfg.src)).map_err(usage)?;
    let (bsha, hsha) = (resolve(&dir, base).map_err(usage)?, resolve(&dir, head).map_err(usage)?);
    let spec = format!(":(top,literal){prefix}");
    let out = git(
        &dir,
        &[
            "-c",
            "core.quotePath=false",
            "diff-tree",
            "-r",
            "-M",
            "--no-commit-id",
            "--name-status",
            "-z",
            &bsha,
            &hsha,
            "--",
            if prefix.is_empty() { ":(top)" } else { &spec },
        ],
    )
    .map_err(usage)?;
    // (status, path, old path) of every changed document
    let mut changed: Vec<(u8, String, String)> = Vec::new();
    let mut it = out.split('\0').filter(|s| !s.is_empty());
    while let Some(st) = it.next() {
        let s = st.as_bytes()[0];
        let two = s == b'R' || s == b'C';
        let a = it.next().unwrap_or_default();
        let b = if two { it.next().unwrap_or_default() } else { a };
        let rel = |p: &str| p.strip_prefix(&prefix).unwrap_or(p).to_string();
        if b.ends_with(".json") {
            changed.push((s, rel(b), if s == b'R' { rel(a) } else { String::new() }));
        }
    }
    if !path.is_empty() {
        changed.retain(|(_, p, o)| p == path || o == path);
        if changed.is_empty() {
            changed.push((b'=', path.to_string(), String::new()));
        }
    }
    let (old, new) = if changed.is_empty() { Default::default() } else { (Side::load(bc), Side::load(hc)) };
    let mut files = Vec::new();
    for (s, p, o) in changed {
        let op = if o.is_empty() { p.clone() } else { o.clone() };
        let (ob, ol, os) = if s == b'A' { Default::default() } else { old.blocks(&op) };
        let (nb, nl, ns) = if s == b'D' { Default::default() } else { new.blocks(&p) };
        let title = [(&new, &p), (&old, &op)].iter().find_map(|(sd, k)| sd.pages.get(*k).map(|x| x.0.title.clone()));
        if title.is_none() && !old.broken && !new.broken {
            continue; // hidden on both sides, or not a document at all
        }
        let broken = (s != b'A' && old.broken) || (s != b'D' && new.broken);
        let mut blocks = Vec::new();
        if !broken {
            for (i, j) in history::ops(&ol, &nl) {
                let op = match (i, j) {
                    (Some(i), Some(j)) if ol[i].text == nl[j].text => "same",
                    (Some(_), Some(_)) => "ch",
                    (Some(_), None) => "rm",
                    _ => "add",
                };
                if op != "same" || !path.is_empty() {
                    blocks.push(Op { op, old: i.map(|i| ob[i].clone()), new: j.map(|j| nb[j].clone()) });
                }
            }
        }
        files.push(DiffFile {
            status: if s == b'=' { "=" } else { status_of(s) },
            id: stem(&p),
            path: p,
            old_path: Some(o).filter(|o| !o.is_empty()),
            title: title.unwrap_or_default(),
            sections: Sides { old: os, new: ns },
            blocks,
            broken,
        });
    }
    Ok(DiffOut { schema: DIFF_SCHEMA_ID, base: bsha, head: hsha, files })
}
