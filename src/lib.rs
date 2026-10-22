//! docc lints documentation written as JSON in docs/src and builds Markdown/HTML from it.
//! Pipeline: loader → schema → engine → render; cli is flag parsing and dispatch only.

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
