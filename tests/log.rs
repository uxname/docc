//! docc log and docc diff: the history of the documents for a program that
//! shows it itself, read from Git (a bare mirror too), with block counts and
//! block pairs from the one alignment, in shapes docc schema log|diff describe.
mod common;

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use common::*;
use serde_json::{Value, json};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// run runs docc with --format json in dir; stdout must be one line of JSON
/// that matches docc schema <kind> when the exit code is 0.
fn run(dir: &Path, kind: &str, args: &[&str]) -> (i32, Value) {
    let mut a = vec![kind, "--format", "json"];
    a.extend(args);
    let (code, out, errs) = do_in(dir, &a);
    assert!(out.ends_with('\n'), "{out} {errs}");
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out} {errs}"));
    if code == 0 {
        assert!(out.matches('\n').count() == 1, "one line: {out}");
        check_schema(kind, &v);
    }
    (code, v)
}

fn check_schema(kind: &str, v: &Value) {
    static SCHEMAS: OnceLock<HashMap<&str, (Value, jsonschema::Validator)>> = OnceLock::new();
    let all = SCHEMAS.get_or_init(|| {
        ["log", "diff"]
            .into_iter()
            .map(|k| {
                let (_, out, _) = do_in(&repo(), &["schema", k]);
                let schema: Value = serde_json::from_str(&out).unwrap();
                let val = jsonschema::validator_for(&schema).expect("the schema compiles");
                (k, (schema, val))
            })
            .collect()
    });
    let (schema, val) = &all[kind];
    assert_eq!(v["$schema"], schema["$id"], "the output names its schema");
    let errs: Vec<String> = val.iter_errors(v).map(|e| format!("{} at {}", e, e.instance_path())).collect();
    assert!(errs.is_empty(), "output does not match docc schema {kind}:\n{}", errs.join("\n"));
}

/// Repo is a project in a Git repository with fixed authors and dates.
struct Repo {
    d: Dir,
}

