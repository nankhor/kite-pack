//! Packs a game with zstd through Forge's own job runner, then reports Forge's checks.

use kite_pipeline::{Compression, OutputFormat, PackError, PackOutcome, PackRequest, Packer, Part, Progress};
use ps5_dump_forge_core::{ConvertRequest, Event, Format, Jobs, PackKind, generated_output};
use ps5_dump_forge_zstd::{VOLUME_HEADER_SIZE, volume_name};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How often the cancel flag is looked at while Forge is quiet.
const POLL: Duration = Duration::from_millis(200);
/// How often the folder being packed is looked at for finished volumes.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// Finds the pack volumes Forge has finished while it packs on, so they can go to the PS5 early.
///
/// Forge writes a folder into `<output>.<job>-<pid>.part` beside the output and renames it once
/// it is checked. Its volumes (`ampr_zstd-000.pak`, `-001`, ...) are written one after the
/// other, and volume N+1 is only created once N is synced: so N is finished, but for its 64-byte
/// header, which Forge writes into every volume once the whole pack is done.
///
/// A volume is announced where it is, in the `.part` folder. On Mac and Linux the sender opens it
/// once and keeps it open, which outlives the rename. On Windows a file held open in a folder
/// stops the folder's rename, which Forge tries once, failing the pack: there it is announced
/// with `copy_first`, copied out and the copy sent, and [`Progress::PartsClosing`] stops any
/// copy the moment Forge leaves writing. It can't be linked elsewhere instead: Forge's check
/// refuses a file with two names.
pub(crate) struct Parts {
    output: PathBuf,
    copy_first: bool,
    part_dir: Option<PathBuf>,
    next: u32,
    last_look: Option<Instant>,
    closed: bool,
}

/// Forge's stages once every volume is written: its check, then publishing the folder.
const AFTER_WRITING: [&str; 2] = ["verify", "finalize"];

/// Forge's `.part` folder for `output` (`<output>.<job>-<pid>.part`), made by this process.
pub(crate) fn part_dir_of(output: &Path) -> Option<PathBuf> {
    let name = output.file_name()?.to_string_lossy().into_owned();
    let suffix = format!("-{}.part", std::process::id());
    let parent = output.parent()?;
    std::fs::read_dir(parent).ok()?.flatten().find_map(|e| {
        let n = e.file_name().to_string_lossy().into_owned();
        let job = n.strip_prefix(&name)?.strip_prefix('.')?.strip_suffix(&suffix)?;
        (job.bytes().all(|b| b.is_ascii_digit()) && e.path().is_dir()).then(|| e.path())
    })
}

impl Parts {
    pub(crate) fn new(output: &Path, copy_first: bool) -> Parts {
        Parts { output: output.to_path_buf(), copy_first, part_dir: None, next: 0, last_look: None, closed: false }
    }

    /// Forge moved on to `stage`: once past writing, says so (once) and announces no more.
    pub(crate) fn stage(&mut self, stage: &str, progress: &dyn Fn(Progress)) {
        if !self.closed && AFTER_WRITING.contains(&stage) {
            self.closed = true;
            progress(Progress::PartsClosing);
        }
    }

    /// Forge's `.part` folder for this output, made by this process.
    fn find_part_dir(&self) -> Option<PathBuf> {
        part_dir_of(&self.output)
    }

    /// Announces every volume finished since the last look (at most every [`LOOK_EVERY`]).
    pub(crate) fn look(&mut self, progress: &dyn Fn(Progress)) {
        if self.closed || self.last_look.is_some_and(|t| t.elapsed() < LOOK_EVERY) {
            return;
        }
        self.last_look = Some(Instant::now());
        if self.part_dir.is_none() {
            self.part_dir = self.find_part_dir();
        }
        let Some(dir) = self.part_dir.clone() else { return };
        while dir.join(volume_name(self.next + 1)).is_file() {
            let name = volume_name(self.next);
            let file = dir.join(&name);
            let Ok(meta) = std::fs::metadata(&file) else { return };
            self.next += 1;
            progress(Progress::PartReady {
                packed: self.output.clone(),
                file,
                part: Part { rel: name, size: meta.len(), settled_from: VOLUME_HEADER_SIZE as u64 },
                copy_first: self.copy_first,
            });
        }
    }
}

