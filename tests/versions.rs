//! Version hashes in links and fingerprints of code (DOC209, DOC402): what
//! replaced docc.lock and docc ack.

mod common;

use std::process::Command;

use common::*;

fn git(dir: &std::path::Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// hash_in is the hash written after target in s: [[target@hash]].
fn hash_in(s: &str, target: &str) -> String {
    let at = s.find(&format!("{target}@")).unwrap_or_else(|| panic!("no {target}@ in {s}")) + target.len() + 1;
    s[at..at + 4].to_string()
}

/// A changed paragraph breaks every link to it until the hash is rewritten
/// by hand: the message shows the change (the old text found in Git by its
/// hash) and the new hash; nothing confirms links in bulk.
#[test]
fn a_changed_target_breaks_its_links() {
    let p = project([
        ("docc.json", r#"{"commitOut":false}"#),
        ("docs/src/index.json", r#"{"title":"Index","body":["The limit is in [[b#^k7f2]]."]}"#),
        ("docs/src/b.json", r#"{"title":"B","parent":"index","body":["Ten attempts per minute. ^k7f2"]}"#),
    ]);
    assert_eq!(p.run(&["check"]).0, 0, "migrate wrote the hashes");
    if !git(&p.path, &["init", "-q"]) || !git(&p.path, &["add", "-A"]) || !git(&p.path, &["commit", "-qm", "one"]) {
        return; // no git here
    }
    let index = p.read("docs/src/index.json");
    let old = hash_in(&index, "b#^k7f2");
    p.write("docs/src/b.json", p.read("docs/src/b.json").replace("Ten", "Five"));
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 1, "{out}");
    let (_, json, _) = p.run(&["check", "--format", "json"]);
    let d = &diagnostics_of(&json)[0];
    let new = s(d, "message").rsplit('@').next().unwrap().to_string();
    assert!(
        d["code"] == "DOC209" && s(d, "message").contains(&format!("[[b#^k7f2@{old}]]")) && new.len() == 4 && new != old,
        "{d}"
    );
    assert!(
        s(d, "hint").contains("- Ten attempts per minute.") && s(d, "hint").contains("+ Five attempts per minute."),
        "the change, the old text from Git: {d}"
    );
    for args in [&["ack", "--all"][..], &["ack", "b"]] {
        assert_eq!(p.run(args).0, 2, "no command confirms links: {args:?}");
    }
    p.write("docs/src/index.json", index.replace(&format!("@{old}"), &format!("@{new}")));
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "the hash rewritten by hand: {out}");
}

/// A hash leaves out block codes, the hashes of links and the readers'
/// comments: two documents that link each other never loop, a new hash in a
/// link breaks nothing, a comment changes no version.
#[test]
fn hashes_do_not_depend_on_hashes() {
    let p = project([
        ("docc.json", r#"{"commitOut":false}"#),
        ("docs/src/index.json", r#"{"title":"Index","body":["See [[b]]."]}"#),
        ("docs/src/b.json", r#"{"title":"B","parent":"index","body":["Back to [[index]]."]}"#),
    ]);
    assert_eq!(p.run(&["check"]).0, 0);
    let b = p.read("docs/src/b.json");
    let h = hash_in(&b, "index");
    let (_, v, _) = p.run(&["compile", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_str(&v).unwrap();
    let index = v["pages"].as_array().unwrap().iter().find(|pg| pg["id"] == "index").unwrap();
    assert_eq!(s(index, "hash"), h, "compile gives the hash links carry");
    // a comment is no edit
    let ops = r#"[{"op":"add","doc":"b","comment":{"at":"/body/0","text":"?","author":"A"}}]"#;
    let (code, _, errs) = do_stdin(&p.path, ops.as_bytes(), &["comments", "apply"]);
    assert_eq!(code, 0, "{errs}");
    assert_eq!(p.run(&["check"]).0, 0, "a comment changes no version");
    // another hash in a link of index does not change the version of index
    p.write("docs/src/index.json", p.read("docs/src/index.json").replace(&hash_in(&p.read("docs/src/index.json"), "b"), "2222"));
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.matches("DOC209").count() == 1 && out.contains("docs/src/index.json"), "only the wrong link: {out}");
    assert_eq!(hash_in(&p.read("docs/src/b.json"), "index"), h);
}

/// A code file carries the version hashes of its targets and fingerprints:
/// of the lines outside its regions on the plain tag, of a region on its
/// docc:begin. An edit inside a region breaks that region's, outside it the
/// file's; writing hashes and fingerprints changes neither.
#[test]
fn code_fingerprints() {
    let p = project([
        ("docc.json", r#"{"commitOut":false,"trace":{"scan":["code/**"]}}"#),
        ("docs/src/index.json", r#"{"title":"Index","body":["Five attempts. ^k7f2","Other. ^m2pq"]}"#),
        ("code/a.go", "// docc: index#^m2pq\npackage a\n\n// docc:begin index#^k7f2\nconst limit = 5\n// docc:end\n\nfunc f() {}\n"),
    ]);
    let src = p.read("code/a.go");
    assert!(src.starts_with("// docc: index#^m2pq@") && src.contains("// docc:begin index#^k7f2@"), "{src}");
    assert_eq!(p.run(&["check"]).0, 0, "{src}");
    // inside the region
    p.write("code/a.go", src.replace("limit = 5", "limit = 6"));
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.matches("DOC402").count() == 1 && out.contains("code/a.go:4:") && out.contains("index#^k7f2: Five attempts."), "{out}");
    // outside
    p.write("code/a.go", src.replace("func f() {}", "func g() {}"));
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.matches("DOC402").count() == 1 && out.contains("code/a.go:1:"), "{out}");
    let (_, json, _) = p.run(&["check", "--format", "json"]);
    let msg = s(&diagnostics_of(&json)[0], "message").to_string();
    let new = &msg[msg.find("is ~").unwrap() + 4..][..4];
    let first = src.lines().next().unwrap();
    let old = &first[first.len() - 4..];
    p.write("code/a.go", src.replace("func f() {}", "func g() {}").replacen(old, new, 1));
    assert_eq!(p.run(&["check"]).0, 0, "the fingerprint rewritten by hand");
    // a changed target breaks the tag like a link
    p.write("docs/src/index.json", p.read("docs/src/index.json").replace("Five", "Six"));
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("code/a.go:4:") && out.contains("DOC209") && !out.contains("DOC402"), "{out}");
}
