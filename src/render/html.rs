use std::collections::{HashMap, HashSet};

use regex::Regex;
use serde::Serialize;

use crate::comments::{Comment, Place};
use crate::engine::{self, Doc};
use crate::history;
use crate::loader::{Kind, Node};

use super::htmlparts::children_of;
use super::page::TreeNode;
use super::lang::html_escape as esc;
use super::render::{Renderer, lf, root};
use super::text::Page;

pub(super) mod compile;
mod markdown;

static HTML_TEMPLATE: &str = include_str!("page.html");
static HTML_CSS: &str = include_str!("page.css");
static HTML_JS: &str = include_str!("app.js");

/// VENDOR_JS is htm/preact/standalone 3.1.1: Preact with hooks and htm, the
/// tagged templates the page script is written in. No build step.
static VENDOR_JS: &str = include_str!("vendor/htm-preact-standalone.js");

/// MERMAID_JS is mermaid 12.2.0 (dist/mermaid.min.js), which draws the
/// ```mermaid blocks; a file gets it only when a page has one (5.5 MB).
static MERMAID_JS: &str = include_str!("vendor/mermaid.min.js");

impl<'a> Renderer<'a> {
    /// HTML returns the single-page application: one self-contained HTML file
    /// whose script (Preact, embedded) draws the pages from a JSON island. Go
    /// renders the Markdown of every block to HTML; the page adds the tree,
    /// search, block history from Git and comments. Same call
    /// contract as Files; the error is a bad --since revision.
    pub fn html(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.html_of(&self.app_data()?))
    }

    /// app_data is the data the HTML file draws its pages from (the JSON
    /// island of html); the error is a bad --since revision.
    fn app_data(&self) -> Result<AppData, Box<dyn std::error::Error + Send + Sync>> {
        // in-file links; the headings get their ids from the converter
        let s = self.variant(true, false, true);
        let (tree, orphans) = s.layout();
        let mut kids: HashMap<String, Vec<String>> = children_of(&[&tree, &orphans])
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.iter().map(|d| d.id.clone()).collect()))
            .collect();
        let mut data = AppData {
            lang: s.p.cfg.lang.clone(),
            full: s.full,
            key: format!("docc:{}", base_name(&s.p.cfg.root.to_string_lossy())),
            ..Default::default()
        };
        let nav = |es: &[super::page::Entry<'_>]| -> Vec<NavEntry> {
            es.iter().map(|e| NavEntry::Page(e.d.id.clone(), e.depth)).collect()
        };
        if s.p.cfg.folders {
            // the tree of the folders, as docc compile gives it
            kids.clear();
            fn walk<'x>(r: &Renderer<'x>, ns: &[TreeNode<'x>], depth: usize, nav: &mut Vec<NavEntry>, kids: &mut HashMap<String, Vec<String>>) {
                for n in ns {
                    match n.page() {
                        Some(d) => {
                            nav.push(NavEntry::Page(d.id.clone(), depth));
                            let ks: Vec<String> = n.kids().iter().filter_map(|k| k.page()).map(|k| k.id.clone()).collect();
                            if !ks.is_empty() {
                                kids.insert(d.id.clone(), ks);
                            }
                        }
                        None => nav.push(NavEntry::Folder(String::new(), depth, r.node_title(n))),
                    }
                    walk(r, n.kids(), depth + 1, nav, kids);
                }
            }
            walk(&s, &s.tree_nodes(), 0, &mut data.nav.tree, &mut kids);
        } else {
            data.nav.tree = nav(&tree);
            data.nav.orphans = nav(&orphans);
        }
        if !s.only.is_empty() {
            // build --page: the file is that page alone
            (data.nav.tree, data.nav.orphans) = (vec![NavEntry::Page(s.only.clone(), 0)], Vec::new());
            kids.clear();
        }
        let current: HashMap<String, &Node> = s
            .pages()
            .iter()
            .map(|d| (d.rel.clone(), root(d)))
            .collect();
        let mut hist = history::load(
            &s.p.cfg.abs(&s.p.cfg.src),
            &current,
            &history::Options {
                full: s.full,
                since: s.since.clone(),
                rev: match &s.p.cfg.source {
                    crate::source::Source::Rev(r) => r.rev().to_string(),
                    crate::source::Source::Disk => String::new(),
                },
            },
        )?;
        let mut pages: Vec<PageData> = Vec::new();
        for d in s.pages() {
            let Built { mut pd, pg, marker, strs } = s.build_page(d);
            pd.summary = s.summary(d);
            pd.kids = kids.get(&d.id).cloned().unwrap_or_default();
            if !s.only.is_empty() {
                (pd.crumbs, pd.parent) = (Vec::new(), String::new()); // pages the file does not have
            }
            // ponytail: old tables, lists and objects stay plain text in the
            // file (their text is flattened); includes and links in old text
            // resolve against the documents as they are now
            if let Some(h) = hist.as_mut().and_then(|h| h.docs.get_mut(&d.rel)) {
                for (p, cur, prop) in &strs {
                    for v in h.blocks.get_mut(p).into_iter().flatten() {
                        if v.text == *cur {
                            continue;
                        }
                        let out = s.to_html(d, &pg.source(&v.text), &marker);
                        let mut o = out.trim();
                        if *prop {
                            o = o.strip_prefix("<p>").unwrap_or(o);
                            o = o.strip_suffix("</p>").unwrap_or(o);
                        }
                        v.html = o.to_string();
                    }
                }
            }
            pages.push(pd);
        }
        if let Some(p) = pages.first() {
            data.title = p.title.clone();
        }
        data.hist = hist;
        // the comments of the documents, at their blocks; the page script draws
        // those that lost their place at the top of the page
        for d in s.pages() {
            for (c, pl) in s.comments_of(d) {
                data.comments.push(AppComment { doc: d.id.clone(), lost: pl == Place::Lost, text: c.words(), c });
            }
        }
        if !pages.is_empty() {
            data.pages = Some(pages);
        }
        let (_, head, print, foot) = s.brand();
        data.brand = BrandData { head, print, foot };
        Ok(data)
    }

    /// to_html converts the Markdown of a page of this (HTML) renderer; footnote
    /// ids are <page id>--fn:1, unique across the pages of the file.
    pub(super) fn to_html(&self, d: &Doc, src: &str, marker: &Regex) -> String {
        let mut g = Ids {
            prefix: d.id.clone(),
            used: HashSet::new(),
            nums: HashSet::new(),
        };
        if let Some(slugs) = self.slugs.borrow().get(&(d as *const Doc)) {
            // filled by Page::build
            for a in slugs.values() {
                if !a.num.is_empty() {
                    g.nums.insert(a.num.clone());
                }
            }
        }
        markdown::convert(src, &format!("{}--", d.id), marker, &mut g)
    }

    /// build_page renders the page of d with this (HTML) renderer and cuts it
    /// into blocks at the markers: the page data without summary and kids, and
    /// what the history needs to render old versions of its string blocks.
    pub(super) fn build_page<'r>(&'r self, d: &'a Doc) -> Built<'r, 'a> {
        // a marker name that the text of the page does not contain, so no agent text can cut it
        let mut pg = Page::new(self, d);
        let mut src = pg.build();
        let mut k = 0;
        while k < 8
            && src.matches(&format!("<!--{}:", pg.mark_tag())).count()
                != pg.marks.len() - pg.cut
        {
            let tag = pg.mark_tag().to_string() + "x";
            pg = Page::new(self, d);
            pg.tag = tag;
            src = pg.build();
            k += 1;
        }
        let marker = pg.mark_re();
        let ptrs = history::pointers(root(d));
        let ptr = |n: &Node| ptrs.get(&(n as *const Node)).cloned().unwrap_or_default();
        let mut pd = PageData {
            id: d.id.clone(),
            title: self.title(d),
            file: d.rel.clone(),
            hidden: self.full && d.hidden,
            kind: root(d)
                .get("kind")
                .filter(|n| n.kind == Kind::String)
                .map(|n| n.str.clone())
                .unwrap_or_default(),
            ..Default::default()
        };
        pd.crumbs = self.crumb_ids(d);
        if let Some(n) = root(d).get("parent")
            && n.kind == Kind::String
            && self.has_page(self.p.doc(&n.str))
        {
            pd.parent = n.str.clone();
        }
        let out = self.to_html(d, &src, &marker);
        // string blocks: [pointer, current text, is a prop]; their old versions get HTML
        let mut strs: Vec<(String, String, bool)> = Vec::new();
        // cut the page at the block markers; footnotes become a block of their own
        let mut add = |n: Option<&Node>, h: &str, head: Option<&(usize, String)>| {
            let input = n.filter(|n| engine::is_input(n)).map(InputData::of);
            if h.trim().is_empty() && input.is_none() {
                return;
            }
            let mut b = BlockData {
                h: h.to_string(),
                i: input,
                head: head.cloned(),
                ..Default::default()
            };
            if let Some(n) = n {
                b.p = ptr(n);
                b.s = history::text(n);
                if n.kind == Kind::String {
                    strs.push((b.p.clone(), b.s.clone(), false));
                }
            }
            pd.blocks.push(b);
        };
        let (mut prev, mut last, mut head) = (0, None, None);
        for c in marker.captures_iter(&out) {
            let (m, num) = (c.get(0).unwrap(), c.get(1).unwrap());
            add(last, &out[prev..m.start()], head);
            let k: usize = num.as_str().parse().unwrap_or(0);
            last = pg.marks.get(k).copied().flatten();
            head = pg.heads.get(&k);
            prev = m.end();
        }
        let tail = &out[prev..];
        if let Some(i) = tail.find(r#"<div class="footnotes""#) {
            add(last, &tail[..i], head);
            add(None, &tail[i..], None);
        } else {
            add(last, tail, head);
        }
        for p in &pg.props {
            let mut n = p.node;
            if n.kind == Kind::Array && n.items.len() == 1 {
                n = &n.items[0];
            }
            let h = self.to_html(d, &p.md, &marker);
            let h = h.trim();
            let h = h.strip_prefix("<p>").unwrap_or(h);
            let h = h.strip_suffix("</p>").unwrap_or(h);
            if n.kind == Kind::String {
                strs.push((ptr(n), history::text(n), true));
            }
            pd.props.push(PropData {
                k: p.key.clone(),
                id: p.id.clone(),
                p: ptr(n),
                h: h.to_string(),
                s: history::text(n),
            });
        }
        Built { pd, pg, marker, strs }
    }

    /// html_of is the HTML file around data.
    fn html_of(&self, data: &AppData) -> String {
        let s = self.variant(true, false, true);
        let brand_css = s.brand().0;
        let title = if data.title.is_empty() {
            "docc".to_string()
        } else {
            data.title.clone()
        };
        let css = HTML_CSS.to_string() + &brand_css;
        let json = script_json(data);
        let js = s.labels(HTML_JS, true);
        // the class markdown.rs gives a ```mermaid block, as the JSON island writes it
        let mermaid = if json.contains(r#"\u003ccode class=\"language-mermaid\">"#) {
            format!("<script>\n{MERMAID_JS}\n</script>\n")
        } else {
            String::new()
        };
        let out = replace_once(
            &s.labels(HTML_TEMPLATE, false),
            &[
                ("{{TITLE}}", &esc(&title)),
                ("{{CSS}}", &css),
                ("{{DATA}}", &json),
                ("{{VENDOR}}", VENDOR_JS),
                ("{{MERMAID}}", &mermaid),
                ("{{JS}}", &js),
            ],
        );
        lf(&out)
    }

    /// crumbIDs is the chain of parents above the title, outermost first.
    fn crumb_ids(&self, d: &Doc) -> Vec<String> {
        let mut chain = Vec::new();
        let mut seen = HashSet::from([d.id.clone()]);
        let mut cur = d;
        while let Some(n) = root(cur).get("parent").filter(|n| n.kind == Kind::String) {
            let p = self.p.doc(&n.str);
            let Some(p) = p.filter(|p| self.has_page(Some(p)) && !seen.contains(&p.id)) else {
                break;
            };
            seen.insert(p.id.clone());
            chain.insert(0, p.id.clone());
            cur = p;
        }
        chain
    }
}

