//! A tiny HTTP server serving one package to the console's installer. Sony's installer fetches
//! the pkg by URL; it asks for byte ranges, so ranges and HEAD are answered.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Serves `pkg` to the console at `console`'s network, answering HEAD and byte ranges, and counts
/// the bytes the console has had (each byte once, whatever order and however often it asks).
pub struct PkgServer {
    url: String,
    size: u64,
    stop: Arc<AtomicBool>,
    seen: Arc<Seen>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// What the console asked for: the exact byte spans it was sent, and how many requests.
#[derive(Default)]
struct Seen {
    spans: Mutex<Vec<(u64, u64)>>,
    requests: AtomicU64,
}

impl Seen {
    /// Adds `[start, end)`, keeping the spans sorted, disjoint and apart.
    fn add(&self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        let mut spans = self.spans.lock().unwrap_or_else(|e| e.into_inner());
        let (mut a, mut b) = (start, end);
        spans.retain(|&(x, y)| {
            if y < a || x > b {
                true
            } else {
                a = a.min(x);
                b = b.max(y);
                false
            }
        });
        let at = spans.partition_point(|&(x, _)| x < a);
        spans.insert(at, (a, b));
    }

    fn total(&self) -> u64 {
        self.spans.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(a, b)| b - a).sum()
    }
}

impl PkgServer {
    /// On a random port, under a random token (a DLC, served once).
    pub fn start(pkg: &Path, console: &str) -> Result<PkgServer, String> {
        PkgServer::start_on(pkg, console, 0, token())
    }

    /// On a port and under a token that come from the package and the console, so after a
    /// restart the console's installer finds the same link again (a random port when that one
    /// is taken).
    pub fn start_stable(pkg: &Path, console: &str) -> Result<PkgServer, String> {
        let text = format!("{}\0{}", pkg.display(), console.trim());
        let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
        let port = 40_000 + (hash % 20_000) as u16;
        PkgServer::start_on(pkg, console, port, format!("{hash:016x}"))
            .or_else(|_| PkgServer::start_on(pkg, console, 0, format!("{hash:016x}")))
    }

    fn start_on(pkg: &Path, console: &str, port: u16, token: String) -> Result<PkgServer, String> {
        let size = std::fs::metadata(pkg).map_err(|e| format!("Couldn't read the package: {e}"))?.len();
        if size == 0 {
            return Err("That package is empty".into());
        }
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("Couldn't open a port to stream it: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let ip = pc_address(console).ok_or("Couldn't find this computer's address on the console's network")?;
        let name = pkg.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "package.pkg".into());
        let url = format!("http://{ip}:{port}/{token}/{}", in_link(&name));
        let file = Arc::new(pkg.to_path_buf());
        let stop = Arc::new(AtomicBool::new(false));
        let seen = Arc::new(Seen::default());
        let (stop2, seen2) = (Arc::clone(&stop), Arc::clone(&seen));
        listener.set_nonblocking(true).ok();
        let thread = std::thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        stream.set_nonblocking(false).ok();
                        let (file, token, seen) = (Arc::clone(&file), token.clone(), Arc::clone(&seen2));
                        std::thread::spawn(move || serve(stream, &file, &token, &seen));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(PkgServer { url, size, stop, seen, thread: Some(thread) })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// The package's size.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Bytes of the package the console has been sent, each counted once.
    pub fn received(&self) -> u64 {
        self.seen.total()
    }

    /// Requests for the package so far (HEAD included).
    pub fn requests(&self) -> u64 {
        self.seen.requests.load(Ordering::SeqCst)
    }
}

impl Drop for PkgServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The port is free once this returns: a server started next can take it again.
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn token() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}{:x}", nanos, std::process::id())
}

/// The address this computer uses to reach the console (not 127.0.0.1): a UDP connect only
/// picks the route, it sends nothing.
fn pc_address(console: &str) -> Option<IpAddr> {
    let host = console.trim();
    let host = host.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host);
    let socket = UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect((host, 9)).ok()?;
    socket.local_addr().ok().map(|a| a.ip())
}

