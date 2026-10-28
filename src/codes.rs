//! Block codes and version hashes.
//!
//! Every text block of a document (a paragraph, a list item, the text value of
//! a field or section) ends with a permanent code, ` ^PAY-14`: the key of its
//! document (two to five capital Latin letters and digits, the reserved field "key"), a
//! dash and a number, given by docc to a block without one as the largest
//! number of the document plus one. Every link carries the version hash of
//! what it points to, `[[pay#^PAY-14@3a9c]]`, `[[pay@9b1e]]`,
//! `[[pay#field@77aa]]`: four signs of ALPHABET from SHA-256 of the normalized
//! target. Normalizing drops the block codes and the `@hash` inside links, so
//! a hash never depends on another hash and two documents that link each
//! other never loop.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};

use crate::loader::{Kind, Node};
use crate::text;

/// ALPHABET are the signs of hashes (and of the codes of the old format):
/// digits and lower case letters without 0, 1, i, l and o, which read alike.
pub const ALPHABET: &str = "23456789abcdefghjkmnpqrstuvwxyz";

/// LEN is the length of a hash.
pub const LEN: usize = 4;

const SIGN: &str = "[2-9a-hjkmnp-z]";

/// CODE is a block code as written in the sources: KEY-N.
const CODE: &str = "[A-Z][A-Z0-9]{1,4}-[1-9][0-9]{0,5}";

/// CODE_MARK is a block code at the very end of a string, after a blank.
pub static CODE_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"[\t\n\f\r ]\^({CODE})$")).unwrap());

/// NAMED_MARK is an anchor of the oldest format, ` ^name`: a marker at the
/// end of a string that a migration turns into a code. A code of the old
/// format matches it too when it starts with a letter.
pub static NAMED_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\t\n\f\r ]\^([a-z][a-z0-9-]*)$").unwrap());

/// ANY_MARK is a code, a code of the old format (four signs of ALPHABET) or
/// an old named anchor: what the output never shows.
pub static ANY_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[\t\n\f\r ]\^({CODE}|[a-z][a-z0-9-]*|{SIGN}{{4}})$")).unwrap());

/// LINK_HASH is the version hash at the end of a link: [[...@3a9c]].
static LINK_HASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?i)(\[\[[^\[\]\n]*?)@{SIGN}{{4}}\]\]")).unwrap());

static CODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!("^{CODE}$")).unwrap());

static KEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^[A-Z][A-Z0-9]{1,4}$").unwrap());

/// is_hash reports whether s is a version hash: four signs of ALPHABET, lower case.
pub fn is_hash(s: &str) -> bool {
    s.len() == LEN && s.bytes().all(|b| ALPHABET.as_bytes().contains(&b))
}

/// is_code reports whether s is a block code as written: KEY-N, upper case.
pub fn is_code(s: &str) -> bool {
    CODE_RE.is_match(s)
}

/// is_key reports whether s can be the key of a document: two to five capital
/// Latin letters and digits, a letter first.
pub fn is_key(s: &str) -> bool {
    KEY_RE.is_match(s)
}

/// key_of is the key part of a code: "PAY" of "PAY-14".
pub fn key_of(code: &str) -> &str {
    code.rsplit_once('-').map_or(code, |(k, _)| k)
}

