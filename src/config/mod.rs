//! Package config reads docc.json (strict JSON, errors with positions) and
//! resolves effective rule levels and parameters.

mod glob;
mod levels;
mod ruleset;

pub use glob::{Glob, compile_glob, match_any};
pub use levels::{Resolved, Setting};
pub(crate) use ruleset::compile_go_regexp;

use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

use regex::Regex;

use crate::diag;
use crate::loader;

/// Error is a configuration error (exit code 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            return write!(f, "{}: config error: {}", self.file, self.msg);
        }
        write!(
            f,
            "{}:{}:{}: config error: {}",
            self.file, self.line, self.col, self.msg
        )
    }
}

impl std::error::Error for Error {}

/// Errors are several config errors found in one run, in file order.
/// Go returns a lone *Error by itself; here it is an Errors of one, which
/// prints and reports the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Errors(pub Vec<Error>);

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let lines: Vec<String> = self.0.iter().map(Error::to_string).collect();
        f.write_str(&lines.join("\n"))
    }
}

impl std::error::Error for Errors {}

impl From<Error> for Errors {
    fn from(e: Error) -> Errors {
        Errors(vec![e])
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params {
    pub words: Vec<String>,
    pub allow: Vec<String>,
}

/// Entry is one value of the rules section: a level and/or parameters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// None: level from the preset
    pub level: Option<diag::Level>,
    pub params: Params,
}

/// RuleSet is the content of the rules section.
/// ByRule is keyed by rule code, or by name for custom rules (DOC800 for all custom).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleSet {
    pub by_rule: HashMap<String, Entry>,
    pub by_group: HashMap<String, diag::Level>,
}

#[derive(Debug, Clone, Default)]
pub struct Custom {
    pub id: String,
    pub message: String,
    pub hint: String,
    pub kind: String,
    pub given: String,
    pub level: diag::Level,
    pub forbid: Option<Regex>,
    pub require: Option<Regex>,
    /// the NFC source of forbid or require, as Go's Regexp.String() gives it
    /// (the compiled expression is rewritten for the regex crate)
    pub pattern: String,
}

/// Branding is the branding section: the name, site, logo and colours of the
/// company the HTML build is made for. None: the page is unbranded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Branding {
    pub name: String,
    pub url: String,
    /// path relative to Root; png, jpg, jpeg, gif, svg or webp
    pub logo: String,
    /// links and marks in the dark theme and on the dark header
    pub accent: String,
    /// links and marks on a light page; falls back to Accent
    pub accent_light: String,
    /// background of the header with the logo
    pub header: String,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// absolute project root
    pub root: PathBuf,
    /// display name of the config file
    pub file: String,
    /// paths relative to Root, with '/'
    pub src: String,
    pub out: String,
    pub schemas: String,
    pub out_full: String,
    pub roots: Vec<String>,
    /// line, column of the roots value (0: default)
    pub roots_pos: [usize; 2],

    pub tags: Vec<String>,
    pub has_tags: bool,

    pub recommended: bool,
    pub numbering: bool,
    /// "en" or "ru": language of the fixed labels in the output
    pub lang: String,
    pub max_warnings: i64,
    pub has_max_warning: bool,
    /// Git revision the HTML file counts the changes from; "" for none
    pub changes_since: String,

    pub rules: RuleSet,
    pub custom: Vec<Custom>,
    pub trace_scan: Vec<Glob>,
    pub brand: Option<Branding>,

    resolved: OnceLock<Resolved>,
}

pub fn default(root: impl Into<PathBuf>) -> Config {
    Config {
        root: root.into(),
        file: "docc.json".into(),
        src: "docs/src".into(),
        out: "docs/out".into(),
        schemas: "docs/schemas".into(),
        out_full: "docs/out-full".into(),
        roots: vec!["index".into()],
        recommended: true,
        lang: "en".into(),
        ..Config::default()
    }
}

impl Config {
    /// Abs returns the OS path for a project-relative '/' path.
    pub fn abs(&self, rel: &str) -> PathBuf {
        clean_os(&self.root.join(from_slash(rel)))
    }
}

