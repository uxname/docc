//! Package comments keeps the review comments left on blocks of the HTML file:
//! the export the browser downloads and docc.comments.json in the project root.

use std::fmt;
use std::io::Write as _;
use std::path::Path;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use regex::bytes::Regex;
use serde::{Deserialize, Serialize};

use crate::history;
use crate::loader::Node;
use crate::schema;

/// File is the name of the comments file in the project root.
pub const FILE: &str = "docc.comments.json";

pub const OPEN: &str = "open";
pub const DONE: &str = "done";

/// Item is one comment on a block: the document id, the JSON Pointer of the
/// block and the text of the block when the comment was written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub doc: String,
    pub path: String,
    pub quote: String,
    /// Fragment is the part of the block the reader selected, when the
    /// comment is about a few words rather than the whole block.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fragment: String,
    /// Prefix and Suffix are a few characters around the fragment, to tell
    /// one occurrence of it from another.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    pub text: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub build: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub status: String,
    /// Reply is the team's answer, shown under the comment in the next HTML file.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reply: String,
    /// Internal comments (imported with --internal) stay out of the HTML file.
    #[serde(skip_serializing_if = "is_false")]
    pub internal: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Export is the file the HTML page downloads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Export {
    pub docc: String,
    pub version: i64,
    pub comments: Vec<Item>,
}

/// Error carries the text Go's error would print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// Load reads docc.comments.json from the project root; a missing file is an
/// empty list.
pub fn load(root: &Path) -> Result<Vec<Item>, Error> {
    let path = root.join(FILE);
    let b = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error(go_path_error("open", &path, &e))),
    };
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Store {
        comments: Vec<Item>,
    }
    let s: Store = decode(FILE, &b)?;
    Ok(s.comments)
}

/// decode reads JSON into T. serde_json refuses a lone surrogate escape such
/// as "\\ud800", which a browser writes for an emoji cut in half, invalid UTF-8
/// and a null where a string belongs; Go reads the first two as U+FFFD and
/// skips the null, so a file serde_json refuses gets a second try through the
/// docc loader.
fn decode<T: serde::de::DeserializeOwned>(name: &str, b: &[u8]) -> Result<T, Error> {
    let err = match serde_json::from_slice(b) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    let r = crate::loader::parse(String::from_utf8_lossy(b).as_bytes());
    match (r.err, r.root) {
        (None, Some(root)) => {
            let mut v = crate::loader::value(&root);
            drop_nulls(&mut v);
            // a type error reads better with the line serde_json gave
            serde_json::from_value(v).map_err(|_| Error(format!("{name}: {err}")))
        }
        _ => Err(Error(format!("{name}: {err}"))),
    }
}

/// drop_nulls removes null members, which Go's decoder skips.
fn drop_nulls(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            m.values_mut().for_each(drop_nulls);
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(drop_nulls),
        _ => {}
    }
}

/// Save writes the comments to docc.comments.json, one comment per line, so a
/// change of status is a one-line diff.
pub fn save(root: &Path, items: &[Item]) -> Result<(), Error> {
    let mut b = String::from("{\"comments\": [");
    for (i, it) in items.iter().enumerate() {
        let line = marshal(it);
        if i > 0 {
            b.push(',');
        }
        b.push_str("\n  ");
        b.push_str(&line);
    }
    if !items.is_empty() {
        b.push('\n');
    }
    b.push_str("]}\n");
    write_file(&root.join(FILE), b.as_bytes())
}

/// marshal is Go's encoder with SetEscapeHTML(false): "<", ">" and "&" stay
/// as they are, U+2028 and U+2029 are still escaped. They can only occur
/// inside strings, so a plain replace is safe.
fn marshal(it: &Item) -> String {
    serde_json::to_string(it)
        .expect("an Item always marshals")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// write_file is os.WriteFile with mode 0644 (before the umask), with Go's
/// error text.
fn write_file(path: &Path, data: &[u8]) -> Result<(), Error> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o644);
    let mut f = o
        .open(path)
        .map_err(|e| Error(go_path_error("open", path, &e)))?;
    f.write_all(data)
        .map_err(|e| Error(go_path_error("write", path, &e)))
}

