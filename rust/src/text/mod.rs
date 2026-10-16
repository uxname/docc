//! Package text holds the word, phrase and sentence logic of the text and style
//! rules. Everything works on one string value and returns byte offsets in it.

mod dict;
mod nfc;

pub use dict::{FORBIDDEN, HEDGING, PLACEHOLDERS, TIMELESS, is_dash_only};
pub use nfc::{BackFn, is_nfc, nfc, nfc_map};

use regex::Regex;
use std::sync::LazyLock;

/// ToLower is Go's strings.ToLower: every rune is mapped to its simple lower
/// case, so "İ" becomes "i" and the byte length can change. Callers of
/// find_lower use it to build the lower-cased text.
pub fn to_lower(s: &str) -> String {
    s.chars()
        .map(|c| if c == '\u{130}' { 'i' } else { c.to_lowercase().next().unwrap_or(c) })
        .collect()
}

// A combining mark belongs to the word: "й" can be и + U+0306.
fn is_word_rune(r: char) -> bool {
    if r.is_ascii() {
        return r.is_ascii_alphanumeric() || r == '_';
    }
    if ('\u{400}'..='\u{481}').contains(&r) {
        return true; // Cyrillic letters, the common case
    }
    // unicode.IsLetter, unicode.IsDigit and the Mn and Mc categories of Go.
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}\p{Nd}\p{Mn}\p{Mc}]$").unwrap());
    WORD.is_match(r.encode_utf8(&mut [0; 4]))
}

fn is_joiner(r: char) -> bool {
    r == '-' || r == '\'' || r == '’'
}

/// The rune that starts at byte offset i of s.
fn rune_at(s: &str, i: usize) -> char {
    s[i..].chars().next().unwrap()
}

// wordBefore reports whether the text just before offset i continues a word.
fn word_before(s: &str, i: usize) -> bool {
    let Some(r) = s[..i].chars().next_back() else {
        return false;
    };
    if is_word_rune(r) {
        return true;
    }
    if is_joiner(r)
        && let Some(p) = s[..i - r.len_utf8()].chars().next_back()
    {
        return is_word_rune(p);
    }
    false
}

// wordAfter reports whether the text starting at offset i continues a word.
fn word_after(s: &str, i: usize) -> bool {
    let mut it = s[i..].chars();
    let Some(r) = it.next() else {
        return false;
    };
    if is_word_rune(r) {
        return true;
    }
    if is_joiner(r)
        && let Some(nx) = it.next()
    {
        return is_word_rune(nx);
    }
    false
}

/// Word is a word token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word<'a> {
    pub start: usize,
    pub end: usize,
    pub text: &'a str,
}

/// Words splits s into words: runs of letters, digits and '_', with '-' or an
/// apostrophe allowed inside a word.
pub fn words(s: &str) -> Vec<Word<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let r = rune_at(s, i);
        if !is_word_rune(r) {
            i += r.len_utf8();
            continue;
        }
        let mut j = i + r.len_utf8();
        while j < s.len() {
            let r2 = rune_at(s, j);
            let n2 = r2.len_utf8();
            if is_word_rune(r2) {
                j += n2;
                continue;
            }
            if is_joiner(r2) && j + n2 < s.len() && is_word_rune(rune_at(s, j + n2)) {
                j += n2;
                continue;
            }
            break;
        }
        out.push(Word { start: i, end: j, text: &s[i..j] });
        i = j;
    }
    out
}

/// Matcher finds entries of a word list in text. An entry is a word or a phrase
/// compared without case; a trailing * on a word makes it a prefix.
pub struct Matcher {
    entries: Vec<Entry>,
}

struct Part {
    s: String, // lower case
    prefix: bool,
}

struct Entry {
    text: String,
    parts: Vec<Part>,
    re: Regex,         // slow path, used when lower-casing changes the text length
    start_bound: bool, // the entry starts with a word character
    end_bound: bool,   // the entry ends with a word character and no star
}

/// NewMatcher compiles word-list entries; empty entries are skipped.
pub fn new_matcher<S: AsRef<str>>(list: &[S]) -> Matcher {
    let mut m = Matcher { entries: Vec::new() };
    for e in list {
        let e = nfc(e.as_ref().trim());
        if e.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        let mut re = String::from("(?i)");
        for (i, p) in e.split_whitespace().enumerate() {
            if i > 0 {
                re.push_str(r"[\t\n\f\r ]+"); // Go's \s is ASCII-only
            }
            let pre = p.ends_with('*');
            let p = p.strip_suffix('*').unwrap_or(p);
            parts.push(Part { s: to_lower(p), prefix: pre });
            re.push_str(&regex::escape(p));
            if pre {
                re.push_str(r"[\p{L}\p{N}_'’-]*");
            }
        }
        let first = e.chars().next().unwrap();
        let last = e.chars().next_back().unwrap();
        m.entries.push(Entry {
            parts,
            re: Regex::new(&re).unwrap(),
            start_bound: is_word_rune(first),
            end_bound: is_word_rune(last) && !e.ends_with('*'),
            text: e,
        });
    }
    m
}

