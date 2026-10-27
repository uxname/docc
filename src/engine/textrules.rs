use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;

use super::core::equal_fold;
use super::project::{Doc, Project};
use super::strings::{SECTIONS_KEY, StrNode, is_letter, is_sections, service_field};
use crate::config::go_quote;
use crate::loader::{Kind, Node};
use crate::text::{self, Matcher};

impl Project {
    /// matcher returns a compiled word list, cached by content.
    fn matcher<S: AsRef<str>>(&self, builtin: &[S], extra: &[String]) -> Rc<Matcher> {
        let join = |l: &mut String, xs: &mut dyn Iterator<Item = &str>| {
            for (i, x) in xs.enumerate() {
                if i > 0 {
                    l.push('\x00');
                }
                l.push_str(x);
            }
        };
        let mut key = String::new();
        join(&mut key, &mut builtin.iter().map(AsRef::as_ref));
        key.push('\x01');
        join(&mut key, &mut extra.iter().map(String::as_str));
        if let Some(m) = self.matchers.borrow().get(&key) {
            return m.clone();
        }
        let list: Vec<&str> = builtin
            .iter()
            .map(AsRef::as_ref)
            .chain(extra.iter().map(String::as_str))
            .collect();
        let m = Rc::new(text::new_matcher(&list));
        self.matchers.borrow_mut().insert(key, m.clone());
        m
    }
}

/// listRule describes a rule driven by a word list.
struct ListRule {
    code: &'static str,
    builtin: &'static [&'static str],
    /// format with the matched text: %q is replaced
    msg: &'static str,
    hint: &'static str,
}

const LIST_RULES: [ListRule; 4] = [
    ListRule {
        code: "DOC301",
        builtin: text::FORBIDDEN,
        msg: "forbidden word or phrase %q",
        hint: "Replace it with a specific, measurable statement.",
    },
    ListRule {
        code: "DOC303",
        builtin: text::PLACEHOLDERS,
        msg: "placeholder %q is left in the text",
        hint: "Write the real text or remove the placeholder.",
    },
    ListRule {
        code: "DOC352",
        builtin: text::HEDGING,
        msg: "hedging word %q",
        hint: "State the fact plainly, or say exactly when it holds.",
    },
    ListRule {
        code: "DOC353",
        builtin: text::TIMELESS,
        msg: "time-bound word %q",
        hint: "Say what is true regardless of the moment, or give a date.",
    },
];

/// a list rule that is on, with its compiled lists
struct ListCheck {
    rule: &'static ListRule,
    m: Rc<Matcher>,
    allow: Option<Rc<Matcher>>,
    first: Prefilter,
}

/// Prefilter finds in one pass whether a lower-cased NFC text may hold an
/// entry of a list. The matcher searches the whole text once per entry, which
/// is most of the time of a check on long text; when the prefilter finds
/// nothing, the matcher would find nothing either and is not run.
pub(super) struct Prefilter(Option<Regex>);

impl Prefilter {
    /// new builds, from the entries as text::new_matcher reads them, a pattern
    /// that every hit of the matcher's fast path matches: the lower-cased
    /// words, anything but white space after a prefix word ("word*"), and
    /// white space between the words of a phrase. None: the list is empty.
    pub(super) fn new<S: AsRef<str>>(list: &[S]) -> Prefilter {
        let mut alts = Vec::new();
        for e in list {
            let e = text::nfc(e.as_ref().trim());
            if e.is_empty() {
                continue;
            }
            let mut re = String::new();
            for (i, p) in e.split_whitespace().enumerate() {
                if i > 0 {
                    re.push_str(r"\s+");
                }
                re.push_str(&regex::escape(&text::to_lower(
                    p.strip_suffix('*').unwrap_or(p),
                )));
                if p.ends_with('*') {
                    re.push_str(r"\S*");
                }
            }
            alts.push(re);
        }
        Prefilter((!alts.is_empty()).then(|| Regex::new(&alts.join("|")).unwrap()))
    }

