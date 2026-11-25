// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Kite Pack's desktop app: the screen in `app/src`, the session from `kite-session`, the PS5
//! side from `kite-ps5`.

mod history;
mod packages;

use history::{Entry, History};
use kite_forge::{DlcInfo, ForgePacker, GameInfo, inspect_source};
use kite_pipeline::{Level, Options, OutputFormat, SendTarget, SystemTrash};
use kite_ps5::{
    Console, Found, FreeSpace, GameIcon, Keys, Library, Measured, Ps5Sender, Settings,
    SmpInstaller, SmpStatus, SonyInstaller, StartOutcome, Status,
};
use kite_session::{Emit, JobView, Ports, Session, UiEvent};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

const EVENT: &str = "kite://event";
/// How long closing the app waits for a running step to stop cleanly.
const STOP_WAIT: Duration = Duration::from_secs(15);
/// How long discovery listens for helpers.
const DISCOVER_WAIT: Duration = Duration::from_secs(2);

/// The Kite helper for the PS5, embedded at build time (empty in builds without it).
static HELPER_ELF: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/kite-helper.elf"));

/// Set once the person agreed to close during an install.
#[derive(Default)]
struct Quitting(std::sync::atomic::AtomicBool);

struct Ps5 {
    keys: Keys,
    settings_path: PathBuf,
    /// Shared with the ShadowMount+ installer, which reads the current address and ports.
    settings: Arc<Mutex<Settings>>,
}

/// ShadowMount+ installs, for the DLC that goes through it and for backports.
struct Smp {
    installer: Arc<SmpInstaller>,
}

impl Ps5 {
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    fn console(&self) -> Option<Console> {
        let s = self.settings();
        s.address.map(|address| Console {
            address,
            port: s.helper_port,
        })
    }
}

/// The options the screen sends with Pack.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackOptions {
    format: OutputFormat,
    level: Level,
    output_dir: PathBuf,
    delete_original: bool,
    #[serde(default)]
    send: bool,
    #[serde(default)]
    delete_packed: bool,
    /// DLC packages to install on the PS5 after the game; only when sending. Not used by the
    /// screen yet (the helper cannot install packages), kept for when it can.
    #[serde(default)]
    dlc: Vec<PathBuf>,
    /// DLC and a backport to fold into the packed copy, before it is written.
    #[serde(default)]
    bundle: BundleOptions,
}

/// What the screen picked to go inside the pack.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BundleOptions {
    dlc: Vec<PathBuf>,
    backport: Option<PathBuf>,
}

/// A DLC package found in a chosen folder.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PickedDlc {
    path: PathBuf,
    info: DlcInfo,
}

/// What a chosen folder holds: DLC for the game, and the packages that aren't.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DlcFolder {
    found: Vec<PickedDlc>,
    /// File names of the packages that aren't DLC for this game, or can't be read.
    skipped: Vec<String>,
}

fn emitter(app: AppHandle) -> Emit {
    Arc::new(move |event: UiEvent| {
        let _ = app.emit(EVENT, event);
    })
}

/// Runs blocking work (disk, network) off the main thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| e.to_string())
}

/// Reads a dropped or chosen item: a game, or a package to install as it is. Archives can take
/// a while, so off the main thread.
#[tauri::command]
async fn inspect(path: PathBuf) -> Result<GameInfo, String> {
    blocking(move || inspect_source(&path).map_err(|p| p.message())).await?
}

/// Installs a package on the PS5 as it is: the PS5 downloads it from this computer.
#[tauri::command]
fn install(app: AppHandle, session: State<'_, Session>, ps5: State<'_, Ps5>, source: PathBuf) -> Result<JobView, String> {
    let settings = ps5.settings();
    let address = settings.address.ok_or("Set up your PS5 in Settings first.")?;
    let options = Options {
        format: OutputFormat::Folder,
        level: Level::Fast,
        output_dir: source.parent().map(PathBuf::from).unwrap_or_default(),
        delete_original: false,
        delete_packed: false,
        send: Some(SendTarget { console: format!("{address}:{}", settings.helper_port), destination: settings.destination }),
        dlc: vec![],
        bundle: Default::default(),
        install: true,
    };
    session.start(source, options, emitter(app))
}