pub struct ForgePacker {
    /// Re-read every byte of the output instead of Forge's fast sampled check.
    pub full_verify: bool,
    /// Volumes sent while packing are copied out of Forge's folder first, and the copy sent:
    /// on Windows, where a file held open in that folder would stop Forge's rename (it tries
    /// once) and fail the pack. Mac and Linux read them in place.
    pub copy_parts: bool,
}

impl Default for ForgePacker {
    fn default() -> ForgePacker {
        ForgePacker { full_verify: false, copy_parts: cfg!(windows) }
    }
}

/// What the staging step folded in, in one line for the job's checks.
fn bundle_note(s: &crate::bundle::Staged) -> String {
    let mut parts = Vec::new();
    if s.dlc > 0 {
        parts.push(format!("{} DLC inside", s.dlc));
    }
    if s.emulator_added {
        parts.push("DLC emulator added".into());
    }
    if s.backport_files > 0 {
        parts.push(format!("backport: {} files replaced", s.backport_files));
    }
    if parts.is_empty() {
        parts.push("nothing to bundle".into());
    }
    format!("bundled: {}", parts.join(", "))
}

/// The volume hook set for one pack, cleared when this goes.
struct HookGuard;

impl HookGuard {
    fn set(hook: std::sync::Arc<dyn ps5_dump_forge_zstd::hook::VolumeHook>) -> HookGuard {
        ps5_dump_forge_zstd::hook::set_volume_hook(Some(hook));
        HookGuard
    }
}

impl Drop for HookGuard {
    fn drop(&mut self) {
        ps5_dump_forge_zstd::hook::set_volume_hook(None);
    }
}

/// Forge's name for a compression level (its `KrakenLevel`); Off stores the blocks raw.
pub fn kraken_level(compression: Compression) -> &'static str {
    match compression {
        Compression::Off => "stored",
        Compression::Fast => "fast",
        Compression::Balanced => "balanced",
        Compression::Smallest => "smallest",
    }
}

pub fn to_forge_format(format: OutputFormat) -> Format {
    match format {
        OutputFormat::Folder => Format::Folder,
        OutputFormat::Exfat => Format::Exfat,
        OutputFormat::Ffpkg => Format::Ffpkg,
        OutputFormat::Ffpfs => Format::Ffpfs,
        OutputFormat::Pkg => Format::Pkg,
    }
}

fn failed(e: impl std::fmt::Display) -> PackError {
    PackError::Failed(format!("{e:#}"))
}

impl Packer for ForgePacker {
    fn pack(
        &self,
        req: &PackRequest,
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<PackOutcome, PackError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(PackError::Cancelled);
        }
        let format = to_forge_format(req.format);
        std::fs::create_dir_all(&req.output_dir)
            .map_err(|e| failed(format!("Couldn't create {}: {e}", req.output_dir.display())))?;
        // A package can't carry files itself. For a folder output the fast way is to pack it
        // first and then unpack the DLC into the finished folder as loose files; for an image
        // there is no folder to add them to, so it is unpacked, folded and packed again.
        let post_bundle = !req.bundle.is_empty()
            && !req.source.is_dir()
            && req.format == OutputFormat::Folder;
        let (pack_source, _staging, staged) = if req.bundle.is_empty() {
            (req.source.clone(), None, None)
        } else if req.source.is_dir() {
            if cancel.load(Ordering::SeqCst) {
                return Err(PackError::Cancelled);
            }
            let (staging, out) = crate::bundle::stage(&req.source, &req.bundle).map_err(PackError::Failed)?;
            (staging.path().to_path_buf(), Some(staging), Some(out))
        } else if post_bundle {
            let checked = crate::bundle::plan(&req.source, &req.bundle);
            if let Some(problem) = checked.problem {
                return Err(PackError::Failed(problem));
            }
            (req.source.clone(), None, None)
        } else {
            let checked = crate::bundle::plan(&req.source, &req.bundle);
            if let Some(problem) = checked.problem {
                return Err(PackError::Failed(problem));
            }
            let unpacked = crate::bundle::unpack_to(&req.source, &req.output_dir).map_err(PackError::Failed)?;
            if cancel.load(Ordering::SeqCst) {
                return Err(PackError::Cancelled);
            }
            let out = crate::bundle::apply(unpacked.path(), &req.bundle).map_err(PackError::Failed)?;
            (unpacked.path().to_path_buf(), Some(unpacked), Some(out))
        };
        // Forge never hands back a path that exists: a second pack gets "-2", "-3", ...
        // A package is Convert: Forge's package writer, no zstd packs (it refuses them).
        let converting = req.format == OutputFormat::Pkg;
        let kind = (!converting).then_some(PackKind::Zstd);
        let output = generated_output(&req.source, format, &req.output_dir, &[], kind, None).map_err(failed)?;
        // Built through serde so every field Forge adds later keeps its own default.
        let request = if converting {
            serde_json::json!({
                "source": pack_source,
                "format": format,
                "output": output,
                // Stored blocks install on every firmware; compressed ones are proven on 5.10 only.
                "kraken_level": kraken_level(req.compression),
                "full_verify": self.full_verify,
                "delete_source": false,
            })
        } else {
            serde_json::json!({
                "source": pack_source,
                "format": format,
                "output": output,
                "zstd_pack": true,
                "zstd_level": req.level.zstd_level(),
                "full_verify": self.full_verify,
                "delete_source": false,
                // A folder pack's space check goes by a sample of the game, not by nothing
                // compressing (third_party/forge-core/README.md).
                "sample_space": req.format == OutputFormat::Folder,
            })
        };
        let request: ConvertRequest = serde_json::from_value(request).map_err(failed)?;

