//! Command docc is a compiler and linter for JSON documentation.

mod comments;
mod flags;
mod rulescmd;
mod update;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::diag::Diagnostic;
use crate::engine::{self, Project};
use crate::render;
use crate::report;
use crate::schema;

use comments::cmd_comments;
use flags::{FlagError, FlagSet};
use rulescmd::{cmd_explain, cmd_rules, write_json};
use update::cmd_update;

/// Error is any error a command fails with; run tells them apart by type.
pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;

/// VERSION is the release version, set at build time with DOCC_VERSION
/// (Go: -X main.version).
const VERSION: &str = match option_env!("DOCC_VERSION") {
    Some(v) => v,
    None => "dev",
};

const USAGE: &str = r#"docc - compiler and linter for JSON documentation

Usage: docc [flags] <command> [flags]

Commands:
  init               create docc.json and docs/src/index.json, build docs/out
  validate           check sources
  build              build Markdown into docs/out (--full: audit, --single/--html: one file)
  check              read-only check for CI: validate + freshness of out/ and docc.lock
  ack <id|file>...   confirm the changed links around a document, target or code file in docc.lock (--all: every changed link)
  explain <code>     describe a rule
  rules              list rules with effective levels
  schema document|config   print a JSON Schema
  update             replace docc with the latest release (--check: only look, --version vX.Y.Z)
  comments           review comments from the HTML file: import <file>, list (--all), done <id>, rm <id>

Flags: --format text|json, --config <file>, --max-diagnostics N, --max-warnings N
(they may stand before or after the command)

Run docc <command> --help for the flags and examples of a command.
"#;

/// cmd_usage is printed by docc <command> --help, before the flag list.
fn cmd_usage(name: &str) -> &'static str {
    match name {
        "init" => {
            "Usage: docc init

Creates docc.json, docs/schemas/page.json and docs/src/index.json (existing files are kept)
and builds docs/out.

Example:
  docc init
"
        }
        "validate" => {
            "Usage: docc validate [flags]

Checks the sources in docs/src and prints diagnostics. Exit code 1: errors, or more
warnings than --max-warnings.

Examples:
  docc validate --format json
"
        }
        "build" => {
            "Usage: docc build [flags]

Validates and writes Markdown into docs/out and the links into docc.lock (new links are
confirmed, vanished ones dropped). With errors in groups 1xx-2xx nothing is written.
--single also writes every page into one Markdown file next to the directory (docs/out.md), --html
into one self-contained HTML application (docs/out.html), no network needed: a document tree,
search, dark theme, a timeline of the Git history with the edits drawn in the text, and comments
on selected words (typed or dictated) copied or downloaded for docc comments import. With --full the files are docs/out-full.md and docs/out-full.html. They
are for reading, are not checked by docc check and are usually kept out of git.

Examples:
  docc build
  docc build --full
  docc build --single
  docc build --html
  docc build --full --single --html
"
        }
        "check" => {
            "Usage: docc check [flags]

Read-only check for CI: validate, and fail when docs/out (DOC501) or docc.lock (DOC503)
differs from the sources.

Example:
  docc check --max-warnings 0
"
        }
        "ack" => {
            "Usage: docc ack <id, target or code file>... | --all

Confirms changed links in docc.lock after a review: sets the stored hashes of every
changed line whose source or target is the argument, or whose target lies in the document
with that id, to the current ones, and prints each link it confirmed. This clears DOC209
and DOC402. Links that did not change are left alone. New links are confirmed by docc build.
Read every document the output lists before you run it: the argument may match more
links than the one you reviewed.

Examples:
  docc ack req-login
  docc ack req-login#^limit
  docc ack internal/auth/login.go
  docc ack --all
"
        }
        "explain" => {
            "Usage: docc explain <code or rule name> [flags]

Describes a rule: what it checks, bad and good examples, how to configure it.

Examples:
  docc explain DOC301
  docc explain text/forbidden-words --format json
"
        }
        "rules" => {
            "Usage: docc rules [flags]

Lists every rule with its effective level.

Examples:
  docc rules
"
        }
        "schema" => {
            "Usage: docc schema document|config

Prints a JSON Schema for documents or for docc.json.

Example:
  docc schema document
"
        }
        "comments" => comments::COMMENTS_USAGE,
        _ => "",
    }
}

