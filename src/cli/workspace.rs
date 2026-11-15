//! docc workspace: everything a reader of the project shows in one JSON
//! object (urn:docc:schema:workspace:1): the compiled pages with the hidden
//! ones, the roles of kinds and the areas of code tags from the "workspace"
//! section of docc.json, the coverage table of the covered blocks, the
//! history of the documents and the run file of the tool that works on them.
//! docc watch serves it at /workspace.json, docc build --html inlines it.

use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;

use super::{Common, Error, load_config, new_flags, no_args, parse, warning_failure, write_compiled};
use crate::changes;
use crate::codes;
use crate::config::{Config, match_any};
use crate::engine::{self, Project};
use crate::render;
use crate::report;

/// SCHEMA_ID is the "$schema" of the object and the "$id" of its JSON Schema.
pub const SCHEMA_ID: &str = "urn:docc:schema:workspace:1";

/// HISTORY is how many commits the history keeps, newest first.
const HISTORY: usize = 200;

/// NOTES is the folder of the notes, from the project root: the reader only
/// shows them, an agent writes them at a person's words.
pub(super) const NOTES: &str = "docs/inbox/notes";

/// Parts is the workspace without the run file and the time: docc watch
/// sends a change of the run file alone without compiling again.
pub(super) struct Parts {
    /// `"compile":…,"roles":…,"coverage":…,"history":…`
    body: String,
    run: Option<PathBuf>,
    /// the sources have errors of groups 1xx-2xx: no pages were compiled
    pub(super) broken: bool,
}

