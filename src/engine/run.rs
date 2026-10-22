use super::project::Project;
use crate::diag::{Diagnostic, Level};

impl Project {
    /// Validate runs every check and returns the diagnostics, sorted.
    pub fn validate(&mut self) -> Vec<Diagnostic> {
        self.check_syntax();
        self.check_comments();
        self.analyze();
        self.check_graph();
        self.parse_ignores();
        self.check_rule_codes();
        self.check_kinds();
        self.check_text();
        self.check_markdown();
        self.check_custom();
        self.check_trace();
        self.check_lock();
        self.suppress();
        self.sorted()
    }
}

/// Structural reports whether the diagnostics contain an error of group 1xx
/// or 2xx; build and check stop on those.
pub fn structural(ds: &[Diagnostic]) -> bool {
    ds.iter().any(|d| {
        d.lvl == Level::Error && (d.code.starts_with("DOC1") || d.code.starts_with("DOC2"))
    })
}
