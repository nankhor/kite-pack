//! Installing a DLC package through Sony's installer, streamed from this computer. This is the
//! route ps5upload uses: the console fetches the pkg from us and `sceAppInstUtilInstallByPackage`
//! writes it to `/user/addcont`. A mounted dump folder is fine — Sony only needs the title
//! registered, which ShadowMount+ already did.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kite_forge::{inspect_dlc, inspect_package};
use kite_pipeline::{InstallError, InstallOutcome, Installer, Progress, SendTarget};

use crate::package_install::{HelperOps, RealClock, install_with};
use crate::{Console, Keys, Ps5Error, PkgServer};

/// Installs DLC through the PS5's own installer, after the game is registered on the console.
pub struct SonyInstaller {
    keys: Keys,
    helper_port: u16,
    /// Where an install that was asked for but not confirmed yet is remembered across restarts.
    markers: std::path::PathBuf,
}

impl SonyInstaller {
    pub fn new(keys: Keys, helper_port: u16) -> SonyInstaller {
        SonyInstaller { keys, helper_port, markers: std::env::temp_dir().join("kite-pack-installs") }
    }

    /// Keeps what it remembers about installs in `dir`.
    pub fn with_state(mut self, dir: std::path::PathBuf) -> SonyInstaller {
        self.markers = dir;
        self
    }

    fn console(&self, target: &SendTarget) -> Console {
        Console::parse(&target.console, self.helper_port)
    }
}

impl Installer for SonyInstaller {
    fn install_package(
        &self,
        pkg: &std::path::Path,
        target: &SendTarget,
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<InstallOutcome, InstallError> {
        let info = inspect_package(pkg).map_err(InstallError::Failed)?;
        let console = self.console(target);
        let server = PkgServer::start_stable(pkg, &console.address).map_err(InstallError::Failed)?;
        let ops = HelperOps { console: &console, keys: &self.keys };
        install_with(&ops, pkg, &info, &server, &self.markers, progress, cancel, &RealClock)
    }

    fn is_installed(&self, pkg: &std::path::Path, target: &SendTarget) -> Result<bool, InstallError> {
        let dlc = inspect_dlc(pkg).map_err(InstallError::Failed)?;
        let console = self.console(target);
        // A read that fails just means "can't confirm": the install attempt will say what's
        // wrong, rather than failing the step before it starts.
        let game = crate::library::installed_title_as(&console, &self.keys, &self.keys.install, &dlc.title_id);
        Ok(game
            .ok()
            .flatten()
            .is_some_and(|g| g.dlc.iter().any(|l| l.eq_ignore_ascii_case(&dlc.info.label))))
    }

    fn install(&self, pkg: &std::path::Path, target: &SendTarget, cancel: &AtomicBool) -> Result<(), InstallError> {
        let dlc = inspect_dlc(pkg).map_err(InstallError::Failed)?;
        let console = self.console(target);

        // The game was just sent: ShadowMount+ may not have registered it yet. Sony answers
        // "not found" until it has, so wait for the title before asking.
        let registered = Instant::now() + Duration::from_secs(180);
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(InstallError::Cancelled);
            }
            match crate::library::installed_title_as(&console, &self.keys, &self.keys.install, &dlc.title_id) {
                Ok(Some(_)) => break,
                Ok(None) => {}
                Err(e) if transient(&e.message()) => {}
                Err(e) => return Err(from_ps5(e)),
            }
            if Instant::now() > registered {
                return Err(InstallError::Failed(
                    "The game isn't registered on the PS5 yet; check ShadowMount+ is running".into(),
                ));
            }
            std::thread::sleep(Duration::from_secs(2));
        }

        let server = PkgServer::start(pkg, &console.address).map_err(InstallError::Failed)?;
        let name = dlc.info.name.clone().unwrap_or_else(|| dlc.info.label.clone());
        let mut accepted = false;
        for attempt in 0..10 {
            if cancel.load(Ordering::SeqCst) {
                return Err(InstallError::Cancelled);
            }
            match crate::install_url(&console, &self.keys, server.url(), &name) {
                Ok(()) => {
                    accepted = true;
                    break;
                }
                // A registration that lands late: the title exists a moment after the send.
                Err(e) if attempt < 9 && (e.message().contains("registered") || transient(&e.message())) => {
                    std::thread::sleep(Duration::from_secs(3));
                }
                Err(e) => return Err(from_ps5(e)),
            }
        }
        if !accepted {
            return Err(InstallError::Failed("The PS5 never took the package".into()));
        }

        // Sony accepted it and is fetching from `server` now; only Done once it is really there.
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(InstallError::Cancelled);
            }
            match crate::library::installed_title_as(&console, &self.keys, &self.keys.install, &dlc.title_id) {
                Ok(Some(game)) if game.dlc.iter().any(|l| l.eq_ignore_ascii_case(&dlc.info.label)) => {
                    // Let Sony's installer settle before the next package starts.
                    std::thread::sleep(Duration::from_secs(2));
                    return Ok(());
                }
                Ok(_) => {}
                Err(e) if transient(&e.message()) => {}
                Err(e) => return Err(from_ps5(e)),
            }
            if Instant::now() > deadline {
                return Err(InstallError::Failed(
                    "Sony accepted the package, but it didn't appear in the DLC list".into(),
                ));
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

/// A session that went away while we were asking: worth another try, unlike a refusal.
fn transient(message: &str) -> bool {
    let low = message.to_ascii_lowercase();
    // A cut session is worth another try. "The PS5 isn't reachable" is not: retrying just
    // stretches the failure over minutes, which is what made a stuck job look frozen.
    low.contains("connection lost") || low.contains("session ended")
}

fn from_ps5(e: Ps5Error) -> InstallError {
    match e {
        Ps5Error::Unreachable(why) => InstallError::Unreachable(why),
        other => InstallError::Failed(other.message()),
    }
}