    /// may_hit is false only when the matcher cannot find anything in seg. The
    /// matcher's fast path searches lower when seg is NFC and lower is as long
    /// as seg; any other text takes a path that is not filtered.
    pub(super) fn may_hit(&self, sp: &Prose) -> bool {
        if sp.lower.len() != sp.seg.len() || !sp.nfc {
            return true;
        }
        self.0.as_ref().is_some_and(|re| re.is_match(&sp.lower))
    }
}

/// prose is one prose segment of a string with its lower case, shared by the
/// word lists.
pub(super) struct Prose<'a> {
    start: usize,
    seg: &'a str,
    lower: String,
    nfc: bool,
}

impl Prose<'_> {
    pub(super) fn new(start: usize, seg: &str) -> Prose<'_> {
        Prose {
            start,
            seg,
            lower: to_lower(seg),
            nfc: text::is_nfc(seg),
        }
    }
}

fn prose_of(sn: &StrNode) -> Vec<Prose<'_>> {
    let s = sn.node().str.as_str();
    sn.scan()
        .prose()
        .into_iter()
        .map(|sp| Prose::new(sp.start, &s[sp.start..sp.end]))
        .collect()
}

static MANUAL_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]+(\.[0-9]+)*\.?[\t\n\f\r ]").unwrap());

impl Project {
    /// checkText runs the word-based rules on every prose string.
    pub(super) fn check_text(&self) {
        let res = self.cfg.resolve();
        let on = |code: &str| res.on(code);
        // the settings and lists do not change between strings: built once
        let mut lists = Vec::new();
        for lr in &LIST_RULES {
            if !on(lr.code) {
                continue;
            }
            let st = res.rule(lr.code);
            let extra: &[String] = if lr.code == "DOC303" {
                &[]
            } else {
                &st.params.words
            };
            let m = self.matcher(lr.builtin, extra);
            let allow =
                (!st.params.allow.is_empty()).then(|| self.matcher::<&str>(&[], &st.params.allow));
            let words: Vec<&str> = lr
                .builtin
                .iter()
                .copied()
                .chain(extra.iter().map(String::as_str))
                .collect();
            lists.push(ListCheck {
                rule: lr,
                m,
                allow,
                first: Prefilter::new(&words),
            });
        }
        let repeat = on("DOC313").then(|| res.rule("DOC313"));
        let repeat_allow = repeat
            .as_ref()
            .filter(|st| !st.params.allow.is_empty())
            .map(|st| self.matcher::<&str>(&[], &st.params.allow));
        let (on304, on303, on314) = (on("DOC304"), on("DOC303"), on("DOC314"));
        for d in &self.docs {
            if !d.parsed() {
                continue;
            }
            if self.cfg.numbering && on314 {
                self.manual_numbers(d);
            }
            for sn in d.strings() {
                if !sn.is_text() {
                    continue;
                }
                let node = sn.node();
                // the text without its block code
                let s = if sn.anchor.is_empty() { node.str.as_str() } else { &node.str[..sn.anchor_at] };
                if on304 && sn.field != "title" && s.trim().is_empty() {
                    self.emit(
                        d,
                        "DOC304",
                        node.start,
                        node.end,
                        "the string is empty".into(),
                        "Fill it in or remove the field.".into(),
                    );
                    continue;
                }
                if on303 && text::is_dash_only(s) {
                    self.emit(
                        d,
                        "DOC303",
                        node.start,
                        node.end,
                        LIST_RULES[1].msg.replace("%q", &go_quote(s.trim())),
                        "Write the real text or remove the field.".into(),
                    );
                    continue;
                }
                let segs = prose_of(sn);
                for lc in &lists {
                    self.list_hits(d, sn, &segs, lc);
                }
                if repeat.is_some() {
                    self.repeated_words(d, sn, repeat_allow.as_deref());
                }
            }
        }
    }