/// HelpError makes run print text to stdout and exit with code 0.
#[derive(Debug)]
struct HelpError {
    text: String,
}

impl fmt::Display for HelpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("help requested")
    }
}

impl std::error::Error for HelpError {}

/// say writes to a stream; a failed write to stdout or stderr has nowhere to go.
fn say(w: &mut dyn Write, s: &str) {
    let _ = w.write_all(s.as_bytes());
}

/// UsageError makes run exit with code 2 and a message on stderr.
#[derive(Debug)]
pub(crate) struct UsageError {
    msg: String,
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for UsageError {}

pub(crate) fn usage_err(msg: impl Into<String>) -> Error {
    Box::new(UsageError { msg: msg.into() })
}

/// run is main without the exit: it returns the exit code.
pub fn run(args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    if args.is_empty() {
        say(stderr, USAGE);
        return 2;
    }
    match args[0].as_str() {
        "--version" | "-version" | "version" => {
            say(stdout, &format!("docc {VERSION}\n"));
            return 0;
        }
        "--help" | "-h" | "help" => {
            say(stdout, USAGE);
            return 0;
        }
        _ => {}
    }
    let mut c = Common { format: "text".into(), config_path: String::new(), max_diag: 20, max_warnings: -1 };
    let err = match dispatch(&mut c, args, stdout, stderr) {
        Ok(code) => return code,
        Err(err) => err,
    };
    if let Some(he) = err.downcast_ref::<HelpError>() {
        say(stdout, &he.text);
        return 0;
    }
    if c.format == "json" {
        if let Err(werr) = report::json(stdout, &error_report(&*err), 0, 0, "") {
            say(stderr, &format!("docc: error: {werr}\n"));
        }
    } else {
        say(stderr, &format!("docc: error: {err}\n"));
    }
    2
}

/// error_report is the JSON form of a launch or config error: the same report as
/// a check, with one error diagnostic per problem (code "config", "usage" or "error").
fn error_report(err: &(dyn std::error::Error + Send + Sync + 'static)) -> Vec<Diagnostic> {
    let mk = |code: &str, rule: &str, file: &str, line: usize, col: usize, msg: &str, hint: &str| Diagnostic {
        code: code.into(),
        rule: rule.into(),
        level: "error".into(),
        file: file.into(),
        line,
        column: col,
        end_line: line,
        end_column: col,
        message: msg.into(),
        hint: hint.into(),
        ..Diagnostic::default()
    };
    const HINT: &str = "Fix docc.json; docc schema config prints its schema.";
    let one = |e: &config::Error| mk("config", "config/invalid", &e.file, e.line, e.col, &e.msg, HINT);
    if let Some(ces) = err.downcast_ref::<config::Errors>() {
        return ces.0.iter().map(one).collect();
    }
    if let Some(ce) = err.downcast_ref::<config::Error>() {
        return vec![one(ce)];
    }
    if let Some(schema::LoadError::Config(ce)) = err.downcast_ref::<schema::LoadError>() {
        return vec![one(ce)];
    }
    if err.is::<UsageError>() || err.is::<engine::UsageError>() {
        return vec![mk("usage", "cli/usage", "", 0, 0, &err.to_string(), "Run docc --help.")];
    }
    vec![mk("error", "cli/error", "", 0, 0, &err.to_string(), "")]
}

/// dispatch takes the global flags that stand before the command, then runs it.
fn dispatch(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let args = {
        let mut gf = new_flags("docc", c);
        if let Err(err) = gf.parse(args) {
            if err == FlagError::Help {
                return Err(Box::new(HelpError { text: USAGE.into() }));
            }
            return Err(usage_err(err.to_string()));
        }
        gf.args().to_vec()
    };
    if args.is_empty() {
        return Err(usage_err("no command given (try docc --help)"));
    }
    let rest = &args[1..];
    match args[0].as_str() {
        "validate" => cmd_validate(c, rest, stdout, stderr),
        "build" => cmd_build(c, rest, stdout, stderr),
        "check" => cmd_check(c, rest, stdout, stderr),
        "init" => cmd_init(c, rest, stdout, stderr),
        "rules" => cmd_rules(c, rest, stdout),
        "ack" => cmd_ack(c, rest, stdout),
        "explain" => cmd_explain(c, rest, stdout),
        "schema" => cmd_schema(rest, stdout).map(|_| 0),
        "update" => cmd_update(rest, stdout),
        "comments" => cmd_comments(c, rest, stdout),
        other => Err(usage_err(format!("unknown command {} (try docc --help)", config::go_quote(other)))),
    }
}

/// Common flags shared by the checking commands.
#[derive(Debug, Clone, Default)]
pub(crate) struct Common {
    pub format: String,
    pub config_path: String,
    pub max_diag: i64,
    pub max_warnings: i64,
}

fn new_flags<'a>(name: &str, c: &'a mut Common) -> FlagSet<'a> {
    let mut fs = FlagSet::new(name);
    // the defaults are the current values: global flags may be given before the command
    fs.string_var(&mut c.format, "format", "text or json");
    fs.string_var(&mut c.config_path, "config", "path to docc.json");
    fs.int_var(&mut c.max_diag, "max-diagnostics", "output limit (0: no limit)");
    fs.int_var(&mut c.max_warnings, "max-warnings", "warning limit (-1: none)");
    fs
}

