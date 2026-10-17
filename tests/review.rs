mod common;

use common::*;

#[test]
fn broken_file_stays_a_node() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","body":"See [[broken]]."}"#),
        ("docs/src/broken.json", "{"),
        ("docs/src/x.json", r#"{"title":"X","parent":"broken"}"#),
    ]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    assert!(!out.contains("DOC203") && out.contains("DOC101"), "x is reachable through the broken node:\n{out}");
}

#[test]
fn trace_ignores_bom_and_config_positions() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I"}"#),
        ("docc.json", r#"{"trace":{"scan":["tests/**"]}}"#),
        ("tests/a.txt", "\u{feff}// docc: nope\r\n"),
    ]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.contains("tests/a.txt:1:4: error trace/bad-code-tag (DOC401)"), "{out}");
    for (input, want) in [
        (r#"{"customRules":[{"id":"custom/x","level":"warn","given":"a","forbid":"x","message":"m"}]}"#, "docc.json:1:"),
        (r#"{"rules":{"DOC301":{"words":["бы*стро"]}}}"#, "docc.json:1:"),
    ] {
        let p = project([("docs/src/index.json", r#"{"title":"I"}"#), ("docc.json", input)]);
        let (code, _, errs) = p.run(&["validate"]);
        assert!(code == 2 && errs.starts_with(&format!("docc: error: {want}")), "{input}: {code} {errs:?}");
    }
}

#[test]
fn tag_without_comma() {
    let p = project([
        ("docc.json", r#"{"trace":{"scan":["*.go"]}}"#),
        ("docs/src/index.json", r#"{"title":"I","body":"See [[a]] and [[b]]."}"#),
        ("docs/src/a.json", r#"{"title":"A"}"#),
        ("docs/src/b.json", r#"{"title":"B"}"#),
        ("x.go", "// docc: a b\n// docc: a, b\n"),
    ]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    assert!(
        out.matches(r#""DOC401""#).count() == 1 && out.contains("separate targets with commas"),
        "only the tag without a comma is reported:\n{out}"
    );
}

#[test]
fn broken_parent_no_cascade() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I"}"#),
        ("docs/src/n.json", r#"{"title":"N","parent":5}"#),
        ("docs/src/s.json", r#"{"title":"S","parent":"s"}"#),
    ]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    assert!(!out.contains("DOC203"), "DOC203 is a cascade here:\n{out}");
    assert!(out.contains("DOC105") && out.matches(r#""DOC204""#).count() == 1, "one signal each:\n{out}");
}

#[test]
fn syntax_hints() {
    let p = project([("docs/src/index.json", r#"{"Title":"I"}"#), ("docs/src/e.json", ""), ("docs/src/a.json", "[1]")]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    for w in [r#"is named \"title\""#, "file is empty", "must be an object"] {
        assert!(out.contains(w), "missing {w:?}:\n{out}");
    }
}
