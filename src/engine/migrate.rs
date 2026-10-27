//! Giving codes to blocks (docc build, docc codes) and moving a project of the
//! old format to codes and version hashes (docc migrate). Both edit the
//! source files in place: only the strings they change are written anew,
//! every other byte stays.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use super::analyze::split_ref;
use super::project::{Doc, Project, go_path_error, load_any};
use super::versions::version_in;
use crate::codes;
use crate::comments;
use crate::config::Config;
use crate::loader::{self, Kind, Node};
use crate::mdscan;
use crate::schema;
use crate::trace;

type Res<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// splice applies edits (byte range of data, new bytes) that do not overlap.
fn splice(data: &[u8], mut edits: Vec<(usize, usize, String)>) -> Vec<u8> {
    edits.sort_by_key(|e| e.0);
    let mut out = Vec::with_capacity(data.len() + edits.len() * 8);
    let mut last = 0;
    for (a, b, s) in edits {
        out.extend_from_slice(&data[last..a]);
        out.extend_from_slice(s.as_bytes());
        last = b;
    }
    out.extend_from_slice(&data[last..]);
    out
}

/// json is s as a JSON string literal.
fn json(s: &str) -> String {
    serde_json::to_string(s).expect("a string encodes")
}

/// write_doc writes the new text of the source file of d, with its BOM if it had one.
fn write_doc(p: &Project, d: &Doc, data: &[u8]) -> Res<()> {
    let abs = p.cfg.abs(&d.file);
    let mut out = Vec::new();
    if std::fs::read(&abs).is_ok_and(|b| b.starts_with(b"\xEF\xBB\xBF")) {
        out.extend_from_slice(b"\xEF\xBB\xBF");
    }
    out.extend_from_slice(data);
    std::fs::write(&abs, out).map_err(|e| go_path_error("open", &abs, &e).into())
}

/// all_codes are the codes at the end of any string of the project.
fn all_codes(p: &Project) -> HashSet<String> {
    let mut used = HashSet::new();
    for d in &p.docs {
        for sn in d.strings() {
            if let Some(c) = codes::code_of(&sn.node().str) {
                used.insert(c.to_string());
            }
        }
    }
    used
}

/// assign_codes gives a new code to every block of the project that has
/// none (docc build, docc codes) and returns how many it gave. It never
/// touches a code that is there or a hash. A block that ends in a marker
/// which is not a code (DOC216) is left alone.
pub fn assign_codes(p: &Project) -> Res<usize> {
    if p.legacy {
        return Ok(0);
    }
    let mut used = all_codes(p);
    let mut n = 0;
    for d in &p.docs {
        let mut edits = Vec::new();
        for sn in d.strings() {
            let s = &sn.node().str;
            if sn.blk && !sn.notes && sn.anchor.is_empty() && !sn.bad_mark && !s.trim().is_empty() {
                let add = codes::with_code(s, &codes::new_code(&mut used, &format!("{}\0{s}", d.rel)))[s.len()..].to_string();
                let lit = json(&add);
                let end = sn.node().end - 1; // before the closing quote
                edits.push((end, end, lit[1..lit.len() - 1].to_string()));
            }
        }
        if !edits.is_empty() {
            n += edits.len();
            write_doc(p, d, &splice(&d.res().src.data, edits))?;
        }
    }
    Ok(n)
}

/// Report is what docc migrate changed.
#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// documents written
    pub documents: usize,
    /// blocks that got a code where they had none
    pub codes_given: usize,
    /// named anchors replaced by codes
    pub anchors_replaced: usize,
    /// links whose anchor became a code
    pub links_rewritten: usize,
    /// version hashes written into links
    pub hashes_added: usize,
    /// comments whose "at" became a code
    pub comments_moved: usize,
    /// code files written
    pub code_files: usize,
    /// id#^anchor in code files that became id#^code (docc: tags, <Req to="...">)
    pub code_refs_rewritten: usize,
    /// version hashes written into docc: tags
    pub tag_hashes_added: usize,
    /// fingerprints written on docc: tags
    pub prints_added: usize,
    /// links and regions that docc.lock held unconfirmed: they got a hash that
    /// does not match, so docc check asks for a review of each
    pub unconfirmed: usize,
    /// docc.lock was deleted
    pub lock_removed: bool,
}

/// OLD_REF is id#^name in a code file.
static OLD_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"([a-z0-9][a-z0-9-]*)#\^([a-z][a-z0-9-]*)").unwrap());

/// placeholder is a hash other than h: the version of a link docc.lock held
/// unconfirmed, so the link stays to be checked.
fn placeholder(h: &str) -> String {
    let mut x = codes::hash4(&format!("{h} unconfirmed"));
    while x == h {
        x = codes::hash4(&x);
    }
    x
}

