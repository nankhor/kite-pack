//! Kite Pack: a hook into the packer for letting parts go as they are packed (README.md).
//!
//! With a hook set, every volume is read back and checked as it closes, and the hook is asked
//! before the next one opens: there it may cut volumes the PS5 already has down to their header,
//! and wait. Forge's core asks the hook which volumes were cut and how many it holds at most.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub trait VolumeHook: Send + Sync {
    /// A volume that has just been written, opened for reading it back.
    fn open(&self, name: &str) -> std::io::Result<std::fs::File>;
    /// Volume `next` is about to open; every volume before it is written and checked. May wait;
    /// an `Interrupted` error cancels the pack, any other fails it.
    fn before_next(&self, next: u32, cancel: &AtomicBool) -> std::io::Result<()>;
    /// The volumes cut down to their header so far.
    fn cut(&self) -> Vec<u32>;
    /// Most full volumes the pack folder holds at once.
    fn most_held(&self) -> u32;
}

static HOOK: Mutex<Option<Arc<dyn VolumeHook>>> = Mutex::new(None);

/// Sets (or, with `None`, clears) the hook the next pack into a folder uses.
pub fn set_volume_hook(hook: Option<Arc<dyn VolumeHook>>) {
    *HOOK.lock().unwrap_or_else(|e| e.into_inner()) = hook;
}

/// The hook set now, if any.
pub fn volume_hook() -> Option<Arc<dyn VolumeHook>> {
    HOOK.lock().unwrap_or_else(|e| e.into_inner()).clone()
}
