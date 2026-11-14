// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Kite Pack's desktop app: the screen in `app/src`, the session from `kite-session`, the PS5
//! side from `kite-ps5`.

use kite_forge::{ForgePacker, GameInfo, inspect_game};
use kite_pipeline::{Level, Options, OutputFormat, SendTarget, SystemTrash};
use kite_ps5::{
    Console, Found, FreeSpace, GameIcon, Keys, Library, Measured, Ps5Sender, Settings, StartOutcome, Status,
};
use kite_session::{Emit, JobView, Ports, Session, UiEvent};
use serde::Deserialize;
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

struct Ps5 {
    keys: Keys,
    settings_path: PathBuf,
    settings: Mutex<Settings>,
}

impl Ps5 {
    fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    fn console(&self) -> Option<Console> {
        let s = self.settings();
        s.address.map(|address| Console { address, port: s.helper_port })
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
    /// DLC and a backport to put inside the packed copy.
    #[serde(default)]
    bundle: BundleOptions,
}

/// What the screen picked to go inside the pack; the emulator comes from Settings.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct BundleOptions {
    dlc: Vec<PathBuf>,
    backport: Option<PathBuf>,
}

fn emitter(app: AppHandle) -> Emit {
    Arc::new(move |event: UiEvent| {
        let _ = app.emit(EVENT, event);
    })
}

/// Runs blocking work (disk, network) off the main thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())
}

/// Reads a dropped or chosen item. Archives can take a while, so off the main thread.
#[tauri::command]
async fn inspect(path: PathBuf) -> Result<GameInfo, String> {
    blocking(move || inspect_game(&path).map_err(|p| p.message())).await?
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
        let address = settings.address.ok_or("Set up your PS5 in Settings first.")?;
        // The port goes with the address, so a job resumed later reaches the same helper.
        Some(SendTarget { console: format!("{address}:{}", settings.helper_port), destination: settings.destination })
    } else {
        None
    };
    let bundle = kite_pipeline::Bundle {
        dlc: options.bundle.dlc,
        backport: options.bundle.backport,
        emulator: ps5.settings().dlc_tools.clone(),
    };
    let options = Options {
        format: options.format,
        level: options.level,
        output_dir: options.output_dir,
        delete_original: options.delete_original,
        delete_packed: options.delete_packed && send.is_some(),
        send,
        dlc: vec![],
        bundle,
    };
    session.start(source, options, emitter(app))
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
    std::env::args().skip(1).find(|a| !a.starts_with('-') && Path::new(a).exists())
}

#[tauri::command]
fn settings(ps5: State<'_, Ps5>) -> Settings {
    ps5.settings()
}

#[tauri::command]
fn save_settings(ps5: State<'_, Ps5>, settings: Settings) -> Result<(), String> {
    settings.save(&ps5.settings_path).map_err(|e| format!("Couldn't save the settings: {e}"))?;
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

/// What the picked DLC and backport would add to a packed copy, and whether they can go in.
#[tauri::command]
async fn bundle_plan(
    app: AppHandle,
    source: PathBuf,
    dlc: Vec<PathBuf>,
    backport: Option<PathBuf>,
) -> Result<kite_forge::BundlePlan, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let bundle = kite_pipeline::Bundle { dlc, backport, emulator: ps5.settings().dlc_tools };
        Ok(kite_forge::bundle::plan(&source, &bundle))
    })
    .await?
}

/// One title on the console by id, for the game screen: `null` when it is not installed.
#[tauri::command]
async fn installed_title(app: AppHandle, title_id: String) -> Result<Option<kite_ps5::InstalledGame>, String> {
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

/// What one installed title takes: its app folder, its update and its add-ons.
#[tauri::command]
async fn measure_app(app: AppHandle, title_id: String) -> Result<Measured, String> {
    blocking(move || {
        let ps5 = app.state::<Ps5>();
        let console = ps5.console().ok_or("Set up your PS5 in Settings first.")?;
        kite_ps5::measure(&console, &ps5.keys, &title_id).map_err(|e| e.message())
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
        kite_ps5::start_helper(&address, s.loader_port, s.helper_port, HELPER_ELF).map_err(|e| e.message())
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
            let ports = Ports {
                packer: Arc::new(ForgePacker { full_verify: false }),
                trash: Arc::new(SystemTrash),
                sender: Arc::new(sender),
            };
            app.manage(Session::new(data.join("jobs"), ports)?);
            app.manage(Ps5 { keys, settings_path, settings: Mutex::new(settings) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            inspect,
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
            library,
            installed_title,
            bundle_plan,
            library_icons,
            measure_app,
            free_space
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
