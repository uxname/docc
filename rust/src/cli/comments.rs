//! docc comments: review comments from the HTML file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

use serde::Serialize;

use super::rulescmd::{indent, write_json};
use super::{Common, Error, go_path_error, load_project, new_flags, parse, say, usage_err};
use crate::comments::{self, Item};
use crate::config::go_quote;
use crate::engine::Project;
use crate::loader::{self, Node};

pub(super) const COMMENTS_USAGE: &str = "Usage: docc comments [--all] | import [--internal] <file|->... | done [--reply TEXT] <id>... | rm <id>...

Review comments left on blocks of the HTML file (docc build --html) leave the page as text
copied for a messenger or as a downloaded JSON file. import adds them to docc.comments.json
in the project root, which belongs in git, as not processed; - reads the copied text from
standard input. A comment already there (same id) is skipped, and a comment whose block
moved is attached to the block with the quoted text. --internal marks the imported comments
as the team's own: they are not put into the HTML file.
Without a subcommand docc comments lists the comments not processed yet, each with the
quoted block and its current text; --all lists every comment. done marks comments as
processed, with --reply the answer the reader sees under the comment in the next HTML
file; rm deletes them. An id may be shortened to any unique prefix.

Examples:
  docc comments import ~/Downloads/comments-2026-10-15.json
  wl-paste | docc comments import -
  docc comments
  docc comments --format json
  docc comments done 3f2a91bc --reply \"Added to section 5.2\"
  docc comments rm 3f2a91bc
";

pub(super) fn cmd_comments(c: &mut Common, args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let (mut all, mut internal, mut reply) = (false, false, String::new());
    let pos = {
        let mut fs = new_flags("comments", c);
        fs.bool_var(&mut all, "all", "list processed comments too");
        fs.bool_var(&mut internal, "internal", "import: the team's own comments, kept out of the HTML file");
        fs.string_var(&mut reply, "reply", "done: the answer shown under the comment in the HTML file");
        parse(&mut fs, args)
    };
    let mut pos = pos?;
    c.check()?;
    let sub = if pos.is_empty() { String::new() } else { pos.remove(0) };
    match sub.as_str() {
        "" | "import" | "done" | "rm" => {}
        _ => return Err(usage_err(format!("unknown subcommand {}: use import, done or rm", go_quote(&sub)))),
    }
    if sub.is_empty() && !pos.is_empty() {
        return Err(usage_err(format!("unexpected argument {}", go_quote(&pos[0]))));
    }
    if internal && sub != "import" {
        return Err(usage_err("--internal is only for import"));
    }
    if !reply.is_empty() && sub != "done" {
        return Err(usage_err("--reply is only for done"));
    }
    if !sub.is_empty() && pos.is_empty() {
        if sub == "import" {
            return Err(usage_err("give the files to import"));
        }
        return Err(usage_err("give the ids of the comments"));
    }
    let p = load_project(c)?;
    let items = comments::load(&p.cfg.root)?;
    match sub.as_str() {
        "import" => return import_comments(c, &p, &p.cfg.root, items, &pos, internal, stdout),
        "done" | "rm" => return change_comments(c, &p.cfg.root, items, &pos, &sub, &reply, stdout),
        _ => {}
    }
    list_comments(c, &p, &items, all, stdout)
}

/// root_of is the parsed document of a comment, None when it is gone.
fn root_of<'a>(p: &'a Project, it: &Item) -> Option<&'a Node> {
    p.doc(&it.doc).and_then(|d| d.root())
}

