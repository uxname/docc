//! markdown turns the Markdown of a page into HTML as the Go build's goldmark
//! setup does: GFM (tables, strikethrough, linkify, task lists), definition
//! lists, footnotes with ids prefixed by the page id, heading ids from the ids
//! generator, no raw HTML except <br>, the field anchors and the block markers
//! of the page. comrak parses; the HTML is written here node by node with
//! goldmark's renderer rules, because comrak's own output differs in many
//! small ways (<br />, footnote sections, heading anchors, URL escaping).

use std::collections::HashMap;
use std::sync::LazyLock;

use comrak::arena_tree::NodeEdge;
use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Options, parse_document};
use regex::Regex;

use super::Ids;

type Node<'a> = &'a AstNode<'a>;

/// OPTIONS: GFM without comrak's autolinks and task lists, which follow
/// goldmark's rules here (linkify, check_box); escaped characters stay
/// visible as nodes, because linkify looks at the source text.
pub(super) static OPTIONS: LazyLock<Options<'static>> = LazyLock::new(|| {
    let mut o = Options::default();
    o.extension.strikethrough = true;
    o.extension.table = true;
    o.extension.footnotes = true;
    o.extension.description_lists = true;
    o.parse.escaped_char_spans = true;
    o.parse.leave_footnote_definitions = true;
    o
});

/// convert renders src. fn_prefix starts the footnote ids, marker matches the
/// block markers that pass as HTML blocks, ids gives the heading ids.
///
/// Where comrak and goldmark parse a character differently, comrak gets it
/// hidden: replaced by a control byte it takes as text (one byte too, so the
/// columns stay), given back in the output. Some of these places are only
/// known once the text is parsed (a bare URL that goldmark's linkify reads
/// over a ~ or a `, a ':' that is indented in its container), so the
/// rendering marks them and the text is parsed again until nothing new is
/// marked.
pub(super) fn convert(src: &str, fn_prefix: &str, marker: &Regex, ids: &mut Ids) -> String {
    // comrak misreads a last line without a line end (a link reference
    // definition with an <angle> destination, the end of a code fence) where
    // goldmark does not; with the line end both agree
    let owned;
    let added = !src.is_empty() && !src.ends_with(['\n', '\r']);
    let src = if added {
        owned = format!("{src}\n");
        owned.as_str()
    } else {
        src
    };
    // a text that already holds the control bytes is parsed as it is
    let hiding = !src.bytes().any(|c| (1..=HIDDEN.len() as u8).contains(&c));
    let mut parsed = src.as_bytes().to_vec();
    if hiding {
        // comrak also takes "~ " at the start of a line as a description
        // marker, goldmark only ":"; such a tilde is text for both
        for m in TILDE_ITEM.captures_iter(src) {
            hide(&mut parsed, m.get(1).unwrap().end());
        }
    }
    for pass in 0.. {
        let arena = Arena::new();
        let text = std::str::from_utf8(&parsed).expect("hiding keeps UTF-8");
        let root = parse_document(&arena, text, &OPTIONS);
        let mut g = ids.clone();
        let mut c = Conv {
            src,
            lines: split_lines(src),
            fn_prefix,
            marker,
            heading_ids: HashMap::new(),
            notes: HashMap::new(),
            out: String::new(),
            hiding,
            hide: Vec::new(),
            added,
        };
        // goldmark generates the ids as the headings close: in source order
        let mut hs: Vec<Node<'_>> = root
            .descendants()
            .filter(|n| matches!(n.data().value, NodeValue::Heading(_)))
            .collect();
        hs.sort_by_key(|n| n.data().sourcepos.start.line);
        for h in hs {
            let raw = c.heading_raw(h);
            let id = g.generate(&raw);
            c.heading_ids.insert(h as *const AstNode<'_>, id);
        }
        c.block(root);
        let mut more = false;
        for at in c.hide {
            more |= hide(&mut parsed, at);
        }
        // ponytail: every pass hides at least one more byte; the cap only
        // guards against a text that keeps finding new ones
        if !more || pass == 16 {
            *ids = g;
            return if hiding { unhide(&c.out) } else { c.out };
        }
    }
    unreachable!()
}

/// HIDDEN are the characters convert can hide from comrak; the one at index
/// i becomes the byte i+1.
const HIDDEN: &[u8] = b"~:`[]_!*";

/// hide replaces the character at byte at of b by its control byte; it
/// reports whether it was not hidden yet.
fn hide(b: &mut [u8], at: usize) -> bool {
    match HIDDEN.iter().position(|&c| c == b[at]) {
        Some(i) => {
            b[at] = i as u8 + 1;
            true
        }
        None => false,
    }
}

/// unhide gives the hidden characters back.
fn unhide(s: &str) -> String {
    if !s.bytes().any(|c| (1..=HIDDEN.len() as u8).contains(&c)) {
        return s.to_string();
    }
    s.chars()
        .map(|c| match c as u32 {
            n @ 1..=8 => HIDDEN[n as usize - 1] as char,
            _ => c,
        })
        .collect()
}

static TILDE_ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^([ \t>]*)~[ \t]").unwrap());

/// split_lines cuts src at \n, \r\n and \r, as comrak counts lines.
fn split_lines(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let (mut start, mut i) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'\n' => {
                out.push(&src[start..i]);
                start = i + 1;
            }
            b'\r' => {
                out.push(&src[start..i]);
                if i + 1 < b.len() && b[i + 1] == b'\n' {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&src[start..]);
    out
}

struct Conv<'s> {
    src: &'s str,
    lines: Vec<&'s str>,
    fn_prefix: &'s str,
    marker: &'s Regex,
    heading_ids: HashMap<*const AstNode<'s>, String>,
    /// footnote references (by address): index and number of the reference
    notes: HashMap<usize, (usize, usize)>,
    out: String,
    /// hiding: comrak got the text with hidden characters
    hiding: bool,
    /// hide collects the bytes of src to hide in the next pass
    hide: Vec<usize>,
    /// added: convert added the line end of the last line
    added: bool,
}

/// is_space is goldmark's util.IsSpace: space, tab, \n, \r.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r')
}

