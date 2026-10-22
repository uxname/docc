//! Package rules is the registry of all rules: code, name, group, default level
//! and the text for `docc explain`. docc rules, docc explain and the
//! agent skill are built from it.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::diag;

#[derive(Debug, PartialEq, Eq)]
pub struct Rule {
    pub code: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    /// level in the recommended preset
    pub default: diag::Level,
    /// cannot be suppressed, disabled or re-levelled
    pub core: bool,
    pub desc: &'static str,
    pub bad: &'static str,
    pub good: &'static str,
}

pub const E: diag::Level = diag::Level::Error;
pub const W: diag::Level = diag::Level::Warn;

#[allow(clippy::too_many_arguments)]
const fn rule(
    code: &'static str,
    name: &'static str,
    group: &'static str,
    default: diag::Level,
    core: bool,
    desc: &'static str,
    bad: &'static str,
    good: &'static str,
) -> Rule {
    Rule { code, name, group, default, core, desc, bad, good }
}

static ALL: [Rule; 43] = [
    rule(
        "DOC101",
        "syntax/invalid-json",
        "syntax",
        E,
        true,
        "The file is not strict JSON (RFC 8259, UTF-8, an object at the root, at most 5 MB and 64 levels deep). A merge conflict marker also triggers it.",
        r#"{ "title": "A", }"#,
        r#"{ "title": "A" }"#,
    ),
    rule(
        "DOC102",
        "syntax/duplicate-key",
        "syntax",
        E,
        true,
        "An object has the same key twice; JSON parsers silently keep only one.",
        r#"{ "title": "A", "title": "B" }"#,
        r#"{ "title": "B" }"#,
    ),
    rule(
        "DOC103",
        "syntax/missing-title",
        "syntax",
        E,
        true,
        "The document has no title, or it is empty or only spaces.",
        r#"{ "kind": "note" }"#,
        r#"{ "title": "Login" }"#,
    ),
    rule(
        "DOC104",
        "syntax/invalid-id",
        "syntax",
        E,
        true,
        "The file or folder name is not a valid id: use a-z, 0-9 and '-', not starting or ending with '-', and no reserved Windows names.",
        "docs/src/My Doc.json",
        "docs/src/my-doc.json",
    ),
    rule(
        "DOC105",
        "syntax/document-schema",
        "syntax",
        E,
        true,
        "A field does not match the built-in document schema: wrong type, or a repeated element in tags.",
        r#"{ "title": "A", "tags": ["x", "x"] }"#,
        r#"{ "title": "A", "tags": ["x"] }"#,
    ),
    rule(
        "DOC106",
        "schema/violation",
        "schema",
        E,
        false,
        "The document violates the schema of its kind from docs/schemas/<kind>.json.",
        r#"{ "title": "A", "kind": "requirement" }  (schema requires "statement")"#,
        r#"{ "title": "A", "kind": "requirement", "statement": "..." }"#,
    ),
    rule(
        "DOC107",
        "schema/unregistered-tag",
        "schema",
        E,
        false,
        "A tag is not in the tags registry in docc.json.",
        r#""tags": ["memo"]"#,
        r#""tags": ["note"]"#,
    ),
    rule(
        "DOC108",
        "syntax/invalid-ignore",
        "syntax",
        E,
        true,
        "docIgnore has no reason, has a wrong shape, or targets a rule that cannot be suppressed.",
        r#""docIgnore": [{ "rule": "text/forbidden-words" }]"#,
        r#""docIgnore": [{ "rule": "text/forbidden-words", "reason": "term from the customer spec" }]"#,
    ),
    rule(
        "DOC109",
        "schema/missing-kind-schema",
        "schema",
        E,
        true,
        "The document has no kind, or there is no schema docs/schemas/<kind>.json for its kind. Every document follows a kind schema.",
        r#"{ "title": "A" }"#,
        r#"{ "title": "A", "kind": "page" }  (with docs/schemas/page.json)"#,
    ),
    rule(
        "DOC110",
        "syntax/reserved-field-typo",
        "syntax",
        W,
        false,
        "A field is named almost like a reserved key (parent, hidden, order, notes, tags, ...) and the kind schema does not declare it, so it is silently treated as prose.",
        r#"{ "title": "A", "parnet": "index" }"#,
        r#"{ "title": "A", "parent": "index" }"#,
    ),
    rule(
        "DOC111",
        "syntax/invalid-input",
        "syntax",
        E,
        false,
        "A form field ({\"$input\": ...}, drawn by the HTML file as a text box, radio buttons or check boxes) is malformed or out of place. It needs $input (text, radio or checkbox), an id (a-z, 0-9 and '-') unique in the document and a label; radio needs at least two distinct options, checkbox at least one, text none. It stands only as an item of an array of paragraphs: body, a free field or a section.",
        r#"{ "title": "A", "body": [{ "$input": "radio", "label": "Export?" }] }"#,
        r#"{ "title": "A", "body": ["Text.", { "$input": "radio", "id": "export", "label": "Export?", "options": ["Yes", "No"] }] }"#,
    ),
    rule(
        "DOC112",
        "syntax/invalid-comment",
        "syntax",
        E,
        false,
        "An item of \"$comments\" (the readers' comments kept in the document) is malformed, or its id is used by another comment of the project. A comment is {id, at (the JSON Pointer of its block), quote (the text of the block when written), text, author, created (RFC 3339)} with optional fragment, prefix, suffix (the selected words and what stands around them) and email; an answer to a form field has input (the field id) and answer (an array of strings) instead of text. An answer gives at least one value, none blank or twice, and fits its field as the field reads now: a text field and radio buttons one value (radio: an option or an own answer), check boxes at most one value that is none of the options. Edit the list with docc comments apply.",
        r#"{ "title": "A", "$comments": [{ "id": "c-1", "text": "Why?", "status": "done" }] }"#,
        r#"{ "title": "A", "$comments": [{ "id": "c-1", "at": "/body/0", "quote": "Alpha.", "text": "Why?", "author": "Ann", "created": "2026-10-22T09:00:00Z" }] }"#,
    ),
    rule(
        "DOC201",
        "graph/duplicate-id",
        "graph",
        E,
        true,
        "Two documents have the same id (file name without extension) in different folders.",
        "docs/src/a/x.json and docs/src/b/x.json",
        "docs/src/a/x.json and docs/src/b/y.json",
    ),
    rule(
        "DOC202",
        "graph/broken-ref",
        "graph",
        E,
        true,
        "parent or [[id]] (also inside notes) point to a document that does not exist.",
        r#""body": "See [[missing]].""#,
        r#""body": "See [[existing-doc]].""#,
    ),
    rule(
        "DOC203",
        "graph/orphan",
        "graph",
        E,
        true,
        "The document is not reachable from any root in roots.",
        "a document nobody links to",
        r#"link it with [[id]] from the text of a reachable document, or set its parent"#,
    ),
    rule(
        "DOC204",
        "graph/parent-cycle",
        "graph",
        E,
        true,
        "The parent chain forms a cycle, including a document that is its own parent.",
        r#"a: parent b; b: parent a"#,
        r#"a: parent b; b: parent index"#,
    ),
    rule(
        "DOC205",
        "graph/root-not-found",
        "graph",
        E,
        true,
        "A root from roots does not exist (or is hidden); an empty src gives it for the default root.",
        r#""roots": ["start"]"#,
        r#""roots": ["index"]"#,
    ),
    rule(
        "DOC209",
        "graph/suspect-link",
        "graph",
        W,
        false,
        "The target of a link ([[id]], [[id#field]], [[id#^anchor]] or a docc: tag in code) changed after the link was confirmed in docc.lock: the source may no longer match it. docc build confirms new links; docc ack confirms after a review.",
        r#"docc.lock: "a b#^limit 1f2e3d4c", then the paragraph ^limit in b is edited"#,
        r#"check a against b#^limit, then run docc ack b#^limit"#,
    ),
    rule(
        "DOC210",
        "graph/link-to-hidden",
        "graph",
        E,
        true,
        "A visible document links to a hidden one (parent, [[id]] outside notes, or an include).",
        r#"visible doc: "body": "See [[secret]]."  (secret is hidden)"#,
        r#"put the reference in notes, or make the target visible"#,
    ),
    rule(
        "DOC211",
        "graph/bad-include",
        "graph",
        E,
        true,
        "An include {{id#field}} is impossible: no document or field, a cycle, a value that is not text, or a service field.",
        r#""body": "{{b}}""#,
        r#""body": "{{b#statement}}""#,
    ),
    rule(
        "DOC212",
        "graph/duplicate-anchor",
        "graph",
        E,
        true,
        "A block anchor ^name is used twice in one document, or equals the anchor of a field or section heading of its page.",
        r#""body": ["First. ^a", "Second. ^a"]"#,
        r#""body": ["First. ^a", "Second. ^b"]"#,
    ),
    rule(
        "DOC213",
        "graph/broken-file",
        "graph",
        E,
        true,
        "A Markdown image ![alt](path) with a relative path names a file that does not exist in src, lies outside src, or is not an image docc copies (png, jpg, jpeg, gif, svg, webp). The path is relative to the folder of the document.",
        r#""body": "![Scheme](img/missing.png)""#,
        r#""body": "![Scheme](img/scheme.png)"  (docs/src/img/scheme.png exists)"#,
    ),
    rule(
        "DOC214",
        "graph/wrong-target-kind",
        "graph",
        E,
        false,
        "A reference in a field whose kind schema sets \"x-docc-target\" (or the parent value) points to a document of another kind.",
        r#""parent": "note-a"  (schema: "parent": { "x-docc-target": ["goal"] })"#,
        r#""parent": "goal-a"  (kind goal)"#,
    ),
    rule(
        "DOC215",
        "graph/comment-lost",
        "graph",
        W,
        false,
        "A comment of \"$comments\" lost its place: no block stands at its pointer \"at\" any more (a paragraph was inserted, moved or removed above it), the words it was left on (\"fragment\") are not in that block, or the form field it answers is gone. Point \"at\" to the block the comment is about now, or delete the comment when it is handled (docc comments apply).",
        r#""body": ["New first.", "Alpha."], "$comments": [{ "at": "/body/0", "fragment": "Alpha", ... }]"#,
        r#""body": ["New first.", "Alpha."], "$comments": [{ "at": "/body/1", "fragment": "Alpha", ... }]"#,
    ),
    rule(
        "DOC301",
        "text/forbidden-words",
        "text",
        W,
        false,
        "Forbidden words and phrases from the list (built-in plus words in docc.json).",
        r#""body": "The app is fast.""#,
        r#""body": "The app answers in under 200 ms.""#,
    ),
    rule(
        "DOC303",
        "text/placeholder",
        "text",
        W,
        false,
        "A placeholder is left: TODO, TBD, FIXME, lorem, ???.",
        r#""body": "TODO""#,
        r#""body": "The final text.""#,
    ),
    rule(
        "DOC304",
        "text/empty-field",
        "text",
        W,
        false,
        "A string is empty or only spaces.",
        r#""rationale": """#,
        r#"remove the field or fill it"#,
    ),
    rule(
        "DOC308",
        "markdown/unclosed-fence",
        "markdown",
        W,
        false,
        "A code fence is opened but not closed in the same value.",
        "```go",
        "```go ... ```",
    ),
    rule(
        "DOC309",
        "markdown/heading-in-field",
        "markdown",
        W,
        false,
        "A line starting with # inside a field breaks the heading structure of the output.",
        r##""body": "# Title""##,
        r#""body": "Title""#,
    ),
    rule(
        "DOC310",
        "markdown/raw-html",
        "markdown",
        W,
        false,
        "Raw HTML, or a link with a javascript:, vbscript: or data: address, in text.",
        r#""body": "<div>x</div>""#,
        r#""body": "x""#,
    ),
    rule(
        "DOC312",
        "markdown/broken-anchor",
        "markdown",
        E,
        false,
        "[[id#field]] points to a field that does not exist or is a service field.",
        r#"[[b#missing]]"#,
        r#"[[b#statement]]"#,
    ),
    rule(
        "DOC313",
        "text/repeated-word",
        "text",
        W,
        false,
        "The same word twice in a row.",
        r#""body": "the the system""#,
        r#""body": "the system""#,
    ),
    rule(
        "DOC314",
        "style/manual-number",
        "style",
        W,
        false,
        "With \"numbering\": true in docc.json, a document title or a section key starts with its own number (\"2.1 Scope\"); docc adds the numbers. Field text is not checked.",
        r#""title": "2.1 Scope""#,
        r#""title": "Scope", "order": 1"#,
    ),
    rule(
        "DOC352",
        "style/hedging",
        "style",
        W,
        false,
        "Hedging and vague-quantity words: maybe, probably, usually, several, various.",
        r#""body": "It will probably work.""#,
        r#""body": "It works.""#,
    ),
    rule(
        "DOC353",
        "style/time-bound-words",
        "style",
        W,
        false,
        "Words that age: currently, recently, soon, new.",
        r#""body": "Currently we use X.""#,
        r#""body": "We use X.""#,
    ),
    rule(
        "DOC401",
        "trace/bad-code-tag",
        "trace",
        E,
        true,
        "A bad code tag: a docc: target that does not resolve (document, field, section or anchor), or a docc:begin region that is not closed, is nested, or a stray docc:end. Works only when trace.scan is set.",
        "// docc: missing-doc",
        "// docc: existing-doc#^limit",
    ),
    rule(
        "DOC403",
        "trace/uncovered",
        "trace",
        W,
        false,
        "A visible document whose kind schema sets \"x-docc-covered\": true has no docc: tag in code. Works only when trace.scan is set.",
        r#""kind": "requirement"  (no // docc: req-x in code)"#,
        "// docc: req-x",
    ),
    rule(
        "DOC402",
        "trace/code-changed",
        "trace",
        W,
        false,
        "A docc:begin ... docc:end region changed after its link was confirmed in docc.lock: the document it points to may no longer match the code. Works only when trace.scan is set.",
        "the lines between // docc:begin req-x and // docc:end are edited",
        "check req-x still matches, then run docc ack <code file>",
    ),
    rule(
        "DOC501",
        "build/stale-output",
        "build",
        E,
        true,
        "out/ differs from what the sources produce: a file is missing, outdated or extra.",
        "docs/out/a.md edited by hand",
        "run docc build",
    ),
    rule(
        "DOC503",
        "build/stale-lock",
        "build",
        E,
        true,
        "docc.lock differs from the links in the sources: a link is missing or extra, a line repeats or is malformed (often after a merge), or the file is missing. Checked by docc check.",
        "docc.lock without the line of a new [[b]] in a",
        "run docc build and commit docc.lock",
    ),
    rule(
        "DOC800",
        "custom/<name>",
        "custom",
        W,
        false,
        "A rule from customRules in docc.json; the rule name is in the rule field.",
        "see customRules",
        "see customRules",
    ),
    rule(
        "DOC901",
        "meta/unused-ignore",
        "meta",
        W,
        false,
        "A docIgnore entry suppresses nothing, or targets a disabled rule.",
        r#""docIgnore": [{ "rule": "text/forbidden-words", "reason": "x" }]  (nothing to suppress)"#,
        "remove the entry",
    ),
    rule(
        "DOC902",
        "meta/rule-code",
        "meta",
        W,
        false,
        "A rule is named by its code (DOC353) or by an old name (style/timeless-words) in the rules of docc.json or in docIgnore; its name (style/time-bound-words) says what it checks. The code and the old name still work.",
        r#""rules": { "DOC352": "off" }"#,
        r#""rules": { "style/hedging": "off" }"#,
    ),
];

