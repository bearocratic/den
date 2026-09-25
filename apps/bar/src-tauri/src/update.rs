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
/// How old an answer may be before opening the menu asks again.
const ASK_AGAIN: Duration = Duration::from_secs(30 * 60);

/// Whether the last answer is old enough to be worth asking again.
pub fn stale(app: &AppHandle) -> bool {
    let shared = app.state::<Arc<Mutex<Model>>>().inner().clone();
    let m = shared.lock().unwrap();
    match m.checked_at {
        Some(at) => at.elapsed().map(|d| d > ASK_AGAIN).unwrap_or(true),
        None => true,
    }
}

/// Ask once and remember the answer.
///
/// Quiet on its own timer — it says nothing unless there is something
/// to say. Asked by a person, the answer is worth showing either way,
/// which is what `told` carries.
pub async fn look(app: &AppHandle, told: bool) -> Option<String> {
    let shared = app.state::<Arc<Mutex<Model>>>().inner().clone();
    if told {
        let mut m = shared.lock().unwrap();
        m.checking = true;
        m.check_failed = None;
        drop(m);
        crate::watch::publish(app, &shared);
    }

    // A check that could not be made is not the same as a check that
    // found nothing. Reporting the first as the second is how an app
    // tells someone they are current while quietly failing.
    let outcome = match app.updater() {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };

    let (version, failure) = match outcome {
        Ok(found) => (found.map(|f| f.version.clone()), None),
        Err(why) => (None, Some(why)),
    };

    {
        let mut m = shared.lock().unwrap();
        m.checking = false;
        m.update = version.clone();
        m.check_failed = failure.clone();
        if told {
            m.checked_at = Some(std::time::SystemTime::now());
        }
    }
    crate::watch::publish(app, &shared);
    match (&version, &failure) {
        (Some(v), _) => crate::watch::log(format!("update available: {v}")),
        (None, Some(why)) => crate::watch::log(format!("could not check for updates: {why}")),
        (None, None) => crate::watch::log("no update"),
    }
    version
}

/// Check at startup, then on a slow timer for as long as we run.
pub fn spawn_checks(app: AppHandle) {
    std::thread::spawn(move || loop {
        let handle = app.clone();
        tauri::async_runtime::block_on(async move {
            look(&handle, false).await;
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
