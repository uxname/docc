use super::project::{Doc, Project};
use super::strings::{resolve, service_field};
use crate::config::go_quote;

impl Project {
    /// checkMarkdown runs DOC308-DOC312.
    pub(super) fn check_markdown(&self) {
        let res = self.cfg.resolve();
        let (on308, on309, on310, on312) = (
            res.on("DOC308"),
            res.on("DOC309"),
            res.on("DOC310"),
            res.on("DOC312"),
        );
        for d in &self.docs {
            if !d.parsed() {
                continue;
            }
            for sn in d.strings() {
                if !sn.is_text() {
                    continue;
                }
                let sc = sn.scan();
                let node = sn.node();
                let s = node.str.as_str();
                if on308 && let Some(fence) = sc.unclosed_fence {
                    let end = match s[fence..].find('\n') {
                        Some(nl) => fence + nl,
                        None => s.len(),
                    };
                    let (a, b) = d.range(node, fence, end);
                    self.emit(
                        d,
                        "DOC308",
                        a,
                        b,
                        "a code fence is opened but never closed".into(),
                        "Close the block with a matching fence line.".into(),
                    );
                }
                if on309 {
                    for h in sc.headings() {
                        let n = s.as_bytes()[h.start..]
                            .iter()
                            .take_while(|&&c| c == b'#')
                            .count();
                        let (a, b) = d.range(node, h.start, h.start + n);
                        self.emit(
                            d,
                            "DOC309",
                            a,
                            b,
                            "a heading line inside a field breaks the structure of the output".into(),
                            "Remove the leading # characters; the field name already becomes a heading.".into(),
                        );
                    }
                }
                if on310 {
                    for tg in sc.html_tags() {
                        let (a, b) = d.range(node, tg.span.start, tg.span.end);
                        self.emit(
                            d,
                            "DOC310",
                            a,
                            b,
                            format!("raw HTML tag <{}> in the text", tg.name),
                            "Use Markdown instead of HTML; scripts are never allowed.".into(),
                        );
                    }
                    for l in sc.script_links() {
                        let (a, b) = d.range(node, l.start, l.end);
                        self.emit(
                            d,
                            "DOC310",
                            a,
                            b,
                            format!("link that runs script or embeds a page: {}", l.inner),
                            "Link to an https: URL or to a document with [[id]]; docc never rewrites the text, so the unsafe link stays in the generated Markdown.".into(),
                        );
                    }
                }
            }
            if on312 && self.in_graph(d) {
                self.anchors(d);
            }
        }
    }

    /// anchors implements DOC312 for [[id#field]] in text and notes.
    fn anchors(&self, d: &Doc) {
        for r in &d.refs {
            if !r.ok || r.field.is_empty() {
                continue;
            }
            let Some(t) = self.doc(&r.target).filter(|t| t.parsed()) else {
                continue; // DOC202 or DOC101 speaks
            };
            let (msg, hint) = fragment_problem(t, &r.target, &r.field);
            if !msg.is_empty() {
                self.emit(d, "DOC312", r.start, r.end, msg, hint);
            }
        }
    }
}

/// fragmentProblem explains why the fragment of [[id#field]] or of a code tag
/// does not resolve in the parsed document t; empty when it does.
pub(super) fn fragment_problem(t: &Doc, id: &str, field: &str) -> (String, String) {
    if let Some(name) = field.strip_prefix('^') {
        if !t.blocks.contains_key(name) {
            if t.legacy {
                return (
                    format!("document {} has no block anchor {}", go_quote(id), go_quote(&format!("^{name}"))),
                    format!("End a paragraph or list item of the target with \" ^{name}\", or fix the name."),
                );
            }
            return (
                format!("document {} has no block with the code {}", go_quote(id), go_quote(&format!("^{name}"))),
                "The block was deleted or moved to another document, or the code is mistyped: find the block you mean and link its code (a code is the key of its document and a number, such as ^PAY-14).".into(),
            );
        }
    } else if service_field(field) {
        return (
            format!(
                "{} is a service field and cannot be an anchor",
                go_quote(field)
            ),
            "Point to a content field such as body or a free field.".into(),
        );
    } else if resolve(t.r(), field).is_none() {
        return (
            format!("document {} has no field {}", go_quote(id), go_quote(field)),
            "Use an existing top-level field or field/section.".into(),
        );
    }
    (String::new(), String::new())
}
