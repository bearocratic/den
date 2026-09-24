//! What the panel may ask for.

use crate::state::{Model, Snapshot};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

type Shared = Arc<Mutex<Model>>;

#[tauri::command]
pub fn snapshot(state: State<'_, Shared>) -> Snapshot {
    state.lock().unwrap().snapshot()
}

#[tauri::command]
pub fn rescan(app: AppHandle) {
    std::thread::spawn(move || crate::watch::rescan(&app, true));
}

/// "terminal" | "editor" | "finder" | "github"
#[tauri::command]
pub fn open(path: String, how: String) {
    let path = PathBuf::from(path);
    if !path.exists() {
        return;
    }
    match how.as_str() {
        "terminal" => crate::actions::terminal(&path),
        "editor" => crate::actions::editor(&path),
        "finder" => crate::actions::reveal(&path),
        "github" => {
            crate::actions::github(&path);
        }
        _ => {}
    }
}

#[tauri::command]
pub fn open_url(url: String) {
    crate::actions::url(&url);
}

#[tauri::command]
pub fn add_folder(app: AppHandle) {
    pick_folder(app);
}

#[tauri::command]
pub fn remove_folder(app: AppHandle, path: String) {
    let shared = app.state::<Shared>().inner().clone();
    {
        let mut m = shared.lock().unwrap();
        m.cfg.folders.retain(|f| f.display().to_string() != path);
        m.cfg.save();
    }
    crate::watch::rewatch(&app);
    let handle = app.clone();
    std::thread::spawn(move || crate::watch::rescan(&handle, true));
}

/// Fetch the version the check found and come back as it.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    crate::update::install(app).await
}

/// Quitting from the panel, since nothing tells a person to
/// right-click a glyph.
#[tauri::command]
pub fn quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    if let Some(window) = app.get_webview_window(crate::tray::PANEL) {
        let _ = window.hide();
    }
}

/// The folder picker, shared by the panel's button and the tray menu.
pub fn pick_folder(app: AppHandle) {
    let handle = app.clone();
    app.dialog().file().pick_folder(move |chosen| {
        let Some(folder) = chosen else { return };
        let Ok(path) = folder.into_path() else { return };
        {
            let shared = handle.state::<Shared>().inner().clone();
            let mut m = shared.lock().unwrap();
            if !m.cfg.add_folder(path) {
                return;
            }
            m.cfg.save();
        }
        crate::watch::rewatch(&handle);
        let h = handle.clone();
        std::thread::spawn(move || crate::watch::rescan(&h, true));
    });
}
