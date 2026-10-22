use serde_json::{Value, json};

use super::DOCUMENT_SCHEMA;
use crate::rules;

const CONFIG_SCHEMA: &str = include_str!("config.json");
const COMPILE_SCHEMA: &str = include_str!("output-compile.json");
const LOG_SCHEMA: &str = include_str!("output-log.json");
const DIFF_SCHEMA: &str = include_str!("output-diff.json");

/// log_output and diff_output are the schemas of the outputs of docc log
/// and docc diff (`docc schema log|diff`).
pub fn log_output() -> &'static [u8] {
    LOG_SCHEMA.as_bytes()
}

pub fn diff_output() -> &'static [u8] {
    DIFF_SCHEMA.as_bytes()
}

/// Compile returns the schema of the output of docc compile, printed by
/// `docc schema compile`; its $id is the "$schema" of that output.
pub fn compile_output() -> &'static [u8] {
    COMPILE_SCHEMA.as_bytes()
}

/// Document returns the document meta-schema printed by `docc schema document`.
pub fn document() -> &'static [u8] {
    DOCUMENT_SCHEMA.as_bytes()
}

/// Config returns the docc.json schema printed by `docc schema config`. The
/// allowed keys of the rules section are taken from the rule registry.
pub fn config() -> Vec<u8> {
    let mut m: Value = serde_json::from_str(CONFIG_SCHEMA).expect("config.json is valid JSON");
    let mut names = Vec::new();
    for r in rules::all() {
        names.push(r.code);
        if r.code != "DOC800" {
            names.push(r.name);
        }
    }
    names.extend(rules::GROUPS);
    let keys = json!({"anyOf": [
        {"enum": names},
        {"pattern": "^custom/[a-z0-9-]+$"},
    ]});
    m["properties"]["rules"]["propertyNames"] = keys;
    // json.MarshalIndent: keys sorted (serde_json's map is ordered), two
    // spaces, and <, >, & escaped as Go does by default
    let s = serde_json::to_string_pretty(&m).expect("a JSON value always encodes");
    let s = s
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    let mut b = s.into_bytes();
    b.push(b'\n');
    b
}
