//! Package schema validates documents with JSON Schema (draft 2020-12): the
//! built-in document schema and per-kind schemas from the schemas directory.
//! Validation errors are mapped back to positions in the source text.

mod compile;
mod formats;
mod kind;
mod public;
#[cfg(test)]
mod tests;
mod texts;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

use jsonschema::error::ValidationErrorKind;
use jsonschema::{ValidationError, Validator};
use serde_json::Value;

use crate::config as cfg;
use crate::loader::{self, Node};
use crate::source::{self, Source};

pub use kind::closest;
pub use public::{comments_apply_output, compile_output, config, diff_output, document, log_output};

use texts::{display, go_quote, join_quoted, printer_float, printer_int, quote, type_of};

/// DOCUMENT_SCHEMA is the built-in document schema.
pub const DOCUMENT_SCHEMA: &str = include_str!("document.json");

const BUILTIN_URL: &str = "docc://builtin/document.json";
const SCHEMAS_URL: &str = "docc://s/schemas/";

/// Violation is one schema error with the place it points to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Violation {
    pub pointer: String,
    pub message: String,
    pub hint: String,
    pub start: usize,
    pub end: usize,
}

/// Set holds the compiled built-in schema and the kind schemas.
pub struct Set {
    builtin: Validator,
    kinds: HashMap<String, Kind>,
    /// the raw schemas, for the keyword values the messages quote and for
    /// the subschemas checked again on their own (contains, propertyNames)
    l: Arc<MemLoader>,
}

/// Kind is a compiled kind schema with what docc reads from its raw JSON:
/// the top-level "additionalProperties" (checked by docc, not by the
/// validator), the names it allows, "x-docc-target" of properties and
/// "x-docc-covered".
pub struct Kind {
    schema: Validator,
    url: String,
    closed: bool,
    allowed: BTreeSet<String>,
    targets: HashMap<String, Vec<String>>,
    pub covered: bool,
}

/// LoadError is what Load fails with: a config error that points at a
/// schema file, or a problem with the built-in schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Config(cfg::Error),
    Other(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Config(e) => e.fmt(f),
            LoadError::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<cfg::Error> for LoadError {
    fn from(e: cfg::Error) -> LoadError {
        LoadError::Config(e)
    }
}

fn cfg_err(file: &str, line: usize, col: usize, msg: impl Into<String>) -> LoadError {
    LoadError::Config(cfg::Error {
        file: file.to_string(),
        line,
        col,
        msg: msg.into(),
    })
}

/// MemLoader serves only preloaded resources: no network, no files outside
/// the schemas directory.
#[derive(Default)]
struct MemLoader {
    /// URL as the Go library spells it -> the schema as the Rust validator
    /// reads it (compile::rewrite)
    docs: HashMap<String, Value>,
    /// URL the Rust validator uses (compile::crate_safe) -> URL in docs
    by_crate: HashMap<String, String>,
    /// URL -> why the Go library rejects the schema when it loads it: its
    /// metaschema, or a regular expression Go cannot compile
    rejected: HashMap<String, String>,
    /// stand-in of a reference that closes a cycle -> the way round the
    /// cycle from the referenced schema (compile::break_cycles)
    cycles: HashMap<String, compile::Round>,
}

impl MemLoader {
    /// insert adds the schema at u, checked against its metaschema and
    /// rewritten for the Rust validator.
    fn insert(&mut self, u: String, mut v: Value) {
        let mut bad = None;
        if let Err(e) = jsonschema::meta::validate(&v) {
            bad = Some(meta_error(&u, &e));
        }
        let mut regex = None;
        compile::rewrite(&mut v, &mut Vec::new(), &mut regex);
        if let Some(e) = bad.or_else(|| {
            regex.map(|e| {
                format!(
                    "{} is not valid against metaschema: {e}",
                    go_quote(&format!("{u}#"))
                )
            })
        }) {
            self.rejected.insert(u.clone(), e);
        }
        self.by_crate.insert(compile::crate_safe(&u), u.clone());
        self.docs.insert(u, v);
    }

    fn load(&self, u: &str) -> Result<&Value, String> {
        if let Some(v) = self.docs.get(u) {
            return Ok(v);
        }
        Err(format!(
            "failing loading {}: reference {} is outside the schemas directory or remote (not allowed)",
            go_quote(u),
            go_quote(u)
        ))
    }

    /// get_crate returns the schema at a URL the Rust validator uses.
    fn get_crate(&self, u: &str) -> Option<&Value> {
        self.by_crate.get(u).and_then(|u| self.docs.get(u))
    }
}

/// Retriever hands the validator the resources of a MemLoader. A resource
/// that does not exist is the schema true: compile::go_compile has already
/// failed for every one a schema can reach, and the Go library never loads
/// the others.
struct Retriever {
    l: Arc<MemLoader>,
}

impl jsonschema::Retrieve for Retriever {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self
            .l
            .get_crate(uri.as_str())
            .cloned()
            .unwrap_or(Value::Bool(true)))
    }
}

