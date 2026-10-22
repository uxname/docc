use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;

use crate::engine::{self, Doc};
use crate::loader::{Kind, Member, Node};
use crate::mdscan;

use super::render::{Renderer, numbered, page_path, rel_path, root};

/// page is the rendering context of one document.
pub(super) struct Page<'r, 'a> {
    pub r: &'r Renderer<'a>,
    pub doc: &'a Doc,
    /// page path relative to the output directory
    pub path: String,

    // HTML file only: the nodes of the blocks in marker order (None for a
    // heading or a service section) and the one-line fields of the property list
    pub marks: Vec<Option<&'a Node>>,
    pub props: Vec<Prop<'a>>,
    /// marker name; "" is "b", see mark_tag
    pub tag: String,
    /// markers removed from the text again (the property list)
    pub cut: usize,
    /// HTML file only: marker number of a field or section heading -> its
    /// level and title
    pub heads: std::collections::HashMap<usize, (usize, String)>,
}

/// prop is one entry of the property list of the HTML file.
pub(super) struct Prop<'a> {
    pub key: String,
    pub id: String,
    pub node: &'a Node,
    pub md: String,
}

static MARK_B: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--b:([0-9]+)-->\n(?:\n)?").unwrap());

impl<'r, 'a> Page<'r, 'a> {
    pub fn new(r: &'r Renderer<'a>, doc: &'a Doc) -> Page<'r, 'a> {
        Page {
            r,
            doc,
            path: page_path(doc),
            marks: Vec::new(),
            props: Vec::new(),
            tag: String::new(),
            cut: 0,
            heads: std::collections::HashMap::new(),
        }
    }

    /// mark starts a block of the HTML file: a comment line the HTML build cuts the
    /// page at, numbered in pg.marks. Elsewhere it is empty.
    pub fn mark(&mut self, n: Option<&'a Node>) -> String {
        if !self.r.html {
            return String::new();
        }
        self.marks.push(n);
        format!("<!--{}:{}-->\n\n", self.mark_tag(), self.marks.len() - 1)
    }

    /// mark_head is mark(None) for a heading of the given level and title.
    pub fn mark_head(&mut self, level: usize, title: &str) -> String {
        self.heads.insert(self.marks.len(), (level, title.to_string()));
        self.mark(None)
    }

    pub fn mark_tag(&self) -> &str {
        if self.tag.is_empty() { "b" } else { &self.tag }
    }

    /// markRe matches the markers of the page, with the blank line after them.
    pub fn mark_re(&self) -> Regex {
        if self.tag.is_empty() {
            return MARK_B.clone();
        }
        Regex::new(&format!(
            r"<!--{}:([0-9]+)-->\n(?:\n)?",
            regex::escape(self.mark_tag())
        ))
        .unwrap()
    }

    /// link renders a link to a document page, with an optional anchor.
    pub fn link(&self, target: &Doc, text: &str, anchor: &str) -> String {
        if self.r.single {
            let mut href = format!("#{}", target.id);
            if !anchor.is_empty() {
                href += "--";
                href += anchor;
            }
            return format!("[{text}]({href})");
        }
        let mut href = rel_path(&self.path, &page_path(target));
        if !anchor.is_empty() {
            href += "#";
            href += anchor;
        }
        format!("[{text}]({href})")
    }
}

const MAX_INCLUDE_DEPTH: usize = 32;

/// images replaces the relative paths of Markdown images outside code with
/// to(path relative to src); a path that leaves src stays as written.
pub(super) fn images(s: &str, doc_rel: &str, mut to: impl FnMut(&str) -> String) -> String {
    let mut b = String::new();
    let mut last = 0;
    for im in mdscan::new(s).images() {
        let (rel, ok) = engine::image_path(doc_rel, &im.inner);
        if ok && mdscan::local_image(&im.inner) {
            b.push_str(&s[last..im.start]);
            b.push_str(&to(&rel));
            last = im.end;
        }
    }
    if last == 0 {
        return s.to_string();
    }
    b + &s[last..]
}