/// parse accepts flags before, between and after positional arguments.
fn parse(fs: &mut FlagSet, args: &[String]) -> Result<Vec<String>, Error> {
    let mut pos = Vec::new();
    let mut args = args.to_vec();
    loop {
        if let Err(err) = fs.parse(&args) {
            if err == FlagError::Help {
                let text = format!("{}\nFlags:\n{}", cmd_usage(fs.name()), fs.defaults());
                return Err(Box::new(HelpError { text }));
            }
            return Err(usage_err(err.to_string()));
        }
        args = fs.args().to_vec();
        if args.is_empty() {
            return Ok(pos);
        }
        pos.push(args.remove(0));
    }
}

impl Common {
    fn check(&self) -> Result<(), Error> {
        if self.format != "text" && self.format != "json" {
            return Err(usage_err("--format must be text or json"));
        }
        if self.max_diag < 0 {
            return Err(usage_err("--max-diagnostics must not be negative (0: no limit)"));
        }
        if self.max_warnings < -1 {
            return Err(usage_err("--max-warnings must be 0 or more (-1: no limit)"));
        }
        Ok(())
    }
}

fn load_config(c: &Common) -> Result<Config, Error> {
    if !c.config_path.is_empty() {
        return config::load(&c.config_path, true);
    }
    if let Some(p) = find_config() {
        return config::load(&p, true);
    }
    config::load("docc.json", false)
}

/// find_config returns the nearest docc.json in the current folder or above it,
/// or None when there is none (the current folder then is the project root with
/// the defaults). Like git and biome, so hooks and agents may run docc from a
/// subfolder.
fn find_config() -> Option<String> {
    // the physical folder, as config::load resolves paths; Go walks a logical $PWD (docs/rust-port/cli.md)
    let wd = std::env::current_dir().ok()?;
    let mut dir: &Path = &wd;
    let mut up = 0;
    loop {
        let p = dir.join("docc.json");
        if std::fs::metadata(&p).is_ok_and(|st| !st.is_dir()) {
            // relative to the current folder: config errors read "docc.json:3:5" here, "../docc.json:3:5" in a subfolder
            return Some(format!("{}docc.json", "../".repeat(up)));
        }
        dir = dir.parent()?;
        up += 1;
    }
}

