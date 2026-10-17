use std::collections::HashSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

use super::*;
use crate::config;

/// convert_snippet is the setup of the Go dump: page id "p", section number
/// "1.2", marker tag "b".
fn convert_snippet(src: &str) -> String {
    let mut g = Ids {
        prefix: "p".into(),
        used: HashSet::new(),
        nums: HashSet::from(["1.2".to_string()]),
    };
    let marker = Regex::new(r"<!--b:([0-9]+)-->\n(?:\n)?").unwrap();
    markdown::convert(src, "p--", &marker, &mut g)
}

/// dump_snippets converts every snippet of $ZZ_IN into $ZZ_OUT, for the
/// comparison with goldmark; skipped without them.
#[test]
fn dump_snippets() {
    let (Ok(input), Ok(output)) = (std::env::var("ZZ_IN"), std::env::var("ZZ_OUT")) else {
        return;
    };
    let srcs: Vec<String> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let outs: Vec<String> = srcs.iter().map(|s| convert_snippet(s)).collect();
    std::fs::write(output, serde_json::to_vec(&outs).unwrap()).unwrap();
}

/// GOLDMARK_CASES are Markdown snippets (the CommonMark spec, goldmark's own
/// extension tests and cases written for docc) with the HTML goldmark gave in
/// the setup of the Go build; "deviation" marks the cases of docs/rust-port/render.md.
static GOLDMARK_CASES: &str = include_str!("goldmark_cases.json");

