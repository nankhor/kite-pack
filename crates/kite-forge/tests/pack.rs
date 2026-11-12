mod common;

use common::*;
use kite_forge::*;
use kite_pipeline::{Level, OutputFormat, PackError, PackRequest, Packer, Progress, Snapshot};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

fn request(source: &Path, out: &Path, format: OutputFormat) -> PackRequest {
    PackRequest { source: source.to_path_buf(), output_dir: out.to_path_buf(), format, level: Level::Fast }
}

fn pack(req: &PackRequest) -> Result<kite_pipeline::PackOutcome, PackError> {
    ForgePacker { full_verify: false }.pack(req, &|_| {}, &AtomicBool::new(false))
}

#[test]
fn packs_the_test_game_to_a_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide (EU) – é");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let before = Snapshot::take(&game).unwrap();

    let done = pack(&request(&game, &out, OutputFormat::Folder)).unwrap();

    assert!(done.output.starts_with(&out));
    assert!(done.output.join("ampr_zstd.index").is_file());
    assert!(done.output.join("eboot.bin").is_file());
    assert!(done.files > 0 && done.bytes > 0);
    assert!(!done.checks.is_empty());
    assert_eq!(inspect_game(&done.output).unwrap_err(), SourceProblem::AlreadyPacked);
    assert!(before.matches(&game).unwrap(), "the source must not change");
}

#[test]
fn second_pack_gets_a_new_name() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let first = pack(&request(&game, &out, OutputFormat::Folder)).unwrap();
    let second = pack(&request(&game, &out, OutputFormat::Folder)).unwrap();
    assert_ne!(first.output, second.output);
    assert!(second.output.file_name().unwrap().to_string_lossy().ends_with("-2"), "{second:?}");
    assert!(first.output.join("ampr_zstd.index").is_file());
}

#[test]
fn packs_to_ffpkg() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let done = pack(&request(&game, &out, OutputFormat::Ffpkg)).unwrap();
    assert_eq!(done.output.extension().unwrap(), "ffpkg");
    assert!(done.output.is_file());
}

#[test]
fn reports_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let seen = Mutex::new(Vec::new());
    ForgePacker { full_verify: false }
        .pack(&request(&game, &out, OutputFormat::Folder), &|p| seen.lock().unwrap().push(p), &AtomicBool::new(false))
        .unwrap();
    let seen = seen.into_inner().unwrap();
    assert!(seen.iter().any(|p| matches!(p, Progress::Bytes { total, .. } if *total > 0)), "{seen:?}");
    assert!(seen.contains(&Progress::Stage("verify".into())), "{seen:?}");
}

#[test]
fn cancel_stops_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let result = ForgePacker { full_verify: false }.pack(
        &request(&game, &out, OutputFormat::Folder),
        &|_| {},
        &AtomicBool::new(true),
    );
    assert_eq!(result.unwrap_err(), PackError::Cancelled);
    assert_eq!(std::fs::read_dir(&out).unwrap().count(), 0);
}

#[test]
fn unreadable_source_fails_with_forges_words() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let result = pack(&request(&tmp.path().join("nothing"), &out, OutputFormat::Folder));
    assert!(matches!(result, Err(PackError::Failed(ref why)) if !why.is_empty()), "{result:?}");
}
