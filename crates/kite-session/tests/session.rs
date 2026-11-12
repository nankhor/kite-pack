mod common;

use common::make_game;
use kite_forge::ForgePacker;
use kite_pipeline::{
    Job, JobStore, Level, Options, OutputFormat, PackError, PackOutcome, PackRequest, Packer,
    Progress, StepState, Trash,
};
use kite_session::{Emit, JobState, JobView, Ports, Session, UiEvent, packed_size};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Moves trashed paths into a bin folder, so tests never touch the real Trash.
struct BinTrash(PathBuf);

impl Trash for BinTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        std::fs::create_dir_all(&self.0).map_err(|e| e.to_string())?;
        std::fs::rename(path, self.0.join(path.file_name().unwrap())).map_err(|e| e.to_string())
    }
}

/// Packs nothing: waits until cancelled, so a cancel always lands mid-step.
struct WaitingPacker;

impl Packer for WaitingPacker {
    fn pack(&self, _: &PackRequest, _: &dyn Fn(Progress), cancel: &AtomicBool) -> Result<PackOutcome, PackError> {
        let start = Instant::now();
        while !cancel.load(Ordering::SeqCst) {
            assert!(start.elapsed() < Duration::from_secs(30), "never cancelled");
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(PackError::Cancelled)
    }
}

struct World {
    tmp: tempfile::TempDir,
    game: PathBuf,
    out: PathBuf,
    bin: PathBuf,
    jobs: PathBuf,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let game = make_game(tmp.path(), "Ashen Tide");
        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        World { bin: tmp.path().join("bin"), jobs: tmp.path().join("jobs"), game, out, tmp }
    }

    fn session(&self) -> Session {
        self.session_with(Arc::new(ForgePacker { full_verify: false }))
    }

    fn session_with(&self, packer: Arc<dyn Packer>) -> Session {
        let ports = Ports { packer, trash: Arc::new(BinTrash(self.bin.clone())) };
        Session::new(self.jobs.clone(), ports).unwrap()
    }

    fn options(&self, delete_original: bool) -> Options {
        Options {
            format: OutputFormat::Folder,
            level: Level::Fast,
            output_dir: self.out.clone(),
            delete_original,
            send: None,
            delete_packed: false,
            dlc: vec![],
        }
    }
}

/// Collects every event the session sends.
#[derive(Clone, Default)]
struct Events(Arc<Mutex<Vec<UiEvent>>>);

impl Events {
    fn emit(&self) -> Emit {
        let events = self.0.clone();
        Arc::new(move |e| events.lock().unwrap().push(e))
    }

    fn jobs(&self) -> Vec<JobView> {
        let events = self.0.lock().unwrap();
        events
            .iter()
            .filter_map(|e| match e {
                UiEvent::Job { job } => Some(job.clone()),
                _ => None,
            })
            .collect()
    }