/// compile builds the validator of the resource at url the way the Go
/// compiler is set up: draft 2020-12 unless "$schema" names another, formats
/// asserted, references resolved only from l. The errors are those of the Go
/// compiler (compile::go_compile).
fn compile(l: &Arc<MemLoader>, url: &str) -> Result<Validator, String> {
    compile::go_compile(l, url)?;
    let root = l.load(url)?;
    build(l, &compile::crate_safe(url), root)
}

/// meta_error words a schema that its metaschema rejects. The Go library
/// prints the whole tree of the metaschema's errors; this is the one leaf the
/// Rust validator reports (docs/rust-port/schema.md).
fn meta_error(url: &str, e: &ValidationError) -> String {
    if let ValidationErrorKind::Referencing(jsonschema::ReferencingError::Unretrievable {
        uri,
        ..
    }) = e.kind()
    {
        // a "$schema" that is not a known metaschema
        return MemLoader::default().load(uri).err().unwrap_or_default();
    }
    format!(
        "{} is not valid against metaschema: at {}: {e}",
        go_quote(&format!("{url}#")),
        quote(e.instance_path().as_str())
    )
}

/// build compiles root with base url (a URI the Rust validator accepts).
fn build(l: &Arc<MemLoader>, url: &str, root: &Value) -> Result<Validator, String> {
    let mut opts = jsonschema::options()
        .should_validate_formats(true)
        .with_retriever(Retriever { l: Arc::clone(l) })
        .with_base_uri(url)
        // the patterns are the regex crate's syntax (compile::go_pattern)
        .with_pattern_options(jsonschema::PatternOptions::regex())
        // formats the Rust validator knows and the Go library does not
        .with_format("idn-email", |_: &str| true)
        .with_format("idn-hostname", |_: &str| true);
    for name in formats::NAMES {
        opts = opts.with_format(name, move |s: &str| {
            !matches!(formats::validate(name, s), Some(Err(_)))
        });
    }
    opts.build(root).map_err(|e| match e.kind() {
        ValidationErrorKind::Referencing(jsonschema::ReferencingError::PointerToNowhere {
            pointer,
        }) => {
            format!(
                "json-pointer in {} not found",
                go_quote(&format!("{url}#{pointer}"))
            )
        }
        _ => e.to_string(),
    })
}

/// Load compiles the built-in schema and every *.json file under dir.
/// A missing dir is fine. display_dir is the project-relative path for messages.
pub fn load(dir: &Path, display_dir: &str, src: &Source) -> Result<Set, LoadError> {
    let res = loader::parse(DOCUMENT_SCHEMA.as_bytes());
    let Some(builtin_root) = res.root.as_ref().filter(|_| res.err.is_none()) else {
        let msg = res.err.map(|e| e.msg).unwrap_or_default();
        return Err(LoadError::Other(format!("built-in schema: {msg}")));
    };
    let mut l = MemLoader::default();
    l.insert(BUILTIN_URL.to_string(), loader::value(builtin_root));
    let mut files: Vec<(String, String)> = Vec::new(); // rel path -> url, sorted
    let mut raw: HashMap<String, Node> = HashMap::new();
    if src.entry(dir) == Some(source::Entry::Dir) {
        let mut rels = Vec::new();
        if let Source::Rev(rev) = src {
            for (rel, b) in rev.files(display_dir) {
                if !b.link && rel.ends_with(".json") {
                    rels.push(rel.to_string());
                }
            }
            rev.prefetch(rels.iter().map(|r| format!("{display_dir}/{r}")).collect::<Vec<_>>().iter().map(String::as_str));
        } else {
            walk_dir(dir, "", &mut rels).map_err(|msg| cfg_err(display_dir, 0, 0, msg))?;
        }
        rels.sort();
        for rel in rels {
            let shown = format!("{display_dir}/{rel}");
            let path = dir.join(&rel);
            let data = src
                .read(&path)
                .map_err(|e| cfg_err(&shown, 0, 0, go_path_error("open", &path, &e)))?;
            let r = loader::parse(&data);
            if let Some(e) = &r.err {
                let (ln, col) = r.src.position(e.offset);
                return Err(cfg_err(
                    &shown,
                    ln,
                    col,
                    format!("invalid schema file: {}", e.msg),
                ));
            }
            if let Some(d) = r.dups.first() {
                let (ln, col) = r.src.position(d.start);
                return Err(cfg_err(
                    &shown,
                    ln,
                    col,
                    format!("duplicate key {}", go_quote(&d.key)),
                ));
            }
            let Some(root) = r.root else {
                continue;
            };
            let u = format!("{SCHEMAS_URL}{rel}");
            let mut v = loader::value(&root);
            if !rel.contains('/') {
                let ap = root.get("additionalProperties");
                if ap.is_none_or(|ap| ap.kind != loader::Kind::Bool) {
                    let (ln, col) = ap.map_or((0, 0), |ap| r.src.position(ap.start));
                    return Err(cfg_err(
                        &shown,
                        ln,
                        col,
                        r#"a kind schema must set "additionalProperties" to true or false at the top level: true allows fields that are not listed in "properties", false allows only the listed fields and the service fields"#,
                    ));
                }
                if let Value::Object(m) = &mut v {
                    m.remove("additionalProperties");
                }
                raw.insert(rel.clone(), root);
            }
            l.insert(u.clone(), v);
            files.push((rel, u));
        }
    }
    compile::break_cycles(&mut l);
    let l = Arc::new(l);
    let builtin =
        compile(&l, BUILTIN_URL).map_err(|e| LoadError::Other(format!("built-in schema: {e}")))?;
    let mut s = Set {
        builtin,
        kinds: HashMap::new(),
        l: Arc::clone(&l),
    };
    for (rel, u) in &files {
        let shown = format!("{display_dir}/{rel}");
        let sc = compile(&l, u).map_err(|e| {
            cfg_err(
                &shown,
                0,
                0,
                format!("cannot compile schema: {}", one_line(&e)),
            )
        })?;
        if let Some(root) = raw.get(rel) {
            let k = Kind::new(sc, root, u, &l).map_err(|e| cfg_err(&shown, 0, 0, e))?;
            s.kinds
                .insert(rel.strip_suffix(".json").unwrap_or(rel).to_string(), k);
        }
    }
    Ok(s)
}

