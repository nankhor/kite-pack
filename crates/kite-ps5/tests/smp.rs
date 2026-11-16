//! Installing through ShadowMount+: the file really lands on the console, then its API is driven.
#![cfg(unix)]
mod support;

use kite_pipeline::{InstallError, Installer, SendTarget};
use kite_ps5::{Keys, SmpInstaller};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::Helper;

/// A tiny HTTP server for ShadowMount+'s API: `answer(path)` gives the body and status, and
/// what it was asked is kept.
struct Smp {
    port: u16,
    asked: Arc<Mutex<Vec<(String, String)>>>,
    stop: Arc<Mutex<bool>>,
}

impl Smp {
    fn start(answer: impl Fn(&str) -> (u16, String) + Send + 'static) -> Smp {
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
                        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
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
                                let body = String::from_utf8_lossy(&buf[end + 4..end + 4 + len])
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
                    asked.lock().unwrap().push((path.clone(), body));
                    let (code, out) = answer(&path);
                    let head = format!(
                        "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        out.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(out.as_bytes());
                }
            });
        }
        Smp { port, asked, stop }
    }

    fn asked(&self) -> Vec<(String, String)> {
        self.asked.lock().unwrap().clone()
    }
}

impl Drop for Smp {
    fn drop(&mut self) {
        *self.stop.lock().unwrap() = true;
    }
}

fn installer(helper: &Helper, incoming: &Path, smp: u16) -> SmpInstaller {
    let keys = Keys::open(&helper.state.join("pc")).unwrap();
    let host = "127.0.0.1".to_string();
    let port = helper.port;
    let mut out = SmpInstaller::new(
        keys,
        helper.state.join("outboards"),
        Arc::new(move || Some((host.clone(), port, smp))),
    );
    out.incoming = incoming.display().to_string();
    out.poll = Duration::from_millis(5);
    out.ceiling = Duration::from_secs(5);
    out
}

fn target() -> SendTarget {
    SendTarget {
        console: "127.0.0.1".into(),
        destination: "/unused".into(),
    }
}

#[test]
fn a_dlc_package_is_sent_and_shadowmount_installs_it() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let incoming = helper.share.join("incoming");
    let pkg = tmp.path().join("suits.pkg");
    std::fs::write(&pkg, b"a fake package").unwrap();
    let calls = Arc::new(Mutex::new(0));
    let count = Arc::clone(&calls);
    let smp = Smp::start(move |path| match path {
        "/api/v1/packages/install" => (
            202,
            r#"{"status":0,"job_id":1,"operation":"install","state":"preparing"}"#.into(),
        ),
        "/api/v1/games/storage/status" => {
            let mut n = count.lock().unwrap();
            *n += 1;
            if *n < 2 {
                (
                    200,
                    r#"{"status":0,"job_id":1,"state":"installing","progress_percent":40}"#.into(),
                )
            } else {
                (
                    200,
                    r#"{"status":0,"job_id":1,"state":"completed","result_status":0}"#.into(),
                )
            }
        }
        other => panic!("asked {other}"),
    });

    installer(&helper, &incoming, smp.port)
        .install(&pkg, &target(), &AtomicBool::new(false))
        .unwrap();

    assert_eq!(
        std::fs::read(incoming.join("suits.pkg")).unwrap(),
        b"a fake package"
    );
    let asked = smp.asked();
    assert_eq!(asked[0].0, "/api/v1/packages/install");
    assert!(
        asked[0]
            .1
            .contains(&incoming.join("suits.pkg").display().to_string()),
        "{}",
        asked[0].1
    );
}

#[test]
fn a_failed_install_says_why() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let pkg = tmp.path().join("map.pkg");
    std::fs::write(&pkg, b"x").unwrap();
    let smp = Smp::start(|path| {
        match path {
        "/api/v1/packages/install" => (202, r#"{"status":0,"job_id":2,"state":"preparing"}"#.into()),
        _ => (
            200,
            r#"{"status":0,"job_id":2,"state":"failed","result_error":"this DLC needs its base content installed"}"#.into(),
        ),
    }
    });

    let err = installer(&helper, &helper.share.join("in"), smp.port)
        .install(&pkg, &target(), &AtomicBool::new(false))
        .unwrap_err();
    match err {
        InstallError::Failed(why) => assert!(why.contains("needs its base"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_backport_folder_is_sent_and_shadowmount_takes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let folder = tmp.path().join("PPSA01234 Game");
    std::fs::create_dir_all(folder.join("fakelib")).unwrap();
    std::fs::write(folder.join("eboot.bin"), b"patched").unwrap();
    std::fs::write(folder.join("fakelib/libSceVideoOut.sprx"), b"fake").unwrap();
    let smp = Smp::start(|path| match path {
        "/api/v1/backports/install" => (
            202,
            r#"{"status":0,"job_id":3,"operation":"copy","state":"preparing"}"#.into(),
        ),
        _ => (
            200,
            r#"{"status":0,"job_id":3,"state":"completed","result_status":0}"#.into(),
        ),
    });

    installer(&helper, &helper.share.join("incoming"), smp.port)
        .install_backport(&folder, &AtomicBool::new(false))
        .unwrap();

    let landed = helper.share.join("incoming").join("PPSA01234 Game");
    assert_eq!(std::fs::read(landed.join("eboot.bin")).unwrap(), b"patched");
    assert!(landed.join("fakelib/libSceVideoOut.sprx").is_file());
    assert_eq!(smp.asked()[0].0, "/api/v1/backports/install");
}

#[test]
fn status_says_whether_the_base_is_installed_or_only_mounted() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let smp = Smp::start(|path| {
        match path {
        "/api/v1/version" => (200, r#"{"status":0,"api_version":1,"shadowmount_version":"1.7"}"#.into()),
        "/api/v1/games/info" => (
            200,
            r#"{"status":0,"title_id":"PPSA01234","installed":false,"base_installed":true,"source_type":"folder"}"#.into(),
        ),
        _ => (404, r#"{"status":2,"error":"no"}"#.into()),
    }
    });

    let got = installer(&helper, &helper.share.join("in"), smp.port)
        .status(Some("PPSA01234"))
        .unwrap();
    assert_eq!(got.version, "1.7");
    assert!(!got.installed);
    assert!(got.base_installed);
    assert_eq!(got.source_type, "folder");
}

/// A package path the helper will accept, for the upload's side.
#[allow(dead_code)]
fn _noop(_: &Path) -> PathBuf {
    PathBuf::new()
}
