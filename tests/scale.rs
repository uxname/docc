mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::*;
use docc::{config, engine, render};

/// gen_scale writes n documents of 5-20 KB of Russian text into dir/docs/src:
/// one index, sections below it, leaves below the sections, cross links and a
/// kind schema.
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
    write("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#);
    let sections = 10;
    write("docs/src/index.json", r#"{"title":"Индекс","kind":"page"}"#);
    for s in 0..sections {
        write(&format!("docs/src/sec-{s}.json"), &format!(r#"{{"title":"Раздел {s}","kind":"page","parent":"index"}}"#));
    }
    let leaves = n - 1 - sections;
    for i in 0..leaves {
        // about 0.35 KB of Cyrillic text (2 bytes per letter) per paragraph
        let paras = 5 + i % 12;
        let next = (i + 1) % leaves;
        let mut b = format!(
            r#"{{"title":"Документ {i}","kind":"note","status":"accepted","parent":"sec-{}","tags":["x"],"statement":"Факт номер {i}, см. [[doc-{next}]].","body":["#,
            i % sections
        );
        for k in 0..paras {
            if k > 0 {
                b.push(',');
            }
            let mut text = para.repeat(2);
            if k == 0 {
                text += &format!("См. [[doc-{next}]].");
            }
            // Go's %q: the text has no quotes, backslashes or control characters
            b.push_str(&format!("\"{text}\""));
        }
        b.push_str("]}");
        write(&format!("docs/src/doc-{i}.json"), &b);
    }
}

fn ms(d: Duration) -> String {
    format!("{}ms", d.as_millis())
}

/// scale reports the time of check on 1000 documents by phase.
/// Run: DOCC_SCALE=1 cargo test --release --test scale -- --nocapture
#[test]
fn scale() {
    if std::env::var_os("DOCC_SCALE").is_none_or(|v| v.is_empty()) {
        eprintln!("set DOCC_SCALE=1 to measure");
        return;
    }
    let dir = temp_dir();
    gen_scale(&dir.path, 1000);
    let start = Instant::now();
    let (code, out, _) = dir.run(&["build"]);
    assert_eq!(code, 0, "build: {out}");
    eprintln!("docc build end to end: {}", ms(start.elapsed()));
    let total: u64 = std::fs::read_dir(dir.join("docs/src")).unwrap().flatten().map(|e| e.metadata().unwrap().len()).sum();
    eprintln!("1000 documents, {:.1} MB of sources", total as f64 / 1e6);
    for run in 1..=3 {
        let t0 = Instant::now();
        let cfg = config::load(&dir.join("docc.json").to_string_lossy(), false).unwrap();
        let mut p = engine::load(cfg).unwrap();
        let t1 = Instant::now();
        p.validate();
        let t2 = Instant::now();
        let files = render::Renderer::new(&p, false).files();
        let t3 = Instant::now();
        render::check_fresh(&p, &p.cfg.out, &files);
        let t4 = Instant::now();
        eprintln!(
            "run {run}: load {}, validate {}, render {}, compare out/ {}, total {} (diagnostics: {})",
            ms(t1 - t0),
            ms(t2 - t1),
            ms(t3 - t2),
            ms(t4 - t3),
            ms(t4 - t0),
            p.diags().len()
        );
    }
    let start = Instant::now();
    let (code, _, _) = dir.run(&["check"]);
    eprintln!("docc check end to end: {} (exit {code})", ms(start.elapsed()));
    let start = Instant::now();
    let (code, _, _) = dir.run(&["build"]);
    eprintln!("docc build end to end, nothing changed: {} (exit {code})", ms(start.elapsed()));
}
