mod common;

use common::*;

#[test]
fn block_codes_check() {
    // only text blocks carry codes: not the title, a table cell or a member of a list object
    let p = raw_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        (
            "docs/src/index.json",
            r#"{"title":"Index ^IND-9","kind":"page","key":"IND","body":["Release ^IND-1","x ^2 ^IND-2","{{index#^ind-1}} ^IND-3"],
			"rows":[{"k":"cell ^IND-7"}],"obj":{"k":"member ^IND-8"},"single":"See [[a]]. ^IND-6"}"#,
        ),
        (
            "docs/src/a.json",
            r#"{"title":"A","kind":"page","key":"AA","parent":"index","body":"[[index#^ind-1]] [[index#^IND-6]] [[index#^IND-7]] [[index#^IND-8]] [[index#^IND-9]] ^AA-1"}"#,
        ),
    ]);
    let (_, out, _) = p.run(&["validate"]);
    assert!(!out.contains("DOC216") && !out.contains("DOC211"), "{out}");
    let n = out.matches("DOC312").count();
    assert!(
        n == 3 && !out.contains(r#""^IND-1""#) && !out.contains(r#""^IND-6""#),
        "want DOC312 for the cell, the member and the title only (a code is found in any case):\n{out}"
    );
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
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","key":"IND","body":["New. ","Old anchor. ^limit","Copy. ^IND-1"]}"#),
        ("docs/src/a.json", r#"{"title":"A","kind":"page","key":"AA","parent":"index","body":["First. ^IND-1"]}"#),
    ]);
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.matches("DOC216").count() == 3 && out.contains("DOC212"), "{out}");
    assert!(out.contains("docs/src/a.json:1:") && out.contains("not of this document"), "the duplicate names the other place: {out}");
    // build gives the new block the next number of its key; the named anchor,
    // the foreign code and the copy stay errors: docc does not choose
    let (code, out, _) = p.run(&["build"]);
    assert!(code == 1 && out.matches("DOC216").count() == 2 && out.contains("DOC212"), "{out}");
    assert!(p.read("docs/src/index.json").contains(r#""New.  ^IND-2""#), "{}", p.read("docs/src/index.json"));
    // docc codes changes nothing more, ever
    let before = p.read("docs/src/index.json");
    let (code, out, _) = p.run(&["codes"]);
    assert!(code == 0 && out.contains("gave codes to 0 blocks") && p.read("docs/src/index.json") == before, "{out}");
    // the agent decides: the copy loses its code and gets the next number
    p.write("docs/src/a.json", r#"{"title":"A","kind":"page","key":"AA","parent":"index","body":["First."]}"#);
    p.run(&["codes"]);
    assert!(p.read("docs/src/a.json").contains(r#""First. ^AA-1""#), "{}", p.read("docs/src/a.json"));
}

#[test]
fn a_new_document_gets_a_key_and_numbers() {
    let p = raw_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", "{\n  \"title\": \"Index\",\n  \"kind\": \"page\",\n  \"key\": \"PAY\",\n  \"body\": [\"See [[open-questions]]. ^PAY-1\"]\n}\n"),
        ("docs/src/open-questions.json", "{\n  \"title\": \"Open questions\",\n  \"kind\": \"page\",\n  \"parent\": \"index\",\n  \"body\": [\"One.\", \"Two.\"]\n}\n"),
        ("docs/src/payments.json", "{\"title\": \"Payments\", \"kind\": \"page\", \"parent\": \"index\", \"body\": \"Three.\"}"),
    ]);
    let (code, out, _) = p.run(&["codes"]);
    assert!(code == 0 && out.contains("gave codes to 3 blocks"), "{out}");
    assert_eq!(
        p.read("docs/src/open-questions.json"),
        "{\n  \"title\": \"Open questions\",\n  \"kind\": \"page\",\n  \"key\": \"OQ\",\n  \"parent\": \"index\",\n  \"body\": [\"One. ^OQ-1\", \"Two. ^OQ-2\"]\n}\n"
    );
    // PAY is taken by the index: the next proposal
    assert_eq!(p.read("docs/src/payments.json"), "{\"title\": \"Payments\", \"kind\": \"page\", \"key\": \"PMN\", \"parent\": \"index\", \"body\": \"Three. ^PMN-1\"}");
    // a deleted last block frees its number: the next new block gets it again
    p.write("docs/src/open-questions.json", "{\"title\": \"Open questions\", \"kind\": \"page\", \"key\": \"OQ\", \"parent\": \"index\", \"body\": [\"One. ^OQ-1\", \"New.\"]}");
    p.run(&["codes"]);
    assert!(p.read("docs/src/open-questions.json").contains(r#""New. ^OQ-2""#), "{}", p.read("docs/src/open-questions.json"));
    let (code, out, _) = p.run(&["check"]);
    assert!(!out.contains("DOC216") && !out.contains("DOC212"), "{code} {out}");
}

#[test]
fn codes_of_the_old_format_are_migrated() {
    // the format of docc 0.5 to 0.7: four signs, unique in the project
    let p = raw_project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#),
        ("docs/src/index.json", r#"{"title":"Index","kind":"page","body":["Fresh [[rules#^k7f2]], stale [[rules#^2wu5@zzzz]]. ^m2pq"],
  "$comments": [{"id": "c1", "at": "^M2PQ", "quote": "Fresh", "text": "Why?", "author": "Ann", "created": "2026-10-22T09:00:00Z"}]}"#),
        ("docs/src/rules.json", r#"{"title":"Rules","kind":"page","parent":"index","body":["Ten a minute. ^k7f2", "Lock for an hour. ^2wu5"]}"#),
        ("code/x.ts", "// docc: rules#^K7F2\nconst x = 1 // <Req to=\"rules#^2wu5\">\n"),
        ("docc.json", r#"{"trace":{"scan":["code/**"]}}"#),
    ]);
    // the hashes of the fixture: what docc says they are now
    let (_, out, _) = p.run(&["validate"]);
    let after = |pat: &str| -> String {
        let i = out.find(pat).unwrap_or_else(|| panic!("{pat} in {out}"));
        out[i + pat.len()..][..4].to_string()
    };
    let (h, print) = (after("rules#^k7f2@"), after("outside its regions is ~"));
    p.write("docs/src/index.json", p.read("docs/src/index.json").replace("[[rules#^k7f2]]", &format!("[[rules#^k7f2@{h}]]")));
    p.write("code/x.ts", p.read("code/x.ts").replace("rules#^K7F2\n", &format!("rules#^K7F2@{h} ~{print}\n")));
    let (_, out, _) = p.run(&["validate"]);
    assert!(out.matches("DOC216").count() == 3 && out.matches("DOC209").count() == 1 && !out.contains("DOC402"), "before: {out}");

    let (code, out, errs) = p.run(&["migrate"]);
    assert_eq!(code, 0, "{out} {errs}");
    let (i, r, x) = (p.read("docs/src/index.json"), p.read("docs/src/rules.json"), p.read("code/x.ts"));
    assert!(r.contains(r#""key":"RUL""#) && r.contains("Ten a minute. ^RUL-1") && r.contains("Lock for an hour. ^RUL-2"), "{r}");
    assert!(i.contains("[[rules#^RUL-1@") && i.contains("[[rules#^RUL-2@zzzz]]") && i.contains(r#""at": "^IND-1""#), "{i}");
    assert!(x.contains("rules#^RUL-1@") && x.contains("<Req to=\"rules#^RUL-2\">"), "{x}");
    // what matched before matches after, the stale link stays stale
    let (_, out, _) = p.run(&["check"]);
    assert!(out.matches("DOC209").count() == 1 && out.contains("zzzz") && !out.contains("DOC402") && !out.contains("DOC216"), "after: {out}");
    let once = (i, r, x);
    p.run(&["migrate"]);
    assert_eq!(once, (p.read("docs/src/index.json"), p.read("docs/src/rules.json"), p.read("code/x.ts")), "migrate twice changes nothing");
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
    let c = a[a.find(" ^").unwrap() + 2..].split('"').next().unwrap().to_string();
    let x = p.read("code/x.ts");
    assert!(x.contains(&format!("<Req to=\"a#^{c}\">")), "{x}");
    // the lock held the link unconfirmed (its hash was 00000000): it stays to be checked
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.matches("DOC209").count() == 1 && !out.contains("DOC216"), "{out}");
    let once = (p.read("docs/src/index.json"), p.read("docs/src/a.json"), p.read("code/x.ts"));
    p.run(&["migrate"]);
    assert_eq!(once, (p.read("docs/src/index.json"), p.read("docs/src/a.json"), p.read("code/x.ts")), "migrate twice changes nothing");
}
