//! Package comments keeps the readers' comments: a flat list in the reserved
//! top-level member "$comments" of each document. It checks the shape of the
//! items (DOC112), tells whether a comment still has its block (DOC215,
//! compile), edits the member in place without touching the rest of the file
//! (docc comments apply), and reads the export of the HTML file.

use std::fmt;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::engine;
use crate::history::{self, Leaf};
use crate::loader::{self, Kind, Node};
use crate::schema;

/// KEY is the reserved top-level member that holds the comments of a document.
pub const KEY: &str = "$comments";

/// Comment is one item of "$comments", the contract Docspace codes against:
/// either a text, or an answer to the form field `input` (then `at` is the
/// pointer of the field).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Comment {
    pub id: String,
    /// JSON Pointer of the block
    pub at: String,
    /// the text of the block when the comment was written
    pub quote: String,
    /// the selected words, and a few characters around them
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fragment: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<Vec<String>>,
    pub author: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub email: String,
    /// RFC 3339
    pub created: String,
    /// RFC 3339: when docc comments apply last edited the text or the answer
    #[serde(skip_serializing_if = "String::is_empty")]
    pub edited: String,
}

/// FIELDS are the members of a comment, in the order docc writes them.
pub const FIELDS: [&str; 13] =
    ["id", "at", "quote", "fragment", "prefix", "suffix", "text", "input", "answer", "author", "email", "created", "edited"];

impl Comment {
    /// words is what the comment says: its text, or the answer given.
    pub fn words(&self) -> String {
        match (&self.text, &self.answer) {
            (Some(t), _) => t.clone(),
            (None, Some(a)) => a.join(", "),
            _ => String::new(),
        }
    }

    /// json is the item as docc writes it into a document: one line, the
    /// members in the order of FIELDS.
    pub fn json(&self) -> String {
        let v = serde_json::to_value(self).expect("a Comment always encodes");
        let parts: Vec<String> = FIELDS
            .iter()
            .filter_map(|k| v.get(*k).map(|x| format!("{}: {}", quote(k), serde_json::to_string(x).unwrap_or_default())))
            .collect();
        format!("{{{}}}", parts.join(", ")).replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// of reads the comments of a document; an item of the wrong shape (DOC112)
/// is left out.
pub fn of(root: &Node) -> Vec<Comment> {
    let Some(v) = root.get(KEY).filter(|v| v.kind == Kind::Array) else {
        return Vec::new();
    };
    v.items.iter().filter_map(of_item).collect()
}

/// of_item reads one item of "$comments", None when it is malformed.
pub fn of_item(it: &Node) -> Option<Comment> {
    if item_problem(it).is_some() {
        return None;
    }
    serde_json::from_value(loader::value(it)).ok()
}

/// check is the first thing wrong with a comment as docc would write it.
pub fn check(c: &Comment) -> Result<(), String> {
    let r = loader::parse(c.json().as_bytes());
    match r.root.as_ref().and_then(item_problem) {
        Some(p) => Err(p.msg),
        None => Ok(()),
    }
}

/// Problem is something wrong with "$comments": the byte range it covers and the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub start: usize,
    pub end: usize,
    pub msg: String,
    pub hint: String,
}

const SHAPE_HINT: &str = "A comment is {\"id\", \"at\", \"quote\", \"text\" or \"input\" and \"answer\", \"author\", \"created\"} with optional \"fragment\", \"prefix\", \"suffix\", \"email\" and \"edited\"; edit the list with docc comments apply.";

static RFC3339: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$").unwrap()
});

/// problems checks the shape of the value of "$comments" (DOC112); the ids
/// repeated across documents are checked by the engine.
pub fn problems(v: &Node) -> Vec<Problem> {
    if v.kind != Kind::Array {
        return vec![Problem {
            start: v.start,
            end: v.end,
            msg: "\"$comments\" must be an array of comments".into(),
            hint: SHAPE_HINT.into(),
        }];
    }
    let mut out: Vec<Problem> = v.items.iter().filter_map(item_problem).collect();
    let mut seen = std::collections::HashSet::new();
    for it in &v.items {
        if let Some(id) = it.get("id").filter(|n| n.kind == Kind::String)
            && !seen.insert(id.str.as_str())
        {
            out.push(Problem {
                start: id.start,
                end: id.end,
                msg: format!("comment id {} is used twice", go_quote(&id.str)),
                hint: "Give every comment its own id (docc comments apply makes one when the id is left out).".into(),
            });
        }
    }
    out
}

