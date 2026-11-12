//! How the PS5 stands, for the header chip and Settings.

use crate::connect::{Console, Keys, hello};
use crate::Ps5Error;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    /// No PS5 chosen yet.
    NotSetUp,
    /// Nothing answers at the PS5's address: off, asleep, or the helper isn't running.
    NotReachable,
    Running { version: String },
    /// The helper answered but won't take this computer; `reason` is for the screen.
    Refused { reason: String },
}

pub fn status(console: Option<&Console>, keys: &Keys) -> Status {
    let Some(console) = console else { return Status::NotSetUp };
    match hello(console, keys) {
        Ok(info) => Status::Running { version: info.version },
        Err(Ps5Error::Unreachable(_)) => Status::NotReachable,
        Err(e) => Status::Refused { reason: e.message() },
    }
}
