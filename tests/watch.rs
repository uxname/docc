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

/// workspace_with waits (at most 20 s) for /workspace.json to hold text.
fn workspace_with(w: &Watch, text: &str) -> String {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let ws = get(w.port, "/workspace.json", &format!("127.0.0.1:{}", w.port));
        if ws.contains(text) {
            return ws;
        }
        assert!(Instant::now() < end, "the workspace never held {text:?}: {}", &ws[..ws.len().min(300)]);
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn index(body: &str) -> String {
    format!(r#"{{"title":"Index","kind":"page","key":"IND","body":[{body}]}}"#)
}

/// Events is an open /events stream.
struct Events(BufReader<TcpStream>);

impl Events {
    fn open(port: u16) -> Events {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        write!(s, "GET /events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
        Events(BufReader::new(s))
    }

    /// next reads up to the next `event: workspace` and returns its data line.
    fn next(&mut self) -> String {
        let mut line = String::new();
        loop {
            line.clear();
            assert!(self.0.read_line(&mut line).unwrap() > 0, "the event stream ended");
            if line.trim() == "event: workspace" {
                line.clear();
                self.0.read_line(&mut line).unwrap();
                return line.trim().to_string();
            }
        }
    }
}

#[test]
fn builds_serves_and_sends_events() {
    let p = project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#.to_string()),
        ("docs/src/index.json", index(r#""First text. ^IND-1""#)),
        ("docc.json", r#"{"workspace":{"run":"docs/.crew/run.json","roles":{"tasks":"task"}}}"#.to_string()),
    ]);
    let w = start(&p);
    let host = format!("127.0.0.1:{}", w.port);

    // the object is the workspace of docc workspace; the page is the reader, which fetches it
    let ws = workspace_with(&w, "First text.");
    assert!(ws.starts_with("HTTP/1.1 200") && ws.contains("Cache-Control: no-store") && ws.contains("application/json"), "{ws}");
    assert!(
        ws.contains(r#""$schema":"urn:docc:schema:workspace:1""#)
            && ws.contains(r#""roles":{"tasks":"task"}"#)
            && ws.contains(r#""run":null"#),
        "{ws}"
    );
    let page = get(w.port, "/", &host);
    assert!(page.starts_with("HTTP/1.1 200") && page.contains(r#"id="docc-reader""#) && !page.contains(r#"id="docc-workspace""#), "{page}");
    assert!(p.join("docs/out/index.md").exists());

    // only the page, the object and the events: no file by a path, no other site's name
    assert!(get(w.port, "/docs/src/index.json", &host).starts_with("HTTP/1.1 404"));
    assert!(get(w.port, "/../docc.json", &host).starts_with("HTTP/1.1 404"));
    assert!(get(w.port, "/workspace.json", "evil.example").starts_with("HTTP/1.1 403"), "a foreign Host must be refused");
    assert!(get(w.port, "/", &format!("localhost:{}", w.port)).starts_with("HTTP/1.1 200"));

    // the stream sends the time of the object at once, and again after a save
    let mut ev = Events::open(w.port);
    let first = ev.next();
    assert!(first.starts_with(r#"data: {"generatedAt":"20"#), "{first}");
    // a new block without a code: the build gives it one
    p.write("docs/src/index.json", index(r#""First text. ^IND-1","Second text.""#));
    let second = ev.next();
    assert!(second.starts_with("data: "), "{second}");
    workspace_with(&w, "Second text.");
    assert!(p.read("docs/src/index.json").contains("Second text. ^IND-2"), "{}", p.read("docs/src/index.json"));

    // the run file alone: an event and the new run, without a build of the documents
    let out_before = std::fs::metadata(p.join("docs/out/index.md")).unwrap().modified().unwrap();
    p.write("docs/.crew/run.json", r#"{"state":"running","text":"Two helpers read the draft"}"#);
    ev.next();
    workspace_with(&w, "Two helpers read the draft");
    assert_eq!(
        std::fs::metadata(p.join("docs/out/index.md")).unwrap().modified().unwrap(),
        out_before,
        "a change of the run file alone builds nothing"
    );

    // a broken file keeps the pages as they were, and the watching goes on: the repaired file shows
    p.write("docs/src/index.json", "{ not json");
    std::thread::sleep(Duration::from_millis(3500));
    assert!(get(w.port, "/workspace.json", &host).contains("Second text."));
    p.write("docs/src/index.json", index(r#""First text. ^IND-1","Second text. ^IND-2","Third text. ^IND-3""#));
    workspace_with(&w, "Third text.");
}

/// post sends one POST with a JSON body (or another type).
fn post(port: u16, path: &str, kind: &str, body: &str, origin: Option<&str>) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(40))).unwrap();
    let origin = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
    write!(
        s,
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{origin}Content-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn answers_a_question_and_shows_notes() {
    let p = project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#.to_string()),
        ("docs/schemas/question.json", r#"{"type":"object","additionalProperties":true}"#.to_string()),
        ("docs/src/index.json", index(r#""Text. ^IND-1""#)),
        ("docs/src/q1.json", r#"{"title":"Which?","kind":"question","key":"QA","hidden":true,"body":["Why we ask. ^QA-1"]}"#.to_string()),
        ("docs/inbox/notes/2026-11-15-idea.md", "# Later: dark mode\n\nMaybe.\n".to_string()),
        ("docc.json", r#"{"workspace":{"roles":{"questions":["question"]}}}"#.to_string()),
    ]);
    let w = start(&p);
    let port = w.port;
    let own = format!("http://127.0.0.1:{port}");
    let ws = workspace_with(&w, "Why we ask.");
    assert!(ws.contains(r#""title":"Later: dark mode""#) && ws.contains(r#""path":"docs/inbox/notes/2026-11-15-idea.md""#), "{ws}");

    // refused: another site, not JSON, a document of another kind, an empty answer
    assert!(
        post(port, "/answer", "application/json", r#"{"doc":"q1","answer":"A"}"#, Some("http://evil.example")).starts_with("HTTP/1.1 403")
    );
    assert!(post(port, "/answer", "text/plain", r#"{"doc":"q1","answer":"A"}"#, None).starts_with("HTTP/1.1 415"));
    assert!(post(port, "/answer", "application/json", r#"{"doc":"index","answer":"A"}"#, None).starts_with("HTTP/1.1 404"));
    assert!(post(port, "/answer", "application/json", r#"{"doc":"q1","answer":"  "}"#, None).starts_with("HTTP/1.1 400"));
    assert!(get(port, "/answer", &format!("127.0.0.1:{port}")).starts_with("HTTP/1.1 404"));
    assert!(!p.read("docs/src/q1.json").contains("Ответ"));

    // the answer goes into «Ответ» only, and the build gives it a code
    let r = post(port, "/answer", "application/json", r#"{"doc":"q1","answer":"The second option."}"#, Some(&own));
    assert!(r.starts_with("HTTP/1.1 200"), "{r}");
    workspace_with(&w, "The second option.");
    let end = Instant::now() + Duration::from_secs(20);
    while !p.read("docs/src/q1.json").contains("The second option. ^QA-2") {
        assert!(Instant::now() < end, "{}", p.read("docs/src/q1.json"));
        std::thread::sleep(Duration::from_millis(200));
    }
    // answered again: the code stays
    assert!(post(port, "/answer", "application/json", r#"{"doc":"q1","answer":"The first."}"#, None).starts_with("HTTP/1.1 200"));
    assert!(p.read("docs/src/q1.json").contains(r#""Ответ": "The first. ^QA-2""#), "{}", p.read("docs/src/q1.json"));
}

/// GET /diff is what docc diff --format json prints; the revisions are
/// checked before git sees them, and the errors are JSON.
#[test]
fn serves_the_diff_of_two_revisions() {
    let p = project([
        ("docs/schemas/page.json", r#"{"type":"object","additionalProperties":true}"#.to_string()),
        ("docs/src/index.json", index(r#""First text. ^IND-1""#)),
    ]);
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(["-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main", "-c", "user.name=T", "-c", "user.email=t@x.test"])
            .args(args)
            .current_dir(&p.path)
            .output();
        let Ok(out) = out else { return None };
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    if git(&["init", "-q"]).is_none() {
        return; // no git here
    }
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "one"]);
    let first = git(&["rev-parse", "HEAD"]).unwrap();
    p.write("docs/src/index.json", index(r#""First text. ^IND-1","Second text. ^IND-2""#));
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "two"]);

    let w = start(&p);
    let host = format!("127.0.0.1:{}", w.port);
    let body = |r: &str| r.split_once("\r\n\r\n").map_or(String::new(), |(_, b)| b.to_string());

    let (code, want, _) = do_in(&p.path, &["diff", "--format", "json", &first, "main"]);
    assert_eq!(code, 0, "{want}");
    let r = get(w.port, &format!("/diff?base={first}&head=main"), &host);
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("application/json"), "{r}");
    assert_eq!(body(&r), want.trim_end(), "exactly what docc diff prints");
    assert!(want.contains("Second text."), "{want}");

    // refused before git: options, revision arithmetic, spaces, a missing head; an unknown ref by docc diff
    for q in ["base=-p&head=main", "base=--output%3D%2Ftmp%2Fx&head=main", "base=HEAD~1&head=main", "base=a+b&head=main", "base=main", "base=nope&head=main"] {
        let r = get(w.port, &format!("/diff?{q}"), &host);
        assert!(r.starts_with("HTTP/1.1 400") && body(&r).starts_with(r#"{"error":""#), "{q}: {r}");
    }
    assert!(get(w.port, &format!("/diff?base={first}&head=main"), "evil.example").starts_with("HTTP/1.1 403"));
}
