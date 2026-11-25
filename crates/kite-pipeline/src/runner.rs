//! Runs a job's steps in order, saving the job before every event it reports.

use crate::job::{Job, JobStore, now_unix};
use crate::options::{OutputFormat, SendTarget};
use crate::ports::{LetGo, 
    InstallError, InstallOutcome, Installer, PackError, PackRequest, Packer, Part, Progress, SendError, Sender,
    Sleeper, Trash,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use crate::snapshot::Snapshot;
use crate::steps::{StepKind, StepState};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEvent {
    StepStarted { index: usize },
    Progress { index: usize, progress: Progress },
    StepWaiting { index: usize, since_unix: u64 },
    StepDone { index: usize, note: String },
    StepFailed { index: usize, reason: String },
    Cancelled { index: usize },
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Finished,
    Failed { index: usize },
    Cancelled { index: usize },
}

pub struct Runner<'a> {
    pub packer: &'a dyn Packer,
    pub trash: &'a dyn Trash,
    pub sender: &'a dyn Sender,
    pub installer: &'a dyn Installer,
    pub sleeper: &'a dyn Sleeper,
    pub store: &'a JobStore,
    /// How long to wait between tries while the PS5 is unreachable.
    pub retry_every: Duration,
}

/// What one step came to.
enum StepEnd {
    Done(String),
    Failed(String),
    Cancelled,
    /// Saving failed part way; the step is already marked and reported.
    Aborted(RunOutcome),
}

const ORIGINAL_CHANGED: &str = "The original changed since packing; it was kept";

/// A finished part, on its way from the packer to the PS5 while the pack goes on.
struct Ready {
    packed: PathBuf,
    file: PathBuf,
    part: Part,
    copy_first: bool,
}

/// Spare room a copy leaves on the disk, beyond what the pack may still write.
const COPY_SPARE: u64 = 1 << 30;
/// What a copy leaves free while parts are let go (the space check's own 2% covers it).
const LET_GO_SPARE: u64 = 256 << 20;
/// A copy reads and writes this much at a time, and looks for [`Progress::PartsClosing`] between.
const COPY_STEP: usize = 8 << 20;

/// What the pack step and its part sender share.
#[derive(Default)]
struct Ahead {
    /// The pack is over: send no more parts (the one on its way finishes).
    closing: AtomicBool,
    /// Stop the part on its way too: the pack failed or was cancelled.
    stop: AtomicBool,
    /// A part is on its way right now.
    busy: AtomicBool,
    /// The parts the PS5 has, in the order they went.
    sent: Mutex<Vec<Part>>,
    /// The packer is publishing its folder: no part may be read where it is any more.
    sealed: AtomicBool,
    /// What the pack has left to read, from its last progress: an upper bound on what it may
    /// still write (a stored chunk is never bigger than its source).
    left: AtomicU64,
    /// Where copies of parts are made, once one is: removed when the step ends.
    copies: Mutex<Option<PathBuf>>,
    /// Bytes of the parts the PS5 has whole, and of the one on its way so far.
    sent_bytes: AtomicU64,
    in_flight: AtomicU64,
    /// Bytes of every part announced so far, and the pack's progress when the last one was.
    announced: AtomicU64,
    pack_done: AtomicU64,
    pack_total: AtomicU64,
    /// What the whole packed copy should come to: 0 until a part is announced.
    estimate: AtomicU64,
}

/// How often the bytes on the PS5 are told while they move.
const AHEAD_EVERY: Duration = Duration::from_millis(250);