/// Load reads the config at file. If explicit is false and the file does not
/// exist, defaults are returned. The project root is the file's directory.
/// Config problems come back as Errors; anything else (no working directory)
/// as the plain error, as in Go.
pub fn load(
    file: &str,
    explicit: bool,
) -> Result<Config, Box<dyn std::error::Error + Send + Sync>> {
    let abs = abs_path(Path::new(file))?;
    let root = abs.parent().unwrap_or(&abs).to_path_buf();
    let data = match std::fs::read(&abs) {
        Ok(d) => d,
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound && !explicit {
                return Ok(default(root));
            }
            let msg = go_path_error(&abs, &err);
            return Err(Box::new(Errors::from(Error {
                file: file.into(),
                line: 0,
                col: 0,
                msg,
            })));
        }
    };
    Ok(parse(&data, &root, &to_slash(file))?)
}

/// abs_path is filepath.Abs: the path joined to the working directory and cleaned.
fn abs_path(p: &Path) -> std::io::Result<PathBuf> {
    if p.is_absolute() {
        return Ok(clean_os(p));
    }
    Ok(clean_os(&std::env::current_dir()?.join(p)))
}

/// clean_os is filepath.Clean for a path that starts at the root: "." and
/// empty parts dropped, ".." removes the part before it and stops at the root.
fn clean_os(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            c => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// go_path_error is the text of Go's *fs.PathError from os.ReadFile:
/// "open <path>: no such file or directory", or "read <path>: is a directory".
fn go_path_error(path: &Path, err: &std::io::Error) -> String {
    let op = if err.kind() == std::io::ErrorKind::IsADirectory {
        "read"
    } else {
        "open"
    };
    let mut text = err.to_string();
    if let Some(code) = err.raw_os_error() {
        // Rust adds " (os error N)" and capitalizes strerror; Go does neither.
        text = text
            .trim_end_matches(&format!(" (os error {code})"))
            .to_string();
        let mut cs = text.chars();
        if let Some(c) = cs.next() {
            text = c.to_lowercase().chain(cs).collect();
        }
    }
    format!("{op} {}: {text}", path.display())
}

/// to_slash is filepath.ToSlash.
fn to_slash(s: &str) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        s.to_string()
    } else {
        s.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

/// from_slash is filepath.FromSlash.
fn from_slash(s: &str) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        s.to_string()
    } else {
        s.replace('/', std::path::MAIN_SEPARATOR_STR)
    }
}

/// path_clean is Go's path.Clean: the shortest '/' path equal to p lexically.
fn path_clean(p: &str) -> String {
    let rooted = p.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..");
                }
            }
            s => parts.push(s),
        }
    }
    let body = parts.join("/");
    match (rooted, body.is_empty()) {
        (true, _) => format!("/{body}"),
        (false, true) => ".".into(),
        (false, false) => body,
    }
}

/// path_ext is Go's path.Ext: the suffix from the last '.' of the last element.
fn path_ext(p: &str) -> &str {
    for (i, b) in p.bytes().enumerate().rev() {
        match b {
            b'/' => break,
            b'.' => return &p[i..],
            _ => {}
        }
    }
    ""
}

/// go_quote is Go's %q of a string (strconv.Quote).
pub(crate) fn go_quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x0b' => out.push_str("\\v"),
            c if is_print(c) => out.push(c),
            c if (c as u32) < 0x20 || c == '\x7f' => out.push_str(&format!("\\x{:02x}", c as u32)),
            c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push_str(&format!("\\U{:08x}", c as u32)),
        }
    }
    out.push('"');
    out
}

/// is_print is unicode.IsPrint: letters, marks, numbers, punctuation, symbols
/// and the ASCII space. Rust's own printable table (used by escape_debug past
/// the first char) excludes the same categories: C*, Z* other than ' '.
fn is_print(c: char) -> bool {
    if c == ' ' || c == '\'' {
        return true;
    }
    if c.is_whitespace() {
        return false;
    }
    let s = format!("a{c}"); // escape_debug also escapes ', which Go prints as is
    s.escape_debug().to_string() == s
}

struct Walker<'a> {
    src: &'a loader::Source,
    file: &'a str,
}

