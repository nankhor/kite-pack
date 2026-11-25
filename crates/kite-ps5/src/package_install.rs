//! Installing a package as it is: the PS5's installer streams it from this computer.
//!
//! This is ps5upload's route for fake packages: Sony's `sceAppInstUtilInstallByPackage` gets a
//! link to the package on this computer and downloads it while it installs, so nothing is copied
//! to the PS5 first. Sony reads the end of the package first and fetches some ranges more than
//! once, so progress counts each byte once, whatever the order. The install is confirmed by
//! finding the package where Sony keeps it, with the same size, beginning and end.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use kite_forge::PackageInfo;
use kite_pipeline::{InstallError, InstallOutcome, Progress};

use crate::connect::{Console, Keys, open};
use crate::meta::{self, MetaError};
use crate::{PkgServer, Ps5Error, runtime};

/// How much of the beginning and of the end is compared to know it's the same package.
const SAMPLE: u64 = 64 * 1024;
/// How often progress is read.
const POLL: Duration = Duration::from_secs(2);
/// How often the PS5 is asked whether the package is installed yet.
const CHECK_EVERY: Duration = Duration::from_secs(15);
/// After a restart, how long to wait for Sony to come back to the link before asking again.
const RESUME_WAIT: Duration = Duration::from_secs(90);
/// No new byte for this long, short of the end: the PS5 stopped downloading.
const STALL: Duration = Duration::from_secs(10 * 60);

/// What installing needs from the PS5.
pub(crate) trait ConsoleOps {
    /// Asks Sony's installer to install the package at `url`, shown as `name`.
    fn request_install(&self, url: &str, name: &str) -> Result<(), Ps5Error>;
    /// The size of the file at `path`, or `None` when there's no file there.
    fn stat_size(&self, path: &str) -> Result<Option<u64>, Ps5Error>;
    fn read_at(&self, path: &str, at: u64, len: usize) -> Result<Vec<u8>, Ps5Error>;
    /// The names in the folder `path`, each with whether it is a folder; empty when it's missing.
    fn list(&self, path: &str) -> Result<Vec<(String, bool)>, Ps5Error>;
}

pub(crate) trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
}

pub(crate) struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
}

/// The file that says an install of this link was asked for and hasn't been confirmed yet.
pub(crate) fn marker(markers: &Path, server: &PkgServer) -> PathBuf {
    let hash = server
        .url()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    markers.join(format!("{hash:016x}.started"))
}

