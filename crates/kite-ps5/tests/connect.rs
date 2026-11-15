#![cfg(unix)]
mod support;

use kite_ps5::{Console, Keys, Ps5Error, discover_at, hello};
use std::path::Path;
use std::time::Duration;
use support::{Helper, closed_port};

fn keys(dir: &Path) -> Keys {
    Keys::open(&dir.join("pc")).unwrap()
}

#[test]
fn connects_with_no_code() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(tmp.path(), &[]);
    let info = hello(&helper.console(), &keys(tmp.path())).unwrap();
    assert_eq!(info.name, "Kite helper");
    assert_eq!(info.version, "0.2.0");
}

#[test]
fn remembers_the_helper() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(tmp.path(), &[]);
    let keys = keys(tmp.path());
    hello(&helper.console(), &keys).unwrap();
    hello(&helper.console(), &keys).unwrap();
    let peers = std::fs::read_to_string(tmp.path().join("pc/peers")).unwrap();
    assert_eq!(peers.lines().filter(|l| !l.trim().is_empty()).count(), 1, "{peers}");
}

#[test]
fn refuses_outside_the_home_network() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(tmp.path(), &["--trust", "10.99.0.0/16"]);
    let err = hello(&helper.console(), &keys(tmp.path())).unwrap_err();
    assert!(matches!(err, Ps5Error::NotHomeNetwork), "{err:?}");
    assert_eq!(err.message(), "The Kite helper only accepts computers on your home network.");
}

#[test]
fn a_reinstalled_helper_is_accepted() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = keys(tmp.path());
    let first = Helper::start(&tmp.path().join("one"), &[]);
    hello(&first.console(), &keys).unwrap();
    let port = first.port;
    first.kill();
    // Same address and port, a new identity: as after /data/kite was wiped.
    let again = Helper::start_at(&tmp.path().join("two/state"), &tmp.path().join("two/ps5"), port, &[]);
    assert_eq!(hello(&again.console(), &keys).unwrap().name, "Kite helper");
}

#[test]
fn closed_port_is_unreachable() {
    let tmp = tempfile::tempdir().unwrap();
    let console = Console { address: "127.0.0.1".into(), port: closed_port() };
    let err = hello(&console, &keys(tmp.path())).unwrap_err();
    assert!(matches!(err, Ps5Error::Unreachable(_)), "{err:?}");
}

#[test]
fn discovery_finds_the_helper() {
    let tmp = tempfile::tempdir().unwrap();
    let helper = Helper::start(tmp.path(), &[]);
    let target = format!("127.0.0.1:{}", helper.port).parse().unwrap();
    let found = discover_at(&[target], Duration::from_secs(1));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].address, "127.0.0.1");
    assert_eq!(found[0].port, helper.port);
    assert_eq!(found[0].version, "0.2.0");
}
