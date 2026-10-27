use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::analyze::split_ref;
use super::linkrules::{canonical, quote};
use super::mdrules::fragment_problem;
use super::project::Project;
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
        let (id, field, has_field, ok) = split_ref(target, true);
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

/// Pairs are (source, target) pairs of links.
pub type Pairs = HashSet<(String, String)>;

impl Project {
    /// stale_links are the links of an old docc.lock that were not confirmed
    /// after their target changed, and the regions whose code changed: docc
    /// migrate does not confirm them on the way. Each is (source, target as written).
    pub fn stale_links(&self) -> (Pairs, Pairs) {
        let (_, stored, _, _) = self.read_lock();
        let mut old: HashMap<String, &Link> = HashMap::new();
        for l in &stored {
            old.entry(l.key()).or_insert(l);
        }
        let (mut target, mut code) = (HashSet::new(), HashSet::new());
        for l in self.links() {
            if let Some(o) = old.get(&l.key()) {
                if o.hash != l.hash {
                    target.insert((l.source.clone(), l.target.clone()));
                }
                if !o.code.is_empty() && !l.code.is_empty() && o.code != l.code {
                    code.insert((l.source, l.target));
                }
            }
        }
        (target, code)
    }
}
