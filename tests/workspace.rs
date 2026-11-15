//! Value fields, "$decided", folders, icon colours and labels in docc compile,
//! and the object of docc workspace (docc schema workspace).
mod common;

use common::*;
use serde_json::Value;

fn json_of(p: &Dir, args: &[&str]) -> Value {
    let (code, out, errs) = p.run(args);
    assert!(code < 2, "{args:?}: {out} {errs}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out} {errs}"))
}

fn page<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["pages"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("no page {id}"))
}

const REPORT: &str = r#"{"type":"object","additionalProperties":true,"properties":{"verdict":{"title":"Итог проверки","enum":["готово","доработать","блокер"]},"findings":{"type":"array","items":{"x-docc-value":true}}}}"#;

fn sample() -> Dir {
    coded_project([
        ("docs/schemas/report.json", REPORT),
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","key":"IND","icon":{"color":"teal"}}"#),
        ("docs/src/q.json", r#"{"title":"Q","kind":"page","key":"QQ","parent":"index","hidden":true,"body":["The answer. ^QQ-1"]}"#),
        (
            "docs/src/r.json",
            r#"{"title":"R","kind":"report","key":"REP","parent":"index","verdict":"готово ^REP-7","findings":["open"],"body":["Checked. ^REP-1","Other."],"$decided":{"REP-1":{"by":"agent-delegated","source":"[[q@rp55]]","confirmed":null}}}"#,
        ),
    ])
}

#[test]
fn value_fields_decided_folders_icons_labels() {
    let p = sample();
    // build cuts the code of the value field and gives none to it, only to the new paragraph (REP-7 is free again)
    let (code, out, errs) = p.run(&["build"]);
    assert!(code == 0, "{out} {errs}");
    assert!(errs.contains("removed the block codes of 1 value fields"), "{errs}");
    let r = p.read("docs/src/r.json");
    assert!(r.contains(r#""verdict":"готово""#) && r.contains(r#""findings":["open"]"#) && r.contains("Other. ^REP-2"), "{r}");
    let (_, out, _) = p.run(&["codes", "--format", "json"]);
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), serde_json::json!({"given": 0, "stripped": 0}));

    let v = json_of(&p, &["compile", "--hidden", "--format", "json"]);
    assert_eq!(v["$schema"], "urn:docc:schema:compile:3");
    let rp = page(&v, "r");
    let props = rp["props"].as_array().unwrap();
    let verdict = props.iter().find(|x| x["k"] == "Verdict").expect("verdict is a property");
    assert!(verdict["label"] == "Итог проверки" && verdict.get("code").is_none(), "{verdict}");
    assert!(props.iter().any(|x| x["k"] == "Findings" && x["label"] == "Findings"), "{props:?}");
    let blocks = rp["blocks"].as_array().unwrap();
    assert!(blocks.iter().all(|b| !b["p"].as_str().unwrap().starts_with("/verdict")), "a value field is no block: {blocks:?}");
    assert_eq!(blocks[0]["decided"]["by"], "agent-delegated");
    assert!(blocks[1].get("decided").is_none());
    // index has children: a folder; its colour goes down the tree
    let ip = page(&v, "index");
    assert!(ip["folder"] == true && ip["icon"] == serde_json::json!({"color": "teal", "inherited": false}), "{ip}");
    assert_eq!(rp["icon"], serde_json::json!({"color": "teal", "inherited": true}));
    assert!(rp.get("folder").is_none());
}

#[test]
fn workspace_object() {
    let p = sample();
    p.write(
        "docc.json",
        r#"{"trace":{"scan":["src/**","tests/**"]},"workspace":{"run":"docs/.crew/run.json","roles":{"checks":"report","questions":["page","report"]},"areas":{"tests":["tests/**"]}}}"#,
    );
    p.write("docs/schemas/story.json", r#"{"type":"object","additionalProperties":true,"x-docc-covered":true}"#);
    p.write("docs/src/s.json", r#"{"title":"S","kind":"story","key":"ST","parent":"index","body":["Counts. ^ST-1","By hand. ^ST-2"],"docIgnore":[{"rule":"trace/uncovered-block","path":"/body/1","reason":"checked on a phone","by":"human"}]}"#);
    p.write("src/a.js", "// docc: s#^ST-1\n");
    p.write("tests/a.spec.js", "\n// docc: s#^ST-1\n");
    p.write("docs/.crew/run.json", r#"{"state":"running","text":"Reading"}"#);
    let (_, out, errs) = p.run(&["workspace", "--format", "json"]);
    assert!(out.ends_with('\n') && out.matches('\n').count() == 1, "one line: {out} {errs}");
    let v: Value = serde_json::from_str(&out).unwrap();
    let (_, schema, _) = do_in(&repo(), &["schema", "workspace"]);
    let schema: Value = serde_json::from_str(&schema).unwrap();
    assert_eq!(v["$schema"], schema["$id"]);
    let val = jsonschema::validator_for(&schema).unwrap();
    let errs: Vec<String> = val.iter_errors(&v).map(|e| format!("{e} at {}", e.instance_path())).collect();
    assert!(errs.is_empty(), "{}", errs.join("\n"));
    assert_eq!(v["compile"]["$schema"], "urn:docc:schema:compile:3");
    assert!(page(&v["compile"], "q")["hidden"] == true, "the hidden documents are in");
    assert_eq!(v["roles"], serde_json::json!({"checks": "report", "questions": ["page", "report"]}));
    assert_eq!(v["run"]["text"], "Reading");
    let cov = v["coverage"].as_array().unwrap();
    assert_eq!(cov.len(), 2, "{cov:?}");
    assert_eq!(cov[0]["text"], "Counts.");
    assert_eq!(
        cov[0]["areas"],
        serde_json::json!({"tests": [{"path": "tests/a.spec.js", "line": 2}], "code": [{"path": "src/a.js", "line": 1}]})
    );
    assert_eq!(cov[1]["manual"], serde_json::json!({"reason": "checked on a phone", "by": "human"}));
    assert_eq!(v["history"], serde_json::json!([]), "not a Git work tree");
}

fn git(dir: &std::path::Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// tree_hidden is the "hidden" of the tree entry of id (false when it is shown), None when it is not in the tree.
fn tree_hidden(es: &Value, id: &str) -> Option<bool> {
    for e in es.as_array().unwrap() {
        if e["id"] == id || e["path"] == id {
            return Some(e["hidden"] == true);
        }
        if let Some(h) = e.get("children").and_then(|c| tree_hidden(c, id)) {
            return Some(h);
        }
    }
    None
}

#[test]
fn spec_004_meetings_notes_folders_actors() {
    let p = coded_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/schemas/task.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/schemas/meeting.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","key":"IND","body":["Home. ^IND-1"]}"#),
        // a parent of hidden tasks only: no visible child, so not in the tree
        ("docs/src/tasks.json", r#"{"title":"Tasks","kind":"page","key":"TSK","parent":"index"}"#),
        ("docs/src/t1.json", r#"{"title":"T1","kind":"task","key":"TA","parent":"tasks","hidden":true,"body":["Do. ^TA-1"]}"#),
        ("docs/src/m1.json", r#"{"title":"Call 15.11","kind":"meeting","key":"MA","parent":"index","body":["Talked. ^MA-1"]}"#),
        ("docs/inbox/notes/2026-11-14-a.md", "no heading here\n"),
        ("docc.json", r#"{"workspace":{"roles":{"tasks":"task","meetings":"meeting"}}}"#),
    ]);
    // a newer note: listed first
    std::thread::sleep(std::time::Duration::from_millis(20));
    p.write("docs/inbox/notes/2026-11-15-b.md", "\n## Dark mode, later\n\ntext\n");
    assert!(git(&p.path, &["init", "-q"]) && git(&p.path, &["add", "-A"]) && git(&p.path, &["commit", "-qm", "start"]));
    p.write("docs/src/index.json", r#"{"title":"Index","kind":"page","key":"IND","body":["Home. ^IND-1","More. ^IND-2"]}"#);
    assert!(git(&p.path, &["commit", "-qam", "crew-spec: ТЗ"]));
    p.write("docs/src/index.json", r#"{"title":"Index","kind":"page","key":"IND","body":["Home. ^IND-1","More! ^IND-2"]}"#);
    assert!(git(&p.path, &["commit", "-qam", "gate: PRD", "-m", "Approved.", "-m", "Crew-Actor: human"]));

    let v = json_of(&p, &["workspace", "--format", "json"]);
    let (_, schema, _) = do_in(&repo(), &["schema", "workspace"]);
    let val = jsonschema::validator_for(&serde_json::from_str(&schema).unwrap()).unwrap();
    let errs: Vec<String> = val.iter_errors(&v).map(|e| format!("{e} at {}", e.instance_path())).collect();
    assert!(errs.is_empty(), "{}", errs.join("\n"));

    // notes newest first, title from the first heading or the file name
    let notes = v["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0]["title"], "Dark mode, later");
    assert_eq!(notes[1]["title"], "2026-11-14-a");
    assert_eq!(notes[1]["path"], "docs/inbox/notes/2026-11-14-a.md");
    assert_eq!(notes[1]["text"], "no heading here\n");

    // meetings and a folder of hidden documents are hidden in the tree; the pages stay
    let tree = &v["compile"]["tree"];
    assert_eq!(tree_hidden(tree, "m1"), Some(true));
    assert_eq!(tree_hidden(tree, "tasks"), Some(true));
    assert_eq!(tree_hidden(tree, "index"), Some(false));
    assert!(page(&v["compile"], "m1").get("hidden").is_none(), "only the tree hides a meeting");
    // the plain compile leaves the empty folder out of the tree
    let c = json_of(&p, &["compile", "--format", "json"]);
    assert_eq!(tree_hidden(&c["tree"], "tasks"), None, "{}", c["tree"]);
    assert_eq!(tree_hidden(&c["tree"], "m1"), Some(false), "the meetings role is the workspace's");
    assert!(c["pages"].as_array().unwrap().iter().any(|p| p["id"] == "tasks"));

    // who made each commit: the trailer, else the subject
    let h: Vec<(&str, &str)> =
        v["history"].as_array().unwrap().iter().map(|c| (c["subject"].as_str().unwrap(), c["actor"].as_str().unwrap())).collect();
    assert_eq!(h, [("gate: PRD", "human"), ("crew-spec: ТЗ", "agent"), ("start", "human")]);
    assert_eq!(v["history"][0]["trailers"], serde_json::json!({"Crew-Actor": "human"}));
    let (_, log, _) = p.run(&["log", "--format", "json"]);
    assert!(log.contains(r#""actor":"agent""#), "{log}");
}

#[test]
fn folder_of_hidden_documents_is_not_in_the_tree() {
    let p = coded_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","key":"IND","body":["Home. ^IND-1"]}"#),
        ("docs/src/tasks/t1.json", r#"{"title":"T1","kind":"page","key":"TA","hidden":true,"body":["Do. ^TA-1"]}"#),
        ("docs/src/spec/s1.json", r#"{"title":"S1","kind":"page","key":"SA","body":["Spec. ^SA-1"]}"#),
        ("docc.json", r#"{"tree":"folders"}"#),
    ]);
    let v = json_of(&p, &["compile", "--hidden", "--format", "json"]);
    assert_eq!(tree_hidden(&v["tree"], "tasks"), Some(true), "{}", v["tree"]);
    assert_eq!(tree_hidden(&v["tree"], "spec"), Some(false));
    let v = json_of(&p, &["compile", "--format", "json"]);
    assert_eq!(tree_hidden(&v["tree"], "tasks"), None, "{}", v["tree"]);
}
