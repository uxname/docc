//! The data of `docc compile`: the pages of the reader (no hidden documents
//! unless Renderer::hidden is set, no notes) cut into blocks with their HTML, their sections and the tree.
//! The pages come from the same build as the HTML file, so a block's HTML
//! here is the block's HTML there.

use std::sync::LazyLock;

use base64::Engine as _;
use regex::Regex;
use serde::Serialize;

use super::super::page::TreeNode;
use super::super::render::{Renderer, root};
use super::super::text::path_ext;
use super::InputData;
use crate::comments::{Comment, Place};
use crate::engine;
use crate::loader::Kind;

/// SCHEMA_ID is the "$schema" of the output and the "$id" of its JSON Schema
/// (`docc schema compile`); the number changes when the shape does.
pub const SCHEMA_ID: &str = "urn:docc:schema:compile:1";

/// Compiled is what the sources compile to; docc compile adds the diagnostics.
#[derive(Serialize, Default)]
pub struct Compiled {
    pub project: ProjectInfo,
    pub tree: Vec<TreeEntry>,
    pub pages: Vec<CPage>,
}

#[derive(Serialize)]
pub struct ProjectInfo {
    /// the title of the first page of the tree
    pub title: String,
    pub lang: String,
    pub brand: Option<BrandInfo>,
}

impl Default for ProjectInfo {
    fn default() -> ProjectInfo {
        ProjectInfo { title: String::new(), lang: "en".into(), brand: None }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandInfo {
    name: String,
    url: Option<String>,
    /// a data: URI of the logo file
    logo: Option<String>,
    accent: Option<String>,
    accent_light: Option<String>,
    header: Option<String>,
}

/// TreeEntry is one entry of the document tree.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TreeEntry {
    Folder {
        /// relative to src
        path: String,
        title: String,
        /// the folder's own page
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// the folder's own page is a hidden document (compile --hidden)
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        hidden: bool,
        children: Vec<TreeEntry>,
    },
    Page {
        id: String,
        path: String,
        title: String,
        /// a hidden document (compile --hidden)
        #[serde(skip_serializing_if = "std::ops::Not::not")]
        hidden: bool,
        /// pages nested by parent (not in folder mode)
        #[serde(skip_serializing_if = "Vec::is_empty")]
        children: Vec<TreeEntry>,
    },
}

#[derive(Serialize)]
pub struct CPage {
    pub(crate) id: String,
    /// path of the source relative to src
    pub(crate) path: String,
    pub(crate) title: String,
    /// a hidden document (compile --hidden); absent otherwise
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    hidden: bool,
    kind: String,
    status: Option<String>,
    tags: Vec<String>,
    parent: Option<String>,
    crumbs: Vec<String>,
    pub(crate) props: Vec<CProp>,
    pub(crate) sections: Vec<Section>,
    pub(crate) blocks: Vec<CBlock>,
    /// the readers' comments of the page ("$comments") with their place
    pub(crate) comments: Vec<CComment>,
}

/// CComment is a comment of a page as the reader sees it: no email.
#[derive(Serialize)]
pub struct CComment {
    #[serde(flatten)]
    c: Comment,
    place: Place,
}

/// CProp is a one-line field shown in the property list instead of a section.
#[derive(Serialize)]
pub struct CProp {
    pub(crate) k: String,
    id: String,
    pub(crate) p: String,
    pub(crate) s: String,
    pub(crate) h: String,
}

/// Section is a field or $sections heading of a page.
#[derive(Serialize, Clone)]
pub struct Section {
    /// the anchor of the heading, as [[id#field]] links and the HTML file use it
    id: String,
    title: String,
    level: usize,
}

#[derive(Serialize, Clone)]
pub struct CBlock {
    /// JSON Pointer of the value in the source; "" for the footnotes of the page
    pub(crate) p: String,
    pub(crate) s: String,
    pub(crate) h: String,
    /// the section the block stands under; None above the first heading
    pub(crate) sec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) i: Option<InputData>,
    /// diff only: the label of a property (a one-line field of the list)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) k: Option<String>,
}

/// Seen is a place of a page that a reader sees (Renderer::seen).
pub struct Seen {
    /// JSON Pointer of the value in the source
    pub p: String,
    /// its source text, the "quote" of a comment on it
    pub s: String,
    /// the id of the form field, when it is one
    pub input: Option<String>,
}

static HEAD_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^<h[1-6] id="([^"]*)">"#).unwrap());

