//! docc ack: confirms one place an agent has just read. It writes the current
//! version hash of the target into the stale links of one line (or into one
//! target of a docc: tag), or the current fingerprint into one tag, and says
//! what it changed. There is no bulk form: each place is named on its own.

use serde::Serialize;

use super::migrate::{split_tail, splice, write_doc};
use super::project::{Project, go_path_error};
use crate::codes;
use crate::trace;

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Acked is one hash or fingerprint docc ack wrote.
#[derive(Debug, Serialize)]
pub struct Acked {
    pub file: String,
    pub line: usize,
    /// the target as written, "" for the fingerprint of a tag
    pub target: String,
    pub old: Option<String>,
    pub new: String,
}

/// Stale is a hash or fingerprint on the line that does not match: its
/// target ("" for a fingerprint), the value written and the value now.
struct Stale {
    target: String,
    had: Option<String>,
    now: String,
    /// the source range of the [[...]] of a link in a document
    span: Option<(usize, usize)>,
}

/// picks says whether the place the user named fits the stale item: a target
/// as in a link (id, id#field, id#^code, #^code, ^code, [[...]], a hash after
/// it is ignored) or "~" for the fingerprint of a tag.
fn picks(pick: &str, s: &Stale) -> bool {
    if pick.starts_with('~') {
        return s.target.is_empty();
    }
    let p = pick.trim().trim_start_matches("[[").trim_end_matches("]]");
    let p = codes::split_hash(p).0.to_ascii_lowercase();
    let t = s.target.to_ascii_lowercase();
    !t.is_empty() && (t == p || (p.starts_with('#') && t.ends_with(&p)) || (p.starts_with('^') && t.ends_with(&format!("#{p}"))))
}

fn show(target: &str) -> String {
    if target.is_empty() { "the fingerprint ~".into() } else { target.to_string() }
}

