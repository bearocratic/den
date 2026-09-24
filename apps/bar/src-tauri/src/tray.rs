//! The menu bar item: one glyph, and the panel it opens.

use crate::state::Badge;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition};

pub const TRAY_ID: &str = "den";
pub const PANEL: &str = "panel";

const CLEAN: &[u8] = include_bytes!("../icons/tray-clean.png");
const DIRTY: &[u8] = include_bytes!("../icons/tray-dirty.png");
const FAILING: &[u8] = include_bytes!("../icons/tray-failing.png");

/// The artwork for a state, and whether macOS may tint it.
///
/// Everything else in a menu bar is a template image: black with an
/// alpha channel, which the system inverts for a light bar and dims
/// when the bar is inactive. A fixed colour ignores all of that and
/// sits there as the only loud thing on the strip — so den is a
/// template too, and spends colour on the one state worth
/// interrupting for.
fn badge_image(badge: Badge) -> Option<(Image<'static>, bool)> {
    let (bytes, template) = match badge {
        Badge::Clean => (CLEAN, true),
        Badge::Dirty => (DIRTY, true),
        Badge::Failing => (FAILING, false),
    };
    Image::from_bytes(bytes).ok().map(|image| (image, template))
}

/// The glyph says one thing, and it is always the worst thing.
pub fn set_badge(app: &AppHandle, badge: Badge) {
    if let (Some(tray), Some((image, template))) = (app.tray_by_id(TRAY_ID), badge_image(badge)) {
        let _ = tray.set_icon(Some(image));
        let _ = tray.set_icon_as_template(template);
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let check = MenuItem::with_id(app, "check", "Check for updates…", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Add folder…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit den", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&refresh, &check, &add, &sep, &quit])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("den")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "refresh" => {
                let handle = app.clone();
                std::thread::spawn(move || crate::watch::rescan(&handle, true));
            }
            "check" => {
                // The answer lands in the panel, which is where an
                // update would be pressed anyway.
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    crate::update::look(&handle, true).await;
                });
            }
            "add" => crate::commands::pick_folder(app.clone()),
            "quit" => crate::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                let app = tray.app_handle().clone();
                let Some(window) = app.get_webview_window(PANEL) else {
                    return;
                };
                let scale = window.scale_factor().unwrap_or(1.0);
                let at = rect.position.to_physical::<f64>(scale);
                let size = rect.size.to_physical::<f64>(scale);
                toggle_panel(&app, at.x, at.y, size.width, size.height);
            }
        });

    if let Some((image, template)) = badge_image(Badge::Clean) {
        builder = builder.icon(image).icon_as_template(template);
    }
    builder.build(app)?;
    Ok(())
}

/// Open the panel under the glyph, or put it away if it is already there.
fn toggle_panel(app: &AppHandle, icon_x: f64, icon_y: f64, icon_w: f64, icon_h: f64) {
    let Some(window) = app.get_webview_window(PANEL) else {
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }

    if let Ok(size) = window.outer_size() {
        let mut x = icon_x + icon_w / 2.0 - size.width as f64 / 2.0;
        let y = icon_y + icon_h + 6.0;

        // Keep the whole panel on the screen it was summoned from.
        if let Ok(Some(monitor)) = window.current_monitor() {
            let area = monitor.size();
            let origin = monitor.position();
            let min = origin.x as f64 + 8.0;
            let max = origin.x as f64 + area.width as f64 - size.width as f64 - 8.0;
            x = x.clamp(min, max.max(min));
        }
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }

    let _ = window.show();
    let _ = window.set_focus();
    let handle = app.clone();
    std::thread::spawn(move || crate::watch::rescan(&handle, true));

    // Opening the menu after a while is a fair moment to ask again:
    // the six-hour timer is silent, and a stale "up to date" is worse
    // than no answer.
    let asking = app.clone();
    tauri::async_runtime::spawn(async move {
        if crate::update::stale(&asking) {
            crate::update::look(&asking, true).await;
        }
    });
}
