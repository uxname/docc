mod common;

use common::*;

#[test]
fn block_codes_check() {
    // only text blocks carry codes: not the title, a table cell or a member of a list object
    let p = raw_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        (
            "docs/src/index.json",
            r#"{"title":"Index ^k7f2","kind":"page","body":["Release ^m2pq","x ^2 ^q6pq","{{index#^m2pq}} ^r7pq"],
			"rows":[{"k":"cell ^n3pq"}],"obj":{"k":"member ^p4pq"},"single":"See [[a]]. ^s8pq"}"#,
        ),
        (
            "docs/src/a.json",
            r#"{"title":"A","kind":"page","parent":"index","body":"[[index#^M2PQ]] [[index#^s8pq]] [[index#^n3pq]] [[index#^p4pq]] [[index#^k7f2]] ^t9pq"}"#,
        ),
    ]);
    p.migrate();
    let (_, out, _) = p.run(&["validate"]);
    assert!(!out.contains("DOC216") && !out.contains("DOC211"), "{out}");
    let n = out.matches("DOC312").count();
    assert!(
        n == 3 && !out.contains(r#""^m2pq""#) && !out.contains(r#""^s8pq""#),
        "want DOC312 for the cell, the member and the title only (a code is found in any case):\n{out}"
    );
    assert!(p.read("docs/src/a.json").contains("[[index#^M2PQ@"), "migrate keeps the case of a code as written");
}

#[test]
fn block_code_include_cycle() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":["See [[a]].","Loop {{a#^b2b2}} ^a2a2"]}"#),
        ("docs/src/a.json", r#"{"title":"A","parent":"index","body":"Back {{index#^a2a2}} ^b2b2"}"#),
    ]);
    let (code, out, _) = p.run(&["validate"]);
    assert!(code != 0 && out.contains("include cycle"), "a cycle through block codes is missed: {code} {out}");
}

#[test]
fn missing_and_copied_codes() {
    let p = raw_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","body":["New. ","Old anchor. ^limit","Copy. ^k7f2"]}"#),
        ("docs/src/a.json", r#"{"title":"A","kind":"page","parent":"index","body":["First. ^k7f2"]}"#),
    ]);
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.matches("DOC216").count() == 2 && out.contains("DOC212"), "{out}");
    assert!(out.contains("docs/src/a.json:1:"), "the duplicate names the other place: {out}");
    // build gives a code to the new block only; the named anchor and the copy stay errors
    let (code, out, _) = p.run(&["build"]);
    assert!(code == 1 && out.matches("DOC216").count() == 1 && out.contains("DOC212"), "{out}");
    assert!(p.read("docs/src/index.json").contains(r#""New.  ^"#), "{}", p.read("docs/src/index.json"));
    // docc codes changes nothing more, ever
    let before = p.read("docs/src/index.json");
    let (code, out, _) = p.run(&["codes"]);
    assert!(code == 0 && out.contains("gave codes to 0 blocks") && p.read("docs/src/index.json") == before, "{out}");
}

#[test]
fn old_projects_are_refused_until_migrated() {
    let p = raw_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","body":["See [[a#^limit]]."]}"#),
        ("docs/src/a.json", r#"{"title":"A","kind":"page","parent":"index","body":["Ten a minute. ^limit"]}"#),
        ("docc.lock", "index a#^limit 00000000\n"),
        ("code/x.ts", "// docc: a#^limit\nconst x = 1\n<Req to=\"a#^limit\">\n"),
        ("docc.json", r#"{"trace":{"scan":["code/**"]}}"#),
    ]);
    for cmd in ["validate", "build", "check", "codes", "compile"] {
        let (code, out, errs) = p.run(&[cmd]);
        assert!(code == 2 && (errs.clone() + &out).contains("run docc migrate"), "{cmd}: {code} {out} {errs}");
    }
    let (code, out, errs) = p.run(&["migrate"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(!p.exists("docc.lock"), "the lock goes");
    let a = p.read("docs/src/a.json");
    let c = &a[a.find(" ^").unwrap() + 2..][..4];
    let x = p.read("code/x.ts");
    assert!(x.contains(&format!("<Req to=\"a#^{c}\">")), "{x}");
    // the lock held the link unconfirmed (its hash was 00000000): it stays to be checked
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.matches("DOC209").count() == 1 && !out.contains("DOC216"), "{out}");
    let once = (p.read("docs/src/index.json"), p.read("docs/src/a.json"), p.read("code/x.ts"));
    p.run(&["migrate"]);
    assert_eq!(once, (p.read("docs/src/index.json"), p.read("docs/src/a.json"), p.read("code/x.ts")), "migrate twice changes nothing");
}
