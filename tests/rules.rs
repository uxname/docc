mod common;

use std::collections::HashMap;

use common::*;
use serde_json::Value;

fn rows(out: &str) -> Vec<HashMap<String, String>> {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

#[test]
fn rules_list() {
    let p = raw_project(Vec::<(&str, &str)>::new());
    let (code, out, _) = p.run(&["rules", "--format", "json"]);
    assert_eq!(code, 0);
    let rows = rows(&out);
    // 39 codes minus the generic DOC800 row
    assert_eq!(rows.len(), 38, "{} rules", rows.len());
    let got: HashMap<&str, &HashMap<String, String>> = rows.iter().map(|r| (r["code"].as_str(), r)).collect();
    let r = got["DOC301"];
    assert!(
        r["level"] == "warn" && r["group"] == "text" && r["rule"] == "text/forbidden-words" && !r["description"].is_empty(),
        "DOC301: {r:?}"
    );
    assert!(
        got["DOC352"]["level"] == "warn"
            && got["DOC101"]["level"] == "error"
            && !got.contains_key("DOC302")
            && !out.contains("fixable"),
        "levels wrong, a retired rule or the fixable column is listed"
    );
    let (_, text, _) = p.run(&["rules"]);
    let lines: Vec<&str> = text.trim().split('\n').collect();
    assert!(lines[0].starts_with("CODE") && lines.len() == 39 && !lines[0].contains("FIXABLE"), "table:\n{text}");
    for ln in lines {
        assert_eq!(ln, ln.trim_end_matches(' '), "trailing spaces");
    }
}

#[test]
fn rules_effective_levels() {
    let p = project([(
        "docc.json",
        r#"{
		  "rules": { "DOC313": "off", "text/forbidden-words": "error" },
		  "customRules": [{ "id": "custom/x", "level": "warn", "message": "m", "given": "/a", "forbid": "q" }]
		}"#,
    )]);
    let level = |args: &[&str]| -> HashMap<String, String> {
        let mut a = vec!["rules", "--format", "json"];
        a.extend_from_slice(args);
        let (_, out, errs) = p.run(&a);
        let rows: Vec<HashMap<String, String>> =
            serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e} {errs}"));
        rows.into_iter().map(|mut r| (r.remove("rule").unwrap(), r.remove("level").unwrap())).collect()
    };
    let base = level(&[]);
    assert!(
        base["text/repeated-word"] == "off"
            && base["text/forbidden-words"] == "error"
            && base["text/placeholder"] == "warn"
            && base["custom/x"] == "warn",
        "config levels: {base:?}"
    );
    assert_eq!(p.run(&["rules", "--file", "docs/src/index.json"]).0, 2, "--file is gone");
}

#[test]
fn explain() {
    let p = project([(
        "docc.json",
        r#"{"customRules":[{"id":"custom/x","level":"warn","message":"No x","hint":"Use y","given":"/a","forbid":"x+"}]}"#,
    )]);
    let (code, out, _) = p.run(&["explain", "DOC301"]);
    assert!(
        code == 0
            && out.starts_with("DOC301 text/forbidden-words\nGroup: text\nLevel: warn\n")
            && out.contains("Violation:")
            && out.contains("Fixed:")
            && !out.contains("Automatic fix"),
        "{code}\n{out}"
    );
    let (_, by_name, _) = p.run(&["explain", "text/forbidden-words"]);
    assert_eq!(by_name, out, "explain by name differs from explain by code");
    let (code, out, _) = p.run(&["explain", "DOC214", "--format", "json"]);
    let e: Value = serde_json::from_str(&out).unwrap();
    assert!(code == 0 && e["code"] == "DOC214" && !out.contains("fix"), "{e}");
    let (code, out, _) = p.run(&["explain", "custom/x"]);
    assert!(code == 0 && out.contains("DOC800 custom/x") && out.contains("x+"), "custom: {code} {out}");
    for bad in [&["explain", "DOC999"][..], &["explain"], &["explain", "DOC301", "DOC302"], &["explain", "custom/none"]] {
        let (code, _, errs) = p.run(bad);
        assert!(code == 2 && !errs.is_empty(), "{bad:?}: {code} {errs:?}");
    }
}

#[test]
fn trace_needs_scan() {
    let p = project([("docs/src/index.json", r#"{"title":"I"}"#), ("tests/a_test.txt", "// docc: ghost\n")]);
    let (code, out, _) = p.run(&["validate"]);
    assert_eq!(code, 0, "without trace.scan DOC401 must not run: {out}");
    let p = project([
        ("docs/src/index.json", r#"{"title":"I"}"#),
        ("tests/a_test.txt", "// docc: ghost\n"),
        ("docc.json", r#"{"trace":{"scan":["tests/**"]}}"#),
    ]);
    let (code, out, _) = p.run(&["validate"]);
    assert!(code == 1 && out.contains("tests/a_test.txt:1:4: error DOC401"), "{code} {out}");
}

#[test]
fn explain_custom_require() {
    let p = project([(
        "docc.json",
        r#"{"customRules":[{"id":"custom/needs-y","level":"error","message":"Need y","given":"/a","require":"y+"}]}"#,
    )]);
    let (code, out, errs) = p.run(&["explain", "custom/needs-y"]);
    assert!(code == 0 && out.contains("does not match y+"), "{code} {out} {errs}");
}

#[test]
fn explain_shows_effective_level_and_configure() {
    let p = project([(
        "docc.json",
        r#"{"rules":{"DOC313":"error"},"customRules":[{"id":"custom/x","level":"warn","message":"No x","hint":"Use y","given":"/a","forbid":"x+"}]}"#,
    )]);
    let (_, out, _) = p.run(&["explain", "DOC313"]);
    assert!(
        out.contains("Level: error (default: warn)")
            && out.contains(r#""rules": { "DOC313": "off" }"#)
            && out.contains(r#""docIgnore""#),
        "{out}"
    );
    let (_, out, _) = p.run(&["explain", "DOC101"]);
    assert!(out.contains("cannot be disabled"), "{out}");
    let (_, out, _) = p.run(&["explain", "custom/x"]);
    assert!(out.contains("No x") && out.contains("x+") && out.contains("Use y"), "custom: {out}");
    let (_, out, _) = p.run(&["rules"]);
    assert!(
        out.find("custom/x") <= out.find("DOC901") && out.contains("DESCRIPTION"),
        "custom rules go where DOC800 is:\n{out}"
    );
}
