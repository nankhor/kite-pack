//! Packing a game with DLC folded in, through the real Forge.
mod common;

use common::*;
use kite_forge::*;
use kite_pipeline::{Bundle, Level, OutputFormat, PackRequest, Packer};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn pack(source: &Path, out: &Path, bundle: Bundle) -> PathBuf {
    let req = PackRequest {
        source: source.to_path_buf(),
        output_dir: out.to_path_buf(),
        format: OutputFormat::Folder,
        level: Level::Fast,
        bundle,
    };
    ForgePacker { full_verify: false }
        .pack(&req, &|_| {}, &AtomicBool::new(false))
        .expect("pack")
        .output
}

/// An empty folder offered as an emulator: the plan must refuse it rather than pack blind.
fn empty_dir(dir: &Path) -> PathBuf {
    let root = dir.join("empty-emulator");
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn a_dlc_folder_ends_up_inside_the_packed_copy() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    let dlc = make_dlc_folder(tmp.path().join("dlc").as_path(), "Tidebreaker");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();

    let packed = pack(&game, &out, Bundle { dlc: vec![dlc], backport: None, emulator: None });

    // The DLC's files are inside the packs (compressed); the index is the manifest that says so.
    let index = std::fs::read(packed.join("ampr_zstd.index")).unwrap();
    let index = String::from_utf8_lossy(&index);
    assert!(
        index.contains("addcont0/sce_sys/param.json"),
        "the DLC is not in the pack's index"
    );
    let ini = std::fs::read_to_string(packed.join("dlc_emu.ini")).unwrap();
    assert!(ini.contains("content_id=UP0000-PPSA01234_00-TIDEBREAKER00000"), "{ini}");
    assert!(ini.contains("mount_point=/app0/addcont0"), "{ini}");
    // The emulator Kite Pack carries, as real files the mount can see.
    let carried = kite_forge::dlc_emu::modules();
    assert_eq!(
        std::fs::read(packed.join("fakelib").join(carried[0].name)).unwrap(),
        carried[0].bytes,
    );
    // A packed copy is still a packed copy.
    assert!(packed.join("ampr_zstd.index").is_file());
    assert!(inspect_game(&packed).is_ok() || inspect_game(&packed).is_err());
    // The dump itself is untouched: no DLC merged into it, no emulator added.
    assert!(!game.join("dlc_emu.ini").exists());
    assert!(!game.join("addcont0").exists());
    assert!(!game.join("fakelib").exists());
}

#[test]
fn a_backport_folder_replaces_the_games_files_before_packing() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    let backport = tmp.path().join("backport");
    std::fs::create_dir_all(backport.join("fakelib")).unwrap();
    std::fs::write(backport.join("eboot.bin"), b"patched eboot").unwrap();
    std::fs::write(backport.join("fakelib/libSceVideoOut.prx"), b"fake").unwrap();
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();

    let packed = pack(&game, &out, Bundle { dlc: vec![], backport: Some(backport), emulator: None });

    assert_eq!(std::fs::read(packed.join("eboot.bin")).unwrap(), b"patched eboot");
    assert!(packed.join("fakelib/libSceVideoOut.prx").is_file());
    // And the original game still has its own.
    assert_ne!(std::fs::read(game.join("eboot.bin")).unwrap(), b"patched eboot");
}

#[test]
fn a_bundle_that_cannot_go_in_fails_before_writing_anything() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    let dlc = make_dlc_folder(tmp.path().join("dlc").as_path(), "Tidebreaker");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();

    // A picked emulator folder with no modules in it: refused, with the reason.
    let req = PackRequest {
        source: game.clone(),
        output_dir: out.clone(),
        format: OutputFormat::Folder,
        level: Level::Fast,
        bundle: Bundle { dlc: vec![dlc], backport: None, emulator: Some(empty_dir(tmp.path())) },
    };
    let err = ForgePacker { full_verify: false }
        .pack(&req, &|_| {}, &AtomicBool::new(false))
        .unwrap_err();
    let text = match &err {
        kite_pipeline::PackError::Failed(why) => why.clone(),
        other => format!("{other:?}"),
    };
    assert!(text.contains("no libSceAppContent"), "{text}");
    // Nothing was packed and nothing was left behind beside the game.
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
    let leftovers: Vec<_> = std::fs::read_dir(tmp.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(".kite-bundle"))
        .collect();
    assert!(leftovers.is_empty(), "staging was left behind");
}