/// walk_dir collects the regular *.json files under dir as slash-separated
/// paths relative to the schemas directory, like filepath.WalkDir: symbolic
/// links are not followed.
fn walk_dir(dir: &Path, rel: &str, out: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| go_path_error("open", dir, &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| go_path_error("readdirent", dir, &e))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let sub = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            walk_dir(&entry.path(), &sub, out)?;
        } else if ft.is_file() && name.ends_with(".json") {
            out.push(sub);
        }
    }
    Ok(())
}

/// go_path_error is the text of Go's *fs.PathError: "open <path>: permission denied".
fn go_path_error(op: &str, path: &Path, err: &std::io::Error) -> String {
    let mut text = err.to_string();
    if let Some(code) = err.raw_os_error() {
        // Rust adds " (os error N)" and capitalizes strerror; Go does neither.
        text = text
            .trim_end_matches(&format!(" (os error {code})"))
            .to_string();
        let mut cs = text.chars();
        if let Some(c) = cs.next() {
            text = c.to_lowercase().chain(cs).collect();
        }
    }
    format!("{op} {}: {text}", path.display())
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// percent_decode decodes %XX escapes; anything else is kept.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            let hex = |c: u8| char::from(c).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(b[i + 1]) << 4 | hex(b[i + 2]));
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl Set {
    /// HasKind reports whether a schema file exists for the kind.
    pub fn has_kind(&self, kind_name: &str) -> bool {
        self.kinds.contains_key(kind_name)
    }

    /// CheckBuiltin validates the built-in fields of a document (DOC105).
    /// A missing or blank title is left to DOC103 and is not reported here.
    pub fn check_builtin(&self, root: &Node) -> Vec<Violation> {
        let mut inst = serde_json::Map::new();
        for f in BUILTIN_FIELDS {
            if let Some(n) = root.get(f) {
                inst.insert(f.to_string(), loader::value(n));
            }
        }
        let vs = self.run(&self.builtin, BUILTIN_URL, &Value::Object(inst), root);
        vs.into_iter()
            .filter(|v| {
                if v.pointer.is_empty() && v.message.starts_with("missing property \"title\"") {
                    return false;
                }
                // blank title: DOC103
                v.pointer != "/title" || v.message.contains("got ")
            })
            .collect()
    }

    /// CheckKind validates a whole document against the schema of its kind
    /// (DOC106), service fields included, and reports fields a closed kind does
    /// not allow.
    pub fn check_kind(&self, kind_name: &str, root: &Node) -> Vec<Violation> {
        let Some(k) = self.kinds.get(kind_name) else {
            return Vec::new();
        };
        let mut inst = serde_json::Map::new();
        for m in &root.members {
            // the comments are docc's own (DOC112), allowed under every kind like the title
            if !inst.contains_key(&m.key) && m.key != crate::comments::KEY {
                inst.insert(m.key.clone(), loader::value(&m.value));
            }
        }
        let mut vs = self.run(&k.schema, &k.url, &Value::Object(inst), root);
        vs.extend(k.unknown_fields(root));
        let t = root.get("title");
        if t.is_none_or(|t| t.kind == loader::Kind::String && t.str.trim().is_empty()) {
            // a missing or blank title is DOC103 already
            vs.retain(|v| {
                !(v.pointer == "/title"
                    || (v.pointer.is_empty()
                        && v.message.starts_with("missing property \"title\"")))
            });
        }
        sort_violations(&mut vs);
        vs
    }

    /// run validates inst with sc, the schema compiled from url.
    fn run(&self, sc: &Validator, url: &str, inst: &Value, root: &Node) -> Vec<Violation> {
        let raws: Vec<Raw> = sc.iter_errors(inst).map(Raw::new).collect();
        if raws.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let origin = Origin {
            at: (url.to_string(), String::new()),
            kw_loc: String::new(),
            wrapped: false,
        };
        self.collect(raws, inst, &[], root, &origin, &mut out);
        sort_violations(&mut out);
        // drop exact duplicates
        out.dedup_by(|v, prev| v.start == prev.start && v.message == prev.message);
        out
    }

    /// raw_at returns the raw schema value at an absolute keyword location
    /// of the Rust validator.
    fn raw_at(&self, abs: &str) -> Option<&Value> {
        let (u, frag) = abs.split_once('#').unwrap_or((abs, ""));
        compile::lookup(self.l.get_crate(u)?, &percent_decode(frag)).ok()
    }

    /// sub validates inst against the subschema at abs on its own, as the Go
    /// validator does for "contains", "propertyNames" and the unevaluated
    /// keywords, and collects its violations at prefix. It reports whether
    /// inst is valid. kw_loc is the keyword path of the subschema as the Go
    /// library prints it.
    // ponytail: compiles the subschema on every call; these keywords are rare
    // in kind schemas, cache the validators by abs if they show up in profiles.
    fn sub(
        &self,
        abs: &str,
        kw_loc: &str,
        inst: &Value,
        prefix: &[String],
        root: &Node,
        out: &mut Vec<Violation>,
    ) -> bool {
        let wrapper = serde_json::json!({ "$ref": abs });
        let Ok(v) = build(&self.l, "docc://sub/schema.json", &wrapper) else {
            return true;
        };
        let raws: Vec<Raw> = v.iter_errors(inst).map(Raw::new).collect();
        if raws.is_empty() {
            return true;
        }
        let (u, frag) = abs.split_once('#').unwrap_or((abs, ""));
        let origin = Origin {
            at: (
                self.l.by_crate.get(u).cloned().unwrap_or_default(),
                percent_decode(frag),
            ),
            kw_loc: kw_loc.to_string(),
            wrapped: true,
        };
        self.collect(raws, inst, prefix, root, &origin, out);
        false
    }

    /// collect turns the validator's errors into violations the way the Go
    /// code walks the error tree of the Go library: anyOf and oneOf are one
    /// violation, required and additionalProperties one per field, and the
    /// rest one per failed keyword with the library's English text.
    fn collect(
        &self,
        raws: Vec<Raw>,
        inst: &Value,
        prefix: &[String],
        root: &Node,
        origin: &Origin,
        out: &mut Vec<Violation>,
    ) {
        let raws = go_order(raws);
        let mut done = HashSet::new(); // keywords handled once per instance
        // the keyword path of an error as the Go library prints it
        let kw_loc = |eval: &str| {
            let eval = if origin.wrapped {
                eval.strip_prefix("/$ref").unwrap_or(eval)
            } else {
                eval
            };
            format!("{}{}", origin.kw_loc, compile::encode(eval))
        };
        for r in &raws {
            let mut loc = prefix.to_vec();
            loc.extend(r.loc.iter().cloned());
            let n = node_at(root, &loc);
            let val = value_at(inst, &r.loc).unwrap_or(&Value::Null);
            let mut text = String::new();
            match &r.kind {
                ValidationErrorKind::AnyOf { .. }
                | ValidationErrorKind::OneOfNotValid { .. }
                | ValidationErrorKind::OneOfMultipleValid { .. } => {
                    out.push(mk(
                        &loc,
                        "value does not match any allowed shape",
                        "Use one of the shapes the schema allows for this field.",
                        n.start,
                        n.end,
                    ));
                    continue;
                }
                ValidationErrorKind::Required { property } => {
                    if let Some(kw) = dependency_keyword(&r.eval) {
                        if done.insert((r.eval.clone(), r.loc.clone())) {
                            for t in self.dependencies(&r.abs, kw, val) {
                                out.push(mk(&loc, &t, DEFAULT_HINT, n.start, n.end));
                            }
                        }
                        continue;
                    }
                    let m = property.as_str().unwrap_or_default();
                    if val.as_object().is_some_and(|o| o.contains_key(m)) {
                        // The Rust validator reports a present field as missing when
                        // "required" names it, "properties" does not and
                        // "additionalProperties" is false; that is reported already.
                        continue;
                    }
                    out.push(mk(
                        &loc,
                        &format!("missing property {}", go_quote(m)),
                        &format!("Add the field {}.", go_quote(m)),
                        n.start,
                        n.start + 1,
                    ));
                    continue;
                }
                ValidationErrorKind::AdditionalProperties { .. }
                | ValidationErrorKind::FalseSchema
                    if matches!(r.kind, ValidationErrorKind::AdditionalProperties { .. })
                        || (r.eval.ends_with("/additionalProperties") && val.is_object()) =>
                {
                    // "additionalProperties": false without "properties"
                    // comes as a false schema at the object: every field
                    let unexpected = match &r.kind {
                        ValidationErrorKind::AdditionalProperties { unexpected } => {
                            unexpected.clone()
                        }
                        _ => val
                            .as_object()
                            .map(|o| o.keys().cloned().collect())
                            .unwrap_or_default(),
                    };
                    for p in &unexpected {
                        let (start, end) = n
                            .members
                            .iter()
                            .find(|m| &m.key == p)
                            .map_or((n.start, n.start + 1), |m| (m.key_start, m.key_end));
                        out.push(mk(
                            &loc,
                            &format!("property {} is not allowed", go_quote(p)),
                            &format!(
                                "Remove the field {} or allow it in the schema.",
                                go_quote(p)
                            ),
                            start,
                            end,
                        ));
                    }
                    continue;
                }
                ValidationErrorKind::UniqueItems => {
                    let Some((j, i)) = val.as_array().and_then(|a| duplicates(a)) else {
                        continue;
                    };
                    let mut iloc = loc.clone();
                    iloc.push(i.to_string());
                    let item = node_at(root, &iloc);
                    let text = format!(
                        "items at {} and {} are equal",
                        printer_int(j as i64),
                        printer_int(i as i64)
                    );
                    out.push(mk(
                        &iloc,
                        &text,
                        "Remove the repeated element.",
                        item.start,
                        item.end,
                    ));
                    continue;
                }
                ValidationErrorKind::Type { kind } => {
                    let mut want: Vec<String> = match kind {
                        jsonschema::error::TypeKind::Single(t) => vec![t.to_string()],
                        jsonschema::error::TypeKind::Multiple(ts) => {
                            ts.iter().map(|t| t.to_string()).collect()
                        }
                    };
                    want.sort_by_key(|t| texts::TYPE_ORDER.iter().position(|o| o == t));
                    let want = want.join(" or ");
                    let got = type_of(val);
                    let hint = match loc.last() {
                        Some(last) => {
                            format!("Field {} must be {want}, not {got}.", go_quote(last))
                        }
                        None => format!("Use a value of type {want}."),
                    };
                    out.push(mk(
                        &loc,
                        &format!("got {got}, want {want}"),
                        &hint,
                        n.start,
                        n.end,
                    ));
                    continue;
                }
                ValidationErrorKind::Enum { options } => {
                    let want = options.as_array().map(Vec::as_slice).unwrap_or_default();
                    out.push(mk(
                        &loc,
                        &enum_text(want),
                        "Use one of the allowed values.",
                        n.start,
                        n.end,
                    ));
                    continue;
                }
                // "propertyNames": false comes as a false schema at the object
                ValidationErrorKind::PropertyNames { .. } | ValidationErrorKind::FalseSchema
                    if matches!(r.kind, ValidationErrorKind::PropertyNames { .. })
                        || r.eval.ends_with("/propertyNames") =>
                {
                    if done.insert((r.abs.clone(), r.loc.clone())) {
                        // the names are checked as values of their own: their
                        // violations point at the document root
                        for name in val.as_object().into_iter().flat_map(|o| o.keys()) {
                            // checked as a value of its own: no keyword path
                            self.sub(&r.abs, "", &Value::String(name.clone()), &[], root, out);
                        }
                    }
                    continue;
                }
                ValidationErrorKind::Contains => {
                    let node = parent(&r.abs).to_string();
                    if done.insert((node.clone(), r.loc.clone())) {
                        self.contains(&node, &kw_loc(parent(&r.eval)), val, &loc, n, root, out);
                    }
                    continue;
                }
                ValidationErrorKind::UnevaluatedProperties { unexpected } => {
                    for p in unexpected {
                        let mut ploc = loc.clone();
                        ploc.push(p.clone());
                        let pval = val.get(p).unwrap_or(&Value::Null);
                        self.unevaluated(&r.abs, &kw_loc(&r.eval), pval, &ploc, root, out);
                    }
                    continue;
                }
                ValidationErrorKind::UnevaluatedItems { unexpected } => {
                    // The items come as JSON text, in order. Of equal items
                    // the evaluated ones are those of "prefixItems" ("items"
                    // and "contains" judge equal items alike), so matching
                    // from the end finds the unevaluated ones.
                    let items = val.as_array().map(Vec::as_slice).unwrap_or_default();
                    let mut upto = items.len();
                    for want in unexpected.iter().rev() {
                        let Some(i) = (0..upto).rev().find(|&i| &items[i].to_string() == want)
                        else {
                            continue;
                        };
                        upto = i;
                        let mut iloc = loc.clone();
                        iloc.push(i.to_string());
                        self.unevaluated(&r.abs, &kw_loc(&r.eval), &items[i], &iloc, root, out);
                    }
                    continue;
                }
                ValidationErrorKind::Constant { expected_value } => {
                    text = match expected_value {
                        Value::Array(_) | Value::Object(_) => "'const' failed".to_string(),
                        v => format!("value must be {}", display(v)),
                    };
                }
                ValidationErrorKind::Format { format } => {
                    let s = val.as_str().unwrap_or_default();
                    let reason = match formats::validate(format, s) {
                        Some(Err(e)) => e,
                        _ => String::new(),
                    };
                    text = format!("{} is not valid {format}: {reason}", display(val));
                }
                ValidationErrorKind::Pattern { pattern } => {
                    // a rewritten pattern keeps the one the schema has next to it
                    let src = self
                        .raw_at(&format!("{}/x-docc-go-pattern", parent(&r.abs)))
                        .and_then(Value::as_str)
                        .unwrap_or(pattern);
                    text = format!(
                        "{} does not match pattern {}",
                        quote(val.as_str().unwrap_or_default()),
                        quote(src)
                    );
                }
                ValidationErrorKind::MinLength { limit } => {
                    let got = val.as_str().map_or(0, |s| s.chars().count());
                    text = format!(
                        "minLength: got {}, want {}",
                        printer_int(got as i64),
                        printer_u64(*limit)
                    );
                }
                ValidationErrorKind::MaxLength { limit } => {
                    let got = val.as_str().map_or(0, |s| s.chars().count());
                    text = format!(
                        "maxLength: got {}, want {}",
                        printer_int(got as i64),
                        printer_u64(*limit)
                    );
                }
                ValidationErrorKind::MinItems { limit } => {
                    let got = val.as_array().map_or(0, Vec::len);
                    text = format!(
                        "minItems: got {}, want {}",
                        printer_int(got as i64),
                        printer_u64(*limit)
                    );
                }
                ValidationErrorKind::MaxItems { limit } => {
                    let got = val.as_array().map_or(0, Vec::len);
                    text = format!(
                        "maxItems: got {}, want {}",
                        printer_int(got as i64),
                        printer_u64(*limit)
                    );
                }
                ValidationErrorKind::MinProperties { limit } => {
                    let got = val.as_object().map_or(0, serde_json::Map::len);
                    text = format!(
                        "minProperties: got {}, want {}",
                        printer_int(got as i64),
                        printer_u64(*limit)
                    );
                }
                ValidationErrorKind::MaxProperties { limit } => {
                    let got = val.as_object().map_or(0, serde_json::Map::len);
                    text = format!(
                        "maxProperties: got {}, want {}",
                        printer_int(got as i64),
                        printer_u64(*limit)
                    );
                }
                ValidationErrorKind::Minimum { limit } => {
                    text = bound("minimum", val, limit.as_f64())
                }
                ValidationErrorKind::Maximum { limit } => {
                    text = bound("maximum", val, limit.as_f64())
                }
                ValidationErrorKind::ExclusiveMinimum { limit } => {
                    text = bound("exclusiveMinimum", val, limit.as_f64())
                }
                ValidationErrorKind::ExclusiveMaximum { limit } => {
                    text = bound("exclusiveMaximum", val, limit.as_f64())
                }
                ValidationErrorKind::MultipleOf { multiple_of } => {
                    text = bound("multipleOf", val, Some(*multiple_of))
                }
                ValidationErrorKind::Not { .. } => text = "'not' failed".to_string(),
                ValidationErrorKind::FalseSchema => {
                    text = "false schema".to_string();
                    if r.abs.starts_with(compile::CYCLE_URL) {
                        let eval = if origin.wrapped {
                            r.eval.strip_prefix("/$ref").unwrap_or(&r.eval)
                        } else {
                            &r.eval
                        };
                        if let Some(t) =
                            compile::cycle_text(&self.l, &origin.at, &origin.kw_loc, eval, &r.abs)
                        {
                            text = t;
                        }
                    }
                }
                ValidationErrorKind::AdditionalItems { limit } => {
                    let count = val.as_array().map_or(0, Vec::len).saturating_sub(*limit);
                    text = format!(
                        "last {} additionalItem(s) not allowed",
                        printer_int(count as i64)
                    );
                }
                _ => {}
            }
            if text.is_empty() {
                text = r.message.clone();
            }
            out.push(mk(&loc, &text, DEFAULT_HINT, n.start, n.end));
        }
    }

    /// dependencies is the text of every "dependentRequired" (or draft-7
    /// "dependencies") entry obj does not satisfy.
    fn dependencies(&self, abs: &str, kw: &str, obj: &Value) -> Vec<String> {
        // the location may end below the keyword ("/dependentRequired/0")
        let (u, frag) = abs.split_once('#').unwrap_or((abs, ""));
        let segs: Vec<&str> = frag.split('/').collect();
        let Some(last) = segs.iter().rposition(|s| *s == kw) else {
            return Vec::new();
        };
        let at = format!("{u}#{}", segs[..=last].join("/"));
        let (Some(Value::Object(deps)), Some(obj)) = (self.raw_at(&at), obj.as_object()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (pname, reqd) in deps {
            let (true, Some(reqd)) = (obj.contains_key(pname), reqd.as_array()) else {
                continue;
            };
            let missing: Vec<String> = reqd
                .iter()
                .filter_map(Value::as_str)
                .filter(|r| !obj.contains_key(*r))
                .map(str::to_string)
                .collect();
            if !missing.is_empty() {
                out.push(format!(
                    "properties {} required, if {} exists",
                    join_quoted(&missing, ", "),
                    quote(pname)
                ));
            }
        }
        out
    }

    /// contains reports "contains", "minContains" and "maxContains" of the
    /// schema at node (keyword path kw_loc) for the array arr as the Go
    /// library does: when too few items match, the errors of the items that
    /// do not.
    #[allow(clippy::too_many_arguments)]
    fn contains(
        &self,
        node: &str,
        kw_loc: &str,
        arr: &Value,
        loc: &[String],
        n: &Node,
        root: &Node,
        out: &mut Vec<Violation>,
    ) {
        let Some(Value::Object(sch)) = self.raw_at(node) else {
            return;
        };
        let items = arr.as_array().map(Vec::as_slice).unwrap_or_default();
        let mut errors = Vec::new();
        let mut matched = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let mut iloc = loc.to_vec();
            iloc.push(i.to_string());
            let kw_loc = format!("{kw_loc}/contains");
            if self.sub(
                &format!("{node}/contains"),
                &kw_loc,
                item,
                &iloc,
                root,
                &mut errors,
            ) {
                matched.push(i);
            }
        }
        let at = matched
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(" ");
        let count = printer_int(matched.len() as i64);
        let mut texts = Vec::new();
        if let Some(min) = sch.get("minContains").and_then(Value::as_u64) {
            if (matched.len() as u64) < min {
                if !errors.is_empty() {
                    out.append(&mut errors);
                } else if matched.is_empty() {
                    texts.push(format!(
                        "min {} items required to match contains schema, but none matched",
                        printer_u64(min)
                    ));
                } else {
                    texts.push(format!(
                        "min {} items required to match contains schema, but matched {count} items at {at}",
                        printer_u64(min)
                    ));
                }
            }
        } else if matched.is_empty() {
            if errors.is_empty() {
                texts.push("no items match contains schema".to_string());
            } else {
                out.append(&mut errors);
            }
        }
        if let Some(max) = sch.get("maxContains").and_then(Value::as_u64)
            && matched.len() as u64 > max
        {
            texts.push(format!(
                "max {} items required to match contains schema, but matched {count} items at {at}",
                printer_u64(max)
            ));
        }
        for t in texts {
            out.push(mk(loc, &t, DEFAULT_HINT, n.start, n.end));
        }
    }

    /// unevaluated reports a property or item the unevaluated keyword at abs
    /// rejects: "false schema" for false, the errors of its schema otherwise.
    fn unevaluated(
        &self,
        abs: &str,
        kw_loc: &str,
        v: &Value,
        loc: &[String],
        root: &Node,
        out: &mut Vec<Violation>,
    ) {
        if let Some(Value::Bool(false)) = self.raw_at(abs) {
            let n = node_at(root, loc);
            out.push(mk(loc, "false schema", DEFAULT_HINT, n.start, n.end));
            return;
        }
        self.sub(abs, kw_loc, v, loc, root, out);
    }
}

const DEFAULT_HINT: &str = "Change the value so that it matches the schema.";

/// builtinFields are the fields the built-in schema is applied to (DOC105).
pub(crate) const BUILTIN_FIELDS: [&str; 9] = [
    "title", "kind", "status", "parent", "tags", "body", "notes", "hidden", "order",
];

/// Origin is where the errors collect gets come from: the schema the
/// validator started at, its keyword path as the Go library prints it, and
/// whether the validator wraps it in a "$ref" (Set::sub).
struct Origin {
    at: (String, String),
    kw_loc: String,
    wrapped: bool,
}

/// Raw is one error of the validator with what collect needs from it.
struct Raw {
    /// instance location as tokens
    loc: Vec<String>,
    /// keyword location through the references taken ("/properties/a/$ref/type")
    eval: String,
    /// absolute keyword location ("docc://s/schemas/a.json#/$defs/t/type")
    abs: String,
    message: String,
    kind: ValidationErrorKind,
}

impl Raw {
    fn new(e: ValidationError) -> Raw {
        let message = e.to_string();
        let abs = e
            .absolute_keyword_location()
            .map(|u| u.as_str().to_string())
            .unwrap_or_default();
        let p = e.into_parts();
        Raw {
            loc: pointer_tokens(p.instance_path.as_str()),
            eval: p.evaluation_path.as_str().to_string(),
            abs,
            message,
            kind: p.kind,
        }
    }

    /// rank is the order the Go validator checks the keywords that end the
    /// check of a schema when they fail: type, const, enum, format.
    fn rank(&self) -> Option<u8> {
        match self.kind {
            ValidationErrorKind::Type { .. } => Some(0),
            ValidationErrorKind::Constant { .. } => Some(1),
            ValidationErrorKind::Enum { .. } => Some(2),
            ValidationErrorKind::Format { .. } => Some(3),
            _ => None,
        }
    }
}

/// go_order drops the errors the Go validator never reaches: when "type",
/// "const", "enum" or "format" of a schema fails, the Go validator returns
/// that one error and checks nothing else of the schema at that value, while
/// the Rust validator checks every keyword.
// ponytail: O(n²) over the errors of one document, which are few.
fn go_order(raws: Vec<Raw>) -> Vec<Raw> {
    let under = |path: &str, node: &str| {
        path.len() > node.len() && path.starts_with(node) && path[node.len()..].starts_with('/')
    };
    let keep: Vec<bool> = raws
        .iter()
        .enumerate()
        .map(|(i, x)| {
            !raws.iter().enumerate().any(|(j, e)| {
                let Some(re) = e.rank() else {
                    return false;
                };
                if i == j || !x.loc.starts_with(&e.loc) {
                    return false;
                }
                let node = parent(&e.eval);
                if !under(&x.eval, node) {
                    return false;
                }
                // two of these keywords of one schema: the first one checked wins
                match x.rank() {
                    Some(rx) if parent(&x.eval) == node && x.loc == e.loc => rx > re,
                    _ => true,
                }
            })
        })
        .collect();
    raws.into_iter()
        .zip(keep)
        .filter_map(|(r, k)| k.then_some(r))
        .collect()
}

/// parent drops the last token of a path or keyword location.
fn parent(path: &str) -> &str {
    &path[..path.rfind('/').unwrap_or(0)]
}

/// dependency_keyword tells a "required" error raised by "dependentRequired"
/// or "dependencies" apart from one of the "required" keyword.
fn dependency_keyword(eval: &str) -> Option<&'static str> {
    let segs: Vec<&str> = eval.rsplit('/').take(2).collect();
    ["dependentRequired", "dependencies"]
        .into_iter()
        .find(|kw| {
            segs.first() == Some(kw)
                || (segs.get(1) == Some(kw) && segs.first() != Some(&"required"))
        })
}

