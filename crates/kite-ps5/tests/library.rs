//! What the console has installed, read through a real helper started with a test root.
#![cfg(unix)]
mod support;

use kite_ps5::{free_space, installed, installed_title, icons, measure, read_file, Keys};
use std::path::{Path, PathBuf};
use support::Helper;

/// A console tree on this computer, which the helper is told is `/`.
fn console_root(tmp: &Path) -> PathBuf {
    let root = tmp.join("console");
    for d in ["user/appmeta", "user/app", "user/patch", "user/addcont"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    root
}

fn write(path: impl AsRef<Path>, bytes: &[u8]) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn param(name: &str, version: &str) -> String {
    format!(
        r#"{{"localizedParameters":{{"defaultLanguage":"en-US","en-US":{{"titleName":"{name}"}}}},
            "contentVersion":"{version}"}}"#
    )
}

/// A title with an appmeta folder, an app folder, an update and two add-ons.
fn full_game(root: &Path, id: &str, name: &str, version: &str) {
    write(root.join(format!("user/appmeta/{id}/param.json")), param(name, version).as_bytes());
    write(root.join(format!("user/appmeta/{id}/icon0.png")), &png());
    write(root.join(format!("user/app/{id}/sce_sys/param.json")), param(name, version).as_bytes());
    write(root.join(format!("user/app/{id}/eboot.bin")), &[0u8; 1000]);
    std::fs::create_dir_all(root.join(format!("user/patch/{id}"))).unwrap();
    write(root.join(format!("user/patch/{id}/patch.pkg")), &[0u8; 500]);
    for label in ["DLC00001", "DLC00002"] {
        write(root.join(format!("user/addcont/{id}/{label}/ac.pkg")), &[0u8; 250]);
    }
}

fn png() -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(&[0u8; 32]);
    v
}

fn keys(tmp: &Path) -> Keys {
    Keys::open(&tmp.join("pc")).unwrap()
}

fn helper(tmp: &Path, extra: &[&str]) -> (Helper, PathBuf) {
    let root = console_root(tmp);
    let root_text = root.to_str().unwrap().to_string();
    let mut args = vec!["--root", root_text.as_str()];
    args.extend_from_slice(extra);
    let helper = Helper::start(&tmp.join("ps5"), &args);
    (helper, root)
}

#[test]
fn lists_installed_games_with_names_versions_updates_and_dlc() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    full_game(&root, "PPSA00001", "Ashen Tide", "01.050.000");
    write(root.join("user/app/PPSA00002/eboot.bin"), &[0u8; 10]);
    write(
        root.join("user/app/PPSA00002/sce_sys/param.json"),
        param("Second Wind", "01.000.000").as_bytes(),
    );
    let (helper, _) = helper(tmp.path(), &[]);
    let k = keys(tmp.path());

    let lib = installed(&helper.console(), &k).unwrap();

    assert!(!lib.truncated);
    let ids: Vec<&str> = lib.games.iter().map(|g| g.title_id.as_str()).collect();
    assert_eq!(ids, ["PPSA00001", "PPSA00002"]);

    let first = &lib.games[0];
    assert_eq!(first.title, "Ashen Tide");
    assert_eq!(first.version.as_deref(), Some("01.050.000"));
    assert!(first.update_installed);
    assert_eq!(first.dlc, ["DLC00001", "DLC00002"]);
    assert!(!first.external);
    assert_eq!(first.found, "appmeta+app");
    assert_eq!(first.dlc_count(), 2);

    // An app folder with no appmeta still lists, named from its own sce_sys.
    let second = &lib.games[1];
    assert_eq!(second.title, "Second Wind");
    assert!(!second.update_installed);
    assert!(second.dlc.is_empty());
    assert_eq!(second.found, "app");
}

#[test]
fn a_title_without_a_param_json_shows_its_id_and_a_console_app_is_not_a_game() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    write(root.join("user/appmeta/PPSA00009/icon0.png"), &png());
    // Sony's own entries and the helper's tile are not games.
    for id in ["NPXS39041", "PSUP00001"] {
        write(root.join(format!("user/appmeta/{id}/param.json")), param("Not a game", "01.000.000").as_bytes());
    }
    // A folder that is not a title id at all.
    std::fs::create_dir_all(root.join("user/appmeta/not-a-title")).unwrap();
    let (helper, _) = helper(tmp.path(), &[]);

    let lib = installed(&helper.console(), &keys(tmp.path())).unwrap();

    assert_eq!(lib.games.len(), 1);
    assert_eq!(lib.games[0].title_id, "PPSA00009");
    assert_eq!(lib.games[0].title, "PPSA00009");
    assert_eq!(lib.games[0].version, None);
}

#[test]
fn a_title_on_an_extended_drive_is_marked_external() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    write(root.join("mnt/ext0/user/app/PPSA00010/sce_sys/param.json"), param("On the SSD", "01.000.000").as_bytes());
    let (helper, _) = helper(tmp.path(), &[]);

    let lib = installed(&helper.console(), &keys(tmp.path())).unwrap();

    assert_eq!(lib.games.len(), 1);
    assert!(lib.games[0].external);
    assert_eq!(lib.games[0].title, "On the SSD");
}