/// Copies `ready`'s part out of the packer's folder to send the copy instead, `COPY_STEP` at a
/// time, so the original is never held open for long. `None`, and nothing left behind, when the
/// disk hasn't room for the copy and the rest of the pack, when the packer starts publishing its
/// folder (the copy stops at once: its rename must not meet an open file), or on any failure.
fn copy_out(ready: &Ready, ahead: &Ahead, letting_go: bool) -> Option<PathBuf> {
    let name = ready.packed.file_name()?.to_string_lossy().into_owned();
    let dir = ready.packed.parent()?.join(format!(".{name}.kite-copy"));
    // Letting parts go, the pack holds while parts wait and its space check counted the copy
    // among the parts it may hold: only the copy has to fit, with a little to spare.
    let need = if letting_go {
        ready.part.size.saturating_add(LET_GO_SPARE)
    } else {
        ahead.left.load(Ordering::SeqCst).saturating_add(ready.part.size).saturating_add(COPY_SPARE)
    };
    if fs4::available_space(ready.packed.parent()?).ok()? < need {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    *ahead.copies.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.clone());
    let to = dir.join(&ready.part.rel);
    let copied = (|| -> std::io::Result<bool> {
        let mut from = std::fs::File::open(&ready.file)?;
        let mut into = std::fs::File::create(&to)?;
        let mut buf = vec![0u8; COPY_STEP];
        let mut done = 0u64;
        loop {
            if ahead.sealed.load(Ordering::SeqCst) || ahead.stop.load(Ordering::SeqCst) {
                return Ok(false);
            }
            let n = from.read(&mut buf)?;
            if n == 0 {
                break;
            }
            into.write_all(&buf[..n])?;
            done += n as u64;
        }
        Ok(done == ready.part.size)
    })();
    if matches!(copied, Ok(true)) {
        Some(to)
    } else {
        let _ = std::fs::remove_file(&to);
        None
    }
}

/// Sends parts ahead one at a time, as the packer finishes them, until the PS5 can't take one
/// (an older helper, the PS5 away, a failure: whatever is left then goes with the rest).
/// How the worker sending parts ahead waits for a PS5 that can't be reached while parts are let
/// go: `every` between tries, in `STEP`s, so a stop is seen at once.
struct Retry<'a> {
    sleeper: &'a dyn Sleeper,
    every: Duration,
}

const STEP: Duration = Duration::from_millis(50);

fn send_parts_ahead(
    sender: &dyn Sender,
    target: &SendTarget,
    parts: mpsc::Receiver<Ready>,
    ahead: &Ahead,
    let_go: Option<&LetGo>,
    retry: Retry,
) {
    let mut taking = true;
    let over = || ahead.closing.load(Ordering::SeqCst) || ahead.stop.load(Ordering::SeqCst);
    for ready in parts {
        if taking && !over() {
            ahead.busy.store(true, Ordering::SeqCst);
            // A part that can't stay open where it is goes as a copy, or not at all.
            let copy = if ready.copy_first { copy_out(&ready, ahead, let_go.is_some()) } else { None };
            if ready.copy_first && copy.is_none() {
                taking = false;
            } else {
                let file = copy.as_deref().unwrap_or(&ready.file);
                let result = loop {
                    ahead.in_flight.store(0, Ordering::SeqCst);
                    let moved = |bytes: u64| ahead.in_flight.store(bytes.min(ready.part.size), Ordering::SeqCst);
                    let result = sender.send_ahead(&ready.packed, file, &ready.part, target, &moved, &ahead.stop);
                    // Letting parts go, a PS5 that is away is waited for: the packer holds until
                    // it takes this part, rather than filling the disk.
                    if !(matches!(result, Err(SendError::Unreachable(_))) && let_go.is_some()) {
                        break result;
                    }
                    let mut waited = Duration::ZERO;
                    while waited < retry.every && !over() {
                        retry.sleeper.sleep(STEP);
                        waited += STEP;
                    }
                    if over() {
                        break result;
                    }
                };
                match result {
                    Ok(true) => {
                        ahead.sent.lock().unwrap_or_else(|e| e.into_inner()).push(ready.part.clone());
                        ahead.sent_bytes.fetch_add(ready.part.size, Ordering::SeqCst);
                        if let Some(let_go) = let_go {
                            let_go.confirm(&ready.part);
                        }
                    }
                    Ok(false) | Err(_) => taking = false,
                }
                ahead.in_flight.store(0, Ordering::SeqCst);
            }
            if !taking {
                // No more parts will be confirmed: the packer stops holding for them.
                if let Some(let_go) = let_go {
                    let_go.end();
                }
            }
            if let Some(copy) = copy {
                let _ = std::fs::remove_file(copy);
            }
            ahead.busy.store(false, Ordering::SeqCst);
        }
    }
}


/// The packed copy may go only once the PS5 has it (sent, or installed by its installer) and
/// every DLC is confirmed.
fn sent_and_installed(job: &Job) -> bool {
    job.steps
        .iter()
        .filter(|s| matches!(s.kind, StepKind::Send | StepKind::InstallDlc { .. } | StepKind::InstallPackage))
        .all(|s| matches!(s.state, StepState::Done { .. }))
}