/// item_problem is the first thing wrong with one comment, None when it is well formed.
fn item_problem(it: &Node) -> Option<Problem> {
    let bad = |n: &Node, msg: String| Some(Problem { start: n.start, end: n.end, msg, hint: SHAPE_HINT.into() });
    if it.kind != Kind::Object {
        return bad(it, "a comment must be an object".into());
    }
    for m in &it.members {
        if !FIELDS.contains(&m.key.as_str()) {
            return Some(Problem {
                start: m.key_start,
                end: m.key_end,
                msg: format!("unknown field {} in a comment", go_quote(&m.key)),
                hint: SHAPE_HINT.into(),
            });
        }
        let ok = match m.key.as_str() {
            "answer" => m.value.kind == Kind::Array && m.value.items.iter().all(|a| a.kind == Kind::String),
            _ => m.value.kind == Kind::String,
        };
        if !ok {
            let want = if m.key == "answer" { "an array of strings" } else { "a string" };
            return bad(&m.value, format!("comment field {} must be {want}", go_quote(&m.key)));
        }
    }
    let s = |k: &str| it.get(k).map(|n| n.str.as_str());
    for k in ["id", "at", "quote", "author", "created"] {
        if it.get(k).is_none() {
            return bad(it, format!("a comment lacks {}", go_quote(k)));
        }
    }
    if s("id").unwrap_or_default().trim().is_empty() {
        return bad(it.get("id")?, "the id of a comment is empty".into());
    }
    let at = s("at").unwrap_or_default();
    if !at.starts_with('/') && !at.strip_prefix('^').is_some_and(|c| crate::codes::is_code(&c.to_ascii_lowercase())) {
        return bad(it.get("at")?, "\"at\" of a comment must be the code of its block (\"^k7f2\"), or the JSON Pointer of a block without a code (\"/body/0\")".into());
    }
    if !RFC3339.is_match(s("created").unwrap_or_default()) {
        return bad(it.get("created")?, "\"created\" of a comment must be an RFC 3339 time (\"2026-10-22T09:30:00Z\")".into());
    }
    if let Some(e) = it.get("edited").filter(|e| !RFC3339.is_match(&e.str)) {
        return bad(e, "\"edited\" of a comment must be an RFC 3339 time (\"2026-10-22T09:30:00Z\")".into());
    }
    match (it.get("text"), it.get("input"), it.get("answer")) {
        (Some(t), None, None) if t.str.trim().is_empty() => bad(t, "the text of a comment is empty".into()),
        (Some(_), None, None) => None,
        (None, Some(i), Some(_)) if i.str.trim().is_empty() => bad(i, "\"input\" of an answer is empty".into()),
        (None, Some(_), Some(a)) => {
            // what no form field takes: no value, a blank one, the same one twice
            let mut seen = std::collections::HashSet::new();
            if a.items.is_empty() {
                return bad(a, "\"answer\" of an answer is empty: give at least one value".into());
            }
            a.items.iter().find_map(|v| {
                if v.str.trim().is_empty() {
                    bad(v, "a value of \"answer\" is blank".into())
                } else if !seen.insert(v.str.as_str()) {
                    bad(v, format!("\"answer\" gives {} twice", go_quote(&v.str)))
                } else {
                    None
                }
            })
        }
        _ => bad(it, "a comment has either \"text\" or \"input\" and \"answer\" (an answer to a form field)".into()),
    }
}

/// options are the options of the form field n.
fn options(n: &Node) -> Vec<&str> {
    n.get("options").map(|o| o.items.iter().map(|i| i.str.as_str()).collect()).unwrap_or_default()
}

/// answer_problem is what the form field n cannot take in answer (the
/// checks of item_problem aside), None when it takes it: a text field and
/// radio buttons exactly one value (radio: an option or an own answer),
/// check boxes at most one value that is none of the options (the own answer).
pub fn answer_problem(n: &Node, answer: &[String]) -> Option<String> {
    let kind = n.get(engine::INPUT_KEY).map_or("", |k| k.str.as_str());
    let opts = options(n);
    let own = answer.iter().filter(|a| !opts.contains(&a.as_str())).count();
    match kind {
        "text" | "radio" if answer.len() != 1 => {
            Some(format!("a {kind} field takes exactly one answer, the answer gives {}", answer.len()))
        }
        "checkbox" if own > 1 => Some(format!(
            "a checkbox field takes at most one own answer (a value that is none of its options), the answer gives {own}"
        )),
        _ => None,
    }
}