#[test]
fn reads_the_tile_as_a_png_data_url_and_skips_a_title_with_none() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    full_game(&root, "PPSA00001", "Ashen Tide", "01.000.000");
    write(root.join("user/app/PPSA00002/eboot.bin"), &[0u8; 10]);
    let (helper, _) = helper(tmp.path(), &[]);

    let got = icons(
        &helper.console(),
        &keys(tmp.path()),
        &["PPSA00001".into(), "PPSA00002".into(), "not-an-id".into()],
    )
    .unwrap();

    assert_eq!(got.len(), 1);
    assert_eq!(got[0].title_id, "PPSA00001");
    assert!(got[0].data_url.starts_with("data:image/png;base64,iVBORw0KGgo"));
}

#[test]
fn measures_app_update_and_dlc_separately() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    full_game(&root, "PPSA00001", "Ashen Tide", "01.000.000");
    let (helper, _) = helper(tmp.path(), &[]);

    let m = measure(&helper.console(), &keys(tmp.path()), "PPSA00001").unwrap();

    assert_eq!(m.update_bytes, 500);
    assert_eq!(m.dlc_bytes, 500);
    // The app folder also holds the sce_sys copy of param.json full_game wrote.
    assert!(m.app_bytes > 1000, "{}", m.app_bytes);
    assert_eq!(m.bytes, m.app_bytes + 500 + 500);
    assert!(!m.partial);
}

#[test]
fn a_title_that_is_not_there_takes_no_space() {
    let tmp = tempfile::tempdir().unwrap();
    console_root(tmp.path());
    let (helper, _) = helper(tmp.path(), &[]);

    let m = measure(&helper.console(), &keys(tmp.path()), "PPSA00001").unwrap();

    assert_eq!(m.bytes, 0);
}

#[test]
fn pages_through_more_dlc_folders_than_one_page_holds() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    full_game(&root, "PPSA00001", "Ashen Tide", "01.000.000");
    for i in 0..300 {
        write(root.join(format!("user/addcont/PPSA00001/LABEL{i:04}/ac.pkg")), &[0u8; 1]);
    }
    let (helper, _) = helper(tmp.path(), &[]);

    let lib = installed(&helper.console(), &keys(tmp.path())).unwrap();

    assert_eq!(lib.games.len(), 1);
    assert_eq!(lib.games[0].dlc.len(), 302);
    assert!(lib.games[0].dlc.contains(&"LABEL0299".to_string()));
}

#[test]
fn reads_a_file_and_refuses_one_outside_the_read_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    write(root.join("user/appmeta/PPSA00001/param.json"), b"hello");
    // A file beside the console folders: it exists, but no read root covers it.
    write(root.join("secret.txt"), b"not yours");
    let (helper, _) = helper(tmp.path(), &[]);
    let console = helper.console();
    let k = keys(tmp.path());

    let got = read_file(&console, &k, "/user/appmeta/PPSA00001/param.json", 1024).unwrap();
    assert_eq!(got, b"hello");

    // Not under a write root or a console title folder: the helper must refuse, and say so.
    let err = read_file(&console, &k, "/secret.txt", 1024).unwrap_err();
    assert!(err.message().contains("won't read"), "{}", err.message());

    // A traversal is refused before any path is joined.
    let err = read_file(&console, &k, "/user/appmeta/../../etc/passwd", 1024).unwrap_err();
    assert!(err.message().contains("won't read"), "{}", err.message());
}

#[test]
fn reports_the_room_left_where_games_go() {
    let tmp = tempfile::tempdir().unwrap();
    console_root(tmp.path());
    let (helper, _) = helper(tmp.path(), &[]);

    let free = free_space(&helper.console(), &keys(tmp.path()), "/user/app").unwrap();

    assert!(free.total > 0);
    assert!(free.free <= free.total);
    assert!(free.usable <= free.free);
    assert_eq!(free.reserve, (free.total / 64).min(1024 * 1024 * 1024));
}

#[test]
fn reads_one_title_by_id_for_the_game_screen() {
    let tmp = tempfile::tempdir().unwrap();
    let root = console_root(tmp.path());
    full_game(&root, "PPSA00001", "Ashen Tide", "01.050.000");
    let (helper, _) = helper(tmp.path(), &[]);
    let console = helper.console();
    let k = keys(tmp.path());

    let got = installed_title(&console, &k, "PPSA00001").unwrap().unwrap();
    assert_eq!(got.title, "Ashen Tide");
    assert_eq!(got.version.as_deref(), Some("01.050.000"));
    assert!(got.update_installed);
    assert_eq!(got.dlc, ["DLC00001", "DLC00002"]);
    assert_eq!(got.found, "appmeta+app");

    assert!(installed_title(&console, &k, "PPSA09999").unwrap().is_none());
    assert!(installed_title(&console, &k, "../../etc").is_err());
}
