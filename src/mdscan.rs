//! Package mdscan is a small scanner for the Markdown that lives inside JSON
//! string values: code blocks, inline code, [[refs]], {{includes}}, headings,
//! raw HTML tags and links. All offsets are byte offsets in the scanned string.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Result of scanning one string value.
#[derive(Debug, Clone)]
pub struct Scan {
    pub s: String,
    /// text outside fenced blocks, inline code and {{includes}}
    pub plain: Vec<Span>,
    /// offset of an unclosed opening fence, or None (Go: -1)
    pub unclosed_fence: Option<usize>,
    includes: Vec<Ref>,
}

/// New scans s.
pub fn new(s: &str) -> Scan {
    let mut sc = Scan { s: s.to_string(), plain: Vec::new(), unclosed_fence: None, includes: Vec::new() };
    let b = s.as_bytes();
    let mut chunk_start: Option<usize> = None;
    let flush = |sc: &mut Scan, chunk_start: &mut Option<usize>, end: usize| {
        if let Some(cs) = *chunk_start
            && end > cs
        {
            sc.inline(cs, end);
        }
        *chunk_start = None;
    };
    let mut fence_ch = 0u8;
    let mut fence_len = 0;
    let mut pos = 0;
    while pos <= b.len() {
        let (line_end, next) = match b[pos..].iter().position(|&c| c == b'\n') {
            Some(end) => (pos + end, pos + end + 1),
            None => (b.len(), b.len() + 1),
        };
        let line = &b[pos..line_end];
        let (ch, n, rest) = fence_run(line);
        if fence_len > 0 {
            if n >= fence_len && ch == fence_ch && trim_space_empty(rest) {
                fence_len = 0;
            }
        } else if n >= 3 {
            flush(&mut sc, &mut chunk_start, pos);
            fence_ch = ch;
            fence_len = n;
            sc.unclosed_fence = Some(pos);
        } else if chunk_start.is_none() {
            chunk_start = Some(pos);
        }
        pos = next;
    }
    if fence_len > 0 {
        // unclosed: UnclosedFence stays at the opener, the rest is code
        sc.cut_includes();
        return sc;
    }
    sc.unclosed_fence = None;
    flush(&mut sc, &mut chunk_start, b.len());
    sc.cut_includes();
    sc
}

/// strings.TrimSpace(rest) == "": the line is separated at ASCII bytes, so the
/// rest is valid UTF-8 whenever the source is.
fn trim_space_empty(rest: &[u8]) -> bool {
    std::str::from_utf8(rest).is_ok_and(|r| r.trim().is_empty())
}

impl Scan {
    /// cutIncludes finds {{...}} and removes them from the plain text: text rules
    /// skip includes, the inserted text is checked in its own document.
    fn cut_includes(&mut self) {
        self.includes = self.delimited("{{", "}}");
        // a block code in an include is matched in any case, as in a link
        for r in &mut self.includes {
            r.inner = crate::codes::split_hash(&r.inner).0;
        }
        self.plain = without(&self.plain, &self.includes);
    }

    /// Prose returns the plain text without [[refs]]: word rules do not read ids
    /// and anchor names.
    pub fn prose(&self) -> Vec<Span> {
        without(&self.plain, &self.refs())
    }

