use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};

const PARAM_JSON: &str = r#"{"titleId":"PPSA01234","contentId":"UP0000-PPSA01234_00-ASHENTIDE0000000","contentVersion":"01.050.000","requiredSystemSoftwareVersion":"0x0700000000000000","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Ashen Tide"}}}"#;

fn make_game(dir: &Path) -> PathBuf {
    let root = dir.join("Ashen Tide");
    fs::create_dir_all(root.join("sce_sys")).unwrap();
    fs::write(root.join("eboot.bin"), vec![7u8; 4096]).unwrap();
    fs::write(root.join("sce_sys/param.json"), PARAM_JSON).unwrap();
    fs::write(root.join("level.bin"), b"AB".repeat(100_000)).unwrap();
    root
}

/// The command, with its saved jobs and (on Linux) its Trash inside `tmp`.
fn kite(tmp: &Path) -> Command {
    let mut cmd = Command::cargo_bin("kite-pack").unwrap();
    cmd.env("KITE_PACK_DATA", tmp.join("data"));
    cmd.env("XDG_DATA_HOME", tmp.join("xdg"));
    cmd
}

#[test]
fn inspect_prints_the_title() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path());
    let out = kite(tmp.path()).arg("inspect").arg(&game).assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("Ashen Tide"), "{stdout}");
    assert!(stdout.contains("PPSA01234"), "{stdout}");
}

#[test]
fn inspect_refuses_a_ps4_package() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = tmp.path().join("ps4.pkg");
    let mut bytes = b"\x7FCNT".to_vec();
    bytes.extend(vec![0u8; 4096]);
    fs::write(&pkg, bytes).unwrap();
    let out = kite(tmp.path()).arg("inspect").arg(&pkg).assert().code(2);
    let all = String::from_utf8_lossy(&out.get_output().stdout).to_string()
        + &String::from_utf8_lossy(&out.get_output().stderr);
    assert!(all.contains("PS4 games can't be packed"), "{all}");
}

#[test]
fn pack_writes_a_packed_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path());
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    kite(tmp.path()).arg("pack").arg(&game).arg("--out").arg(&out).assert().success();
    let entries: Vec<PathBuf> = fs::read_dir(&out).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert!(entries[0].join("ampr_zstd.index").is_file());
    assert!(game.exists());
    let jobs = kite(tmp.path()).arg("jobs").assert().success();
    assert!(String::from_utf8_lossy(&jobs.get_output().stdout).contains("No unfinished jobs"));
}

#[test]
fn pack_with_delete_original() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path());
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    kite(tmp.path())
        .arg("pack")
        .arg(&game)
        .arg("--out")
        .arg(&out)
        .arg("--delete-original")
        .assert()
        .success();
    assert!(!game.exists(), "the original should be in the Trash");
    assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
}

#[test]
fn bad_level_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path());
    kite(tmp.path()).arg("pack").arg(&game).arg("--out").arg(tmp.path()).arg("--level").arg("huge").assert().failure();
}

#[test]
fn refused_source_packs_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let dlc = tmp.path().join("dlc");
    fs::create_dir_all(dlc.join("sce_sys")).unwrap();
    fs::write(dlc.join("sce_sys/param.json"), r#"{"titleId":"PPSA01234","contentId":"UP0000-PPSA01234_00-TIDEBREAKER00000"}"#).unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    kite(tmp.path()).arg("pack").arg(&dlc).arg("--out").arg(&out).assert().code(2);
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn resume_from_another_folder_uses_the_paths_typed_at_pack_time() {
    let tmp = tempfile::tempdir().unwrap();
    let work = tmp.path().join("work");
    make_game(&work);
    // A file where the output folder should be: the pack fails and the job stays open.
    fs::write(work.join("out"), b"in the way").unwrap();
    kite(tmp.path())
        .current_dir(&work)
        .args(["pack", "Ashen Tide", "--out", "out"])
        .assert()
        .code(1);
    fs::remove_file(work.join("out")).unwrap();

    let listed = kite(tmp.path()).current_dir(tmp.path()).arg("jobs").assert().success();
    let stdout = String::from_utf8(listed.get_output().stdout.clone()).unwrap();
    let id = stdout.split_whitespace().next().unwrap().to_string();
    assert!(stdout.contains(&*work.join("Ashen Tide").to_string_lossy()), "{stdout}");

    kite(tmp.path()).current_dir(tmp.path()).args(["resume", &id]).assert().success();
    let packed: Vec<_> = fs::read_dir(work.join("out")).unwrap().collect();
    assert_eq!(packed.len(), 1);
    assert!(packed[0].as_ref().unwrap().path().join("ampr_zstd.index").is_file());
}
