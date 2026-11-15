//! docc watch: builds again whenever a source is saved and serves the reader
//! on 127.0.0.1: GET / the reader page, GET /workspace.json the workspace
//! object (docc workspace), GET /events Server-Sent Events: `event: workspace`
//! after every change, and the reader fetches the object again. A change of
//! only the run file of the workspace section sends the event without a build.
//! It is made for reading the documents while an agent writes them.
//!
//! The server is std only: one thread per connection, and it hands out nothing
//! but the page, the object and the event stream (no file is read by a path of
//! the request). GET /diff?base=<rev>&head=<rev> is what `docc diff <base>
//! <head> --format json` prints (it runs this binary), for 127.0.0.1 only and
//! with both revisions checked before git sees them. It writes exactly one
//! thing: POST /answer sets the field
//! «Ответ» of a document of a questions kind (the person answers in the
//! reader); the save starts the build like any other.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::workspace::{self, Parts};
use super::{BuildOpts, Common, Error, load_config, new_flags, no_args, parse, run_build, say, usage_err};
use crate::render;

/// POLL is how often the sources are looked at; QUIET how long they must stay
/// unchanged before a build starts (an agent saves several files in a row);
/// MAX_WAIT how long a steady stream of saves may hold a build back.
const POLL: Duration = Duration::from_millis(250);
const QUIET: Duration = Duration::from_millis(1000);
const MAX_WAIT: Duration = Duration::from_secs(3);
/// how often an idle event stream gets a comment, so proxies and the browser keep it open
const HOLD: Duration = Duration::from_secs(25);
const MAX_CONNECTIONS: usize = 64;

/// WAITING is the workspace before the first build: no pages yet.
const WAITING: &str = r#"{"$schema":"urn:docc:schema:workspace:1","compile":{"$schema":"urn:docc:schema:compile:3","ok":false,"summary":{"checked":0,"errors":0,"warnings":0,"shown":0,"truncated":false},"diagnostics":[],"project":{"title":"","lang":"en","brand":null},"tree":[],"pages":[]},"roles":{},"coverage":[],"history":[],"notes":[],"run":null,"generatedAt":""}"#;

/// Shared is the workspace object the server hands out and the number of its version.
struct Shared {
    ws: Mutex<(String, u64)>,
    news: Condvar,
}