impl Walker<'_> {
    fn err_at(&self, off: usize, msg: impl Into<String>) -> Error {
        let (line, col) = self.src.position(off);
        Error {
            file: self.file.to_string(),
            line,
            col,
            msg: msg.into(),
        }
    }

    fn type_err(&self, n: &loader::Node, want: &str) -> Error {
        self.err_at(n.start, format!("expected {want}"))
    }

    fn str(&self, n: &loader::Node) -> Result<String, Error> {
        if n.kind != loader::Kind::String {
            return Err(self.type_err(n, "a string"));
        }
        Ok(n.str.clone())
    }

    fn strs(&self, n: &loader::Node) -> Result<Vec<String>, Error> {
        if n.kind != loader::Kind::Array {
            return Err(self.type_err(n, "an array of strings"));
        }
        n.items.iter().map(|it| self.str(it)).collect()
    }

    fn path(&self, n: &loader::Node) -> Result<String, Error> {
        let s = path_clean(&to_slash(&self.str(n)?));
        if s == "." || s.starts_with('/') || s.starts_with("../") || s == ".." {
            return Err(self.err_at(
                n.start,
                "path must be relative to the project root and inside it",
            ));
        }
        Ok(s)
    }

    fn non_neg_int(&self, n: &loader::Node) -> Result<i64, Error> {
        if n.kind != loader::Kind::Number {
            return Err(self.type_err(n, "a non-negative integer"));
        }
        // Go: Sscanf %d (int64 range) and no '.', 'e' or 'E' in the number.
        match n.str.parse::<i64>() {
            Ok(v) if v >= 0 && !n.str.contains(['.', 'e', 'E']) => Ok(v),
            _ => Err(self.type_err(n, "a non-negative integer")),
        }
    }

    fn trace(&self, c: &mut Config, n: &loader::Node) -> Result<(), Error> {
        if n.kind != loader::Kind::Object {
            return Err(self.type_err(n, "an object"));
        }
        for m in &n.members {
            if m.key != "scan" {
                return Err(self.err_at(m.key_start, format!("unknown key {}", go_quote(&m.key))));
            }
            for mk in self.strs(&m.value)? {
                match compile_glob(&mk) {
                    Ok(g) => c.trace_scan.push(g),
                    Err(err) => {
                        return Err(self.err_at(
                            m.value.start,
                            format!("bad mask {}: {}", go_quote(&mk), regex_err_text(&err)),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn branding(&self, n: &loader::Node, root: &Path) -> Result<Branding, Error> {
        if n.kind != loader::Kind::Object {
            return Err(self.type_err(n, "an object"));
        }
        let mut b = Branding::default();
        for m in &n.members {
            let v = &m.value;
            match m.key.as_str() {
                "name" => {
                    b.name = self.str(v)?;
                    if b.name.trim().is_empty() {
                        return Err(self.err_at(v.start, "name must not be empty"));
                    }
                }
                "url" => {
                    b.url = self.str(v)?;
                    if !http_url_ok(&b.url) {
                        return Err(self.err_at(v.start, "url must be an http or https address"));
                    }
                }
                "logo" => {
                    b.logo = self.path(v)?;
                    if !LOGO_EXT.contains(&crate::text::to_lower(path_ext(&b.logo)).as_str()) {
                        return Err(self.err_at(
                            v.start,
                            "logo must be a png, jpg, jpeg, gif, svg or webp file",
                        ));
                    }
                    match std::fs::metadata(root.join(from_slash(&b.logo))) {
                        Ok(st) if !st.is_dir() => {}
                        _ => {
                            return Err(self.err_at(
                                v.start,
                                format!("logo file {} does not exist", go_quote(&b.logo)),
                            ));
                        }
                    }
                }
                "accent" | "accentLight" | "header" => {
                    let s = self.str(v)?;
                    if !COLOR_RE.is_match(&s) {
                        return Err(self
                            .err_at(v.start, format!("{} must be a colour like #57e9c7", m.key)));
                    }
                    match m.key.as_str() {
                        "accent" => b.accent = s,
                        "accentLight" => b.accent_light = s,
                        _ => b.header = s,
                    }
                }
                _ => {
                    return Err(
                        self.err_at(m.key_start, format!("unknown key {}", go_quote(&m.key)))
                    );
                }
            }
        }
        if b.name.is_empty() {
            return Err(self.err_at(n.start, "branding needs a name"));
        }
        Ok(b)
    }
}

/// Parse parses docc.json content. root is the project root.
pub fn parse(data: &[u8], root: &Path, file: &str) -> Result<Config, Errors> {
    let r = loader::parse(data);
    let w = Walker { src: &r.src, file };
    if let Some(err) = &r.err {
        return Err(w.err_at(err.offset, err.msg.clone()).into());
    }
    if !r.dups.is_empty() {
        let errs = r
            .dups
            .iter()
            .map(|d| w.err_at(d.start, format!("duplicate key {}", go_quote(&d.key))));
        return Err(Errors(errs.collect()));
    }
    let root_node = r
        .root
        .as_ref()
        .expect("loader returns a root without an error");
    let mut c = default(root);
    c.file = file.to_string();
    let mut errs = Vec::new();

    // custom rules first: rules refer to their names
    if let Some(n) = root_node.get("customRules") {
        // the other errors may only follow from it
        w.custom_rules(&mut c, n)?;
    }
    for m in &root_node.members {
        let v = &m.value;
        let res: Result<(), Error> = (|| {
            match m.key.as_str() {
                "customRules" => {}
                "src" => c.src = w.path(v)?,
                "out" => c.out = w.path(v)?,
                "schemas" => c.schemas = w.path(v)?,
                "outFull" => c.out_full = w.path(v)?,
                "roots" => {
                    let (l, col) = r.src.position(v.start);
                    c.roots_pos = [l, col];
                    c.roots = w.strs(v)?;
                    if c.roots.is_empty() {
                        return Err(w.err_at(v.start, "roots must not be empty"));
                    }
                }
                "tags" => {
                    c.has_tags = true;
                    c.tags = w.strs(v)?;
                }
                "recommended" => {
                    if v.kind != loader::Kind::Bool {
                        return Err(w.type_err(v, "true or false"));
                    }
                    c.recommended = v.str == "true";
                }
                "numbering" => {
                    if v.kind != loader::Kind::Bool {
                        return Err(w.type_err(v, "true or false"));
                    }
                    c.numbering = v.str == "true";
                }
                "lang" => {
                    c.lang = w.str(v)?;
                    if c.lang != "en" && c.lang != "ru" {
                        return Err(w.err_at(v.start, "lang must be \"en\" or \"ru\""));
                    }
                }
                "maxWarnings" => {
                    c.max_warnings = w.non_neg_int(v)?;
                    c.has_max_warning = true;
                }
                "changesSince" => {
                    c.changes_since = w.str(v)?;
                    if c.changes_since.trim().is_empty() {
                        return Err(w.err_at(v.start, "changesSince must name a Git revision"));
                    }
                }
                "rules" => c.rules = w.rule_set(&c, v)?,
                "trace" => w.trace(&mut c, v)?,
                "branding" => c.brand = Some(w.branding(v, root)?),
                _ => return Err(w.err_at(m.key_start, format!("unknown key {}", go_quote(&m.key)))),
            }
            Ok(())
        })();
        if let Err(e) = res {
            errs.push(e);
        }
    }
    if !errs.is_empty() {
        return Err(Errors(errs));
    }
    Ok(c)
}

static COLOR_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#([0-9a-fA-F]{3}|[0-9a-fA-F]{6})$").unwrap());

/// as engine.ImageExt
const LOGO_EXT: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp"];

/// regex_err_text is the one-line reason of a regex error: the last line of
/// its text without the "error: " prefix.
fn regex_err_text(err: &regex::Error) -> String {
    let s = err.to_string();
    let last = s.lines().last().unwrap_or("").trim();
    last.strip_prefix("error: ").unwrap_or(last).to_string()
}

/// http_url_ok is the Go test `u, err := url.Parse(s); err == nil &&
/// (u.Scheme == "http" || u.Scheme == "https") && u.Host != ""`, with the
/// parts of net/url that decide it ported.
fn http_url_ok(raw: &str) -> bool {
    // Parse cuts off #frag; parse checks control bytes before it only.
    let (u, frag) = raw.split_once('#').unwrap_or((raw, ""));
    if u.bytes().any(|b| b < b' ' || b == 0x7f) {
        return false;
    }
    // getScheme
    let Some(colon) = u.find(':') else {
        return false;
    };
    let scheme = &u[..colon];
    let first_ok = scheme
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic());
    if !first_ok
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
    {
        return false;
    }
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let rest = &u[colon + 1..];
    let rest = rest.split_once('?').map_or(rest, |(r, _)| r);
    // Without "//" the URL is opaque or has an empty host.
    let Some(authority) = rest.strip_prefix("//") else {
        return false;
    };
    let (authority, path) = authority
        .find('/')
        .map_or((authority, ""), |i| authority.split_at(i));
    let (userinfo, host) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    if host.is_empty() || !parse_host(host) {
        return false;
    }
    if let Some(ui) = userinfo {
        let ok = ui
            .chars()
            .all(|r| r.is_ascii_alphanumeric() || "-._:~!$&'()*+,;=%@".contains(r));
        let (user, pass) = ui.split_once(':').unwrap_or((ui, ""));
        if !ok || !unescape_ok(user, Mode::Other) || !unescape_ok(pass, Mode::Other) {
            return false;
        }
    }
    unescape_ok(path, Mode::Other) && unescape_ok(frag, Mode::Other)
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Host,
    Zone,
    Other,
}

/// host_byte_ok is !shouldEscape(c, encodeHost) for an ASCII byte.
fn host_byte_ok(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!$&'()*+,;=:[]<>\"-_.~".contains(&c)
}

/// unescape_ok reports whether net/url's unescape accepts s in the mode.
fn unescape_ok(s: &str, mode: Mode) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            if i + 2 >= b.len() || !b[i + 1].is_ascii_hexdigit() || !b[i + 2].is_ascii_hexdigit() {
                return false;
            }
            let hex = |c: u8| (c as char).to_digit(16).unwrap() as u8;
            let pct25 = &b[i..i + 3] == b"%25";
            if mode == Mode::Host && hex(b[i + 1]) < 8 && !pct25 {
                return false;
            }
            if mode == Mode::Zone {
                let v = hex(b[i + 1]) << 4 | hex(b[i + 2]);
                if !pct25 && v != b' ' && !host_byte_ok(v) {
                    return false;
                }
            }
            i += 3;
            continue;
        }
        if mode != Mode::Other && b[i] < 0x80 && !host_byte_ok(b[i]) {
            return false;
        }
        i += 1;
    }
    true
}

/// parse_host is net/url's parseHost for an http(s) URL: whether it succeeds.
fn parse_host(host: &str) -> bool {
    match host.rfind('[') {
        Some(i) if i > 0 => false,
        Some(_) => {
            let Some(close) = host.rfind(']') else {
                return false;
            };
            let colon_port = &host[close + 1..];
            if !valid_optional_port(colon_port) || !unescape_ok(colon_port, Mode::Host) {
                return false;
            }
            let hostname = &host[1..close];
            let unescaped = match hostname.find("%25") {
                Some(z) => {
                    if !unescape_ok(&hostname[..z], Mode::Host)
                        || !unescape_ok(&hostname[z..], Mode::Zone)
                    {
                        return false;
                    }
                    unescape(hostname)
                }
                None => {
                    if !unescape_ok(hostname, Mode::Host) {
                        return false;
                    }
                    unescape(hostname)
                }
            };
            // netip.ParseAddr and not IPv4: an IPv6 address, maybe with a non-empty %zone.
            let addr = match unescaped.split_once('%') {
                Some((_, "")) => return false,
                Some((a, _)) => a,
                None => &unescaped[..],
            };
            addr.parse::<std::net::Ipv6Addr>().is_ok()
        }
        None => {
            // Strict colons for http and https: the port starts at the first ':'.
            if let Some(i) = host.find(':')
                && !valid_optional_port(&host[i..])
            {
                return false;
            }
            unescape_ok(host, Mode::Host)
        }
    }
}

/// unescape decodes %XX of a string unescape_ok accepted.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn valid_optional_port(port: &str) -> bool {
    match port.strip_prefix(':') {
        Some(d) => d.bytes().all(|b| b.is_ascii_digit()),
        None => port.is_empty(),
    }
}

#[cfg(test)]
mod tests;
