//! How the pipeline reaches the outside world. Every effect goes through one of these traits,
//! so each rule is tested with fakes and the real implementations live in other crates.

use crate::options::{Level, OutputFormat, SendTarget};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Condvar, Mutex};
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
    /// Parts may leave the PC once the PS5 has them: the packed folder is sent and then deleted.
    /// Says which parts the PS5 confirmed.
    pub let_go: Option<Arc<LetGo>>,
}

/// The parts of a packed folder the PS5 has confirmed while it is still being packed, for a
/// packer that lets them go ([`PackRequest::let_go`]); and whether more may still come.
#[derive(Debug, Default)]
pub struct LetGo {
    state: Mutex<LetGoState>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct LetGoState {
    confirmed: Vec<Part>,
    ended: bool,
}

impl LetGo {
    fn state(&self) -> std::sync::MutexGuard<'_, LetGoState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The PS5 has `part`, whole and checked.
    pub fn confirm(&self, part: &Part) {
        self.state().confirmed.push(part.clone());
        self.changed.notify_all();
    }

    /// No more parts will be confirmed: the pack is over, or parts stopped going.
    pub fn end(&self) {
        self.state().ended = true;
        self.changed.notify_all();
    }

    /// The parts confirmed so far, in the order they were.
    pub fn confirmed(&self) -> Vec<Part> {
        self.state().confirmed.clone()
    }

    pub fn ended(&self) -> bool {
        self.state().ended
    }

    /// Waits until a part is confirmed or letting go ends, at most `timeout`.
    pub fn wait_change(&self, timeout: Duration) {
        let state = self.state();
        let _ = self.changed.wait_timeout(state, timeout);
    }
}

/// Two requests are the same whether or not they share one [`LetGo`].
impl PartialEq for LetGo {
    fn eq(&self, _: &LetGo) -> bool {
        true
    }
}

impl Eq for LetGo {}

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
    /// once and keep it open). With `copy_first`, it must not be held open there for long (on
    /// Windows a file open in a folder stops the folder's rename, and the packer renames its
    /// folder to publish the pack): the runner copies it out and sends the copy, and stops
    /// copying at once on [`Progress::PartsClosing`]. Only sent when the request asked for it;
    /// the runner keeps it.
    PartReady { packed: PathBuf, file: PathBuf, part: Part, copy_first: bool },
    /// The packer is about to publish its folder: nothing may read a part where it is any more.
    /// The runner keeps it.
    PartsClosing,
    /// From the runner, during Pack: how many parts of the packed copy the PS5 has, whether
    /// the next one is on its way, the bytes on the PS5 (the part on its way included), and what
    /// the whole packed copy should come to (0 until a first part says: the parts so far over
    /// how far the pack is).
    PartsAhead { sent: usize, sending: bool, on_ps5: u64, estimate: u64 },
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

    /// Whether parts sent ahead to `target` can leave the PC before the pack ends: the PS5 is
    /// there and its helper can put each part's header right afterwards. Asked once, before a
    /// pack that would let parts go.
    fn lets_parts_go(&self, _target: &SendTarget) -> bool {
        false
    }

    /// Sends one finished part of the folder `packed` ahead of the rest, to where it sits once
    /// `packed` is sent to `target`; `file` is where to read it now. `progress` hears the part's
    /// bytes the PS5 has so far. `Ok(false)`: the PS5 can't take parts ahead (an older helper),
    /// so it goes with the rest.
    fn send_ahead(
        &self,
        _packed: &Path,
        _file: &Path,
        _part: &Part,
        _target: &SendTarget,
        _progress: &(dyn Fn(u64) + Sync),
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
/// How installing a package as it is ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    /// The PS5 already had this very package: nothing was asked of it.
    AlreadyThere,
}

pub trait Installer: Send + Sync {
    fn is_installed(&self, pkg: &Path, target: &SendTarget) -> Result<bool, InstallError>;
    fn install(&self, pkg: &Path, target: &SendTarget, cancel: &AtomicBool) -> Result<(), InstallError>;

    /// Installs the package `pkg` (a game, an update or a DLC) as it is: the PS5's installer
    /// streams it from this computer. `progress` hears the bytes the PS5 has received.
    fn install_package(
        &self,
        _pkg: &Path,
        _target: &SendTarget,
        _progress: &dyn Fn(Progress),
        _cancel: &AtomicBool,
    ) -> Result<InstallOutcome, InstallError> {
        Err(InstallError::Failed("This Kite Pack can't install packages".into()))
    }

    /// The person stopped installing `pkg`: forget it was going on, so the next install of it
    /// asks the PS5 at once instead of waiting for the stopped one to come back.
    fn forget_package(&self, _pkg: &Path, _target: &SendTarget) {}
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