/// split_hash cuts the version hash from the inside of a link or a code tag
/// target: "pay#^pay-14@3A9C" gives ("pay#^PAY-14", Some("3a9c")). A code
/// after "#^" is upper-cased, as codes are matched in any case.
pub fn split_hash(inner: &str) -> (String, Option<String>) {
    let (mut t, mut h) = (inner, None);
    if let Some(i) = inner.rfind('@') {
        let x = inner[i + 1..].to_ascii_lowercase();
        if is_hash(&x) {
            (t, h) = (&inner[..i], Some(x));
        }
    }
    let mut t = t.to_string();
    if let Some(i) = t.find("#^") {
        let c = t[i + 2..].to_ascii_uppercase();
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

/// Numbers gives new codes: per key, the largest number in use plus one. A
/// number freed by a deleted block may be given again.
#[derive(Default)]
pub struct Numbers(HashMap<String, u32>);

impl Numbers {
    /// note records a code in use.
    pub fn note(&mut self, code: &str) {
        if let Some((k, n)) = code.rsplit_once('-')
            && let Ok(n) = n.parse::<u32>()
        {
            let m = self.0.entry(k.to_string()).or_default();
            *m = (*m).max(n);
        }
    }

    /// next is a new code of key.
    pub fn next(&mut self, key: &str) -> String {
        let m = self.0.entry(key.to_string()).or_default();
        *m += 1;
        format!("{key}-{m}")
    }
}

/// BLOCKED are keys nobody wants to read aloud; a key that starts with one is
/// never proposed.
const BLOCKED: &[&str] = &["ASS", "CUM", "FAG", "FUC", "FUK", "KKK", "NIG", "SEX", "TIT", "WTF", "XXX"];

/// propose_key is a key for the document id that taken does not hold:
/// - several words: their initials (open-questions: OQ, run-1: R1), then the
///   initials with more of the last word (decisions-audit: DAU);
/// - one word with a number: its letters and the number (q52: Q52);
/// - else the first three letters (payments: PAY), the first letter and the
///   next two other consonants (assumptions: ASM), four and five letters;
/// - none free: the first of them with a letter added; no letters: DOC.
pub fn propose_key(id: &str, taken: &dyn Fn(&str) -> bool) -> String {
    // a key starts with a letter: the digits an id starts with are dropped
    let id = id.trim_start_matches(|c: char| !c.is_ascii_alphabetic());
    let parts: Vec<String> = id.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_ascii_uppercase).collect();
    let first = |w: &str, n: usize| -> String { w.chars().take(n).collect() };
    let letters: String = parts.concat().chars().filter(char::is_ascii_alphabetic).collect();
    let mut cands: Vec<String> = Vec::new();
    if parts.len() > 1 {
        let init: String = parts.iter().map(|p| if p.bytes().all(|b| b.is_ascii_digit()) { p.clone() } else { first(p, 1) }).collect();
        cands.push(init);
        let head: String = parts[..parts.len() - 1].iter().map(|p| first(p, 1)).collect();
        let last = parts.last().unwrap();
        if last.bytes().all(|b| b.is_ascii_alphabetic()) {
            cands.push(format!("{head}{}", first(last, 2)));
            cands.push(format!("{head}{}", first(last, 3)));
        }
    } else if let Some(p) = parts.first()
        && p.bytes().any(|b| b.is_ascii_digit())
    {
        let lead: String = p.chars().take_while(char::is_ascii_alphabetic).collect();
        cands.push(format!("{}{}", first(&lead, 3), &p[lead.len()..]));
    }
    cands.push(first(&letters, 3));
    let mut cons: Vec<char> = Vec::new();
    for c in letters.chars().skip(1).filter(|c| !"AEIOUY".contains(*c)) {
        if cons.len() < 2 && !cons.contains(&c) && Some(c) != letters.chars().next() {
            cons.push(c);
        }
    }
    cands.push(letters.chars().take(1).chain(cons).collect());
    cands.push(first(&letters, 4));
    cands.push(first(&letters, 5));
    let mut ok: Vec<String> = cands.into_iter().filter(|c| is_key(c) && !BLOCKED.iter().any(|b| c.starts_with(b))).collect();
    if ok.is_empty() {
        ok.push("DOC".into());
    }
    if let Some(k) = ok.iter().find(|k| !taken(k)) {
        return k.clone();
    }
    let base: String = ok.first().map_or("DOC", |k| k.as_str()).chars().take(4).collect();
    for c in 'A'..='Z' {
        let k = format!("{base}{c}");
        if !taken(&k) {
            return k;
        }
    }
    // ponytail: 26 keys sharing four signs is not a real project
    (b'A'..=b'Z').flat_map(|a| (b'A'..=b'Z').map(move |b| format!("{}{}", a as char, b as char))).find(|k| !taken(k)).unwrap_or_default()
}

/// code_of is the block code at the end of s, if any.
pub fn code_of(s: &str) -> Option<&str> {
    CODE_MARK.captures(s).map(|m| m.get(1).unwrap().as_str())
}

/// any_code_of is the code at the end of s in either format: what links of
/// a project not yet migrated still find their blocks by.
pub fn any_code_of(s: &str) -> Option<&str> {
    HASHED_MARK.captures(s).map(|m| m.get(1).unwrap().as_str())
}

/// canon is a code as written in the sources from a code in any case: upper
/// case for a code, lower case for a code of the old format.
pub fn canon(c: &str) -> String {
    let u = c.to_ascii_uppercase();
    if is_code(&u) { u } else { c.to_ascii_lowercase() }
}

/// HASHED_MARK is what a hash leaves out at the end of a block: a code, or
/// a code of the old format, so a migration changes no block's version.
static HASHED_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"[\t\n\f\r ]\^({CODE}|{SIGN}{{4}})$")).unwrap());

