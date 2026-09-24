//! The two clocks behind the bar.
//!
//! Local git state is cheap, so it is recomputed when something in a
//! watched folder actually changes and whenever the panel opens.
//! Asking GitHub is not cheap, so CI has its own slow timer and the
//! panel renders the last answer with its age rather than waiting.

use crate::state::Model;
use den_core::{github, repo};
use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::{AppHandle, Emitter, Manager};

pub const SNAPSHOT_EVENT: &str = "den://snapshot";

/// `DEN_BAR_LOG=1` says what the two clocks are doing, on stderr.
pub fn log(line: impl AsRef<str>) {
    if std::env::var_os("DEN_BAR_LOG").is_some() {
        eprintln!("den-bar: {}", line.as_ref());
    }
}

/// A scan of every repository takes long enough that a burst of file
/// changes must not queue one behind another: while one runs, the
/// rest are dropped, and the next is allowed after this.
const QUIET: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Scans {
    busy: AtomicBool,
    last: Mutex<Option<Instant>>,
}

/// Told when the watched folders change, so the watcher can follow.
pub struct Rewatch(pub Sender<Vec<PathBuf>>);

/// Recompute local state, then tell the panel and the glyph.
///
/// `forced` is a person asking (the panel opening, or Refresh); those
/// always run. A scan the file system asked for yields to one already
/// in flight and to the quiet period after it.
pub fn rescan(app: &AppHandle, forced: bool) {
    let scans = app.state::<Arc<Scans>>().inner().clone();

    if !forced {
        if scans.busy.load(Ordering::SeqCst) {
            return;
        }
        let recent = scans
            .last
            .lock()
            .unwrap()
            .map(|t| t.elapsed() < QUIET)
            .unwrap_or(false);
        if recent {
            return;
        }
    }
    if scans.busy.swap(true, Ordering::SeqCst) && !forced {
        return;
    }

    let shared = app.state::<Arc<Mutex<Model>>>().inner().clone();
    shared.lock().unwrap().scanning = true;
    publish(app, &shared);

    // The scan runs outside the lock: holding it would block the panel
    // asking for a snapshot, and leave it reading "scanning…".
    let (cfg, folders) = {
        let m = shared.lock().unwrap();
        (m.cfg.clone(), m.cfg.folders.clone())
    };
    let started = Instant::now();
    let repos = Model::scan_folders(&folders, cfg.depth());
    log(format!(
        "scan {} — {} repos in {}ms",
        if forced { "asked" } else { "on change" },
        repos.len(),
        started.elapsed().as_millis()
    ));

    {
        let mut m = shared.lock().unwrap();
        m.reload_marks();
        m.repos = repos;
        m.scanning = false;
    }
    *scans.last.lock().unwrap() = Some(Instant::now());
    scans.busy.store(false, Ordering::SeqCst);
    publish(app, &shared);
}

pub fn publish(app: &AppHandle, shared: &Arc<Mutex<Model>>) {
    let snapshot = shared.lock().unwrap().snapshot();
    crate::tray::set_badge(app, snapshot.badge);
    let _ = app.emit(SNAPSHOT_EVENT, snapshot);
}

/// Follow the folders as they are added and dropped.
pub fn rewatch(app: &AppHandle) {
    let folders = {
        let shared = app.state::<Arc<Mutex<Model>>>().inner().clone();
        let m = shared.lock().unwrap();
        m.cfg.folders.clone()
    };
    let _ = app.state::<Rewatch>().0.send(folders);
}

/// Watch every folder for changes on disk, debounced and filtered.
pub fn spawn_fs_watcher(app: AppHandle, folders: Vec<PathBuf>) -> Sender<Vec<PathBuf>> {
    let (tx, rx) = channel::<Vec<PathBuf>>();

    std::thread::spawn(move || {
        let handle = app.clone();
        let debouncer = new_debouncer(
            Duration::from_millis(800),
            move |res: DebounceEventResult| {
                let Ok(events) = res else { return };
                // A compiler writing into target/ is not news, and neither
                // is git repacking its objects.
                if events.iter().all(|e| repo::is_noise(&e.path)) {
                    log(format!("ignored {} noisy events", events.len()));
                    return;
                }
                log(format!(
                    "woken by {}",
                    events
                        .iter()
                        .find(|e| !repo::is_noise(&e.path))
                        .map(|e| e.path.display().to_string())
                        .unwrap_or_default()
                ));
                rescan(&handle, false);
            },
        );
        let Ok(mut debouncer) = debouncer else { return };

        let mut watched: Vec<PathBuf> = Vec::new();
        let apply = |debouncer: &mut notify_debouncer_mini::Debouncer<_>,
                     watched: &mut Vec<PathBuf>,
                     next: Vec<PathBuf>| {
            for old in watched.iter() {
                let _ = debouncer.watcher().unwatch(old);
            }
            for folder in &next {
                let _ = debouncer.watcher().watch(folder, RecursiveMode::Recursive);
            }
            log(format!("watching {} folders", next.len()));
            *watched = next;
        };
        apply(&mut debouncer, &mut watched, folders);

        // The debouncer stops watching when it drops, so this thread
        // holds it for as long as the app runs.
        while let Ok(next) = rx.recv() {
            apply(&mut debouncer, &mut watched, next);
        }
    });

    tx
}

/// Follow the folder list itself.
///
/// The list is a file two surfaces write: this app when someone adds
/// a folder, and `den <folder>` in a terminal. A config read once at
/// startup would mean the app quietly disagreed with the file for as
/// long as it ran.
pub fn spawn_config_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let handle = app.clone();
        let debouncer = new_debouncer(
            Duration::from_millis(400),
            move |res: DebounceEventResult| {
                let Ok(events) = res else { return };
                if !events.iter().any(|e| e.path.ends_with("bar.toml")) {
                    return;
                }
                let shared = handle.state::<Arc<Mutex<Model>>>().inner().clone();
                let fresh = den_core::BarConfig::load();
                let changed = {
                    let mut m = shared.lock().unwrap();
                    let changed = m.cfg.folders != fresh.folders;
                    m.cfg = fresh;
                    changed
                };
                if !changed {
                    // Our own save, coming back to us.
                    return;
                }
                log("the folder list changed on disk");
                rewatch(&handle);
                rescan(&handle, true);
            },
        );
        let Ok(mut debouncer) = debouncer else { return };
        let dir = den_core::session::den_dir();
        den_core::session::ensure_dir(&dir);
        if debouncer
            .watcher()
            .watch(&dir, RecursiveMode::NonRecursive)
            .is_err()
        {
            return;
        }
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    });
}

/// Ask GitHub what CI made of each repository, slowly.
pub fn spawn_fetch_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let shared = app.state::<Arc<Mutex<Model>>>().inner().clone();
        shared.lock().unwrap().gh = github::gh_authed();

        loop {
            let (paths, interval, gh) = {
                let m = shared.lock().unwrap();
                (
                    m.repos.iter().map(|r| r.path.clone()).collect::<Vec<_>>(),
                    m.cfg.fetch_secs(),
                    m.gh,
                )
            };

            if gh {
                for path in &paths {
                    if let Some(info) = github::detect_ci(path) {
                        shared.lock().unwrap().ci.insert(path.clone(), info);
                    }
                }
                shared.lock().unwrap().ci_at = Some(SystemTime::now());
                publish(&app, &shared);
            }

            if interval == 0 {
                return;
            }
            std::thread::sleep(Duration::from_secs(interval));
        }
    });
}
