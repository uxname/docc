use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::engine::Doc;
use crate::loader::Kind;

use super::page::{Entry, reserved};
use super::render::{Renderer, one_line, root};
use super::text::Page;

/// statusClass picks the badge colour for a status value by its common meaning.
pub(super) fn status_class(s: &str) -> &'static str {
    match go_lower(s).as_str() {
        "accepted" | "approved" | "done" | "implemented" | "stable" | "active" | "resolved"
        | "verified" => "ok",
        "draft" | "proposed" | "open" | "planned" | "review" | "in-progress" | "wip"
        | "pending" => "warn",
        "rejected" | "deprecated" | "obsolete" | "superseded" | "blocked" | "wontfix" => "bad",
        _ => "",
    }
}

/// statusRu names the usual statuses in Russian on the badge of the HTML page;
/// other values are shown as written.
fn status_ru(s: &str) -> Option<&'static str> {
    Some(match s {
        "accepted" => "принято",
        "approved" => "согласовано",
        "done" => "готово",
        "implemented" => "реализовано",
        "stable" | "active" => "действует",
        "resolved" => "решено",
        "verified" => "проверено",
        "draft" => "черновик",
        "proposed" => "предложено",
        "open" => "открыто",
        "planned" => "запланировано",
        "review" => "на согласовании",
        "in-progress" | "wip" => "в работе",
        "pending" => "ожидает",
        "rejected" => "отклонено",
        "deprecated" | "obsolete" => "устарело",
        "superseded" => "заменено",
        "blocked" => "заблокировано",
        "wontfix" => "не будет сделано",
        _ => return None,
    })
}

impl<'a> Renderer<'a> {
    pub(super) fn status_label(&self, s: &str) -> String {
        if let Some(v) = status_ru(&go_lower(s))
            && self.p.cfg.lang == "ru"
        {
            return v.to_string();
        }
        s.to_string()
    }

    /// summary is the first body paragraph (or free text field) as plain text, cut to about 300 characters.
    pub(super) fn summary(&self, d: &'a Doc) -> String {
        let mut pg = Page::new(self, d);
        let mut body = root(d).get("body");
        if body.is_none() {
            // no body: the first free text field stands in for it
            body = root(d)
                .members
                .iter()
                .find(|m| !reserved(&m.key) && m.value.kind == Kind::String)
                .map(|m| &m.value);
        }
        let Some(body) = body else {
            return String::new();
        };
        let ps = pg.paragraphs(body, true);
        let Some(first) = ps.first() else {
            return String::new();
        };
        let s = MD_LINK.replace_all(first, "$1");
        let s = HTML_NOISE.replace_all(&s, "");
        let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if s.chars().count() > 300 {
            let mut cut: String = s.chars().take(300).collect();
            // a byte index, as in Go
            if let Some(i) = cut.rfind(' ')
                && i > 200
            {
                cut.truncate(i);
            }
            s = cut.trim().to_string() + "…";
        }
        s
    }
}

/// go_lower is strings.ToLower for the comparisons above: Go maps İ to a
/// plain i, Rust adds a combining dot.
fn go_lower(s: &str) -> String {
    s.replace('İ', "i").to_lowercase()
}

pub(super) fn str_field(d: &Doc, key: &str) -> String {
    match root(d).get(key) {
        Some(v) if matches!(v.kind, Kind::String | Kind::Number) => one_line(&v.str),
        _ => String::new(),
    }
}

static MD_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap());
static HTML_NOISE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--.*?-->|<[^>]*>|[*`]").unwrap());

/// childrenOf maps a document id to its children, read from tree entries.
pub(super) fn children_of<'a>(lists: &[&[Entry<'a>]]) -> HashMap<&'a str, Vec<&'a Doc>> {
    let mut out: HashMap<&'a str, Vec<&'a Doc>> = HashMap::new();
    for es in lists {
        let mut stack: Vec<&'a Doc> = Vec::new();
        for e in *es {
            stack.truncate(e.depth);
            stack.push(e.d);
            if e.depth > 0 {
                let p = stack[e.depth - 1];
                out.entry(p.id.as_str()).or_default().push(e.d);
            }
        }
    }
    out
}
