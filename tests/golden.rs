//! testdata/<CODE>/{red,green} against expected.json; DOCC_UPDATE=1 rewrites
//! the expected files.

mod common;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use common::*;
use serde::{Deserialize, Serialize};

fn update() -> bool {
    std::env::var_os("DOCC_UPDATE").is_some_and(|v| !v.is_empty())
}

/// GoldenDiag is a diagnostic of the JSON report with its fields in the
/// report's order, so that it marshals back like Go's json.RawMessage.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenDiag {
    code: String,
    rule: String,
    level: String,
    file: String,
    line: u64,
    column: u64,
    end_line: u64,
    end_column: u64,
    message: String,
    hint: String,
}

/// go_marshal_indent is json.MarshalIndent(v, "", "  ") with Go's HTML escaping.
fn go_marshal_indent(v: &impl Serialize) -> String {
    serde_json::to_string_pretty(v)
        .unwrap()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// golden runs every testdata/<CODE>/{red,green}[-variant] case: the project in the
/// directory is checked with `docc <args> --format json` (args from args.txt,
/// default "validate") and stdout is compared to expected.json.
/// A red case must report its code, a green case must not.
#[test]
fn golden() {
    let root = repo().join("testdata");
    let mut codes: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
    codes.sort_by_key(|e| e.file_name());
    let mut fails = Vec::new();
    let mut n = 0;
    for c in codes {
        if !c.file_type().unwrap().is_dir() {
            continue;
        }
        let name = c.file_name().to_string_lossy().into_owned();
        // red and green, and variants of them such as green-folders
        let mut cases: Vec<String> = std::fs::read_dir(c.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|k| ["red", "green"].contains(&k.split('-').next().unwrap()))
            .collect();
        cases.sort();
        for case in cases {
            let dir = root.join(&name).join(&case);
            n += 1;
            if let Err(e) = run_golden(&dir, &name, case.split('-').next().unwrap()) {
                fails.push(format!("{name}/{case}: {e}"));
            }
        }
    }
    assert!(n > 0, "no testdata");
    assert!(fails.is_empty(), "{}", fails.join("\n\n"));
}

fn run_golden(dir: &Path, code: &str, kind: &str) -> Result<(), String> {
    let mut args: Vec<String> = vec!["validate".into()];
    if let Ok(b) = std::fs::read_to_string(dir.join("args.txt")) {
        args = b.split_whitespace().map(String::from).collect();
    }
    args.extend(["--format", "json", "--max-diagnostics", "0"].map(String::from));
    // run in a copy outside the repository: a case without docc.json would
    // otherwise find the repository's own docc.json above it
    let work = temp_dir();
    copy_tree(dir, &work.path).map_err(|e| e.to_string())?;
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let (exit, envelope, errb) = work.run(&argv);
    if exit == 2 {
        return Err(format!("usage/config error: {errb}"));
    }
    // expected.json holds the diagnostics array of the report
    #[derive(Deserialize)]
    struct Rep {
        diagnostics: Vec<GoldenDiag>,
    }
    let rep: Rep = serde_json::from_str(&envelope).map_err(|e| e.to_string())?;
    let out = go_marshal_indent(&rep.diagnostics) + "\n";
    let exp = dir.join("expected.json");
    if update() {
        std::fs::write(&exp, &out).map_err(|e| e.to_string())?;
    }
    let want = std::fs::read_to_string(&exp).map_err(|e| format!("missing expected.json (run with DOCC_UPDATE=1): {e}"))?;
    if want != out {
        return Err(format!("output differs from expected.json\n--- got\n{out}\n--- want\n{want}"));
    }
    let mut found = false;
    let mut has_err = false;
    for d in &rep.diagnostics {
        if d.code == code || (code == "DOC800" && d.rule.starts_with("custom/")) {
            found = true;
        }
        if d.level == "error" {
            has_err = true;
        }
    }
    let mut errs = Vec::new();
    match kind {
        "red" if !found => errs.push(format!("red case does not report {code}")),
        "green" if found => errs.push(format!("green case reports {code}")),
        _ => {}
    }
    let want_exit = if has_err { 1 } else { 0 };
    if exit != want_exit {
        errs.push(format!("exit code {exit}, want {want_exit}"));
    }
    if errs.is_empty() { Ok(()) } else { Err(errs.join("; ")) }
}

/// render_golden builds testdata/render/<case> (normal and --full) and
/// compares the generated trees with expect/out and expect/out-full.
#[test]
fn render_golden() {
    let root = repo().join("testdata/render");
    let mut cases: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
    cases.sort_by_key(|e| e.file_name());
    let mut fails = Vec::new();
    for c in cases {
        if !c.file_type().unwrap().is_dir() {
            continue;
        }
        let src = c.path();
        // the HTML file keys its comments by the folder name, which is 001 in Go's t.TempDir()
        let tmp = temp_dir();
        let work = Dir { path: tmp.join("001"), auto: false };
        copy_tree(&src.join("docs"), &work.join("docs")).unwrap();
        if let Ok(b) = std::fs::read(src.join("docc.json")) {
            std::fs::write(work.join("docc.json"), b).unwrap();
        }
        // single-file builds: names next to the output directory
        let modes: [(&[&str], &str, &str, [&str; 2]); 2] = [
            (&["build", "--single", "--html"], "docs/out", "out", ["out.md", "out.html"]),
            (&["build", "--full", "--single", "--html"], "docs/out-full", "out-full", ["out-full.md", "out-full.html"]),
        ];
        for (args, out, expect, files) in modes {
            let (code, _, errb) = work.run(args);
            assert_ne!(code, 2, "{args:?}: {errb}");
            let expect_dir = src.join("expect").join(expect);
            if update() {
                let _ = std::fs::remove_dir_all(&expect_dir);
                copy_tree(&work.join(out), &expect_dir).unwrap();
            }
            compare_trees(&mut fails, &expect_dir, &work.join(out));
            for f in files {
                let got = std::fs::read_to_string(work.join("docs").join(f)).unwrap();
                let want = src.join("expect").join(f);
                if update() {
                    std::fs::write(&want, &got).unwrap();
                }
                let b = std::fs::read_to_string(&want).unwrap_or_default();
                // the zip inside the HTML is compressed by another library: compare what it holds
                if unzip_html(&b) != unzip_html(&got) {
                    fails.push(format!("{f} differs from {}:\n{got}", want.display()));
                }
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// unzip_html replaces the base64 zip in the data of an HTML file with the
/// names and contents of the files in it.
fn unzip_html(s: &str) -> String {
    const KEY: &str = "\"zip\":\"";
    let Some(a) = s.find(KEY).map(|i| i + KEY.len()) else {
        return s.to_string();
    };
    let b = a + s[a..].find('"').unwrap();
    let raw = base64::engine::general_purpose::STANDARD.decode(&s[a..b]).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(raw)).unwrap();
    let mut files = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut text = String::new();
        f.read_to_string(&mut text).unwrap();
        files.insert(f.name().unwrap().to_string(), text);
    }
    format!("{}{:?}{}", &s[..a], files, &s[b..])
}

fn read_tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, m: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                walk(base, &p, m);
            } else {
                let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                m.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut m = BTreeMap::new();
    walk(dir, dir, &mut m);
    m
}

fn compare_trees(fails: &mut Vec<String>, want_dir: &Path, got_dir: &Path) {
    let (want, got) = (read_tree(want_dir), read_tree(got_dir));
    for (name, w) in &want {
        match got.get(name) {
            None => fails.push(format!("{name}: not generated")),
            Some(g) if g != w => fails.push(format!(
                "{name} differs\n--- got\n{}\n--- want\n{}",
                String::from_utf8_lossy(g),
                String::from_utf8_lossy(w)
            )),
            _ => {}
        }
    }
    for name in got.keys() {
        if !want.contains_key(name) {
            fails.push(format!("{name}: unexpected file"));
        }
    }
}
