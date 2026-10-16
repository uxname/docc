use super::project::{Doc, Project};
use super::strings::{RESERVED_KEYS, reserved_key};
use crate::config::go_quote;
use crate::loader::Kind;
use crate::schema;

impl Project {
    /// checkSyntax runs the per-document structure checks: DOC103, DOC105 and DOC110.
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