/// duplicates finds the first item equal to an earlier one: the indexes of
/// the earlier one and of it.
fn duplicates(arr: &[Value]) -> Option<(usize, usize)> {
    for i in 1..arr.len() {
        for j in 0..i {
            if json_equal(&arr[i], &arr[j]) {
                return Some((j, i));
            }
        }
    }
    None
}

/// json_equal compares as the Go library does: numbers by value.
fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_i64(), y.as_i64()) {
            (Some(x), Some(y)) => x == y,
            _ => x.as_f64() == y.as_f64(),
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| json_equal(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_equal(v, w)))
        }
        _ => a == b,
    }
}

fn enum_text(want: &[Value]) -> String {
    if want.iter().any(|v| v.is_array() || v.is_object()) {
        return "'enum' failed".to_string();
    }
    if want.len() == 1 {
        return format!("value must be {}", display(&want[0]));
    }
    let list: Vec<String> = want.iter().map(display).collect();
    format!("value must be one of {}", list.join(", "))
}

/// bound is the text of minimum, maximum and the like: "maximum: got 7, want 3".
fn bound(name: &str, got: &Value, want: Option<f64>) -> String {
    let got = got.as_f64().unwrap_or(0.0);
    format!(
        "{name}: got {}, want {}",
        printer_float(got),
        printer_float(want.unwrap_or(0.0))
    )
}

