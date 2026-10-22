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
