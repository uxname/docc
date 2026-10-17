//! Port of cmd/docc/brand_test.go.

mod common;

use common::*;

const BRAND_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2" fill="#fff"/></svg>"##;

/// html_with_brand is the HTML file plus the brand parts of its data island.
fn html_with_brand(p: &Dir) -> String {
    let html = p.read("docs/out.html");
    let a = read_app(p, "docs/out.html");
    let b = &a.data["brand"];
    format!("{html}{}{}{}", s(b, "head"), s(b, "print"), s(b, "foot"))
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
        r#"--brand-logo:url("data:image/svg+xml;base64,"#,
        "--brand-bg:#1f1f1f",
        "--accent:#0b7a64",
        "--accent:#57e9c7",
        r#"<div class="brand-print">A &amp; B</div>"#,
        r#"<footer class="brand-foot">A &amp; B · <a href="https://example.com/x">example.com</a></footer>"#,
    ] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
    assert_eq!(html.matches("--brand-logo:url(").count(), 1, "the logo must be embedded once");
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
    assert!(
        html.contains(":root{--brand-bg:#1f1f1f;--brand-line:var(--accent)}\n:root{--hbg:var(--brand-bg)")
            && !html.contains("--brand-logo:"),
        "no logo and no colours were given: only the default header colour is set"
    );
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
