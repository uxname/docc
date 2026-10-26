use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::analyze::split_ref;
use super::linkrules::{canonical, quote};
use super::mdrules::fragment_problem;
use super::project::{Project, go_path_error, usage};
use super::strings::resolve;

/// LockFile is the link lock next to docc.json.
pub const LOCK_FILE: &str = "docc.lock";

/// Link is one (source, target) pair of the lock: a document id or a code file,
/// and a target as written. Hash is the hash of the target value, Code the hash
/// of the code regions of a region link ("" otherwise).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Link {
    pub source: String,
    pub target: String,
    pub hash: String,
    pub code: String,

    /// source document (index in docs), None for a code file
    pub(super) doc: Option<usize>,
    /// reference in doc
    pub(super) start: usize,
    pub(super) end: usize,
    /// tag in the code file
    pub(super) line: usize,
    pub(super) col: usize,
    /// line in docc.lock
    pub(super) at: usize,
}

impl Link {
    fn key(&self) -> String {
        format!("{} {}", self.source, self.target)
    }
}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            lock_esc(&self.source),
            lock_esc(&self.target),
            self.hash
        )?;
        if !self.code.is_empty() {
            write!(f, " {}", self.code)?;
        }
        Ok(())
    }
}

/// lockEsc escapes '%' and white space as %XX so a line splits on spaces;
/// a section key such as "2.1 Overview" stays readable otherwise.
fn lock_esc(s: &str) -> String {
    let mut b = String::with_capacity(s.len());
    for r in s.chars() {
        if r == '%' || r.is_whitespace() || r.is_control() {
            for c in r.encode_utf8(&mut [0; 4]).bytes() {
                let _ = write!(b, "%{c:02X}");
            }
            continue;
        }
        b.push(r);
    }
    b
}

fn hash8(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))[..8].to_string()
}

fn is_hash(s: &str) -> bool {
    s.len() == 8
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// path_unescape is Go's url.PathUnescape; None on a malformed escape.
// ponytail: bytes that are not UTF-8 after unescaping become U+FFFD.
fn path_unescape(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() || !b[i + 1].is_ascii_hexdigit() || !b[i + 2].is_ascii_hexdigit() {
                return None;
            }
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}

/// lockSkip are the top-level keys a whole-document target does not depend on.
const LOCK_SKIP: &[&str] = &["notes", "docIgnore", "hidden", "order", "parent", "$comments"];

impl Project {
    /// targetHash hashes the canonical JSON of the value a target points to: the
    /// document without service fields, order and parent; a field or section; the
    /// text of an anchored paragraph. None when the target does not resolve.
    fn target_hash(&self, target: &str) -> Option<String> {
        let (id, field, has_field, ok) = split_ref(target);
        let t = self.doc(id)?;
        let root = t.root()?;
        if !ok {
            return None;
        }
        let mut b = String::new();
        if !has_field {
            canonical(&mut b, root, LOCK_SKIP);
        } else {
            if !fragment_problem(t, id, field).0.is_empty() {
                return None;
            }
            if let Some(name) = field.strip_prefix('^') {
                let sn = t
                    .block_anchor(name)
                    .expect("fragment_problem checked the anchor");
                let (text, _) = t.block(sn.node());
                quote(&mut b, text);
            } else {
                canonical(
                    &mut b,
                    resolve(root, field).expect("fragment_problem checked the field"),
                    &[],
                );
            }
        }
        Some(hash8(&b))
    }

    /// Links returns the current links sorted by key, with the hashes of their
    /// targets now: [[...]] in the text and notes of every document (hidden too)
    /// and docc: tags in code. Self links and targets that do not resolve are left
    /// out; a pair seen twice keeps its first place.
    pub fn links(&self) -> Vec<Link> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut cache: HashMap<String, Option<String>> = HashMap::new(); // many links share a target
        let mut out: Vec<Link> = Vec::new();
        let mut add = |mut l: Link| {
            let key = l.key();
            if seen.contains(&key) {
                return;
            }
            let h = cache
                .entry(l.target.clone())
                .or_insert_with(|| self.target_hash(&l.target));
            let Some(h) = h else {
                return;
            };
            seen.insert(key);
            l.hash = h.clone();
            out.push(l);
        };
        for (i, d) in self.docs.iter().enumerate() {
            if !d.parsed() || !self.in_graph(d) {
                continue;
            }
            for r in &d.refs {
                if r.ok && r.target != d.id {
                    add(Link {
                        source: d.id.clone(),
                        target: r.inner.clone(),
                        doc: Some(i),
                        start: r.start,
                        end: r.end,
                        ..Default::default()
                    });
                }
            }
        }
        let mut keys: Vec<Link> = self
            .code
            .iter()
            .map(|(k, h)| {
                let at = self.code_at.get(k).copied().unwrap_or_default();
                Link {
                    source: k.file.clone(),
                    target: k.target.clone(),
                    code: if h.is_empty() {
                        String::new()
                    } else {
                        h[..8].to_string()
                    },
                    line: at.0,
                    col: at.1,
                    ..Default::default()
                }
            })
            .collect();
        keys.sort_by_cached_key(Link::key);
        for l in keys {
            add(l);
        }
        out.sort_by_cached_key(Link::key);
        out
    }

    /// readLock returns the text of docc.lock, the lines that parse, the number
    /// of the first one that does not (0: none) and whether the file exists.
    fn read_lock(&self) -> (String, Vec<Link>, usize, bool) {
        let Ok(b) = self.cfg.source.read(&self.cfg.abs(LOCK_FILE)) else {
            return (String::new(), Vec::new(), 0, false);
        };
        let data = String::from_utf8_lossy(&b).replace("\r\n", "\n"); // a checkout with autocrlf
        let (mut ls, mut bad) = (Vec::new(), 0);
        for (i, row) in lock_rows(&data).into_iter().enumerate() {
            if let Some(mut l) = parse_lock_line(row) {
                l.at = i + 1;
                ls.push(l);
            } else if bad == 0 {
                bad = i + 1;
            }
        }
        (data, ls, bad, true)
    }
}

