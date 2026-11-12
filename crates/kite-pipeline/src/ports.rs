//! How the pipeline reaches the outside world. Every effect goes through one of these traits,
//! so each rule is tested with fakes and the real implementations live in other crates.

use crate::options::{Level, OutputFormat, SendTarget};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest {
    pub source: PathBuf,
    pub output_dir: PathBuf,
    pub format: OutputFormat,
    pub level: Level,
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
