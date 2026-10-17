//! docc update: replace the running docc with a release from GitHub.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use sha2::{Digest, Sha256};

use super::{Error, HelpError, VERSION, go_path_error, say, usage_err, write_mode};
use crate::config::go_quote;

/// RELEASES_URL is where the release archives and checksums.txt live; DOCC_BASE_URL overrides it
/// (a mirror, or a test server). install.sh and install.ps1 read the same variable.
const RELEASES_URL: &str = "https://github.com/uxname/docc/releases";

const UPDATE_USAGE: &str = "Usage: docc update [--check] [--version vX.Y.Z]

Downloads the release archive for this system, checks it against checksums.txt of the release
and replaces the running docc. Without --version it takes the latest release.
--check only tells whether a newer release exists (exit code 1: there is one).

Examples:
  docc update
  docc update --check
  docc update --version v0.2.0
";

pub(super) fn cmd_update(args: &[String], stdout: &mut dyn Write) -> Result<i32, Error> {
    let (mut check, mut want) = (false, String::new());
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--version" => {
                i += 1;
                if i == args.len() {
                    return Err(usage_err("--version needs a value, for example v0.2.0"));
                }
                want = args[i].clone();
            }
            "--help" | "-h" => return Err(Box::new(HelpError { text: UPDATE_USAGE.into() })),
            a => return Err(usage_err(format!("unexpected argument {} for docc update", go_quote(a)))),
        }
        i += 1;
    }
    let cur = VERSION;
    if cur == "dev" {
        return Err(usage_err(
            "this is a development build with no release version: build from source again or install a release",
        ));
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    let base = std::env::var("DOCC_BASE_URL").unwrap_or_default();
    let base = if base.is_empty() { RELEASES_URL.to_string() } else { base };
    update_from(&base, &exe, cur, &want, check, go_os(), go_arch(), stdout)
}

/// go_os is runtime.GOOS, the name the release archives use.
fn go_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        os => os,
    }
}

/// go_arch is runtime.GOARCH.
fn go_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        arch => arch,
    }
}

/// ARCHIVE_RE matches a line of checksums.txt: "<sha256>  docc_<version>_<os>_<arch>.<tar.gz|zip>".
/// (Go's \s and \S are ASCII.)
static ARCHIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^([0-9a-f]{64})[\t\n\x0C\r ]+\*?(docc_([^\t\n\x0C\r ]+)_([a-z0-9]+)_([a-z0-9]+)\.(?:tar\.gz|zip))$",
    )
    .unwrap()
});

#[allow(clippy::too_many_arguments)]
fn update_from(
    base: &str,
    exe: &Path,
    cur: &str,
    want: &str,
    check: bool,
    goos: &str,
    goarch: &str,
    stdout: &mut dyn Write,
) -> Result<i32, Error> {
    let mut list_url = format!("{base}/latest/download/checksums.txt");
    if !want.is_empty() {
        list_url = format!("{base}/download/{want}/checksums.txt");
    }
    let list = fetch(&list_url).map_err(|e| format!("cannot read the release list: {e}"))?;
    let (mut sum, mut name, mut latest) = (String::new(), String::new(), String::new());
    for line in String::from_utf8_lossy(&list).split('\n') {
        if let Some(m) = ARCHIVE_RE.captures(line.trim())
            && &m[4] == goos
            && &m[5] == goarch
        {
            (sum, name, latest) = (m[1].to_string(), m[2].to_string(), m[3].to_string());
        }
    }
    if name.is_empty() {
        return Err(format!("the release has no archive for {goos}/{goarch}").into());
    }
    if latest == cur {
        say(stdout, &format!("docc {cur} is up to date\n"));
        return Ok(0);
    }
    if check {
        say(stdout, &format!("docc {latest} is available (installed: {cur}); run docc update\n"));
        return Ok(1);
    }
    let data = fetch(&format!("{base}/download/{latest}/{name}")).map_err(|e| format!("download failed: {e}"))?;
    if hex::encode(Sha256::digest(&data)) != sum {
        return Err(format!("checksum of {name} does not match checksums.txt: nothing was changed").into());
    }
    let bin = extract_binary(&data, &name)?;
    if let Err(e) = replace_exe(exe, &bin) {
        return Err(format!(
            "cannot replace {}: {e} (is the folder writable? try the install script)",
            exe.display()
        )
        .into());
    }
    say(stdout, &format!("updated docc {cur} -> {latest}\n"));
    Ok(0)
}

