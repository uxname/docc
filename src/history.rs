//! Package history reads the Git history of the documents and follows every
//! block (a string of an array, a field value) through it, so the HTML file can
//! show what a paragraph said in each commit.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Serialize, Serializer};

use crate::config::go_quote;
use crate::engine::{SECTIONS_KEY, is_input, is_sections};
use crate::loader::{self, Kind, Node};

/// Commit is one commit that touched the documents. The working tree, when it
/// differs from HEAD, is the last entry with an empty Hash. Author and Subject
/// are filled only for a full build: they are the team's words, not the reader's.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Commit {
    #[serde(rename = "h")]
    pub hash: String,
    #[serde(rename = "a", skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(rename = "d")]
    pub date: String,
    #[serde(rename = "m", skip_serializing_if = "String::is_empty")]
    pub subject: String,
}

/// Version is the text of a block after commit C (an index into Commits).
/// Html, the rendered text, is filled by the HTML build for the old versions
/// of string blocks, so a page can be shown as it was.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Version {
    pub c: usize,
    #[serde(rename = "t")]
    pub text: String,
    #[serde(rename = "h", skip_serializing_if = "String::is_empty")]
    pub html: String,
}

/// Change is what one commit did to a document.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Change {
    pub c: usize,
    /// [pointer, old text, new text]
    #[serde(rename = "ch", skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<Vec<String>>,
    /// [pointer, text]
    #[serde(rename = "add", skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<Vec<String>>,
    /// [pointer, last text]
    #[serde(rename = "rm", skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<Vec<String>>,
    #[serde(rename = "new", skip_serializing_if = "std::ops::Not::not")]
    pub created: bool,
}

/// Doc is the history of one document: the versions of its current blocks
/// (only those where the text changed) and its changes, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Doc {
    #[serde(rename = "b")]
    pub blocks: BTreeMap<String, Vec<Version>>,
    #[serde(rename = "ch", serialize_with = "null_if_empty")]
    pub changes: Vec<Change>,
}

/// History of the documents; Docs is keyed by the path relative to src. With
/// Base the first commit is the starting point (Options.Since): every document
/// is as it was then, and only the later commits are history.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct History {
    #[serde(serialize_with = "null_if_empty")]
    pub commits: Vec<Commit>,
    pub docs: BTreeMap<String, Doc>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub head: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub base: bool,
}

/// Go's encoding/json writes a nil slice as null; these slices are empty
/// exactly when they are nil in Go.
fn null_if_empty<S: Serializer, T: Serialize>(v: &[T], s: S) -> Result<S::Ok, S::Error> {
    if v.is_empty() {
        s.serialize_none()
    } else {
        v.serialize(s)
    }
}

/// Error of Load: a bad Since revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Options of Load. Full keeps everything (the audit build). Otherwise the
/// hidden fields of a document (notes, docIgnore, hidden) are left out, as in
/// the pages, and so are the authors and messages of the commits. Since, a Git
/// revision, cuts the history: what changed before it is not in the file.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub full: bool,
    pub since: String,
}

/// hiddenFields are the top-level fields a reader never sees.
const HIDDEN_FIELDS: [&str; 3] = ["/notes", "/docIgnore", "/hidden"];

impl Options {
    /// visible lists the blocks of a document that may go into the file;
    /// "$comments" holds the comments of the document, which are not blocks.
    fn visible(&self, root: &Node) -> Vec<Leaf> {
        let under = |ptr: &str, f: &str| ptr == f || ptr.strip_prefix(f).is_some_and(|r| r.starts_with('/'));
        flatten(root)
            .into_iter()
            .filter(|l| !under(&l.ptr, "/$comments") && (self.full || !HIDDEN_FIELDS.iter().any(|f| under(&l.ptr, f))))
            .collect()
    }
}

/// Meta is one commit of the log, oldest first.
#[derive(Debug, Clone, Default)]
pub(crate) struct Meta {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: (String, String),
    pub committer: (String, String),
    /// author date, strict ISO 8601
    pub date: String,
    pub subject: String,
    pub body: String,
}

/// One line of `git log --name-status`, paths relative to the folder.
pub(crate) struct Entry {
    pub c: usize,
    pub status: u8,
    pub old: String,
    pub path: String,
    /// path and old path as Git printed them, for cat-file
    pub git_path: Vec<u8>,
    pub git_old: Vec<u8>,
}