/// without_time is the object without its generatedAt, which changes every time.
fn without_time(ws: &str) -> &str {
    ws.rfind(r#","generatedAt":"#).map_or(ws, |i| &ws[..i])
}

impl Shared {
    /// set replaces the object; the version changes only when the object
    /// does (its time aside).
    fn set(&self, ws: String) -> bool {
        let mut g = self.ws.lock().unwrap_or_else(|e| e.into_inner());
        if without_time(&g.0) == without_time(&ws) {
            return false;
        }
        *g = (ws, g.1 + 1);
        self.news.notify_all();
        true
    }
}

pub(super) fn cmd_watch(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let (mut full, mut no_comments) = (false, false);
    let pos = {
        let mut fs = new_flags("watch", c);
        fs.bool_var(&mut full, "full", "audit build into outFull (the reader shows the reader's pages either way)");
        fs.bool_var(&mut no_comments, "no-comments", "leave the readers' comments ($comments) out of everything this build writes");
        parse(&mut fs, args)
    };
    no_args(&pos?)?;
    c.check()?;
    let cfg = load_config(c)?;

    let port = match std::env::var("DOCC_WATCH_PORT") {
        Ok(v) => v.parse::<u16>().map_err(|_| usage_err("DOCC_WATCH_PORT must be a port number"))?,
        Err(_) => port_for(&cfg.root),
    };
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| {
        usage_err(format!(
            "cannot listen on 127.0.0.1:{port}: {e}; is docc watch already running for this project (or for another one that got the same port)?"
        ))
    })?;
    let port = listener.local_addr()?.port();
    let shared = Arc::new(Shared { ws: Mutex::new((WAITING.to_string(), 0)), news: Condvar::new() });
    {
        let (shared, page) = (shared.clone(), render::reader_page("docc", &cfg.lang, ""));
        // ponytail: the source folder and onAnswer of the start; a save of docc.json that changes them needs a restart for answers
        let src = Spot { src: cfg.abs(&cfg.src), root: cfg.root.clone(), on_answer: cfg.workspace.on_answer.clone() };
        let cfg_args: Vec<String> = if c.config_path.is_empty() { vec![] } else { vec!["--config".into(), c.config_path.clone()] };
        std::thread::spawn(move || serve(listener, port, shared, page, src, cfg_args));
    }
    say(stdout, &format!("docc: watching http://127.0.0.1:{port} (stop with Ctrl+C)\n"));
    let _ = stdout.flush();

    let (mut watched, mut run) = watched_of(&cfg);
    let mut parts: Option<Parts> = None;
    loop {
        // read again: a save of docc.json may move the sources (a broken one changes nothing)
        if let Ok(cfg) = load_config(c) {
            (watched, run) = watched_of(&cfg);
        }
        // taken before the build: a save during it differs from this and builds again
        let before = snapshot(&watched);
        let mut run_seen = snapshot(&run);
        let opts = BuildOpts { no_comments, ..Default::default() };
        if let Err(e) = run_build(c, full, false, false, "", "", opts, stdout, stderr) {
            say(stderr, &format!("docc: error: {e}\n"));
        }
        match workspace::parts(c, !no_comments) {
            // the reader keeps the last pages; the reasons are in the terminal
            Ok(p) if p.broken && parts.is_some() => {}
            Ok(p) => {
                if shared.set(p.json()) {
                    say(stdout, "docc: page updated\n");
                }
                parts = Some(p);
            }
            // the reader keeps what it has; the next save tries again
            Err(e) => say(stderr, &format!("docc: error: {e}\n")),
        }
        let _ = stdout.flush();
        // ponytail: when a build gave codes, its own writes differ from `before` and one more
        // build follows, which writes nothing and changes no page; skipping it would lose a save made during the build
        let mut last = loop {
            std::thread::sleep(POLL);
            let now = snapshot(&watched);
            if now != before {
                break now;
            }
            let r = snapshot(&run);
            if r != run_seen {
                run_seen = r;
                if let Some(p) = &parts
                    && shared.set(p.json())
                {
                    say(stdout, "docc: run updated\n");
                    let _ = stdout.flush();
                }
            }
        };
        let first = Instant::now();
        while first.elapsed() < MAX_WAIT {
            std::thread::sleep(QUIET);
            let now = snapshot(&watched);
            if now == last {
                break;
            }
            last = now;
        }
    }
}

/// Watched are the places whose files start a build, and the files under
/// them that do not (the output, which the build itself writes).
struct Watched {
    paths: Vec<PathBuf>,
    skip: Vec<PathBuf>,
}

/// watched_of are the places whose files start a build (the sources, the
/// schemas, the config and the folders the code tags are read from) and the
/// run file, whose change alone only sends the workspace again.
/// ponytail: the logo of the branding section is not watched; add it when that matters.
fn watched_of(cfg: &crate::config::Config) -> (Watched, Watched) {
    let mut paths = vec![
        cfg.abs(&cfg.src),
        cfg.abs(&cfg.schemas),
        cfg.root.join(Path::new(&cfg.file).file_name().unwrap_or_default()),
        cfg.root.join(workspace::NOTES),
    ];
    for g in &cfg.trace_masks {
        // the part of the mask before its first wildcard: a folder or one file
        let fixed = &g[..g.find(['*', '?', '[']).unwrap_or(g.len())];
        let fixed = if fixed.len() < g.len() { fixed.rsplit_once('/').map_or("", |(d, _)| d) } else { fixed };
        paths.push(cfg.abs(fixed));
    }
    // a prefix of a path, not a folder: docs/out also skips docs/out.html and docs/out-full
    let mut skip: Vec<PathBuf> = [&cfg.out, &cfg.out_full].iter().map(|o| cfg.abs(o)).collect();
    let run: Vec<PathBuf> = workspace::run_file(cfg).into_iter().collect();
    skip.extend(run.iter().cloned());
    (Watched { paths, skip }, Watched { paths: run, skip: Vec::new() })
}

/// port_for is the port of a project: the same every time, so the readers'
/// comments (kept by the browser per address) stay, and other projects get others.
fn port_for(root: &Path) -> u16 {
    let mut h: u32 = 2166136261; // FNV-1a
    for b in root.as_os_str().as_encoded_bytes() {
        h = (h ^ u32::from(*b)).wrapping_mul(16777619);
    }
    20000 + (h % 10000) as u16
}

type Snapshot = BTreeMap<PathBuf, (SystemTime, u64)>;