impl Runner<'_> {
    pub fn run(&self, job: &mut Job, cancel: &AtomicBool, events: &dyn Fn(RunEvent)) -> RunOutcome {
        let Some(start) = job.next_open() else {
            events(RunEvent::Finished);
            return RunOutcome::Finished;
        };
        if let Some(index) = (start..job.steps.len())
            .find(|&i| matches!(job.steps[i].state, StepState::Failed { .. }))
        {
            return RunOutcome::Failed { index };
        }

        // A failed DLC doesn't stop the next one; the job still ends Failed at the first.
        let mut first_failure: Option<usize> = None;

        for index in start..job.steps.len() {
            if matches!(job.steps[index].state, StepState::Done { .. }) {
                continue;
            }
            if job.steps[index].kind == StepKind::DeletePacked && !sent_and_installed(job) {
                return RunOutcome::Failed { index: first_failure.unwrap_or(index) };
            }
            if cancel.load(Ordering::SeqCst) {
                events(RunEvent::Cancelled { index });
                return RunOutcome::Cancelled { index };
            }
            if let Err(outcome) = self.set(job, index, StepState::Running, events) {
                return outcome;
            }
            events(RunEvent::StepStarted { index });

            let end = match job.steps[index].kind.clone() {
                StepKind::Pack => self.pack(job, index, cancel, events),
                StepKind::DeleteOriginal => self.delete_original(job),
                StepKind::Send => self.send(job, index, cancel, events),
                StepKind::InstallDlc { pkg } => self.install(job, index, &pkg, cancel, events),
                StepKind::InstallPackage => self.install_package(job, index, cancel, events),
                StepKind::DeletePacked => self.delete_packed(job),
            };

            match end {
                StepEnd::Done(note) => {
                    if let Err(o) = self.set(job, index, StepState::Done { note: note.clone() }, events) {
                        return o;
                    }
                    events(RunEvent::StepDone { index, note });
                }
                StepEnd::Failed(reason) => {
                    if let Err(o) = self.set(job, index, StepState::Failed { reason: reason.clone() }, events) {
                        return o;
                    }
                    events(RunEvent::StepFailed { index, reason });
                    if matches!(job.steps[index].kind, StepKind::InstallDlc { .. }) {
                        first_failure.get_or_insert(index);
                        continue;
                    }
                    return RunOutcome::Failed { index };
                }
                StepEnd::Cancelled => {
                    if let Err(o) = self.set(job, index, StepState::Pending, events) {
                        return o;
                    }
                    events(RunEvent::Cancelled { index });
                    return RunOutcome::Cancelled { index };
                }
                StepEnd::Aborted(outcome) => return outcome,
            }
        }
        if let Some(index) = first_failure {
            return RunOutcome::Failed { index };
        }
        events(RunEvent::Finished);
        RunOutcome::Finished
    }

    /// Marks the step Waiting the first time the PS5 doesn't answer; later misses keep the
    /// first `since`, so the screen can say "waiting since 14:02".
    fn waiting(&self, job: &mut Job, index: usize, events: &dyn Fn(RunEvent)) -> Result<(), RunOutcome> {
        if matches!(job.steps[index].state, StepState::Waiting { .. }) {
            return Ok(());
        }
        let since_unix = now_unix();
        self.set(job, index, StepState::Waiting { since_unix }, events)?;
        events(RunEvent::StepWaiting { index, since_unix });
        Ok(())
    }

    /// Bytes are moving after a wait: the PS5 is back, so the step is running again. Saved
    /// before it's reported, like every state. If the save fails the step stays waiting, which
    /// resumes the same way after a restart, and the next progress tries again; the send isn't
    /// stopped for it, and the next step's save still reports a disk that keeps failing.
    fn moving_again(&self, job: &mut Job, index: usize, events: &dyn Fn(RunEvent)) {
        let was = job.steps[index].state.clone();
        if !matches!(was, StepState::Waiting { .. }) {
            return;
        }
        job.steps[index].state = StepState::Running;
        if self.store.save(job).is_ok() {
            events(RunEvent::StepStarted { index });
        } else {
            job.steps[index].state = was;
        }
    }

    /// Waits once between tries. `Some(end)` when the step must stop instead.
    fn wait_and_retry(
        &self,
        job: &mut Job,
        index: usize,
        cancel: &AtomicBool,
        events: &dyn Fn(RunEvent),
    ) -> Option<StepEnd> {
        if let Err(outcome) = self.waiting(job, index, events) {
            return Some(StepEnd::Aborted(outcome));
        }
        self.sleeper.sleep(self.retry_every);
        cancel.load(Ordering::SeqCst).then_some(StepEnd::Cancelled)
    }

    fn send(&self, job: &mut Job, index: usize, cancel: &AtomicBool, events: &dyn Fn(RunEvent)) -> StepEnd {
        let (Some(packed), Some(target)) = (job.packed.clone(), job.options.send.clone()) else {
            return StepEnd::Failed("Nothing packed to send".into());
        };
        loop {
            let ahead = job.ahead.clone();
            let result = {
                // The sender calls back on this thread, so the job can be lent to the callback
                // to mark the step running again when a retry gets going.
                let lent = RefCell::new(&mut *job);
                let progress = |progress: Progress| {
                    self.moving_again(&mut lent.borrow_mut(), index, events);
                    events(RunEvent::Progress { index, progress });
                };
                if ahead.is_empty() {
                    self.sender.send(&packed, &target, &progress, cancel)
                } else {
                    self.sender.send_rest(&packed, &target, &ahead, &progress, cancel)
                }
            };
            match result {
                Ok(()) => return StepEnd::Done("Sent and checked".into()),
                Err(SendError::Unreachable(_)) => {
                    if let Some(end) = self.wait_and_retry(job, index, cancel, events) {
                        return end;
                    }
                }
                Err(SendError::Failed(reason)) => return StepEnd::Failed(reason),
                Err(SendError::Cancelled) => return StepEnd::Cancelled,
            }
        }
    }

    /// The source package, installed as it is: a PS5 away is waited for, as for DLC.
    fn install_package(&self, job: &mut Job, index: usize, cancel: &AtomicBool, events: &dyn Fn(RunEvent)) -> StepEnd {
        let Some(target) = job.options.send.clone() else {
            return StepEnd::Failed("Installing needs the PS5 set up in Settings".into());
        };
        // A converted package once it's written; otherwise the source, a package, as it is.
        let pkg = if job.steps.iter().any(|s| s.kind == StepKind::Pack) {
            match &job.packed {
                Some(packed) => packed.clone(),
                None => return StepEnd::Failed("Nothing converted to install".into()),
            }
        } else {
            job.source.clone()
        };
        loop {
            let result = {
                let lent = RefCell::new(&mut *job);
                let progress = |progress: Progress| {
                    self.moving_again(&mut lent.borrow_mut(), index, events);
                    events(RunEvent::Progress { index, progress });
                };
                self.installer.install_package(&pkg, &target, &progress, cancel)
            };
            match result {
                Ok(InstallOutcome::Installed) => return StepEnd::Done("Installed and confirmed".into()),
                Ok(InstallOutcome::AlreadyThere) => return StepEnd::Done("Already on the PS5".into()),
                Err(InstallError::Unreachable(_)) => {
                    if let Some(end) = self.wait_and_retry(job, index, cancel, events) {
                        return end;
                    }
                }
                Err(InstallError::Refused { code, hint }) => return StepEnd::Failed(format!("{hint} (0x{code:08X})")),
                Err(InstallError::NotConfirmed) => {
                    return StepEnd::Failed("Handed to the PS5 but never showed up".into());
                }
                Err(InstallError::Failed(reason)) => return StepEnd::Failed(reason),
                Err(InstallError::Cancelled) => return StepEnd::Cancelled,
            }
        }
    }

    fn install(
        &self,
        job: &mut Job,
        index: usize,
        pkg: &Path,
        cancel: &AtomicBool,
        events: &dyn Fn(RunEvent),
    ) -> StepEnd {
        let Some(target) = job.options.send.clone() else {
            return StepEnd::Failed("DLC can only be installed when sending to the PS5".into());
        };
        loop {
            let result = match self.installer.is_installed(pkg, &target) {
                Ok(true) => return StepEnd::Done("Already on the PS5".into()),
                Ok(false) => self.installer.install(pkg, &target, cancel),
                Err(e) => Err(e),
            };
            match result {
                Ok(()) => return StepEnd::Done("Installed and confirmed".into()),
                Err(InstallError::Unreachable(_)) => {
                    if let Some(end) = self.wait_and_retry(job, index, cancel, events) {
                        return end;
                    }
                }
                Err(InstallError::Refused { code, hint }) => {
                    return StepEnd::Failed(format!("{hint} (0x{code:08X})"));
                }
                Err(InstallError::NotConfirmed) => {
                    return StepEnd::Failed("Handed to the PS5 but never showed up".into());
                }
                Err(InstallError::Failed(reason)) => return StepEnd::Failed(reason),
                Err(InstallError::Cancelled) => return StepEnd::Cancelled,
            }
        }
    }

    fn delete_packed(&self, job: &Job) -> StepEnd {
        let Some(packed) = &job.packed else {
            return StepEnd::Failed("Nothing packed to delete".into());
        };
        match self.trash.trash(packed) {
            Ok(()) => StepEnd::Done("Moved to the Trash".into()),
            Err(r) => StepEnd::Failed(format!("Couldn't move the packed folder to the Trash: {r}")),
        }
    }

    /// Sets a step's state and saves the job. A failed save fails the step: nothing runs
    /// (and nothing is deleted) on progress that isn't on disk.
    fn set(
        &self,
        job: &mut Job,
        index: usize,
        state: StepState,
        events: &dyn Fn(RunEvent),
    ) -> Result<(), RunOutcome> {
        job.steps[index].state = state;
        if let Err(e) = self.store.save(job) {
            let reason = format!("Couldn't save progress: {e}");
            job.steps[index].state = StepState::Failed { reason: reason.clone() };
            let _ = self.store.save(job);
            events(RunEvent::StepFailed { index, reason });
            return Err(RunOutcome::Failed { index });
        }
        Ok(())
    }

    fn pack(&self, job: &mut Job, index: usize, cancel: &AtomicBool, events: &dyn Fn(RunEvent)) -> StepEnd {
        if job.options.delete_original {
            match Snapshot::take(&job.source) {
                Ok(snapshot) => job.snapshot = Some(snapshot),
                Err(e) => return StepEnd::Failed(format!("Couldn't read the original: {e}")),
            }
        }
        // A packed folder that goes to the PS5 goes part by part as the parts are finished, so
        // sending overlaps packing instead of starting after it.
        let target = job.options.send.clone().filter(|_| job.options.format == OutputFormat::Folder);
        let req = PackRequest {
            source: job.source.clone(),
            output_dir: job.options.output_dir.clone(),
            format: job.options.format,
            level: job.options.level,
            compress: job.options.compress,
            bundle: job.options.bundle.clone(),
            ahead: target.is_some(),
            // Sent and then deleted: each part may leave the PC once the PS5 has it, if the PS5
            // can put the parts' headers right (else the whole folder waits, and the space check
            // counts all of it).
            let_go: target
                .as_ref()
                .filter(|t| job.options.delete_packed && self.sender.lets_parts_go(t))
                .map(|_| Arc::new(LetGo::default())),
        };
        job.ahead.clear();
        let ahead = Ahead::default();
        let let_go = req.let_go.clone();
        let result = std::thread::scope(|scope| {
            let (tx, rx) = mpsc::channel::<Ready>();
            let sender = self.sender;
            let worker = target.as_ref().map(|t| {
                let ahead = &ahead;
                let let_go = let_go.as_deref();
                let retry = Retry { sleeper: self.sleeper, every: self.retry_every };
                scope.spawn(move || send_parts_ahead(sender, t, rx, ahead, let_go, retry))
            });
            // What the PS5 has: whenever a part arrives or one sets off, and every quarter of a
            // second while bytes move (or always, with `force`).
            let shown = Cell::new((0usize, false, 0u64, 0u64));
            let told_at = Cell::new(None::<std::time::Instant>);
            let show_ahead_at = |force: bool| {
                let parts = (ahead.sent.lock().map(|s| s.len()).unwrap_or(0), ahead.busy.load(Ordering::SeqCst));
                let on_ps5 = ahead.sent_bytes.load(Ordering::SeqCst) + ahead.in_flight.load(Ordering::SeqCst);
                let estimate = ahead.estimate.load(Ordering::SeqCst);
                let now = (parts.0, parts.1, on_ps5, estimate);
                let last = shown.get();
                if now == last {
                    return;
                }
                let due = told_at.get().is_none_or(|t| t.elapsed() >= AHEAD_EVERY);
                if force || (parts.0, parts.1) != (last.0, last.1) || due {
                    shown.set(now);
                    told_at.set(Some(std::time::Instant::now()));
                    let progress = Progress::PartsAhead { sent: now.0, sending: now.1, on_ps5, estimate };
                    events(RunEvent::Progress { index, progress });
                }
            };
            let show_ahead = || show_ahead_at(false);
            let progress = |progress: Progress| {
                match progress {
                    // No part sender (not sending, or not a folder): nothing waits for it.
                    Progress::PartReady { packed, file, part, copy_first } => {
                        // The parts so far over how far the pack is: what the whole should come to.
                        let announced = ahead.announced.fetch_add(part.size, Ordering::SeqCst) + part.size;
                        let (done, total) = (ahead.pack_done.load(Ordering::SeqCst), ahead.pack_total.load(Ordering::SeqCst));
                        if done > 0 && total >= done {
                            let estimate = (u128::from(announced) * u128::from(total) / u128::from(done)) as u64;
                            ahead.estimate.store(estimate.max(announced), Ordering::SeqCst);
                        }
                        let _ = tx.send(Ready { packed, file, part, copy_first });
                    }
                    Progress::PartsClosing => ahead.sealed.store(true, Ordering::SeqCst),
                    Progress::Bytes { done, total } => {
                        ahead.left.store(total.saturating_sub(done), Ordering::SeqCst);
                        ahead.pack_done.store(done, Ordering::SeqCst);
                        ahead.pack_total.store(total, Ordering::SeqCst);
                        events(RunEvent::Progress { index, progress: Progress::Bytes { done, total } });
                    }
                    other => events(RunEvent::Progress { index, progress: other }),
                }
                show_ahead();
            };
            let out = self.packer.pack(&req, &progress, cancel);
            if let Some(let_go) = &let_go {
                let_go.end();
            }
            drop(tx);
            ahead.closing.store(true, Ordering::SeqCst);
            if out.is_err() {
                ahead.stop.store(true, Ordering::SeqCst);
            }
            if let Some(worker) = worker {
                // A cancel while the last part finishes stops it too.
                while !worker.is_finished() {
                    if cancel.load(Ordering::SeqCst) {
                        ahead.stop.store(true, Ordering::SeqCst);
                    }
                    show_ahead();
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = worker.join();
                show_ahead_at(true);
            }
            if let Some(dir) = ahead.copies.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = std::fs::remove_dir_all(dir);
            }
            out
        });
        job.ahead = ahead.sent.into_inner().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(out) => {
                job.packed = Some(out.output);
                let mut note = format!("{} files, {} bytes, checked", out.files, out.bytes);
                let n = job.ahead.len();
                if n > 0 {
                    note.push_str(&format!(" · {n} part{} sent while packing", if n == 1 { "" } else { "s" }));
                }
                // What the packer folded in (DLC, a backport), when it says: the screen shows it
                // under the step, so the packed copy's contents are visible before sending.
                if let Some(what) = out.checks.iter().find_map(|c| c.strip_prefix("bundled: ")) {
                    note.push_str(" · ");
                    note.push_str(what);
                }
                StepEnd::Done(note)
            }
            Err(PackError::Failed(reason)) => StepEnd::Failed(reason),
            Err(PackError::Cancelled) => StepEnd::Cancelled,
        }
    }

    fn delete_original(&self, job: &Job) -> StepEnd {
        let unchanged = job
            .snapshot
            .as_ref()
            .is_some_and(|s| s.matches(&job.source).unwrap_or(false));
        if !unchanged {
            return StepEnd::Failed(ORIGINAL_CHANGED.into());
        }
        match self.trash.trash(&job.source) {
            Ok(()) => StepEnd::Done("Moved to the Trash".into()),
            Err(r) => StepEnd::Failed(format!("Couldn't move the original to the Trash: {r}")),
        }
    }
}
