//! Command docc is a compiler and linter for JSON documentation.

mod comments;
mod flags;
mod rulescmd;
mod update;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::changes;
use crate::config::{self, Config};
use crate::diag::Diagnostic;
use crate::engine::{self, Project};
use crate::render;
use crate::report;
use crate::schema;
use crate::source::{Rev, Source};

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
  validate [paths]   check sources (paths: report only the problems in those files and folders)
  build              build Markdown into docs/out (--full: audit, --single/--html: one file, --no-comments, --since, --page, --rev)
  compile            print the pages, tree and diagnostics as one JSON object; writes nothing (--rev: a Git revision, --hidden: with hidden documents)
  log                print the commits that changed the documents, with the files and block counts of each
  diff <base> <head> print the blocks of the documents of two revisions lined up (--path: one document)
  check [paths]      read-only check for CI and hooks: validate + freshness of out/ (paths: as validate)
  codes              give a code to every block that has none (build does it too)
  migrate            move a project of the old format (docc.lock, named anchors) to block codes and version hashes
  explain <code>     describe a rule
  rules              list rules with effective levels
  schema document|config|compile|log|diff|comments-apply   print a JSON Schema
  update             replace docc with the latest release (--check: only look, --version vX.Y.Z)
  comments           the readers' comments in the documents: list, apply (edits from stdin), import <file>, rm <id>

Flags: --format text|json, --config <file>, --max-diagnostics N, --max-warnings N
(they may stand before or after the command)

Run docc <command> --help for the flags and examples of a command.
"#;