/// path_base is Go's path.Base.
fn path_base(p: &str) -> &str {
    if p.is_empty() {
        return ".";
    }
    let p = p.trim_end_matches('/');
    if p.is_empty() {
        return "/";
    }
    p.rfind('/').map_or(p, |i| &p[i + 1..])
}

/// path_ext is Go's path.Ext.
pub(super) fn path_ext(p: &str) -> &str {
    for (i, b) in p.bytes().enumerate().rev() {
        match b {
            b'/' => break,
            b'.' => return &p[i..],
            _ => {}
        }
    }
    ""
}

impl Renderer<'_> {
    /// image is the link to an image (path relative to src) from the page at
    /// pagePath: the copy next to the pages, the copy in the output directory for
    /// the single Markdown file, a data URI in the single HTML file.
    pub(super) fn image(&self, page_path: &str, rel: &str) -> String {
        if self.html {
            let Ok(b) = self.p.cfg.source.read(&self.src_file(rel)) else {
                return rel.to_string();
            };
            return format!(
                "data:{};base64,{}",
                engine::image_ext(&path_ext(rel).to_ascii_lowercase()).unwrap_or(""),
                base64::engine::general_purpose::STANDARD.encode(b)
            );
        }
        if self.single {
            let out = if self.full {
                &self.p.cfg.out_full
            } else {
                &self.p.cfg.out
            };
            return format!("{}/{rel}", path_base(out));
        }
        if let Some(imgs) = self.imgs.borrow_mut().as_mut() {
            imgs.insert(rel.to_string());
        }
        rel_path(page_path, rel)
    }
}

/// cut is strings.Cut(s, "#"): before, after, found.
fn cut(s: &str) -> (&str, &str, bool) {
    match s.split_once('#') {
        Some((a, b)) => (a, b, true),
        None => (s, "", false),
    }
}