/// snapshot is the modification time and size of every file under the
/// watched paths; folders whose name starts with '.', node_modules and target are left out.
fn snapshot(w: &Watched) -> Snapshot {
    fn walk(p: &Path, skip: &[PathBuf], top: bool, out: &mut Snapshot) {
        let s = p.as_os_str().as_encoded_bytes();
        if skip.iter().any(|k| s.starts_with(k.as_os_str().as_encoded_bytes())) {
            return;
        }
        let Ok(m) = std::fs::symlink_metadata(p) else {
            return;
        };
        if m.is_dir() {
            let name = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
            if !top && (name.starts_with('.') || name == "node_modules" || name == "target") {
                return;
            }
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    walk(&e.path(), skip, false, out);
                }
            }
        } else if !p.to_string_lossy().ends_with(".docc-tmp") {
            // a symlink to a file is followed: its target is what changes
            let m = std::fs::metadata(p).unwrap_or(m);
            out.insert(p.to_path_buf(), (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
        }
    }
    let mut out = Snapshot::new();
    for p in &w.paths {
        walk(p, &w.skip, true, &mut out);
    }
    out
}

/// Spot is where an answer goes: the source folder, the project root (git
/// and the onAnswer command run there) and the command of workspace.onAnswer.
struct Spot {
    src: PathBuf,
    root: PathBuf,
    on_answer: String,
}

