//! Package loader parses strict JSON into a node tree that keeps byte offsets,
//! so diagnostics can point at the exact place in the source file.

use std::fmt;
use std::sync::OnceLock;

pub const MAX_FILE_SIZE: usize = 5 << 20;
pub const MAX_DEPTH: usize = 64;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Null,
    Bool,
    Number,
    String,
    Array,
    Object,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub key: String,
    pub key_start: usize,
    pub key_end: usize,
    pub value: Node,
}

/// Node is one JSON value. Start/End are byte offsets of the token in Source.Data.
/// Str holds the decoded string, or the raw text for numbers and booleans.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
    pub str: String,
    pub items: Vec<Node>,
    pub members: Vec<Member>,
}

impl Node {
    fn new(kind: Kind, start: usize) -> Node {
        Node { kind, start, end: start, str: String::new(), items: Vec::new(), members: Vec::new() }
    }

    /// Get returns the first member with the given key, or None.
    pub fn get(&self, key: &str) -> Option<&Node> {
        if self.kind != Kind::Object {
            return None;
        }
        self.members.iter().find(|m| m.key == key).map(|m| &m.value)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Source {
    /// without BOM
    pub data: Vec<u8>,
    pub bom: bool,
    line_starts: OnceLock<Vec<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for Error {}

/// Dup is a repeated key; Start/End cover the repeated key token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dup {
    pub key: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Result {
    pub src: Source,
    pub root: Option<Node>,
    pub err: Option<Error>,
    pub dups: Vec<Dup>,
}

type PResult<T> = std::result::Result<T, Error>;

fn new_source(data: &[u8]) -> Source {
    let mut s = Source::default();
    let data = match data.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => {
            s.bom = true;
            rest
        }
        None => data,
    };
    s.data = data.to_vec();
    s
}

/// rune_count counts runes like Go's utf8.RuneCount: each invalid byte is one rune.
fn rune_count(b: &[u8]) -> usize {
    b.utf8_chunks().map(|c| c.valid().chars().count() + c.invalid().len()).sum()
}

/// decode_rune_len is the width utf8.DecodeRune reports: 1 for an invalid byte.
fn decode_rune_len(b: &[u8]) -> usize {
    // a rune is at most 4 bytes: looking further would make callers quadratic
    match b[..b.len().min(4)].utf8_chunks().next() {
        Some(c) => c.valid().chars().next().map_or(1, char::len_utf8),
        None => 0,
    }
}

impl Source {
    fn lines(&self) -> &[usize] {
        self.line_starts.get_or_init(|| {
            let mut ls = vec![0];
            for (i, &b) in self.data.iter().enumerate() {
                if b == b'\n' {
                    ls.push(i + 1);
                }
            }
            ls
        })
    }

    /// Position returns 1-based line and rune column of a byte offset.
    pub fn position(&self, off: usize) -> (usize, usize) {
        let ls = self.lines();
        let off = off.min(self.data.len());
        let i = ls.partition_point(|&s| s <= off) - 1;
        (i + 1, rune_count(&self.data[ls[i]..off]) + 1)
    }

    /// StrOffset maps a byte index of the decoded string value of n to a source
    /// offset. Bytes produced by one escape sequence map to the start of that escape.
    pub fn str_offset(&self, n: &Node, i: usize) -> usize {
        let raw = &self.data[n.start + 1..n.end - 1];
        if !raw.contains(&b'\\') {
            return n.start + 1 + i.min(raw.len());
        }
        let mut d = 0;
        let mut p = 0;
        while p < raw.len() {
            let (k, l) = if raw[p] != b'\\' {
                let l = decode_rune_len(&raw[p..]);
                (l, l)
            } else {
                escape_size(&raw[p..])
            };
            if i < d + k {
                return n.start + 1 + p;
            }
            d += k;
            p += l;
        }
        n.end - 1
    }
}

/// escape_size returns decoded byte count and raw length of the escape at b[0]=='\\'.
fn escape_size(b: &[u8]) -> (usize, usize) {
    if b.len() < 2 || b[1] != b'u' {
        return (1, 2);
    }
    let Some(r) = hex4(&b[2..]) else {
        return (1, 2);
    };
    if (0xD800..0xDC00).contains(&r) && b.len() >= 12 && b[6] == b'\\' && b[7] == b'u' {
        if let Some(r2) = hex4(&b[8..]) {
            if (0xDC00..0xE000).contains(&r2) {
                return (4, 12);
            }
        }
    }
    if (0xD800..0xE000).contains(&r) {
        return (3, 6);
    }
    // r is a BMP code point outside the surrogates, so it is a valid char.
    (char::from_u32(r).map_or(3, char::len_utf8), 6)
}

fn hex4(b: &[u8]) -> Option<u32> {
    if b.len() < 4 {
        return None;
    }
    let mut v = 0u32;
    for &c in &b[..4] {
        v = v << 4
            | match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return None,
            } as u32;
    }
    Some(v)
}

/// quote_rune is Go's %q of rune(c) for a single byte c (strconv.QuoteRune on U+0000..U+00FF).
fn quote_rune(c: u8) -> String {
    let body = match c {
        0x07 => "\\a".to_string(),
        0x08 => "\\b".to_string(),
        0x0C => "\\f".to_string(),
        b'\n' => "\\n".to_string(),
        b'\r' => "\\r".to_string(),
        b'\t' => "\\t".to_string(),
        0x0B => "\\v".to_string(),
        b'\'' => "\\'".to_string(),
        b'\\' => "\\\\".to_string(),
        0x00..0x20 | 0x7F => format!("\\x{c:02x}"),
        // C1 controls, NBSP and the soft hyphen are not printable for Go.
        0x80..0xA1 | 0xAD => format!("\\u{c:04x}"),
        _ => char::from(c).to_string(),
    };
    format!("'{body}'")
}

struct Parser<'a> {
    d: &'a [u8],
    p: usize,
    dups: Vec<Dup>,
}

