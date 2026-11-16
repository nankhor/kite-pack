//! Installing on the PS5 through ShadowMount+: a DLC `.pkg` or a backport folder is sent to the
//! console, then ShadowMount+'s API installs it with Sony's own installer (packages) or copies
//! it to `/data/homebrew/backports` (backports), where it is applied at the next launch.
//!
//! This is the native route: nothing is packed in, and no `dlc_emu` is involved. It needs
//! ShadowMount+ running, and a DLC package needs its base game installed as a package (a
//! folder/image mount counts as `base_installed`, not `installed` — ShadowMount+ refuses DLC
//! for it, so the caller picks the in-pack route for those games instead).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kite_pipeline::{InstallError, Installer, SendError, SendTarget};
use kite_smp::{Client, DEFAULT_PORT, Job};

use crate::Keys;
use crate::send::Ps5Sender;

/// Where packages and backports are put on the console for ShadowMount+ to pick up. Under
/// `/data`, which the Kite helper may write.
pub const INCOMING: &str = "/data/kite/incoming";

/// Reads the console to install on: address, helper port, ShadowMount+ port. Read per call so a
/// changed address takes effect without restarting the app.
pub type ConsoleLookup = Arc<dyn Fn() -> Option<(String, u16, u16)> + Send + Sync>;

/// Installs DLC packages and backports through ShadowMount+.
pub struct SmpInstaller {
    keys: Keys,
    outboards: PathBuf,
    console: ConsoleLookup,
    /// Where on the console uploads land; tests point it at a temp folder.
    pub incoming: String,
    /// How often the job's status is read, and how long it may take.
    pub poll: Duration,
    pub ceiling: Duration,
}

impl SmpInstaller {
    pub fn new(keys: Keys, outboards: PathBuf, console: ConsoleLookup) -> SmpInstaller {
        SmpInstaller {
            keys,
            outboards,
            console,
            incoming: INCOMING.to_string(),
            poll: Duration::from_secs(2),
            ceiling: Duration::from_secs(30 * 60),
        }
    }

    /// ShadowMount+ for this console, and the game, when a title is given.
    pub fn status(&self, title_id: Option<&str>) -> Result<crate::SmpStatus, String> {
        let (host, _, port) = (self.console)().ok_or("Set up your PS5 in the Settings first.")?;
        let client = Client::new(&host, port);
        let version = client.probe()?;
        let game = match title_id {
            Some(id) if !id.is_empty() => client.game(id).ok(),
            _ => None,
        };
        Ok(crate::SmpStatus {
            host,
            port,
            version: version.shadowmount_version,
            api_version: version.api_version,
            installed: game.as_ref().map(|g| g.installed).unwrap_or(false),
            base_installed: game.as_ref().map(|g| g.base_installed).unwrap_or(false),
            source_type: game.map(|g| g.source_type).unwrap_or_default(),
        })
    }

    pub fn port(&self) -> u16 {
        (self.console)()
            .map(|(_, _, port)| port)
            .unwrap_or(DEFAULT_PORT)
    }

    /// Installs a backport folder: sent to the console, then ShadowMount+ copies it to its own
    /// backports folder and applies it at the next launch.
    pub fn install_backport(&self, folder: &Path, cancel: &AtomicBool) -> Result<(), InstallError> {
        if !folder.is_dir() {
            return Err(InstallError::Failed("A backport is a folder".into()));
        }
        let name = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| InstallError::Failed("That folder has no name".into()))?;
        let (host, helper_port, smp_port) = self
            .console()
            .ok_or_else(|| InstallError::Failed("Set up your PS5 in the Settings first.".into()))?;
        let remote = format!("{}/{name}", self.incoming.trim_end_matches('/'));
        self.send(folder, &host, helper_port, cancel)?;
        let client = Client::new(&host, smp_port);
        let job = client
            .install_backport(&remote)
            .map_err(InstallError::Failed)?;
        self.settle(&client, job, cancel)
    }

    fn console(&self) -> Option<(String, u16, u16)> {
        (self.console)()
    }

    fn send(
        &self,
        local: &Path,
        host: &str,
        helper_port: u16,
        cancel: &AtomicBool,
    ) -> Result<(), InstallError> {
        let sender = Ps5Sender::new(self.keys.clone(), helper_port, self.outboards.clone());
        let target = SendTarget {
            console: host.to_string(),
            destination: self.incoming.clone(),
        };
        sender.upload(local, &target, cancel).map_err(from_send)
    }

    /// Waits for a job to stop, reporting why it failed.
    fn settle(&self, client: &Client, job: Job, cancel: &AtomicBool) -> Result<(), InstallError> {
        let start = Instant::now();
        let mut job = job;
        loop {
            if cancel.load(Ordering::SeqCst) {
                let _ = client.cancel_job(job.job_id);
                return Err(InstallError::Cancelled);
            }
            if job.stopped() {
                return match job.state.as_str() {
                    "completed" => Ok(()),
                    "cancelled" => Err(InstallError::Cancelled),
                    _ => Err(match job.failure() {
                        Some(why) if job.native_status != 0 => InstallError::Refused {
                            code: job.native_status as u32,
                            hint: why,
                        },
                        Some(why) => InstallError::Failed(why),
                        None => InstallError::Failed("ShadowMount+ couldn't install it".into()),
                    }),
                };
            }
            if start.elapsed() > self.ceiling {
                return Err(InstallError::Failed(
                    "ShadowMount+ didn't finish installing it in time".into(),
                ));
            }
            runtime_sleep(self.poll);
            job = client.job(job.job_id).map_err(InstallError::Failed)?;
        }
    }
}

impl Installer for SmpInstaller {
    fn is_installed(&self, _pkg: &Path, _target: &SendTarget) -> Result<bool, InstallError> {
        // ShadowMount+ refuses a package that is already installed, and the app checks the
        // console's DLC labels before it asks; no per-package probe is worth the call here.
        Ok(false)
    }

    fn install(
        &self,
        pkg: &Path,
        _target: &SendTarget,
        cancel: &AtomicBool,
    ) -> Result<(), InstallError> {
        if !pkg.is_file() {
            return Err(InstallError::Failed("That DLC package isn't a file".into()));
        }
        let name = pkg
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| InstallError::Failed("That package has no name".into()))?;
        let (host, helper_port, smp_port) = self
            .console()
            .ok_or_else(|| InstallError::Failed("Set up your PS5 in the Settings first.".into()))?;
        let remote = format!("{}/{name}", self.incoming.trim_end_matches('/'));
        self.send(pkg, &host, helper_port, cancel)?;
        let client = Client::new(&host, smp_port);
        let job = client
            .install_package(&remote)
            .map_err(InstallError::Failed)?;
        self.settle(&client, job, cancel)
    }
}

/// ShadowMount+ has no sleeper to borrow; the runner's is for pipeline tests.
fn runtime_sleep(d: Duration) {
    std::thread::sleep(d);
}

fn from_send(e: SendError) -> InstallError {
    match e {
        SendError::Unreachable(why) => InstallError::Unreachable(why),
        SendError::Cancelled => InstallError::Cancelled,
        SendError::Failed(why) => InstallError::Failed(why),
    }
}
