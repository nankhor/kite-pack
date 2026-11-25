//! A pack that lets parts go, through Forge, in a test binary of its own: the hook is process-wide.
mod common;

use common::*;
use kite_forge::*;
use kite_pipeline::{LetGo, Level, OutputFormat, PackRequest, Packer};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

fn request(game: &std::path::Path, out: &std::path::Path, let_go: Option<Arc<LetGo>>) -> PackRequest {
    PackRequest {
        source: game.to_path_buf(),
        output_dir: out.to_path_buf(),
        format: OutputFormat::Folder,
        level: Level::Fast,
        compress: false,
        bundle: Default::default(),
        ahead: true,
        let_go,
    }
}

#[test]
fn packs_the_same_with_letting_go() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    // Whether Forge's packer had the hook while it packed.
    let pack = |out: &str, let_go| {
        let out = tmp.path().join(out);
        std::fs::create_dir_all(&out).unwrap();
        let hooked = AtomicBool::new(false);
        let seen = |_| {
            if ps5_dump_forge_zstd::hook::volume_hook().is_some() {
                hooked.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        };
        let done = ForgePacker::default().pack(&request(&game, &out, let_go), &seen, &AtomicBool::new(false)).unwrap();
        (done, hooked.into_inner())
    };
    let (plain, hooked) = pack("plain", None);
    assert!(!hooked);
    let let_go = Arc::new(LetGo::default());
    let (went, hooked) = pack("let-go", Some(let_go));
    assert!(hooked, "letting go, the packer goes through the hook");
    assert!(ps5_dump_forge_zstd::hook::volume_hook().is_none(), "the hook goes with the pack");

    fn walk(root: &std::path::Path, dir: &std::path::Path, files: &mut Vec<(std::path::PathBuf, u64)>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.file_type().unwrap().is_dir() {
                walk(root, &e.path(), files);
            } else {
                files.push((e.path().strip_prefix(root).unwrap().to_path_buf(), e.metadata().unwrap().len()));
            }
        }
    }
    let listing = |dir: &std::path::Path| {
        let mut files = Vec::new();
        walk(dir, dir, &mut files);
        files.sort();
        files
    };
    assert_eq!(listing(&plain.output), listing(&went.output));
    let volume = "ampr_zstd-000.pak";
    let (a, b) = (std::fs::read(plain.output.join(volume)).unwrap(), std::fs::read(went.output.join(volume)).unwrap());
    assert!(a[64..] == b[64..], "the same packed bytes");
    assert_eq!(plain.files, went.files);
}
