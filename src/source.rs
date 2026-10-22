//! Package source reads the files of a project: from the disk, or from a Git
//! revision without a checkout (`docc compile --rev`). Every read of the
//! project (docc.json, schemas, sources, docc.lock, images, the logo, code
//! files of trace.scan) goes through it, so a revision compiles exactly as a
//! checkout of it would.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::history::cat_files;

/// Source is where the files of the project come from.
#[derive(Clone, Default)]
pub enum Source {
    /// the working directory as it is
    #[default]
    Disk,
    /// the tree of a Git revision
    Rev(Arc<Rev>),
}

impl fmt::Debug for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Disk => f.write_str("Disk"),
            Source::Rev(r) => write!(f, "Rev({})", r.rev),
        }
    }
}

/// Entry is what a path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    File,
    Dir,
    /// a symbolic link or anything else that is not read
    Other,
}

impl Source {
    /// read returns the content of a file (an OS path below the project root).
    pub fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        match self {
            Source::Disk => std::fs::read(p),
            Source::Rev(r) => r.read(p),
        }
    }

    /// entry tells what p is, following symbolic links on the disk; None when
    /// nothing is there.
    pub fn entry(&self, p: &Path) -> Option<Entry> {
        match self {
            Source::Disk => std::fs::metadata(p).ok().map(|m| {
                if m.is_dir() {
                    Entry::Dir
                } else if m.is_file() {
                    Entry::File
                } else {
                    Entry::Other
                }
            }),
            Source::Rev(r) => r.entry(p),
        }
    }

    /// len is the size of a file, None when it is not there.
    pub fn len(&self, p: &Path) -> Option<u64> {
        match self {
            Source::Disk => std::fs::metadata(p).ok().map(|m| m.len()),
            Source::Rev(r) => r.blob(p).map(|b| b.size),
        }
    }
}

/// Blob is one file of the revision.
#[derive(Debug, Clone)]
pub struct Blob {
    oid: String,
    pub size: u64,
    /// a symbolic link: never read as a file
    pub link: bool,
}

/// Rev is the file tree of one revision below the project root, read with
/// `git ls-tree`; contents come from `git cat-file --batch` on demand, many
/// at once where the caller knows what it needs (prefetch).
pub struct Rev {
    rev: String,
    /// absolute project root; files are keyed relative to it
    root: PathBuf,
    /// where git runs: the root, or its nearest existing parent (a bare
    /// mirror has no checkout to step into)
    dir: PathBuf,
    files: BTreeMap<String, Blob>,
    cache: Mutex<HashMap<String, Vec<u8>>>,
}

/// git runs git in dir and returns its standard output, or the first line of
/// its error output.
fn git(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("cannot run git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(err.lines().next().unwrap_or("git failed").trim().to_string());
    }
    Ok(out.stdout)
}

/// slash is a relative path with '/' separators, None when it leaves the base.
fn slash(p: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()?.to_string()),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