/// link_strings are the strings of a document where links may stand: all
/// but the comments and docIgnore.
fn link_strings<'a>(n: &'a Node, top: bool, out: &mut Vec<&'a Node>) {
    match n.kind {
        Kind::String => out.push(n),
        Kind::Array => n.items.iter().for_each(|it| link_strings(it, false, out)),
        Kind::Object => n
            .members
            .iter()
            .filter(|m| !(top && (m.key == comments::KEY || m.key == "docIgnore")))
            .for_each(|m| link_strings(&m.value, false, out)),
        _ => {}
    }
}

/// migrate moves the project of cfg to block codes and version hashes:
/// every block gets a code (a named anchor becomes one), every link and
/// every id#^anchor in the code files follows, links and docc: tags get the
/// hashes of their targets and tags their fingerprints, the comments point
/// at codes, docc.lock goes. A project already moved stays as it is (only
/// what is missing is added), so running it twice changes nothing.
pub fn migrate(cfg: Config) -> Res<Report> {
    let mut p = load_any(cfg)?;
    p.validate();
    let mut rep = Report::default();
    let (stale_t, stale_c) = if p.legacy { p.stale_links() } else { Default::default() };

    // 1. codes: the codes there keep their blocks (the first one in path order
    // when copied), named anchors and blocks without a marker get new ones
    let mut used: HashSet<String> = HashSet::new();
    let mut dup: HashSet<*const Node> = HashSet::new();
    for d in &p.docs {
        for sn in d.strings().iter().filter(|sn| sn.blk && !sn.notes) {
            if let Some(c) = codes::code_of(&sn.node().str)
                && !used.insert(c.to_string())
            {
                dup.insert(sn.node());
            }
        }
    }
    let mut map: HashMap<(String, String), String> = HashMap::new(); // (id, old name) -> code
    let mut text: HashMap<*const Node, String> = HashMap::new(); // new text of a string
    let mut code_at: HashMap<*const Node, String> = HashMap::new(); // the code of a block
    for d in &p.docs {
        for sn in d.strings().iter().filter(|sn| sn.blk && !sn.notes) {
            let n = sn.node();
            let s = &n.str;
            let salt = format!("{}\0{s}", d.rel);
            if let Some(m) = codes::CODE_MARK.captures(s) {
                let c = m[1].to_string();
                if dup.contains(&(n as *const Node)) {
                    let new = codes::new_code(&mut used, &salt);
                    let at = m.get(1).unwrap().start();
                    text.insert(n, format!("{}{new}", &s[..at]));
                    map.entry((d.id.clone(), c)).or_insert_with(|| new.clone());
                    code_at.insert(n, new);
                    rep.anchors_replaced += 1;
                } else {
                    map.entry((d.id.clone(), c.clone())).or_insert_with(|| c.clone());
                    code_at.insert(n, c);
                }
            } else if let Some(m) = codes::NAMED_MARK.captures(s) {
                let new = codes::new_code(&mut used, &salt);
                let at = m.get(1).unwrap().start();
                text.insert(n, format!("{}{new}", &s[..at]));
                map.entry((d.id.clone(), m[1].to_string())).or_insert_with(|| new.clone());
                code_at.insert(n, new);
                rep.anchors_replaced += 1;
            } else if !s.trim().is_empty() {
                let new = codes::new_code(&mut used, &salt);
                text.insert(n, codes::with_code(s, &new));
                code_at.insert(n, new);
                rep.codes_given += 1;
            }
        }
    }
    let rename = |target: &str| -> Option<String> {
        let (id, name) = target.split_once("#^")?;
        let c = map.get(&(id.to_string(), name.to_string()))?;
        (c != name).then(|| format!("{id}#^{c}"))
    };

    // 2. links follow the codes, comments point at codes
    let mut stale_new: HashSet<(String, String)> = HashSet::new();
    let mut data: Vec<Option<Vec<u8>>> = Vec::new(); // new text of each document
    for d in &p.docs {
        let Some(root) = d.root() else {
            data.push(None);
            continue;
        };
        let mut edits = Vec::new();
        let mut strs = Vec::new();
        link_strings(root, true, &mut strs);
        for n in strs {
            let cur = text.get(&(n as *const Node)).cloned().unwrap_or_else(|| n.str.clone());
            let mut out = String::new();
            let mut last = 0;
            for r in mdscan::new(&cur).refs() {
                let new = rename(&r.inner);
                let inner = new.clone().unwrap_or_else(|| r.inner.clone());
                if stale_t.contains(&(d.id.clone(), r.inner.clone())) {
                    stale_new.insert((d.id.clone(), inner.clone()));
                }
                if new.is_some() {
                    out.push_str(&cur[last..r.start]);
                    out.push_str(&format!("[[{inner}{}]]", r.hash.map(|h| format!("@{h}")).unwrap_or_default()));
                    last = r.end;
                    rep.links_rewritten += 1;
                }
            }
            out.push_str(&cur[last..]);
            // includes follow the codes too; they carry no hash
            let cur = out;
            let mut out = String::new();
            let mut last = 0;
            for r in mdscan::new(&cur).includes() {
                if let Some(new) = rename(&r.inner) {
                    out.push_str(&cur[last..r.start]);
                    out.push_str(&format!("{{{{{new}}}}}"));
                    last = r.end;
                    rep.links_rewritten += 1;
                }
            }
            out.push_str(&cur[last..]);
            if out != n.str {
                edits.push((n.start, n.end, json(&out)));
            }
        }
        if let Some(cs) = root.get(comments::KEY).filter(|c| c.kind == Kind::Array) {
            for it in &cs.items {
                // an answer stays at its form field, which has no code
                let Some(at) = it.get("at").filter(|a| a.kind == Kind::String && a.str.starts_with('/') && it.get("input").is_none()) else {
                    continue;
                };
                if let Some(c) = schema::resolve_pointer(root, &at.str).and_then(|n| code_at.get(&(n as *const Node))) {
                    edits.push((at.start, at.end, json(&format!("^{c}"))));
                    rep.comments_moved += 1;
                }
            }
        }
        data.push((!edits.is_empty()).then(|| splice(&d.res().src.data, edits)));
    }

    // 3. hashes: computed on the new texts, so the codes and links are already in
    let parsed: Vec<Option<loader::Result>> = p
        .docs
        .iter()
        .zip(&data)
        .map(|(d, b)| match b {
            Some(b) => Some(loader::parse(b)),
            None => d.root().map(|_| loader::parse(&d.res().src.data)),
        })
        .collect();
    let root_of = |id: &str| -> Option<&Node> {
        let i = *p.graph.get(id)?;
        parsed[i].as_ref().filter(|r| r.err.is_none())?.root.as_ref()
    };
    let hash_of = |target: &str| -> Option<String> {
        let (id, field, _, ok) = split_ref(target, false);
        if !ok {
            return None;
        }
        Some(version_in(root_of(id)?, field)?.hash)
    };
    for (i, d) in p.docs.iter().enumerate() {
        let Some(r) = parsed[i].as_ref().filter(|r| r.err.is_none()) else {
            continue;
        };
        let root = r.root.as_ref().expect("a parsed document");
        let mut strs = Vec::new();
        link_strings(root, true, &mut strs);
        let mut edits = Vec::new();
        for n in strs {
            let mut out = String::new();
            let mut last = 0;
            for rf in mdscan::new(&n.str).refs() {
                if rf.hash.is_some() {
                    continue;
                }
                let Some(mut h) = hash_of(&rf.inner) else {
                    continue;
                };
                if stale_new.contains(&(d.id.clone(), rf.inner.clone())) {
                    h = placeholder(&h);
                    rep.unconfirmed += 1;
                }
                out.push_str(&n.str[last..rf.end - 2]);
                out.push('@');
                out.push_str(&h);
                last = rf.end - 2;
                rep.hashes_added += 1;
            }
            if last > 0 {
                out.push_str(&n.str[last..]);
                edits.push((n.start, n.end, json(&out)));
            }
        }
        let bytes = if edits.is_empty() { data[i].take() } else { Some(splice(&r.src.data, edits)) };
        if let Some(b) = bytes {
            write_doc(&p, d, &b)?;
            rep.documents += 1;
        }
    }

    // 4. code files
    if !p.cfg.trace_scan.is_empty() {
        let skip = [p.cfg.abs(&p.cfg.src), p.cfg.abs(&p.cfg.out), p.cfg.abs(&p.cfg.out_full)];
        for rel in trace::files(&p.cfg.root, &p.cfg.trace_scan, &skip)? {
            let abs = p.cfg.root.join(&rel);
            let Ok(old) = std::fs::read(&abs) else { continue };
            let Ok(old) = String::from_utf8(old) else { continue };
            if old.contains('\0') {
                continue;
            }
            let new = migrate_code(&rel, &old, &rename, &hash_of, &stale_t, &stale_c, &mut rep);
            if new != old {
                std::fs::write(&abs, new).map_err(|e| go_path_error("open", &abs, &e))?;
                rep.code_files += 1;
            }
        }
    }

    // 5. the lock goes: hashes live in the links now
    let lock = p.cfg.abs(super::LOCK_FILE);
    if std::fs::metadata(&lock).is_ok() {
        std::fs::remove_file(&lock).map_err(|e| go_path_error("remove", &lock, &e))?;
        rep.lock_removed = true;
    }
    Ok(rep)
}

