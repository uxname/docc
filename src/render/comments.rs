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
        format!("> 💬 {}", self.md_comment_line(c, Some(n)).replace('\n', "\n> "))
    }

    /// md_comment_line is "**Author**, date, on «words»: text"; an answer is
    /// "**Author**, date, answer to «question»: the answer — note" (the
    /// question when the block n of the comment is its field).
    fn md_comment_line(&self, c: &Comment, n: Option<&Node>) -> String {
        let label = n.and_then(|n| n.get("label")).map(|l| l.str.as_str());
        let names: Vec<String> = n.and_then(|n| n.get("labels")).map(|l| l.items.iter().map(|i| i.str.clone()).collect()).unwrap_or_default();
        let who = if c.author.trim().is_empty() { self.t("Anonymous") } else { md_escape(c.author.trim()) };
        let mut s = format!("**{who}**, {}", c.created.chars().take(10).collect::<String>());
        if !c.fragment.is_empty() {
            s += &format!(", {} «{}»", self.t("on"), md_escape(&c.fragment));
        }
        if c.input.is_none() {
            return s + &format!(": {}", md_escape(c.text.as_deref().unwrap_or_default()));
        }
        let mut a = match (&c.flag, &c.answer, &c.ratings) {
            (Some(f), _, _) => self.flag(f),
            (_, Some(a), _) => md_escape(&a.join(", ")),
            (_, _, Some(r)) => r
                .iter()
                .map(|r| {
                    let mut x = format!("{}: {}", md_escape(&r.option), self.rating(&r.rating, &names));
                    if let Some(n) = &r.note {
                        x += &format!(" — {}", md_escape(n));
                    }
                    x
                })
                .collect::<Vec<_>>()
                .join("; "),
            _ => String::new(),
        };
        if let Some(n) = &c.note {
            a += &format!(" — {}", md_escape(n));
        }
        match label {
            Some(q) => s + &format!(", {} «{}»: {a}", self.t("answer to"), md_escape(q)),
            None => s + &format!(", {}: {a}", self.t("answer")),
        }
    }

    /// flag is the flag of an answer in words; an unknown flag as itself.
    fn flag(&self, f: &str) -> String {
        match crate::comments::flag_words(f) {
            "" => f.to_string(),
            w => self.t(w),
        }
    }

    /// rating is a rating of an item in words: the field's own names (labels),
    /// else the default ones.
    fn rating(&self, r: &str, names: &[String]) -> String {
        if let (Some(i), 3) = (crate::comments::RATINGS.iter().position(|x| *x == r), names.len()) {
            return md_escape(&names[i]);
        }
        match r {
            "like" => self.t("Like"),
            "unsure" => self.t("Not sure"),
            "dislike" => self.t("Dislike"),
            _ => r.to_string(),
        }
    }
}

/// md_escape keeps a reader's words from turning into HTML in the Markdown.
fn md_escape(s: &str) -> String {
    s.replace('<', "&lt;")
}
