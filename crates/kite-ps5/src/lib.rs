//! Kite Pack's PS5 side: find the Kite helper on the home network, connect to it with no code,
//! and send packed copies to it over AVA1.

mod connect;
mod discover;
mod launch;
mod send;
mod settings;
mod status;

pub use connect::{Console, HelperInfo, Keys, hello};
pub use discover::{DISCOVERY_PORT, Found, discover, discover_at};
pub use launch::{StartOutcome, start_helper};
pub use send::Ps5Sender;
pub use settings::Settings;
pub use status::{Status, status};

use std::sync::OnceLock;
use tokio::runtime::Runtime;

/// What went wrong talking to the PS5, in words the screen can show.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Ps5Error {
    #[error("unreachable: {0}")]
    Unreachable(String),
    #[error("not on the home network")]
    NotHomeNetwork,
    #[error("{0}")]
    Failed(String),
}

impl Ps5Error {
    pub fn message(&self) -> String {
        match self {
            Ps5Error::Unreachable(_) => {
                "The PS5 isn't reachable. Check it's on and the Kite helper is running.".into()
            }
            Ps5Error::NotHomeNetwork => {
                "The Kite helper only accepts computers on your home network.".into()
            }
            Ps5Error::Failed(why) => why.clone(),
        }
    }
}

/// One runtime for every call into AVA1; the calls themselves block, so callers stay simple.
pub(crate) fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("kite-ps5")
            .build()
            .expect("the network runtime couldn't start")
    })
}