#[test]
fn markdown_matches_goldmark() {
    #[derive(serde::Deserialize)]
    struct Case {
        md: String,
        html: String,
        #[serde(default)]
        deviation: bool,
    }
    let cases: Vec<Case> = serde_json::from_str(GOLDMARK_CASES).unwrap();
    assert!(cases.len() > 1000, "only {} cases", cases.len());
    let mut fails = Vec::new();
    for c in &cases {
        let got = convert_snippet(&c.md);
        if (got == c.html) == c.deviation {
            fails.push(format!(
                "{:?} (deviation {})\n  rust {got:?}\n  go   {:?}",
                c.md, c.deviation, c.html
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} differ:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

#[test]
fn heading_ids() {
    let mut g = Ids {
        prefix: "doc".into(),
        used: HashSet::new(),
        nums: HashSet::from(["2.1".to_string()]),
    };
    for (v, want) in [
        ("Goals", "doc--goals"),
        ("Goals", "doc--goals-1"),
        ("2.1 Goals", "doc--goals-2"),
        ("2.2 Plan", "doc--22-plan"),
        ("Сроки и цели!", "doc--сроки-и-цели"),
        ("", "doc--"),
    ] {
        assert_eq!(g.generate(v), want, "{v}");
    }
}

#[test]
fn script_json_escapes() {
    #[derive(Serialize)]
    struct T {
        a: &'static str,
        b: Vec<(String, usize)>,
    }
    let v = T {
        a: "</script> & \u{2028}\u{2029} \"é\" \u{1}\t",
        b: vec![("x".into(), 2)],
    };
    // the bytes Go printed for the same value
    assert_eq!(
        script_json(&v),
        r#"{"a":"\u003c/script> & \u2028\u2029 \"é\" \u0001\t","b":[["x",2]]}"#
    );
}

#[test]
fn replace_once_does_not_rescan() {
    assert_eq!(
        replace_once("a{{X}}b{{Y}}{{X}}", &[("{{X}}", "{{Y}}"), ("{{Y}}", "y")]),
        "a{{Y}}by{{Y}}"
    );
    assert_eq!(base_name("/a/b/"), "b");
    assert_eq!(base_name("/"), "/");
    assert_eq!(base_name(""), ".");
    // filepath.Base on Windows also cuts at '\\' and leaves out the drive
    if cfg!(windows) {
        assert_eq!(base_name(r"C:\Users\a\proj"), "proj");
        assert_eq!(base_name(r"C:\"), r"\");
        assert_eq!(base_name("C:"), ".");
    } else {
        assert_eq!(base_name(r"C:\Users\a\proj"), r"C:\Users\a\proj");
    }
}

#[test]
fn zip_names_and_times() {
    let files = HashMap::from([
        ("b.md".to_string(), b"B".to_vec()),
        ("a/c.md".to_string(), b"C".to_vec()),
        ("img/p.png".to_string(), vec![0x89, 0, 0xff]),
    ]);
    let b = base64::engine::general_purpose::STANDARD
        .decode(zip_files(&files))
        .unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(b)).unwrap();
    let mut got = Vec::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        assert_eq!(f.compression(), zip::CompressionMethod::Deflated);
        let t = f.last_modified().unwrap();
        assert_eq!(
            (t.year(), t.month(), t.day(), t.hour(), t.minute()),
            (2000, 1, 1, 0, 0)
        );
        let mut c = Vec::new();
        f.read_to_end(&mut c).unwrap();
        got.push((f.name().unwrap().to_string(), c));
    }
    assert_eq!(
        got,
        [
            ("docs/a/c.md".to_string(), b"C".to_vec()),
            ("docs/b.md".to_string(), b"B".to_vec()),
            ("docs/img/p.png".to_string(), vec![0x89, 0, 0xff]),
        ]
    );
}

fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "docc-html-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let ft = e.file_type().unwrap();
        let to = dst.join(e.file_name());
        if ft.is_dir() {
            copy_tree(&e.path(), &to);
        } else if ft.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(e.path()).unwrap(), to).unwrap();
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// load_valid loads and validates the project in dir; Err when it cannot be built.
fn load_valid(dir: &Path) -> Result<crate::engine::Project, String> {
    let cfg = match std::fs::read(dir.join("docc.json")) {
        Ok(data) => config::parse(&data, dir, "docc.json").map_err(|e| e.to_string())?,
        Err(_) => config::default(dir),
    };
    let mut p = crate::engine::load(cfg).map_err(|e| e.to_string())?;
    let ds = p.validate();
    if crate::engine::structural(&ds) {
        return Err("errors".into());
    }
    Ok(p)
}

const ISLAND: &str = r#"<script type="application/json" id="docc-data">"#;

/// unzip lists the files of the base64 zip of the page data.
fn unzip(b64: &str) -> Value {
    let b = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(b)).unwrap();
    let mut out = serde_json::Map::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut c = Vec::new();
        f.read_to_end(&mut c).unwrap();
        out.insert(
            f.name().unwrap().to_string(),
            Value::String(String::from_utf8_lossy(&c).into_owned()),
        );
    }
    Value::Object(out)
}

/// diff_values lists the paths where two JSON values differ.
fn diff_values(path: &str, got: &Value, want: &Value, out: &mut Vec<String>) {
    match (got, want) {
        (Value::Object(g), Value::Object(w)) => {
            let keys: std::collections::BTreeSet<&String> = g.keys().chain(w.keys()).collect();
            for k in keys {
                diff_values(
                    &format!("{path}.{k}"),
                    g.get(k).unwrap_or(&Value::Null),
                    w.get(k).unwrap_or(&Value::Null),
                    out,
                );
            }
        }
        (Value::Array(g), Value::Array(w)) if g.len() == w.len() => {
            for (i, (a, b)) in g.iter().zip(w).enumerate() {
                diff_values(&format!("{path}[{i}]"), a, b, out);
            }
        }
        _ if got != want => out.push(format!("{path}:\n    rust {got}\n    go   {want}")),
        _ => {}
    }
}

/// compare_html compares an HTML file with the one Go wrote: the page around
/// the data byte for byte, the data field by field with the zip unpacked,
/// then the data as text (the encoding of the JSON).
fn compare_html(got: &str, want: &str) -> Vec<String> {
    let split = |s: &str| -> (String, String) {
        let i = s.find(ISLAND).expect("no data island") + ISLAND.len();
        let j = i + s[i..].find("</script>").unwrap();
        (format!("{}{}", &s[..i], &s[j..]), s[i..j].to_string())
    };
    let ((gp, gj), (wp, wj)) = (split(got), split(want));
    let mut out = Vec::new();
    if gp != wp {
        let n = gp
            .bytes()
            .zip(wp.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        out.push(format!(
            "page differs at {n}: got {:?} want {:?}",
            &gp[n.saturating_sub(40)..(n + 80).min(gp.len())],
            &wp[n.saturating_sub(40)..(n + 80).min(wp.len())]
        ));
    }
    let parse = |s: &str| -> Value {
        let mut v: Value = serde_json::from_str(s).unwrap();
        let z = unzip(v["zip"].as_str().unwrap());
        v["zip"] = z;
        v
    };
    let (gv, wv) = (parse(&gj), parse(&wj));
    diff_values("", &gv, &wv, &mut out);
    if out.is_empty() {
        let blank = |s: &str| {
            Regex::new(r#""zip":"[^"]*""#)
                .unwrap()
                .replace(s, "")
                .into_owned()
        };
        let (g, w) = (blank(&gj), blank(&wj));
        if g != w {
            let n = g.bytes().zip(w.bytes()).take_while(|(a, b)| a == b).count();
            out.push(format!(
                "data encoded differently at {n}: got {:?} want {:?}",
                &g[n.saturating_sub(20)..(n + 40).min(g.len())],
                &w[n.saturating_sub(20)..(n + 40).min(w.len())]
            ));
        }
    }
    out
}

/// The HTML files of the render golden: testdata/render/basic built
/// in a folder named 001 (the page keys its comments by it) gives
/// expect/out.html and expect/out-full.html, the zip compared by its files.
#[test]
fn golden_render_basic() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/render/basic");
    let work = temp_dir().join("001");
    copy_tree(&src.join("docs"), &work.join("docs"));
    let p = load_valid(&work).unwrap();
    let mut fails = Vec::new();
    for (full, name) in [(false, "out.html"), (true, "out-full.html")] {
        let got = Renderer::new(&p, full).html().unwrap();
        let want = std::fs::read_to_string(src.join("expect").join(name)).unwrap();
        fails.extend(
            compare_html(&got, &want)
                .into_iter()
                .map(|d| format!("{name}: {d}")),
        );
    }
    let _ = std::fs::remove_dir_all(work.parent().unwrap());
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// RICH is a project that uses every part of the HTML file: numbered sections
/// (left out of the heading ids), the property list, headings and footnotes in
/// the text, tables, task lists, images (an SVG one goldmark drops), raw HTML,
/// a hidden page, Russian labels and statuses, code coverage, branding and
/// comments (internal ones, moved and missing blocks).
const RICH: &[(&str, &str)] = &[
    (
        "docc.json",
        r##"{ "roots": ["index"], "numbering": true, "lang": "ru", "trace": { "scan": ["code/**"] },
  "rules": { "DOC312": "off" },
  "branding": { "name": "Acme <Co>", "url": "https://acme.example/docs", "logo": "logo.png", "accent": "#336699" } }"##,
    ),
    ("logo.png", "\u{89}PNG"),
    ("code/x.go", "// docc: a\n"),
    (
        "docs/schemas/page.json",
        r#"{ "type": "object", "additionalProperties": true }"#,
    ),
    (
        "docs/schemas/requirement.json",
        r#"{ "type": "object", "additionalProperties": true, "x-docc-covered": true }"#,
    ),
    ("docs/src/img/p.png", "\u{89}PNG\r\n"),
    (
        "docs/src/img/s.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    ),
    (
        "docs/src/index.json",
        r###"{
  "title": "Главная", "kind": "page", "status": "Draft", "priority": 1, "tags": ["x", "y"],
  "body": [
    "First paragraph with **bold**, `code`, [[a]], www.example.com and a@b.co.[^1] More than enough words to make a summary that goes on and on.",
    "## Раздел в тексте\n\nText under it.\n\n## Раздел в тексте",
    "| a | b |\n|:--|--:|\n| `x\\|y` | 1 |\n| 2 |",
    "- [ ] open\n- [x] done\n\n1. one\n2. two",
    "![png](img/p.png) ![svg](img/s.svg) <br> <span>dropped</span> <a id=\"anc\"></a>",
    "Term\n: Definition with ~~strike~~.",
    "<div>block</div>",
    "<!--b:0-->",
    "[^1]: The note, see https://example.com/a_b.\n\n[^2]: Unused."
  ],
  "owner": "Ann <ann@example.com>",
  "due": ["2026-12-01"],
  "state": "short [[a]]",
  "plan": { "$sections": true,
    "Цели": "Goals text.[^n]\n\n[^n]: Section note.",
    "Сроки": { "$sections": true, "Этап 1": ["Step *one*", "Step two ^s2"], "Этап 2": "" }
  },
  "notes": ["hidden note"]
}"###,
    ),
    (
        "docs/src/a.json",
        r###"{
  "title": "Требование A", "parent": "index", "kind": "requirement", "status": "accepted",
  "body": ["Body of A with a heading:\n\n### 2.1 Numbered heading text\n\nand [[index#plan/Цели]].", "Second <b>&</b> text"],
  "owner": "Bob",
  "parts": { "$sections": true, "Обзор": "## 1.1 Обзор\n\nSame title as the section.", "Детали": "x" }
}"###,
    ),
    (
        "docs/src/h.json",
        r#"{ "title": "Скрытый", "parent": "index", "kind": "page", "hidden": true, "body": "Secret [[a]]." }"#,
    ),
    (
        "docc.comments.json",
        r#"{"comments": [
{"id": "c1", "doc": "a", "path": "/body/0", "quote": "Body of A with a heading:\n\n### 2.1 Numbered heading text\n\nand [[index#plan/Цели]].", "text": "Fix this", "author": "Eve", "created": "2026-10-01T10:00:00Z"},
{"id": "c2", "doc": "a", "path": "/body/5", "quote": "Second <b>&</b> text", "text": "moved", "fragment": "text", "prefix": "</b> ", "status": "done", "reply": "ok"},
{"id": "c3", "doc": "a", "path": "/owner", "quote": "Bob", "text": "internal note", "internal": true},
{"id": "c4", "doc": "gone", "path": "/body", "quote": "x", "text": "missing doc"}
]}"#,
    ),
];

