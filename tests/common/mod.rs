//! Helpers of the CLI tests: every call runs the real binary as a subprocess
//! in the project directory, so the tests can run in parallel.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use docc::loader;

/// Dir is a temporary directory removed on drop (Go: t.TempDir()).
pub struct Dir {
    pub path: PathBuf,
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// temp_dir makes a new empty directory.
pub fn temp_dir() -> Dir {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("docc-test-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    // the canonical path: what the binary sees as its working directory
    let path = path.canonicalize().unwrap();
    Dir { path }
}

impl Dir {
    /// run is do(...) in the project root.
    pub fn run(&self, args: &[&str]) -> (i32, String, String) {
        do_in(&self.path, args)
    }

    /// run_in is do(...) in a folder of the project.
    pub fn run_in(&self, sub: &str, args: &[&str]) -> (i32, String, String) {
        do_in(&self.path.join(sub), args)
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.path.join(rel)
    }

    /// read is readFile: the file must exist.
    pub fn read(&self, rel: &str) -> String {
        let p = self.path.join(rel);
        String::from_utf8_lossy(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
            .into_owned()
    }

    /// write is write(t, name, content).
    pub fn write(&self, rel: &str, content: impl AsRef<[u8]>) {
        let p = self.path.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
    }

    pub fn exists(&self, rel: &str) -> bool {
        std::fs::metadata(self.path.join(rel)).is_ok()
    }

    /// must_build is mustBuild.
    pub fn must_build(&self) {
        let (code, out, errs) = self.run(&["build"]);
        assert_eq!(code, 0, "build: {code} {out} {errs}");
    }
}

/// Files are path -> content of a test project.
pub type Files = BTreeMap<String, Vec<u8>>;

/// files builds Files from pairs.
pub fn files<K: AsRef<str>, V: AsRef<[u8]>>(pairs: impl IntoIterator<Item = (K, V)>) -> Files {
    pairs.into_iter().map(|(k, v)| (k.as_ref().to_string(), v.as_ref().to_vec())).collect()
}

/// project creates a temp project from path -> content. Documents without a
/// kind get the kind "page" (see page_kind); raw_project keeps the files as given.
pub fn project<K: AsRef<str>, V: AsRef<[u8]>>(pairs: impl IntoIterator<Item = (K, V)>) -> Dir {
    let mut fs = files(pairs);
    page_kind(&mut fs);
    write_project(&fs)
}

pub fn raw_project<K: AsRef<str>, V: AsRef<[u8]>>(pairs: impl IntoIterator<Item = (K, V)>) -> Dir {
    write_project(&files(pairs))
}

fn write_project(fs: &Files) -> Dir {
    let dir = temp_dir();
    for (name, content) in fs {
        dir.write(name, content);
    }
    dir
}

/// page_kind gives every document under a src/ folder that parses and has no
/// kind the kind "page" (as the last member, so positions above it stay) and
/// adds an open docs/schemas/page.json when the project has none. Tests that
/// are not about kinds stay short this way.
pub fn page_kind(files: &mut Files) {
    let mut added = false;
    for (name, text) in files.iter_mut() {
        if !name.ends_with(".json") || !format!("/{name}").contains("/src/") {
            continue;
        }
        let r = loader::parse(text);
        let ok = r.err.is_none()
            && r.dups.is_empty()
            && r.root.as_ref().is_some_and(|n| n.kind == loader::Kind::Object && n.get("kind").is_none());
        if !ok {
            continue;
        }
        let i = text.iter().rposition(|&b| b == b'}').unwrap();
        let head_len = text[..i].iter().rposition(|b| !b" \t\r\n".contains(b)).map_or(0, |k| k + 1);
        let sep: &[u8] = if text[..head_len].ends_with(b"{") { b"" } else { b"," };
        let mut out = text[..head_len].to_vec();
        out.extend_from_slice(sep);
        out.extend_from_slice(br#""kind":"page""#);
        out.extend_from_slice(&text[head_len..]);
        *text = out;
        added = true;
    }
    if added && !files.contains_key("docs/schemas/page.json") {
        files.insert("docs/schemas/page.json".into(), br#"{"type":"object","additionalProperties":true}"#.to_vec());
    }
}

/// do_in runs docc with args in dir and returns the exit code, stdout and stderr.
pub fn do_in(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_docc"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// do_stdin is do_in with input on standard input (Go: the stdin variable).
pub fn do_stdin(dir: &Path, input: &[u8], args: &[&str]) -> (i32, String, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut ch = Command::new(env!("CARGO_BIN_EXE_docc"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // the command may exit before it reads its input (a flag it refuses): the
    // broken pipe is then its answer, not a failure of the test
    if let Err(e) = ch.stdin.take().unwrap().write_all(input) {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "{e}");
    }
    let out = ch.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// diagnostics_of returns the "diagnostics" array of a JSON report.
pub fn diagnostics_of(out: &str) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(out) {
        Ok(v) => v.get("diagnostics").cloned().unwrap_or(serde_json::Value::Null),
        Err(_) => serde_json::Value::String(out.to_string()),
    }
}

/// copy_tree copies the files of from into to.
pub fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let dst = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_tree(&e.path(), &dst)?;
        } else {
            std::fs::write(&dst, std::fs::read(e.path())?)?;
        }
    }
    Ok(())
}

/// repo is the path of the repository root.
pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// App is the HTML file split for tests: the shell (template, styles and
/// scripts), the data island, and the rendered HTML of every block and property.
pub struct App {
    pub shell: String,
    pub blocks: String,
    pub data: serde_json::Value,
}

impl App {
    pub fn pages(&self) -> &Vec<serde_json::Value> {
        self.data["pages"].as_array().unwrap()
    }
}

/// read_app is readApp. Go unmarshals into a struct with case-insensitive
/// field names; the island's keys are lower case.
pub fn read_app(dir: &Dir, rel: &str) -> App {
    let s = dir.read(rel);
    const OPEN: &str = r#"<script type="application/json" id="docc-data">"#;
    let a = s.find(OPEN).unwrap_or_else(|| panic!("{rel} has no data island")) + OPEN.len();
    let b = a + s[a..].find("</script>").unwrap();
    let data: serde_json::Value = serde_json::from_str(&s[a..b]).unwrap_or_else(|e| panic!("{rel}: data: {e}"));
    let mut blocks = String::new();
    for p in data["pages"].as_array().into_iter().flatten() {
        for pr in p["props"].as_array().into_iter().flatten() {
            blocks.push_str(pr["h"].as_str().unwrap_or(""));
            blocks.push('\n');
        }
        for bl in p["blocks"].as_array().into_iter().flatten() {
            blocks.push_str(bl["h"].as_str().unwrap_or(""));
            blocks.push('\n');
        }
    }
    App { shell: format!("{}{}", &s[..a], &s[b..]), blocks, data }
}

/// s is a JSON string field, "" when missing.
pub fn s<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
