//! docc watch: builds again whenever a source is saved and serves the HTML page
//! on 127.0.0.1; the page reloads itself when a build changes it. It is made for
//! reading the documents while an agent writes them.
//!
//! The server is std only: one thread per connection, GET only, and it hands out
//! nothing but the page and the channel the page waits on (no file is read by a
//! path of the request).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use super::{BuildOpts, Common, Error, load_config, new_flags, no_args, parse, run_build, say, usage_err};

/// POLL is how often the sources are looked at; QUIET how long they must stay
/// unchanged before a build starts (an agent saves several files in a row);
/// MAX_WAIT how long a steady stream of saves may hold a build back.
const POLL: Duration = Duration::from_millis(250);
const QUIET: Duration = Duration::from_millis(1000);
const MAX_WAIT: Duration = Duration::from_secs(3);
/// how long a page waits for news before it asks again
const HOLD: Duration = Duration::from_secs(25);
const MAX_CONNECTIONS: usize = 64;

const WAITING: &str = "<!doctype html><meta charset=\"utf-8\"><title>docc</title><body style=\"font:16px system-ui;margin:3rem\"><p>docc: waiting for a build without errors. The reasons are in the terminal where docc watch runs; this page opens the documents as soon as it can.</p></body>";

/// The script put before </body> of the page as it is served (never into the
/// file on disk). It asks for news; a new version means reload, but not while the
/// reader has typed text into the page (a comment, an answer): the words must not be lost.
const RELOAD: &str = "<script>(function(){var v=@V@;function busy(){return [].some.call(document.querySelectorAll('main textarea,main input[type=text],main input:not([type])'),function(e){return !!e.value.trim();});}function go(){if(busy()){setTimeout(go,1000);}else{location.reload();}}function wait(){fetch('/__docc/wait?v='+v,{cache:'no-store'}).then(function(r){return r.text();}).then(function(t){if(t.trim()===String(v)){wait();}else{go();}}).catch(function(){setTimeout(wait,1000);});}wait();})();</script>";

/// Shared is the page the server hands out and the number of its version.
struct Shared {
    page: Mutex<(Vec<u8>, u64)>,
    news: Condvar,
}

impl Shared {
    /// set replaces the page; the version changes only when the page does.
    fn set(&self, html: Vec<u8>) -> bool {
        let mut g = self.page.lock().unwrap_or_else(|e| e.into_inner());
        if g.0 == html {
            return false;
        }
        *g = (html, g.1 + 1);
        self.news.notify_all();
        true
    }
}

pub(super) fn cmd_watch(c: &mut Common, args: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> Result<i32, Error> {
    let (mut full, mut no_comments) = (false, false);
    let pos = {
        let mut fs = new_flags("watch", c);
        fs.bool_var(&mut full, "full", "audit build into outFull, and serve that page");
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
    let start = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(1, |d| d.as_millis() as u64);
    let shared = Arc::new(Shared { page: Mutex::new((WAITING.as_bytes().to_vec(), start)), news: Condvar::new() });
    {
        let shared = shared.clone();
        std::thread::spawn(move || serve(listener, port, shared));
    }
    say(stdout, &format!("docc: watching http://127.0.0.1:{port} (stop with Ctrl+C)\n"));
    let _ = stdout.flush();

    let mut watched = watched_of(&cfg);
    loop {
        // read again: a save of docc.json may move the sources (a broken one changes nothing)
        if let Ok(cfg) = load_config(c) {
            watched = watched_of(&cfg);
        }
        // taken before the build: a save during it differs from this and builds again
        let before = snapshot(&watched);
        let opts = BuildOpts { no_comments, ..Default::default() };
        match run_build(c, full, false, true, "", "", opts, stdout, stderr) {
            Ok((_, Some(file))) => {
                if let Ok(html) = std::fs::read(&file) {
                    if shared.set(html) {
                        say(stdout, "docc: page updated\n");
                    }
                }
            }
            Ok(_) => {}
            // the page stays as it was; the next save tries again
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

/// watched_of are the places whose files start a build: the sources, the schemas and the config.
/// ponytail: the logo of the branding section is not watched; add it when that matters.
fn watched_of(cfg: &crate::config::Config) -> [PathBuf; 3] {
    [cfg.abs(&cfg.src), cfg.abs(&cfg.schemas), cfg.root.join(Path::new(&cfg.file).file_name().unwrap_or_default())]
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

/// snapshot is the modification time and size of every file under paths.
fn snapshot(paths: &[PathBuf]) -> Snapshot {
    fn walk(p: &Path, out: &mut Snapshot) {
        let Ok(m) = std::fs::symlink_metadata(p) else { return };
        if m.is_dir() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    walk(&e.path(), out);
                }
            }
        } else if !p.to_string_lossy().ends_with(".docc-tmp") {
            // a symlink to a file is followed: its target is what changes
            let m = std::fs::metadata(p).unwrap_or(m);
            out.insert(p.to_path_buf(), (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
        }
    }
    let mut out = Snapshot::new();
    for p in paths {
        walk(p, &mut out);
    }
    out
}

fn serve(listener: TcpListener, port: u16, shared: Arc<Shared>) {
    let open = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(conn) = conn else {
            std::thread::sleep(Duration::from_millis(100)); // out of descriptors, say: do not spin
            continue;
        };
        if open.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            open.fetch_sub(1, Ordering::SeqCst);
            continue; // dropped: the browser asks again
        }
        let (shared, open) = (shared.clone(), open.clone());
        std::thread::spawn(move || {
            let _ = handle(conn, port, &shared);
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
fn handle(mut s: TcpStream, port: u16, shared: &Shared) -> std::io::Result<()> {
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
    let (mut host, mut origin) = (String::new(), None);
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
                _ => {}
            }
        }
    }
    let own = |h: &str| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}");
    if !own(&host) || origin.as_deref().is_some_and(|o| !own(o.strip_prefix("http://").unwrap_or(o))) {
        return reply(&mut s, "403 Forbidden", "text/plain", b"forbidden");
    }
    if method != "GET" {
        return reply(&mut s, "405 Method Not Allowed", "text/plain", b"GET only");
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    match path {
        "/" => {
            let (html, v) = shared.page.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let mut html = String::from_utf8_lossy(&html).into_owned();
            let script = RELOAD.replace("@V@", &v.to_string());
            let at = html.rfind("</body>").unwrap_or(html.len());
            html.insert_str(at, &script);
            reply(&mut s, "200 OK", "text/html; charset=utf-8", html.as_bytes())
        }
        "/__docc/wait" => {
            let known = query.strip_prefix("v=").and_then(|v| v.parse::<u64>().ok());
            let g = shared.page.lock().unwrap_or_else(|e| e.into_inner());
            let (g, _) = shared.news.wait_timeout_while(g, HOLD, |g| Some(g.1) == known).unwrap_or_else(|e| e.into_inner());
            let v = g.1;
            drop(g);
            reply(&mut s, "200 OK", "text/plain", v.to_string().as_bytes())
        }
        _ => reply(&mut s, "404 Not Found", "text/plain", b"not found"),
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
    fn the_page_version_moves_only_with_the_page() {
        let s = Shared { page: Mutex::new((b"a".to_vec(), 5)), news: Condvar::new() };
        assert!(!s.set(b"a".to_vec()));
        assert!(s.set(b"b".to_vec()));
        assert_eq!(s.page.lock().unwrap().1, 6);
    }
}