/// Hit is one match of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
    pub entry: String,
}

impl Matcher {
    /// Find returns all matches in s ordered by position. The text is compared in
    /// NFC, so a word typed with combining marks ("й" as и + U+0306) is found too;
    /// the offsets are those of s.
    pub fn find(&self, s: &str) -> Vec<Hit> {
        self.find_lower(s, &to_lower(s))
    }

    /// FindLower is Find with to_lower(s) given, so several matchers can
    /// share it.
    pub fn find_lower(&self, s: &str, lower: &str) -> Vec<Hit> {
        if is_nfc(s) {
            return self.find_in(s, lower);
        }
        let (n, back) = nfc_map(s);
        let mut hits = self.find_in(&n, &to_lower(&n));
        for h in &mut hits {
            (h.start, h.end) = back(h.start, h.end);
        }
        hits
    }

    fn find_in(&self, s: &str, lower: &str) -> Vec<Hit> {
        let fast = lower.len() == s.len();
        let mut hits = Vec::new();
        for e in &self.entries {
            if fast {
                e.find_fast(lower, &mut hits);
            } else {
                e.find_slow(s, &mut hits);
            }
        }
        hits.sort_by_key(|h| (h.start, h.end));
        hits.dedup_by(|h, prev| h.start == prev.start && h.end == prev.end); // two entries hit the same text
        hits
    }
}

/// The offset after the rune at a, or a + 1 at the end of s.
fn next_pos(s: &str, a: usize) -> usize {
    a + s[a..].chars().next().map_or(1, char::len_utf8)
}

impl Entry {
    fn bounded(&self, s: &str, a: usize, b: usize) -> bool {
        let start_ok = !self.start_bound || !word_before(s, a);
        let end_ok = !self.end_bound || !word_after(s, b);
        start_ok && end_ok
    }

    // findFast searches the lower-cased text with str::find.
    fn find_fast(&self, lower: &str, hits: &mut Vec<Hit>) {
        let first = &self.parts[0];
        let mut pos = 0;
        while pos <= lower.len() {
            let Some(i) = lower[pos..].find(first.s.as_str()) else {
                break;
            };
            let a = pos + i;
            let mut b = a + first.s.len();
            if first.prefix {
                b = extend(lower, b);
            }
            let mut ok = true;
            for p in &self.parts[1..] {
                let mut k = b;
                while k < lower.len() {
                    let r = rune_at(lower, k);
                    if !r.is_whitespace() {
                        break;
                    }
                    k += r.len_utf8();
                }
                if k == b || !lower[k..].starts_with(p.s.as_str()) {
                    ok = false;
                    break;
                }
                b = k + p.s.len();
                if p.prefix {
                    b = extend(lower, b);
                }
            }
            if ok && self.bounded(lower, a, b) {
                hits.push(Hit { start: a, end: b, entry: self.text.clone() });
                // Go stays at b, which loops forever on an empty hit (entry "*"); see rust/diff/text.md.
                pos = if b > a { b } else { next_pos(lower, a) };
                continue;
            }
            pos = next_pos(lower, a);
        }
    }

    // findSlow uses the case-insensitive regular expression.
    fn find_slow(&self, s: &str, hits: &mut Vec<Hit>) {
        let mut pos = 0;
        while pos <= s.len() {
            let Some(loc) = self.re.find(&s[pos..]) else {
                break;
            };
            let (a, b) = (pos + loc.start(), pos + loc.end());
            if !self.bounded(s, a, b) {
                pos = next_pos(s, a);
                continue;
            }
            hits.push(Hit { start: a, end: b, entry: self.text.clone() });
            // Go moves one byte past an empty hit, into the middle of a rune; this moves one rune.
            pos = if b > a { b } else { next_pos(s, a) };
        }
    }
}

// extend moves b over the rest of a word for a prefix part.
fn extend(s: &str, mut b: usize) -> usize {
    while b < s.len() {
        let r = rune_at(s, b);
        if !is_word_rune(r) && !is_joiner(r) {
            break;
        }
        b += r.len_utf8();
    }
    b
}

