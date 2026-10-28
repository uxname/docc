use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn parse(s: &str) -> loader::Result {
    let r = loader::parse(s.as_bytes());
    assert!(r.err.is_none(), "{s}: {:?}", r.err);
    r
}

fn root(r: &loader::Result) -> &Node {
    r.root.as_ref().unwrap()
}

/// temp_dir is a new empty directory for one test.
fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "docc-schema-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn load_files(files: &[(&str, &str)]) -> Result<Set, LoadError> {
    let dir = temp_dir().join("schemas");
    for (name, content) in files {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }
    load(&dir, "docs/schemas", &crate::source::Source::Disk)
}

fn messages(vs: &[Violation]) -> String {
    vs.iter()
        .map(|v| format!("{} | {}", v.message, v.hint))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn test_builtin() {
    let s = load_files(&[]).unwrap();
    let ok = parse(
        r#"{"title":"A","kind":"note","tags":["a","b"],"body":["x","y"],"hidden":true,"free":{"any":1}}"#,
    );
    let vs = s.check_builtin(root(&ok));
    assert!(vs.is_empty(), "unexpected: {vs:?}");

    let cases = [
        (
            "dup tag pos",
            "{\n\"title\":\"A\",\n\"tags\":[\"b\",\"c\",\"b\"]}",
            "items at 0 and 2",
            "3:17",
        ),
        (
            "dup tag",
            r#"{"title":"A","tags":["x","x"]}"#,
            "equal",
            "1:26",
        ),
        ("title type", r#"{"title":5}"#, "/title", "1:10"),
        (
            "kind chars",
            r#"{"title":"A","kind":"Bad Kind"}"#,
            "/kind",
            "1:21",
        ),
        (
            "body shape",
            r#"{"title":"A","body":5}"#,
            "does not match any allowed shape",
            "1:21",
        ),
        (
            "parent type",
            r#"{"title":"A","parent":["x"]}"#,
            "/parent",
            "1:23",
        ),
        (
            "hidden type",
            r#"{"title":"A","hidden":"yes"}"#,
            "/hidden",
            "1:23",
        ),
    ];
    for (name, input, want, at) in cases {
        let r = parse(input);
        let vs = s.check_builtin(root(&r));
        assert_eq!(vs.len(), 1, "{name}: got {} violations: {vs:?}", vs.len());
        assert!(
            vs[0].message.contains(want),
            "{name}: message {:?} lacks {want:?}",
            vs[0].message
        );
        let (l, col) = r.src.position(vs[0].start);
        assert_eq!(format!("{l}:{col}"), at, "{name}: position");
    }
}

#[test]
fn test_blank_title_left_to_doc103() {
    let s = load_files(&[]).unwrap();
    for input in [r#"{}"#, r#"{"title":""}"#, r#"{"title":"  "}"#] {
        let r = parse(input);
        let vs = s.check_builtin(root(&r));
        assert!(vs.is_empty(), "{input}: {vs:?}");
    }
}

#[test]
fn test_kind_schema() {
    let s = load_files(&[
        (
            "requirement.json",
            r#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object",
		  "required":["statement"],"properties":{"status":{"enum":["draft","accepted"]},"statement":{"$ref":"common.json#/$defs/text"}},
		  "additionalProperties":false}"#,
        ),
        ("common.json", r#"{"$defs":{"text":{"type":"string","minLength":3}},"additionalProperties":true}"#),
    ])
    .unwrap();
    assert!(
        s.has_kind("requirement") && s.has_kind("common"),
        "kinds loaded"
    );
    let doc =
        parse("{\n\"title\":\"A\",\"kind\":\"requirement\",\n\"status\":\"wip\",\"extra\":1}");
    let vs = s.check_kind("requirement", root(&doc));
    let joined = messages(&vs);
    for w in [
        r#"missing property "statement""#,
        "/status",
        r#"unknown field "extra""#,
    ] {
        assert!(joined.contains(w), "missing {w:?} in:\n{joined}");
    }
    for v in &vs {
        if v.message.contains(r#""extra""#) {
            assert_eq!(doc.src.position(v.start), (3, 16), "extra key position");
        }
    }
    let ok = parse(
        r#"{"title":"A","kind":"requirement","status":"draft","statement":"abcd","notes":["n"],"hidden":true,
		"parent":"x","tags":["t"],"body":"b","order":2,"docIgnore":[]}"#,
    );
    let vs = s.check_kind("requirement", root(&ok));
    assert!(
        vs.is_empty(),
        "service fields must be allowed by a closed kind: {vs:?}"
    );
}

// The allowed names come from properties, allOf branches and local $ref
// targets (followed recursively), not from if/then.
#[test]
fn test_allowed_names() {
    let s = load_files(&[
        (
            "req.json",
            r#"{"properties":{"a":{}},"allOf":[{"properties":{"b":{}}},{"$ref":"parts/c.json"}],
			"if":{"required":["a"]},"then":{"properties":{"hidden-x":{}}},"additionalProperties":false}"#,
        ),
        ("parts/c.json", r#"{"properties":{"c":{"x-docc-target":["goal"]}},"$ref":"d.json"}"#),
        (
            "parts/d.json",
            r##"{"properties":{"d":{}},"allOf":[{"$ref":"#/$defs/e"}],"$defs":{"e":{"properties":{"e":{}}}}}"##,
        ),
    ])
    .unwrap();
    let k = s.kind("req").unwrap();
    for n in ["a", "b", "c", "d", "e"] {
        assert!(k.allowed.contains(n), "{n} is not allowed: {:?}", k.allowed);
    }
    assert!(
        !k.allowed.contains("hidden-x"),
        "if/then properties must not count"
    );
    assert_eq!(k.targets("c"), ["goal"], "targets of c");
    let r = parse(r#"{"title":"T","a":1,"b":1,"c":1,"d":1,"e":1,"hidden-x":1}"#);
    let vs = s.check_kind("req", root(&r));
    assert!(
        vs.len() == 1 && vs[0].message.contains(r#"unknown field "hidden-x""#),
        "{}",
        messages(&vs)
    );
    // a $ref cycle does not hang the walk
    let _ = load_files(&[(
        "loop.json",
        r##"{"allOf":[{"$ref":"#"}],"$ref":"loop.json","additionalProperties":true}"##,
    )]);
}

// A typo in a closed kind is reported once with the closest name.
#[test]
fn test_unknown_field_suggestion() {
    let s = load_files(&[
        (
            "req.json",
            r#"{"properties":{"statement":{"type":"string"}},"additionalProperties":false,"x-docc-covered":true}"#,
        ),
        ("open.json", r#"{"additionalProperties":true}"#),
    ])
    .unwrap();
    assert!(
        s.kind("req").unwrap().covered && !s.kind("open").unwrap().covered,
        "x-docc-covered"
    );
    let f = load_files(&[(
        "f.json",
        r#"{"properties":{"a":{"x-docc-covered":true},"b":{"x-docc-covered":false},"c":{}},"additionalProperties":true}"#,
    )])
    .unwrap();
    let k = f.kind("f").unwrap();
    assert!(!k.covered && k.covered_fields.iter().eq(["a"]), "covered fields: {:?}", k.covered_fields);
    let r = parse(r#"{"title":"T","statment":"x","zzzzzzzz":1,"parnt":"p"}"#);
    let vs = s.check_kind("req", root(&r));
    let joined = messages(&vs);
    assert!(
        vs.len() == 3
            && joined.contains(r#"Did you mean "statement"?"#)
            && joined.contains(r#"Did you mean "parent"?"#)
            && joined.matches("Did you mean").count() == 2,
        "{joined}"
    );
    let r = parse(r#"{"title":"T","statment":"x"}"#);
    let vs = s.check_kind("open", root(&r));
    assert!(vs.is_empty(), "open kind: {}", messages(&vs));
}

// A failing allOf branch must not turn its properties into unknown fields:
// the document missing "parent" gets exactly one error.
#[test]
fn test_closed_kind_no_cascade() {
    let s = load_files(&[(
        "req.json",
        r#"{"type":"object","allOf":[{"required":["parent"],"properties":{"statement":{"type":"string"},"status":{"enum":["draft"]}}}],
			"additionalProperties":false}"#,
    )])
    .unwrap();
    let r = parse(r#"{"title":"T","statement":"s","status":"draft","notes":["n"]}"#);
    let vs = s.check_kind("req", root(&r));
    assert!(
        vs.len() == 1 && vs[0].message.contains(r#"missing property "parent""#),
        "{}",
        messages(&vs)
    );
    let r = parse(r#"{"title":"T","parent":"p","statement":"s","status":"wip"}"#);
    let vs = s.check_kind("req", root(&r));
    assert!(
        vs.len() == 1 && vs[0].message.contains("/status"),
        "status restricted: {}",
        messages(&vs)
    );
}

#[test]
fn test_additional_properties_required() {
    let cases = [
        ("missing", r#"{"type":"object"}"#),
        (
            "not boolean",
            r#"{"additionalProperties":{"type":"string"}}"#,
        ),
        (
            "bad target",
            r#"{"properties":{"p":{"x-docc-target":"goal"}},"additionalProperties":true}"#,
        ),
        (
            "bad covered",
            r#"{"x-docc-covered":"yes","additionalProperties":true}"#,
        ),
        (
            "bad field covered",
            r#"{"properties":{"p":{"x-docc-covered":1}},"additionalProperties":true}"#,
        ),
    ];
    for (name, content) in cases {
        let Err(err) = load_files(&[("a.json", content)]) else {
            panic!("{name}: expected error");
        };
        if !name.starts_with("bad ") {
            assert!(
                err.to_string().contains("additionalProperties"),
                "{name}: {err}"
            );
        }
    }
    let r = load_files(&[
        ("a.json", r#"{"additionalProperties":true}"#),
        ("sub/b.json", r#"{"type":"string"}"#),
    ]);
    assert!(
        r.is_ok(),
        "a file in a subfolder is not a kind: {:?}",
        r.err()
    );
}

#[test]
fn test_bad_schema_files() {
    let cases = [
        ("invalid json", r#"{"#),
        ("bad schema", r#"{"type":"nonsense"}"#),
        ("remote ref", r#"{"$ref":"https://example.com/x.json"}"#),
        ("outside ref", r#"{"$ref":"../x.json"}"#),
        ("missing ref", r#"{"$ref":"nope.json"}"#),
    ];
    for (name, content) in cases {
        assert!(
            load_files(&[("a.json", content)]).is_err(),
            "{name}: expected error"
        );
    }
}

#[test]
fn test_resolve_pointer() {
    let r = parse(r#"{"a":[{"b/c":"x"}],"d":1}"#);
    let n = resolve_pointer(root(&r), "/a/0/b~1c");
    assert!(n.is_some_and(|n| n.str == "x"), "nested");
    assert!(
        resolve_pointer(root(&r), "/a/1").is_none() && resolve_pointer(root(&r), "a").is_none(),
        "should be nil"
    );
    assert!(
        std::ptr::eq(resolve_pointer(root(&r), "").unwrap(), root(&r)),
        "root"
    );
}

/// compile_bytes compiles a schema the way Load compiles a kind schema.
fn compile_bytes(b: &[u8]) -> Validator {
    let r = loader::parse(b);
    assert!(r.err.is_none(), "{:?}", r.err);
    let url = "docc://t/x.json";
    let mut l = MemLoader::default();
    l.insert(url.to_string(), loader::value(r.root.as_ref().unwrap()));
    let l = Arc::new(l);
    compile(&l, url).unwrap_or_else(|e| panic!("{e}"))
}

fn value_of(s: &str) -> Value {
    let r = loader::parse(s.as_bytes());
    assert!(r.err.is_none(), "{s}: {:?}", r.err);
    loader::value(r.root.as_ref().unwrap())
}

const SPEC_CONFIG: &str = r#"{
  "src": "docs/src", "out": "docs/out", "schemas": "docs/schemas", "outFull": "docs/out-full",
  "roots": ["index"], "tags": ["draft"], "recommended": true,
  "rules": {
    "text/forbidden-words": { "level": "warn", "words": ["a"], "allow": [] },
    "text/repeated-word": { "level": "warn", "allow": ["that"] },
    "markdown/raw-html": "off", "style": "warn", "custom/x": "off", "DOC800": "off"
  },
  "customRules": [{ "id": "custom/no-vague", "level": "warn", "kind": "requirement", "given": "/statement",
    "forbid": "x", "message": "m", "hint": "h" }],
  "trace": { "scan": ["tests/**"] }, "maxWarnings": 3
}"#;

#[test]
fn test_config_schema() {
    let sc = compile_bytes(&config());
    let ok = value_of(SPEC_CONFIG);
    assert!(
        sc.validate(&ok).is_ok(),
        "spec example must validate: {:?}",
        sc.validate(&ok).err()
    );
    let cases = [
        ("unknown key", r#"{"foo":1}"#),
        ("bad level", r#"{"rules":{"DOC301":"loud"}}"#),
        ("unknown rule", r#"{"rules":{"nonsense":"off"}}"#),
        ("retired rule", r#"{"rules":{"DOC302":"off"}}"#),
        ("nursery", r#"{"rules":{"nursery":"warn"}}"#),
        ("overrides", r#"{"overrides":[]}"#),
        ("kinds", r#"{"kinds":["note"]}"#),
        ("schema key", r#"{"$schema":"x"}"#),
        ("max param", r#"{"rules":{"DOC301":{"max":1}}}"#),
        ("empty roots", r#"{"roots":[]}"#),
        (
            "both rx",
            r#"{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a","require":"b"}]}"#,
        ),
        (
            "bad custom id",
            r#"{"customRules":[{"id":"a","level":"warn","message":"m","given":"/x","forbid":"a"}]}"#,
        ),
    ];
    for (name, bad) in cases {
        assert!(sc.validate(&value_of(bad)).is_err(), "{name}: should fail");
    }
}

#[test]
fn test_document_schema_is_valid() {
    let sc = compile_bytes(document());
    let v = value_of(r#"{"title":"A","docIgnore":[{"rule":"DOC301","reason":"x"}]}"#);
    assert!(sc.validate(&v).is_ok(), "{:?}", sc.validate(&v).err());
}

#[test]
fn test_format_is_an_assertion() {
    let s = load_files(&[(
        "event.json",
        r#"{"properties":{"due":{"type":"string","format":"date"}},"additionalProperties":true}"#,
    )])
    .unwrap();
    let r = parse(r#"{"title":"A","due":"31.12.2026"}"#);
    let vs = s.check_kind("event", root(&r));
    assert_eq!(vs.len(), 1, "bad date: {vs:?}");
    let r = parse(r#"{"title":"A","due":"2026-12-31"}"#);
    let vs = s.check_kind("event", root(&r));
    assert!(vs.is_empty(), "good date: {vs:?}");
}

/// The printed docc.json schema: first the Go output byte for byte, since
/// DOC902 and the name-first description its own.
#[test]
fn config_matches_go() {
    let want = include_str!("go_config.json");
    assert_eq!(String::from_utf8(config()).unwrap(), want);
}

/// Every case of go_cases.json (kind schemas, a document and what the Go
/// code reports for them, written by running the Go package) gives the same
/// violations, positions and hints here. Texts the Rust validator words
/// differently (docs/rust-port/schema.md) are compared up to that text; a case
/// with "rust" is a declared deviation ("deviation" names it) and gives that.
#[test]
fn violations_match_go() {
    #[derive(serde::Deserialize)]
    struct Case {
        files: std::collections::BTreeMap<String, String>,
        doc: String,
        kind: String,
        want: Vec<String>,
        #[serde(default)]
        rust: Option<Vec<String>>,
    }
    let cases: Vec<Case> = serde_json::from_str(include_str!("go_cases.json")).unwrap();
    let mut bad = Vec::new();
    for c in &cases {
        let files: Vec<(&str, &str)> = c
            .files
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let got = match load_files(&files) {
            Err(e) => vec![format!("ERR: {e}")],
            Ok(s) => {
                let r = loader::parse(c.doc.as_bytes());
                let root = r.root.as_ref().unwrap();
                let vs = if c.kind.is_empty() {
                    s.check_builtin(root)
                } else {
                    s.check_kind(&c.kind, root)
                };
                vs.iter()
                    .map(|v| {
                        format!(
                            "{} | {} | {} | {}-{}",
                            v.pointer, v.message, v.hint, v.start, v.end
                        )
                    })
                    .collect()
            }
        };
        // the reason a schema does not compile is worded by the validator, the
        // reason a regular expression is bad partly by the regex crate
        let cut = |s: &str| -> String {
            ["cannot compile schema: ", "is not valid regex: "]
                .iter()
                .find_map(|m| s.split_once(m).map(|p| format!("{}{m}", p.0)))
                .unwrap_or_else(|| s.to_string())
        };
        let want = c.rust.as_ref().unwrap_or(&c.want);
        let same = want.len() == got.len() && want.iter().zip(&got).all(|(w, g)| cut(w) == cut(g));
        if std::env::var("SHOW_COMPILE_ERRORS").is_ok()
            && c.want.iter().any(|w| w.starts_with("ERR: "))
        {
            eprintln!("want {:?}\ngot  {:?}\n", c.want, got);
        }
        if !same {
            bad.push(format!(
                "files {:?}\ndoc {}\n  want {:#?}\n  got  {:#?}",
                c.files, c.doc, c.want, got
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} cases differ:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

/// Numbers print as Go's fmt %v and the English message printer print them.
#[test]
fn numbers_match_go() {
    let printer = [
        (0.0, "0"),
        (3.0, "3"),
        (-3.0, "-3"),
        (0.3, "0.3"),
        (1500.5, "1,500.5"),
        (-1500.5, "-1,500.5"),
        (123456.5, "123,456.5"),
        (999999.0, "999,999"),
        (1000000.0, "1\u{202f}×\u{202f}10⁰⁶"),
        (12345678.25, "1.234567825\u{202f}×\u{202f}10⁰⁷"),
        (0.0001, "0.0001"),
        (0.00001, "1\u{202f}×\u{202f}10⁻⁰⁵"),
        (-0.000012345, "-1.2345\u{202f}×\u{202f}10⁻⁰⁵"),
        (1e21, "1\u{202f}×\u{202f}10²¹"),
        (1e300, "1\u{202f}×\u{202f}10³⁰⁰"),
        (5e-324, "5\u{202f}×\u{202f}10⁻³²⁴"),
        (
            1.7976931348623157e308,
            "1.7976931348623157\u{202f}×\u{202f}10³⁰⁸",
        ),
        (100000.25, "100,000.25"),
        (2.5e-7, "2.5\u{202f}×\u{202f}10⁻⁰⁷"),
        (0.1234567890123, "0.1234567890123"),
        (
            123456789012345678.0,
            "1.2345678901234568\u{202f}×\u{202f}10¹⁷",
        ),
    ];
    for (f, want) in printer {
        assert_eq!(texts::printer_float(f), want, "{f}");
    }
    let ints = [
        (0, "0"),
        (7, "7"),
        (999, "999"),
        (1000, "1,000"),
        (-1000, "-1,000"),
        (1234567, "1,234,567"),
        (-12, "-12"),
    ];
    for (i, want) in ints {
        assert_eq!(texts::printer_int(i), want, "{i}");
    }
    let v = [
        (1234567.0, "1.234567e+06"),
        (123456.5, "123456.5"),
        (1e20, "1e+20"),
        (1e21, "1e+21"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (1.5, "1.5"),
        (-2.5e-7, "-2.5e-07"),
        (123456789.0, "1.23456789e+08"),
        (100000.0, "100000"),
        (1000000.0, "1e+06"),
    ];
    for (f, want) in v {
        assert_eq!(texts::go_v_float(f), want, "{f}");
    }
}
