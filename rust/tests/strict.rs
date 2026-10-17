//! Port of cmd/docc/strict_test.go.

mod common;

use std::collections::BTreeMap;

use common::*;

/// DOC109 is a core rule: every document needs a kind with a schema, and the
/// rule cannot be switched off. A kind schema without a boolean top-level
/// additionalProperties is a config error.
#[test]
fn kind_schema_required() {
    let mut fs = BTreeMap::from([
        ("docs/src/index.json", r#"{"title":"I","body":"See [[a]]."}"#),
        ("docs/src/a.json", r#"{"title":"A","kind":"ghost"}"#),
    ]);
    let p = raw_project(fs.clone());
    let (code, out, _) = p.run(&["validate"]);
    assert!(code == 1 && out.matches("error DOC109").count() == 2, "default: want two DOC109 errors: {code} {out}");
    fs.insert("docc.json", r#"{"rules":{"DOC109":"off"}}"#);
    let p = raw_project(fs.clone());
    let (code, _, errs) = p.run(&["validate"]);
    assert!(code == 2 && errs.contains("DOC109 is a core rule"), "off: {code} {errs}");
    fs.remove("docc.json");
    fs.insert("docs/schemas/page.json", r#"{"type":"object","required":["title"],"additionalProperties":true}"#);
    fs.insert("docs/schemas/ghost.json", r#"{"type":"object","required":["title"]}"#);
    fs.insert("docs/src/index.json", r#"{"title":"I","kind":"page","body":"See [[a]]."}"#);
    let p = raw_project(fs.clone());
    let (code, _, errs) = p.run(&["validate"]);
    assert!(
        code == 2 && errs.matches("additionalProperties").count() == 1 && errs.contains("docs/schemas/ghost.json"),
        "no additionalProperties: {code} {errs}"
    );
    fs.insert("docs/schemas/ghost.json", r#"{"type":"object","required":["title"],"additionalProperties":false}"#);
    let p = raw_project(fs);
    let (code, out, _) = p.run(&["validate"]);
    assert_eq!(code, 0, "with schemas: {out}");
}
