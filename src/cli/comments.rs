//! docc comments: the readers' comments kept in "$comments" of the documents.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::rulescmd::{indent, write_json};
use super::{Common, Error, go_path_error, load_project, new_flags, parse, say, usage_err, write_line};
use crate::comments::{self, Comment, Edit, KEY, Place};
use crate::config::go_quote;
use crate::engine::{self, Doc, Project};
use crate::render::{Renderer, Seen};
use crate::history;
use crate::loader::{self, Kind, Node};

/// APPLY_SCHEMA_ID is the "$schema" of the output of docc comments apply.
pub(super) const APPLY_SCHEMA_ID: &str = "urn:docc:schema:comments-apply:1";

pub(super) const COMMENTS_USAGE: &str = "Usage: docc comments [list] | apply | import <file|->... | rm <id>...

The readers' comments live in the documents themselves, in the top-level member
\"$comments\": [{id, at, quote, fragment?, prefix?, suffix?, text | input+answer, author,
email?, created}]. A comment is about the block at the JSON Pointer at; an answer to a form
field ({\"$input\": ...}) has the field id in input and the answer in answer instead of text.
There are no statuses and no replies: a handled comment is deleted, in the same commit as
the fix.

list (the default) prints every comment with its document, its block now and its place:
same, changed (the block was edited) or lost (no block at at, or the selected words are gone).
apply reads a JSON array of operations from standard input and edits the files in place:
only \"$comments\" changes, every other byte of a file stays; a new \"$comments\" is the
last member of the document. The operations:
  {\"op\": \"add\", \"doc\": <id or path>, \"comment\": {...}}   id and created are filled in when left out
  {\"op\": \"rm\", \"id\": <id>}
  {\"op\": \"edit\", \"id\": <id>, \"text\": <new text>}
  {\"op\": \"edit\", \"id\": <id>, \"answer\": [<new answer>]}   an answer to the same form field, quoting it as it is now
An answer must fit its field, on add and edit: a text field and radio buttons take one
value (radio: an option or an own answer), check boxes at most one value that is none of
the options; no value blank or twice. Values that are not options are trimmed.
A comment stands only where a reader looks: at must be the pointer of a block, a property
or (for an answer) the form field that docc compile shows on the page of the document;
notes, docIgnore, hidden, the title and \"$comments\" are refused. quote is always the
source text of that block: a quote given is replaced. A hidden document takes no comment,
and rm and edit do not touch its comments, unless --hidden is given (an agent working by
hand). rm and edit refuse an id that more than one comment has (DOC112).
One bad operation (an unknown document or id, a malformed comment, a place readers do not
see) fails the whole batch: nothing is written and the exit code is 2. With --format json apply prints
{\"$schema\", added: [{id, doc, path}], removed, edited, files}; docc schema comments-apply
prints its JSON Schema.
import adds the comments of the HTML file (docc build --html), copied as text for a
messenger or downloaded as a JSON file (- reads the copied text from standard input), to
their documents through apply. A comment already there (same id) is skipped; a comment left
on an older build is moved to its block in the current text; a comment that would not
stand where a reader looks (a hidden document or block, a block that is gone) is skipped
and listed. rm deletes comments; an id may
be shortened to any unique prefix.

Examples:
  docc comments
  docc comments --format json
  echo '[{\"op\":\"add\",\"doc\":\"index\",\"comment\":{\"at\":\"/body/0\",\"text\":\"Why?\",\"author\":\"Ann\"}}]' | docc comments apply
  docc comments import ~/Downloads/comments-2026-10-15.json
  wl-paste | docc comments import -
  docc comments rm c-3f2a91bc
";

pub(super) fn cmd_comments(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let mut hidden = false;
    let mut pos = {
        let mut fs = new_flags("comments", c);
        fs.bool_var(&mut hidden, "hidden", "apply, rm, import: allow comments on hidden documents (for an agent working by hand; Docspace never gives it)");
        parse(&mut fs, args)?
    };
    c.check()?;
    let sub = if pos.is_empty() { String::new() } else { pos.remove(0) };
    match sub.as_str() {
        "" | "list" | "apply" => {
            if let Some(a) = pos.first() {
                return Err(usage_err(format!("unexpected argument {}", go_quote(a))));
            }
        }
        "import" | "rm" if pos.is_empty() => {
            return Err(usage_err(if sub == "import" { "give the files to import" } else { "give the ids of the comments" }));
        }
        "import" | "rm" => {}
        "done" => return Err(usage_err("comments have no status any more: delete a handled comment with docc comments rm")),
        _ => return Err(usage_err(format!("unknown subcommand {}: use list, apply, import or rm", go_quote(&sub)))),
    }
    let p = load_project(c)?;
    match sub.as_str() {
        "apply" => {
            let mut b = Vec::new();
            std::io::stdin().read_to_end(&mut b)?;
            let ops: serde_json::Value = serde_json::from_slice(&b).map_err(|e| usage_err(format!("standard input: {e}")))?;
            let ops = ops.as_array().ok_or_else(|| usage_err("standard input: give a JSON array of operations"))?;
            let ops = ops
                .iter()
                .enumerate()
                .map(|(i, o)| serde_json::from_value(o.clone()).map_err(|e| usage_err(format!("operation {}: {e}", i + 1))))
                .collect::<Result<Vec<Op>, Error>>()?;
            let a = apply(&p, &ops, hidden)?;
            print_applied(c, &a, stdout)
        }
        "import" => import(c, &p, &pos, hidden, stdout),
        "rm" => {
            let all: Vec<String> = all_comments(&p).into_iter().map(|(_, c)| c.id).collect();
            let ops = pos.iter().map(|id| find(&all, id).map(|id| Op::Rm { id })).collect::<Result<Vec<Op>, Error>>()?;
            let a = apply(&p, &ops, hidden)?;
            print_applied(c, &a, stdout)
        }
        _ => list(c, &p, stdout),
    }
}

/// Op is one operation of docc comments apply.
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub(super) enum Op {
    Add { doc: String, comment: serde_json::Value },
    Rm { id: String },
    /// a comment gets a new text, an answer a new answer (one of the two)
    Edit { id: String, text: Option<String>, answer: Option<Vec<String>> },
}

/// Applied is the output of docc comments apply.
#[derive(Serialize, Default)]
pub(super) struct Applied {
    #[serde(rename = "$schema")]
    schema: &'static str,
    added: Vec<Added>,
    removed: Vec<String>,
    edited: Vec<String>,
    /// the files changed, relative to src
    files: Vec<String>,
}

#[derive(Serialize)]
struct Added {
    id: String,
    doc: String,
    path: String,
}

/// all_comments is every comment of the project with its document, in path order.
fn all_comments(p: &Project) -> Vec<(&Doc, Comment)> {
    p.docs.iter().filter_map(|d| d.root().map(|r| (d, r))).flat_map(|(d, r)| comments::of(r).into_iter().map(move |c| (d, c))).collect()
}

/// ids_in are the ids of the comments of a document, malformed items included.
fn ids_in(root: &Node) -> Vec<String> {
    root.get(KEY)
        .map(|v| v.items.iter().filter_map(|it| it.get("id").filter(|n| n.kind == Kind::String)).map(|n| n.str.clone()).collect())
        .unwrap_or_default()
}

/// find_doc is the document named by an id, a path relative to src or the
/// project root, or a path on the disk.
fn find_doc<'a>(p: &'a Project, name: &str) -> Option<(usize, &'a Doc)> {
    let canon = |s: &Path| std::fs::canonicalize(s).ok();
    let disk = canon(Path::new(name));
    p.docs.iter().enumerate().find(|(_, d)| d.id == name).or_else(|| {
        p.docs.iter().enumerate().find(|(_, d)| {
            d.rel == name || d.file == name || (disk.is_some() && canon(&p.cfg.abs(&d.file)) == disk)
        })
    })
}

/// Seer is the one rule for every comment docc writes (apply, rm, import;
/// Docspace calls apply): a comment stands only where a reader looks. Its
/// document is a page of the reader (hidden documents only with --hidden, for
/// an agent working by hand), its "at" is a block, a property or a form field
/// that docc compile outputs for that page, and its quote is the source text
/// of that place, so nothing a reader does not see (notes, docIgnore, the
/// title, "$comments") ever reaches a reader as a quote.
struct Seer<'a> {
    p: &'a Project,
    r: Renderer<'a>,
    hidden: bool,
    /// the project has errors of groups 1xx-2xx: compile shows no page
    broken: bool,
    cache: HashMap<usize, Vec<Seen>>,
}

impl<'a> Seer<'a> {
    fn new(p: &'a Project, hidden: bool) -> Seer<'a> {
        Seer { p, r: Renderer::new(p, false), hidden, broken: engine::structural(&p.diags()), cache: HashMap::new() }
    }

    /// doc fails for a hidden document unless --hidden was given.
    fn doc(&self, i: usize) -> Result<(), String> {
        if self.p.docs[i].hidden && !self.hidden {
            return Err(format!(
                "{} is a hidden document: readers do not see it, so it takes no comments (--hidden allows it, for an agent working by hand)",
                self.p.docs[i].rel
            ));
        }
        Ok(())
    }

    /// quote is the source text of the place at of document i that a reader
    /// sees; an answer (input) needs the form field with that id there.
    fn quote(&mut self, i: usize, at: &str, input: Option<&str>) -> Result<String, String> {
        self.doc(i)?;
        if self.broken {
            return Err("the project has errors of groups 1xx-2xx, so readers see no page: fix them first (docc check)".into());
        }
        let d = &self.p.docs[i];
        let seen = self.cache.entry(i).or_insert_with(|| self.r.seen(d));
        let rel = &d.rel;
        let Some(v) = seen.iter().find(|v| v.p == at) else {
            return Err(format!(
                "{rel} shows a reader no block at {}: a comment stands on a block or a form field of the page, never on what readers do not see (notes, docIgnore, hidden, the title, \"$comments\")",
                go_quote(at)
            ));
        };
        match input {
            Some(f) if v.input.as_deref() != Some(f) => Err(format!("{rel} has no form field {} at {}", go_quote(f), go_quote(at))),
            _ => Ok(v.s.clone()),
        }
    }
}

/// apply runs the operations on the files of the project, all or none: the
/// files are written only when every operation succeeded and every changed
/// "$comments" is well formed. Every operation obeys Seer; rm and edit refuse
/// an id that more than one comment of the project has (DOC112).
pub(super) fn apply(p: &Project, ops: &[Op], hidden: bool) -> Result<Applied, Error> {
    let mut seer = Seer::new(p, hidden);
    // comment id -> the index of its document, once per comment
    let mut owner: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, d) in p.docs.iter().enumerate() {
        for id in d.root().map(ids_in).unwrap_or_default() {
            owner.entry(id).or_default().push(i);
        }
    }
    let mut files: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    let read = |i: usize, files: &mut BTreeMap<usize, Vec<u8>>| -> Result<(), Error> {
        if !files.contains_key(&i) {
            let path = p.cfg.abs(&p.docs[i].file);
            files.insert(i, std::fs::read(&path).map_err(|e| go_path_error("open", &path, &e))?);
        }
        Ok(())
    };
    let mut out = Applied { schema: APPLY_SCHEMA_ID, ..Default::default() };
    for (k, op) in ops.iter().enumerate() {
        let fail = |msg: String| usage_err(format!("operation {}: {msg}", k + 1));
        let (i, change) = match op {
            Op::Add { doc, comment } => {
                let (i, d) = find_doc(p, doc).ok_or_else(|| fail(format!("no document {}", go_quote(doc))))?;
                read(i, &mut files)?;
                let mut obj = comment.as_object().cloned().ok_or_else(|| fail("comment must be an object".into()))?;
                let given = obj.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let id = if given.is_empty() {
                    let mut id = comments::new_id();
                    while owner.contains_key(&id) {
                        id = comments::new_id();
                    }
                    id
                } else if let Some(j) = owner.get(&given) {
                    return Err(fail(format!("comment id {} is taken (in {})", go_quote(&given), p.docs[j[0]].rel)));
                } else {
                    given
                };
                obj.insert("id".into(), id.clone().into());
                obj.entry("quote").or_insert_with(|| "".into());
                obj.entry("created").or_insert_with(|| comments::now().into());
                if !obj.contains_key("author") {
                    return Err(fail("comment: a comment lacks \"author\" (the name of the reader; may be \"\")".into()));
                }
                let mut c: Comment =
                    serde_json::from_value(serde_json::Value::Object(obj)).map_err(|e| fail(format!("comment: {e}")))?;
                comments::check(&c).map_err(|m| fail(format!("comment: {m}")))?;
                // the quote is always the text the reader sees there; a quote given that differs is replaced
                c.quote = seer.quote(i, &c.at, c.input.as_deref()).map_err(fail)?;
                answer_fits(d.root(), &mut c).map_err(|m| fail(format!("comment: {m}")))?;
                owner.insert(id.clone(), vec![i]);
                out.added.push(Added { id: id.clone(), doc: d.id.clone(), path: d.rel.clone() });
                let b = comments::edit(&files[&i], &Edit::Add(&c)).map_err(|m| fail(format!("{}: {m}", d.rel)))?;
                (i, b)
            }
            Op::Rm { id } | Op::Edit { id, .. } => {
                let is = owner.get(id).ok_or_else(|| fail(format!("no comment {}", go_quote(id))))?;
                if is.len() > 1 {
                    let mut at: Vec<&str> = is.iter().map(|&j| p.docs[j].rel.as_str()).collect();
                    at.dedup();
                    return Err(fail(format!(
                        "comment id {} is used by {} comments (in {}): it names none of them; give each its own id first (DOC112)",
                        go_quote(id),
                        is.len(),
                        at.join(", ")
                    )));
                }
                let i = is[0];
                seer.doc(i).map_err(fail)?;
                read(i, &mut files)?;
                let again = match op {
                    Op::Edit { text: None, answer: Some(a), .. } => Some(answer_again(&mut seer, i, &files[&i], id, a).map_err(fail)?),
                    _ => None,
                };
                let e = match (op, &again) {
                    (_, Some(c)) => Edit::Replace(id, c),
                    (Op::Edit { text: Some(t), answer: None, .. }, _) => Edit::Text(id, t),
                    (Op::Edit { .. }, _) => return Err(fail("edit takes either \"text\" (a comment) or \"answer\" (an answer to a form field)".into())),
                    _ => Edit::Remove(id),
                };
                let b = comments::edit(&files[&i], &e).map_err(|m| fail(format!("{}: {m}", p.docs[i].rel)))?;
                if let Op::Rm { .. } = op {
                    owner.remove(id);
                    out.removed.push(id.clone());
                } else {
                    out.edited.push(id.clone());
                }
                (i, b)
            }
        };
        files.insert(i, change);
    }
    // the result must read back as the comments it was meant to be
    for (&i, b) in &files {
        let r = loader::parse(b.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(b));
        let rel = &p.docs[i].rel;
        let Some(root) = r.root.as_ref().filter(|_| r.err.is_none()) else {
            return Err(usage_err(format!("{rel}: the file would not parse after the change")));
        };
        if let Some(prob) = root.get(KEY).and_then(|v| comments::problems(v).into_iter().next()) {
            return Err(usage_err(format!("{rel}: {}", prob.msg)));
        }
    }
    for (&i, b) in &files {
        let path = p.cfg.abs(&p.docs[i].file);
        std::fs::write(&path, b).map_err(|e| go_path_error("write", &path, &e))?;
        out.files.push(p.docs[i].rel.clone());
    }
    Ok(out)
}

/// answer_again is the answer id of document i (its file now in src) with a
/// new answer: it stays an answer to the same form field, follows the field
/// if it moved, and quotes the field as the reader sees it now.
fn answer_again(seer: &mut Seer, i: usize, src: &[u8], id: &str, answer: &[String]) -> Result<Comment, String> {
    let r = loader::parse(src.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(src));
    let root = r.root.as_ref().filter(|_| r.err.is_none()).ok_or("the file is not valid JSON")?;
    let cur = comments::of(root).into_iter().find(|c| c.id == id).ok_or_else(|| format!("comment {} is malformed (DOC112)", go_quote(id)))?;
    let Some(input) = cur.input.clone() else {
        return Err(format!("comment {} is not an answer to a form field: edit its \"text\"", go_quote(id)));
    };
    let at = comments::find_input(root, "", &input).unwrap_or(cur.at.clone());
    let quote = seer.quote(i, &at, Some(&input))?;
    let mut c = Comment { at, quote, answer: Some(answer.to_vec()), ..cur };
    comments::check(&c)?;
    answer_fits(Some(root), &mut c)?;
    Ok(c)
}

/// answer_fits trims an answer as the HTML file does and checks that the form
/// field at its "at" (Seer found it there) takes it: Docspace passes on what
/// any client sends, so the shape is checked here, not only by the page.
fn answer_fits(root: Option<&Node>, c: &mut Comment) -> Result<(), String> {
    let Some(f) = root.and_then(|r| crate::schema::resolve_pointer(r, &c.at)).filter(|f| engine::is_input(f)) else {
        return Ok(());
    };
    if let Some(a) = c.answer.as_mut() {
        comments::tidy_answer(f, a);
    }
    comments::check(c)?;
    comments::answer_problem(f, c.answer.as_deref().unwrap_or_default()).map_or(Ok(()), Err)
}

fn print_applied(c: &Common, a: &Applied, stdout: &mut dyn Write) -> Result<i32, Error> {
    if c.format == "json" {
        write_line(stdout, a)?;
        return Ok(0);
    }
    for x in &a.added {
        say(stdout, &format!("added {} to {} ({})\n", x.id, x.doc, x.path));
    }
    for id in &a.edited {
        say(stdout, &format!("edited {id}\n"));
    }
    for id in &a.removed {
        say(stdout, &format!("deleted {id}\n"));
    }
    Ok(0)
}

/// find returns the comment id equal to id or the only one that starts with it.
fn find(all: &[String], id: &str) -> Result<String, Error> {
    if all.iter().any(|x| x == id) {
        return Ok(id.to_string());
    }
    let mut hits = all.iter().filter(|x| x.starts_with(id));
    match (hits.next(), hits.next()) {
        (Some(x), None) => Ok(x.clone()),
        (Some(_), Some(_)) => {
            Err(usage_err(format!("{} matches more than one comment: give more characters", go_quote(id))))
        }
        _ => Err(usage_err(format!("no comment {}", go_quote(id)))),
    }
}

/// import adds the comments exported by the HTML file to their documents.
/// A comment left on an older build (its build commit) follows its block to
/// where it stands now (comments::follow); an answer goes to its form field.
/// A comment that would not stand where a reader looks (Seer: a hidden
/// document or block, or a block that is gone) is skipped and listed.
fn import(c: &Common, p: &Project, files: &[String], hidden: bool, stdout: &mut dyn Write) -> Result<i32, Error> {
    let mut have: HashSet<String> = p.docs.iter().filter_map(|d| d.root()).flat_map(ids_in).collect();
    let mut items = Vec::new();
    let mut dups = 0;
    for f in files {
        let (name, b) = if f == "-" {
            // stdin is where import - reads the copied comments
            let mut b = Vec::new();
            std::io::stdin().read_to_end(&mut b)?;
            ("standard input", b)
        } else {
            (f.as_str(), std::fs::read(f).map_err(|e| go_path_error("open", Path::new(f), &e))?)
        };
        for it in comments::parse_export(name, &b)? {
            if have.insert(it.id.clone()) {
                items.push(it);
            } else {
                dups += 1;
            }
        }
    }
    let old = old_versions(p, &items);
    let mut seer = Seer::new(p, hidden);
    if seer.broken && !items.is_empty() {
        return Err(usage_err("the project has errors of groups 1xx-2xx, so readers see no page: fix them first (docc check)"));
    }
    let (mut ops, mut gone, mut unseen, mut moved) = (Vec::new(), Vec::new(), Vec::new(), 0);
    for it in items {
        let Some((i, d)) = find_doc(p, &it.doc).filter(|(_, d)| d.root().is_some()) else {
            gone.push(it.id);
            continue;
        };
        let root = d.root().unwrap();
        let at = if !it.input.is_empty() {
            comments::find_input(root, "", &it.input)
        } else {
            old.get(&(it.build.clone(), d.rel.clone())).and_then(|o| comments::follow(o, root, &it.path))
        }
        .unwrap_or_else(|| it.path.clone());
        if seer.quote(i, &at, Some(it.input.as_str()).filter(|f| !f.is_empty())).is_err() {
            unseen.push(it.id);
            continue;
        }
        if at != it.path {
            moved += 1;
        }
        let answer = (!it.input.is_empty()).then(|| if it.answer.is_empty() { vec![it.text.clone()] } else { it.answer.clone() });
        let cm = Comment {
            id: it.id,
            at,
            quote: it.quote,
            fragment: it.fragment,
            prefix: it.prefix,
            suffix: it.suffix,
            text: answer.is_none().then_some(it.text),
            input: answer.is_some().then_some(it.input),
            answer,
            author: it.author,
            email: String::new(),
            created: if it.created.is_empty() { comments::now() } else { it.created },
        };
        ops.push(Op::Add { doc: d.id.clone(), comment: serde_json::to_value(&cm)? });
    }
    let a = apply(p, &ops, hidden)?;
    if c.format == "json" {
        #[derive(Serialize)]
        struct Imported<'a> {
            imported: usize,
            duplicates: usize,
            moved: usize,
            gone: &'a [String],
            /// not on a place a reader sees: a hidden document or part, or a block that is gone
            unseen: &'a [String],
            files: &'a [String],
        }
        write_json(
            stdout,
            &Imported { imported: a.added.len(), duplicates: dups, moved, gone: &gone, unseen: &unseen, files: &a.files },
        )?;
        return Ok(0);
    }
    say(stdout, &format!("imported {} comments into {} documents", a.added.len(), a.files.len()));
    let mut notes = Vec::new();
    if dups > 0 {
        notes.push(format!("{dups} already there"));
    }
    if moved > 0 {
        notes.push(format!("{moved} moved to their block"));
    }
    if !gone.is_empty() {
        notes.push(format!("{} on a page that is gone, not imported: {}", gone.len(), gone.join(", ")));
    }
    if !unseen.is_empty() {
        notes.push(format!(
            "{} not on a block a reader sees (hidden, or gone), not imported: {}",
            unseen.len(),
            unseen.join(", ")
        ));
    }
    if !notes.is_empty() {
        say(stdout, &format!(" ({})", notes.join("; ")));
    }
    say(stdout, "\n");
    Ok(0)
}

/// old_versions reads from Git the documents the comments were left on, as
/// they were in the builds the comments name: (build, path relative to src)
/// to the parsed document. Without Git, or for an unknown build, nothing.
fn old_versions(p: &Project, items: &[comments::Item]) -> HashMap<(String, String), Node> {
    let mut want: Vec<(String, String)> = items
        .iter()
        .filter(|it| !it.build.is_empty() && it.input.is_empty())
        .filter_map(|it| find_doc(p, &it.doc).map(|(_, d)| (it.build.clone(), d.rel.clone())))
        .collect();
    want.sort();
    want.dedup();
    let mut out = HashMap::new();
    if want.is_empty() {
        return out;
    }
    let Ok((dir, prefix)) = history::locate_git(&p.cfg.abs(&p.cfg.src)) else {
        return out;
    };
    let req: Vec<u8> = want.iter().flat_map(|(b, rel)| format!("{b}:{prefix}{rel}\n").into_bytes()).collect();
    let Ok(blobs) = history::cat_files(&dir, req) else {
        return out;
    };
    for (k, b) in want.into_iter().zip(blobs) {
        if let Some(root) = b.and_then(|b| loader::parse(&b).root) {
            out.insert(k, root);
        }
    }
    out
}

#[derive(Serialize)]
struct Listed<'a> {
    #[serde(flatten)]
    c: &'a Comment,
    doc: &'a str,
    path: &'a str,
    title: String,
    place: Place,
    /// the text of the block now, "" when the comment lost its place
    current: String,
}

fn list(c: &Common, p: &Project, stdout: &mut dyn Write) -> Result<i32, Error> {
    let all = all_comments(p);
    let title_of = |d: &Doc| d.root().and_then(|r| r.get("title")).filter(|t| t.kind == Kind::String).map(|t| t.str.clone());
    let title = |id: &str| p.doc(id).and_then(title_of);
    let out: Vec<Listed> = all
        .iter()
        .map(|(d, cm)| {
            let (place, current) = comments::place(d.root().unwrap(), cm, &title);
            Listed { c: cm, doc: &d.id, path: &d.rel, title: title_of(d).unwrap_or_default(), place, current }
        })
        .collect();
    if c.format == "json" {
        #[derive(Serialize)]
        struct List<'a> {
            comments: &'a [Listed<'a>],
            total: usize,
        }
        write_json(stdout, &List { comments: &out, total: out.len() })?;
        return Ok(0);
    }
    for l in &out {
        let cm = l.c;
        let mut b = format!("{}  {} {} {}", cm.id, l.doc, go_quote(&l.title), cm.at);
        if l.place != Place::Same {
            b += &format!(" ({})", if l.place == Place::Lost { "lost its place" } else { "block changed" });
        }
        b.push('\n');
        if !cm.fragment.is_empty() {
            b += &format!("  about: «{}»\n", indent(&cm.fragment));
        }
        let what = match &cm.input {
            Some(f) => {
                b += &format!("  answers form field {f}\n");
                "field"
            }
            None => "block",
        };
        b += &format!("  {}{}\n", who(&cm.author, &cm.created), indent(&cm.words()));
        b += &match l.place {
            Place::Same => format!("  {what}: {}\n", indent(&cm.quote)),
            Place::Changed => {
                format!("  {what} when commented: {}\n  {what} now: {}\n", indent(&cm.quote), indent(&l.current))
            }
            Place::Lost => format!("  {what} when commented: {}\n", indent(&cm.quote)),
        };
        b.push('\n');
        say(stdout, &b);
    }
    let docs: HashSet<&str> = out.iter().map(|l| l.doc).collect();
    let lost = out.iter().filter(|l| l.place == Place::Lost).count();
    say(stdout, &format!("{} comments in {} documents, {lost} lost their place\n", out.len(), docs.len()));
    Ok(0)
}

/// who is "author date: " before the text of a comment, "" when both are empty.
fn who(author: &str, created: &str) -> String {
    let date: String = created.chars().take(10).collect();
    let w = format!("{author} {date}").trim().to_string();
    if w.is_empty() { w } else { w + ": " }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_and_find() {
        assert_eq!(who("", ""), "");
        assert_eq!(who("C", "2026-10-15T10:00:00Z"), "C 2026-10-15: ");
        assert_eq!(who("", "2026-10-15T10:00:00Z"), "2026-10-15: ");
        let all = ["c-abc".to_string(), "c-abd".to_string()];
        assert_eq!(find(&all, "c-abc").unwrap(), "c-abc");
        assert!(find(&all, "c-ab").unwrap_err().to_string().contains("more than one"));
        assert_eq!(find(&all, "c-abd").unwrap(), "c-abd");
        assert!(find(&all, "x").unwrap_err().to_string().contains("no comment"));
    }
}
