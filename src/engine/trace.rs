use std::collections::HashMap;

use super::analyze::split_ref;
use super::mdrules::fragment_problem;
use super::project::{Doc, Project};
use crate::config::go_quote;
use crate::loader::Kind;
use crate::trace::{self, Key, Tag};

impl Project {
    /// checkTrace reads the docc: tags in code: DOC401 for a malformed tag or
    /// region and for a target that does not resolve, DOC403 for a document of a
    /// covered kind that no tag points to. It runs only when trace.scan is set.
    pub(super) fn check_trace(&mut self) {
        if self.cfg.trace_scan.is_empty() {
            return;
        }
        let skip = [
            self.cfg.abs(&self.cfg.src),
            self.cfg.abs(&self.cfg.out),
            self.cfg.abs(&self.cfg.out_full),
        ];
        let Ok(tags) = trace::scan(&self.cfg.root, &self.cfg.trace_scan, &skip, &self.cfg.source) else {
            return;
        };
        let mut covers: HashMap<String, Vec<String>> = HashMap::new();
        let (mut code, mut code_at) = (HashMap::new(), HashMap::new());
        let bad = |t: &Tag, msg: String, hint: String| {
            self.emit_file(
                &t.file,
                "DOC401",
                t.line,
                t.col,
                format!("bad code tag: {msg}"),
                hint,
            );
        };
        for t in &tags {
            if !t.err.is_empty() {
                bad(
                    t,
                    t.err.clone(),
                    "Close every docc:begin with docc:end; regions do not nest.".into(),
                );
                continue;
            }
            if t.targets.is_empty() {
                bad(
                    t,
                    "no targets after docc:".into(),
                    "Write // docc: id, id#field or id#^anchor.".into(),
                );
            }
            for g in &t.targets {
                let (id, field, has_field, ok) = split_ref(g);
                if !ok {
                    bad(
                        t,
                        format!("{} is not a target", go_quote(g)),
                        "Write id, id#field/section or id#^anchor, and separate targets with commas.".into(),
                    );
                    continue;
                }
                let Some(d) = self.doc(id) else {
                    bad(
                        t,
                        format!("{} points to a document that does not exist", go_quote(g)),
                        format!(
                            "Create {}/{}.json or fix the id in the tag.",
                            self.cfg.src, id
                        ),
                    );
                    continue;
                };
                if has_field && d.parsed() {
                    let (msg, hint) = fragment_problem(d, id, field);
                    if !msg.is_empty() {
                        bad(t, msg, hint);
                        continue;
                    }
                }
                let k = Key {
                    file: t.file.clone(),
                    target: g.clone(),
                };
                code_at.entry(k.clone()).or_insert((t.line, t.col));
                code.insert(k, String::new());
                let files = covers.entry(id.to_string()).or_default();
                if !files.contains(&t.file) {
                    files.push(t.file.clone());
                }
            }
        }
        for (k, h) in trace::hashes(&tags) {
            if let Some(v) = code.get_mut(&k) {
                *v = h;
            }
        }
        for files in covers.values_mut() {
            files.sort();
        }
        (self.covers, self.code, self.code_at) = (Some(covers), code, code_at);
        for d in &self.docs {
            if self.needs_code(d) {
                let k = d.r().get("kind").expect("needs_code checked the kind");
                self.emit(
                    d,
                    "DOC403",
                    k.start,
                    k.end,
                    format!(
                        "no code tag points to {}, and kind {} expects one",
                        go_quote(&d.id),
                        go_quote(&k.str)
                    ),
                    format!("Add // docc: {} to the code that implements it.", d.id),
                );
            }
        }
    }

    /// NeedsCode reports whether d is a visible document whose kind schema sets
    /// "x-docc-covered" and no code tag points to it; false without trace.scan.
    pub fn needs_code(&self, d: &Doc) -> bool {
        let Some(covers) = &self.covers else {
            return false;
        };
        if !d.parsed()
            || !self.in_graph(d)
            || d.hidden
            || covers.get(&d.id).is_some_and(|f| !f.is_empty())
        {
            return false;
        }
        let Some(k) = d.r().get("kind").filter(|k| k.kind == Kind::String) else {
            return false;
        };
        self.schemas.kind(&k.str).is_some_and(|kd| kd.covered)
    }
}