fn serve(mut stream: TcpStream, file: &Path, token: &str, seen: &Seen) {
    let mut head = Vec::new();
    let mut buf = [0u8; 4096];
    while head.len() < 16 * 1024 {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        head.extend_from_slice(&buf[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.lines();
    let Some(request) = lines.next() else { return };
    let mut parts = request.split_whitespace();
    let (Some(method), Some(path), _) = (parts.next(), parts.next(), parts.next()) else { return };
    if !path.contains(token) {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return;
    }
    seen.requests.fetch_add(1, Ordering::SeqCst);
    let size = match std::fs::metadata(file) {
        Ok(m) => m.len(),
        Err(_) => return,
    };
    let mut range: Option<(u64, u64)> = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        if name.eq_ignore_ascii_case("range") {
            range = parse_range(value.trim(), size);
        }
    }
    let (start, end, status) = match range {
        Some((a, b)) if a <= b && a < size => (a, b.min(size - 1), "206 Partial Content"),
        Some(_) => {
            let _ = stream.write_all(format!("HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{size}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes());
            return;
        }
        None => (0, size.saturating_sub(1), "200 OK"),
    };
    let len = end.saturating_sub(start).saturating_add(1);
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/octet-stream\r\nContent-Length: {len}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n"
    );
    if status.starts_with("206") {
        head.push_str(&format!("Content-Range: bytes {start}-{end}/{size}\r\n"));
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).is_err() || method.eq_ignore_ascii_case("HEAD") {
        return;
    }
    let Ok(mut f) = File::open(file) else { return };
    if f.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let mut left = len;
    let mut chunk = vec![0u8; 256 * 1024];
    while left > 0 {
        let want = (chunk.len() as u64).min(left) as usize;
        match f.read(&mut chunk[..want]) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if stream.write_all(&chunk[..n]).is_err() {
                    break;
                }
                let at = start + (len - left);
                seen.add(at, at + n as u64);
                left -= n as u64;
            }
        }
    }
}

fn parse_range(value: &str, size: u64) -> Option<(u64, u64)> {
    let rest = value.strip_prefix("bytes=")?;
    let (a, b) = rest.split_once('-')?;
    if a.is_empty() {
        let n: u64 = b.trim().parse().ok()?;
        return Some((size.saturating_sub(n), size.saturating_sub(1)));
    }
    let start: u64 = a.trim().parse().ok()?;
    let end = if b.trim().is_empty() { size.saturating_sub(1) } else { b.trim().parse().ok()? };
    Some((start, end))
}

/// `name` as it can sit in a link: a game's file name often has spaces and brackets, which
/// Sony's downloader would not take as they are.
fn in_link(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_a_byte_range_and_a_head() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("suits.pkg");
        std::fs::write(&pkg, b"0123456789").unwrap();
        let server = PkgServer::start(&pkg, "127.0.0.1").unwrap();
        let url = server.url().to_string();
        let rest = url.trim_start_matches("http://");
        let (addr, path) = rest.split_once('/').unwrap();
        let path = format!("/{path}");
        let mut s = TcpStream::connect(addr).unwrap();
        let request = format!("GET {path} HTTP/1.1\r\nHost: x\r\nRange: bytes=2-5\r\n\r\n");
        s.write_all(request.as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(text.starts_with("HTTP/1.1 206"), "{text}");
        assert!(text.contains("Content-Range: bytes 2-5/10"), "{text}");
        assert!(out.ends_with(b"2345"));
    }

    /// GETs `range` (a `Range:` value) of the server's package, whole response.
    fn get(url: &str, range: &str) -> Vec<u8> {
        let rest = url.trim_start_matches("http://");
        let (addr, path) = rest.split_once('/').unwrap();
        let mut s = TcpStream::connect(addr).unwrap();
        s.write_all(format!("GET /{path} HTTP/1.1\r\nHost: x\r\nRange: {range}\r\n\r\n").as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn a_name_with_spaces_is_safe_in_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("Ashen Tide (EU) 1.02.pkg");
        std::fs::write(&pkg, b"0123456789").unwrap();
        let server = PkgServer::start(&pkg, "127.0.0.1").unwrap();
        assert!(server.url().ends_with("/Ashen%20Tide%20%28EU%29%201.02.pkg"), "{}", server.url());
        assert!(get(server.url(), "bytes=2-5").ends_with(b"2345"));
    }

    /// Waits for the server's count to reach `want` (it counts once its bytes are written).
    fn settles(server: &PkgServer, want: u64) -> u64 {
        let since = std::time::Instant::now();
        while server.received() != want && since.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(5));
        }
        server.received()
    }

    #[test]
    fn counts_each_byte_once_whatever_the_order() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("game.pkg");
        let size: u64 = (1 << 20) + 100;
        std::fs::write(&pkg, vec![5u8; size as usize]).unwrap();
        let server = PkgServer::start_stable(&pkg, "127.0.0.1").unwrap();
        assert_eq!((server.size(), server.received(), server.requests()), (size, 0, 0));
        let half = size / 2;
        // Sony reads the end first, then the bulk, some of it twice.
        get(server.url(), "bytes=-65536");
        assert_eq!(settles(&server, 65536), 65536);
        get(server.url(), &format!("bytes=0-{}", half - 1));
        assert_eq!(settles(&server, half + 65536), half + 65536);
        get(server.url(), &format!("bytes=0-{}", half - 1));
        assert_eq!(settles(&server, half + 65536), half + 65536, "a range fetched twice counts once");
        get(server.url(), &format!("bytes={half}-"));
        assert_eq!(settles(&server, size), size);
        assert_eq!(server.requests(), 4);
    }

    #[test]
    fn the_same_package_gets_the_same_link() {
        let tmp = tempfile::tempdir().unwrap();
        let pkg = tmp.path().join("game.pkg");
        std::fs::write(&pkg, b"0123456789").unwrap();
        let first = PkgServer::start_stable(&pkg, "127.0.0.1").unwrap().url().to_string();
        let again = PkgServer::start_stable(&pkg, "127.0.0.1").unwrap();
        assert_eq!(again.url(), first, "after a restart the PS5 asks the same link");
        assert!(get(again.url(), "bytes=0-3").ends_with(b"0123"));
        let other = tmp.path().join("other.pkg");
        std::fs::write(&other, b"x").unwrap();
        assert_ne!(PkgServer::start_stable(&other, "127.0.0.1").unwrap().url(), first);
    }
}
