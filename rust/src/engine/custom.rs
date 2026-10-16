use regex::Regex;

use super::project::{Doc, Project};
use crate::config::Custom;
use crate::diag::{Diagnostic, Level};
use crate::loader::{Kind, Node};
use crate::mdscan;
use crate::text;

/// pointerTokens splits a JSON pointer; "*" stays as the wildcard token.
fn pointer_tokens(ptr: &str) -> Vec<String> {
    ptr[1..]
        .split('/')
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// find resolves tokens against a node; "*" matches every array element and
/// every object value.
fn find<'a>(n: &'a Node, tokens: &[String], out: &mut Vec<&'a Node>) {
    let Some((t, rest)) = tokens.split_first() else {
        out.push(n);
        return;
    };
    match n.kind {
        Kind::Object => {
            if t == "*" {
                for m in &n.members {
                    find(&m.value, rest, out);
                }
            } else if let Some(v) = n.get(t) {
                find(v, rest, out);
            }
        }
        Kind::Array => {
            if t == "*" {
                for it in &n.items {
                    find(it, rest, out);
                }
            } else if let Some(i) = atoi(t)
                && i < n.items.len()
            {
                find(&n.items[i], rest, out);
            }
        }
        _ => {}
    }
}

/// atoi is a non-negative strconv.Atoi(t) whose result prints back as t.
fn atoi(t: &str) -> Option<usize> {
    let i: i64 = t.parse().ok()?;
    (i >= 0 && i.to_string() == t).then_some(i as usize)
}

/// textValues returns the strings found by a pointer: a string, or the
/// elements of an array of strings. None when a found value is neither.
fn text_values<'a>(hits: &[&'a Node]) -> Option<Vec<&'a Node>> {
    let mut strs = Vec::new();
    for h in hits {
        match h.kind {
            Kind::String => strs.push(*h),
            Kind::Array => {
                for it in &h.items {
                    if it.kind != Kind::String {
                        return None;
                    }
                    strs.push(it);
                }
            }
            _ => return None,
        }
    }
    Some(strs)
}

impl Project {
    /// checkCustom runs the customRules of docc.json (DOC800).
    pub(super) fn check_custom(&self) {
        for cr in &self.cfg.custom {
            let tokens = pointer_tokens(&cr.given);
            let lvl = self.cfg.resolve().custom(&cr.id).level;
            for d in &self.docs {
                let Some(root) = d.root() else {
                    continue;
                };
                if lvl == Level::Off {
                    continue;
                }
                if !cr.kind.is_empty() {
                    match root.get("kind") {
                        Some(k) if k.kind == Kind::String && k.str == cr.kind => {}
                        _ => continue,
                    }
                }
                self.run_custom(d, cr, &tokens, lvl);
            }
        }
    }

    fn run_custom(&self, d: &Doc, cr: &Custom, tokens: &[String], lvl: Level) {
        let mut hits = Vec::new();
        find(d.r(), tokens, &mut hits);
        let strs = match text_values(&hits) {
            Some(strs) if !strs.is_empty() => strs,
            _ => {
                // forbid skips the document; require treats a missing field as a violation
                if cr.require.is_some() && hits.is_empty() {
                    self.emit_no_pos_at(
                        d,
                        "DOC800",
                        &cr.id,
                        lvl,
                        cr.message.clone(),
                        cr.hint.clone(),
                    );
                }
                return;
            }
        };
        for n in strs {
            let sc = mdscan::new(&n.str);
            if let Some(forbid) = &cr.forbid {
                for sp in &sc.plain {
                    let (seg, back) = text::nfc_map(&n.str[sp.start..sp.end]);
                    for m in forbid.find_iter(&seg) {
                        if m.start() == m.end() {
                            continue;
                        }
                        let (s, e) = back(m.start(), m.end());
                        let (a, b) = d.range(n, sp.start + s, sp.start + e);
                        self.emit_at(
                            d,
                            "DOC800",
                            &cr.id,
                            lvl,
                            a,
                            b,
                            cr.message.clone(),
                            cr.hint.clone(),
                        );
                    }
                }
                continue;
            }
            if let Some(re) = &cr.require
                && !matches_any(re, &n.str, &sc)
            {
                self.emit_at(
                    d,
                    "DOC800",
                    &cr.id,
                    lvl,
                    n.start,
                    n.end,
                    cr.message.clone(),
                    cr.hint.clone(),
                );
            }
        }
    }

    /// emitNoPosAt adds a document diagnostic without a position and an explicit
    /// rule name and level.
    fn emit_no_pos_at(
        &self,
        d: &Doc,
        code: &str,
        name: &str,
        lvl: Level,
        msg: String,
        hint: String,
    ) {
        let r = crate::rules::by_code(code).expect("a registered rule");
        let mut dg = Diagnostic {
            code: code.into(),
            rule: r.name.into(),
            file: d.file.clone(),
            ..Default::default()
        };
        dg.rule = name.to_string();
        dg.doc_id = d.id.clone();
        (dg.message, dg.hint) = (msg, hint);
        (dg.line, dg.column, dg.end_line, dg.end_column) = (1, 1, 1, 1);
        dg.no_pos = true;
        dg.lvl = lvl;
        dg.level = lvl.to_string();
        self.diags.borrow_mut().push(dg);
    }
}

fn matches_any(re: &Regex, s: &str, sc: &mdscan::Scan) -> bool {
    sc.plain
        .iter()
        .any(|sp| re.is_match(&text::nfc(&s[sp.start..sp.end])))
}
