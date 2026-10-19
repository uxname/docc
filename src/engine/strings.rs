use std::cell::OnceCell;
use std::ptr::NonNull;
use std::sync::LazyLock;

use regex::Regex;

use super::project::Doc;
use crate::loader::{Kind, Node};
use crate::mdscan;
use crate::text;

/// reservedKeys are top-level keys with a meaning of their own; all other keys
/// are free fields that hold prose.
pub(super) fn reserved_key(k: &str) -> bool {
    matches!(
        k,
        "title"
            | "kind"
            | "status"
            | "parent"
            | "body"
            | "tags"
            | "notes"
            | "hidden"
            | "docIgnore"
            | "order"
    )
}

/// reservedKeys sorted, for typo suggestions.
pub(super) const RESERVED_KEYS: [&str; 10] = [
    "body",
    "docIgnore",
    "hidden",
    "kind",
    "notes",
    "order",
    "parent",
    "status",
    "tags",
    "title",
];

/// ServiceFields cannot be anchors or include sources.
pub const SERVICE_FIELDS: [&str; 3] = ["notes", "docIgnore", "hidden"];

pub(super) fn service_field(k: &str) -> bool {
    SERVICE_FIELDS.contains(&k)
}

/// StrNode is one string value of a document with its location.
#[derive(Debug)]
pub struct StrNode {
    // points into the tree of the document that owns this StrNode; the tree
    // is never changed after loading and its string nodes live in heap
    // buffers that do not move with the Doc
    node: NonNull<Node>,
    /// JSON pointer of the value
    pub ptr: String,
    /// top-level key
    pub field: String,
    /// field plus the section keys down to the value, joined by "/"
    pub path: String,
    /// inside notes: hidden from readers, text rules do not apply
    pub notes: bool,
    /// Anchor is the block anchor name of a trailing " ^name", "" for none;
    /// the marker starts at anchor_at.
    pub anchor: String,
    pub anchor_at: usize,
    pub(super) scan: OnceCell<mdscan::Scan>,
}

// SAFETY: the node is only read, like the rest of the StrNode.
unsafe impl Send for StrNode {}

impl StrNode {
    /// Node is the string value.
    pub fn node(&self) -> &Node {
        // SAFETY: see the field: the node outlives every borrow of the
        // StrNode, which lives in the same Doc as the tree.
        unsafe { self.node.as_ref() }
    }
}

/// blockAnchor is a block anchor marker at the very end of a string. The name
/// starts with a letter, so "x ^2" and footnotes `[^1]` are not anchors.
static BLOCK_ANCHOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\t\n\f\r ]\^([a-z][a-z0-9-]*)$").unwrap());

/// AnchorName reports whether s is a valid block anchor name.
pub fn anchor_name(s: &str) -> bool {
    BLOCK_ANCHOR.is_match(&format!(" ^{s}"))
}

/// is_letter is unicode.IsLetter; is_digit is unicode.IsDigit.
pub(super) fn is_letter(r: char) -> bool {
    static L: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{L}$").unwrap());
    if r.is_ascii() {
        return r.is_ascii_alphabetic();
    }
    L.is_match(r.encode_utf8(&mut [0; 4]))
}

fn is_digit(r: char) -> bool {
    static ND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Nd}$").unwrap());
    if r.is_ascii() {
        return r.is_ascii_digit();
    }
    ND.is_match(r.encode_utf8(&mut [0; 4]))
}

/// Anchor builds a heading anchor: lower case, spaces to '-', punctuation
/// dropped, Unicode letters kept.
pub fn anchor(h: &str) -> String {
    let mut b = String::new();
    for r in text::to_lower(h).chars() {
        if r == ' ' {
            b.push('-');
        } else if is_letter(r) || is_digit(r) || r == '-' || r == '_' {
            b.push(r);
        }
    }
    b
}

/// HeadingAnchor is the anchor of the heading of a field or section key.
pub fn heading_anchor(key: &str) -> String {
    anchor(&key.replace(['_', '-'], " "))
}

/// SplitAnchor cuts a trailing block anchor marker from s.
pub fn split_anchor(s: &str) -> (&str, &str) {
    match BLOCK_ANCHOR.captures(s) {
        Some(m) => (&s[..m.get(0).unwrap().start()], m.get(1).unwrap().as_str()),
        None => (s, ""),
    }
}

fn esc_tok(s: &str) -> String {
    if !s.contains(['~', '/']) {
        return s.to_string();
    }
    s.replace('~', "~0").replace('/', "~1")
}

/// SectionsKey marks an object whose other keys are section titles.
pub const SECTIONS_KEY: &str = "$sections";

