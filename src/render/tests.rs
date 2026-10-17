use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use regex::Regex;

use super::lang::RU;
use super::page::collator;
use super::*;
use crate::config;
use crate::engine;

fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "docc-render-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// copy_tree copies a project; skip names folders that are not copied.
fn copy_tree(src: &Path, dst: &Path, skip: &[&str]) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let name = e.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let ft = e.file_type().unwrap();
        if ft.is_dir() {
            copy_tree(&e.path(), &dst.join(&name), skip);
        } else if ft.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), dst.join(&name))
                .unwrap();
        } else {
            std::fs::copy(e.path(), dst.join(&name)).unwrap();
        }
    }
}

#[test]
fn single_file_refuses_sources_and_schemas() {
    let root = temp_dir();
    std::fs::create_dir_all(root.join("docs/src")).unwrap();
    let mut cfg = config::default(&root);
    (cfg.file, cfg.src, cfg.schemas) =
        ("docc.json".into(), "docs/src".into(), "docs/schemas".into());
    let p = engine::load(cfg).unwrap();
    for out in ["docs/src/out", "docs/schemas/out", "docs/src"] {
        if let Ok(f) = single_file(&p, out, ".md")
            && out != "docs/src"
        {
            panic!("SingleFile({out:?}) = {f:?}, want a refusal");
        }
    }
    if let Err(e) = single_file(&p, "docs/out", ".md") {
        panic!("docs/out must be allowed: {e}");
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Every label of the output has a Russian entry, and every entry is used: a
/// label renamed in the code but not in ru fails both ways.
#[test]
fn labels_translated() {
    let mut used = HashSet::new();
    for s in [
        include_str!("page.html"),
        include_str!("app.js"),
        include_str!("page.css"),
    ] {
        used.extend(labels(s));
    }
    // rustfmt may put the label on the line after the call
    let call = Regex::new(r#"(?:\.t|(?-u:\b)group|(?-u:\b)add)\([\t\n\f\r ]*"([^"]+)""#).unwrap();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render");
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_none_or(|x| x != "rs") || p.ends_with("tests.rs") {
            continue;
        }
        let src = std::fs::read_to_string(&p).unwrap();
        for m in call.captures_iter(&src) {
            used.insert(m[1].to_string());
        }
    }
    assert!(
        used.len() >= 30,
        "found only {} labels, the scan is broken",
        used.len()
    );
    let ru: HashSet<&str> = RU.iter().map(|(k, _)| *k).collect();
    let mut errs = Vec::new();
    for k in &used {
        if !ru.contains(k.as_str()) {
            errs.push(format!("label {k:?} has no Russian entry in ru"));
        }
    }
    for k in &ru {
        if !used.contains(*k) {
            errs.push(format!("ru entry {k:?} is not used by any label"));
        }
    }
    assert!(errs.is_empty(), "{}", errs.join("\n"));
}

/// heading upper-cases the first rune as Go's unicode.ToUpper (checked against
/// Go on every rune): the simple mapping only.
#[test]
fn heading_upper_case() {
    for (key, want) in [
        ("open_questions", "Open questions"),
        ("x-y_z", "X y z"),
        ("ёлка", "Ёлка"),
        ("ßa", "ßa"),
        ("ᾀx", "ᾈx"),
        ("ῳ", "ῼ"),
        ("ǆ", "Ǆ"),
        ("_a", " a"),
        ("", "\u{FFFD}"),
    ] {
        assert_eq!(super::render::heading(key), want, "{key}");
    }
}

/// The pure parts of app.js (word marks of an edit, fragments, removed
/// paragraphs, the edits of a page) are checked by app_test.js in Node;
/// without Node the test is skipped.
#[test]
fn app_logic() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render");
    let out = match Command::new("node")
        .arg("app_test.js")
        .arg("app.js")
        .current_dir(&dir)
        .output()
    {
        Ok(o) => o,
        Err(_) => {
            eprintln!("node is not installed");
            return;
        }
    };
    assert!(
        out.status.success(),
        "app_test.js: {}\n{}{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Titles sort as with Go's collate.New(language.Und, collate.IgnoreCase,
/// collate.Numeric), equal ones by the text: the expected lists are what Go
/// printed for these inputs, except the deviations of docs/rust-port/render.md
/// (marked "Go:"). On 354 000 pairs of random titles of Latin, Cyrillic,
/// digits 1-9 and punctuation Rust and Go agree on every one.
#[test]
fn collation_matches_go() {
    let cases: &[&[&str]] = &[
        &[
            "Item", "Item 0", "Item 001", "Item 01", "Item 1", "Item 2", "Item 2 b", "Item 2a",
            "Item 9", "item 9", "Item 10", "Item 100", "item10",
        ],
        &[
            "Е", "е", "Ё", "ё", "еж", "Ёж", "ёж", "ежи", "ёжик", "ей", "Елка", "елка", "ёлка",
            "ея", "ж",
        ],
        &["А", "а", "Аб", "аБ", "аб", "Б", "б", "Ба", "бА", "ба", "в"],
        &[
            "a", "a b", "a_b", "a-b", "a,b", "a.b", "a1", "A1B", "a01b", "a1b", "a2b", "a10b", "ab",
        ],
        &[
            "_x", "-x", "'x'", "\"x\"", "«x»", "(x)", "[x]", "@x", "*x", "#x", "1x", "x", "x ",
            "x y", "x_y", "x-y", "x–y", "x—y", "x:y", "x!", "x?", "x.y", "x/y", "x1", "xy",
        ],
        // Go: "½" first
        &[
            "1,5",
            "1.5",
            "123",
            "½",
            "a",
            "Ä",
            "ae",
            "Æ",
            "Apple",
            "apple",
            "e",
            "é",
            "f",
            "ss",
            "ß",
            "u",
            "Ü",
            "ü",
            "Zebra",
            "zebra",
            "Ω",
            "ω",
            "Аист",
            "Яблоко",
        ],
        // Go: "2²" after "１０" (read as 22)
        &[
            "2²", "03", "3", "٣", "3.0", "4", "9", "10", "１０", "Ⅳ", "v1.2.3", "v1.9", "v1.10",
            "v1.10.1", "x³",
        ],
        &["", "\ta", " ", "  a", "a", "a b", "a\u{a0}b"],
        &[
            "I",
            "i",
            "İ",
            "ı",
            "L",
            "Ł",
            "O",
            "Ő",
            "Ø",
            "Řeka",
            "глава 1",
            "Глава 1.9",
            "Глава 1.10",
            "Глава 2",
            "Глава 10",
            "Раздел",
            "раздел А",
            "раздел а",
            "Раздел Б",
            "Ресурсы",
        ],
        // Go: "0 x", "0-1", "0.5", "0a" after every number, "a0b" after "a1b", "Item 0a" after "Item 1"
        &[
            "0",
            "00",
            "0 x",
            "0-1",
            "0–1",
            "0.5",
            "0a",
            "1",
            "1.5",
            "1a",
            "02",
            "2",
            "07",
            "10",
            "090",
            "a0b",
            "a1b",
            "Item 0",
            "Item 0-1",
            "Item 0a",
            "Item 1",
            "Глава 0",
            "Глава 1",
        ],
        // Go: "0. Введение" after "10. Итоги", "Этап 0 — подготовка" last
        &[
            "0. Введение",
            "1. Цели",
            "2. План",
            "10. Итоги",
            "Этап 0 — подготовка",
            "Этап 1 — запуск",
            "Этап 2 — рост",
        ],
        // Go: "½ ставки" first, "x²" before "x2"
        &[
            "1 ставка",
            "2 ставки",
            "½ ставки",
            "x2",
            "x3",
            "x²",
            "Площадь 2 м²",
            "Площадь 3 м",
            "Площадь 10 м",
        ],
        // the titles of the docc and o2 documentation
        &[
            "Agent workflow",
            "Architecture",
            "Commands",
            "Configuration",
            "Decisions and assumptions",
            "docc",
            "Document model",
            "Goals and non-goals",
            "Performance",
            "Rendering",
            "Rules and diagnostics",
        ],
        &[
            "1. Цель проекта и задачи системы",
            "2. Формат решения и архитектура",
            "3. Ключевые бизнес-сущности",
            "4. Пользовательские роли",
            "5. Функциональные требования",
            "6. Нефункциональные требования",
            "7. Карточка компании",
            "8. Отчёты по компаниям",
            "9. Пользовательские сценарии",
            "9.1 Создание проекта",
            "9.2 Поступление запроса от клиента",
            "9.3 Ведение задачи",
            "9.4 Регулярное администрирование и автопродление",
            "9.5 Работа по депозиту клиента",
            "9.6 Работа с документами (клиент)",
            "9.7 Работа с документами (менеджер)",
            "9.8 Запрос клиента на действие менеджеру",
            "10. Дополнительные требования к сценарию клиентских запросов",
            "12. Верхнеуровневые макеты основных экранов",
            "13. Проект миграции данных",
            "14. Первый этап внедрения (MVP)",
            "15. Открытые вопросы",
            "16. Принятые решения",
            "16.1 Решение: у проекта есть статус и сводка по задачам",
            "16.2 Решение: пять статусов задачи, Kanban на макете упрощён",
            "16.3 Решение: диаграммы и макеты нормативны наравне с текстом",
            "16.4 Решение: разовый переезд из Zygos и Bitrix без синхронизации",
            "16.5 Решение: монолит вместо сервис-ориентированной архитектуры",
            "16.6 Решение: на Kanban нет завершённых и отменённых задач",
            "16.7 Решение: каждый счёт привязан ровно к одной задаче",
            "16.8 Решение: контакт привязан к компании и отделён от офицера",
            "17. Аудит ТЗ на CRM: проблемы и предложения",
            "ТЗ: CRM для Family Office",
        ],
    ];
    let c = collator();
    let mut fails = Vec::new();
    for want in cases {
        let mut got: Vec<&str> = want.iter().rev().copied().collect();
        got.sort_by(|a, b| c.compare(a, b).then_with(|| a.cmp(b)));
        if got != *want {
            fails.push(format!("got  {got:?}\nwant {want:?}"));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n\n"));
}

/// ORACLE (env DOCC_ORACLE) is an older docc build the Markdown output is compared with.
static ORACLE: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| std::env::var("DOCC_ORACLE").unwrap_or_default());

/// read_out reads every file below dir: path relative to dir ('/') to bytes.
fn read_out(dir: &Path) -> HashMap<String, Vec<u8>> {
    fn rec(dir: &Path, rel: &str, out: &mut HashMap<String, Vec<u8>>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() {
                name
            } else {
                format!("{rel}/{name}")
            };
            if e.file_type().unwrap().is_dir() {
                rec(&e.path(), &r, out);
            } else {
                out.insert(r, std::fs::read(e.path()).unwrap());
            }
        }
    }
    let mut out = HashMap::new();
    rec(dir, "", &mut out);
    out
}

/// first_diff describes where two files differ.
fn first_diff(name: &str, got: &[u8], want: &[u8]) -> String {
    let (g, w) = (String::from_utf8_lossy(got), String::from_utf8_lossy(want));
    for (i, (a, b)) in g.lines().zip(w.lines()).enumerate() {
        if a != b {
            return format!("{name}:{}:\n  rust {a:?}\n  go   {b:?}", i + 1);
        }
    }
    format!(
        "{name}: rust {} lines, go {} lines",
        g.lines().count(),
        w.lines().count()
    )
}

/// load_valid loads and validates the project in dir with the Rust port; the
/// error lists the errors that keep it from being built.
fn load_valid(dir: &Path) -> Result<engine::Project, String> {
    let cfg = match std::fs::read(dir.join("docc.json")) {
        Ok(data) => config::parse(&data, dir, "docc.json").map_err(|e| e.to_string())?,
        Err(_) => config::default(dir),
    };
    let mut p = engine::load(cfg).map_err(|e| e.to_string())?;
    let ds = p.validate();
    if engine::structural(&ds) {
        let errs: Vec<String> = ds
            .iter()
            .filter(|d| d.lvl == crate::diag::Level::Error)
            .map(|d| format!("{} {}: {}", d.code, d.file, d.message))
            .collect();
        return Err(errs.join("; "));
    }
    Ok(p)
}

/// compare_out compares the Rust build (files and single file) of the output
/// directory rel with the Go build of it below want_dir.
fn compare_out(
    want_dir: &Path,
    rel: &str,
    files: &HashMap<String, Vec<u8>>,
    single: &str,
) -> Vec<String> {
    let mut diffs = Vec::new();
    let Ok(want_single) = std::fs::read(want_dir.join(format!("{rel}.md"))) else {
        return vec![format!("{}: no {rel}.md", want_dir.display())];
    };
    let want = read_out(&want_dir.join(rel));
    let mut names: Vec<&String> = files
        .keys()
        .chain(want.keys())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    names.sort();
    for n in names {
        match (files.get(n), want.get(n)) {
            (Some(g), Some(w)) if g == w => {}
            (Some(g), Some(w)) => diffs.push(first_diff(&format!("{rel}/{n}"), g, w)),
            (Some(_), None) => diffs.push(format!("{rel}/{n}: only in rust")),
            (None, _) => diffs.push(format!("{rel}/{n}: only in go")),
        }
    }
    if single.as_bytes() != want_single {
        diffs.push(first_diff(
            &format!("{rel}.md"),
            single.as_bytes(),
            &want_single,
        ));
    }
    diffs
}

/// go_build runs the Go oracle in dir: build --single, plain or full.
fn go_build(dir: &Path, full: bool) -> std::process::Output {
    let mut args = vec!["build", "--single"];
    if full {
        args.push("--full");
    }
    Command::new(ORACLE.as_str())
        .args(&args)
        .current_dir(dir)
        .output()
        .unwrap()
}

/// compare_build loads the project in dir with the Rust port, then builds it
/// with the Go oracle (Markdown pages and single file, plain and full) and
/// returns the differences; Err when the Rust port does not build it.
fn compare_build(dir: &Path) -> Result<Vec<String>, String> {
    let p = load_valid(dir)?;
    let mut diffs = Vec::new();
    for full in [false, true] {
        let r = Renderer::new(&p, full);
        let (files, single) = (r.files(), r.single());
        let st = go_build(dir, full);
        let rel = if full { &p.cfg.out_full } else { &p.cfg.out };
        if !dir.join(format!("{rel}.md")).exists() {
            diffs.push(format!(
                "{}: go build (full {full}) wrote nothing: {}{}",
                dir.display(),
                String::from_utf8_lossy(&st.stdout),
                String::from_utf8_lossy(&st.stderr)
            ));
            continue;
        }
        diffs.extend(compare_out(dir, rel, &files, &single));
    }
    Ok(diffs)
}

/// doc501 lists the DOC501 diagnostics of a JSON report as "file: message".
fn doc501(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Array(a) => a.iter().for_each(|x| doc501(x, out)),
        serde_json::Value::Object(o) => {
            if o.get("code").and_then(|c| c.as_str()) == Some("DOC501") {
                out.push(format!(
                    "{}: {}",
                    o["file"].as_str().unwrap(),
                    o["message"].as_str().unwrap()
                ));
            }
            o.values().for_each(|x| doc501(x, out));
        }
        _ => {}
    }
}

/// fresh_matches_go spoils the built output of dir in every way CheckFresh
/// tells apart and compares its DOC501 diagnostics with `docc check` of Go;
/// then Write must refuse as Go's build does and, once the foreign files are
/// gone, leave exactly the files.
fn fresh_matches_go(dir: &Path) -> Vec<String> {
    let out = dir.join("docs/out");
    let w = |p: &str, c: &[u8]| std::fs::write(out.join(p), c).unwrap();
    w("x.txt", b"foreign");
    w("old.md", b"<!-- GENERATED by docc, DO NOT EDIT -->\nold");
    w(".DS_Store", b"junk");
    w("short.md", b"<!-- GEN");
    let page = std::fs::read(out.join("c.md")).unwrap();
    let mut crlf = b"\xEF\xBB\xBF".to_vec();
    crlf.extend(
        String::from_utf8(page)
            .unwrap()
            .replace('\n', "\r\n")
            .into_bytes(),
    );
    w("c.md", &crlf); // still fresh
    w("a.md", b"changed");
    std::fs::remove_file(out.join("b.md")).unwrap();
    std::fs::remove_file(out.join("sub/deep.md")).unwrap();
    let mut png = std::fs::read(out.join("img/p.png")).unwrap();
    png.retain(|&b| b != b'\r'); // an image compares byte for byte
    w("img/p.png", &png);
    std::fs::create_dir_all(dir.join("docs/src/img2")).unwrap();
    std::fs::write(dir.join("docs/src/img2/q.png"), b"q").unwrap();
    std::fs::create_dir_all(out.join("img2")).unwrap();
    w("img2/q.png", b"copy of a source image");

    let cfg = config::parse(
        &std::fs::read(dir.join("docc.json")).unwrap(),
        dir,
        "docc.json",
    )
    .unwrap();
    let mut p = engine::load(cfg).unwrap();
    p.validate();
    let files = Renderer::new(&p, false).files();
    check_fresh(&p, "docs/out", &files);
    let mut got: Vec<String> = p
        .sorted()
        .iter()
        .filter(|d| d.code == "DOC501")
        .map(|d| format!("{}: {}", d.file, d.message))
        .collect();
    let st = Command::new(ORACLE.as_str())
        .args(["check", "--format", "json"])
        .current_dir(dir)
        .output()
        .unwrap();
    let mut want = Vec::new();
    doc501(&serde_json::from_slice(&st.stdout).unwrap(), &mut want);
    got.sort();
    want.sort();
    if got.len() < 9 || got != want {
        return vec![format!("check: rust {got:#?}\ngo {want:#?}")];
    }
    // build refuses while foreign files are there, then removes what it generated before
    let src = dir.join("docs/src");
    let err = write(&out, "docs/out", &src, &files)
        .unwrap_err()
        .to_string();
    let st = Command::new(ORACLE.as_str())
        .arg("build")
        .current_dir(dir)
        .output()
        .unwrap();
    if !String::from_utf8_lossy(&st.stderr).contains(&err) {
        return vec![format!(
            "write: rust {err:?}\ngo {}",
            String::from_utf8_lossy(&st.stderr)
        )];
    }
    std::fs::remove_file(out.join("x.txt")).unwrap();
    std::fs::remove_file(out.join("short.md")).unwrap();
    write(&out, "docs/out", &src, &files).unwrap();
    let mut got: Vec<_> = read_out(&out).into_iter().collect();
    let mut want: Vec<_> = files.into_iter().collect();
    got.sort();
    want.sort();
    if got != want || out.join("img2").exists() {
        return vec!["write: the output directory does not hold exactly the files".into()];
    }
    Vec::new()
}

/// golden_projects are the testdata cases and the eval projects of the skill.
fn golden_projects() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".");
    let mut out = Vec::new();
    for base in ["testdata", "skills/docc/evals"] {
        let mut ds: Vec<PathBuf> = std::fs::read_dir(root.join(base))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        ds.sort();
        for d in ds {
            if base == "testdata" {
                out.extend(
                    ["red", "green"]
                        .map(|k| d.join(k))
                        .into_iter()
                        .filter(|p| p.is_dir()),
                );
            } else if d.is_dir() {
                out.push(d);
            }
        }
    }
    // DOCC_GOLDEN_DIRS adds other projects (o2, this repository), ':'-separated
    if let Ok(extra) = std::env::var("DOCC_GOLDEN_DIRS") {
        out.extend(
            extra
                .split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }
    out
}

/// RICH is a project that uses every feature of the pages: numbering, sections,
/// tables, lists, includes, block anchors, images, hidden documents, notes,
/// docIgnore, code coverage, sibling order and namesake anchors.
const RICH: &[(&str, &str)] = &[
    (
        "docs/schemas/page.json",
        r#"{ "type": "object", "additionalProperties": true }"#,
    ),
    (
        "docs/schemas/requirement.json",
        r#"{ "type": "object", "additionalProperties": true, "x-docc-covered": true }"#,
    ),
    ("code/a.txt", "// docc: a\n"),
    (
        "docs/src/index.json",
        r#"{
  "title": "Home\r\npage [[b]]",
  "kind": "page", "status": "draft\nnow", "priority": 2, "tags": ["x", "y"],
  "body": [
    "See [[a]], [[a#goals]], [[a#^g1]], [[sec#content/Part one/Sub]], [[sec#content/Part two]], [[sec#nope/deep]], [[a#body]] and hidden.",
    "![pic](img/p.png) ![web](https://x.org/a.png) `[[a]]` ![c](img/p.png \"t\") ^intro",
    "{{a#goals}}",
    "Inline {{a#short}} and {{a#^g1}} end.",
    "{{sec#content/Part two}}"
  ],
  "short_field": "one line",
  "other-field": 42,
  "flag": true,
  "a b": "first",
  "a-b": "second namesake",
  "empty": "",
  "table": [ { "name": "x|y", "value": "line\r\nbreak [[a]]" }, { "name": "z", "value": 1 } ],
  "list": [ { "k": "v", "n": { "deep": ["p", "q\nr"] } }, { "k": "w" } ],
  "mixed": [ "s", { "o": 1 }, [ "inner", "inner2" ], null, "" ],
  "nested": { "one": "1", "two": { "three": "3" }, "none": [] },
  "notes": [ "a note [[a]]\nsecond line" ],
  "docIgnore": [ { "rule": "DOC352", "reason": "test" } ]
}"#,
    ),
    (
        "docs/src/a.json",
        r#"{
  "title": "Item 10", "parent": "index", "order": 2, "kind": "requirement",
  "body": "A body with ![sub](img/p.png).",
  "goals": [ "G1 ^g1", "G2 with [[index]] ^g2" ],
  "short": "short text"
}"#,
    ),
    (
        "docs/src/b.json",
        r#"{ "kind": "page", "title": "b", "parent": "index", "order": 1, "body": "B." }"#,
    ),
    (
        "docs/src/c.json",
        r#"{ "kind": "page", "title": "Item 9", "parent": "index", "body": "C [[d]]." }"#,
    ),
    (
        "docs/src/d.json",
        r#"{ "kind": "page", "title": "item 9", "parent": "index", "body": "D." }"#,
    ),
    (
        "docs/src/e.json",
        r#"{ "kind": "page", "title": "Ёлка", "parent": "index", "order": 1e10 }"#,
    ),
    (
        "docs/src/f.json",
        r#"{ "kind": "page", "title": "елка", "parent": "index", "order": -0.5 }"#,
    ),
    (
        "docs/src/req.json",
        r#"{ "title": "Req", "parent": "a", "kind": "requirement" }"#,
    ),
    (
        "docs/src/sec.json",
        r#"{
  "title": "Sections", "parent": "a", "kind": "page",
  "content": { "$sections": true,
    "Part one": { "$sections": true, "Sub": "sub text", "Sub_": "namesake", "Deep": { "$sections": true, "Leaf": ["l1", "l2 ^lf"] } },
    "Part two": "two text",
    "Empty part": ""
  },
  "more": { "$sections": true, "Part one": "again" }
}"#,
    ),
    (
        "docs/src/hidden.json",
        r#"{ "kind": "page", "title": "Hidden one", "parent": "index", "hidden": true, "body": "Secret [[a]]." }"#,
    ),
    (
        "docs/src/loose.json",
        r#"{ "kind": "page", "title": "Loose", "body": "No parent." }"#,
    ),
    (
        "docs/src/sub/deep.json",
        r#"{ "kind": "page", "title": "Deep", "parent": "loose", "body": "![up](../img/p.png) {{a#body}} [[index#short_field]] [[index#a-b]]" }"#,
    ),
];