/// emit prints diagnostics and returns the exit code.
fn emit(stdout: &mut dyn Write, stderr: &mut dyn Write, c: &Common, p: &Project, ds: &[Diagnostic]) -> Result<i32, Error> {
    let (errs, warns) = report::counts(ds);
    let (mut limit, mut has) = (c.max_warnings, c.max_warnings >= 0);
    let mut failure = String::new();
    if !has && p.cfg.has_max_warning {
        (limit, has) = (p.cfg.max_warnings, true);
    }
    if has && warns as i64 > limit {
        failure = format!("too many warnings: {warns} > --max-warnings {limit}");
    }
    let max = c.max_diag as usize;
    if c.format == "json" {
        report::json(stdout, ds, max, p.docs.len(), &failure)?;
    } else {
        report::text(stdout, ds, max, p.docs.len())?;
        if !failure.is_empty() {
            say(stderr, &format!("{failure}\n"));
        }
    }
    if errs > 0 || !failure.is_empty() {
        return Ok(1);
    }
    Ok(0)
}

fn cmd_validate(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("validate", c), args)?;
    if let Some(a) = pos.first() {
        return Err(usage_err(format!("unexpected argument {}", config::go_quote(a))));
    }
    c.check()?;
    let cfg = load_config(c)?;
    let mut p = engine::load(cfg)?;
    let ds = p.validate();
    emit(stdout, stderr, c, &p, &ds)
}

fn cmd_schema(args: &[String], stdout: &mut dyn Write) -> Result<(), Error> {
    let pos = parse(&mut FlagSet::new("schema"), args)?;
    if pos.len() != 1 {
        return Err(usage_err("usage: docc schema document|config"));
    }
    match pos[0].as_str() {
        "document" => stdout.write_all(schema::document())?,
        "config" => stdout.write_all(&schema::config())?,
        other => return Err(usage_err(format!("unknown schema {}: use document or config", config::go_quote(other)))),
    }
    Ok(())
}

/// load_project loads the config and the project and runs validation.
fn load_project(c: &Common) -> Result<Project, Error> {
    let cfg = load_config(c)?;
    let mut p = engine::load(cfg)?;
    p.validate();
    Ok(p)
}

fn no_args(pos: &[String]) -> Result<(), Error> {
    if let Some(a) = pos.first() {
        return Err(usage_err(format!("unexpected argument {}", config::go_quote(a))));
    }
    Ok(())
}

/// Built is what build made: the number of files, the one-file builds written
/// and the error of the HTML file, which stops only that file.
struct Built {
    n: usize,
    extra: Vec<String>,
    html_err: Option<Error>,
}

/// build validates, and when there are no errors of groups 1xx-2xx writes the
/// Markdown into out (or outFull) and returns the number of files. With single
/// and html, or when that file is already there, it also writes the one-file
/// Markdown and HTML next to the directory and returns their paths in extra.
fn build(p: &Project, warn: &mut dyn Write, full: bool, mut single: bool, mut html: bool) -> Result<Built, Error> {
    let mut b = Built { n: 0, extra: Vec::new(), html_err: None };
    if engine::structural(&p.diags()) {
        return Ok(b);
    }
    let dir = if full { &p.cfg.out_full } else { &p.cfg.out };
    render::check_dir(p, dir)?;
    // a one-file build made once is kept up to date by every later build, if docc wrote it
    for (ext, on) in [(".md", &mut single), (".html", &mut html)] {
        if let Ok(f) = render::single_file(p, dir, ext) {
            if *on || std::fs::metadata(&f).is_err() {
                continue;
            }
            if render::generated_file(&f, ext) {
                *on = true;
            } else {
                say(
                    warn,
                    &format!(
                        "docc: {} is not a file docc wrote: left alone (build with --{} to replace it)\n",
                        f.display(),
                        &ext[1..]
                    ),
                );
            }
        }
    }
    let r = render::Renderer::new(p, full);
    let files = r.files();
    let mut one: Vec<(&str, String)> = Vec::new(); // extension -> content of a single-file build
    if single {
        one.push((".md", r.single()));
    }
    // a bad changesSince stops only the HTML file, not the pages and the lock
    if html {
        match r.html() {
            Ok(h) => one.push((".html", h)),
            Err(e) => b.html_err = Some(e),
        }
    }
    let mut paths: HashMap<&str, PathBuf> = HashMap::new();
    for (ext, _) in &one {
        paths.insert(ext, render::single_file(p, dir, ext)?);
    }
    render::write(&p.cfg.abs(dir), dir, &p.cfg.abs(&p.cfg.src), &files)?;
    p.write_lock()?;
    for (ext, content) in &one {
        render::write_file(&paths[ext], content.as_bytes())?;
        b.extra.push(format!("{dir}{ext}"));
    }
    b.n = files.len();
    Ok(b)
}