    /// inline splits chunk [a,b) into plain spans, cutting out inline code.
    fn inline(&mut self, a: usize, b: usize) {
        let s = self.s.as_bytes();
        let mut start = a;
        let mut i = a;
        while i < b {
            match s[i] {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'`' => {
                    let mut j = i;
                    while j < b && s[j] == b'`' {
                        j += 1;
                    }
                    let n = j - i;
                    // find closing run of exactly n backticks
                    let mut k = j;
                    let mut closed = None;
                    while k < b {
                        if s[k] != b'`' {
                            k += 1;
                            continue;
                        }
                        let mut m = k;
                        while m < b && s[m] == b'`' {
                            m += 1;
                        }
                        if m - k == n {
                            closed = Some(m);
                            break;
                        }
                        k = m;
                    }
                    let Some(closed) = closed else {
                        i = j;
                        continue;
                    };
                    if i > start {
                        self.plain.push(Span { start, end: i });
                    }
                    start = closed;
                    i = closed;
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        if start < b {
            self.plain.push(Span { start, end: b });
        }
    }

    fn delimited(&self, open: &str, close: &str) -> Vec<Ref> {
        let mut out = Vec::new();
        for sp in &self.plain {
            let t = &self.s[sp.start..sp.end];
            let mut i = 0;
            while i < t.len() {
                let Some(a) = t[i..].find(open) else { break };
                let a = a + i;
                let Some(z) = t[a + open.len()..].find(close) else { break };
                let inner = &t[a + open.len()..a + open.len() + z];
                if inner.contains(['[', ']', '{', '}', '\n']) {
                    i = a + 1;
                    continue;
                }
                out.push(Ref {
                    start: sp.start + a,
                    end: sp.start + a + open.len() + z + close.len(),
                    inner: inner.to_string(),
                    hash: None,
                });
                i = a + open.len() + z + close.len();
            }
        }
        out
    }

    /// Refs returns [[...]] occurrences outside code; the version hash of a
    /// link ("@3a9c" at its end) is cut from inner into hash, a block code in
    /// it is upper-cased (one of the old format lower-cased).
    pub fn refs(&self) -> Vec<Ref> {
        let mut rs = self.delimited("[[", "]]");
        for r in &mut rs {
            (r.inner, r.hash) = crate::codes::split_hash(&r.inner);
        }
        rs
    }

    /// Images returns the paths of Markdown images ![alt](path) outside code:
    /// Start and End cover the path only.
    pub fn images(&self) -> Vec<Ref> {
        self.group1(&IMAGE_RE)
    }

    /// Ref for capture group 1 of every match of re inside the plain spans.
    fn group1(&self, re: &Regex) -> Vec<Ref> {
        let mut out = Vec::new();
        for sp in &self.plain {
            for m in re.captures_iter(&self.s[sp.start..sp.end]) {
                let g = m.get(1).unwrap();
                let (a, b) = (sp.start + g.start(), sp.start + g.end());
                out.push(Ref { start: a, end: b, inner: self.s[a..b].to_string(), hash: None });
            }
        }
        out
    }

    /// Includes returns {{...}} occurrences outside code.
    pub fn includes(&self) -> &[Ref] {
        &self.includes
    }

    /// Headings returns the offsets of lines that start with 1-6 '#' and a space,
    /// outside code blocks.
    pub fn headings(&self) -> Vec<Span> {
        let s = self.s.as_bytes();
        let mut out = Vec::new();
        for sp in &self.plain {
            let mut off = sp.start;
            while off < sp.end {
                // only line starts count
                if off == 0 || s[off - 1] == b'\n' {
                    let mut n = 0;
                    while off + n < sp.end && s[off + n] == b'#' {
                        n += 1;
                    }
                    if (1..=6).contains(&n) && off + n < s.len() && s[off + n] == b' ' {
                        let end = s[off..].iter().position(|&c| c == b'\n').unwrap_or(s.len() - off);
                        out.push(Span { start: off, end: off + end });
                    }
                }
                let Some(nl) = s[off..sp.end].iter().position(|&c| c == b'\n') else { break };
                off += nl + 1;
            }
        }
        out
    }

    /// ScriptLinks returns the destinations of Markdown links that run script or
    /// embed a page (javascript:, vbscript:, data:) outside code; Start and End
    /// cover the destination.
    pub fn script_links(&self) -> Vec<Ref> {
        self.group1(&SCRIPT_LINK_RE)
    }

    /// HTMLTags returns opening tags with known HTML element names outside code.
    pub fn html_tags(&self) -> Vec<Tag> {
        let mut out = Vec::new();
        for sp in &self.plain {
            let t = &self.s[sp.start..sp.end];
            for m in HTML_OPEN.captures_iter(t) {
                let (all, g) = (m.get(0).unwrap(), m.get(1).unwrap());
                let name = g.as_str().to_ascii_lowercase();
                if HTML_ELEMENTS.contains(name.as_str()) {
                    out.push(Tag { span: Span { start: sp.start + all.start(), end: sp.start + all.end() }, name });
                }
            }
        }
        out
    }
}

/// Without cuts the occurrences rs out of the spans.
pub fn without(plain: &[Span], rs: &[Ref]) -> Vec<Span> {
    if rs.is_empty() {
        return plain.to_vec();
    }
    let mut out = Vec::new();
    for sp in plain {
        let mut from = sp.start;
        for r in rs {
            if r.start >= sp.start && r.end <= sp.end {
                if r.start > from {
                    out.push(Span { start: from, end: r.start });
                }
                from = r.end;
            }
        }
        if from < sp.end {
            out.push(Span { start: from, end: sp.end });
        }
    }
    out
}

/// fenceRun detects a fence line: up to 3 spaces, then 3+ of ` or ~.
fn fence_run(line: &[u8]) -> (u8, usize, &[u8]) {
    let mut i = 0;
    while i < line.len() && i < 3 && line[i] == b' ' {
        i += 1;
    }
    if i >= line.len() || (line[i] != b'`' && line[i] != b'~') {
        return (0, 0, &[]);
    }
    let ch = line[i];
    let mut j = i;
    while j < line.len() && line[j] == ch {
        j += 1;
    }
    if ch == b'`' && line[j..].contains(&b'`') {
        return (0, 0, &[]); // inline code, not a fence
    }
    (ch, j - i, &line[j..])
}

/// Ref is a [[...]] or {{...}} occurrence: Start/End include the brackets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub start: usize,
    pub end: usize,
    pub inner: String,
    /// the version hash of a link, lower case
    pub hash: Option<String>,
}

// Go's \s is ASCII [\t\n\f\r ]; spelled out because Rust's \s is Unicode.
static IMAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"!\[[^\]\n]*\]\(([^()\t\n\f\r ]+)(?:[\t\n\f\r ]+"[^"\n]*")?\)"#).unwrap());

static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.-]*:").unwrap());

