use std::cell::{OnceCell, Ref, RefCell};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;

use super::analyze::{IncUse, RefUse};
use super::ignore::Ignore;
use super::strings::StrNode;
use crate::config::{Config, go_quote};
use crate::diag::{self, Diagnostic, Level};
use crate::loader::{self, Node};
use crate::rules;
use crate::schema;
use crate::source::{self, Rev, Source};
use crate::text;
use crate::trace;

static ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9]([a-z0-9-]*[a-z0-9])?$").unwrap());

fn reserved_name(name: &str) -> bool {
    match name {
        "con" | "prn" | "aux" | "nul" => true,
        _ => {
            let b = name.as_bytes();
            b.len() == 4
                && (name.starts_with("com") || name.starts_with("lpt"))
                && (b'1'..=b'9').contains(&b[3])
        }
    }
}

/// ValidID reports whether name is a valid document id or folder name.
pub fn valid_id(name: &str) -> bool {
    ID_RE.is_match(name) && !reserved_name(name)
}

/// Doc is one source file.
#[derive(Debug, Default)]
pub struct Doc {
    pub id: String,
    /// path relative to src, with '/'
    pub rel: String,
    /// path relative to the project root, with '/'
    pub file: String,
    // private: the string nodes of the tree are referenced by StrNode, so the
    // tree must never change once the document is loaded
    res: loader::Result,
    pub hidden: bool,
    /// the project is in the old format (named anchors, links without hashes,
    /// docc.lock): only docc migrate and old revisions read it
    pub legacy: bool,

    pub refs: Vec<RefUse>,
    pub incs: Vec<IncUse>,
    pub ignores: Vec<Ignore>,
    /// block anchor name -> index in strings() of the first string with it
    pub blocks: HashMap<String, usize>,

    pub(super) block_of: HashMap<*const Node, usize>,

    pub(super) strs: OnceCell<Vec<StrNode>>,
}

impl Doc {
    /// Res is the parse result of the file.
    pub fn res(&self) -> &loader::Result {
        &self.res
    }

    /// Root is the root object, None when the file does not parse.
    pub fn root(&self) -> Option<&Node> {
        if self.res.err.is_some() {
            None
        } else {
            self.res.root.as_ref()
        }
    }

    /// Parsed reports whether the file is valid JSON with an object root.
    pub fn parsed(&self) -> bool {
        self.root().is_some()
    }

    /// root_obj is the root of a parsed document.
    pub(super) fn r(&self) -> &Node {
        self.root().expect("a parsed document")
    }

    /// BlockAnchor returns the first string that carries the block anchor name
    /// (Go: Blocks[name]).
    pub fn block_anchor(&self, name: &str) -> Option<&StrNode> {
        self.blocks.get(name).map(|&i| &self.strings()[i])
    }
}

/// Project is the loaded project.
pub struct Project {
    pub cfg: Config,
    pub schemas: schema::Set,
    /// every file with a valid name, ordered by path
    pub docs: Vec<Doc>,
    /// first document per id: index in docs
    pub graph: HashMap<String, usize>,
    pub(super) diags: RefCell<Vec<Diagnostic>>,
    /// document id -> sorted code files with a docc: tag to it (only when trace.scan is set)
    pub covers: Option<HashMap<String, Vec<String>>>,
    /// every resolved (code file, target) pair -> hash of its regions, "" for a plain tag
    pub code: HashMap<trace::Key, String>,

    /// first tag line and column per pair of code
    pub(super) code_at: HashMap<trace::Key, (usize, usize)>,
    pub(super) by_file: HashMap<String, usize>,
    pub(super) matchers: RefCell<HashMap<String, Rc<text::Matcher>>>,
    /// the project is in the old format: it has a docc.lock (Doc::legacy)
    pub legacy: bool,
    /// the blocks some [[id#^code]] links to, as "id#code": only they get an
    /// anchor tag in the Markdown
    pub linked: std::collections::HashSet<String>,
}

/// UsageError is a problem with the environment (exit code 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    pub msg: String,
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl Error for UsageError {}

pub(super) fn usage(msg: String) -> Box<dyn Error + Send + Sync> {
    Box::new(UsageError { msg })
}