fn cmd_build(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let (mut full, mut single, mut html) = (false, false, false);
    let pos = {
        let mut fs = new_flags("build", c);
        fs.bool_var(&mut full, "full", "audit build into outFull");
        fs.bool_var(&mut single, "single", "also write all pages as one Markdown file next to the output directory");
        fs.bool_var(&mut html, "html", "also write all pages as one self-contained HTML file next to the output directory");
        parse(&mut fs, args)
    };
    let pos = pos?;
    no_args(&pos)?;
    c.check()?;
    let mut p = load_project(c)?;
    let b = build(&p, stderr, full, single, html)?;
    let ds = p.sorted();
    let res = emit(stdout, stderr, c, &p, &ds);
    let mut code = *res.as_ref().unwrap_or(&2);
    if let Some(e) = &b.html_err {
        // after the diagnostics, which the failed HTML part must not hide
        say(stderr, &format!("docc: error: {e}\n"));
        code = code.max(2);
    }
    if res.is_err() || c.format != "text" {
        return res.map(|_| code);
    }
    let dir = if full { &p.cfg.out_full } else { &p.cfg.out };
    if b.n > 0 {
        say(stdout, &format!("built {} files in {dir}\n", b.n));
        for f in &b.extra {
            say(stdout, &format!("built {f}\n"));
        }
    } else if engine::structural(&p.diags()) {
        let (errs, _) = report::counts(&p.diags());
        say(stdout, &format!("build skipped: {errs} errors, {dir} not changed\n"));
    }
    Ok(code)
}

fn cmd_check(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("check", c), args)?;
    no_args(&pos)?;
    c.check()?;
    let mut p = load_project(c)?;
    if !engine::structural(&p.diags()) {
        render::check_fresh(&p, &p.cfg.out, &render::Renderer::new(&p, false).files());
        p.check_lock_file();
    }
    let ds = p.sorted();
    emit(stdout, stderr, c, &p, &ds)
}

fn cmd_ack(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let mut all = false;
    let pos = {
        let mut fs = new_flags("ack", c);
        fs.bool_var(&mut all, "all", "confirm every link");
        parse(&mut fs, args)
    };
    let pos = pos?;
    if all != pos.is_empty() {
        return Err(usage_err("give the ids, targets or code files to confirm, or --all"));
    }
    c.check()?;
    let p = load_project(c)?;
    let done = p.ack(&pos, all)?;
    if c.format == "json" {
        #[derive(serde::Serialize)]
        struct Ack {
            confirmed: usize,
            links: Vec<std::collections::BTreeMap<&'static str, String>>,
        }
        let links = done
            .iter()
            .map(|l| [("source", l.source.clone()), ("target", l.target.clone())].into_iter().collect())
            .collect();
        write_json(stdout, &Ack { confirmed: done.len(), links })?;
        return Ok(0);
    }
    for l in &done {
        say(stdout, &format!("confirmed {} -> {}\n", l.source, l.target));
    }
    say(stdout, &format!("confirmed {} links in {}\n", done.len(), engine::LOCK_FILE));
    Ok(0)
}

const INIT_CONFIG: &str = r#"{
  "src": "docs/src",
  "out": "docs/out",
  "roots": ["index"],
  "maxWarnings": 0
}
"#;

const INIT_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "required": ["title"],
  "additionalProperties": true
}
"#;

const INIT_HOOK: &str = "#!/bin/sh\n# docc: refuse a commit with invalid documentation\nexec docc check\n";

/// BUILT_IGNORE are the single-file and audit builds that are not committed.
const BUILT_IGNORE: [&str; 5] = ["docs/out.md", "docs/out.html", "docs/out-full/", "docs/out-full.md", "docs/out-full.html"];