        // Letting parts go: Forge's packer goes through the hook for this pack only (one pack runs
        // at a time; the guard clears it however the pack ends).
        let _hook = req
            .let_go
            .as_ref()
            .filter(|_| req.ahead && req.format == OutputFormat::Folder)
            .map(|let_go| HookGuard::set(std::sync::Arc::new(crate::let_go::Shrink::new(&output, let_go.clone()))));
        let (tx, rx) = mpsc::channel();
        let jobs = Jobs::new(move |event| {
            let _ = tx.send(event);
        });
        let id = jobs.start(request);
        let mut stage = String::new();
        let mut cancel_sent = false;
        // Volumes that can go to the PS5 before the pack ends: a folder output, when asked.
        let mut parts = (req.ahead && req.format == OutputFormat::Folder).then(|| Parts::new(&output, self.copy_parts));
        loop {
            if !cancel_sent && cancel.load(Ordering::SeqCst) {
                jobs.cancel(id);
                cancel_sent = true;
            }
            if let Some(parts) = parts.as_mut() {
                parts.look(progress);
            }
            match rx.recv_timeout(POLL) {
                Ok(Event::Progress { job, stage: now, done, total }) if job == id => {
                    if now != stage {
                        // First: a copy reading a volume in Forge's folder stops before its rename.
                        if let Some(parts) = parts.as_mut() {
                            parts.stage(&now, progress);
                        }
                        stage = now.clone();
                        progress(Progress::Stage(now));
                    }
                    progress(Progress::Bytes { done, total });
                }
                Ok(Event::Done { job, result }) if job == id => {
                    return match result {
                        Ok(report) => {
                            let mut checks = report.checks;
                            if post_bundle {
                                if cancel.load(Ordering::SeqCst) {
                                    let _ = std::fs::remove_dir_all(&report.output);
                                    return Err(PackError::Cancelled);
                                }
                                match crate::bundle::apply(&report.output, &req.bundle) {
                                    Ok(s) => checks.push(bundle_note(&s)),
                                    Err(why) => {
                                        let _ = std::fs::remove_dir_all(&report.output);
                                        return Err(PackError::Failed(why));
                                    }
                                }
                                if cancel.load(Ordering::SeqCst) {
                                    let _ = std::fs::remove_dir_all(&report.output);
                                    return Err(PackError::Cancelled);
                                }
                            } else if let Some(s) = &staged {
                                checks.push(bundle_note(s));
                            }
                            Ok(PackOutcome {
                                output: report.output,
                                bytes: report.bytes,
                                files: report.files,
                                checks,
                            })
                        }
                        Err(why) if why.starts_with("cancelled") => Err(PackError::Cancelled),
                        Err(why) => Err(PackError::Failed(crate::words::plain(&why))),
                    };
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(PackError::Failed("Forge stopped without finishing the pack".into()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn look(parts: &mut Parts) -> Vec<Progress> {
        let seen = RefCell::new(Vec::new());
        parts.last_look = None;
        parts.look(&|p| seen.borrow_mut().push(p));
        seen.into_inner()
    }

    #[test]
    fn a_volume_is_announced_once_the_next_one_starts() {
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("[Ashen Tide]-[PPSA01234]");
        let mut parts = Parts::new(&output, false);
        assert!(look(&mut parts).is_empty(), "no folder yet");

        // Another process's folder, and a look-alike name, are none of ours.
        std::fs::create_dir_all(tmp.path().join("[Ashen Tide]-[PPSA01234].7-1.part")).unwrap();
        std::fs::create_dir_all(tmp.path().join(format!("[Ashen Tide]-[PPSA01234].x-{}.part", std::process::id()))).unwrap();
        let dir = tmp.path().join(format!("[Ashen Tide]-[PPSA01234].7-{}.part", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ampr_zstd-000.pak"), vec![1u8; 300]).unwrap();
        assert!(look(&mut parts).is_empty(), "volume 0 may still grow");

        std::fs::write(dir.join("ampr_zstd-001.pak"), vec![2u8; 10]).unwrap();
        let seen = look(&mut parts);
        let [Progress::PartReady { packed, file, part, copy_first }] = &seen[..] else { panic!("{seen:?}") };
        assert!(!copy_first);
        assert_eq!(packed, &output);
        assert_eq!(file, &dir.join("ampr_zstd-000.pak"));
        assert_eq!(part, &Part { rel: "ampr_zstd-000.pak".into(), size: 300, settled_from: 64 });
        assert!(look(&mut parts).is_empty(), "announced once");

        std::fs::write(dir.join("ampr_zstd-002.pak"), b"").unwrap();
        std::fs::write(dir.join("ampr_zstd-003.pak"), b"").unwrap();
        let seen = look(&mut parts);
        let rels: Vec<_> = seen
            .iter()
            .map(|p| match p {
                Progress::PartReady { part, .. } => part.rel.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(rels, ["ampr_zstd-001.pak", "ampr_zstd-002.pak"]);
    }

    #[test]
    fn writing_ends_at_the_check_and_is_told_once() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(format!("G.1-{}.part", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ampr_zstd-000.pak"), b"x").unwrap();
        let mut parts = Parts::new(&tmp.path().join("G"), true);
        let seen = RefCell::new(Vec::new());
        for stage in ["scan", "write", "pack", "verify", "finalize"] {
            parts.stage(stage, &|p| seen.borrow_mut().push(p));
        }
        assert_eq!(seen.into_inner(), vec![Progress::PartsClosing]);
        // And nothing is announced after it: those volumes go with the rest.
        std::fs::write(dir.join("ampr_zstd-001.pak"), b"x").unwrap();
        assert!(look(&mut parts).is_empty());
    }

    #[test]
    fn on_windows_parts_are_to_be_copied_first() {
        assert_eq!(ForgePacker::default().copy_parts, cfg!(windows));
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(format!("G.1-{}.part", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ampr_zstd-000.pak"), b"x").unwrap();
        std::fs::write(dir.join("ampr_zstd-001.pak"), b"x").unwrap();
        let mut parts = Parts::new(&tmp.path().join("G"), true);
        let seen = look(&mut parts);
        assert!(matches!(&seen[..], [Progress::PartReady { copy_first: true, .. }]), "{seen:?}");
    }

    #[test]
    fn looks_are_spaced_out() {
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("G");
        let dir = tmp.path().join(format!("G.1-{}.part", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut parts = Parts::new(&output, false);
        let n = std::cell::Cell::new(0);
        parts.look(&|_| n.set(n.get() + 1));
        std::fs::write(dir.join("ampr_zstd-000.pak"), b"x").unwrap();
        std::fs::write(dir.join("ampr_zstd-001.pak"), b"x").unwrap();
        parts.look(&|_| n.set(n.get() + 1));
        assert_eq!(n.get(), 0, "too soon after the last look");
        std::thread::sleep(LOOK_EVERY);
        parts.look(&|_| n.set(n.get() + 1));
        assert_eq!(n.get(), 1);
    }
}