/// The person chose "Close anyway" while an install runs.
#[tauri::command]
fn quit(window: tauri::Window, quitting: State<'_, Quitting>) -> Result<(), String> {
    quitting.0.store(true, std::sync::atomic::Ordering::SeqCst);
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
fn start(
    app: AppHandle,
    session: State<'_, Session>,
    ps5: State<'_, Ps5>,
    source: PathBuf,
    options: PackOptions,
) -> Result<JobView, String> {
    let send = if options.send {
        let settings = ps5.settings();
        let address = settings
            .address
            .ok_or("Set up your PS5 in Settings first.")?;
        // The port goes with the address, so a job resumed later reaches the same helper.
        Some(SendTarget {
            console: format!("{address}:{}", settings.helper_port),
            destination: settings.destination,
        })
    } else {
        None
    };
    let sending = send.is_some();
    let options = Options {
        format: options.format,
        level: options.level,
        output_dir: options.output_dir,
        delete_original: options.delete_original,
        delete_packed: options.delete_packed && sending,
        send,
        dlc: if sending { options.dlc } else { vec![] },
        bundle: kite_pipeline::Bundle {
            dlc: options.bundle.dlc,
            backport: options.bundle.backport,
            // The emulator is carried by the app; a dump with its own keeps it.
            emulator: None,
        },
        install: false,
    };
    session.start(source, options, emitter(app))
}

/// Checks a picked package is DLC for the game, and reads its name, size and icon.
#[tauri::command]
async fn check_dlc(path: PathBuf, title_id: String) -> Result<DlcInfo, String> {
    blocking(move || kite_forge::check_dlc(&path, &title_id)).await?
}

/// Every `.pkg` in a folder, and up to two folders down, that is DLC for the game; the rest by name.
#[tauri::command]
async fn dlc_in_folder(path: PathBuf, title_id: String) -> Result<DlcFolder, String> {
    blocking(move || {
        let pkgs =
            packages::packages_in(&path).map_err(|e| format!("Couldn't read the folder: {e}"))?;
        let mut folder = DlcFolder {
            found: vec![],
            skipped: vec![],
        };
        for pkg in pkgs {
            match kite_forge::check_dlc(&pkg, &title_id) {
                Ok(info) => folder.found.push(PickedDlc { path: pkg, info }),
                Err(_) => folder.skipped.push(packages::shown_name(&path, &pkg)),
            }
        }
        Ok(folder)
    })
    .await?
}

/// How far an update's download has come, for the banner.
#[derive(Clone, Serialize)]
struct UpdateProgress {
    done: u64,
    total: u64,
}

/// A newer Kite Pack on GitHub, or None.
#[tauri::command]
async fn check_update() -> Result<Option<kite_update::Update>, String> {
    blocking(|| kite_update::check(env!("CARGO_PKG_VERSION"), kite_update::LATEST)).await?
}

/// Downloads the update, checks it and puts it in place; it runs from the next launch.
#[tauri::command]
async fn install_update(app: AppHandle, update: kite_update::Update) -> Result<(), String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("update");
    blocking(move || {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut shown = u64::MAX;
        let file = kite_update::download(&update, &dir, &cancel, |done, total| {
            // One event per percent is plenty.
            let percent = if total > 0 { done * 100 / total } else { 0 };
            if percent != shown {
                shown = percent;
                let _ = app.emit("kite://update", UpdateProgress { done, total });
            }
        })?;
        let installed = kite_update::install(&file);
        let _ = std::fs::remove_file(&file);
        installed
    })
    .await?
}

/// Starts the new version. Refused while a game packs or sends.
#[tauri::command]
fn restart_app(app: AppHandle) -> Result<(), String> {
    if app.state::<Session>().is_running() {
        return Err("Restart once this game is done".into());
    }
    app.restart();
}

/// ShadowMount+'s version, and what it knows of this game: whether its base is installed as a
/// package (DLC can be installed) or only mounted (DLC cannot).
#[tauri::command]
async fn smp_status(smp: State<'_, Smp>, title_id: Option<String>) -> Result<SmpStatus, String> {
    let installer = smp.installer.clone();
    blocking(move || installer.status(title_id.as_deref())).await?
}

/// Installs a backport folder through ShadowMount+: it is sent to the console, then copied to
/// ShadowMount+'s backports folder, where it is applied at the next launch.
#[tauri::command]
async fn install_backport(smp: State<'_, Smp>, path: PathBuf) -> Result<(), String> {
    let installer = smp.installer.clone();
    blocking(move || {
        installer
            .install_backport(&path, &std::sync::atomic::AtomicBool::new(false))
            .map_err(install_error_message)
    })
    .await?
}