/// write_rich writes RICH into a new folder named rich (the page keys its
/// comments by the folder name).
fn write_rich() -> PathBuf {
    let dir = temp_dir().join("rich");
    for (p, c) in RICH {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    dir
}

/// island returns the data of an HTML file.
fn island(html: &str) -> &str {
    let i = html.find(ISLAND).unwrap() + ISLAND.len();
    &html[i..i + html[i..].find("</script>").unwrap()]
}

/// golden_rich_html: the HTML files of RICH carry the data the Go build wrote,
/// kept in testdata/rich-<plain|full>.json so the test needs no Go. With
/// DOCC_GOLDEN_UPDATE=1 and the Go oracle it rewrites them first.
#[test]
fn golden_rich_html() {
    let dir = write_rich();
    let want_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/render/html/testdata");
    let p = load_valid(&dir).unwrap();
    let mut fails = Vec::new();
    for (full, name) in [(false, "rich-plain.json"), (true, "rich-full.json")] {
        if std::env::var_os("DOCC_GOLDEN_UPDATE").is_some() {
            let mut args = vec!["build", "--html"];
            if full {
                args.push("--full");
            }
            let st = Command::new(ORACLE.as_str())
                .args(&args)
                .current_dir(&dir)
                .output()
                .unwrap();
            let rel = if full { &p.cfg.out_full } else { &p.cfg.out };
            let html = std::fs::read_to_string(dir.join(format!("{rel}.html")))
                .unwrap_or_else(|_| panic!("go: {}", String::from_utf8_lossy(&st.stderr)));
            std::fs::create_dir_all(&want_dir).unwrap();
            std::fs::write(want_dir.join(name), island(&html)).unwrap();
        }
        let want = std::fs::read_to_string(want_dir.join(name)).unwrap();
        let got = Renderer::new(&p, full).html().unwrap();
        let wrap = |s: &str| format!("{ISLAND}{s}</script>");
        fails.extend(
            compare_html(&wrap(island(&got)), &wrap(&want))
                .into_iter()
                .map(|d| format!("{name}: {d}")),
        );
    }
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// ORACLE (env DOCC_ORACLE) is an older docc build the HTML files are compared with.
static ORACLE: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| std::env::var("DOCC_ORACLE").unwrap_or_default());

/// html_projects are the testdata cases, the eval projects of the skill and
/// the folders of DOCC_HTML_DIRS (':'-separated: clones of this repository and
/// of o2, so the history comes from Git).
fn html_projects() -> Vec<PathBuf> {
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
                for k in ["red", "green", "basic"] {
                    if d.join(k).is_dir() {
                        out.push(d.join(k));
                    }
                }
            } else if d.is_dir() {
                out.push(d);
            }
        }
    }
    if let Ok(extra) = std::env::var("DOCC_HTML_DIRS") {
        out.extend(
            extra
                .split(':')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }
    out
}

