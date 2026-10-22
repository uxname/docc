use std::collections::HashSet;
use std::fmt::Write;

use sha2::{Digest, Sha256};

use crate::loader::{Kind, Node};
use crate::text;

/// Fingerprint is the first 8 hex digits of SHA-256 of the canonical JSON of a
/// document without docIgnore, notes and the readers' comments.
pub fn fingerprint(root: &Node) -> String {
    let mut b = String::new();
    canonical(&mut b, root, FINGERPRINT_SKIP);
    let sum = Sha256::digest(b.as_bytes());
    hex::encode(sum)[..8].to_string()
}

pub(super) const FINGERPRINT_SKIP: &[&str] = &["docIgnore", "notes", "$comments"];

/// canonical writes n as JSON with sorted keys and NFC strings; skip names the
/// keys dropped at the top level only.
pub(super) fn canonical(b: &mut String, n: &Node, skip: &[&str]) {
    match n.kind {
        Kind::Null | Kind::Bool | Kind::Number => b.push_str(&n.str),
        Kind::String => quote(b, &n.str),
        Kind::Array => {
            b.push('[');
            for (i, it) in n.items.iter().enumerate() {
                if i > 0 {
                    b.push(',');
                }
                canonical(b, it, &[]);
            }
            b.push(']');
        }
        Kind::Object => {
            let mut items: Vec<(&str, &Node)> = Vec::new();
            let mut seen = HashSet::new();
            for m in &n.members {
                if !seen.insert(m.key.as_str()) {
                    continue;
                }
                if skip.contains(&m.key.as_str()) {
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
                canonical(b, v, &[]);
            }
            b.push('}');
        }
    }
}

/// quote writes a JSON string with minimal escaping: quote, backslash, and
/// control characters as \u00XX.
pub(super) fn quote(b: &mut String, s: &str) {
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