/// ack confirms the place rel:line (rel relative to the project root): the
/// one stale target there, or the one pick names when there are several.
/// Two links to the same target on one line are one place.
pub fn ack(p: &Project, rel: &str, line: usize, pick: Option<&str>) -> Res<Vec<Acked>> {
    let doc = p.docs.iter().find(|d| d.file == rel && d.parsed());
    let mut stale = Vec::new();
    let mut code = None;
    if let Some(d) = doc {
        let data = &d.res().src.data;
        for r in d.refs.iter().filter(|r| r.ok) {
            if data[..r.start].iter().filter(|&&b| b == b'\n').count() + 1 != line {
                continue;
            }
            if let Some(v) = p.version(&r.inner).filter(|v| r.hash.as_deref() != Some(v.hash.as_str())) {
                stale.push(Stale { target: r.inner.clone(), had: r.hash.clone(), now: v.hash, span: Some((r.start, r.end)) });
            }
        }
    } else {
        let abs = p.cfg.root.join(rel);
        let data = std::fs::read(&abs).map_err(|e| go_path_error("open", &abs, &e))?;
        let bom = data.starts_with(b"\xEF\xBB\xBF");
        let text = String::from_utf8(data[if bom { 3 } else { 0 }..].to_vec())
            .map_err(|_| format!("{rel} is not UTF-8: write the hash or fingerprint by hand"))?;
        for t in trace::scan_file(rel, text.as_bytes()).into_iter().filter(|t| t.line == line && t.err.is_empty()) {
            for (g, had) in t.targets.iter().zip(&t.hashes) {
                if let Some(v) = p.version(g).filter(|v| had.as_deref() != Some(v.hash.as_str())) {
                    stale.push(Stale { target: g.clone(), had: had.clone(), now: v.hash, span: None });
                }
            }
            if !t.targets.is_empty() && t.print.as_deref() != Some(t.want.as_str()) {
                stale.push(Stale { target: String::new(), had: t.print.clone(), now: t.want.clone(), span: None });
            }
        }
        code = Some((abs, bom, text));
    }
    if let Some(pk) = pick {
        stale.retain(|s| picks(pk, s));
    }
    let Some(first) = stale.first() else {
        let what = pick.map(|pk| format!(" for {pk}")).unwrap_or_default();
        return Err(format!("{rel}:{line}: nothing to confirm{what}: no link or docc: tag there whose hash or fingerprint is old (docc validate lists them)").into());
    };
    let target = first.target.clone();
    if stale.iter().any(|s| s.target != target) {
        let names: Vec<String> = stale.iter().map(|s| show(&s.target)).collect();
        return Err(format!(
            "{rel}:{line} has {} things to confirm: {}. Name one after the place, as in docc ack {rel}:{line} '{}' ('~' for the fingerprint)",
            stale.len(),
            names.join(", "),
            if target.is_empty() { "~" } else { target.as_str() }
        )
        .into());
    }
    if let Some((abs, bom, text)) = code {
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        let s = &stale[0];
        lines[line - 1] = if s.target.is_empty() { write_print(&lines[line - 1], s) } else { write_target(&lines[line - 1], s) };
        let out = [if bom { &b"\xEF\xBB\xBF"[..] } else { b"" }, lines.join("\n").as_bytes()].concat();
        std::fs::write(&abs, out).map_err(|e| go_path_error("open", &abs, &e))?;
    } else {
        let d = doc.expect("a document");
        let data = &d.res().src.data;
        let mut edits = Vec::new();
        for s in &stale {
            let (_, e) = s.span.expect("a link");
            // the hash is the last four signs before "]]", as docc migrate writes it
            if &data[e - 2..e] != b"]]" || (s.had.is_some() && data[e - 7] != b'@') {
                return Err(format!("{rel}:{line}: the link to {} is written in a way docc ack does not edit: write @{} by hand", s.target, s.now).into());
            }
            edits.push(match s.had {
                Some(_) => (e - 6, e - 2, s.now.clone()),
                None => (e - 2, e - 2, format!("@{}", s.now)),
            });
        }
        write_doc(p, d, &splice(data, edits))?;
    }
    Ok(stale
        .into_iter()
        .map(|s| Acked { file: rel.to_string(), line, target: s.target, old: s.had, new: s.now })
        .collect())
}

/// write_target puts the hash now after the target s on a docc: line.
fn write_target(line: &str, s: &Stale) -> String {
    let at = trace::tag_at(line.as_bytes()).expect("a tag line");
    let mut start = at + "docc:".len();
    if line[start..].split(char::is_whitespace).next() == Some("begin") {
        start += "begin".len();
    }
    let (body, tail) = split_tail(&line[start..]);
    let pieces: Vec<String> = body
        .split(',')
        .map(|piece| {
            let core = piece.trim();
            // the fingerprint after the last target stays where it is
            let tgt = core.split([' ', '\t']).next().unwrap_or("");
            let (t, h) = codes::split_hash(tgt);
            if !t.eq_ignore_ascii_case(&s.target) {
                return piece.to_string();
            }
            let bare = if h.is_some() { &tgt[..tgt.rfind('@').expect("a hash")] } else { tgt };
            piece.replacen(core, &format!("{bare}@{}{}", s.now, &core[tgt.len()..]), 1)
        })
        .collect();
    format!("{}{}{tail}", &line[..start], pieces.join(","))
}

/// write_print puts the fingerprint now on a docc: line.
fn write_print(line: &str, s: &Stale) -> String {
    if let Some(p) = &s.had
        && let Some(at) = line.to_ascii_lowercase().rfind(&format!("~{p}"))
    {
        let mut l = line.to_string();
        l.replace_range(at + 1..at + 1 + p.len(), &s.now);
        return l;
    }
    let cr = line.ends_with('\r');
    let l = line.trim_end_matches('\r');
    let at = trace::tag_at(l.as_bytes()).expect("a tag line");
    let (body, tail) = split_tail(&l[at..]);
    format!("{}{} ~{}{tail}{}", &l[..at], body.trim_end(), s.now, if cr { "\r" } else { "" })
}