fn fetch(url: &str) -> Result<Vec<u8>, Error> {
    // Go's http.Client: one 2-minute timeout for the whole exchange, redirects followed by the
    // client (at most 10), and the proxy chosen per request by http.ProxyFromEnvironment.
    // ureq picks one proxy for every URL, so redirects are followed here, one agent per hop.
    let deadline = Instant::now() + Duration::from_secs(120);
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let mut at = url.to_string();
    for hop in 0.. {
        let proxy = match url::Url::parse(&at) {
            Ok(u) => proxy_for_url(&u, &env)?,
            Err(_) => None, // ureq reports the bad URL
        };
        let proxy = match proxy {
            // Go's SOCKS5 dialer always sends the host name, as socks5h does
            Some(p) if p.scheme() == "socks5" => Some(ureq::Proxy::new(&p.as_str().replacen("socks5", "socks5h", 1))?),
            Some(p) => Some(ureq::Proxy::new(p.as_str())?),
            None => None,
        };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(deadline.saturating_duration_since(Instant::now())))
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(proxy)
            .tls_config(TLS.clone())
            .build()
            .into();
        let mut resp = agent.get(&at).call()?;
        let loc = resp.headers().get("location").map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
        if let (301 | 302 | 303 | 307 | 308, Some(loc)) = (resp.status().as_u16(), loc.filter(|l| !l.is_empty())) {
            if hop == 10 {
                return Err("stopped after 10 redirects".into());
            }
            at = url::Url::parse(&at)
                .and_then(|u| u.join(&loc))
                .map_err(|e| format!("failed to parse Location header {}: {e}", go_quote(&loc)))?
                .to_string();
            continue;
        }
        if resp.status() != 200 {
            return Err(format!("{url}: {}", resp.status()).into());
        }
        let mut b = Vec::new();
        resp.body_mut().as_reader().take(200 << 20).read_to_end(&mut b)?;
        return Ok(b);
    }
    unreachable!()
}

/// proxy_for_url is Go's http.ProxyFromEnvironment (golang.org/x/net/http/httpproxy): HTTPS_PROXY
/// for https URLs, HTTP_PROXY for http ones (upper case first, then lower case), never ALL_PROXY,
/// and no proxy for localhost, loopback addresses and hosts listed in NO_PROXY.
fn proxy_for_url(u: &url::Url, getenv: &dyn Fn(&str) -> String) -> Result<Option<url::Url>, Error> {
    let any = |names: [&str; 2]| names.iter().map(|n| getenv(n)).find(|v| !v.is_empty()).unwrap_or_default();
    let proxy = match u.scheme() {
        "https" => parse_proxy(&any(["HTTPS_PROXY", "https_proxy"])),
        "http" => {
            let p = parse_proxy(&any(["HTTP_PROXY", "http_proxy"]));
            if p.is_some() && !getenv("REQUEST_METHOD").is_empty() {
                return Err("refusing to use HTTP_PROXY value in CGI environment; see golang.org/s/cgihttpproxy".into());
            }
            p
        }
        _ => None,
    };
    let Some(proxy) = proxy else { return Ok(None) };
    if !use_proxy(u, &any(["NO_PROXY", "no_proxy"])) {
        return Ok(None);
    }
    Ok(Some(proxy))
}

/// parse_proxy is httpproxy's parseProxy: an address without a scheme means http://.
/// An address that does not parse means no proxy, as in Go.
fn parse_proxy(p: &str) -> Option<url::Url> {
    if p.is_empty() {
        return None;
    }
    match url::Url::parse(p) {
        Ok(u) if u.host_str().is_some_and(|h| !h.is_empty()) => Some(u),
        _ => url::Url::parse(&format!("http://{p}")).ok(),
    }
}

