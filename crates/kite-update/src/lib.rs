//! Kite Pack's updates, from its GitHub releases.
//!
//! [`check`] asks GitHub for the newest release and picks the file for this computer (the same
//! files people download: the Windows .exe, the Mac .dmg, the Linux AppImage). [`download`] saves
//! it, then checks it against the release's `SHA256SUMS.txt`. [`install`] puts it where the running
//! app is: the running copy moves aside (`<name>.old`), the new one takes its place, and it starts
//! with the next launch. [`clean_up`] removes the old copy then.
//!
//! No signing keys: the files come over HTTPS from the repository's own releases and must match
//! the checksums published with them.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Where the releases are.
pub const LATEST: &str = "https://api.github.com/repos/nankhor/kite-pack/releases/latest";
const SUMS: &str = "SHA256SUMS.txt";

/// A newer release, with its file for this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    /// "0.1.4".
    pub version: String,
    /// "v0.1.4".
    pub tag: String,
    /// The release notes (its CHANGELOG section).
    pub notes: String,
    /// The release's page on GitHub.
    pub page: String,
    /// This computer's file, by name.
    pub file: String,
    pub url: String,
    pub size: u64,
    /// The release's checksums, when it has them.
    pub sums_url: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// How this computer's file name ends in a release, or None where there is no build for it.
pub fn platform_suffix() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("-windows-x64.exe")
    } else if cfg!(target_os = "macos") {
        Some("-macos.dmg")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-linux-x64.AppImage")
    } else {
        None
    }
}

/// Whether `tag` ("v0.1.4") is newer than `current` ("0.1.3").
pub fn is_newer(current: &str, tag: &str) -> bool {
    let parse = |s: &str| semver::Version::parse(s.trim().trim_start_matches('v')).ok();
    match (parse(current), parse(tag)) {
        (Some(have), Some(latest)) => latest > have,
        _ => false,
    }
}

/// The update in GitHub's answer for the newest release, if it is newer than `current`.
pub fn pick(release_json: &str, current: &str, suffix: &str) -> Result<Option<Update>, String> {
    let release: Release = serde_json::from_str(release_json).map_err(|e| format!("GitHub's answer didn't make sense: {e}"))?;
    if release.draft || release.prerelease || !is_newer(current, &release.tag_name) {
        return Ok(None);
    }
    let version = release.tag_name.trim_start_matches('v').to_string();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .ok_or_else(|| format!("Kite Pack {version} has no file for this computer yet"))?;
    let sums_url = release.assets.iter().find(|a| a.name == SUMS).map(|a| a.browser_download_url.clone());
    Ok(Some(Update {
        version,
        tag: release.tag_name.clone(),
        notes: release.body.unwrap_or_default().trim().to_string(),
        page: release.html_url,
        file: asset.name.clone(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
        sums_url,
    }))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .user_agent("Kite-Pack")
        .build()
        .into()
}

fn http_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(403) | ureq::Error::StatusCode(429) => "GitHub asked to wait. Try again later.".into(),
        ureq::Error::StatusCode(404) => "GitHub has no release to offer".into(),
        ureq::Error::StatusCode(code) => format!("GitHub answered {code}"),
        other => format!("Couldn't reach GitHub: {other}"),
    }
}