/// cmd_usage is printed by docc <command> --help, before the flag list.
fn cmd_usage(name: &str) -> &'static str {
    match name {
        "init" => {
            "Usage: docc init

Creates docc.json (with \"commitOut\": false: docs/out is built but not committed, so it
goes into .gitignore with the one-file builds), docs/schemas/page.json and
docs/src/index.json (existing files are kept) and builds docs/out.

Example:
  docc init
"
        }
        "validate" => {
            "Usage: docc validate [paths...] [flags]

Checks the sources in docs/src and prints diagnostics. Exit code 1: errors, or more
warnings than --max-warnings. Paths (files or folders, relative to the current folder or
absolute, inside the project) limit the report, the exit code and --max-warnings to the
diagnostics of files under them, as docc check <paths> does; the whole project is still
loaded and checked.

Examples:
  docc validate --format json
  docc validate docs/src/decisions --format json
"
        }
        "build" => {
            "Usage: docc build [flags]

First gives a code to every block that has none (as docc codes; never a hash), then
validates and writes Markdown into docs/out. With errors in groups 1xx-2xx nothing is written,
except a link whose version hash is missing or old (DOC209) and a block without a code
(DOC216): the pages are written and the exit code is 1.
--single also writes every page into one Markdown file next to the directory (docs/out.md), --html
into one self-contained HTML application (docs/out.html), no network needed: a document tree,
search, dark theme, a timeline of the Git history with the edits drawn in the text, and comments
on selected words (typed or dictated) copied or downloaded for docc comments import. With
--full the files are docs/out-full.md and docs/out-full.html. Once written, such a file is
rebuilt by every later docc build. A program that shows the pages itself reads docc compile.
They are for reading, are not checked by docc check and are usually kept out of git.
The readers' comments (\"$comments\" of the documents) stand under their blocks in the
Markdown and the HTML file, those that lost their place at the top of the page;
--no-comments leaves them out. --since names the revision (tag, branch, commit) of the
version the reader last received: the HTML file shows the edits since it and no history
before it. Fenced ```mermaid blocks are drawn as diagrams in the HTML file (the drawing
library goes into the file only when a page has one); elsewhere they stay code.
--page writes one page alone: its Markdown page in docs/out with the pictures it shows (no
other page, no _tree.md; a later build without --page writes them back) and, with --single
or --html, a one-file build of that page. It is named by its path relative to src as docc
compile gives it (decisions/monolith.json, .json may be left out) or by its id.
--rev builds the tree of a Git revision without a checkout, as docc compile --rev reads it
(a bare mirror too, with --config naming the project's docc.json): it needs --html and writes
only that file, into the current folder (out.html, out-full.html with --full), with the
history up to that revision; docs/out and the sources are not written.

Examples:
  docc build
  docc build --full
  docc build --single
  docc build --html
  docc build --html --since sent-2026-10-06 --no-comments
  docc build --full --single --html
  docc build --page decisions/monolith.json --html
  docc build --rev 3f2a91bc --html --config docs/docc.json
"
        }
        "compile" => {
            "Usage: docc compile [--rev <revision>] [--hidden] [flags]

Prints what the sources compile to, for a program that shows the pages itself, and writes
nothing. With --format json (the format for programs) it is one JSON object on one line:
$schema, ok, summary and diagnostics as validate reports them, project, tree and the pages
of the reader (no hidden documents, no notes) cut into blocks with their HTML and sections.
--hidden adds the hidden documents to pages and tree, each marked hidden: true; notes stay out and
the diagnostics are the same (a visible document linking to a hidden one is still DOC210).
The object is printed even when the sources are broken: then pages and tree are empty and
ok is false. Exit code as validate: 0 ok, 1 errors or too many warnings, 2 failure.
--rev compiles the tree of a Git revision (commit, branch, tag) without a checkout.
docc schema compile prints the JSON Schema of the object.

Examples:
  docc compile --format json
  docc compile --rev HEAD~3 --format json
  docc compile --hidden --format json
"
        }
        "log" => {
            "Usage: docc log [--rev <revision>] [--since <commit>] [flags]

Prints the commits of the first-parent chain of --rev (default HEAD), newest first, that
changed anything in the source folder, down to --since (not included). Each commit has its
full hash, parents, author, committer, date, subject and body, and the files it changed:
status A, M, D or R, path relative to src (oldPath for a rename), id and title of a document,
how many blocks it added, changed and removed (the alignment of docc diff), and commentsOnly
when only its \"$comments\" changed. A deleted document keeps its last title. With --format
json (the format for programs) it is one JSON object on one line; docc schema log prints its
JSON Schema. Works in a bare mirror with --config naming the project's docc.json in it.

Examples:
  docc log --format json
  docc log --since 1a2b3c4 --format json
"
        }
        "diff" => {
            "Usage: docc diff <base> <head> [--path <file>] [flags]

Lines up the blocks of the documents of two revisions (any commit, branch, tag or tree, the
empty tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904 for a root commit). Each side is compiled
from its own revision, as docc compile --rev does: blocks are those of compile, links and
includes resolved in that revision. Without --path: the changed documents, each with the blocks
it added, removed or changed. With --path (a path relative to src, of either side): that
document with every block, the unchanged ones as \"same\", in an order for two columns. With
--format json it is one JSON object on one line; docc schema diff prints its JSON Schema.

Examples:
  docc diff HEAD~1 HEAD --format json
  docc diff HEAD~5 HEAD --path decisions/monolith.json --format json
"
        }
        "check" => {
            "Usage: docc check [paths...] [flags]

Read-only check for CI: validate (a block without a code is DOC216, a link whose version
hash is missing or old is DOC209), and fail when docs/out (DOC501) differs from the sources. docs/out may be built with or without the comments; with
\"commitOut\": false in docc.json it is not compared.
Paths (files or folders, relative to the current folder or absolute, inside the project)
scope the check to them, for the pre-commit hook of one part of a project (a git submodule):
the whole project is still loaded and its whole graph built, but the report, the exit code
and --max-warnings count only the diagnostics of files under the paths, and docs/out is
compared only when a path is docs/out, inside it or above it. DOC404 then also reports a
block whose tag stood under a path at HEAD of that path's own Git repository (a submodule's
own) and that no tag names now: this change removed its last tag. It is reported at the
place of the removed tag; a block nobody has tagged yet is left to the full check.
Without paths everything is reported.

Examples:
  docc check --max-warnings 0
  cd .. && docc check backend --max-warnings 0   # the hook of the submodule backend/
"
        }
        "codes" => {
            "Usage: docc codes [flags]

Gives a code to every block of the documents that has none (a new paragraph, a list item, the
text of a field) and prints how many it gave: the source files get \" ^PAY-14\" at the end of
those blocks (the key of the document, a dash and its largest number plus one), and a document
without \"key\" gets one; nothing else changes. It never touches a code that is there, never writes a
version hash and checks nothing; docc build does the same before it builds. For a program
that writes documents (Docspace) and needs the codes in at once.

Examples:
  docc codes
  docc codes --format json
"
        }
        "migrate" => {
            "Usage: docc migrate [flags]

Moves a project of an old format to block codes ^PAY-14 and version hashes, once, without
asking anything. The old formats: codes of four signs (^k7f2, docc 0.5 to 0.7), or a docc.lock
with named anchors ^name and links without version hashes. Every document with blocks gets a
\"key\" and every block a code of it (an old code or named anchor becomes one); every link in
the documents and every id#^code in the code files of trace.scan (a docc: tag,
<Req to=\"pay#^PAY-14\">) follows; \"at\" of the readers' comments follows its block. Links
and the targets of docc: tags get the version hash of their target, docc: tags the
fingerprint of their code. A hash or fingerprint that matched before still matches (only the
codes in its target changed), one that did not still does not: nothing is confirmed by the
move. docc.lock is deleted; a link or region it held unconfirmed gets a hash that does not
match. Running it again changes nothing. Then run docc build and commit.

Examples:
  docc migrate
"
        }
        "explain" => {
            "Usage: docc explain <code or rule name> [flags]

Describes a rule: what it checks, bad and good examples, how to configure it.

Examples:
  docc explain text/forbidden-words
  docc explain DOC301 --format json   # a code works too
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
            "Usage: docc schema document|config|compile|log|diff|comments-apply

Prints a JSON Schema for documents, for docc.json or for the output of docc compile, log,
diff or comments apply.

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
        "codes" => cmd_codes(c, rest, stdout),
        "migrate" => cmd_migrate(c, rest, stdout),
        "explain" => cmd_explain(c, rest, stdout),
        "schema" => cmd_schema(rest, stdout).map(|_| 0),
        "compile" => cmd_compile(c, rest, stdout, stderr),
        "log" => cmd_log(c, rest, stdout),
        "diff" => cmd_diff(c, rest, stdout),
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
    load_config_at(c, "")
}

/// load_config_at loads the config with the project read from the Git
/// revision rev ("" for the disk). The project root is found on the disk as
/// always (or given with --config, which need not exist on the disk then: a
/// bare mirror has no checkout); a docc.json missing in the revision means
/// the defaults, as a project without one has.
fn load_config_at(c: &Common, rev: &str) -> Result<Config, Error> {
    let open = |root: &Path| -> Result<Source, Error> {
        if rev.is_empty() {
            return Ok(Source::Disk);
        }
        Ok(Source::Rev(Arc::new(Rev::open(root, rev).map_err(usage_err)?)))
    };
    if !c.config_path.is_empty() {
        return config::load_from(&c.config_path, rev.is_empty(), open);
    }
    if let Some(p) = find_config() {
        return config::load_from(&p, rev.is_empty(), open);
    }
    config::load_from("docc.json", false, open)
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

/// warning_failure is why the exit code is 1 without errors: more warnings
/// than --max-warnings (or maxWarnings); "" when there are not.
fn warning_failure(c: &Common, p: &Project, ds: &[Diagnostic]) -> String {
    let (_, warns) = report::counts(ds);
    let (mut limit, mut has) = (c.max_warnings, c.max_warnings >= 0);
    if !has && p.cfg.has_max_warning {
        (limit, has) = (p.cfg.max_warnings, true);
    }
    if has && warns as i64 > limit {
        return format!("too many warnings: {warns} > --max-warnings {limit}");
    }
    String::new()
}

/// emit prints diagnostics and returns the exit code.
fn emit(stdout: &mut dyn Write, stderr: &mut dyn Write, c: &Common, p: &Project, ds: &[Diagnostic]) -> Result<i32, Error> {
    let (errs, _) = report::counts(ds);
    let failure = warning_failure(c, p, ds);
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
    c.check()?;
    let mut p = load_scoped(c, &pos)?;
    let mut ds = p.sorted();
    ds.retain(|d| p.in_scope(&d.file));
    emit(stdout, stderr, c, &p, &ds)
}

/// load_scoped loads and validates the whole project, with the paths of
/// docc check/validate <paths> as its scope.
fn load_scoped(c: &Common, paths: &[String]) -> Result<Project, Error> {
    let cfg = load_config(c)?;
    let scope = scope(&cfg.root, paths)?;
    let mut p = engine::load(cfg)?;
    p.scope = scope;
    p.validate();
    Ok(p)
}

/// scope turns paths (relative to the current folder, or absolute) into
/// project-relative '/' paths, "" for the root. A path must exist and lie
/// inside the project; links are resolved, as the project root is found.
fn scope(root: &Path, paths: &[String]) -> Result<Vec<String>, Error> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let root = std::fs::canonicalize(root).map_err(|e| go_path_error("open", root, &e))?;
    paths
        .iter()
        .map(|a| {
            let abs = std::fs::canonicalize(a).map_err(|e| go_path_error("open", Path::new(a), &e))?;
            let rel = abs.strip_prefix(&root).map_err(|_| {
                usage_err(format!("{} is outside the project {}", config::go_quote(a), root.display()))
            })?;
            let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
            parts.map(|p| p.join("/")).ok_or_else(|| usage_err(format!("{} is not a UTF-8 path", config::go_quote(a))))
        })
        .collect()
}

/// cmd_compile prints what the sources compile to as one JSON object on
/// stdout and writes nothing: the diagnostics of validate and, without
/// errors of groups 1xx-2xx, the tree and the pages of the reader. The object
/// is printed in every case, a failure (exit code 2) included.
fn cmd_compile(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let mut rev = String::new();
    let mut hidden = false;
    let pos = {
        let mut fs = new_flags("compile", c);
        fs.bool_var(&mut hidden, "hidden", "also compile the hidden documents (\"hidden\": true), marked hidden in tree and pages; notes stay out");
        fs.string_var(&mut rev, "rev", "compile the tree of this Git revision (commit, branch, tag) instead of the files on disk");
        parse(&mut fs, args)
    };
    if let Err(e) = &pos
        && e.is::<HelpError>()
    {
        return pos.map(|_| 0);
    }
    let res = pos.and_then(|pos| {
        no_args(&pos)?;
        c.check()?;
        let mut p = engine::load(load_config_at(c, &rev)?)?;
        p.validate();
        let out = if engine::structural(&p.diags()) {
            render::Compiled { project: render::Renderer::new(&p, false).project_info(String::new()), ..Default::default() }
        } else {
            let mut r = render::Renderer::new(&p, false);
            r.hidden = hidden;
            r.compile()
        };
        Ok((p, out))
    });
    let (mut p, out) = match res {
        Ok(v) => v,
        Err(e) if c.format == "json" => {
            let ds = error_report(&*e);
            write_compiled(stdout, false, &ds, 0, "", &render::Compiled::default())?;
            return Ok(2);
        }
        Err(e) => return Err(e),
    };
    let ds = p.sorted();
    if c.format != "json" {
        let code = emit(stdout, stderr, c, &p, &ds)?;
        if out.pages.is_empty() && engine::structural(&ds) {
            say(stdout, "compile skipped: errors in groups 1xx-2xx\n");
        } else {
            say(stdout, &format!("compiled {} pages\n", out.pages.len()));
        }
        return Ok(code);
    }
    let failure = warning_failure(c, &p, &ds);
    let (errs, _) = report::counts(&ds);
    let ok = errs == 0 && failure.is_empty();
    write_compiled(stdout, ok, &ds, p.docs.len(), &failure, &out)?;
    Ok(if ok { 0 } else { 1 })
}

/// write_compiled writes the object of docc compile on one line.
fn write_compiled(
    w: &mut dyn Write,
    ok: bool,
    ds: &[Diagnostic],
    checked: usize,
    failure: &str,
    out: &render::Compiled,
) -> Result<(), Error> {
    #[derive(serde::Serialize)]
    struct Obj<'a> {
        #[serde(rename = "$schema")]
        schema: &'static str,
        ok: bool,
        summary: report::Summary,
        diagnostics: &'a [Diagnostic],
        #[serde(flatten)]
        out: &'a render::Compiled,
    }
    let (errors, warnings) = report::counts(ds);
    let summary = report::Summary { checked, errors, warnings, shown: ds.len(), truncated: false, failure: failure.into() };
    let s = serde_json::to_string(&Obj { schema: render::COMPILE_SCHEMA_ID, ok, summary, diagnostics: ds, out })?
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    Ok(())
}

/// write_line writes v as JSON on one line.
fn write_line(w: &mut dyn Write, v: &impl serde::Serialize) -> Result<(), Error> {
    let s = serde_json::to_string(v)?.replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029");
    w.write_all(s.as_bytes())?;
    w.write_all(b"\n")?;
    Ok(())
}

/// blocks_text is the block counts of a file for the text output.
fn blocks_text(n: changes::Counts) -> String {
    format!("+{} ~{} -{}", n.add, n.ch, n.rm)
}

/// cmd_log prints the commits that changed the source folder (changes::log).
fn cmd_log(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let (mut rev, mut since) = ("HEAD".to_string(), String::new());
    let pos = {
        let mut fs = new_flags("log", c);
        fs.string_var(&mut rev, "rev", "the last commit (commit, branch, tag)");
        fs.string_var(&mut since, "since", "stop before this commit of the first-parent chain");
        parse(&mut fs, args)
    }?;
    no_args(&pos)?;
    c.check()?;
    let cfg = load_config_at(c, &rev)?;
    let out = changes::log(&cfg, &rev, &since).map_err(usage_err)?;
    if c.format == "json" {
        write_line(stdout, &out)?;
        return Ok(0);
    }
    for cm in &out.commits {
        say(stdout, &format!("{} {} {}\n", &cm.sha[..cm.sha.len().min(8)], cm.date, cm.subject));
        for f in &cm.files {
            say(stdout, &format!("  {} {} {}\n", f.status, f.path, blocks_text(f.blocks)));
        }
    }
    Ok(0)
}

/// cmd_diff prints the blocks of two revisions lined up (changes::diff).
fn cmd_diff(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let mut path = String::new();
    let pos = {
        let mut fs = new_flags("diff", c);
        fs.string_var(&mut path, "path", "one document (path relative to src) with all its blocks");
        parse(&mut fs, args)
    }?;
    if pos.len() != 2 {
        return Err(usage_err("usage: docc diff <base> <head> [--path <file>]"));
    }
    c.check()?;
    let out = changes::diff(|rev| load_config_at(c, rev), |e| e.is::<UsageError>(), &pos[0], &pos[1], &path)?;
    if c.format == "json" {
        write_line(stdout, &out)?;
        return Ok(0);
    }
    for f in &out.files {
        let mut n = changes::Counts::default();
        for b in &f.blocks {
            match b.op {
                "add" => n.add += 1,
                "ch" => n.ch += 1,
                "rm" => n.rm += 1,
                _ => {}
            }
        }
        say(stdout, &format!("{} {} {}\n", f.status, f.path, blocks_text(n)));
    }
    Ok(0)
}

fn cmd_schema(args: &[String], stdout: &mut dyn Write) -> Result<(), Error> {
    let pos = parse(&mut FlagSet::new("schema"), args)?;
    if pos.len() != 1 {
        return Err(usage_err("usage: docc schema document|config|compile|log|diff|comments-apply"));
    }
    match pos[0].as_str() {
        "document" => stdout.write_all(schema::document())?,
        "config" => stdout.write_all(&schema::config())?,
        "compile" => stdout.write_all(schema::compile_output())?,
        "log" => stdout.write_all(schema::log_output())?,
        "diff" => stdout.write_all(schema::diff_output())?,
        "comments-apply" => stdout.write_all(schema::comments_apply_output())?,
        other => {
            return Err(usage_err(format!(
                "unknown schema {}: use document, config, compile, log, diff or comments-apply",
                config::go_quote(other)
            )));
        }
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

/// BuildOpts are the flags of docc build that change what is written.
#[derive(Default)]
struct BuildOpts {
    /// leave the readers' comments out (--no-comments)
    no_comments: bool,
    /// the revision the HTML file counts the edits from (--since)
    since: String,
    /// the id of the one page to write (--page); "" for every page
    page: String,
    /// the sources come from a Git revision (--rev): only the HTML file is
    /// written, into the current folder
    rev: bool,
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
fn build(p: &Project, warn: &mut dyn Write, full: bool, mut single: bool, mut html: bool, o: &BuildOpts) -> Result<Built, Error> {
    let mut b = Built { n: 0, extra: Vec::new(), html_err: None };
    if engine::structural(&p.diags()) {
        return Ok(b);
    }
    let dir = if full { &p.cfg.out_full } else { &p.cfg.out };
    let mut r = render::Renderer::new(p, full);
    (r.comments, r.since, r.only) = (!o.no_comments, o.since.clone(), o.page.clone());
    if o.rev {
        // docs/out and docc.lock belong to a checkout; the HTML file goes where docc runs
        let f = PathBuf::from(render::single_file(p, dir, ".html")?.file_name().unwrap_or_default());
        match r.html() {
            Ok(h) => {
                render::write_file(&f, h.as_bytes())?;
                b.extra.push(f.display().to_string());
            }
            Err(e) => b.html_err = Some(e),
        }
        return Ok(b);
    }
    render::check_dir(p, dir)?;
    // a one-file build made once is kept up to date by every later build, if docc wrote it
    for (ext, flag, on) in [(".md", "single", &mut single), (".html", "html", &mut html)] {
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
                        "docc: {} is not a file docc wrote: left alone (build with --{flag} to replace it)\n",
                        f.display(),
                    ),
                );
            }
        }
    }
    let files = r.files();
    let mut one: Vec<(&str, String)> = Vec::new(); // extension -> content of a single-file build
    if single {
        one.push((".md", r.single()));
    }
    // a bad --since stops only the HTML file, not the pages and the lock
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
    for (ext, content) in &one {
        render::write_file(&paths[ext], content.as_bytes())?;
        b.extra.push(format!("{dir}{ext}"));
    }
    b.n = files.len();
    Ok(b)
}

fn cmd_build(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let (mut full, mut single, mut html) = (false, false, false);
    let (mut page, mut rev) = (String::new(), String::new());
    let mut o = BuildOpts::default();
    let pos = {
        let mut fs = new_flags("build", c);
        fs.bool_var(&mut full, "full", "audit build into outFull");
        fs.bool_var(&mut single, "single", "also write all pages as one Markdown file next to the output directory");
        fs.bool_var(&mut html, "html", "also write all pages as one self-contained HTML file next to the output directory");
        fs.bool_var(&mut o.no_comments, "no-comments", "leave the readers' comments ($comments) out of everything this build writes");
        fs.string_var(&mut o.since, "since", "HTML file: the Git revision (tag, branch, commit) of the version the reader last received; the edits since it are shown, the history before it is left out");
        fs.string_var(&mut page, "page", "write only this page: its path relative to src as docc compile gives it (decisions/monolith.json, .json may be left out) or its id");
        fs.string_var(&mut rev, "rev", "build the tree of this Git revision (commit, branch, tag) without a checkout; needs --html and writes only the HTML file, into the current folder");
        parse(&mut fs, args)
    };
    let pos = pos?;
    no_args(&pos)?;
    c.check()?;
    if !o.since.is_empty() && o.since.trim().is_empty() {
        return Err(usage_err("--since must name a Git revision"));
    }
    if !rev.is_empty() && (!html || single) {
        return Err(usage_err("--rev writes only the HTML file: give --html, and no --single (its pages link the images of the output directory, which --rev does not write)"));
    }
    o.rev = !rev.is_empty();
    if rev.is_empty() {
        // blocks without a code get one first; the project is read again below
        engine::assign_codes(&engine::load(load_config(c)?)?)?;
    }
    let mut p = engine::load(load_config_at(c, &rev)?)?;
    p.validate();
    if !page.is_empty() {
        o.page = render::Renderer::new(&p, full).page_id(&page).ok_or_else(|| {
            usage_err(format!(
                "--page {}: no such page; give its path relative to {} as docc compile shows it (decisions/monolith.json) or its id{}",
                config::go_quote(&page),
                p.cfg.src,
                if full { "" } else { " (a hidden page needs --full)" }
            ))
        })?;
    }
    let b = build(&p, stderr, full, single, html, &o)?;
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
    }
    for f in &b.extra {
        say(stdout, &format!("built {f}\n"));
    }
    if b.n == 0 && b.extra.is_empty() && engine::structural(&p.diags()) {
        let (errs, _) = report::counts(&p.diags());
        say(stdout, &format!("build skipped: {errs} errors, {dir} not changed\n"));
    }
    Ok(code)
}

fn cmd_check(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("check", c), args)?;
    c.check()?;
    let mut p = load_scoped(c, &pos)?;
    // docs/out is compared only when the paths reach it
    let out = p.cfg.out.trim_start_matches("./").trim_end_matches('/').to_string();
    let fresh = p.scope.is_empty() || p.scope.iter().any(|s| engine::under(s, &out) || engine::under(&out, s));
    // docs/out may be built with or without the readers' comments
    if fresh && !engine::structural(&p.diags()) && p.cfg.commit_out {
        let r = render::Renderer::new(&p, false);
        let mut bare = render::Renderer::new(&p, false);
        bare.comments = false;
        let with = r.files();
        let without = p.docs.iter().any(|d| d.root().is_some_and(|r| r.get(crate::comments::KEY).is_some())).then(|| bare.files());
        render::check_fresh(&p, &p.cfg.out, &with, without.as_ref());
    }
    let mut ds = p.sorted();
    ds.retain(|d| p.in_scope(&d.file));
    emit(stdout, stderr, c, &p, &ds)
}

/// cmd_codes gives a code to every block without one (engine::assign_codes).
fn cmd_codes(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("codes", c), args)?;
    no_args(&pos)?;
    c.check()?;
    let n = engine::assign_codes(&engine::load(load_config(c)?)?)?;
    if c.format == "json" {
        #[derive(serde::Serialize)]
        struct Out {
            given: usize,
        }
        write_json(stdout, &Out { given: n })?;
    } else {
        say(stdout, &format!("gave codes to {n} blocks\n"));
    }
    Ok(0)
}

/// cmd_migrate moves a project of the old format to codes and hashes (engine::migrate).
fn cmd_migrate(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("migrate", c), args)?;
    no_args(&pos)?;
    c.check()?;
    let r = engine::migrate(load_config(c)?)?;
    if c.format == "json" {
        write_json(stdout, &r)?;
        return Ok(0);
    }
    say(
        stdout,
        &format!(
            "codes given: {}, old codes and anchors replaced: {}, links rewritten: {}, link hashes added: {}, comments moved: {}\n\
             code files written: {}, id#^code rewritten in code: {}, tag hashes added: {}, fingerprints added: {}\n\
             hashes kept matching: {}, fingerprints kept matching: {}\n\
             left to review (unconfirmed before): {}, documents written: {}, docc.lock removed: {}\n\
             Next: docc build, docc check, then commit.\n",
            r.codes_given,
            r.anchors_replaced,
            r.links_rewritten,
            r.hashes_added,
            r.comments_moved,
            r.code_files,
            r.code_refs_rewritten,
            r.tag_hashes_added,
            r.prints_added,
            r.hashes_moved,
            r.prints_moved,
            r.unconfirmed,
            r.documents,
            if r.lock_removed { "yes" } else { "no" },
        ),
    );
    Ok(0)
}

const INIT_CONFIG: &str = r#"{
  "src": "docs/src",
  "out": "docs/out",
  "roots": ["index"],
  "maxWarnings": 0,
  "commitOut": false
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
const BUILT_IGNORE: [&str; 5] = [
    "docs/out.md",
    "docs/out.html",
    "docs/out-full/",
    "docs/out-full.md",
    "docs/out-full.html",
];

/// ignore_built adds the lines of BUILT_IGNORE that .gitignore lacks, and the
/// output directory when it is not committed (out: "commitOut": false),
/// creating the file.
fn ignore_built(stdout: &mut dyn Write, out: Option<&str>) -> Result<(), Error> {
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
    let out = out.map(|o| format!("{}/", o.trim_end_matches('/')));
    for l in BUILT_IGNORE.iter().copied().chain(out.as_deref()) {
        if !have.contains(l) && !have.contains(l.trim_end_matches('/')) {
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
    let cfg = load_config(c)?;
    ignore_built(stdout, (!cfg.commit_out).then_some(cfg.out.as_str()))?;
    create(stdout, "docs/schemas/page.json", INIT_SCHEMA, 0o644)?;
    create(stdout, "docs/src/index.json", "{ \"title\": \"Index\", \"kind\": \"page\" }\n", 0o644)?;
    let mut hook = false; // a pre-commit hook is created now; an existing one is never touched
    if std::fs::metadata(".git").is_ok_and(|st| st.is_dir()) {
        let name = ".git/hooks/pre-commit";
        hook = std::fs::metadata(name).is_err();
        create(stdout, name, INIT_HOOK, 0o755)?;
    }
    let mut p = load_project(c)?;
    let b = build(&p, stderr, false, false, false, &BuildOpts::default())?;
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