/// LocalImage reports whether an image path names a file of the project: not
/// a URL (http:, data:, ...) and not an absolute path.
pub fn local_image(p: &str) -> bool {
    !SCHEME.is_match(p) && !p.starts_with('/')
}

static HTML_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<([A-Za-z][A-Za-z0-9]*)(?:[\t\n\f\r ][^<>]*)?/?>").unwrap());

static HTML_ELEMENTS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "a abbr address area article aside audio b base bdi bdo blockquote body br button canvas caption cite code col colgroup data datalist dd del details dfn dialog div dl dt em embed fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 head header hgroup hr html i iframe img input ins kbd label legend li link main map mark menu meta meter nav noscript object ol optgroup option output p param picture pre progress q rp rt ruby s samp script search section select slot small source span strong style sub summary sup table tbody td template textarea tfoot th thead time title tr track u ul var video wbr"
        .split_whitespace()
        .collect()
});

static SCRIPT_LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\]\([\t\n\f\r ]*<?((?:javascript|vbscript|data):[^)\t\n\f\r >]*)").unwrap());

/// Tag is a raw HTML opening tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub span: Span,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_text(sc: &Scan) -> String {
        let mut b = String::new();
        for sp in &sc.plain {
            b.push_str(&sc.s[sp.start..sp.end]);
            b.push('|');
        }
        b
    }

    #[test]
    fn test_plain() {
        let cases = [
            ("abc", "abc|"),
            ("a `x` b", "a | b|"),
            ("a ``x ` y`` b", "a | b|"),
            ("a `x b", "a `x b|"),
            ("a\n```\ncode [[x]]\n```\nb", "a\n|b|"),
            ("~~~\ncode\n~~~", ""),
            ("a `b\nc` d", "a | d|"),
            ("\\`x` y", "\\`x` y|"),
        ];
        for (input, want) in cases {
            assert_eq!(plain_text(&new(input)), want, "{input:?}");
        }
    }

    #[test]
    fn test_unclosed_fence() {
        assert_eq!(new("a\n```go\ncode").unclosed_fence, Some(2));
        assert_eq!(new("```\nx\n```\n").unclosed_fence, None, "closed fence reported");
        assert_eq!(new("``` inline ``` text").unclosed_fence, None, "inline triple ticks");
        assert_eq!(new("````\n```\n````").unclosed_fence, None, "longer fence containing shorter");
    }

    #[test]
    fn test_refs() {
        let sc = new("see [[a#b]] and `[[no]]` and [[c]] [[bad [x]]");
        let inner: Vec<String> = sc.refs().into_iter().map(|r| r.inner).collect();
        assert_eq!(inner, ["a#b", "c"]);
        let r = &sc.refs()[0];
        assert_eq!(&sc.s[r.start..r.end], "[[a#b]]");
        let sc2 = new("x {{a#b}} `{{no}}`");
        let inc = sc2.includes();
        assert!(inc.len() == 1 && inc[0].inner == "a#b", "{inc:?}");
    }

    #[test]
    fn test_headings() {
        let sc = new("text\n# H1\n####### seven\n#nospace\n```\n# code\n```\n## H2 tail");
        let got: Vec<&str> = sc.headings().iter().map(|h| &sc.s[h.start..h.end]).collect();
        assert_eq!(got, ["# H1", "## H2 tail"]);
    }

    #[test]
    fn test_html_tags() {
        let sc = new("a <div class=\"x\">b</div> <id> <Script> `<b>` <br/> <foo>");
        let names: Vec<String> = sc.html_tags().into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["div", "script", "br"]);
    }

    #[test]
    fn test_includes_are_not_plain_text() {
        let sc = new("a {{x#y}} b `c` {{p#q}}");
        assert_eq!(plain_text(&sc), "a | b | |", "plain");
        assert!(sc.includes().len() == 2 && sc.includes()[1].inner == "p#q", "{:?}", sc.includes());
    }
}
