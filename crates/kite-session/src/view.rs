//! What the screen receives: a job as plain data, and the events that update it.

use kite_pipeline::{Job, StepKind, StepState};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum UiEvent {
    /// The whole job, after every step change and once at the end.
    Job { job: JobView },
    /// What the running step is doing now, in Forge's words (`scan`, `write`, ...).
    Stage { id: String, index: usize, stage: String },
    /// Bytes done in the running step; at most every 100 ms.
    Progress { id: String, index: usize, done: u64, total: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Running,
    /// Not running and nothing failed: left by a closed app, ready to carry on.
    Paused,
    Finished,
    Failed,
    /// Stopped by the person; the job is gone.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub source: PathBuf,
    pub output_dir: PathBuf,
    pub delete_original: bool,
    pub packed: Option<PathBuf>,
    /// The packed copy's size on disk, once the job has finished.
    pub packed_bytes: Option<u64>,
    pub steps: Vec<StepView>,
    pub state: JobState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    /// `pack`, `deleteOriginal`, `send`, `installDlc` or `deletePacked`.
    pub kind: String,
    /// `pending`, `running`, `waiting`, `done` or `failed`.
    pub state: String,
    /// What a done step came to, or why a step failed.
    pub note: Option<String>,
}

impl JobView {
    pub fn of(job: &Job, state: JobState) -> JobView {
        JobView {
            id: job.id.clone(),
            source: job.source.clone(),
            output_dir: job.options.output_dir.clone(),
            delete_original: job.options.delete_original,
            packed: job.packed.clone(),
            packed_bytes: None,
            steps: job.steps.iter().map(|s| step_view(&s.kind, &s.state)).collect(),
            state,
        }
    }

    /// A job that isn't running: failed if a step failed, otherwise paused.
    pub fn at_rest(job: &Job) -> JobView {
        let failed = job.steps.iter().any(|s| matches!(s.state, StepState::Failed { .. }));
        JobView::of(job, if failed { JobState::Failed } else { JobState::Paused })
    }
}

fn step_view(kind: &StepKind, state: &StepState) -> StepView {
    let kind = match kind {
        StepKind::Pack => "pack",
        StepKind::DeleteOriginal => "deleteOriginal",
        StepKind::Send => "send",
        StepKind::InstallDlc { .. } => "installDlc",
        StepKind::DeletePacked => "deletePacked",
    };
    let (state, note) = match state {
        StepState::Pending => ("pending", None),
        StepState::Running => ("running", None),
        StepState::Waiting { .. } => ("waiting", None),
        StepState::Done { note } => ("done", Some(note.clone())),
        StepState::Failed { reason } => ("failed", Some(reason.clone())),
    };
    StepView { kind: kind.into(), state: state.into(), note }
}

/// The size on disk of a packed copy: a file's length, or the sum of a folder's files.
/// Zero when it can't be read.
pub fn packed_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}
