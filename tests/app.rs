//! The HTML page as a whole; read_app is in common.

mod common;

use common::*;

/// Blocks carry the JSON Pointer and source text of their element; headings
/// have none; a block marker written by an agent is dropped like other raw HTML.
#[test]
fn html_blocks_have_pointers() {
    let p = project([(
        "docs/src/index.json",
        r#"{"title":"I","body":["One.","<!--b:0-->","Two."],"field":"Free text.","list":{"a":"x"}}"#,
    )]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let a = read_app(&p, "docs/out.html");
    let got: Vec<String> = a.pages()[0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| format!("{}={}", s(b, "p"), s(b, "s")))
        .collect();
    let want = "/body/0=One. /body/1=<!--b:0--> /body/2=Two. = /field=Free text. = /list=a: x";
    assert_eq!(got.join(" "), want, "blocks");
    assert!(
        a.blocks.contains("<!-- raw HTML omitted -->") && !a.blocks.contains("<!--b:"),
        "a marker in the text must be dropped:\n{}",
        a.blocks
    );
}

#[test]
fn comments() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","body":["Alpha text here.","Beta text here."]}"#),
        (
            "export.json",
            r#"{"docc":"comments","version":1,"comments":[
			{"id":"aaaa1111","doc":"index","path":"/body/1","quote":"Beta text here.","fragment":"Beta","prefix":"","suffix":" text","text":"Fix beta","author":"C","created":"2026-10-15T10:00:00Z"},
			{"id":"bbbb2222","doc":"index","path":"/body/0","quote":"Alpha text here.","text":"Fix alpha"}]}"#,
        ),
        ("bad.json", r#"{"comments":[]}"#),
    ]);
    let (code, out, errs) = p.run(&["comments", "import", "export.json"]);
    assert!(code == 0 && out.contains("imported 2 comments"), "{code} {out} {errs}");
    let (code, out, _) = p.run(&["comments", "import", "export.json"]);
    assert!(
        code == 0 && out.contains("imported 0 comments into docc.comments.json (2 already there)"),
        "second import: {code} {out}"
    );
    let (code, _, errs) = p.run(&["comments", "import", "bad.json"]);
    assert!(code == 2 && errs.contains("not a comments file"), "bad file: {code} {errs}");
    // an agent inserts a paragraph above beta and edits alpha
    p.write(
        "docs/src/index.json",
        r#"{"title":"I","kind":"page","body":["Alpha text here, edited.","Inserted.","Beta text here."]}"#,
    );
    let (code, out, _) = p.run(&["comments"]);
    for want in [
        "aaaa1111  open  index \"I\" /body/2\n  about: «Beta»\n  C 2026-10-15: Fix beta\n  block: Beta text here.",
        "bbbb2222  open  index \"I\" /body/0\n  Fix alpha\n  block when commented: Alpha text here.\n  block now: Alpha text here, edited.",
        "2 not processed, 2 in all (docc.comments.json)",
    ] {
        assert!(code == 0 && out.contains(want), "list lacks {want:?}:\n{out}");
    }
    let (code, out, _) = p.run(&["comments", "done", "aaaa"]);
    assert!(code == 0 && out == "processed aaaa1111\n", "done: {code} {out:?}");
    let (code, _, errs) = p.run(&["comments", "done", "zz"]);
    assert!(code == 2 && errs.contains(r#"no comment "zz""#), "unknown id: {code} {errs}");
    let (_, out, _) = p.run(&["comments", "--format", "json"]);
    let list: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e} {out}"));
    let cs = list["comments"].as_array().unwrap();
    assert!(list["open"] == 1 && cs.len() == 1 && s(&cs[0], "state") == "changed", "{out}");
    // the HTML file shows both on their blocks, beta at its new place
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let a = read_app(&p, "docs/out.html");
    let cs = a.data["comments"].as_array().unwrap();
    assert!(
        cs.len() == 2 && s(&cs[0], "path") == "/body/2" && s(&cs[0], "status") == "done" && s(&cs[0], "suffix") == " text",
        "comments in the page: {cs:?}"
    );
    let (code, out, _) = p.run(&["comments", "rm", "bbbb2222", "aaaa1111"]);
    assert!(code == 0 && out == "deleted bbbb2222\ndeleted aaaa1111\n", "rm: {code} {out:?}");
    assert_eq!(p.read("docc.comments.json"), "{\"comments\": []}\n", "after rm");
}