impl Rev {
    /// open lists the files of rev below root. rev is a commit, branch, tag or
    /// any tree-ish Git understands.
    pub fn open(root: &Path, rev: &str) -> Result<Rev, String> {
        if rev.is_empty() || rev.starts_with('-') {
            return Err(format!("--rev {rev:?} is not a Git revision"));
        }
        let mut dir = root.to_path_buf();
        while !dir.is_dir() {
            if !dir.pop() {
                return Err(format!("{} does not exist", root.display()));
            }
        }
        let prefix = slash(root.strip_prefix(&dir).unwrap_or(Path::new(""))).unwrap_or_default();
        let tree = git(&dir, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{tree}}")])
            .map_err(|e| if e == "git failed" { format!("no such Git revision {rev:?}") } else { e })?;
        let tree = String::from_utf8_lossy(&tree).trim().to_string();
        // in a work tree git lists the tree below the current folder, relative to it;
        // in a bare repository there is no current folder and paths are full
        let out = git(&dir, &["-c", "core.quotePath=false", "ls-tree", "-r", "-z", "-l", &tree])?;
        let mut files = BTreeMap::new();
        for rec in out.split(|&b| b == 0) {
            // <mode> SP <type> SP <object> SP+ <size> TAB <path>
            let Some(tab) = rec.iter().position(|&b| b == b'\t') else {
                continue;
            };
            let (head, path) = (String::from_utf8_lossy(&rec[..tab]), &rec[tab + 1..]);
            let f: Vec<&str> = head.split_whitespace().collect();
            let Ok(path) = std::str::from_utf8(path) else {
                continue; // ponytail: a path that is not UTF-8 is dropped, as trace does
            };
            if f.len() < 4 || f[1] != "blob" {
                continue; // a submodule
            }
            let rel = if prefix.is_empty() {
                path
            } else {
                match path.strip_prefix(&prefix).and_then(|r| r.strip_prefix('/')) {
                    Some(r) => r,
                    None => continue,
                }
            };
            files.insert(
                rel.to_string(),
                Blob { oid: f[2].to_string(), size: f[3].parse().unwrap_or(0), link: f[0] == "120000" },
            );
        }
        Ok(Rev { rev: rev.to_string(), root: root.to_path_buf(), dir, files, cache: Mutex::new(HashMap::new()) })
    }

    /// rel is the project-relative '/' path of an OS path below the root.
    fn rel(&self, p: &Path) -> Option<String> {
        slash(p.strip_prefix(&self.root).ok()?)
    }

    fn blob(&self, p: &Path) -> Option<&Blob> {
        self.files.get(&self.rel(p)?)
    }

    fn entry(&self, p: &Path) -> Option<Entry> {
        let rel = self.rel(p)?;
        if let Some(b) = self.files.get(&rel) {
            return Some(if b.link { Entry::Other } else { Entry::File });
        }
        if rel.is_empty() {
            return Some(Entry::Dir);
        }
        let pre = rel + "/";
        self.files.range(pre.clone()..).next().filter(|(k, _)| k.starts_with(&pre)).map(|_| Entry::Dir)
    }

    /// files lists the files below the project-relative folder dir ("" for the
    /// root) as (path relative to dir, blob), in byte order of the path.
    pub fn files(&self, dir: &str) -> impl Iterator<Item = (&str, &Blob)> {
        let pre = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        self.files
            .range(pre.clone()..)
            .take_while(move |(k, _)| k.starts_with(&pre))
            .map(move |(k, b)| (&k[if dir.is_empty() { 0 } else { dir.len() + 1 }..], b))
    }

    /// prefetch reads the files at the given project-relative paths with one
    /// git process, so later reads come from memory.
    pub fn prefetch<'a>(&self, rels: impl IntoIterator<Item = &'a str>) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        let want: Vec<(&str, &Blob)> = rels
            .into_iter()
            .filter(|r| !cache.contains_key(*r))
            .filter_map(|r| self.files.get(r).filter(|b| !b.link).map(|b| (r, b)))
            .collect();
        if want.is_empty() {
            return;
        }
        let req: Vec<u8> = want.iter().flat_map(|(_, b)| format!("{}\n", b.oid).into_bytes()).collect();
        if let Ok(blobs) = cat_files(&self.dir, req) {
            for ((r, _), data) in want.iter().zip(blobs) {
                if let Some(data) = data {
                    cache.insert(r.to_string(), data);
                }
            }
        }
    }

    fn read(&self, p: &Path) -> io::Result<Vec<u8>> {
        let missing = || io::Error::new(io::ErrorKind::NotFound, "no such file in the revision");
        let rel = self.rel(p).ok_or_else(missing)?;
        match self.files.get(&rel) {
            Some(b) if !b.link => {}
            _ => return Err(missing()),
        }
        self.prefetch([rel.as_str()]);
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.get(&rel).cloned().ok_or_else(|| io::Error::other("git cat-file failed"))
    }
}