/// read_log reads `git log --first-parent` of revs (oldest first) for the
/// files below prefix (the folder's path from the top of the repository, ""
/// or ending with '/'): the commits and every file each one changed, merges
/// against their first parent, renames followed inside the folder. dir is
/// where git runs.
/// ponytail: a subject or body holding the bytes 0x01, 0x1e or 0x1f breaks
/// its record; Git messages do not hold control bytes in practice.
pub(crate) fn read_log(dir: &Path, revs: &[&str], prefix: &[u8]) -> std::io::Result<(Vec<Meta>, Vec<Entry>)> {
    let spec = format!(":(top,literal){}", String::from_utf8_lossy(prefix));
    let mut args = vec![
        "-c",
        "core.quotePath=false",
        "log",
        "--reverse",
        "--first-parent",
        "-m",
        "-M",
        "--name-status",
        "--format=%x01%H%x1f%P%x1f%an%x1f%ae%x1f%cn%x1f%ce%x1f%aI%x1f%s%x1f%b%x1e",
    ];
    args.extend(revs);
    args.extend(["--", if prefix.is_empty() { ":(top)" } else { &spec }]);
    let log = git(dir, &args)?;
    // The log is read as bytes: a path that is not UTF-8 still names its blob.
    let (mut commits, mut entries) = (Vec::new(), Vec::new());
    for rec in log.split(|&b| b == 0x01).skip(1) {
        let Some(end) = rec.iter().position(|&b| b == 0x1e) else {
            continue;
        };
        let f: Vec<String> = rec[..end].split(|&b| b == 0x1f).map(lossy).collect();
        if f.len() < 9 {
            continue;
        }
        let c = commits.len();
        commits.push(Meta {
            hash: f[0].clone(),
            parents: f[1].split_whitespace().map(str::to_string).collect(),
            author: (f[2].clone(), f[3].clone()),
            committer: (f[4].clone(), f[5].clone()),
            date: f[6].clone(),
            subject: f[7].clone(),
            body: f[8].trim_end().to_string(),
        });
        for l in rec[end + 1..].split(|&b| b == b'\n') {
            let p: Vec<&[u8]> = l.split(|&b| b == b'\t').collect();
            if p.len() < 2 {
                continue;
            }
            let status = p[0].first().copied().unwrap_or(0);
            let last = p[p.len() - 1];
            let Some(path) = last.strip_prefix(prefix) else {
                continue;
            };
            let (old, git_old) = match (status, p[1].strip_prefix(prefix)) {
                (b'R' | b'C', Some(o)) => (lossy(o), p[1].to_vec()),
                _ => (String::new(), Vec::new()),
            };
            entries.push(Entry { c, status, old, path: lossy(path), git_path: last.to_vec(), git_old });
        }
    }
    Ok((commits, entries))
}

/// Applied is what one entry of the log did to the state of its file.
pub(crate) enum Applied {
    /// the file was deleted: its last state, if it had one
    Gone(Option<Replay>),
    /// the new version is not a JSON object: the state keeps the last good one
    Broken,
    /// the state stepped to the new version
    Done,
}

/// apply replays one entry: a deletion drops the state of the file, a rename
/// moves it, and the new content (data, None for a deletion) steps it.
pub(crate) fn apply(state: &mut HashMap<String, Replay>, e: &Entry, data: Option<&[u8]>, o: &Options) -> Applied {
    match e.status {
        b'D' => return Applied::Gone(state.remove(&e.path)),
        b'R' => {
            if let Some(st) = state.remove(&e.old) {
                state.insert(e.path.clone(), st);
            }
        }
        _ => {}
    }
    let Some(root) = parse(data.unwrap_or_default()) else {
        return Applied::Broken; // a commit with broken JSON: keep the last good version
    };
    state.entry(e.path.clone()).or_default().step(&root, e.c, o);
    Applied::Done
}

