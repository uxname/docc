//! Package trace finds docc: tags in code files.
//!
//! A code file is not required to be UTF-8, so lines are handled as bytes,
//! as Go handles them inside a string.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::{Glob, match_any};
use crate::source::{Rev, Source};
use crate::text;

const MAX_FILE_SIZE: u64 = 1 << 20;

/// Tag is one "docc:" tag, or the opening "docc:begin" line of a region.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tag {
    /// path relative to the project root, with '/'
    pub file: String,
    pub line: usize,
    pub col: usize,
    /// as written: id, id#field[/section] or id#^anchor
    pub targets: Vec<String>,
    /// docc:begin ... docc:end
    pub region: bool,
    /// region lines between the markers, trimmed, NFC, joined with \n;
    /// bytes, because the file may hold invalid UTF-8 and the hash covers it as is
    pub body: Vec<u8>,
    /// a malformed region: unclosed, nested or a stray end
    pub err: String,
    /// the version hash written after each target ("@3a9c"), lower case
    pub hashes: Vec<Option<String>>,
    /// the fingerprint written on the line ("~e91d"), lower case
    pub print: Option<String>,
    /// the fingerprint of the code now: of the region for docc:begin, of the
    /// lines outside the regions of the file for a plain tag
    pub want: String,
}

/// TAG_HASHES are the version hashes and the fingerprint on a docc: line,
/// which no fingerprint covers (so writing them never changes one).
static TAG_HASHES: std::sync::LazyLock<regex::bytes::Regex> = std::sync::LazyLock::new(|| {
    regex::bytes::Regex::new(r"(?i)@[2-9a-hjkmnp-z]{4}\b|[\t ]*~[2-9a-hjkmnp-z]{4}\b").unwrap()
});

/// PRINT is the fingerprint on a docc: line.
static PRINT: std::sync::LazyLock<regex::bytes::Regex> =
    std::sync::LazyLock::new(|| regex::bytes::Regex::new(r"(?i)(?:^|[\t ])~([2-9a-hjkmnp-z]{4})\b").unwrap());

/// tag_at is the offset of "docc:" in a line that is a tag: only blanks and
/// comment markers stand before it.
pub fn tag_at(line: &[u8]) -> Option<usize> {
    line.windows(5).position(|w| w == b"docc:").filter(|&at| comment_only(&line[..at]))
}

/// Key is a pair of a code file and a target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key {
    pub file: String,
    pub target: String,
}

/// Hashes returns, per file and target, the hex SHA-256 of the bodies of all
/// closed regions with that target, in file order. A plain tag has no hash.
pub fn hashes(tags: &[Tag]) -> HashMap<Key, String> {
    let mut bodies: HashMap<Key, Vec<&[u8]>> = HashMap::new();
    for t in tags {
        if t.region && t.err.is_empty() {
            for g in &t.targets {
                let k = Key { file: t.file.clone(), target: g.clone() };
                bodies.entry(k).or_default().push(&t.body);
            }
        }
    }
    bodies
        .into_iter()
        .map(|(k, b)| (k, hex::encode(Sha256::digest(b.join(&0u8))))) // NUL never occurs in a scanned file
        .collect()
}

/// Scan searches the files that match masks (relative to root). Inside a git
/// work tree the files are those of `git ls-files -co --exclude-standard`,
/// and of each submodule by its own git; otherwise the tree is walked without
/// .git, node_modules and vendor.
/// Skipped always: the given directories (docc's own src, out and outFull),
/// symbolic links, files over 1 MB and files with a NUL byte.
pub fn scan(root: &Path, masks: &[Glob], skip_dirs: &[PathBuf], src: &Source) -> io::Result<Vec<Tag>> {
    if let Source::Rev(rev) = src {
        // ponytail: a submodule's code is not read at a revision (only its commit is in the tree); read its own history if --rev ever needs it
        return Ok(rev_tags(root, "", rev, masks, skip_dirs));
    }
    let paths = list(root, skip_dirs)?;
    let mut tags = Vec::new();
    for rel in &paths {
        if !match_any(masks, rel) {
            continue;
        }
        let abs = root.join(rel);
        match std::fs::symlink_metadata(&abs) {
            Ok(st) if st.is_file() && st.len() <= MAX_FILE_SIZE => {}
            _ => continue,
        }
        let data = match std::fs::read(&abs) {
            Ok(d) if !d.contains(&0) => d,
            _ => continue,
        };
        tags.extend(scan_file(rel, data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data)));
    }
    Ok(tags)
}

