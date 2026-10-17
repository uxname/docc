//! Port of cmd/docc/anchors_test.go.

mod common;

use common::*;

#[test]
fn block_anchors_check() {
    let p = project([
        (
            "docs/src/index.json",
            r#"{"title":"Index ^t","body":["Release ^new","x ^2","{{index#^new}}"],
			"rows":[{"k":"cell ^cell"}],"obj":{"k":"member ^member"},"single":"See [[a]]. ^one"}"#,
        ),
        (
            "docs/src/a.json",
            r#"{"title":"A","parent":"index","body":"[[index#^new]] [[index#^one]] [[index#^cell]] [[index#^member]] [[index#^t]]"}"#,
        ),
    ]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(!out.contains("DOC353"), "the marker ^new reaches text rules:\n{out}");
    assert!(!out.contains("DOC211"), "an include of a paragraph of the same field is taken for a cycle:\n{out}");
    let n = out.matches("DOC312").count();
    assert!(
        n == 3 && !out.contains(r#""^new""#) && !out.contains(r#""^one""#),
        "want DOC312 for ^cell, ^member and ^t only:\n{out}"
    );
}

#[test]
fn block_anchor_include_cycle() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":["See [[a]].","Loop {{a#^b}} ^a"]}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index","body":"Back {{index#^a}} ^b"}"#),
    ]);
    let (code, out, _) = p.run(&["validate"]);
    assert!(code != 0 && out.contains("include cycle"), "a cycle through block anchors is missed: {code} {out}");
}
