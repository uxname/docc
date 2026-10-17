//! Port of cmd/docc/numbering_test.go.

mod common;

use common::*;

fn numbering_project(numbering: bool) -> Dir {
    let cfg = if numbering { r#"{"roots":["index","appx"],"numbering":true}"# } else { r#"{"roots":["index","appx"]}"# };
    project([
        ("docc.json", cfg),
        ("docs/src/index.json", r#"{"title":"Index","body":"See [[b1]], [[b1#spec/Limits/Size]] and [[b1#more/Roles]]."}"#),
        ("docs/src/a.json", r#"{"title":"Zeta","parent":"index","order":1}"#),
        ("docs/src/h.json", r#"{"title":"Hidden","parent":"index","order":2,"hidden":true}"#),
        ("docs/src/b.json", r#"{"title":"Alpha","parent":"index","order":3}"#),
        ("docs/src/c.json", r#"{"title":"Beta","parent":"index"}"#),
        (
            "docs/src/b1.json",
            r#"{"title":"Deep","parent":"b",
			"spec":{"$sections":true,"Limits":{"$sections":true,"Size":"Up to 5."},"Users":"Twelve."},
			"more":{"$sections":true,"Roles":"Two."}}"#,
        ),
        ("docs/src/appx.json", r#"{"title":"Appendix"}"#),
        ("docs/src/x.json", r#"{"title":"Extra","parent":"appx"}"#),
    ])
}

/// page_title is the title of the page with the id in the data island
/// (Go: fmt.Sprint(a.Data.Pages) contains "<id> <title>").
fn page_title(a: &App, id: &str) -> String {
    a.pages().iter().find(|p| s(p, "id") == id).map(|p| s(p, "title").to_string()).unwrap_or_default()
}

#[test]
fn numbering() {
    let p = numbering_project(true);
    let (code, out, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{out} {errs}");
    let tree = p.read("docs/out/_tree.md");
    let want = "- [Index](index.md)\n  - [1 Zeta](a.md)\n  - [2 Alpha](b.md)\n    - [2.1 Deep](b1.md)\n  - [3 Beta](c.md)\n- [Appendix](appx.md)\n  - [4 Extra](x.md)";
    assert!(tree.contains(want), "tree lacks {want:?}:\n{tree}");
    let page = p.read("docs/out/b1.md");
    for want in [
        "# 2.1 Deep",
        "## Spec",
        "<a id=\"limits\"></a>\n\n### 2.1.1 Limits",
        "<a id=\"size\"></a>\n\n#### 2.1.1.1 Size",
        "### 2.1.2 Users",
        "## More",
        "### 2.1.3 Roles",
        "[2 Alpha](b.md)",
    ] {
        assert!(page.contains(want), "b1.md lacks {want:?}:\n{page}");
    }
    let index = p.read("docs/out/index.md");
    assert!(
        index.contains("# Index\n") && index.contains("See [2.1 Deep](b1.md), [2.1.1.1 Size](b1.md#size) and [2.1.3 Roles](b1.md#roles)."),
        "links lack numbers or the root got one:\n{index}"
    );
    let (code, _, errs) = p.run(&["build", "--full"]);
    assert_eq!(code, 0, "{errs}");
    let full = p.read("docs/out-full/_tree.md");
    assert!(
        full.contains("  - [1 Zeta](a.md)\n  - [Hidden](h.md) (hidden)\n  - [2 Alpha](b.md)") && full.contains("[4 Extra](x.md)"),
        "--full gives other numbers:\n{full}"
    );
    let (code, _, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{errs}");
    let a = read_app(&p, "docs/out.html");
    for want in [r#"<h4 id="b1--size">2.1.1.1 Size</h4>"#, r#"<h3 id="b1--roles">2.1.3 Roles</h3>"#, r##"href="#b1--size">2.1.1.1 Size</a>"##] {
        assert!(a.blocks.contains(want), "out.html lacks {want:?}");
    }
    assert_eq!(page_title(&a, "b1"), "2.1 Deep", "the page title lacks its number");
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "check after build: {out}");
}

#[test]
fn numbering_off_keeps_titles_and_anchors() {
    let p = numbering_project(false);
    let (code, _, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{errs}");
    let a = read_app(&p, "docs/out.html");
    for want in [r#"<h4 id="b1--size">Size</h4>"#, r##"href="#b1--size">Size</a>"##] {
        assert!(a.blocks.contains(want), "out.html lacks {want:?}");
    }
    assert_eq!(page_title(&a, "b1"), "Deep", "page title");
    p.run(&["build"]);
    let index = p.read("docs/out/index.md");
    assert!(
        index.contains("See [Deep](b1.md), [Size](b1.md#size) and [Roles](b1.md#roles)."),
        "numbering off changed links:\n{index}"
    );
    let tree = p.read("docs/out/_tree.md");
    assert!(tree.contains("[Zeta](a.md)\n  - [Alpha](b.md)"), "order must hold without numbering:\n{tree}");
    let page = p.read("docs/out/b1.md");
    assert!(!page.contains("<a id="), "unnumbered headings need no explicit anchors:\n{page}");
}
