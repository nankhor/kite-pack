mod support;

use kite_ps5::{Console, Keys, Ps5Error, StartOutcome, Status, start_helper, status};
use std::io::Read;
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use support::closed_port;

/// A stand-in ELF loader: records what each connection sent.
fn fake_loader() -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut got = vec![];
        s.read_to_end(&mut got).unwrap();
        got
    });
    (port, handle)
}

#[test]
fn start_sends_the_elf_to_the_loader() {
    let (loader, got) = fake_loader();
    let elf = b"\x7fELF and the rest of the helper".to_vec();
    let outcome = start_helper("127.0.0.1", loader, closed_port(), &elf).unwrap();
    assert_eq!(outcome, StartOutcome::Sent);
    assert_eq!(got.join().unwrap(), elf);
}

#[test]
fn start_does_nothing_when_the_helper_is_up() {
    let helper = TcpListener::bind("127.0.0.1:0").unwrap();
    let helper_port = helper.local_addr().unwrap().port();
    let loader = TcpListener::bind("127.0.0.1:0").unwrap();
    let loader_port = loader.local_addr().unwrap().port();
    loader.set_nonblocking(true).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let outcome = start_helper("127.0.0.1", loader_port, helper_port, b"\x7fELF").unwrap();
    if loader.accept().is_ok() {
        calls.fetch_add(1, Ordering::SeqCst);
    }
    assert_eq!(outcome, StartOutcome::AlreadyRunning);
    assert_eq!(calls.load(Ordering::SeqCst), 0, "the loader must not be contacted");
}

#[test]
fn a_build_without_the_helper_says_so() {
    let err = start_helper("127.0.0.1", closed_port(), closed_port(), b"").unwrap_err();
    assert_eq!(err, Ps5Error::Failed("This build doesn't include the Kite helper.".into()));
}

#[test]
fn a_closed_loader_is_explained() {
    let err = start_helper("127.0.0.1", closed_port(), closed_port(), b"\x7fELF").unwrap_err();
    assert!(err.message().contains("ELF loader"), "{}", err.message());
}

#[test]
fn status_not_set_up() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(status(None, &Keys::open(tmp.path()).unwrap()), Status::NotSetUp);
}

#[test]
fn status_not_reachable() {
    let tmp = tempfile::tempdir().unwrap();
    let console = Console { address: "127.0.0.1".into(), port: closed_port() };
    assert_eq!(status(Some(&console), &Keys::open(tmp.path()).unwrap()), Status::NotReachable);
}

#[cfg(unix)]
#[test]
fn status_running() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = support::Helper::start(&tmp.path().join("console"), &[]);
    let got = status(Some(&helper.console()), &Keys::open(&tmp.path().join("pc")).unwrap());
    assert_eq!(got, Status::Running { version: "0.1.0".into() });
}

#[test]
fn status_serialises_for_the_screen() {
    let json = serde_json::to_value(Status::Running { version: "1".into() }).unwrap();
    assert_eq!(json, serde_json::json!({ "state": "running", "version": "1" }));
    assert_eq!(serde_json::to_value(Status::NotSetUp).unwrap(), serde_json::json!({ "state": "notSetUp" }));
}