/// Without removes hits that overlap an allow-list match.
pub fn without(hits: Vec<Hit>, allow: &[Hit]) -> Vec<Hit> {
    if allow.is_empty() {
        return hits;
    }
    hits.into_iter()
        .filter(|h| !allow.iter().any(|a| h.start < a.end && a.start < h.end))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found<'a>(m: &Matcher, s: &'a str) -> Vec<&'a str> {
        m.find(s).iter().map(|h| &s[h.start..h.end]).collect()
    }

    #[test]
    fn test_words() {
        let got: Vec<&str> = words("По-видимому, it's 3 раза_два; x- -y").iter().map(|w| w.text).collect();
        assert_eq!(got, ["По-видимому", "it's", "3", "раза_два", "x", "y"]);
    }

    #[test]
    fn test_matcher() {
        let m = new_matcher(&["быстро", "быстр*", "и т.д.", "как правило", "user-friendly", "etc."]);
        let cases: &[(&str, &[&str])] = &[
            ("Работает БЫСТРО.", &["БЫСТРО"]), // exact and prefix entries hit the same text
            ("быстрота", &["быстрота"]),
            ("небыстро", &[]),
            ("Файлы, ссылки и т.д. и прочее", &["и т.д."]),
            ("Как  правило, да", &["Как  правило"]),
            ("A user-friendly UI, etc.", &["user-friendly", "etc."]),
            ("user-friendlyish", &[]),
            ("быстро-быстро", &["быстро-быстро"]), // prefix entry: the hyphenated word starts with быстр
        ];
        for (input, want) in cases {
            assert_eq!(found(&m, input), *want, "{input:?}");
        }
    }

    #[test]
    fn test_prefix_and_allow() {
        let m = new_matcher(&["быстр*"]);
        let allow = new_matcher(&["быстрорастворимый"]);
        let s = "быстрорастворимый кофе и быстрая доставка";
        let hits = without(m.find(s), &allow.find(s));
        assert!(hits.len() == 1 && &s[hits[0].start..hits[0].end] == "быстрая", "{hits:?}");
    }

    #[test]
    fn test_placeholders() {
        let m = new_matcher(&["TODO", "TBD", "FIXME", "lorem", "???"]);
        assert_eq!(
            found(&m, "todo: fix; TBD, Lorem ipsum ??? and todos, xtbd"),
            ["todo", "TBD", "Lorem", "???"]
        );
    }

    // Lower-casing U+0130 changes the byte length, which forces the slow path;
    // both paths must give the same answer.
    #[test]
    fn test_slow_path_agrees() {
        let m = new_matcher(&["быстро", "быстр*", "как правило", "etc."]);
        for s in ["Как  правило, быстро и быстрая; etc.", "Работает БЫСТРО.", "небыстро"] {
            let want = found(&m, s);
            let slow = format!("İ {s}");
            let got = found(&m, &slow);
            assert_eq!(got, want, "{s:?}: slow {got:?} fast {want:?}");
        }
    }

    #[test]
    fn test_new_placeholders() {
        let m = new_matcher(PLACEHOLDERS);
        let got = found(
            &m,
            "Значение N/A, см. Выше. Всё Работает корректно, по необходимости. See above: it works correctly. NA, n/ab",
        );
        assert_eq!(got, ["N/A", "см. Выше", "Работает корректно", "по необходимости", "See above", "works correctly"]);
        for (s, want) in [(" — ", true), ("-", true), ("–", true), ("a — b", false), ("--", false), ("", false)] {
            assert_eq!(is_dash_only(s), want, "IsDashOnly({s:?}) = {}", !want);
        }
    }

    // A word typed with a combining mark (й as и + U+0306, é as e + U+0301) is
    // found, and the offsets cover the original bytes.
    #[test]
    fn test_matcher_nfc() {
        let m = new_matcher(&["новый", "café"]);
        let s = "Это новыи\u{0306} и cafe\u{0301} и новый.";
        assert_eq!(found(&m, s), ["новыи\u{0306}", "cafe\u{0301}", "новый"]);
        assert_eq!(found(&new_matcher(&["новыи\u{0306}"]), "это новый"), ["новый"], "decomposed entry");
    }

    // An entry that is only "*" hangs Go; here it ends (rust/diff/text.md).
    #[test]
    fn star_entry_ends() {
        assert_eq!(new_matcher(&["*"]).find("ab").len(), 2);
    }
}