/// replace_once is strings.NewReplacer(...).Replace: one pass from left to
/// right, the replacements are not searched again.
fn replace_once(s: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    'outer: while i < s.len() {
        for (old, new) in pairs {
            if s[i..].starts_with(old) {
                out.push_str(new);
                i += old.len();
                continue 'outer;
            }
        }
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// base_name is filepath.Base: the last element of a path, cut at the
/// separators of the system ('/', and '\\' too on Windows, where a drive
/// letter "C:" is left out first).
// ponytail: a UNC volume (\\server\share) is not left out as Go does; it only
// matters for a project at the root of a share.
fn base_name(p: &str) -> String {
    use std::path::{MAIN_SEPARATOR, is_separator};
    let b = p.as_bytes();
    let p = if cfg!(windows) && b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        &p[2..]
    } else {
        p
    };
    if p.is_empty() {
        return ".".into();
    }
    let t = p.trim_end_matches(is_separator);
    if t.is_empty() {
        return MAIN_SEPARATOR.to_string();
    }
    t.rsplit(is_separator).next().unwrap_or(t).to_string()
}

#[derive(Serialize, Default)]
struct AppData {
    title: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    lang: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    full: bool,
    /// localStorage key of the comments
    key: String,
    nav: NavData,
    pages: Option<Vec<PageData>>,
    hist: Option<history::History>,
    comments: Vec<AppComment>,
    brand: BrandData,
}