fn printer_u64(n: u64) -> String {
    printer_int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn sort_violations(vs: &mut [Violation]) {
    vs.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then_with(|| a.message.cmp(&b.message))
    });
}

fn mk(loc: &[String], msg: &str, hint: &str, start: usize, end: usize) -> Violation {
    let p = pointer(loc);
    let message = if p.is_empty() {
        msg.to_string()
    } else {
        format!("{p}: {msg}")
    };
    Violation {
        pointer: p,
        message,
        hint: hint.to_string(),
        start,
        end,
    }
}

/// Pointer renders tokens as a JSON pointer.
pub fn pointer<S: AsRef<str>>(tokens: &[S]) -> String {
    let mut b = String::new();
    for t in tokens {
        b.push('/');
        b.push_str(&t.as_ref().replace('~', "~0").replace('/', "~1"));
    }
    b
}

/// pointer_tokens splits a JSON pointer into unescaped tokens.
fn pointer_tokens(ptr: &str) -> Vec<String> {
    if ptr.is_empty() {
        return Vec::new();
    }
    ptr[1..]
        .split('/')
        .map(|t| t.replace("~1", "/").replace("~0", "~"))
        .collect()
}

/// value_at walks a value by pointer tokens.
fn value_at<'a>(v: &'a Value, tokens: &[String]) -> Option<&'a Value> {
    tokens.iter().try_fold(v, |v, t| match v {
        Value::Object(m) => m.get(t),
        Value::Array(a) => t.parse::<usize>().ok().and_then(|i| a.get(i)),
        _ => None,
    })
}