/// tidy_answer trims the values of an answer to the form field n that are
/// not its options, as the HTML file does with what a reader types (the text
/// of a text field, the own answer); an option is kept exactly.
pub fn tidy_answer(n: &Node, answer: &mut [String]) {
    let opts = options(n);
    for a in answer.iter_mut().filter(|a| !opts.contains(&a.as_str())) {
        *a = a.trim().to_string();
    }
}

fn go_quote(s: &str) -> String {
    crate::config::go_quote(s)
}

/// Place is where a comment stands in the current text: its block reads as
/// quoted, the block was edited, or the comment lost its place (no block at
/// `at`, the selected words are not in it, or the answered field is gone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Place {
    Same,
    Changed,
    Lost,
}

/// blocks are the blocks of a document, "$comments" left out.
pub fn blocks(root: &Node) -> Vec<Leaf> {
    let pre = format!("/{KEY}");
    history::flatten(root).into_iter().filter(|l| l.ptr != pre && !l.ptr.starts_with(&format!("{pre}/"))).collect()
}

/// pointer is the JSON Pointer of the block a comment stands at: the block
/// with the code ("^k7f2", any case), or at itself when it is a pointer;
/// None when no block has the code.
pub fn pointer(root: &Node, at: &str) -> Option<String> {
    let Some(code) = at.strip_prefix('^') else {
        return Some(at.to_string());
    };
    let n = engine::find_code(root, &code.to_ascii_lowercase())?;
    history::pointers(root).get(&(n as *const Node)).cloned()
}

/// at_of is what "at" of a comment on the block at the pointer ptr is: the
/// code of the block when it has one, else the pointer.
pub fn at_of(root: &Node, ptr: &str) -> String {
    match schema::resolve_pointer(root, ptr).filter(|n| n.kind == Kind::String).and_then(|n| crate::codes::code_of(&n.str)) {
        Some(c) => format!("^{c}"),
        None => ptr.to_string(),
    }
}

/// plain is the text of a block without its code: what a quote is compared by.
fn plain(s: &str) -> &str {
    engine::split_anchor(s).0
}

/// place tells where c stands in the document root and returns the current
/// text of its block ("" when lost); title gives the title of a linked document.
pub fn place(root: &Node, c: &Comment, title: &dyn Fn(&str) -> Option<String>) -> (Place, String) {
    let Some(at) = pointer(root, &c.at) else {
        return (Place::Lost, String::new());
    };
    if let Some(input) = &c.input {
        let n = schema::resolve_pointer(root, &at)
            .filter(|n| engine::is_input(n) && n.get("id").is_some_and(|v| v.str == *input));
        return match n {
            Some(n) => {
                let t = history::text(n);
                (if t == c.quote { Place::Same } else { Place::Changed }, t)
            }
            None => (Place::Lost, String::new()),
        };
    }
    let Some(l) = blocks(root).into_iter().find(|l| l.ptr == at) else {
        return (Place::Lost, String::new());
    };
    if !c.fragment.is_empty() && !squeeze(&readable(&crate::codes::strip_link_hashes(plain(&l.text)), title)).contains(&squeeze(&c.fragment)) {
        return (Place::Lost, plain(&l.text).to_string());
    }
    let same = plain(&l.text) == plain(&c.quote);
    (if same { Place::Same } else { Place::Changed }, plain(&l.text).to_string())
}

static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]#|]+)(?:#([^\]|]*))?(?:\|([^\]]*))?\]\]").unwrap());

/// readable is the source text of a block with its links as the reader sees
/// them: the title of the page, the label, or the name of the section.
// ponytail: includes ({{id#field}}) are not expanded; a fragment selected in
// the included text reads as lost.
fn readable(s: &str, title: &dyn Fn(&str) -> Option<String>) -> String {
    LINK.replace_all(s, |c: &regex::Captures| {
        if let Some(l) = c.get(3) {
            return l.as_str().to_string();
        }
        if let Some(a) = c.get(2).map(|a| a.as_str()).filter(|a| a.contains('/')) {
            return a[a.rfind('/').unwrap() + 1..].to_string();
        }
        title(c[1].trim()).unwrap_or_else(|| c[1].to_string())
    })
    .into_owned()
}

