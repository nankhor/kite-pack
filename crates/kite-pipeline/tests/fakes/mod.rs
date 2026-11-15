//! Test doubles for the pipeline's ports. Each one writes a line per call to a shared log so a
//! test can assert the exact order of what the runner did.
#![allow(dead_code)]

use kite_pipeline::*;
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type CallLog = Arc<Mutex<Vec<String>>>;

pub fn calls(log: &CallLog) -> Vec<String> {
    log.lock().unwrap().clone()
}

pub enum PackResultKind {
    Ok,
    Fail(String),
    Cancel,
}

pub struct FakePacker {
    pub log: CallLog,
    pub result: PackResultKind,
    pub output: PathBuf,
    /// Runs first: lets a test change the source while it is "being packed".
    pub before: Option<Box<dyn Fn() + Send + Sync>>,
}

impl Packer for FakePacker {
    fn pack(
        &self,
        _req: &PackRequest,
        progress: &dyn Fn(Progress),
        _cancel: &AtomicBool,
    ) -> Result<PackOutcome, PackError> {
        self.log.lock().unwrap().push("pack".into());
        if let Some(before) = &self.before {
            before();
        }
        progress(Progress::Bytes { done: 1, total: 1 });
        match &self.result {
            PackResultKind::Ok => {
                fs::create_dir_all(&self.output).unwrap();
                fs::write(self.output.join("ampr_zstd.index"), b"AMPRZPK1").unwrap();
                Ok(PackOutcome { output: self.output.clone(), bytes: 100, files: 3, checks: vec![] })
            }
            PackResultKind::Fail(r) => Err(PackError::Failed(r.clone())),
            PackResultKind::Cancel => Err(PackError::Cancelled),
        }
    }
}

pub struct FakeTrash {
    pub log: CallLog,
    pub fail: Option<String>,
    /// Where trashed paths are moved, so a test can see the original is gone.
    pub bin: PathBuf,
}

impl Trash for FakeTrash {
    fn trash(&self, path: &Path) -> Result<(), String> {
        self.log.lock().unwrap().push(format!("trash {}", path.display()));
        if let Some(reason) = &self.fail {
            return Err(reason.clone());
        }
        fs::create_dir_all(&self.bin).map_err(|e| e.to_string())?;
        let n = fs::read_dir(&self.bin).map(|d| d.count()).unwrap_or(0);
        fs::rename(path, self.bin.join(format!("{n}"))).map_err(|e| e.to_string())
    }
}

pub struct FakeSender {
    pub log: CallLog,
    /// Results handed out in order; once empty, every send succeeds.
    pub script: Mutex<VecDeque<Result<(), SendError>>>,
}

impl Sender for FakeSender {
    fn send(
        &self,
        _local: &Path,
        _target: &SendTarget,
        _progress: &dyn Fn(Progress),
        _cancel: &AtomicBool,
    ) -> Result<(), SendError> {
        self.log.lock().unwrap().push("send".into());
        self.script.lock().unwrap().pop_front().unwrap_or(Ok(()))
    }
}

pub struct FakeInstaller {
    pub log: CallLog,
    /// File names of packages the PS5 already has.
    pub installed: Mutex<HashSet<String>>,
    pub script: Mutex<VecDeque<Result<(), InstallError>>>,
}

fn name(pkg: &Path) -> String {
    pkg.file_name().unwrap().to_string_lossy().into_owned()
}

impl Installer for FakeInstaller {
    fn is_installed(&self, pkg: &Path, _target: &SendTarget) -> Result<bool, InstallError> {
        self.log.lock().unwrap().push(format!("installed? {}", name(pkg)));
        Ok(self.installed.lock().unwrap().contains(&name(pkg)))
    }

    fn install(&self, pkg: &Path, _target: &SendTarget, _cancel: &AtomicBool) -> Result<(), InstallError> {
        self.log.lock().unwrap().push(format!("install {}", name(pkg)));
        let result = self.script.lock().unwrap().pop_front().unwrap_or(Ok(()));
        if result.is_ok() {
            self.installed.lock().unwrap().insert(name(pkg));
        }
        result
    }
}