/// nodeAt walks the loader tree by pointer tokens; falls back to the deepest
/// node reached.
fn node_at<'a>(n: &'a Node, tokens: &[String]) -> &'a Node {
    let mut n = n;
    for t in tokens {
        let next = match n.kind {
            loader::Kind::Object => n.get(t),
            loader::Kind::Array => t.parse::<usize>().ok().and_then(|i| n.items.get(i)),
            _ => None,
        };
        match next {
            Some(x) => n = x,
            None => return n,
        }
    }
    n
}

/// ResolvePointer resolves an RFC 6901 pointer string ("" is the root) on a
/// document. Returns None when the pointer does not exist.
pub fn resolve_pointer<'a>(root: &'a Node, ptr: &str) -> Option<&'a Node> {
    if ptr.is_empty() {
        return Some(root);
    }
    let rest = ptr.strip_prefix('/')?;
    let mut n = root;
    for t in rest.split('/') {
        let t = t.replace("~1", "/").replace("~0", "~");
        let next = match n.kind {
            loader::Kind::Object => n.get(&t),
            loader::Kind::Array => match t.parse::<i64>() {
                // fmt.Sprint(i) == t: no sign, no leading zeros
                Ok(i) if i >= 0 && i.to_string() == t => n.items.get(i as usize),
                _ => None,
            },
            _ => None,
        };
        n = next?;
    }
    Some(n)
}