impl<'a> Page<'_, 'a> {
    /// refs replaces [[id]] and [[id#field]] outside code with Markdown links and
    /// points relative image paths at the output.
    pub fn refs(&self, s: &str) -> String {
        let s = images(s, &self.doc.rel, |rel| self.r.image(&self.path, rel));
        let rs = mdscan::new(&s).refs();
        if rs.is_empty() {
            return s;
        }
        let mut b = String::new();
        let mut last = 0;
        for rf in rs {
            let (id, field, has_field) = cut(&rf.inner);
            let t = self.r.p.doc(id);
            let Some(t) = t.filter(|t| self.r.has_page(Some(t))) else {
                continue; // validation rejects this; keep the text
            };
            let (mut anc, mut text) = (String::new(), self.r.title(t));
            if let Some(name) = field.strip_prefix('^') {
                anc = name.to_string(); // a block anchor link reads as the document title
            } else if has_field && field != "body" && field != "title" {
                let a = self.r.anchor_of(t, engine::resolve(root(t), field));
                anc = a.anchor;
                if anc.is_empty() {
                    // unknown path: the old guess, by the last key
                    anc = engine::heading_anchor(&field[field.rfind('/').map_or(0, |i| i + 1)..]);
                }
                if !a.title.is_empty() {
                    // a section is named by its own title
                    text = numbered(&a.num, &a.title);
                }
            }
            b.push_str(&s[last..rf.start]);
            b.push_str(&self.link(t, &text, &anc));
            last = rf.end;
        }
        b.push_str(&s[last..]);
        b
    }

    /// target returns the text elements of an include target field or block
    /// anchor, without block anchor markers: the anchor stays on its own page.
    fn target(&self, id: &str, field: &str) -> Option<Vec<String>> {
        let t = self.r.p.doc(id).filter(|t| t.parsed())?;
        if let Some(name) = field.strip_prefix('^') {
            let sn = t.block_anchor(name)?;
            let (text, _) = t.block(sn.node());
            return Some(vec![self.rebase(text, t)]);
        }
        let v = engine::resolve(root(t), field)?;
        match v.kind {
            Kind::String => {
                let (text, _) = t.block(v);
                Some(vec![self.rebase(text, t)])
            }
            Kind::Array => {
                let mut out = Vec::new();
                // form fields stay on their own page
                for it in v.items.iter().filter(|it| !engine::is_input(it)) {
                    if it.kind != Kind::String {
                        return None;
                    }
                    let (text, _) = t.block(it);
                    out.push(self.rebase(text, t));
                }
                (!out.is_empty()).then_some(out)
            }
            _ => None,
        }
    }

    /// rebase rewrites the image paths of text included from t relative to the
    /// folder of this page's document, so refs resolves them like its own.
    fn rebase(&self, text: &str, t: &Doc) -> String {
        if t.rel == self.doc.rel {
            return text.to_string();
        }
        images(text, &t.rel, |rel| rel_path(&self.doc.rel, rel))
    }

    fn comment(&self, open: bool, inner: &str) -> String {
        if open {
            return format!("<!-- {} {inner} -->", self.r.t("include"));
        }
        format!("<!-- /{} -->", self.r.t("include"))
    }

    /// expand resolves {{id#field}} in one element. A whole-element include turns
    /// into the paragraphs of the target; an include inside a string is replaced by
    /// the target text joined with spaces.
    fn expand(&self, s: &str, depth: usize) -> Vec<String> {
        if depth > MAX_INCLUDE_DEPTH {
            return vec![s.to_string()];
        }
        let sc = mdscan::new(s);
        let incs = sc.includes();
        if incs.is_empty() {
            return vec![s.to_string()];
        }
        if incs.len() == 1 && s.trim() == &s[incs[0].start..incs[0].end] {
            let (id, field, _) = cut(&incs[0].inner);
            let Some(elems) = self.target(id, field) else {
                return vec![s.to_string()];
            };
            let mut out = Vec::new();
            if self.r.full {
                out.push(self.comment(true, &incs[0].inner));
            }
            for e in elems {
                out.extend(self.expand(&e, depth + 1));
            }
            if self.r.full {
                out.push(self.comment(false, ""));
            }
            return out;
        }
        let mut b = String::new();
        let mut last = 0;
        for inc in incs {
            let (id, field, _) = cut(&inc.inner);
            let Some(elems) = self.target(id, field) else {
                continue;
            };
            let mut parts = Vec::new();
            for e in elems {
                parts.extend(self.expand(&e, depth + 1));
            }
            b.push_str(&s[last..inc.start]);
            if self.r.full {
                b.push_str(&self.comment(true, &inc.inner));
            }
            b.push_str(&parts.join(" "));
            if self.r.full {
                b.push_str(&self.comment(false, ""));
            }
            last = inc.end;
        }
        b.push_str(&s[last..]);
        vec![b]
    }

    /// paragraphs renders the string values of a field as paragraphs; a block
    /// anchor marker turns into an anchor tag before its paragraph. A form
    /// field is only a marker in the HTML file (the page script draws it) and
    /// is left out of the Markdown.
    pub fn paragraphs(&mut self, v: &'a Node, includes: bool) -> Vec<String> {
        let elems: Vec<&'a Node> = match v.kind {
            Kind::String => vec![v],
            Kind::Array => v
                .items
                .iter()
                .filter(|it| it.kind == Kind::String || (self.r.html && engine::is_input(it)))
                .collect(),
            _ => Vec::new(),
        };
        let mut out = Vec::new();
        for n in elems {
            if n.kind != Kind::String {
                out.push(self.mark(Some(n)).trim_end().to_string());
                continue;
            }
            let (e, tag) = self.block(n);
            let xs = if includes {
                self.expand(&e, 0)
            } else {
                vec![e]
            };
            for (i, x) in xs.into_iter().enumerate() {
                let mut x = self.refs(&x);
                if i == 0 {
                    x = self.mark(Some(n)) + &tag + &x;
                }
                out.push(x);
            }
        }
        out
    }

    /// block returns the text of a string of this page without its block anchor
    /// marker, and the anchor tag that replaces it ("" for none).
    fn block(&self, n: &Node) -> (String, String) {
        let (text, name) = self.doc.block(n);
        let tag = if name.is_empty() {
            String::new()
        } else if self.r.single {
            self.anchor_tag(name)
        } else {
            format!(r#"<a id="{name}"></a>"#)
        };
        (text.to_string(), tag)
    }

    /// inline renders a string for use inside a list item: includes joined by
    /// spaces, refs resolved, further lines indented.
    pub fn inline(&self, s: &str, includes: bool) -> String {
        if includes {
            return self.refs(&self.expand(s, 0).join(" "));
        }
        self.refs(s)
    }

    /// source renders an old version of a string block the way paragraphs
    /// renders the current one, for the history of the HTML file.
    pub fn source(&self, s: &str) -> String {
        let (s, _) = engine::split_anchor(s);
        self.expand(s, 0)
            .iter()
            .map(|x| self.refs(x))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// listLines renders an object or array value as a nested Markdown list.
    pub fn list_lines(&self, n: &Node, indent: &str, includes: bool) -> Vec<String> {
        let mut out = Vec::new();
        match n.kind {
            Kind::Object => {
                for m in &n.members {
                    out.extend(self.member(m, indent, includes));
                }
            }
            Kind::Array => {
                let all_objects =
                    !n.items.is_empty() && n.items.iter().all(|it| it.kind == Kind::Object);
                for (i, it) in n.items.iter().enumerate() {
                    let marker = if all_objects {
                        format!("{}. ", i + 1)
                    } else {
                        "- ".to_string()
                    };
                    out.extend(self.item(it, indent, &marker, includes));
                }
            }
            _ => {}
        }
        out
    }

    /// member renders "- **key:** value" with nested values below.
    fn member(&self, m: &Member, indent: &str, includes: bool) -> Vec<String> {
        if is_empty(&m.value) {
            return Vec::new();
        }
        let head = format!("{indent}- **{}:**", m.key);
        if matches!(m.value.kind, Kind::Object | Kind::Array) {
            let mut out = vec![head];
            out.extend(self.list_lines(&m.value, &format!("{indent}  "), includes));
            return out;
        }
        let text = self.inline(
            scalar_text(&m.value),
            includes && m.value.kind == Kind::String,
        );
        vec![format!(
            "{head} {}",
            indent_lines(&text, &format!("{indent}  "))
        )]
    }

    /// item renders one array element with the given list marker.
    fn item(&self, it: &Node, indent: &str, marker: &str, includes: bool) -> Vec<String> {
        if is_empty(it) {
            return Vec::new();
        }
        let pad = " ".repeat(marker.len());
        if matches!(it.kind, Kind::Object | Kind::Array) {
            let inner = format!("{indent}{pad}");
            let mut lines = self.list_lines(it, &inner, includes);
            if lines.is_empty() {
                return lines;
            }
            // the first nested line shares the marker line
            lines[0] = format!(
                "{indent}{marker}{}",
                lines[0].strip_prefix(&inner).unwrap_or(&lines[0])
            );
            return lines;
        }
        let (mut text, mut tag) = (scalar_text(it).to_string(), String::new());
        if includes && it.kind == Kind::String {
            // not in docIgnore
            (text, tag) = self.block(it);
        }
        let body = self.inline(&text, includes && it.kind == Kind::String);
        vec![format!(
            "{indent}{marker}{tag}{}",
            indent_lines(&body, &format!("{indent}{pad}"))
        )]
    }
}

fn indent_lines(s: &str, indent: &str) -> String {
    s.replace('\n', &format!("\n{indent}"))
}

pub(super) fn scalar_text(n: &Node) -> &str {
    match n.kind {
        Kind::String | Kind::Number | Kind::Bool => &n.str,
        _ => "",
    }
}

pub(super) fn is_empty(n: &Node) -> bool {
    match n.kind {
        Kind::Null => true,
        Kind::String => n.str.is_empty(),
        Kind::Array => n.items.is_empty(),
        Kind::Object => n.members.is_empty(),
        _ => false,
    }
}