/// scan_head reads the tags of the files below the project-relative folder
/// dir ("" for the root) as they are at HEAD of the Git repository that holds
/// dir: the project's, or a submodule's own. None when dir is not in a Git
/// work tree or the repository has no commit yet.
pub fn scan_head(root: &Path, dir: &str, masks: &[Glob], skip_dirs: &[PathBuf]) -> Option<Vec<Tag>> {
    let rev = Rev::open(&root.join(dir), "HEAD").ok()?;
    Some(rev_tags(root, dir, &rev, masks, skip_dirs))
}

/// rev_tags scans the files of a revision opened at the project-relative
/// folder pre; there is nothing untracked or ignored in it. Tags are named
/// by their project-relative path, as on the disk.
fn rev_tags(root: &Path, pre: &str, rev: &Rev, masks: &[Glob], skip_dirs: &[PathBuf]) -> Vec<Tag> {
    let proj = |rel: &str| if pre.is_empty() { rel.to_string() } else { format!("{pre}/{rel}") };
    let under = |rel: &str| skip_dirs.iter().any(|d| inside(d, &root.join(rel)));
    let paths: Vec<(&str, String)> = rev
        .files("")
        .map(|(rel, b)| (rel, b, proj(rel)))
        .filter(|(_, b, p)| !b.link && b.size <= MAX_FILE_SIZE && match_any(masks, p) && !under(p))
        .map(|(rel, _, p)| (rel, p))
        .collect();
    rev.prefetch(paths.iter().map(|(rel, _)| *rel));
    let mut tags = Vec::new();
    for (_, p) in paths {
        if let Ok(data) = rev.read(&root.join(&p))
            && !data.contains(&0)
        {
            tags.extend(scan_file(&p, data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data)));
        }
    }
    tags
}

/// files are the code files on the disk that scan reads: under masks, not
/// in skip_dirs, regular, at most 1 MB.
pub fn files(root: &Path, masks: &[Glob], skip_dirs: &[PathBuf]) -> io::Result<Vec<String>> {
    Ok(list(root, skip_dirs)?
        .into_iter()
        .filter(|rel| {
            match_any(masks, rel)
                && std::fs::symlink_metadata(root.join(rel)).is_ok_and(|st| st.is_file() && st.len() <= MAX_FILE_SIZE)
        })
        .collect())
}

/// list returns the candidate files, relative to root with '/', sorted.
// ponytail: a path that is not UTF-8 is dropped; Go would scan it under its raw bytes.
fn list(root: &Path, skip_dirs: &[PathBuf]) -> io::Result<Vec<String>> {
    let under = |abs: &Path| skip_dirs.iter().any(|d| inside(d, abs));
    let mut paths = Vec::new();
    let out = crate::git()
        .args(["ls-files", "-co", "--exclude-standard", "-z"])
        .current_dir(root)
        .output();
    if let Some(out) = out.ok().filter(|o| o.status.success()) {
        for p in out.stdout.split(|&b| b == 0) {
            if let Ok(p) = std::str::from_utf8(p)
                && !p.is_empty()
                && !under(&root.join(p))
            {
                // a submodule, or a repository nested in the tree, is one entry
                // here: its files are listed by its own git, with its own ignores.
                // Only a real folder: a link is never followed (one to "." loops)
                let p = p.trim_end_matches('/');
                if std::fs::symlink_metadata(root.join(p)).is_ok_and(|m| m.is_dir()) && root.join(p).join(".git").exists() {
                    paths.extend(list(&root.join(p), skip_dirs)?.into_iter().map(|q| format!("{p}/{q}")));
                } else {
                    paths.push(p.to_string());
                }
            }
        }
        paths.sort();
        paths.dedup(); // unmerged files are listed once per stage
        return Ok(paths);
    }
    match std::fs::symlink_metadata(root) {
        Err(_) => {} // unreadable entries are skipped
        Ok(st) if st.is_dir() => walk(root, "", &under, &mut paths),
        Ok(_) => paths.push(".".into()),
    }
    paths.sort();
    Ok(paths)
}

/// walk adds the files below dir (rel is its path from the root, "" for the
/// root) in name order, as filepath.WalkDir visits them.
fn walk(dir: &Path, rel: &str, under: &dyn Fn(&Path) -> bool, paths: &mut Vec<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return; // unreadable entries are skipped
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Some(name) = e.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            if !(name == ".git" || name == "node_modules" || name == "vendor" || under(&p)) {
                walk(&p, &r, under, paths);
            }
            continue;
        }
        paths.push(r);
    }
}