/// Comments copied from the page as text come in through standard input; the
/// team's own (--internal) stay out of the HTML file, a reply goes into it.
#[test]
fn comments_copied_and_internal() {
    use base64::Engine as _;
    let p = project([("docs/src/index.json", r#"{"title":"I","body":["Alpha text here."]}"#)]);
    let export = |id: &str| {
        let js = format!(
            r#"{{"docc":"comments","version":1,"comments":[{{"id":"{id}","doc":"index","path":"/body/0","quote":"Alpha text here.","text":"Note {id}"}}]}}"#
        );
        format!(
            "Замечания (1):\n1. «Alpha» — Note\n\ndocc-comments:{}\n",
            base64::engine::general_purpose::STANDARD.encode(js)
        )
    };
    let (code, out, errs) = do_stdin(&p.path, export("cust0001").as_bytes(), &["comments", "import", "-"]);
    assert!(code == 0 && out.contains("imported 1 comments"), "copied text: {code} {out} {errs}");
    let (code, out, errs) = do_stdin(&p.path, export("team0001").as_bytes(), &["comments", "import", "--internal", "-"]);
    assert_eq!(code, 0, "internal: {out} {errs}");
    let (code, _, errs) = do_stdin(&p.path, b"just some words", &["comments", "import", "-"]);
    assert!(code == 2 && errs.contains("no comments in it"), "text without comments: {code} {errs}");
    let (code, _, errs) = p.run(&["comments", "rm", "--reply", "x", "cust"]);
    assert!(code == 2 && errs.contains("--reply is only for done"), "reply outside done: {code} {errs}");
    let (code, out, errs) = p.run(&["comments", "done", "cust", "--reply", "Fixed in 5.2"]);
    assert_eq!(code, 0, "done --reply: {out} {errs}");
    let (_, out, _) = p.run(&["comments", "--all"]);
    assert!(out.contains("reply: Fixed in 5.2") && out.contains("team0001  open internal"), "list:\n{out}");
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let html = p.read("docs/out.html");
    assert!(
        !html.contains("team0001") && html.contains("Fixed in 5.2"),
        "the file must have the reply and no internal comment"
    );
}

/// Once made, the HTML file is rebuilt by a plain docc build, so a stale file
/// is never sent; changesSince must name a revision of the repository.
#[test]
fn html_kept_up_to_date() {
    let p = project([("docs/src/index.json", r#"{"title":"I","body":["One."]}"#)]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    p.write("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Two."]}"#);
    let (code, out, errs) = p.run(&["build"]);
    assert!(code == 0 && out.contains("built docs/out.html"), "{code} {out} {errs}");
    assert!(p.read("docs/out.html").contains("Two."), "docs/out.html was not rebuilt");
    p.write("docc.json", r#"{"changesSince":"no-such-tag"}"#);
    p.write("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Three."]}"#);
    let (code, out, errs) = p.run(&["build"]);
    assert!(
        code == 2 && errs.contains(r#"changes since "no-such-tag""#) && out.contains("files in docs/out"),
        "unknown revision: {code} {out} {errs}"
    );
    // the pages are still built: only the HTML file waits for a good revision
    let md = p.read("docs/out/index.md");
    assert!(md.contains("Three."), "docs/out was not built:\n{md}");
}

/// A one-file build is kept up to date only when docc wrote it: a foreign
/// docs/out.html or docs/out.md next to the output stays as it is.
#[test]
fn foreign_one_file_builds_left_alone() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","body":["One."]}"#),
        ("docs/out.html", "<p>mine</p>"),
        ("docs/out.md", "my notes"),
    ]);
    let (code, out, errs) = p.run(&["build"]);
    assert!(
        code == 0
            && !out.contains("out.html")
            && errs.contains("out.html is not a file docc wrote")
            && errs.contains("out.md is not a file docc wrote"),
        "{code} {out} {errs}"
    );
    assert!(
        p.read("docs/out.html") == "<p>mine</p>" && p.read("docs/out.md") == "my notes",
        "a foreign file was overwritten"
    );
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert!(code == 0 && p.read("docs/out.html").contains("One."), "explicit --html must replace it: {code} {out} {errs}");
}

/// A page without body blocks has an empty list in the data island, not null,
/// which the app cannot map over.
#[test]
fn html_page_without_blocks() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","body":["One."]}"#),
        ("docs/src/empty.json", r#"{"title":"Empty","kind":"page","parent":"index"}"#),
    ]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(!p.read("docs/out.html").contains(r#""blocks":null"#), "blocks of an empty page must be [], not null");
}

/// The old versions of a string block carry their HTML, so the timeline can
/// show the page as it was; the current text needs none, it is the block's own.
#[test]
fn html_old_versions_rendered() {
    let p = project([("docs/src/index.json", r#"{"title":"I","body":["Old **bold**.","Kept."]}"#)]);
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(p.join(""))
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    if !git(&["init", "-q"]) {
        return; // no git here
    }
    assert!(git(&["add", "-A"]) && git(&["commit", "-qm", "one"]));
    p.write("docs/src/index.json", r#"{"title":"I","body":["Other **bold** text.","Kept."],"kind":"page"}"#);
    assert!(git(&["commit", "-qam", "two"]));
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let a = read_app(&p, "docs/out.html");
    let b = &a.data["hist"]["docs"]["index.json"]["b"];
    let vs = b["/body/0"].as_array().unwrap();
    assert_eq!(vs.len(), 2, "{b}");
    assert_eq!(s(&vs[0], "h"), "<p>Old <strong>bold</strong>.</p>", "the old version is rendered");
    assert_eq!(s(&vs[1], "h"), "", "the current version is the block itself");
    assert_eq!(s(&b["/body/1"][0], "h"), "", "an unchanged block needs no old HTML");
}