    /// Waits for the job to stop running and returns how it ended.
    fn last(&self) -> JobView {
        let start = Instant::now();
        loop {
            if let Some(job) = self.jobs().into_iter().rev().find(|j| j.state != JobState::Running) {
                return job;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "the job never ended: {:?}", self.jobs());
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn kinds_and_states(job: &JobView) -> Vec<(String, String)> {
    job.steps.iter().map(|s| (s.kind.clone(), s.state.clone())).collect()
}

fn pair(kind: &str, state: &str) -> (String, String) {
    (kind.to_string(), state.to_string())
}

#[test]
fn start_packs_and_reports_the_packed_size() {
    let w = World::new();
    let session = w.session();
    let events = Events::default();
    let started = session.start(w.game.clone(), w.options(false), events.emit()).unwrap();
    assert_eq!(kinds_and_states(&started), vec![pair("pack", "pending")]);

    let done = events.last();
    assert_eq!(done.state, JobState::Finished);
    assert_eq!(kinds_and_states(&done), vec![pair("pack", "done")]);
    let packed = done.packed.clone().unwrap();
    assert!(packed.join("ampr_zstd.index").is_file());
    assert_eq!(done.packed_bytes, Some(packed_size(&packed)));
    assert!(events.jobs().iter().any(|j| j.state == JobState::Running), "a running view came first");
    assert!(!session.is_running());
}

#[test]
fn delete_original_moves_the_game_to_the_trash() {
    let w = World::new();
    let session = w.session();
    let events = Events::default();
    session.start(w.game.clone(), w.options(true), events.emit()).unwrap();
    let done = events.last();
    assert_eq!(done.state, JobState::Finished);
    assert_eq!(kinds_and_states(&done), vec![pair("pack", "done"), pair("deleteOriginal", "done")]);
    assert!(!w.game.exists());
    assert!(w.bin.join("Ashen Tide").is_dir());
}

#[test]
fn second_start_while_running_is_refused() {
    let w = World::new();
    let session = w.session_with(Arc::new(WaitingPacker));
    let events = Events::default();
    session.start(w.game.clone(), w.options(false), events.emit()).unwrap();
    let again = session.start(w.game.clone(), w.options(false), Events::default().emit());
    assert_eq!(again.unwrap_err(), "A game is already being packed");
    assert!(session.is_running());
    session.cancel();
    assert_eq!(events.last().state, JobState::Cancelled);
}

#[test]
fn cancel_removes_the_job() {
    let w = World::new();
    let session = w.session_with(Arc::new(WaitingPacker));
    let events = Events::default();
    session.start(w.game.clone(), w.options(false), events.emit()).unwrap();
    session.cancel();
    let end = events.last();
    assert_eq!(end.state, JobState::Cancelled);
    assert!(session.unfinished().is_none(), "a cancelled job must not come back on the next launch");
    assert_eq!(std::fs::read_dir(&w.out).unwrap().count(), 0);
    assert!(w.game.is_dir());
}

#[test]
fn unfinished_returns_a_job_left_mid_run() {
    let w = World::new();
    let store = JobStore::open(w.jobs.clone()).unwrap();
    let mut job = Job::new(w.game.clone(), w.options(false)).unwrap();
    job.steps[0].state = StepState::Running; // the app died here
    store.save(&job).unwrap();

    let session = w.session();
    let left = session.unfinished().unwrap();
    assert_eq!(left.id, job.id);
    assert_eq!(left.state, JobState::Paused);
    assert_eq!(kinds_and_states(&left), vec![pair("pack", "pending")]);

    let events = Events::default();
    session.carry_on(&job.id, events.emit()).unwrap();
    assert_eq!(events.last().state, JobState::Finished);
    assert!(session.unfinished().is_none());
}

#[test]
fn failed_job_is_reported_and_retry_works() {
    let w = World::new();
    std::fs::remove_dir(&w.out).unwrap();
    std::fs::write(&w.out, b"in the way").unwrap();
    let session = w.session();
    let events = Events::default();
    let job = session.start(w.game.clone(), w.options(false), events.emit()).unwrap();
    let failed = events.last();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.steps[0].state, "failed");
    assert!(failed.steps[0].note.as_deref().is_some_and(|n| !n.is_empty()));
    assert_eq!(session.unfinished().unwrap().state, JobState::Failed);

    std::fs::remove_file(&w.out).unwrap();
    let again = Events::default();
    session.carry_on(&job.id, again.emit()).unwrap();
    assert_eq!(again.last().state, JobState::Finished);
}

#[test]
fn discard_removes_a_failed_job() {
    let w = World::new();
    std::fs::remove_dir(&w.out).unwrap();
    std::fs::write(&w.out, b"in the way").unwrap();
    let session = w.session();
    let events = Events::default();
    let job = session.start(w.game.clone(), w.options(false), events.emit()).unwrap();
    assert_eq!(events.last().state, JobState::Failed);
    session.discard(&job.id).unwrap();
    assert!(session.unfinished().is_none());
}

#[test]
fn packed_size_counts_files_in_a_folder_and_a_file() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("packed");
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    std::fs::write(dir.join("a.bin"), vec![0u8; 1000]).unwrap();
    std::fs::write(dir.join("sub/b.bin"), vec![0u8; 234]).unwrap();
    assert_eq!(packed_size(&dir), 1234);
    assert_eq!(packed_size(&dir.join("a.bin")), 1000);
    assert_eq!(packed_size(&tmp.path().join("missing")), 0);
}

#[test]
fn events_serialise_for_the_screen() {
    let w = World::new();
    let session = w.session();
    let events = Events::default();
    session.start(w.game.clone(), w.options(false), events.emit()).unwrap();
    let done = events.last();
    let json = serde_json::to_value(UiEvent::Job { job: done }).unwrap();
    assert_eq!(json["type"], "job");
    assert_eq!(json["job"]["state"], "finished");
    assert_eq!(json["job"]["steps"][0]["kind"], "pack");
    assert!(json["job"]["packedBytes"].as_u64().unwrap() > 0);
    assert!(json["job"]["outputDir"].is_string());
    let _ = w.tmp.path();
}