/// write_rich writes RICH with the given docc.json into a new directory.
fn write_rich(config: &str) -> PathBuf {
    let dir = temp_dir();
    let mut files: Vec<(&str, &[u8])> = RICH.iter().map(|(p, c)| (*p, c.as_bytes())).collect();
    files.push(("docc.json", config.as_bytes()));
    files.push((
        "docs/src/img/p.png",
        &[0x89, b'P', b'N', b'G', 0xff, 0x00, b'\r', b'\n'],
    ));
    for (p, c) in files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

/// RICH_CONFIGS are the docc.json files RICH is built with.
const RICH_CONFIGS: [&str; 2] = [
    r#"{ "roots": ["index"], "numbering": true, "lang": "ru", "trace": { "scan": ["code/**"] }, "rules": { "DOC312": "off" } }"#,
    r#"{ "roots": ["index", "loose"], "trace": { "scan": ["code/**"] }, "rules": { "DOC312": "off" } }"#,
];

/// golden_rich: Files and Single of RICH give the bytes the Go build gave,
/// kept in testdata/rich-<n> so the test needs no Go. With DOCC_GOLDEN_UPDATE=1
/// and the Go oracle it rewrites them first.
#[test]
fn golden_rich() {
    let mut fails = Vec::new();
    for (i, config) in RICH_CONFIGS.iter().enumerate() {
        let work = write_rich(config);
        let want = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("src/render/testdata/rich-{i}"));
        let p = load_valid(&work).unwrap();
        if std::env::var_os("DOCC_GOLDEN_UPDATE").is_some() {
            let _ = std::fs::remove_dir_all(&want);
            for full in [false, true] {
                go_build(&work, full);
                let rel = if full { &p.cfg.out_full } else { &p.cfg.out };
                copy_tree(&work.join(rel), &want.join(rel), &[]);
                std::fs::copy(work.join(format!("{rel}.md")), want.join(format!("{rel}.md"))).unwrap();
            }
        }
        for full in [false, true] {
            let r = Renderer::new(&p, full);
            let rel = if full { &p.cfg.out_full } else { &p.cfg.out };
            fails.extend(
                compare_out(&want, rel, &r.files(), &r.single())
                    .into_iter()
                    .map(|d| format!("rich-{i}: {d}")),
            );
        }
        let _ = std::fs::remove_dir_all(&work);
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// golden_build: Files and Single give the same bytes as the Go build of every
/// testdata case and eval project (and of DOCC_GOLDEN_DIRS). Skipped without
/// the Go oracle.
#[test]
fn golden_build() {
    if !Path::new(ORACLE.as_str()).exists() {
        eprintln!("set DOCC_ORACLE to an older docc build to compare with: skipped");
        return;
    }
    let (mut built, mut fails) = (0, Vec::new());
    for config in RICH_CONFIGS {
        let work = write_rich(config);
        match compare_build(&work) {
            Ok(d) => fails.extend(d.into_iter().map(|d| format!("rich {config}: {d}"))),
            Err(e) => fails.push(format!("rich {config}: not built: {e}")),
        }
        fails.extend(
            fresh_matches_go(&work)
                .into_iter()
                .map(|d| format!("rich {config}: {d}")),
        );
        let _ = std::fs::remove_dir_all(&work);
    }
    for src in golden_projects() {
        let work = temp_dir();
        copy_tree(&src, &work, &[".git", "target", "node_modules"]);
        match compare_build(&work) {
            Ok(d) => {
                built += 1;
                fails.extend(d.into_iter().map(|d| format!("{}: {d}", src.display())));
            }
            Err(e) => eprintln!("{}: not built: {e}", src.display()),
        }
        let _ = std::fs::remove_dir_all(&work);
    }
    eprintln!("{built} projects compared with Go");
    assert!(built > 20, "only {built} projects built");
    assert!(
        fails.is_empty(),
        "{} differences:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

/// small_project writes the project of docc init (one page, index) into a new
/// directory and builds docs/out with write.
fn small_project() -> (PathBuf, engine::Project, HashMap<String, Vec<u8>>) {
    let dir = temp_dir();
    for (f, c) in [
        (
            "docc.json",
            r#"{ "src": "docs/src", "out": "docs/out", "roots": ["index"], "maxWarnings": 0 }"#,
        ),
        ("docs/src/index.json", r#"{ "title": "Index", "kind": "page" }"#),
        (
            "docs/schemas/page.json",
            r#"{ "type": "object", "required": ["title"], "additionalProperties": true }"#,
        ),
    ] {
        std::fs::create_dir_all(dir.join(f).parent().unwrap()).unwrap();
        std::fs::write(dir.join(f), c).unwrap();
    }
    let p = load_valid(&dir).unwrap();
    let files = Renderer::new(&p, false).files();
    write(&dir.join("docs/out"), "docs/out", &dir.join("docs/src"), &files).unwrap();
    (dir, p, files)
}

/// fresh_diags runs check_fresh on docs/out and returns its DOC501 as "file: message".
fn fresh_diags(p: &mut engine::Project, files: &HashMap<String, Vec<u8>>) -> Vec<String> {
    check_fresh(p, "docs/out", files);
    p.sorted()
        .iter()
        .filter(|d| d.code == "DOC501")
        .map(|d| format!("{}: {}", d.file, d.message))
        .collect()
}

/// An unreadable directory in the output is skipped as filepath.WalkDir with
/// Go's callback does: check still sees the files after it, build still finds
/// the foreign ones, and without them build fails naming the directory (all
/// checked against the Go build).
#[cfg(unix)]
#[test]
fn walk_skips_unreadable_dir() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, mut p, files) = small_project();
    let out = dir.join("docs/out");
    let a0 = out.join("a0");
    std::fs::create_dir(&a0).unwrap();
    std::fs::set_permissions(&a0, std::fs::Permissions::from_mode(0)).unwrap();
    if std::fs::read_dir(&a0).is_ok() {
        eprintln!("running as root: skipped");
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    std::fs::write(out.join("zz.txt"), "x").unwrap();
    assert_eq!(
        fresh_diags(&mut p, &files),
        ["docs/out/zz.txt: extra file in the output directory that docc did not generate"]
    );
    let src = dir.join("docs/src");
    assert_eq!(
        write(&out, "docs/out", &src, &files).unwrap_err().to_string(),
        "docs/out holds files that docc did not generate and would delete: zz.txt; move or delete them, then run docc build again"
    );
    std::fs::remove_file(out.join("zz.txt")).unwrap();
    assert_eq!(
        write(&out, "docs/out", &src, &files).unwrap_err().to_string(),
        format!("open {}: permission denied", a0.display())
    );
    std::fs::set_permissions(&a0, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A page docc generated under a name that is not UTF-8 is its own (Go opens
/// the raw name): check calls it stale, build removes it.
#[cfg(target_os = "linux")]
#[test]
fn raw_name_is_ours() {
    use std::os::unix::ffi::OsStrExt;
    let (dir, mut p, files) = small_project();
    let out = dir.join("docs/out");
    let old = out.join(std::ffi::OsStr::from_bytes(b"old\xff.md"));
    std::fs::write(&old, "<!-- GENERATED by docc, DO NOT EDIT -->\nx").unwrap();
    assert_eq!(
        fresh_diags(&mut p, &files),
        ["docs/out/old\u{FFFD}.md: extra file in the output directory (no source produces it)"]
    );
    write(&out, "docs/out", &dir.join("docs/src"), &files).unwrap();
    assert!(!old.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pages are written 0644 and directories 0755 (before the umask), as Go does:
/// whatever the umask, nobody but the owner gets write access.
#[cfg(unix)]
#[test]
fn write_modes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir();
    let out = dir.join("out");
    let files = HashMap::from([("sub/a.md".to_string(), b"a".to_vec())]);
    write(&out, "out", &dir.join("src"), &files).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&out.join("sub/a.md")) & 0o133, 0);
    assert_eq!(mode(&out.join("sub")) & 0o022, 0);
    write_file(&dir.join("one.md"), b"x").unwrap();
    assert_eq!(mode(&dir.join("one.md")) & 0o133, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// In the HTML file one-line fields form the property list: the values in
/// Prop, the markers they held counted in cut and removed by mark_re, also
/// under another marker name. Expected values printed by the Go build.
#[test]
fn props_and_marker_tag() {
    let dir = temp_dir();
    std::fs::create_dir_all(dir.join("docs/src")).unwrap();
    std::fs::write(
        dir.join("docs/src/a.json"),
        r#"{"kind":"page","title":"A","owner":"Ann","due-date":["2026"],"state":"has <!--b:0--> text","notes":["one","two"],"long":"para one\n\npara two"}"#,
    )
    .unwrap();
    let mut p = engine::load(config::default(&dir)).unwrap();
    p.validate();
    let r = Renderer::new(&p, false).variant(true, false, true);
    let d = p.doc("a").unwrap();
    for tag in ["", "bx"] {
        let mut pg = text::Page::new(&r, d);
        pg.tag = tag.to_string();
        let src = pg.build();
        let t = if tag.is_empty() { "b" } else { tag };
        assert_eq!(
            src,
            format!("<!--{t}:4-->\n\n## Long\n\n<!--{t}:3-->\n\npara one\n\npara two\n"),
            "tag {tag:?}"
        );
        assert_eq!((pg.cut, pg.marks.len()), (3, 5), "tag {tag:?}");
        let props: Vec<(&str, &str, &str)> = pg
            .props
            .iter()
            .map(|p| (p.key.as_str(), p.id.as_str(), p.md.as_str()))
            .collect();
        assert_eq!(
            props,
            [
                ("Owner", "a--owner", "Ann"),
                ("Due date", "a--due-date", "2026"),
                ("State", "a--state", "has <!--b:0--> text"),
            ],
            "tag {tag:?}"
        );
        let re = pg.mark_re();
        assert_eq!(
            re.replace_all(&format!("x<!--{t}:12-->\n\ny<!--{t}:3-->\nz<!--{t}:-->\n"), ""),
            format!("xyz<!--{t}:-->\n")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