/// An install failure in the screen's words.
fn install_error_message(e: kite_pipeline::InstallError) -> String {
    use kite_pipeline::InstallError::*;
    match e {
        Unreachable(why) => format!("The PS5 isn't reachable: {why}"),
        Refused { code, hint } => format!("{hint} (0x{code:08X})"),
        NotConfirmed => "It was handed over, but never showed up on the PS5".into(),
        Failed(why) => why,
        Cancelled => "Stopped".into(),
    }
}

/// Finished jobs, newest first.
#[tauri::command]
async fn load_history(app: AppHandle) -> Result<Vec<Entry>, String> {
    blocking(move || app.state::<History>().load()).await
}

#[tauri::command]
fn add_history(history: State<'_, History>, entry: Entry) -> Result<(), String> {
    history
        .add(entry)
        .map_err(|e| format!("Couldn't save the history: {e}"))
}

#[tauri::command]
fn clear_history(history: State<'_, History>) -> Result<(), String> {
    history
        .clear()
        .map_err(|e| format!("Couldn't clear the history: {e}"))
}

#[tauri::command]
fn cancel(session: State<'_, Session>) {
    session.cancel();
}

#[tauri::command]
fn carry_on(app: AppHandle, session: State<'_, Session>, id: String) -> Result<JobView, String> {
    session.carry_on(&id, emitter(app))
}

#[tauri::command]
fn discard(session: State<'_, Session>, id: String) -> Result<(), String> {
    session.discard(&id)
}

#[tauri::command]
fn unfinished(session: State<'_, Session>) -> Option<JobView> {
    session.unfinished()
}

/// A game given when the app started: "Open with", or dropped on the app's icon.
#[tauri::command]
fn initial_path() -> Option<String> {
    std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-') && Path::new(a).exists())
}

#[tauri::command]
fn settings(ps5: State<'_, Ps5>) -> Settings {
    ps5.settings()
}

#[tauri::command]
fn save_settings(ps5: State<'_, Ps5>, settings: Settings) -> Result<(), String> {
    settings
        .save(&ps5.settings_path)
        .map_err(|e| format!("Couldn't save the settings: {e}"))?;
    *ps5.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
async fn ps5_status(app: AppHandle) -> Result<Status, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        kite_ps5::status(ps5.console().as_ref(), &ps5.keys)
    })
    .await
}

#[tauri::command]
async fn discover() -> Result<Vec<Found>, String> {
    blocking(|| kite_ps5::discover(DISCOVER_WAIT)).await
}

/// The DLC emulator release Kite Pack carries (it goes into packs whose dump has none).
#[tauri::command]
fn dlc_emu_version() -> &'static str {
    kite_forge::dlc_emu::VERSION
}

/// What the picked DLC and backport would add to a packed copy, and whether they can go in.
#[tauri::command]
async fn bundle_plan(
    app: AppHandle,
    source: PathBuf,
    dlc: Vec<PathBuf>,
    backport: Option<PathBuf>,
) -> Result<kite_forge::BundlePlan, String> {
    blocking(move || {
        let _ = app;
        let bundle = kite_pipeline::Bundle {
            dlc,
            backport,
            emulator: None,
        };
        Ok(kite_forge::bundle::plan(&source, &bundle))
    })
    .await?
}

/// One title on the console by id, for the game screen: `null` when it is not installed.
#[tauri::command]
async fn installed_title(
    app: AppHandle,
    title_id: String,
) -> Result<Option<kite_ps5::InstalledGame>, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let console = ps5.console().ok_or("Set up your PS5 in Settings first.")?;
        kite_ps5::installed_title(&console, &ps5.keys, &title_id).map_err(|e| e.message())
    })
    .await?
}

/// What the PS5 has installed. Reads the console through the helper; changes nothing.
#[tauri::command]
async fn library(app: AppHandle) -> Result<Library, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let console = ps5.console().ok_or("Set up your PS5 in Settings first.")?;
        kite_ps5::installed(&console, &ps5.keys).map_err(|e| e.message())
    })
    .await?
}

/// The tiles of the titles in `ids`, as data: URLs.
#[tauri::command]
async fn library_icons(app: AppHandle, ids: Vec<String>) -> Result<Vec<GameIcon>, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let console = ps5.console().ok_or("Set up your PS5 in Settings first.")?;
        kite_ps5::icons(&console, &ps5.keys, &ids).map_err(|e| e.message())
    })
    .await?
}

