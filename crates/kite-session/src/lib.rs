//! The app's session: one job at a time, run on its own thread, reported as [`UiEvent`]s.

mod view;

pub use view::{JobState, JobView, StepView, UiEvent, packed_size, packed_size_with};

use kite_pipeline::{
    Installer, Job, JobStore, Options, Packer, Progress, RunEvent, RunOutcome, Runner, Sender, StepKind,
    StepState, ThreadSleeper, Trash,
};
use std::cell::Cell;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What packs, what deletes, what sends to the PS5, and what installs DLC there.
pub struct Ports {
    pub packer: Arc<dyn Packer>,
    pub trash: Arc<dyn Trash>,
    pub sender: Arc<dyn Sender>,
    /// Installs DLC packages on the console; ShadowMount+'s API, in the app.
    pub installer: Arc<dyn Installer>,
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

    /// Whether the running job is installing a package: closing the app would stop the PS5's
    /// download. A convert is an install only once its package is written.
    pub fn running_install(&self) -> bool {
        let Some(id) = self.running.lock().unwrap().as_ref().map(|r| r.id.clone()) else {
            return false;
        };
        // The step it is on (or about to start): the first one not done.
        self.store
            .load(&id)
            .is_ok_and(|job| job.next_open().is_some_and(|i| job.steps[i].kind == StepKind::InstallPackage))
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
            *running = Some(Running {
                id: job.id.clone(),
                cancel: cancel.clone(),
                exiting: exiting.clone(),
            });
        }
        // Stamped before the thread starts, so every event's view is newer than this reply.
        let view = self.revs.stamp(JobView::of(&job, JobState::Running));
        let (store, running, revs) = (self.store.clone(), self.running.clone(), self.revs.clone());
        let ports = Ports {
            packer: self.ports.packer.clone(),
            trash: self.ports.trash.clone(),
            sender: self.ports.sender.clone(),
            installer: self.ports.installer.clone(),
        };
        std::thread::spawn(move || {
            let end = run(job, &store, &ports, &cancel, &exiting, &emit, &revs);
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
    ports: &Ports,
    cancel: &AtomicBool,
    exiting: &AtomicBool,
    emit: &Emit,
    revs: &Revs,
) -> JobView {
    let id = job.id.clone();
    let last_progress: Cell<Option<Instant>> = Cell::new(None);
    // The packed copy's size, measured once Pack is done: Delete the packed folder may move it
    // away before the job ends. A resumed job whose Pack is already done is measured now.
    let pack_done = |j: &Job| j.steps.iter().any(|st| st.kind == StepKind::Pack && matches!(st.state, StepState::Done { .. }));
    let measured: Cell<Option<u64>> =
        Cell::new(job.packed.as_deref().filter(|p| pack_done(&job) && p.exists()).map(|p| packed_size_with(p, &job.ahead)));
    let runner = Runner {
        packer: &*ports.packer,
        trash: &*ports.trash,
        sender: &*ports.sender,
        installer: &*ports.installer,
        sleeper: &ThreadSleeper,
        store,
        retry_every: Duration::from_secs(5),
    };
    let events = |event: RunEvent| match event {
        RunEvent::Progress { index, progress: Progress::Stage(stage) } => {
            emit(UiEvent::Stage { id: id.clone(), index, stage });
        }
        // The runner keeps finished parts to itself (it sends them ahead); none reach here.
        RunEvent::Progress { progress: Progress::PartReady { .. } | Progress::PartsClosing, .. } => {}
        RunEvent::Progress { index, progress: Progress::PartsAhead { sent, sending, on_ps5, estimate } } => {
            emit(UiEvent::Ahead { id: id.clone(), index, sent, sending, on_ps5, estimate });
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
                if measured.get().is_none() && pack_done(&saved) {
                    measured.set(saved.packed.as_deref().map(|p| packed_size_with(p, &saved.ahead)));
                }
                let mut view = JobView::of(&saved, JobState::Running);
                view.packed_bytes = measured.get();
                emit(UiEvent::Job { job: revs.stamp(view) });
            }
        }
        RunEvent::Cancelled { .. } | RunEvent::Finished => {}
    };
    match runner.run(&mut job, cancel, &events) {
        RunOutcome::Finished => {
            let mut view = JobView::of(&job, JobState::Finished);
            view.packed_bytes = measured.get().or_else(|| job.packed.as_deref().map(|p| packed_size_with(p, &job.ahead)));
            view
        }
        RunOutcome::Failed { .. } => JobView::of(&job, JobState::Failed),
        // Stopped for the app closing: the step is back to pending and saved; it carries on
        // next time.
        RunOutcome::Cancelled { .. } if exiting.load(Ordering::SeqCst) => JobView::at_rest(&job),
        RunOutcome::Cancelled { .. } => {
            // A cancelled job doesn't come back on the next launch.
            let _ = store.remove(&job.id);
            // Nor does a stopped install: the next one of the same package asks at once.
            // The package it installed: a convert's own, or the source as it is.
            let converted = job.steps.iter().any(|s| s.kind == StepKind::Pack);
            let pkg = if converted { job.packed.as_ref() } else { Some(&job.source) };
            if job.options.install
                && let (Some(target), Some(pkg)) = (&job.options.send, pkg)
            {
                ports.installer.forget_package(pkg, target);
            }
            JobView::of(&job, JobState::Cancelled)
        }
    }
}