/// is_punct is goldmark's util.IsPunct: ASCII punctuation.
fn is_punct(c: u8) -> bool {
    c.is_ascii_punctuation()
}

fn trim_space(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_ascii() && is_space(c as u8))
}

/// escape_html is goldmark's util.EscapeHTML: ", &, < and >.
fn escape_html(s: &str) -> String {
    let mut b = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => b.push_str("&quot;"),
            '&' => b.push_str("&amp;"),
            '<' => b.push_str("&lt;"),
            '>' => b.push_str("&gt;"),
            '\0' => b.push('\u{FFFD}'),
            _ => b.push(c),
        }
    }
    b
}

/// url_escape is goldmark's util.URLEscape on a resolved URL: characters
/// outside the URL set are percent-encoded, an existing %XX stays.
fn url_escape(v: &str) -> String {
    fn keep(c: u8) -> bool {
        c.is_ascii_alphanumeric() || b"!#$&'()*+,-./:;=?@_~".contains(&c)
    }
    let b = v.as_bytes();
    let mut out = String::with_capacity(v.len());
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if keep(c) {
            out.push(c as char);
            i += 1;
            continue;
        }
        if c == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&v[i..i + 3]);
            i += 3;
            continue;
        }
        if c == b' ' {
            out.push_str("%20");
            i += 1;
            continue;
        }
        let ch = v[i..].chars().next().unwrap();
        let mut buf = [0u8; 4];
        for x in ch.encode_utf8(&mut buf).bytes() {
            out.push_str(&format!("%{x:02X}"));
        }
        i += ch.len_utf8();
    }
    out
}

/// is_dangerous_url is goldmark's html.IsDangerousURL.
fn is_dangerous_url(url: &str) -> bool {
    let has = |s: &str, p: &str| {
        s.len() >= p.len() && s.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
    };
    if has(url, "data:image/") && url.len() >= 11 {
        let v = &url[11..];
        return !(has(v, "png;") || has(v, "gif;") || has(v, "jpeg;") || has(v, "webp;"));
    }
    has(url, "javascript:") || has(url, "vbscript:") || has(url, "file:") || has(url, "data:")
}

/// ANCHOR_OPEN matches the anchors <a id="x"> that mark field targets.
static ANCHOR_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^<a id="[A-Za-z0-9_.-]+">$"#).unwrap());

/// raw_inline is brOnly for raw inline HTML: <br> (table cells use it for line
/// breaks) and the anchors <a id="x"> and </a> pass, anything else is dropped.
fn raw_inline(tag: &str) -> String {
    let t = tag.replace(' ', "").to_lowercase();
    if t == "<br>" || t == "<br/>" {
        return "<br>".into();
    }
    if t == "</a>" || ANCHOR_OPEN.is_match(tag) {
        return tag.into();
    }
    "<!-- raw HTML omitted -->".into()
}

/// TASK is goldmark's task list check box at the start of the first line of a
/// list item: [ ], [x] or [X], then any white space.
static TASK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([\t\n\f\r xX])\][\t\n\f\r ]*").unwrap());

