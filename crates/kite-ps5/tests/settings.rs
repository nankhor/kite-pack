use kite_ps5::Settings;
use std::path::PathBuf;

#[test]
fn missing_settings_are_the_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Settings::load(&tmp.path().join("settings.json"));
    assert_eq!(s, Settings::default());
    assert_eq!(s.address, None);
    assert_eq!(s.helper_port, 9130);
    assert_eq!(s.loader_port, 9021);
    assert_eq!(s.destination, "/data/homebrew");
    assert_eq!(s.packed_folder, None);
}

#[test]
fn settings_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    let s = Settings {
        address: Some("192.168.1.42".into()),
        helper_port: 9131,
        loader_port: 9020,
        destination: "/mnt/ext0/homebrew".into(),
        packed_folder: Some(PathBuf::from("/Games/Packed")),
    };
    s.save(&path).unwrap();
    assert_eq!(Settings::load(&path), s);
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(json["helperPort"], 9131, "camelCase for the screen");
}

#[test]
fn corrupt_settings_are_the_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, b"{ not json").unwrap();
    assert_eq!(Settings::load(&path), Settings::default());
}

#[test]
fn settings_from_an_older_app_keep_their_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.json");
    std::fs::write(&path, br#"{"address":"10.0.0.5"}"#).unwrap();
    let s = Settings::load(&path);
    assert_eq!(s.address.as_deref(), Some("10.0.0.5"));
    assert_eq!(s.helper_port, 9130);
}