/// go_path_error is the text of Go's *fs.PathError: "open <path>: permission denied".
fn go_path_error(op: &str, path: &Path, err: &std::io::Error) -> String {
    let op = if err.kind() == std::io::ErrorKind::IsADirectory {
        "read"
    } else {
        op
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

/// Marker starts the machine part of the comments copied from the HTML page
/// as text: the export JSON in base64, so a messenger cannot mangle it.
pub const MARKER: &str = "docc-comments:";

// the base64 may be folded into lines by a messenger or mail, so it runs to the end
// (Go's \s is ASCII: tab, newline, form feed, carriage return and space)
static MARKER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "{}([A-Za-z0-9+/=\\t\\n\\x0C\\r ]+)",
        regex::escape(MARKER)
    ))
    .unwrap()
});

/// STD_ENCODING is Go's base64.StdEncoding: padding required, but bits after
/// the last full byte need not be zero.
static STD_ENCODING: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_allow_trailing_bits(true)
        .with_decode_padding_mode(DecodePaddingMode::RequireCanonical),
);

/// ParseExport reads what the HTML page gives away: the downloaded file, or
/// the copied text with a Marker line at its end (the last one counts: a
/// comment may quote the marker). name is for errors.
pub fn parse_export(name: &str, b: &[u8]) -> Result<Vec<Item>, Error> {
    let raw;
    let mut b = b;
    if let Some(last) = MARKER_RE.captures_iter(b).last() {
        // strings.Fields: the group holds only ASCII, so dropping its spaces is the same
        let s: Vec<u8> = last[1]
            .iter()
            .copied()
            .filter(|c| !matches!(c, b'\t' | b'\n' | 0x0C | b'\r' | b' '))
            .collect();
        raw = STD_ENCODING.decode(&s).map_err(|_| {
            Error(format!(
                "{name}: the {MARKER} line is damaged: copy the comments again"
            ))
        })?;
        b = &raw;
    } else if !String::from_utf8_lossy(b).trim_start().starts_with('{') {
        return Err(Error(format!(
            "{name}: no comments in it: give the file downloaded from the page or the text copied there (it has a {MARKER} line)"
        )));
    }
    let e: Export = decode(name, b)?;
    let path = name;
    if e.docc != "comments" {
        return Err(Error(format!(
            "{path}: not a comments file of docc (no \"docc\": \"comments\")"
        )));
    }
    if e.version != 1 {
        return Err(Error(format!("{path}: unsupported version {}", e.version)));
    }
    for (i, it) in e.comments.iter().enumerate() {
        if it.id.is_empty() || it.doc.is_empty() || it.text.is_empty() {
            return Err(Error(format!(
                "{path}: comment {} lacks id, doc or text",
                i + 1
            )));
        }
    }
    Ok(e.comments)
}

// States of a comment's block, see Locate.
/// the block still reads as quoted
pub const UNCHANGED: &str = "unchanged";
/// the quoted text stands at another pointer now
pub const MOVED: &str = "moved";
/// the block was edited
pub const CHANGED: &str = "changed";
/// no block at the pointer
pub const REMOVED: &str = "removed";
/// no such document
pub const MISSING: &str = "missing";

