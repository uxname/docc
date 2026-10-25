//! docc compile: one JSON object on stdout, nothing written, the same pages
//! for a Git revision as for its checkout, and a shape that docc schema
//! compile describes.
mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use common::*;
use serde_json::Value;

/// compile runs docc compile --format json in dir and returns the exit code
/// and the object; stdout must be exactly one line of JSON.
fn compile(dir: &Path, extra: &[&str]) -> (i32, Value) {
    let mut args = vec!["compile", "--format", "json"];
    args.extend(extra);
    let (code, out, errs) = do_in(dir, &args);
    assert!(out.ends_with('\n') && out.matches('\n').count() == 1, "one line: {out} {errs}");
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out} {errs}"));
    check_schema(&v);
    (code, v)
}

/// check_schema validates an output against docc schema compile.
fn check_schema(v: &Value) {
    static SCHEMA: OnceLock<(Value, jsonschema::Validator)> = OnceLock::new();
    let (schema, val) = SCHEMA.get_or_init(|| {
        let (_, out, _) = do_in(&repo(), &["schema", "compile"]);
        let schema: Value = serde_json::from_str(&out).unwrap();
        let val = jsonschema::validator_for(&schema).expect("the schema compiles");
        (schema, val)
    });
    assert_eq!(v["$schema"], schema["$id"], "the output names its schema");
    let errs: Vec<String> = val.iter_errors(v).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    assert!(errs.is_empty(), "output does not match docc schema compile:\n{}", errs.join("\n"));
}

/// listing is every file below dir with its content.
fn listing(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, m: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, m);
            } else {
                m.insert(p.strip_prefix(base).unwrap().to_string_lossy().into(), std::fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dir, dir, &mut m);
    m
}