static BY_CODE: LazyLock<HashMap<&'static str, &'static Rule>> =
    LazyLock::new(|| ALL.iter().map(|r| (r.code, r)).collect());
static BY_NAME: LazyLock<HashMap<&'static str, &'static Rule>> =
    LazyLock::new(|| ALL.iter().map(|r| (r.name, r)).collect());

/// All returns every rule ordered by code.
pub fn all() -> &'static [Rule] {
    &ALL
}

pub fn by_code(code: &str) -> Option<&'static Rule> {
    BY_CODE.get(code).copied()
}

pub fn by_name(name: &str) -> Option<&'static Rule> {
    BY_NAME.get(name).copied()
}

/// OLD_NAMES are names a rule had before it was renamed: they keep working.
const OLD_NAMES: [(&str, &str); 3] = [
    ("schema/unregistered", "DOC107"),
    ("style/timeless-words", "DOC353"),
    ("trace/unknown-id", "DOC401"),
];

/// Lookup resolves a name (text/forbidden-words), a code (DOC301) or an old
/// name; the code and the old names stay aliases: the names are what people
/// and agents write.
pub fn lookup(key: &str) -> Option<&'static Rule> {
    by_code(key).or_else(|| by_name(key)).or_else(|| {
        OLD_NAMES.iter().find(|(n, _)| *n == key).and_then(|(_, c)| by_code(c))
    })
}

