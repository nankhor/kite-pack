//! A tiny HTTP server serving one package to the console's installer. Sony's installer fetches
//! the pkg by URL; it asks for byte ranges, so ranges and HEAD are answered.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Serves `pkg` to the console at `console`'s network, on a random port, under a random token.
pub struct PkgServer {
    url: String,
    stop: Arc<AtomicBool>,
}

impl PkgServer {
    pub fn start(pkg: &Path, console: &str) -> Result<PkgServer, String> {
        let size = std::fs::metadata(pkg).map_err(|e| format!("Couldn't read the package: {e}"))?.len();
        if size == 0 {
            return Err("That package is empty".into());
        }
        let listener = TcpListener::bind(("0.0.0.0", 0)).map_err(|e| format!("Couldn't open a port to stream it: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let ip = pc_address(console).ok_or("Couldn't find this computer's address on the console's network")?;
        let token = token();
        let name = pkg.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "dlc.pkg".into());
        let url = format!("http://{ip}:{port}/{token}/{name}");
        let file = Arc::new(pkg.to_path_buf());
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = Arc::clone(&stop);
        listener.set_nonblocking(true).ok();
        std::thread::spawn(move || {
            while !stop2.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let file = Arc::clone(&file);
                        let token = token.clone();
                        std::thread::spawn(move || serve(stream, &file, &token));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(PkgServer { url, stop })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

impl Drop for PkgServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
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

fn serve(mut stream: TcpStream, file: &Path, token: &str) {
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
}