/// inside reports whether abs is d or below it, as filepath.Rel(d, abs)
/// giving neither an error nor a path that starts with "..".
fn inside(d: &Path, abs: &Path) -> bool {
    let (d, abs) = (clean(d), clean(abs));
    if d.is_absolute() != abs.is_absolute() {
        return false;
    }
    abs.strip_prefix(&d)
        .is_ok_and(|rest| rest.components().next() != Some(Component::ParentDir))
}

/// clean is filepath.Clean without the final ".": "." dropped, ".." removes
/// the part before it.
fn clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.components().next_back() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(".."),
            },
            c => out.push(c),
        }
    }
    out
}

/// markers are the comment openers a tag may follow; longer ones first.
const MARKERS: [&str; 10] = ["<!--", "/**", "/*", "//", "--", "#", "*", ";", "%", "'"];

/// comment_only reports whether s holds only blanks and comment markers.
fn comment_only(s: &[u8]) -> bool {
    let mut s = trim_space(s);
    while !s.is_empty() {
        let Some(m) = MARKERS.iter().find(|m| s.starts_with(m.as_bytes())) else {
            return false;
        };
        s = trim_space(&s[m.len()..]);
    }
    true
}

/// targets splits "a@3a9c, b#x */ ~e91d" into its targets (without their
/// hashes) and their hashes; comment closers and the fingerprint are dropped.
fn targets(s: &[u8]) -> (Vec<String>, Vec<Option<String>>) {
    let s = PRINT.replace_all(s, &b""[..]);
    let (mut ts, mut hs) = (Vec::new(), Vec::new());
    for t in raw_targets(&s) {
        let (t, h) = crate::codes::split_hash(&t);
        ts.push(t);
        hs.push(h);
    }
    (ts, hs)
}

fn raw_targets(s: &[u8]) -> Vec<String> {
    let mut s = trim_space(s);
    for c in ["*/", "-->"] {
        s = trim_space(s.strip_suffix(c.as_bytes()).unwrap_or(s));
    }
    if s.is_empty() {
        return Vec::new();
    }
    // ponytail: a target with invalid UTF-8 never resolves; it is shown with U+FFFD.
    s.split(|&b| b == b',')
        .map(|t| String::from_utf8_lossy(trim_space(t)).into_owned())
        .collect()
}

/// norm_line is a line as fingerprints read it: trimmed, NFC, and on a
/// docc: line without the hashes and the fingerprint.
fn norm_line(line: &[u8]) -> Vec<u8> {
    let l = nfc(trim_space(line));
    if tag_at(&l).is_some() {
        return TAG_HASHES.replace_all(&l, &b""[..]).into_owned();
    }
    l
}

/// scan_file finds the tags of a file; every tag gets the fingerprint it should carry.
pub fn scan_file(rel: &str, data: &[u8]) -> Vec<Tag> {
    let mut tags = scan_lines(rel, data);
    let mut outside: Vec<Vec<u8>> = Vec::new();
    let mut inside = false;
    for line in data.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let word = tag_at(line).map(|at| {
            let rest = &line[at + 5..];
            let w = &rest[..index_space(rest).unwrap_or(rest.len())];
            let w = w.strip_suffix(b"*/").unwrap_or(w);
            trim_space(w.strip_suffix(b"-->").unwrap_or(w)).to_vec()
        });
        match word.as_deref() {
            Some(b"begin") => {
                outside.push(norm_line(line));
                inside = true;
            }
            Some(b"end") => {
                outside.push(norm_line(line));
                inside = false;
            }
            _ if !inside => outside.push(norm_line(line)),
            _ => {}
        }
    }
    let file = crate::codes::hash4_bytes(&outside.join(&b'\n'));
    for t in &mut tags {
        t.want = if t.region { crate::codes::hash4_bytes(&t.body) } else { file.clone() };
    }
    tags
}

