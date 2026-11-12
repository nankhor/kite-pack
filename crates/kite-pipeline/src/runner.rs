//! Runs a job's steps in order, saving the job before every event it reports.

use crate::job::{Job, JobStore};
use crate::ports::{
    Installer, PackError, PackRequest, Packer, Progress, Sender, Sleeper, Trash,
};
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
}

const ORIGINAL_CHANGED: &str = "The original changed since packing; it was kept";

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

        for index in start..job.steps.len() {
            if matches!(job.steps[index].state, StepState::Done { .. }) {
                continue;
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
                StepKind::Send | StepKind::InstallDlc { .. } | StepKind::DeletePacked => {
                    StepEnd::Failed("not implemented".into())
                }
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
                    return RunOutcome::Failed { index };
                }
                StepEnd::Cancelled => {
                    if let Err(o) = self.set(job, index, StepState::Pending, events) {
                        return o;
                    }
                    events(RunEvent::Cancelled { index });
                    return RunOutcome::Cancelled { index };
                }
            }
        }
        events(RunEvent::Finished);
        RunOutcome::Finished
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