/// Where a mounter finds dumps: always /data/homebrew, and wherever games are set to go.
const HOMEBREW: &str = "/data/homebrew";

/// What one installed title takes: its app folder, its update, its add-ons, and the dump the PS5
/// mounts it from (found in the folders games go to).
#[tauri::command]
async fn measure_app(app: AppHandle, title_id: String) -> Result<Measured, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let console = ps5.console().ok_or("Set up your PS5 in Settings first.")?;
        let game_dirs = [ps5.settings().destination, HOMEBREW.to_string()];
        kite_ps5::measure(&console, &ps5.keys, &title_id, &game_dirs).map_err(|e| e.message())
    })
    .await?
}

/// The room left on the volume `path` is on (`/user/app`, or wherever games go).
#[tauri::command]
async fn free_space(app: AppHandle, path: String) -> Result<FreeSpace, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let console = ps5.console().ok_or("Set up your PS5 in Settings first.")?;
        kite_ps5::free_space(&console, &ps5.keys, &path).map_err(|e| e.message())
    })
    .await?
}

#[tauri::command]
async fn start_helper(app: AppHandle) -> Result<StartOutcome, String> {
    blocking(move || {
        let s = app.state::<Ps5>().settings();
        let address = s.address.ok_or("Choose your PS5 first.")?;
        kite_ps5::start_helper(&address, s.loader_port, s.helper_port, HELPER_ELF)
            .map_err(|e| e.message())
    })
    .await?
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            let settings_path = data.join("settings.json");
            let settings = Settings::load(&settings_path);
            let keys = Keys::open(&data.join("ava1"))?;
            let sender = Ps5Sender::new(keys.clone(), settings.helper_port, data.join("outboards"));
            let settings_port = settings.helper_port;
            let settings = Arc::new(Mutex::new(settings));
            let smp = {
                let settings = Arc::clone(&settings);
                let lookup = Arc::new(move || {
                    let s = settings.lock().unwrap().clone();
                    let host = s.address?;
                    Some((host, s.helper_port, s.smp_port))
                });
                Arc::new(SmpInstaller::new(
                    keys.clone(),
                    data.join("outboards"),
                    lookup,
                ))
            };
            let ports = Ports {
                packer: Arc::new(ForgePacker::default()),
                trash: Arc::new(SystemTrash),
                sender: Arc::new(sender),
                // DLC installs go through Sony's installer, streamed from this computer.
                installer: Arc::new(SonyInstaller::new(keys.clone(), settings_port).with_state(data.join("installs"))),
            };
            app.manage(Session::new(data.join("jobs"), ports)?);
            app.manage(Ps5 {
                keys,
                settings_path,
                settings,
            });
            app.manage(Smp { installer: smp });
            app.manage(Quitting::default());
            app.manage(History::new(data));
            // The copy an update replaced, if one did.
            std::thread::spawn(kite_update::clean_up);
            Ok(())
        })
        // Closing the window mid-install stops the PS5's download: ask first.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event
                && window.state::<Session>().running_install()
                && !window.state::<Quitting>().0.load(std::sync::atomic::Ordering::SeqCst)
            {
                api.prevent_close();
                let _ = window.emit(EVENT, UiEvent::CloseAsked);
            }
        })
        .invoke_handler(tauri::generate_handler![
            inspect,
            install,
            quit,
            start,
            cancel,
            carry_on,
            discard,
            unfinished,
            initial_path,
            settings,
            save_settings,
            ps5_status,
            discover,
            start_helper,
            check_dlc,
            dlc_in_folder,
            load_history,
            add_history,
            clear_history,
            dlc_emu_version,
            bundle_plan,
            installed_title,
            library,
            library_icons,
            measure_app,
            free_space,
            check_update,
            install_update,
            restart_app,
            smp_status,
            install_backport
        ])
        .build(tauri::generate_context!())
        .expect("Kite Pack couldn't start");
    app.run(|handle, event| {
        // Closing mid-pack: stop the step cleanly so Forge removes its partial output, keep
        // the job for the next launch, then quit.
        if let RunEvent::ExitRequested { api, .. } = event
            && handle.state::<Session>().is_running()
        {
            api.prevent_exit();
            let handle = handle.clone();
            std::thread::spawn(move || {
                handle.state::<Session>().stop_for_exit(STOP_WAIT);
                handle.exit(0);
            });
        }
    });
}
