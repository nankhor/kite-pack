//! The Kite helper, built for this computer, run as a stand-in PS5.
#![allow(dead_code)]

use kite_ps5::Console;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Once;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// `make -C helper host`, once per test binary.
pub fn helper_bin() -> PathBuf {
    static BUILD: Once = Once::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../helper");
    BUILD.call_once(|| {
        let status = Command::new("make").arg("-C").arg(&root).arg("host").status().expect("make");
        assert!(status.success(), "building the host helper failed");
    });
    root.join("build/host/kite-helper")
}

pub struct Helper {
    child: Child,
    lines: Receiver<String>,
    pub port: u16,
    pub state: PathBuf,
    pub share: PathBuf,
}

impl Helper {
    /// A helper on any free port, with its state and share folders inside `dir`.
    pub fn start(dir: &Path, extra: &[&str]) -> Helper {
        Helper::start_at(&dir.join("state"), &dir.join("ps5"), 0, extra)
    }

    pub fn start_at(state: &Path, share: &Path, port: u16, extra: &[&str]) -> Helper {
        std::fs::create_dir_all(share).unwrap();
        let mut child = Command::new(helper_bin())
            .arg("--state")
            .arg(state)
            .arg("--allow")
            .arg(share)
            .arg("--port")
            .arg(port.to_string())
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start the helper");
        let (tx, lines) = channel();
        let out = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let port = loop {
            let line = lines.recv_timeout(Duration::from_secs(10)).expect("the helper never said it was listening");
            if let Some(p) = line.strip_prefix("kite helper listening on ") {
                break p.trim().parse().unwrap();
            }
        };
        Helper { child, lines, port, state: state.to_path_buf(), share: std::fs::canonicalize(share).unwrap() }
    }

    pub fn console(&self) -> Console {
        Console { address: "127.0.0.1".into(), port: self.port }
    }

    /// Stops it the hard way, as a PS5 that goes to rest mode would.
    pub fn kill(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A port nothing listens on.
pub fn closed_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}