fn lock_rows(data: &str) -> Vec<&str> {
    if data.is_empty() {
        return Vec::new();
    }
    data.strip_suffix('\n')
        .unwrap_or(data)
        .split('\n')
        .collect()
}

fn parse_lock_line(row: &str) -> Option<Link> {
    let f: Vec<&str> = row.split(' ').collect();
    if f.len() != 3 && f.len() != 4 {
        return None;
    }
    let mut l = Link {
        hash: f[2].to_string(),
        ..Default::default()
    };
    let (s, t) = (path_unescape(f[0]), path_unescape(f[1]));
    let mut ok = true;
    if f.len() == 4 {
        l.code = f[3].to_string();
        ok = is_hash(&l.code);
    }
    let (Some(s), Some(t)) = (s, t) else {
        return None;
    };
    (l.source, l.target) = (s, t);
    (ok && !l.source.is_empty() && !l.target.is_empty() && is_hash(&l.hash)).then_some(l)
}

/// firstByKey maps a key to its first line in the lock.
fn first_by_key(ls: &[Link]) -> HashMap<String, &Link> {
    let mut m = HashMap::new();
    for l in ls {
        m.entry(l.key()).or_insert(l);
    }
    m
}

/// moved pairs a new region link with the region link it replaces when a code file
/// moved: the old source is gone from the links and, for that target, exactly one region
/// link vanished that way and exactly one new one appeared. The new link keeps the old
/// fingerprints, so a change made on the way is still unconfirmed (DOC402, DOC209)
/// instead of being taken as new. Two or more on either side cannot be told apart and
/// are left new.
fn moved<'a>(
    cur: &[Link],
    stored: &'a [Link],
    old: &HashMap<String, &Link>,
) -> HashMap<String, &'a Link> {
    let sources: HashSet<&str> = cur.iter().map(|l| l.source.as_str()).collect();
    let mut gone: HashMap<&str, Vec<&'a Link>> = HashMap::new();
    for o in stored {
        if !o.code.is_empty() && !sources.contains(o.source.as_str()) {
            gone.entry(o.target.as_str()).or_default().push(o);
        }
    }
    let mut new: HashMap<&str, Vec<&Link>> = HashMap::new();
    for l in cur {
        if !l.code.is_empty() && !old.contains_key(&l.key()) {
            new.entry(l.target.as_str()).or_default().push(l);
        }
    }
    let mut out = HashMap::new();
    for (target, ls) in new {
        if let (Some([o]), [l]) = (gone.get(target).map(Vec::as_slice), ls.as_slice()) {
            out.insert(l.key(), *o);
        }
    }
    out
}