/// AppComment is a comment of a document as the page script reads it: the
/// fields of the browser's own comments (path is the block, text the words).
#[derive(Serialize)]
struct AppComment {
    #[serde(flatten, serialize_with = "app_comment")]
    c: Comment,
    doc: String,
    text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    lost: bool,
}

fn app_comment<S: serde::Serializer>(c: &Comment, s: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct C<'a> {
        id: &'a str,
        path: &'a str,
        quote: &'a str,
        #[serde(skip_serializing_if = "str::is_empty")]
        fragment: &'a str,
        #[serde(skip_serializing_if = "str::is_empty")]
        prefix: &'a str,
        #[serde(skip_serializing_if = "str::is_empty")]
        suffix: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        input: &'a Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        answer: &'a Option<Vec<String>>,
        #[serde(skip_serializing_if = "str::is_empty")]
        author: &'a str,
        created: &'a str,
    }
    C {
        id: &c.id,
        path: &c.at,
        quote: &c.quote,
        fragment: &c.fragment,
        prefix: &c.prefix,
        suffix: &c.suffix,
        input: &c.input,
        answer: &c.answer,
        author: &c.author,
        created: &c.created,
    }
    .serialize(s)
}

#[derive(Serialize, Default)]
struct NavData {
    tree: Vec<NavEntry>,
    orphans: Vec<NavEntry>,
}

