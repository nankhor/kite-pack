//! Parts of a packed folder go to the PS5 while the rest is still being packed.
mod fakes;

use fakes::*;
use kite_pipeline::*;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn part(i: u32) -> Part {
    Part { rel: format!("ampr_zstd-{i:03}.pak"), size: 1000 + u64::from(i), settled_from: 64 }
}

/// Announces `parts` as it "packs", each from a file of its own, waiting up to a second for the
/// sender to take each before the next; then ends with `end`.
struct PartsPacker {
    log: CallLog,
    output: PathBuf,
    links: PathBuf,
    parts: u32,
    end: Result<(), PackError>,
    /// The request's `ahead`, as the runner set it.
    asked: Mutex<Option<bool>>,
    taken: Arc<AtomicUsize>,
}

impl Packer for PartsPacker {
    fn pack(&self, req: &PackRequest, progress: &dyn Fn(Progress), _cancel: &AtomicBool) -> Result<PackOutcome, PackError> {
        self.log.lock().unwrap().push("pack".into());
        *self.asked.lock().unwrap() = Some(req.ahead);
        std::fs::create_dir_all(&self.links).unwrap();
        progress(Progress::Stage("Packing".into()));
        for i in 0..self.parts {
            let p = part(i);
            let file = self.links.join(&p.rel);
            std::fs::create_dir_all(&self.links).unwrap();
            std::fs::write(&file, vec![0u8; p.size as usize]).unwrap();
            progress(Progress::PartReady { packed: self.output.clone(), file, part: p });
            let since = Instant::now();
            while self.taken.load(Ordering::SeqCst) <= i as usize && since.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(5));
                progress(Progress::Bytes { done: u64::from(i), total: u64::from(self.parts) });
            }
        }
        self.end.clone()?;
        std::fs::create_dir_all(&self.output).unwrap();
        Ok(PackOutcome { output: self.output.clone(), bytes: 100, files: 3, checks: vec![] })
    }
}

/// Logs what it was asked. `ahead` answers each send_ahead in turn (then Ok(true)); `hold`
/// keeps one send_ahead going that long, or until it is told to stop.
struct AheadSender {
    log: CallLog,
    ahead: Mutex<Vec<Result<bool, SendError>>>,
    hold: Option<Duration>,
    taken: Arc<AtomicUsize>,
}

impl Sender for AheadSender {
    fn send(&self, _: &Path, _: &SendTarget, _: &dyn Fn(Progress), _: &AtomicBool) -> Result<(), SendError> {
        self.log.lock().unwrap().push("send".into());
        Ok(())
    }

