use std::cell::Cell;
use std::collections::HashMap;

use super::project::{Doc, Project, valid_id};
use super::strings::{StrNode, anchor_name};
use crate::loader::{Kind, Node};
use crate::mdscan;

/// RefUse is one [[id]] or [[id#field]] occurrence in prose.
#[derive(Debug, Clone, Default)]
pub struct RefUse {
    /// text between the brackets
    pub inner: String,
    /// document id (valid only when ok)
    pub target: String,
    /// anchor field or "^name" of a block anchor, "" for none
    pub field: String,
    /// the inner text has the form id, id#field or id#^name
    pub ok: bool,
    /// source offsets of the whole [[...]]
    pub start: usize,
    pub end: usize,
    pub notes: bool,
    /// pointer of the string that holds it
    pub ptr: String,
}

/// IncUse is one {{id#field}} occurrence in body or a free field.
#[derive(Debug, Clone, Default)]
pub struct IncUse {
    pub inner: String,
    pub target: String,
    pub field: String,
    /// well formed (id#field)
    pub ok: bool,
    pub start: usize,
    pub end: usize,
    /// field or section path that contains the include
    pub src_field: String,
    /// block anchor of the string that contains the include
    pub src_anchor: String,
    pub ptr: String,
    /// target and field exist and hold text
    pub valid: Cell<bool>,
}

impl StrNode {
    /// Scan returns the cached Markdown scan of the string. A block anchor marker
    /// is cut from the plain text: text rules do not see it.
    pub fn scan(&self) -> &mdscan::Scan {
        self.scan.get_or_init(|| {
            let mut sc = mdscan::new(&self.node().str);
            if !self.anchor.is_empty() {
                let at = self.anchor_at;
                sc.plain.retain_mut(|sp| {
                    sp.end = sp.end.min(at);
                    sp.start < at
                });
            }
            sc
        })
    }

    /// IsText reports whether text rules apply: not in notes.
    pub fn is_text(&self) -> bool {
        !self.notes
    }

    /// CanInclude reports whether {{...}} is expanded in this string.
    pub fn can_include(&self) -> bool {
        !self.notes && self.field != "title"
    }
}

impl Doc {
    /// Block returns the text of n without its block anchor marker and the
    /// anchor name ("" when n carries none).
    pub fn block<'a>(&'a self, n: &'a Node) -> (&'a str, &'a str) {
        if let Some(&i) = self.block_of.get(&(n as *const Node)) {
            let sn = &self.strings()[i];
            return (&n.str[..sn.anchor_at], &sn.anchor);
        }
        (&n.str, "")
    }
}

/// split_ref splits id#field: (id, field, has_field, ok).
pub(super) fn split_ref(inner: &str) -> (&str, &str, bool, bool) {
    let (mut id, mut field, mut has_field) = (inner, "", false);
    if let Some(i) = inner.find('#') {
        (id, field, has_field) = (&inner[..i], &inner[i + 1..], true);
        if field.is_empty() || field.contains('#') {
            return (id, field, has_field, false);
        }
        if let Some(name) = field.strip_prefix('^')
            && !anchor_name(name)
        {
            return (id, field, has_field, false);
        }
    }
    (id, field, has_field, valid_id(id))
}

impl Project {
    /// analyze extracts references and includes from every parsed document.
    pub(super) fn analyze(&mut self) {
        for d in &mut self.docs {
            if !d.parsed() {
                continue;
            }
            let (mut refs, mut incs) = (Vec::new(), Vec::new());
            let (mut blocks, mut block_of) = (HashMap::new(), HashMap::new());
            let strs = d.strings();
            for (i, sn) in strs.iter().enumerate() {
                let node = sn.node();
                if !sn.anchor.is_empty() {
                    block_of.insert(node as *const Node, i);
                    blocks.entry(sn.anchor.clone()).or_insert(i);
                }
                let sc = sn.scan();
                for r in sc.refs() {
                    let (id, field, _, ok) = split_ref(&r.inner);
                    let (a, b) = d.range(node, r.start, r.end);
                    refs.push(RefUse {
                        target: id.to_string(),
                        field: field.to_string(),
                        ok,
                        start: a,
                        end: b,
                        notes: sn.notes,
                        ptr: sn.ptr.clone(),
                        inner: r.inner,
                    });
                }
                if !sn.can_include() {
                    continue;
                }
                for r in sc.includes() {
                    let (id, field, has_field, ok) = split_ref(&r.inner);
                    let (a, b) = d.range(node, r.start, r.end);
                    incs.push(IncUse {
                        inner: r.inner.clone(),
                        target: id.to_string(),
                        field: field.to_string(),
                        ok: ok && has_field,
                        start: a,
                        end: b,
                        src_field: sn.path.clone(),
                        src_anchor: sn.anchor.clone(),
                        ptr: sn.ptr.clone(),
                        valid: Cell::new(false),
                    });
                }
            }
            (d.refs, d.incs, d.blocks, d.block_of) = (refs, incs, blocks, block_of);
        }
    }
}

/// StringList returns the string items of an array node (non-strings skipped).
pub fn string_list(n: Option<&Node>) -> Vec<&Node> {
    match n {
        Some(n) if n.kind == Kind::Array => n
            .items
            .iter()
            .filter(|it| it.kind == Kind::String)
            .collect(),
        _ => Vec::new(),
    }
}