/// Asks GitHub whether there is a newer Kite Pack than `current`, at `latest_url` (normally [`LATEST`]).
pub fn check(current: &str, latest_url: &str) -> Result<Option<Update>, String> {
    let suffix = platform_suffix().ok_or("Updates aren't offered for this computer")?;
    let mut res = agent()
        .get(latest_url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(http_error)?;
    let body = res.body_mut().read_to_string().map_err(|e| format!("Couldn't read GitHub's answer: {e}"))?;
    pick(&body, current, suffix)
}

/// The SHA-256 a `SHA256SUMS.txt` gives for `file` (lines of "<hash>  <name>" or "<hash> *<name>").
pub fn expected_sum(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file && hash.len() == 64).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads the update into `dir`, reporting `progress(done, total)`, and checks it against the
/// release's checksums. Returns the file. `cancel` stops it between chunks.
pub fn download(
    update: &Update,
    dir: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let sums_url = update.sums_url.as_deref().ok_or("This release has no checksums to check the download against")?;
    fs::create_dir_all(dir).map_err(|e| format!("Couldn't make the download folder: {e}"))?;
    let part = dir.join(format!("{}.part", update.file));
    let done_path = dir.join(&update.file);

    let mut res = agent().get(&update.url).call().map_err(http_error)?;
    let mut reader = res.body_mut().with_config().limit(u64::MAX).reader();
    let mut out = fs::File::create(&part).map_err(|e| format!("Couldn't save the download: {e}"))?;
    let total = update.size;
    let mut done = 0u64;
    let mut buf = vec![0u8; 1 << 20];
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = fs::remove_file(&part);
            return Err("Stopped".into());
        }
        let n = reader.read(&mut buf).map_err(|e| format!("The download broke off: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("Couldn't save the download: {e}"))?;
        done += n as u64;
        progress(done, total);
    }
    out.sync_all().map_err(|e| format!("Couldn't save the download: {e}"))?;
    drop(out);
    if total > 0 && done != total {
        let _ = fs::remove_file(&part);
        return Err(format!("The download stopped at {done} of {total} bytes"));
    }

    let mut sums = agent().get(sums_url).call().map_err(http_error)?;
    let sums = sums.body_mut().read_to_string().map_err(|e| format!("Couldn't read the checksums: {e}"))?;
    let want = expected_sum(&sums, &update.file).ok_or_else(|| format!("The checksums don't list {}", update.file))?;
    let have = sha256_file(&part).map_err(|e| format!("Couldn't check the download: {e}"))?;
    if have != want {
        let _ = fs::remove_file(&part);
        return Err("The download doesn't match its checksum, so it wasn't installed".into());
    }
    fs::rename(&part, &done_path).map_err(|e| format!("Couldn't save the download: {e}"))?;
    Ok(done_path)
}

/// `<path>.old`: where the replaced copy waits until the next launch.
pub fn old_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    path.with_file_name(name)
}

/// Puts `new` where `target` is. `target` moves aside to `<target>.old` first, because a running
/// program can be renamed but not overwritten on Windows; if the copy fails, it comes back.
pub fn swap_file(target: &Path, new: &Path) -> io::Result<()> {
    let old = old_path(target);
    let _ = fs::remove_file(&old);
    fs::rename(target, &old)?;
    // A copy, not a rename: the download may be on another drive.
    let copied = fs::copy(new, target).and_then(|_| make_runnable(target));
    if let Err(e) = copied {
        let _ = fs::remove_file(target);
        let _ = fs::rename(&old, target);
        return Err(e);
    }
    Ok(())
}

/// Puts the folder `new` where the folder `target` is, the same way (the Mac's .app is a folder).
pub fn swap_dir(target: &Path, new: &Path) -> io::Result<()> {
    let old = old_path(target);
    if old.exists() {
        fs::remove_dir_all(&old)?;
    }
    fs::rename(target, &old)?;
    if let Err(e) = fs::rename(new, target) {
        let _ = fs::rename(&old, target);
        return Err(e);
    }
    Ok(())
}

#[cfg(unix)]
fn make_runnable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_runnable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// The running app's file (Windows .exe, Linux AppImage) or folder (Mac .app).
fn running_app() -> Result<PathBuf, String> {
    if cfg!(target_os = "linux") {
        return std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .ok_or_else(|| "Updates install into the AppImage; this copy isn't one".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find the running app: {e}"))?;
    if cfg!(target_os = "macos") {
        // …/Kite Pack.app/Contents/MacOS/kite-app
        return exe
            .ancestors()
            .nth(3)
            .filter(|p| p.extension().is_some_and(|x| x == "app"))
            .map(Path::to_path_buf)
            .ok_or_else(|| "Kite Pack isn't running from its .app, so it can't update itself".to_string());
    }
    Ok(exe)
}

/// Puts the downloaded update where the running app is. The new copy starts with the next launch.
pub fn install(downloaded: &Path) -> Result<(), String> {
    let target = running_app()?;
    if cfg!(target_os = "macos") {
        return install_from_dmg(downloaded, &target);
    }
    swap_file(&target, downloaded).map_err(|e| format!("Couldn't put the new version in place: {e}"))
}

/// The .app inside the downloaded .dmg replaces the running one.
fn install_from_dmg(dmg: &Path, bundle: &Path) -> Result<(), String> {
    use std::process::Command;
    let mount = std::env::temp_dir().join(format!("kite-update-{}", std::process::id()));
    let _ = fs::create_dir_all(&mount);
    let attached = Command::new("hdiutil")
        .args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
        .arg(&mount)
        .arg(dmg)
        .output()
        .map_err(|e| format!("Couldn't open the .dmg: {e}"))?;
    if !attached.status.success() {
        return Err(format!("Couldn't open the .dmg: {}", String::from_utf8_lossy(&attached.stderr).trim()));
    }
    let result = (|| {
        let app = fs::read_dir(&mount)
            .map_err(|e| format!("Couldn't read the .dmg: {e}"))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().is_some_and(|x| x == "app"))
            .ok_or("The .dmg has no app in it")?;
        let mut staged_name = bundle.file_name().unwrap_or_default().to_os_string();
        staged_name.push(".new");
        let staged = bundle.with_file_name(staged_name);
        let _ = fs::remove_dir_all(&staged);
        let copied = Command::new("ditto").arg(&app).arg(&staged).status().map_err(|e| e.to_string())?;
        if !copied.success() {
            let _ = fs::remove_dir_all(&staged);
            return Err(format!("Couldn't copy the new version next to {}", bundle.display()));
        }
        swap_dir(bundle, &staged).map_err(|e| {
            let _ = fs::remove_dir_all(&staged);
            format!("Couldn't put the new version in place: {e}")
        })?;
        // Files this app wrote carry no quarantine mark; make sure, so the Mac opens it as before.
        let _ = Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(bundle).status();
        Ok(())
    })();
    let _ = Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mount).status();
    let _ = fs::remove_dir(&mount);
    result
}