/// migrate_code rewrites one code file: id#^anchor becomes id#^code, every
/// target of a docc: tag gets its hash and every tag its fingerprint.
fn migrate_code(
    rel: &str,
    old: &str,
    rename: &dyn Fn(&str) -> Option<String>,
    hash_of: &dyn Fn(&str) -> Option<String>,
    stale_t: &HashSet<(String, String)>,
    stale_c: &HashSet<(String, String)>,
    rep: &mut Report,
) -> String {
    // what docc.lock held unconfirmed, by the targets as they will be written
    let (mut st, mut sc) = (HashSet::new(), HashSet::new());
    for t in trace::scan_file(rel, old.as_bytes()) {
        for g in &t.targets {
            let n = rename(g).unwrap_or_else(|| g.clone());
            if stale_t.contains(&(rel.to_string(), g.clone())) {
                st.insert(n.clone());
            }
            if stale_c.contains(&(rel.to_string(), g.clone())) {
                sc.insert(t.line);
            }
        }
    }
    let s = OLD_REF.replace_all(old, |c: &regex::Captures| match rename(&c[0]) {
        Some(n) => {
            rep.code_refs_rewritten += 1;
            n
        }
        None => c[0].to_string(),
    });
    // hashes on the tag lines
    let mut lines: Vec<String> = s.split('\n').map(str::to_string).collect();
    for line in &mut lines {
        let Some(at) = trace::tag_at(line.as_bytes()) else { continue };
        let mut start = at + "docc:".len();
        let rest = &line[start..];
        let word = rest.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        if word == "end" || word.starts_with("end*/") || word.starts_with("end-->") {
            continue;
        }
        if word == "begin" {
            start += word.len();
        }
        let (body, tail) = split_tail(&line[start..]);
        let mut pieces: Vec<String> = Vec::new();
        for piece in body.split(',') {
            let core = piece.trim();
            if core.is_empty() || codes::split_hash(core).1.is_some() {
                pieces.push(piece.to_string());
                continue;
            }
            let (t, _) = codes::split_hash(core);
            match hash_of(&t) {
                Some(mut h) => {
                    if st.contains(&t) {
                        h = placeholder(&h);
                        rep.unconfirmed += 1;
                    }
                    rep.tag_hashes_added += 1;
                    pieces.push(piece.replacen(core, &format!("{core}@{h}"), 1));
                }
                None => pieces.push(piece.to_string()),
            }
        }
        *line = format!("{}{}{tail}", &line[..start], pieces.join(","));
    }
    let s = lines.join("\n");
    // fingerprints: they leave out hashes and fingerprints, so writing them changes none
    let tags = trace::scan_file(rel, s.as_bytes());
    let mut lines: Vec<String> = s.split('\n').map(str::to_string).collect();
    for t in tags {
        if t.print.is_some() || t.targets.is_empty() || !t.err.is_empty() {
            continue;
        }
        let mut want = t.want.clone();
        if sc.contains(&t.line) {
            want = placeholder(&want);
            rep.unconfirmed += 1;
        }
        let line = &mut lines[t.line - 1];
        let cr = line.ends_with('\r');
        let l = line.trim_end_matches('\r');
        let at = trace::tag_at(l.as_bytes()).expect("a tag line");
        let (body, tail) = split_tail(&l[at..]);
        *line = format!("{}{} ~{want}{tail}{}", &l[..at], body.trim_end(), if cr { "\r" } else { "" });
        rep.prints_added += 1;
    }
    lines.join("\n")
}

/// split_tail cuts the end of a tag line: blanks, a comment closer and the
/// line's carriage return, and an existing fingerprint stays in the body.
fn split_tail(s: &str) -> (&str, &str) {
    let t = s.trim_end_matches(['\r', ' ', '\t']);
    for c in ["*/", "-->"] {
        if let Some(b) = t.strip_suffix(c) {
            let b2 = b.trim_end();
            return (&s[..b2.len()], &s[b2.len()..]);
        }
    }
    (&s[..t.len()], &s[t.len()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails() {
        assert_eq!(split_tail("a, b */"), ("a, b", " */"));
        assert_eq!(split_tail("a -->\r"), ("a", " -->\r"));
        assert_eq!(split_tail("a, b"), ("a, b", ""));
    }
}