/// IsSections reports whether the object is a set of sections: it has the
/// member "$sections": true. It counts only where a section may stand (a free
/// field or the value of a section); elsewhere it is an ordinary list object.
pub fn is_sections(n: &Node) -> bool {
    n.get(SECTIONS_KEY)
        .is_some_and(|m| m.kind == Kind::Bool && m.str == "true")
}

/// InputKey marks an object that the HTML file draws as a form field: a
/// text box, radio buttons or check boxes. It stands as an item of an array
/// of paragraphs; the Markdown output leaves it out.
pub const INPUT_KEY: &str = "$input";

/// IsInput reports whether the object is a form field: it has a member "$input".
pub fn is_input(n: &Node) -> bool {
    n.kind == Kind::Object && n.get(INPUT_KEY).is_some()
}

/// INPUT_PROSE are the members of a form field read as prose; its kind and id are not.
pub const INPUT_PROSE: [&str; 2] = ["label", "options"];

/// Resolve finds a free field or a section by path: "field" or "field/section/...".
/// A key may itself contain "/", so every split is tried. Only sections are entered.
pub fn resolve<'a>(root: &'a Node, path: &str) -> Option<&'a Node> {
    resolve_in(root, path, true)
}

fn resolve_in<'a>(n: &'a Node, path: &str, top: bool) -> Option<&'a Node> {
    if n.kind != Kind::Object || (!top && !is_sections(n)) {
        return None;
    }
    for m in &n.members {
        if m.key == SECTIONS_KEY {
            continue;
        }
        if m.key == path {
            return Some(&m.value);
        }
        if let Some(rest) = path
            .strip_prefix(m.key.as_str())
            .and_then(|r| r.strip_prefix('/'))
            && !(top && service_field(&m.key))
            && let Some(v) = resolve_in(&m.value, rest, false)
        {
            return Some(v);
        }
    }
    None
}

impl Doc {
    /// Strings returns every prose string of the document: title, body, free
    /// fields at any depth, and notes (marked). Keys of objects are not included.
    pub fn strings(&self) -> &[StrNode] {
        let Some(root) = self.root() else {
            return &[];
        };
        self.strs.get_or_init(|| {
            let mut out = Vec::new();
            for m in &root.members {
                let k = m.key.as_str();
                if k == "title" || k == "body" || !reserved_key(k) {
                    walk(
                        &m.value,
                        &format!("/{}", esc_tok(k)),
                        k,
                        k,
                        !reserved_key(k),
                        false,
                        k != "title",
                        &mut out,
                    );
                } else if k == "notes" {
                    walk(&m.value, "/notes", k, k, false, true, false, &mut out);
                }
            }
            out
        })
    }

    /// Range maps a byte range [a,b) of a decoded string value to source offsets.
    pub fn range(&self, n: &Node, a: usize, b: usize) -> (usize, usize) {
        let src = &self.res().src;
        (src.str_offset(n, a), src.str_offset(n, b))
    }
}

/// walk collects the strings below n; sec is true where n may be a set of
/// sections, blk where a string may carry a block anchor: the value of a field
/// or section and an array item, not a member of a list object or table row.
#[allow(clippy::too_many_arguments)]
fn walk(
    n: &Node,
    ptr: &str,
    field: &str,
    path: &str,
    sec: bool,
    notes: bool,
    blk: bool,
    out: &mut Vec<StrNode>,
) {
    match n.kind {
        Kind::String => {
            let mut sn = StrNode {
                node: NonNull::from(n),
                ptr: ptr.to_string(),
                field: field.to_string(),
                path: path.to_string(),
                notes,
                anchor: String::new(),
                anchor_at: 0,
                scan: OnceCell::new(),
            };
            if blk && let Some(m) = BLOCK_ANCHOR.captures(&n.str) {
                sn.anchor = m.get(1).unwrap().as_str().to_string();
                sn.anchor_at = m.get(0).unwrap().start();
            }
            out.push(sn);
        }
        Kind::Array => {
            for (i, it) in n.items.iter().enumerate() {
                walk(
                    it,
                    &format!("{ptr}/{i}"),
                    field,
                    path,
                    false,
                    notes,
                    blk,
                    out,
                );
            }
        }
        Kind::Object => {
            let is_sec = sec && is_sections(n);
            let input = is_input(n);
            for m in &n.members {
                if input && !INPUT_PROSE.contains(&m.key.as_str()) {
                    continue;
                }
                let p = if is_sec {
                    format!("{path}/{}", m.key)
                } else {
                    path.to_string()
                };
                walk(
                    &m.value,
                    &format!("{ptr}/{}", esc_tok(&m.key)),
                    field,
                    &p,
                    is_sec,
                    notes,
                    blk && is_sec,
                    out,
                );
            }
        }
        _ => {}
    }
}