/// alias reports whether key names a rule by its code or an old name (DOC902);
/// DOC800, the code of every custom rule, has no name to write instead.
pub fn alias(key: &str) -> Option<&'static Rule> {
    lookup(key).filter(|r| key != r.name && r.code != "DOC800")
}

/// IsCustomName reports whether name looks like a custom rule name.
pub fn is_custom_name(name: &str) -> bool {
    name.starts_with("custom/")
}

/// Groups are the group keys accepted in the rules section of docc.json.
pub const GROUPS: [&str; 10] = [
    "syntax", "schema", "graph", "text", "markdown", "style", "trace", "build", "meta", "custom",
];

pub fn is_group(s: &str) -> bool {
    GROUPS.contains(&s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry() {
        let n = all().len();
        assert_eq!(n, 43, "want 43 rules, got {n}");
        let mut core = 0;
        for r in all() {
            assert!(
                lookup(r.code).is_some_and(|x| std::ptr::eq(x, r))
                    && lookup(r.name).is_some_and(|x| std::ptr::eq(x, r)),
                "lookup {}",
                r.code
            );
            if r.core {
                core += 1;
            }
            assert!(is_group(r.group), "{}: bad group {}", r.code, r.group);
        }
        // 101-105, 108, 109, 201-205, 210-213, 401, 501, 503
        assert_eq!(core, 19, "core rules: {core}");
    }
}
