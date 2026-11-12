//! Connecting to the Kite helper with no code: the helper trusts computers on the home
//! network, and this side remembers the helper's key the first time it meets it.

use crate::{Ps5Error, runtime};
use ava1::Ava1Error;
use ava1::keys::Identity;
use ava1::peers::PeerStore;
use ava1::session::{Session, Timing, connect};
use serde::Serialize;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long reaching the PS5 may take before it counts as unreachable.
const REACH: Duration = Duration::from_secs(3);
const MY_NAME: &str = "Kite Pack";

/// Where the Kite helper listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Console {
    pub address: String,
    pub port: u16,
}

impl Console {
    pub(crate) fn socket(&self) -> String {
        if self.address.contains(':') {
            format!("[{}]:{}", self.address, self.port)
        } else {
            format!("{}:{}", self.address, self.port)
        }
    }
}

/// This computer's AVA1 identity and the helpers it knows, kept in one folder.
#[derive(Clone)]
pub struct Keys {
    pub(crate) identity: Arc<Identity>,
    pub(crate) peers: Arc<Mutex<PeerStore>>,
}

impl Keys {
    pub fn open(dir: &Path) -> io::Result<Keys> {
        std::fs::create_dir_all(dir)?;
        let identity = Identity::load_or_create(&dir.join("identity"))?;
        let peers = PeerStore::load(&dir.join("peers"))?;
        Ok(Keys { identity: Arc::new(identity), peers: Arc::new(Mutex::new(peers)) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HelperInfo {
    pub name: String,
    pub version: String,
}

pub(crate) fn map_error(e: Ava1Error) -> Ps5Error {
    match e {
        Ava1Error::Io(e) => Ps5Error::Unreachable(e.to_string()),
        Ava1Error::Timeout => Ps5Error::Unreachable("timed out".into()),
        Ava1Error::Refused { code, .. } if code == ava1::r#gen::ERR_PAIRING_CLOSED => Ps5Error::NotHomeNetwork,
        Ava1Error::Refused { message, .. } => Ps5Error::Failed(message),
        other => Ps5Error::Failed(other.to_string()),
    }
}

async fn connect_once(console: &Console, keys: &Keys) -> Result<Session, Ps5Error> {
    let addr = console.socket();
    let attempt = connect(&addr, keys.identity.clone(), keys.peers.clone(), MY_NAME, Timing::default());
    match tokio::time::timeout(REACH, attempt).await {
        Ok(result) => result.map_err(map_error),
        Err(_) => Err(Ps5Error::Unreachable("timed out".into())),
    }
}

/// A session the helper and this computer both trust. On first contact (or after the helper
/// was reinstalled with a new key) this side remembers the helper's key and connects again.
pub(crate) async fn open(console: &Console, keys: &Keys) -> Result<Session, Ps5Error> {
    let session = connect_once(console, keys).await?;
    if !session.pairing_pending() {
        return Ok(session);
    }
    let key = session.peer_key();
    session.close().await;
    keys.peers
        .lock()
        .unwrap()
        .add(key, "Kite helper")
        .map_err(|e| Ps5Error::Failed(format!("Couldn't remember the PS5: {e}")))?;
    let again = connect_once(console, keys).await?;
    if again.pairing_pending() {
        again.close().await;
        return Err(Ps5Error::NotHomeNetwork);
    }
    Ok(again)
}

/// Connects, and says which helper answered.
pub fn hello(console: &Console, keys: &Keys) -> Result<HelperInfo, Ps5Error> {
    runtime().block_on(async {
        let session = open(console, keys).await?;
        let info = session.node_info().await.map_err(map_error);
        session.close().await;
        let info = info?;
        Ok(HelperInfo { name: info.name, version: info.version })
    })
}