/// Load reads the history of the JSON files in srcDir. current maps the path
/// of every document relative to srcDir to its parsed root as it is on disk;
/// a document that differs from HEAD gets a last version from the working
/// tree. Without git or a repository it returns None; an unknown Since is an
/// error.
/// ponytail: commits are replayed in log order with the full file at each one;
/// on merged branches the order is by date, not by topology.
pub fn load(
    src_dir: &Path,
    current: &HashMap<String, &Node>,
    o: &Options,
) -> Result<Option<History>, Error> {
    let Ok(prefix) = git(src_dir, &["rev-parse", "--show-prefix"]) else {
        if !o.since.is_empty() {
            return Err(Error(format!(
                "changes since {}: the project is not in a Git repository",
                go_quote(&o.since)
            )));
        }
        return Ok(None);
    };
    let mut before: Option<HashSet<String>> = None; // full hashes of the commits up to Since
    let mut since_date = String::new();
    if !o.since.is_empty() {
        let rev = format!("{}^{{commit}}", o.since);
        let Ok(list) = git(
            src_dir,
            &["rev-list", "--first-parent", "--end-of-options", &rev],
        ) else {
            return Err(Error(format!(
                "changes since {}: no such Git revision",
                go_quote(&o.since)
            )));
        };
        if git(src_dir, &["merge-base", "--is-ancestor", &rev, "HEAD"]).is_err() {
            return Err(Error(format!(
                "changes since {}: not in the history of HEAD",
                go_quote(&o.since)
            )));
        }
        if let Ok(d) = git(
            src_dir,
            &["log", "-1", "--format=%aI", "--end-of-options", &rev],
        ) {
            since_date = lossy(&d).trim().to_string();
        }
        before = Some(
            lossy(&list)
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        );
    }
    let prefix = prefix.trim_ascii();
    let mut h = History::default();
    if let Ok(head) = git(src_dir, &["rev-parse", "--short=8", "HEAD"]) {
        h.head = lossy(&head).trim().to_string();
    }
    let Ok((log, mut entries)) = read_log(src_dir, &[], prefix) else {
        return Ok(Some(h));
    };
    entries.retain(|e| e.path.ends_with(".json"));
    let full: Vec<String> = log.iter().map(|m| m.hash.clone()).collect(); // full hashes by commit index
    for m in log {
        let mut cm = Commit {
            hash: m.hash.get(..8).unwrap_or(&m.hash).to_string(),
            date: m.date,
            ..Default::default()
        };
        if o.full {
            (cm.author, cm.subject) = (m.author.0, m.subject);
        }
        h.commits.push(cm);
    }
    // read every blob the replay needs through one cat-file process
    let mut req: Vec<u8> = Vec::new();
    for e in &entries {
        if e.status != b'D' {
            req.extend_from_slice(full[e.c].as_bytes());
            req.push(b':');
            req.extend_from_slice(&e.git_path);
            req.push(b'\n');
        }
    }
    let Ok(blobs) = cat_files(src_dir, req) else {
        return Ok(Some(h));
    };
    let mut blobs = blobs.into_iter();
    let mut state: HashMap<String, Replay> = HashMap::new();
    for e in &entries {
        // Go would panic on a missing blob; cat-file answers every line, so it does not happen
        let data = if e.status == b'D' { None } else { blobs.next().flatten() };
        apply(&mut state, e, data.as_deref(), o);
    }
    let mut work: Option<usize> = None;
    for (rel, root) in current {
        let st = state.entry(rel.clone()).or_default();
        // st.leaves are the visible blocks of st.root, so this is o.same(st.root, root)
        if st.root && st.leaves == o.visible(root) {
            continue;
        }
        let w = *work.get_or_insert_with(|| {
            h.commits.push(Commit::default());
            h.commits.len() - 1
        });
        st.step(root, w, o);
    }
    for rel in current.keys() {
        let st = state.remove(rel).unwrap_or_default();
        let mut doc = st.doc;
        doc.blocks = st.blocks.into_iter().collect();
        h.docs.insert(rel.clone(), doc);
    }
    if let Some(before) = before {
        // the last commit of the log at or before Since
        if let Some(base) = (0..full.len()).rev().find(|&c| before.contains(&full[c])) {
            h.cut(base);
            h.commits[0].date = since_date; // the version is the one sent, even if the documents did not change in that commit
        }
    }
    Ok(Some(h))
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl History {
    /// cut makes commit base the starting point: the blocks keep their text at
    /// base as version 0, and the changes before it are dropped.
    fn cut(&mut self, base: usize) {
        self.commits.drain(..base);
        self.base = true;
        for d in self.docs.values_mut() {
            for vs in d.blocks.values_mut() {
                let mut out: Vec<Version> = Vec::new();
                for v in vs.drain(..) {
                    if v.c > base {
                        out.push(Version {
                            c: v.c - base,
                            ..v
                        });
                    } else if out.is_empty() {
                        out.push(Version { c: 0, ..v });
                    } else {
                        out[0].text = v.text;
                    }
                }
                *vs = out;
            }
            d.changes.retain_mut(|c| {
                if c.c > base {
                    c.c -= base;
                    return true;
                }
                false
            });
        }
    }
}

fn parse(data: &[u8]) -> Option<Node> {
    let res = loader::parse(data);
    if res.err.is_some() {
        return None;
    }
    res.root.filter(|r| r.kind == Kind::Object)
}

/// replay is the state of one file while the commits are replayed.
#[derive(Default)]
pub(crate) struct Replay {
    /// whether a version was replayed (Go keeps the root; only its visible
    /// blocks, leaves, are ever compared)
    root: bool,
    pub leaves: Vec<Leaf>,
    blocks: HashMap<String, Vec<Version>>,
    pub doc: Doc,
    /// the last version says "hidden": true
    pub hidden: bool,
}

impl Replay {
    fn step(&mut self, root: &Node, c: usize, o: &Options) {
        let leaves = o.visible(root);
        let matched = align(&self.leaves, &leaves);
        let old: HashMap<&str, &Leaf> = self.leaves.iter().map(|l| (l.ptr.as_str(), l)).collect();
        let mut ch = Change {
            c,
            created: !self.root,
            ..Default::default()
        };
        let mut blocks: HashMap<String, Vec<Version>> = HashMap::new();
        let mut kept: HashSet<String> = HashSet::new();
        for l in &leaves {
            let vs = match matched.get(&l.ptr).cloned() {
                Some(prev) => {
                    let mut vs = self.blocks.get(&prev).cloned().unwrap_or_default();
                    let old_text = old.get(prev.as_str()).map_or("", |o| o.text.as_str());
                    if old_text != l.text {
                        vs.push(Version {
                            c,
                            text: l.text.clone(),
                            ..Default::default()
                        });
                        ch.changed
                            .push(vec![l.ptr.clone(), old_text.to_string(), l.text.clone()]);
                    }
                    kept.insert(prev);
                    vs
                }
                None => {
                    if !ch.created {
                        ch.added.push(vec![l.ptr.clone(), l.text.clone()]);
                    }
                    vec![Version {
                        c,
                        text: l.text.clone(),
                        ..Default::default()
                    }]
                }
            };
            blocks.insert(l.ptr.clone(), vs);
        }
        for l in &self.leaves {
            if !kept.contains(&l.ptr) {
                ch.removed.push(vec![l.ptr.clone(), l.text.clone()]);
            }
        }
        if ch.created || ch.changed.len() + ch.added.len() + ch.removed.len() > 0 {
            self.doc.changes.push(ch);
        }
        (self.root, self.leaves, self.blocks) = (true, leaves, blocks);
        self.hidden = root.get("hidden").is_some_and(|n| n.kind == Kind::Bool && n.str == "true");
    }
}

/// Leaf is one block of a document: Parent is the pointer of the array of
/// strings it belongs to ("" for a block that is not an element of one).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Leaf {
    pub ptr: String,
    pub parent: String,
    pub text: String,
}

