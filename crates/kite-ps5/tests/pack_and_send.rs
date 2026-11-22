//! A whole job, packed with Forge and sent to the helper built for this computer: a game big
//! enough for two pack volumes, so the first goes while the second is packed. Ignored by
//! default: it writes about 13 GB. Run with `cargo test -p kite-ps5 --test pack_and_send --
//! --ignored --nocapture`.
#![cfg(unix)]
mod support;

use kite_forge::ForgePacker;
use kite_pipeline::*;
use kite_ps5::{Keys, Ps5Sender};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use support::Helper;

const PARAM_JSON: &str = r#"{"titleId":"PPSA01234","contentId":"UP0000-PPSA01234_00-ASHENTIDE0000000","contentVersion":"01.050.000","requiredSystemSoftwareVersion":"0x0700000000000000","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Ashen Tide"}}}"#;

/// A game whose packed size passes one volume (4 GiB): incompressible bytes, written fast.
fn big_game(dir: &Path) -> PathBuf {
    let root = dir.join("Ashen Tide");
    std::fs::create_dir_all(root.join("sce_sys")).unwrap();
    std::fs::create_dir_all(root.join("data")).unwrap();
    std::fs::write(root.join("eboot.bin"), vec![7u8; 4096]).unwrap();
    std::fs::write(root.join("sce_sys/param.json"), PARAM_JSON).unwrap();
    let mut f = std::io::BufWriter::new(std::fs::File::create(root.join("data/world.bin")).unwrap());
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    let mut block = vec![0u8; 1 << 20];
    let mb: usize = std::env::var("KITE_E2E_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(4400);
    for _ in 0..mb {
        for b in block.chunks_exact_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            b.copy_from_slice(&x.to_le_bytes());
        }
        f.write_all(&block).unwrap();
    }
    f.flush().unwrap();
    root
}

fn same_tree(a: &Path, b: &Path) {
    for e in std::fs::read_dir(a).unwrap() {
        let p = e.unwrap().path();
        let rel = p.strip_prefix(a).unwrap();
        if p.is_dir() {
            same_tree(&p, &b.join(rel));
        } else {
            let (x, y) = (std::fs::metadata(&p).unwrap().len(), std::fs::metadata(b.join(rel)).unwrap().len());
            assert_eq!(x, y, "{}", rel.display());
            let same = std::process::Command::new("cmp").arg("-s").arg(&p).arg(b.join(rel)).status().unwrap();
            assert!(same.success(), "{} differs", rel.display());
        }
    }
}

#[test]
#[ignore]
fn a_two_volume_pack_sends_its_first_volume_while_packing() {
    two_volumes(ForgePacker::default(), false);
}

/// As Windows does it: the volume is copied out of Forge's folder and the copy sent.
#[test]
#[ignore]
fn a_two_volume_pack_sends_a_copy_of_its_first_volume_while_packing() {
    two_volumes(ForgePacker { full_verify: false, copy_parts: true }, true);
}

fn two_volumes(packer: ForgePacker, copied: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(&tmp.path().join("console"), &[]);
    let game = big_game(tmp.path());
    let out = tmp.path().join("out");
    let options = Options {
        format: OutputFormat::Folder,
        level: Level::Fast,
        output_dir: out.clone(),
        delete_original: false,
        send: Some(SendTarget { console: "127.0.0.1".into(), destination: helper.share.join("homebrew").display().to_string() }),
        delete_packed: false,
        dlc: vec![],
        bundle: Default::default(),
    };
    let mut job = Job::new(game, options).unwrap();
    let store = JobStore::open(tmp.path().join("jobs")).unwrap();
    let sender = Ps5Sender::new(Keys::open(&tmp.path().join("pc")).unwrap(), helper.port, tmp.path().join("outboards"));
    let runner = Runner {
        packer: &packer,
        trash: &SystemTrash,
        sender: &sender,
        installer: &Unavailable,
        sleeper: &ThreadSleeper,
        store: &store,
        retry_every: Duration::from_secs(1),
    };
    let started = Instant::now();
    let log = Mutex::new(Vec::new());
    let outcome = runner.run(&mut job, &AtomicBool::new(false), &|e| {
        let line = match e {
            RunEvent::Progress { progress: Progress::Stage(s), index } => format!("step {index}: {s}"),
            RunEvent::StepDone { index, note } => format!("step {index} done: {note}"),
            RunEvent::StepFailed { index, reason } => format!("step {index} failed: {reason}"),
            _ => return,
        };
        println!("{:>6.1}s {line}", started.elapsed().as_secs_f64());
        log.lock().unwrap().push(line);
    });
    assert_eq!(outcome, RunOutcome::Finished, "{:?}", log.lock().unwrap());
    assert!(!job.ahead.is_empty(), "volumes went while packing");
    assert_eq!(job.ahead[0].rel, "ampr_zstd-000.pak");
    println!("sent while packing: {:?}", job.ahead.iter().map(|p| &p.rel).collect::<Vec<_>>());

    let _ = copied;
    let packed = job.packed.clone().unwrap();
    let landed = helper.share.join("homebrew").join(packed.file_name().unwrap());
    same_tree(&packed, &landed);
    // Nothing of the links is left beside the output.
    let names: Vec<_> = std::fs::read_dir(&out).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, vec![packed.file_name().unwrap().to_os_string()], "{names:?}");
}