/// lockText is what docc build writes: every current link, with the hashes
/// stored for it when the lock has it (or for the link it moved from), else with
/// the current ones.
fn lock_text(cur: &[Link], stored: &[Link]) -> String {
    let old = first_by_key(stored);
    let moved = moved(cur, stored, &old);
    let mut b = String::new();
    for l in cur {
        let (mut hash, mut code) = (&l.hash, &l.code);
        if let Some(o) = old.get(&l.key()).or_else(|| moved.get(&l.key())) {
            hash = &o.hash;
            if !o.code.is_empty() && !l.code.is_empty() {
                code = &o.code;
            }
        }
        let l = Link {
            hash: hash.clone(),
            code: code.clone(),
            ..l.clone()
        };
        let _ = writeln!(b, "{l}");
    }
    b
}

/// stale reports whether the lock line o no longer matches the current link l:
/// the target (or, for a region link, the code) changed after it was confirmed.
fn stale(o: &Link, l: &Link) -> bool {
    o.hash != l.hash || o.code != l.code
}

impl Project {
    /// WriteLock rewrites docc.lock for docc build: new links are confirmed with
    /// their current hashes, vanished ones dropped. Without links there is no file.
    pub fn write_lock(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let (data, stored, _, exists) = self.read_lock();
        let want = lock_text(&self.links(), &stored);
        let abs = self.cfg.abs(LOCK_FILE);
        if want == data && exists {
            return Ok(());
        }
        if want.is_empty() {
            return match std::fs::remove_file(&abs) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    Err(go_path_error("remove", &abs, &e).into())
                }
                _ => Ok(()),
            };
        }
        std::fs::write(&abs, want).map_err(|e| go_path_error("open", &abs, &e).into())
    }

    /// checkLock reports links whose target (DOC209) or code region (DOC402)
    /// changed after they were confirmed in docc.lock. Several stale links of one
    /// document to parts of the same document are one DOC209, at the first of them.
    pub(super) fn check_lock(&self) {
        let (_, stored, _, _) = self.read_lock();
        if stored.is_empty() {
            return;
        }
        let old = first_by_key(&stored);
        struct Group {
            first: Link,
            targets: Vec<String>,
        }
        let mut groups: HashMap<String, Group> = HashMap::new();
        let mut order: Vec<String> = Vec::new(); // groups in link order
        for l in self.links() {
            let Some(o) = old.get(&l.key()) else {
                continue;
            };
            if o.hash != l.hash {
                if l.doc.is_some() {
                    let (id, _, _, _) = split_ref(&l.target);
                    let k = format!("{} {}", l.source, id);
                    match groups.get_mut(&k) {
                        None => {
                            order.push(k.clone());
                            groups.insert(
                                k,
                                Group {
                                    targets: vec![l.target.clone()],
                                    first: l.clone(),
                                },
                            );
                        }
                        Some(g) => {
                            if l.start < g.first.start {
                                g.first = l.clone();
                            }
                            g.targets.push(l.target.clone());
                        }
                    }
                } else {
                    let msg = format!(
                        "the target {} changed after the link from {} was confirmed",
                        l.target, l.source
                    );
                    self.emit_file(
                        &l.source,
                        "DOC209",
                        l.line,
                        l.col,
                        msg,
                        format!(
                            "Check the code in {} against {}, then run docc ack {}.",
                            l.source, l.target, l.target
                        ),
                    );
                }
            }
            if !o.code.is_empty() && !l.code.is_empty() && o.code != l.code {
                self.emit_file(
                    &l.source,
                    "DOC402",
                    l.line,
                    l.col,
                    format!("code region in {} for {} changed", l.source, l.target),
                    format!(
                        "Check the document {} still matches, then run docc ack {}.",
                        l.target, l.source
                    ),
                );
            }
        }
        for k in &order {
            let g = &groups[k];
            let l = &g.first;
            let mut msg = format!(
                "the target {} changed after the link from {} was confirmed",
                l.target, l.source
            );
            if g.targets.len() > 1 {
                msg = format!(
                    "the targets {} changed after the links from {} were confirmed",
                    g.targets.join(", "),
                    l.source
                );
            }
            let mut hint = format!(
                "Check {} against {}, then run docc ack {} (it prints the links it confirms).",
                l.source, l.target, l.target
            );
            if !l.target.contains('#') {
                let _ = write!(
                    hint,
                    " To be warned only when one part changes, link {}#^anchor or {}#field instead of the whole document.",
                    l.target, l.target
                );
            }
            let d = &self.docs[l.doc.expect("a document link")];
            self.emit(d, "DOC209", l.start, l.end, msg, hint);
        }
    }

    /// CheckLock reports DOC503 when docc.lock is not what docc build would write:
    /// a link is missing or extra, a key repeats, a line is malformed.
    pub fn check_lock_file(&self) {
        let (data, stored, bad, exists) = self.read_lock();
        let cur = self.links();
        if lock_text(&cur, &stored) == data {
            return;
        }
        let (mut line, mut msg) = (
            1,
            "docc.lock is not in the form docc build writes".to_string(),
        );
        let now: HashSet<String> = cur.iter().map(Link::key).collect();
        let first = first_by_key(&stored);
        if !exists {
            msg = format!(
                "docc.lock is missing, and the sources have {} links",
                cur.len()
            );
        } else if bad > 0 {
            (line, msg) = (
                bad,
                format!("line {bad} is not \"<source> <target> <hash> [<code hash>]\""),
            );
        } else {
            let mut found = false;
            for l in &stored {
                if first[&l.key()].at != l.at {
                    (line, msg, found) = (
                        l.at,
                        format!("the link from {} to {} is listed twice", l.source, l.target),
                        true,
                    );
                } else if !now.contains(&l.key()) {
                    (line, msg, found) = (
                        l.at,
                        format!(
                            "the link from {} to {} no longer exists",
                            l.source, l.target
                        ),
                        true,
                    );
                }
                if found {
                    break;
                }
            }
            for l in &cur {
                if !first.contains_key(&l.key()) && !found {
                    (msg, found) = (
                        format!(
                            "the link from {} to {} is not in docc.lock",
                            l.source, l.target
                        ),
                        true,
                    );
                }
            }
        }
        self.emit_file(
            LOCK_FILE,
            "DOC503",
            line,
            1,
            msg,
            "Run docc build and commit docc.lock.".into(),
        );
    }

    /// Ack confirms the stale lock lines around each name: its source or target is
    /// the name, or the target lies in the document with that id; with all, every
    /// stale line. Lines that are not stale are left as they are, and so are lines
    /// of links that no longer exist (docc build drops them). It returns the links
    /// it confirmed.
    pub fn ack(
        &self,
        names: &[String],
        all: bool,
    ) -> Result<Vec<Link>, Box<dyn Error + Send + Sync>> {
        let (data, _, _, exists) = self.read_lock();
        if !exists {
            return Err(usage("docc.lock not found: run docc build first".into()));
        }
        let cur: HashMap<String, Link> = self.links().into_iter().map(|l| (l.key(), l)).collect();
        let mut rows: Vec<String> = lock_rows(&data).into_iter().map(String::from).collect();
        let mut hit: HashSet<&str> = HashSet::new();
        let mut done = Vec::new();
        for row in &mut rows {
            let Some(mut l) = parse_lock_line(row) else {
                continue;
            };
            let Some(c) = cur.get(&l.key()) else {
                continue;
            };
            let mut matched = all;
            let (id, _, _, _) = split_ref(&l.target);
            for x in names {
                if l.source == *x || l.target == *x || id == x {
                    matched = true;
                    hit.insert(x);
                }
            }
            if matched && stale(&l, c) {
                (l.hash, l.code) = (c.hash.clone(), c.code.clone());
                *row = l.to_string();
                done.push(Link {
                    source: l.source,
                    target: l.target,
                    ..Default::default()
                });
            }
        }
        for x in names {
            if !hit.contains(x.as_str()) {
                return Err(usage(format!(
                    "no link in docc.lock matches {} (a new link gets there with docc build)",
                    crate::config::go_quote(x)
                )));
            }
        }
        if done.is_empty() {
            return Ok(Vec::new());
        }
        let abs = self.cfg.abs(LOCK_FILE);
        std::fs::write(&abs, rows.join("\n") + "\n")
            .map_err(|e| go_path_error("open", &abs, &e))?;
        Ok(done)
    }
}
