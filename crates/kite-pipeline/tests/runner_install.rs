//! A job that installs a package as it is, streamed by the PS5's installer.
mod fakes;

use fakes::*;
use kite_pipeline::*;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

/// Answers each install in turn from `script`, telling `bytes` as progress first.
struct Streamer {
    log: CallLog,
    bytes: Vec<(u64, u64)>,
    script: Mutex<VecDeque<Result<InstallOutcome, InstallError>>>,
}

impl Installer for Streamer {
    fn is_installed(&self, _: &Path, _: &SendTarget) -> Result<bool, InstallError> {
        unreachable!("not a DLC")
    }
    fn install(&self, _: &Path, _: &SendTarget, _: &AtomicBool) -> Result<(), InstallError> {
        unreachable!("not a DLC")
    }
    fn install_package(&self, pkg: &Path, target: &SendTarget, progress: &dyn Fn(Progress), _: &AtomicBool) -> Result<InstallOutcome, InstallError> {
        self.log.lock().unwrap().push(format!("install {} on {}", pkg.file_name().unwrap().to_string_lossy(), target.console));
        for &(done, total) in &self.bytes {
            progress(Progress::Bytes { done, total });
        }
        self.script.lock().unwrap().pop_front().unwrap_or(Ok(InstallOutcome::Installed))
    }
}

fn install_job(w: &World) -> Job {
    let mut options = w.options(false, true, false, 0);
    options.install = true;
    Job::new(w.tmp.path().join("Ashen Tide.pkg"), options).unwrap()
}

fn go(w: &World, job: &mut Job, installer: &Streamer) -> (RunOutcome, Vec<RunEvent>, Vec<(usize, u64, u64)>) {
    let (packer, trash, sender) = (w.packer(PackResultKind::Ok), w.trash(None), w.sender(vec![]));
    let progress = Mutex::new(Vec::new());
    let events = Mutex::new(Vec::new());
    let runner = Runner {
        packer: &packer,
        trash: &trash,
        sender: &sender,
        installer,
        sleeper: &NoSleep::new(),
        store: &w.store,
        retry_every: std::time::Duration::from_secs(5),
    };
    let outcome = runner.run(job, &AtomicBool::new(false), &|e| match e {
        RunEvent::Progress { index, progress: Progress::Bytes { done, total } } => progress.lock().unwrap().push((index, done, total)),
        RunEvent::Progress { .. } => {}
        other => events.lock().unwrap().push(other),
    });
    (outcome, events.into_inner().unwrap(), progress.into_inner().unwrap())
}

fn streamer(w: &World, script: Vec<Result<InstallOutcome, InstallError>>) -> Streamer {
    Streamer { log: w.log.clone(), bytes: vec![(1, 2), (2, 2)], script: Mutex::new(script.into()) }
}

#[test]
fn installs_and_reports_progress() {
    let w = world();
    let mut job = install_job(&w);
    let installer = streamer(&w, vec![]);
    let (outcome, _, progress) = go(&w, &mut job, &installer);
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(calls(&w.log), ["install Ashen Tide.pkg on ps5"], "nothing packed, sent or trashed");
    assert_eq!(progress, vec![(0, 1, 2), (0, 2, 2)]);
    assert_eq!(job.steps[0].state, StepState::Done { note: "Installed and confirmed".into() });
}

#[test]
fn waits_for_a_ps5_that_is_away() {
    let w = world();
    let mut job = install_job(&w);
    let installer = streamer(&w, vec![Err(InstallError::Unreachable("away".into())), Ok(InstallOutcome::Installed)]);
    let (outcome, events, _) = go(&w, &mut job, &installer);
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(calls(&w.log).len(), 2);
    assert!(events.iter().any(|e| matches!(e, RunEvent::StepWaiting { index: 0, .. })), "{events:?}");
}

#[test]
fn says_when_it_was_already_there() {
    let w = world();
    let mut job = install_job(&w);
    let (outcome, _, _) = go(&w, &mut job, &streamer(&w, vec![Ok(InstallOutcome::AlreadyThere)]));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(job.steps[0].state, StepState::Done { note: "Already on the PS5".into() });
}

#[test]
fn a_refusal_fails_the_step_with_its_reason() {
    let w = world();
    let mut job = install_job(&w);
    let refused = InstallError::Refused { code: 0x80B2_116F, hint: "Sony's installer refused it".into() };
    let (outcome, _, _) = go(&w, &mut job, &streamer(&w, vec![Err(refused)]));
    assert_eq!(outcome, RunOutcome::Failed { index: 0 });
    assert!(matches!(&job.steps[0].state, StepState::Failed { reason } if reason.contains("0x80B2116F")), "{:?}", job.steps[0].state);
}