/// use_proxy is httpproxy's useProxy: false for localhost, loopback and NO_PROXY matches.
fn use_proxy(u: &url::Url, no_proxy: &str) -> bool {
    let port = u.port_or_known_default().map(|p| p.to_string()).unwrap_or_default();
    let (host, ip) = match u.host() {
        Some(url::Host::Domain(d)) => (d.to_string(), None),
        Some(url::Host::Ipv4(a)) => (a.to_string(), Some(std::net::IpAddr::V4(a))),
        Some(url::Host::Ipv6(a)) => (a.to_string(), Some(std::net::IpAddr::V6(a).to_canonical())),
        None => return true,
    };
    if host == "localhost" || ip.is_some_and(|ip| ip.is_loopback()) {
        return false;
    }
    let host = host.trim().to_lowercase();
    for p in no_proxy.split(',') {
        let p = p.trim().to_lowercase();
        if p.is_empty() {
            continue;
        }
        if p == "*" {
            return false;
        }
        if let Some((net, bits)) = p.split_once('/') {
            // a CIDR block matches IP hosts only
            if let (Ok(net), Ok(bits)) = (net.parse::<std::net::IpAddr>(), bits.parse::<u32>()) {
                if ip.is_some_and(|ip| cidr_contains(net, bits, ip)) {
                    return false;
                }
                continue;
            }
        }
        let (mut phost, pport) = split_host_port(&p).unwrap_or((p.clone(), String::new()));
        if phost.is_empty() {
            continue;
        }
        if let Ok(pip) = phost.parse::<std::net::IpAddr>() {
            if ip == Some(pip.to_canonical()) && (pport.is_empty() || pport == port) {
                return false;
            }
            continue;
        }
        if let Some(rest) = phost.strip_prefix("*.") {
            phost = format!(".{rest}");
        }
        let match_host = !phost.starts_with('.');
        if match_host {
            phost = format!(".{phost}");
        }
        if !phost.is_ascii()
            && let Ok(url::Host::Domain(d)) = url::Host::parse(&phost)
        {
            phost = d;
        }
        if ip.is_none()
            && (host.ends_with(&phost) || match_host && host == phost[1..])
            && (pport.is_empty() || pport == port)
        {
            return false;
        }
    }
    true
}

/// cidr_contains is net.IPNet.Contains for a block parsed by net.ParseCIDR.
fn cidr_contains(net: std::net::IpAddr, bits: u32, ip: std::net::IpAddr) -> bool {
    let (n, i, len) = match (net, ip) {
        (std::net::IpAddr::V4(n), std::net::IpAddr::V4(i)) => (u32::from(n) as u128, u32::from(i) as u128, 32),
        (std::net::IpAddr::V6(n), std::net::IpAddr::V6(i)) => (u128::from(n), u128::from(i), 128),
        _ => return false,
    };
    if bits > len {
        return false;
    }
    let mask = if bits == 0 { 0 } else { u128::MAX << (128 - bits) >> (128 - len) };
    n & mask == i & mask
}

/// split_host_port is net.SplitHostPort: "host:port" or "[v6]:port"; None when there is no port.
fn split_host_port(s: &str) -> Option<(String, String)> {
    if let Some(rest) = s.strip_prefix('[') {
        let (h, p) = rest.split_once(']')?;
        let port = p.strip_prefix(':')?;
        return (!h.contains(['[', ']']) && !port.contains([':', '[', ']'])).then(|| (h.into(), port.into()));
    }
    let (h, port) = s.rsplit_once(':')?;
    (!h.contains([':', '[', ']']) && !port.contains(['[', ']'])).then(|| (h.into(), port.into()))
}

/// TLS is the TLS setup of Go's default transport: the system certificate store.
static TLS: LazyLock<ureq::tls::TlsConfig> =
    LazyLock::new(|| ureq::tls::TlsConfig::builder().root_certs(system_roots()).build());

