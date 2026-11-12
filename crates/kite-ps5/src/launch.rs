//! Starting the Kite helper: send it to the PS5's ELF loader, unless it is already running.

use crate::Ps5Error;
use serde::Serialize;
use std::io::Write;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::time::Duration;

const CHECK: Duration = Duration::from_millis(1500);
const LOADER: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum StartOutcome {
    /// The helper was already answering; nothing was sent.
    AlreadyRunning,
    /// The helper went to the ELF loader; it starts in a few seconds.
    Sent,
}

fn reach(address: &str, port: u16, wait: Duration) -> std::io::Result<TcpStream> {
    let target = (address, port)
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| std::io::Error::other("no such address"))?;
    TcpStream::connect_timeout(&target, wait)
}

/// Sends `elf` to the ELF loader at `address:loader_port` if nothing answers on `helper_port`.
pub fn start_helper(address: &str, loader_port: u16, helper_port: u16, elf: &[u8]) -> Result<StartOutcome, Ps5Error> {
    if elf.is_empty() {
        return Err(Ps5Error::Failed("This build doesn't include the Kite helper.".into()));
    }
    if reach(address, helper_port, CHECK).is_ok() {
        return Ok(StartOutcome::AlreadyRunning);
    }
    let mut loader = reach(address, loader_port, LOADER).map_err(|_| {
        Ps5Error::Failed(format!(
            "The PS5's ELF loader isn't answering on port {loader_port}. Check the PS5 is jailbroken and its loader is running."
        ))
    })?;
    loader
        .write_all(elf)
        .and_then(|_| loader.shutdown(Shutdown::Write))
        .map_err(|e| Ps5Error::Failed(format!("Couldn't send the Kite helper: {e}")))?;
    Ok(StartOutcome::Sent)
}
