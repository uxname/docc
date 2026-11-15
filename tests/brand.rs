mod common;

use common::*;

const BRAND_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2" fill="#fff"/></svg>"##;

/// html_with_brand is the brand parts of the data island of the HTML file.
fn html_with_brand(p: &Dir) -> String {
    let a = read_app(p, "docs/out.html");
    let b = &a.data["brand"];
    format!("{}{}{}", s(b, "head"), s(b, "print"), s(b, "foot"))
}

/// brand_data is project.brand of the workspace object the HTML file inlines:
/// the reader draws the logo and the colours from it.
fn brand_data(p: &Dir) -> serde_json::Value {
    let html = p.read("docs/out.html");
    const OPEN: &str = r#"<script type="application/json" id="docc-workspace">"#;
    let a = html.find(OPEN).expect("the workspace is inlined") + OPEN.len();
    let b = a + html[a..].find("</script>").unwrap();
    let ws: serde_json::Value = serde_json::from_str(&html[a..b]).unwrap();
    ws["compile"]["project"]["brand"].clone()
}

#[test]
fn branding_in_html_only() {
    let p = project([
        (
            "docc.json",
            r##"{"branding":{"name":"A & B","url":"https://example.com/x","logo":"docs/logo.svg",
			"accent":"#57e9c7","accentLight":"#0b7a64","header":"#1f1f1f"}}"##,
        ),
        ("docs/logo.svg", BRAND_SVG),
        ("docs/src/index.json", r#"{"title":"Index","body":"Text."}"#),
    ]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let html = html_with_brand(&p);
    for want in [
        r#"<a class="brand" href="https://example.com/x"><span class="logo" role="img" aria-label="A &amp; B"></span></a>"#,
        r#"<div class="brand-print">A &amp; B</div>"#,
        r#"<footer class="brand-foot">A &amp; B · <a href="https://example.com/x">example.com</a></footer>"#,
    ] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
    let b = brand_data(&p);
    assert!(
        b["name"] == "A & B"
            && b["accent"] == "#57e9c7"
            && b["accentLight"] == "#0b7a64"
            && b["header"] == "#1f1f1f"
            && b["logo"].as_str().is_some_and(|l| l.starts_with("data:image/svg+xml;base64,")),
        "{b}"
    );
    let md = p.read("docs/out/index.md");
    assert!(!md.contains("A & B"), "Markdown is branded:\n{md}");
}

#[test]
fn branding_name_only() {
    let p = project([
        ("docc.json", r#"{"branding":{"name":"Acme"}}"#),
        ("docs/src/index.json", r#"{"title":"Index","body":"Text."}"#),
    ]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let html = html_with_brand(&p);
    for want in [r#"<span class="brand">Acme</span>"#, r#"<footer class="brand-foot">Acme</footer>"#] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
    let b = brand_data(&p);
    assert!(b["name"] == "Acme" && b["logo"].is_null() && b["accent"].is_null(), "no logo and no colours were given: {b}");
}

#[test]
fn branding_errors() {
    for (cfg, want) in [
        (r#"{"branding":{}}"#, "needs a name"),
        (r#"{"branding":{"name":"A","logo":"nope.svg"}}"#, "does not exist"),
        (r#"{"branding":{"name":"A","logo":"docs/logo.txt"}}"#, "png, jpg"),
        (r#"{"branding":{"name":"A","accent":"red"}}"#, "accent must be a colour"),
        (r##"{"branding":{"name":"A","header":"#12345"}}"##, "header must be a colour"),
        (r#"{"branding":{"name":"A","url":"javascript:alert"}}"#, "http or https"),
        (r##"{"branding":{"name":"A","color":"#fff"}}"##, "unknown key"),
    ] {
        let p = project([("docc.json", cfg), ("docs/logo.txt", "x"), ("docs/src/index.json", r#"{"title":"I"}"#)]);
        let (code, out, errs) = p.run(&["validate"]);
        assert!(code == 2 && (out.clone() + &errs).contains(want), "{cfg}: {code} {out:?} {errs:?}, want {want:?}");
    }
}
