//! The readers' comments ("$comments") in the output: under their blocks in
//! the Markdown, in the data of the HTML file and of docc compile.

use std::collections::HashMap;

use crate::comments::{self, Comment, Place};
use crate::engine::Doc;
use crate::loader::Node;
use crate::schema;

use super::render::{Renderer, root, title_of};

impl<'a> Renderer<'a> {
    /// comments_of is every comment of d with its place; none when the build
    /// leaves the comments out (--no-comments).
    pub(super) fn comments_of(&self, d: &Doc) -> Vec<(Comment, Place)> {
        if !self.comments {
            return Vec::new();
        }
        let title = |id: &str| self.p.doc(id).map(title_of);
        comments::of(root(d))
            .into_iter()
            .map(|c| {
                let pl = comments::place(root(d), &c, &title).0;
                (c, pl)
            })
            .collect()
    }

    /// md_comments renders the comments of d for its Markdown page: the
    /// comments of each block by the address of its node, and the list of the
    /// comments that lost their place ("" for none).
    pub(super) fn md_comments(&self, d: &'a Doc) -> (HashMap<*const Node, String>, String) {
        let mut by: HashMap<*const Node, Vec<String>> = HashMap::new();
        let mut lost = Vec::new();
        for (c, pl) in self.comments_of(d) {
            let n = crate::comments::pointer(root(d), &c.at).and_then(|at| schema::resolve_pointer(root(d), &at));
            match n {
                Some(n) if pl != Place::Lost => by.entry(n as *const Node).or_default().push(self.md_comment(&c, n)),
                _ => lost.push(format!("> - {}", self.md_comment_line(&c, None).replace('\n', "\n>   "))),
            }
        }
        let by = by.into_iter().map(|(k, v)| (k, v.join("\n>\n"))).collect();
        let lost = if lost.is_empty() {
            String::new()
        } else {
            format!("> 💬 {}\n>\n{}", self.t("Comments that lost their place:"), lost.join("\n"))
        };
        (by, lost)
    }

    /// md_comment is one comment under its block, as a quote.
    fn md_comment(&self, c: &Comment, n: &Node) -> String {
        format!("> 💬 {}", self.md_comment_line(c, n.get("label").map(|l| l.str.as_str())).replace('\n', "\n> "))
    }

    /// md_comment_line is "**Author**, date, on «words»: text"; an answer is
    /// "**Author**, date, answer to «question»: the answer".
    fn md_comment_line(&self, c: &Comment, label: Option<&str>) -> String {
        let who = if c.author.trim().is_empty() { self.t("Anonymous") } else { md_escape(c.author.trim()) };
        let mut s = format!("**{who}**, {}", c.created.chars().take(10).collect::<String>());
        if !c.fragment.is_empty() {
            s += &format!(", {} «{}»", self.t("on"), md_escape(&c.fragment));
        }
        match (&c.answer, label) {
            (Some(a), Some(q)) => s += &format!(", {} «{}»: {}", self.t("answer to"), md_escape(q), md_escape(&a.join(", "))),
            (Some(a), None) => s += &format!(", {}: {}", self.t("answer"), md_escape(&a.join(", "))),
            (None, _) => s += &format!(": {}", md_escape(c.text.as_deref().unwrap_or_default())),
        }
        s
    }
}

/// md_escape keeps a reader's words from turning into HTML in the Markdown.
fn md_escape(s: &str) -> String {
    s.replace('<', "&lt;")
}
