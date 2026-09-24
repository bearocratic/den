//! Replacing ourselves.
//!
//! A menu bar app is opened once and then runs for months, so it has
//! to be able to fetch its own next version. The check is quiet: it
//! says nothing until there is something to say, and nothing is
//! installed until the person presses it.

use crate::state::Model;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

/// How often a long-running app asks whether it is out of date.
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);

/// Ask once, quietly, and remember the answer.
pub async fn look(app: &AppHandle) -> Option<String> {
    let updater = app.updater().ok()?;
    let found = updater.check().await.ok()??;
    let version = found.version.clone();

    let shared = app.state::<Arc<Mutex<Model>>>().inner().clone();
    shared.lock().unwrap().update = Some(version.clone());
    crate::watch::publish(app, &shared);
    crate::watch::log(format!("update available: {version}"));
    Some(version)
}

/// Check at startup, then on a slow timer for as long as we run.
pub fn spawn_checks(app: AppHandle) {
    std::thread::spawn(move || loop {
        let handle = app.clone();
        tauri::async_runtime::block_on(async move {
            look(&handle).await;
        });
        std::thread::sleep(EVERY);
    });
}

/// Fetch it, put it in place, and come back as the new version.
pub async fn install(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
