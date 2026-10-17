use std::cell::Cell;

use super::project::{Doc, Project};
use crate::config::go_quote;
use crate::diag::Diagnostic;
use crate::loader::{Kind, Node};
use crate::rules::{self, Rule};
use crate::schema;

/// Ignore is one valid docIgnore entry.
#[derive(Debug, Clone, Default)]
pub struct Ignore {
    /// None for a custom rule named by custom
    pub rule: Option<&'static Rule>,
    /// custom rule name, "" otherwise
    pub custom: String,
    /// DOC800: every custom rule
    pub all_cust: bool,
    pub reason: String,
    pub path: String,
    pub has_path: bool,
    /// byte range of the resolved path value, None when the path does not exist
    pub path_node: Option<(usize, usize)>,
    /// byte range of the entry
    pub node: (usize, usize),
    pub used: Cell<bool>,
}

/// rule_code_msg and rule_code_hint word DOC902 for docc.json and docIgnore.
fn rule_code_msg(key: &str, r: &Rule) -> String {
    if key == r.code {
        return format!("the rule is named by its code {}", r.code);
    }
    format!("the rule {} has a new name", go_quote(key))
}

fn rule_code_hint(r: &Rule) -> String {
    format!(
        "Write {} instead: the name says what the rule checks, and the old key still works.",
        go_quote(r.name)
    )
}

impl Project {
    /// rule_codes warns about the keys of the rules section of docc.json
    /// written as codes or old names (DOC902).
    pub(super) fn check_rule_codes(&self) {
        for (line, col, key) in &self.cfg.rule_codes {
            if let Some(r) = rules::alias(key) {
                self.emit_file(
                    &self.cfg.file,
                    "DOC902",
                    *line,
                    *col,
                    rule_code_msg(key, r),
                    rule_code_hint(r),
                );
            }
        }
    }

    /// parseIgnores validates docIgnore of every document (DOC108) and keeps the
    /// valid entries in Doc.ignores.
    pub(super) fn parse_ignores(&mut self) {
        let mut all = Vec::with_capacity(self.docs.len());
        for d in &self.docs {
            let mut out = Vec::new();
            if let Some(n) = d.root().and_then(|r| r.get("docIgnore")) {
                let bad = |at: &Node, msg: String, hint: &str| {
                    self.emit(d, "DOC108", at.start, at.end, msg, hint.to_string())
                };
                if n.kind != Kind::Array {
                    bad(
                        n,
                        "docIgnore must be an array of entries".into(),
                        r#"Write "docIgnore": [{ "rule": "text/forbidden-words", "reason": "why" }]."#,
                    );
                } else {
                    for it in &n.items {
                        if let Some(ig) = self.parse_ignore(d, it, &bad) {
                            out.push(ig);
                        }
                    }
                }
            }
            all.push(out);
        }
        for (d, igs) in self.docs.iter_mut().zip(all) {
            d.ignores = igs;
        }
    }

