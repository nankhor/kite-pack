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
        // Forge never hands back a path that exists: a second pack gets "-2", "-3", ...
        let output = generated_output(&req.source, format, &req.output_dir, &[], Some(PackKind::Zstd), None)
            .map_err(failed)?;
        // Built through serde so every field Forge adds later keeps its own default.
        let request: ConvertRequest = serde_json::from_value(serde_json::json!({
            "source": req.source,
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
                        Ok(report) => Ok(PackOutcome {
                            output: report.output,
                            bytes: report.bytes,
                            files: report.files,
                            checks: report.checks,
                        }),
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