fn fail<T>(off: usize, msg: impl Into<String>) -> PResult<T> {
    Err(Error { offset: off, msg: msg.into() })
}

/// Parse parses a document file. The root must be an object.
pub fn parse(data: &[u8]) -> Result {
    let src = new_source(data);
    let mut r = Result { src, ..Default::default() };
    let d = &r.src.data;
    if d.len() > MAX_FILE_SIZE {
        r.err = Some(Error { offset: 0, msg: "file is larger than 5 MB".into() });
        return r;
    }
    if let Err(e) = std::str::from_utf8(d) {
        r.err = Some(Error { offset: e.valid_up_to(), msg: "file is not valid UTF-8".into() });
        return r;
    }
    let mut p = Parser { d, p: 0, dups: Vec::new() };
    let res = (|| {
        p.ws();
        if p.p >= d.len() {
            return fail(p.p, "file is empty");
        }
        let root = p.value(0)?;
        p.ws();
        if p.p < d.len() {
            return fail(p.p, "unexpected data after the top-level value");
        }
        if root.kind != Kind::Object {
            return fail(root.start, "the top-level value must be an object");
        }
        Ok(root)
    })();
    match res {
        Ok(root) => {
            r.dups = p.dups;
            r.root = Some(root);
        }
        Err(e) => r.err = Some(e),
    }
    r
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.p < self.d.len() && matches!(self.d[self.p], b' ' | b'\t' | b'\n' | b'\r') {
            self.p += 1;
        }
    }

    fn value(&mut self, depth: usize) -> PResult<Node> {
        if depth >= MAX_DEPTH {
            return fail(self.p, format!("nesting is deeper than {MAX_DEPTH} levels"));
        }
        if self.p >= self.d.len() {
            return fail(self.p, "unexpected end of file");
        }
        match self.d[self.p] {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => self.str(),
            b'-' | b'0'..=b'9' => self.number(),
            b't' => self.lit("true", Kind::Bool),
            b'f' => self.lit("false", Kind::Bool),
            b'n' => self.lit("null", Kind::Null),
            c => fail(self.p, format!("unexpected character {}", quote_rune(c))),
        }
    }

    fn lit(&mut self, s: &str, k: Kind) -> PResult<Node> {
        if !self.d[self.p..].starts_with(s.as_bytes()) {
            return fail(self.p, "invalid literal");
        }
        let mut n = Node::new(k, self.p);
        n.end = self.p + s.len();
        n.str = s.to_string();
        self.p += s.len();
        Ok(n)
    }

    fn digits(&mut self) -> usize {
        let s = self.p;
        while self.p < self.d.len() && self.d[self.p].is_ascii_digit() {
            self.p += 1;
        }
        self.p - s
    }

    fn number(&mut self) -> PResult<Node> {
        let st = self.p;
        if self.d[self.p] == b'-' {
            self.p += 1;
        }
        if self.p < self.d.len() && self.d[self.p] == b'0' {
            self.p += 1;
        } else if self.digits() == 0 {
            return fail(self.p, "invalid number");
        }
        if self.p < self.d.len() && self.d[self.p] == b'.' {
            self.p += 1;
            if self.digits() == 0 {
                return fail(self.p, "invalid number");
            }
        }
        if self.p < self.d.len() && matches!(self.d[self.p], b'e' | b'E') {
            self.p += 1;
            if self.p < self.d.len() && matches!(self.d[self.p], b'+' | b'-') {
                self.p += 1;
            }
            if self.digits() == 0 {
                return fail(self.p, "invalid number");
            }
        }
        let mut n = Node::new(Kind::Number, st);
        n.end = self.p;
        n.str = String::from_utf8_lossy(&self.d[st..self.p]).into_owned();
        Ok(n)
    }

    fn str(&mut self) -> PResult<Node> {
        let st = self.p;
        self.p += 1;
        let mut plain = true;
        loop {
            if self.p >= self.d.len() {
                return fail(st, "unterminated string");
            }
            let c = self.d[self.p];
            if c == b'"' {
                break;
            }
            if c < 0x20 {
                return fail(self.p, "control character in string");
            }
            if c == b'\\' {
                plain = false;
                self.p += 1;
                if self.p >= self.d.len() {
                    return fail(st, "unterminated string");
                }
                match self.d[self.p] {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        if hex4(&self.d[self.p + 1..]).is_none() {
                            return fail(self.p - 1, "invalid \\u escape");
                        }
                        self.p += 4;
                    }
                    _ => return fail(self.p - 1, "invalid escape"),
                }
            }
            self.p += 1;
        }
        let end = self.p + 1;
        let raw = &self.d[st + 1..self.p];
        self.p = end;
        let mut n = Node::new(Kind::String, st);
        n.end = end;
        // The data was checked to be valid UTF-8, so raw is too.
        n.str = if plain { String::from_utf8_lossy(raw).into_owned() } else { unescape(raw) };
        Ok(n)
    }

    fn array(&mut self, depth: usize) -> PResult<Node> {
        let mut n = Node::new(Kind::Array, self.p);
        self.p += 1;
        self.ws();
        if self.p < self.d.len() && self.d[self.p] == b']' {
            self.p += 1;
            n.end = self.p;
            return Ok(n);
        }
        loop {
            self.ws();
            n.items.push(self.value(depth + 1)?);
            self.ws();
            if self.p >= self.d.len() {
                return fail(n.start, "unterminated array");
            }
            if self.d[self.p] == b',' {
                self.p += 1;
                continue;
            }
            if self.d[self.p] == b']' {
                self.p += 1;
                n.end = self.p;
                return Ok(n);
            }
            return fail(self.p, "expected ',' or ']'");
        }
    }

    fn object(&mut self, depth: usize) -> PResult<Node> {
        let mut n = Node::new(Kind::Object, self.p);
        self.p += 1;
        self.ws();
        if self.p < self.d.len() && self.d[self.p] == b'}' {
            self.p += 1;
            n.end = self.p;
            return Ok(n);
        }
        let mut seen = std::collections::HashSet::new();
        loop {
            self.ws();
            if self.p >= self.d.len() || self.d[self.p] != b'"' {
                return fail(self.p, "expected a string key");
            }
            let k = self.str()?;
            self.ws();
            if self.p >= self.d.len() || self.d[self.p] != b':' {
                return fail(self.p, "expected ':'");
            }
            self.p += 1;
            self.ws();
            let v = self.value(depth + 1)?;
            if !seen.insert(k.str.clone()) {
                self.dups.push(Dup { key: k.str.clone(), start: k.start, end: k.end });
            }
            n.members.push(Member { key: k.str, key_start: k.start, key_end: k.end, value: v });
            self.ws();
            if self.p >= self.d.len() {
                return fail(n.start, "unterminated object");
            }
            if self.d[self.p] == b',' {
                self.p += 1;
                continue;
            }
            if self.d[self.p] == b'}' {
                self.p += 1;
                n.end = self.p;
                return Ok(n);
            }
            return fail(self.p, "expected ',' or '}'");
        }
    }
}

