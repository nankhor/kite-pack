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
    /// Parts to be copied out first, as on Windows.
    copy_first: bool,
    /// Says Forge left writing right after announcing this part.
    close_after: Option<u32>,
    /// Pretends this much is left to pack, before any part.
    left: Option<u64>,
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
        if let Some(left) = self.left {
            progress(Progress::Bytes { done: 0, total: left });
        }
        for i in 0..self.parts {
            let p = part(i);
            let file = self.links.join(&p.rel);
            std::fs::create_dir_all(&self.links).unwrap();
            std::fs::write(&file, vec![i as u8 + 1; p.size as usize]).unwrap();
            // Part i is out once i + 1 of the pack's parts' worth is read.
            progress(Progress::Bytes { done: u64::from(i + 1), total: self.left.unwrap_or(u64::from(self.parts)) });
            progress(Progress::PartReady { packed: self.output.clone(), file, part: p, copy_first: self.copy_first });
            if self.close_after == Some(i) {
                progress(Progress::PartsClosing);
            }
            let since = Instant::now();
            while self.taken.load(Ordering::SeqCst) <= i as usize && since.elapsed() < Duration::from_secs(1) {
                std::thread::sleep(Duration::from_millis(5));
                progress(Progress::Bytes { done: u64::from(i + 1), total: self.left.unwrap_or(u64::from(self.parts)) });
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
    /// Where each part was read from.
    files: Mutex<Vec<PathBuf>>,
    ahead: Mutex<Vec<Result<bool, SendError>>>,
    hold: Option<Duration>,
    taken: Arc<AtomicUsize>,
}

impl Sender for AheadSender {
    fn send(&self, _: &Path, _: &SendTarget, _: &dyn Fn(Progress), _: &AtomicBool) -> Result<(), SendError> {
        self.log.lock().unwrap().push("send".into());
        Ok(())
    }

    fn send_ahead(&self, packed: &Path, file: &Path, part: &Part, _: &SendTarget, progress: &(dyn Fn(u64) + Sync), cancel: &AtomicBool) -> Result<bool, SendError> {
        progress(part.size / 2);
        assert!(file.is_file(), "a part is read from a file that is there");
        assert_eq!(packed.file_name().unwrap(), "PPSA01234");
        let i: u8 = part.rel[10..13].parse().unwrap();
        assert_eq!(std::fs::read(file).unwrap(), vec![i + 1; part.size as usize], "the part's own bytes");
        self.files.lock().unwrap().push(file.to_path_buf());
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
        copy_first: false,
        close_after: None,
        left: None,
        asked: Mutex::new(None),
        taken: taken.clone(),
    };
    let sender = AheadSender { log: w.log.clone(), files: Mutex::new(vec![]), ahead: Mutex::new(ahead), hold, taken };
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
    let bytes = Mutex::new(Vec::new());
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
        RunEvent::Progress { index: 0, progress: Progress::PartsAhead { sent, sending, on_ps5, estimate } } => {
            seen.lock().unwrap().push((sent, sending));
            bytes.lock().unwrap().push((on_ps5, estimate));
        }
        RunEvent::Progress { index: 0, progress: Progress::Stage(stage) } => assert_eq!(stage, "Packing", "the packer's stage goes through as it is"),
        _ => {}
    });
    let seen = seen.into_inner().unwrap();
    let bytes = bytes.into_inner().unwrap();
    // The estimate is the parts so far over how far the pack is: 1000 at half way (2000), then
    // 2001 at the end, what the two parts come to. The last word has both parts whole.
    assert!(bytes.iter().any(|&(_, e)| e == 2000), "{bytes:?}");
    assert_eq!(bytes.last(), Some(&(2001, 2001)), "{bytes:?}");
    assert!(bytes.windows(2).all(|w| w[0].0 <= w[1].0), "bytes on the PS5 only grow: {bytes:?}");
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

// ---- parts copied out first (Windows) --------------------------------------------------------

fn copies_dir(s: &Setup) -> PathBuf {
    s.w.out.join(".PPSA01234.kite-copy")
}

#[test]
fn parts_to_copy_go_as_copies_and_the_copies_are_removed() {
    let mut s = setup(2, Ok(()), vec![], None);
    s.packer.copy_first = true;
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(job.ahead, vec![part(0), part(1)]);
    let files = s.sender.files.lock().unwrap().clone();
    assert_eq!(files, vec![copies_dir(&s).join("ampr_zstd-000.pak"), copies_dir(&s).join("ampr_zstd-001.pak")]);
    assert!(!copies_dir(&s).exists(), "no copy is left");
    assert_eq!(std::fs::read(s.packer.links.join("ampr_zstd-001.pak")).unwrap(), vec![2u8; 1001], "originals untouched");
}

#[test]
fn no_copy_without_room_for_it_and_the_rest_of_the_pack() {
    let mut s = setup(2, Ok(()), vec![], None);
    s.packer.copy_first = true;
    s.packer.left = Some(u64::MAX / 4);
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert!(job.ahead.is_empty());
    assert_eq!(calls(&s.w.log), ["pack", "send"], "everything goes after the pack");
    assert!(!copies_dir(&s).exists());
}

#[test]
fn nothing_is_copied_once_the_packer_publishes() {
    // Part 0 goes (held 300 ms on its way); part 1 is announced, then Forge leaves writing at
    // once: by the time the sender is free, part 1 may no longer be read, so it isn't copied.
    let mut s = setup(2, Ok(()), vec![], Some(Duration::from_millis(300)));
    s.packer.copy_first = true;
    s.packer.close_after = Some(1);
    let mut job = s.w.job(false, true, false, 0);
    let (outcome, _) = go(&s, &mut job, &AtomicBool::new(false));

    assert_eq!(outcome, RunOutcome::Finished);
    assert_eq!(job.ahead, vec![part(0)]);
    assert_eq!(calls(&s.w.log), ["pack", "ahead ampr_zstd-000.pak", "send rest without ampr_zstd-000.pak"]);
    assert!(!copies_dir(&s).exists());
}

#[test]
fn closing_doesnt_stop_parts_read_in_place() {
    // Mac and Linux: the file stays open through the rename, so closing changes nothing.
    let mut s = setup(2, Ok(()), vec![], Some(Duration::from_millis(300)));
    s.packer.close_after = Some(1);
    let mut job = s.w.job(false, true, false, 0);
    go(&s, &mut job, &AtomicBool::new(false));
    assert_eq!(job.ahead, vec![part(0), part(1)], "part 1 still goes ahead, read where it is");
}
