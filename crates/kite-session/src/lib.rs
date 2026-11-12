//! The app's session: one job at a time, run on its own thread, reported as [`UiEvent`]s.

mod view;

pub use view::{JobState, JobView, StepView, UiEvent, packed_size};

use kite_pipeline::{
    Job, JobStore, Options, Packer, Progress, RunEvent, RunOutcome, Runner, ThreadSleeper, Trash,
    Unavailable,
};
use std::cell::Cell;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What packs and what deletes. Sending arrives with the Kite helper.
pub struct Ports {
    pub packer: Arc<dyn Packer>,
    pub trash: Arc<dyn Trash>,
}

pub type Emit = Arc<dyn Fn(UiEvent) + Send + Sync>;

const BUSY: &str = "A game is already being packed";
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

struct Running {
    id: String,
    cancel: Arc<AtomicBool>,
    /// Stopped because the app is closing: the job stays for the next launch.
    exiting: Arc<AtomicBool>,
}

pub struct Session {
    store: JobStore,
    ports: Ports,
    running: Arc<Mutex<Option<Running>>>,
    revs: Revs,
}

/// Hands out ever larger revision numbers for job views.
#[derive(Clone, Default)]
struct Revs(Arc<AtomicU64>);

impl Revs {
    fn stamp(&self, mut view: JobView) -> JobView {
        view.rev = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        view
    }
}

impl Session {
    pub fn new(jobs_dir: PathBuf, ports: Ports) -> io::Result<Session> {
        Ok(Session {
            store: JobStore::open(jobs_dir)?,
            ports,
            running: Arc::new(Mutex::new(None)),
            revs: Revs::default(),
        })
    }

    /// Saves a new job and starts it.
    pub fn start(&self, source: PathBuf, options: Options, emit: Emit) -> Result<JobView, String> {
        let job = Job::new(source, options).map_err(|e| e.to_string())?;
        self.spawn(job, emit)
    }

    /// Carries on a job that isn't running, from its first unfinished step.
    pub fn carry_on(&self, id: &str, emit: Emit) -> Result<JobView, String> {
        let Some(mut job) = self.store.unfinished().into_iter().find(|j| j.id == id) else {
            return Err("This job has already ended".into());
        };
        if let Some(next) = job.next_open() {
            job.retry_from(next);
        }
        self.spawn(job, emit)
    }

    /// Stops the running job; its step stops and the job is removed.
    pub fn cancel(&self) {
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Forgets a job that isn't running.
    pub fn discard(&self, id: &str) -> Result<(), String> {
        if self.running.lock().unwrap().as_ref().is_some_and(|r| r.id == id) {
            return Err(BUSY.into());
        }
        self.store.remove(id).map_err(|e| e.to_string())
    }

    /// The newest job left unfinished, as the app found it on launch.
    pub fn unfinished(&self) -> Option<JobView> {
        self.store.unfinished().first().map(|job| self.revs.stamp(JobView::at_rest(job)))
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    /// The app is closing: stops the running step cleanly (Forge removes its partial output)
    /// and keeps the job, so the next launch carries on. True once nothing is running; false
    /// if the step didn't stop within `wait`.
    pub fn stop_for_exit(&self, wait: Duration) -> bool {
        if let Some(running) = self.running.lock().unwrap().as_ref() {
            running.exiting.store(true, Ordering::SeqCst);
            running.cancel.store(true, Ordering::SeqCst);
        }
        let start = Instant::now();
        while self.is_running() {
            if start.elapsed() >= wait {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    fn spawn(&self, job: Job, emit: Emit) -> Result<JobView, String> {
        let cancel = Arc::new(AtomicBool::new(false));
        let exiting = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap();
            if running.is_some() {
                return Err(BUSY.into());
            }
            self.store.save(&job).map_err(|e| format!("Couldn't save the job: {e}"))?;
            *running = Some(Running { id: job.id.clone(), cancel: cancel.clone(), exiting: exiting.clone() });
        }
        // Stamped before the thread starts, so every event's view is newer than this reply.
        let view = self.revs.stamp(JobView::of(&job, JobState::Running));
        let (store, packer, trash, running, revs) = (
            self.store.clone(),
            self.ports.packer.clone(),
            self.ports.trash.clone(),
            self.running.clone(),
            self.revs.clone(),
        );
        std::thread::spawn(move || {
            let end = run(job, &store, &*packer, &*trash, &cancel, &exiting, &emit, &revs);
            *running.lock().unwrap() = None;
            emit(UiEvent::Job { job: revs.stamp(end) });
        });
        Ok(view)
    }
}

/// Runs the job to its end and returns the final view.
fn run(
    mut job: Job,
    store: &JobStore,
    packer: &dyn Packer,
    trash: &dyn Trash,
    cancel: &AtomicBool,
    exiting: &AtomicBool,
    emit: &Emit,
    revs: &Revs,
) -> JobView {
    let id = job.id.clone();
    let last_progress: Cell<Option<Instant>> = Cell::new(None);
    let runner = Runner {
        packer,
        trash,
        sender: &Unavailable,
        installer: &Unavailable,
        sleeper: &ThreadSleeper,
        store,
        retry_every: Duration::from_secs(5),
    };
    let events = |event: RunEvent| match event {
        RunEvent::Progress { index, progress: Progress::Stage(stage) } => {
            emit(UiEvent::Stage { id: id.clone(), index, stage });
        }
        RunEvent::Progress { index, progress: Progress::Bytes { done, total } } => {
            let due = last_progress.get().is_none_or(|t| t.elapsed() >= PROGRESS_EVERY);
            if due || done >= total {
                last_progress.set(Some(Instant::now()));
                emit(UiEvent::Progress { id: id.clone(), index, done, total });
            }
        }
        // Every other event follows a save, so the saved job is the one to show.
        RunEvent::StepStarted { .. }
        | RunEvent::StepWaiting { .. }
        | RunEvent::StepDone { .. }
        | RunEvent::StepFailed { .. } => {
            if let Ok(saved) = store.load(&id) {
                emit(UiEvent::Job { job: revs.stamp(JobView::of(&saved, JobState::Running)) });
            }
        }
        RunEvent::Cancelled { .. } | RunEvent::Finished => {}
    };
    match runner.run(&mut job, cancel, &events) {
        RunOutcome::Finished => {
            let mut view = JobView::of(&job, JobState::Finished);
            view.packed_bytes = job.packed.as_deref().map(packed_size);
            view
        }
        RunOutcome::Failed { .. } => JobView::of(&job, JobState::Failed),
        // Stopped for the app closing: the step is back to pending and saved; it carries on
        // next time.
        RunOutcome::Cancelled { .. } if exiting.load(Ordering::SeqCst) => JobView::at_rest(&job),
        RunOutcome::Cancelled { .. } => {
            // A cancelled job doesn't come back on the next launch.
            let _ = store.remove(&job.id);
            JobView::of(&job, JobState::Cancelled)
        }
    }
}