/// Removes the copy an update replaced, once the new one is running.
pub fn clean_up() {
    let Ok(app) = running_app() else { return };
    let old = old_path(&app);
    if old.is_dir() {
        let _ = fs::remove_dir_all(&old);
    } else if old.exists() {
        let _ = fs::remove_file(&old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::thread;

    const RELEASE: &str = r#"{
        "tag_name": "v0.1.4",
        "html_url": "https://github.com/nankhor/kite-pack/releases/tag/v0.1.4",
        "body": "- The Pack page shows the game's story.\n",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": "Kite-Pack-v0.1.4-macos.dmg", "browser_download_url": "https://example.com/mac.dmg", "size": 10},
            {"name": "Kite-Pack-v0.1.4-windows-x64.exe", "browser_download_url": "https://example.com/win.exe", "size": 20},
            {"name": "SHA256SUMS.txt", "browser_download_url": "https://example.com/sums", "size": 300}
        ]
    }"#;

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.1.3", "v0.1.4"));
        assert!(is_newer("0.1.3", "0.2.0"));
        assert!(!is_newer("0.1.3", "v0.1.3"));
        assert!(!is_newer("0.1.4", "v0.1.3"));
        assert!(!is_newer("0.1.3", "nightly"));
    }

    #[test]
    fn picks_this_computers_file() {
        let u = pick(RELEASE, "0.1.3", "-windows-x64.exe").unwrap().unwrap();
        assert_eq!(u.version, "0.1.4");
        assert_eq!(u.tag, "v0.1.4");
        assert_eq!(u.file, "Kite-Pack-v0.1.4-windows-x64.exe");
        assert_eq!(u.url, "https://example.com/win.exe");
        assert_eq!(u.size, 20);
        assert_eq!(u.sums_url.as_deref(), Some("https://example.com/sums"));
        assert_eq!(u.notes, "- The Pack page shows the game's story.");
    }

    #[test]
    fn offers_nothing_old_or_unfinished() {
        assert_eq!(pick(RELEASE, "0.1.4", "-macos.dmg").unwrap(), None);
        let pre = RELEASE.replace(r#""prerelease": false"#, r#""prerelease": true"#);
        assert_eq!(pick(&pre, "0.1.3", "-macos.dmg").unwrap(), None);
        let draft = RELEASE.replace(r#""draft": false"#, r#""draft": true"#);
        assert_eq!(pick(&draft, "0.1.3", "-macos.dmg").unwrap(), None);
    }

    #[test]
    fn says_when_this_computer_has_no_file() {
        let e = pick(RELEASE, "0.1.3", "-linux-x64.AppImage").unwrap_err();
        assert!(e.contains("no file for this computer"), "{e}");
        assert!(pick("not json", "0.1.3", "-macos.dmg").is_err());
    }

    #[test]
    fn reads_checksums() {
        let sums = format!("{}  Kite-Pack-v0.1.4-macos.dmg\n{} *kite-helper.elf\n", "a".repeat(64), "B".repeat(64));
        assert_eq!(expected_sum(&sums, "Kite-Pack-v0.1.4-macos.dmg"), Some("a".repeat(64)));
        assert_eq!(expected_sum(&sums, "kite-helper.elf"), Some("b".repeat(64)));
        assert_eq!(expected_sum(&sums, "other"), None);
    }

    #[test]
    fn hashes_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("abc");
        fs::write(&f, b"abc").unwrap();
        assert_eq!(sha256_file(&f).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn swaps_a_file_and_keeps_the_old_one_aside() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("kite-app.exe");
        let new = tmp.path().join("download").join("new.exe");
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        fs::write(&app, b"old").unwrap();
        fs::write(&new, b"new").unwrap();
        swap_file(&app, &new).unwrap();
        assert_eq!(fs::read(&app).unwrap(), b"new");
        assert_eq!(fs::read(tmp.path().join("kite-app.exe.old")).unwrap(), b"old");
    }

    #[test]
    fn puts_the_old_file_back_when_the_copy_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("kite-app.exe");
        fs::write(&app, b"old").unwrap();
        assert!(swap_file(&app, &tmp.path().join("missing")).is_err());
        assert_eq!(fs::read(&app).unwrap(), b"old");
    }

    #[test]
    fn swaps_a_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("Kite Pack.app");
        let new = tmp.path().join("Kite Pack.app.new");
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/v"), b"old").unwrap();
        fs::create_dir_all(new.join("Contents")).unwrap();
        fs::write(new.join("Contents/v"), b"new").unwrap();
        swap_dir(&app, &new).unwrap();
        assert_eq!(fs::read(app.join("Contents/v")).unwrap(), b"new");
        assert_eq!(fs::read(tmp.path().join("Kite Pack.app.old/Contents/v")).unwrap(), b"old");
        assert!(!new.exists());
    }

    /// Serves `files` (path → body) over HTTP on localhost, for as many requests as it gets.
    fn serve(files: Vec<(&'static str, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).unwrap() <= 2 {
                        break;
                    }
                }
                let mut stream = stream;
                match files.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                        stream.write_all(body).unwrap();
                    }
                    None => write!(stream, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap(),
                }
            }
        });
        format!("http://{addr}")
    }

    fn update(base: &str, size: u64) -> Update {
        Update {
            version: "0.1.4".into(),
            tag: "v0.1.4".into(),
            notes: String::new(),
            page: String::new(),
            file: "Kite-Pack-v0.1.4-linux-x64.AppImage".into(),
            url: format!("{base}/app"),
            size,
            sums_url: Some(format!("{base}/sums")),
        }
    }

    #[test]
    fn downloads_and_checks_the_file() {
        let body = vec![7u8; 3 << 20];
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("body"), &body).unwrap();
        let sum = sha256_file(&tmp.path().join("body")).unwrap();
        let sums = format!("{sum}  Kite-Pack-v0.1.4-linux-x64.AppImage\n").into_bytes();
        let base = serve(vec![("/app", body.clone()), ("/sums", sums)]);

        let mut seen = vec![];
        let file = download(&update(&base, body.len() as u64), &tmp.path().join("dl"), &AtomicBool::new(false), |d, t| {
            seen.push((d, t))
        })
        .unwrap();
        assert_eq!(fs::read(&file).unwrap(), body);
        assert_eq!(seen.last(), Some(&(body.len() as u64, body.len() as u64)));
        assert!(!tmp.path().join("dl/Kite-Pack-v0.1.4-linux-x64.AppImage.part").exists());
    }

    #[test]
    fn refuses_a_file_that_doesnt_match() {
        let sums = format!("{}  Kite-Pack-v0.1.4-linux-x64.AppImage\n", "0".repeat(64)).into_bytes();
        let base = serve(vec![("/app", b"tampered".to_vec()), ("/sums", sums)]);
        let tmp = tempfile::tempdir().unwrap();
        let e = download(&update(&base, 8), tmp.path(), &AtomicBool::new(false), |_, _| {}).unwrap_err();
        assert!(e.contains("doesn't match"), "{e}");
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn refuses_a_release_without_checksums() {
        let mut u = update("http://127.0.0.1:9", 1);
        u.sums_url = None;
        let tmp = tempfile::tempdir().unwrap();
        assert!(download(&u, tmp.path(), &AtomicBool::new(false), |_, _| {}).unwrap_err().contains("no checksums"));
    }

    #[test]
    fn stops_when_asked() {
        let base = serve(vec![("/app", vec![1u8; 1 << 20]), ("/sums", vec![])]);
        let tmp = tempfile::tempdir().unwrap();
        let e = download(&update(&base, 1 << 20), tmp.path(), &AtomicBool::new(true), |_, _| {}).unwrap_err();
        assert_eq!(e, "Stopped");
    }

    #[test]
    fn checks_github_for_a_newer_release() {
        let base = serve(vec![("/latest", RELEASE.as_bytes().to_vec())]);
        let latest = format!("{base}/latest");
        // This computer's file may or may not be in the sample; either way it's newer than 0.1.3.
        match check("0.1.3", &latest) {
            Ok(Some(u)) => assert_eq!(u.version, "0.1.4"),
            Ok(None) => panic!("0.1.4 is newer"),
            Err(e) => assert!(e.contains("no file for this computer") || e.contains("aren't offered"), "{e}"),
        }
        assert_eq!(check("0.1.4", &latest).ok().flatten(), None);
    }
}