/// squeeze keeps only the letters and digits of s, in lower case: the words
/// a reader selects on the page match the Markdown source whatever marks and
/// spaces stand between them.
fn squeeze(s: &str) -> String {
    crate::text::to_lower(s).chars().filter(|c| c.is_alphanumeric()).collect()
}

/// follow is the pointer of the block that continues the block at ptr of an
/// older version of a document (history::ops, the alignment of the history
/// and docc diff); None when that block was removed or there is none at ptr.
pub fn follow(old: &Node, cur: &Node, ptr: &str) -> Option<String> {
    let (a, b) = (blocks(old), blocks(cur));
    let i = a.iter().position(|l| l.ptr == ptr)?;
    history::ops(&a, &b).into_iter().find(|(o, _)| *o == Some(i)).and_then(|(_, n)| n).map(|j| b[j].ptr.clone())
}

/// find_input is the pointer of the form field with the given id.
pub fn find_input(n: &Node, ptr: &str, id: &str) -> Option<String> {
    if engine::is_input(n) {
        return n.get("id").is_some_and(|v| v.str == id).then(|| ptr.to_string());
    }
    let items = n.items.iter().enumerate().map(|(i, it)| (i.to_string(), it));
    let members = n.members.iter().filter(|m| m.key != KEY).map(|m| (history::escape(&m.key), &m.value));
    items.chain(members).find_map(|(k, v)| find_input(v, &format!("{ptr}/{k}"), id))
}

// ---- editing "$comments" in place ----