/// go_path_error is the text of Go's *fs.PathError: "<op> <path>: <reason>".
pub(super) fn go_path_error(op: &str, path: &Path, err: &std::io::Error) -> String {
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

/// MIGRATE is what docc says about a project of the old format.
pub const MIGRATE: &str = "this project is in the old format of docc (docc.lock, named anchors ^name, links without version hashes): run docc migrate once, then docc build";

/// Load reads the config, schemas and all documents, and emits the file-level
/// diagnostics (DOC101, DOC102, DOC104). A project of the old format (it has
/// a docc.lock) is refused on the disk; a Git revision of it is read as it
/// was, so the history and old revisions still show.
pub fn load(cfg: Config) -> Result<Project, Box<dyn Error + Send + Sync>> {
    let p = load_any(cfg)?;
    if p.legacy && !matches!(p.cfg.source, Source::Rev(_)) {
        return Err(usage(MIGRATE.into()));
    }
    Ok(p)
}

/// load_any is load that also reads a project of the old format (docc migrate).
pub fn load_any(cfg: Config) -> Result<Project, Box<dyn Error + Send + Sync>> {
    let set = schema::load(&cfg.abs(&cfg.schemas), &cfg.schemas, &cfg.source)?;
    let mut p = Project {
        cfg,
        schemas: set,
        docs: Vec::new(),
        graph: HashMap::new(),
        diags: RefCell::new(Vec::new()),
        covers: None,
        code: HashMap::new(),
        code_at: HashMap::new(),
        by_file: HashMap::new(),
        matchers: RefCell::new(HashMap::new()),
        legacy: false,
        linked: Default::default(),
    };
    p.legacy = p.cfg.source.entry(&p.cfg.abs(super::LOCK_FILE)) == Some(source::Entry::File);
    let src_abs = p.cfg.abs(&p.cfg.src);
    if p.cfg.source.entry(&src_abs) != Some(source::Entry::Dir) {
        return Err(usage(format!(
            "source directory {} not found",
            go_quote(&p.cfg.src)
        )));
    }
    let mut paths = Vec::new();
    if let Source::Rev(rev) = &p.cfg.source {
        p.walk_rev(rev, &src_abs, &mut paths);
    } else if !std::fs::symlink_metadata(&src_abs).is_ok_and(|m| m.file_type().is_symlink()) {
        // filepath.WalkDir Lstat's the root: a src that is itself a symbolic
        // link is not a directory to it, so nothing below it is loaded
        p.walk(&src_abs, b"", &mut paths)?;
    }
    paths.sort();
    for (raw, abs) in paths {
        let name = &raw[raw.iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1)..];
        let id = String::from_utf8_lossy(name.strip_suffix(b".json").unwrap_or(name)).into_owned();
        let rel = String::from_utf8_lossy(&raw).into_owned();
        let file = format!("{}/{}", p.cfg.src, rel);
        if !valid_id(&id) {
            p.emit_file(
                &file,
                "DOC104",
                1,
                1,
                format!(
                    "file name {} is not a valid id",
                    go_quote_bytes(name.strip_suffix(b".json").unwrap_or(name))
                ),
                "Rename the file: use only a-z, 0-9 and '-', not starting or ending with '-', and avoid reserved Windows names.".into(),
            );
            continue;
        }
        let mut d = Doc {
            id,
            rel,
            file,
            legacy: p.legacy,
            ..Default::default()
        };
        p.read(&mut d, &abs);
        p.by_file.insert(d.file.clone(), p.docs.len());
        p.docs.push(d);
    }
    for (i, d) in p.docs.iter().enumerate() {
        p.graph.entry(d.id.clone()).or_insert(i);
    }
    Ok(p)
}

/// go_quote_bytes is Go's %q of a string that may hold bytes that are not
/// UTF-8 (a file name): each such byte is written as \xNN.
fn go_quote_bytes(b: &[u8]) -> String {
    let mut out = String::from("\"");
    for c in b.utf8_chunks() {
        let q = go_quote(c.valid());
        out.push_str(&q[1..q.len() - 1]);
        for x in c.invalid() {
            out.push_str(&format!("\\x{x:02x}"));
        }
    }
    out.push('"');
    out
}

/// has_sub reports whether data contains pat (bytes.Contains).
fn has_sub(data: &[u8], pat: &[u8]) -> bool {
    data.windows(pat.len()).any(|w| w == pat)
}

