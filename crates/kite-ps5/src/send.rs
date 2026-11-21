//! Sending a packed copy to the Kite helper: the pipeline's Send step.

use crate::connect::{Console, Keys, open};
use crate::{Ps5Error, runtime};
use ava1::manifest;
use ava1::send::{Progress as Ava1Progress, SendError as Ava1SendError, SendOptions, send_job};
use ava1::source::{LocalSource, ReadAt, Source, SourceMeta};
use crate::meta;
use kite_pipeline::{Part, Progress, SendError, SendTarget, Sender};
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

impl Ps5Sender {
    /// Sends one file or folder to `target`'s destination, with nothing to report along the way.
    /// Used to put a DLC package or a backport somewhere on the console for ShadowMount+ to
    /// install; the packed copy goes through [`Sender::send`] instead.
    pub fn upload(&self, local: &Path, target: &SendTarget, cancel: &AtomicBool) -> Result<(), SendError> {
        self.send(local, target, &|_| {}, cancel)
    }
}

impl Ps5Sender {
    /// Sends `local` to `target`, leaving out the top-level files named in `skip` (parts that
    /// went ahead and were found whole). Each skipped part first has its first bytes put right:
    /// they are read from `local`, written with the helper's `fs.patch` and read back.
    fn send_files(
        &self,
        local: &Path,
        target: &SendTarget,
        ahead: &[Part],
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<(), SendError> {
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
        let console = Console::parse(&target.console, self.port);
        runtime().block_on(async {
            let session = open(&console, &self.keys.identity, &self.keys).await.map_err(from_ps5)?;
            let mut skip: Vec<String> = Vec::new();
            if only.is_none() {
                for part in ahead {
                    if fixed_up(&session, local, &root, part).await {
                        skip.push(part.rel.clone());
                    }
                }
            }
            let source = LocalSource::new(source_dir);
            let files = manifest::walk(&source, &|rel: &str| {
                only.as_deref().is_some_and(|f| rel != f) || skip.iter().any(|s| s == rel)
            })
            .map_err(|e| SendError::Failed(format!("Couldn't read the packed copy: {e}")))?;

            let id = job_id_skipping(local, target, &skip);
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

/// One file as a whole source, opened once and read by position. The path can change under
/// it (Forge publishes a pack by renaming its folder) without the send noticing.
struct HeldFile {
    rel: String,
    file: Arc<std::fs::File>,
    meta: SourceMeta,
}

impl HeldFile {
    fn open(path: &Path, rel: &str) -> std::io::Result<HeldFile> {
        let file = std::fs::File::open(path)?;
        let m = file.metadata()?;
        let mtime = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let meta = SourceMeta { size: m.len(), mtime, mode: 0o644, is_dir: false };
        Ok(HeldFile { rel: rel.to_string(), file: Arc::new(file), meta })
    }
}

struct Positional(Arc<std::fs::File>);

impl ReadAt for Positional {
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        #[cfg(unix)]
        return std::os::unix::fs::FileExt::read_at(&*self.0, buf, off);
        #[cfg(windows)]
        return std::os::windows::fs::FileExt::seek_read(&*self.0, buf, off);
    }
}

impl Source for HeldFile {
    fn open(&self, rel: &str) -> std::io::Result<Box<dyn ReadAt>> {
        if rel != self.rel {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        Ok(Box::new(Positional(self.file.clone())))
    }

    fn list(&self, rel: &str) -> std::io::Result<Vec<(String, SourceMeta)>> {
        if !rel.is_empty() {
            return Err(std::io::ErrorKind::NotFound.into());
        }
        Ok(vec![(self.rel.clone(), self.meta)])
    }

    fn stat(&self, rel: &str) -> std::io::Result<SourceMeta> {
        match rel {
            "" => Ok(SourceMeta { size: 0, mtime: self.meta.mtime, mode: 0o755, is_dir: true }),
            r if r == self.rel => Ok(self.meta),
            _ => Err(std::io::ErrorKind::NotFound.into()),
        }
    }
}

/// Whether `part`, sent ahead, is on the console whole at `<root>/<rel>`, with its first bytes
/// now matching `local`'s: any doubt, and it goes again with the rest.
async fn fixed_up(session: &ava1::session::Session, local: &Path, root: &str, part: &Part) -> bool {
    if part.rel.is_empty() || part.rel.contains('/') || part.settled_from as usize > meta::PATCH_MAX {
        return false;
    }
    let file = local.join(&part.rel);
    if std::fs::metadata(&file).map(|m| m.len()).ok() != Some(part.size) {
        return false;
    }
    let remote = format!("{root}/{}", part.rel);
    match meta::stat(session, &remote).await {
        Ok(e) if e.kind == "file" && e.size == part.size => {}
        _ => return false,
    }
    if part.settled_from == 0 {
        return true;
    }
    let head = match read_head(&file, part.settled_from as usize) {
        Ok(head) => head,
        Err(_) => return false,
    };
    if meta::patch(session, &remote, 0, &head).await.is_err() {
        return false;
    }
    matches!(meta::read_at(session, &remote, 0, head.len()).await, Ok(back) if back == head)
}

fn read_head(file: &Path, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut head = vec![0; len];
    std::fs::File::open(file)?.read_exact(&mut head)?;
    Ok(head)
}

/// The job for `local` with `skip` left out: the same id as [`job_id`] when nothing is, so a
/// send that ran before parts went ahead resumes as it did.
fn job_id_skipping(local: &Path, target: &SendTarget, skip: &[String]) -> [u8; 16] {
    let mut id = job_id(local, target);
    if !skip.is_empty() {
        let extra = job_id(Path::new(&skip.join("\0")), &SendTarget { console: "ahead".into(), destination: String::new() });
        for (a, b) in id.iter_mut().zip(extra) {
            *a ^= b;
        }
    }
    id
}

impl Sender for Ps5Sender {
    fn send(&self, local: &Path, target: &SendTarget, progress: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<(), SendError> {
        self.send_files(local, target, &[], progress, cancel)
    }

    fn send_rest(
        &self,
        local: &Path,
        target: &SendTarget,
        ahead: &[Part],
        progress: &dyn Fn(Progress),
        cancel: &AtomicBool,
    ) -> Result<(), SendError> {
        self.send_files(local, target, ahead, progress, cancel)
    }

    /// Sends one finished part of `packed` to `<destination>/<packed's name>/<rel>`, alone in its
    /// own job, once the helper is known to take `fs.patch` (or its header could never be put
    /// right, and sending it early would only send it twice).
    fn send_ahead(
        &self,
        packed: &Path,
        file: &Path,
        part: &Part,
        target: &SendTarget,
        cancel: &AtomicBool,
    ) -> Result<bool, SendError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(SendError::Cancelled);
        }
        let Some(name) = packed.file_name() else { return Ok(false) };
        if part.rel.contains('/') || file.file_name().is_none_or(|f| f.to_string_lossy() != part.rel) {
            return Ok(false);
        }
        let root = format!("{}/{}", target.destination.trim_end_matches('/'), name.to_string_lossy());
        let console = Console::parse(&target.console, self.port);
        runtime().block_on(async {
            let session = open(&console, &self.keys.identity, &self.keys).await.map_err(from_ps5)?;
            match meta::can_patch(&session).await {
                Ok(true) => {}
                Ok(false) => {
                    session.close().await;
                    return Ok(false);
                }
                Err(e) => {
                    session.close().await;
                    return Err(from_ps5(meta::into_ps5(e)));
                }
            }
            // Opened once: Forge renames the folder it is in when the pack is published.
            let source = HeldFile::open(file, &part.rel)
                .map_err(|e| SendError::Failed(format!("Couldn't read the packed copy: {e}")))?;
            let files = manifest::walk(&source, &|_: &str| false)
                .map_err(|e| SendError::Failed(format!("Couldn't read the packed copy: {e}")))?;
            let id = job_id(Path::new(&format!("{root}/{}", part.rel)), target);
            let stop = Arc::new(AtomicBool::new(false));
            let mut opts = SendOptions::upload(&root);
            opts.cancel = stop.clone();
            opts.bandwidth_cap = self.bandwidth_cap;
            opts.persist = Some(self.outboards.join(hex(&id)));
            let mut job = session.job(id);
            let result = {
                let send = send_job(&mut job, Arc::new(files), Arc::new(source), opts);
                tokio::pin!(send);
                let mut tick = tokio::time::interval(PROGRESS_EVERY);
                loop {
                    tokio::select! {
                        result = &mut send => break result,
                        _ = tick.tick() => {
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
                Ok(done) if done.status == ava1::r#gen::STATUS_OK => Ok(true),
                Ok(done) => Err(SendError::Failed(
                    done.message.unwrap_or_else(|| format!("The PS5 ended the send ({})", done.status)),
                )),
                Err(e) => Err(from_ava1(e)),
            }
        })
    }
}
