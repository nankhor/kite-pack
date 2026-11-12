// No console window behind the app on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Kite Pack's desktop app: the screen in `app/src`, the session from `kite-session`.

use kite_forge::{ForgePacker, GameInfo, inspect_game};
use kite_pipeline::{Level, Options, OutputFormat, SystemTrash};
use kite_session::{Emit, JobView, Ports, Session, UiEvent};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

const EVENT: &str = "kite://event";
/// How long closing the app waits for a running step to stop cleanly.
const STOP_WAIT: Duration = Duration::from_secs(15);

/// The options the screen sends with Pack.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackOptions {
    format: OutputFormat,
    level: Level,
    output_dir: PathBuf,
    delete_original: bool,
}

fn emitter(app: AppHandle) -> Emit {
    Arc::new(move |event: UiEvent| {
        let _ = app.emit(EVENT, event);
    })
}

/// Reads a dropped or chosen item. Archives can take a while, so off the main thread.
#[tauri::command]
async fn inspect(path: PathBuf) -> Result<GameInfo, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_game(&path).map_err(|p| p.message()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn start(app: AppHandle, session: State<'_, Session>, source: PathBuf, options: PackOptions) -> Result<JobView, String> {
    let options = Options {
        format: options.format,
        level: options.level,
        output_dir: options.output_dir,
        delete_original: options.delete_original,
        send: None,
        delete_packed: false,
        dlc: vec![],
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

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let jobs = app.path().app_data_dir()?.join("jobs");
            let ports = Ports { packer: Arc::new(ForgePacker { full_verify: false }), trash: Arc::new(SystemTrash) };
            app.manage(Session::new(jobs, ports)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![inspect, start, cancel, carry_on, discard, unfinished, initial_path])
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