/// Edit is one change of the comments of a file.
pub enum Edit<'a> {
    Add(&'a Comment),
    Remove(&'a str),
    /// the comment with this id gets a new text and "edited" set to the time given
    Text(&'a str, &'a str, &'a str),
    /// the comment with this id is replaced whole (an answer given again)
    Replace(&'a str, &'a Comment),
}

/// edit applies e to the JSON text of a document and returns the new text:
/// only the "$comments" member changes, every other byte stays. A new member
/// goes last in the top-level object, indented like the file; the member is
/// removed with its last comment.
pub fn edit(src: &[u8], e: &Edit) -> Result<Vec<u8>, String> {
    let (bom, data) = match src.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(rest) => (true, rest),
        None => (false, src),
    };
    let r = loader::parse(data);
    let root = match (&r.err, &r.root) {
        (None, Some(root)) if root.kind == Kind::Object => root,
        _ => return Err("the file is not valid JSON with an object at the root".into()),
    };
    if r.dups.iter().any(|d| d.key == KEY) {
        return Err(format!("the file has {} twice", go_quote(KEY)));
    }
    let nl = if data.windows(2).any(|w| w == b"\r\n") { "\r\n" } else { "\n" };
    let member = root.members.iter().find(|m| m.key == KEY);
    let arr = member.map(|m| &m.value);
    if arr.is_some_and(|a| a.kind != Kind::Array) {
        return Err(format!("{} is not an array", go_quote(KEY)));
    }
    let items: &[Node] = arr.map_or(&[], |a| &a.items);
    let pos = |id: &str| {
        items
            .iter()
            .position(|it| it.get("id").is_some_and(|v| v.str == id))
            .ok_or_else(|| format!("no comment {} in the file", go_quote(id)))
    };
    let ws = |a: usize, b: usize| String::from_utf8_lossy(&data[a..b]).into_owned();
    // the indent of the top-level members, None for an object on one line
    let indent = root.members.first().and_then(|m| {
        let lead = &data[root.start + 1..m.key_start];
        let at = lead.iter().rposition(|&c| c == b'\n')?;
        Some(String::from_utf8_lossy(&lead[at + 1..]).into_owned())
    });
    let (start, end, text) = match e {
        Edit::Add(c) => {
            let item = c.json();
            match (member, items.last()) {
                (Some(_), Some(last)) => {
                    let sep = if items.len() >= 2 {
                        ws(items[items.len() - 2].end, last.start)
                    } else {
                        let lead = ws(arr.unwrap().start + 1, last.start);
                        if lead.contains('\n') { format!(",{lead}") } else { ", ".into() }
                    };
                    (last.end, last.end, format!("{sep}{item}"))
                }
                (Some(m), None) => (m.value.start, m.value.end, array(&[item], &indent, nl)),
                (None, _) => {
                    let Some(last) = root.members.last() else {
                        return Ok(join(bom, &[&data[..root.start + 1], format!("\"{KEY}\": [{item}]").as_bytes(), &data[root.start + 1..]]));
                    };
                    let lead = match &indent {
                        Some(ind) => format!(",{nl}{ind}"),
                        None => ", ".into(),
                    };
                    (last.value.end, last.value.end, format!("{lead}\"{KEY}\": {}", array(&[item], &indent, nl)))
                }
            }
        }
        Edit::Remove(id) => {
            let i = pos(id)?;
            if items.len() > 1 {
                if i > 0 { (items[i - 1].end, items[i].end, String::new()) } else { (items[0].start, items[1].start, String::new()) }
            } else {
                let k = root.members.iter().position(|m| m.key == KEY).unwrap();
                let m = &root.members[k];
                if k > 0 {
                    (root.members[k - 1].value.end, m.value.end, String::new())
                } else if let Some(next) = root.members.get(1) {
                    (m.key_start, next.key_start, String::new())
                } else {
                    (m.key_start, m.value.end, String::new())
                }
            }
        }
        Edit::Text(id, text, when) => {
            let it = &items[pos(id)?];
            let Some(t) = it.get("text") else {
                return Err(format!("comment {} is an answer to a form field: it has no text to edit", go_quote(id)));
            };
            // two changes inside the item: the text, and "edited" replaced or added after the last member
            let mut cut = vec![(t.start, t.end, quote(text).replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029"))];
            match it.get("edited") {
                Some(e) => cut.push((e.start, e.end, quote(when))),
                None => {
                    let ms = &it.members;
                    let sep = if ms.len() >= 2 { ws(ms[ms.len() - 2].value.end, ms[ms.len() - 1].key_start) } else { ", ".into() };
                    let end = ms.last().map_or(t.end, |m| m.value.end);
                    cut.push((end, end, format!("{sep}\"edited\": {}", quote(when))));
                }
            }
            cut.sort_by_key(|c| std::cmp::Reverse(c.0));
            let mut item = data[it.start..it.end].to_vec();
            for (a, b, s) in cut {
                item.splice(a - it.start..b - it.start, s.into_bytes());
            }
            (it.start, it.end, String::from_utf8_lossy(&item).into_owned())
        }
        Edit::Replace(id, c) => {
            let it = &items[pos(id)?];
            (it.start, it.end, c.json())
        }
    };
    Ok(join(bom, &[&data[..start], text.as_bytes(), &data[end..]]))
}

/// array is a new "$comments" array of items: one item per line, one level
/// below the member, or on one line in an object written on one line.
fn array(items: &[String], indent: &Option<String>, nl: &str) -> String {
    match indent {
        Some(ind) => format!("[{nl}{ind}{ind}{}{nl}{ind}]", items.join(&format!(",{nl}{ind}{ind}"))),
        None => format!("[{}]", items.join(", ")),
    }
}

fn join(bom: bool, parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    if bom {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

/// new_id is a fresh comment id: "c-" and 10 characters of base32 (a-z, 2-7).
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    h.write_u32(std::process::id());
    let mut v = h.finish();
    let mut s = String::from("c-");
    for _ in 0..10 {
        s.push(b"abcdefghijklmnopqrstuvwxyz234567"[(v & 31) as usize] as char);
        v >>= 5;
    }
    s
}

/// now is the current time in RFC 3339, UTC, to the second.
pub fn now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil_from_days (H. Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

// ---- the export of the HTML file ----

/// Item is one comment as the HTML file exports it: the document id, the
/// JSON Pointer of the block, the text of the block when the comment was
/// written, and the commit the file was built from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: String,
    pub doc: String,
    pub path: String,
    pub quote: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub fragment: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    pub text: String,
    /// Input is the id of the form field this comment answers; Answer is the
    /// value given (one text, the picked option, or the ticked options), and
    /// Text the same in words.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub answer: Vec<String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub edited: String,
    /// the short hash of the commit the HTML file was built from
    #[serde(skip_serializing_if = "String::is_empty")]
    pub build: String,
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

/// Marker starts the machine part of the comments copied from the HTML page
/// as text: the export JSON in base64, so a messenger cannot mangle it.
pub const MARKER: &str = "docc-comments:";

// the base64 may be folded into lines by a messenger or mail, so it runs to the end
// (Go's \s is ASCII: tab, newline, form feed, carriage return and space)
static MARKER_RE: LazyLock<regex::bytes::Regex> = LazyLock::new(|| {
    regex::bytes::Regex::new(&format!(
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
    // version 2 has answers to form fields; a file without them stays version 1
    if e.version != 1 && e.version != 2 {
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

    /// Inputs run through Go's ParseExport (src/comments_go_cases.json):
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
            // what Go accepts and docc does not (docs/rust-port/comments.md): a repeated
            // key, a key in another case, a null in place of an object; and version 2
            // (answers to form fields), which Go refuses
            if !crate::loader::parse(shown.as_bytes()).dups.is_empty()
                || shown.contains("\"version\":2")
                || ODD_KEY.is_match(&shown)
                || shown.contains("[null]")
                || &*input == b"docc-comments:bnVsbA=="
            {
                continue;
            }
            // the statuses of docc.comments.json are not read any more
            if c.err.as_deref().is_some_and(|e| ["internal", "status", "reply"].iter().any(|k| e.contains(&format!("comments.0.{k} ")))) {
                continue;
            }
            match (parse_export("t", &input), c.err) {
                // serde_json words a malformed file its own way (docs/rust-port/comments.md)
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
                    // the old statuses of docc.comments.json are not read any more
                    let mut want = want;
                    for it in want.as_array_mut().into_iter().flatten() {
                        if let Some(o) = it.as_object_mut() {
                            for k in ["status", "reply", "internal"] {
                                o.remove(k);
                            }
                        }
                    }
                    if serde_json::to_value(&items).unwrap() != want {
                        off.push(format!("{shown:?}: items differ"));
                    }
                }
                (got, want) => off.push(format!("{shown:?}: got {got:?}, Go gave {want:?}")),
            }
        }
        assert!(off.is_empty(), "{}", off.join("\n"));
    }

    fn parse(s: &str) -> Node {
        crate::loader::parse(s.as_bytes()).root.unwrap_or_else(|| panic!("not JSON: {s}"))
    }

    fn comment(id: &str, at: &str) -> Comment {
        Comment {
            id: id.into(),
            at: at.into(),
            quote: "Alpha.".into(),
            text: Some("Why?".into()),
            author: "Ann".into(),
            created: "2026-10-22T09:00:00Z".into(),
            ..Default::default()
        }
    }

    /// Adding to a file without comments appends "$comments" as its last member,
    /// indented like the file; removing the comment gives back the same bytes.
    #[test]
    fn edit_in_place() {
        for src in [
            "{\n  \"title\": \"T\",\n  \"body\": [\"Alpha.\"]\n}\n",
            "{\r\n\t\"title\": \"T\",\r\n\t\"body\": [\"Alpha.\"]\r\n}",
            "{\"title\":\"T\",\"body\":[\"Alpha.\"]}",
            "\u{feff}{ \"title\": \"T\", \"body\": [\"Alpha.\"] }\n",
        ] {
            let (a, b) = (comment("c-1", "/body/0"), comment("c-2", "/body/0"));
            let one = edit(src.as_bytes(), &Edit::Add(&a)).unwrap();
            let two = edit(&one, &Edit::Add(&b)).unwrap();
            let s = String::from_utf8(two.clone()).unwrap();
            let root = parse(s.trim_start_matches('\u{feff}'));
            assert_eq!(of(&root), vec![a.clone(), b.clone()], "{s}");
            assert!(s.starts_with(&src[..src.find("\"body\"").unwrap()]), "the members before stay: {s}");
            let ed = String::from_utf8(edit(&two, &Edit::Text("c-1", "Why \"so\"?", "2026-10-23T10:48:18Z")).unwrap()).unwrap();
            let want = s.replacen("\"text\": \"Why?\"", "\"text\": \"Why \\\"so\\\"?\"", 1).replacen(
                "\"created\": \"2026-10-22T09:00:00Z\"",
                "\"created\": \"2026-10-22T09:00:00Z\", \"edited\": \"2026-10-23T10:48:18Z\"",
                1,
            );
            assert_eq!(ed, want, "the text and a new \"edited\" after the last member, the rest stays");
            let again = String::from_utf8(edit(ed.as_bytes(), &Edit::Text("c-1", "Why \"so\"?", "2026-10-24T08:00:00Z")).unwrap()).unwrap();
            assert_eq!(again, want.replacen("2026-10-23T10:48:18Z", "2026-10-24T08:00:00Z", 1), "a second edit overwrites \"edited\"");
            let back = edit(&edit(&two, &Edit::Remove("c-2")).unwrap(), &Edit::Remove("c-1")).unwrap();
            assert_eq!(String::from_utf8(back).unwrap(), src, "the file is as it was");
            assert_eq!(String::from_utf8(edit(&two, &Edit::Remove("c-2")).unwrap()).unwrap(), String::from_utf8(one).unwrap());
        }
        let pretty = "{\n  \"title\": \"T\",\n  \"body\": [\"Alpha.\"]\n}\n";
        let s = String::from_utf8(edit(pretty.as_bytes(), &Edit::Add(&comment("c-1", "/body/0"))).unwrap()).unwrap();
        assert_eq!(
            s,
            "{\n  \"title\": \"T\",\n  \"body\": [\"Alpha.\"],\n  \"$comments\": [\n    {\"id\": \"c-1\", \"at\": \"/body/0\", \"quote\": \"Alpha.\", \"text\": \"Why?\", \"author\": \"Ann\", \"created\": \"2026-10-22T09:00:00Z\"}\n  ]\n}\n"
        );
        // the first member, and the comments of an answer have no text to edit
        let first = "{\"$comments\": [{\"id\": \"c-1\"}], \"title\": \"T\"}";
        assert_eq!(String::from_utf8(edit(first.as_bytes(), &Edit::Remove("c-1")).unwrap()).unwrap(), "{\"title\": \"T\"}");
        assert!(edit(first.as_bytes(), &Edit::Text("c-1", "x", "2026-10-23T10:48:18Z")).unwrap_err().contains("no text"));
        assert!(edit(first.as_bytes(), &Edit::Remove("c-9")).unwrap_err().contains("no comment"));
        // a replaced item keeps its place in the list, the other bytes stay
        let two = "{\"title\": \"T\", \"$comments\": [{\"id\": \"c-1\"},\n  {\"id\": \"c-2\"}]}";
        let c = comment("c-1", "/body/0");
        let got = String::from_utf8(edit(two.as_bytes(), &Edit::Replace("c-1", &c)).unwrap()).unwrap();
        assert_eq!(got, two.replacen("{\"id\": \"c-1\"}", &c.json(), 1));
    }

    #[test]
    fn shapes() {
        let probs = |s: &str| problems(parse(&format!("{{\"c\": {s}}}")).get("c").unwrap()).into_iter().map(|p| p.msg).collect::<Vec<_>>();
        let ok = r#"{"id":"c-1","at":"/body/0","quote":"q","text":"t","author":"A","created":"2026-10-22T09:00:00+03:00"}"#;
        assert!(probs(&format!("[{ok}]")).is_empty());
        let ans = r#"{"id":"c-2","at":"/body/1","quote":"q","input":"x","answer":["Yes"],"author":"","created":"2026-10-22T09:00:00.5Z"}"#;
        assert!(probs(&format!("[{ans}]")).is_empty());
        let edited = ok.replace("\"author\"", "\"edited\":\"2026-10-23T10:48:18Z\",\"author\"");
        assert!(probs(&format!("[{edited}]")).is_empty(), "\"edited\" is an RFC 3339 time like \"created\"");
        assert_eq!(probs("{}"), ["\"$comments\" must be an array of comments"]);
        assert_eq!(probs(&format!("[{ok}, {ok}]")), ["comment id \"c-1\" is used twice"]);
        for (bad, want) in [
            (ok.replace("\"text\":\"t\"", "\"text\":\"t\",\"input\":\"x\""), "a comment has either"),
            (ok.replace("\"text\":\"t\",", ""), "a comment has either"),
            (ok.replace("2026-10-22T09:00:00+03:00", "yesterday"), "RFC 3339"),
            (ok.replace("\"author\"", "\"edited\":\"yesterday\",\"author\""), "\"edited\" of a comment must be an RFC 3339 time"),
            (ok.replace("\"author\"", "\"edited\":\"\",\"author\""), "\"edited\" of a comment must be an RFC 3339 time"),
            (ok.replace("\"author\"", "\"edited\":1,\"author\""), "\"edited\" must be a string"),
            (ok.replace("/body/0", "body"), "JSON Pointer"),
            (ok.replace("\"author\":\"A\",", ""), "lacks \"author\""),
            (ok.replace("\"quote\"", "\"status\""), "unknown field \"status\""),
            (ok.replace("\"t\"", "1"), "\"text\" must be a string"),
            (ans.replace("[\"Yes\"]", "\"Yes\""), "\"answer\" must be an array"),
            (ans.replace("[\"Yes\"]", "[]"), "\"answer\" of an answer is empty"),
            (ans.replace("[\"Yes\"]", "[\"Yes\", \" \"]"), "a value of \"answer\" is blank"),
            (ans.replace("[\"Yes\"]", "[\"Yes\", \"Yes\"]"), "\"answer\" gives \"Yes\" twice"),
        ] {
            let got = probs(&format!("[{bad}]"));
            assert!(got.len() == 1 && got[0].contains(want), "{bad}: {got:?}");
        }
    }

    #[test]
    fn places() {
        let root = parse(r#"{"title":"T","body":["Alpha **beta** [[x]].","Two.",{"$input":"radio","id":"q","label":"Q?","options":["A","B"]}]}"#);
        let title = |id: &str| (id == "x").then(|| "Gamma page".to_string());
        let at = |at: &str, quote: &str, frag: &str| {
            let c = Comment { at: at.into(), quote: quote.into(), fragment: frag.into(), ..comment("c", "") };
            place(&root, &c, &title).0
        };
        assert_eq!(at("/body/1", "Two.", ""), Place::Same);
        assert_eq!(at("/body/1", "One.", ""), Place::Changed);
        assert_eq!(at("/body/0", "", "beta Gamma"), Place::Changed, "marks and links read as on the page");
        assert_eq!(at("/body/0", "", "delta"), Place::Lost, "the words are gone");
        assert_eq!(at("/body/7", "Two.", ""), Place::Lost, "no block there");
        let field = Comment { input: Some("q".into()), answer: Some(vec!["A".into()]), text: None, ..comment("c", "/body/2") };
        assert_eq!(place(&root, &field, &title).0, Place::Changed);
        let gone = Comment { input: Some("z".into()), ..field };
        assert_eq!(place(&root, &gone, &title).0, Place::Lost);
        assert_eq!(find_input(&root, "", "q").as_deref(), Some("/body/2"));
    }

    /// Each kind of field takes its shape of answer: text and radio one
    /// value, checkbox the options and at most one own answer.
    #[test]
    fn answer_shapes() {
        let root = parse(r#"{"body":[{"$input":"text","id":"t","label":"T"},{"$input":"radio","id":"r","label":"R","options":["A","B"]},{"$input":"checkbox","id":"c","label":"C","options":["A"," B "]}]}"#);
        let f = |i: usize| &root.get("body").unwrap().items[i];
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        for (i, ans, want) in [
            (0, a(&["Free text"]), None),
            (0, a(&["One", "Two"]), Some("a text field takes exactly one answer, the answer gives 2")),
            (1, a(&["A"]), None),
            (1, a(&["Own"]), None),
            (1, a(&["A", "Own"]), Some("a radio field takes exactly one answer, the answer gives 2")),
            (2, a(&["A", " B ", "Own"]), None),
            (2, a(&["Own"]), None),
            (2, a(&["A", "Own", "More"]), Some("at most one own answer (a value that is none of its options), the answer gives 2")),
        ] {
            let got = answer_problem(f(i), &ans);
            assert!(got.as_deref().map(|g| want.is_some_and(|w| g.contains(w))).unwrap_or(want.is_none()), "{ans:?}: {got:?}");
        }
        let mut ans = a(&[" B ", "  Own  "]);
        tidy_answer(f(2), &mut ans);
        assert_eq!(ans, a(&[" B ", "Own"]), "an option stays as it is, typed text is trimmed");
    }

    /// A paragraph inserted above moves the commented block down by one.
    #[test]
    fn follows_its_block() {
        let old = parse(r#"{"title":"T","body":["One.","Two.","Three."]}"#);
        let cur = parse(r#"{"title":"T","body":["Zero.","One.","Two, edited.","Three."],"$comments":[]}"#);
        assert_eq!(follow(&old, &cur, "/body/1").as_deref(), Some("/body/2"));
        assert_eq!(follow(&old, &cur, "/title").as_deref(), Some("/title"));
        assert_eq!(follow(&old, &cur, "/body/9"), None);
    }

    #[test]
    fn ids_and_time() {
        let (a, b) = (new_id(), new_id());
        assert!(a != b && a.len() == 12 && a.starts_with("c-"), "{a} {b}");
        assert!(RFC3339.is_match(&now()), "{}", now());
    }
}
