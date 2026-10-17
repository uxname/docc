use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use super::{Config, Custom, Entry, Error, RuleSet, Walker, go_quote, regex_err_text};
use crate::diag::Level;
use crate::loader::{Kind, Member, Node};
use crate::rules;
use crate::text;

/// allowed_params lists the parameters each rule accepts besides "level".
fn allowed_params(code: &str) -> &'static [&'static str] {
    match code {
        "DOC301" | "DOC352" | "DOC353" => &["words", "allow"],
        "DOC313" => &["allow"],
        _ => &[],
    }
}

fn parse_level(s: &str) -> Option<Level> {
    match s {
        "error" => Some(Level::Error),
        "warn" => Some(Level::Warn),
        "off" => Some(Level::Off),
        _ => None,
    }
}

/// KeyRef is what a key of the rules section names.
enum KeyRef {
    /// rule code or custom name, and whether the rule is core
    Rule(String, bool),
    Group(String),
}

impl Walker<'_> {
    fn level(&self, n: &Node) -> Result<Level, Error> {
        if n.kind == Kind::String
            && let Some(l) = parse_level(&n.str)
        {
            return Ok(l);
        }
        Err(self.err_at(n.start, r#"level must be "error", "warn" or "off""#))
    }

    /// resolve_key maps a key of the rules section to a rule identity:
    /// the entry key (rule code or custom name), or the group.
    fn resolve_key(&self, c: &Config, m: &Member) -> Result<KeyRef, Error> {
        let k = &m.key;
        if rules::is_group(k) {
            return Ok(KeyRef::Group(k.clone()));
        }
        if rules::is_custom_name(k) {
            if c.custom.iter().any(|cr| &cr.id == k) {
                return Ok(KeyRef::Rule(k.clone(), false));
            }
            return Err(self.err_at(m.key_start, format!("unknown custom rule {}", go_quote(k))));
        }
        if let Some(r) = rules::lookup(k) {
            return Ok(KeyRef::Rule(r.code.to_string(), r.core));
        }
        Err(self.err_at(
            m.key_start,
            format!("unknown rule or group {}", go_quote(k)),
        ))
    }

    pub(super) fn rule_set(&self, c: &Config, n: &Node) -> Result<RuleSet, Error> {
        let mut rs = RuleSet::default();
        if n.kind != Kind::Object {
            return Err(self.type_err(n, "an object"));
        }
        let mut seen_group = HashSet::new();
        for m in &n.members {
            let rk = match self.resolve_key(c, m)? {
                KeyRef::Group(grp) => {
                    if !seen_group.insert(grp.clone()) {
                        return Err(self.err_at(
                            m.key_start,
                            format!("duplicate entry for group {}", go_quote(&grp)),
                        ));
                    }
                    let l = self.level(&m.value)?;
                    rs.by_group.insert(grp, l);
                    continue;
                }
                KeyRef::Rule(_, true) => {
                    return Err(self.err_at(
                        m.key_start,
                        format!("{} is a core rule and cannot be configured", m.key),
                    ));
                }
                KeyRef::Rule(rk, false) => rk,
            };
            if rs.by_rule.contains_key(&rk) {
                return Err(self.err_at(
                    m.key_start,
                    format!(
                        "rule {} is configured twice (by code and by name)",
                        go_quote(&m.key)
                    ),
                ));
            }
            let ent = self.entry(&rk, &m.value)?;
            rs.by_rule.insert(rk, ent);
        }
        Ok(rs)
    }

    fn entry(&self, rk: &str, n: &Node) -> Result<Entry, Error> {
        if n.kind == Kind::String {
            return Ok(Entry {
                level: Some(self.level(n)?),
                ..Default::default()
            });
        }
        if n.kind != Kind::Object {
            return Err(self.type_err(n, r#"a level or an object with "level" and parameters"#));
        }
        let mut ent = Entry::default();
        for m in &n.members {
            if m.key == "level" {
                ent.level = Some(self.level(&m.value)?);
                continue;
            }
            if !allowed_params(rk).contains(&m.key.as_str()) {
                return Err(self.err_at(
                    m.key_start,
                    format!("parameter {} is not supported by {rk}", go_quote(&m.key)),
                ));
            }
            match m.key.as_str() {
                "words" => {
                    ent.params.words = self.strs(&m.value)?;
                    // Go keeps looping: the error names the last bad word.
                    if let Some(x) = ent.params.words.iter().rev().find(|x| !star_at_word_end(x)) {
                        return Err(self.err_at(
                            m.value.start,
                            format!("{}: * is allowed only at the end of a word", go_quote(x)),
                        ));
                    }
                }
                "allow" => ent.params.allow = self.strs(&m.value)?,
                _ => {}
            }
        }
        Ok(ent)
    }

    pub(super) fn custom_rules(&self, c: &mut Config, n: &Node) -> Result<(), Error> {
        if n.kind != Kind::Array {
            return Err(self.type_err(n, "an array"));
        }
        let mut seen = HashMap::new();
        for it in &n.items {
            if it.kind != Kind::Object {
                return Err(self.type_err(it, "an object"));
            }
            let mut cr = Custom::default();
            let (mut have_level, mut have_forbid, mut have_require) = (false, false, false);
            for m in &it.members {
                let v = &m.value;
                match m.key.as_str() {
                    "id" => {
                        cr.id = self.str(v)?;
                        if !CUSTOM_ID.is_match(&cr.id) {
                            return Err(self.err_at(
                                v.start,
                                "id must be custom/ followed by a-z, 0-9 and '-'",
                            ));
                        }
                        if seen.insert(cr.id.clone(), true).is_some() {
                            return Err(self.err_at(
                                v.start,
                                format!("duplicate custom rule id {}", go_quote(&cr.id)),
                            ));
                        }
                    }
                    "level" => {
                        cr.level = self.level(v)?;
                        have_level = true;
                    }
                    "message" => cr.message = self.str(v)?,
                    "hint" => cr.hint = self.str(v)?,
                    "kind" => cr.kind = self.str(v)?,
                    "given" => {
                        cr.given = self.str(v)?;
                        if !cr.given.starts_with('/') {
                            return Err(self.err_at(
                                v.start,
                                "given must be a JSON pointer starting with '/'",
                            ));
                        }
                    }
                    "forbid" | "require" => {
                        let s = self.str(v)?;
                        // the text is matched in NFC too
                        let re = compile_go_regexp(&text::nfc(&s)).map_err(|err| {
                            self.err_at(v.start, format!("bad regular expression: {err}"))
                        })?;
                        cr.pattern = text::nfc(&s);
                        if m.key == "forbid" {
                            cr.forbid = Some(re);
                            have_forbid = true;
                        } else {
                            cr.require = Some(re);
                            have_require = true;
                        }
                    }
                    _ => {
                        return Err(
                            self.err_at(m.key_start, format!("unknown key {}", go_quote(&m.key)))
                        );
                    }
                }
            }
            if cr.id.is_empty() {
                return Err(self.err_at(it.start, r#"custom rule needs "id""#));
            }
            if !have_level || cr.message.is_empty() || cr.given.is_empty() {
                return Err(self.err_at(
                    it.start,
                    format!(
                        r#"custom rule {} needs "level", "message" and "given""#,
                        cr.id
                    ),
                ));
            }
            if have_forbid == have_require {
                return Err(self.err_at(
                    it.start,
                    format!(
                        "custom rule {} needs exactly one of forbid and require",
                        cr.id
                    ),
                ));
            }
            c.custom.push(cr);
        }
        Ok(())
    }
}

static CUSTOM_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^custom/[a-z0-9-]+$").unwrap());

/// star_at_word_end checks that * appears only at the end of a space-separated word.
fn star_at_word_end(s: &str) -> bool {
    s.split_whitespace()
        .all(|p| p.find('*').is_none_or(|i| i == p.len() - 1))
}

/// compile_go_regexp compiles a pattern written for Go's regexp with Go's
/// meaning: \d \w \s and \b are ASCII-only there and Unicode in the regex
/// crate, so they are rewritten as explicit ASCII classes; \Q...\E quotes, an
/// escaped punctuation character is a literal (Rust reads \< and \> as word
/// boundaries) and octal escapes \0, \12, \123 become \x{...}. Syntax Go
/// rejects and the regex crate would accept (\u0041, [[:foo:]], (?P<a.b>x))
/// fails with Go's message; the error is the text after "bad regular
/// expression: ".
pub(crate) fn compile_go_regexp(pat: &str) -> Result<Regex, String> {
    let re = go_to_rust_regexp(pat)?;
    Regex::new(&re).map_err(|e| regex_err_text(&e))
}

/// POSIX_CLASSES are the [:name:] classes Go knows.
const POSIX_CLASSES: &[&str] = &[
    "alnum", "alpha", "ascii", "blank", "cntrl", "digit", "graph", "lower", "print", "punct",
    "space", "upper", "word", "xdigit",
];

fn go_to_rust_regexp(pat: &str) -> Result<String, String> {
    const DIGIT: &str = "0-9";
    const WORD: &str = "0-9A-Za-z_";
    const SPACE: &str = r"\t\n\f\r ";
    let go_err = |code: &str, expr: &str| Err(format!("error parsing regexp: {code}: `{expr}`"));
    let cs: Vec<char> = pat.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    // in_class: inside [...]; class_first: at the first item, where ']' is literal
    let (mut in_class, mut class_first) = (false, false);
    // lo: the last class item was a single character that a '-' turns into the
    // start of a range; hi: the next class item ends a range.
    let (mut lo, mut hi) = (false, false);
    // item records a class item; single is false for \d, \p{..} and [:alpha:].
    let item = |lo: &mut bool, hi: &mut bool, single: bool| *lo = !std::mem::take(hi) && single;
    while i < cs.len() {
        let c = cs[i];
        if c == '\\' && i + 1 < cs.len() {
            let e = cs[i + 1];
            i += 2;
            let perl = match e {
                'd' => Some((DIGIT, false)),
                'D' => Some((DIGIT, true)),
                'w' => Some((WORD, false)),
                'W' => Some((WORD, true)),
                's' => Some((SPACE, false)),
                'S' => Some((SPACE, true)),
                _ => None,
            };
            let mut single = true;
            if let Some((set, neg)) = perl {
                let neg = if neg { "^" } else { "" };
                // Under (?i) both Go and Rust fold the class, so (?i)\w also matches K (Kelvin) and ſ.
                out.push_str(&format!("[{neg}{set}]"));
                single = false;
            } else if (e == 'b' || e == 'B') && !in_class {
                out.push_str(&format!("(?-u:\\{e})"));
            } else if e == 'Q' && !in_class {
                let lit: String = cs[i..].iter().collect();
                let (lit, _) = lit.split_once(r"\E").unwrap_or((&lit, ""));
                out.push_str(&regex::escape(lit));
                i += lit.chars().count() + 2;
            } else if e.is_ascii() && !e.is_ascii_alphanumeric() {
                out.push_str(&regex::escape(e.encode_utf8(&mut [0; 4])));
            } else if e == '0'
                || (('1'..='7').contains(&e) && cs.get(i).is_some_and(|d| ('0'..='7').contains(d)))
            {
                let mut v = e as u32 - '0' as u32;
                for _ in 1..3 {
                    match cs.get(i) {
                        Some(d) if ('0'..='7').contains(d) => {
                            v = v * 8 + (*d as u32 - '0' as u32);
                            i += 1;
                        }
                        _ => break,
                    }
                }
                out.push_str(&format!("\\x{{{v:x}}}"));
            } else if !(if in_class { "pPxafnrtv" } else { "AzpPxafnrtv" }).contains(e) {
                // Go knows no other escape; the regex crate takes \u0041, \U00000041 too
                return go_err("invalid escape sequence", &format!("\\{e}"));
            } else if matches!(e, 'p' | 'P') {
                single = false;
                // \p{^Greek} is Go's negation, written \P{Greek} in Rust
                let neg = cs.get(i) == Some(&'{') && cs.get(i + 1) == Some(&'^');
                out.push('\\');
                out.push(if neg {
                    if e == 'p' { 'P' } else { 'p' }
                } else {
                    e
                });
                if neg {
                    out.push('{');
                    i += 2;
                }
                if cs.get(i) == Some(&'{') {
                    while i < cs.len() {
                        out.push(cs[i]);
                        i += 1;
                        if cs[i - 1] == '}' {
                            break;
                        }
                    }
                }
            } else {
                out.push('\\');
                out.push(e);
                // \x{10FFFF} keeps its braces
                if e == 'x' && cs.get(i) == Some(&'{') {
                    while i < cs.len() {
                        out.push(cs[i]);
                        i += 1;
                        if cs[i - 1] == '}' {
                            break;
                        }
                    }
                }
            }
            if in_class {
                item(&mut lo, &mut hi, single);
            }
            class_first = false;
            continue;
        }
        i += 1;
        if !in_class {
            if c == '{' && !go_repeat(&cs[i - 1..]) {
                out.push_str(r"\{");
                continue;
            }
            if c == '(' && cs.get(i) == Some(&'?') {
                let rest: String = cs[i - 1..].iter().collect();
                if rest.starts_with("(?)") {
                    // an empty flag group changes nothing in Go and is an error in Rust
                    i += 2;
                    continue;
                }
                let name_at = if rest.starts_with("(?P<") && rest.len() > 4 {
                    4
                } else if rest.starts_with("(?<") && rest.len() > 3 {
                    3
                } else {
                    0
                };
                if name_at > 0 {
                    // Go allows names Rust does not (1a, a repeated name) and the
                    // reverse (a.b, я); names are never used, so the group goes
                    // unnamed once Go accepts the name.
                    let Some(end) = rest.find('>') else {
                        return go_err("invalid named capture", &rest);
                    };
                    let name = &rest[name_at..end];
                    if name.is_empty()
                        || !name.bytes().all(|b| b == b'_' || b.is_ascii_alphanumeric())
                    {
                        return go_err("invalid named capture", &rest[..=end]);
                    }
                    out.push('(');
                    i += rest[..=end].chars().count() - 1;
                    continue;
                }
            }
            out.push(c);
            if c == '[' {
                in_class = true;
                class_first = true;
                (lo, hi) = (false, false);
                if cs.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            continue;
        }
        match c {
            ']' if !class_first => {
                in_class = false;
                out.push(']');
            }
            '[' if cs.get(i) == Some(&':') => {
                // [:alpha:] passes through whole; without ":]" the '[' is a literal
                let rest: String = cs[i - 1..].iter().collect();
                match rest[2..].find(":]") {
                    Some(e) => {
                        let class = &rest[..e + 4];
                        let name = &class[2..class.len() - 2];
                        if !POSIX_CLASSES.contains(&name.strip_prefix('^').unwrap_or(name)) {
                            return go_err("invalid character class range", class);
                        }
                        out.push_str(class);
                        i += class.chars().count() - 1;
                        item(&mut lo, &mut hi, false);
                    }
                    None => {
                        out.push_str(r"\[");
                        item(&mut lo, &mut hi, true);
                    }
                }
            }
            // Go reads '-' as a range only right after a single character and
            // not before ']'; Rust also reads -- as set difference.
            '-' if lo && cs.get(i).is_some_and(|&n| n != ']') => {
                out.push('-');
                (lo, hi) = (false, true);
            }
            // Rust reads [ as a nested class and && ~~ -- as set operations; in Go they are literals.
            '[' | ']' | '&' | '~' | '-' => {
                out.push('\\');
                out.push(c);
                item(&mut lo, &mut hi, true);
            }
            _ => {
                out.push(c);
                item(&mut lo, &mut hi, true);
            }
        }
        class_first = false;
    }
    Ok(out)
}

/// go_repeat reports whether cs starts with a repeat Go parses: {n}, {n,} or
/// {n,m} without leading zeros. Any other '{' is a literal in Go and an error
/// in the regex crate.
fn go_repeat(cs: &[char]) -> bool {
    let int = |s: &[char]| {
        let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
        (n > 0 && !(n >= 2 && s[0] == '0')).then_some(n)
    };
    let mut i = 1;
    let Some(n) = int(&cs[i..]) else { return false };
    i += n;
    if cs.get(i) == Some(&',') {
        i += 1;
        if cs.get(i) != Some(&'}') {
            let Some(n) = int(&cs[i..]) else { return false };
            i += n;
        }
    }
    cs.get(i) == Some(&'}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_regexp_semantics() {
        // (pattern, text, Go's leftmost match or None); expected values from Go's regexp.
        let cases: &[(&str, &str, Option<&str>)] = &[
            (r"\w+", "привет word", Some("word")),
            (r"\d", "٣3", Some("3")),
            (r"\s", "a\u{a0}b c", Some(" ")),
            (r"\bx", "éx x", Some("x")),
            (r"[\w]+", "ёa_1", Some("a_1")),
            (r"[^\d]+", "12ab", Some("ab")),
            (r"(?i)\w", "\u{212a}k", Some("\u{212a}")),
            (r"\<a\>", "<a>", Some("<a>")),
            (r"\Qa.b\E.", "a.bc", Some("a.bc")),
            (r"\Qa.b", "xa.b", Some("a.b")),
            (r"\101", "A", Some("A")),
            (r"\0", "\0", Some("\0")),
            (r"[a&&b]", "&", Some("&")),
            (r"[a~~b]", "~", Some("~")),
            (r"[[:digit:]x]+", "x1", Some("x1")),
            (r"[]a]+", "]a", Some("]a")),
            (r"[^]a]+", "]ab", Some("b")),
            (r"[[]", "[", Some("[")),
            (r"\.", "a.", Some(".")),
            (r"\p{Cyrillic}+", "abc где", Some("где")),
            (r"a{,3}", "aa{,3}", Some("a{,3}")),
            (r"{{t:", "x{{t:y", Some("{{t:")),
            (r"a{01}", "a{01}", Some("a{01}")),
            (r"a{2}}", "aa}", Some("aa}")),
            (r"a{1,2}b", "aab", Some("aab")),
            (r"a{2,}", "aaa", Some("aaa")),
            (r"\x{41}{2}", "AAA", Some("AA")),
            (r"[\x{41}-\x{43}]+", "ABCD", Some("ABC")),
            // '-' in a class is a literal unless it ends a range; Rust reads -- as set difference
            (r"[a-z0-9--]+", "e-mail", Some("e-mail")),
            (r"[0-9--]+", "1-2", Some("1-2")),
            (r"[\w--]+", "a-b", Some("a-b")),
            (r"[\p{L}--]+", "я-b", Some("я-b")),
            (r"[\d--]", "-", Some("-")),
            (r"[^\w--]+", "a- !b", Some(" !")),
            (r"[a-z--/]+", "a-./", Some("a-./")),
            (r"[-a]+", "-a", Some("-a")),
            (r"[a-]+", "-a", Some("-a")),
            (r"[]-a]+", "^]a", Some("^]a")),
            (r"[[:a]+", "[:a", Some("[:a")),
            (r"[[:^alpha:]]+", "1a", Some("1")),
            (r"\p{^L}+", "ab12 c", Some("12 ")),
            (r"\P{^L}+", "12ab", Some("ab")),
            (r"[\p{^L}]+", "ab12", Some("12")),
            (r"a(?)b", "ab", Some("ab")),
            (r"a(?)*", "aaa", Some("aaa")),
            (r"(?<1a>x)", "x", Some("x")),
            (r"(?P<_a>x)(?P<_a>y)", "xy", Some("xy")),
            (r"\a\v", "\x07\x0b", Some("\x07\x0b")),
            (r"\Az", "zz", Some("z")),
        ];
        for (p, s, want) in cases {
            let re = compile_go_regexp(p).unwrap_or_else(|e| panic!("{p}: {e}"));
            assert_eq!(
                re.find(s).map(|m| m.as_str()),
                *want,
                "{p} on {s:?} ({:?})",
                go_to_rust_regexp(p)
            );
        }
        for p in [
            "(",
            "a)",
            "[a",
            r"\1",
            "x{2,1}",
            r"[\w.--]",
            "[–—--]",
            r"[a-\d]",
            r"[a-\x43]",
        ] {
            assert!(compile_go_regexp(p).is_err(), "{p} must fail as in Go");
        }
        // the messages Go gives for syntax the regex crate would accept
        for (p, want) in [
            (r"\U00000041", r"invalid escape sequence: `\U`"),
            (r"\u{5c}u0041", r"invalid escape sequence: `\u`"),
            (r"\e", r"invalid escape sequence: `\e`"),
            (r"[\A]", r"invalid escape sequence: `\A`"),
            (r"[\b]", r"invalid escape sequence: `\b`"),
            (r"[\z]", r"invalid escape sequence: `\z`"),
            (r"[\Q]", r"invalid escape sequence: `\Q`"),
            ("[[:foo:]]", "invalid character class range: `[:foo:]`"),
            ("(?P<a.b>x)", "invalid named capture: `(?P<a.b>`"),
            ("(?P<я>x)", "invalid named capture: `(?P<я>`"),
            ("(?P<ab", "invalid named capture: `(?P<ab`"),
        ] {
            assert_eq!(
                compile_go_regexp(p).err(),
                Some(format!("error parsing regexp: {want}")),
                "{p}"
            );
        }
    }

    #[test]
    fn star_at_word_end_cases() {
        assert!(star_at_word_end("foo* bar"));
        assert!(!star_at_word_end("f*o"));
        assert!(!star_at_word_end("a *b"));
    }
}