fn serve(listener: TcpListener, port: u16, shared: Arc<Shared>, page: String, src: Spot, cfg_args: Vec<String>) {
    let (open, page, src, cfg_args) = (Arc::new(AtomicUsize::new(0)), Arc::new(page), Arc::new(src), Arc::new(cfg_args));
    for conn in listener.incoming() {
        let Ok(conn) = conn else {
            std::thread::sleep(Duration::from_millis(100)); // out of descriptors, say: do not spin
            continue;
        };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            continue; // dropped: the browser asks again
        }
        let (shared, open, page, src, cfg_args) = (shared.clone(), open.clone(), page.clone(), src.clone(), cfg_args.clone());
        std::thread::spawn(move || {
            let _ = handle(conn, port, &shared, &page, &src, &cfg_args);
            open.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

fn reply(s: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        s,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: frame-ancestors 'none'\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    s.write_all(body)
}

/// handle answers one request. The Host and Origin of the request must be this
/// server's own address: a page of another site that points its name at
/// 127.0.0.1 (DNS rebinding) must not be able to read the documents.
fn handle(mut s: TcpStream, port: u16, shared: &Shared, page: &str, src: &Spot, cfg_args: &[String]) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    // a request is a line and some headers (a browser sends the cookies of every
    // local server): more than 64 KiB, or more than 10 s to send it, is not one
    let began = Instant::now();
    let mut r = BufReader::new((&s).take(65536));
    let mut line = String::new();
    r.read_line(&mut line)?;
    let (method, target) = {
        let mut it = line.split_whitespace();
        (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string())
    };
    let (mut host, mut origin, mut length, mut json) = (String::new(), None, None, false);
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 || began.elapsed() > Duration::from_secs(10) {
            return reply(&mut s, "400 Bad Request", "text/plain", b"bad request");
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => host = v.trim().to_string(),
                "origin" => origin = Some(v.trim().to_string()),
                "content-length" => length = v.trim().parse::<usize>().ok(),
                "content-type" => json = v.trim().starts_with("application/json"),
                _ => {}
            }
        }
    }
    let own = |h: &str| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}");
    if !own(&host) || origin.as_deref().is_some_and(|o| !own(o.strip_prefix("http://").unwrap_or(o))) {
        return reply(&mut s, "403 Forbidden", "text/plain", b"forbidden");
    }
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let local = s.peer_addr().is_ok_and(|a| a.ip() == std::net::Ipv4Addr::LOCALHOST);
    if method == "GET" && path == "/diff" {
        if !local {
            return reply(&mut s, "403 Forbidden", "application/json", br#"{"error":"forbidden"}"#);
        }
        let (status, body) = diff(query, cfg_args);
        return reply(&mut s, status, "application/json; charset=utf-8", body.as_bytes());
    }
    if method == "POST" && path == "/answer" {
        // only this machine; a JSON body, which a page of another site cannot send without asking first
        if !local {
            return reply(&mut s, "403 Forbidden", "text/plain", b"forbidden");
        }
        if !json {
            return reply(&mut s, "415 Unsupported Media Type", "text/plain", b"application/json only");
        }
        let Some(n) = length.filter(|&n| n <= ANSWER_MAX) else {
            return reply(&mut s, "413 Payload Too Large", "text/plain", b"a body of at most 16 KiB with Content-Length");
        };
        let mut body = vec![0; n];
        r.read_exact(&mut body)?;
        let ws = shared.ws.lock().unwrap_or_else(|e| e.into_inner()).0.clone();
        return match answer(&ws, src, &body) {
            Ok((committed, handed_over)) => {
                let out = format!(r#"{{"ok":true,"committed":{committed},"handedOver":{handed_over}}}"#);
                reply(&mut s, "200 OK", "application/json; charset=utf-8", out.as_bytes())
            }
            Err((status, why)) => reply(&mut s, status, "text/plain; charset=utf-8", why.as_bytes()),
        };
    }
    if method != "GET" {
        return reply(&mut s, "405 Method Not Allowed", "text/plain", b"GET only, and POST /answer");
    }
    match path {
        "/" => reply(&mut s, "200 OK", "text/html; charset=utf-8", page.as_bytes()),
        "/workspace.json" => {
            let ws = shared.ws.lock().unwrap_or_else(|e| e.into_inner()).0.clone();
            reply(&mut s, "200 OK", "application/json; charset=utf-8", ws.as_bytes())
        }
        "/events" => events(s, shared),
        _ => reply(&mut s, "404 Not Found", "text/plain", b"not found"),
    }
}

/// diff answers GET /diff: the output of `docc diff <base> <head> --format
/// json` run by this binary in the folder and with the config of the watch.
/// Both revisions must pass is_rev first. A usage error of docc diff (an
/// unknown revision, say) is 400, any other failure 500; errors are
/// `{"error": text}`.
fn diff(query: &str, cfg_args: &[String]) -> (&'static str, String) {
    let err = |status, msg: &str| (status, serde_json::json!({ "error": msg }).to_string());
    let (mut base, mut head) = (None, None);
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        let Some(v) = unescape(v) else { return err("400 Bad Request", "bad percent-encoding") };
        match k {
            "base" => base = Some(v),
            "head" => head = Some(v),
            _ => {}
        }
    }
    // ponytail: docc diff compares two revisions only; head = the working tree waits for docc diff to support it
    let (Some(base), Some(head)) = (base, head) else {
        return err("400 Bad Request", "base and head are both required (docc diff has no working-tree side)");
    };
    for r in [&base, &head] {
        if !is_rev(r) {
            return err("400 Bad Request", &format!("not a commit hash or ref name: {r:?}"));
        }
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return err("500 Internal Server Error", &format!("cannot find docc: {e}")),
    };
    let out = match std::process::Command::new(exe)
        .args(["diff", "--format", "json"])
        .args(cfg_args)
        .args([&base, &head])
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(o) => o,
        Err(e) => return err("500 Internal Server Error", &format!("cannot run docc diff: {e}")),
    };
    let text = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    if out.status.success() {
        return ("200 OK", text);
    }
    // a failed run prints the error report of --format json: its first diagnostic says why
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let d = &v["diagnostics"][0];
    let msg = d["message"].as_str().map_or_else(|| String::from_utf8_lossy(&out.stderr).trim().to_string(), str::to_string);
    let status = if d["code"] == "usage" { "400 Bad Request" } else { "500 Internal Server Error" };
    err(status, &msg)
}

