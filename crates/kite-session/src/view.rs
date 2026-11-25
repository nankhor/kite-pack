//! What the screen receives: a job as plain data, and the events that update it.

use kite_pipeline::{Job, OutputFormat, StepKind, StepState};
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
    /// While Pack runs: parts of the packed copy already on the PS5, whether the next one is
    /// on its way, the bytes on the PS5, and what the packed copy should come to (0: not known
    /// yet).
    Ahead {
        id: String,
        index: usize,
        sent: usize,
        sending: bool,
        #[serde(rename = "onPs5")]
        on_ps5: u64,
        estimate: u64,
    },
    /// The window was asked to close while an install runs: the screen asks whether to.
    CloseAsked,
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
    /// What it packs to: `pkg` is Convert, which has a page of its own.
    pub format: OutputFormat,
    pub delete_original: bool,
    pub packed: Option<PathBuf>,
    /// The packed copy's size on disk, once the job has finished.
    pub packed_bytes: Option<u64>,
    /// Bytes of the packed copy that went to the PS5 while it was packed: Send leaves them
    /// out, so its own progress counts only the rest.
    pub ahead_bytes: u64,
    pub steps: Vec<StepView>,
    pub state: JobState,
    /// Grows with every view the session hands out, so the screen keeps the newest when an
    /// event and a command's reply cross.
    pub rev: u64,
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
    /// The package an `installDlc` step installs; none for the other steps.
    pub pkg: Option<PathBuf>,
}

impl JobView {
    pub fn of(job: &Job, state: JobState) -> JobView {
        JobView {
            id: job.id.clone(),
            source: job.source.clone(),
            output_dir: job.options.output_dir.clone(),
            format: job.options.format,
            delete_original: job.options.delete_original,
            packed: job.packed.clone(),
            packed_bytes: None,
            ahead_bytes: job.ahead.iter().map(|p| p.size).sum(),
            steps: job.steps.iter().map(|s| step_view(&s.kind, &s.state)).collect(),
            state,
            rev: 0,
        }
    }

    /// A job that isn't running: failed if a step failed, otherwise paused.
    pub fn at_rest(job: &Job) -> JobView {
        let failed = job.steps.iter().any(|s| matches!(s.state, StepState::Failed { .. }));
        JobView::of(job, if failed { JobState::Failed } else { JobState::Paused })
    }
}

fn step_view(kind: &StepKind, state: &StepState) -> StepView {
    let pkg = match kind {
        StepKind::InstallDlc { pkg } => Some(pkg.clone()),
        _ => None,
    };
    let kind = match kind {
        StepKind::Pack => "pack",
        StepKind::DeleteOriginal => "deleteOriginal",
        StepKind::Send => "send",
        StepKind::InstallDlc { .. } => "installDlc",
        StepKind::InstallPackage => "installPackage",
        StepKind::DeletePacked => "deletePacked",
    };
    let (state, note) = match state {
        StepState::Pending => ("pending", None),
        StepState::Running => ("running", None),
        StepState::Waiting { .. } => ("waiting", None),
        StepState::Done { note } => ("done", Some(note.clone())),
        StepState::Failed { reason } => ("failed", Some(reason.clone())),
    };
    StepView { kind: kind.into(), state: state.into(), note, pkg }
}

/// The size on disk of a packed copy: a file's length, or the sum of a folder's files.
/// Zero when it can't be read.
/// [`packed_size`], counting each of `parts` (sent while packing) at its full size where the
/// packed folder holds it cut down to its header (letting parts go).
pub fn packed_size_with(path: &Path, parts: &[kite_pipeline::Part]) -> u64 {
    let cut: u64 = parts
        .iter()
        .filter_map(|p| std::fs::metadata(path.join(&p.rel)).ok().map(|m| p.size.saturating_sub(m.len())))
        .sum();
    packed_size(path) + cut
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use kite_pipeline::{Level, Options, SendTarget};

    #[test]
    fn dlc_steps_name_their_package() {
        let options = Options {
            format: OutputFormat::Folder,
            level: Level::Fast,
            output_dir: PathBuf::from("/out"),
            delete_original: false,
            send: Some(SendTarget { console: "ps5".into(), destination: "/data/homebrew".into() }),
            delete_packed: false,
            dlc: vec![PathBuf::from("/dlc/suits.pkg"), PathBuf::from("/dlc/map.pkg")],
            bundle: Default::default(),
            install: false,
            compress: false,
        };
        let job = Job::new(PathBuf::from("/games/ashen"), options).unwrap();
        let view = JobView::at_rest(&job);
        let dlc: Vec<_> = view.steps.iter().filter(|s| s.kind == "installDlc").map(|s| s.pkg.clone()).collect();
        assert_eq!(dlc, vec![Some(PathBuf::from("/dlc/suits.pkg")), Some(PathBuf::from("/dlc/map.pkg"))]);
        assert!(view.steps.iter().filter(|s| s.kind != "installDlc").all(|s| s.pkg.is_none()));
    }

    #[test]
    fn a_view_says_its_format_so_the_screen_finds_its_page() {
        let options = Options {
            format: OutputFormat::Pkg,
            level: Level::Fast,
            output_dir: PathBuf::from("/Packages"),
            delete_original: false,
            send: None,
            delete_packed: false,
            dlc: vec![],
            bundle: Default::default(),
            install: false,
            compress: false,
        };
        let job = Job::new(PathBuf::from("/games/ashen"), options).unwrap();
        let json = serde_json::to_value(JobView::at_rest(&job)).unwrap();
        assert_eq!(json["format"], "pkg");
    }
}
