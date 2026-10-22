//! docc comments apply: the readers' comments written into "$comments" of the
//! documents, all or nothing, with every other byte of a file kept; and the
//! comments in docc compile and in the HTML export that comes back.
mod common;

use std::path::Path;
use std::process::Command;

use common::*;
use serde_json::{Value, json};

const SCHEMA: (&str, &str) = ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#);

const PAGE: &str = "{\n  \"title\": \"Index\",\n  \"kind\": \"page\",\n  \"body\": [\n    \"Alpha text.\",\n    \"Beta text.\"\n  ]\n}\n";

/// apply runs docc comments apply --format json with ops on standard input.
fn apply(dir: &Path, ops: &Value) -> (i32, Value, String) {
    let (code, out, errs) = do_stdin(dir, ops.to_string().as_bytes(), &["comments", "apply", "--format", "json"]);
    let errs = match serde_json::from_str::<Value>(&out) {
        Ok(r) if code != 0 => format!("{errs}{}", r["diagnostics"][0]["message"].as_str().unwrap_or_default()),
        _ => errs,
    };
    let v = if code == 0 {
        let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out} {errs}"));
        check_schema("comments-apply", &v);
        v
    } else {
        Value::Null
    };
    (code, v, errs)
}

/// check_schema validates an output against docc schema <name>.
fn check_schema(name: &str, v: &Value) {
    let (_, out, _) = do_in(&repo(), &["schema", name]);
    let schema: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["$schema"], schema["$id"], "the output names its schema");
    let val = jsonschema::validator_for(&schema).expect("the schema compiles");
    let errs: Vec<String> = val.iter_errors(v).map(|e| format!("{e} at {}", e.instance_path())).collect();
    assert!(errs.is_empty(), "output does not match docc schema {name}:\n{}", errs.join("\n"));
}

