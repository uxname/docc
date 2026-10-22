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
    // the quote is the text of the block now (apply never keeps a quote it is given), so both read same
    assert_eq!(at, [("/body/2", "same"), ("/body/3", "same")]);
}

/// apply_with runs docc comments apply with extra flags; it returns the exit
/// code and standard output and error together.
fn apply_with(dir: &Path, ops: &Value, flags: &[&str]) -> (i32, String) {
    let mut args = vec!["comments", "apply"];
    args.extend_from_slice(flags);
    let (code, out, errs) = do_stdin(dir, ops.to_string().as_bytes(), &args);
    (code, out + &errs)
}

/// A comment stands only where a reader looks: an "at" in a part readers do
/// not see is refused with nothing written, so docc never copies hidden text
/// into a quote that compile then shows to readers.
#[test]
fn apply_refuses_places_readers_do_not_see() {
    let page = r#"{"title":"Index","kind":"page","body":["Alpha text.",{"$input":"text","id":"when","label":"When?"}],"owner":"Ann","due":"Monday","notes":["INTERNAL: the price is 40k"],"hidden":false,"docIgnore":[{"rule":"DOC353","reason":"x"}]}"#;
    let p = project([SCHEMA, ("docs/src/index.json", page)]);
    // the reproduction: no quote given, at points into notes
    let leak = json!([{"op": "add", "doc": "index", "comment": {"at": "/notes/0", "text": "?", "author": "Eve"}}]);
    let (code, out) = apply_with(&p.path, &leak, &[]);
    assert!(code == 2 && out.contains("index.json shows a reader no block at \"/notes/0\""), "{code} {out}");
    assert_eq!(p.read("docs/src/index.json"), page, "nothing written");
    for at in ["/notes", "/hidden", "/docIgnore/0/reason", "/title", "/kind", "/body", "/body/7", "/$comments/0", "", "/"] {
        let ops = json!([{"op": "add", "doc": "index", "comment": {"at": at, "text": "?", "author": "Eve"}}]);
        let (code, out) = apply_with(&p.path, &ops, &[]);
        assert_eq!(code, 2, "{at}: {out}");
        assert_eq!(p.read("docs/src/index.json"), page, "{at}: nothing written");
    }
    // an answer needs the form field with its id at at
    let ops = json!([{"op": "add", "doc": "index", "comment": {"at": "/body/1", "input": "other", "answer": ["x"], "author": "Eve"}}]);
    let (code, out) = apply_with(&p.path, &ops, &[]);
    assert!(code == 2 && out.contains("has no form field \"other\" at \"/body/1\""), "{code} {out}");
    let ops = json!([{"op": "add", "doc": "index", "comment": {"at": "/body/0", "input": "when", "answer": ["x"], "author": "Eve"}}]);
    assert_eq!(apply_with(&p.path, &ops, &[]).0, 2, "a paragraph is no form field");
    // a block, a property and a form field are fine; a quote given is replaced by the block's text
    let (code, v, errs) = apply(
        &p.path,
        &json!([
            {"op": "add", "doc": "index", "comment": {"id": "c-a", "at": "/body/0", "quote": "INTERNAL: the price is 40k", "text": "?", "author": "Eve"}},
            {"op": "add", "doc": "index", "comment": {"id": "c-b", "at": "/owner", "text": "Who?", "author": "Eve"}},
            {"op": "add", "doc": "index", "comment": {"id": "c-c", "at": "/body/1", "input": "when", "answer": ["Monday"], "author": "Eve"}}
        ]),
    );
    assert_eq!(code, 0, "{errs}");
    assert_eq!(v["added"].as_array().unwrap().len(), 3);
    let (_, out, _) = p.run(&["compile", "--format", "json"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let quotes: Vec<&str> = v["pages"][0]["comments"].as_array().unwrap().iter().map(|c| c["quote"].as_str().unwrap()).collect();
    assert_eq!(quotes, ["Alpha text.", "Ann", "$input: text; id: when; label: When?"]);
    assert!(!out.contains("INTERNAL"), "compile shows no hidden text: {out}");
    // a comment on a comment is refused too
    let ops = json!([{"op": "add", "doc": "index", "comment": {"at": "/$comments/0", "text": "?", "author": "Eve"}}]);
    assert_eq!(apply_with(&p.path, &ops, &[]).0, 2);
}

/// A hidden document takes no comment and keeps its comments unless --hidden
/// is given (an agent working by hand); Docspace never gives it.
#[test]
fn hidden_documents_need_the_flag() {
    let secret = r#"{"title":"Secret","kind":"page","parent":"index","hidden":true,"body":["Plans."]}"#;
    let p = project([SCHEMA, ("docs/src/index.json", PAGE), ("docs/src/secret.json", secret)]);
    for doc in ["secret", "secret.json", "docs/src/secret.json"] {
        let ops = json!([{"op": "add", "doc": doc, "comment": {"at": "/body/0", "text": "?", "author": "Eve"}}]);
        let (code, out) = apply_with(&p.path, &ops, &[]);
        assert!(code == 2 && out.contains("secret.json is a hidden document"), "{doc}: {code} {out}");
        assert_eq!(p.read("docs/src/secret.json"), secret);
    }
    let ops = json!([{"op": "add", "doc": "secret", "comment": {"id": "c-s", "at": "/body/0", "text": "?", "author": "Agent"}}]);
    let (code, out) = apply_with(&p.path, &ops, &["--hidden"]);
    assert_eq!(code, 0, "{out}");
    assert!(p.read("docs/src/secret.json").contains(r#""quote": "Plans.""#));
    let with = p.read("docs/src/secret.json");
    for ops in [json!([{"op": "rm", "id": "c-s"}]), json!([{"op": "edit", "id": "c-s", "text": "!"}])] {
        let (code, out) = apply_with(&p.path, &ops, &[]);
        assert!(code == 2 && out.contains("hidden document"), "{ops}: {code} {out}");
        assert_eq!(p.read("docs/src/secret.json"), with);
    }
    let (code, out, errs) = p.run(&["comments", "rm", "c-s"]);
    assert!(code == 2 && errs.contains("hidden document"), "{out} {errs}");
    let (code, out, errs) = p.run(&["comments", "rm", "c-s", "--hidden"]);
    assert!(code == 0, "{out} {errs}");
    assert_eq!(p.read("docs/src/secret.json"), secret);
}

/// rm and edit of an id that two comments share name neither: refused, nothing written.
#[test]
fn shared_ids_are_ambiguous() {
    let c = r#"{"id":"c-dup","at":"/body/0","quote":"Alpha text.","text":"?","author":"A","created":"2026-10-22T09:00:00Z"}"#;
    let a = format!(r#"{{"title":"Index","kind":"page","body":["Alpha text."],"$comments":[{c}]}}"#);
    let b = format!(r#"{{"title":"B","kind":"page","parent":"index","body":["Alpha text."],"$comments":[{c}]}}"#);
    let p = project([SCHEMA, ("docs/src/index.json", a.as_str()), ("docs/src/b.json", b.as_str())]);
    for ops in [json!([{"op": "rm", "id": "c-dup"}]), json!([{"op": "edit", "id": "c-dup", "text": "!"}])] {
        let (code, out) = apply_with(&p.path, &ops, &[]);
        assert!(code == 2 && out.contains("comment id \"c-dup\" is used by 2 comments (in b.json, index.json)"), "{ops}: {code} {out}");
        assert_eq!((p.read("docs/src/index.json"), p.read("docs/src/b.json")), (a.clone(), b.clone()));
    }
    // twice in one file is ambiguous too
    let twice = format!(r#"{{"title":"Index","kind":"page","body":["Alpha text."],"$comments":[{c},{c}]}}"#);
    let p = project([SCHEMA, ("docs/src/index.json", twice.as_str())]);
    let (code, out) = apply_with(&p.path, &json!([{"op": "rm", "id": "c-dup"}]), &[]);
    assert!(code == 2 && out.contains("is used by 2 comments (in index.json)"), "{code} {out}");
    assert_eq!(p.read("docs/src/index.json"), twice);
}

/// The HTML import obeys the same rule: a comment on a hidden part or a
/// hidden document is skipped and listed, the others are imported.
#[test]
fn import_skips_what_readers_do_not_see() {
    let page = r#"{"title":"Index","kind":"page","body":["Alpha text."],"notes":["INTERNAL"]}"#;
    let secret = r#"{"title":"Secret","kind":"page","parent":"index","hidden":true,"body":["Plans."]}"#;
    let p = project([SCHEMA, ("docs/src/index.json", page), ("docs/src/secret.json", secret)]);
    p.write(
        "export.json",
        json!({"docc": "comments", "version": 2, "comments": [
            {"id": "c-ok00000001", "doc": "index", "path": "/body/0", "quote": "forged", "text": "Fine", "author": "Ann", "created": "2026-10-20T10:00:00Z"},
            {"id": "c-no00000001", "doc": "index", "path": "/notes/0", "quote": "", "text": "Leak?", "author": "Eve", "created": "2026-10-20T10:00:00Z"},
            {"id": "c-no00000002", "doc": "secret", "path": "/body/0", "quote": "", "text": "Leak?", "author": "Eve", "created": "2026-10-20T10:00:00Z"}
        ]})
        .to_string(),
    );
    let (code, out, errs) = p.run(&["comments", "import", "export.json"]);
    assert!(
        code == 0
            && out == "imported 1 comments into 1 documents (2 not on a block a reader sees (hidden, or gone), not imported: c-no00000001, c-no00000002)\n",
        "{code} {out} {errs}"
    );
    assert_eq!(p.read("docs/src/secret.json"), secret);
    let src = p.read("docs/src/index.json");
    assert!(src.contains(r#""quote": "Alpha text.""#) && !src.contains("forged") && !src.contains("Leak?"), "{src}");
}