impl<'s> Conv<'s> {
    /// heading_raw is the text goldmark gives the ids generator: the last line
    /// of the heading as written, without the # marks.
    fn heading_raw<'a>(&self, n: Node<'a>) -> String {
        let NodeValue::Heading(h) = &n.data().value else {
            return String::new();
        };
        let sp = n.data().sourcepos;
        if !h.setext {
            let line = self.lines.get(sp.start.line - 1).copied().unwrap_or("");
            let mut i = (sp.start.column.max(1) - 1).min(line.len());
            let b = line.as_bytes();
            while i < b.len() && b[i] == b' ' {
                i += 1;
            }
            while i < b.len() && b[i] == b'#' {
                i += 1;
            }
            let rest = trim_space(&line[i..]);
            let b = rest.as_bytes();
            if b.is_empty() {
                return String::new();
            }
            // the closing sequence of '#'
            let mut stop = b.len();
            let mut j = stop - 1;
            while b[j] == b'#' && j > 0 {
                j -= 1;
            }
            if j == 0 && b[0] == b'#' {
                return String::new();
            }
            if j != stop - 1 && is_space(b[j]) {
                stop = j;
                while stop > 0 && is_space(b[stop - 1]) {
                    stop -= 1;
                }
            }
            return rest[..stop].to_string();
        }
        // setext: the last line of the paragraph, the line before the underline
        let ln = sp.end.line.saturating_sub(1).max(sp.start.line);
        let line = self.lines.get(ln - 1).copied().unwrap_or("");
        let mut s = if ln == sp.start.line {
            &line[(sp.start.column.max(1) - 1).min(line.len())..]
        } else {
            line
        };
        if ln != sp.start.line {
            // a continuation line still carries the > of its block quotes
            let quotes = n
                .ancestors()
                .filter(|a| matches!(a.data().value, NodeValue::BlockQuote))
                .count();
            for _ in 0..quotes {
                let t = s.trim_start_matches([' ', '\t']);
                match t.strip_prefix('>') {
                    Some(r) => s = r,
                    None => break,
                }
            }
        }
        trim_space(s).to_string()
    }

    fn block<'a>(&mut self, n: Node<'a>) {
        let v = n.data().value.clone();
        match v {
            NodeValue::Document => {
                let notes = self.number_notes(n);
                self.children(n);
                if !notes.is_empty() {
                    self.out
                        .push_str("<div class=\"footnotes\" role=\"doc-endnotes\">\n<hr>\n<ol>\n");
                    for (index, refs, d) in notes {
                        self.footnote(d, index, refs);
                    }
                    self.out.push_str("</ol>\n</div>\n");
                }
            }
            // drawn at the end of the document
            NodeValue::FootnoteDefinition(_) => {}
            NodeValue::Heading(h) => {
                let id = self
                    .heading_ids
                    .get(&(n as *const AstNode<'a> as *const AstNode<'s>))
                    .cloned()
                    .unwrap_or_default();
                self.out
                    .push_str(&format!("<h{} id=\"{}\">", h.level, escape_html(&id)));
                self.inlines(n);
                self.out.push_str(&format!("</h{}>\n", h.level));
            }
            NodeValue::Paragraph => {
                if self.text_block(n) {
                    self.inlines(n);
                    if n.next_sibling().is_some() && n.first_child().is_some() {
                        self.out.push('\n');
                    }
                } else {
                    self.out.push_str("<p>");
                    self.inlines(n);
                    self.out.push_str("</p>\n");
                }
            }
            NodeValue::BlockQuote => {
                self.out.push_str("<blockquote>\n");
                self.children(n);
                self.out.push_str("</blockquote>\n");
            }
            NodeValue::CodeBlock(cb) => {
                self.out.push_str("<pre><code");
                if cb.fenced && !cb.info.is_empty() {
                    let lang = cb.info.split(' ').next().unwrap_or("");
                    self.out
                        .push_str(&format!(" class=\"language-{}\"", escape_html(lang)));
                }
                self.out.push('>');
                self.out.push_str(&escape_html(&cb.literal));
                self.out.push_str("</code></pre>\n");
            }
            NodeValue::HtmlBlock(hb) => {
                // a block marker of the page passes; any other HTML block is dropped
                let mut raw = hb.literal.as_str();
                // the block marker of the last line needs its own line end
                if self.added && n.data().sourcepos.end.line + 1 >= self.lines.len() {
                    raw = raw.strip_suffix('\n').unwrap_or(raw);
                }
                match self.marker.find(raw) {
                    Some(m) if raw.trim().len() == m.as_str().trim().len() => {
                        self.out.push_str(raw.trim());
                        self.out.push('\n');
                    }
                    _ => self.out.push_str("<!-- raw HTML omitted -->\n"),
                }
            }
            NodeValue::ThematicBreak => self.out.push_str("<hr>\n"),
            NodeValue::List(l) => {
                let tag = if l.list_type == ListType::Ordered {
                    "ol"
                } else {
                    "ul"
                };
                self.out.push('<');
                self.out.push_str(tag);
                if l.list_type == ListType::Ordered && l.start != 1 {
                    self.out.push_str(&format!(" start=\"{}\"", l.start));
                }
                self.out.push_str(">\n");
                self.children(n);
                self.out.push_str(&format!("</{tag}>\n"));
            }
            NodeValue::Item(_) => {
                self.out.push_str("<li>");
                if let Some(fc) = n.first_child()
                    && !self.text_block(fc)
                {
                    self.out.push('\n');
                }
                self.children(n);
                self.out.push_str("</li>\n");
            }
            NodeValue::Table(t) => {
                self.out.push_str("<table>\n");
                let rows: Vec<Node<'a>> = n.children().collect();
                for (i, r) in rows.iter().enumerate() {
                    let head = matches!(r.data().value, NodeValue::TableRow(true));
                    if head {
                        self.out.push_str("<thead>\n<tr>\n");
                    } else {
                        self.out.push_str("<tr>\n");
                    }
                    let tag = if head { "th" } else { "td" };
                    let mut prev_end = 0;
                    for (j, cell) in r.children().enumerate() {
                        self.out.push('<');
                        self.out.push_str(tag);
                        // goldmark pads a short row with cells that have no alignment;
                        // comrak's padding cells start right after the last real cell
                        let sp = cell.data().sourcepos;
                        let padded = j > 0 && sp.start.column == prev_end + 1 && sp.start == sp.end;
                        if !padded {
                            prev_end = sp.end.column;
                        }
                        match t.alignments.get(j).filter(|_| !padded) {
                            Some(TableAlignment::Left) => {
                                self.out.push_str(" style=\"text-align:left\"")
                            }
                            Some(TableAlignment::Center) => {
                                self.out.push_str(" style=\"text-align:center\"")
                            }
                            Some(TableAlignment::Right) => {
                                self.out.push_str(" style=\"text-align:right\"")
                            }
                            _ => {}
                        }
                        self.out.push('>');
                        self.inlines(cell);
                        self.out.push_str(&format!("</{tag}>\n"));
                    }
                    if head {
                        self.out.push_str("</tr>\n</thead>\n");
                        if i + 1 < rows.len() {
                            self.out.push_str("<tbody>\n");
                        }
                    } else {
                        self.out.push_str("</tr>\n");
                        if i + 1 == rows.len() {
                            self.out.push_str("</tbody>\n");
                        }
                    }
                }
                self.out.push_str("</table>\n");
            }
            NodeValue::DescriptionList => {
                self.out.push_str("<dl>\n");
                let mut prev: Option<Node<'a>> = None;
                for c in n.children() {
                    // a link reference definition between two items ends the list
                    // for goldmark; comrak drops it and goes on
                    if let Some(p) = prev {
                        let (a, b) = (p.data().sourcepos.end.line, c.data().sourcepos.start.line);
                        if (a + 1..b).any(|l| {
                            !self
                                .source_at(l, 1)
                                .trim_start_matches([' ', '\t', '>'])
                                .trim()
                                .is_empty()
                        }) {
                            self.out.push_str("</dl>\n<dl>\n");
                        }
                    }
                    self.block(c);
                    prev = Some(c);
                }
                self.out.push_str("</dl>\n");
            }
            NodeValue::DescriptionItem(_) => self.children(n),
            NodeValue::DescriptionTerm => {
                // goldmark makes every line of the term paragraph a term of its own
                for p in n.children() {
                    let mut line: Vec<Node<'a>> = Vec::new();
                    let kids: Vec<Node<'a>> = p.children().collect();
                    for (i, k) in kids.iter().enumerate() {
                        let brk =
                            matches!(k.data().value, NodeValue::SoftBreak | NodeValue::LineBreak);
                        if !brk {
                            line.push(k);
                        }
                        if brk || i + 1 == kids.len() {
                            self.out.push_str("<dt>");
                            self.inline_seq(&line, &mut 0, None);
                            // a backslash at the end of a term line is no line break there
                            let src = self.source_at(k.data().sourcepos.start.line, 1);
                            if matches!(k.data().value, NodeValue::LineBreak) && src.ends_with('\\')
                            {
                                self.out.push('\\');
                            }
                            self.out.push_str("</dt>\n");
                            line.clear();
                        }
                    }
                }
            }
            NodeValue::DescriptionDetails => {
                self.indented_marker(n);
                if self.tight_details(n) {
                    self.out.push_str("<dd>");
                } else {
                    self.out.push_str("<dd>\n");
                }
                self.children(n);
                self.out.push_str("</dd>\n");
            }
            _ => self.children(n),
        }
    }

    /// number_notes numbers the footnotes as goldmark does: a reference is
    /// resolved when its label matches a definition exactly (the first one);
    /// the inline text is read in document order, the definitions all at the
    /// place of the first one, so references inside a definition count too,
    /// even when nothing refers to that definition. It returns the referenced
    /// definitions by index with their number of references.
    fn number_notes<'a>(&mut self, root: Node<'a>) -> Vec<(usize, usize, Node<'a>)> {
        let mut defs: Vec<Node<'a>> = Vec::new();
        let mut first: HashMap<String, Node<'a>> = HashMap::new();
        for d in root.descendants() {
            if let NodeValue::FootnoteDefinition(fd) = &d.data().value {
                defs.push(d);
                first.entry(fd.name.clone()).or_insert(d);
            }
        }
        let mut order: Vec<Node<'a>> = Vec::new();
        let mut defs_done = false;
        let mut walk = root.traverse();
        while let Some(edge) = walk.next() {
            let NodeEdge::Start(n) = edge else {
                continue;
            };
            match &n.data().value {
                NodeValue::FootnoteDefinition(_) => {
                    if !defs_done {
                        defs_done = true;
                        for d in &defs {
                            order.extend(d.descendants().filter(|x| {
                                matches!(x.data().value, NodeValue::FootnoteReference(_))
                            }));
                        }
                    }
                    // the definition was read above: skip its contents
                    for e in walk.by_ref() {
                        if matches!(e, NodeEdge::End(x) if std::ptr::eq(x, n)) {
                            break;
                        }
                    }
                }
                NodeValue::FootnoteReference(_) => order.push(n),
                _ => {}
            }
        }
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut count: HashMap<String, usize> = HashMap::new();
        for r in order {
            let NodeValue::FootnoteReference(fr) = &r.data().value else {
                continue;
            };
            let name = written_label(fr);
            if !first.contains_key(&name) {
                continue;
            }
            let n = index.len() + 1;
            let ix = *index.entry(name.clone()).or_insert(n);
            let c = count.entry(name).or_default();
            self.notes
                .insert(r as *const AstNode<'a> as usize, (ix, *c));
            *c += 1;
        }
        let mut out: Vec<(usize, usize, Node<'a>)> = index
            .iter()
            .map(|(name, &ix)| (ix, count[name], first[name]))
            .collect();
        out.sort_by_key(|x| x.0);
        out
    }

    /// footnote renders one footnote; the links back to its references follow
    /// its last paragraph.
    fn footnote<'a>(&mut self, n: Node<'a>, index: usize, refs: usize) {
        self.out
            .push_str(&format!("<li id=\"{}fn:{index}\">\n", self.fn_prefix));
        let back = self.backlinks(index, refs);
        let last_para = n
            .last_child()
            .filter(|c| matches!(c.data().value, NodeValue::Paragraph));
        for c in n.children() {
            if last_para.is_some_and(|l| std::ptr::eq(l, c)) {
                self.out.push_str("<p>");
                self.inlines(c);
                self.out.push_str(&back);
                self.out.push_str("</p>\n");
            } else {
                self.block(c);
            }
        }
        if last_para.is_none() {
            self.out.push_str(&back);
        }
        self.out.push_str("</li>\n");
    }

    fn children<'a>(&mut self, n: Node<'a>) {
        for c in n.children() {
            self.block(c);
        }
    }

    /// text_block reports whether a paragraph renders without <p>: in an item of
    /// a tight list or in a tight description.
    fn text_block<'a>(&self, n: Node<'a>) -> bool {
        if !matches!(n.data().value, NodeValue::Paragraph) {
            return false;
        }
        let Some(p) = n.parent() else { return false };
        match &p.data().value {
            NodeValue::Item(_) => p
                .parent()
                .is_some_and(|l| matches!(&l.data().value, NodeValue::List(nl) if nl.tight)),
            // goldmark's loop over a tight description stops after the first paragraph
            // it replaces, so only that one loses its <p>
            NodeValue::DescriptionDetails => {
                self.tight_details(p)
                    && p.children()
                        .find(|c| matches!(c.data().value, NodeValue::Paragraph))
                        .is_some_and(|f| std::ptr::eq(f, n))
            }
            _ => false,
        }
    }

    /// indented_marker hides the ':' of a description that is indented in its
    /// container: goldmark starts a description only at indent 0, so the line
    /// goes on the paragraph before it.
    fn indented_marker<'a>(&mut self, n: Node<'a>) {
        let Some(c) = n.parent().and_then(|i| i.parent()).and_then(|l| l.parent()) else {
            return;
        };
        let sp = n.data().sourcepos.start;
        let (Some(line), Some(from)) = (
            self.lines.get(sp.line.wrapping_sub(1)),
            self.content_col(c, sp.line),
        ) else {
            return;
        };
        let colon = sp.column.saturating_sub(1);
        if colon > from
            && line.as_bytes().get(colon) == Some(&b':')
            && line.as_bytes()[from..colon].iter().all(|&b| b == b' ')
        {
            self.hide
                .push(line.as_ptr() as usize - self.src.as_ptr() as usize + colon);
        }
    }

    /// content_col is the byte of line where the content of container n
    /// starts; None where it is not known (lazy lines, tabs, other containers).
    // ponytail: a description is measured on its first line only.
    fn content_col<'a>(&self, n: Node<'a>, line: usize) -> Option<usize> {
        match &n.data().value {
            NodeValue::Document => Some(0),
            NodeValue::BlockQuote => {
                let mut i = self.content_col(n.parent()?, line)?;
                let b = self.lines.get(line.wrapping_sub(1))?.as_bytes();
                let mut sp = 0;
                while sp < 3 && b.get(i) == Some(&b' ') {
                    i += 1;
                    sp += 1;
                }
                if b.get(i) != Some(&b'>') {
                    return None;
                }
                i += 1;
                match b.get(i) {
                    Some(b' ') => Some(i + 1),
                    Some(b'\t') => None,
                    _ => Some(i),
                }
            }
            NodeValue::Item(l) => {
                Some(self.content_col(n.parent()?.parent()?, line)? + l.marker_offset + l.padding)
            }
            // a description: after the ':' and the spaces after it, or one space
            // when there are more than four
            NodeValue::DescriptionDetails => {
                let sp = n.data().sourcepos.start;
                let b = self.lines.get(sp.line.wrapping_sub(1))?.as_bytes();
                let colon = sp.column.checked_sub(1)?;
                let spaces = b
                    .get(colon + 1..)?
                    .iter()
                    .take_while(|&&c| c == b' ')
                    .count();
                match b.get(colon + 1 + spaces) {
                    None | Some(b'\t') => None,
                    _ if spaces > 4 => Some(colon + 2),
                    _ => Some(colon + 1 + spaces),
                }
            }
            _ => None,
        }
    }

    /// tight_details: goldmark makes a description tight when no blank line
    /// comes before its ':'.
    fn tight_details<'a>(&self, n: Node<'a>) -> bool {
        let l = n.data().sourcepos.start.line;
        l < 2 || !self.source_at(l - 1, 1).trim().is_empty()
    }

    /// backlinks are the links from a footnote back to each of its references.
    fn backlinks(&self, index: usize, refs: usize) -> String {
        let mut b = String::new();
        for i in 0..refs.max(1) {
            let ri = if i > 0 { i.to_string() } else { String::new() };
            b.push_str(&format!(
                "&#160;<a href=\"#{}fnref{ri}:{index}\" class=\"footnote-backref\" role=\"doc-backlink\">&#x21a9;&#xfe0e;</a>",
                self.fn_prefix
            ));
        }
        b
    }

    /// inlines renders the inline children of a block.
    fn inlines<'a>(&mut self, n: Node<'a>) {
        let kids: Vec<Node<'a>> = n.children().collect();
        let mut depth = 0;
        let first_of_item = n.parent().is_some_and(|p| {
            matches!(p.data().value, NodeValue::Item(_))
                && p.first_child().is_some_and(|f| std::ptr::eq(f, n))
        });
        if first_of_item && let Some((rest, lead)) = self.check_box(&kids) {
            return self.inline_seq(&rest, &mut depth, Some(lead));
        }
        self.inline_seq(&kids, &mut depth, None);
    }

    /// check_box writes goldmark's task check box when the first line of a list
    /// item starts with one: it returns the inlines and the text left after it.
    #[allow(clippy::type_complexity)]
    fn check_box<'a>(
        &mut self,
        kids: &[Node<'a>],
    ) -> Option<(Vec<Node<'a>>, (String, Option<(usize, usize)>))> {
        // the leading text as one string: comrak may cut it at the brackets
        let n = kids
            .iter()
            .take_while(|k| matches!(k.data().value, NodeValue::Text(_)))
            .count();
        let t: String = kids[..n]
            .iter()
            .map(|k| match &k.data().value {
                NodeValue::Text(t) => t.to_string(),
                _ => String::new(),
            })
            .collect();
        let m = TASK.captures(&t)?;
        let checked = matches!(&m[1], "x" | "X");
        let end = m.get(0).unwrap().end();
        let mut rest: Vec<Node<'a>> = kids[n..].to_vec();
        // \s* also eats the end of the line
        if end == t.len()
            && rest.first().is_some_and(|k| {
                matches!(k.data().value, NodeValue::SoftBreak | NodeValue::LineBreak)
            })
        {
            rest.remove(0);
        }
        self.out.push_str(if checked {
            "<input checked=\"\" disabled=\"\" type=\"checkbox\"> "
        } else {
            "<input disabled=\"\" type=\"checkbox\"> "
        });
        // the text left ends where the last text node ends in the source
        let pos = kids[..n].last().map(|k| {
            let e = k.data().sourcepos.end;
            (e.line, e.column)
        });
        Some((rest, (t[end..].to_string(), pos)))
    }

    /// inline_seq renders a list of sibling inlines; runs of text go through
    /// linkify. lead is text left over in front of the first node (after a task
    /// check box). depth counts the open [ of possible link labels, inside which
    /// goldmark does not linkify.
    fn inline_seq<'a>(
        &mut self,
        kids: &[Node<'a>],
        depth: &mut usize,
        lead: Option<(String, Option<(usize, usize)>)>,
    ) {
        let mut run: Vec<Piece> = Vec::new();
        if let Some((l, pos)) = lead
            && !l.is_empty()
        {
            run.push((l, false, pos));
        }
        for k in kids {
            match &k.data().value {
                NodeValue::Text(t) => {
                    let sp = k.data().sourcepos.end;
                    run.push((t.to_string(), false, Some((sp.line, sp.column))));
                    continue;
                }
                NodeValue::Escaped => {
                    let t: String = k
                        .children()
                        .map(|c| match &c.data().value {
                            NodeValue::Text(t) => t.to_string(),
                            _ => String::new(),
                        })
                        .collect();
                    run.push((t, true, None));
                    continue;
                }
                _ => {}
            }
            self.flush_run(&mut run, depth);
            self.inline(k, depth);
        }
        self.flush_run(&mut run, depth);
    }

    fn inline<'a>(&mut self, n: Node<'a>, depth: &mut usize) {
        let v = n.data().value.clone();
        match v {
            NodeValue::SoftBreak => {
                // goldmark keeps the white space before a line break but its last character
                let line = self.source_at(n.data().sourcepos.start.line, 1);
                let ws = &line[line.trim_end_matches([' ', '\t']).len()..];
                if ws.len() > 1 {
                    self.out.push_str(&ws[..ws.len() - 1]);
                }
                self.out.push('\n');
            }
            NodeValue::LineBreak => {
                // goldmark keeps the spaces before the last two but one of a hard break
                let line = self.source_at(n.data().sourcepos.start.line, 1);
                if line.ends_with("  ") {
                    let ws = &line[line.trim_end_matches([' ', '\t']).len()..];
                    let w = &ws[..ws.len() - 2];
                    if w.len() > 1 {
                        self.out.push_str(&w[..w.len() - 1]);
                    }
                }
                self.out.push_str("<br>\n");
            }
            NodeValue::Code(c) => {
                self.out.push_str("<code>");
                self.out.push_str(&escape_html(&c.literal));
                self.out.push_str("</code>");
            }
            NodeValue::HtmlInline(h) => self.out.push_str(&raw_inline(&h)),
            NodeValue::Emph => self.wrap(n, "em", depth),
            NodeValue::Strong => self.wrap(n, "strong", depth),
            NodeValue::Strikethrough => self.wrap(n, "del", depth),
            NodeValue::Link(l) => {
                if let Some((raw, email)) = self.autolink_raw(n) {
                    self.out.push_str("<a href=\"");
                    let url = url_escape(&raw);
                    if email && !url.to_ascii_lowercase().starts_with("mailto:") {
                        self.out.push_str("mailto:");
                    }
                    if !is_dangerous_url(&url) {
                        self.out.push_str(&escape_html(&url));
                    }
                    self.out.push_str("\">");
                    self.out.push_str(&escape_html(&raw));
                    self.out.push_str("</a>");
                    return;
                }
                self.out.push_str("<a href=\"");
                let dest = url_escape(&l.url);
                if !is_dangerous_url(&dest) {
                    self.out.push_str(&escape_html(&dest));
                }
                self.out.push('"');
                if !l.title.is_empty() || self.empty_title(n) {
                    self.out
                        .push_str(&format!(" title=\"{}\"", escape_html(&l.title)));
                }
                self.out.push('>');
                let kids: Vec<Node<'a>> = n.children().collect();
                let mut d = 1; // goldmark does not linkify inside a link label
                self.inline_seq(&kids, &mut d, None);
                self.out.push_str("</a>");
            }
            NodeValue::Image(l) => {
                self.out.push_str("<img src=\"");
                let dest = url_escape(&l.url);
                if !is_dangerous_url(&dest) {
                    self.out.push_str(&escape_html(&dest));
                }
                self.out.push_str("\" alt=\"");
                self.alt(n);
                self.out.push('"');
                if !l.title.is_empty() || self.empty_title(n) {
                    self.out
                        .push_str(&format!(" title=\"{}\"", escape_html(&l.title)));
                }
                self.out.push('>');
            }
            NodeValue::FootnoteReference(r) => {
                let Some(&(index, ri)) = self.notes.get(&(n as *const AstNode<'a> as usize)) else {
                    // no definition with exactly this label
                    self.out
                        .push_str(&escape_html(&format!("[^{}]", written_label(&r))));
                    return;
                };
                let ri = if ri > 0 {
                    ri.to_string()
                } else {
                    String::new()
                };
                let p = self.fn_prefix;
                self.out.push_str(&format!(
                    "<sup id=\"{p}fnref{ri}:{index}\"><a href=\"#{p}fn:{index}\" class=\"footnote-ref\" role=\"doc-noteref\">{index}</a></sup>"
                ));
            }
            _ => {
                let kids: Vec<Node<'a>> = n.children().collect();
                self.inline_seq(&kids, depth, None);
            }
        }
    }

    fn wrap<'a>(&mut self, n: Node<'a>, tag: &str, depth: &mut usize) {
        self.out.push_str(&format!("<{tag}>"));
        let kids: Vec<Node<'a>> = n.children().collect();
        self.inline_seq(&kids, depth, None);
        self.out.push_str(&format!("</{tag}>"));
    }

    /// empty_title reports whether an inline link or image was written with an
    /// empty title ("", '' or ()), which goldmark still prints.
    fn empty_title<'a>(&self, n: Node<'a>) -> bool {
        let sp = n.data().sourcepos;
        let line = self
            .lines
            .get(sp.end.line.wrapping_sub(1))
            .copied()
            .unwrap_or("");
        let Some(s) = line.get(..sp.end.column.min(line.len())) else {
            return false;
        };
        let Some(s) = s.strip_suffix(')').map(|s| s.trim_end_matches([' ', '\t'])) else {
            return false;
        };
        ["\"\"", "''", "()"].iter().any(|t| {
            s.strip_suffix(t)
                .is_some_and(|r| r.ends_with([' ', '\t']) || r.is_empty())
        })
    }

    /// alt is goldmark's renderTexts: the text of the image description, line
    /// breaks kept, markup dropped.
    fn alt<'a>(&mut self, n: Node<'a>) {
        for c in n.children() {
            match &c.data().value {
                NodeValue::Text(t) => self.out.push_str(&escape_html(t)),
                NodeValue::SoftBreak => {
                    // goldmark keeps the white space before a line break but its last character
                    let line = self.source_at(n.data().sourcepos.start.line, 1);
                    let ws = &line[line.trim_end_matches([' ', '\t']).len()..];
                    if ws.len() > 1 {
                        self.out.push_str(&ws[..ws.len() - 1]);
                    }
                    self.out.push('\n');
                }
                NodeValue::LineBreak => self.out.push_str("<br>\n"),
                NodeValue::Code(code) => self.out.push_str(&escape_html(&code.literal)),
                NodeValue::Link(_) if self.autolink_raw(c).is_some() => {}
                NodeValue::HtmlInline(_) | NodeValue::FootnoteReference(_) => {}
                _ => self.alt(c),
            }
        }
    }

    /// source_at returns the source from a line and column on.
    fn source_at(&self, line: usize, col: usize) -> &'s str {
        let l = self.lines.get(line.wrapping_sub(1)).copied().unwrap_or("");
        let i = col.saturating_sub(1).min(l.len());
        if !l.is_char_boundary(i) {
            return "";
        }
        &l[i..]
    }

    /// autolink_raw returns the URL of an autolink <...> as written, and whether
    /// it is an e-mail address; None for any other link.
    fn autolink_raw<'a>(&self, n: Node<'a>) -> Option<(String, bool)> {
        let NodeValue::Link(l) = &n.data().value else {
            return None;
        };
        let sp = n.data().sourcepos;
        let s = self.source_at(sp.start.line, sp.start.column);
        let inner = s.strip_prefix('<')?;
        let end = inner.find('>')?;
        let raw = &inner[..end];
        let email =
            l.url.starts_with("mailto:") && !raw.to_ascii_lowercase().starts_with("mailto:");
        Some((raw.to_string(), email))
    }

    /// flush_run renders a run of text with goldmark's linkify, then empties it.
    fn flush_run(&mut self, run: &mut Vec<Piece>, depth: &mut usize) {
        if run.is_empty() {
            return;
        }
        // the source as goldmark sees it: an escaped character keeps its backslash
        let mut raw = String::new();
        let mut inserted: Vec<usize> = Vec::new();
        // where the text pieces start in raw and in the source
        let mut starts: Vec<(usize, usize, (usize, usize))> = Vec::new();
        for (t, esc, pos) in run.drain(..) {
            if esc {
                inserted.push(raw.len());
                raw.push('\\');
            }
            let from = raw.len();
            if self.hiding {
                raw.push_str(&unhide(&t));
            } else {
                raw.push_str(&t);
            }
            if let Some(p) = pos {
                starts.push((from, raw.len(), p));
            }
        }
        let b = raw.as_bytes();
        let shown = |from: usize, to: usize| -> String {
            let mut s = String::new();
            let mut last = from;
            for &i in inserted.iter().filter(|&&i| i >= from && i < to) {
                s.push_str(&raw[last..i]);
                last = i + 1;
            }
            s.push_str(&raw[last..to]);
            escape_html(&s)
        };
        let mut out = String::new();
        let (mut p, mut emitted) = (0, 0);
        let mut start = true; // goldmark's i == 0: line start or right after an inline node
        let mut escaped = false;
        while p < b.len() {
            let c = b[p];
            let punct = is_punct(c);
            let space = c == b' ' || c == b'\t';
            if *depth == 0 && ((punct && !escaped) || space || start) {
                let consume = matches!(c, b' ' | b'*' | b'_' | b'~' | b'(');
                if consume || (start && !punct) {
                    let at = if consume { p + 1 } else { p };
                    let found = linkify(&raw[at..]);
                    self.hide_url(&raw, at, found.as_ref().map_or(0, |m| m.len), &starts);
                    if let Some(m) = found {
                        out.push_str(&shown(emitted, at));
                        let label = &raw[at..at + m.len];
                        let url = if m.www {
                            format!("http://{label}")
                        } else {
                            label.to_string()
                        };
                        out.push_str("<a href=\"");
                        let u = url_escape(&url);
                        if m.email {
                            out.push_str("mailto:");
                        }
                        if !is_dangerous_url(&u) {
                            out.push_str(&escape_html(&u));
                        }
                        out.push_str("\">");
                        out.push_str(&escape_html(label));
                        out.push_str("</a>");
                        p = at + m.len;
                        emitted = p;
                        start = true;
                        escaped = false;
                        continue;
                    }
                }
            }
            start = false;
            if escaped {
                escaped = false;
                p += utf8_len(c);
                continue;
            }
            if c == b'\\' {
                escaped = true;
                p += 1;
                continue;
            }
            if c == b'[' {
                *depth += 1;
            } else if c == b']' && *depth > 0 {
                *depth -= 1;
            }
            p += utf8_len(c);
        }
        out.push_str(&shown(emitted, b.len()));
        self.out.push_str(&out);
    }
}

