//! Version hashes of links (DOC209): every [[...]] and every target of a
//! docc: tag in code carries the hash of what it points to, and a hash that
//! is missing or no longer matches is an error until an agent reads the
//! change and writes the new hash by hand.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::process::Command;

use super::project::{Doc, Project};
use super::strings::{SECTIONS_KEY, resolve};
use crate::codes;
use crate::comments;
use crate::config::go_quote;
use crate::loader::{self, Kind, Node};

/// Version is the hash of a target now and its text as a reader sees it.
#[derive(Clone, Debug)]
pub struct Version {
    pub hash: String,
    pub text: String,
}

/// find_code is the string block of root that ends in the code.
pub fn find_code<'a>(root: &'a Node, code: &str) -> Option<&'a Node> {
    fn walk<'a>(n: &'a Node, top: bool, code: &str) -> Option<&'a Node> {
        match n.kind {
            Kind::String => (codes::code_of(&n.str) == Some(code)).then_some(n),
            Kind::Array => n.items.iter().find_map(|it| walk(it, false, code)),
            Kind::Object => n
                .members
                .iter()
                .filter(|m| !(top && matches!(m.key.as_str(), "notes" | "docIgnore" | comments::KEY)))
                .find_map(|m| walk(&m.value, false, code)),
            _ => None,
        }
    }
    walk(root, true, code)
}

/// lines is the text of a value, one block per line, without codes.
fn lines(n: &Node, out: &mut Vec<String>) {
    match n.kind {
        Kind::String => out.push(codes::normalize(&n.str)),
        Kind::Array => n.items.iter().for_each(|it| lines(it, out)),
        Kind::Object => {
            for m in &n.members {
                if m.key == SECTIONS_KEY {
                    continue;
                }
                if m.value.kind == Kind::Object || m.value.kind == Kind::Array {
                    out.push(format!("{}:", m.key));
                    lines(&m.value, out);
                } else {
                    out.push(format!("{}: {}", m.key, codes::normalize(&m.value.str)));
                }
            }
        }
        Kind::Null => {}
        _ => out.push(n.str.clone()),
    }
}

/// version_in is the version of the target field of the document root: ""
/// for the whole document, "^code" for a block, else a field or section path.
pub fn version_in(root: &Node, field: &str) -> Option<Version> {
    let mut text = Vec::new();
    let hash = if field.is_empty() {
        for m in &root.members {
            if !codes::DOC_SKIP.contains(&m.key.as_str()) && m.key != "kind" {
                if m.key == "title" {
                    lines(&m.value, &mut text);
                } else {
                    text.push(format!("{}:", m.key));
                    lines(&m.value, &mut text);
                }
            }
        }
        codes::doc_hash(root)
    } else if let Some(code) = field.strip_prefix('^') {
        let n = find_code(root, code)?;
        lines(n, &mut text);
        codes::value_hash(n)
    } else {
        let n = resolve(root, field)?;
        lines(n, &mut text);
        codes::value_hash(n)
    };
    Some(Version { hash, text: text.join("\n") })
}

/// diff is a line diff of a and b: only the changed lines, "- " for the old
/// and "+ " for the new ones.
pub fn diff(a: &str, b: &str) -> String {
    let (x, y): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    // ponytail: quadratic LCS; targets are a block, a field or one document
    let (n, m) = (x.len(), y.len());
    let mut l = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if x[i] == y[j] { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, String::new());
    while i < n || j < m {
        if i < n && j < m && x[i] == y[j] {
            (i, j) = (i + 1, j + 1);
        } else if i < n && (j == m || l[i + 1][j] >= l[i][j + 1]) {
            let _ = writeln!(out, "- {}", x[i]);
            i += 1;
        } else {
            let _ = writeln!(out, "+ {}", y[j]);
            j += 1;
        }
    }
    out.trim_end().to_string()
}

/// indent puts the lines of s under the hint
/// (at most HINT_LINES lines: a whole document would flood the report).
fn indent(s: &str) -> String {
    const HINT_LINES: usize = 30;
    let n = s.lines().count();
    let mut out: String = s.lines().take(HINT_LINES).map(|l| format!("\n    {l}")).collect();
    if n > HINT_LINES {
        out.push_str(&format!("\n    … {} more lines", n - HINT_LINES));
    }
    out
}

impl Project {
    /// version is the version of a target written as in a link, without its
    /// hash: "id", "id#field" or "id#^code"; None when it does not resolve.
    pub fn version(&self, target: &str) -> Option<Version> {
        let (id, field) = target.split_once('#').unwrap_or((target, ""));
        version_in(self.doc(id)?.root()?, field)
    }