/// unescape decodes the percent-escapes of a query value ("+" is a space);
/// None when one is broken or the bytes are not UTF-8.
fn unescape(v: &str) -> Option<String> {
    let b = v.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(b.len()), 0);
    while i < b.len() {
        match b[i] {
            b'%' => {
                out.push(u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// is_rev is true for a commit hash (4 to 64 hex digits) or a ref name by the
/// rules of git check-ref-format (HEAD, main, origin/main, v1.0): nothing that
/// starts with "-" (an option of git), no spaces or control characters, none
/// of ~^:?*[\, no "..", "@{", "//", no part that starts with "." or ends
/// with ".lock". So HEAD~1 is refused: the reader sends hashes.
fn is_rev(r: &str) -> bool {
    if r.is_empty() || r.len() > 255 || r.starts_with('-') {
        return false;
    }
    if (4..=64).contains(&r.len()) && r.bytes().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    !(r == "@"
        || r.bytes().any(|c| c <= b' ' || c == 0x7f || b"~^:?*[\\".contains(&c))
        || r.contains("..")
        || r.contains("@{")
        || r.contains("//")
        || r.starts_with('/')
        || r.ends_with('/')
        || r.ends_with('.')
        || r.split('/').any(|part| part.starts_with('.') || part.ends_with(".lock")))
}

/// ANSWER_MAX is the largest body of POST /answer.
const ANSWER_MAX: usize = 16384;

/// ANSWER is the field docc watch writes the answer into.
const ANSWER: &str = "Ответ";

/// answer takes `{"doc": id, "answer": text}` and writes text into the field
/// «Ответ» of that document, when the workspace object ws lists it with a kind
/// of the questions role, with who answered (git user.name) and when
/// (answeredBy, answeredAt). The block code of the field stays; nothing else of
/// the file changes. The save starts the build. Then it commits that file
/// alone when nothing is staged (`Ответ человека: <title>`, trailer
/// `Crew-Actor: human`) and runs workspace.onAnswer with the id; it returns
/// whether it committed and whether the command succeeded.
fn answer(ws: &str, spot: &Spot, body: &[u8]) -> Result<(bool, bool), (&'static str, String)> {
    let src = spot.src.as_path();
    let bad = |why: &str| ("400 Bad Request", why.to_string());
    let req: serde_json::Value = serde_json::from_slice(body).map_err(|e| bad(&format!("not JSON: {e}")))?;
    let (Some(id), Some(text)) = (req["doc"].as_str(), req["answer"].as_str()) else {
        return Err(bad(r#"expected {"doc": "<id>", "answer": "<text>"}"#));
    };
    let text = crate::codes::strip_code(text.trim()).trim_end();
    if text.is_empty() {
        return Err(bad("the answer is empty"));
    }
    let ws: serde_json::Value = serde_json::from_str(ws).map_err(|e| ("500 Internal Server Error", e.to_string()))?;
    let kinds: Vec<&str> = match &ws["roles"]["questions"] {
        serde_json::Value::String(k) => vec![k.as_str()],
        serde_json::Value::Array(ks) => ks.iter().filter_map(|k| k.as_str()).collect(),
        _ => Vec::new(),
    };
    let page = ws["compile"]["pages"].as_array().into_iter().flatten().find(|p| p["id"] == id);
    let Some(page) = page.filter(|p| p["kind"].as_str().is_some_and(|k| kinds.contains(&k))) else {
        return Err(("404 Not Found", format!("no question document {id:?} (a document of a kind of workspace.roles.questions)")));
    };
    let rel = page["path"].as_str().unwrap_or("");
    if rel.is_empty() || Path::new(rel).components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
        return Err(("500 Internal Server Error", format!("bad path {rel:?}")));
    }
    let file = std::fs::canonicalize(src.join(rel)).map_err(|e| ("404 Not Found", e.to_string()))?;
    let data = std::fs::read(&file).map_err(|e| ("500 Internal Server Error", e.to_string()))?;
    let mut out = set_field(&data, ANSWER, text).map_err(|e| ("409 Conflict", e))?;
    let by = git(&spot.root, &["config", "user.name"]).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    if !by.is_empty() {
        out = set_field(&out, "answeredBy", &by).map_err(|e| ("409 Conflict", e))?;
    }
    out = set_field(&out, "answeredAt", &crate::comments::now()).map_err(|e| ("409 Conflict", e))?;
    let mut tmp = file.clone().into_os_string();
    tmp.push(".docc-tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &out).and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        ("500 Internal Server Error", e.to_string())
    })?;
    let title = page["title"].as_str().unwrap_or(id);
    let committed = git(&spot.root, &["diff", "--cached", "--quiet"]).is_some()
        && git(&spot.root, &["add", "--", &file.to_string_lossy()]).is_some()
        && git(&spot.root, &["commit", "-q", "-m", &format!("Ответ человека: {title}"), "-m", "Crew-Actor: human", "--", &file.to_string_lossy()]).is_some();
    Ok((committed, hand_over(&spot.on_answer, id, &spot.root)))
}

/// git runs git in the project root; None when it cannot run or fails.
fn git(root: &Path, args: &[&str]) -> Option<std::process::Output> {
    let out = std::process::Command::new("git").arg("-C").arg(root).args(args).stdin(std::process::Stdio::null()).output().ok()?;
    out.status.success().then_some(out)
}

/// hand_over runs the command cmd (workspace.onAnswer) with the document id
/// as its one more argument, without a shell, in the project root, for at
/// most 30 s; its output is dropped. It reports whether it ran and succeeded.
fn hand_over(cmd: &str, id: &str, root: &Path) -> bool {
    let Some(mut words) = split_words(cmd) else { return false };
    if words.is_empty() {
        return false;
    }
    // the command crew writes is "python3 ~/.claude/...": the shell would expand ~/, so it is done here
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        for w in &mut words {
            if let Some(rest) = w.strip_prefix("~/") {
                *w = Path::new(&home).join(rest).to_string_lossy().into_owned();
            }
        }
    }
    let null = std::process::Stdio::null;
    let Ok(mut child) = std::process::Command::new(&words[0]).args(&words[1..]).arg(id).current_dir(root).stdin(null()).stdout(null()).stderr(null()).spawn() else {
        return false;
    };
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// split_words splits a command line like a POSIX shell does (the shell-words
/// rules): blanks separate words, '...' keeps everything, "..." keeps all but
/// \" \\ \$ \` and a backslash keeps the next sign. None for an open quote.
fn split_words(s: &str) -> Option<Vec<String>> {
    let (mut words, mut cur, mut any) = (Vec::new(), String::new(), false);
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            ' ' | '\t' | '\n' => {
                if any {
                    words.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            '\'' => {
                any = true;
                loop {
                    match it.next()? {
                        '\'' => break,
                        c => cur.push(c),
                    }
                }
            }
            '"' => {
                any = true;
                loop {
                    match it.next()? {
                        '"' => break,
                        '\\' => match it.next()? {
                            c @ ('"' | '\\' | '$' | '`') => cur.push(c),
                            '\n' => {}
                            c => {
                                cur.push('\\');
                                cur.push(c);
                            }
                        },
                        c => cur.push(c),
                    }
                }
            }
            '\\' => {
                any = true;
                match it.next() {
                    Some('\n') | None => {}
                    Some(c) => cur.push(c),
                }
            }
            c => {
                any = true;
                cur.push(c);
            }
        }
    }
    if any {
        words.push(cur);
    }
    Some(words)
}

/// set_field is the JSON text of a document with the top-level field key set
/// to text: its value replaced (a block code at the end of a string stays), or a
/// new member after the last one, indented like the file.
fn set_field(src: &[u8], key: &str, text: &str) -> Result<Vec<u8>, String> {
    let (bom, data) = match src.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (&src[..3], rest),
        None => (&src[..0], src),
    };
    let r = crate::loader::parse(data);
    let root = match (&r.err, &r.root) {
        (None, Some(root)) if root.kind == crate::loader::Kind::Object => root,
        _ => return Err("the file is not valid JSON with an object at the root".into()),
    };
    if r.dups.iter().any(|d| d.key == key) {
        return Err(format!("the file has {key:?} twice"));
    }
    let lit = |s: &str| serde_json::to_string(s).unwrap_or_default().replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029");
    let (start, end, new) = match root.members.iter().find(|m| m.key == key) {
        Some(m) => {
            let code = (m.value.kind == crate::loader::Kind::String).then(|| crate::codes::code_of(&m.value.str)).flatten();
            let v = code.map_or_else(|| text.to_string(), |c| crate::codes::with_code(text, c));
            (m.value.start, m.value.end, lit(&v))
        }
        None => {
            let Some(last) = root.members.last() else {
                return Ok([bom, b"{", format!("{}: {}", lit(key), lit(text)).as_bytes(), &data[root.start + 1..]].concat());
            };
            let first = &root.members[0];
            let lead = &data[root.start + 1..first.key_start];
            let sep = match lead.iter().rposition(|&c| c == b'\n') {
                Some(at) => format!(
                    ",{}{}",
                    if data.windows(2).any(|w| w == b"\r\n") { "\r\n" } else { "\n" },
                    String::from_utf8_lossy(&lead[at + 1..])
                ),
                None => ", ".into(),
            };
            (last.value.end, last.value.end, format!("{sep}{}: {}", lit(key), lit(text)))
        }
    };
    Ok([bom, &data[..start], new.as_bytes(), &data[end..]].concat())
}

/// events streams `event: workspace` with the time of the object every time
/// it changes, the first one at once; a comment line keeps an idle stream open.
fn events(mut s: TcpStream, shared: &Shared) -> std::io::Result<()> {
    s.set_read_timeout(None)?;
    write!(
        s,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: keep-alive\r\n\r\nretry: 1000\n\n"
    )?;
    let mut known = None;
    loop {
        let g = shared.ws.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = shared.news.wait_timeout_while(g, HOLD, |g| Some(g.1) == known).unwrap_or_else(|e| e.into_inner());
        if Some(g.1) == known {
            drop(g);
            s.write_all(b": idle\n\n")?;
            continue;
        }
        known = Some(g.1);
        let at = g.0.rsplit_once(r#""generatedAt":""#).map_or("", |(_, t)| t.trim_end_matches(['"', '}']));
        let msg = format!("event: workspace\ndata: {{\"generatedAt\":\"{at}\"}}\n\n");
        drop(g);
        s.write_all(msg.as_bytes())?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_is_stable_and_in_range() {
        let a = port_for(Path::new("/home/x/docc"));
        assert_eq!(a, port_for(Path::new("/home/x/docc")));
        assert!((20000..30000).contains(&a));
        assert_ne!(a, port_for(Path::new("/home/x/o2")));
    }

    #[test]
    fn the_version_moves_only_with_the_object() {
        let s = Shared { ws: Mutex::new((r#"{"a":1,"generatedAt":"t1"}"#.into(), 5)), news: Condvar::new() };
        assert!(!s.set(r#"{"a":1,"generatedAt":"t2"}"#.into()));
        assert!(s.set(r#"{"a":2,"generatedAt":"t3"}"#.into()));
        assert_eq!(s.ws.lock().unwrap().1, 6);
    }

    #[test]
    fn revisions_are_hashes_or_ref_names() {
        for ok in ["HEAD", "main", "origin/main", "v1.0", "feature/x-y_z", "1a2b", "4b825dc642cb6eb9a060e54bf8d69288fbee4904"] {
            assert!(is_rev(ok), "{ok}");
        }
        for bad in ["", "-p", "--output=/tmp/x", "a b", "HEAD~1", "HEAD^", "a..b", "a:b", "@", "x@{1}", "a//b", "/a", "a/", "a.", ".a", "a/.b", "x.lock", "a\\b", "a*", "a?", "a[", "a\tb", "a\nb"] {
            assert!(!is_rev(bad), "{bad}");
        }
        assert_eq!(unescape("origin%2Fmain").as_deref(), Some("origin/main"));
        assert_eq!(unescape("a+b").as_deref(), Some("a b"));
        assert_eq!(unescape("%zz"), None);
        assert_eq!(unescape("%4"), None);
    }

    #[test]
    fn set_field_writes_only_the_field() {
        let set_answer = |src: &[u8], t: &str| set_field(src, ANSWER, t);
        let got = |src: &str, t: &str| String::from_utf8(set_answer(src.as_bytes(), t).unwrap()).unwrap();
        assert_eq!(got("{\n  \"title\": \"Q?\"\n}\n", "Yes"), "{\n  \"title\": \"Q?\",\n  \"Ответ\": \"Yes\"\n}\n");
        assert_eq!(got(r#"{"title":"Q?","Ответ":"No ^QA-3","x":1}"#, "Yes"), r#"{"title":"Q?","Ответ":"Yes ^QA-3","x":1}"#);
        assert_eq!(got(r#"{"title":"Q?","Ответ":["a"]}"#, "Yes"), r#"{"title":"Q?","Ответ":"Yes"}"#);
        assert_eq!(got("{}", "Yes"), r#"{"Ответ": "Yes"}"#);
        assert!(set_answer(r#"{"Ответ":"a","Ответ":"b"}"#.as_bytes(), "x").is_err());
        assert!(set_answer(b"[1]", "x").is_err());
    }

    #[test]
    fn commands_split_like_a_shell() {
        let w = |s: &str| split_words(s).map(|v| v.join("|"));
        assert_eq!(w("python3 ~/.claude/crew.py answered").as_deref(), Some("python3|~/.claude/crew.py|answered"));
        assert_eq!(w(r#"  a 'b c' "d \"e\" \x" f\ g ''  "#).as_deref(), Some(r#"a|b c|d "e" \x|f g|"#));
        assert_eq!(w("").as_deref(), Some(""));
        assert_eq!(w("a 'b"), None);
        assert_eq!(w(r#"a "b"#), None);
    }
}
