mod common;

use common::*;
use kite_forge::*;
use kite_pipeline::{Compression, Level, OutputFormat, PackError, PackRequest, Packer, Progress, Snapshot};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

fn request(source: &Path, out: &Path, format: OutputFormat) -> PackRequest {
    PackRequest { source: source.to_path_buf(), output_dir: out.to_path_buf(), format, level: Level::Fast, compression: Default::default(), bundle: Default::default(), ahead: false, let_go: None }
}

fn pack(req: &PackRequest) -> Result<kite_pipeline::PackOutcome, PackError> {
    ForgePacker::default().pack(req, &|_| {}, &AtomicBool::new(false))
}

#[test]
fn packs_the_test_game_to_a_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide (EU) – é");
    let out = tmp.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let before = Snapshot::take(&game).unwrap();

    let done = pack(&request(&game, &out, OutputFormat::Folder)).unwrap();

    assert!(done.output.starts_with(dunce::canonicalize(&out).unwrap()), "{done:?}");
    assert!(done.output.join("ampr_zstd.index").is_file());
    assert!(done.output.join("eboot.bin").is_file());
    assert!(done.files > 0 && done.bytes > 0);
    assert!(!done.checks.is_empty());
    assert_eq!(inspect_game(&done.output).unwrap_err(), SourceProblem::AlreadyPacked);
    assert!(before.matches(&game).unwrap(), "the source must not change");
}

#[test]
fn packed_games_get_the_emulator_kite_pack_carries() {
    // ampr-emu-ng v0.5.0, the release vendored with Forge's runtime crate (third_party/forge-lz4).
    const RUNTIME: &[u8] = include_bytes!("../../../third_party/forge-lz4/ampr-emu-ng/libSceAmpr.sprx");
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    let out = tmp.path().join("out");

    let done = pack(&request(&game, &out, OutputFormat::Folder)).unwrap();

    let runtime = std::fs::read(done.output.join("fakelib/libSceAmpr.sprx")).unwrap();
    assert!(runtime == RUNTIME, "the packed game has another runtime ({} bytes)", runtime.len());
    assert!(runtime.windows(17).any(|w| w == b"ampr-emu-ng 0.5.0"));
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
    ForgePacker::default()
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
    let result = ForgePacker::default().pack(
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

#[test]
fn creates_a_missing_output_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    let out = tmp.path().join("not yet").join("out");
    let done = pack(&request(&game, &out, OutputFormat::Folder)).unwrap();
    assert!(done.output.starts_with(dunce::canonicalize(&out).unwrap()), "{done:?}");
    assert!(done.output.join("ampr_zstd.index").is_file());
}

fn convert(source: &Path, out: &Path, compression: Compression) -> kite_pipeline::PackOutcome {
    let req = PackRequest { compression, ..request(source, out, OutputFormat::Pkg) };
    pack(&req).unwrap()
}

#[test]
fn converts_to_a_fake_package_sony_s_installer_takes() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    let out = tmp.path().join("Packages");
    let before = Snapshot::take(&game).unwrap();

    let done = convert(&game, &out, Compression::Fast);

    assert_eq!(done.output.extension().unwrap(), "pkg", "{done:?}");
    assert!(done.output.is_file());
    assert!(done.output.starts_with(dunce::canonicalize(&out).unwrap()), "{done:?}");
    let info = inspect_package(&done.output).unwrap();
    assert_eq!((info.platform.as_str(), info.kind.as_str()), ("ps5", "game"));
    assert_eq!(info.content_id, "UP0000-PPSA01234_00-ASHENTIDE0000000");
    assert!(before.matches(&game).unwrap(), "the dump must not change");
}

#[test]
fn every_compression_level_makes_a_package_and_off_is_the_biggest() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide");
    let out = tmp.path().join("Packages");
    let size = |p: &Path| std::fs::metadata(p).unwrap().len();

    let off = size(&convert(&game, &out, Compression::Off).output);
    for level in [Compression::Fast, Compression::Balanced, Compression::Smallest] {
        let done = convert(&game, &out, level);
        assert_eq!(inspect_package(&done.output).unwrap().content_id, "UP0000-PPSA01234_00-ASHENTIDE0000000", "{level:?}");
        assert!(size(&done.output) < off, "{level:?}: {} vs {off}", size(&done.output));
    }
}

#[test]
fn each_level_is_forge_s_own() {
    assert_eq!(
        [Compression::Off, Compression::Fast, Compression::Balanced, Compression::Smallest].map(kraken_level),
        ["stored", "fast", "balanced", "smallest"]
    );
}
