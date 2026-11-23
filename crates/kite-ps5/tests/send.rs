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

// ---- parts sent while packing ----------------------------------------------------------------

use kite_pipeline::Part;

/// A packed folder whose first volume went ahead while packing: `part` holds that volume as it
/// was then, its 64-byte header still zeros; the folder holds it as finished.
fn packed_with_a_part_ahead(dir: &Path) -> (PathBuf, PathBuf, Part) {
    let packed = packed_folder(dir);
    let mut volume = std::fs::read(packed.join("ampr_zstd-000.pak")).unwrap();
    volume[..64].fill(0);
    let part_dir = dir.join(".[Ashen Tide]-[PPSA01234].kite-ahead");
    std::fs::create_dir_all(&part_dir).unwrap();
    std::fs::write(part_dir.join("ampr_zstd-000.pak"), &volume).unwrap();
    let part = Part { rel: "ampr_zstd-000.pak".into(), size: volume.len() as u64, settled_from: 64 };
    (packed, part_dir.join("ampr_zstd-000.pak"), part)
}

#[test]
fn a_part_sent_ahead_is_left_out_and_its_header_put_right() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let (packed, early, part) = packed_with_a_part_ahead(tmp.path());
    let s = sender(tmp.path(), helper.port);
    let t = target(&helper);
    let landed = helper.share.join("homebrew/[Ashen Tide]-[PPSA01234]");

    assert!(s.send_ahead(&packed, &early, &part, &t, &|_| {}, &AtomicBool::new(false)).unwrap());
    let there = std::fs::read(landed.join("ampr_zstd-000.pak")).unwrap();
    assert_eq!(&there[..64], &[0u8; 64][..], "it went with the header it had then");

    let totals = Mutex::new(Vec::new());
    s.send_rest(&packed, &t, &[part.clone()], &|p| {
        if let Progress::Bytes { total, .. } = p {
            totals.lock().unwrap().push(total);
        }
    }, &AtomicBool::new(false))
    .unwrap();

    same_tree(&packed, &landed);
    let total = *totals.lock().unwrap().last().unwrap();
    assert!(total < part.size, "the volume wasn't sent again ({total} bytes went)");
}

#[test]
fn a_part_gone_from_the_console_goes_again() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let (packed, early, part) = packed_with_a_part_ahead(tmp.path());
    let s = sender(tmp.path(), helper.port);
    let t = target(&helper);
    let landed = helper.share.join("homebrew/[Ashen Tide]-[PPSA01234]");

    assert!(s.send_ahead(&packed, &early, &part, &t, &|_| {}, &AtomicBool::new(false)).unwrap());
    std::fs::remove_file(landed.join("ampr_zstd-000.pak")).unwrap();
    s.send_rest(&packed, &t, &[part], &|_| {}, &AtomicBool::new(false)).unwrap();
    same_tree(&packed, &landed);
}

#[test]
fn a_part_that_changed_size_goes_again() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let (packed, early, part) = packed_with_a_part_ahead(tmp.path());
    let s = sender(tmp.path(), helper.port);
    let t = target(&helper);
    assert!(s.send_ahead(&packed, &early, &part, &t, &|_| {}, &AtomicBool::new(false)).unwrap());
    // The finished volume isn't the one announced: never trust what went ahead then.
    std::fs::write(packed.join("ampr_zstd-000.pak"), noise(2_000_000, 9)).unwrap();
    s.send_rest(&packed, &t, &[part], &|_| {}, &AtomicBool::new(false)).unwrap();
    same_tree(&packed, &helper.share.join("homebrew/[Ashen Tide]-[PPSA01234]"));
}

#[test]
fn a_part_cut_to_its_header_is_patched_not_sent() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let (packed, early, part) = packed_with_a_part_ahead(tmp.path());
    let s = sender(tmp.path(), helper.port);
    let t = target(&helper);
    let landed = helper.share.join("homebrew/[Ashen Tide]-[PPSA01234]");
    assert!(s.send_ahead(&packed, &early, &part, &t, &|_| {}, &AtomicBool::new(false)).unwrap());

    // The PC let the part go once the PS5 had it: only its final header is left there.
    let finished = std::fs::read(packed.join("ampr_zstd-000.pak")).unwrap();
    std::fs::write(packed.join("ampr_zstd-000.pak"), &finished[..64]).unwrap();
    let totals = Mutex::new(Vec::new());
    s.send_rest(&packed, &t, &[part.clone()], &|p| {
        if let Progress::Bytes { total, .. } = p {
            totals.lock().unwrap().push(total);
        }
    }, &AtomicBool::new(false))
    .unwrap();

    let there = std::fs::read(landed.join("ampr_zstd-000.pak")).unwrap();
    assert_eq!(there.len() as u64, part.size, "the PS5 keeps the whole part");
    assert!(there == finished, "with the final header on it");
    assert!(landed.join("ampr_zstd.index").is_file(), "the rest went");
    let total = *totals.lock().unwrap().last().unwrap();
    assert!(total < part.size, "the part wasn't sent again ({total} bytes went)");
}

// ---- the helper's one write ------------------------------------------------------------------

fn keys(dir: &Path) -> Keys {
    Keys::open(&dir.join("pc")).unwrap()
}

#[test]
fn a_patch_writes_inside_an_existing_file_and_nowhere_else() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let (console, k) = (helper.console(), keys(tmp.path()));
    let file = helper.share.join("game.pak");
    std::fs::write(&file, vec![7u8; 100]).unwrap();
    let path = file.display().to_string();

    kite_ps5::patch_file(&console, &k, &path, 10, b"kite").unwrap();
    let after = std::fs::read(&file).unwrap();
    assert_eq!(after.len(), 100, "never grown");
    assert_eq!(&after[10..14], b"kite");
    assert!(after[..10].iter().chain(&after[14..]).all(|&b| b == 7));

    // Never past the end, so never grown.
    let past = kite_ps5::patch_file(&console, &k, &path, 98, b"kite").unwrap_err();
    assert!(matches!(past, kite_ps5::MetaError::Refused { code: 41, .. }), "{past:?}");
    assert_eq!(std::fs::read(&file).unwrap(), after);
    // Never makes a file.
    let missing = helper.share.join("new.pak").display().to_string();
    assert!(kite_ps5::patch_file(&console, &k, &missing, 0, b"x").unwrap_err().is_not_found());
    assert!(!helper.share.join("new.pak").exists());
    // Never outside the folders transfers may write, not even through a link.
    let outside = tmp.path().join("outside.bin");
    std::fs::write(&outside, vec![1u8; 100]).unwrap();
    let refused = kite_ps5::patch_file(&console, &k, &outside.display().to_string(), 0, b"x").unwrap_err();
    assert_eq!(refused, kite_ps5::MetaError::Path);
    std::os::unix::fs::symlink(&outside, helper.share.join("link.pak")).unwrap();
    let through = helper.share.join("link.pak").display().to_string();
    assert_eq!(kite_ps5::patch_file(&console, &k, &through, 0, b"x").unwrap_err(), kite_ps5::MetaError::Path);
    assert_eq!(std::fs::read(&outside).unwrap(), vec![1u8; 100]);
    // Never a folder, and never more than 4 KiB at once.
    let folder = helper.share.display().to_string();
    assert_eq!(kite_ps5::patch_file(&console, &k, &folder, 0, b"x").unwrap_err(), kite_ps5::MetaError::Path);
    assert!(kite_ps5::patch_file(&console, &k, &path, 0, &[0u8; 4097]).is_err());
}
