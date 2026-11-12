//! Sending a packed copy to the Kite helper: the pipeline's Send step.

use crate::connect::{Console, Keys, open};
use crate::{Ps5Error, runtime};
use ava1::manifest;
use ava1::send::{Progress as Ava1Progress, SendError as Ava1SendError, SendOptions, send_job};
use ava1::source::LocalSource;
use kite_pipeline::{Progress, SendError, SendTarget, Sender};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const PROGRESS_EVERY: Duration = Duration::from_millis(200);

pub struct Ps5Sender {
    keys: Keys,
    port: u16,
    /// Where the sender keeps what lets a restarted send skip re-reading (AVA1 outboards).
    outboards: PathBuf,
    /// Bytes per second for the whole send; None for as fast as the network goes.
    pub bandwidth_cap: Option<u64>,
}

impl Ps5Sender {
    pub fn new(keys: Keys, port: u16, outboards: PathBuf) -> Ps5Sender {
        Ps5Sender { keys, port, outboards, bandwidth_cap: None }
    }
}

/// The same packed copy to the same place always gets the same job, so a retry resumes it
/// from the helper's journal. FNV-1a over the two paths, twice with different seeds.
fn job_id(local: &Path, target: &SendTarget) -> [u8; 16] {
    let text = format!("{}\0{}\0{}", local.display(), target.console, target.destination);
    let fnv = |seed: u64| {
        text.bytes().fold(seed, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3))
    };
    let mut id = [0u8; 16];
    id[..8].copy_from_slice(&fnv(0xcbf2_9ce4_8422_2325).to_le_bytes());
    id[8..].copy_from_slice(&fnv(0x6c62_272e_07bb_0142).to_le_bytes());
    id
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_ps5(e: Ps5Error) -> SendError {
    match e {
        Ps5Error::Unreachable(why) => SendError::Unreachable(why),
        other => SendError::Failed(other.message()),
    }
}

fn from_ava1(e: Ava1SendError) -> SendError {
    match e {
        Ava1SendError::Cancelled => SendError::Cancelled,
        Ava1SendError::Disconnected(why) => SendError::Unreachable(why),
        Ava1SendError::OpenTimeout(_) => SendError::Unreachable("the PS5 didn't answer".into()),
        Ava1SendError::Source(io) => SendError::Failed(format!("Couldn't read the packed copy: {io}")),
        Ava1SendError::Refused { message, .. } => SendError::Failed(format!("The PS5 refused it: {message}")),
        other => SendError::Failed(other.to_string()),
    }
}

impl Sender for Ps5Sender {
    fn send(&self, local: &Path, target: &SendTarget, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<(), SendError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(SendError::Cancelled);
        }
        let name = local
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| SendError::Failed("The packed copy has no name".into()))?;
        let destination = target.destination.trim_end_matches('/');
        // A folder lands as <destination>/<folder>; an image file as <destination>/<file>.
        let (source_dir, only, root) = if local.is_dir() {
            (local.to_path_buf(), None, format!("{destination}/{name}"))
        } else {
            let parent = local.parent().unwrap_or(Path::new(".")).to_path_buf();
            (parent, Some(name), destination.to_string())
        };
        let source = LocalSource::new(source_dir);
        let files = manifest::walk(&source, &|rel: &str| only.as_deref().is_some_and(|f| rel != f))
            .map_err(|e| SendError::Failed(format!("Couldn't read the packed copy: {e}")))?;

        let id = job_id(local, target);
        let console = Console { address: target.console.clone(), port: self.port };
        runtime().block_on(async {
            let session = open(&console, &self.keys).await.map_err(from_ps5)?;
            let counters = Arc::new(Ava1Progress::default());
            let stop = Arc::new(AtomicBool::new(false));
            let mut opts = SendOptions::upload(&root);
            opts.progress = counters.clone();
            opts.cancel = stop.clone();
            opts.bandwidth_cap = self.bandwidth_cap;
            opts.persist = Some(self.outboards.join(hex(&id)));
            let mut job = session.job(id);
            let report = |counters: &Ava1Progress| {
                let total = counters.bytes_total.load(Ordering::Relaxed);
                if total > 0 {
                    progress(Progress::Bytes { done: counters.bytes_durable.load(Ordering::Relaxed), total });
                }
            };

            let result = {
                let send = send_job(&mut job, Arc::new(files), Arc::new(source), opts);
                tokio::pin!(send);
                let mut tick = tokio::time::interval(PROGRESS_EVERY);
                loop {
                    tokio::select! {
                        result = &mut send => break result,
                        _ = tick.tick() => {
                            report(&counters);
                            if cancel.load(Ordering::SeqCst) {
                                stop.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                }
            };
            drop(job);
            session.close().await;
            match result {
                Ok(done) if done.status == ava1::r#gen::STATUS_OK => {
                    report(&counters);
                    Ok(())
                }
                Ok(done) => Err(SendError::Failed(
                    done.message.unwrap_or_else(|| format!("The PS5 ended the send ({})", done.status)),
                )),
                Err(e) => Err(from_ava1(e)),
            }
        })
    }
}
