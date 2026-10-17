use super::*;
use crate::diag::Level;

fn temp_dir() -> PathBuf {
    std::env::temp_dir()
}

fn must_parse(s: &str) -> Config {
    parse(s.as_bytes(), &temp_dir(), "docc.json").unwrap_or_else(|e| panic!("{s}: {e}"))
}

#[test]
fn defaults() {
    let c = must_parse("{}");
    let r = c.resolve();
    assert!(
        r.rule("DOC301").level == Level::Warn && r.rule("DOC101").level == Level::Error,
        "preset levels wrong"
    );
    assert!(c.src == "docs/src" && c.roots[0] == "index", "defaults");
}

#[test]
fn precedence() {
    let c = must_parse(
        r#"{"rules":{"style":"error","DOC353":"off","text/forbidden-words":{"level":"error","words":["x"]}}}"#,
    );
    let r = c.resolve();
    assert_eq!(r.rule("DOC352").level, Level::Error, "group"); // group beats preset
    assert_eq!(r.rule("DOC353").level, Level::Off, "rule"); // rule beats group
    let s = r.rule("DOC301");
    assert!(
        s.level == Level::Error && s.params.words.len() == 1,
        "params {s:?}"
    );
}

#[test]
fn recommended_false() {
    let c = must_parse(r#"{"recommended":false}"#);
    let r = c.resolve();
    assert!(
        r.rule("DOC301").level == Level::Off && r.rule("DOC201").level == Level::Error,
        "recommended=false"
    );
}

#[test]
fn group_does_not_touch_core() {
    let c = must_parse(r#"{"rules":{"syntax":"off","graph":"off"}}"#);
    let r = c.resolve();
    assert!(
        r.rule("DOC101").level == Level::Error && r.rule("DOC201").level == Level::Error,
        "core changed by group"
    );
    assert_eq!(
        r.rule("DOC214").level,
        Level::Off,
        "non-core graph rule should be off"
    );
}

#[test]
fn errors() {
    let cases = [
        ("unknown key", r#"{"foo":1}"#),
        ("bad type", r#"{"src":1}"#),
        ("abs path", r#"{"src":"/etc"}"#),
        ("empty roots", r#"{"roots":[]}"#),
        ("core by rule", r#"{"rules":{"DOC101":"off"}}"#),
        (
            "both code+name",
            r#"{"rules":{"DOC301":"off","text/forbidden-words":"warn"}}"#,
        ),
        ("unknown rule", r#"{"rules":{"nope":"off"}}"#),
        ("bad level", r#"{"rules":{"DOC301":"loud"}}"#),
        ("bad param", r#"{"rules":{"DOC301":{"max":1}}}"#),
        ("group object", r#"{"rules":{"text":{"level":"off"}}}"#),
        ("dup key", r#"{"src":"a","src":"b"}"#),
        (
            "bad regexp",
            r#"{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"("}]}"#,
        ),
        (
            "forbid+require",
            r#"{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a","require":"b"}]}"#,
        ),
        (
            "dup custom",
            r#"{"customRules":[{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a"},{"id":"custom/a","level":"warn","message":"m","given":"/x","forbid":"a"}]}"#,
        ),
        (
            "bad custom id",
            r#"{"customRules":[{"id":"x/a","level":"warn","message":"m","given":"/x","forbid":"a"}]}"#,
        ),
        ("unknown custom", r#"{"rules":{"custom/zzz":"off"}}"#),
        ("bad max warnings", r#"{"maxWarnings":-1}"#),
        ("overrides", r#"{"overrides":[]}"#),
        ("kinds", r#"{"kinds":["note"]}"#),
        ("schema key", r#"{"$schema":"x"}"#),
        ("nursery group", r#"{"rules":{"nursery":"warn"}}"#),
        ("retired rule", r#"{"rules":{"DOC302":"off"}}"#),
        (
            "words file",
            r#"{"rules":{"DOC301":{"wordsFile":"w.txt"}}}"#,
        ),
    ];
    for (name, input) in cases {
        match parse(input.as_bytes(), &temp_dir(), "docc.json") {
            Ok(_) => panic!("{name}: expected error"),
            Err(err) => assert!(err.to_string().contains("config error"), "{name}: {err}"),
        }
    }
}

#[test]
fn error_position() {
    let err = parse(b"{\n  \"src\": 1\n}", &temp_dir(), "docc.json")
        .err()
        .expect("error");
    assert_eq!(
        err.to_string(),
        "docc.json:2:10: config error: expected a string"
    );
}

#[test]
fn custom_rules() {
    let c = must_parse(
        r#"{"customRules":[{"id":"custom/a","level":"error","message":"m","given":"/x","forbid":"q"}],
	  "rules":{"custom/a":"warn"}}"#,
    );
    assert_eq!(
        c.resolve().custom("custom/a").level,
        Level::Warn,
        "custom level override"
    );
    let c = must_parse(
        r#"{"customRules":[{"id":"custom/a","level":"error","message":"m","given":"/x","forbid":"q"}],
	  "rules":{"DOC800":"off"}}"#,
    );
    assert_eq!(
        c.resolve().custom("custom/a").level,
        Level::Off,
        "DOC800 switches all custom rules"
    );
}

// The tests below are not in Go: they pin the Rust stand-ins for Go library
// behaviour (net/url, %q, error texts) to values printed by Go.

#[test]
fn several_errors_in_file_order() {
    let err = parse(
        br#"{"foo":1,"lang":"de","words":[]}"#,
        &temp_dir(),
        "docc.json",
    )
    .err()
    .unwrap();
    assert_eq!(
        err.to_string(),
        "docc.json:1:2: config error: unknown key \"foo\"\n\
         docc.json:1:17: config error: lang must be \"en\" or \"ru\"\n\
         docc.json:1:22: config error: unknown key \"words\""
    );
}

#[test]
fn messages_match_go() {
    let cases = [
        (
            r#"{"src":"a","src":"b"}"#,
            "docc.json:1:12: config error: duplicate key \"src\"",
        ),
        (
            r#"{"rules":{"DOC101":"off"}}"#,
            "docc.json:1:11: config error: DOC101 is a core rule and cannot be configured",
        ),
        (
            r#"{"rules":{"DOC301":{"words":["a*b","c","d*e"]}}}"#,
            "docc.json:1:29: config error: \"d*e\": * is allowed only at the end of a word",
        ),
        (
            r#"{"maxWarnings":1.0}"#,
            "docc.json:1:16: config error: expected a non-negative integer",
        ),
        (
            r#"{"branding":{"url":"x"}}"#,
            "docc.json:1:20: config error: url must be an http or https address",
        ),
        (
            r#"{"branding":{}}"#,
            "docc.json:1:13: config error: branding needs a name",
        ),
        (
            r##"{"branding":{"name":"n","accent":"#12"}}"##,
            "docc.json:1:34: config error: accent must be a colour like #57e9c7",
        ),
        (
            r#"{"branding":{"name":"n","logo":"a.txt"}}"#,
            "docc.json:1:32: config error: logo must be a png, jpg, jpeg, gif, svg or webp file",
        ),
        (
            r#"{"branding":{"name":"n","logo":"nope/x.PNG"}}"#,
            "docc.json:1:32: config error: logo file \"nope/x.PNG\" does not exist",
        ),
        (
            r#"{"trace":{"x":1}}"#,
            "docc.json:1:11: config error: unknown key \"x\"",
        ),
        (
            r#"{"src":"a/../.."}"#,
            "docc.json:1:8: config error: path must be relative to the project root and inside it",
        ),
        (
            "{\"x\u{a0}\\n\":1}",
            "docc.json:1:2: config error: unknown key \"x\\u00a0\\n\"",
        ),
    ];
    for (input, want) in cases {
        let err = parse(input.as_bytes(), &temp_dir(), "docc.json")
            .err()
            .unwrap_or_else(|| panic!("{input}: no error"));
        assert_eq!(err.to_string(), want, "{input}");
    }
}

#[test]
fn values() {
    let c = must_parse(
        r##"{"src":"./a//b/","maxWarnings":0,"tags":[],"lang":"ru","trace":{"scan":["src/**"]},"branding":{"name":"N","url":"HTTPS://x.org","accent":"#aBc"}}"##,
    );
    assert_eq!(c.src, "a/b");
    assert!(c.has_max_warning && c.max_warnings == 0 && c.has_tags && c.lang == "ru");
    assert!(match_any(&c.trace_scan, "src/x/y.go"));
    assert_eq!(c.brand.as_ref().unwrap().accent, "#aBc");
    let err = parse(b"[]", &temp_dir(), "docc.json").err().unwrap();
    assert_eq!(
        err.to_string(),
        "docc.json:1:1: config error: the top-level value must be an object"
    );
}

#[test]
fn load_missing() {
    let dir = temp_dir().join(format!("docc-config-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("docc.json");
    let f = file.to_str().unwrap();
    let c = load(f, false).unwrap();
    assert_eq!(c.root, dir);
    let err = load(f, true).err().unwrap().to_string();
    assert_eq!(
        err,
        format!("{f}: config error: open {f}: no such file or directory")
    );
    let d = dir.to_str().unwrap();
    let err = load(d, true).err().unwrap().to_string();
    assert_eq!(err, format!("{d}: config error: read {d}: is a directory"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn http_url_matches_go() {
    // expected values printed by Go's url.Parse check in branding
    let cases = [
        ("https://example.com", true),
        ("http://example.com/a?b#c", true),
        ("HTTP://X", true),
        ("http://", false),
        ("http:example.com", false),
        ("http:/x", false),
        ("ftp://x", false),
        ("https://x:8080", true),
        ("https://x:", true),
        ("https://x:99999", true),
        ("https://x:a", false),
        ("https://a:1:2", false),
        ("https://[::1]:80/", true),
        ("https://[1.2.3.4]/", false),
        ("https://[::1", false),
        ("https://[fe80::1%25en0]/", true),
        ("https://[fe80::1%25]/", false),
        ("https://u:p@x", true),
        ("https://u@", false),
        ("https://u p@x", false),
        ("https://ex ample.com", false),
        ("https://x/%zz", false),
        ("https://x/#%zz", false),
        ("https://x/a b", true),
        ("https://x%41", false),
        ("https://x%C3%A9", true),
        ("https://x\n", false),
        ("https://x/#\n", true),
        ("https://пример.рф", true),
        ("https://x?a=%zz", true),
        ("https://a@b@c", true),
        ("https://x/?", true),
        ("1http://x", false),
        ("https://u:%zz@x", false),
        ("https://u:a:%zz@x", false),
        ("//x", false),
        ("https://[::ffff:1.2.3.4]", true),
        ("https://a[b]", false),
        ("https://x<y", true),
        ("https://x|y", false),
    ];
    for (s, want) in cases {
        assert_eq!(http_url_ok(s), want, "{s:?}");
    }
}

#[test]
fn go_quote_matches_go() {
    let cases = [
        ("a\u{a0}b", "\"a\x5cu00a0b\""),
        ("x\u{200b}y", "\"x\x5cu200by\""),
        ("é", "\"é\""),
        ("a\u{301}", "\"a\u{301}\""),
        ("\u{301}", "\"\u{301}\""),
        ("\x01", r#""\x01""#),
        ("😀", "\"😀\""),
        ("\u{2028}", "\"\x5cu2028\""),
        ("\u{ad}", "\"\x5cu00ad\""),
    ];
    for (s, want) in cases {
        assert_eq!(go_quote(s), want, "{s:?}");
    }
}

#[test]
fn path_helpers() {
    for (p, want) in [
        ("", "."),
        ("a/./b/", "a/b"),
        ("../a", "../a"),
        ("a/../..", ".."),
        ("/../a", "/a"),
        ("a/b/../c", "a/c"),
    ] {
        assert_eq!(path_clean(p), want, "{p}");
    }
    assert_eq!(path_ext("a/b.c/d"), "");
    assert_eq!(path_ext("a/x.PNG"), ".PNG");
    assert_eq!(clean_os(Path::new("/a/../../b/./c")), PathBuf::from("/b/c"));
}
