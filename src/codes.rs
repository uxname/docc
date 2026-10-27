//! Block codes and version hashes.
//!
//! Every text block of a document (a paragraph, a list item, the text value of
//! a field or section) ends with a permanent code, ` ^k7f2`: four signs of
//! ALPHABET, unique in the project, given by docc to a block without one.
//! Every link carries the version hash of what it points to, `[[i4#^k7f2@3a9c]]`,
//! `[[i4@9b1e]]`, `[[i4#field@77aa]]`: four signs of the same alphabet from
//! SHA-256 of the normalized target. Normalizing drops the block codes and the
//! `@hash` inside links, so a hash never depends on another hash and two
//! documents that link each other never loop.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::loader::{Kind, Node};
use crate::text;

/// ALPHABET are the signs of codes and hashes: digits and lower case letters
/// without 0, 1, i, l and o, which read alike.
pub const ALPHABET: &str = "23456789abcdefghjkmnpqrstuvwxyz";

/// LEN is the length of a code and of a hash.
pub const LEN: usize = 4;

const SIGN: &str = "[2-9a-hjkmnp-z]";

/// CODE_MARK is a block code at the very end of a string, after a blank.
pub static CODE_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[\t\n\f\r ]\^({SIGN}{{4}})$")).unwrap());

/// NAMED_MARK is an anchor of the old format, ` ^name`: a marker at the end of
/// a string that a migration turns into a code. A code matches it too when it
/// starts with a letter.
pub static NAMED_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\t\n\f\r ]\^([a-z][a-z0-9-]*)$").unwrap());

/// ANY_MARK is a code or an old named anchor: what the output never shows.
pub static ANY_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[\t\n\f\r ]\^([a-z][a-z0-9-]*|{SIGN}{{4}})$")).unwrap());

/// LINK_HASH is the version hash at the end of a link: [[...@3a9c]].
static LINK_HASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)(\[\[[^\[\]\n]*?)@{SIGN}{{4}}\]\]")).unwrap());

/// is_code reports whether s is a code (or a hash): four signs of ALPHABET, lower case.
pub fn is_code(s: &str) -> bool {
    s.len() == LEN && s.bytes().all(|b| ALPHABET.as_bytes().contains(&b))
}

/// split_hash cuts the version hash from the inside of a link or a code tag
/// target: "i4#^K7F2@3A9C" gives ("i4#^k7f2", Some("3a9c")). A code after
/// "#^" is lower-cased, as codes are matched in any case.
pub fn split_hash(inner: &str) -> (String, Option<String>) {
    let (mut t, mut h) = (inner, None);
    if let Some(i) = inner.rfind('@') {
        let x = inner[i + 1..].to_ascii_lowercase();
        if is_code(&x) {
            (t, h) = (&inner[..i], Some(x));
        }
    }
    let mut t = t.to_string();
    if let Some(i) = t.find("#^") {
        let c = t[i + 2..].to_ascii_lowercase();
        if is_code(&c) {
            t.replace_range(i + 2.., &c);
        }
    }
    (t, h)
}

/// hash4 is the four-sign hash of s: SHA-256, its first eight bytes in ALPHABET.
pub fn hash4(s: &str) -> String {
    hash4_bytes(s.as_bytes())
}

/// hash4_bytes is hash4 of bytes that need not be UTF-8 (a line of code).
pub fn hash4_bytes(s: &[u8]) -> String {
    let d = Sha256::digest(s);
    let mut n = u64::from_be_bytes(d[..8].try_into().unwrap());
    let a = ALPHABET.as_bytes();
    let mut out = String::with_capacity(LEN);
    for _ in 0..LEN {
        out.push(a[(n % a.len() as u64) as usize] as char);
        n /= a.len() as u64;
    }
    out
}

/// FIXED makes new codes a function of their place instead of random, for
/// tests whose output shows codes (DOCC_CODE_SEED in the environment does
/// the same for the binary).
pub static FIXED: AtomicBool = AtomicBool::new(false);

/// new_code is a random code that used does not hold; it is added there.
/// salt names the block (its file and text): with FIXED or DOCC_CODE_SEED
/// the code depends on it alone, so a test that writes a document again gets
/// the same codes for the same paragraphs.
pub fn new_code(used: &mut HashSet<String>, salt: &str) -> String {
    let seed = std::env::var("DOCC_CODE_SEED").ok().or_else(|| FIXED.load(Ordering::Relaxed).then(String::new));
    let mut k = 0usize;
    loop {
        let c = match &seed {
            Some(seed) => hash4(&format!("{seed}\0{salt}\0{k}")),
            None => {
                // RandomState is seeded from the system for every new value
                let mut h = std::collections::hash_map::RandomState::new().build_hasher();
                h.write_usize(used.len() + k);
                let mut n = h.finish();
                let a = ALPHABET.as_bytes();
                let mut c = String::with_capacity(LEN);
                for _ in 0..LEN {
                    c.push(a[(n % a.len() as u64) as usize] as char);
                    n /= a.len() as u64;
                }
                c
            }
        };
        k += 1;
        if used.insert(c.clone()) {
            return c;
        }
    }
}