/// golden_html: the HTML file (plain and full) of every project is the one
/// the Go build writes. Skipped without the Go oracle.
#[test]
fn golden_html() {
    if !Path::new(ORACLE.as_str()).exists() {
        eprintln!("set DOCC_ORACLE to an older docc build to compare with: skipped");
        return;
    }
    let (mut built, mut fails) = (0, Vec::new());
    for src in html_projects() {
        let name = src.file_name().unwrap().to_owned();
        let (a, b) = (temp_dir().join(&name), temp_dir().join(&name));
        if src.join(".git").exists() {
            for d in [&a, &b] {
                let ok = Command::new("git")
                    .args(["clone", "-q", "--no-hardlinks"])
                    .arg(&src)
                    .arg(d)
                    .status()
                    .unwrap();
                assert!(ok.success());
            }
        } else {
            copy_tree(&src, &a);
            copy_tree(&src, &b);
        }
        let Ok(p) = load_valid(&b) else {
            continue;
        };
        for full in [false, true] {
            let mut args = vec!["build", "--html"];
            if full {
                args.push("--full");
            }
            let st = Command::new(ORACLE.as_str())
                .args(&args)
                .current_dir(&a)
                .output()
                .unwrap();
            let rel = if full { &p.cfg.out_full } else { &p.cfg.out };
            let Ok(want) = std::fs::read_to_string(a.join(format!("{rel}.html"))) else {
                fails.push(format!(
                    "{}: go wrote no HTML: {}",
                    src.display(),
                    String::from_utf8_lossy(&st.stderr)
                ));
                continue;
            };
            built += 1;
            if std::env::var_os("DOCC_HTML_DIRS").is_some() {
                eprintln!("{} (full {full}): {} bytes", src.display(), want.len());
            }
            match Renderer::new(&p, full).html() {
                Ok(got) => fails.extend(
                    compare_html(&got, &want)
                        .into_iter()
                        .map(|d| format!("{} (full {full}): {d}", src.display())),
                ),
                Err(e) => fails.push(format!("{}: {e}", src.display())),
            }
        }
        let _ = std::fs::remove_dir_all(a.parent().unwrap());
        let _ = std::fs::remove_dir_all(b.parent().unwrap());
    }
    eprintln!("{built} HTML files compared with Go");
    assert!(built > 40, "only {built} HTML files built");
    assert!(
        fails.is_empty(),
        "{} differences:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

/// dump_ast prints comrak's tree of snippet $ZZ_AST of $ZZ_IN (debugging aid).
#[test]
fn dump_ast() {
    let (Ok(input), Ok(i)) = (std::env::var("ZZ_IN"), std::env::var("ZZ_AST")) else {
        return;
    };
    let srcs: Vec<String> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let src = &srcs[i.parse::<usize>().unwrap()];
    let arena = comrak::Arena::new();
    let root = comrak::parse_document(&arena, src, &markdown::OPTIONS);
    let mut out = String::new();
    comrak::format_xml(root, &markdown::OPTIONS, &mut out).unwrap();
    eprintln!("{src:?}\n{out}");
}

/// build_html writes the project files into a new folder and returns its
/// plain HTML file and the data of it (the Go tests' do("build", "--html")
/// and readApp).
fn build_html(files: &[(&str, &str)]) -> (String, Value, PathBuf) {
    let dir = temp_dir().join("proj");
    // a document without a kind becomes a page
    let mut files: Vec<(String, String)> = files
        .iter()
        .map(|(p, c)| (p.to_string(), c.to_string()))
        .collect();
    let mut added = false;
    for (name, text) in files.iter_mut() {
        if !name.ends_with(".json") || !format!("/{name}").contains("/src/") {
            continue;
        }
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if m.contains_key("kind") {
            continue;
        }
        let i = text.rfind('}').unwrap();
        let head = text[..i].trim_end_matches([' ', '\t', '\r', '\n']);
        let sep = if head.ends_with('{') { "" } else { "," };
        *text = format!("{head}{sep}\"kind\":\"page\"{}", &text[head.len()..]);
        added = true;
    }
    if added && !files.iter().any(|(p, _)| p == "docs/schemas/page.json") {
        files.push((
            "docs/schemas/page.json".into(),
            r#"{"type":"object","additionalProperties":true}"#.into(),
        ));
    }
    for (p, c) in &files {
        let f = dir.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, c).unwrap();
    }
    let p = load_valid(&dir).unwrap();
    let html = Renderer::new(&p, false).html().unwrap();
    let data: Value = serde_json::from_str(island(&html)).unwrap();
    (html, data, dir)
}

/// blocks_html is readApp's Blocks: the HTML of every property and block.
fn blocks_html(data: &Value) -> String {
    let mut b = String::new();
    for p in data["pages"].as_array().unwrap() {
        for k in ["props", "blocks"] {
            for x in p[k].as_array().into_iter().flatten() {
                b.push_str(x["h"].as_str().unwrap());
                b.push('\n');
            }
        }
    }
    b
}

/// Blocks carry the JSON Pointer and
/// source text of their element; headings have none; a block marker written
/// by an agent is dropped like other raw HTML.
#[test]
fn html_blocks_have_pointers() {
    let (_, data, _) = build_html(&[(
        "docs/src/index.json",
        r#"{"title":"I","body":["One.","<!--b:0-->","Two."],"field":"Free text.","list":{"a":"x"}}"#,
    )]);
    let got: Vec<String> = data["pages"][0]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            format!(
                "{}={}",
                b["p"].as_str().unwrap_or(""),
                b["s"].as_str().unwrap_or("")
            )
        })
        .collect();
    assert_eq!(
        got.join(" "),
        "/body/0=One. /body/1=<!--b:0--> /body/2=Two. = /field=Free text. = /list=a: x"
    );
    let b = blocks_html(&data);
    assert!(
        b.contains("<!-- raw HTML omitted -->") && !b.contains("<!--b:"),
        "a marker in the text must be dropped:\n{b}"
    );
}