/// Installs `pkg`, served by `server`, and waits until the PS5 has it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn install_with<O: ConsoleOps + ?Sized>(
    ops: &O,
    pkg: &Path,
    info: &PackageInfo,
    server: &PkgServer,
    markers: &Path,
    progress: &dyn Fn(Progress),
    cancel: &AtomicBool,
    clock: &dyn Clock,
) -> Result<InstallOutcome, InstallError> {
    if cancel.load(Ordering::SeqCst) {
        return Err(InstallError::Cancelled);
    }
    let size = server.size();
    let local = Samples::of(pkg, size).map_err(|e| InstallError::Failed(format!("Couldn't read the package: {e}")))?;
    let marker = marker(markers, server);

    if find_installed(ops, info, size, &local).map_err(from_ps5)? {
        std::fs::remove_file(&marker).ok();
        return Ok(InstallOutcome::AlreadyThere);
    }

    // Started before a restart: Sony may still be downloading from the same link. Give it time
    // to come back before asking for a second install of the same package.
    let mut carried_on = false;
    if marker.exists() {
        let until = clock.now() + RESUME_WAIT;
        while clock.now() < until {
            if cancel.load(Ordering::SeqCst) {
                return Err(InstallError::Cancelled);
            }
            if server.requests() > 0 {
                carried_on = true;
                break;
            }
            clock.sleep(POLL);
        }
    }
    if !carried_on {
        let name = info.title.clone().unwrap_or_else(|| info.content_id.clone());
        ops.request_install(server.url(), &name).map_err(refused)?;
        if let Some(dir) = marker.parent() {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(&marker, server.url()).ok();
    }

    let finishing = Duration::from_secs((size / 50_000_000).max(15 * 60));
    let mut shown = 0;
    let mut last_change = clock.now();
    let mut last_check = clock.now();
    let mut nearly_all_at: Option<Instant> = None;
    loop {
        if cancel.load(Ordering::SeqCst) {
            std::fs::remove_file(&marker).ok();
            return Err(InstallError::Cancelled);
        }
        let now = clock.now();
        let got = server.received().min(size);
        if got > shown {
            shown = got;
            last_change = now;
            progress(Progress::Bytes { done: got, total: size });
        }
        let nearly_all = got.saturating_mul(100) >= size.saturating_mul(99);
        if nearly_all && nearly_all_at.is_none() {
            nearly_all_at = Some(now);
        }
        if now.duration_since(last_check) >= CHECK_EVERY || (nearly_all && nearly_all_at == Some(now)) {
            last_check = now;
            // A check that fails is not the install failing: the bytes say whether it goes on.
            if find_installed(ops, info, size, &local).unwrap_or(false) {
                if shown < size {
                    progress(Progress::Bytes { done: size, total: size });
                }
                std::fs::remove_file(&marker).ok();
                return Ok(InstallOutcome::Installed);
            }
        }
        if !nearly_all && now.duration_since(last_change) >= STALL {
            std::fs::remove_file(&marker).ok();
            return Err(InstallError::Failed(
                "The PS5 stopped downloading the package. Check it's still on and connected, then try again.".into(),
            ));
        }
        if let Some(at) = nearly_all_at
            && now.duration_since(at) >= finishing
        {
            std::fs::remove_file(&marker).ok();
            return Err(InstallError::Failed(
                "The PS5 has the whole package but never finished installing it. Check the PS5's notifications.".into(),
            ));
        }
        clock.sleep(POLL);
    }
}

/// The beginning and the end of a package, to recognise it on the PS5.
struct Samples {
    head: Vec<u8>,
    tail: Vec<u8>,
}

impl Samples {
    fn of(pkg: &Path, size: u64) -> std::io::Result<Samples> {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = std::fs::File::open(pkg)?;
        let len = SAMPLE.min(size);
        let mut head = vec![0; len as usize];
        f.read_exact(&mut head)?;
        let mut tail = vec![0; len as usize];
        f.seek(SeekFrom::Start(size - len))?;
        f.read_exact(&mut tail)?;
        Ok(Samples { head, tail })
    }
}

/// Whether the package is on the PS5 where Sony keeps that kind of package.
fn find_installed<O: ConsoleOps + ?Sized>(ops: &O, info: &PackageInfo, size: u64, local: &Samples) -> Result<bool, Ps5Error> {
    let id = &info.title_id;
    let mut places: Vec<String> = Vec::new();
    match info.kind.as_str() {
        "update" => places.push(format!("/user/patch/{id}/patch.pkg")),
        "dlc" => {
            let root = format!("/user/addcont/{id}");
            for (name, folder) in ops.list(&root)? {
                if folder {
                    for (inner, inner_folder) in ops.list(&format!("{root}/{name}"))? {
                        if !inner_folder && is_pkg(&inner) {
                            places.push(format!("{root}/{name}/{inner}"));
                        }
                    }
                } else if is_pkg(&name) {
                    places.push(format!("{root}/{name}"));
                }
            }
        }
        _ => {
            for drive in ["", "/mnt/ext0", "/mnt/ext1"] {
                places.push(format!("{drive}/user/app/{id}/app.pkg"));
            }
        }
    }
    for place in places {
        if ops.stat_size(&place)? != Some(size) {
            continue;
        }
        let len = local.head.len();
        if ops.read_at(&place, 0, len)? == local.head && ops.read_at(&place, size - len as u64, len)? == local.tail {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_pkg(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pkg")
}

fn from_ps5(e: Ps5Error) -> InstallError {
    match e {
        Ps5Error::Unreachable(why) => InstallError::Unreachable(why),
        other => InstallError::Failed(other.message()),
    }
}

/// Sony said no. With a fake package that almost always means kstuff's fake-package support
/// isn't running.
fn refused(e: Ps5Error) -> InstallError {
    match e {
        Ps5Error::Unreachable(why) => InstallError::Unreachable(why),
        other => {
            let why = other.message();
            let why = why.trim_end_matches('.');
            InstallError::Failed(format!(
                "{why}. Fake packages install only while kstuff with fake-package support is running on the PS5."
            ))
        }
    }
}

/// The PS5 through the Kite helper.
pub(crate) struct HelperOps<'a> {
    pub console: &'a Console,
    pub keys: &'a Keys,
}

impl ConsoleOps for HelperOps<'_> {
    fn request_install(&self, url: &str, name: &str) -> Result<(), Ps5Error> {
        crate::install_url(self.console, self.keys, url, name)
    }

    fn stat_size(&self, path: &str) -> Result<Option<u64>, Ps5Error> {
        runtime().block_on(async {
            let s = open(self.console, &self.keys.checker, self.keys).await?;
            let out = meta::stat(&s, path).await;
            s.close().await;
            match out {
                Ok(e) if e.kind == "file" => Ok(Some(e.size)),
                Ok(_) | Err(MetaError::NotFound) | Err(MetaError::Path) => Ok(None),
                Err(e) => Err(meta::into_ps5(e)),
            }
        })
    }

    fn read_at(&self, path: &str, at: u64, len: usize) -> Result<Vec<u8>, Ps5Error> {
        runtime().block_on(async {
            let s = open(self.console, &self.keys.checker, self.keys).await?;
            let out = meta::read_at(&s, path, at, len).await;
            s.close().await;
            out.map_err(meta::into_ps5)
        })
    }

    fn list(&self, path: &str) -> Result<Vec<(String, bool)>, Ps5Error> {
        runtime().block_on(async {
            let s = open(self.console, &self.keys.checker, self.keys).await?;
            let out = meta::list_lenient(&s, path).await;
            s.close().await;
            match out {
                Ok(entries) => Ok(entries.into_iter().map(|e| { let folder = e.is_folder(); (e.name, folder) }).collect()),
                Err(MetaError::Path) => Ok(Vec::new()),
                Err(e) => Err(meta::into_ps5(e)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::io::{Read, Write};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;

    /// How the fake PS5 answers an install request.
    #[derive(Clone)]
    enum Does {
        /// Fetches everything (the end first, one range twice), then the file is at `path`.
        Install(String),
        /// Fetches half, then nothing more.
        StallHalfway,
        /// Refuses with this message.
        Refuse(String),
    }

    struct FakePs5 {
        files: Mutex<HashMap<String, Vec<u8>>>,
        asked: Mutex<Vec<String>>,
        does: Does,
        away: AtomicBool,
    }

    impl FakePs5 {
        fn new(does: Does) -> Arc<FakePs5> {
            Arc::new(FakePs5 { files: Mutex::new(HashMap::new()), asked: Mutex::new(Vec::new()), does, away: AtomicBool::new(false) })
        }

        /// Sony's installer at work: fetches `url` from this computer, then installs.
        fn fetch(self: &Arc<Self>, url: String) {
            let me = Arc::clone(self);
            std::thread::spawn(move || {
                let size = head_size(&url);
                let mut got = vec![0u8; size as usize];
                let mut take = |from: u64, to: u64| {
                    let bytes = get(&url, from, to);
                    got[from as usize..=to as usize].copy_from_slice(&bytes);
                };
                let tail = size.saturating_sub(64 * 1024);
                take(tail, size - 1);
                let step = 1 << 20;
                let stop = if matches!(me.does, Does::StallHalfway) { size / 2 } else { size };
                let mut at = 0;
                while at < stop {
                    let end = (at + step).min(stop) - 1;
                    take(at, end);
                    if at == 0 {
                        take(at, end);
                    }
                    at = end + 1;
                }
                if let Does::Install(path) = &me.does {
                    me.files.lock().unwrap().insert(path.clone(), got);
                }
            });
        }
    }

    fn head_size(url: &str) -> u64 {
        let text = String::from_utf8_lossy(&request(url, "HEAD", None)).into_owned();
        let line = text.lines().find(|l| l.to_ascii_lowercase().starts_with("content-length:")).unwrap();
        line.split(':').nth(1).unwrap().trim().parse().unwrap()
    }

    fn get(url: &str, from: u64, to: u64) -> Vec<u8> {
        let out = request(url, "GET", Some((from, to)));
        let at = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        out[at..].to_vec()
    }

    fn request(url: &str, method: &str, range: Option<(u64, u64)>) -> Vec<u8> {
        let rest = url.trim_start_matches("http://");
        let (addr, path) = rest.split_once('/').unwrap();
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        let range = range.map(|(a, b)| format!("Range: bytes={a}-{b}\r\n")).unwrap_or_default();
        s.write_all(format!("{method} /{path} HTTP/1.1\r\nHost: x\r\n{range}\r\n").as_bytes()).unwrap();
        let mut out = Vec::new();
        s.read_to_end(&mut out).unwrap();
        out
    }

    impl ConsoleOps for Arc<FakePs5> {
        fn request_install(&self, url: &str, name: &str) -> Result<(), Ps5Error> {
            if self.away.load(Ordering::SeqCst) {
                return Err(Ps5Error::Unreachable("away".into()));
            }
            self.asked.lock().unwrap().push(name.to_string());
            if let Does::Refuse(why) = &self.does {
                return Err(Ps5Error::Failed(why.clone()));
            }
            self.fetch(url.to_string());
            Ok(())
        }
        fn stat_size(&self, path: &str) -> Result<Option<u64>, Ps5Error> {
            if self.away.load(Ordering::SeqCst) {
                return Err(Ps5Error::Unreachable("away".into()));
            }
            Ok(self.files.lock().unwrap().get(path).map(|b| b.len() as u64))
        }
        fn read_at(&self, path: &str, at: u64, len: usize) -> Result<Vec<u8>, Ps5Error> {
            let files = self.files.lock().unwrap();
            let b = files.get(path).ok_or_else(|| Ps5Error::Failed("missing".into()))?;
            let at = (at as usize).min(b.len());
            Ok(b[at..(at + len).min(b.len())].to_vec())
        }
        fn list(&self, path: &str) -> Result<Vec<(String, bool)>, Ps5Error> {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            let mut names: Vec<(String, bool)> = Vec::new();
            for key in self.files.lock().unwrap().keys() {
                if let Some(rest) = key.strip_prefix(&prefix) {
                    let (first, deeper) = match rest.split_once('/') {
                        Some((f, _)) => (f.to_string(), true),
                        None => (rest.to_string(), false),
                    };
                    if !names.iter().any(|(n, _)| *n == first) {
                        names.push((first, deeper));
                    }
                }
            }
            Ok(names)
        }
    }

    /// Time that passes as fast as the test wants: each sleep moves it on, with a moment of real
    /// time so the fake PS5's threads get to run.
    struct FastClock(Mutex<Instant>);

    impl Clock for FastClock {
        fn now(&self) -> Instant {
            *self.0.lock().unwrap()
        }
        fn sleep(&self, d: Duration) {
            std::thread::sleep(Duration::from_millis(5));
            *self.0.lock().unwrap() += d;
        }
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        pkg: PathBuf,
        bytes: Vec<u8>,
        info: PackageInfo,
        markers: PathBuf,
    }

    fn setup(kind: &str) -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let mut x = 0x1234_5678u32;
        let bytes: Vec<u8> = (0..(3 << 20) + 123)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let pkg = tmp.path().join("Ashen Tide.pkg");
        std::fs::write(&pkg, &bytes).unwrap();
        let info = PackageInfo {
            platform: "ps5".into(),
            kind: kind.into(),
            title_id: "PPSA01234".into(),
            content_id: "UP0000-PPSA01234_00-TIDEBREAKER00000".into(),
            title: Some("Ashen Tide".into()),
            version: None,
            bytes: bytes.len() as u64,
            icon_data_url: None,
            packable: true,
        };
        let markers = tmp.path().join("installs");
        Setup { _tmp: tmp, pkg, bytes, info, markers }
    }

    fn run(s: &Setup, ps5: &Arc<FakePs5>, cancel: &AtomicBool) -> (Result<InstallOutcome, InstallError>, Vec<(u64, u64)>) {
        let server = PkgServer::start_stable(&s.pkg, "127.0.0.1").unwrap();
        let seen = Mutex::new(Vec::new());
        let progress = |p: Progress| {
            if let Progress::Bytes { done, total } = p {
                seen.lock().unwrap().push((done, total));
            }
        };
        let clock = FastClock(Mutex::new(Instant::now()));
        let out = install_with(ps5, &s.pkg, &s.info, &server, &s.markers, &progress, cancel, &clock);
        (out, seen.into_inner().unwrap())
    }

    const GAME: &str = "/user/app/PPSA01234/app.pkg";

    #[test]
    fn installs_a_game_and_confirms_it() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install(GAME.into()));
        let (out, progress) = run(&s, &ps5, &AtomicBool::new(false));
        assert_eq!(out, Ok(InstallOutcome::Installed));
        assert_eq!(*ps5.asked.lock().unwrap(), ["Ashen Tide"]);
        let size = s.bytes.len() as u64;
        assert_eq!(progress.last(), Some(&(size, size)));
        assert!(progress.iter().all(|&(d, t)| d <= t && t == size), "never past the whole");
        assert!(progress.windows(2).all(|w| w[0].0 <= w[1].0), "only goes up");
        assert_eq!(std::fs::read_dir(&s.markers).map(|d| d.count()).unwrap_or(0), 0, "no marker left");
    }

    #[test]
    fn an_update_lands_in_patch() {
        let s = setup("update");
        let ps5 = FakePs5::new(Does::Install("/user/patch/PPSA01234/patch.pkg".into()));
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::Installed));
    }

    #[test]
    fn a_dlc_lands_under_addcont() {
        let s = setup("dlc");
        let ps5 = FakePs5::new(Does::Install("/user/addcont/PPSA01234/TIDEBREAKER00000/ac.pkg".into()));
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::Installed));
    }

    #[test]
    fn finds_a_game_on_an_extended_drive() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install("/mnt/ext1/user/app/PPSA01234/app.pkg".into()));
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::Installed));
    }

    #[test]
    fn already_there_asks_nothing() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install(GAME.into()));
        ps5.files.lock().unwrap().insert(GAME.into(), s.bytes.clone());
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::AlreadyThere));
        assert!(ps5.asked.lock().unwrap().is_empty());
    }

    #[test]
    fn another_build_of_the_same_game_is_installed_over() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install(GAME.into()));
        let mut other = s.bytes.clone();
        let last = other.len() - 1;
        other[last] ^= 1;
        ps5.files.lock().unwrap().insert(GAME.into(), other);
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::Installed));
        assert_eq!(ps5.asked.lock().unwrap().len(), 1);
        assert!(ps5.files.lock().unwrap()[GAME] == s.bytes);
    }

    #[test]
    fn carries_on_an_install_after_a_restart() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install(GAME.into()));
        // Started before the restart: the marker is there and Sony keeps fetching the same link.
        let server = PkgServer::start_stable(&s.pkg, "127.0.0.1").unwrap();
        std::fs::create_dir_all(&s.markers).unwrap();
        std::fs::write(marker(&s.markers, &server), b"").unwrap();
        let url = server.url().to_string();
        drop(server);
        let later = Arc::clone(&ps5);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            later.fetch(url);
        });
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::Installed));
        assert!(ps5.asked.lock().unwrap().is_empty(), "no second install while the first goes on");
    }

    #[test]
    fn asks_again_when_nothing_comes_after_a_restart() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install(GAME.into()));
        let server = PkgServer::start_stable(&s.pkg, "127.0.0.1").unwrap();
        std::fs::create_dir_all(&s.markers).unwrap();
        std::fs::write(marker(&s.markers, &server), b"").unwrap();
        drop(server);
        assert_eq!(run(&s, &ps5, &AtomicBool::new(false)).0, Ok(InstallOutcome::Installed));
        assert_eq!(ps5.asked.lock().unwrap().len(), 1);
    }

    #[test]
    fn fails_when_the_ps5_stops_fetching() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::StallHalfway);
        let (out, _) = run(&s, &ps5, &AtomicBool::new(false));
        assert!(matches!(&out, Err(InstallError::Failed(why)) if why.contains("stopped downloading")), "{out:?}");
    }

    #[test]
    fn a_refusal_names_kstuff() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Refuse("Sony's installer refused it (0x80B2116F)".into()));
        let (out, _) = run(&s, &ps5, &AtomicBool::new(false));
        assert!(matches!(&out, Err(InstallError::Failed(why)) if why.contains("0x80B2116F") && why.contains("kstuff")), "{out:?}");
    }

    #[test]
    fn a_ps5_away_at_the_start_is_waited_for() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::Install(GAME.into()));
        ps5.away.store(true, Ordering::SeqCst);
        let (out, _) = run(&s, &ps5, &AtomicBool::new(false));
        assert!(matches!(out, Err(InstallError::Unreachable(_))), "{out:?}");
    }

    #[test]
    fn cancel_stops_it() {
        let s = setup("game");
        let ps5 = FakePs5::new(Does::StallHalfway);
        let cancel = AtomicBool::new(true);
        assert_eq!(run(&s, &ps5, &cancel).0, Err(InstallError::Cancelled));
    }
}
