//! Tiny fake PS5 games and packages that Forge reads like real ones.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

/// Deterministic filler bytes (a linear congruential generator), so packing results repeat.
fn noise(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed;
    (0..len)
        .map(|_| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (x >> 24) as u8
        })
        .collect()
}

pub const PARAM_JSON: &str = r#"{"titleId":"PPSA01234","contentId":"UP0000-PPSA01234_00-ASHENTIDE0000000","contentVersion":"01.050.000","requiredSystemSoftwareVersion":"0x0700000000000000","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Ashen Tide"}}}"#;

/// A game folder named `name` inside `dir`: eboot.bin, param.json and two data files.
pub fn make_game(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(root.join("sce_sys")).unwrap();
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join("eboot.bin"), noise(4096, 1)).unwrap();
    fs::write(root.join("sce_sys/param.json"), PARAM_JSON).unwrap();
    fs::write(root.join("data/level1.bin"), b"AB".repeat(300_000)).unwrap();
    fs::write(root.join("data/audio.bnk"), noise(200_000, 2)).unwrap();
    root
}

/// A DLC folder for PPSA01234: only its param.json, no eboot.bin.
pub fn make_dlc_folder(dir: &Path, name: &str) -> PathBuf {
    let root = dir.join(name);
    fs::create_dir_all(root.join("sce_sys")).unwrap();
    fs::write(
        root.join("sce_sys/param.json"),
        r#"{"titleId":"PPSA01234","contentId":"UP0000-PPSA01234_00-TIDEBREAKER00000","contentVersion":"01.000.000","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Tidebreaker Expansion"}}}"#,
    )
    .unwrap();
    root
}

/// A file that starts like a PS4 package.
pub fn make_ps4_pkg(dir: &Path) -> PathBuf {
    let path = dir.join("ps4.pkg");
    let mut bytes = b"\x7FCNT".to_vec();
    bytes.extend(std::iter::repeat_n(0u8, 4096));
    fs::write(&path, bytes).unwrap();
    path
}
