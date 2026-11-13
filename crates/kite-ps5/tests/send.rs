#![cfg(unix)]
mod support;

use kite_pipeline::{Progress, SendError, SendTarget, Sender};
use kite_ps5::{Keys, Ps5Sender};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use support::{Helper, closed_port};

/// Deterministic, poorly compressible bytes.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect()
}

/// A packed folder like Forge makes: a few files, one of them large.
fn packed_folder(dir: &Path) -> PathBuf {
    let root = dir.join("[Ashen Tide]-[PPSA01234]");
    std::fs::create_dir_all(root.join("sce_sys")).unwrap();
    std::fs::write(root.join("ampr_zstd.index"), noise(4096, 1)).unwrap();
    std::fs::write(root.join("ampr_zstd-000.pak"), noise(3_000_000, 2)).unwrap();
    std::fs::write(root.join("sce_sys/param.json"), b"{}").unwrap();
    root
}

fn sender(dir: &Path, port: u16) -> Ps5Sender {
    Ps5Sender::new(Keys::open(&dir.join("pc")).unwrap(), port, dir.join("outboards"))
}

fn target(helper: &Helper) -> SendTarget {
    SendTarget { console: "127.0.0.1".into(), destination: helper.share.join("homebrew").display().to_string() }
}

fn same_tree(a: &Path, b: &Path) {
    for entry in walk(a) {
        let rel = entry.strip_prefix(a).unwrap();
        assert_eq!(std::fs::read(&entry).unwrap(), std::fs::read(b.join(rel)).unwrap(), "{}", rel.display());
    }
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() { out.extend(walk(&p)) } else { out.push(p) }
    }
    out
}

#[test]
fn sends_a_packed_folder_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let packed = packed_folder(tmp.path());
    sender(tmp.path(), helper.port).send(&packed, &target(&helper), &|_| {}, &AtomicBool::new(false)).unwrap();
    same_tree(&packed, &helper.share.join("homebrew/[Ashen Tide]-[PPSA01234]"));
}

#[test]
fn sends_a_single_image_file() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let image = tmp.path().join("[Ashen Tide]-[PPSA01234].ffpkg");
    std::fs::write(&image, noise(2_500_000, 3)).unwrap();
    sender(tmp.path(), helper.port).send(&image, &target(&helper), &|_| {}, &AtomicBool::new(false)).unwrap();
    let landed = helper.share.join("homebrew/[Ashen Tide]-[PPSA01234].ffpkg");
    assert!(landed.is_file(), "the image must land as one file");
    assert_eq!(std::fs::read(&landed).unwrap(), std::fs::read(&image).unwrap());
}

#[test]
fn reports_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let packed = packed_folder(tmp.path());
    let seen = Mutex::new(vec![]);
    sender(tmp.path(), helper.port)
        .send(&packed, &target(&helper), &|p| seen.lock().unwrap().push(p), &AtomicBool::new(false))
        .unwrap();
    let seen = seen.into_inner().unwrap();
    let Some(Progress::Bytes { done, total }) = seen.last().cloned() else { panic!("no progress: {seen:?}") };
    assert_eq!(done, total);
    assert!(total >= 3_000_000, "{total}");
}

#[test]
fn outside_the_home_network_gets_a_plain_reason() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &["--trust", "10.99.0.0/16"]);
    let packed = packed_folder(tmp.path());
    let err = sender(tmp.path(), helper.port).send(&packed, &target(&helper), &|_| {}, &AtomicBool::new(false)).unwrap_err();
    assert_eq!(err, SendError::Failed("The Kite helper only accepts computers on your home network.".into()));
}

#[test]
fn closed_port_is_unreachable() {
    let tmp = tempfile::tempdir().unwrap();
    let packed = packed_folder(tmp.path());
    let target = SendTarget { console: "127.0.0.1".into(), destination: "/data/homebrew".into() };
    let err = sender(tmp.path(), closed_port()).send(&packed, &target, &|_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(matches!(err, SendError::Unreachable(_)), "{err:?}");
}

#[test]
fn cancel_stops_the_send() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let image = tmp.path().join("big.ffpkg");
    std::fs::write(&image, noise(40_000_000, 4)).unwrap();
    let mut s = sender(tmp.path(), helper.port);
    s.bandwidth_cap = Some(10_000_000);
    let cancel = AtomicBool::new(false);
    let err = s
        .send(&image, &target(&helper), &|p| {
            if matches!(p, Progress::Bytes { done, .. } if done > 0) {
                cancel.store(true, Ordering::SeqCst);
            }
        }, &cancel)
        .unwrap_err();
    assert_eq!(err, SendError::Cancelled);
}

