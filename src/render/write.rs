use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::go_quote;
use crate::engine::{self, Project, UsageError};

use super::text::path_ext;

/// inside reports whether path a is b or lies under b.
// ponytail: no filepath.Clean, every caller passes paths from Config.abs, which are clean.
fn inside(a: &Path, b: &Path) -> bool {
    let (a, b) = (
        a.as_os_str().as_encoded_bytes(),
        b.as_os_str().as_encoded_bytes(),
    );
    a == b
        || (a.starts_with(b) && a[b.len()..].starts_with(std::path::MAIN_SEPARATOR_STR.as_bytes()))
}

/// config_path is filepath.Join(cfg.Root, filepath.Base(cfg.File)).
fn config_path(p: &Project) -> PathBuf {
    let base = Path::new(&p.cfg.file)
        .file_name()
        .map(OsString::from)
        .unwrap_or_else(|| ".".into());
    p.cfg.root.join(base)
}

/// CheckDir refuses an output directory that docc must not empty: one that
/// holds the project root, the config, the sources or the schemas, lies inside
/// the sources or the schemas, or overlaps the other output directory.
pub fn check_dir(p: &Project, rel: &str) -> Result<(), UsageError> {
    let cfg = &p.cfg;
    let out = cfg.abs(rel);
    let (src, sch) = (cfg.abs(&cfg.src), cfg.abs(&cfg.schemas));
    let mut bad = inside(&cfg.root, &out)
        || inside(&src, &out)
        || inside(&sch, &out)
        || inside(&config_path(p), &out)
        || inside(&out, &src)
        || inside(&out, &sch);
    if cfg.abs(&cfg.out) == cfg.abs(&cfg.out_full) {
        bad = true;
    }
    for other in [&cfg.out, &cfg.out_full] {
        if other != rel && (inside(&out, &cfg.abs(other)) || inside(&cfg.abs(other), &out)) {
            bad = true;
        }
    }
    if bad {
        return Err(UsageError {
            msg: format!(
                "output directory {} overlaps the project (root, config, sources, schemas or the other output directory); choose a dedicated directory",
                go_quote(rel)
            ),
        });
    }
    Ok(())
}

/// SingleFile returns the absolute path of a single-file build: the output
/// directory with ext added ("docs/out" gives "docs/out.md"). It refuses a path
/// that would overwrite the config, the sources or the schemas.
pub fn single_file(p: &Project, out_rel: &str, ext: &str) -> Result<PathBuf, UsageError> {
    let cfg = &p.cfg;
    let mut f = cfg.abs(out_rel).into_os_string();
    f.push(ext);
    let f = PathBuf::from(f);
    if inside(&f, &cfg.abs(&cfg.src)) || inside(&f, &cfg.abs(&cfg.schemas)) || f == config_path(p) {
        return Err(UsageError {
            msg: format!(
                "single file {} overlaps the project (config, sources or schemas); choose another output directory",
                go_quote(&f.to_string_lossy())
            ),
        });
    }
    Ok(f)
}

/// WriteFile writes one file (LF, UTF-8) unless it already has this content.
pub fn write_file(
    path: &Path,
    content: &[u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::fs::read(path).is_ok_and(|old| old == content) {
        return Ok(());
    }
    Ok(os_write_file(path, content)?)
}

/// os_write_file is os.WriteFile(path, content, 0o644) with Go's error text.
fn os_write_file(path: &Path, content: &[u8]) -> Result<(), String> {
    use std::io::Write as _;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o644);
    let mut f = o.open(path).map_err(|e| go_path_error("open", path, &e))?;
    f.write_all(content)
        .map_err(|e| go_path_error("write", path, &e))
}

/// mkdir_all is os.MkdirAll(p, 0o755) with Go's error text, which names the
/// directory that could not be made.
fn mkdir_all(p: &Path) -> Result<(), String> {
    match std::fs::metadata(p) {
        Ok(st) if st.is_dir() => return Ok(()),
        Ok(_) => return Err(format!("mkdir {}: not a directory", p.display())),
        Err(_) => {}
    }
    if let Some(parent) = p.parent()
        && !parent.as_os_str().is_empty()
    {
        mkdir_all(parent)?;
    }
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o755);
    match b.create(p) {
        Ok(()) => Ok(()),
        Err(_) if std::fs::symlink_metadata(p).is_ok_and(|st| st.is_dir()) => Ok(()),
        Err(e) => Err(go_path_error("mkdir", p, &e)),
    }
}

/// go_path_error is the text of Go's *fs.PathError: "<op> <path>: <reason>".
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

