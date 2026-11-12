mod fakes;

use fakes::*;
use kite_pipeline::*;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn unreachable() -> Result<(), SendError> {
    Err(SendError::Unreachable("no route to host".into()))
}

#[test]
fn all_six_toggle_combinations() {
    let combos = [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (false, true, true),
        (true, true, false),
        (true, true, true),
    ];
    for (delete_original, send, delete_packed) in combos {
        for dlc in if send { vec![0, 1] } else { vec![0] } {
            let w = world();
            let mut job = w.job(delete_original, send, delete_packed, dlc);
            let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
            let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
            let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));

            let mut expected = vec!["pack".to_string()];
            if delete_original {
                expected.push(format!("trash {}", w.source.display()));
            }
            if send {
                expected.push("send".into());
                if dlc == 1 {
                    expected.push("installed? 0.pkg".into());
                    expected.push("install 0.pkg".into());
                }
                if delete_packed {
                    expected.push(format!("trash {}", w.packed_path().display()));
                }
            }
            let case = format!("{delete_original}/{send}/{delete_packed}/dlc {dlc}");
            assert_eq!(outcome, RunOutcome::Finished, "{case}");
            assert!(job.is_finished(), "{case}");
            assert_eq!(calls(&w.log), expected, "{case}");
        }
    }
}

#[test]
fn unreachable_ps5_waits_then_sends() {
    let w = world();
    let mut job = w.job(false, true, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![unreachable(), unreachable(), Ok(())]);
    let installer = w.installer(&[], vec![]);
    let sleep = NoSleep::new();
    let (outcome, events) = run(&w, &mut job, &packer, &trash, &sender, &installer, &sleep, &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(*sleep.slept.lock().unwrap(), vec![Duration::from_secs(5); 2]);
    let waits = events.iter().filter(|e| matches!(e, RunEvent::StepWaiting { .. })).count();
    assert_eq!(waits, 1);
    assert_eq!(job.steps[1].state, StepState::Done { note: "Sent and checked".into() });
}

/// Reads the saved job each time the runner sleeps, to see what the app would show.
struct SpySleeper {
    store: JobStore,
    id: String,
    seen: Mutex<Vec<StepState>>,
}

impl Sleeper for SpySleeper {
    fn sleep(&self, _: Duration) {
        let job = self.store.load(&self.id).unwrap();
        self.seen.lock().unwrap().push(job.steps[1].state.clone());
    }
}

#[test]
fn waiting_since_is_kept_across_retries() {
    let w = world();
    let mut job = w.job(false, true, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![unreachable(), unreachable(), unreachable(), Ok(())]);
    let installer = w.installer(&[], vec![]);
    let spy = SpySleeper { store: w.store.clone(), id: job.id.clone(), seen: Mutex::new(vec![]) };
    run(&w, &mut job, &packer, &trash, &sender, &installer, &spy, &AtomicBool::new(false));
    let seen = spy.seen.into_inner().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(matches!(seen[0], StepState::Waiting { .. }));
    assert!(seen.iter().all(|s| *s == seen[0]), "{seen:?}");
}

#[test]
fn cancel_while_waiting() {
    let w = world();
    let mut job = w.job(false, true, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender((0..10).map(|_| unreachable()).collect());
    let installer = w.installer(&[], vec![]);
    let flag = Arc::new(AtomicBool::new(false));
    let sleep = NoSleep::cancelling_on(3, flag.clone());
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &sleep, &flag);
    assert_eq!(outcome, RunOutcome::Cancelled { index: 1 });
    assert_eq!(job.steps[1].state, StepState::Pending);
    assert_eq!(calls(&w.log).iter().filter(|c| *c == "send").count(), 3);
}

#[test]
fn send_failure_keeps_packed_copy() {
    let w = world();
    let mut job = w.job(false, true, true, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![Err(SendError::Failed("checksum".into()))]);
    let installer = w.installer(&[], vec![]);
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Failed { index: 1 });
    assert_eq!(calls(&w.log), vec!["pack", "send"]);
    assert!(w.packed_path().exists());
    assert_eq!(job.steps[2].state, StepState::Pending);
}

#[test]
fn refused_dlc_does_not_stop_the_next_one() {
    let w = world();
    let mut job = w.job(false, true, true, 2);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![]);
    let installer = w.installer(
        &[],
        vec![Err(InstallError::Refused { code: 0x80B2_2404, hint: "Sony refused it".into() }), Ok(())],
    );
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Failed { index: 2 });
    assert_eq!(job.steps[2].state, StepState::Failed { reason: "Sony refused it (0x80B22404)".into() });
    assert_eq!(job.steps[3].state, StepState::Done { note: "Installed and confirmed".into() });
    assert_eq!(job.steps[4].state, StepState::Pending);
    assert!(calls(&w.log).contains(&"install 1.pkg".to_string()));
    assert!(w.packed_path().exists());
}

#[test]
fn unconfirmed_dlc_keeps_packed_copy() {
    let w = world();
    let mut job = w.job(false, true, true, 1);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![]);
    let installer = w.installer(&[], vec![Err(InstallError::NotConfirmed)]);
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Failed { index: 2 });
    assert_eq!(
        job.steps[2].state,
        StepState::Failed { reason: "Handed to the PS5 but never showed up".into() }
    );
    assert!(!calls(&w.log).iter().any(|c| c.starts_with("trash")));
}

#[test]
fn already_installed_dlc_is_skipped() {
    let w = world();
    let mut job = w.job(false, true, false, 1);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![]);
    let installer = w.installer(&["0.pkg"], vec![]);
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(job.steps[2].state, StepState::Done { note: "Already on the PS5".into() });
    assert_eq!(calls(&w.log), vec!["pack", "send", "installed? 0.pkg"]);
}

#[test]
fn dlc_waits_while_unreachable() {
    let w = world();
    let mut job = w.job(false, true, false, 1);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let sender = w.sender(vec![]);
    let installer = w.installer(&[], vec![Err(InstallError::Unreachable("asleep".into())), Ok(())]);
    let sleep = NoSleep::new();
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &sleep, &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(sleep.slept.lock().unwrap().len(), 1);
}

#[test]
fn resume_after_crash_mid_send() {
    let w = world();
    let mut job = w.job(false, true, false, 0);
    std::fs::create_dir_all(w.packed_path()).unwrap();
    job.packed = Some(w.packed_path());
    job.steps[0].state = StepState::Done { note: "packed".into() };
    job.steps[1].state = StepState::Running;
    w.store.save(&job).unwrap();

    let mut resumed = w.store.unfinished().remove(0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut resumed, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(calls(&w.log), vec!["send"]);
}

#[test]
fn resume_after_crash_mid_pack() {
    let w = world();
    let mut job = w.job(false, true, false, 0);
    job.steps[0].state = StepState::Running;
    w.store.save(&job).unwrap();

    let mut resumed = w.store.unfinished().remove(0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut resumed, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(calls(&w.log), vec!["pack", "send"]);
}