/// Flatten lists the blocks of a document in file order: every string (or
/// form field) of an array of paragraphs, every other field or section value as a whole. Sections
/// are entered. The pointers are those of Pointers.
pub fn flatten(root: &Node) -> Vec<Leaf> {
    fn walk(n: &Node, ptr: &str, top: bool, out: &mut Vec<Leaf>) {
        if n.kind == Kind::Object && (top || is_sections(n)) {
            for m in &n.members {
                if m.key != SECTIONS_KEY {
                    walk(&m.value, &format!("{ptr}/{}", escape(&m.key)), false, out);
                }
            }
        } else if n.kind == Kind::Array && all_strings(n) {
            for (i, it) in n.items.iter().enumerate() {
                out.push(Leaf {
                    ptr: format!("{ptr}/{i}"),
                    parent: ptr.to_string(),
                    text: text(it),
                });
            }
        } else {
            out.push(Leaf {
                ptr: ptr.to_string(),
                parent: String::new(),
                text: text(n),
            });
        }
    }
    let mut out = Vec::new();
    walk(root, "", true, &mut out);
    out
}

fn all_strings(n: &Node) -> bool {
    !n.items.is_empty() && n.items.iter().all(|it| it.kind == Kind::String || is_input(it))
}

/// Text is the plain text of a block: a string as it is, a list or table one
/// item per line, an object as "key: value" pairs.
pub fn text(n: &Node) -> String {
    match n.kind {
        Kind::Array => n.items.iter().map(text).collect::<Vec<_>>().join("\n"),
        Kind::Object => n
            .members
            .iter()
            .map(|m| format!("{}: {}", m.key, text(&m.value)))
            .collect::<Vec<_>>()
            .join("; "),
        Kind::Null => "null".to_string(),
        _ => n.str.clone(),
    }
}

/// Escape escapes a key for a JSON Pointer (RFC 6901).
pub fn escape(k: &str) -> String {
    k.replace('~', "~0").replace('/', "~1")
}

/// Pointers maps every node of a document (by address) to its JSON Pointer.
pub fn pointers(root: &Node) -> HashMap<*const Node, String> {
    fn walk(n: &Node, ptr: String, out: &mut HashMap<*const Node, String>) {
        for (i, it) in n.items.iter().enumerate() {
            walk(it, format!("{ptr}/{i}"), out);
        }
        for m in &n.members {
            walk(&m.value, format!("{ptr}/{}", escape(&m.key)), out);
        }
        out.insert(n as *const Node, ptr);
    }
    let mut out = HashMap::new();
    walk(root, String::new(), &mut out);
    out
}

/// pair matches the unmatched strings of one gap: the most similar pairs
/// first (Dice coefficient of character bigrams at least 0.3); what is left
/// pairs in order when both sides have the same count, as a rewrite. A pair
/// never crosses another one: blocks keep their order, so a block removed in
/// one place and one added in another are a removal and an addition.
/// ponytail: greedy over all pairs, O(n²) per gap; gaps are a few paragraphs.
fn pair(a: &[Leaf], b: &[Leaf], matched: &mut HashMap<String, String>) {
    let (mut used_a, mut used_b) = (vec![false; a.len()], vec![false; b.len()]);
    let ba: Vec<_> = a.iter().map(|l| bigrams(&l.text)).collect();
    let bb: Vec<_> = b.iter().map(|l| bigrams(&l.text)).collect();
    let mut done: Vec<(usize, usize)> = Vec::new();
    let crosses =
        |done: &[(usize, usize)], i: usize, j: usize| done.iter().any(|d| (i < d.0) != (j < d.1));
    let mut take = |i: usize,
                    j: usize,
                    used_a: &mut [bool],
                    used_b: &mut [bool],
                    done: &mut Vec<(usize, usize)>| {
        used_a[i] = true;
        used_b[j] = true;
        done.push((i, j));
        matched.insert(b[j].ptr.clone(), a[i].ptr.clone());
    };
    loop {
        let (mut best, mut bi, mut bj) = (0.3, None, 0);
        for i in 0..a.len() {
            for j in 0..b.len() {
                if !used_a[i] && !used_b[j] && !crosses(&done, i, j) {
                    let s = dice(&ba[i], &bb[j]);
                    if s >= best {
                        (best, bi, bj) = (s, Some(i), j);
                    }
                }
            }
        }
        let Some(bi) = bi else { break };
        take(bi, bj, &mut used_a, &mut used_b, &mut done);
    }
    let ra: Vec<usize> = (0..a.len()).filter(|&i| !used_a[i]).collect();
    let rb: Vec<usize> = (0..b.len()).filter(|&j| !used_b[j]).collect();
    if ra.len() == rb.len() {
        for k in 0..ra.len() {
            if !crosses(&done, ra[k], rb[k]) {
                take(ra[k], rb[k], &mut used_a, &mut used_b, &mut done);
            }
        }
    }
}

/// Similar is the Dice coefficient of the character bigrams of two texts: 1
/// for equal texts, 0 for texts without a common pair of letters.
pub fn similar(a: &str, b: &str) -> f64 {
    dice(&bigrams(a), &bigrams(b))
}

