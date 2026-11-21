//! Packs a game with zstd through Forge's own job runner, then reports Forge's checks.

use kite_pipeline::{OutputFormat, PackError, PackOutcome, PackRequest, Packer, Part, Progress};
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
/// A volume is announced where it is, in the `.part` folder: the sender opens it once and keeps
/// it open, which outlives the rename. It can't be linked elsewhere instead: Forge's check
/// refuses a file with two names. Not on Windows, where a file held open in a folder stops the
/// folder's rename (and so the pack): there every volume goes after the pack, as before.
pub(crate) struct Parts {
    output: PathBuf,
    part_dir: Option<PathBuf>,
    next: u32,
    last_look: Option<Instant>,
}

impl Parts {
    pub(crate) fn new(output: &Path) -> Parts {
        Parts { output: output.to_path_buf(), part_dir: None, next: 0, last_look: None }
    }

    /// Forge's `.part` folder for this output, made by this process.
    fn find_part_dir(&self) -> Option<PathBuf> {
        let name = self.output.file_name()?.to_string_lossy().into_owned();
        let suffix = format!("-{}.part", std::process::id());
        let parent = self.output.parent()?;
        std::fs::read_dir(parent).ok()?.flatten().find_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let job = n.strip_prefix(&name)?.strip_prefix('.')?.strip_suffix(&suffix)?;
            (job.bytes().all(|b| b.is_ascii_digit()) && e.path().is_dir()).then(|| e.path())
        })
    }

    /// Announces every volume finished since the last look (at most every [`LOOK_EVERY`]).
    pub(crate) fn look(&mut self, progress: &dyn Fn(Progress)) {
        if self.last_look.is_some_and(|t| t.elapsed() < LOOK_EVERY) {
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
            });
        }
    }
}

pub struct ForgePacker {
    /// Re-read every byte of the output instead of Forge's fast sampled check.
    pub full_verify: bool,
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

pub fn to_forge_format(format: OutputFormat) -> Format {
    match format {
        OutputFormat::Folder => Format::Folder,
        OutputFormat::Exfat => Format::Exfat,
        OutputFormat::Ffpkg => Format::Ffpkg,
        OutputFormat::Ffpfs => Format::Ffpfs,
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
        let output = generated_output(&req.source, format, &req.output_dir, &[], Some(PackKind::Zstd), None)
            .map_err(failed)?;
        // Built through serde so every field Forge adds later keeps its own default.
        let request: ConvertRequest = serde_json::from_value(serde_json::json!({
            "source": pack_source,
            "format": format,
            "output": output,
            "zstd_pack": true,
            "zstd_level": req.level.zstd_level(),
            "full_verify": self.full_verify,
            "delete_source": false,
        }))
        .map_err(failed)?;

        let (tx, rx) = mpsc::channel();
        let jobs = Jobs::new(move |event| {
            let _ = tx.send(event);
        });
        let id = jobs.start(request);
        let mut stage = String::new();
        let mut cancel_sent = false;
        // Volumes that can go to the PS5 before the pack ends: a folder output, when asked.
        let mut parts =
            (req.ahead && req.format == OutputFormat::Folder && !cfg!(windows)).then(|| Parts::new(&output));
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
                        Err(why) => Err(PackError::Failed(why)),
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
        let mut parts = Parts::new(&output);
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
        let [Progress::PartReady { packed, file, part }] = &seen[..] else { panic!("{seen:?}") };
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
    fn looks_are_spaced_out() {
        let tmp = tempfile::tempdir().unwrap();
        let output = tmp.path().join("G");
        let dir = tmp.path().join(format!("G.1-{}.part", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut parts = Parts::new(&output);
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