/// system_roots is Go's crypto/x509 loadSystemRoots: SSL_CERT_FILE (or the first bundle that reads
/// from CERT_FILES) plus every file in SSL_CERT_DIR (or CERT_DIRS). On macOS and Windows, without
/// either variable Go asks the system to verify; Rust has no such verifier here and uses ureq's
/// bundled Mozilla list instead (declared in rust/diff/update.md).
fn system_roots() -> ureq::tls::RootCerts {
    let (file, dir) = (std::env::var("SSL_CERT_FILE").unwrap_or_default(), std::env::var("SSL_CERT_DIR").unwrap_or_default());
    if cfg!(any(windows, target_os = "macos", target_os = "ios")) && file.is_empty() && dir.is_empty() {
        return ureq::tls::RootCerts::WebPki;
    }
    let mut certs = Vec::new();
    let mut add = |data: &[u8]| {
        certs.extend(ureq::tls::parse_pem(data).filter_map(|i| match i {
            Ok(ureq::tls::PemItem::Certificate(c)) => Some(c),
            _ => None,
        }))
    };
    let files: Vec<&str> = if file.is_empty() { CERT_FILES.to_vec() } else { vec![&file] };
    if let Some(data) = files.iter().find_map(|f| std::fs::read(f).ok()) {
        add(&data);
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let dirs: Vec<&str> = if dir.is_empty() { CERT_DIRS.to_vec() } else { dir.split(sep).collect() };
    for d in dirs {
        let Ok(entries) = std::fs::read_dir(d) else { continue };
        for e in entries.flatten() {
            // readUniqueDirectoryEntries: skip symlinks to a file in the same folder
            let same_dir_link = e.file_type().is_ok_and(|t| t.is_symlink())
                && std::fs::read_link(e.path()).is_ok_and(|t| !t.to_string_lossy().contains(std::path::MAIN_SEPARATOR));
            if !same_dir_link && let Ok(data) = std::fs::read(e.path()) {
                add(&data);
            }
        }
    }
    ureq::tls::RootCerts::new_with_certs(&certs)
}

/// CERT_FILES and CERT_DIRS are crypto/x509's certFiles and certDirectories for Linux
/// (empty on macOS and Windows). ponytail: other Unix systems use the Linux list too.
const CERT_FILES: &[&str] = if cfg!(any(windows, target_os = "macos", target_os = "ios")) {
    &[]
} else {
    &[
        "/etc/ssl/certs/ca-certificates.crt",                // Debian/Ubuntu/Gentoo etc.
        "/etc/pki/tls/certs/ca-bundle.crt",                  // Fedora/RHEL 6
        "/etc/ssl/ca-bundle.pem",                            // OpenSUSE
        "/etc/pki/tls/cacert.pem",                           // OpenELEC
        "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem", // CentOS/RHEL 7
        "/etc/ssl/cert.pem",                                 // Alpine Linux
    ]
};
const CERT_DIRS: &[&str] = if cfg!(any(windows, target_os = "macos", target_os = "ios")) {
    &[]
} else {
    &["/etc/ssl/certs", "/etc/pki/tls/certs"]
};

/// extract_binary returns docc (docc.exe) from a release archive.
fn extract_binary(data: &[u8], name: &str) -> Result<Vec<u8>, Error> {
    // path.Base
    let is_bin = |p: &[u8]| {
        let p = p.strip_suffix(b"/").unwrap_or(p);
        let b = p.rsplit(|&c| c == b'/').next().unwrap_or(p);
        b == b"docc" || b == b"docc.exe"
    };
    if name.ends_with(".zip") {
        // Go's zip.NewReader reports any unreadable archive as ErrFormat
        let mut zr = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|_| "zip: not a valid zip file")?;
        for i in 0..zr.len() {
            let mut f = zr.by_index(i)?;
            if is_bin(f.name_raw()) {
                let mut b = Vec::new();
                f.read_to_end(&mut b)?;
                return Ok(b);
            }
        }
    } else {
        if let Some(e) = gzip_header_err(data) {
            return Err(e.into());
        }
        let mut tr = tar::Archive::new(flate2::read::MultiGzDecoder::new(data));
        if let Ok(entries) = tr.entries() {
            for h in entries {
                let Ok(mut h) = h else { break };
                let path = h.path_bytes().into_owned();
                // Go reads the old type '\0' as a regular file, or a folder when the name ends with '/'
                let t = h.header().entry_type().as_byte();
                if (t == b'0' || t == 0 && !path.ends_with(b"/")) && is_bin(&path) {
                    let mut b = Vec::new();
                    h.read_to_end(&mut b)?;
                    return Ok(b);
                }
            }
        }
    }
    Err(format!("{name} has no docc binary").into())
}