impl Parts {
    /// json is the whole object with the run file as it is now.
    pub(super) fn json(&self) -> String {
        let run = self
            .run
            .as_ref()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .unwrap_or(serde_json::Value::Null);
        let run = serde_json::to_string(&run).unwrap_or_else(|_| "null".into());
        format!(r#"{{"$schema":"{SCHEMA_ID}",{},"run":{run},"generatedAt":"{}"}}"#, self.body, crate::comments::now())
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029")
    }
}

/// run_file is the absolute path of the run file of cfg, if it names one.
pub(super) fn run_file(cfg: &Config) -> Option<PathBuf> {
    Some(&cfg.workspace.run).filter(|r| !r.is_empty()).map(|r| cfg.abs(r))
}

#[derive(Serialize)]
struct Place {
    path: String,
    line: usize,
}

#[derive(Serialize)]
struct Manual {
    reason: String,
    by: String,
}

#[derive(Serialize)]
struct Row {
    doc: String,
    code: String,
    text: String,
    areas: serde_json::Map<String, serde_json::Value>,
    manual: Option<Manual>,
}

/// parts loads and checks the project and builds the workspace from it;
/// without comments the pages carry no readers' comments (--no-comments).
pub(super) fn parts(c: &Common, comments: bool) -> Result<Parts, Error> {
    let cfg = load_config(c)?;
    let mut p = engine::load(cfg)?;
    p.validate();
    let broken = engine::structural(&p.diags());
    let out = if broken {
        render::Compiled { project: render::Renderer::new(&p, false).project_info(String::new()), ..Default::default() }
    } else {
        let mut r = render::Renderer::new(&p, false);
        (r.hidden, r.comments) = (true, comments);
        // the meetings have their own tab: the tree leaves them out
        r.tree_hide = kinds_of(&p.cfg, "meetings");
        r.compile()
    };
    let ds = p.sorted();
    let failure = warning_failure(c, &p, &ds);
    let (errs, _) = report::counts(&ds);
    let mut compile = Vec::new();
    write_compiled(&mut compile, errs == 0 && failure.is_empty(), &ds, p.docs.len(), &failure, &out)?;
    let compile = String::from_utf8(compile)?;
    let roles: serde_json::Map<String, serde_json::Value> = p.cfg.workspace.roles.iter().map(|(r, k)| (r.clone(), k.clone())).collect();
    let history = match changes::log(&p.cfg, "HEAD", "") {
        Ok(mut l) => {
            l.commits.truncate(HISTORY);
            serde_json::to_string(&l.commits)?
        }
        // not a Git work tree, or no commit yet
        Err(_) => "[]".into(),
    };
    let body = format!(
        r#""compile":{},"roles":{},"coverage":{},"history":{history},"notes":{}"#,
        compile.trim_end(),
        serde_json::to_string(&roles)?,
        serde_json::to_string(&coverage(&p))?,
        serde_json::to_string(&notes(&p.cfg))?
    );
    Ok(Parts { body, run: run_file(&p.cfg), broken })
}

/// kinds_of are the kinds of a role of the workspace section: one kind or a list.
pub(super) fn kinds_of(cfg: &Config, role: &str) -> Vec<String> {
    match cfg.workspace.roles.iter().find(|(r, _)| r == role).map(|(_, k)| k) {
        Some(serde_json::Value::String(k)) => vec![k.clone()],
        Some(serde_json::Value::Array(ks)) => ks.iter().filter_map(|k| k.as_str().map(String::from)).collect(),
        _ => Vec::new(),
    }
}

#[derive(Serialize)]
struct Note {
    path: String,
    title: String,
    text: String,
    modified: String,
}

/// notes are the *.md files of the notes folder, newest first: the first
/// heading or the file name, the text as written.
fn notes(cfg: &Config) -> Vec<Note> {
    let mut out: Vec<(std::time::SystemTime, Note)> = Vec::new();
    for e in std::fs::read_dir(cfg.root.join(NOTES)).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let (Some(stem), Ok(m)) = (name.strip_suffix(".md"), e.metadata()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(e.path()) else {
            continue;
        };
        if !m.is_file() {
            continue;
        }
        let title = text
            .lines()
            .find_map(|l| l.trim_start().strip_prefix('#').map(|h| h.trim_start_matches('#').trim().to_string()))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| stem.to_string());
        let t = m.modified().unwrap_or(std::time::UNIX_EPOCH);
        out.push((t, Note { path: format!("{NOTES}/{name}"), title, text, modified: crate::comments::rfc3339(t) }));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    out.into_iter().map(|(_, n)| n).collect()
}

/// coverage is one row per block that its kind schema wants covered by code
/// (x-docc-covered), with the code tags that name it sorted into areas.
fn coverage(p: &Project) -> Vec<Row> {
    let mut rows = Vec::new();
    let areas = &p.cfg.workspace.areas;
    for d in &p.docs {
        let Some((_, blocks)) = p.covered_blocks(d) else {
            continue;
        };
        for sn in blocks {
            let mut m: serde_json::Map<String, serde_json::Value> =
                areas.iter().map(|(a, _)| (a.clone(), serde_json::Value::Array(Vec::new()))).collect();
            m.entry("code").or_insert_with(|| serde_json::Value::Array(Vec::new()));
            for (file, line) in p.tag_places.get(&format!("{}#^{}", d.id, sn.anchor)).into_iter().flatten() {
                let area = areas.iter().find(|(_, gs)| match_any(gs, file)).map_or("code", |(a, _)| a.as_str());
                if let Some(serde_json::Value::Array(list)) = m.get_mut(area) {
                    list.push(serde_json::to_value(Place { path: file.clone(), line: *line }).unwrap_or_default());
                }
            }
            let n = sn.node();
            let (start, _) = d.range(n, sn.anchor_at, n.str.len());
            let manual = p
                .ignore_of(d, "DOC404", start)
                .map(|ig| Manual { reason: ig.reason.clone(), by: if ig.by.is_empty() { "agent".into() } else { ig.by.clone() } });
            rows.push(Row { doc: d.id.clone(), code: sn.anchor.clone(), text: codes::normalize(&n.str), areas: m, manual });
        }
    }
    rows
}

/// cmd_workspace prints the workspace object on one line.
pub(super) fn cmd_workspace(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let pos = parse(&mut new_flags("workspace", c), args)?;
    no_args(&pos)?;
    c.check()?;
    let ws = parts(c, true)?;
    stdout.write_all(ws.json().as_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(0)
}
