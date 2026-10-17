//! Package report prints diagnostics as text or JSON.

use std::fmt::Write as _;
use std::io::{self, Write};

use serde::Serialize;

use crate::diag::{self, Diagnostic};

/// Counts returns the number of errors and warnings.
pub fn counts(ds: &[Diagnostic]) -> (usize, usize) {
    let (mut errors, mut warnings) = (0, 0);
    for d in ds {
        match d.lvl {
            diag::Level::Error => errors += 1,
            diag::Level::Warn => warnings += 1,
            diag::Level::Off => {}
        }
    }
    (errors, warnings)
}

fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        return format!("{n} {one}");
    }
    format!("{n} {one}s")
}

/// limit cuts ds to max items; max == 0 means no limit.
fn limit(ds: &[Diagnostic], max: usize) -> &[Diagnostic] {
    if max > 0 && ds.len() > max {
        return &ds[..max];
    }
    ds
}

/// Text writes `file:line:col: level CODE (rule) message`, a hint line per
/// diagnostic, a truncation banner when max > 0 cuts the list, and a summary
/// of the checked files.
pub fn text(w: &mut dyn Write, ds: &[Diagnostic], max: usize, checked: usize) -> io::Result<()> {
    let mut b = String::new();
    let shown = limit(ds, max);
    for d in shown {
        let _ = writeln!(
            b,
            "{}:{}:{}: {} {} ({}) {}",
            d.file, d.line, d.column, d.level, d.rule, d.code, d.message
        );
        if !d.hint.is_empty() {
            let _ = writeln!(b, "  hint: {}", d.hint);
        }
    }
    if shown.len() < ds.len() {
        let _ = writeln!(b, "showing {} of {}", shown.len(), ds.len());
    }
    let (e, wn) = counts(ds);
    let _ = writeln!(
        b,
        "checked {}: {}, {}",
        plural(checked, "file"),
        plural(e, "error"),
        plural(wn, "warning")
    );
    if !ds.is_empty() {
        b.push_str("Run docc explain <rule> for details\n");
    }
    w.write_all(b.as_bytes())
}

/// Summary is the "summary" object of the JSON report.
#[derive(Debug, Serialize)]
pub struct Summary {
    pub checked: usize,
    pub errors: usize,
    pub warnings: usize,
    pub shown: usize,
    pub truncated: bool,
    /// why the exit code is 1 without errors
    #[serde(skip_serializing_if = "String::is_empty")]
    pub failure: String,
}

/// JSON writes {"summary": ..., "diagnostics": [...]}; max > 0 cuts the list.
pub fn json(
    w: &mut dyn Write,
    ds: &[Diagnostic],
    max: usize,
    checked: usize,
    failure: &str,
) -> io::Result<()> {
    #[derive(Serialize)]
    struct Out<'a> {
        summary: Summary,
        diagnostics: &'a [Diagnostic],
    }
    let shown = limit(ds, max);
    let (e, wn) = counts(ds);
    let out = Out {
        summary: Summary {
            checked,
            errors: e,
            warnings: wn,
            shown: shown.len(),
            truncated: shown.len() < ds.len(),
            failure: failure.to_string(),
        },
        diagnostics: shown,
    };
    // Like Go's encoder with SetEscapeHTML(false): "<", ">" and "&" stay
    // readable in messages, but U+2028 and U+2029 are still escaped. They can
    // only occur inside strings, so a plain replace is safe.
    let s = serde_json::to_string_pretty(&out)?
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(lvl: diag::Level, msg: &str) -> Diagnostic {
        Diagnostic {
            code: "DOC301".into(),
            rule: "text/forbidden-words".into(),
            level: lvl.to_string(),
            file: "docs/src/a.json".into(),
            line: 2,
            column: 3,
            end_line: 2,
            end_column: 7,
            message: msg.into(),
            lvl,
            ..Default::default()
        }
    }

    #[test]
    fn text_and_json() {
        let ds = [diag(diag::Level::Warn, "a <b> & c\u{2028}"), diag(diag::Level::Error, "x")];
        let mut out = Vec::new();
        text(&mut out, &ds, 1, 1).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "docs/src/a.json:2:3: warn text/forbidden-words (DOC301) a <b> & c\u{2028}\n\
             showing 1 of 2\n\
             checked 1 file: 1 error, 1 warning\n\
             Run docc explain <rule> for details\n"
        );
        let mut out = Vec::new();
        json(&mut out, &ds, 1, 3, "").unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#"{
  "summary": {
    "checked": 3,
    "errors": 1,
    "warnings": 1,
    "shown": 1,
    "truncated": true
  },
  "diagnostics": [
    {
      "code": "DOC301",
      "rule": "text/forbidden-words",
      "level": "warn",
      "file": "docs/src/a.json",
      "line": 2,
      "column": 3,
      "endLine": 2,
      "endColumn": 7,
      "message": "a <b> & c\u2028",
      "hint": ""
    }
  ]
}
"#
        );
        let mut out = Vec::new();
        json(&mut out, &[], 0, 0, "why").unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\n  \"summary\": {\n    \"checked\": 0,\n    \"errors\": 0,\n    \"warnings\": 0,\n    \"shown\": 0,\n    \"truncated\": false,\n    \"failure\": \"why\"\n  },\n  \"diagnostics\": []\n}\n"
        );
    }
}