/// gzip_header_err is the error of Go's gzip.NewReader on a broken gzip header
/// (flate2 reads the header lazily and says nothing specific about it).
fn gzip_header_err(data: &[u8]) -> Option<&'static str> {
    const EOF: &str = "unexpected EOF";
    const BAD: &str = "gzip: invalid header";
    if data.is_empty() {
        return Some("EOF");
    }
    if data.len() < 10 {
        return Some(EOF);
    }
    if data[..3] != [0x1f, 0x8b, 8] {
        return Some(BAD);
    }
    let (flg, mut at) = (data[3], 10);
    if flg & 4 != 0 {
        // FEXTRA: a little-endian length and that many bytes
        let Some(n) = data.get(at..at + 2) else { return Some(EOF) };
        at += 2 + usize::from(u16::from_le_bytes([n[0], n[1]]));
        if at > data.len() {
            return Some(EOF);
        }
    }
    for bit in [8, 16] {
        // FNAME, FCOMMENT: zero-terminated, at most 511 bytes before the zero
        if flg & bit != 0 {
            match data[at..].iter().take(512).position(|&b| b == 0) {
                Some(i) => at += i + 1,
                None if data.len() - at < 512 => return Some(EOF),
                None => return Some(BAD),
            }
        }
    }
    if flg & 2 != 0 {
        // FHCRC: the low 16 bits of the CRC-32 of the header so far
        let Some(n) = data.get(at..at + 2) else { return Some(EOF) };
        let mut crc = flate2::Crc::new();
        crc.update(&data[..at]);
        if u16::from_le_bytes([n[0], n[1]]) != crc.sum() as u16 {
            return Some(BAD);
        }
    }
    None
}