/// GeneratedFile reports whether the single-file build f (ext ".md", ".html" or
/// ".json") was written by docc: the Markdown starts with the mark, the HTML
/// holds the data island, the data holds its comments key.
pub fn generated_file(f: &Path, ext: &str) -> bool {
    let Ok(b) = std::fs::read(f) else {
        return false;
    };
    if ext == ".md" {
        return b.starts_with(GENERATED_MARK.as_bytes());
    }
    // the JSON data has no mark of its own: its comments key is docc:<folder>
    let pat: &[u8] = if ext == ".json" { br#""key":"docc:"# } else { br#"id="docc-data""# };
    b.windows(pat.len()).any(|w| w == pat)
}

/// generatedMark starts every page docc writes.
const GENERATED_MARK: &str = "<!-- GENERATED by docc";

/// junk are files a file manager adds; docc may remove them.
fn junk(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini")
}

/// ours reports whether the file rel in the output directory dir is one docc
/// may remove when no source produces it any more: a page it generated, a copy
/// of an image that lies at the same path in src, or file manager junk.
fn ours(dir: &Path, src_dir: &Path, rel: &Path) -> bool {
    if rel.file_name().and_then(|n| n.to_str()).is_some_and(junk) {
        return true;
    }
    if std::fs::metadata(src_dir.join(rel)).is_ok_and(|st| st.is_file()) {
        return true;
    }
    let Ok(f) = std::fs::File::open(dir.join(rel)) else {
        return false;
    };
    let mut head = Vec::new();
    let _ = f.take(GENERATED_MARK.len() as u64).read_to_end(&mut head);
    head == GENERATED_MARK.as_bytes()
}

/// Entry is one file or directory met by walk_dir.
#[derive(Clone)]
struct Entry {
    /// OS path
    path: PathBuf,
    /// path relative to the root, as the OS names it (raw bytes kept)
    rel_os: PathBuf,
    /// rel_os with '/' separators, U+FFFD for bytes that are not UTF-8
    rel: String,
    /// rel names the file exactly (no U+FFFD substituted)
    utf8: bool,
    is_dir: bool,
}

impl Entry {
    /// key is the path as a key of files: none for a name that is not UTF-8,
    /// which no source can produce.
    fn key(&self) -> Option<&str> {
        self.utf8.then_some(self.rel.as_str())
    }
}

/// walk_dir is filepath.WalkDir without following links: every entry below
/// root in lexical order. A root that is not a directory is the one entry ".".
/// Like WalkDir with a callback that returns nil on an error, a directory that
/// cannot be read is skipped and the walk goes on; the errors (Go's text) are
/// returned in walk order.
fn walk_dir(root: &Path) -> (Vec<Entry>, Vec<String>) {
    let (mut out, mut errs) = (Vec::new(), Vec::new());
    match std::fs::symlink_metadata(root) {
        Err(e) => {
            errs.push(go_path_error("lstat", root, &e));
            return (out, errs);
        }
        Ok(st) if !st.is_dir() => {
            out.push(Entry {
                path: root.to_path_buf(),
                rel_os: PathBuf::from("."),
                rel: ".".to_string(),
                utf8: true,
                is_dir: false,
            });
            return (out, errs);
        }
        Ok(_) => {}
    }
    fn rec(dir: &Path, parent: Option<&Entry>, out: &mut Vec<Entry>, errs: &mut Vec<String>) {
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                errs.push(go_path_error("open", dir, &e));
                return;
            }
        };
        // os.ReadDir: the entries read before an error, sorted
        let mut ents = Vec::new();
        for e in rd {
            match e.and_then(|e| e.file_type().map(|t| (e, t))) {
                Ok(e) => ents.push(e),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    errs.push(go_path_error("readdirent", dir, &e));
                    break;
                }
            }
        }
        ents.sort_by_key(|(e, _)| e.file_name());
        for (e, t) in ents {
            let name = e.file_name();
            let lossy = name.to_string_lossy();
            let ent = Entry {
                path: e.path(),
                rel_os: parent.map_or_else(|| PathBuf::from(&name), |p| p.rel_os.join(&name)),
                rel: parent.map_or_else(|| lossy.to_string(), |p| format!("{}/{lossy}", p.rel)),
                utf8: parent.is_none_or(|p| p.utf8) && name.to_str().is_some(),
                is_dir: t.is_dir(),
            };
            out.push(ent.clone());
            if ent.is_dir {
                rec(&ent.path, Some(&ent), out, errs);
            }
        }
    }
    rec(root, None, &mut out, &mut errs);
    (out, errs)
}

/// foreignFiles lists the files of dir that no source produces and docc did not
/// generate (relative, '/' separators, sorted).
fn foreign_files(dir: &Path, src_dir: &Path, files: &HashMap<String, Vec<u8>>) -> Vec<String> {
    let mut out: Vec<Entry> = walk_dir(dir)
        .0
        .into_iter()
        .filter(|e| {
            !e.is_dir
                && !e.key().is_some_and(|k| files.contains_key(k))
                && !ours(dir, src_dir, &e.rel_os)
        })
        .collect();
    // sort.Strings on the raw names
    out.sort_by(|a, b| {
        let raw = |e: &Entry| e.rel_os.as_os_str().as_encoded_bytes().to_vec();
        raw(a).cmp(&raw(b))
    });
    out.into_iter().map(|e| e.rel).collect()
}

