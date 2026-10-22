mod common;

use std::collections::BTreeMap;

use base64::Engine;
use common::*;

#[test]
fn lang_ru() {
    let p = project([
        ("docc.json", r#"{"lang":"ru"}"#),
        ("docs/src/index.json", r#"{"title":"Главная","body":"См. [[child]]."}"#),
        ("docs/src/child.json", r#"{"title":"Раздел","parent":"index","priority":1}"#),
        ("docs/src/loose.json", r#"{"title":"Ничей","hidden":true}"#),
    ]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let (child, index, tree) = (p.read("docs/out/child.md"), p.read("docs/out/index.md"), p.read("docs/out/_tree.md"));
    for want in ["page · приоритет 1", "## Родитель\n\n[Главная](index.md)", "## Ссылаются"] {
        assert!(child.contains(want), "child.md lacks {want:?}:\n{child}");
    }
    assert!(
        index.contains("Дерево документов: [_tree.md](_tree.md)") && tree.contains("# Дерево документов"),
        "tree label:\n{index}\n{tree}"
    );
    let html = p.read("docs/out.html");
    for want in [
        ">Страницы раздела<",
        "<small>Далее</small>",
        "<small>Назад</small>",
        r#"<html lang="ru">"#,
        r#"placeholder="Поиск  ( / )""#,
        r#"aria-label="Шкала времени""#,
        "'Копировать'",
        "'Ничего не найдено'",
        " ещё ↓",
        "🎤 Надиктовать",
        "Скопировать комментарии",
        "Правки с:",
    ] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
    for bad in ["Pages in this section", "'Copy'", "'On this page'", r#"aria-label="On this page""#, ">Parent<", "{{t:"] {
        assert!(!html.contains(bad), "out.html keeps {bad:?}");
    }
    let (code, out, _) = p.run(&["build", "--full"]);
    assert!(
        code == 0
            && p.read("docs/out-full/loose.md").contains("> Скрытый документ.")
            && p.read("docs/out-full/_tree.md").contains("## Без родителя"),
        "full build labels: {code} {out}"
    );
    p.write("docc.json", r#"{"lang":"de"}"#);
    let (code, _, errs) = p.run(&["check"]);
    assert!(code == 2 && errs.contains(r#"lang must be "en" or "ru""#), "{code} {errs}");
}

#[test]
fn images() {
    let png: &[u8] = b"\x89PNG\r\n\x1a\n\x00 bytes";
    let p = project([
        ("docs/src/img/a.png", png.to_vec()),
        ("docs/src/img/unused.png", b"x".to_vec()),
        (
            "docs/src/index.json",
            br#"{"title":"Index","body":["Own ![A](img/a.png) and code `![C](nope.png)`.","{{p#body}}"]}"#.to_vec(),
        ),
        ("docs/src/sub/p.json", br#"{"title":"P","parent":"index","body":"From sub ![B](../img/a.png \"t\")."}"#.to_vec()),
    ]);
    let (code, out, errs) = p.run(&["build", "--single", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let got = std::fs::read(p.join("docs/out/img/a.png")).unwrap();
    assert_eq!(got, png, "copy differs");
    assert!(!p.exists("docs/out/img/unused.png"), "an image no page shows is copied");
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let html_want = format!(r#"src="data:image/png;base64,{b64}""#);
    let checks: BTreeMap<&str, Vec<&str>> = BTreeMap::from([
        // the include is rebased
        ("docs/out/index.md", vec!["Own ![A](img/a.png)", "`![C](nope.png)`", "From sub ![B](img/a.png \"t\")"]),
        ("docs/out/sub/p.md", vec!["![B](../img/a.png \"t\")"]),
        ("docs/out.md", vec!["![A](out/img/a.png)", "![B](out/img/a.png \"t\")"]),
        ("docs/out.html", vec![html_want.as_str()]),
    ]);
    for (f, wants) in checks {
        let got = if f == "docs/out.html" { read_app(&p, f).blocks } else { p.read(f) };
        for want in wants {
            assert!(got.contains(want), "{f} lacks {want:?}");
        }
    }
    let (code, out, _) = p.run(&["check"]);
    assert_eq!(code, 0, "fresh build must pass check: {out}");
    // a copy that differs only by line endings is stale: images compare byte for byte
    p.write("docs/out/img/a.png", b"\x89PNG\n\x1a\n\x00 bytes");
    let (code, out, _) = p.run(&["check"]);
    assert!(code == 1 && out.contains("docs/out/img/a.png") && out.contains("DOC501"), "{code} {out}");
}

#[test]
fn footnotes_in_html() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":["Claim[^1].","[^1]: Source one."]}"#),
        ("docs/src/p.json", r#"{"title":"P","parent":"index","body":["Other[^1].","[^1]: Source two."]}"#),
    ]);
    let (code, out, errs) = p.run(&["build", "--html"]);
    assert_eq!(code, 0, "{out} {errs}");
    let html = read_app(&p, "docs/out.html").blocks;
    for want in [r#"id="index--fn:1""#, r##"href="#index--fn:1""##, r#"id="p--fn:1""#, r##"href="#p--fnref:1""##, "Source two."] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
}

/// Custom rules and DOC313 see "й" typed as и + U+0306 like the composed
/// letter, and report the column of the source text.
#[test]
fn custom_rules_and_repeats_in_nfc() {
    let p = project([
        (
            "docc.json",
            r#"{"customRules":[
			{"id":"custom/no-new","level":"warn","given":"/body/*","forbid":"новый","message":"no new"},
			{"id":"custom/needs-key","level":"warn","given":"/key","require":"ключ","message":"needs key"}]}"#,
        ),
        ("docs/src/index.json", r#"{"title":"Index","key":"ключ","body":["Это новый текст.","Слово новый новый подряд."]}"#),
    ]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    let ds = diagnostics_of(&out);
    let mut at: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for x in ds.as_array().unwrap() {
        at.entry(s(x, "rule").to_string()).or_default().push(x["column"].as_u64().unwrap());
    }
    // the custom rule hits the same three places as the built-in DOC353 word list
    let (got, want) = (at.get("custom/no-new"), at.get("style/time-bound-words"));
    assert!(got == want && got.map_or(0, Vec::len) == 3, "custom/no-new at {got:?}, DOC353 at {want:?}");
    assert!(
        !at.contains_key("custom/needs-key") && at.get("text/repeated-word").map_or(0, Vec::len) == 1,
        "want one DOC313 and no custom/needs-key: {at:?}"
    );
}

#[test]
fn placeholders_and_nfc() {
    let p = project([("docs/src/index.json", r#"{"title":"Index","body":["—","Value: N/A.","Это новый текст."]}"#)]);
    let (_, out, _) = p.run(&["validate", "--format", "json"]);
    let d = serde_json::to_string_pretty(&diagnostics_of(&out)).unwrap();
    for want in [r#"placeholder \"—\""#, r#"placeholder \"N/A\""#, r#""DOC353""#] {
        assert!(d.contains(want), "lacks {want}:\n{d}");
    }
}

/// build --page writes one page: its Markdown page with the images it shows
/// (no other page, no _tree.md), and the one-file builds with that page alone.
/// The page is named by its path as docc compile gives it, with or without
/// .json, or by its id; an unknown or hidden one is an error.
#[test]
fn one_page() {
    let p = project([
        ("docs/src/index.json", r#"{"title":"Index","body":"See [[q50]]. ![a](img/a.png)"}"#),
        ("docs/src/questions/q50.json", r#"{"title":"Q50","parent":"index","body":["Why? ![b](../img/b.svg)"],"$comments":[{"id":"c-1","at":"/body/0","quote":"Why? ![b](../img/b.svg)","text":"Hm","author":"A","created":"2026-10-22T09:00:00Z"}]}"#),
        ("docs/src/other.json", r#"{"title":"Other","parent":"index","body":"Else.","$comments":[{"id":"c-2","at":"/body","quote":"Else.","text":"No","author":"B","created":"2026-10-22T09:00:00Z"}]}"#),
        ("docs/src/secret.json", r#"{"title":"Secret","parent":"index","hidden":true}"#),
        ("docs/src/img/a.png", "\u{89}PNG"),
        ("docs/src/img/b.svg", "<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
    ]);
    for name in ["questions/q50.json", "questions/q50", "q50"] {
        let (code, out, errs) = p.run(&["build", "--page", name, "--single", "--html"]);
        assert_eq!(code, 0, "{name}: {out} {errs}");
        let mut got = walk(&p.join("docs/out"));
        got.sort();
        assert_eq!(got, ["img/b.svg", "questions/q50.md"], "{name}: the page and its picture only");
        let single = p.read("docs/out.md");
        assert!(single.contains("# Q50") && !single.contains("# Index") && !single.contains("Document tree"), "{single}");
        let a = read_app(&p, "docs/out.html");
        let ids: Vec<&str> = a.pages().iter().map(|pg| s(pg, "id")).collect();
        assert_eq!(ids, ["q50"], "{name}");
        assert_eq!(a.data["nav"]["tree"], serde_json::json!([["q50", 0]]));
        assert!(a.pages()[0].get("crumbs").is_none_or(|c| c.as_array().is_some_and(|c| c.is_empty())), "no crumbs to pages the file lacks");
        let cs: Vec<&str> = a.data["comments"].as_array().unwrap().iter().map(|c| s(c, "id")).collect();
        assert_eq!(cs, ["c-1"], "the comments of that page");
        assert!(a.blocks.contains("data:image/svg+xml;base64,"), "the picture is in the file");
    }
    for (name, want) in [("nope", "--page \"nope\": no such page"), ("secret", "a hidden page needs --full"), ("q50.json", "no such page")] {
        let (code, out, errs) = p.run(&["build", "--page", name]);
        assert!(code == 2 && errs.contains(want), "{name}: {code} {out} {errs}");
    }
    let (code, out, errs) = p.run(&["build", "--page", "secret", "--full"]);
    assert_eq!(code, 0, "{out} {errs}");
    assert!(p.exists("docs/out-full/secret.md") && !p.exists("docs/out-full/index.md"));
    // a build without --page writes every page again
    p.must_build();
    assert!(p.exists("docs/out/index.md") && p.exists("docs/out/_tree.md") && p.exists("docs/out/img/a.png"));
}

/// walk lists the files below dir, relative, with '/'.
fn walk(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let path = e.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if path.is_dir() {
            out.extend(walk(&path).into_iter().map(|f| format!("{name}/{f}")));
        } else {
            out.push(name);
        }
    }
    out
}