/// replace_exe swaps the file in place. A running exe cannot be overwritten on Windows but can be renamed.
fn replace_exe(exe: &Path, bin: &[u8]) -> Result<(), Error> {
    let tmp = exe.with_file_name(format!("{}.new", exe.file_name().unwrap_or_default().to_string_lossy()));
    write_mode(&tmp, bin, 0o755)?;
    if cfg!(windows) {
        let old = exe.with_file_name(format!("{}.old", exe.file_name().unwrap_or_default().to_string_lossy()));
        let _ = std::fs::remove_file(&old);
        if let Err(e) = go_rename(exe, &old) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
    }
    if let Err(e) = go_rename(&tmp, exe) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// go_rename is os.Rename with its *os.LinkError text, "rename <old> <new>: <reason>". Like Go on
/// Unix, a folder in the way of a file gives "file exists" rather than the system's answer.
fn go_rename(old: &Path, new: &Path) -> Result<(), Error> {
    let op = format!("rename {}", old.display());
    if !cfg!(windows) && std::fs::symlink_metadata(new).is_ok_and(|m| m.is_dir()) {
        match std::fs::symlink_metadata(old) {
            Err(e) => return Err(go_path_error(&op, new, &e)),
            Ok(m) if old == new || !same_file(&m, &std::fs::symlink_metadata(new)?) => {
                return Err(go_path_error(&op, new, &std::io::Error::from_raw_os_error(17))); // EEXIST
            }
            Ok(_) => {}
        }
    }
    std::fs::rename(old, new).map_err(|e| go_path_error(&op, new, &e))
}

/// same_file is os.SameFile: the same device and inode.
fn same_file(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    fn release_archive(content: &str) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tw = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        h.set_size(content.len() as u64);
        h.set_mode(0o755);
        h.set_entry_type(tar::EntryType::Regular);
        tw.append_data(&mut h, "docc_v2_linux_amd64/docc", content.as_bytes()).unwrap();
        tw.into_inner().unwrap().finish().unwrap()
    }

    /// serve is httptest.NewServer: every request gets handler(path) -> (status, body).
    fn serve(handler: impl Fn(&str) -> (u16, Vec<u8>) + Send + Sync + 'static) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                let _ = r.read_line(&mut first);
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = first.split(' ').nth(1).unwrap_or("").to_string();
                let (code, body) = handler(&path);
                // a 3xx body is the Location of the redirect
                let (reason, loc, body) = match code {
                    200 => ("OK", String::new(), body),
                    300..400 => ("Found", format!("Location: {}\r\n", String::from_utf8_lossy(&body)), vec![]),
                    _ => ("Not Found", String::new(), body),
                };
                let _ = write!(
                    s,
                    "HTTP/1.1 {code} {reason}\r\n{loc}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(&body);
            }
        });
        url
    }

    #[test]
    fn update() {
        let arc = release_archive("new-binary");
        let sum = hex::encode(Sha256::digest(&arc));
        let good = format!("{sum}  docc_v2_linux_amd64.tar.gz\n{sum}  docc_v2_darwin_arm64.tar.gz\n");
        let checks = Arc::new(Mutex::new(good.clone()));
        let srv = {
            let checks = checks.clone();
            serve(move |path| {
                if path.ends_with("/checksums.txt") {
                    (200, checks.lock().unwrap().clone().into_bytes())
                } else if path.ends_with("/download/v2/docc_v2_linux_amd64.tar.gz") {
                    (200, arc.clone())
                } else {
                    (404, b"404 page not found\n".to_vec())
                }
            })
        };
        let dir = std::env::temp_dir().join(format!("docc-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("docc");
        std::fs::write(&exe, "old-binary").unwrap();
        let mut out = Vec::new();

        let r = update_from(&srv, &exe, "v1", "", true, "linux", "amd64", &mut out);
        assert!(
            matches!(r, Ok(1)) && String::from_utf8_lossy(&out).contains("v2 is available"),
            "--check: {r:?} {:?}",
            String::from_utf8_lossy(&out)
        );
        let r = update_from(&srv, &exe, "v2", "", false, "linux", "amd64", &mut out);
        assert!(
            matches!(r, Ok(0)) && String::from_utf8_lossy(&out).contains("up to date"),
            "up to date: {r:?} {:?}",
            String::from_utf8_lossy(&out)
        );
        assert!(
            update_from(&srv, &exe, "v1", "", false, "linux", "riscv64", &mut out).is_err(),
            "a system without an archive must fail"
        );
        // a wrong checksum leaves the binary alone
        *checks.lock().unwrap() = good.replacen(&sum, &"0".repeat(64), 1);
        let r = update_from(&srv, &exe, "v1", "", false, "linux", "amd64", &mut out);
        assert!(matches!(&r, Err(e) if e.to_string().contains("checksum")), "checksum: {r:?}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"old-binary", "binary changed despite a bad checksum");
        *checks.lock().unwrap() = good;
        let r = update_from(&srv, &exe, "v1", "", false, "linux", "amd64", &mut out);
        assert!(matches!(r, Ok(0)), "update: {r:?}");
        assert_eq!(std::fs::read(&exe).unwrap(), b"new-binary", "binary not replaced");
        // a missing file reads as Go's "<url>: 404 Not Found"
        assert_eq!(fetch(&format!("{srv}/x")).unwrap_err().to_string(), format!("{srv}/x: 404 Not Found"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Redirects are followed like Go's http.Client: at most 10, relative Location resolved.
    #[test]
    fn redirects_like_go() {
        let srv = serve(|path| match path {
            "/a" => (302, b"/b".to_vec()),
            "/b" => (301, b"c".to_vec()),
            "/c" => (200, b"ok".to_vec()),
            "/loop" => (307, b"/loop".to_vec()),
            "/empty" => (302, b"".to_vec()),
            _ => (404, vec![]),
        });
        assert_eq!(fetch(&format!("{srv}/a")).unwrap(), b"ok");
        assert_eq!(fetch(&format!("{srv}/loop")).unwrap_err().to_string(), "stopped after 10 redirects");
        // a 3xx without Location is the answer itself
        assert_eq!(fetch(&format!("{srv}/empty")).unwrap_err().to_string(), format!("{srv}/empty: 302 Found"));
    }

    /// Cases of x/net/http/httpproxy's proxy_test.go that apply to ProxyFromEnvironment.
    #[test]
    fn proxy_from_environment() {
        let cases: &[(&[(&str, &str)], &str, Result<&str, &str>)] = &[
            (&[], "http://example.com", Ok("")),
            (&[("HTTP_PROXY", "127.0.0.1:8080")], "http://example.com", Ok("http://127.0.0.1:8080/")),
            (&[("HTTP_PROXY", "cache.corp.example.com:1234")], "http://example.com", Ok("http://cache.corp.example.com:1234/")),
            (&[("HTTP_PROXY", "cache.corp.example.com")], "http://example.com", Ok("http://cache.corp.example.com/")),
            (&[("HTTP_PROXY", "https://cache.corp.example.com")], "http://example.com", Ok("https://cache.corp.example.com/")),
            (&[("HTTP_PROXY", "http://127.0.0.1:8080")], "http://example.com", Ok("http://127.0.0.1:8080/")),
            (&[("HTTP_PROXY", "socks5://127.0.0.1")], "http://example.com", Ok("socks5://127.0.0.1")),
            (&[("http_proxy", "http://lower")], "http://example.com", Ok("http://lower/")),
            (&[("HTTP_PROXY", "http://upper"), ("http_proxy", "http://lower")], "http://example.com", Ok("http://upper/")),
            // HTTP_PROXY does not apply to https, HTTPS_PROXY not to http, ALL_PROXY never
            (&[("HTTP_PROXY", "http://http.proxy.tld")], "https://secure.tld/", Ok("")),
            (&[("HTTPS_PROXY", "http://secure.proxy.tld")], "https://secure.tld/", Ok("http://secure.proxy.tld/")),
            (&[("HTTPS_PROXY", "http://secure.proxy.tld")], "http://insecure.tld/", Ok("")),
            (&[("ALL_PROXY", "http://all.proxy.tld"), ("all_proxy", "http://all.proxy.tld")], "https://github.com/", Ok("")),
            // localhost and loopback never go through a proxy
            (&[("HTTP_PROXY", "http://127.0.0.1:9")], "http://127.0.0.1:8000/x", Ok("")),
            (&[("HTTP_PROXY", "http://127.0.0.1:9")], "http://localhost:8000/x", Ok("")),
            (&[("HTTP_PROXY", "http://127.0.0.1:9")], "http://[::1]:8000/x", Ok("")),
            (&[("HTTP_PROXY", "http://127.0.0.1:9")], "http://127.0.0.2/", Ok("")),
            // CGI
            (&[("HTTP_PROXY", "http://10.1.2.3:8080"), ("REQUEST_METHOD", "GET")], "http://example.com", Err("refusing to use HTTP_PROXY value in CGI environment; see golang.org/s/cgihttpproxy")),
            (&[("HTTPS_PROXY", "http://10.1.2.3:8080"), ("REQUEST_METHOD", "GET")], "https://example.com", Ok("http://10.1.2.3:8080/")),
        ];
        for (env, req, want) in cases {
            let get = |k: &str| env.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()).unwrap_or_default();
            let got = proxy_for_url(&url::Url::parse(req).unwrap(), &get)
                .map(|p| p.map(|p| p.to_string()).unwrap_or_default())
                .map_err(|e| e.to_string());
            assert_eq!(got.as_deref().map_err(|e| e.as_str()), *want, "{env:?} {req}");
        }
    }

    /// TestUseProxy of httpproxy: hosts against NO_PROXY.
    #[test]
    fn no_proxy_like_go() {
        let no_proxy = "foobar.com, .barbaz.net, *.wildcard.com, 192.168.1.1, 192.168.1.2:81, 192.168.1.3:80, 10.0.0.0/30, 2001:db8::52:0:1, [2001:db8::52:0:2]:443, [2001:db8::52:0:3]:80, 2002:db8:a::45/64, bar.com:80, *.example.org:443";
        let cases = [
            // Never proxy localhost:
            ("http://localhost", false),
            ("http://127.0.0.1", false),
            ("http://127.0.0.2", false),
            ("http://[::1]", false),
            ("http://[::2]", true), // not a loopback address
            ("http://192.168.1.1", false),
            ("http://192.168.1.2", true),
            ("http://192.168.1.2:81", false),
            ("http://192.168.1.3", false),
            ("http://10.0.0.2", false),
            ("http://[2001:db8::52:0:1]", false),
            ("http://[2001:db8::52:0:2]", true),
            ("https://[2001:db8::52:0:2]", false),
            ("http://[2001:db8::52:0:3]", false),
            ("http://[2002:db8:a::123]", false),
            ("http://www.barbaz.net", false),
            ("http://barbaz.net", true),
            ("http://foobar.com", false),
            ("http://www.foobar.com", false),
            ("http://foofoobar.com", true),
            ("http://foo.wildcard.com", false),
            ("http://wildcard.com", true),
            ("http://bar.com", false),
            ("http://bar.com:81", true),
            ("http://foo.example.org", true),
            ("https://foo.example.org", false),
            ("http://example.org", true),
            ("http://foo.bar.com:80", false),
            ("http://google.com", true),
        ];
        for (req, want) in cases {
            assert_eq!(use_proxy(&url::Url::parse(req).unwrap(), no_proxy), want, "{req}");
        }
        assert!(!use_proxy(&url::Url::parse("http://anything.tld").unwrap(), "a.tld, *"));
    }

    /// os.Rename's *os.LinkError text, and Go's "file exists" for a folder in the way.
    #[test]
    fn rename_errors_like_go() {
        let dir = std::env::temp_dir().join(format!("docc-rename-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder/x")).unwrap();
        std::fs::write(dir.join("file"), "x").unwrap();
        let (file, folder, missing) = (dir.join("file"), dir.join("folder"), dir.join("missing"));
        let e = go_rename(&missing, &file).unwrap_err().to_string();
        assert_eq!(e, format!("rename {} {}: no such file or directory", missing.display(), file.display()));
        let e = go_rename(&file, &folder).unwrap_err().to_string();
        assert_eq!(e, format!("rename {} {}: file exists", file.display(), folder.display()));
        let e = go_rename(&missing, &folder).unwrap_err().to_string();
        assert_eq!(e, format!("rename {} {}: no such file or directory", missing.display(), folder.display()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The errors of broken archives, as Go's extractBinary gives them.
    #[test]
    fn broken_archives_like_go() {
        let h = |flg: u8, rest: &[u8]| [&[0x1f, 0x8b, 8, flg, 0, 0, 0, 0, 0, 0xff][..], rest].concat();
        let mut crc = flate2::Crc::new();
        crc.update(&h(2, b""));
        let a = |n: usize| vec![b'a'; n];
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(b"garbage that is not a tar").unwrap();
        let none = "a.tar.gz has no docc binary";
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("a.tar.gz", vec![], "EOF"),
            ("a.tar.gz", vec![0x1f], "unexpected EOF"),
            ("a.tar.gz", b"not a gzip at all".to_vec(), "gzip: invalid header"),
            ("a.tar.gz", vec![0x1f, 0x8b, 7, 0, 0, 0, 0, 0, 0, 0xff], "gzip: invalid header"),
            ("a.tar.gz", h(4, b"\x05"), "unexpected EOF"),
            ("a.tar.gz", h(4, b"\x05\x00ab"), "unexpected EOF"),
            ("a.tar.gz", h(8, b"abc"), "unexpected EOF"),
            ("a.tar.gz", h(8, &[a(511), vec![0]].concat()), none),
            ("a.tar.gz", h(8, &[a(512), vec![0]].concat()), "gzip: invalid header"),
            ("a.tar.gz", h(16, &a(600)), "gzip: invalid header"),
            ("a.tar.gz", h(2, b"\x00"), "unexpected EOF"),
            ("a.tar.gz", h(2, b"\x00\x00"), "gzip: invalid header"),
            ("a.tar.gz", h(2, &(crc.sum() as u16).to_le_bytes()), none),
            ("a.tar.gz", h(0xe0, b""), none),
            ("a.tar.gz", gz.finish().unwrap(), none),
            ("a.zip", vec![], "zip: not a valid zip file"),
            ("a.zip", b"PK\x03\x04garbage".to_vec(), "zip: not a valid zip file"),
        ];
        for (name, data, want) in cases {
            let got = extract_binary(&data, name).map(|_| ()).unwrap_err().to_string();
            assert_eq!(got, want, "{name} {data:?}");
        }
    }
}