fn scan_lines(rel: &str, data: &[u8]) -> Vec<Tag> {
    let mut tags: Vec<Tag> = Vec::new();
    let mut open: Option<usize> = None; // index in tags of the open region
    let mut body: Vec<Vec<u8>> = Vec::new();
    for (i, line) in data.split(|&b| b == b'\n').enumerate() {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(at) = tag_at(line) else {
            if open.is_some() {
                body.push(nfc(trim_space(line)));
            }
            continue;
        };
        // line[..at] is only blanks and markers, so it is valid UTF-8
        let col = String::from_utf8_lossy(&line[..at]).chars().count() + 1;
        let mut t = Tag { file: rel.to_string(), line: i + 1, col, ..Tag::default() };
        t.print = PRINT.captures(line).map(|c| String::from_utf8_lossy(&c[1]).to_ascii_lowercase());
        let rest = &line[at + "docc:".len()..];
        let (word, args) = match index_space(rest) {
            Some(j) => (&rest[..j], &rest[j..]),
            None => (rest, &rest[rest.len()..]),
        };
        let word = word.strip_suffix(b"*/").unwrap_or(word);
        let word = trim_space(word.strip_suffix(b"-->").unwrap_or(word));
        match word {
            b"begin" => {
                t.region = true;
                (t.targets, t.hashes) = targets(args);
                if open.is_some() {
                    t.err = "a region opened inside another region".into();
                } else {
                    open = Some(tags.len());
                    body = Vec::new();
                }
            }
            b"end" => match open.take() {
                None => t.err = "docc:end without docc:begin".into(),
                Some(o) => {
                    tags[o].body = body.join(&b'\n');
                    continue;
                }
            },
            _ => {
                (t.targets, t.hashes) = targets(rest);
                if open.is_some() {
                    body.push(norm_line(line));
                }
            }
        }
        tags.push(t);
    }
    if let Some(o) = open {
        tags[o].err = "docc:begin without docc:end".into();
    }
    tags
}

/// nfc is norm.NFC.Bytes: the valid UTF-8 runs are normalized, invalid bytes
/// are kept and nothing composes across them.
fn nfc(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for c in s.utf8_chunks() {
        out.extend_from_slice(text::nfc(c.valid()).as_bytes());
        out.extend_from_slice(c.invalid());
    }
    out
}

/// first_rune decodes the rune at the start of s; None for an invalid byte or
/// an empty s. It returns the rune and its width.
fn first_rune(s: &[u8]) -> Option<(char, usize)> {
    let c = s[..s.len().min(4)].utf8_chunks().next()?;
    c.valid().chars().next().map(|r| (r, r.len_utf8()))
}

/// trim_space is bytes.TrimSpace: leading and trailing Unicode white space
/// removed; an invalid byte is not white space.
fn trim_space(mut s: &[u8]) -> &[u8] {
    while let Some((_, n)) = first_rune(s).filter(|(r, _)| r.is_whitespace()) {
        s = &s[n..];
    }
    // a white space rune is 1 to 3 bytes long; only one of the tails can be a whole rune
    'outer: loop {
        for n in 1..=3.min(s.len()) {
            if let Ok(t) = std::str::from_utf8(&s[s.len() - n..])
                && t.chars().count() == 1
            {
                if t.chars().all(char::is_whitespace) {
                    s = &s[..s.len() - n];
                    continue 'outer;
                }
                break 'outer;
            }
        }
        break;
    }
    s
}

