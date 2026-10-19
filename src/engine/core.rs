use std::collections::HashSet;

use super::project::{Doc, Project};
use super::strings::{INPUT_KEY, RESERVED_KEYS, is_input, is_sections, reserved_key};
use crate::config::go_quote;
use crate::loader::{Kind, Node};
use crate::schema;

impl Project {
    /// checkSyntax runs the per-document structure checks: DOC103, DOC105, DOC110 and DOC111.
    /// (DOC101, DOC102 and DOC104 are emitted while loading.)
    pub(super) fn check_syntax(&self) {
        for d in &self.docs {
            let Some(root) = d.root() else {
                continue;
            };
            match root.get("title") {
                None => {
                    let mut hint = r#"Add a non-empty "title" string."#;
                    if root.get("Title").is_some() {
                        hint = r#"The field is named "title" (lowercase): rename "Title"."#;
                    }
                    self.emit(
                        d,
                        "DOC103",
                        root.start,
                        root.start + 1,
                        "the document has no title".into(),
                        hint.into(),
                    );
                }
                Some(t) if t.kind == Kind::String && t.str.trim().is_empty() => {
                    self.emit(
                        d,
                        "DOC103",
                        t.start,
                        t.end,
                        "the title is empty".into(),
                        "Write a short, meaningful title.".into(),
                    );
                }
                _ => {}
            }
            for v in self.schemas.check_builtin(root) {
                self.emit(
                    d,
                    "DOC105",
                    v.start,
                    v.end,
                    format!("document does not match the built-in schema: {}", v.message),
                    v.hint,
                );
            }
            self.reserved_typos(d);
            self.inputs(d);
        }
    }