impl Project {
    /// walk_rev is walk over the tree of a Git revision: the same files, the
    /// same DOC104 for folder names; the contents are read with one git process.
    fn walk_rev(&self, rev: &Rev, src_abs: &Path, paths: &mut Vec<(Vec<u8>, PathBuf)>) {
        let mut dirs = std::collections::BTreeSet::new();
        for (rel, b) in rev.files(&self.cfg.src) {
            let parts: Vec<&str> = rel.split('/').collect();
            if parts.iter().any(|n| n.starts_with('.')) || b.link {
                continue;
            }
            for i in 1..parts.len() {
                dirs.insert(parts[..i].join("/"));
            }
            if rel.ends_with(".json") {
                paths.push((rel.as_bytes().to_vec(), src_abs.join(rel)));
            }
        }
        for d in dirs {
            let name = &d[d.rfind('/').map_or(0, |i| i + 1)..];
            if !valid_id(name) {
                self.emit_file(
                    &format!("{}/{d}", self.cfg.src),
                    "DOC104",
                    1,
                    1,
                    format!("folder name {} is not a valid id", go_quote(name)),
                    "Use only a-z, 0-9 and '-' in folder names, not starting or ending with '-', and avoid reserved Windows names.".into(),
                );
            }
        }
        let rels: Vec<String> = paths.iter().map(|(r, _)| format!("{}/{}", self.cfg.src, String::from_utf8_lossy(r))).collect();
        rev.prefetch(rels.iter().map(String::as_str));
    }

    /// walk is filepath.WalkDir below src: entries in lexical order, names
    /// starting with '.' and symbolic links skipped, DOC104 for a folder name
    /// that is not an id; the .json regular files go to paths as their path
    /// relative to src (bytes, as Go keeps a name that is not UTF-8) and the
    /// OS path.
    fn walk(
        &self,
        dir: &Path,
        rel_dir: &[u8],
        paths: &mut Vec<(Vec<u8>, PathBuf)>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let rd = std::fs::read_dir(dir).map_err(|e| usage(go_path_error("open", dir, &e)))?;
        let mut ents = Vec::new();
        for e in rd {
            let e = e.map_err(|e| usage(go_path_error("readdirent", dir, &e)))?;
            ents.push(e);
        }
        ents.sort_by_key(|e| e.file_name());
        for e in ents {
            let os_name = e.file_name();
            let name = os_name.as_encoded_bytes();
            let ft = e
                .file_type()
                .map_err(|err| usage(go_path_error("lstat", &e.path(), &err)))?;
            if name.starts_with(b".") || ft.is_symlink() {
                continue;
            }
            let mut rel = rel_dir.to_vec();
            if !rel.is_empty() {
                rel.push(b'/');
            }
            rel.extend_from_slice(name);
            if ft.is_dir() {
                if !std::str::from_utf8(name).is_ok_and(valid_id) {
                    self.emit_file(
                        &format!("{}/{}", self.cfg.src, String::from_utf8_lossy(&rel)),
                        "DOC104",
                        1,
                        1,
                        format!("folder name {} is not a valid id", go_quote_bytes(name)),
                        "Use only a-z, 0-9 and '-' in folder names, not starting or ending with '-', and avoid reserved Windows names.".into(),
                    );
                }
                self.walk(&e.path(), &rel, paths)?;
                continue;
            }
            if name.ends_with(b".json") && ft.is_file() {
                paths.push((rel, e.path()));
            }
        }
        Ok(())
    }

    fn read(&self, d: &mut Doc, abs: &Path) {
        let mut data = Vec::new();
        let err_res = |msg: String| loader::Result {
            err: Some(loader::Error { offset: 0, msg }),
            ..Default::default()
        };
        if self.cfg.source.len(abs).is_some_and(|n| n > loader::MAX_FILE_SIZE as u64) {
            d.res = err_res("file is larger than 5 MB".into());
        } else {
            match self.cfg.source.read(abs) {
                Err(e) => {
                    d.res = err_res(format!(
                        "cannot read file: {}",
                        go_path_error("open", abs, &e)
                    ))
                }
                Ok(b) => {
                    d.res = loader::parse(&b);
                    data = b;
                }
            }
        }
        if let Some(e) = &d.res.err {
            let msg = e.msg.as_str();
            let mut hint = "Fix the JSON syntax: strict JSON only, no comments or trailing commas.";
            if msg.contains("file is empty") {
                hint = r#"The file is empty: write an object such as {"title": "..."} or delete the file."#;
            } else if msg.contains("larger than 5 MB") {
                hint = "The file is too big for one document: split it into several documents and link them with [[id]].";
            } else if msg.contains("not valid UTF-8") {
                hint = "The file is not UTF-8 (for example Windows-1251 or UTF-16): save it again as UTF-8.";
            } else if msg.contains("must be an object") {
                hint = r#"The top-level value must be an object: wrap it as {"title": "..."}."#;
            }
            if has_sub(&data, b"<<<<<<<") || has_sub(&data, b">>>>>>>") {
                hint = "Resolve the merge conflict markers in the file first.";
            }
            let (off, m) = (e.offset, format!("invalid JSON: {}", e.msg));
            self.emit(d, "DOC101", off, off, m, hint.into());
            return;
        }
        for dup in &d.res.dups {
            self.emit(
                d,
                "DOC102",
                dup.start,
                dup.end,
                format!("duplicate key {}", go_quote(&dup.key)),
                "Remove one of the repeated keys or merge their values.".into(),
            );
        }
        if let Some(h) = d.r().get("hidden")
            && h.kind == loader::Kind::Bool
            && h.str == "true"
        {
            d.hidden = true;
        }
    }