fn git(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main", "-c", "maintenance.auto=false", "-c", "gc.auto=0"])
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn ids(v: &Value) -> Vec<&str> {
    v["pages"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect()
}

fn page<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["pages"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("no page {id}"))
}

/// The reader's pages: blocks with pointer, source, HTML and section; the
/// sections of fields and $sections; props; form fields; includes and links
/// resolved; hidden documents and notes left out; nothing written.
#[test]
fn compile_prints_the_reader_pages_and_writes_nothing() {
    let p = project([
        ("docc.json", r#"{"lang":"ru","branding":{"name":"Acme","url":"https://acme.test","logo":"docs/logo.svg"}}"#),
        ("docs/logo.svg", "<svg xmlns='http://www.w3.org/2000/svg'/>"),
        (
            "docs/src/index.json",
            r#"{"title":"Home","body":["See [[a#why/Cost]]: {{a#summary}}","A note[^1] and ![pic](pic.png).","[^1]: The note."],"notes":["secret note"]}"#,
        ),
        (
            "docs/src/a.json",
            r#"{"title":"A","parent":"index","status":"draft","tags":["x"],"body":["Before.",{"$input":"radio","id":"q","label":"Yes?","options":["Yes","No"]}],"summary":"Short fact.","owner":"Ann","why":{"$sections":true,"Cost":"Cheap.","Team":["Small.","Big team."],"Later":""}}"#,
        ),
        ("docs/src/pic.png", "png"),
        ("docs/src/secret.json", r#"{"title":"S","hidden":true,"body":"hidden text"}"#),
    ]);
    let before = listing(&p.path);
    let (code, v) = compile(&p.path, &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(listing(&p.path), before, "compile writes nothing: no docs/out, no docc.lock");
    assert_eq!(v["ok"], true);
    assert_eq!(v["diagnostics"][0]["code"], "DOC304", "the empty section is a warning");
    assert_eq!(v["summary"]["checked"], 3);
    assert_eq!(ids(&v), ["index", "a"], "tree order, the hidden document left out");
    let all = v.to_string();
    assert!(!all.contains("secret note") && !all.contains("hidden text"), "notes and hidden documents stay out");
    assert_eq!(v["project"]["title"], "Home");
    assert_eq!(v["project"]["lang"], "ru");
    assert_eq!(v["project"]["brand"]["name"], "Acme");
    assert!(s(&v["project"]["brand"], "logo").starts_with("data:image/svg+xml;base64,"), "{}", v["project"]);
    assert_eq!(
        v["tree"],
        serde_json::json!([{"type":"page","id":"index","path":"index.json","title":"Home",
            "children":[{"type":"page","id":"a","path":"a.json","title":"A"}]}])
    );

    let a = page(&v, "a");
    assert_eq!((s(a, "path"), s(a, "status"), s(a, "parent"), s(a, "kind")), ("a.json", "draft", "index", "page"));
    assert_eq!((a["tags"].clone(), a["crumbs"].clone()), (serde_json::json!(["x"]), serde_json::json!(["index"])));
    let props: Vec<(&str, &str, &str)> =
        a["props"].as_array().unwrap().iter().map(|p| (s(p, "k"), s(p, "id"), s(p, "p"))).collect();
    assert_eq!(props, [("Summary", "summary", "/summary"), ("Owner", "owner", "/owner")]);
    assert_eq!(
        a["sections"],
        serde_json::json!([
            {"id":"why","title":"Why","level":2},
            {"id":"cost","title":"Cost","level":3},
            {"id":"team","title":"Team","level":3},
            {"id":"later","title":"Later","level":3}
        ])
    );
    let blocks: Vec<(&str, &str, Value)> =
        a["blocks"].as_array().unwrap().iter().map(|b| (s(b, "p"), s(b, "s"), b["sec"].clone())).collect();
    assert_eq!(
        blocks,
        [
            ("/body/0", "Before.", Value::Null),
            ("/body/1", "$input: radio; id: q; label: Yes?; options: Yes\nNo", Value::Null),
            ("/why/Cost", "Cheap.", "cost".into()),
            ("/why/Team/0", "Small.", "team".into()),
            ("/why/Team/1", "Big team.", "team".into()),
        ]
    );
    let field = &a["blocks"][1]["i"];
    assert_eq!((s(field, "t"), s(field, "id"), s(field, "label")), ("radio", "q", "Yes?"));
    assert_eq!(s(&a["blocks"][3], "h"), "<p>Small.</p>\n");

    let home = page(&v, "index");
    let h0 = s(&home["blocks"][0], "h");
    assert!(h0.contains(r##"href="#a--cost""##) && h0.contains("Short fact."), "link and include resolved: {h0}");
    assert!(s(&home["blocks"][1], "h").contains("data:image/png;base64,"), "{}", home["blocks"][1]);
    let last = home["blocks"].as_array().unwrap().last().unwrap();
    assert!(s(last, "p").is_empty() && s(last, "h").contains("footnotes"), "footnotes are the last block: {last}");

    // the blocks are those of the HTML file, without the headings
    let (code, _, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{errs}");
    let app = read_app(&p, "docs/out.html");
    for pg in app.pages() {
        let want: Vec<&str> = pg["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| !(s(b, "p").is_empty() && s(b, "h").starts_with("<h")))
            .map(|b| s(b, "h"))
            .collect();
        let got: Vec<&str> =
            page(&v, s(pg, "id"))["blocks"].as_array().unwrap().iter().map(|b| s(b, "h")).collect();
        assert_eq!(got, want, "page {}", pg["id"]);
    }
}

/// Broken sources still give the object: diagnostics, ok false, no pages;
/// warnings over maxWarnings make ok false with the pages.
#[test]
fn compile_broken_sources_still_print() {
    let p = project([("docs/src/index.json", "{")]);
    let (code, v) = compile(&p.path, &[]);
    assert_eq!(code, 1);
    assert_eq!((v["ok"].clone(), v["diagnostics"][0]["code"].clone()), (Value::Bool(false), "DOC101".into()));
    assert!(v["pages"].as_array().unwrap().is_empty() && v["tree"].as_array().unwrap().is_empty(), "{v}");
    assert!(!p.exists("docs/out") && !p.exists("docc.lock"));

    let p = project([("docc.json", r#"{"maxWarnings":0}"#), ("docs/src/index.json", r#"{"title":"I","body":"TODO"}"#)]);
    let (code, v) = compile(&p.path, &[]);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert!(s(&v["summary"], "failure").contains("too many warnings"), "{}", v["summary"]);
    assert_eq!(ids(&v), ["index"], "warnings do not stop the pages");
    // text: the report and a line
    let (code, out, _) = p.run(&["compile"]);
    assert!(code == 1 && out.contains("DOC303") && out.contains("compiled 1 pages"), "{code} {out}");
}

/// A failure (bad config, bad revision) is still the object, with exit code 2.
#[test]
fn compile_failure_is_json() {
    let p = project([("docc.json", r#"{"tree":"flat"}"#), ("docs/src/index.json", r#"{"title":"I"}"#)]);
    let (code, v) = compile(&p.path, &[]);
    assert_eq!(code, 2);
    assert_eq!(v["ok"], false);
    assert_eq!(v["diagnostics"][0]["code"], "config", "{v}");
    assert!(s(&v["diagnostics"][0], "message").contains(r#"tree must be "parents" or "folders""#), "{v}");
    let (code, _, errs) = p.run(&["compile"]);
    assert!(code == 2 && errs.contains("config error"), "{code} {errs}");

    let p = project([("docs/src/index.json", r#"{"title":"I"}"#)]);
    let (code, v) = compile(&p.path, &["--rev", "HEAD"]);
    assert_eq!(code, 2, "not a Git repository");
    assert_eq!(v["diagnostics"][0]["code"], "usage", "{v}");
    let (code, v) = compile(&p.path, &["--rev", "--all"]);
    assert_eq!((code, v["ok"].clone()), (2, Value::Bool(false)), "an option is not a revision");
}

/// --rev compiles the tree of a commit without a checkout: its documents,
/// schemas, docc.json and docc.lock, with includes resolved in it and
/// positions in its files; the working tree is not touched. A bare
/// repository works too, with --config naming the project folder.
#[test]
fn compile_rev_reads_git_without_checkout() {
    let p = temp_dir();
    if !git(&p.path, &["init", "-q"]) {
        return; // no git here
    }
    let w = |rel: &str, s: &str| p.write(&format!("proj/{rel}"), s);
    w("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#);
    w("docs/src/index.json", "{\n  \"title\": \"I\",\n  \"kind\": \"page\",\n  \"body\": [\"Old TODO\", \"{{a#summary}}\"]\n}\n");
    w("docs/src/a.json", r#"{"title":"A","kind":"page","parent":"index","summary":"First summary."}"#);
    w("docs/src/gone.json", r#"{"title":"Gone","kind":"page","parent":"index"}"#);
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "one"]));
    w("docc.json", r#"{"lang":"ru","trace":{"scan":["code/**"]}}"#);
    w("code/a.go", "package a\n\n// docc: missing-doc\n");
    w("docs/src/index.json", r#"{"title":"I","kind":"page","body":["New","{{a#summary}}"]}"#);
    w("docs/src/a.json", r#"{"title":"A","kind":"page","parent":"index","summary":"Second summary."}"#);
    std::fs::remove_file(p.join("proj/docs/src/gone.json")).unwrap();
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "two"]));
    w("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Working copy"]}"#);
    w("code/a.go", "package a\n// docc: index\n");
    let before = listing(&p.path);

    let dir = p.join("proj");
    let (code, v) = compile(&dir, &["--rev", "HEAD~1"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(ids(&v), ["index", "a", "gone"], "the documents of the commit");
    assert_eq!(v["project"]["lang"], "en", "no docc.json in that commit: the defaults");
    let home = page(&v, "index");
    assert_eq!(s(&home["blocks"][0], "s"), "Old TODO");
    assert!(s(&home["blocks"][1], "h").contains("First summary."), "the include reads the same commit");
    let d = &v["diagnostics"][0];
    assert_eq!(
        (s(d, "code"), s(d, "file"), d["line"].as_u64(), d["column"].as_u64()),
        ("DOC303", "docs/src/index.json", Some(4), Some(17)),
        "positions in the file of the commit: {d}"
    );

    let (code, v) = compile(&dir, &["--rev", "main"]);
    assert_eq!(code, 1, "the code tag of the commit is broken");
    let d = &v["diagnostics"][0];
    assert_eq!((s(d, "code"), s(d, "file"), d["line"].as_u64()), ("DOC401", "code/a.go", Some(3)), "{d}");
    assert_eq!(ids(&v), ["index", "a"]);
    assert_eq!(v["project"]["lang"], "ru");
    assert!(s(&page(&v, "index")["blocks"][1], "h").contains("Second summary."));
    let (code, v) = compile(&dir, &[]);
    assert_eq!(s(&page(&v, "index")["blocks"][0], "s"), "Working copy", "without --rev: the disk");
    assert_eq!(code, 0, "the code tag on the disk is fine: {}", v["diagnostics"]);
    let after = listing(&p.path);
    let changed: Vec<&String> = after.keys().chain(before.keys()).filter(|k| after.get(*k) != before.get(*k)).collect();
    assert!(changed.is_empty(), "the working tree is not touched: {changed:?}");

    let bare = temp_dir();
    let target = bare.join("mirror.git");
    assert!(git(&p.path, &["clone", "-q", "--bare", ".", target.to_str().unwrap()]));
    let (code, v) = compile(&target, &["--rev", "HEAD~1", "--config", "proj/docc.json"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(ids(&v), ["index", "a", "gone"], "a bare mirror: no checkout at all, no docc.json yet");
    let (_, v) = compile(&target, &["--rev", "HEAD", "--config", "proj/docc.json"]);
    assert_eq!((ids(&v), v["project"]["lang"].clone()), (vec!["index", "a"], Value::from("ru")));
}

/// --hidden puts the hidden documents into pages and tree, marked
/// "hidden": true, on their place by parent; their notes never come out, the
/// numbers of the others stay, links among hidden documents work, and without
/// the flag nothing changes.
#[test]
fn compile_hidden_adds_hidden_pages_without_notes() {
    let p = project([
        ("docc.json", r#"{"numbering":true}"#),
        ("docs/src/index.json", r#"{"title":"Home","notes":["index note"]}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index","body":"Visible."}"#),
        (
            "docs/src/h1.json",
            r#"{"title":"H1","parent":"index","hidden":true,"order":-1,"body":"See [[a]] and [[h2]].","notes":["secret note"]}"#,
        ),
        ("docs/src/h2.json", r#"{"title":"H2","parent":"h1","hidden":true,"body":"Back to [[h1]].","notes":["other secret"]}"#),
        ("docs/src/b.json", r#"{"title":"B","parent":"index"}"#),
    ]);
    let (code, plain) = compile(&p.path, &[]);
    assert_eq!(code, 0, "{plain}");
    assert_eq!(ids(&plain), ["index", "a", "b"], "without the flag no hidden page");
    assert!(!plain.to_string().contains("\"hidden\""), "and no hidden field: {plain}");

    let (code, v) = compile(&p.path, &["--hidden"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["diagnostics"], plain["diagnostics"], "the verdict does not change");
    assert_eq!(ids(&v), ["index", "h1", "h2", "a", "b"], "hidden pages in tree order");
    let all = v.to_string();
    assert!(!all.contains("secret") && !all.contains("index note"), "no notes: {all}");
    assert_eq!(
        v["tree"],
        serde_json::json!([{"type":"page","id":"index","path":"index.json","title":"Home","children":[
            {"type":"page","id":"h1","path":"h1.json","title":"H1","hidden":true,"children":[
                {"type":"page","id":"h2","path":"h2.json","title":"H2","hidden":true}]},
            {"type":"page","id":"a","path":"a.json","title":"1 A"},
            {"type":"page","id":"b","path":"b.json","title":"2 B"}]}])
    );
    for id in ["index", "a", "b"] {
        assert!(page(&v, id).get("hidden").is_none(), "{id} is visible");
        assert_eq!(page(&v, id), page(&plain, id), "{id} reads as without the flag: numbers stay");
    }
    let h2 = page(&v, "h2");
    assert_eq!((h2["hidden"].clone(), s(h2, "parent")), (Value::Bool(true), "h1"));
    assert_eq!(h2["crumbs"], serde_json::json!(["index", "h1"]));
    assert_eq!(page(&v, "h1")["crumbs"], serde_json::json!(["index"]));
    assert_eq!(s(&page(&v, "h2")["blocks"][0], "h"), "<p>Back to <a href=\"#h1\">H1</a>.</p>\n");
    let h1 = s(&page(&v, "h1")["blocks"][0], "h");
    assert!(h1.contains("href=\"#a\"") && h1.contains("href=\"#h2\""), "{h1}");

    // a visible document that links to a hidden one stays an error, the flag or not
    p.write("docs/src/a.json", r#"{"title":"A","parent":"index","body":"Link [[h1]]."}"#);
    let (c0, no) = compile(&p.path, &[]);
    let (c1, yes) = compile(&p.path, &["--hidden"]);
    assert_eq!((c0, c1), (1, 1));
    assert_eq!(yes["diagnostics"], no["diagnostics"]);
    assert!(yes["diagnostics"].to_string().contains("DOC210"), "{yes}");
    assert_eq!((yes["ok"].clone(), yes["pages"].clone()), (Value::Bool(false), serde_json::json!([])));
}

/// --hidden together with --rev and --config: the hidden documents of the
/// commit, read from a bare mirror too; the disk is not read.
#[test]
fn compile_hidden_with_rev_and_config() {
    let p = temp_dir();
    if !git(&p.path, &["init", "-q"]) {
        return; // no git here
    }
    let w = |rel: &str, s: &str| p.write(&format!("proj/{rel}"), s);
    w("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#);
    w("docs/src/index.json", r#"{"title":"I","kind":"page"}"#);
    w("docs/src/h.json", r#"{"title":"H","kind":"page","parent":"index","hidden":true,"notes":["secret"]}"#);
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "one"]));
    w("docs/src/h.json", r#"{"title":"H on disk","kind":"page","parent":"index","hidden":true}"#);
    let dir = p.join("proj");
    let (code, v) = compile(&dir, &["--rev", "HEAD", "--hidden"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!((ids(&v), s(page(&v, "h"), "title"), page(&v, "h")["hidden"].clone()), (vec!["index", "h"], "H", Value::Bool(true)));
    assert!(!v.to_string().contains("secret"));
    let (_, v) = compile(&dir, &["--rev", "HEAD"]);
    assert_eq!(ids(&v), ["index"]);

    let bare = temp_dir();
    let target = bare.join("mirror.git");
    assert!(git(&p.path, &["clone", "-q", "--bare", ".", target.to_str().unwrap()]));
    let (code, v) = compile(&target, &["--rev", "HEAD", "--config", "proj/docc.json", "--hidden"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(ids(&v), ["index", "h"]);
    assert_eq!(v["tree"][0]["children"][0]["hidden"], true);
}

/// In folder mode a hidden page is in its folder with the mark, and a folder
/// whose own page is hidden is marked too.
#[test]
fn compile_hidden_in_folders() {
    let p = project([
        ("docc.json", r#"{"tree":"folders"}"#),
        ("docs/src/index.json", r#"{"title":"Home"}"#),
        ("docs/src/g/g.json", r#"{"title":"G","hidden":true,"body":"x"}"#),
        ("docs/src/g/k.json", r#"{"title":"K","hidden":true,"body":"y"}"#),
    ]);
    let (_, plain) = compile(&p.path, &[]);
    assert_eq!(ids(&plain), ["index"]);
    let (code, v) = compile(&p.path, &["--hidden"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(ids(&v), ["g", "k", "index"], "the folder G comes before Home");
    assert_eq!(
        v["tree"][0],
        serde_json::json!({"type":"folder","path":"g","title":"G","id":"g","hidden":true,"children":[
            {"type":"page","id":"k","path":"g/k.json","title":"K","hidden":true}]})
    );
}

/// "tree": "folders": the tree is the folders of src, a folder's own page is
/// the document named like it, siblings by order, then title; roots, DOC203
/// and DOC205 do not apply; _tree.md follows the folders.
#[test]
fn folder_tree() {
    let p = project([
        ("docc.json", r#"{"tree":"folders","roots":["nope"]}"#),
        ("docs/src/index.json", r#"{"title":"Home","order":1}"#),
        ("docs/src/zeta.json", r#"{"title":"Zeta"}"#),
        ("docs/src/decisions/decisions.json", r#"{"title":"Decisions","body":"All."}"#),
        ("docs/src/decisions/monolith.json", r#"{"title":"Monolith","parent":"decisions"}"#),
        ("docs/src/decisions/alpha.json", r#"{"title":"Alpha","order":5}"#),
        ("docs/src/decisions/old/ancient.json", r#"{"title":"Ancient"}"#),
        ("docs/src/guides/intro.json", r#"{"title":"Intro"}"#),
        ("docs/src/guides/secret.json", r#"{"title":"Secret","hidden":true}"#),
    ]);
    let (code, v) = compile(&p.path, &[]);
    assert_eq!(code, 0, "no DOC203 for zeta, no DOC205 for nope: {}", v["diagnostics"]);
    assert_eq!(
        v["tree"],
        serde_json::json!([
            {"type":"page","id":"index","path":"index.json","title":"Home"},
            {"type":"folder","path":"decisions","title":"Decisions","id":"decisions","children":[
                {"type":"page","id":"alpha","path":"decisions/alpha.json","title":"Alpha"},
                {"type":"page","id":"monolith","path":"decisions/monolith.json","title":"Monolith"},
                {"type":"folder","path":"decisions/old","title":"old","children":[
                    {"type":"page","id":"ancient","path":"decisions/old/ancient.json","title":"Ancient"}
                ]}
            ]},
            {"type":"folder","path":"guides","title":"guides","children":[
                {"type":"page","id":"intro","path":"guides/intro.json","title":"Intro"}
            ]},
            {"type":"page","id":"zeta","path":"zeta.json","title":"Zeta"}
        ])
    );
    assert_eq!(ids(&v), ["index", "decisions", "alpha", "monolith", "ancient", "intro", "zeta"]);
    assert_eq!(page(&v, "monolith")["crumbs"], serde_json::json!(["decisions"]), "parent still gives crumbs");
    p.must_build();
    assert_eq!(
        p.read("docs/out/_tree.md"),
        "<!-- GENERATED by docc, DO NOT EDIT -->\n\n# Document tree\n\n\
         - [Home](index.md)\n\
         - [Decisions](decisions/decisions.md)\n  - [Alpha](decisions/alpha.md)\n  - [Monolith](decisions/monolith.md)\n  - old\n    - [Ancient](decisions/old/ancient.md)\n\
         - guides\n  - [Intro](guides/intro.md)\n\
         - [Zeta](zeta.md)\n"
    );
    // the HTML file nests its tree the same way: a folder without a page is a line with its name
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let app = read_app(&p, "docs/out.html");
    assert_eq!(
        app.data["nav"]["tree"],
        serde_json::json!([["index", 0], ["decisions", 0], ["alpha", 1], ["monolith", 1], ["", 1, "old"], ["ancient", 2], ["", 0, "guides"], ["intro", 1], ["zeta", 0]])
    );
    let kids = |id: &str| app.pages().iter().find(|p| p["id"] == id).unwrap()["kids"].clone();
    assert_eq!(kids("decisions"), serde_json::json!(["alpha", "monolith"]), "the pages of a folder are its own page's kids");
    std::fs::remove_file(p.join("docs/out.html")).unwrap();
    // numbering follows the folders
    p.write("docc.json", r#"{"tree":"folders","numbering":true}"#);
    let (_, v) = compile(&p.path, &[]);
    assert_eq!(v["tree"][1]["children"][2]["title"], "2.3 old");
    assert_eq!(page(&v, "ancient")["title"], "2.3.1 Ancient");
    // without folders the same project has orphans
    p.write("docc.json", "{}");
    let (code, out, _) = p.run(&["validate"]);
    assert!(code == 1 && out.contains("DOC203"), "{out}");
}

/// A folder's own page with no text (a title-only folder page) only names and
/// orders its folder: no page file, no page in --single or --html, the folder
/// is a plain name in _tree.md and the HTML tree, a link to it is its title;
/// docc compile keeps it in the tree and in pages.
#[test]
fn title_only_folder_page() {
    let p = project([
        ("docc.json", r#"{"tree":"folders"}"#),
        ("docs/src/index.json", r#"{"title":"Home","order":0,"body":"See [[notes]] and [[decisions]]."}"#),
        ("docs/src/decisions/decisions.json", r#"{"title":"Decisions","body":"All."}"#),
        ("docs/src/decisions/monolith.json", r#"{"title":"Monolith"}"#),
        ("docs/src/notes/notes.json", r#"{"title":"Notes","order":2,"tags":["x"]}"#),
        ("docs/src/notes/first.json", r#"{"title":"First","parent":"notes"}"#),
        ("docs/src/notes/sub/sub.json", r#"{"title":"Sub","body":""}"#),
        ("docs/src/notes/sub/deep.json", r#"{"title":"Deep"}"#),
    ]);
    if git(&p.path, &["init", "-q"]) {
        assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "one"]));
    }
    p.must_build();
    assert!(!p.exists("docs/out/notes/notes.md") && !p.exists("docs/out/notes/sub/sub.md"), "no page file");
    assert!(p.exists("docs/out/decisions/decisions.md"), "a folder page with text keeps its file");
    assert_eq!(
        p.read("docs/out/_tree.md"),
        "<!-- GENERATED by docc, DO NOT EDIT -->\n\n# Document tree\n\n\
         - [Home](index.md)\n\
         - Notes\n  - [First](notes/first.md)\n  - Sub\n    - [Deep](notes/sub/deep.md)\n\
         - [Decisions](decisions/decisions.md)\n  - [Monolith](decisions/monolith.md)\n"
    );
    assert!(p.read("docs/out/index.md").contains("See Notes and [Decisions](decisions/decisions.md)."), "{}", p.read("docs/out/index.md"));
    assert!(!p.read("docs/out/notes/first.md").contains("notes.md"), "no Parent link to a missing page");
    let (code, out, errs) = p.run(&["build", "--single", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let md = p.read("docs/out.md");
    assert!(!md.contains(r#"id="notes""#) && !md.contains("](#notes)") && !md.contains(r#"id="sub""#), "{md}");
    assert!(md.contains("- Notes\n") && md.contains("See Notes and"), "{md}");
    let app = read_app(&p, "docs/out.html");
    assert_eq!(
        app.data["nav"]["tree"],
        serde_json::json!([["index", 0], ["", 0, "Notes"], ["first", 1], ["", 1, "Sub"], ["deep", 2], ["decisions", 0], ["monolith", 1]])
    );
    let shown: Vec<&str> = app.pages().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(shown, ["index", "first", "deep", "decisions", "monolith"]);
    assert!(!app.blocks.contains("#notes") && app.blocks.contains("See Notes and"), "{}", app.blocks);
    let first = app.pages().iter().find(|p| p["id"] == "first").unwrap();
    assert!(first["crumbs"].as_array().is_none_or(|c| c.is_empty()) && s(first, "parent").is_empty(), "{first}");
    if let Some(h) = app.data["hist"]["docs"].as_object() {
        assert!(!h.contains_key("notes/notes.json") && h.contains_key("notes/first.json"), "no history of a page the file does not have");
    }
    let (code, _, _) = p.run(&["build", "--page", "notes"]);
    assert_ne!(code, 0, "--page names no page");
    // docc compile keeps the folder's id and title and the page itself
    let (_, v) = compile(&p.path, &[]);
    assert_eq!((&v["tree"][1]["id"], &v["tree"][1]["title"]), (&Value::from("notes"), &Value::from("Notes")));
    assert_eq!(ids(&v), ["index", "notes", "first", "sub", "deep", "decisions", "monolith"]);
}

/// The output of every fixture project (golden cases, render cases, skill
/// evals, this repository) matches docc schema compile.
#[test]
fn compile_fixtures_match_schema() {
    // this repository in place: compile writes nothing
    let (code, v) = compile(&repo(), &[]);
    assert_eq!(code, 0, "{}", v["diagnostics"]);
    let mut dirs = Vec::new();
    for base in ["testdata", "testdata/render", "skills/docc/evals"] {
        for e in std::fs::read_dir(repo().join(base)).unwrap().flatten() {
            for kind in ["", "red", "green"] {
                let d = e.path().join(kind);
                if d.join("docs").is_dir() || d.join("docc.json").is_file() {
                    dirs.push(d);
                }
            }
        }
    }
    assert!(dirs.len() > 80, "{} fixtures", dirs.len());
    std::thread::scope(|sc| {
        for d in &dirs {
            sc.spawn(move || {
                // a copy: a case without docc.json would find the repository's own above it
                let work = temp_dir();
                copy_tree(d, &work.path).unwrap();
                let (code, v) = compile(&work.path, &[]);
                assert!(code != 2, "{}: {v}", d.display());
            });
        }
    });
}

/// build --rev builds the HTML file of a commit without a checkout, into the
/// current folder: the pages of that commit and its history up to it, and
/// nothing else written (no docs/out, no docc.lock). A bare mirror works with
/// --config naming the project folder. Without Git the file has no history.
#[test]
fn build_rev_writes_only_the_html_file() {
    let p = temp_dir();
    if !git(&p.path, &["init", "-q"]) {
        return; // no git here
    }
    let w = |rel: &str, s: &str| p.write(&format!("proj/{rel}"), s);
    w("docc.json", r#"{"lang":"ru"}"#);
    w("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#);
    w("docs/src/index.json", r#"{"title":"I","kind":"page","body":["One"]}"#);
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "one"]));
    w("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Two"]}"#);
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "two"]));
    w("docs/src/index.json", r#"{"title":"I","kind":"page","body":["Working copy"]}"#);
    let before = listing(&p.path);
    let dir = p.join("proj");
    let html = |dir: &Path| {
        let s = std::fs::read_to_string(dir.join("out.html")).unwrap();
        const OPEN: &str = r#"<script type="application/json" id="docc-data">"#;
        let a = s.find(OPEN).unwrap() + OPEN.len();
        let v: Value = serde_json::from_str(&s[a..a + s[a..].find("</script>").unwrap()]).unwrap();
        std::fs::remove_file(dir.join("out.html")).unwrap();
        v
    };
    let text = |v: &Value| v["pages"][0]["blocks"][0]["s"].as_str().unwrap().to_string();

    let (code, out, errs) = do_in(&dir, &["build", "--rev", "HEAD~1", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(out.contains("built out.html"), "{out}");
    let v = html(&dir);
    assert_eq!(text(&v), "One", "the documents of the commit");
    assert_eq!(v["hist"]["commits"].as_array().unwrap().len(), 1, "the history ends at the commit: {}", v["hist"]);
    assert_eq!(listing(&p.path), before, "nothing else is written");

    let bare = temp_dir();
    let mirror = bare.join("mirror.git");
    assert!(git(&p.path, &["clone", "-q", "--bare", ".", mirror.to_str().unwrap()]));
    let (code, out, errs) = do_in(&mirror, &["build", "--rev", "main", "--html", "--config", "proj/docc.json", "--format", "json"]);
    assert_eq!(code, 0, "{out} {errs}");
    let v = html(&mirror);
    assert_eq!((text(&v), v["hist"]["commits"].as_array().unwrap().len(), v["lang"].as_str()), ("Two".into(), 2, Some("ru")));
    // run elsewhere, the mirror named by --config: the file lands where docc runs
    let cwd = temp_dir();
    let cfg = mirror.join("proj/docc.json");
    let (code, out, errs) = do_in(&cwd.path, &["build", "--rev", "HEAD~1", "--html", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {errs}");
    assert_eq!(text(&html(&cwd.path)), "One");

    for args in [&["build", "--rev", "HEAD"][..], &["build", "--rev", "HEAD", "--html", "--single"], &["build", "--rev", "nope", "--html"]] {
        let (code, _, errs) = do_in(&dir, args);
        assert_eq!(code, 2, "{args:?}: {errs}");
    }

    // no Git at all (a checkout without .git): the file is built without history
    let plain = temp_dir();
    copy_tree(&dir, &plain.path).unwrap();
    let (code, out, errs) = do_in(&plain.path, &["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let s = plain.read("docs/out.html");
    assert!(s.contains("Working copy") && s.contains(r#""hist":null"#), "no timeline without Git");
}

/// The review's reproduction: a page drafted hidden, edited while hidden and
/// then shown. The reader's file (build --rev --html from a bare mirror, as
/// Docspace builds it) has none of the hidden versions; the audit build has.
#[test]
fn hidden_versions_stay_out_of_the_timeline() {
    let p = temp_dir();
    if !git(&p.path, &["init", "-q"]) {
        return; // no git here
    }
    let w = |rel: &str, s: &str| p.write(&format!("proj/{rel}"), s);
    let pay = |hidden: bool, text: &str| format!(r#"{{"title":"Pay","kind":"page","parent":"index","hidden":{hidden},"body":["{text}"]}}"#);
    w("docc.json", "{}");
    w("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#);
    w("docs/src/index.json", r#"{"title":"I","kind":"page","body":["One"]}"#);
    w("docs/src/pay.json", &pay(true, "HIDDENDOCTEXT"));
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "hidden"]));
    w("docs/src/pay.json", &pay(true, "WHILEHIDDEN"));
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "hidden edit"]));
    w("docs/src/pay.json", &pay(false, "NOWPUBLIC"));
    assert!(git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "unhidden"]));
    let bare = temp_dir();
    let mirror = bare.join("mirror.git");
    assert!(git(&p.path, &["clone", "-q", "--bare", ".", mirror.to_str().unwrap()]));
    let cfg = mirror.join("proj/docc.json");
    let cwd = temp_dir();
    let (code, out, errs) = do_in(&cwd.path, &["build", "--rev", "HEAD", "--html", "--no-comments", "--config", cfg.to_str().unwrap()]);
    assert_eq!(code, 0, "{out} {errs}");
    let s = cwd.read("out.html");
    assert!(s.contains("NOWPUBLIC"), "the page as it is now");
    for leak in ["HIDDENDOCTEXT", "WHILEHIDDEN"] {
        assert!(!s.contains(leak), "the reader's file holds the hidden version {leak:?}");
    }
    let (code, out, errs) = do_in(&p.join("proj"), &["build", "--html", "--full"]);
    assert_eq!(code, 0, "{out} {errs}");
    let s = p.read("proj/docs/out-full.html");
    assert!(s.contains("HIDDENDOCTEXT") && s.contains("WHILEHIDDEN"), "the audit build keeps every version");
}
