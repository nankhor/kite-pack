//! Fake PKGs (debug `.pkg` packages, as Forge and ps5upload build them) as a game to pack.
mod common;

use common::*;
use kite_forge::*;
use kite_pipeline::{Level, OutputFormat, PackRequest, Packer, Snapshot};
use std::sync::atomic::AtomicBool;

fn pack(source: &std::path::Path, out: &std::path::Path, format: OutputFormat) -> kite_pipeline::PackOutcome {
    std::fs::create_dir_all(out).unwrap();
    let req = PackRequest { source: source.to_path_buf(), output_dir: out.to_path_buf(), format, level: Level::Fast };
    ForgePacker { full_verify: false }.pack(&req, &|_| {}, &AtomicBool::new(false)).unwrap()
}

#[test]
fn a_fake_pkg_reads_like_the_game_inside_it() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = make_fake_pkg(tmp.path());
    let info = inspect_game(&pkg).unwrap();
    assert_eq!(info.kind, "pkg");
    assert_eq!(info.title.as_deref(), Some("Ashen Tide"));
    assert_eq!(info.title_id.as_deref(), Some("PPSA01234"));
    assert_eq!(info.content_id.as_deref(), Some("UP0000-PPSA01234_00-ASHENTIDE0000000"));
    assert_eq!(info.version.as_deref(), Some("01.050.000"));
    assert!(info.total_bytes > 0);
}

#[test]
fn a_fake_pkg_packs_to_a_folder_and_stays_as_it_was() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = make_fake_pkg(tmp.path());
    // What Delete the original compares against before it moves the .pkg to the Trash.
    let before = Snapshot::take(&pkg).unwrap();
    let done = pack(&pkg, &tmp.path().join("out"), OutputFormat::Folder);
    assert!(done.output.join("ampr_zstd.index").is_file(), "{done:?}");
    assert!(done.output.join("eboot.bin").is_file());
    assert!(done.output.join("sce_sys/param.json").is_file());
    assert_eq!(inspect_game(&done.output).unwrap_err(), SourceProblem::AlreadyPacked);
    assert!(before.matches(&pkg).unwrap(), "the .pkg must not change");
}

#[test]
fn a_fake_pkg_packs_to_ffpkg() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = make_fake_pkg(tmp.path());
    let done = pack(&pkg, &tmp.path().join("out"), OutputFormat::Ffpkg);
    assert_eq!(done.output.extension().unwrap(), "ffpkg");
    assert!(done.output.is_file());
}

#[test]
fn a_retail_package_is_refused_in_plain_words() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = make_fake_pkg(tmp.path());
    // The header's signed byte (offset 5) is 0x00 in a debug package and set in a retail one.
    let mut bytes = std::fs::read(&pkg).unwrap();
    bytes[5] = 0x01;
    let retail = tmp.path().join("retail.pkg");
    std::fs::write(&retail, bytes).unwrap();
    let problem = inspect_game(&retail).unwrap_err();
    assert_eq!(problem, SourceProblem::RetailPackage);
    assert_eq!(problem.message(), "Retail packages can't be read");
}
