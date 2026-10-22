//! docc comments: the readers' comments kept in "$comments" of the documents.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::rulescmd::{indent, write_json};
use super::{Common, Error, go_path_error, load_project, new_flags, parse, say, usage_err, write_line};
use crate::comments::{self, Comment, Edit, KEY, Place};
use crate::config::go_quote;
use crate::engine::{Doc, Project};
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
  {\"op\": \"add\", \"doc\": <id or path>, \"comment\": {...}}   id, quote and created are filled in when left out
  {\"op\": \"rm\", \"id\": <id>}
  {\"op\": \"edit\", \"id\": <id>, \"text\": <new text>}
One bad operation (an unknown document or id, a malformed comment) fails the whole batch:
nothing is written and the exit code is 2. With --format json apply prints
{\"$schema\", added: [{id, doc, path}], removed, edited, files}; docc schema comments-apply
prints its JSON Schema.
import adds the comments of the HTML file (docc build --html), copied as text for a
messenger or downloaded as a JSON file (- reads the copied text from standard input), to
their documents through apply. A comment already there (same id) is skipped; a comment left
on an older build is moved to its block in the current text. rm deletes comments; an id may
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
    let mut pos = parse(&mut new_flags("comments", c), args)?;
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
            let a = apply(&p, &ops)?;
            print_applied(c, &a, stdout)
        }
        "import" => import(c, &p, &pos, stdout),
        "rm" => {
            let all: Vec<String> = all_comments(&p).into_iter().map(|(_, c)| c.id).collect();
            let ops = pos.iter().map(|id| find(&all, id).map(|id| Op::Rm { id })).collect::<Result<Vec<Op>, Error>>()?;
            let a = apply(&p, &ops)?;
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
    Edit { id: String, text: String },
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

/// apply runs the operations on the files of the project, all or none: the
/// files are written only when every operation succeeded and every changed
/// "$comments" is well formed.
pub(super) fn apply(p: &Project, ops: &[Op]) -> Result<Applied, Error> {
    // comment id -> index of its document
    let mut owner: HashMap<String, usize> = HashMap::new();
    for (i, d) in p.docs.iter().enumerate() {
        for id in d.root().map(ids_in).unwrap_or_default() {
            owner.entry(id).or_insert(i);
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
                } else if let Some(&j) = owner.get(&given) {
                    return Err(fail(format!("comment id {} is taken (in {})", go_quote(&given), p.docs[j].rel)));
                } else {
                    given
                };
                obj.insert("id".into(), id.clone().into());
                if !obj.contains_key("quote")
                    && let Some(at) = obj.get("at").and_then(|v| v.as_str())
                {
                    let r = loader::parse(files[&i].strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&files[&i]));
                    if let Some(n) = r.root.as_ref().and_then(|root| crate::schema::resolve_pointer(root, at)) {
                        obj.insert("quote".into(), history::text(n).into());
                    }
                }
                obj.entry("created").or_insert_with(|| comments::now().into());
                if !obj.contains_key("author") {
                    return Err(fail("comment: a comment lacks \"author\" (the name of the reader; may be \"\")".into()));
                }
                let c: Comment =
                    serde_json::from_value(serde_json::Value::Object(obj)).map_err(|e| fail(format!("comment: {e}")))?;
                comments::check(&c).map_err(|m| fail(format!("comment: {m}")))?;
                owner.insert(id.clone(), i);
                out.added.push(Added { id: id.clone(), doc: d.id.clone(), path: d.rel.clone() });
                let b = comments::edit(&files[&i], &Edit::Add(&c)).map_err(|m| fail(format!("{}: {m}", d.rel)))?;
                (i, b)
            }
            Op::Rm { id } | Op::Edit { id, .. } => {
                let i = *owner.get(id).ok_or_else(|| fail(format!("no comment {}", go_quote(id))))?;
                read(i, &mut files)?;
                let e = match op {
                    Op::Edit { text, .. } => Edit::Text(id, text),
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
fn import(c: &Common, p: &Project, files: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
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
    let (mut ops, mut gone, mut moved) = (Vec::new(), Vec::new(), 0);
    for it in items {
        let Some((_, d)) = find_doc(p, &it.doc).filter(|(_, d)| d.root().is_some()) else {
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
    let a = apply(p, &ops)?;
    if c.format == "json" {
        #[derive(Serialize)]
        struct Imported<'a> {
            imported: usize,
            duplicates: usize,
            moved: usize,
            gone: &'a [String],
            files: &'a [String],
        }
        write_json(stdout, &Imported { imported: a.added.len(), duplicates: dups, moved, gone: &gone, files: &a.files })?;
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
