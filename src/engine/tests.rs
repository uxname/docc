use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::linkrules::{FINGERPRINT_SKIP, canonical};
use super::*;
use crate::config;
use crate::diag::Diagnostic;
use crate::loader;

// anchors

#[test]
fn test_split_anchor() {
    let cases = [
        ("Limit is 5. ^limit", "Limit is 5.", "limit"),
        ("Release ^new", "Release", "new"),
        ("line one\nline two ^end", "line one\nline two", "end"),
        ("x ^2", "x ^2", ""),
        ("react ^18", "react ^18", ""),
        ("a note[^1]", "a note[^1]", ""),
        ("see ^1a", "see ^1a", ""),
        ("Upper ^Name", "Upper ^Name", ""),
        ("^solo", "^solo", ""),
        ("middle ^a\nnext line", "middle ^a\nnext line", ""),
        ("trailing ^a ", "trailing ^a ", ""),
    ];
    for (input, text, name) in cases {
        assert_eq!(split_anchor(input), (text, name), "{input:?}");
    }
}

// fingerprints

fn fp(s: &str) -> String {
    let r = loader::parse(s.as_bytes());
    assert!(r.err.is_none(), "{:?}", r.err);
    fingerprint(r.root.as_ref().unwrap())
}

fn canon(s: &str) -> String {
    let r = loader::parse(s.as_bytes());
    let mut b = String::new();
    canonical(&mut b, r.root.as_ref().unwrap(), FINGERPRINT_SKIP);
    b
}

