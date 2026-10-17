mod common;

use common::*;
use docc::rules;
use regex::Regex;

/// Every code has a red and a green golden case (the definition of done).
#[test]
fn every_rule_has_golden_cases() {
    let root = repo().join("testdata");
    let mut missing = Vec::new();
    for r in rules::all() {
        for kind in ["red", "green"] {
            if !root.join(r.code).join(kind).join("expected.json").exists() {
                missing.push(format!("{}: no {kind} case with expected.json", r.code));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

/// Positions inside strings with escapes and in multi-line values.
#[test]
fn positions_with_escapes_and_newlines() {
    let bs = "\\";
    // body: "line1" LF "line2 fast" with an escaped cyrillic letter before the word
    let doc = format!(r#"{{"title":"I","body":"Первая строка{bs}nвторая {bs}u0430 fast{bs}n{bs}ttail"}}"#);
    let p = project([("docs/src/index.json", doc.as_str())]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    let ds = diagnostics_of(&out);
    let ds = ds.as_array().unwrap_or_else(|| panic!("{out}"));
    assert!(ds.len() == 1 && ds[0]["code"] == "DOC301", "{out}");
    // source column of "fast": the file is one line; count runes before it
    let idx = doc.find("fast").unwrap();
    let want = doc[..idx].chars().count() as u64 + 1;
    let d = &ds[0];
    let at = |k: &str| d[k].as_u64().unwrap();
    assert!(
        at("line") == 1 && at("column") == want && at("endColumn") == want + 4 && at("endLine") == 1,
        "got {}:{}-{}:{}, want 1:{want}-1:{}",
        at("line"),
        at("column"),
        at("endLine"),
        at("endColumn"),
        want + 4
    );
    // a pretty-printed file: the diagnostic is on line 3
    let pretty = "{\n  \"title\": \"I\",\n  \"body\": \"x fast\"\n}\n";
    let p = project([("docs/src/index.json", pretty)]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    let ds = diagnostics_of(&out);
    assert!(ds[0]["line"] == 3 && ds[0]["column"] == 14, "{ds}");
    // a diagnostic that spans lines: a match across escaped newlines
    let multi = format!("{{\n  \"title\": \"I\",\n  \"body\": \"one two three{bs}nfour five six\"\n}}\n");
    let p = project([
        ("docs/src/index.json", multi.as_str()),
        (
            "docc.json",
            r#"{"customRules":[{"id":"custom/x","level":"warn","message":"m","given":"/body","forbid":"three\\nfour"}]}"#,
        ),
    ]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    let ds = diagnostics_of(&out);
    let ds = ds.as_array().unwrap_or_else(|| panic!("{out}"));
    assert!(ds.len() == 1 && ds[0]["line"] == 3 && ds[0]["endLine"] == 3, "{ds:?}");
    // real line breaks inside the JSON text are not allowed in strings; the value may span lines only via \n
    assert!(ds[0]["endColumn"].as_u64() > ds[0]["column"].as_u64(), "range must have a width: {}", ds[0]);
}

/// The skill must not mention rules or commands that do not exist.
#[test]
fn skill_matches_registry_and_commands() {
    let text = std::fs::read_to_string(repo().join("skills/docc/SKILL.md")).unwrap();
    let mut errs = Vec::new();
    for m in Regex::new(r"DOC[0-9]{3}").unwrap().find_iter(&text) {
        if rules::by_code(m.as_str()).is_none() {
            errs.push(format!("skill mentions unknown code {}", m.as_str()));
        }
    }
    for cmd in Regex::new("`docc ([a-z]+)").unwrap().captures_iter(&text) {
        match &cmd[1] {
            "validate" | "build" | "check" | "explain" | "rules" | "schema" | "init" | "ack" | "update" | "comments" => {}
            other => errs.push(format!("skill mentions unknown command docc {other}")),
        }
    }
    assert!(errs.is_empty(), "{}", errs.join("\n"));
}