    /// old_text is the text the target had when its version was want: the
    /// newest version of the document file in the Git history with that hash.
    fn old_text(&self, target: &str, want: &str) -> Option<String> {
        if !matches!(self.cfg.source, crate::source::Source::Disk) {
            return None;
        }
        let (id, field) = target.split_once('#').unwrap_or((target, ""));
        let file = &self.doc(id)?.file;
        let git = |args: &[&str]| {
            Command::new("git").args(args).current_dir(&self.cfg.root).output().ok().filter(|o| o.status.success()).map(|o| o.stdout)
        };
        let log = git(&["log", "--format=%H", "-n", "200", "--", file])?;
        for sha in String::from_utf8_lossy(&log).lines() {
            let Some(b) = git(&["show", &format!("{sha}:./{file}")]) else {
                continue;
            };
            let r = loader::parse(&b);
            if let Some(root) = r.root.as_ref().filter(|_| r.err.is_none())
                && let Some(v) = version_in(root, field).filter(|v| v.hash == want)
            {
                return Some(v.text);
            }
        }
        None
    }

    /// stale_message is the message and hint of a link to target written
    /// with the hash had (None: no hash) when its version is now v.
    pub(super) fn stale_message(&self, written: &str, target: &str, had: Option<&str>, v: &Version) -> (String, String) {
        thread_local! {
            static OLD: RefCell<HashMap<(String, String), Option<String>>> = RefCell::new(HashMap::new());
        }
        let Some(had) = had else {
            return (
                format!("{written} has no version hash"),
                format!(
                    "Read {target}, check that this place agrees with it, then write the hash into the link: {target}@{}. It reads now:{}",
                    v.hash,
                    indent(&v.text)
                ),
            );
        };
        let old = OLD.with(|c| {
            c.borrow_mut()
                .entry((target.to_string(), had.to_string()))
                .or_insert_with(|| self.old_text(target, had))
                .clone()
        });
        let what = match old {
            Some(o) => format!("What changed since @{had}:{}", indent(&diff(&o, &v.text))),
            None => format!("Version @{had} is not in the Git history; it reads now:{}", indent(&v.text)),
        };
        (
            format!("{written}: the target changed, its hash is now @{}", v.hash),
            format!(
                "Read the change, check that this place still agrees with it (fix it if not), then write @{} in place of @{had} by hand. {what}",
                v.hash
            ),
        )
    }

    /// check_versions reports DOC209 on every link whose hash is missing or
    /// does not match its target now. Links that do not resolve are DOC202
    /// and DOC312.
    pub(super) fn check_versions(&self) {
        let mut cache: HashMap<String, Option<Version>> = HashMap::new();
        for d in &self.docs {
            if !d.parsed() {
                continue;
            }
            for r in &d.refs {
                if !r.ok {
                    continue;
                }
                let v = cache.entry(r.inner.clone()).or_insert_with(|| self.version(&r.inner));
                let Some(v) = v.as_ref() else {
                    continue;
                };
                if r.hash.as_deref() == Some(v.hash.as_str()) {
                    continue;
                }
                let written = go_quote(&String::from_utf8_lossy(&d.res().src.data[r.start..r.end]));
                let (msg, hint) = self.stale_message(&format!("link {written}"), &r.inner, r.hash.as_deref(), v);
                self.emit(d, "DOC209", r.start, r.end, msg, hint);
            }
        }
    }
}

/// doc_version is the hash of a whole document (docc compile).
pub fn doc_version(d: &Doc) -> Option<String> {
    d.root().map(codes::doc_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_lines() {
        assert_eq!(diff("a\nb\nc", "a\nx\nc"), "- b\n+ x");
        assert_eq!(diff("", "a"), "+ a");
    }

    #[test]
    fn versions() {
        let r = loader::parse(br#"{"title": "T", "body": ["One [[b#^m2pq@3a9c]]. ^k7f2", "Two. ^m3pq"], "f": {"$sections": true, "S": "x ^n4pq"}}"#);
        let root = r.root.unwrap();
        let v = version_in(&root, "^k7f2").unwrap();
        assert_eq!(v.text, "One [[b#^m2pq]].");
        let r2 = loader::parse(br#"{"title": "T", "body": ["One [[b#^m2pq@zzzz]]. ^k7f2", "Two. ^m3pq"], "notes": ["n"]}"#);
        let root2 = r2.root.unwrap();
        // the hashes of links, the codes and the notes are not part of a version
        assert_eq!(version_in(&root2, "^k7f2").unwrap().hash, v.hash);
        assert!(version_in(&root, "f/S").is_some() && version_in(&root, "^zzzz").is_none());
        assert_ne!(version_in(&root, "").unwrap().hash, version_in(&root2, "").unwrap().hash);
    }
}