/// Piece is a piece of a text run: the text, whether it is an escaped
/// character, and where a text node ends in the source (line, column).
type Piece = (String, bool, Option<(usize, usize)>);

impl Conv<'_> {
    /// hide_url marks what comrak parsed inside a URL or e-mail address that
    /// goldmark's linkify reads on: one at byte at of raw that linkify, run on
    /// the source line, finds longer than the len bytes found in the text
    /// comrak gave (a ~, `, [, ], _, * or ! in it started a strikethrough, code
    /// span, link, footnote reference or emphasis). starts gives the source of
    /// the text pieces.
    fn hide_url(
        &mut self,
        raw: &str,
        at: usize,
        len: usize,
        starts: &[(usize, usize, (usize, usize))],
    ) {
        if !self.hiding {
            return;
        }
        let Some(&(_, to, (line, col))) = starts.iter().rev().find(|s| s.0 <= at && at < s.1)
        else {
            return;
        };
        let Some(l) = self.lines.get(line.wrapping_sub(1)) else {
            return;
        };
        // the source is known where the text from at to the end of the piece is
        // the source as written (no entities; comrak's position is right)
        let Some(i) = col.checked_sub(to - at) else {
            return;
        };
        if l.get(i..col) != raw.get(at..to) {
            return;
        }
        // ponytail: a table cell ends at '|'; an e-mail address with a '|' in it
        // is not looked for beyond one
        let Some(rest) = l.get(i..).map(|r| r.split('|').next().unwrap_or(r)) else {
            return;
        };
        let Some(m) = linkify(rest) else { return };
        if m.len <= len || rest.get(..len) != raw.get(at..at + len) {
            return;
        }
        let base = l.as_ptr() as usize - self.src.as_ptr() as usize + i;
        for (k, c) in rest.as_bytes()[..m.len].iter().enumerate() {
            if HIDDEN.contains(c) {
                self.hide.push(base + k);
            }
        }
    }
}