fn bigrams(s: &str) -> HashMap<(char, Option<char>), usize> {
    let r: Vec<char> = crate::text::to_lower(s).chars().collect();
    let mut m = HashMap::new();
    for w in r.windows(2) {
        *m.entry((w[0], Some(w[1]))).or_insert(0) += 1;
    }
    if r.len() == 1 {
        *m.entry((r[0], None)).or_insert(0) += 1;
    }
    m
}

fn dice(a: &HashMap<(char, Option<char>), usize>, b: &HashMap<(char, Option<char>), usize>) -> f64 {
    let (mut na, mut common) = (0, 0);
    for (k, &v) in a {
        na += v;
        common += v.min(b.get(k).copied().unwrap_or(0));
    }
    let nb: usize = b.values().sum();
    if na + nb == 0 {
        return 0.0;
    }
    2.0 * common as f64 / (na + nb) as f64
}

/// Align maps the pointers of the new blocks to the blocks they continue.
/// Blocks outside arrays match by pointer. The strings of one array match by
/// a longest common subsequence of equal texts; unmatched strings between two
/// matches pair up by similarity (see pair), the rest are added or removed.
pub fn align(old: &[Leaf], cur: &[Leaf]) -> HashMap<String, String> {
    let mut matched = HashMap::new();
    let mut old_ptr: HashSet<&str> = HashSet::new();
    let mut old_arr: HashMap<&str, Vec<Leaf>> = HashMap::new();
    for l in old {
        if l.parent.is_empty() {
            old_ptr.insert(&l.ptr);
        } else {
            old_arr.entry(&l.parent).or_default().push(l.clone());
        }
    }
    let mut cur_arr: HashMap<&str, Vec<Leaf>> = HashMap::new();
    let mut order: Vec<&str> = Vec::new();
    for l in cur {
        if l.parent.is_empty() {
            if old_ptr.contains(l.ptr.as_str()) {
                matched.insert(l.ptr.clone(), l.ptr.clone());
            }
            continue;
        }
        if !cur_arr.contains_key(l.parent.as_str()) {
            order.push(&l.parent);
        }
        cur_arr.entry(&l.parent).or_default().push(l.clone());
    }
    for p in order {
        let a = old_arr.get(p).map_or(&[][..], |v| v.as_slice());
        let b = cur_arr[p].as_slice();
        // LCS table over equal texts
        let (n, m) = (a.len(), b.len());
        let mut dp = vec![vec![0usize; m + 1]; n + 1];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                dp[i][j] = if a[i].text == b[j].text {
                    dp[i + 1][j + 1] + 1
                } else {
                    dp[i + 1][j].max(dp[i][j + 1])
                };
            }
        }
        // the current gap of unmatched strings
        let (mut ga, mut gb): (Vec<Leaf>, Vec<Leaf>) = (Vec::new(), Vec::new());
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if a[i].text == b[j].text {
                pair(&ga, &gb, &mut matched);
                ga.clear();
                gb.clear();
                matched.insert(b[j].ptr.clone(), a[i].ptr.clone());
                (i, j) = (i + 1, j + 1);
            } else if dp[i + 1][j] >= dp[i][j + 1] {
                ga.push(a[i].clone());
                i += 1;
            } else {
                gb.push(b[j].clone());
                j += 1;
            }
        }
        ga.extend_from_slice(&a[i..]);
        gb.extend_from_slice(&b[j..]);
        pair(&ga, &gb, &mut matched);
    }
    matched
}

/// Ops lines two versions of the blocks of a document up for showing them
/// side by side: (old index, new index) pairs as align matches them, a
/// block of one side only with None on the other. New blocks keep their
/// order; a removed block stands before the next block of the new side that
/// comes after it in the old one. This is the one alignment of docc: the HTML
/// history, docc log and docc diff all follow blocks through it, and a
/// comment can follow its block by the pair its old pointer is in.
pub fn ops(old: &[Leaf], new: &[Leaf]) -> Vec<(Option<usize>, Option<usize>)> {
    let m = align(old, new);
    let at: HashMap<&str, usize> = old.iter().enumerate().map(|(i, l)| (l.ptr.as_str(), i)).collect();
    let pair: Vec<Option<usize>> = new.iter().map(|l| m.get(&l.ptr).and_then(|p| at.get(p.as_str())).copied()).collect();
    let mut matched = vec![false; old.len()];
    for i in pair.iter().flatten() {
        matched[*i] = true;
    }
    // next[j]: the old index of the first matched block at or after j
    let mut next = vec![old.len(); new.len() + 1];
    for j in (0..new.len()).rev() {
        next[j] = pair[j].unwrap_or(next[j + 1]);
    }
    let (mut out, mut k) = (Vec::new(), 0);
    let flush = |upto: usize, k: &mut usize, out: &mut Vec<(Option<usize>, Option<usize>)>| {
        while *k < upto {
            if !matched[*k] {
                out.push((Some(*k), None));
            }
            *k += 1;
        }
    };
    for j in 0..new.len() {
        flush(next[j], &mut k, &mut out);
        out.push((pair[j], Some(j)));
        if let Some(i) = pair[j] {
            k = k.max(i + 1);
        }
    }
    flush(old.len(), &mut k, &mut out);
    out
}