#[test]
fn apply_edits_only_comments() {
    let p = project([SCHEMA, ("docs/src/index.json", PAGE), ("docs/src/other.json", "{\"title\":\"Other\",\"kind\":\"page\",\"parent\":\"index\",\"body\":[\"Gamma, see [[index]].\"]}")]);
    // a comment is no edit of the text: the links to the document stay confirmed (DOC209)
    p.must_build();
    let ops = json!([
        {"op": "add", "doc": "index", "comment": {"at": "/body/1", "fragment": "Beta", "text": "Why beta?", "author": "Ann", "email": "ann@example.com"}},
        {"op": "add", "doc": "docs/src/other.json", "comment": {"id": "c-given", "at": "/body/0", "text": "And this?", "author": "Bob", "created": "2026-10-22T09:00:00Z"}}
    ]);
    let (code, v, errs) = apply(&p.path, &ops);
    assert_eq!(code, 0, "{errs}");
    let id = v["added"][0]["id"].as_str().unwrap().to_string();
    assert!(id.len() == 12 && id.starts_with("c-") && id[2..].chars().all(|c| c.is_ascii_lowercase() || ('2'..='7').contains(&c)), "{id}");
    assert_eq!(v["added"][1], json!({"id": "c-given", "doc": "other", "path": "other.json"}));
    assert_eq!(v["files"], json!(["index.json", "other.json"]));
    let src = p.read("docs/src/index.json");
    let head = PAGE.strip_suffix("\n}\n").unwrap();
    assert!(src.starts_with(head), "the members before stay as they were:\n{src}");
    let rest = &src[head.len()..];
    assert!(rest.starts_with(&format!(",\n  \"$comments\": [\n    {{\"id\": \"{id}\", \"at\": \"/body/1\", \"quote\": \"Beta text.\", \"fragment\": \"Beta\", \"text\": \"Why beta?\", \"author\": \"Ann\", \"email\": \"ann@example.com\", \"created\": \"20")), "{rest}");
    assert!(rest.ends_with("\"}\n  ]\n}\n"), "the last member, indented like the file:\n{rest}");
    assert_eq!(
        p.read("docs/src/other.json"),
        "{\"title\":\"Other\",\"kind\":\"page\",\"parent\":\"index\",\"body\":[\"Gamma, see [[index]].\"], \"$comments\": [{\"id\": \"c-given\", \"at\": \"/body/0\", \"quote\": \"Gamma, see [[index]].\", \"text\": \"And this?\", \"author\": \"Bob\", \"created\": \"2026-10-22T09:00:00Z\"}]}",
        "a file on one line stays on one line"
    );
    let (code, out, errs) = p.run(&["validate", "--max-warnings", "0"]);
    assert_eq!(code, 0, "{out} {errs}");

    // one bad operation fails the batch and nothing is written
    let before = (p.read("docs/src/index.json"), p.read("docs/src/other.json"));
    for (ops, want) in [
        (json!([{"op": "rm", "id": "c-given"}, {"op": "rm", "id": "c-nope"}]), "operation 2: no comment \"c-nope\""),
        (json!([{"op": "add", "doc": "nope", "comment": {"at": "/body/0", "text": "x", "author": "A"}}]), "operation 1: no document \"nope\""),
        (json!([{"op": "add", "doc": "index", "comment": {"at": "/body/0", "text": "x", "input": "f", "answer": ["Y"], "author": "A"}}]), "a comment has either"),
        (json!([{"op": "add", "doc": "index", "comment": {"at": "/body/0", "text": "x"}}]), "lacks \"author\""),
        (json!([{"op": "add", "doc": "index", "comment": {"at": "/body/0", "text": "x", "author": "A", "status": "done"}}]), "unknown field `status`"),
        (json!([{"op": "add", "doc": "index", "comment": {"id": "c-given", "at": "/body/0", "text": "x", "author": "A"}}]), "comment id \"c-given\" is taken (in other.json)"),
        (json!([{"op": "edit", "id": "c-given", "text": ""}]), "the text of a comment is empty"),
        (json!([{"op": "done", "id": "c-given"}]), "operation 1: unknown variant `done`"),
        (json!({"op": "rm"}), "give a JSON array of operations"),
    ] {
        let (code, _, errs) = apply(&p.path, &ops);
        assert!(code == 2 && errs.contains(want), "{ops}: {code} {errs}");
        assert_eq!((p.read("docs/src/index.json"), p.read("docs/src/other.json")), before, "{ops}: nothing written");
    }

    // edit, then remove: the files are exactly as they were before the comments
    let (code, v, errs) = apply(&p.path, &json!([{"op": "edit", "id": id, "text": "Why \"beta\"?"}]));
    assert!(code == 0 && v["edited"] == json!([id]), "{errs}");
    assert!(p.read("docs/src/index.json").contains(r#""text": "Why \"beta\"?""#));
    let (code, v, errs) = apply(&p.path, &json!([{"op": "rm", "id": id}, {"op": "rm", "id": "c-given"}]));
    assert!(code == 0 && v["removed"] == json!([id, "c-given"]), "{errs}");
    assert_eq!(p.read("docs/src/index.json"), PAGE);
    assert_eq!(p.read("docs/src/other.json"), "{\"title\":\"Other\",\"kind\":\"page\",\"parent\":\"index\",\"body\":[\"Gamma, see [[index]].\"]}");
    let (_, out, _) = do_in(&p.path, &["schema", "comments-apply"]);
    assert!(out.contains("urn:docc:schema:comments-apply:1"));
}

/// compile gives each page its comments with their place and without email;
/// a lost comment is shown at the top of the Markdown page.
#[test]
fn compile_and_markdown_show_comments() {
    let p = project([SCHEMA, ("docs/src/index.json", PAGE)]);
    let ops = json!([
        {"op": "add", "doc": "index", "comment": {"at": "/body/0", "text": "Same.", "author": "Ann", "email": "ann@example.com", "created": "2026-10-22T09:00:00Z"}},
        {"op": "add", "doc": "index", "comment": {"at": "/body/1", "fragment": "Beta", "text": "Line one\nline two", "author": "", "created": "2026-10-22T09:00:00Z"}},
        {"op": "add", "doc": "index", "comment": {"at": "/body/1", "fragment": "Delta", "text": "Lost <b>.", "author": "Bob", "created": "2026-10-22T09:00:00Z"}}
    ]);
    let (code, _, errs) = apply(&p.path, &ops);
    assert_eq!(code, 0, "{errs}");
    p.write("docs/src/index.json", p.read("docs/src/index.json").replacen("\"Beta text.\"", "\"Beta text, edited.\"", 1));
    let (_, out, _) = p.run(&["compile", "--format", "json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    check_schema("compile", &v);
    let cs = v["pages"][0]["comments"].as_array().unwrap();
    let places: Vec<&str> = cs.iter().map(|c| c["place"].as_str().unwrap()).collect();
    assert_eq!(places, ["same", "changed", "lost"]);
    assert!(cs[0].get("email").is_none() && cs[0]["text"] == "Same." && cs[0]["at"] == "/body/0", "{}", cs[0]);
    p.must_build();
    let md = p.read("docs/out/index.md");
    assert!(
        md.contains("# Index\n\npage\n\n> 💬 Comments that lost their place:\n>\n> - **Bob**, 2026-10-22, on «Delta»: Lost &lt;b>.\n\nAlpha text.\n\n> 💬 **Ann**, 2026-10-22: Same.\n\nBeta text, edited.\n\n> 💬 **Anonymous**, 2026-10-22, on «Beta»: Line one\n> line two\n"),
        "{md}"
    );
    // docc check takes docs/out with or without the comments
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "{out}");
    let (code, out, errs) = p.run(&["build", "--no-comments"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(!p.read("docs/out/index.md").contains("💬"));
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "{out}");
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A comment left on an older build follows its block to where it stands
/// now: the build commit is read from Git and lined up with the current text
/// by the alignment of docc diff; an answer goes to its form field.
#[test]
fn import_moves_a_comment_from_an_older_build() {
    let field = r#"{"$input":"text","id":"when","label":"When?"}"#;
    let page = format!(r#"{{"title":"I","kind":"page","body":["One.","Two.",{field}]}}"#);
    let p = project([SCHEMA, ("docs/src/index.json", page.as_str())]);
    git(&p.path, &["init", "-q"]);
    git(&p.path, &["add", "-A"]);
    git(&p.path, &["commit", "-q", "-m", "first"]);
    let build = git(&p.path, &["rev-parse", "--short=8", "HEAD"]);
    p.write("docs/src/index.json", format!(r#"{{"title":"I","kind":"page","body":["Zero.","One.","Two, edited.",{field}]}}"#));
    git(&p.path, &["commit", "-q", "-am", "second"]);
    p.write(
        "export.json",
        json!({"docc": "comments", "version": 2, "comments": [
            {"id": "c-old0000001", "doc": "index", "path": "/body/1", "quote": "Two.", "fragment": "Two", "text": "Why two?", "author": "Ann", "created": "2026-10-20T10:00:00Z", "build": build},
            {"id": "c-old0000002", "doc": "index", "path": "/body/2", "quote": "", "input": "when", "answer": ["Monday"], "text": "Monday", "author": "Ann", "created": "2026-10-20T10:00:00Z", "build": build},
            {"id": "c-old0000003", "doc": "gone", "path": "/body/0", "quote": "x", "text": "On a deleted page", "build": build}
        ]})
        .to_string(),
    );
    let (code, out, errs) = p.run(&["comments", "import", "export.json"]);
    assert!(
        code == 0 && out == "imported 2 comments into 1 documents (2 moved to their block; 1 on a page that is gone, not imported: c-old0000003)\n",
        "{code} {out} {errs}"
    );
    let (_, out, _) = p.run(&["comments", "--format", "json"]);
    let list: Value = serde_json::from_str(&out).unwrap();
    let at: Vec<(&str, &str)> = list["comments"].as_array().unwrap().iter().map(|c| (c["at"].as_str().unwrap(), c["place"].as_str().unwrap())).collect();
    assert_eq!(at, [("/body/2", "changed"), ("/body/3", "changed")]);
}
