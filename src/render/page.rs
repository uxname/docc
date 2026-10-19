use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;

use icu_collator::options::{CollatorOptions, Strength};
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use regex::Regex;

use crate::engine::{self, Doc};
use crate::loader::{Kind, Node};

use super::render::{HEADER, Renderer, heading, lf, numbered, one_line, page_path, root, title_of};
use super::text::{Page, Prop, is_empty, scalar_text};

pub(super) fn reserved(k: &str) -> bool {
    matches!(
        k,
        "title"
            | "kind"
            | "status"
            | "parent"
            | "body"
            | "tags"
            | "priority"
            | "notes"
            | "hidden"
            | "docIgnore"
            | "order"
    )
}

impl<'a> Renderer<'a> {
    pub(super) fn page(&self, d: &'a Doc) -> String {
        Page::new(self, d).build()
    }
}

/// field is one free field of a page with its rendered content.
struct Field<'a> {
    key: String,
    content: String,
    node: &'a Node,
    short: bool,
}

impl<'a> Page<'_, 'a> {
    /// build renders the page of pg.doc; in the HTML file it also fills pg.marks and pg.props.
    pub fn build(&mut self) -> String {
        let (r, d) = (self.r, self.doc);
        let mut blocks: Vec<String> = Vec::new();
        if !r.single {
            blocks.push(HEADER.to_string());
        } else if r.anchors {
            blocks.push(self.anchor_tag(""));
        }
        if !r.html {
            // the HTML file draws title, badges and relations from the data itself
            blocks.push(format!(
                "# {}",
                numbered(&r.number_of(d), &self.refs(&one_line(&raw_title(d))))
            ));
            let meta = r.meta_line(d);
            if !meta.is_empty() {
                blocks.push(meta);
            }
            if r.full && d.hidden {
                blocks.push(format!("> {}", r.t("Hidden document.")));
            }
        }
        if let Some(body) = root(d).get("body") {
            let ps = self.paragraphs(body, true);
            blocks.extend(ps);
        }
        let mut fields: Vec<Field<'a>> = Vec::new();
        let mut short = 0;
        for m in &root(d).members {
            if !reserved(&m.key) {
                let c = self.field_content(&m.value, 3);
                if !c.is_empty() {
                    // a form field is a block of its own, never a one-line property
                    let sh = !has_input(&m.value) && short_value(&self.mark_re().replace_all(&c, ""));
                    fields.push(Field {
                        key: m.key.clone(),
                        content: c,
                        node: &m.value,
                        short: sh,
                    });
                    if sh {
                        short += 1;
                    }
                }
            }
        }
        // HTML: one-line values form a property list before the sections, so a page
        // is not a stack of headings with a single word under each
        let props = r.html && short >= 2;
        if props {
            // drawn by the page script from pg.props
            for f in &fields {
                if f.short {
                    let re = self.mark_re();
                    self.cut += re.find_iter(&f.content).count();
                    self.props.push(Prop {
                        key: heading(&f.key),
                        id: format!("{}--{}", d.id, r.anchor_of(d, Some(f.node)).anchor),
                        node: f.node,
                        md: re.replace_all(&f.content, "").into_owned(),
                    });
                }
            }
        }
        for f in fields {
            if props && f.short {
                continue;
            }
            if r.anchors {
                blocks.push(self.anchor_tag(&r.anchor_of(d, Some(f.node)).anchor));
            }
            blocks.push(self.mark(None) + "## " + &heading(&f.key));
            blocks.push(f.content);
        }
        if r.full {
            if r.html {
                blocks.push(self.mark(None).trim().to_string());
            }
            self.service_sections(&mut blocks);
        }
        if !r.html {
            self.relations(&mut blocks);
        }
        blocks.join("\n\n") + "\n"
    }

    /// anchorTag is the target of an in-file link to this page (field == "") or to
    /// one of its field sections.
    // ponytail: ids are <docid>--<anchor>; a document id that itself contains "--" can collide.
    pub fn anchor_tag(&self, field: &str) -> String {
        let mut id = self.doc.id.clone();
        if !field.is_empty() {
            id += "--";
            id += field;
        }
        format!(r#"<a id="{id}"></a>"#)
    }

    /// fieldContent renders the value of a free field or a section; level is the
    /// heading level of its subsections.
    pub fn field_content(&mut self, v: &'a Node, level: usize) -> String {
        if is_empty(v) {
            return String::new();
        }
        if engine::is_sections(v) {
            return self.sections(v, level);
        }
        let t = self.table(v);
        if !t.is_empty() {
            return self.mark(Some(v)) + &t;
        }
        match v.kind {
            Kind::String => return self.paragraphs(v, true).join("\n\n"),
            Kind::Number | Kind::Bool => return self.mark(Some(v)) + &v.str,
            Kind::Array if v.items.iter().all(|it| it.kind == Kind::String || engine::is_input(it)) => {
                return self.paragraphs(v, true).join("\n\n");
            }
            _ => {}
        }
        self.mark(Some(v)) + &self.list_lines(v, "", true).join("\n")
    }

    /// sections renders the members of a set of sections as headings of the given
    /// level (at most 6), each followed by its content. A section stays visible
    /// without content: a link to it must have a target.
    fn sections(&mut self, v: &'a Node, level: usize) -> String {
        let mut blocks = Vec::new();
        for m in &v.members {
            if m.key == engine::SECTIONS_KEY {
                continue;
            }
            let a = self.r.anchor_of(self.doc, Some(&m.value));
            if self.r.anchors {
                blocks.push(self.anchor_tag(&a.anchor));
            } else if !a.num.is_empty() && !self.r.single {
                // the numbered heading gets another id on GitHub; links keep the plain anchor
                blocks.push(format!(r#"<a id="{}"></a>"#, a.anchor));
            }
            blocks.push(
                self.mark(None) + &"#".repeat(level) + " " + &numbered(&a.num, &heading(&m.key)),
            );
            let c = self.field_content(&m.value, (level + 1).min(6));
            if !c.is_empty() {
                blocks.push(c);
            }
        }
        blocks.join("\n\n")
    }

    /// serviceSections adds notes and docIgnore to the audit build.
    fn service_sections(&self, blocks: &mut Vec<String>) {
        let r = self.r;
        let mut add = |title: &str, content: String| {
            if !content.is_empty() {
                blocks.push(format!("## {}", r.t(title)));
                blocks.push(format!("> {}", r.t("Service information.")));
                blocks.push(content);
            }
        };
        let d = root(self.doc);
        if let Some(n) = d.get("notes").filter(|n| n.kind == Kind::Array) {
            let items: Vec<String> = engine::string_list(Some(n))
                .into_iter()
                .map(|it| format!("- {}", self.refs(&it.str).replace('\n', "\n  ")))
                .collect();
            add("Notes (hidden)", items.join("\n"));
        }
        if let Some(n) = d.get("docIgnore").filter(|n| !is_empty(n)) {
            add(
                "Suppressions (docIgnore)",
                self.list_lines(n, "", false).join("\n"),
            );
        }
    }

    /// table renders an array of objects with identical scalar keys as a Markdown
    /// table; it returns "" for any other shape.
    fn table(&self, v: &Node) -> String {
        if v.kind != Kind::Array || v.items.is_empty() {
            return String::new();
        }
        let mut keys: Vec<&str> = Vec::new();
        for (i, it) in v.items.iter().enumerate() {
            if it.kind != Kind::Object || it.members.is_empty() || engine::is_input(it) {
                return String::new();
            }
            for (j, m) in it.members.iter().enumerate() {
                if matches!(m.value.kind, Kind::Object | Kind::Array) {
                    return String::new();
                }
                if i == 0 {
                    keys.push(&m.key);
                } else if j >= keys.len() || keys[j] != m.key {
                    return String::new();
                }
            }
            if i > 0 && it.members.len() != keys.len() {
                return String::new();
            }
        }
        let cell = |n: &Node| {
            let s = self
                .inline(scalar_text(n), n.kind == Kind::String)
                .replace('|', r"\|");
            lf(&s).replace('\n', "<br>")
        };
        let mut rows = vec![
            format!("| {} |", keys.join(" | ")),
            format!("|{}", " --- |".repeat(keys.len())),
        ];
        for it in &v.items {
            let cells: Vec<String> = it.members.iter().map(|m| cell(&m.value)).collect();
            rows.push(format!("| {} |", cells.join(" | ")));
        }
        rows.join("\n")
    }

    /// relations adds the closing block: Parent, Referenced by, Covered by.
    fn relations(&self, blocks: &mut Vec<String>) {
        let (r, d) = (self.r, self.doc);
        let mut add = |title: &str, content: String| {
            if !content.is_empty() {
                blocks.push(format!("## {}", r.t(title)));
                blocks.push(content);
            }
        };
        if let Some(v) = root(d).get("parent").filter(|v| v.kind == Kind::String) {
            let t = r.p.doc(&v.str);
            if let Some(t) = t.filter(|t| r.has_page(Some(t))) {
                add("Parent", self.link(t, &r.title(t), ""));
            }
        }
        let back = r.backrefs();
        let items: Vec<String> = back
            .get(&d.id)
            .into_iter()
            .flatten()
            .map(|s| format!("- {}", self.link(s, &r.title(s), "")))
            .collect();
        add("Referenced by", items.join("\n"));
        let items: Vec<String> =
            r.p.covers
                .as_ref()
                .and_then(|c| c.get(&d.id))
                .into_iter()
                .flatten()
                .map(|c| format!("- `{c}`"))
                .collect();
        add("Covered by", items.join("\n"));
        if !r.single && r.p.cfg.roots.contains(&d.id) {
            blocks.push(format!("{}: [_tree.md](_tree.md)", r.t("Document tree")));
        }
    }
}

/// has_input reports whether a field value holds a form field anywhere.
fn has_input(n: &Node) -> bool {
    engine::is_input(n) || n.items.iter().any(has_input) || n.members.iter().any(|m| has_input(&m.value))
}

static BLOCK_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(- |[0-9]+\. |> |#|\|)").unwrap());

/// shortValue reports whether a rendered field is one plain line of up to 100
/// characters: no list, table, quote or heading.
fn short_value(c: &str) -> bool {
    !c.contains('\n') && c.chars().count() <= 100 && !BLOCK_START.is_match(c)
}

/// entry is one line of the document tree.
#[derive(Clone, Copy)]
pub(super) struct Entry<'a> {
    pub d: &'a Doc,
    pub depth: usize,
}

/// orderOf returns the order field of a document, if it is a number.
fn order_of(d: &Doc) -> Option<f64> {
    let n = root(d).get("order").filter(|n| n.kind == Kind::Number)?;
    // strconv.ParseFloat fails beyond the float range
    n.str.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// collator compares titles as collate.New(language.Und, collate.IgnoreCase,
/// collate.Numeric) does: root order, case ignored, numbers by value.
pub(super) fn collator() -> CollatorBorrowed<'static> {
    let mut prefs = CollatorPreferences::default();
    prefs.numeric_ordering = Some(CollationNumericOrdering::True);
    let mut opts = CollatorOptions::default();
    opts.strength = Some(Strength::Secondary);
    Collator::try_new(prefs, opts).expect("compiled collation data")
}

/// byOrder sorts siblings by order (documents without it come last), then by
/// the title without its number: case-insensitive, numbers by value, Latin
/// before Cyrillic. Equal titles fall back to the id, so the order is the same
/// on every build.
fn by_order(c: &CollatorBorrowed<'_>, list: &mut [&Doc]) {
    list.sort_by(|a, b| {
        let (oa, ob) = (order_of(a), order_of(b));
        match (oa, ob) {
            (Some(_), None) => return Ordering::Less,
            (None, Some(_)) => return Ordering::Greater,
            (Some(x), Some(y)) if x != y => return x.partial_cmp(&y).unwrap(),
            _ => {}
        }
        c.compare(&title_of(a), &title_of(b))
            .then_with(|| a.id.cmp(&b.id))
    });
}

impl<'a> Renderer<'a> {
    /// layout nests the documents by parent (siblings by order, then title): first those
    /// reachable from the roots, then those without a visible parent (with their own children).
    pub(super) fn layout(&self) -> (Vec<Entry<'a>>, Vec<Entry<'a>>) {
        let docs = self.docs();
        let mut is_root = HashSet::new();
        let mut roots = Vec::new();
        for id in &self.p.cfg.roots {
            let d = self.p.doc(id);
            if self.has_page(d) && !is_root.contains(id.as_str()) {
                is_root.insert(id.as_str());
                roots.push(d.unwrap());
            }
        }
        let mut children: HashMap<&str, Vec<&'a Doc>> = HashMap::new();
        let mut loose = Vec::new();
        for d in docs {
            if is_root.contains(d.id.as_str()) {
                continue;
            }
            let pid = match root(d).get("parent") {
                Some(n) if n.kind == Kind::String => n.str.as_str(),
                _ => "",
            };
            if !pid.is_empty() && self.has_page(self.p.doc(pid)) {
                children.entry(pid).or_default().push(d);
            } else {
                loose.push(d);
            }
        }
        let c = collator();
        for list in children.values_mut() {
            by_order(&c, list);
        }
        by_order(&c, &mut loose);

        fn emit<'a>(
            d: &'a Doc,
            depth: usize,
            children: &HashMap<&str, Vec<&'a Doc>>,
            seen: &mut HashSet<&'a str>,
            out: &mut Vec<Entry<'a>>,
        ) {
            if !seen.insert(d.id.as_str()) {
                return;
            }
            out.push(Entry { d, depth });
            for c in children.get(d.id.as_str()).into_iter().flatten() {
                emit(c, depth + 1, children, seen, out);
            }
        }
        let mut seen = HashSet::new();
        let (mut tree, mut orphans) = (Vec::new(), Vec::new());
        for d in roots {
            emit(d, 0, &children, &mut seen, &mut tree);
        }
        for d in loose {
            emit(d, 0, &children, &mut seen, &mut orphans);
        }
        (tree, orphans)
    }

    /// href is the link target of a page: its file, or an in-file anchor in a
    /// single-file build.
    pub(super) fn href(&self, d: &Doc) -> String {
        if self.single {
            return format!("#{}", d.id);
        }
        page_path(d)
    }

    /// treeLines renders tree entries as a nested list.
    fn tree_lines(&self, es: &[Entry<'_>]) -> String {
        let mut lines = Vec::new();
        for e in es {
            let mut line = format!(
                "{}- [{}]({})",
                "  ".repeat(e.depth),
                self.title(e.d),
                self.href(e.d)
            );
            if self.full && e.d.hidden {
                line += &format!(" ({})", self.t("hidden"));
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    /// treeBlocks are the blocks of the document tree, without the header.
    pub(super) fn tree_blocks(&self) -> Vec<String> {
        let (tree, orphans) = self.layout();
        let mut out = Vec::new();
        if self.full {
            out.push(format!(
                "> {}",
                self.t("full build: includes hidden documents and notes — do not publish")
            ));
        }
        out.push(format!("# {}", self.t("Document tree")));
        if !tree.is_empty() {
            out.push(self.tree_lines(&tree));
        }
        if !orphans.is_empty() {
            out.push(format!("## {}", self.t("Without parent")));
            out.push(self.tree_lines(&orphans));
        }
        let un = self.uncovered();
        if !un.is_empty() {
            let lines: Vec<String> = un
                .iter()
                .map(|d| format!("- [{}]({})", self.title(d), self.href(d)))
                .collect();
            out.push(format!("## {}", self.t("Documents without code coverage")));
            out.push(lines.join("\n"));
        }
        out
    }

    /// uncovered lists the documents of a covered kind no code tag points to
    /// (DOC403); empty without trace.scan.
    pub(super) fn uncovered(&self) -> Vec<&'a Doc> {
        self.docs()
            .into_iter()
            .filter(|d| self.p.needs_code(d))
            .collect()
    }

    /// tree renders _tree.md: documents nested by parent.
    pub(super) fn tree(&self) -> String {
        let mut blocks = vec![HEADER.to_string()];
        blocks.extend(self.tree_blocks());
        blocks.join("\n\n") + "\n"
    }

    /// metaLine is the one-line summary under the title: kind · status · priority N · tags.
    fn meta_line(&self, d: &Doc) -> String {
        let rt = root(d);
        let mut parts = Vec::new();
        for k in ["kind", "status"] {
            if let Some(v) = rt
                .get(k)
                .filter(|v| v.kind == Kind::String && !v.str.is_empty())
            {
                parts.push(one_line(&v.str));
            }
        }
        if let Some(v) = rt
            .get("priority")
            .filter(|v| matches!(v.kind, Kind::Number | Kind::String) && !v.str.is_empty())
        {
            parts.push(format!("{} {}", self.t("priority"), one_line(&v.str)));
        }
        let tags: Vec<&str> = engine::string_list(rt.get("tags"))
            .into_iter()
            .map(|n| n.str.as_str())
            .collect();
        if !tags.is_empty() {
            parts.push(tags.join(", "));
        }
        parts.join(" · ")
    }

    /// backrefs maps a document id to the documents that point at it by parent,
    /// or [[id]] in the text (notes excluded), ordered by path.
    pub(super) fn backrefs(&self) -> Rc<HashMap<String, Vec<&'a Doc>>> {
        if let Some(b) = self.back.borrow().as_ref() {
            return b.clone();
        }
        let mut back: HashMap<String, Vec<&'a Doc>> = HashMap::new();
        for d in self.docs() {
            let mut seen = HashSet::from([d.id.as_str()]);
            let mut add = |id: &'a str| {
                if !seen.contains(id) && self.has_page(self.p.doc(id)) {
                    seen.insert(id);
                    back.entry(id.to_string()).or_default().push(d);
                }
            };
            if let Some(n) = root(d).get("parent").filter(|n| n.kind == Kind::String) {
                add(&n.str);
            }
            for rf in &d.refs {
                if rf.ok && !rf.notes {
                    add(&rf.target);
                }
            }
        }
        let back = Rc::new(back);
        *self.back.borrow_mut() = Some(back.clone());
        back
    }
}

fn raw_title(d: &Doc) -> String {
    match root(d).get("title") {
        Some(t) if t.kind == Kind::String => t.str.clone(),
        _ => d.id.clone(),
    }
}
