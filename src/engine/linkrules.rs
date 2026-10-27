use sha2::{Digest, Sha256};

use crate::loader::Node;

/// Fingerprint is the first 8 hex digits of SHA-256 of the canonical JSON of a
/// document without docIgnore, notes and the readers' comments.
pub fn fingerprint(root: &Node) -> String {
    let mut b = String::new();
    canonical(&mut b, root, FINGERPRINT_SKIP);
    let sum = Sha256::digest(b.as_bytes());
    hex::encode(sum)[..8].to_string()
}

pub(super) const FINGERPRINT_SKIP: &[&str] = &["docIgnore", "notes", "$comments"];

pub(super) fn canonical(b: &mut String, n: &Node, skip: &[&str]) {
    crate::codes::canonical(b, n, skip, false)
}

pub(super) use crate::codes::quote;