fn unescape(raw: &[u8]) -> String {
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let push = |out: &mut Vec<u8>, c: char| out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != b'\\' {
            out.push(raw[i]);
            i += 1;
            continue;
        }
        match raw[i + 1] {
            b'b' => out.push(0x08),
            b'f' => out.push(0x0C),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'u' => {
                let (dec, l) = escape_size(&raw[i..]);
                let r = hex4(&raw[i + 2..]).unwrap_or(0);
                let c = if l == 12 {
                    let r2 = hex4(&raw[i + 8..]).unwrap_or(0);
                    char::from_u32(0x10000 + ((r - 0xD800) << 10) + (r2 - 0xDC00))
                } else if dec == 3 && (0xD800..0xE000).contains(&r) {
                    None
                } else {
                    char::from_u32(r)
                };
                // A lone surrogate becomes U+FFFD, as utf8.AppendRune does.
                push(&mut out, c.unwrap_or(char::REPLACEMENT_CHARACTER));
                i += l;
                continue;
            }
            // " \ /
            c => out.push(c),
        }
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Value converts a node to plain values for schema validation
/// (objects as a map with the first occurrence of a key winning).
pub fn value(n: &Node) -> serde_json::Value {
    use serde_json::Value as V;
    match n.kind {
        Kind::Null => V::Null,
        Kind::Bool => V::Bool(n.str == "true"),
        Kind::Number => number_value(&n.str),
        Kind::String => V::String(n.str.clone()),
        Kind::Array => V::Array(n.items.iter().map(value).collect()),
        Kind::Object => {
            let mut m = serde_json::Map::new();
            for mb in &n.members {
                if !m.contains_key(&mb.key) {
                    m.insert(mb.key.clone(), value(&mb.value));
                }
            }
            V::Object(m)
        }
    }
}

fn number_value(s: &str) -> serde_json::Value {
    if let Ok(i) = s.parse::<i64>() {
        return i.into();
    }
    let f: f64 = s.parse().unwrap_or(0.0);
    // Go keeps ±Inf for out-of-range numbers; JSON values cannot hold it, so the
    // largest finite float with the same sign stands in (rust/diff/loader.md #1).
    let f = if f.is_infinite() { f64::MAX.copysign(f) } else { f };
    serde_json::Number::from_f64(f).map_or(serde_json::Value::Null, serde_json::Value::Number)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(r: &Result, off: usize) -> String {
        let (l, c) = r.src.position(off);
        format!("{l}:{c}")
    }

    fn root(r: &Result) -> &Node {
        r.root.as_ref().unwrap()
    }

    #[test]
    fn test_positions_plain() {
        let r = parse("{\n  \"title\": \"Привет\",\n  \"x\": 1\n}".as_bytes());
        assert!(r.err.is_none(), "{:?}", r.err);
        let title = root(&r).get("title").unwrap();
        assert_eq!(pos(&r, title.start), "2:12", "title start");
        assert_eq!(pos(&r, title.end), "2:20", "title end"); // after 8 runes: quote + 6 + quote
        assert_eq!(pos(&r, root(&r).members[1].key_start), "3:3", "key x");
    }

    #[test]
    fn test_bom_and_crlf() {
        let r = parse(b"\xEF\xBB\xBF{\r\n  \"a\": \"b\"\r\n}");
        assert!(r.err.is_none() && r.src.bom, "{:?} {}", r.err, r.src.bom);
        assert_eq!(pos(&r, root(&r).get("a").unwrap().start), "2:8");
        assert_eq!(pos(&r, root(&r).end - 1), "3:1", "closing brace");
    }

    #[test]
    fn test_escaped_strings() {
        // "a\nA😀z"  decoded: a, \n, A, 😀 (4 bytes), z
        let bs = "\\"; // backslash, so escapes stay escapes in the input
        let input = format!(r#"{{"s":"a{bs}n{bs}u0041{bs}ud83d{bs}ude00z"}}"#);
        let r = parse(input.as_bytes());
        assert!(r.err.is_none(), "{:?}", r.err);
        let n = root(&r).get("s").unwrap();
        assert_eq!(n.str, "a\nA😀z", "decoded");
        let want = [
            (0, "1:7"),  // a
            (1, "1:8"),  // \n escape start
            (2, "1:10"), // A
            (3, "1:16"), // emoji (first byte)
            (6, "1:16"), // emoji (last byte) still the same escape
            (7, "1:28"), // z
            (8, "1:29"), // closing quote
        ];
        for (i, w) in want {
            assert_eq!(pos(&r, r.src.str_offset(n, i)), w, "index {i}");
        }
    }

    #[test]
    fn test_literal_astral_rune_counts_once() {
        let r = parse(r#"{"s":"😀x"}"#.as_bytes());
        let n = root(&r).get("s").unwrap();
        assert_eq!(pos(&r, r.src.str_offset(n, 4)), "1:8");
    }

    #[test]
    fn test_duplicate_keys() {
        let r = parse(b"{\"a\":1,\n \"a\":2}");
        assert!(r.err.is_none() && r.dups.len() == 1, "{:?} {:?}", r.err, r.dups);
        assert_eq!(pos(&r, r.dups[0].start), "2:2");
    }

    #[test]
    fn test_errors() {
        let deep = "[".repeat(70) + &"]".repeat(70);
        let deep_in = format!(r#"{{"a":{deep}}}"#);
        let cases: [(&str, &[u8]); 10] = [
            ("empty", b""),
            ("array root", b"[]"),
            ("comment", br#"{"a":1 /* x */}"#),
            ("trailing", br#"{"a":1,}"#),
            ("leading 0", br#"{"a":01}"#),
            ("ctrl", b"{\"a\":\"x\ny\"}"),
            ("bad escape", br#"{"a":"\x"}"#),
            ("bad utf8", b"{\"a\":\"\xff\"}"),
            ("after value", b"{} {}"),
            ("deep", deep_in.as_bytes()),
        ];
        for (name, input) in cases {
            assert!(parse(input).err.is_some(), "{name}: expected error");
        }
    }

    #[test]
    fn test_value() {
        let r = parse(br#"{"a":[1,2.5,true,null,"x"],"b":{"c":"d"}}"#);
        let m = value(root(&r));
        assert_eq!(m["a"].as_array().unwrap().len(), 5, "{m}");
        assert_eq!(m["b"]["c"], "d", "{m}");
    }

    /// FuzzParse: the Go fuzz target run over its seed corpus.
    #[test]
    fn fuzz_parse() {
        let seeds: [&[u8]; 5] = [
            r#"{"a":[1,2,{"b":"c"}],"d":"é😀"}"#.as_bytes(),
            b"{\n\"a\" : 1 ,\r\n\"a\":2}",
            br#"{"x":-0.5e+10}"#,
            b"\xEF\xBB\xBF{}",
            br#"{"s":"\q"}"#,
        ];
        for data in seeds {
            let r = parse(data);
            if let Some(e) = &r.err {
                assert!(e.offset <= r.src.data.len(), "error offset {} out of range", e.offset);
                continue;
            }
            fn visit(r: &Result, n: &Node) {
                assert!(n.end <= r.src.data.len() && n.start < n.end, "bad range {}..{}", n.start, n.end);
                r.src.position(n.start);
                if n.kind == Kind::String {
                    for i in 0..=n.str.len() {
                        let off = r.src.str_offset(n, i);
                        assert!(off >= n.start && off < n.end, "StrOffset({i})={off} outside {}..{}", n.start, n.end);
                    }
                }
                n.items.iter().for_each(|it| visit(r, it));
                n.members.iter().for_each(|m| visit(r, &m.value));
            }
            visit(&r, root(&r));
        }
    }

    fn synth_doc(i: usize) -> Vec<u8> {
        let para = "Система должна обеспечивать устойчивую работу при нагрузке. ".repeat(6);
        let mut b = format!("{{\n  \"title\": \"Документ {i}\",\n  \"kind\": \"note\",\n  \"body\": [\n");
        let n = 5 + i % 12; // 5..16 paragraphs of ~0.36 KB of Cyrillic (2 bytes/rune) => 5..20 KB
        for k in 0..n {
            if k > 0 {
                b.push_str(",\n");
            }
            b.push_str(&format!("    \"{para}\""));
        }
        b.push_str("\n  ]\n}\n");
        b.into_bytes()
    }

    /// BenchmarkParse1000: run with `cargo test --release --lib loader -- --ignored --nocapture`.
    #[test]
    #[ignore = "benchmark"]
    fn benchmark_parse1000() {
        let docs: Vec<Vec<u8>> = (0..1000).map(synth_doc).collect();
        let total: usize = docs.iter().map(Vec::len).sum();
        let t = std::time::Instant::now();
        for d in &docs {
            let r = parse(d);
            assert!(r.err.is_none(), "{:?}", r.err);
        }
        let el = t.elapsed();
        println!("{total} bytes in {el:?} ({:.1} MB/s)", total as f64 / el.as_secs_f64() / 1e6);
    }

    #[test]
    fn error_messages_and_offsets() {
        // Messages and offsets become DOC101..DOC108, so they must match Go exactly.
        let cases: [(&[u8], usize, &str); 12] = [
            (b"", 0, "file is empty"),
            (b"  []", 2, "the top-level value must be an object"),
            (b"{} x", 3, "unexpected data after the top-level value"),
            (b"{\"a\":\x01}", 5, "unexpected character '\\x01'"),
            ("{\"a\":Ж}".as_bytes(), 5, "unexpected character 'Ð'"),
            ("{\"a\":\u{a0}}".as_bytes(), 5, "unexpected character 'Â'"),
            (br#"{"a":"\u12"}"#, 6, "invalid \\u escape"),
            (br#"{"a":tru}"#, 5, "invalid literal"),
            (br#"{"a":-}"#, 6, "invalid number"),
            (br#"{"a":"x"#, 5, "unterminated string"),
            (br#"{"a":[1"#, 5, "unterminated array"),
            (b"{\"a\":\"\xe2\x82\"}", 6, "file is not valid UTF-8"),
        ];
        for (input, off, msg) in cases {
            let e = parse(input).err.unwrap();
            assert_eq!((e.offset, e.msg.as_str()), (off, msg), "{:?}", String::from_utf8_lossy(input));
        }
        let e = parse(format!(r#"{{"a":{}}}"#, "[".repeat(70)).as_bytes()).err.unwrap();
        assert_eq!((e.offset, e.msg.as_str()), (68, "nesting is deeper than 64 levels"));
    }

    #[test]
    fn unescape_and_numbers() {
        let r = parse(br#"{"s":"\ud800x\/\"\t","n":[0,-0,1.5e2,1e999,9223372036854775808]}"#);
        let n = root(&r);
        assert_eq!(n.get("s").unwrap().str, "\u{fffd}x/\"\t");
        let v = value(n);
        assert_eq!(v["n"].to_string(), "[0,0,150.0,1.7976931348623157e+308,9.223372036854776e+18]");
    }

    #[test]
    fn quote_rune_matches_go() {
        // Go: for c := 0; c < 256; c++ { s += fmt.Sprintf("%q", rune(byte(c))) }
        let want = r#"'\x00''\x01''\x02''\x03''\x04''\x05''\x06''\a''\b''\t''\n''\v''\f''\r''\x0e''\x0f''\x10''\x11''\x12''\x13''\x14''\x15''\x16''\x17''\x18''\x19''\x1a''\x1b''\x1c''\x1d''\x1e''\x1f'' ''!''"''#''$''%''&''\'''('')''*''+'',''-''.''/''0''1''2''3''4''5''6''7''8''9'':'';''<''=''>''?''@''A''B''C''D''E''F''G''H''I''J''K''L''M''N''O''P''Q''R''S''T''U''V''W''X''Y''Z''[''\\'']''^''_''`''a''b''c''d''e''f''g''h''i''j''k''l''m''n''o''p''q''r''s''t''u''v''w''x''y''z''{''|''}''~''\x7f''\u0080''\u0081''\u0082''\u0083''\u0084''\u0085''\u0086''\u0087''\u0088''\u0089''\u008a''\u008b''\u008c''\u008d''\u008e''\u008f''\u0090''\u0091''\u0092''\u0093''\u0094''\u0095''\u0096''\u0097''\u0098''\u0099''\u009a''\u009b''\u009c''\u009d''\u009e''\u009f''\u00a0''¡''¢''£''¤''¥''¦''§''¨''©''ª''«''¬''\u00ad''®''¯''°''±''²''³''´''µ''¶''·''¸''¹''º''»''¼''½''¾''¿''À''Á''Â''Ã''Ä''Å''Æ''Ç''È''É''Ê''Ë''Ì''Í''Î''Ï''Ð''Ñ''Ò''Ó''Ô''Õ''Ö''×''Ø''Ù''Ú''Û''Ü''Ý''Þ''ß''à''á''â''ã''ä''å''æ''ç''è''é''ê''ë''ì''í''î''ï''ð''ñ''ò''ó''ô''õ''ö''÷''ø''ù''ú''û''ü''ý''þ''ÿ'"#;
        let got: String = (0..=255u8).map(quote_rune).collect();
        assert_eq!(got, want);
    }
}
