//! ShadowMount+'s HTTP API, the part Kite Pack uses: what version answers, whether a game is
//! installed as a package, and installing a DLC package or a backport.
//!
//! The API is `/api/v1`, JSON in and out, one POST per call, on port 10101 by default. It has no
//! authentication, so the console must be on a trusted network — the caller decides when to talk
//! to it at all. Docs: `docs/api.md` in drakmor/ShadowMountPlus.

use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

/// The port ShadowMount+ answers on unless its config says otherwise.
pub const DEFAULT_PORT: u16 = 10101;

/// What `/api/v1/version` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Version {
    pub status: i64,
    pub api_version: u32,
    pub shadowmount_version: String,
    pub capabilities: Vec<String>,
}

/// One game, as `/api/v1/games/info` describes it. Only the fields Kite Pack reads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Game {
    pub status: i64,
    pub title_id: String,
    pub title_name: String,
    pub version: String,
    /// The base is installed as a native package (patches and DLC can be installed).
    pub installed: bool,
    /// The base is registered from a folder or image (a mount, not an installed package).
    pub base_installed: bool,
    /// `folder`, `image`, `pkg` ...
    pub source_type: String,
    /// The backport directory ShadowMount+ found for it, empty when none.
    pub backport_path: String,
    pub error: Option<String>,
    pub error_reason: Option<String>,
}

/// A storage job: installs, copies and backport installs share one shape.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Job {
    pub status: i64,
    pub job_id: i64,
    pub operation: String,
    /// `idle`, `preparing`, `measuring`, `transferring`, `deleting`, `finalizing`, `installing`,
    /// `completed`, `failed`, `cancelled`.
    pub state: String,
    pub active: bool,
    pub cancellable: bool,
    pub title_id: String,
    pub title_name: String,
    pub source: String,
    pub destination: String,
    pub total_bytes: i64,
    pub processed_bytes: i64,
    pub progress_percent: f64,
    pub speed_bytes_per_second: i64,
    /// Sony's installer result, an unsigned 32-bit code in an i64.
    pub native_status: i64,
    pub package_kind: String,
    pub result_status: i64,
    pub result_error: Option<String>,
    pub result_error_reason: Option<String>,
    pub error: Option<String>,
    pub error_reason: Option<String>,
}

impl Job {
    /// The job has stopped: completed, failed or cancelled.
    pub fn stopped(&self) -> bool {
        matches!(self.state.as_str(), "completed" | "failed" | "cancelled")
    }

    /// Why it failed, in ShadowMount+'s words, or None while it is fine.
    pub fn failure(&self) -> Option<String> {
        if self.state != "failed" {
            return None;
        }
        if let Some(why) = self.result_error.as_deref().filter(|s| !s.is_empty()) {
            return Some(why.to_string());
        }
        if self.native_status != 0 {
            return Some(format!(
                "Sony's installer refused it (0x{:08X})",
                self.native_status as u32
            ));
        }
        if let Some(why) = self.error.as_deref().filter(|s| !s.is_empty()) {
            return Some(why.to_string());
        }
        Some("the install failed".to_string())
    }
}

/// A client for one console's ShadowMount+.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// `http://<host>:<port>`, no trailing slash.
    base: String,
}

