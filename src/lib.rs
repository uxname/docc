//! docc lints documentation written as JSON in docs/src and builds Markdown/HTML from it.
//! Pipeline: loader → schema → engine → render; cli is flag parsing and dispatch only.

pub mod changes;
pub mod codes;
pub mod cli;
pub mod comments;
pub mod config;
pub mod diag;
pub mod engine;
pub mod history;
pub mod loader;
pub mod mdscan;
pub mod render;
pub mod report;
pub mod rules;
pub mod schema;
pub mod source;
pub mod text;
pub mod trace;

/// git is a git command that reads the repository it is run in, whatever the
/// environment names: inside a Git hook GIT_DIR, GIT_INDEX_FILE and the rest
/// point at the repository the hook runs for (a submodule, say), not at the
/// project docc checks. The list is `git rev-parse --local-env-vars`, what git
/// clears itself when it steps into a submodule.
pub(crate) fn git() -> std::process::Command {
    const LOCAL_ENV: [&str; 15] = [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ];
    let mut c = std::process::Command::new("git");
    for v in LOCAL_ENV {
        c.env_remove(v);
    }
    c
}
