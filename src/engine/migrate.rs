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

/// key_in is the key the document says it has.
fn key_in(d: &Doc) -> Option<String> {
    d.root()?.get("key").filter(|k| k.kind == Kind::String && codes::is_key(&k.str)).map(|k| k.str.clone())
}

/// numbers notes every code of the project.
fn numbers(p: &Project) -> codes::Numbers {
    let mut n = codes::Numbers::default();
    for d in &p.docs {
        for sn in d.strings() {
            if let Some(c) = codes::code_of(&sn.node().str) {
                n.note(c);
            }
        }
    }
    n
}

/// Keys gives each document its key: the one it says it has, else the one
/// most of its codes carry, else one made from its id that no other
/// document has.
struct Keys(HashSet<String>);

impl Keys {
    fn of(p: &Project) -> Keys {
        let mut taken: HashSet<String> = p.docs.iter().filter_map(key_in).collect();
        for d in &p.docs {
            for sn in d.strings() {
                if let Some(c) = codes::code_of(&sn.node().str) {
                    taken.insert(codes::key_of(c).to_string());
                }
            }
        }
        Keys(taken)
    }

    /// pick is the key of d and whether the file still has to say it.
    fn pick(&mut self, d: &Doc, codes_there: &[&str]) -> (String, bool) {
        if let Some(k) = key_in(d) {
            return (k, false);
        }
        let mut count: HashMap<&str, usize> = HashMap::new();
        for c in codes_there {
            *count.entry(codes::key_of(c)).or_default() += 1;
        }
        let k = match count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0))) {
            Some((k, _)) => k.to_string(),
            None => codes::propose_key(&d.id, &|k| self.0.contains(k)),
        };
        self.0.insert(k.clone());
        (k, true)
    }
}

/// key_edit inserts "key" after "kind" (else after "title", else first),
/// on a line of its own indented like that member when the file has lines.
fn key_edit(d: &Doc, key: &str) -> Option<(usize, usize, String)> {
    let root = d.root()?;
    let data = &d.res().src.data;
    let Some(m) = ["kind", "title"].iter().find_map(|k| root.members.iter().find(|m| m.key == *k)) else {
        let lit = format!("\"key\": {}", json(key));
        let at = root.start + 1;
        return Some((at, at, if root.members.is_empty() { lit } else { format!("{lit}, ") }));
    };
    // the colon written like that member's
    let colon = String::from_utf8_lossy(&data[m.key_end..m.value.start]);
    let lit = format!("\"key\"{colon}{}", json(key));
    let ls = data[..m.key_start].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
    let indent = &data[ls..m.key_start];
    let sep = if ls > 0 && indent.iter().all(|b| *b == b' ' || *b == b'\t') {
        format!("\n{}", String::from_utf8_lossy(indent))
    } else {
        " ".to_string()
    };
    Some((m.value.end, m.value.end, format!(",{sep}{lit}")))
}