/// Locate finds the block of a comment in the current document (root None: the
/// document is gone): the pointer to use, its current text and the state.
pub fn locate(root: Option<&Node>, it: &Item) -> (String, String, &'static str) {
    let Some(root) = root else {
        return (it.path.clone(), String::new(), MISSING);
    };
    let mut text = String::new();
    if let Some(n) = schema::resolve_pointer(root, &it.path)
        && !it.path.is_empty()
    {
        text = history::text(n);
        if text == it.quote {
            return (it.path.clone(), text, UNCHANGED);
        }
    }
    let leaves = history::flatten(root);
    for l in &leaves {
        if l.text == it.quote {
            return (l.ptr.clone(), l.text.clone(), MOVED);
        }
    }
    // edited: the block at the pointer if it still resembles the quote, else the most similar one
    if !text.is_empty() && history::similar(&text, &it.quote) >= 0.5 {
        return (it.path.clone(), text, CHANGED);
    }
    let (mut best, mut bp, mut bt) = (0.3, "", "");
    for l in &leaves {
        let s = history::similar(&l.text, &it.quote);
        if s > best || (s == best && l.ptr == it.path) {
            (best, bp, bt) = (s, &l.ptr, &l.text);
        }
    }
    if !bp.is_empty() {
        return (bp.to_string(), bt.to_string(), CHANGED);
    }
    (it.path.clone(), String::new(), REMOVED)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The text copied from the page may have the marker quoted in a comment, and
    // a messenger may fold the base64 into lines.
    #[test]
    fn parse_export_copied_text() {
        let js = r#"{"docc":"comments","version":1,"comments":[{"id":"c1","doc":"d","text":"the docc-comments: line"}]}"#;
        let mut b = base64::engine::general_purpose::STANDARD.encode(js);
        let text = format!(
            "1. D — «q»\nthe docc-comments: line is damaged\n\nFor the team:\n{MARKER}{b}\n"
        );
        match parse_export("t", text.as_bytes()) {
            Ok(items) if items.len() == 1 => {}
            r => panic!("quoted marker: {r:?}"),
        }
        let mut folded = Vec::new();
        while b.len() > 20 {
            let rest = b.split_off(20);
            folded.push(b);
            b = rest;
        }
        folded.push(b);
        let text = format!("x\n{MARKER}{}\n", folded.join("\n"));
        match parse_export("t", text.as_bytes()) {
            Ok(items) if items.len() == 1 => {}
            r => panic!("folded base64: {r:?}"),
        }
    }

    /// Inputs run through Go's ParseExport (rust/src/comments_go_cases.json):
    /// the same items or the same error text.
    #[test]
    fn parse_export_matches_go() {
        // docc's own messages; Go's JSON syntax and type errors read differently
        const DOMAIN: [&str; 5] = [
            "t: no comments in it",
            "t: the docc-comments: line is damaged",
            "t: not a comments file",
            "t: unsupported version",
            "t: comment ",
        ];
        #[derive(serde::Deserialize)]
        struct Case {
            #[serde(rename = "in")]
            input: String,
            items: Option<serde_json::Value>,
            err: Option<String>,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("comments_go_cases.json")).unwrap();
        assert!(cases.len() > 250);
        static ODD_KEY: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r#""(?:[A-Z][A-Za-z]*|ſtatus)":"#).unwrap());
        let mut off = Vec::new();
        for c in cases {
            let input = base64::engine::general_purpose::STANDARD
                .decode(&c.input)
                .unwrap();
            let shown = String::from_utf8_lossy(&input);
            // what Go accepts and docc does not (rust/diff/comments.md): a repeated
            // key, a key in another case, a null in place of an object
            if !crate::loader::parse(shown.as_bytes()).dups.is_empty()
                || ODD_KEY.is_match(&shown)
                || shown.contains("[null]")
                || &*input == b"docc-comments:bnVsbA=="
            {
                continue;
            }
            match (parse_export("t", &input), c.err) {
                // serde_json words a malformed file its own way (rust/diff/comments.md)
                (Err(_), Some(want)) if !DOMAIN.iter().any(|d| want.starts_with(d)) => {}
                (Err(e), Some(want)) => {
                    if e.0 != want {
                        off.push(format!("{shown:?}: {e} / Go: {want}"));
                    }
                }
                (Ok(items), None) => {
                    let want = c.items.unwrap_or(serde_json::Value::Array(Vec::new()));
                    let want = if want.is_null() {
                        serde_json::Value::Array(Vec::new())
                    } else {
                        want
                    };
                    if serde_json::to_value(&items).unwrap() != want {
                        off.push(format!("{shown:?}: items differ"));
                    }
                }
                (got, want) => off.push(format!("{shown:?}: got {got:?}, Go gave {want:?}")),
            }
        }
        assert!(off.is_empty(), "{}", off.join("\n"));
    }

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("docc-comments-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The bytes Go's Save writes for the same items.
    #[test]
    fn save_matches_go() {
        let dir = tmp_dir("save");
        save(&dir, &[]).unwrap();
        assert_eq!(
            std::fs::read(dir.join(FILE)).unwrap(),
            b"{\"comments\": []}\n"
        );
        let items = vec![
            Item {
                id: "c1".into(),
                doc: "d".into(),
                path: "/a".into(),
                quote: "<b> & \u{2028} \u{2029} \x01\x7f\x08\x0c\t\n\r\"\\ é😀".into(),
                text: "t".into(),
                internal: true,
                status: DONE.into(),
                ..Default::default()
            },
            Item {
                id: "c2".into(),
                doc: "e".into(),
                text: "x".into(),
                ..Default::default()
            },
        ];
        save(&dir, &items).unwrap();
        assert_eq!(
            String::from_utf8(std::fs::read(dir.join(FILE)).unwrap()).unwrap(),
            "{\"comments\": [\n  {\"id\":\"c1\",\"doc\":\"d\",\"path\":\"/a\",\"quote\":\"<b> & \\u2028 \\u2029 \\u0001\x7f\\b\\f\\t\\n\\r\\\"\\\\ é😀\",\"text\":\"t\",\"status\":\"done\",\"internal\":true},\n  {\"id\":\"c2\",\"doc\":\"e\",\"path\":\"\",\"quote\":\"\",\"text\":\"x\"}\n]}\n"
        );
        assert_eq!(load(&dir).unwrap(), items);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Load's errors as Go prints them.
    #[test]
    fn load_matches_go() {
        let dir = tmp_dir("load");
        assert_eq!(load(&dir).unwrap(), Vec::<Item>::new());
        for (data, want) in [
            (
                r#"{"comments":"x"}"#,
                "docc.comments.json: json: cannot unmarshal string into Go struct field store.comments of type []comments.Item",
            ),
            (
                r#"{"comments":["#,
                "docc.comments.json: unexpected end of JSON input",
            ),
            (
                r#"{"comments":[{"id":1}]}"#,
                "docc.comments.json: json: cannot unmarshal number into Go struct field store.comments.0.id of type string",
            ),
        ] {
            std::fs::write(dir.join(FILE), data).unwrap();
            // Go words its decoder errors otherwise (rust/diff/comments.md): {want}
            let _ = want;
            let e = load(&dir).unwrap_err().0;
            assert!(e.starts_with("docc.comments.json: "), "{data}: {e}");
        }
        std::fs::write(
            dir.join(FILE),
            r#"{"comments":[{"id":"a","x":[{}]}],"other":1}"#,
        )
        .unwrap();
        assert_eq!(
            load(&dir).unwrap(),
            vec![Item {
                id: "a".into(),
                ..Default::default()
            }]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn locate_states() {
        let r = crate::loader::parse(br#"{"title":"T","body":["alpha beta gamma","second para"]}"#);
        let root = r.root.as_ref().unwrap();
        let at = |path: &str, quote: &str| {
            let it = Item {
                path: path.into(),
                quote: quote.into(),
                ..Default::default()
            };
            let (p, t, s) = locate(Some(root), &it);
            (p, t, s.to_string())
        };
        let want = |p: &str, t: &str, s: &str| (p.to_string(), t.to_string(), s.to_string());
        assert_eq!(
            at("/body/0", "alpha beta gamma"),
            want("/body/0", "alpha beta gamma", UNCHANGED)
        );
        assert_eq!(
            at("/body/1", "alpha beta gamma"),
            want("/body/0", "alpha beta gamma", MOVED)
        );
        assert_eq!(
            at("/body/0", "alpha beta gamme"),
            want("/body/0", "alpha beta gamma", CHANGED)
        );
        assert_eq!(
            at("/x", "second paragraph"),
            want("/body/1", "second para", CHANGED)
        );
        assert_eq!(at("/x", "zzzz"), want("/x", "", REMOVED));
        let it = Item {
            path: "/a".into(),
            ..Default::default()
        };
        assert_eq!(
            locate(None, &it),
            ("/a".to_string(), String::new(), MISSING)
        );
    }
}