/// code_of is the block code at the end of s, if any.
pub fn code_of(s: &str) -> Option<&str> {
    CODE_MARK.captures(s).map(|m| m.get(1).unwrap().as_str())
}

/// strip_code cuts a trailing block code from s.
pub fn strip_code(s: &str) -> &str {
    match CODE_MARK.find(s) {
        Some(m) => &s[..m.start()],
        None => s,
    }
}

/// strip_link_hashes drops the version hashes of the links in s.
pub fn strip_link_hashes(s: &str) -> std::borrow::Cow<'_, str> {
    LINK_HASH.replace_all(s, "$1]]")
}

/// normalize is the text a hash covers: no trailing code, no hashes in links.
pub fn normalize(s: &str) -> String {
    strip_link_hashes(strip_code(s)).into_owned()
}

/// with_code is s with the code appended: after a line break when the last
/// line closes a code fence (a blank would keep the fence open), else after a blank.
pub fn with_code(s: &str, code: &str) -> String {
    let last = s.rsplit('\n').next().unwrap_or("").trim_start();
    let sep = if last.starts_with("```") || last.starts_with("~~~") { '\n' } else { ' ' };
    format!("{s}{sep}^{code}")
}

/// canonical writes n as JSON with sorted keys and NFC strings; skip names the
/// keys dropped at the top level only. With norm, strings are normalized.
pub fn canonical(b: &mut String, n: &Node, skip: &[&str], norm: bool) {
    match n.kind {
        Kind::Null | Kind::Bool | Kind::Number => b.push_str(&n.str),
        Kind::String if norm => quote(b, &normalize(&n.str)),
        Kind::String => quote(b, &n.str),
        Kind::Array => {
            b.push('[');
            for (i, it) in n.items.iter().enumerate() {
                if i > 0 {
                    b.push(',');
                }
                canonical(b, it, &[], norm);
            }
            b.push(']');
        }
        Kind::Object => {
            let mut items: Vec<(&str, &Node)> = Vec::new();
            let mut seen = HashSet::new();
            for m in &n.members {
                if !seen.insert(m.key.as_str()) || skip.contains(&m.key.as_str()) {
                    continue;
                }
                items.push((&m.key, &m.value));
            }
            items.sort_unstable_by(|x, y| x.0.cmp(y.0));
            b.push('{');
            for (i, (k, v)) in items.into_iter().enumerate() {
                if i > 0 {
                    b.push(',');
                }
                quote(b, k);
                b.push(':');
                canonical(b, v, &[], norm);
            }
            b.push('}');
        }
    }
}

/// quote writes a JSON string with minimal escaping: quote, backslash, and
/// control characters as \u00XX.
pub fn quote(b: &mut String, s: &str) {
    b.push('"');
    for r in text::nfc(s).chars() {
        match r {
            '"' => b.push_str("\\\""),
            '\\' => b.push_str("\\\\"),
            r if (r as u32) < 0x20 => {
                let _ = write!(b, "\\u{:04x}", r as u32);
            }
            r => b.push(r),
        }
    }
    b.push('"');
}

/// DOC_SKIP are the top-level keys the hash of a whole document leaves out:
/// what readers do not see, the order among siblings and the parent.
pub const DOC_SKIP: &[&str] = &["notes", "docIgnore", "hidden", "order", "parent", "$comments"];

/// value_hash is the version hash of a value: its normalized canonical JSON.
pub fn value_hash(n: &Node) -> String {
    let mut b = String::new();
    canonical(&mut b, n, &[], true);
    hash4(&b)
}

/// doc_hash is the version hash of a whole document.
pub fn doc_hash(root: &Node) -> String {
    let mut b = String::new();
    canonical(&mut b, root, DOC_SKIP, true);
    hash4(&b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_codes() {
        assert!(is_code("k7f2") && !is_code("K7F2") && !is_code("k7f") && !is_code("k0f2") && !is_code("l7f2"));
        assert_eq!(split_hash("i4#^K7F2@3A9C"), ("i4#^k7f2".into(), Some("3a9c".into())));
        assert_eq!(split_hash("i4@3a9c"), ("i4".into(), Some("3a9c".into())));
        assert_eq!(split_hash("i4#a@b"), ("i4#a@b".into(), None));
        assert_eq!(split_hash("i4#^i4-3"), ("i4#^i4-3".into(), None));
        let h = hash4("x");
        assert!(is_code(&h) && h == hash4("x") && h != hash4("y"));
        // a block's own code and the hashes of its links are not part of its version
        assert_eq!(normalize("See [[a#^k7f2@3a9c]]. ^m2pq"), "See [[a#^k7f2]].");
        assert_eq!(normalize("See [[a@3a9c]] ^x"), "See [[a]] ^x");
        assert_eq!(with_code("a", "k7f2"), "a ^k7f2");
        assert_eq!(with_code("```\nx\n```", "k7f2"), "```\nx\n```\n^k7f2");
        let mut used = HashSet::new();
        for i in 0..200 {
            assert!(is_code(&new_code(&mut used, &i.to_string())));
        }
        assert_eq!(used.len(), 200);
        assert_eq!(code_of("text ^k7f2"), Some("k7f2"));
        assert_eq!(code_of("text ^i4-3"), None);
    }
}
