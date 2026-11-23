//! Letting parts go: Forge's packer asks this hook before each new volume, and it cuts every
//! part the PS5 has confirmed down to its 64-byte header, then holds the pack while
//! [`HOLD`] finished parts still wait for the PS5.

use kite_pipeline::LetGo;
use ps5_dump_forge_zstd::VOLUME_HEADER_SIZE;
use ps5_dump_forge_zstd::hook::VolumeHook;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Finished parts that may wait for the PS5 before the pack holds.
pub(crate) const HOLD: usize = 2;

/// Most full parts the pack folder holds at once: [`HOLD`] waiting, one being written and a
/// copy on its way (Windows).
pub(crate) const MOST_HELD: u32 = 4;

/// How long a hold waits between looks at the PS5's confirmations and the cancel flag.
const LOOK: Duration = Duration::from_millis(200);

/// The hook for one pack into `output`.
pub(crate) struct Shrink {
    output: PathBuf,
    dir: Mutex<Option<PathBuf>>,
    let_go: Arc<LetGo>,
    cut: Mutex<Vec<u32>>,
}

impl Shrink {
    pub(crate) fn new(output: &Path, let_go: Arc<LetGo>) -> Shrink {
        Shrink { output: output.to_path_buf(), dir: Mutex::new(None), let_go, cut: Mutex::new(Vec::new()) }
    }

    /// Forge's `.part` folder for the output, found once.
    fn dir(&self) -> io::Result<PathBuf> {
        let mut dir = self.dir.lock().unwrap_or_else(|e| e.into_inner());
        if dir.is_none() {
            *dir = crate::packer::part_dir_of(&self.output);
        }
        dir.clone().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Forge's .part folder"))
    }

    /// Cuts every confirmed part not cut yet; how many parts are confirmed.
    fn cut_confirmed(&self) -> io::Result<usize> {
        let confirmed = self.let_go.confirmed();
        let mut cut = self.cut.lock().unwrap_or_else(|e| e.into_inner());
        for part in &confirmed {
            let Some(volume) = volume_of(&part.rel) else { continue };
            if cut.contains(&volume) {
                continue;
            }
            let file = std::fs::OpenOptions::new().write(true).open(self.dir()?.join(&part.rel))?;
            file.set_len(VOLUME_HEADER_SIZE as u64)?;
            cut.push(volume);
        }
        Ok(confirmed.len())
    }
}

/// `ampr_zstd-012.pak` → 12.
fn volume_of(rel: &str) -> Option<u32> {
    rel.strip_prefix("ampr_zstd-")?.strip_suffix(".pak")?.parse().ok()
}

impl VolumeHook for Shrink {
    fn open(&self, name: &str) -> io::Result<std::fs::File> {
        std::fs::File::open(self.dir()?.join(name))
    }

    fn before_next(&self, next: u32, cancel: &AtomicBool) -> io::Result<()> {
        loop {
            let confirmed = self.cut_confirmed()?;
            let waiting = (next as usize).saturating_sub(confirmed);
            if waiting < HOLD || self.let_go.ended() {
                return Ok(());
            }
            if cancel.load(Ordering::SeqCst) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.let_go.wait_change(LOOK);
        }
    }

    fn cut(&self) -> Vec<u32> {
        self.cut.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn most_held(&self) -> u32 {
        MOST_HELD
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kite_pipeline::{LetGo, Part};
    use ps5_dump_forge_zstd::hook::VolumeHook;
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    /// Forge's `.part` folder for `<tmp>/Game`, with `n` parts of 300 bytes.
    fn packing(tmp: &Path, n: u32) -> (std::path::PathBuf, std::path::PathBuf) {
        let output = tmp.join("Game");
        let dir = tmp.join(format!("Game.7-{}.part", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("ampr_zstd-{i:03}.pak")), vec![1; 300]).unwrap();
        }
        (output, dir)
    }

    fn part(i: u32) -> Part {
        Part { rel: format!("ampr_zstd-{i:03}.pak"), size: 300, settled_from: 64 }
    }

    fn len(dir: &Path, i: u32) -> u64 {
        std::fs::metadata(dir.join(format!("ampr_zstd-{i:03}.pak"))).unwrap().len()
    }

    #[test]
    fn cuts_confirmed_parts_before_the_next() {
        let tmp = tempfile::tempdir().unwrap();
        let (output, dir) = packing(tmp.path(), 2);
        let let_go = Arc::new(LetGo::default());
        let hook = Shrink::new(&output, let_go.clone());
        let_go.confirm(&part(0));
        hook.before_next(2, &AtomicBool::new(false)).unwrap();
        assert_eq!((len(&dir, 0), len(&dir, 1)), (64, 300));
        assert_eq!(hook.cut(), [0]);
        assert_eq!(hook.most_held(), MOST_HELD);
        // Read back through the hook, from the same folder.
        assert_eq!(std::io::read_to_string(hook.open("ampr_zstd-001.pak").unwrap()).unwrap().len(), 300);
    }

    #[test]
    fn holds_while_two_parts_wait() {
        let tmp = tempfile::tempdir().unwrap();
        let (output, dir) = packing(tmp.path(), 2);
        let let_go = Arc::new(LetGo::default());
        let hook = Shrink::new(&output, let_go.clone());
        let later = let_go.clone();
        let started = Instant::now();
        let confirm = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            later.confirm(&part(0));
        });
        hook.before_next(2, &AtomicBool::new(false)).unwrap();
        assert!(started.elapsed() >= Duration::from_millis(300), "it waited for the PS5");
        assert_eq!(len(&dir, 0), 64);
        confirm.join().unwrap();
        // One part waiting: no hold.
        let started = Instant::now();
        hook.before_next(2, &AtomicBool::new(false)).unwrap();
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn ends_holding_when_letting_go_ends() {
        let tmp = tempfile::tempdir().unwrap();
        let (output, dir) = packing(tmp.path(), 2);
        let let_go = Arc::new(LetGo::default());
        let hook = Shrink::new(&output, let_go.clone());
        let later = let_go.clone();
        let end = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            later.end();
        });
        hook.before_next(2, &AtomicBool::new(false)).unwrap();
        end.join().unwrap();
        assert_eq!((len(&dir, 0), len(&dir, 1)), (300, 300), "nothing confirmed, nothing cut");
        assert!(hook.cut().is_empty());
    }

    #[test]
    fn stops_on_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let (output, _) = packing(tmp.path(), 2);
        let hook = Shrink::new(&output, Arc::new(LetGo::default()));
        let cancel = Arc::new(AtomicBool::new(false));
        let later = cancel.clone();
        let stop = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            later.store(true, Ordering::SeqCst);
        });
        let error = hook.before_next(2, &cancel).unwrap_err();
        stop.join().unwrap();
        assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    }
}