/// locate_git finds where to run git for a folder that need not exist on
/// the disk (a project inside a bare mirror): the nearest existing folder
/// above it, and the folder's path from the top of the repository ("" or
/// ending with '/').
pub(crate) fn locate_git(folder: &Path) -> Result<(std::path::PathBuf, String), String> {
    let mut dir = folder.to_path_buf();
    while !dir.is_dir() {
        if !dir.pop() {
            return Err(format!("{} does not exist", folder.display()));
        }
    }
    let rest: Vec<String> = folder
        .strip_prefix(&dir)
        .unwrap_or(Path::new(""))
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let top = git(&dir, &["rev-parse", "--show-prefix"])
        .map_err(|_| format!("{} is not in a Git repository", folder.display()))?;
    let mut prefix = lossy(top.trim_ascii());
    for r in rest {
        prefix += &r;
        prefix.push('/');
    }
    Ok((dir, prefix))
}

/// git runs git in dir and returns its standard output; an error when git is
/// missing or fails.
fn git(dir: &Path, args: &[&str]) -> std::io::Result<Vec<u8>> {
    let out = Command::new("git").args(args).current_dir(dir).output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!("git: {}", out.status)));
    }
    Ok(out.stdout)
}

/// catFiles returns the contents of the objects named one per line in req, in
/// order; None for a missing one.
pub(crate) fn cat_files(dir: &Path, req: Vec<u8>) -> std::io::Result<Vec<Option<Vec<u8>>>> {
    let mut cmd = Command::new("git")
        .args(["cat-file", "--batch"])
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = cmd.stdin.take().expect("piped stdin");
    // feed the requests from another thread, as Go's exec does, so a full pipe cannot block both sides
    let feeder = std::thread::spawn(move || {
        let _ = stdin.write_all(&req);
    });
    let mut blobs = Vec::new();
    let mut r = BufReader::new(cmd.stdout.take().expect("piped stdout"));
    let mut line = Vec::new();
    loop {
        line.clear();
        if r.read_until(b'\n', &mut line).is_err() || line.last() != Some(&b'\n') {
            break;
        }
        let s = String::from_utf8_lossy(&line);
        let f: Vec<&str> = s.split_whitespace().collect();
        if f.len() < 3 {
            // "<name> missing"
            blobs.push(None);
            continue;
        }
        let size: usize = f[2].parse().unwrap_or(0);
        let mut b = vec![0; size + 1]; // content and the closing newline
        if r.read_exact(&mut b).is_err() {
            break;
        }
        b.truncate(size);
        blobs.push(Some(b));
    }
    drop(r);
    let _ = feeder.join();
    let status = cmd.wait()?;
    if !status.success() {
        return Err(std::io::Error::other(format!("git cat-file: {status}")));
    }
    Ok(blobs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn no_git() -> bool {
        Command::new("git").arg("--version").output().is_err()
    }

    fn temp_dir() -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("docc-history-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn root(s: &str) -> Node {
        loader::parse(s.as_bytes()).root.unwrap()
    }

    /// A throwaway repository with docs/src in it.
    struct Repo {
        dir: PathBuf,
        src: PathBuf,
        env: Vec<(&'static str, &'static str)>,
    }

    impl Repo {
        fn new(env: Vec<(&'static str, &'static str)>) -> Repo {
            let dir = temp_dir();
            let src = dir.join("docs").join("src");
            std::fs::create_dir_all(&src).unwrap();
            Repo { dir, src, env }
        }
        fn run(&self, args: &[&str]) {
            let out = Command::new("git")
                .args(args)
                .current_dir(&self.dir)
                .envs(self.env.iter().copied())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        fn write(&self, s: &str) {
            std::fs::write(self.src.join("a.json"), s).unwrap();
        }
        fn step(&self, doc: &str, msg: &str) {
            self.write(doc);
            self.run(&["add", "-A"]);
            self.run(&["commit", "-q", "-m", msg]);
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const ENV: [(&str, &str); 4] = [
        ("GIT_AUTHOR_NAME", "A"),
        ("GIT_AUTHOR_EMAIL", "a@x"),
        ("GIT_COMMITTER_NAME", "A"),
        ("GIT_COMMITTER_EMAIL", "a@x"),
    ];

    fn since(s: &str) -> Options {
        Options {
            since: s.into(),
            ..Default::default()
        }
    }

    // A paragraph inserted above an edited one does not move the history of the
    // edited one onto its neighbour; a removed paragraph is reported with its text;
    // an uncommitted edit is the last version.
    #[test]
    fn history_follows_blocks() {
        if no_git() {
            return; // no git
        }
        let mut env = ENV.to_vec();
        env.extend([
            ("GIT_AUTHOR_DATE", "2026-10-01T10:00:00Z"),
            ("GIT_COMMITTER_DATE", "2026-10-01T10:00:00Z"),
        ]);
        let r = Repo::new(env);
        r.run(&["init", "-q"]);
        r.step(
            r#"{"title":"A","body":["one","two","three"],"f":"x"}"#,
            "first",
        );
        r.step(
            r#"{"title":"A","body":["one","new","two edited","three"],"f":"x"}"#,
            "second",
        );
        r.step(
            r#"{"title":"A","body":["one","new","two edited"],"f":"y"}"#,
            "third",
        );
        r.write(r#"{"title":"A","body":["one!","new","two edited"],"f":"y"}"#);
        let cur = root(r#"{"title":"A","body":["one!","new","two edited"],"f":"y"}"#);

        let files = HashMap::from([("a.json".to_string(), &cur)]);
        let h = load(
            &r.src,
            &files,
            &Options {
                full: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert!(
            h.commits.len() == 4
                && h.commits[3].hash.is_empty()
                && h.commits[1].subject == "second",
            "commits: {h:?}"
        );
        let d = &h.docs["a.json"];
        let vs = &d.blocks["/body/2"];
        assert!(
            vs.len() == 2 && vs[0].text == "two" && vs[1].text == "two edited" && vs[1].c == 1,
            "edited paragraph: {vs:?}"
        );
        let vs = &d.blocks["/body/1"];
        assert!(vs.len() == 1 && vs[0].c == 1, "inserted paragraph: {vs:?}");
        let vs = &d.blocks["/body/0"];
        assert!(vs.len() == 2 && vs[1].c == 3, "uncommitted edit: {vs:?}");
        let vs = &d.blocks["/f"];
        assert!(vs.len() == 2 && vs[1].text == "y", "field: {vs:?}");
        let ch = &d.changes;
        assert!(
            ch.len() == 4
                && ch[0].created
                && ch[2].removed.len() == 1
                && ch[2].removed[0][1] == "three",
            "changes: {ch:?}"
        );
    }

    #[test]
    fn no_repository() {
        let (a, b) = (temp_dir(), temp_dir());
        let h = load(&a, &HashMap::new(), &Options::default());
        assert!(matches!(h, Ok(None)), "history outside a repository: {h:?}");
        let h = load(&b, &HashMap::new(), &since("v1"));
        assert_eq!(
            h.unwrap_err().to_string(),
            r#"changes since "v1": the project is not in a Git repository"#,
            "changes since a revision outside a repository must be an error"
        );
        let _ = (std::fs::remove_dir_all(a), std::fs::remove_dir_all(b));
    }

    // The file for the reader has no hidden fields, no authors or messages of
    // the commits and nothing before the starting point; the full build keeps all.
    #[test]
    fn history_for_the_reader() {
        if no_git() {
            return;
        }
        let mut env = ENV.to_vec();
        env[0].1 = "Secret Author";
        let r = Repo::new(env);
        r.run(&["init", "-q"]);
        r.step(
            r#"{"title":"A","body":["draft one"],"notes":["internal guess"]}"#,
            "draft by the agents",
        );
        r.step(
            r#"{"title":"A","body":["sent one"],"notes":["internal guess 2"]}"#,
            "sent to the customer",
        );
        r.run(&["tag", "sent"]);
        r.step(
            r#"{"title":"A","body":["sent one","added later"],"notes":["internal guess 3"]}"#,
            "after the call",
        );
        let cur =
            root(r#"{"title":"A","body":["sent one","added later"],"notes":["internal guess 3"]}"#);
        let files = HashMap::from([("a.json".to_string(), &cur)]);

        let h = load(&r.src, &files, &since("sent")).unwrap().unwrap();
        let js = serde_json::to_string(&h).unwrap();
        for leak in [
            "internal guess",
            "/notes",
            "Secret Author",
            "draft by the agents",
            "after the call",
            "draft one",
        ] {
            assert!(
                !js.contains(leak),
                "the reader's history holds {leak:?}:\n{js}"
            );
        }
        let d = &h.docs["a.json"];
        assert!(
            h.base && h.commits.len() == 2,
            "commits from the starting point: {:?}",
            h.commits
        );
        let vs = &d.blocks["/body/0"];
        assert!(
            vs.len() == 1 && vs[0].c == 0 && vs[0].text == "sent one",
            "block as sent: {vs:?}"
        );
        let vs = &d.blocks["/body/1"];
        assert!(vs.len() == 1 && vs[0].c == 1, "block added after: {vs:?}");
        let ch = &d.changes;
        assert!(
            ch.len() == 1 && ch[0].c == 1 && ch[0].added.len() == 1,
            "changes after the starting point: {ch:?}"
        );
        let e = load(&r.src, &files, &since("no-such-tag"));
        assert!(
            e.as_ref()
                .is_err_and(|e| e.0.contains("no such Git revision")),
            "unknown revision: {e:?}"
        );
        let full = load(
            &r.src,
            &files,
            &Options {
                full: true,
                ..Default::default()
            },
        )
        .unwrap();
        let js = serde_json::to_string(&full).unwrap();
        assert!(
            js.contains("internal guess 3") && js.contains("Secret Author"),
            "the full build keeps the notes and authors:\n{js}"
        );
    }

    // A merge is one change on the main line: a clean tree after a merge has no
    // uncommitted commit, and --since must be an ancestor of HEAD.
    #[test]
    fn history_across_merge() {
        if no_git() {
            return;
        }
        let r = Repo::new(ENV.to_vec());
        // the title and the last paragraph are far apart, so Git merges them
        let doc = |title: &str, extra: &str| {
            let last = if extra.is_empty() {
                String::new()
            } else {
                format!(r#","{extra}""#)
            };
            format!("{{\"title\":\"{title}\",\n\n\n\n\n\"body\":[\"A\",\n\n\n\n\n\"B\"{last}]}}\n")
        };
        r.run(&["init", "-q", "-b", "main"]);
        r.step(&doc("A", ""), "base");
        r.run(&["checkout", "-q", "-b", "feat"]);
        r.step(&doc("A", "F-para"), "feature");
        r.run(&["checkout", "-q", "main"]);
        r.step(&doc("A changed", ""), "main edit");
        r.run(&["tag", "onmain"]);
        r.run(&["merge", "-q", "--no-edit", "feat"]);
        let data = std::fs::read(r.src.join("a.json")).unwrap();
        let merged = String::from_utf8_lossy(&data);
        assert!(
            merged.contains("A changed") && merged.contains("F-para"),
            "merge result: {merged}"
        );
        let cur = loader::parse(&data).root.unwrap();
        let files = HashMap::from([("a.json".to_string(), &cur)]);
        let h = load(&r.src, &files, &Options::default()).unwrap().unwrap();
        assert!(
            !h.commits.last().unwrap().hash.is_empty(),
            "a clean tree after a merge has no uncommitted commit: {:?}",
            h.commits
        );
        let h = load(&r.src, &files, &since("onmain")).unwrap().unwrap();
        let vs = &h.docs["a.json"].blocks["/body/2"];
        assert!(
            vs.len() == 1 && vs[0].c != 0,
            "the merged paragraph is new since the tag: {vs:?}"
        );
        let e = load(&r.src, &files, &since("feat"));
        assert!(e.is_ok(), "merged branch is an ancestor of HEAD: {e:?}");
        r.run(&["checkout", "-q", "-b", "other", "main~2"]);
        r.step(r#"{"title":"Other","body":["A"]}"#, "other");
        r.run(&["checkout", "-q", "main"]);
        let e = load(&r.src, &files, &since("other"));
        assert!(
            e.as_ref()
                .is_err_and(|e| e.0.contains("not in the history")),
            "a revision outside the history of HEAD: {e:?}"
        );
    }

    // A paragraph removed in the middle and one added at the end are a removal
    // and an addition, not one rewritten paragraph: pairs keep the order.
    #[test]
    fn pair_keeps_order() {
        let leaf = |ptr: &str, text: &str| Leaf {
            ptr: ptr.into(),
            text: text.into(),
            ..Default::default()
        };
        let a = [
            leaf("/b/1", "Confirmed by SMS within 5 minutes."),
            leaf(
                "/b/3",
                "A client who missed two visits in a row cannot book online.",
            ),
            leaf("/b/4", "Reminder a day before the visit."),
        ];
        let b = [
            leaf("/b/1", "Confirmed by SMS or Telegram within 1 minute."),
            leaf("/b/3", "Reminder two hours before the visit."),
            leaf(
                "/b/4",
                "A client can book two or three services in a row with one master.",
            ),
        ];
        let mut m = HashMap::new();
        pair(&a, &b, &mut m);
        let get = |k: &str| m.get(k).map_or("", |s: &String| s.as_str());
        assert!(
            get("/b/1") == "/b/1" && get("/b/3") == "/b/4" && get("/b/4").is_empty(),
            "pairs: {m:?}"
        );
    }

    // ops: removed blocks stand before the additions of their gap, a field
    // that moved up is paired where it stands now, nothing is lost.
    #[test]
    fn ops_order() {
        let leaf = |ptr: &str, parent: &str, text: &str| Leaf { ptr: ptr.into(), parent: parent.into(), text: text.into() };
        let old = [
            leaf("/f", "", "F"),
            leaf("/b/0", "/b", "Alpha paragraph."),
            leaf("/b/1", "/b", "Beta paragraph."),
            leaf("/b/2", "/b", "Gamma paragraph."),
            leaf("/g", "", "G"),
        ];
        let new = [
            leaf("/g", "", "G2"),
            leaf("/f", "", "F"),
            leaf("/b/0", "/b", "Alpha paragraph."),
            leaf("/b/1", "/b", "Something else entirely, longer."),
            leaf("/b/2", "/b", "Gamma paragraph."),
            leaf("/b/3", "/b", "Zeta."),
        ];
        // Beta and the new paragraph do not look alike, but the gap is one to one: a rewrite
        let s = |a: Option<usize>, b: Option<usize>| (a, b);
        assert_eq!(
            ops(&old, &new),
            [s(Some(4), Some(0)), s(Some(0), Some(1)), s(Some(1), Some(2)), s(Some(2), Some(3)), s(Some(3), Some(4)), s(None, Some(5))]
        );
        // one paragraph for two unlike ones: the removal stands first
        let old = [leaf("/b/0", "/b", "Alpha."), leaf("/b/1", "/b", "Beta."), leaf("/b/2", "/b", "Gamma.")];
        let new = [leaf("/b/0", "/b", "Alpha."), leaf("/b/1", "/b", "Xyz."), leaf("/b/2", "/b", "Qrs."), leaf("/b/3", "/b", "Gamma.")];
        assert_eq!(
            ops(&old, &new),
            [s(Some(0), Some(0)), s(Some(1), None), s(None, Some(1)), s(None, Some(2)), s(Some(2), Some(3))]
        );
    }

    // A form field among the paragraphs is a block of its own: the paragraphs
    // around it keep their history.
    #[test]
    fn flatten_form_field() {
        let r = crate::loader::parse(
            br#"{"body":["One.",{"$input":"text","id":"q","label":"Q?"},"Two."],"list":["x",{"a":1}]}"#,
        );
        let got: Vec<String> = flatten(r.root.as_ref().unwrap())
            .iter()
            .map(|l| format!("{}={}", l.ptr, l.text))
            .collect();
        assert_eq!(
            got,
            ["/body/0=One.", "/body/1=$input: text; id: q; label: Q?", "/body/2=Two.", "/list=x\na: 1"]
        );
    }
}