    /// doc returns the graph node of an id: the first document by path.
    pub fn doc(&self, id: &str) -> Option<&Doc> {
        self.graph.get(id).map(|&i| &self.docs[i])
    }

    /// Diags returns the diagnostics found so far.
    pub fn diags(&self) -> Ref<'_, Vec<Diagnostic>> {
        self.diags.borrow()
    }

    fn new_diag(file: &str, code: &str) -> Diagnostic {
        let r = rules::by_code(code).expect("a registered rule");
        Diagnostic {
            code: code.to_string(),
            rule: r.name.to_string(),
            file: file.to_string(),
            ..Default::default()
        }
    }

    fn push(&self, mut dg: Diagnostic, lvl: Level) {
        dg.lvl = lvl;
        dg.level = lvl.to_string();
        self.diags.borrow_mut().push(dg);
    }

    pub(super) fn level(&self, code: &str) -> Level {
        self.cfg.resolve().rule(code).level
    }

    /// emit adds a diagnostic on a document, at the byte range [start,end).
    /// Nothing is added when the rule is off.
    pub(super) fn emit(
        &self,
        d: &Doc,
        code: &str,
        start: usize,
        end: usize,
        msg: String,
        hint: String,
    ) {
        let lvl = self.level(code);
        if lvl == Level::Off {
            return;
        }
        self.emit_at(
            d,
            code,
            rules::by_code(code).expect("a registered rule").name,
            lvl,
            start,
            end,
            msg,
            hint,
        );
    }

    /// emitAt adds a document diagnostic with an explicit rule name and level
    /// (custom rules use DOC800 with their own name).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_at(
        &self,
        d: &Doc,
        code: &str,
        name: &str,
        lvl: Level,
        start: usize,
        end: usize,
        msg: String,
        hint: String,
    ) {
        let mut dg = Self::new_diag(&d.file, code);
        dg.rule = name.to_string();
        dg.doc_id = d.id.clone();
        (dg.message, dg.hint) = (msg, hint);
        (dg.start, dg.end) = (start, end);
        (dg.line, dg.column) = d.res.src.position(start);
        (dg.end_line, dg.end_column) = d.res.src.position(end);
        self.push(dg, lvl);
    }

    /// emitNoPos adds a document diagnostic that has no place in the JSON text.
    pub(super) fn emit_no_pos(&self, d: &Doc, code: &str, msg: String, hint: String) {
        let lvl = self.level(code);
        if lvl == Level::Off {
            return;
        }
        let mut dg = Self::new_diag(&d.file, code);
        dg.doc_id = d.id.clone();
        (dg.message, dg.hint) = (msg, hint);
        (dg.line, dg.column, dg.end_line, dg.end_column) = (1, 1, 1, 1);
        dg.no_pos = true;
        self.push(dg, lvl);
    }

    /// emitFile adds a diagnostic on a non-document file (config, folder, code, out).
    pub(super) fn emit_file(
        &self,
        file: &str,
        code: &str,
        line: usize,
        col: usize,
        msg: String,
        hint: String,
    ) {
        let lvl = self.level(code);
        if lvl == Level::Off {
            return;
        }
        let mut dg = Self::new_diag(file, code);
        (dg.message, dg.hint) = (msg, hint);
        (dg.line, dg.column, dg.end_line, dg.end_column) = (line, col, line, col);
        dg.no_pos = true;
        self.push(dg, lvl);
    }

    /// DocByFile returns the document for a project-relative file path.
    pub fn doc_by_file(&self, file: &str) -> Option<&Doc> {
        self.by_file.get(file).map(|&i| &self.docs[i])
    }

    /// Off reports whether a rule is disabled, so a whole pass can be skipped.
    pub fn off(&self, code: &str) -> bool {
        !self.cfg.resolve().on(code)
    }

    /// ReportFile adds a diagnostic on a generated or scanned file (DOC501)
    /// and keeps the list sorted.
    pub fn report_file(
        &self,
        file: &str,
        code: &str,
        line: usize,
        col: usize,
        msg: &str,
        hint: &str,
    ) {
        self.emit_file(file, code, line, col, msg.to_string(), hint.to_string());
    }

    /// Sorted sorts the diagnostics and returns them.
    pub fn sorted(&mut self) -> Vec<Diagnostic> {
        let ds = self.diags.get_mut();
        diag::sort(ds);
        ds.clone()
    }
}
