mod fakes;

use fakes::*;
use kite_pipeline::*;
use std::fs;
use std::sync::atomic::AtomicBool;

fn state(job: &Job, i: usize) -> StepState {
    job.steps[i].state.clone()
}

#[test]
fn pack_only_finishes() {
    let w = world();
    let mut job = w.job(false, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(job.packed, Some(w.packed_path()));
    assert_eq!(calls(&w.log), vec!["pack"]);
}

#[test]
fn pack_then_delete_original() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(calls(&w.log), vec!["pack".to_string(), format!("trash {}", w.source.display())]);
    assert!(!w.source.exists());
    assert_eq!(state(&job, 1), StepState::Done { note: "Moved to the Trash".into() });
}

#[test]
fn pack_failure_deletes_nothing() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Fail("disk full".into())), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Failed { index: 0 });
    assert_eq!(state(&job, 0), StepState::Failed { reason: "disk full".into() });
    assert_eq!(state(&job, 1), StepState::Pending);
    assert_eq!(calls(&w.log), vec!["pack"]);
    assert!(w.source.exists());
}

#[test]
fn changed_source_is_kept() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let mut packer = w.packer(PackResultKind::Ok);
    let source = w.source.clone();
    packer.before = Some(Box::new(move || fs::write(source.join("new.bin"), b"x").unwrap()));
    let trash = w.trash(None);
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Failed { index: 1 });
    assert_eq!(
        state(&job, 1),
        StepState::Failed { reason: "The original changed since packing; it was kept".into() }
    );
    assert_eq!(calls(&w.log), vec!["pack"]);
    assert!(w.source.exists());
}

#[test]
fn trash_refusal_keeps_the_original() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(Some("no Trash on this drive")));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Failed { index: 1 });
    assert_eq!(
        state(&job, 1),
        StepState::Failed { reason: "Couldn't move the original to the Trash: no Trash on this drive".into() }
    );
    assert!(w.source.exists());
}

#[test]
fn cancel_before_start() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, _) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(true));
    assert_eq!(outcome, RunOutcome::Cancelled { index: 0 });
    assert_eq!(state(&job, 0), StepState::Pending);
    assert!(calls(&w.log).is_empty());
}

#[test]
fn pack_cancelled_returns_step_to_pending() {
    let w = world();
    let mut job = w.job(false, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Cancel), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (outcome, events) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(outcome, RunOutcome::Cancelled { index: 0 });
    assert_eq!(state(&job, 0), StepState::Pending);
    assert_eq!(events.last(), Some(&RunEvent::Cancelled { index: 0 }));
}

#[test]
fn every_state_change_is_saved() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(w.store.load(&job.id).unwrap(), job);
}

#[test]
fn failed_step_blocks_until_retry() {
    let w = world();
    let mut job = w.job(false, false, false, 0);
    let (bad, trash) = (w.packer(PackResultKind::Fail("disk full".into())), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    run(&w, &mut job, &bad, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    let good = w.packer(PackResultKind::Ok);
    let (again, _) = run(&w, &mut job, &good, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(again, RunOutcome::Failed { index: 0 });
    assert_eq!(calls(&w.log), vec!["pack"]);
    job.retry_from(0);
    let (after, _) = run(&w, &mut job, &good, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(after, RunOutcome::Finished);
}

#[test]
fn events_follow_the_steps() {
    let w = world();
    let mut job = w.job(true, false, false, 0);
    let (packer, trash) = (w.packer(PackResultKind::Ok), w.trash(None));
    let (sender, installer) = (w.sender(vec![]), w.installer(&[], vec![]));
    let (_, events) = run(&w, &mut job, &packer, &trash, &sender, &installer, &NoSleep::new(), &AtomicBool::new(false));
    assert_eq!(
        events,
        vec![
            RunEvent::StepStarted { index: 0 },
            RunEvent::StepDone { index: 0, note: "3 files, 100 bytes, checked".into() },
            RunEvent::StepStarted { index: 1 },
            RunEvent::StepDone { index: 1, note: "Moved to the Trash".into() },
            RunEvent::Finished,
        ]
    );
}
