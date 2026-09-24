// den in the menu bar.
//
// An agent app: no Dock icon and no window of its own until the glyph
// is clicked. Everything it knows comes from den-core, so the panel
// and `den` in a terminal describe the same repositories the same way.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod commands;
mod state;
mod tray;
mod update;
mod watch;

use state::Model;
use std::sync::{Arc, Mutex};
use tauri::utils::config::WindowEffectsConfig;
use tauri::window::{Effect, EffectState};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

// A menu, not a window: sized like one, and rounded like one.
const PANEL_W: f64 = 360.0;
const PANEL_H: f64 = 480.0;
const PANEL_RADIUS: f64 = 10.0;

/// Leave for good.
///
/// The run loop refuses ExitRequested so that closing the panel does
/// not end the app — and that refusal swallows app.exit() as well, so
/// asking to quit has to go around it, after Tauri has had its chance
/// to tidy up.
pub fn quit(app: &tauri::AppHandle) {
    app.cleanup_before_exit();
    std::process::exit(0);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::rescan,
            commands::open,
            commands::open_url,
            commands::add_folder,
            commands::remove_folder,
            commands::hide_panel,
            commands::quit,
            commands::check_updates,
            commands::install_update,
        ])
        .setup(|app| {
            // No Dock icon. Info.plist asks for that with LSUIElement
            // and the bundle carries it, but Tauri sets the activation
            // policy to Regular as it starts and that wins — so it has
            // to be said here, where it also covers a plain cargo run.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let model = Model::new(app.package_info().version.to_string());
            let folders = model.cfg.folders.clone();
            app.manage(Arc::new(Mutex::new(model)));
            app.manage(Arc::new(watch::Scans::default()));

            // The panel exists from the start but stays hidden: making
            // it on first click would show an empty frame while the
            // webview boots.
            let panel =
                WebviewWindowBuilder::new(app, tray::PANEL, WebviewUrl::App("index.html".into()))
                    .title("den")
                    .inner_size(PANEL_W, PANEL_H)
                    .resizable(false)
                    .decorations(false)
                    .transparent(true)
                    .shadow(true)
                    // The system's own menu material, so the panel sits
                    // in the menu bar's world instead of painting its own.
                    .effects(WindowEffectsConfig {
                        effects: vec![Effect::Menu],
                        state: Some(EffectState::Active),
                        radius: Some(PANEL_RADIUS),
                        color: None,
                    })
                    .always_on_top(true)
                    .skip_taskbar(true)
                    .visible(false)
                    .build()?;

            // Clicking anywhere else puts it away, the way a menu behaves.
            let handle = panel.clone();
            panel.on_window_event(move |event| {
                if let WindowEvent::Focused(false) = event {
                    let _ = handle.hide();
                }
            });

            tray::build(app.handle())?;
            let rewatch = watch::spawn_fs_watcher(app.handle().clone(), folders);
            app.manage(watch::Rewatch(rewatch));
            watch::spawn_fetch_loop(app.handle().clone());
            update::spawn_checks(app.handle().clone());

            let first = app.handle().clone();
            std::thread::spawn(move || watch::rescan(&first, true));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("den failed to start")
        .run(|_app, event| {
            // Closing the panel is not quitting: the glyph stays.
            // Quit den goes through quit() above, which does not ask.
            // Quit den goes through quit() above, which does not ask.
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