/// NavEntry is a line of the tree: [id, depth] for a page, ["", depth, title]
/// for a folder without a page of its own (tree "folders").
#[derive(Serialize)]
#[serde(untagged)]
enum NavEntry {
    Page(String, usize),
    Folder(String, usize, String),
}

#[derive(Serialize, Default)]
struct BrandData {
    #[serde(skip_serializing_if = "String::is_empty")]
    head: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    print: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    foot: String,
}

/// Built is one page cut into blocks (build_page).
pub(super) struct Built<'r, 'a> {
    pub pd: PageData,
    pub pg: Page<'r, 'a>,
    pub marker: Regex,
    /// string blocks: pointer, current text, whether it is a property
    pub strs: Vec<(String, String, bool)>,
}

#[derive(Serialize, Default)]
pub(super) struct PageData {
    id: String,
    title: String,
    /// path relative to src: the key of the history
    file: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
    /// picks the icon in the tree
    #[serde(skip_serializing_if = "String::is_empty")]
    kind: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    crumbs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    kids: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    parent: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    summary: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    props: Vec<PropData>,
    blocks: Vec<BlockData>,
}

/// blockData is one block of a page: its JSON Pointer and source text ("" for
/// headings and footnotes, which have no history) and its HTML.
#[derive(Serialize, Default)]
struct BlockData {
    #[serde(skip_serializing_if = "String::is_empty")]
    p: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    s: String,
    h: String,
    /// a form field, drawn by the page script in place of h
    #[serde(skip_serializing_if = "Option::is_none")]
    i: Option<InputData>,
    /// a field or section heading: its level and title (docc compile)
    #[serde(skip)]
    head: Option<(usize, String)>,
}

/// inputData is a form field: its kind (text, radio, checkbox), id, question
/// and choices, as written (DOC111 checks them).
#[derive(Serialize, Default, Clone)]
pub struct InputData {
    t: String,
    id: String,
    label: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    options: Vec<String>,
}

impl InputData {
    fn of(n: &Node) -> InputData {
        let s = |k: &str| n.get(k).filter(|v| v.kind == Kind::String).map(|v| v.str.clone()).unwrap_or_default();
        InputData {
            t: s(engine::INPUT_KEY),
            id: s("id"),
            label: s("label"),
            options: engine::string_list(n.get("options")).into_iter().map(|o| o.str.clone()).collect(),
        }
    }
}

#[derive(Serialize, Default)]
struct PropData {
    k: String,
    id: String,
    p: String,
    s: String,
    h: String,
}

/// scriptJSON encodes v for a <script type="application/json"> element: every
/// "<" is escaped, so no text inside can end the element or open a tag. As
/// Go's encoder with SetEscapeHTML(false), U+2028 and U+2029 are escaped too;
/// they can only occur inside strings, so a plain replace is safe.
fn script_json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v)
        .unwrap_or_default() // plain data, cannot fail
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .replace('<', "\\u003c")
}

/// ids gives heading ids <prefix>--<anchor>: the page id plus the same anchor
/// that [[id#field]] links use, unique within the page. A section number
/// (from nums) is left out, so numbering does not move the anchors.
#[derive(Clone)]
pub(super) struct Ids {
    prefix: String,
    used: HashSet<String>,
    nums: HashSet<String>,
}

impl Ids {
    fn generate(&mut self, value: &str) -> String {
        let mut v = value;
        if let Some((num, rest)) = v.split_once(' ')
            && self.nums.contains(num)
        {
            v = rest;
        }
        let base = format!("{}--{}", self.prefix, engine::anchor(v));
        let mut id = base.clone();
        let mut n = 1;
        while self.used.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        self.used.insert(id.clone());
        id
    }
}

#[cfg(test)]
mod tests;