#[test]
fn resumes_after_the_helper_restarts() {
    let tmp = tempfile::tempdir().unwrap();
    let console = tmp.path().join("console");
    let first = Helper::start(&console, &[]);
    let port = first.port;
    let target = target(&first);
    let image = tmp.path().join("big.ffpkg");
    let size = 48_000_000u64;
    std::fs::write(&image, noise(size as usize, 5)).unwrap();
    let mut s = sender(tmp.path(), port);
    s.bandwidth_cap = Some(12_000_000);

    // The PS5 goes away once a third of the file is safely written.
    let helper = Arc::new(Mutex::new(Some(first)));
    let err = s
        .send(&image, &target, &|p| {
            if matches!(p, Progress::Bytes { done, .. } if done >= size / 3)
                && let Some(h) = helper.lock().unwrap().take()
            {
                h.kill();
            }
        }, &AtomicBool::new(false))
        .unwrap_err();
    assert!(matches!(err, SendError::Unreachable(_)), "{err:?}");

    // It comes back on the same state: the send carries on instead of starting over.
    let _again = Helper::start_at(&console.join("state"), &console.join("ps5"), port, &[]);
    let first_done = Mutex::new(None);
    s.send(&image, &target, &|p| {
        if let Progress::Bytes { done, .. } = p {
            first_done.lock().unwrap().get_or_insert(done);
        }
    }, &AtomicBool::new(false))
    .unwrap();
    let resumed_from = first_done.into_inner().unwrap().unwrap_or(0);
    assert!(resumed_from >= size / 4, "it started over: first progress {resumed_from} of {size}");
    assert_eq!(std::fs::read(Path::new(&target.destination).join("big.ffpkg")).unwrap(), std::fs::read(&image).unwrap());
}

#[test]
fn a_port_in_the_console_address_is_used() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let packed = packed_folder(tmp.path());
    let mut target = target(&helper);
    target.console = format!("127.0.0.1:{}", helper.port);
    // The sender's own port is wrong; the one in the address wins.
    sender(tmp.path(), closed_port()).send(&packed, &target, &|_| {}, &AtomicBool::new(false)).unwrap();
    assert!(helper.share.join("homebrew/[Ashen Tide]-[PPSA01234]/ampr_zstd.index").is_file());
}

/// The app checks the PS5 every 10 seconds, sending or not. The helper keeps one session per
/// device and a new one from the same key replaces it, so the check must not dial as the
/// sender: that cut the send, which showed as Paused until the retry, again and again.
#[test]
fn a_status_check_doesnt_cut_a_send() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let image = tmp.path().join("big.ffpkg");
    std::fs::write(&image, noise(30_000_000, 6)).unwrap();
    // One set of keys for both, as the app has.
    let keys = Keys::open(&tmp.path().join("pc")).unwrap();
    let mut s = Ps5Sender::new(keys.clone(), helper.port, tmp.path().join("outboards"));
    s.bandwidth_cap = Some(10_000_000);
    let (moving, wait) = std::sync::mpsc::channel();
    let console = helper.console();
    let check = std::thread::spawn(move || {
        wait.recv().unwrap();
        kite_ps5::status(Some(&console), &keys)
    });
    let told = AtomicBool::new(false);
    let result = s.send(&image, &target(&helper), &|p| {
        if matches!(p, Progress::Bytes { done, .. } if done > 0) && !told.swap(true, Ordering::SeqCst) {
            let _ = moving.send(());
        }
    }, &AtomicBool::new(false));
    assert!(matches!(check.join().unwrap(), kite_ps5::Status::Running { .. }));
    result.expect("the send must carry on through a status check");
    assert_eq!(std::fs::read(helper.share.join("homebrew/big.ffpkg")).unwrap(), std::fs::read(&image).unwrap());
}