/// written_label is the label of a footnote reference as written: comrak
/// replaces the name by the one of the definition it matched without case.
fn written_label(r: &comrak::nodes::NodeFootnoteReference) -> String {
    let t: String = r.texts.iter().map(|x| x.0.as_str()).collect();
    match t.strip_prefix('^') {
        Some(l) => l.to_string(),
        None => r.name.clone(),
    }
}

fn utf8_len(c: u8) -> usize {
    match c {
        0xF0..=0xFF => 4,
        0xE0..=0xEF => 3,
        0xC0..=0xDF => 2,
        _ => 1,
    }
}

struct Linkified {
    len: usize,
    www: bool,
    email: bool,
}

static WWW_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^www\.[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-z]+(?:[/#?][-a-zA-Z0-9@:%_\+.~#!?&/=\(\);,'">\^{}\[\]`]*)?"#).unwrap()
});
static URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:http|https|ftp)://[-a-zA-Z0-9@:%._\+~#=]{1,256}\.[a-z]+(?::[0-9]+)?(?:[/#?][-a-zA-Z0-9@:%_+.~#$!?&/=\(\);,'">\^{}\[\]`]*)?"#).unwrap()
});
static EMAIL_DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)*").unwrap()
});

/// linkify is goldmark's linkify parser at the start of line: a URL with
/// http, https or ftp, a www. address or an e-mail address, without the
/// punctuation that ends a sentence.
fn linkify(line: &str) -> Option<Linkified> {
    let b = line.as_bytes();
    let mut www = false;
    let mut email = false;
    let mut m: Option<usize> = None;
    if line.starts_with("http:") || line.starts_with("https:") || line.starts_with("ftp:") {
        m = URL.find(line).map(|x| x.end());
    }
    if m.is_none() && line.starts_with("www.") {
        m = WWW_URL.find(line).map(|x| x.end());
        www = m.is_some();
    }
    let end = match m {
        Some(e) => e,
        None => {
            if !b.is_empty() && is_punct(b[0]) {
                return None;
            }
            email = true;
            let stop = find_email_index(b)?;
            let at = line.find('@')?;
            if !line[at..stop - 1].contains('.') {
                return None;
            }
            let mut e = stop;
            if b[e - 1] == b'.' {
                e -= 1;
            }
            if e < b.len() && (b[e] == b'-' || b[e] == b'_') {
                return None;
            }
            e
        }
    };
    let mut i = end - 1;
    while i > 0 && matches!(b[i], b'?' | b'!' | b'.' | b',' | b':' | b'*' | b'_' | b'~') {
        i -= 1;
    }
    i += 1;
    if b[i - 1] == b')' {
        let close = b[..i].iter().filter(|&&c| c == b')').count();
        let open = b[..i].iter().filter(|&&c| c == b'(').count();
        i -= close.saturating_sub(open);
    } else if b[i - 1] == b';' {
        let mut j = i as isize - 2;
        while j >= 0 && b[j as usize].is_ascii_alphanumeric() {
            j -= 1;
        }
        if j != i as isize - 2 && j >= 0 && b[j as usize] == b'&' {
            i = j as usize;
        }
    }
    if i == 0 {
        return None;
    }
    Some(Linkified { len: i, www, email })
}

/// find_email_index is goldmark's util.FindEmailIndex.
fn find_email_index(b: &[u8]) -> Option<usize> {
    let local = |c: u8| c.is_ascii_alphanumeric() || b"!#$%&'*+-./=?^_`{|}~".contains(&c);
    let mut i = 0;
    while i < b.len() && local(b[i]) {
        i += 1;
    }
    if i == 0 || i >= b.len() || b[i] != b'@' {
        return None;
    }
    i += 1;
    if i >= b.len() {
        return None;
    }
    let s = std::str::from_utf8(&b[i..]).ok()?;
    let m = EMAIL_DOMAIN.find(s)?;
    Some(i + m.end())
}
