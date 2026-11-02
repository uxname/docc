//! The HTML page as a whole; read_app is in common.

mod common;

use common::*;

/// Blocks carry the JSON Pointer, source text, code and version hash of their
/// element; headings have none; a block marker written by an agent is dropped
/// like other raw HTML.
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
        .map(|b| {
            let t = s(b, "s");
            // the source text ends in the code of the block, which the data carries apart too
            let c = s(b, "c");
            assert!(c.is_empty() || (t.ends_with(&format!(" ^{c}")) && s(b, "v").len() == 4), "{b}");
            format!("{}={}", s(b, "p"), t.strip_suffix(&format!(" ^{c}")).unwrap_or(t))
        })
        .collect();
    let want = "/body/0=One. /body/1=<!--b:0--> /body/2=Two. = /field=Free text. = /list=a: x";
    assert_eq!(got.join(" "), want, "blocks");
    assert!(
        a.blocks.contains("<!-- raw HTML omitted -->") && !a.blocks.contains("<!--b:"),
        "a marker in the text must be dropped:\n{}",
        a.blocks
    );
}

/// The export of the HTML file goes into "$comments" of its documents; the
/// rest of each file stays byte for byte, a second import adds nothing, and a
/// comment whose words are gone reads as lost (DOC215) until it is moved or
/// deleted.
#[test]
fn comments() {
    let src = r#"{"title":"I","body":["Alpha text here.","Beta text here."]}"#;
    let p = project([
        ("docs/src/index.json", src),
        (
            "export.json",
            r#"{"docc":"comments","version":1,"comments":[
			{"id":"aaaa1111","doc":"index","path":"/body/1","quote":"Beta text here.","fragment":"Beta","prefix":"","suffix":" text","text":"Fix beta","author":"C","created":"2026-10-15T10:00:00Z"},
			{"id":"bbbb2222","doc":"index","path":"/body/0","quote":"Alpha text here.","text":"Fix alpha"}]}"#,
        ),
        ("bad.json", r#"{"comments":[]}"#),
    ]);
    let before = p.read("docs/src/index.json");
    let code_of = |t: &str| before[before.find(t).unwrap() + t.len() + 2..].split('"').next().unwrap().to_string();
    let (alpha, beta) = (code_of("Alpha text here."), code_of("Beta text here."));
    let (code, out, errs) = p.run(&["comments", "import", "export.json"]);
    assert!(code == 0 && out == "imported 2 comments into 1 documents\n", "{code} {out} {errs}");
    let after = p.read("docs/src/index.json");
    let head = before.trim_end().strip_suffix('}').unwrap();
    assert!(after.starts_with(head) && after[head.len()..].starts_with(&format!(r#", "$comments": [{{"id": "aaaa1111", "at": "^{beta}", "quote": "Beta text here.""#)), "only $comments is added, at the codes of the blocks:\n{after}");
    let (code, out, _) = p.run(&["comments", "import", "export.json"]);
    assert!(code == 0 && out == "imported 0 comments into 0 documents (2 already there)\n", "second import: {code} {out}");
    assert_eq!(p.read("docs/src/index.json"), after, "nothing written the second time");
    let (code, _, errs) = p.run(&["comments", "import", "bad.json"]);
    assert!(code == 2 && errs.contains("not a comments file"), "bad file: {code} {errs}");
    let (code, out, errs) = p.run(&["validate", "--max-warnings", "0"]);
    assert_eq!(code, 0, "imported comments are valid: {out} {errs}");
    // an agent edits alpha and replaces beta by another paragraph (a paragraph
    // inserted above would not move a comment: it stands at a code); the new
    // paragraph gets the number beta had, and the words of the comment are not in it
    p.write(
        "docs/src/index.json",
        after.replace(&format!(r#""Alpha text here. ^{alpha}","Beta text here. ^{beta}""#), &format!(r#""Alpha text here, edited. ^{alpha}","Inserted.""#)),
    );
    p.run(&["codes"]);
    let (code, out, _) = p.run(&["comments"]);
    for want in [
        &format!("aaaa1111  index \"I\" ^{beta} (lost its place)\n  about: «Beta»\n  C 2026-10-15: Fix beta\n  block when commented: Beta text here."),
        &format!("bbbb2222  index \"I\" ^{alpha} (block changed)\n  "),
        "Fix alpha\n  block when commented: Alpha text here.\n  block now: Alpha text here, edited.",
        "2 comments in 1 documents, 1 lost their place",
    ] {
        assert!(code == 0 && out.contains(want), "list lacks {want:?}:\n{out}");
    }
    let (code, out, _) = p.run(&["validate", "--format", "json"]);
    let ds = diagnostics_of(&out);
    let lost: Vec<&serde_json::Value> = ds.as_array().unwrap().iter().filter(|d| d["code"] == "DOC215").collect();
    assert!(code == 0 && lost.len() == 1 && s(lost[0], "message").contains(&format!(r#"comment "aaaa1111" lost its place: the words "Beta" are not in the block at "^{beta}""#)), "{out}");
    let (_, out, _) = p.run(&["comments", "--format", "json"]);
    let list: serde_json::Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e} {out}"));
    let cs = list["comments"].as_array().unwrap();
    assert!(list["total"] == 2 && s(&cs[0], "place") == "lost" && s(&cs[1], "place") == "changed" && s(&cs[1], "current") == "Alpha text here, edited.", "{out}");
    // the HTML file: alpha on its block, the lost one marked for the top of the page
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "the lost comment is a warning: {out} {errs}");
    let a = read_app(&p, "docs/out.html");
    let cs = a.data["comments"].as_array().unwrap();
    assert!(
        cs.len() == 2 && cs[0]["lost"] == true && s(&cs[0], "suffix") == " text" && s(&cs[1], "path") == "/body/0" && cs[1].get("lost").is_none() && cs[1].get("status").is_none(),
        "comments in the page: {cs:?}"
    );
    let md = p.read("docs/out/index.md");
    assert!(md.contains("> 💬 Comments that lost their place:\n>\n> - **C**, 2026-10-15, on «Beta»: Fix beta\n"), "lost on top:\n{md}");
    assert!(md.contains("Alpha text here, edited.\n\n> 💬 **Anonymous**, 20") && md.contains(": Fix alpha\n\nInserted."), "under its block:\n{md}");
    let (code, out, _) = p.run(&["comments", "rm", "bbbb", "aaaa1111"]);
    assert!(code == 0 && out == "deleted bbbb2222\ndeleted aaaa1111\n", "rm: {code} {out:?}");
    assert!(!p.read("docs/src/index.json").contains("$comments"), "the member goes with its last comment");
    let (code, _, errs) = p.run(&["comments", "done", "x"]);
    assert!(code == 2 && errs.contains("no status any more"), "done is gone: {code} {errs}");
}

/// A form field is a block of the HTML file and its question with the options
/// in the Markdown; an answer to it comes back as a comment (version 2) with input and answer,
/// stands at the field, and is lost once the field moves away from at.
#[test]
fn form_field_answers() {
    let field = r#"{"$input":"radio","id":"export","label":"Export?","options":["Yes","No"]}"#;
    let p = project([("docs/src/index.json", format!(r#"{{"title":"I","body":["Intro.",{field},"End."]}}"#).as_str())]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(p.read("docs/out/index.md").contains("Intro.\n\n**Export?**\n\n- Yes\n- No\n\nEnd."), "the field in the Markdown:\n{}", p.read("docs/out/index.md"));
    let a = read_app(&p, "docs/out.html");
    let b = &a.pages()[0]["blocks"][1];
    assert!(s(b, "p") == "/body/1" && s(&b["i"], "t") == "radio" && b["i"]["options"][1] == "No", "field block: {b}");
    // what the page exports: the field as it read, the id and the answer
    let export = |version: u8| {
        serde_json::json!({"docc": "comments", "version": version, "comments": [{"id": "ans00001", "doc": "index",
            "path": "/body/1", "quote": s(b, "s"), "input": "export", "answer": ["Yes"], "text": "Yes", "author": "Ivan", "created": "2026-10-15T10:00:00Z"}]})
        .to_string()
    };
    p.write("v4.json", export(4));
    let (code, _, errs) = p.run(&["comments", "import", "v4.json"]);
    assert!(code == 2 && errs.contains("unsupported version 4"), "{code} {errs}");
    p.write("export.json", export(2));
    let (code, out, errs) = p.run(&["comments", "import", "export.json"]);
    assert!(code == 0 && out == "imported 1 comments into 1 documents\n", "{code} {out} {errs}");
    let src = p.read("docs/src/index.json");
    assert!(src.contains(r#""input": "export", "answer": ["Yes"], "author": "Ivan""#) && !src.contains(r#""text""#), "an answer has no text:\n{src}");
    let (_, out, _) = p.run(&["comments"]);
    assert!(out.contains("ans00001  index \"I\" /body/1\n  answers form field export\n  Ivan 2026-10-15: Yes\n"), "{out}");
    let (code, out, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(p.read("docs/out/index.md").contains("> 💬 **Ivan**, 2026-10-15, answer to «Export?»: Yes"), "the answer in the Markdown");
    // a paragraph inserted above moves the field away from at
    p.write("docs/src/index.json", src.replace(r#"{"$input""#, r#""Inserted.",{"$input""#));
    p.run(&["codes"]);
    let (code, out, _) = p.run(&["validate"]);
    assert!(code == 0 && out.contains(r#"comment "ans00001" lost its place: the form field "export" is not at "/body/1""#), "{out}");
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let cs = read_app(&p, "docs/out.html").data["comments"].clone();
    assert!(s(&cs[0], "input") == "export" && cs[0]["answer"][0] == "Yes" && cs[0]["lost"] == true, "{cs}");
}

/// A question with explained options, a recommendation and a rating: the
/// Markdown prints them in the language of the project, the HTML data gives
/// them as inline HTML, and answers with a note, a flag or ratings come in
/// through the comments file version 3 and docc comments apply.
#[test]
fn rich_questions() {
    let radio = r#"{"$input":"radio","id":"plan","label":"Which **plan**?","options":[{"label":"Small","description":"One `server`, <b>cheap</b>"},{"label":"Big","recommended":true}]}"#;
    let rating = r#"{"$input":"rating","id":"ideas","label":"Rate","options":["Dark",{"label":"Export","description":"PDF"}]}"#;
    let p = project([
        ("docc.json", r#"{"src":"docs/src","out":"docs/out","lang":"ru"}"#),
        ("docs/src/index.json", format!(r#"{{"title":"I","body":["Intro.",{radio},{rating}]}}"#).as_str()),
    ]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let md = p.read("docs/out/index.md");
    assert!(
        md.contains("Which **plan**?\n\n- Small — One `server`, &lt;b>cheap&lt;/b>\n- Big (рекомендую)\n\n**Rate** (Нравится / Не уверен / Не нравится)\n\n- Dark\n- Export — PDF"),
        "{md}"
    );
    let a = read_app(&p, "docs/out.html");
    let (b, r) = (&a.pages()[0]["blocks"][1], &a.pages()[0]["blocks"][2]);
    assert_eq!(b["i"]["options"], serde_json::json!(["Small", "Big"]), "{b}");
    assert_eq!(s(&b["i"], "labelHtml"), "Which <strong>plan</strong>?");
    assert_eq!(b["i"]["descriptions"], serde_json::json!(["One <code>server</code>, &lt;b&gt;cheap&lt;/b&gt;", ""]), "{b}");
    assert_eq!(b["i"]["recommended"], serde_json::json!([1]));
    assert!(r["i"].get("recommended").is_none() && r["i"].get("labels").is_none(), "{r}");
    let item = |id: &str, extra: serde_json::Value| {
        let mut it = serde_json::json!({"id": id, "doc": "index", "path": "/body/1", "quote": s(b, "s"), "input": "plan", "text": "x", "author": "Ivan", "created": "2026-10-15T10:00:00Z"});
        it.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        it
    };
    let file = serde_json::json!({"docc": "comments", "version": 3, "comments": [
        item("ans1", serde_json::json!({"flag": "help_choose", "note": "  What does Big cost?  "})),
        item("ans2", serde_json::json!({"answer": ["Big"], "note": "Growth"})),
        item("ans3", serde_json::json!({"path": "/body/2", "quote": s(r, "s"), "input": "ideas", "ratings": [{"option": "Dark", "rating": "like"}, {"option": "Export", "rating": "dislike", "note": "Nobody prints"}]})),
    ]});
    p.write("v3.json", file.to_string());
    let (code, out, errs) = p.run(&["comments", "import", "v3.json"]);
    assert!(code == 0 && out == "imported 3 comments into 1 documents\n", "{code} {out} {errs}");
    let src = p.read("docs/src/index.json");
    assert!(src.contains(r#""input": "plan", "flag": "help_choose", "note": "What does Big cost?", "author""#), "{src}");
    let (code, out, _) = p.run(&["validate"]);
    assert_eq!(code, 0, "{out}");
    let (_, out, _) = p.run(&["comments"]);
    assert!(out.contains("  Ivan 2026-10-15: Help me choose (help_choose)\n  note: What does Big cost?\n"), "{out}");
    assert!(out.contains("  Ivan 2026-10-15: rated:\n    Dark: like\n    Export: dislike — Nobody prints\n"), "{out}");
    let (code, out, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{out} {errs}");
    let md = p.read("docs/out/index.md");
    assert!(md.contains("ответ на «Which **plan**?»: Помоги выбрать — What does Big cost?"), "{md}");
    assert!(md.contains("ответ на «Rate»: Dark: Нравится; Export: Не нравится — Nobody prints"), "{md}");
    // a new description and recommendation keep the answers to the question as it is
    p.write("docs/src/index.json", src.replace(r#""recommended": true"#, r#""description": "More""#).replace("\"recommended\":true", "\"description\":\"More\""));
    let (_, out, _) = p.run(&["comments", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["comments"].as_array().unwrap().iter().all(|c| c["place"] == "same"), "{out}");
    // apply: a rating must rate every option; edit gives the answer anew
    let apply = |ops: serde_json::Value| do_stdin(&p.path, ops.to_string().as_bytes(), &["comments", "apply"]);
    let (code, _, errs) = apply(serde_json::json!([{"op": "add", "doc": "index", "comment": {"at": "/body/2", "input": "ideas", "ratings": [{"option": "Dark", "rating": "like"}], "author": "A"}}]));
    assert!(code == 2 && errs.contains("leave out the option \"Export\""), "{code} {errs}");
    let (code, _, errs) = apply(serde_json::json!([{"op": "edit", "id": "ans2", "flag": "decide_for_me"}]));
    assert_eq!(code, 0, "{errs}");
    let src = p.read("docs/src/index.json");
    assert!(src.contains(r#""id": "ans2""#) && src.contains(r#""flag": "decide_for_me", "author": "Ivan""#), "the note goes with the old answer:\n{src}");
    let (code, _, errs) = apply(serde_json::json!([{"op": "edit", "id": "ans2", "answer": ["Big"], "flag": "help_choose"}]));
    assert!(code == 2 && errs.contains("a flag replaces the choice"), "{code} {errs}");
}

/// Comments copied from the page as text come in through standard input;
/// statuses, replies and internal comments are gone.
#[test]
fn comments_copied() {
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
    let (code, _, errs) = do_stdin(&p.path, export("team0001").as_bytes(), &["comments", "import", "--internal", "-"]);
    assert!(code == 2 && errs.contains("-internal"), "internal is gone: {code} {errs}");
    let (code, _, errs) = do_stdin(&p.path, b"just some words", &["comments", "import", "-"]);
    assert!(code == 2 && errs.contains("no comments in it"), "text without comments: {code} {errs}");
    let (code, _, errs) = p.run(&["comments", "rm", "--reply", "x", "cust"]);
    assert!(code == 2 && errs.contains("-reply"), "reply is gone: {code} {errs}");
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(p.read("docs/out.html").contains("Note cust0001"), "the comment is in the file");
    // --no-comments leaves them out of everything the build writes, and docc check takes either build
    let (code, out, errs) = p.run(&["build", "--html", "--no-comments"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(!p.read("docs/out.html").contains("Note cust0001") && !p.read("docs/out/index.md").contains("Note cust0001"));
    let (code, out, errs) = p.run(&["check"]);
    assert_eq!(code, 0, "out/ without comments is fresh: {out} {errs}");
}

/// Once made, the HTML file is rebuilt by a plain docc build, so a stale file
/// is never sent; --since must name a revision of the repository, and the old
/// key changesSince of docc.json says where it went.
#[test]
fn html_kept_up_to_date() {
    let p = project([("docs/src/index.json", r#"{"title":"I","body":["One."]}"#)]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    p.write("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Two."]}"#);
    let (code, out, errs) = p.run(&["build"]);
    assert!(code == 0 && out.contains("built docs/out.html"), "{code} {out} {errs}");
    assert!(p.read("docs/out.html").contains("Two."), "docs/out.html was not rebuilt");
    p.write("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Three."]}"#);
    let (code, out, errs) = p.run(&["build", "--since", "no-such-tag"]);
    assert!(
        code == 2 && errs.contains(r#"changes since "no-such-tag""#) && out.contains("files in docs/out"),
        "unknown revision: {code} {out} {errs}"
    );
    // the pages are still built: only the HTML file waits for a good revision
    let md = p.read("docs/out/index.md");
    assert!(md.contains("Three."), "docs/out was not built:\n{md}");
    p.write("docc.json", r#"{"changesSince":"v1"}"#);
    let (code, _, errs) = p.run(&["build"]);
    assert!(code == 2 && errs.contains("changesSince is no longer a key of docc.json: give the revision to the HTML build (docc build --html --since <revision>)"), "{code} {errs}");
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
    let edited = p.read("docs/src/index.json").replace("Old **bold**.", "Other **bold** text.");
    p.write("docs/src/index.json", edited);
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

/// What the HTML file exports is what `docc comments import` takes: app.js
/// (in Node, as app_test.js runs it) answers the fields of a built page with a
/// note, a flag and ratings, exports the file, and the import writes it.
#[test]
fn html_export_imports() {
    let radio = r#"{"$input":"radio","id":"plan","label":"Which plan?","options":[{"label":"Small","description":"One server"},{"label":"Big","recommended":true}]}"#;
    let boxes = r#"{"$input":"checkbox","id":"os","label":"Which systems?","options":["Linux","Mac"]}"#;
    let rating = r#"{"$input":"rating","id":"ideas","label":"Rate","options":["Dark","Export"]}"#;
    let p = project([("docs/src/index.json", format!(r#"{{"title":"I","body":["Intro.",{radio},{boxes},{rating}]}}"#).as_str())]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let a = read_app(&p, "docs/out.html");
    p.write("data.json", a.data.to_string());
    let app = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/app.js");
    p.write(
        "export.js",
        r#"'use strict';
const fs = require('fs');
const data = fs.readFileSync('data.json', 'utf8');
let kept = null, T;
const noop = () => {};
global.window = global;
global.htmPreact = { html: () => null, render: noop, useState: (v) => [v, noop], useEffect: noop, useRef: (v) => ({ current: v }) };
global.document = { getElementById: () => ({ textContent: data }), createElement: () => ({}) };
global.localStorage = { getItem: (k) => (k.endsWith(':comments') ? kept : null), setItem: noop };
global.addEventListener = noop;
global.doccTest = (t) => { T = t; };
const load = () => new Function(fs.readFileSync(process.argv[2], 'utf8'))();
load();
const st = (o) => Object.assign({ v: [], own: [false, ''], note: '', flag: '', rates: {} }, o);
const blocks = JSON.parse(data).pages[0].blocks.filter((b) => b.i);
const given = [st({ v: ['Big'], note: ' Growth ' }), st({ flag: 'more_options' }), st({ rates: { Dark: ['like', ''], Export: ['dislike', 'Nobody prints'] } })];
kept = JSON.stringify(blocks.map((b, k) => {
  const o = T.fieldAnswer(b.i, given[k]);
  if (!T.answerOk(b.i, o)) { throw new Error('not ready: ' + b.i.id); }
  return Object.assign({ id: 'c-exportaaa' + k, doc: 'index', path: b.p, quote: b.s, text: T.answerWords(b.i, o), input: b.i.id }, o,
    { author: 'Ivan', created: '2026-11-02T10:00:00Z', build: '' });
}));
load();
process.stdout.write(T.exportJSON());
"#,
    );
    let out = match std::process::Command::new("node").arg("export.js").arg(&app).current_dir(&p.path).output() {
        Ok(o) => o,
        Err(_) => {
            eprintln!("node is not installed");
            return;
        }
    };
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let file = String::from_utf8(out.stdout).unwrap();
    assert!(file.contains(r#""version": 3"#), "{file}");
    p.write("export.json", &file);
    let (code, out, errs) = p.run(&["comments", "import", "export.json"]);
    assert!(code == 0 && out == "imported 3 comments into 1 documents\n", "{code} {out} {errs}\n{file}");
    let src = p.read("docs/src/index.json");
    for want in [r#""answer": ["Big"], "note": "Growth""#, r#""flag": "more_options""#, r#""note":"Nobody prints""#, r#""rating":"dislike""#] {
        assert!(src.contains(want), "lacks {want}:\n{src}");
    }
    let (code, out, _) = p.run(&["validate"]);
    assert_eq!(code, 0, "{out}");
}
