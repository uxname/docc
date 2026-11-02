//! docc watch: the real binary builds, serves the page and reloads it.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::*;

/// Watch is a running docc watch, killed when the test ends (also when it fails).
struct Watch {
    child: Child,
    port: u16,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(p: &Dir) -> Watch {
    let mut child = Command::new(env!("CARGO_BIN_EXE_docc"))
        .arg("watch")
        .current_dir(&p.path)
        .env("DOCC_CODE_SEED", "test")
        .env("DOCC_WATCH_PORT", "0") // any free port: the tests run in parallel
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let port = line.split("http://127.0.0.1:").nth(1).map(|t| t.chars().take_while(char::is_ascii_digit).collect::<String>()).and_then(|t| t.parse().ok());
    // the rest of its output is read away, so the pipe never fills
    std::thread::spawn(move || {
        for l in out.lines().map_while(Result::ok) {
            eprintln!("watch: {l}");
        }
    });
    Watch { child, port: port.unwrap_or_else(|| panic!("no address in {line:?}")) }
}

/// get is one request; the answer is the status line and everything after it.
fn get(port: u16, path: &str, host: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

/// page waits (at most 20 s) for the page to hold text.
fn page_with(w: &Watch, text: &str) -> String {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let page = get(w.port, "/", &format!("127.0.0.1:{}", w.port));
        if page.contains(text) {
            return page;
        }
        assert!(Instant::now() < end, "the page never held {text:?}: {}", &page[..page.len().min(300)]);
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn index(body: &str) -> String {
    format!(r#"{{"title":"Index","kind":"page","key":"IND","body":[{body}]}}"#)
}

#[test]
fn builds_serves_and_reloads() {
    let p = project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#.to_string()),
        ("docs/src/index.json", index(r#""First text. ^IND-1""#)),
    ]);
    let w = start(&p);
    let host = format!("127.0.0.1:{}", w.port);

    // the page is the built HTML, with the version of docc and the reload script
    let page = page_with(&w, "First text.");
    assert!(page.starts_with("HTTP/1.1 200") && page.contains("Cache-Control: no-store"), "{page}");
    assert!(page.contains(r#""version":"dev""#) && page.contains("/__docc/wait"), "version or script missing");
    assert!(p.join("docs/out.html").exists() && p.join("docs/out/index.md").exists());
    // the script goes only into what is served
    assert!(!p.read("docs/out.html").contains("/__docc/wait"));

    // only the page and the channel: no file by a path, no other site's name, no POST
    assert!(get(w.port, "/docs/src/index.json", &host).starts_with("HTTP/1.1 404"));
    assert!(get(w.port, "/../docc.json", &host).starts_with("HTTP/1.1 404"));
    assert!(get(w.port, "/", "evil.example").starts_with("HTTP/1.1 403"), "a foreign Host must be refused");
    assert!(get(w.port, "/", &format!("localhost:{}", w.port)).starts_with("HTTP/1.1 200"));

    // the page asks for news with the version it holds; a save answers it with a new one
    let v = page.split("var v=").nth(1).and_then(|t| t.split(';').next()).unwrap().to_string();
    let port = w.port;
    let waiter = std::thread::spawn({
        let v = v.clone();
        move || get(port, &format!("/__docc/wait?v={v}"), &format!("127.0.0.1:{port}"))
    });
    std::thread::sleep(Duration::from_millis(300));
    // a new block without a code: the build gives it one (or it would stop at DOC212)
    p.write("docs/src/index.json", index(r#""First text. ^IND-1","Second text.""#));
    let news = waiter.join().unwrap();
    let new_v = news.rsplit("\r\n\r\n").next().unwrap().trim().to_string();
    assert!(news.starts_with("HTTP/1.1 200") && new_v != v, "no news after a save: {news}");
    page_with(&w, "Second text.");
    assert!(p.read("docs/src/index.json").contains("Second text. ^IND-2"), "{}", p.read("docs/src/index.json"));

    // a broken file keeps the page as it was, and the watching goes on: the repaired file shows
    p.write("docs/src/index.json", "{ not json");
    std::thread::sleep(Duration::from_millis(3500));
    assert!(get(w.port, "/", &host).contains("Second text."));
    p.write("docs/src/index.json", index(r#""First text. ^IND-1","Second text. ^IND-2","Third text. ^IND-3""#));
    page_with(&w, "Third text.");
}