    fn send_ahead(&self, packed: &Path, file: &Path, part: &Part, _: &SendTarget, cancel: &AtomicBool) -> Result<bool, SendError> {
        assert!(file.is_file(), "a part is read from a file that is there");
        assert_eq!(packed.file_name().unwrap(), "PPSA01234");
        self.log.lock().unwrap().push(format!("ahead {}", part.rel));
        self.taken.fetch_add(1, Ordering::SeqCst);
        if let Some(hold) = self.hold {
            let since = Instant::now();
            while since.elapsed() < hold {
                if cancel.load(Ordering::SeqCst) {
                    return Err(SendError::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let mut script = self.ahead.lock().unwrap();
        if script.is_empty() { Ok(true) } else { script.remove(0) }
    }

    fn send_rest(&self, _: &Path, _: &SendTarget, ahead: &[Part], _: &dyn Fn(Progress), _: &AtomicBool) -> Result<(), SendError> {
        let names: Vec<_> = ahead.iter().map(|p| p.rel.as_str()).collect();
        self.log.lock().unwrap().push(format!("send rest without {}", names.join(" ")));
        Ok(())
    }
}

struct Setup {
    w: World,
    packer: PartsPacker,
    sender: AheadSender,
}

fn setup(parts: u32, end: Result<(), PackError>, ahead: Vec<Result<bool, SendError>>, hold: Option<Duration>) -> Setup {
    let w = world();
    let taken = Arc::new(AtomicUsize::new(0));
    let packer = PartsPacker {
        log: w.log.clone(),
        output: w.packed_path(),
        links: w.out.join(".PPSA01234.kite-ahead"),
        parts,
        end,
        asked: Mutex::new(None),
        taken: taken.clone(),
    };
    let sender = AheadSender { log: w.log.clone(), ahead: Mutex::new(ahead), hold, taken };
    Setup { w, packer, sender }
}

fn go(s: &Setup, job: &mut Job, cancel: &AtomicBool) -> (RunOutcome, Vec<RunEvent>) {
    let (trash, installer) = (s.w.trash(None), s.w.installer(&[], vec![]));
    run(&s.w, job, &s.packer, &trash, &s.sender, &installer, &NoSleep::new(), cancel)
}

fn pack_note(job: &Job) -> String {
    match &job.steps[0].state {
        StepState::Done { note } => note.clone(),
        other => panic!("pack not done: {other:?}"),
    }
}

#[test]
fn parts_go_ahead_while_packing_and_the_rest_after() {
    let s = setup(2, Ok(()), vec![], None);
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(*s.packer.asked.lock().unwrap(), Some(true));
    assert_eq!(
        calls(&s.w.log),
        ["pack", "ahead ampr_zstd-000.pak", "ahead ampr_zstd-001.pak", "send rest without ampr_zstd-000.pak ampr_zstd-001.pak"]
    );
    assert_eq!(job.ahead, vec![part(0), part(1)]);
    assert!(pack_note(&job).ends_with("· 2 parts sent while packing"), "{}", pack_note(&job));
    // Saved with the job, so a send after a restart still leaves them out.
    assert_eq!(s.w.store.load(&job.id).unwrap().ahead, vec![part(0), part(1)]);
}

#[test]
fn the_screen_hears_how_many_parts_the_ps5_has() {
    let s = setup(2, Ok(()), vec![], None);
    let mut job = s.w.job(false, true, false, 0);
    let seen = Mutex::new(Vec::new());
    let (trash, installer) = (s.w.trash(None), s.w.installer(&[], vec![]));
    let runner = Runner {
        packer: &s.packer,
        trash: &trash,
        sender: &s.sender,
        installer: &installer,
        sleeper: &NoSleep::new(),
        store: &s.w.store,
        retry_every: Duration::from_secs(5),
    };
    runner.run(&mut job, &AtomicBool::new(false), &|e| match e {
        RunEvent::Progress { index: 0, progress: Progress::PartsAhead { sent, sending } } => seen.lock().unwrap().push((sent, sending)),
        RunEvent::Progress { index: 0, progress: Progress::Stage(stage) } => assert_eq!(stage, "Packing", "the packer's stage goes through as it is"),
        _ => {}
    });
    let seen = seen.into_inner().unwrap();
    assert!(seen.windows(2).all(|w| w[0] != w[1]), "only changes are told: {seen:?}");
    assert!(seen.iter().any(|&(sent, _)| sent == 1), "{seen:?}");
    assert_eq!(seen.last(), Some(&(2, false)), "{seen:?}");
}

#[test]
fn an_older_helper_takes_no_parts_ahead() {
    let s = setup(3, Ok(()), vec![Ok(false)], None);
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    // Tried once; the other parts go with the rest, in one ordinary send.
    assert_eq!(calls(&s.w.log), ["pack", "ahead ampr_zstd-000.pak", "send"]);
    assert!(job.ahead.is_empty());
    assert!(!pack_note(&job).contains("while packing"));
}

#[test]
fn a_part_that_fails_leaves_the_rest_for_the_send() {
    let s = setup(3, Ok(()), vec![Ok(true), Err(SendError::Unreachable("away".into()))], None);
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(
        calls(&s.w.log),
        ["pack", "ahead ampr_zstd-000.pak", "ahead ampr_zstd-001.pak", "send rest without ampr_zstd-000.pak"]
    );
    assert_eq!(job.ahead, vec![part(0)]);
}

#[test]
fn nothing_goes_ahead_when_not_sending() {
    let s = setup(2, Ok(()), vec![], None);
    let mut job = s.w.job(false, false, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(*s.packer.asked.lock().unwrap(), Some(false));
    assert_eq!(calls(&s.w.log), ["pack"]);
}

#[test]
fn an_image_sends_nothing_ahead() {
    let s = setup(0, Ok(()), vec![], None);
    let mut options = s.w.options(false, true, false, 0);
    options.format = OutputFormat::Exfat;
    let mut job = Job::new(s.w.source.clone(), options).unwrap();
    go(&s, &mut job, &AtomicBool::new(false));
    assert_eq!(*s.packer.asked.lock().unwrap(), Some(false));
    assert_eq!(calls(&s.w.log), ["pack", "send"]);
}

#[test]
fn the_part_on_its_way_finishes_after_the_pack() {
    // The pack ends while its one part is still going: the step waits for it.
    let s = setup(1, Ok(()), vec![], Some(Duration::from_millis(300)));
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(job.ahead, vec![part(0)]);
    assert_eq!(calls(&s.w.log), ["pack", "ahead ampr_zstd-000.pak", "send rest without ampr_zstd-000.pak"]);
}

#[test]
fn a_failed_pack_stops_the_part_on_its_way() {
    let s = setup(1, Err(PackError::Failed("disk full".into())), vec![], Some(Duration::from_secs(30)));
    let mut job = s.w.job(false, true, false, 0);
    let since = Instant::now();
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Failed { index: 0 });
    assert!(since.elapsed() < Duration::from_secs(10), "the part was stopped, not waited for");
    assert!(job.ahead.is_empty());
}

#[test]
fn a_cancel_while_the_last_part_finishes_stops_it() {
    let s = setup(1, Ok(()), vec![], Some(Duration::from_secs(30)));
    let mut job = s.w.job(false, true, false, 0);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let raiser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    let since = Instant::now();
    let (outcome, _) = go(&s, &mut job, &cancel);
    raiser.join().unwrap();

    assert!(since.elapsed() < Duration::from_secs(10), "the cancel stopped the part");
    assert!(job.ahead.is_empty());
    // The pack itself had finished: the cancel lands on the next step.
    assert!(matches!(outcome, RunOutcome::Cancelled { .. }), "{outcome:?}");
}

#[test]
fn a_new_pack_starts_without_old_parts() {
    let s = setup(0, Ok(()), vec![], None);
    let mut job = s.w.job(false, true, false, 0);
    job.ahead = vec![part(7)];
    go(&s, &mut job, &AtomicBool::new(false));
    assert!(job.ahead.is_empty());
    assert_eq!(calls(&s.w.log), ["pack", "send"]);
}