impl<'a> Renderer<'a> {
    /// compile builds the pages for a reader; call it only when validation
    /// found no errors of groups 1xx and 2xx.
    pub fn compile(&self) -> Compiled {
        let s = self.variant(true, false, true);
        let mut out = Compiled::default();
        for d in s.ordered() {
            let pd = s.build_page(d).pd;
            let prefix = format!("{}--", d.id);
            let slug = |id: &str| id.strip_prefix(&prefix).unwrap_or(id).replace("&amp;", "&");
            let str_of = |k: &str| root(d).get(k).filter(|n| n.kind == Kind::String).map(|n| n.str.clone());
            let mut pg = CPage {
                id: pd.id,
                path: pd.file,
                title: pd.title,
                hidden: pd.hidden,
                kind: pd.kind,
                status: str_of("status"),
                tags: engine::string_list(root(d).get("tags")).into_iter().map(|n| n.str.clone()).collect(),
                parent: Some(pd.parent).filter(|p| !p.is_empty()),
                crumbs: pd.crumbs,
                props: pd
                    .props
                    .into_iter()
                    .map(|p| CProp { id: slug(&p.id), k: p.k, p: p.p, s: p.s, h: p.h })
                    .collect(),
                sections: Vec::new(),
                blocks: Vec::new(),
                comments: s
                    .comments_of(d)
                    .into_iter()
                    .map(|(c, place)| CComment { c: Comment { email: String::new(), ..c }, place })
                    .collect(),
            };
            let mut sec: Option<String> = None;
            for b in pd.blocks {
                if let Some((level, title)) = b.head {
                    let id = HEAD_ID.captures(&b.h).map(|c| slug(&c[1])).unwrap_or_default();
                    sec = Some(id.clone());
                    pg.sections.push(Section { id, title, level });
                    continue;
                }
                pg.blocks.push(CBlock { p: b.p, s: b.s, h: b.h, sec: sec.clone(), i: b.i, k: None });
            }
            out.pages.push(pg);
        }
        out.tree = s.tree_entries(&s.tree_nodes());
        out.project = s.project_info(out.pages.first().map(|p| p.title.clone()).unwrap_or_default());
        out
    }

    /// seen are the places of the page of d that a reader sees: the blocks
    /// and properties docc compile outputs for it (headings and footnotes
    /// left out), with their source text and, for a form field, its id. They
    /// are the only places a comment may stand (docc comments apply). It
    /// renders d as a reader page whether or not d is hidden: the caller
    /// decides whether a hidden document counts.
    pub fn seen(&self, d: &'a engine::Doc) -> Vec<Seen> {
        let pd = self.variant(true, false, true).build_page(d).pd;
        let props = pd.props.into_iter().map(|p| Seen { p: p.p, s: p.s, input: None });
        let blocks = pd.blocks.into_iter().filter(|b| b.head.is_none()).map(|b| Seen { p: b.p, s: b.s, input: b.i.map(|i| i.id) });
        props.chain(blocks).filter(|v| !v.p.is_empty()).collect()
    }

    /// project_info is the project part of the output; title is the title of
    /// the first page ("" when nothing was compiled).
    pub fn project_info(&self, title: String) -> ProjectInfo {
        let s = self;
        let brand = s.p.cfg.brand.as_ref().map(|b| {
            let some = |v: &str| Some(v.to_string()).filter(|v| !v.is_empty());
            let logo = some(&b.logo).and_then(|l| {
                let data = s.p.cfg.source.read(&s.p.cfg.abs(&l)).ok()?;
                let mime = engine::image_ext(&path_ext(&l).to_ascii_lowercase()).unwrap_or("");
                Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(data)))
            });
            BrandInfo {
                name: b.name.clone(),
                url: some(&b.url),
                logo,
                accent: some(&b.accent),
                accent_light: some(&b.accent_light),
                header: some(&b.header),
            }
        });
        ProjectInfo { title, lang: s.p.cfg.lang.clone(), brand }
    }

    fn tree_entries(&self, ns: &[TreeNode<'a>]) -> Vec<TreeEntry> {
        ns.iter()
            .map(|n| {
                let children = self.tree_entries(n.kids());
                let title = self.node_title(n);
                match n {
                    TreeNode::Page { d, .. } => TreeEntry::Page { id: d.id.clone(), path: d.rel.clone(), title, hidden: d.hidden, children },
                    TreeNode::Folder { path, page, .. } => {
                        TreeEntry::Folder {
                            path: path.clone(),
                            title,
                            id: page.map(|d| d.id.clone()),
                            hidden: page.is_some_and(|d| d.hidden),
                            children,
                        }
                    }
                }
            })
            .collect()
    }
}
