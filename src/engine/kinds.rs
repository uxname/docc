use std::sync::LazyLock;

use regex::Regex;

use super::analyze::string_list;
use super::graph::parent_of;
use super::project::{Doc, Project};
use crate::config::go_quote;
use crate::loader::Kind;
use crate::schema;

static KIND_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9-]+$").unwrap());

impl Project {
    /// checkKinds emits DOC106 (kind schema), DOC107 (tags registry), DOC109 (no
    /// kind schema) and DOC214 (reference to a document of a wrong kind).
    pub(super) fn check_kinds(&self) {
        for d in &self.docs {
            let Some(root) = d.root() else {
                continue;
            };
            let k = root.get("kind");
            if k.is_none() {
                self.emit(
                    d,
                    "DOC109",
                    root.start,
                    root.start + 1,
                    "the document has no kind".into(),
                    format!(
                        r#"Add "kind" with the name of a schema from {}."#,
                        self.cfg.schemas
                    ),
                );
            }
            if let Some(k) = k
                && k.kind == Kind::String
                && KIND_NAME.is_match(&k.str)
            {
                // a malformed kind is DOC105
                if !self.schemas.has_kind(&k.str) {
                    self.emit(
                        d,
                        "DOC109",
                        k.start,
                        k.end,
                        format!("there is no schema for kind {}", go_quote(&k.str)),
                        format!(
                            "Create {}/{}.json or use a kind that has a schema.",
                            self.cfg.schemas, k.str
                        ),
                    );
                }
                if !self.off("DOC106") && self.schemas.has_kind(&k.str) {
                    for v in self.schemas.check_kind(&k.str, root) {
                        self.emit(
                            d,
                            "DOC106",
                            v.start,
                            v.end,
                            format!(
                                "document violates the schema of kind {}: {}",
                                go_quote(&k.str),
                                v.message
                            ),
                            v.hint,
                        );
                    }
                }
                if let Some(kd) = self.schemas.kind(&k.str)
                    && self.in_graph(d)
                {
                    self.wrong_targets(d, &k.str, kd);
                }
            }
            if self.cfg.has_tags {
                for t in string_list(root.get("tags")) {
                    if !self.cfg.tags.contains(&t.str) {
                        self.emit(
                            d,
                            "DOC107",
                            t.start,
                            t.end,
                            format!("tag {} is not in the tags registry", go_quote(&t.str)),
                            registry_hint("tags", &self.cfg.tags, &self.cfg.file),
                        );
                    }
                }
            }
        }
    }

    /// wrongTargets emits DOC214: the parent or a reference in a field whose
    /// schema property has "x-docc-target" points to a document of another kind.
    /// A missing target or one without a kind is left to DOC202 and DOC109.
    fn wrong_targets(&self, d: &Doc, kind_name: &str, k: &schema::Kind) {
        let check = |field: &str, id: &str, start: usize, end: usize, what: &str| {
            let want = k.targets(field);
            let Some(troot) = self.doc(id).and_then(Doc::root) else {
                return;
            };
            if want.is_empty() {
                return;
            }
            let Some(tk) = troot.get("kind") else {
                return;
            };
            if tk.kind != Kind::String || want.contains(&tk.str) {
                return;
            }
            self.emit(
                d,
                "DOC214",
                start,
                end,
                format!(
                    "{what} points to {} of kind {}, but field {} takes only kind {}",
                    go_quote(id),
                    go_quote(&tk.str),
                    go_quote(field),
                    want.join(", ")
                ),
                format!(
                    "Point it to a document of kind {}, or change \"x-docc-target\" of {} in {}/{}.json.",
                    want.join(" or "),
                    go_quote(field),
                    self.cfg.schemas,
                    kind_name
                ),
            );
        };
        if let Some(n) = parent_of(d) {
            check("parent", &n.str, n.start, n.end, "parent");
        }
        for r in &d.refs {
            if r.ok {
                let p = r.ptr.strip_prefix('/').unwrap_or(&r.ptr);
                let field = p.split_once('/').map_or(p, |(f, _)| f);
                let field = field.replace("~1", "/").replace("~0", "~");
                check(
                    &field,
                    &r.target,
                    r.start,
                    r.end,
                    &format!("reference [[{}]]", r.inner),
                );
            }
        }
    }
}

fn registry_hint(name: &str, allowed: &[String], file: &str) -> String {
    if allowed.is_empty() {
        return format!("The {name} registry in {file} is empty: no values are allowed.");
    }
    format!(
        "Use one of: {}; or add the value to {} in {file}.",
        allowed.join(", "),
        go_quote(name)
    )
}
