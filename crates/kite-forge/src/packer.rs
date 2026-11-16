//! Packs a game with zstd through Forge's own job runner, then reports Forge's checks.

use kite_pipeline::{OutputFormat, PackError, PackOutcome, PackRequest, Packer, Progress};
use ps5_dump_forge_core::{ConvertRequest, Event, Format, Jobs, PackKind, generated_output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// How often the cancel flag is looked at while Forge is quiet.
const POLL: Duration = Duration::from_millis(200);

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
        loop {
            if !cancel_sent && cancel.load(Ordering::SeqCst) {
                jobs.cancel(id);
                cancel_sent = true;
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