fn import_comments(
    c: &Common,
    p: &Project,
    root: &Path,
    mut items: Vec<Item>,
    files: &[String],
    internal: bool,
    stdout: &mut dyn Write,
) -> Result<i32, Error> {
    let mut have: HashSet<String> = items.iter().map(|it| it.id.clone()).collect();
    let (mut added, mut dups, mut moved, mut lost) = (0, 0, 0, 0);
    for f in files {
        let (name, b) = if f == "-" {
            // stdin is where import - reads the copied comments
            let mut b = Vec::new();
            std::io::stdin().read_to_end(&mut b)?;
            ("standard input", b)
        } else {
            (f.as_str(), std::fs::read(f).map_err(|e| go_path_error("open", Path::new(f), &e))?)
        };
        for mut it in comments::parse_export(name, &b)? {
            if have.contains(&it.id) {
                dups += 1;
                continue;
            }
            have.insert(it.id.clone());
            let (path, _, state) = comments::locate(root_of(p, &it), &it);
            match state {
                comments::MOVED => {
                    it.path = path;
                    moved += 1;
                }
                comments::CHANGED | comments::REMOVED | comments::MISSING => lost += 1,
                _ => {}
            }
            (it.status, it.internal, it.reply) = (comments::OPEN.into(), internal, String::new());
            items.push(it);
            added += 1;
        }
    }
    comments::save(root, &items)?;
    if c.format == "json" {
        let m: BTreeMap<&str, i32> =
            [("imported", added), ("duplicates", dups), ("moved", moved), ("changed", lost)].into_iter().collect();
        write_json(stdout, &m)?;
        return Ok(0);
    }
    say(stdout, &format!("imported {added} comments into {}", comments::FILE));
    let mut notes = Vec::new();
    if dups > 0 {
        notes.push(format!("{dups} already there"));
    }
    if moved > 0 {
        notes.push(format!("{moved} attached to their moved block"));
    }
    if lost > 0 {
        notes.push(format!("{lost} on a block that changed or is gone"));
    }
    if !notes.is_empty() {
        say(stdout, &format!(" ({})", notes.join(", ")));
    }
    say(stdout, "\n");
    Ok(0)
}

/// find returns the index of the comment with the given id or unique id prefix.
fn find(items: &[Item], id: &str) -> Result<usize, Error> {
    let mut at = None;
    for (i, it) in items.iter().enumerate() {
        if it.id == id {
            return Ok(i);
        }
        if it.id.starts_with(id) {
            if at.is_some() {
                return Err(usage_err(format!(
                    "{} matches more than one comment: give more characters",
                    go_quote(id)
                )));
            }
            at = Some(i);
        }
    }
    at.ok_or_else(|| usage_err(format!("no comment {} in {}", go_quote(id), comments::FILE)))
}

fn change_comments(
    c: &Common,
    root: &Path,
    mut items: Vec<Item>,
    ids: &[String],
    sub: &str,
    reply: &str,
    stdout: &mut dyn Write,
) -> Result<i32, Error> {
    let mut drop = HashSet::new();
    let mut done = Vec::new();
    for id in ids {
        let i = find(&items, id)?;
        if sub == "done" {
            items[i].status = comments::DONE.into();
            if !reply.is_empty() {
                items[i].reply = reply.into();
            }
        } else {
            drop.insert(i);
        }
        done.push(items[i].id.clone());
    }
    let keep: Vec<Item> = items.into_iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, it)| it).collect();
    comments::save(root, &keep)?;
    let verb = HashMap::from([("done", "processed"), ("rm", "deleted")])[sub];
    if c.format == "json" {
        write_json(stdout, &BTreeMap::from([(verb, &done)]))?;
        return Ok(0);
    }
    for id in &done {
        let mut b = format!("{verb} ").into_bytes();
        b.extend_from_slice(short(id));
        b.push(b'\n');
        let _ = stdout.write_all(&b);
    }
    Ok(0)
}

/// short is the first 8 bytes of an id (bytes, as Go slices them).
fn short(id: &str) -> &[u8] {
    &id.as_bytes()[..id.len().min(8)]
}

#[derive(Serialize)]
struct Listed {
    #[serde(flatten)]
    item: Item,
    #[serde(skip_serializing_if = "String::is_empty")]
    title: String,
    current: String,
    /// unchanged, moved, changed, removed (the block), missing (the document)
    state: &'static str,
}

