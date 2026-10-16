//! Package diag defines the diagnostic record shared by all checks and reports.

use std::fmt;

use serde::Serialize;

/// Levels are ordered so that sorting ascending puts errors first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    #[default]
    Error,
    Warn,
    Off,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Off => "off",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Diagnostic is one finding. Fields below the JSON block are internal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub code: String,
    pub rule: String,
    pub level: String,
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub message: String,
    pub hint: String,

    #[serde(skip)]
    pub lvl: Level,
    /// byte offsets in the source file; meaningless when no_pos
    #[serde(skip)]
    pub start: usize,
    #[serde(skip)]
    pub end: usize,
    /// no position in the JSON text (reported at 1:1)
    #[serde(skip)]
    pub no_pos: bool,
    /// owner document id ("" for config/code/out files)
    #[serde(skip)]
    pub doc_id: String,
}

/// Sort orders diagnostics deterministically: level, file path (bytes), line,
/// column, code, rule name.
pub fn sort(ds: &mut [Diagnostic]) {
    ds.sort_by(|a, b| {
        (a.lvl, &a.file, a.line, a.column, &a.code, &a.rule).cmp(&(
            b.lvl,
            &b.file,
            b.line,
            b.column,
            &b.code,
            &b.rule,
        ))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_orders_by_level_then_file() {
        let d = |lvl, file: &str, line| Diagnostic {
            lvl,
            file: file.into(),
            line,
            ..Default::default()
        };
        let mut ds = vec![d(Level::Warn, "a", 1), d(Level::Error, "b", 2), d(Level::Error, "b", 1)];
        sort(&mut ds);
        assert_eq!(ds, vec![d(Level::Error, "b", 1), d(Level::Error, "b", 2), d(Level::Warn, "a", 1)]);
        assert_eq!(Level::Warn.to_string(), "warn");
    }
}