/// assign_codes gives a new code to every block of the project that has
/// none (docc build, docc codes): the key of its document, a dash and the
/// largest number of that key plus one; a document without "key" gets one.
/// It returns how many codes it gave and never touches a code that is
/// there or a hash. A block that ends in a marker which is not a code
/// (DOC216) is left alone.
pub fn assign_codes(p: &Project) -> Res<usize> {
    if p.legacy {
        return Ok(0);
    }
    let mut nums = numbers(p);
    let mut keys = Keys::of(p);
    let mut n = 0;
    for d in &p.docs {
        let strs = d.strings();
        let need: Vec<_> = strs
            .iter()
            .filter(|sn| sn.blk && !sn.notes && sn.anchor.is_empty() && !sn.bad_mark && !sn.node().str.trim().is_empty())
            .collect();
        let there: Vec<&str> = strs.iter().filter(|sn| !sn.notes).filter_map(|sn| codes::code_of(&sn.node().str)).collect();
        if need.is_empty() && (there.is_empty() || key_in(d).is_some()) {
            continue;
        }
        let (key, write) = keys.pick(d, &there);
        let mut edits = Vec::new();
        if write {
            edits.extend(key_edit(d, &key));
        }
        for sn in &need {
            let s = &sn.node().str;
            let add = codes::with_code(s, &nums.next(&key))[s.len()..].to_string();
            let lit = json(&add);
            let end = sn.node().end - 1; // before the closing quote
            edits.push((end, end, lit[1..lit.len() - 1].to_string()));
        }
        n += need.len();
        if !edits.is_empty() {
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
    /// hashes rewritten because the codes inside their targets moved: a link
    /// that matched its target before matches it after, one that did not
    /// still does not
    pub hashes_moved: usize,
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
    /// fingerprints rewritten because the codes in their lines moved, kept
    /// matching or not matching as before
    pub prints_moved: usize,
    /// links and regions that docc.lock held unconfirmed: they got a hash that
    /// does not match, so docc check asks for a review of each
    pub unconfirmed: usize,
    /// docc.lock was deleted
    pub lock_removed: bool,
}

/// OLD_REF is id#^name in a code file: a named anchor, a code of either format.
static OLD_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)([a-z0-9][a-z0-9-]*)#\^([a-z0-9][a-z0-9-]*)").unwrap());

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

    // 1. codes: a code of its own document keeps its block (the first one in
    // path order when copied); codes of the old format, named anchors, codes
    // of another document and blocks without a marker get new ones: the key
    // of the document and the next number of it
    let mut keys = Keys::of(&p);
    let mut key_of: HashMap<String, (String, bool)> = HashMap::new(); // id -> key, to be written
    for d in &p.docs {
        let strs = d.strings();
        let there: Vec<&str> = strs.iter().filter(|sn| !sn.notes).filter_map(|sn| codes::code_of(&sn.node().str)).collect();
        if d.root().is_some() && strs.iter().any(|sn| sn.blk && !sn.notes && !sn.node().str.trim().is_empty()) {
            key_of.insert(d.id.clone(), keys.pick(d, &there));
        }
    }
    let mut used: HashSet<String> = HashSet::new();
    let mut keep: HashSet<*const Node> = HashSet::new();
    let mut nums = codes::Numbers::default();
    for d in &p.docs {
        let Some((key, _)) = key_of.get(&d.id) else { continue };
        for sn in d.strings().iter().filter(|sn| sn.blk && !sn.notes) {
            if let Some(c) = codes::code_of(&sn.node().str)
                && codes::key_of(c) == key
                && used.insert(c.to_string())
            {
                keep.insert(sn.node());
                nums.note(c);
            }
        }
    }
    let mut map: HashMap<(String, String), String> = HashMap::new(); // (id, old name) -> code
    let mut text: HashMap<*const Node, String> = HashMap::new(); // new text of a string
    let mut code_at: HashMap<*const Node, String> = HashMap::new(); // the code of a block
    for d in &p.docs {
        let Some((key, _)) = key_of.get(&d.id) else { continue };
        for sn in d.strings().iter().filter(|sn| sn.blk && !sn.notes) {
            let n = sn.node();
            let s = &n.str;
            if keep.contains(&(n as *const Node)) {
                let c = codes::code_of(s).unwrap().to_string();
                map.entry((d.id.clone(), c.clone())).or_insert_with(|| c.clone());
                code_at.insert(n, c);
            } else if let Some(m) = codes::ANY_MARK.captures(s) {
                let new = nums.next(key);
                let at = m.get(1).unwrap().start();
                text.insert(n, format!("{}{new}", &s[..at]));
                map.entry((d.id.clone(), codes::canon(&m[1]))).or_insert_with(|| new.clone());
                code_at.insert(n, new);
                rep.anchors_replaced += 1;
            } else if !s.trim().is_empty() {
                let new = nums.next(key);
                text.insert(n, codes::with_code(s, &new));
                code_at.insert(n, new);
                rep.codes_given += 1;
            }
        }
    }
    let rename = |target: &str| -> Option<String> {
        let (id, name) = target.split_once("#^")?;
        let c = map.get(&(id.to_string(), codes::canon(name)))?;
        (name != c.as_str()).then(|| format!("{id}#^{c}"))
    };
    // a link or tag whose hash matched its target before keeps matching after
    // (its target's text changed only by the codes); one that did not keeps
    // not matching: the move confirms nothing
    let old_hash = |target: &str| -> Option<String> {
        let (t, _) = codes::split_hash(target);
        let (id, field, _, ok) = split_ref(&t, p.legacy);
        if !ok {
            return None;
        }
        let d = &p.docs[*p.graph.get(id)?];
        Some(version_in(d.root()?, field)?.hash)
    };

    // 2. links follow the codes, comments point at codes
    let mut stale_new: HashSet<(String, String)> = HashSet::new();
    // (document, link as written after the move, its hash) that matched before
    let mut held: HashSet<(String, String, String)> = HashSet::new();
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
                if let Some(h) = &r.hash
                    && old_hash(&r.inner).as_ref() == Some(h)
                {
                    held.insert((d.id.clone(), inner.clone(), h.clone()));
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
        if let Some((k, true)) = key_of.get(&d.id) {
            edits.extend(key_edit(d, k));
        }
        if let Some(cs) = root.get(comments::KEY).filter(|c| c.kind == Kind::Array) {
            for it in &cs.items {
                // a comment at a code that moved follows it
                if let Some(at) = it.get("at").filter(|a| a.kind == Kind::String)
                    && let Some(old) = at.str.strip_prefix('^')
                {
                    if let Some(c) = map.get(&(d.id.clone(), codes::canon(old))).filter(|c| **c != old) {
                        edits.push((at.start, at.end, json(&format!("^{c}"))));
                        rep.comments_moved += 1;
                    }
                    continue;
                }
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
                if let Some(h) = &rf.hash {
                    let Some(now) = hash_of(&rf.inner) else { continue };
                    let new = if held.contains(&(d.id.clone(), rf.inner.clone(), h.clone())) {
                        now
                    } else if now == *h {
                        rep.unconfirmed += 1;
                        placeholder(&now)
                    } else {
                        continue;
                    };
                    if new != *h {
                        // the hash is the last four signs before "]]"
                        out.push_str(&n.str[last..rf.end - 6]);
                        out.push_str(&new);
                        last = rf.end - 2;
                        rep.hashes_moved += 1;
                    }
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
            let new = migrate_code(&rel, &old, &rename, &hash_of, &old_hash, &stale_t, &stale_c, &mut rep);
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

/// moved is the hash a migration writes where h was: the target's hash now
/// when h matched the target before, h when it did not and still does not,
/// and one that does not match when it did not but now would.
fn moved(held: bool, h: &str, now: &str, rep: &mut Report) -> String {
    if held {
        now.to_string()
    } else if h == now {
        rep.unconfirmed += 1;
        placeholder(now)
    } else {
        h.to_string()
    }
}

/// migrate_code rewrites one code file: id#^anchor becomes id#^code, every
/// target of a docc: tag gets its hash and every tag its fingerprint. A hash
/// or fingerprint that matched before matches after; one that did not, not.
#[allow(clippy::too_many_arguments)]
fn migrate_code(
    rel: &str,
    old: &str,
    rename: &dyn Fn(&str) -> Option<String>,
    hash_of: &dyn Fn(&str) -> Option<String>,
    old_hash: &dyn Fn(&str) -> Option<String>,
    stale_t: &HashSet<(String, String)>,
    stale_c: &HashSet<(String, String)>,
    rep: &mut Report,
) -> String {
    // what docc.lock held unconfirmed, by the targets as they will be written;
    // the hashes and fingerprints that matched before
    let (mut st, mut sc) = (HashSet::new(), HashSet::new());
    let (mut held_t, mut held_p) = (HashSet::new(), HashSet::new());
    for t in trace::scan_file(rel, old.as_bytes()) {
        if t.print.is_some() && t.print.as_deref() == Some(t.want.as_str()) {
            held_p.insert(t.line);
        }
        for (g, h) in t.targets.iter().zip(&t.hashes) {
            if let Some(h) = h
                && old_hash(g).as_deref() == Some(h.as_str())
            {
                held_t.insert((rename(g).unwrap_or_else(|| g.clone()), h.clone()));
            }
        }
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
            if let (t, Some(h)) = codes::split_hash(core)
                && let Some(now) = hash_of(&t)
            {
                let new = moved(held_t.contains(&(t.clone(), h.clone())), &h, &now, rep);
                if new != h {
                    let at = core.rfind('@').expect("a hash");
                    pieces.push(piece.replacen(core, &format!("{}@{new}", &core[..at]), 1));
                    rep.hashes_moved += 1;
                    continue;
                }
            }
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
        if let Some(p) = &t.print {
            let new = moved(held_p.contains(&t.line), p, &t.want, rep);
            let line = &mut lines[t.line - 1];
            if new != *p
                && let Some(at) = line.to_ascii_lowercase().rfind(&format!("~{p}"))
            {
                line.replace_range(at + 1..at + 1 + p.len(), &new);
                rep.prints_moved += 1;
            }
            continue;
        }
        if t.targets.is_empty() || !t.err.is_empty() {
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