/// Write makes dir contain exactly files: writes them (LF, UTF-8, no BOM) and
/// removes the files docc generated earlier that no source produces any more.
/// It refuses, and changes nothing, when dir holds other files: shown as relDir
/// they would be lost. srcDir is the sources directory.
pub fn write(
    dir: &Path,
    rel_dir: &str,
    src_dir: &Path,
    files: &HashMap<String, Vec<u8>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let foreign = foreign_files(dir, src_dir, files);
    if !foreign.is_empty() {
        let mut list = foreign.clone();
        if list.len() > 5 {
            list.truncate(5);
            list.push(format!("and {} more", foreign.len() - 5));
        }
        return Err(Box::new(UsageError {
            msg: format!(
                "{rel_dir} holds files that docc did not generate and would delete: {}; move or delete them, then run docc build again",
                list.join(", ")
            ),
        }));
    }
    mkdir_all(dir)?;
    for (name, content) in files {
        let p = dir.join(name);
        if let Some(parent) = p.parent() {
            mkdir_all(parent)?;
        }
        if std::fs::read(&p).is_ok_and(|old| old == *content) {
            continue;
        }
        os_write_file(&p, content)?;
    }
    // remove stale entries, deepest first
    let (entries, errs) = walk_dir(dir);
    if let Some(e) = errs.into_iter().next() {
        return Err(e.into());
    }
    let mut extras: Vec<PathBuf> = entries
        .into_iter()
        .filter(|e| e.is_dir || !e.key().is_some_and(|k| files.contains_key(k))) // dirs are removed below when empty
        .map(|e| e.path)
        .collect();
    extras.sort_by_key(|p| std::cmp::Reverse(p.as_os_str().len()));
    for p in extras {
        let Ok(st) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if st.is_dir() {
            if std::fs::read_dir(&p).is_ok_and(|mut d| d.next().is_none()) {
                std::fs::remove_dir(&p).map_err(|e| go_path_error("remove", &p, &e))?;
            }
            continue;
        }
        std::fs::remove_file(&p).map_err(|e| go_path_error("remove", &p, &e))?;
    }
    Ok(())
}

/// normalize strips a BOM and turns CRLF into LF, so files edited on Windows
/// compare equal.
fn normalize(b: &[u8]) -> Vec<u8> {
    let b = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// src names the source document of a generated page, if there is one.
fn src(p: &Project, rel: &str) -> String {
    if let Some(id) = rel.strip_suffix(".md")
        && let Some(d) = p.doc(id)
    {
        return format!(" (source: {})", d.file);
    }
    String::new()
}

/// CheckFresh compares dir with the expected files and reports DOC501 for
/// missing, outdated and extra files.
pub fn check_fresh(p: &Project, out_rel: &str, files: &HashMap<String, Vec<u8>>) {
    let dir = p.cfg.abs(out_rel);
    let mut seen = std::collections::HashSet::new();
    for e in walk_dir(&dir).0 {
        if e.is_dir {
            continue;
        }
        let (path, rel) = (&e.path, &e.rel);
        if let Some(k) = e.key() {
            seen.insert(k.to_string());
        }
        let file = format!("{out_rel}/{rel}");
        let Some(want) = e.key().and_then(|k| files.get(k)) else {
            if ours(&dir, &p.cfg.abs(&p.cfg.src), &e.rel_os) {
                p.report_file(
                    &file,
                    "DOC501",
                    1,
                    1,
                    "extra file in the output directory (no source produces it)",
                    "Run docc build: it removes files that no source produces.",
                );
            } else {
                p.report_file(&file, "DOC501", 1, 1, "extra file in the output directory that docc did not generate", "Move or delete it: docc build refuses to run while the output directory holds files it did not generate.");
            }
            continue;
        };
        let got = std::fs::read(path).map(|g| {
            // pages compare as text, images byte for byte
            if engine::image_ext(&path_ext(&rel).to_ascii_lowercase()).is_none() {
                normalize(&g)
            } else {
                g
            }
        });
        if !got.is_ok_and(|g| g == *want) {
            p.report_file(
                &file,
                "DOC501",
                1,
                1,
                &format!("the generated file is outdated{}", src(p, rel)),
                "Run docc build and commit the result.",
            );
        }
    }
    let mut missing: Vec<&String> = files.keys().filter(|rel| !seen.contains(*rel)).collect();
    missing.sort();
    for rel in missing {
        p.report_file(
            &format!("{out_rel}/{rel}"),
            "DOC501",
            1,
            1,
            &format!("the generated file is missing{}", src(p, rel)),
            "Run docc build and commit the result.",
        );
    }
}