    fn parse_ignore(
        &self,
        d: &Doc,
        it: &Node,
        bad: &dyn Fn(&Node, String, &str),
    ) -> Option<Ignore> {
        if it.kind != Kind::Object {
            bad(
                it,
                "a docIgnore entry must be an object".into(),
                r#"Write { "rule": "text/forbidden-words", "reason": "why" }."#,
            );
            return None;
        }
        let mut ig = Ignore {
            node: (it.start, it.end),
            ..Default::default()
        };
        let (mut rule_node, mut reason_node) = (None, None);
        for m in &it.members {
            match m.key.as_str() {
                "rule" => rule_node = Some(&m.value),
                "reason" => reason_node = Some(&m.value),
                "path" => {
                    if m.value.kind != Kind::String
                        || (!m.value.str.is_empty() && !m.value.str.starts_with('/'))
                    {
                        bad(
                            &m.value,
                            "path must be a JSON pointer string".into(),
                            r#"Write a pointer such as "/statement" or "/body/1"."#,
                        );
                        return None;
                    }
                    (ig.path, ig.has_path) = (m.value.str.clone(), true);
                }
                _ => {
                    self.emit(
                        d,
                        "DOC108",
                        m.key_start,
                        m.key_end,
                        format!("unknown key {} in a docIgnore entry", go_quote(&m.key)),
                        "Use only rule, reason and path.".into(),
                    );
                    return None;
                }
            }
        }
        let Some(reason) =
            reason_node.filter(|n| n.kind == Kind::String && !n.str.trim().is_empty())
        else {
            bad(
                it,
                "a docIgnore entry needs a reason".into(),
                r#"Add "reason": explain why the rule does not apply here."#,
            );
            return None;
        };
        ig.reason = reason.str.clone();
        let Some(rule_node) = rule_node.filter(|n| n.kind == Kind::String) else {
            bad(
                it,
                "a docIgnore entry needs a rule".into(),
                r#"Add "rule": a rule name such as text/forbidden-words; see docc rules."#,
            );
            return None;
        };
        let key = rule_node.str.as_str();
        if rules::is_custom_name(key) {
            if !self.cfg.custom.iter().any(|c| c.id == key) {
                bad(
                    rule_node,
                    format!("unknown custom rule {}", go_quote(key)),
                    "Use the id of a rule from customRules, or DOC800 for all of them.",
                );
                return None;
            }
            ig.custom = key.to_string();
        } else if let Some(r) = rules::lookup(key) {
            if r.core {
                bad(
                    rule_node,
                    format!("{} is a core rule and cannot be suppressed", r.name),
                    "Fix the problem instead of suppressing it.",
                );
                return None;
            }
            if rules::alias(key).is_some() {
                self.emit(
                    d,
                    "DOC902",
                    rule_node.start,
                    rule_node.end,
                    rule_code_msg(key, r),
                    rule_code_hint(r),
                );
            }
            if r.code == "DOC800" {
                ig.all_cust = true;
            } else {
                ig.rule = Some(r);
            }
        } else {
            bad(
                rule_node,
                format!("unknown rule {}", go_quote(key)),
                "Use a rule name such as text/forbidden-words; see docc rules.",
            );
            return None;
        }
        if ig.has_path {
            ig.path_node = schema::resolve_pointer(d.r(), &ig.path).map(|n| (n.start, n.end));
        }
        Some(ig)
    }
}

impl Ignore {
    /// covers reports whether the entry suppresses the diagnostic.
    fn covers(&self, dg: &Diagnostic) -> bool {
        if dg.code == "DOC800" {
            if !self.all_cust && self.custom != dg.rule {
                return false;
            }
        } else if self.rule.is_none_or(|r| r.code != dg.code) {
            return false;
        }
        if !self.has_path {
            return true; // the whole document, also diagnostics without a position
        }
        self.path_node
            .is_some_and(|(s, e)| !dg.no_pos && dg.start >= s && dg.start < e)
    }
}

impl Project {
    /// suppress drops diagnostics covered by docIgnore and reports entries that
    /// cover nothing (DOC901).
    pub(super) fn suppress(&self) {
        self.diags.borrow_mut().retain(|dg| {
            let mut suppressed = false;
            if let Some(d) = self.doc_by_file(&dg.file)
                && !is_core(&dg.code)
            {
                for ig in &d.ignores {
                    if ig.covers(dg) {
                        ig.used.set(true);
                        suppressed = true;
                    }
                }
            }
            !suppressed
        });
        for d in &self.docs {
            for ig in &d.ignores {
                if ig.used.get() {
                    continue;
                }
                if ig.path_node.is_none() && ig.has_path {
                    self.emit(
                        d,
                        "DOC901",
                        ig.node.0,
                        ig.node.1,
                        format!(
                            "the path {} does not exist in this document",
                            go_quote(&ig.path)
                        ),
                        "Fix the path or remove the docIgnore entry.".into(),
                    );
                    continue;
                }
                self.emit(
                    d,
                    "DOC901",
                    ig.node.0,
                    ig.node.1,
                    "this docIgnore entry suppresses nothing".into(),
                    "Remove the entry: the rule is off here, or nothing triggers it.".into(),
                );
            }
        }
    }
}

fn is_core(code: &str) -> bool {
    rules::by_code(code).is_some_and(|r| r.core)
}