/// index_space is the byte offset of the first Unicode white space rune in s.
fn index_space(s: &[u8]) -> Option<usize> {
    let mut off = 0;
    for c in s.utf8_chunks() {
        if let Some(j) = c.valid().find(char::is_whitespace) {
            return Some(off + j);
        }
        off += c.valid().len() + c.invalid().len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use crate::config::compile_glob;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn comment_prefix() {
        for (line, want) in [
            ("// docc: a", true),
            ("  # docc: a", true),
            ("-- docc: a", true),
            ("/* docc: a */", true),
            (" * docc: a", true),
            ("; docc: a", true),
            ("<!-- docc: a -->", true),
            ("% docc: a", true),
            ("' docc: a", true),
            ("/** docc: a", true),
            ("docc: a", true),
            (r#"s := "docc: a""#, false),
            ("x = 1 // docc: a", false),
            ("// see mydocc: a", false),
            ("// text then docc: a", false),
        ] {
            let tags = scan_file("f", line.as_bytes());
            let got = tags.len() == 1 && tags[0].targets == ["a"];
            assert_eq!(got, want, "{line:?}: got {got} {tags:?}");
        }
    }

    fn key(f: &str, t: &str) -> Key {
        Key { file: f.into(), target: t.into() }
    }

    #[test]
    fn regions() {
        let tags = scan_file(
            "f",
            b"// docc:begin a, b#^x\n  x := 1  \r\n// docc: c\n// docc:end\n// docc:begin a\ny\n// docc:end\n",
        );
        assert!(
            tags.len() == 3 && tags[0].region && tags[0].body == b"x := 1\n// docc: c" && tags[2].body == b"y",
            "{tags:?}"
        );
        let h = hashes(&tags);
        assert!(
            h.len() == 2 && h[&key("f", "a")] != h[&key("f", "b#^x")] && !h.contains_key(&key("f", "c")),
            "a has two regions, b one, c is a plain tag: {h:?}"
        );
        // indentation, line ends and Unicode normal form do not change the hash
        let same = scan_file("f", b"\t// docc:begin b#^x\r\nx := 1\n\t// docc: c\n// docc:end");
        assert_eq!(hashes(&same)[&key("f", "b#^x")], h[&key("f", "b#^x")], "trim changed the hash");
        let nfc = hashes(&scan_file("f", "// docc:begin a\n\u{e9}\n// docc:end\n".as_bytes()));
        let nfd = hashes(&scan_file("f", "// docc:begin a\ne\u{301}\n// docc:end\n".as_bytes()));
        assert_eq!(nfc[&key("f", "a")], nfd[&key("f", "a")], "NFC and NFD differ");
        for (src, want) in [
            ("// docc:begin a\n", "docc:begin without docc:end"),
            ("// docc:end\n", "docc:end without docc:begin"),
            ("// docc:begin a\n// docc:begin b\n// docc:end", "a region opened inside another region"),
        ] {
            assert!(scan_file("f", src.as_bytes()).iter().any(|t| t.err == want), "{src:?}: want {want:?}");
        }
    }

    // Values taken from the Go implementation: invalid UTF-8 is hashed as is,
    // a no-break space separates the word, nothing composes across a bad byte.
    #[test]
    fn invalid_utf8_like_go() {
        assert!(scan_file("f", b"\xff\xfe // docc: a\xff, b\n").is_empty());
        let tags = scan_file("f", "\u{a0}// docc:begin\u{a0}a\nx\n//docc:end".as_bytes());
        assert_eq!((tags[0].col, &tags[0].targets[..], &tags[0].body[..]), (5, &["a".to_string()][..], &b"x"[..]));
        let tags = scan_file("f", b"// docc:begin a\n\xe2\x80 x\xff\n//docc:end");
        assert_eq!(hashes(&tags)[&key("f", "a")], "fed973ad322301171dd085ccc0a594b2b76f510515f1bac0684de8987df96989");
        assert_eq!(nfc(b"\xffe\xcc\x81"), b"\xff\xc3\xa9");
        assert_eq!(nfc(b"e\xff\xcc\x81"), b"e\xff\xcc\x81");
        assert_eq!(trim_space("\u{a0} a\u{2028}\u{85}".as_bytes()), b"a");
        assert_eq!(trim_space(b"\xe2\x80 a \xff "), b"\xe2\x80 a \xff");
    }

    fn temp_dir() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "docc-trace-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn listing() {
        let write = |root: &Path, files: &[&str]| {
            for f in files {
                let p = root.join(f);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, "// docc: x\n").unwrap();
            }
        };
        let files = |root: &Path| -> Vec<String> {
            let all = compile_glob("**").unwrap();
            let tags = scan(root, &[all], &[root.join("docs")], &Source::Disk).unwrap();
            tags.into_iter().map(|t| t.file).collect()
        };
        let plain = temp_dir();
        write(&plain, &["a.go", "node_modules/m.js", "vendor/v.go", "docs/d.go"]);
        assert_eq!(files(&plain), ["a.go"], "walk");
        std::fs::remove_dir_all(&plain).unwrap();
        if Command::new("git").arg("--version").output().is_err() {
            return; // no git
        }
        let repo = temp_dir();
        write(&repo, &["a.go", "ignored.go", "vendor/v.go", "docs/d.go", ".gitignore"]);
        std::fs::write(repo.join(".gitignore"), "ignored.go\n").unwrap();
        let out = Command::new("git").arg("-C").arg(&repo).args(["init", "-q"]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(files(&repo), ["a.go", "vendor/v.go"], "git");
        std::fs::remove_dir_all(&repo).unwrap();
    }

    // A committed link to a folder that holds .git (here the root itself) is
    // not a nested repository: following it read loop/loop/... until ELOOP.
    #[cfg(unix)]
    #[test]
    fn symlink_to_a_repository_is_not_followed() {
        if Command::new("git").arg("--version").output().is_err() {
            return; // no git
        }
        let repo = temp_dir();
        std::fs::write(repo.join("a.go"), "// docc: x\n").unwrap();
        std::os::unix::fs::symlink(".", repo.join("loop")).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("git").arg("-C").arg(&repo).args(args).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q"]);
        git(&["add", "a.go", "loop"]);
        assert_eq!(list(&repo, &[]).unwrap(), ["a.go", "loop"]);
        std::fs::remove_dir_all(&repo).unwrap();
    }
}