/// strip_code cuts a trailing block code (of either format) from s.
pub fn strip_code(s: &str) -> &str {
    match HASHED_MARK.find(s) {
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
/// what readers do not see, the order among siblings, the parent and the key.
pub const DOC_SKIP: &[&str] = &["notes", "docIgnore", "hidden", "order", "parent", "key", "$comments"];

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
        assert!(is_hash("k7f2") && !is_hash("K7F2") && !is_hash("k7f") && !is_hash("k0f2") && !is_hash("l7f2"));
        assert!(is_code("PAY-14") && is_code("OQ-1") && !is_code("pay-14") && !is_code("P-1") && !is_code("PAY-0") && !is_code("PAY-014") && !is_code("PAYMEN-1") && !is_code("k7f2"));
        assert!(is_key("PAY") && is_key("PAY1") && !is_key("P") && !is_key("pay") && !is_key("1PAY") && !is_key("ABCDEF"));
        assert_eq!(key_of("PAY-14"), "PAY");
        assert_eq!(split_hash("pay#^pay-14@3A9C"), ("pay#^PAY-14".into(), Some("3a9c".into())));
        assert_eq!(split_hash("i4@3a9c"), ("i4".into(), Some("3a9c".into())));
        assert_eq!(split_hash("i4#a@b"), ("i4#a@b".into(), None));
        assert_eq!(split_hash("i4#^k7f2"), ("i4#^k7f2".into(), None));
        let h = hash4("x");
        assert!(is_hash(&h) && h == hash4("x") && h != hash4("y"));
        // a block's own code (of either format) and the hashes of its links are not part of its version
        assert_eq!(normalize("See [[a#^A-2@3a9c]]. ^PAY-14"), "See [[a#^A-2]].");
        assert_eq!(normalize("Old. ^m2pq"), "Old.");
        assert_eq!(normalize("See [[a@3a9c]] ^x"), "See [[a]] ^x");
        assert_eq!(with_code("a", "PAY-1"), "a ^PAY-1");
        assert_eq!(with_code("```\nx\n```", "PAY-1"), "```\nx\n```\n^PAY-1");
        let mut n = Numbers::default();
        n.note("PAY-7");
        n.note("PAY-3");
        n.note("OQ-2");
        assert_eq!((n.next("PAY"), n.next("PAY"), n.next("OQ"), n.next("NEW")), ("PAY-8".into(), "PAY-9".into(), "OQ-3".into(), "NEW-1".into()));
        assert_eq!(code_of("text ^PAY-14"), Some("PAY-14"));
        assert_eq!(code_of("text ^k7f2"), None);
        assert!(ANY_MARK.is_match("text ^k7f2") && ANY_MARK.is_match("text ^2wu5") && ANY_MARK.is_match("text ^PAY-1"));
    }

    #[test]
    fn keys() {
        let none = |_: &str| false;
        assert_eq!(propose_key("payments", &none), "PAY");
        assert_eq!(propose_key("open-questions", &none), "OQ");
        assert_eq!(propose_key("data_model_entities", &none), "DME");
        assert_eq!(propose_key("run-1", &none), "R1");
        assert_eq!(propose_key("q52", &none), "Q52");
        assert_eq!(propose_key("i4", &none), "I4");
        assert_eq!(propose_key("t1", &none), "T1");
        assert_eq!(propose_key("2fa", &none), "FA");
        assert_eq!(propose_key("план", &none), "DOC");
        assert_eq!(propose_key("assumptions", &none), "ASM");
        assert_eq!(propose_key("payments", &|k| k == "PAY"), "PMN");
        assert_eq!(propose_key("decisions-audit", &|k| k == "DA"), "DAU");
        assert_eq!(propose_key("vision", &|k| k == "VIS"), "VSN");
        assert_eq!(propose_key("pay", &|k| k == "PAY"), "PAYA");
        assert_eq!(propose_key("x", &|k| k == "DOC"), "DOCA");
        assert!(is_key("Q52") && !is_key("5Q") && is_code("Q52-3"));
    }
}