impl Client {
    /// `host` is an IPv4 or IPv6 address without a scheme; IPv6 goes in brackets.
    pub fn new(host: &str, port: u16) -> Client {
        // A pasted address may carry a scheme, a port, or brackets around an IPv6 address.
        let raw = host.trim();
        let raw = raw
            .strip_prefix("http://")
            .or_else(|| raw.strip_prefix("https://"))
            .unwrap_or(raw)
            .trim_end_matches('/');
        let host = if let Some(rest) = raw.strip_prefix('[') {
            // [v6] or [v6]:port: take the bracket contents.
            rest.split(']').next().unwrap_or(rest).to_string()
        } else if raw.matches(':').count() == 1 {
            // host:port, but not a bare IPv6 address.
            let (before, after) = raw.split_once(':').unwrap_or((raw, ""));
            if after.chars().all(|c| c.is_ascii_digit()) && !after.is_empty() {
                before.to_string()
            } else {
                raw.to_string()
            }
        } else {
            raw.to_string()
        };
        let host = if host.contains(':') { format!("[{host}]") } else { host };
        Client {
            base: format!("http://{host}:{port}"),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The API version and capabilities; fails when nothing answers.
    pub fn probe(&self) -> Result<Version, String> {
        self.post("/api/v1/version", json!({}))
    }

    /// One game by title id.
    pub fn game(&self, title_id: &str) -> Result<Game, String> {
        self.post("/api/v1/games/info", json!({ "title_id": title_id }))
    }

    /// Installs a `.pkg` that is on the console (under `/data` or a USB drive).
    pub fn install_package(&self, path: &str) -> Result<Job, String> {
        self.post("/api/v1/packages/install", json!({ "path": path }))
    }

    /// Installs a backport folder that is on the console.
    pub fn install_backport(&self, path: &str) -> Result<Job, String> {
        self.post("/api/v1/backports/install", json!({ "path": path }))
    }

    /// The storage job `id`; ShadowMount+ keeps the last one after it ends.
    pub fn job(&self, id: i64) -> Result<Job, String> {
        self.post("/api/v1/games/storage/status", json!({ "job_id": id }))
    }

    /// Asks ShadowMount+ to stop a job while it is still cancellable.
    pub fn cancel_job(&self, id: i64) -> Result<Job, String> {
        self.post("/api/v1/games/storage/cancel", json!({ "job_id": id }))
    }

    fn post<T: for<'de> Deserialize<'de>>(
        &self,
        route: &str,
        body: serde_json::Value,
    ) -> Result<T, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(3)))
            .timeout_recv_response(Some(Duration::from_secs(20)))
            // The body's own `error` is more useful than a bare code.
            .http_status_as_error(false)
            .user_agent("Kite-Pack")
            .build()
            .into();
        let url = format!("{}{route}", self.base);
        let mut res = agent
            .post(&url)
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| match e {
                ureq::Error::Timeout(_) => "ShadowMount+ didn't answer in time".to_string(),
                other => format!("Couldn't reach ShadowMount+: {other}"),
            })?;
        let code = res.status().as_u16();
        let text = res
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("Couldn't read ShadowMount+'s answer: {e}"))?;
        if !(200..300).contains(&code) {
            let said = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
            return Err(match said {
                Some(why) if !why.is_empty() => format!("ShadowMount+ refused: {why}"),
                _ => format!("ShadowMount+ answered {code}"),
            });
        }
        serde_json::from_str(&text)
            .map_err(|e| format!("ShadowMount+'s answer didn't make sense: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// A tiny HTTP server: answers with `reply(path, body)` and keeps what it was asked.
    struct Mock {
        base: String,
        asked: Arc<Mutex<Vec<(String, String)>>>,
        stop: Arc<Mutex<bool>>,
    }

    impl Mock {
        fn start(reply: impl Fn(&str, &str) -> (u16, String) + Send + 'static) -> Mock {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let asked = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(Mutex::new(false));
            {
                let asked = Arc::clone(&asked);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    for stream in listener.incoming() {
                        if *stop.lock().unwrap() {
                            break;
                        }
                        let Ok(mut stream) = stream else { continue };
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let (path, body) = loop {
                            let n = stream.read(&mut chunk).unwrap_or(0);
                            if n == 0 {
                                break (String::new(), String::new());
                            }
                            buf.extend_from_slice(&chunk[..n]);
                            if let Some(end) = find(&buf, b"\r\n\r\n") {
                                let head = String::from_utf8_lossy(&buf[..end]).to_string();
                                let len = head
                                    .lines()
                                    .find_map(|l| {
                                        l.to_ascii_lowercase()
                                            .strip_prefix("content-length:")
                                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                    })
                                    .unwrap_or(0);
                                if buf.len() >= end + 4 + len {
                                    let body =
                                        String::from_utf8_lossy(&buf[end + 4..end + 4 + len])
                                            .to_string();
                                    let path = head
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                        .split_whitespace()
                                        .nth(1)
                                        .unwrap_or("")
                                        .to_string();
                                    break (path, body);
                                }
                            }
                        };
                        asked.lock().unwrap().push((path.clone(), body.clone()));
                        let (code, out) = reply(&path, &body);
                        let head = format!(
                            "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            out.len()
                        );
                        let _ = stream.write_all(head.as_bytes());
                        let _ = stream.write_all(out.as_bytes());
                    }
                });
            }
            Mock {
                base: format!("127.0.0.1:{port}"),
                asked,
                stop,
            }
        }

        fn client(&self) -> Client {
            let (host, port) = self.base.rsplit_once(':').unwrap();
            Client::new(host, port.parse().unwrap())
        }

        fn asked(&self) -> Vec<(String, String)> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            *self.stop.lock().unwrap() = true;
        }
    }

    fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
        hay.windows(needle.len()).position(|w| w == needle)
    }

    #[test]
    fn probe_reads_the_version_and_capabilities() {
        let mock = Mock::start(|path, _| {
            assert_eq!(path, "/api/v1/version");
            (200, r#"{"status":0,"api_version":1,"shadowmount_version":"1.7","capabilities":["pkg_install","backport_install"]}"#.into())
        });
        let v = mock.client().probe().unwrap();
        assert_eq!(v.api_version, 1);
        assert_eq!(v.shadowmount_version, "1.7");
        assert!(v.capabilities.iter().any(|c| c == "pkg_install"));
    }

    #[test]
    fn installing_a_package_posts_its_console_path() {
        let mock = Mock::start(|path, body| {
            assert_eq!(path, "/api/v1/packages/install");
            assert!(body.contains("/data/kite/incoming/suits.pkg"), "{body}");
            (202, r#"{"status":0,"job_id":7,"operation":"install","state":"preparing","active":true}"#.into())
        });
        let job = mock
            .client()
            .install_package("/data/kite/incoming/suits.pkg")
            .unwrap();
        assert_eq!(job.job_id, 7);
        assert_eq!(job.state, "preparing");
        assert!(!job.stopped());
        assert_eq!(mock.asked()[0].0, "/api/v1/packages/install");
    }

    #[test]
    fn a_finished_job_says_how_it_went() {
        let mock = Mock::start(|_, _| {
            (
                200,
                r#"{"status":0,"job_id":7,"state":"completed","result_status":0}"#.into(),
            )
        });
        assert!(mock.client().job(7).unwrap().stopped());
        assert_eq!(mock.client().job(7).unwrap().failure(), None);
    }

    #[test]
    fn a_failed_install_carries_the_reason_and_the_native_code() {
        let mock = Mock::start(|_, _| {
            (
                200,
                r#"{"status":0,"job_id":9,"state":"failed","result_error":"this DLC needs its base content installed","native_status":2158166039}"#
                    .into(),
            )
        });
        let job = mock.client().job(9).unwrap();
        assert_eq!(
            job.failure().unwrap(),
            "this DLC needs its base content installed"
        );

        let mock = Mock::start(|_, _| {
            (
                200,
                r#"{"status":0,"job_id":9,"state":"failed","native_status":2158166039}"#.into(),
            )
        });
        let said = mock.client().job(9).unwrap().failure().unwrap();
        assert!(said.contains("0x80A30017"), "{said}");
    }

    #[test]
    fn a_refusal_keeps_shadowmounts_own_words() {
        let mock =
            Mock::start(|_, _| (409, r#"{"status":16,"error":"base game required"}"#.into()));
        let why = mock.client().install_package("/data/x.pkg").unwrap_err();
        assert_eq!(why, "ShadowMount+ refused: base game required");
    }

    #[test]
    fn a_game_says_whether_its_base_is_installed() {
        let mock = Mock::start(|path, body| {
            assert_eq!(path, "/api/v1/games/info");
            assert!(body.contains("PPSA01234"), "{body}");
            (
                200,
                r#"{"status":0,"title_id":"PPSA01234","installed":true,"base_installed":true,"source_type":"pkg","backport_path":""}"#
                    .into(),
            )
        });
        let g = mock.client().game("PPSA01234").unwrap();
        assert!(g.installed);
        assert_eq!(g.source_type, "pkg");
    }

    #[test]
    fn a_backport_is_installed_by_its_folder_path() {
        let mock = Mock::start(|path, body| {
            assert_eq!(path, "/api/v1/backports/install");
            assert!(body.contains("PPSA01234 Game"), "{body}");
            (
                202,
                r#"{"status":0,"job_id":3,"operation":"copy","state":"preparing"}"#.into(),
            )
        });
        let job = mock
            .client()
            .install_backport("/data/kite/incoming/PPSA01234 Game")
            .unwrap();
        assert_eq!(job.job_id, 3);
    }

    #[test]
    fn addresses_with_a_scheme_or_a_port_still_point_at_the_api() {
        let a = Client::new("192.168.1.42", 10101);
        assert_eq!(a.base(), "http://192.168.1.42:10101");
        assert_eq!(Client::new("192.168.1.42:9131", 10101).base(), a.base());
        assert_eq!(Client::new("http://192.168.1.42:9131/", 10101).base(), a.base());
        assert_eq!(Client::new("[fe80::1]", 10101).base(), "http://[fe80::1]:10101");
        assert_eq!(Client::new("[fe80::1]:9131", 10101).base(), "http://[fe80::1]:10101");
    }

    #[test]
    fn nothing_answering_is_in_words() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let why = Client::new("127.0.0.1", port).probe().unwrap_err();
        assert!(why.contains("Couldn't reach ShadowMount+"), "{why}");
    }
}