impl Repo {
    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(&self.d.path)
            .envs([
                ("GIT_AUTHOR_NAME", "Ann"),
                ("GIT_AUTHOR_EMAIL", "ann@x.test"),
                ("GIT_COMMITTER_NAME", "Bot"),
                ("GIT_COMMITTER_EMAIL", "bot@x.test"),
                ("GIT_AUTHOR_DATE", "2026-10-01T10:00:00+03:00"),
                ("GIT_COMMITTER_DATE", "2026-10-01T10:00:00+03:00"),
            ])
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn commit(&self, msg: &str, files: &[(&str, &str)]) -> String {
        for (name, content) in files {
            if content.is_empty() {
                self.git(&["rm", "-q", name]);
            } else {
                self.d.write(name, content);
            }
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
        self.git(&["rev-parse", "HEAD"])
    }
}

/// history builds the project the tests read; the shas by subject. Each
/// step is described where it is made.
fn history() -> Option<(Repo, HashMap<&'static str, String>)> {
    let r = Repo { d: temp_dir() };
    if Command::new("git").arg("--version").output().is_err() {
        return None;
    }
    r.git(&["init", "-q"]);
    let mut sha = HashMap::new();
    let a = |owner: &str, body: &str, extra: &str| {
        format!(r#"{{"title":"A","kind":"page","body":[{body}],"notes":["{owner} note"],"owner":"{owner}","team":"Core"{extra}}}"#)
    };
    sha.insert(
        "first",
        r.commit(
            "first",
            &[
                ("docc.json", r#"{"roots":["a"]}"#),
                ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
                ("docs/src/a.json", &a("Ann", r#""One.","Two.","Three.""#, "")),
                ("docs/src/b.json", r#"{"title":"B","kind":"page","parent":"a","summary":"Bee summary."}"#),
                ("docs/src/c.json", r#"{"title":"C","kind":"page","parent":"a","body":["Owner: {{a#owner}}."]}"#),
                ("docs/src/pic.png", "png 1"),
                ("README.md", "readme 1"),
            ],
        ),
    );
    // a: a paragraph added, one edited, the owner (a property) changed, the
    // notes and title changed (not blocks); b renamed as it is; a hidden one
    r.git(&["mv", "docs/src/b.json", "docs/src/b2.json"]);
    std::fs::create_dir_all(r.d.join("docs/src/sub")).unwrap();
    r.git(&["mv", "docs/src/b2.json", "docs/src/sub/b.json"]);
    let a2 = a("Bob", r#""One.","New one.","Two edited.","Three.""#, "").replace(r#""title":"A""#, r#""title":"A2""#);
    sha.insert(
        "second",
        r.commit(
            "second\n\nThe body.\n",
            &[("docs/src/a.json", &a2), ("docs/src/h.json", r#"{"title":"H","kind":"page","hidden":true,"body":"Secret."}"#)],
        ),
    );
    // only the comments of a change (and its layout)
    let commented = a2.replace(r#""team":"Core""#, "\n  \"team\": \"Core\",\n  \"$comments\": [{\"id\": \"c1\", \"at\": \"/body/0\", \"quote\": \"One.\", \"text\": \"Hi\", \"author\": \"Ann\", \"created\": \"2026-10-01T10:00:00Z\"}]");
    sha.insert("comments", r.commit("comments", &[("docs/src/a.json", &commented)]));
    sha.insert("broken", r.commit("broken", &[("docs/src/a.json", "{")]));
    // a fixed against its last good version; b deleted
    let a5 = a("Bob", r#""One.","Two edited.""#, "").replace(r#""title":"A""#, r#""title":"A2""#);
    sha.insert("fix", r.commit("fix", &[("docs/src/a.json", &a5), ("docs/src/sub/b.json", "")]));
    sha.insert("pic", r.commit("pic", &[("docs/src/pic.png", "png 2")]));
    r.commit("outside", &[("README.md", "readme 2")]);
    // a branch merged into main: one change on the first-parent line
    r.git(&["checkout", "-q", "-b", "feat"]);
    let a6 = a("Bob", r#""One.","Two edited.","Four.""#, "").replace(r#""title":"A""#, r#""title":"A2""#);
    sha.insert("feat", r.commit("feat", &[("docs/src/a.json", &a6)]));
    r.git(&["checkout", "-q", "main"]);
    r.commit("main side", &[("README.md", "readme 3")]);
    r.git(&["merge", "-q", "--no-ff", "-m", "merge feat", "feat"]);
    sha.insert("merge", r.git(&["rev-parse", "HEAD"]));
    Some((r, sha))
}

fn file<'a>(c: &'a Value, path: &str) -> &'a Value {
    c["files"].as_array().unwrap().iter().find(|f| f["path"] == path).unwrap_or_else(|| panic!("no file {path} in {c}"))
}

fn commit<'a>(log: &'a Value, subject: &str) -> &'a Value {
    log["commits"].as_array().unwrap().iter().find(|c| c["subject"] == subject).unwrap_or_else(|| panic!("no {subject}"))
}

/// blocks is (add, ch, rm) of a log file.
fn blocks(f: &Value) -> (u64, u64, u64) {
    let n = |k: &str| f["blocks"][k].as_u64().unwrap();
    (n("add"), n("ch"), n("rm"))
}

/// The commits of the first-parent chain that touched src, newest first,
/// with full hashes, people, dates, messages and every changed file: counts
/// of page blocks, renames, deletions with their last title, comments-only
/// changes, broken versions, hidden documents and files that are not
/// documents. --since starts after a commit and gives the same commits; a
/// bare mirror gives the same log.
#[test]
fn log_lists_commits_files_and_blocks() {
    let Some((r, sha)) = history() else { return };
    let (code, log) = run(&r.d.path, "log", &[]);
    assert_eq!(code, 0, "{log}");
    let subjects: Vec<&str> = log["commits"].as_array().unwrap().iter().map(|c| s(c, "subject")).collect();
    assert_eq!(subjects, ["merge feat", "pic", "fix", "broken", "comments", "second", "first"], "README-only commits and the merged branch are not on the list");

    let c = commit(&log, "second");
    assert_eq!(s(c, "sha"), sha["second"]);
    assert_eq!(c["parents"], json!([sha["first"]]));
    assert_eq!((c["author"].clone(), c["committer"].clone()), (json!({"name":"Ann","email":"ann@x.test"}), json!({"name":"Bot","email":"bot@x.test"})));
    assert_eq!((s(c, "date"), s(c, "body")), ("2026-10-01T10:00:00+03:00", "The body."));
    let a = file(c, "a.json");
    assert_eq!((s(a, "status"), s(a, "id"), s(a, "title")), ("M", "a", "A2"));
    assert_eq!(blocks(a), (1, 2, 0), "a paragraph added, one edited, the owner property changed; notes and title are not blocks");
    let b = file(c, "sub/b.json");
    assert_eq!((s(b, "status"), s(b, "oldPath"), blocks(b)), ("R", "b.json", (0, 0, 0)));
    let h = file(c, "h.json");
    assert_eq!((s(h, "status"), h["hidden"].clone(), blocks(h)), ("A", Value::Bool(true), (1, 0, 0)));
    assert_eq!(commit(&log, "first")["parents"], json!([]));
    assert_eq!(blocks(file(commit(&log, "first"), "a.json")), (5, 0, 0), "a new document: every block, properties too");

    let c = commit(&log, "comments");
    assert!(c["commentsOnly"] == true && file(c, "a.json")["commentsOnly"] == true, "{c}");
    assert_eq!(blocks(file(c, "a.json")), (0, 0, 0));
    let c = commit(&log, "broken");
    let a = file(c, "a.json");
    assert!(a["broken"] == true && s(a, "title") == "A2" && blocks(a) == (0, 0, 0), "{a}");
    assert_eq!(c["commentsOnly"], false);
    let c = commit(&log, "fix");
    assert_eq!(blocks(file(c, "a.json")), (0, 0, 2), "against the last good version");
    let b = file(c, "sub/b.json");
    assert_eq!((s(b, "status"), s(b, "title"), blocks(b)), ("D", "B", (0, 0, 1)), "a deleted document keeps its title");
    let p = file(commit(&log, "pic"), "pic.png");
    assert!(p.get("id").is_none() && p.get("title").is_none() && blocks(p) == (0, 0, 0), "{p}");
    let c = commit(&log, "merge feat");
    assert_eq!(c["parents"].as_array().unwrap().len(), 2);
    assert_eq!(blocks(file(c, "a.json")), (1, 0, 0), "the merge against its first parent");

    // --since: the same commits after it, counted from the versions there
    for since in ["comments", "pic", "first"] {
        let (code, part) = run(&r.d.path, "log", &["--since", &sha[since]]);
        assert_eq!(code, 0, "{part}");
        let n = subjects.iter().position(|s| *s == since).unwrap();
        assert_eq!(part["commits"], json!(log["commits"].as_array().unwrap()[..n]), "--since {since}");
    }
    let (code, part) = run(&r.d.path, "log", &["--since", &sha["merge"]]);
    assert_eq!((code, part["commits"].clone()), (0, json!([])));
    let (code, v) = run(&r.d.path, "log", &["--since", &sha["feat"]]);
    assert_eq!(code, 2);
    assert!(s(&v["diagnostics"][0], "message").contains("not on the first-parent history"), "{v}");
    let (code, part) = run(&r.d.path, "log", &["--rev", &sha["fix"], "--since", &sha["second"]]);
    assert_eq!(code, 0);
    let got: Vec<&str> = part["commits"].as_array().unwrap().iter().map(|c| s(c, "subject")).collect();
    assert_eq!(got, ["fix", "broken", "comments"]);

    let bare = temp_dir();
    let mirror = bare.join("m.git");
    r.git(&["clone", "-q", "--bare", ".", mirror.to_str().unwrap()]);
    let (code, v) = run(&mirror, "log", &["--config", "docc.json"]);
    assert_eq!((code, &v), (0, &log), "a bare mirror");

    let (code, out, _) = r.d.run(&["log"]);
    assert!(code == 0 && out.contains("  M a.json +1 ~2 -0\n"), "{out}");
}

/// diff lines the blocks of two revisions up, each side compiled from its
/// own revision: the changed documents with their changed blocks; one
/// document with all of them; renames, the empty tree, a broken side, an
/// unchanged document; and the counts of docc log are those of diff.
#[test]
fn diff_lines_blocks_up() {
    let Some((r, sha)) = history() else { return };
    let dir = &r.d.path;
    let (code, v) = run(dir, "diff", &[&sha["first"], &sha["second"]]);
    assert_eq!(code, 0, "{v}");
    assert_eq!((s(&v, "base"), s(&v, "head")), (sha["first"].as_str(), sha["second"].as_str()));
    let mut paths: Vec<&str> = v["files"].as_array().unwrap().iter().map(|f| s(f, "path")).collect();
    paths.sort();
    assert_eq!(paths, ["a.json", "sub/b.json"], "the hidden document is left out");
    let a = file(&v, "a.json");
    assert_eq!((s(a, "status"), s(a, "id"), s(a, "title")), ("M", "a", "A2"));
    let ops: Vec<(&str, Value, Value)> = a["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (s(b, "op"), b["old"]["s"].clone(), b["new"]["s"].clone()))
        .collect();
    assert_eq!(
        ops,
        [("ch", json!("Ann"), json!("Bob")), ("add", Value::Null, json!("New one.")), ("ch", json!("Two."), json!("Two edited."))]
    );
    assert_eq!(a["blocks"][0]["new"]["k"], "Owner", "a property has its label");
    let b = file(&v, "sub/b.json");
    assert_eq!((s(b, "status"), s(b, "oldPath"), b["blocks"].clone()), ("R", "b.json", json!([])));

    // one document: every block, in the order of two columns
    let (_, v) = run(dir, "diff", &[&sha["first"], &sha["second"], "--path", "a.json"]);
    let a = &v["files"][0];
    let ops: Vec<(&str, &str, &str)> =
        a["blocks"].as_array().unwrap().iter().map(|b| (s(b, "op"), s(&b["old"], "p"), s(&b["new"], "p"))).collect();
    assert_eq!(
        ops,
        [
            ("ch", "/owner", "/owner"),
            ("same", "/team", "/team"),
            ("same", "/body/0", "/body/0"),
            ("add", "", "/body/1"),
            ("ch", "/body/1", "/body/2"),
            ("same", "/body/2", "/body/3"),
        ]
    );
    for old in ["b.json", "sub/b.json"] {
        let (_, v) = run(dir, "diff", &[&sha["first"], &sha["second"], "--path", old]);
        let b = &v["files"][0];
        assert_eq!((s(b, "path"), s(b, "status")), ("sub/b.json", "R"), "--path {old}");
        assert_eq!(b["sections"]["old"], json!([{"id":"summary","title":"Summary","level":2}]));
        assert_eq!(b["blocks"][0]["op"], "same");
    }
    // an unchanged document: its include reads each side's revision
    let (_, v) = run(dir, "diff", &[&sha["first"], &sha["second"], "--path", "c.json"]);
    let c = &v["files"][0];
    assert_eq!((s(c, "status"), c["blocks"][0]["op"].clone()), ("=", json!("same")));
    let (old, new) = (s(&c["blocks"][0]["old"], "h"), s(&c["blocks"][0]["new"], "h"));
    assert!(old.contains("Ann") && new.contains("Bob"), "{old} {new}");
    let (_, v) = run(dir, "diff", &[&sha["first"], &sha["second"], "--path", "nope.json"]);
    assert_eq!(v["files"], json!([]));

    // the empty tree before a root commit
    let (code, v) = run(dir, "diff", &[EMPTY_TREE, &sha["first"]]);
    assert_eq!((code, s(&v, "base")), (0, EMPTY_TREE));
    let a = file(&v, "a.json");
    assert!(s(a, "status") == "A" && a["blocks"].as_array().unwrap().iter().all(|b| b["op"] == "add" && b["old"].is_null()), "{a}");
    assert_eq!(a["sections"]["old"], json!([]));
    // a side that does not compile
    let (_, v) = run(dir, "diff", &[&sha["comments"], &sha["broken"]]);
    let a = file(&v, "a.json");
    assert!(a["broken"] == true && a["blocks"] == json!([]), "{a}");
    let (code, v) = run(dir, "diff", &["HEAD", "no-such-rev"]);
    assert!(code == 2 && v["diagnostics"][0]["code"] == "usage", "{v}");

    // the counts of docc log are the blocks docc diff lines up
    let (_, log) = run(dir, "log", &[]);
    let mut compared = 0;
    for c in log["commits"].as_array().unwrap() {
        let base = c["parents"].get(0).and_then(Value::as_str).unwrap_or(EMPTY_TREE);
        let (_, v) = run(dir, "diff", &[base, s(c, "sha")]);
        for f in c["files"].as_array().unwrap() {
            if f.get("id").is_none() || f["hidden"] == true || f["broken"] == true {
                continue;
            }
            let d = file(&v, s(f, "path"));
            if d["broken"] == true {
                continue;
            }
            let n = |op: &str| d["blocks"].as_array().unwrap().iter().filter(|b| b["op"] == op).count() as u64;
            assert_eq!(blocks(f), (n("add"), n("ch"), n("rm")), "{} {}", s(c, "subject"), s(f, "path"));
            compared += 1;
        }
    }
    assert_eq!(compared, 7, "every file of a commit whose both sides compile");
}