/// A page without body blocks has an
/// empty list in the data island, not null.
#[test]
fn html_page_without_blocks() {
    let (html, _, _) = build_html(&[
        ("docs/src/index.json", r#"{"title":"I","body":["One."]}"#),
        (
            "docs/src/empty.json",
            r#"{"title":"Empty","kind":"page","parent":"index"}"#,
        ),
    ]);
    assert!(
        !html.contains(r#""blocks":null"#),
        "blocks of an empty page must be [], not null"
    );
}

/// Footnote ids are prefixed by the page id.
#[test]
fn footnotes_in_html() {
    let (_, data, _) = build_html(&[
        (
            "docs/src/index.json",
            r#"{"title":"Index","body":["Claim[^1].","[^1]: Source one."]}"#,
        ),
        (
            "docs/src/p.json",
            r#"{"title":"P","parent":"index","body":["Other[^1].","[^1]: Source two."]}"#,
        ),
    ]);
    let b = blocks_html(&data);
    for want in [
        r#"id="index--fn:1""#,
        r##"href="#index--fn:1""##,
        r#"id="p--fn:1""#,
        r##"href="#p--fnref:1""##,
        "Source two.",
    ] {
        assert!(b.contains(want), "out.html lacks {want:?}");
    }
}

/// Pages in tree order with their parts as data,
/// field anchors and links, no raw HTML of the agents, no hidden pages.
#[test]
fn html_build() {
    let (html, data, dir) = build_html(&[
        (
            "docs/src/index.json",
            r#"{"title":"Index <b>","body":["See [[req#statement]] and [[req]].","<script>alert(1)</script> text"]}"#,
        ),
        (
            "docs/src/req.json",
            r#"{"title":"Req","kind":"requirement","status":"accepted","priority":2,"tags":["t1"],
			"parent":"index","statement":["Fact.","More."],"owner":"Team","stage":"alpha",
			"body":"Click <a id=\"x\" onclick=\"evil()\">here</a>.","rows":[{"a":"one\ntwo","b":"x"}]}"#,
        ),
        (
            "docs/src/h.json",
            r#"{"title":"Hid","hidden":true,"parent":"index","body":"Secret"}"#,
        ),
        (
            "docs/schemas/requirement.json",
            r#"{"additionalProperties":true,"x-docc-covered":true}"#,
        ),
    ]);
    let ps = data["pages"].as_array().unwrap();
    let s = |v: &Value| v.as_str().unwrap_or("").to_string();
    let list = |v: &Value| {
        v.as_array()
            .into_iter()
            .flatten()
            .map(|x| x.as_str().unwrap().to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    // pages in tree order (the page script links neighbours by it), with their parts as data
    assert!(
        ps.len() == 2
            && s(&ps[0]["id"]) == "index"
            && s(&ps[0]["title"]) == "Index <b>"
            && s(&ps[1]["id"]) == "req",
        "pages: {ps:?}"
    );
    let req = &ps[1];
    assert!(
        s(&req["kind"]) == "requirement"
            && s(&req["status"]) == "accepted"
            && s(&req["sc"]) == "ok"
            && s(&req["priority"]) == "2"
            && list(&req["tags"]) == "t1"
            && list(&req["crumbs"]) == "index"
            && s(&req["parent"]) == "index"
            && list(&ps[0]["kids"]) == "req",
        "req: {req:?}"
    );
    // one-line fields form a property list
    let props = req["props"].as_array().unwrap();
    assert!(
        props.len() == 2
            && s(&props[0]["k"]) == "Owner"
            && s(&props[0]["id"]) == "req--owner"
            && s(&props[0]["h"]) == "Team"
            && s(&props[0]["p"]) == "/owner",
        "props: {props:?}"
    );
    let b = blocks_html(&data);
    for want in [
        r#"<h2 id="req--statement">Statement</h2>"#, // target of [[req#statement]]
        r##"<a href="#req--statement">Req</a>"##,    // the link itself
        r##"<a href="#req">Req</a>"##,               // plain page link
        "<td>one<br>two</td>",                       // line break in a table cell survives
    ] {
        assert!(b.contains(want), "blocks lack {want:?}");
    }
    let i = html.find(ISLAND).unwrap();
    let shell = format!(
        "{}{}",
        &html[..i],
        &html[i + ISLAND.len() + island(&html).len()..]
    );
    for want in [
        "<title>Index &lt;b&gt;</title>",
        r#"type="search""#,
        "prefers-color-scheme",
        "@media print",
    ] {
        assert!(shell.contains(want), "out.html lacks {want:?}");
    }
    // agents' raw HTML never reaches the blocks; hidden documents stay out; nothing is fetched
    for bad in [
        "<script>alert",
        "onclick",
        "evil",
        ".md\"",
        "<h2 id=\"req--owner\"",
    ] {
        assert!(!b.contains(bad), "blocks contain {bad:?}");
    }
    for bad in [
        "Secret",
        "Hid",
        "<script src",
        "<link",
        "</script>alert",
        "<script>alert",
    ] {
        assert!(!html.contains(bad), "out.html contains {bad:?}");
    }
    // a second build gives the same file; the full one has the hidden document
    let p = load_valid(&dir).unwrap();
    assert_eq!(
        Renderer::new(&p, false).html().unwrap(),
        html,
        "the second build changed the file"
    );
    assert!(
        Renderer::new(&p, true).html().unwrap().contains("Secret"),
        "out-full.html lacks the hidden document"
    );
}

const BRAND_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2" fill="#fff"/></svg>"##;

/// brand_html is the HTML file with the branding parts of its data appended,
/// as the Go tests read it.
fn brand_html(html: &str, data: &Value) -> String {
    let b = &data["brand"];
    let s = |k: &str| b[k].as_str().unwrap_or("").to_string();
    format!("{html}{}{}{}", s("head"), s("print"), s("foot"))
}

/// The branding reaches the HTML file only.
#[test]
fn branding_in_html_only() {
    let (html, data, dir) = build_html(&[
        (
            "docc.json",
            r##"{"branding":{"name":"A & B","url":"https://example.com/x","logo":"docs/logo.svg",
			"accent":"#57e9c7","accentLight":"#0b7a64","header":"#1f1f1f"}}"##,
        ),
        ("docs/logo.svg", BRAND_SVG),
        ("docs/src/index.json", r#"{"title":"Index","body":"Text."}"#),
    ]);
    let html = brand_html(&html, &data);
    for want in [
        r#"<a class="brand" href="https://example.com/x"><span class="logo" role="img" aria-label="A &amp; B"></span></a>"#,
        r#"--brand-logo:url("data:image/svg+xml;base64,"#,
        "--brand-bg:#1f1f1f",
        "--accent:#0b7a64",
        "--accent:#57e9c7",
        r#"<div class="brand-print">A &amp; B</div>"#,
        r#"<footer class="brand-foot">A &amp; B · <a href="https://example.com/x">example.com</a></footer>"#,
    ] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
    assert_eq!(
        html.matches("--brand-logo:url(").count(),
        1,
        "the logo must be embedded once"
    );
    // the Go test reads docs/out/index.md; the Markdown of the single file is the same text
    let p = load_valid(&dir).unwrap();
    let md = Renderer::new(&p, false).single();
    assert!(!md.contains("A & B"), "Markdown is branded:\n{md}");
}

/// A name alone, no logo and no colours.
#[test]
fn branding_name_only() {
    let (html, data, _) = build_html(&[
        ("docc.json", r#"{"branding":{"name":"Acme"}}"#),
        ("docs/src/index.json", r#"{"title":"Index","body":"Text."}"#),
    ]);
    let html = brand_html(&html, &data);
    for want in [
        r#"<span class="brand">Acme</span>"#,
        r#"<footer class="brand-foot">Acme</footer>"#,
    ] {
        assert!(html.contains(want), "out.html lacks {want:?}");
    }
    assert!(
        html.contains(
            ":root{--brand-bg:#1f1f1f;--brand-line:var(--accent)}\n:root{--hbg:var(--brand-bg)"
        ) && !html.contains("--brand-logo:"),
        "no logo and no colours were given: only the default header colour is set"
    );
}

/// The Markdown cases the review of the port found (goldmark_cases.json has
/// them with the HTML of goldmark; these repeat the shape for the reader).
#[test]
fn review_markdown_cases() {
    for (md, want) in [
        // an indented ':' does not start a description
        (
            "Term\n  : indented definition\n",
            "<p>Term\n: indented definition</p>\n",
        ),
        // a link reference definition between two description lists ends the first
        (
            "A\n: a\n\n[x]: /u\n\nB\n: b\n",
            "<dl>\n<dt>A</dt>\n<dd>a</dd>\n</dl>\n<dl>\n<dt>B</dt>\n<dd>b</dd>\n</dl>\n",
        ),
        // linkify reads over ~ in a bare URL
        (
            "Docs: https://example.com/~team/page~2\n",
            "<p>Docs: <a href=\"https://example.com/~team/page~2\">https://example.com/~team/page~2</a></p>\n",
        ),
    ] {
        assert_eq!(convert_snippet(md), want, "{md:?}");
    }
}

/// The ids generator with a numbered heading whose number is not a section
/// number, inside lists and quotes: the number stays in the id.
#[test]
fn heading_ids_numbers_in_containers() {
    let got = convert_snippet(
        "- ## 1.2 Kept out

> ## 3.4 Stays

1. # 1.2 Kept out
",
    );
    assert_eq!(
        got,
        "<ul>\n<li>\n<h2 id=\"p--kept-out\">1.2 Kept out</h2>\n</li>\n</ul>\n<blockquote>\n<h2 id=\"p--34-stays\">3.4 Stays</h2>\n</blockquote>\n<ol>\n<li>\n<h1 id=\"p--kept-out-1\">1.2 Kept out</h1>\n</li>\n</ol>\n"
    );
}
