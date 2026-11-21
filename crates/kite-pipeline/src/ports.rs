//! How the pipeline reaches the outside world. Every effect goes through one of these traits,
//! so each rule is tested with fakes and the real implementations live in other crates.

use crate::options::{Level, OutputFormat, SendTarget};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest {
    pub source: PathBuf,
    pub output_dir: PathBuf,
    pub format: OutputFormat,
    pub level: Level,
    /// DLC and a backport to fold in before packing; empty packs the source as it is.
    pub bundle: crate::Bundle,
    /// Say when a part of a packed folder is finished ([`Progress::PartReady`]), so it can go
    /// to the PS5 while the rest is still being packed.
    pub ahead: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackOutcome {
    pub output: PathBuf,
    pub bytes: u64,
    pub files: u64,
    pub checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    Stage(String),
    Bytes { done: u64, total: u64 },
    /// A part of the packed folder `packed` is finished and can be sent before the pack ends.
    /// `file` is where it can be read now (it may move when the pack is published: open it
    /// once and keep it open). Only sent when the request asked for it; the runner keeps it.
    PartReady { packed: PathBuf, file: PathBuf, part: Part },
    /// From the runner, during Pack: how many parts of the packed copy the PS5 has, and
    /// whether the next one is on its way.
    PartsAhead { sent: usize, sending: bool },
}

/// A finished part of a packed folder, such as a pack volume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    /// Where it sits inside the packed folder (`ampr_zstd-000.pak`).
    pub rel: String,
    /// Its length, which no longer changes.
    pub size: u64,
    /// The bytes before this offset are written once more when the pack ends (a volume's
    /// header); every byte from here on is final.
    pub settled_from: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackError {
    Failed(String),
    Cancelled,
}

/// Packs a game with zstd and checks the output before returning.
pub trait Packer: Send + Sync {
    fn pack(
        &self,
        req: &PackRequest,
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<PackOutcome, PackError>;
}

/// Moves a file or folder to the system Trash. Never deletes for good.
pub trait Trash: Send + Sync {
    fn trash(&self, path: &Path) -> Result<(), String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendError {
    /// The PS5 doesn't answer right now; the runner waits and tries again.
    Unreachable(String),
    Failed(String),
    Cancelled,
}

/// Sends the packed game to the PS5 and checks every file there.
pub trait Sender: Send + Sync {
    fn send(
        &self,
        local: &Path,
        target: &SendTarget,
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<(), SendError>;

    /// Sends one finished part of the folder `packed` ahead of the rest, to where it sits once
    /// `packed` is sent to `target`; `file` is where to read it now. `Ok(false)`: the PS5 can't
    /// take parts ahead (an older helper), so it goes with the rest.
    fn send_ahead(
        &self,
        _packed: &Path,
        _file: &Path,
        _part: &Part,
        _target: &SendTarget,
        _cancel: &AtomicBool,
    ) -> Result<bool, SendError> {
        Ok(false)
    }

    /// Sends `local` when the parts in `ahead` went over already: each one found whole on the
    /// PS5 is left out and has its first bytes put right; any other goes again with the rest.
    fn send_rest(
        &self,
        local: &Path,
        target: &SendTarget,
        _ahead: &[Part],
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<(), SendError> {
        self.send(local, target, progress, cancel)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    Unreachable(String),
    /// Sony's installer said no; `hint` is the code in plain words.
    Refused { code: u32, hint: String },
    /// Handed over, but it never showed up on the PS5.
    NotConfirmed,
    Failed(String),
    Cancelled,
}

/// Installs a DLC package on the PS5 and confirms it landed.
pub trait Installer: Send + Sync {
    fn is_installed(&self, pkg: &Path, target: &SendTarget) -> Result<bool, InstallError>;
    fn install(&self, pkg: &Path, target: &SendTarget, cancel: &AtomicBool) -> Result<(), InstallError>;
}

pub trait Sleeper: Send + Sync {
    fn sleep(&self, d: Duration);
}

pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// Stands in for sending and installing until the Kite helper exists.
pub struct Unavailable;

const UNAVAILABLE: &str = "Sending to the PS5 isn't available yet";

impl Sender for Unavailable {
    fn send(&self, _: &Path, _: &SendTarget, _: &dyn Fn(Progress), _: &AtomicBool) -> Result<(), SendError> {
        Err(SendError::Failed(UNAVAILABLE.into()))
    }
}

impl Installer for Unavailable {
    fn is_installed(&self, _: &Path, _: &SendTarget) -> Result<bool, InstallError> {
        Err(InstallError::Failed(UNAVAILABLE.into()))
    }

    fn install(&self, _: &Path, _: &SendTarget, _: &AtomicBool) -> Result<(), InstallError> {
        Err(InstallError::Failed(UNAVAILABLE.into()))
    }
}
