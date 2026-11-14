//! Runs a job's steps in order, saving the job before every event it reports.

use crate::job::{Job, JobStore, now_unix};
use crate::ports::{
    InstallError, Installer, PackError, PackRequest, Packer, Progress, SendError, Sender,
    Sleeper, Trash,
};
use std::cell::RefCell;
use std::path::Path;
use crate::snapshot::Snapshot;
use crate::steps::{StepKind, StepState};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// The packed copy may go only once the PS5 has it and every DLC is confirmed.
fn sent_and_installed(job: &Job) -> bool {
    job.steps
        .iter()
        .filter(|s| matches!(s.kind, StepKind::Send | StepKind::InstallDlc { .. }))
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
            let result = {
                // The sender calls back on this thread, so the job can be lent to the callback
                // to mark the step running again when a retry gets going.
                let lent = RefCell::new(&mut *job);
                let progress = |progress: Progress| {
                    self.moving_again(&mut lent.borrow_mut(), index, events);
                    events(RunEvent::Progress { index, progress });
                };
                self.sender.send(&packed, &target, &progress, cancel)
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
        let req = PackRequest {
            source: job.source.clone(),
            output_dir: job.options.output_dir.clone(),
            format: job.options.format,
            level: job.options.level,
            bundle: job.options.bundle.clone(),
        };
        let progress = |progress: Progress| events(RunEvent::Progress { index, progress });
        match self.packer.pack(&req, &progress, cancel) {
            Ok(out) => {
                job.packed = Some(out.output);
                StepEnd::Done(format!("{} files, {} bytes, checked", out.files, out.bytes))
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
