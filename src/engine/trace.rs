use std::collections::{HashMap, HashSet};

use super::analyze::split_ref;
use super::mdrules::fragment_problem;
use super::project::{Doc, Project};
use super::strings::StrNode;
use crate::config::go_quote;
use crate::loader::Kind;
use crate::trace::{self, Key, Tag};

impl Project {
    /// checkTrace reads the docc: tags in code: DOC401 for a malformed tag or
    /// region and for a target that does not resolve, DOC403 for a document of a
    /// covered kind that no tag points to, DOC404 for a block of one that no
    /// tag names. It runs only when trace.scan is set.
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
        // the blocks a tag names as id#^code, the id as resolved (DOC404)
        let mut tagged: HashSet<String> = HashSet::new();
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
                    "Write // docc: id@hash, id#field@hash or id#^code@hash, and ~fingerprint at the end.".into(),
                );
            }
            let mut texts = Vec::new();
            for (gi, g) in t.targets.iter().enumerate() {
                let (id, field, has_field, ok) = split_ref(g, self.legacy);
                if !ok {
                    bad(
                        t,
                        format!("{} is not a target", go_quote(g)),
                        "Write id, id#field/section or id#^code, each with its @hash, and separate targets with commas.".into(),
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
                if let Some(code) = field.strip_prefix('^') {
                    tagged.insert(format!("{}#^{code}", d.id));
                }
                if !self.legacy
                    && let Some(v) = self.version(g)
                {
                    let had = t.hashes.get(gi).cloned().flatten();
                    if had.as_deref() != Some(v.hash.as_str()) {
                        let (msg, hint) = self.stale_message(&format!("code tag target {}", go_quote(g)), g, had.as_deref(), &v);
                        self.emit_file(&t.file, "DOC209", t.line, t.col, msg, hint);
                    }
                    texts.push(format!("{g}: {}", v.text.replace('\n', " / ")));
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
            if !self.legacy && !t.targets.is_empty() && t.print.as_deref() != Some(t.want.as_str()) {
                let what = if t.region { "the region of this docc:begin" } else { "the code of this file outside its regions" };
                let msg = match &t.print {
                    None => format!("the code tag has no fingerprint; {what} is ~{}", t.want),
                    Some(p) => format!("{what} changed: its fingerprint is ~{} now, not ~{p}", t.want),
                };
                let mut hint = format!(
                    "Check that the code agrees with what it points to (fix the code or the documents if not), then write ~{} at the end of the tag line by hand. The targets read now:",
                    t.want
                );
                for x in &texts {
                    hint.push_str("\n    ");
                    hint.push_str(x);
                }
                self.emit_file(&t.file, "DOC402", t.line, t.col, msg, hint);
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
        self.check_block_coverage(&tagged);
    }

    /// check_block_coverage is DOC404: every block that the kind schema wants
    /// covered (all blocks of a kind marked "x-docc-covered", or those of its
    /// marked fields) needs a tag that names it, id#^code. A tag on the whole
    /// document or on a field is not one: it does not say which code
    /// implements the block.
    fn check_block_coverage(&self, tagged: &HashSet<String>) {
        if self.off("DOC404") {
            return;
        }
        for d in &self.docs {
            let Some((kind, blocks)) = self.covered_blocks(d) else {
                continue;
            };
            for sn in blocks {
                let target = format!("{}#^{}", d.id, sn.anchor);
                if tagged.contains(&target) {
                    continue;
                }
                let n = sn.node();
                let (start, end) = d.range(n, sn.anchor_at, n.str.len());
                let hash = self.version(&target).map(|v| format!("@{}", v.hash)).unwrap_or_default();
                self.emit(
                    d,
                    "DOC404",
                    start,
                    end,
                    format!(
                        "no code tag points to block {}, and kind {} expects one on {}",
                        go_quote(&target),
                        go_quote(kind),
                        self.which_blocks(kind),
                    ),
                    format!("Add // docc: {target}{hash} to the code that implements this block."),
                );
            }
        }
    }

    /// covered_blocks are the blocks of d that DOC404 wants a tag on, with
    /// the kind: none for a hidden document, one outside the graph or of a
    /// kind that asks for no coverage.
    pub(super) fn covered_blocks<'d>(&self, d: &'d Doc) -> Option<(&'d str, Vec<&'d StrNode>)> {
        if !d.parsed() || !self.in_graph(d) || d.hidden {
            return None;
        }
        let k = d.r().get("kind").filter(|k| k.kind == Kind::String)?;
        let kd = self.schemas.kind(&k.str)?;
        if !kd.covered && kd.covered_fields.is_empty() {
            return None;
        }
        let blocks = d
            .strings()
            .iter()
            .filter(|sn| sn.blk && !sn.notes && !sn.anchor.is_empty())
            .filter(|sn| kd.covered_fields.is_empty() || kd.covered_fields.contains(&sn.field))
            .collect();
        Some((&k.str, blocks))
    }

    /// which_blocks words the blocks a kind wants covered: "every block", or
    /// "every block of field \"statement\"".
    fn which_blocks(&self, kind: &str) -> String {
        let fields = self.schemas.kind(kind).map(|kd| &kd.covered_fields);
        match fields.filter(|f| !f.is_empty()) {
            None => "every block".into(),
            Some(f) => {
                let names: Vec<String> = f.iter().map(|x| go_quote(x)).collect();
                format!("every block of {} {}", if f.len() == 1 { "field" } else { "fields" }, names.join(", "))
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
