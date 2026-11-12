mod common;

use common::*;
use kite_forge::*;

#[test]
fn reads_the_test_game() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    let info = inspect_game(&game).unwrap();
    assert_eq!(info.kind, "folder");
    assert_eq!(info.title.as_deref(), Some("Ashen Tide"));
    assert_eq!(info.title_id.as_deref(), Some("PPSA01234"));
    assert_eq!(info.version.as_deref(), Some("01.050.000"));
    assert!(info.dlcs.is_empty());
    assert!(info.findings.is_empty(), "{:?}", info.findings);
    assert_eq!(info.total_bytes, 4096 + PARAM_JSON.len() as u64 + 600_000 + 200_000);
}

#[test]
fn odd_folder_name_works() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "Ashen Tide (EU) – é");
    assert_eq!(inspect_game(&game).unwrap().title_id.as_deref(), Some("PPSA01234"));
}

#[test]
fn dlc_folder_is_not_a_game() {
    let tmp = tempfile::tempdir().unwrap();
    let dlc = make_dlc_folder(tmp.path(), "tidebreaker");
    assert_eq!(inspect_game(&dlc).unwrap_err(), SourceProblem::NotAGame);
}

#[test]
fn ps4_package_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(inspect_game(&make_ps4_pkg(tmp.path())).unwrap_err(), SourceProblem::Ps4Package);
}

#[test]
fn missing_path_is_unreadable() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        inspect_game(&tmp.path().join("nothing")).unwrap_err(),
        SourceProblem::Unreadable(_)
    ));
}

#[test]
fn messages_are_plain() {
    assert_eq!(SourceProblem::Ps4Package.message(), "PS4 games can't be packed");
    assert_eq!(SourceProblem::NotAGame.message(), "This is an update or a DLC on its own, not a game");
    assert_eq!(SourceProblem::AlreadyPacked.message(), "Already packed");
    assert_eq!(SourceProblem::Unreadable("bad header".into()).message(), "bad header");
}

#[test]
fn dlc_for_this_game_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let dlc = make_dlc_folder(tmp.path(), "tidebreaker");
    let info = check_dlc(&dlc, "PPSA01234").unwrap();
    assert_eq!(info.label, "TIDEBREAKER00000");
    assert_eq!(info.content_id, "UP0000-PPSA01234_00-TIDEBREAKER00000");
    assert_eq!(info.name.as_deref(), Some("Tidebreaker Expansion"));
}

#[test]
fn dlc_for_another_game_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let dlc = make_dlc_folder(tmp.path(), "tidebreaker");
    assert_eq!(check_dlc(&dlc, "PPSA99999").unwrap_err(), "This isn't DLC for PPSA99999");
}

#[test]
fn a_game_is_not_dlc() {
    let tmp = tempfile::tempdir().unwrap();
    let game = make_game(tmp.path(), "ashen");
    assert_eq!(check_dlc(&game, "PPSA01234").unwrap_err(), "This isn't DLC for PPSA01234");
}