    fn list_hits(&self, d: &Doc, sn: &StrNode, segs: &[Prose], lc: &ListCheck) {
        for sp in segs {
            let seg = sp.seg;
            if !lc.first.may_hit(sp) {
                continue;
            }
            let mut hits = lc.m.find_lower(seg, &sp.lower);
            if let Some(allow) = &lc.allow {
                hits = text::without(hits, &allow.find_lower(seg, &sp.lower));
            }
            for h in hits {
                let (a, b) = d.range(sn.node(), sp.start + h.start, sp.start + h.end);
                self.emit(
                    d,
                    lc.rule.code,
                    a,
                    b,
                    lc.rule.msg.replace("%q", &go_quote(&seg[h.start..h.end])),
                    lc.rule.hint.into(),
                );
            }
        }
    }

    /// repeatedWords implements DOC313.
    fn repeated_words(&self, d: &Doc, sn: &StrNode, allow: Option<&Matcher>) {
        let s = sn.node().str.as_str();
        for sp in sn.scan().prose() {
            let seg = &s[sp.start..sp.end];
            let ws = text::words(seg);
            for i in 1..ws.len() {
                let (a, b) = (&ws[i - 1], &ws[i]);
                if !same_word(a.text, b.text) || !has_letter(a.text) {
                    continue;
                }
                let gap = &seg[a.end..b.start];
                if gap.is_empty() || !gap.trim_matches([' ', '\t']).is_empty() {
                    continue;
                }
                if allow.is_some_and(|m| !m.find(&seg[a.start..b.end]).is_empty()) {
                    continue;
                }
                let (ra, rb) = d.range(sn.node(), sp.start + a.start, sp.start + b.end);
                self.emit(
                    d,
                    "DOC313",
                    ra,
                    rb,
                    format!("the word {} is repeated", go_quote(a.text)),
                    "Remove the repeated word.".into(),
                );
            }
        }
    }

    /// manualNumbers implements DOC314: with numbering on, a title or a section
    /// key must not carry its own number. Field text is not checked.
    fn manual_numbers(&self, d: &Doc) {
        const MSG: &str = "the number is added by docc; remove it from the title";
        const HINT: &str =
            "Set the position with the order field; docc numbers pages and sections itself.";
        let root = d.r();
        if let Some(t) = root.get("title")
            && t.kind == Kind::String
            && MANUAL_NUMBER.is_match(&t.str)
        {
            self.emit(d, "DOC314", t.start, t.end, MSG.into(), HINT.into());
        }
        fn walk(p: &Project, d: &Doc, n: &Node) {
            for m in &n.members {
                if m.key == SECTIONS_KEY {
                    continue;
                }
                if MANUAL_NUMBER.is_match(&m.key) {
                    p.emit(d, "DOC314", m.key_start, m.key_end, MSG.into(), HINT.into());
                }
                if is_sections(&m.value) {
                    walk(p, d, &m.value);
                }
            }
        }
        for m in &root.members {
            if !service_field(&m.key) && is_sections(&m.value) {
                walk(self, d, &m.value);
            }
        }
    }
}

/// to_lower is text::to_lower with the ASCII and Cyrillic letters mapped
/// without the Unicode tables, which are slow on long Russian text.
pub(super) fn to_lower(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    s.chars()
        .map(|c| match c {
            '\0'..='\x7f' => c.to_ascii_lowercase(),
            'А'..='Я' | 'Ѐ'..='Џ' => {
                char::from_u32(c as u32 + if c >= 'А' { 0x20 } else { 0x50 }).unwrap()
            }
            'а'..='џ' => c,
            '\u{130}' => 'i',
            _ => c.to_lowercase().next().unwrap_or(c),
        })
        .collect()
}

/// same_word is strings.EqualFold of the NFC forms of a and b.
fn same_word(a: &str, b: &str) -> bool {
    if text::is_nfc(a) && text::is_nfc(b) {
        return equal_fold(a, b);
    }
    equal_fold(&text::nfc(a), &text::nfc(b))
}

fn has_letter(s: &str) -> bool {
    s.chars().any(is_letter)
}