// Reference values were computed independently with Python
// (json.dumps(sort_keys=True, separators=(',', ':'), ensure_ascii=False), sha256).
#[test]
fn test_fingerprint_reference() {
    let cases = [
        (r#"{ "title": "Beta", "parent": "index" }"#, "04b48c1c"),
        (r#"{ "title": "Gamma", "parent": "index" }"#, "1789f10f"),
        (
            r#"{ "title": "Alpha", "parent": "index", "statement": "Первый факт", "n": 3, "items": [1, 2.5, true, null], "notes": ["ignored"] }"#,
            "52a40e88",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(fp(input), want, "{input}");
    }
}

#[test]
fn test_fingerprint_canonical_form() {
    // notes and docIgnore are dropped at the top level only; keys sorted at every level
    let got = canon(
        r#"{"b":{"z":1,"a":2},"a":[3,"x"],"notes":["n"],"$schema":"s","docIgnore":[],"c":{"notes":1}}"#,
    );
    assert_eq!(
        got,
        r#"{"$schema":"s","a":[3,"x"],"b":{"a":2,"z":1},"c":{"notes":1}}"#
    );
    // numbers keep their source text
    assert!(
        canon(r#"{"n":2.50}"#) == r#"{"n":2.50}"# && fp(r#"{"n":2.50}"#) != fp(r#"{"n":2.5}"#),
        "numbers must stay as written"
    );
    // minimal escaping: control characters as \u00XX, others raw
    let input = "{\"s\":\"a\\n\\t\\\"\\\\\\/é\"}";
    assert_eq!(
        canon(input),
        "{\"s\":\"a\\u000a\\u0009\\\"\\\\/é\"}",
        "escaping"
    );
    // key order in the file does not matter, whitespace does not matter
    assert_eq!(
        fp(r#"{"a":1,"b":2}"#),
        fp("{\n  \"b\": 2,\n  \"a\": 1\n}"),
        "order or whitespace changed the fingerprint"
    );
}

// golden: testdata/<CODE>/{red,green} run as `docc validate` (tests/golden.rs runs them through the CLI)

static TMP: AtomicUsize = AtomicUsize::new(0);

fn temp_dir() -> PathBuf {
    let n = TMP.fetch_add(1, Ordering::Relaxed);
    let p = std::env::temp_dir().join(format!("docc-engine-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn copy_tree(src: &Path, dst: &Path) {
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            std::fs::create_dir_all(&to).unwrap();
            copy_tree(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

/// golden_json does what cmdValidate (or, with check, cmdCheck without
/// render.CheckFresh) does in work and returns the diagnostics array as the
/// golden test writes it (MarshalIndent of the report's diagnostics: HTML
/// characters escaped).
fn golden_json(work: &Path, check: bool) -> Result<(String, Vec<Diagnostic>), String> {
    let cfg = match std::fs::read(work.join("docc.json")) {
        Ok(data) => config::parse(&data, work, "docc.json").map_err(|e| e.to_string())?,
        Err(_) => config::default(work),
    };
    let mut p = load(cfg).map_err(|e| e.to_string())?;
    let mut ds = p.validate();
    if check && !structural(&ds) {
        let files = crate::render::Renderer::new(&p, false).files();
        if p.cfg.commit_out {
            crate::render::check_fresh(&p, &p.cfg.out, &files, None);
        }
        p.check_lock_file();
        ds = p.sorted();
    }
    let s = serde_json::to_string_pretty(&ds)
        .unwrap()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok((s + "\n", ds))
}

/// golden_cases returns the testdata cases whose args.txt is args (plain
/// `docc validate` when there is no args.txt).
fn golden_cases(args: &[&str]) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
    let mut codes: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    codes.sort();
    let mut out = Vec::new();
    for c in codes {
        for kind in ["red", "green"] {
            let dir = c.join(kind);
            if !dir.is_dir() {
                continue;
            }
            let got = std::fs::read_to_string(dir.join("args.txt"))
                .unwrap_or_else(|_| "validate".into());
            if got.split_whitespace().collect::<Vec<_>>() == args {
                out.push(dir);
            }
        }
    }
    out
}

/// run_golden is runGolden: the diagnostics must equal expected.json, a red
/// case must report its code and a green one must not. The exit code check
/// waits for the CLI: it follows from the levels in expected.json.
fn run_golden(cases: Vec<PathBuf>, check: bool) {
    let mut fails = Vec::new();
    for dir in cases {
        let work = temp_dir();
        copy_tree(&dir, &work);
        let got = golden_json(&work, check);
        let _ = std::fs::remove_dir_all(&work);
        let want = std::fs::read_to_string(dir.join("expected.json")).unwrap();
        let ds = match got {
            Ok((got, ds)) if got == want => ds,
            Ok((got, _)) => {
                fails.push(format!("{}\n--- got\n{got}--- want\n{want}", dir.display()));
                continue;
            }
            Err(e) => {
                fails.push(format!("{}: {e}", dir.display()));
                continue;
            }
        };
        let code = dir.parent().unwrap().file_name().unwrap().to_string_lossy();
        let kind = dir.file_name().unwrap().to_string_lossy();
        let found = ds
            .iter()
            .any(|d| d.code == code || (code == "DOC800" && d.rule.starts_with("custom/")));
        match (&*kind, found) {
            ("red", false) => fails.push(format!("{}: red case does not report {code}", dir.display())),
            ("green", true) => fails.push(format!("{}: green case reports {code}", dir.display())),
            _ => {}
        }
    }
    assert!(
        fails.is_empty(),
        "{} cases differ:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

#[test]
fn golden_validate() {
    let cases = golden_cases(&["validate"]);
    assert!(cases.len() > 60, "testdata not found");
    run_golden(cases, false);
}

/// golden_check runs the `docc check` cases whose output is fresh, so that
/// render.CheckFresh adds nothing: they test check_lock_file (Go CheckLock).
#[test]
fn golden_check() {
    let cases: Vec<_> = golden_cases(&["check"])
        .into_iter()
        .filter(|d| !d.to_string_lossy().contains("DOC501"))
        .collect();
    assert_eq!(cases.len(), 2, "testdata DOC503 not found");
    run_golden(cases, true);
}

#[test]
fn golden_check_fresh() {
    run_golden(golden_cases(&["check"]), true);
}

/// write_lock writes the same docc.lock as Go for testdata/DOC503/green, and
/// rewrites the red case's lock to it (the missing link to c is added).
#[test]
fn write_lock_matches_go() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/DOC503");
    let want = std::fs::read_to_string(root.join("green/docc.lock")).unwrap();
    for kind in ["green", "red"] {
        let work = temp_dir();
        copy_tree(&root.join(kind), &work);
        let mut p = load(config::default(&work)).unwrap();
        p.validate();
        p.write_lock().unwrap();
        let got = std::fs::read_to_string(work.join(LOCK_FILE)).unwrap();
        let _ = std::fs::remove_dir_all(&work);
        assert_eq!(got, want, "{kind}");
    }
}

/// A src folder that is itself a symbolic link is not walked, as
/// filepath.WalkDir does not follow a root that is a link: no documents.
#[cfg(unix)]
#[test]
fn src_symlink_root_is_not_walked() {
    let work = temp_dir();
    std::fs::create_dir_all(work.join("real")).unwrap();
    std::fs::create_dir_all(work.join("p/docs")).unwrap();
    std::fs::write(
        work.join("real/index.json"),
        r#"{"title":"A","body":"x [[nope]]"}"#,
    )
    .unwrap();
    std::os::unix::fs::symlink("../../real", work.join("p/docs/src")).unwrap();
    let mut p = load(config::default(&work.join("p"))).unwrap();
    let ds = p.validate();
    let _ = std::fs::remove_dir_all(&work);
    let got: Vec<_> = ds.iter().map(|d| format!("{} {} {}", d.file, d.code, d.message)).collect();
    assert_eq!(got, [r#"docc.json DOC205 root "index" does not exist"#]);
}

/// gen_scale writes n documents of 5-20 KB of Russian text into dir/docs/src,
/// as gen_scale in tests/scale.rs.
fn gen_scale(dir: &Path, n: usize) {
    let para = "Система должна обеспечивать устойчивую работу при изменении нагрузки на сервис. Администратор получает уведомление о каждом сбое и может открыть журнал событий за выбранный период. ";
    let write = |rel: &str, content: &str| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    };
    write(
        "docs/schemas/note.json",
        r#"{"type":"object","properties":{"status":{"enum":["draft","accepted"]},"statement":{"type":"string","minLength":5}},"additionalProperties":true}"#,
    );
    write(
        "docs/schemas/page.json",
        r#"{"type":"object","additionalProperties":true}"#,
    );
    let sections = 10;
    write("docs/src/index.json", r#"{"title":"Индекс","kind":"page"}"#);
    for s in 0..sections {
        write(
            &format!("docs/src/sec-{s}.json"),
            &format!(r#"{{"title":"Раздел {s}","kind":"page","parent":"index"}}"#),
        );
    }
    let m = n - 1 - sections;
    for i in 0..m {
        let paras = 5 + i % 12;
        let mut b = format!(
            r#"{{"title":"Документ {i}","kind":"note","status":"accepted","parent":"sec-{}","tags":["x"],"statement":"Факт номер {i}, см. [[doc-{}]].","body":["#,
            i % sections,
            (i + 1) % m
        );
        for k in 0..paras {
            if k > 0 {
                b.push(',');
            }
            let mut text = para.repeat(2);
            if k == 0 {
                text += &format!("См. [[doc-{}]].", (i + 1) % m);
            }
            b += &format!("\"{text}\"");
        }
        b.push_str("]}");
        write(&format!("docs/src/doc-{i}.json"), &b);
    }
}

/// scale reports the time of load and validate on 1000 documents.
/// Run: cargo test --release --lib engine::tests::scale -- --ignored --nocapture
#[test]
#[ignore = "a measurement"]
fn scale() {
    let dir = temp_dir();
    gen_scale(&dir, 1000);
    for run in 1..=3 {
        let t0 = std::time::Instant::now();
        let mut p = load(config::default(&dir)).unwrap();
        let t1 = std::time::Instant::now();
        let ds = p.validate();
        let t2 = std::time::Instant::now();
        eprintln!(
            "run {run}: load {:?}, validate {:?} (diagnostics: {})",
            t1 - t0,
            t2 - t1,
            ds.len()
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// helpers with Go semantics that have no Go test of their own

#[test]
fn equal_fold_is_go() {
    use super::core::equal_fold;
    // values from Go's strings.EqualFold
    for (a, b, want) in [
        ("Title", "title", true),
        ("TITLE", "tItLe", true),
        ("ёЛКА", "Ёлка", true),
        ("k", "\u{212a}", true), // Kelvin sign
        ("ſ", "S", true),        // long s
        ("ς", "Σ", true),
        ("ß", "ẞ", true),
        ("ΐ", "\u{1fd3}", true),
        ("ﬅ", "ﬆ", true),
        ("i", "ı", false), // dotless i
        ("i", "İ", false),
        ("I", "İ", false),
        ("ss", "ß", false),
        ("title", "titles", false),
    ] {
        assert_eq!(equal_fold(a, b), want, "{a} {b}");
    }
}

#[test]
fn fast_to_lower_is_text_to_lower() {
    let s = "ABC xyz ЁЛКА Ѐ Џ ѐ џ АаЯя İ Σσς ẞ ǅ \u{212a} Ω é e\u{301} 😀";
    assert_eq!(super::textrules::to_lower(s), crate::text::to_lower(s));
    let all: String = (0..0x3000u32).filter_map(char::from_u32).collect();
    assert_eq!(
        super::textrules::to_lower(&all),
        crate::text::to_lower(&all)
    );
}

/// The prefilter never hides a hit of the matcher.
#[test]
fn prefilter_keeps_every_hit() {
    use super::textrules::{Prefilter, Prose};
    let lists: Vec<Vec<&str>> = vec![
        crate::text::FORBIDDEN.to_vec(),
        crate::text::PLACEHOLDERS.to_vec(),
        crate::text::HEDGING.to_vec(),
        crate::text::TIMELESS.to_vec(),
        vec!["foo*", "a* b*", "x  y", "по-*", "İz", "ſt", "*"],
        vec!["word"],
        vec![],
    ];
    let texts = [
        "Это быстро и удобно, и т.д. и\u{a0}т.п.",
        "Система работает корректно, см.   выше",
        "foobar fooз a1 bcd A-B Bx X\tY x y",
        "По-видимому скоро, НОВЫЙ сейчас, Maybe several",
        "İz iz İZ ſt st ST",
        "и\u{306} й e\u{301}",
        "words word, sword WORD",
        "TODO ??? N/A lorem — TBD",
        "",
    ];
    for list in &lists {
        let m = crate::text::new_matcher(list);
        let pf = Prefilter::new(list);
        for t in texts {
            let sp = Prose::new(0, t);
            let hits = m.find(t);
            if !hits.is_empty() {
                assert!(pf.may_hit(&sp), "{list:?} {t:?}: {hits:?}");
            }
        }
    }
}