    /// inputs reports DOC111: a form field that is malformed, has an id used
    /// before in the document, or stands where the Markdown would print it as a list.
    fn inputs(&self, d: &Doc) {
        fn visit<'n>(n: &'n Node, para: bool, item: bool, sec: bool, out: &mut Vec<(&'n Node, bool)>) {
            if is_input(n) {
                out.push((n, item));
                return;
            }
            match n.kind {
                Kind::Array => {
                    // an array of paragraphs: the value of a field or section, strings and fields only
                    let ok = para && n.items.iter().all(|it| it.kind == Kind::String || is_input(it));
                    for it in &n.items {
                        visit(it, false, ok, false, out);
                    }
                }
                Kind::Object => {
                    let s = sec && is_sections(n);
                    for m in &n.members {
                        visit(&m.value, s, false, s, out);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        for m in &d.r().members {
            if m.key == "body" || !reserved_key(&m.key) {
                visit(&m.value, true, false, m.key != "body", &mut found);
            }
        }
        let mut ids = HashSet::new();
        for (n, item) in found {
            let (msg, hint) = input_problem(n, item, &mut ids);
            if !msg.is_empty() {
                self.emit(d, "DOC111", n.start, n.end, msg, hint.into());
            }
        }
    }
    /// reservedTypos reports DOC110: a free field whose name is one or two edits
    /// from a reserved key ("parnet", "hiden"), which would silently be prose.
    fn reserved_typos(&self, d: &Doc) {
        let root = d.r();
        let kind = root.get("kind");
        for m in &root.members {
            if reserved_key(&m.key) || equal_fold(&m.key, "title") {
                continue; // a wrong-case title is DOC103
            }
            if let Some(k) = kind
                && self.schemas.declares(&k.str, &m.key)
            {
                continue;
            }
            let (c, dist) = schema::closest(&m.key, &RESERVED_KEYS);
            if c.is_empty()
                || m.key.chars().count() < 3
                || (dist != 1 && !(dist == 2 && same_letters(&m.key, &c)))
            {
                continue; // one edit, or two letters swapped; "other" or "3 notes" are prose fields
            }
            self.emit(
                d,
                "DOC110",
                m.key_start,
                m.key_end,
                format!("field {} looks like a misspelled {}", go_quote(&m.key), go_quote(&c)),
                format!(
                    "Did you mean {}? Rename it, or declare {} in the schema of the kind if it is meant.",
                    go_quote(&c),
                    go_quote(&m.key)
                ),
            );
        }
    }
}

/// input_problem is the first thing wrong with a form field, ("", "") when
/// nothing is; ids collects the ids of the document.
fn input_problem(n: &Node, item: bool, ids: &mut HashSet<String>) -> (String, &'static str) {
    const SHAPE: &str = r#"A form field is {"$input": "radio", "id": "export", "label": "Export?", "options": ["Yes", "No"]}; $input is text, radio or checkbox."#;
    if !item {
        return (
            "a form field stands only as an item of an array of paragraphs".into(),
            r#"Put it into the array of paragraphs of body, a free field or a section: "body": ["Text.", {"$input": ...}]."#,
        );
    }
    if let Some(m) = n.members.iter().find(|m| !matches!(m.key.as_str(), INPUT_KEY | "id" | "label" | "options")) {
        return (format!("form field has an unknown member {}", go_quote(&m.key)), SHAPE);
    }
    let kind = n.get(INPUT_KEY).filter(|k| k.kind == Kind::String).map_or("", |k| k.str.as_str());
    if !matches!(kind, "text" | "radio" | "checkbox") {
        return ("form field kind must be \"text\", \"radio\" or \"checkbox\"".into(), SHAPE);
    }
    let id = match n.get("id") {
        Some(v) if v.kind == Kind::String && input_id(&v.str) => v.str.as_str(),
        _ => return ("form field needs an id of a-z, 0-9 and '-'".into(), "Give it an id such as \"export-format\": the answers are kept by it."),
    };
    if !ids.insert(id.to_string()) {
        return (
            format!("form field id {} is used twice in the document", go_quote(id)),
            "Give every form field of the document its own id.",
        );
    }
    if !n.get("label").is_some_and(|l| l.kind == Kind::String && !l.str.trim().is_empty()) {
        return ("form field needs a label".into(), "Write the question as \"label\".");
    }
    let opts = n.get("options");
    if kind == "text" {
        if opts.is_some() {
            return ("a text field has no options".into(), "Remove \"options\", or make it \"radio\" or \"checkbox\".");
        }
        return (String::new(), "");
    }
    let min = if kind == "radio" { 2 } else { 1 };
    let Some(opts) = opts.filter(|o| o.kind == Kind::Array && o.items.len() >= min) else {
        return (
            format!("form field {kind} needs at least {min} options"),
            "List the choices as \"options\": [\"Yes\", \"No\"].",
        );
    };
    let mut seen = HashSet::new();
    for o in &opts.items {
        if o.kind != Kind::String || o.str.trim().is_empty() || !seen.insert(o.str.as_str()) {
            return ("form field options must be distinct non-empty strings".into(), "Every option is a string of its own.");
        }
    }
    (String::new(), "")
}

/// input_id reports whether s is an id of a form field: a-z and 0-9 in runs joined by '-'.
fn input_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('-').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// sameLetters reports whether a and b have the same letters in another order.
fn same_letters(a: &str, b: &str) -> bool {
    let (mut x, mut y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    x.sort_unstable();
    y.sort_unstable();
    x == y
}

/// single returns the only char of a case mapping, None when it has several.
fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

/// fold maps r to a representative of its case-folding orbit in Go's
/// unicode.SimpleFold: the simple lower case of its simple upper case. The
/// Turkish dotted and dotless i fold only to themselves in Go. Checked against
/// Go 1.27 on every rune: two runes fold alike exactly when EqualFold is true.
fn fold(r: char) -> char {
    match r {
        '\0'..='\x7f' => return r.to_ascii_lowercase(),
        'А'..='Я' | 'Ѐ'..='Џ' => {
            return char::from_u32(r as u32 + if r >= 'А' { 0x20 } else { 0x50 }).unwrap();
        }
        'а'..='џ' => return r,
        '\u{130}' | '\u{131}' => return r,
        // orbits of Go that no case mapping links: ΐ ΐ, ΰ ΰ, ﬅ ﬆ
        '\u{1fd3}' => return '\u{390}',
        '\u{1fe3}' => return '\u{3b0}',
        '\u{fb05}' => return '\u{fb06}',
        _ => {}
    }
    let u = single(r.to_uppercase()).unwrap_or(r);
    single(u.to_lowercase()).unwrap_or(u)
}

/// equal_fold is Go's strings.EqualFold: equal under simple Unicode case folding.
pub(super) fn equal_fold(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (a.chars(), b.chars());
    loop {
        match (x.next(), y.next()) {
            (None, None) => return true,
            (Some(p), Some(q)) if p == q || fold(p) == fold(q) => {}
            _ => return false,
        }
    }
}
