mod common;

use common::*;

fn read_lock(p: &Dir) -> String {
    p.read("docc.lock")
}

#[test]
fn lock_suspect_and_ack() {
    let p = project([
        (
            "docs/src/index.json",
            r#"{"title":"Index","body":"See [[a]] and [[a#^limit]] and [[a#spec/Rate limits]]. [[index]]","notes":["[[b]]"]}"#,
        ),
        (
            "docs/src/a.json",
            r#"{"title":"A","parent":"index","body":["Five per minute. ^limit","Other."],"spec":{"$sections":true,"Rate limits":"R"}}"#,
        ),
        ("docs/src/b.json", r#"{"title":"B","parent":"index","hidden":true,"body":"[[a]]"}"#),
    ]);
    p.must_build();
    let lock = read_lock(&p);
    for l in ["b a ", "index a ", "index a#^limit ", "index a#spec/Rate%20limits ", "index b "] {
        assert!(lock.contains(&format!("\n{l}")) || lock.starts_with(l), "no line {l:?}:\n{lock}");
    }
    assert!(!lock.contains("index index") && lock.matches('\n').count() == 5, "self link or wrong count:\n{lock}");
    p.write(
        "docs/src/a.json",
        r#"{"kind":"page","title":"A","parent":"index","order":2,"body":["Ten per minute. ^limit","Other."],"spec":{"$sections":true,"Rate limits":"R"}}"#,
    );
    let (_, out, _) = p.run(&["validate"]);
    assert!(
        out.matches("DOC209").count() == 2 && out.contains("the targets a, a#^limit changed") && out.contains("docs/src/b.json"),
        "want one DOC209 for a and a#^limit from index and one for a from b:\n{out}"
    );
    let (_, out, _) = p.run(&["check"]);
    assert!(!out.contains("DOC503") && !out.contains("DOC109"), "the lock is fresh:\n{out}");
    let (code, out, _) = p.run(&["ack", "a#^limit"]);
    assert!(code == 0 && out.contains("confirmed index -> a#^limit") && out.contains("confirmed 1 links"), "ack: {code} {out}");
    let (_, out, _) = p.run(&["validate"]);
    assert!(
        out.matches("DOC209").count() == 2 && !out.contains("a#^limit changed"),
        "ack of a#^limit must clear only that line:\n{out}"
    );
    let (code, out, _) = p.run(&["ack", "a"]);
    assert!(
        code == 0 && out.contains("confirmed 2 links") && out.contains("confirmed b -> a") && !out.contains("a#spec"),
        "ack a must list and confirm only the stale lines: {code} {out}"
    );
    let (code, out, _) = p.run(&["ack", "a"]);
    assert!(code == 0 && out.contains("confirmed 0 links"), "a second ack has nothing to confirm: {code} {out}");
    let (code, out, _) = p.run(&["ack", "a", "--format", "json"]);
    assert!(code == 0 && out.contains(r#""links": []"#), "ack json lists the links: {code} {out}");
    let (_, out, _) = p.run(&["validate"]);
    assert!(!out.contains("DOC209"), "ack a must confirm every link to a:\n{out}");
    let (code, _, errs) = p.run(&["ack", "nothing"]);
    assert!(code == 2 && errs.contains("nothing"), "ack of an unknown name: {code} {errs}");
    assert_eq!(p.run(&["ack"]).0, 2, "ack without arguments must fail");
}

#[test]
fn lock_title_typo_only_whole_doc() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":"[[a]] [[a#^limit]]"}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index","body":"Five. ^limit"}"#),
    ]);
    p.must_build();
    p.write("docs/src/a.json", r#"{"kind":"page","title":"Aa","parent":"index","body":"Five. ^limit"}"#);
    let (_, out, _) = p.run(&["validate"]);
    assert!(
        out.matches("DOC209").count() == 1 && out.contains("the target a changed"),
        "want DOC209 for [[a]] only:\n{out}"
    );
}

#[test]
fn lock_code() {
    let p = project([
        ("docc.json", r#"{"trace":{"scan":["code/**"]}}"#),
        ("docs/src/index.json", r#"{"title":"Index","body":"Five. ^limit"}"#),
        (
            "code/a.go",
            "package a\n// docc:begin index#^limit\nconst a = 5\n// docc:end\n\n// docc:begin index#^limit\nconst b = 6\n// docc:end\n// docc: index\n",
        ),
    ]);
    p.must_build();
    let lock = read_lock(&p);
    assert!(
        lock.matches('\n').count() == 2
            && lock.contains("code/a.go index#^limit ")
            && lock.split('\n').next().unwrap().split_whitespace().count() == 3,
        "want one line per target, code hash only for the region:\n{lock}"
    );
    p.write(
        "code/a.go",
        "package a\n\nfunc f() {}\n// docc:begin index#^limit\n   const a = 5\n// docc:end\n\n// docc:begin index#^limit\nconst b = 6\n// docc:end\n// docc: index\n",
    );
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 0 && !out.contains("DOC402"), "code around the region or indentation changed nothing: {code}\n{out}");
    p.write(
        "code/a.go",
        "package a\n// docc:begin index#^limit\nconst a = 5\n// docc:end\n\n// docc:begin index#^limit\nconst b = 7\n// docc:end\n// docc: index\n",
    );
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.contains("DOC402") && out.contains("code/a.go:2"), "want DOC402 at the first region tag:\n{out}");
    assert_eq!(p.run(&["ack", "code/a.go"]).0, 0);
    let (_, out, _) = p.run(&["validate"]);
    assert!(!out.contains("DOC402"), "ack did not confirm the region:\n{out}");
}

#[test]
fn lock_merge_problems() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":"[[a]] [[b]]"}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index"}"#),
        ("docs/src/b.json", r#"{"title":"B","parent":"index"}"#),
    ]);
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("DOC503") && out.contains("missing"), "no lock with links: {code}\n{out}");
    p.must_build();
    let full = read_lock(&p);
    let lines: Vec<&str> = full.split_inclusive('\n').collect();
    let l0 = lines[0];
    let stale = l0.replacen(&l0[l0.len() - 9..l0.len() - 1], "00000000", 1);
    p.write("docc.lock", &stale);
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("index to b is not in docc.lock"), "a lost line: {code}\n{out}");
    p.must_build();
    assert_eq!(read_lock(&p), stale.clone() + lines[1], "build must re-add the lost line and keep the stored hash");
    p.write("docc.lock", stale.clone() + lines[1] + lines[1]);
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("listed twice"), "a repeated key: {code}\n{out}");
    p.write("docc.lock", format!("<<<<<<< HEAD\n{full}"));
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("line 1 is not"), "a malformed line: {code}\n{out}");
    p.write("docc.lock", format!("{}{}{}", lines[0], lines[0], lines[1]));
    p.must_build();
    assert_eq!(read_lock(&p), full, "build must keep one line per key");
}

#[test]
fn lock_renamed_section() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":"[[a#spec/Old]]"}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index","spec":{"$sections":true,"Old":"x"}}"#),
    ]);
    p.must_build();
    p.write("docs/src/a.json", r#"{"kind":"page","title":"A","parent":"index","spec":{"$sections":true,"New":"x"}}"#);
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.contains("DOC312"), "a renamed section must break the link:\n{out}");
    p.write("docs/src/index.json", r#"{"kind":"page","title":"Index","body":"[[a#spec/New]]"}"#);
    p.must_build();
    let lock = read_lock(&p);
    assert!(lock == lock.replacen("Old", "", 1) && lock.starts_with("index a#spec/New "), "the old line must go:\n{lock}");
    assert_eq!(p.run(&["check"]).0, 0, "check after build");
}