/// Never sleeps; records what it was asked, and can raise the cancel flag on its Nth call.
pub struct NoSleep {
    pub slept: Mutex<Vec<Duration>>,
    pub cancel_on_call: Option<(usize, Arc<AtomicBool>)>,
    calls: AtomicUsize,
}

impl NoSleep {
    pub fn new() -> NoSleep {
        NoSleep { slept: Mutex::new(vec![]), cancel_on_call: None, calls: AtomicUsize::new(0) }
    }

    pub fn cancelling_on(call: usize, flag: Arc<AtomicBool>) -> NoSleep {
        NoSleep { cancel_on_call: Some((call, flag)), ..NoSleep::new() }
    }
}

impl Sleeper for NoSleep {
    fn sleep(&self, d: Duration) {
        self.slept.lock().unwrap().push(d);
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some((at, flag)) = &self.cancel_on_call {
            if n >= *at {
                flag.store(true, Ordering::SeqCst);
            }
        }
    }
}

/// A source game folder, an output folder and a job store, all in one temp dir.
pub struct World {
    pub tmp: tempfile::TempDir,
    pub source: PathBuf,
    pub out: PathBuf,
    pub store: JobStore,
    pub log: CallLog,
}

pub fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("Ashen Tide");
    fs::create_dir_all(source.join("sce_sys")).unwrap();
    fs::write(source.join("eboot.bin"), b"boot").unwrap();
    fs::write(source.join("sce_sys/param.json"), b"{}").unwrap();
    let out = tmp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    let store = JobStore::open(tmp.path().join("jobs")).unwrap();
    World { tmp, source, out, store, log: CallLog::default() }
}

impl World {
    pub fn options(&self, delete_original: bool, send: bool, delete_packed: bool, dlc: usize) -> Options {
        Options {
            format: OutputFormat::Folder,
            level: Level::Fast,
            output_dir: self.out.clone(),
            delete_original,
            send: send.then(|| SendTarget { console: "ps5".into(), destination: "/data/homebrew".into() }),
            delete_packed,
            dlc: (0..dlc).map(|i| self.tmp.path().join(format!("{i}.pkg"))).collect(),
            bundle: Default::default(),
        }
    }

    pub fn job(&self, delete_original: bool, send: bool, delete_packed: bool, dlc: usize) -> Job {
        Job::new(self.source.clone(), self.options(delete_original, send, delete_packed, dlc)).unwrap()
    }

    pub fn packed_path(&self) -> PathBuf {
        self.out.join("PPSA01234")
    }

    pub fn packer(&self, result: PackResultKind) -> FakePacker {
        FakePacker { log: self.log.clone(), result, output: self.packed_path(), before: None }
    }

    pub fn trash(&self, fail: Option<&str>) -> FakeTrash {
        FakeTrash { log: self.log.clone(), fail: fail.map(str::to_string), bin: self.tmp.path().join("bin") }
    }

    pub fn sender(&self, script: Vec<Result<(), SendError>>) -> FakeSender {
        FakeSender { log: self.log.clone(), script: Mutex::new(script.into()) }
    }

    pub fn installer(&self, installed: &[&str], script: Vec<Result<(), InstallError>>) -> FakeInstaller {
        FakeInstaller {
            log: self.log.clone(),
            installed: Mutex::new(installed.iter().map(|s| s.to_string()).collect()),
            script: Mutex::new(script.into()),
        }
    }
}

/// Runs `job` and returns the outcome with every event except Progress.
pub fn run(
    w: &World,
    job: &mut Job,
    packer: &dyn Packer,
    trash: &dyn Trash,
    sender: &dyn Sender,
    installer: &dyn Installer,
    sleeper: &dyn Sleeper,
    cancel: &AtomicBool,
) -> (RunOutcome, Vec<RunEvent>) {
    let events = Mutex::new(Vec::new());
    let runner = Runner {
        packer,
        trash,
        sender,
        installer,
        sleeper,
        store: &w.store,
        retry_every: Duration::from_secs(5),
    };
    let outcome = runner.run(job, cancel, &|e| {
        if !matches!(e, RunEvent::Progress { .. }) {
            events.lock().unwrap().push(e);
        }
    });
    (outcome, events.into_inner().unwrap())
}