/// ignore_built adds the lines of BUILT_IGNORE that .gitignore lacks, creating the file.
fn ignore_built(stdout: &mut dyn Write) -> Result<(), Error> {
    // bytes, not a string: a file in another encoding is kept as it is
    let old = match std::fs::read(".gitignore") {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(go_path_error("open", Path::new(".gitignore"), &e)),
    };
    // a line with bytes that are not UTF-8 matches no entry of BUILT_IGNORE either way
    let have: HashSet<String> =
        old.split(|&c| c == b'\n').map(|l| String::from_utf8_lossy(l).trim().to_string()).collect();
    let mut text = old.clone();
    if !text.is_empty() && !text.ends_with(b"\n") {
        text.push(b'\n');
    }
    let mut added = false;
    for l in BUILT_IGNORE {
        if !have.contains(l) {
            text.extend_from_slice(l.as_bytes());
            text.push(b'\n');
            added = true;
        }
    }
    if !added {
        return Ok(());
    }
    write_mode(Path::new(".gitignore"), &text, 0o644)?;
    if old.is_empty() {
        say(stdout, "created .gitignore\n");
    } else {
        say(stdout, "updated .gitignore\n");
    }
    Ok(())
}

/// go_path_error is the text of Go's *fs.PathError: "<op> <path>: <reason>".
fn go_path_error(op: &str, path: &Path, err: &std::io::Error) -> Error {
    let mut text = err.to_string();
    if let Some(code) = err.raw_os_error() {
        // Rust adds " (os error N)" and capitalizes strerror; Go does neither.
        text = text.trim_end_matches(&format!(" (os error {code})")).to_string();
        let mut cs = text.chars();
        if let Some(ch) = cs.next() {
            text = ch.to_lowercase().chain(cs).collect();
        }
    }
    format!("{op} {}: {text}", path.display()).into()
}

/// write_mode is os.WriteFile(name, content, mode).
fn write_mode(name: &Path, content: &[u8], mode: u32) -> Result<(), Error> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, mode);
    #[cfg(not(unix))]
    let _ = mode;
    let mut f = o.open(name).map_err(|e| go_path_error("open", name, &e))?;
    f.write_all(content).map_err(|e| go_path_error("write", name, &e))?;
    Ok(())
}

fn cmd_init(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("init", c), args)?;
    no_args(&pos)?;
    c.check()?;
    let create = |stdout: &mut dyn Write, name: &str, content: &str, mode: u32| -> Result<(), Error> {
        let path = Path::new(name);
        if std::fs::metadata(path).is_ok() {
            say(stdout, &format!("skipped {name}: already exists\n"));
            return Ok(());
        }
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| go_path_error("mkdir", dir, &e))?;
        }
        write_mode(path, content.as_bytes(), mode)?;
        say(stdout, &format!("created {name}\n"));
        // WriteFile leaves the umask on
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
                .map_err(|e| go_path_error("chmod", path, &e))?;
        }
        Ok(())
    };
    create(stdout, "docc.json", INIT_CONFIG, 0o644)?;
    ignore_built(stdout)?;
    create(stdout, "docs/schemas/page.json", INIT_SCHEMA, 0o644)?;
    create(stdout, "docs/src/index.json", "{ \"title\": \"Index\", \"kind\": \"page\" }\n", 0o644)?;
    let mut hook = false; // a pre-commit hook is created now; an existing one is never touched
    if std::fs::metadata(".git").is_ok_and(|st| st.is_dir()) {
        let name = ".git/hooks/pre-commit";
        hook = std::fs::metadata(name).is_err();
        create(stdout, name, INIT_HOOK, 0o755)?;
    }
    let mut p = load_project(c)?;
    let b = build(&p, stderr, false, false, false)?;
    if b.n > 0 {
        say(stdout, &format!("built {} files in {}\n", b.n, p.cfg.out));
    }
    let ds = p.sorted();
    let code = emit(stdout, stderr, c, &p, &ds)?;
    if c.format == "text" {
        say(stdout, "Next:\n  1. edit docs/src/index.json and add documents (each needs a kind with a schema in docs/schemas)\n  2. docc validate --format json\n  3. agents: read skills/docc/SKILL.md in the docc repository\n");
        if hook {
            say(stdout, "  4. git commit runs docc check (.git/hooks/pre-commit) and refuses invalid documentation\n");
        }
    }
    Ok(code)
}
