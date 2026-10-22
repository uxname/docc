mod common;

use std::collections::BTreeMap;

use common::*;
use serde_json::Value;

#[test]
fn missing_src_is_usage_error() {
    let p = project(Vec::<(&str, &str)>::new());
    let (code, out, errs) = p.run(&["validate"]);
    assert!(code == 2 && out.is_empty() && errs.contains("docc: error:"), "{code} {out:?} {errs:?}");
}

#[test]
fn empty_src_gives_doc205() {
    let p = project([("docs/src/.keep", "")]);
    let (code, out, _) = p.run(&["validate", "--format", "json"]);
    assert!(code == 1 && out.contains(r#""DOC205""#), "{code} {out}");
}

#[test]
fn bad_config_is_exit2() {
    let p = project([
        ("docc.json", "{\n  \"rules\": { \"DOC101\": \"off\" }\n}"),
        ("docs/src/index.json", r#"{"title":"I"}"#),
    ]);
    let (code, out, errs) = p.run(&["validate"]);
    assert!(
        code == 2 && out.is_empty() && errs.starts_with("docc: error: docc.json:2:") && errs.contains("config error"),
        "{code} {out:?} {errs:?}"
    );
}

#[test]
fn clean_project() {
    let p = project([("docs/src/index.json", r#"{"title":"I"}"#)]);
    let (code, out, _) = p.run(&["validate"]);
    assert!(code == 0 && out == "checked 1 file: 0 errors, 0 warnings\n", "{code} {out:?}");
    let (code, out, _) = p.run(&["validate", "--format", "json"]);
    assert!(
        code == 0
            && out.contains(
                r#""summary": {
    "checked": 1,
    "errors": 0,
    "warnings": 0,
    "shown": 0,
    "truncated": false
  }"#
            )
            && out.contains(r#""diagnostics": []"#),
        "{code} {out:?}"
    );
}

#[test]
fn text_format_and_limit() {
    let mut fs = vec![("docs/src/index.json".to_string(), r#"{"title":"I"}"#)];
    for i in 0..5 {
        fs.push((format!("docs/src/o{i}.json"), r#"{"title":"O"}"#));
    }
    let p = project(fs);
    let (code, out, _) = p.run(&["validate", "--max-diagnostics", "2"]);
    assert_eq!(code, 1);
    let want = [
        r#"docs/src/o0.json:1:1: error graph/orphan (DOC203) document "o0" is not reachable from the roots [index]"#,
        "  hint: Link it from a reachable document ([[id]] in the text) or set its parent.",
        r#"docs/src/o1.json:1:1: error graph/orphan (DOC203) document "o1" is not reachable from the roots [index]"#,
        "  hint: Link it from a reachable document ([[id]] in the text) or set its parent.",
        "showing 2 of 5",
        "checked 6 files: 5 errors, 0 warnings",
        "Run docc explain <rule> for details",
    ];
    assert_eq!(out.trim(), want.join("\n"), "got:\n{out}");
    let (_, out, _) = p.run(&["validate", "--format", "json", "--max-diagnostics", "2"]);
    assert!(
        out.matches(r#""code""#).count() == 2
            && out.contains(r#""truncated": true"#)
            && out.contains(r#""shown": 2"#)
            && out.contains(r#""errors": 5"#),
        "json output is limited like text, and says so:\n{out}"
    );
}

#[test]
fn flags_after_arguments() {
    let p = project([("docs/src/index.json", r#"{"title":"I"}"#)]);
    let (code, _, errs) = p.run(&["validate", "--format"]);
    assert!(code == 2 && !errs.is_empty(), "missing flag value: {code} {errs:?}");
    assert_eq!(p.run(&["validate", "--bogus"]).0, 2, "unknown flag");
    assert_eq!(p.run(&["validate", "--fix"]).0, 2, "--fix is gone");
    assert_eq!(p.run(&["nope"]).0, 2, "unknown command");
    let (code, out, _) = p.run(&["--version"]);
    assert!(code == 0 && out.starts_with("docc "), "version");
}

#[test]
fn schema_command() {
    let p = project(Vec::<(&str, &str)>::new());
    let (code, out, _) = p.run(&["schema", "document"]);
    assert!(code == 0 && out.contains(r#""title": "docc document""#), "{code} {out:?}");
    let (code, out, _) = p.run(&["schema", "config"]);
    assert!(
        code == 0 && out.contains(r#""additionalProperties": false"#) && out.contains("text/forbidden-words"),
        "{code} {out:?}"
    );
    assert_eq!(p.run(&["schema"]).0, 2, "schema without argument must be a usage error");
    assert_eq!(p.run(&["schema", "other"]).0, 2, "unknown schema");
}

#[test]
fn broken_schema_files_are_exit2() {
    let cases: [(&str, Vec<(&str, &str)>); 4] = [
        ("invalid json", vec![("docs/schemas/a.json", "{")]),
        ("bad schema", vec![("docs/schemas/a.json", r#"{"type":"nonsense"}"#)]),
        ("remote ref", vec![("docs/schemas/a.json", r#"{"$ref":"https://example.com/s.json"}"#)]),
        (
            "outside ref",
            vec![("docs/schemas/a.json", r#"{"$ref":"../secret.json"}"#), ("docs/secret.json", r#"{"type":"string"}"#)],
        ),
    ];
    for (name, mut fs) in cases {
        fs.push(("docs/src/index.json", r#"{"title":"I"}"#));
        let p = project(fs);
        let (code, out, errs) = p.run(&["validate"]);
        assert!(code == 2 && out.is_empty() && errs.contains("docs/schemas/a.json"), "{name}: {code} {out:?} {errs:?}");
    }
}

#[test]
fn kind_schema_file_with_local_ref() {
    let p = project([
        ("docs/schemas/note.json", r#"{"properties":{"status":{"$ref":"common/status.json"}},"additionalProperties":true}"#),
        ("docs/schemas/common/status.json", r#"{"enum":["a","b"]}"#),
        ("docs/src/index.json", r#"{"title":"I","kind":"note","status":"c"}"#),
    ]);
    let (code, out, _) = p.run(&["validate", "--format", "json"]);
    assert!(code == 1 && out.contains("DOC106"), "{code} {out}");
}

#[test]
fn max_warnings() {
    let mut fs = BTreeMap::from([("docs/src/index.json", r#"{"title":"I","body":"TODO"}"#)]);
    let p = project(fs.clone());
    assert_eq!(p.run(&["validate"]).0, 0, "a warning alone is not a failure");
    assert_eq!(p.run(&["validate", "--max-warnings", "0"]).0, 1, "--max-warnings 0 must fail on one warning");
    assert_eq!(p.run(&["validate", "--max-warnings", "1"]).0, 0, "one warning is within the limit 1");
    fs.insert("docc.json", r#"{"maxWarnings":0}"#);
    let p = project(fs);
    assert_eq!(p.run(&["validate"]).0, 1, "maxWarnings from docc.json");
    assert_eq!(p.run(&["validate", "--max-warnings", "5"]).0, 0, "the flag must win over docc.json");
}

#[test]
fn build_cleans_out_and_is_idempotent() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","body":"See [[a]]."}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index"}"#),
        ("docs/out/stale.md", "<!-- GENERATED by docc, DO NOT EDIT -->\nold\n"),
        ("docs/out/old/deep/x.md", "<!-- GENERATED by docc, DO NOT EDIT -->\nold\n"),
        ("docs/out/.DS_Store", "junk"),
    ]);
    let (code, _, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{errs}");
    let first = p.read("docs/out/index.md") + &p.read("docs/out/_tree.md");
    for gone in ["docs/out/stale.md", "docs/out/old", "docs/out/.DS_Store"] {
        assert!(!p.exists(gone), "{gone} must be removed");
    }
    assert_eq!(p.run(&["check"]).0, 0, "check after build must be green");
    p.run(&["build"]);
    assert_eq!(first, p.read("docs/out/index.md") + &p.read("docs/out/_tree.md"), "two builds differ");
    // stale: change a source without building
    p.write("docs/src/a.json", r#"{"title":"A2","parent":"index","kind":"page"}"#);
    let (code, out, _) = p.run(&["check", "--format", "json"]);
    assert!(code == 1 && out.contains("DOC501"), "{code} {out}");
}

#[test]
fn build_stops_on_structural_errors() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","body":"See [[nope]]."}"#),
        ("docs/out/keep.md", "keep"),
    ]);
    let (code, out, _) = p.run(&["build"]);
    assert!(code == 1 && out.contains("DOC202"), "{code} {out}");
    assert_eq!(p.read("docs/out/keep.md"), "keep", "build must not touch out/ when sources have errors");
    assert_eq!(p.run(&["build", "--full"]).0, 1, "--full refuses the same errors");
    assert!(!p.exists("docs/out-full"), "out-full must not be created");
    // check does not compare out/ when sources are broken
    let (code, out, _) = p.run(&["check", "--format", "json"]);
    assert!(code == 1 && !out.contains("DOC501"), "check: {code} {out}");
}

#[test]
fn check_normalizes_crlf_and_bom() {
    let p = project([("docs/src/index.json", r#"{"title":"I"}"#)]);
    p.run(&["build"]);
    for f in ["docs/out/index.md", "docs/out/_tree.md"] {
        let b = p.read(f);
        p.write(f, format!("\u{feff}{}", b.replace('\n', "\r\n")));
    }
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "CRLF and BOM must compare equal: {out}");
    // and a CRLF source works too
    p.write("docs/src/index.json", "\u{feff}{\r\n  \"title\": \"I\",\r\n  \"kind\": \"page\"\r\n}\r\n");
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "{out}");
}

#[test]
fn full_build_leaves_out_alone() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I","notes":["see [[h]]"]}"#),
        ("docs/src/h.json", r#"{"title":"H","hidden":true,"notes":["secret"]}"#),
    ]);
    let (code, _, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{errs}");
    let before = p.read("docs/out/index.md") + &p.read("docs/out/_tree.md");
    assert!(!p.exists("docs/out/h.md"), "hidden documents get no page in out/");
    let (code, _, errs) = p.run(&["build", "--full"]);
    assert_eq!(code, 0, "{errs}");
    assert_eq!(before, p.read("docs/out/index.md") + &p.read("docs/out/_tree.md"), "--full must not change out/");
    let h = p.read("docs/out-full/h.md");
    assert!(h.contains("> Hidden document.") && h.contains("secret"), "full page of a hidden document:\n{h}");
    // check ignores out-full: a stray file there is not DOC501
    p.write("docs/out-full/stray.md", "<!-- GENERATED by docc, DO NOT EDIT -->\nx");
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "{out}");
    // and --full removes stray files in out-full
    p.run(&["build", "--full"]);
    assert!(!p.exists("docs/out-full/stray.md"), "--full must remove extra files in out-full");
}

#[test]
fn build_refuses_foreign_files_in_out() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I"}"#),
        ("docs/out/NOTES.txt", "mine"),
        ("docs/out/img/logo.png", "png"),
    ]);
    let (code, _, errs) = p.run(&["build"]);
    assert!(
        code == 2 && errs.contains("NOTES.txt") && errs.contains("img/logo.png"),
        "build must refuse and name the files: {code} {errs}"
    );
    assert_eq!(p.read("docs/out/NOTES.txt"), "mine", "nothing may be deleted");
    assert!(!p.exists("docs/out/index.md"), "nothing may be written");
    let (_, out, _) = p.run(&["check"]);
    assert!(out.contains("did not generate"), "check must say the files are foreign:\n{out}");
    std::fs::remove_file(p.join("docs/out/NOTES.txt")).unwrap();
    std::fs::remove_dir_all(p.join("docs/out/img")).unwrap();
    let (code, _, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{errs}");
}

#[test]
fn unsafe_out_dir_is_usage_error() {
    for out in [".", "docs", "docs/src", "docs/src/sub"] {
        let cfg = format!(r#"{{"out":"{out}"}}"#);
        let p = project([("docs/src/index.json", r#"{"title":"I"}"#), ("docc.json", cfg.as_str())]);
        if out == "." {
            // "." is rejected when the config is read
            assert_eq!(p.run(&["build"]).0, 2, "out={out:?}");
            continue;
        }
        let (code, _, errs) = p.run(&["build"]);
        assert!(code == 2 && errs.contains("output directory"), "out={out:?}: {code} {errs:?}");
        assert!(p.exists("docs/src/index.json"), "out={out:?} destroyed the sources");
    }
}

#[test]
fn check_rejects_fix() {
    let p = project([("docs/src/index.json", r#"{"title":"I"}"#)]);
    assert_eq!(p.run(&["check", "--fix"]).0, 2, "--fix with check is a usage error");
}

#[test]
fn init() {
    let p = temp_dir();
    let (code, out, errs) = p.run(&["init"]);
    assert!(code == 0 && out.contains("created docc.json"), "{code} {out} {errs}");
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "init output must pass check: {out}");
    p.write("docs/src/index.json", r#"{"title":"Mine","kind":"page"}"#);
    let (code, out, _) = p.run(&["init"]);
    assert!(code == 0 && out.contains("skipped docs/src/index.json"), "{code} {out}");
    assert!(p.read("docs/src/index.json").contains("Mine"), "init must not overwrite");
    assert!(p.read("docs/out/index.md").contains("# Mine"), "init always builds");
}

#[test]
fn init_config_blocks_suspect_links() {
    let p = temp_dir();
    let (code, out, _) = p.run(&["init"]);
    assert_eq!(code, 0, "{out}");
    p.write("docs/src/index.json", r#"{"title":"Index","kind":"page","body":"See [[a]]."}"#);
    p.write("docs/src/a.json", r#"{"title":"A","kind":"page","parent":"index","body":"Five."}"#);
    p.must_build();
    p.write("docs/src/a.json", r#"{"title":"A","kind":"page","parent":"index","body":"Ten."}"#);
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("DOC209"), "a stale link must fail check in a project made by init: {code}\n{out}");
}

#[cfg(unix)]
#[test]
fn init_hook() {
    use std::os::unix::fs::PermissionsExt;
    const INIT_HOOK: &str = "#!/bin/sh\n# docc: refuse a commit with invalid documentation\nexec docc check\n";
    let p = temp_dir();
    let (code, out, _) = p.run(&["init"]);
    assert!(code == 0 && !out.contains("pre-commit"), "no .git, no hook: {code} {out}");
    std::fs::create_dir_all(p.join(".git/hooks")).unwrap();
    let (code, out, _) = p.run(&["init"]);
    assert!(
        code == 0 && out.contains("created .git/hooks/pre-commit") && out.contains("4. git commit runs docc check"),
        "{code} {out}"
    );
    let st = std::fs::metadata(p.join(".git/hooks/pre-commit")).unwrap();
    assert!(st.permissions().mode() & 0o777 == 0o755 && p.read(".git/hooks/pre-commit") == INIT_HOOK, "hook: {st:?}");
    p.write(".git/hooks/pre-commit", "#!/bin/sh\nmine\n");
    let (code, out, _) = p.run(&["init"]);
    assert!(
        code == 0 && out.contains("skipped .git/hooks/pre-commit") && !out.contains("4. git"),
        "{code} {out}"
    );
    assert_eq!(p.read(".git/hooks/pre-commit"), "#!/bin/sh\nmine\n", "init must not overwrite a hook");
}

#[test]
fn line_breaks_in_values_stay_lf() {
    let p = project([("docs/src/index.json", r#"{"title":"I\r\nx","body":"a\r\nb\rc"}"#)]);
    let (code, _, errs) = p.run(&["build"]);
    assert_eq!(code, 0, "{errs}");
    for f in ["docs/out/index.md", "docs/out/_tree.md"] {
        assert!(!p.read(f).contains('\r'), "{f} contains a carriage return");
    }
    assert!(p.read("docs/out/index.md").contains("# I x\n"), "a line break in the title must become a space in the heading");
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "check right after build must pass: {out}");
}

#[test]
fn same_out_and_out_full_is_usage_error() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"I"}"#),
        ("docc.json", r#"{"out":"docs/out","outFull":"docs/out"}"#),
    ]);
    let (code, _, errs) = p.run(&["build", "--full"]);
    assert!(code == 2 && errs.contains("output directory"), "{code} {errs}");
}

#[test]
fn help_per_command() {
    let p = project(Vec::<(&str, &str)>::new());
    for cmd in ["init", "validate", "build", "check", "explain", "rules", "schema"] {
        for h in ["--help", "-h"] {
            let (code, out, errs) = p.run(&[cmd, h]);
            assert!(
                code == 0 && errs.is_empty() && out.starts_with(&format!("Usage: docc {cmd}")) && out.contains("Example"),
                "{cmd} {h}: {code} {out:?} {errs:?}"
            );
        }
    }
    let (_, out, _) = p.run(&["validate", "--help"]);
    assert!(out.contains("-max-warnings") && out.contains(r#"(default "text")"#), "flags with defaults missing:\n{out}");
    let (code, out, _) = p.run(&["--format", "json", "--help"]);
    assert!(code == 0 && out.starts_with("docc - "), "{code} {out:?}");
}

#[test]
fn global_flags_before_command() {
    let mut fs = vec![("docs/src/index.json".to_string(), r#"{"title":"I"}"#)];
    for i in 0..3 {
        fs.push((format!("docs/src/o{i}.json"), r#"{"title":"O"}"#));
    }
    let p = project(fs);
    let (code, out, _) = p.run(&["--format", "json", "--max-diagnostics", "1", "validate"]);
    assert!(code == 1 && out.contains(r#""truncated": true"#) && out.matches(r#""code""#).count() == 1, "{code} {out}");
    let (code, out, _) = p.run(&["--format", "json", "explain", "DOC301"]);
    assert!(code == 0 && out.contains(r#""group": "text""#) && !out.contains("fixable"), "explain: {code} {out}");
    let (code, out, _) = p.run(&["--format", "json", "rules"]);
    assert!(code == 0 && out.starts_with('['), "rules: {code} {out}");
    let (code, out, _) = p.run(&["--max-warnings", "3", "schema", "document"]);
    assert!(code == 0 && out.contains("docc document"), "schema: {code} {out}");
    let (code, _, errs) = p.run(&["--bogus", "validate"]);
    assert!(code == 2 && errs.contains("docc: error:"), "{code} {errs}");
    assert_eq!(p.run(&["--format", "json"]).0, 2, "flags without a command");
    let (code, _, errs) = p.run(&["validate", "--max-diagnostics", "-1"]);
    assert!(code == 2 && errs.contains("negative"), "{code} {errs}");
}

#[test]
fn errors_are_json_with_format_json() {
    let p = project([
        ("docc.json", "{\n  \"rules\": { \"DOC101\": \"off\" }\n}"),
        ("docs/src/index.json", r#"{"title":"I"}"#),
    ]);
    for args in [&["validate", "--format", "json"][..], &["--format", "json", "validate"][..]] {
        let (code, out, errs) = p.run(args);
        let r: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{args:?}: {e} {out}"));
        let ds = r["diagnostics"].as_array().unwrap();
        assert!(code == 2 && errs.is_empty() && r["summary"]["errors"] == 1 && ds.len() == 1, "{args:?}: {code} {out} {errs:?}");
        let e = &ds[0];
        assert!(
            e["code"] == "config"
                && e["file"] == "docc.json"
                && e["line"] == 2
                && e["column"] != 0
                && !s(e, "message").is_empty()
                && !s(e, "hint").is_empty(),
            "{args:?}: {e}"
        );
    }
    let (code, out, _) = p.run(&["validate", "--format", "json", "--bogus"]);
    assert!(code == 2 && out.contains(r#""code": "usage""#) && out.contains(r#""summary""#), "{code} {out}");
    // every problem of docc.json at once, in both forms
    let p = project([
        ("docc.json", "{\n  \"maxWarning\": 0,\n  \"lang\": \"fr\",\n  \"roots\": \"index\"\n}"),
        ("docs/src/index.json", r#"{"title":"I"}"#),
    ]);
    let (code, out, _) = p.run(&["validate", "--format", "json"]);
    let r: Value = serde_json::from_str(&out).unwrap();
    assert!(
        code == 2 && r["diagnostics"].as_array().unwrap().len() == 3 && r["summary"]["errors"] == 3,
        "want three config errors: {code} {out}"
    );
    let (code, _, errs) = p.run(&["validate"]);
    assert!(
        code == 2 && errs.matches("config error").count() == 3 && errs.starts_with("docc: error: docc.json:2:"),
        "text form: {code} {errs:?}"
    );
}

#[test]
fn too_many_warnings_explained() {
    let p = project([("docs/src/index.json", r#"{"title":"I","body":"TODO"}"#)]);
    let (code, out, errs) = p.run(&["validate", "--max-warnings", "0"]);
    assert!(
        code == 1
            && errs == "too many warnings: 1 > --max-warnings 0\n"
            && out.contains("checked 1 file: 0 errors, 1 warning\n"),
        "{code} {out:?} {errs:?}"
    );
    let (code, out, errs) = p.run(&["validate", "--max-warnings", "0", "--format", "json"]);
    assert!(
        code == 1 && errs.is_empty() && out.contains(r#""failure": "too many warnings: 1 > --max-warnings 0""#),
        "{code} {out:?} {errs:?}"
    );
}

#[test]
fn build_skipped_and_init_next() {
    let p = project([("docs/src/index.json", r#"{"title":"I","body":"See [[nope]]."}"#)]);
    let (code, out, _) = p.run(&["build"]);
    assert!(code == 1 && out.contains("build skipped: 1 errors, docs/out not changed\n"), "{code} {out}");
    let p = project(Vec::<(&str, &str)>::new());
    let (code, out, _) = p.run(&["init"]);
    assert!(
        code == 0
            && out.contains("Next:\n  1. edit docs/src/index.json")
            && out.contains("docc validate --format json")
            && out.contains("SKILL.md"),
        "{code} {out}"
    );
}

#[test]
fn config_found_in_parent_folder() {
    let p = project([
        ("docc.json", r#"{"src":"docs/src","out":"docs/out"}"#),
        ("docs/src/index.json", r#"{"title":"I","body":"See [[gone]]."}"#),
        ("docs/src/sub/x.txt", ""),
    ]);
    let (code, out, errs) = p.run_in("docs/src/sub", &["validate", "--format", "json"]);
    assert!(
        code == 1 && out.contains(r#""file": "docs/src/index.json""#),
        "a subfolder must find the root docc.json, paths from the root: {code} {out} {errs}"
    );
}

#[test]
fn config_error_in_parent_folder_names_the_file() {
    let p = project([("docc.json", r#"{"src": 1}"#)]);
    std::fs::create_dir_all(p.join("a")).unwrap();
    let (code, _, errs) = p.run_in("a", &["validate"]);
    assert!(code == 2 && errs.contains("../docc.json:"), "{code} {errs}");
}

#[test]
fn config_explicit_beats_parent() {
    let p = project([
        ("docc.json", r#"{"src":"nope"}"#),
        ("other/docc.json", r#"{"src":"docs/src"}"#),
        ("other/docs/src/index.json", r#"{"title":"I","kind":"page"}"#),
        ("other/docs/schemas/page.json", r#"{"additionalProperties":true}"#),
    ]);
    let (code, out, errs) = p.run_in("other", &["validate", "--config", "docc.json"]);
    assert_eq!(code, 0, "{out} {errs}");
}

#[test]
fn typos_get_suggestions() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","body":"See [[requirments]] and [[unrelated-thing]]."}"#),
        ("docs/src/requirements.json", r#"{"title":"R","kind":"page","parent":"index","parnet":"index","body":"x"}"#),
    ]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.contains("Did you mean `requirements`?"), "DOC202 must name the nearest id:\n{out}");
    assert!(
        !out.contains("Did you mean `requirements` or") && !out.contains("unrelated-thing\"\n          hint: Did you mean"),
        "a far id gets no suggestion:\n{out}"
    );
    assert!(out.contains("DOC110") && out.contains(r#"misspelled "parent""#), "want DOC110 for parnet:\n{out}");
}

#[test]
fn file_problems_get_specific_hints() {
    let big = format!(r#"{{"title":"B","body":"{}"}}"#, "x".repeat(5 << 20));
    let p = project([
        ("docs/src/index.json", br#"{"title":"I","body":"[[win]] [[big]]"}"#.to_vec()),
        ("docs/src/win.json", b"{\"title\":\"\xcf\xf0\xe8\xe2\xe5\xf2\"}".to_vec()),
        ("docs/src/big.json", big.into_bytes()),
    ]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(
        out.contains("save it again as UTF-8") && out.contains("split it into several documents"),
        "hints for encoding and size:\n{out}"
    );
}

#[test]
fn small_cli_polish() {
    let p = project([("docs/src/index.json", r#"{"title":"I","kind":"page"}"#)]);
    let (code, _, errs) = p.run(&["validate", "--max-warnings", "-2"]);
    assert!(code == 2 && errs.contains("--max-warnings"), "negative limit: {code} {errs:?}");
    let (code, out, _) = p.run(&["explain", "doc209"]);
    assert!(code == 0 && out.starts_with("graph/suspect-link (DOC209)"), "lower-case code: {code} {out:?}");
    let (code, out, _) = p.run(&["explain", "graph"]);
    assert!(code == 0 && out.contains("DOC209") && !out.contains("DOC301"), "a group lists its rules: {code} {out:?}");
    assert_eq!(p.run(&["explain", "nonsense"]).0, 2, "an unknown name is still an error");
    // a blank title is DOC103 only, not DOC106 as well
    let p = project([
        ("docs/src/index.json", r#"{"title":"  ","kind":"page"}"#),
        (
            "docs/schemas/page.json",
            r#"{"type":"object","required":["title"],"properties":{"title":{"type":"string","minLength":1}},"additionalProperties":true}"#,
        ),
    ]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.contains("DOC103") && !out.contains("DOC106"), "one diagnostic for a blank title:\n{out}");
}

#[test]
fn init_ignores_built_files() {
    let p = temp_dir();
    p.write(".gitignore", "node_modules\ndocs/out.md");
    let (code, out, _) = p.run(&["init"]);
    assert!(code == 0 && out.contains("updated .gitignore"), "{code} {out}");
    let got = p.read(".gitignore");
    assert_eq!(
        got,
        "node_modules\ndocs/out.md\ndocs/out.html\ndocs/out-full/\ndocs/out-full.md\ndocs/out-full.html\n",
        "existing lines stay, missing ones are added once"
    );
    let (_, out, _) = p.run(&["init"]);
    assert!(!out.contains(".gitignore"), "a second init changes nothing: {out}");
}

/// A .gitignore in another encoding (CP1251 comments from Windows) keeps its
/// bytes: init only appends.
#[test]
fn init_keeps_gitignore_bytes() {
    let p = temp_dir();
    p.write(".gitignore", b"a\nb\xff\xfe x\n");
    let (code, out, errs) = p.run(&["init"]);
    assert!(code == 0 && out.contains("updated .gitignore"), "{code} {out} {errs}");
    let got = std::fs::read(p.join(".gitignore")).unwrap();
    assert!(got.starts_with(b"a\nb\xff\xfe x\ndocs/out.md\n"), "{got:?}");
}

/// Help of the commands outside Go's TestHelpPerCommand list, and a flag
/// typed in the Russian layout is a usage error, not a crash.
#[test]
fn help_and_odd_flags_of_comments_and_update() {
    let p = project(Vec::<(&str, &str)>::new());
    for (args, want) in [
        (vec!["update", "--help"], "Usage: docc update [--check]"),
        (vec!["comments", "--help"], "Usage: docc comments [--all]"),
    ] {
        let (code, out, errs) = p.run(&args);
        assert!(code == 0 && errs.is_empty() && out.starts_with(want), "{args:?}: {code} {out:?} {errs:?}");
    }
    for (args, want) in [
        (vec!["validate", "--ащкьфе", "json"], "docc: error: flag provided but not defined: -ащкьфе\n"),
        (vec!["--é"], "docc: error: flag provided but not defined: -é\n"),
        (vec!["validate", "-é=1"], "docc: error: flag provided but not defined: -é\n"),
    ] {
        let (code, _, errs) = p.run(&args);
        assert!(code == 2 && errs == want, "{args:?}: {code} {errs:?}");
    }
}

#[test]
fn script_links_are_doc310() {
    let p = project([(
        "docs/src/index.json",
        r#"{"title":"I","kind":"page","body":"[x](javascript:alert(1)) [y](DATA:text/html,hi) [ok](https://example.com) `[z](javascript:no)`"}"#,
    )]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(
        out.matches("DOC310").count() == 2 && out.contains("javascript:alert") && !out.contains("javascript:no"),
        "two script links, none in code, none for https:\n{out}"
    );
}

/// build --data is gone: a program that draws the pages itself reads docc
/// compile, which writes nothing; a docs/out.json left from it is not touched.
#[test]
fn build_data_is_gone() {
    let p = project([("docs/src/index.json", r#"{"title":"I","body":["One."]}"#)]);
    let (code, _, errs) = p.run(&["build", "--data"]);
    assert!(code == 2 && errs.contains("-data"), "{code} {errs}");
    p.write("docs/out.json", r#"{"key":"docc:x"}"#);
    let (code, out, errs) = p.run(&["build"]);
    assert!(code == 0 && !out.contains("out.json") && errs.is_empty(), "{code} {out} {errs}");
    assert_eq!(p.read("docs/out.json"), r#"{"key":"docc:x"}"#);
}