/// List is the JSON of docc comments (Go: a map, so the keys are sorted).
#[derive(Serialize)]
struct List<'a> {
    comments: &'a [Listed],
    open: usize,
    total: usize,
}

fn list_comments(c: &Common, p: &Project, items: &[Item], all: bool, stdout: &mut dyn Write) -> Result<i32, Error> {
    let mut out = Vec::new();
    let mut open = 0;
    for it in items {
        if it.status != comments::DONE {
            open += 1;
        } else if !all {
            continue;
        }
        let (path, current, state) = comments::locate(root_of(p, it), it);
        let mut l = Listed { item: it.clone(), title: String::new(), current, state };
        l.item.path = path;
        if let Some(t) = root_of(p, it).and_then(|r| r.get("title"))
            && t.kind == loader::Kind::String
        {
            l.title = t.str.clone();
        }
        out.push(l);
    }
    if c.format == "json" {
        write_json(stdout, &List { comments: &out, open, total: items.len() })?;
        return Ok(0);
    }
    for l in &out {
        let it = &l.item;
        let kind = if it.internal { " internal" } else { "" };
        let mut b = short(&it.id).to_vec();
        b.extend_from_slice(
            format!("  {}{kind}  {} {} {}\n", status_word(&it.status), it.doc, go_quote(&l.title), it.path).as_bytes(),
        );
        if !it.fragment.is_empty() {
            b.extend_from_slice(format!("  about: «{}»\n", indent(&it.fragment)).as_bytes());
        }
        b.extend_from_slice(b"  ");
        b.extend_from_slice(&who(&it.author, &it.created));
        b.extend_from_slice(format!("{}\n", indent(&it.text)).as_bytes());
        if !it.reply.is_empty() {
            b.extend_from_slice(format!("  reply: {}\n", indent(&it.reply)).as_bytes());
        }
        let s = match l.state {
            comments::UNCHANGED | comments::MOVED => format!("  block: {}\n", indent(&it.quote)),
            comments::CHANGED => {
                format!("  block when commented: {}\n  block now: {}\n", indent(&it.quote), indent(&l.current))
            }
            comments::REMOVED => format!("  block is gone; it read: {}\n", indent(&it.quote)),
            comments::MISSING => format!("  document {} is gone; the block read: {}\n", it.doc, indent(&it.quote)),
            _ => String::new(),
        };
        b.extend_from_slice(s.as_bytes());
        b.push(b'\n');
        let _ = stdout.write_all(&b);
    }
    say(stdout, &format!("{open} not processed, {} in all ({})\n", items.len(), comments::FILE));
    Ok(0)
}

/// who is "author date: " before the text of a comment, "" when both are
/// empty. Go cuts created at byte 10, which may split a character: the cut
/// bytes stay, and strings.TrimSpace stops at them.
fn who(author: &str, created: &str) -> Vec<u8> {
    let mut w = author.as_bytes().to_vec();
    if !created.is_empty() {
        let cut = &created.as_bytes()[..created.len().min(10)];
        let ok = cut.utf8_chunks().next().map_or(0, |ch| ch.valid().len());
        let head = format!("{author} {}", &created[..ok]);
        w = if ok == cut.len() { head.trim() } else { head.trim_start() }.as_bytes().to_vec();
        w.extend_from_slice(&cut[ok..]);
    }
    if !w.is_empty() {
        w.extend_from_slice(b": ");
    }
    w
}

fn status_word(s: &str) -> &'static str {
    if s == comments::DONE { "done" } else { "open" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_like_go() {
        assert_eq!(who("", ""), b"");
        assert_eq!(who("C", "2026-10-15T10:00:00Z"), b"C 2026-10-15: ");
        assert_eq!(who("", "2026-10-15T10:00:00Z"), b"2026-10-15: ");
        assert_eq!(who("C", ""), b"C: ");
        // a cut inside "é" (bytes 9 and 10) keeps the first byte, as Go
        assert_eq!(who("", "2026-10-1é"), b"2026-10-1\xC3: ");
        assert_eq!(who(" ", "         é"), b"\xC3: ");
    }
}
