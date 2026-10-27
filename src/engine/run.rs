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
        if !self.legacy {
            self.check_versions();
        }
        self.suppress();
        self.sorted()
    }
}

/// Structural reports whether the diagnostics contain an error of group 1xx
/// or 2xx; build, compile and comments stop on those. A link whose version
/// hash is missing or old (DOC209) and a block without a code (DOC216) are
/// not: the pages still render, so a draft an agent is checking stays
/// readable and commentable; validate and check still fail on them.
pub fn structural(ds: &[Diagnostic]) -> bool {
    ds.iter().any(|d| {
        d.lvl == Level::Error
            && (d.code.starts_with("DOC1") || d.code.starts_with("DOC2"))
            && !NON_BLOCKING.contains(&d.code.as_str())
    })
}

/// NON_BLOCKING are the errors of groups 1xx-2xx that do not stop the output.
pub const NON_BLOCKING: [&str; 2] = ["DOC209", "DOC216"];
