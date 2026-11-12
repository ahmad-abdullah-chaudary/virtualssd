//! System tray integration.
//!
//! The tray icon uses the same 4-dot brand mark as the application sidebar and
//! the installed application icon. The tray provides the standard desktop
//! affordances: show/hide the main window, open the Diagnostics view, and a
//! real quit entry that terminates the process.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Identifiers used to dispatch tray menu events back to the application.
const MENU_TOGGLE: &str = "virtualssd-tray-toggle";
const MENU_DIAGNOSTICS: &str = "virtualssd-tray-diagnostics";
const MENU_QUIT: &str = "virtualssd-tray-quit";

/// Emits a frontend event so the shell can switch to the Diagnostics tab.
const SHOW_DIAGNOSTICS_EVENT: &str = "show-diagnostics";

/// Builds and installs the system tray icon.
///
/// The tray is intentionally not created in the browser preview: there is no
/// native shell to host it. Failures are logged rather than silently ignored,
/// because a missing tray icon is a real, user-visible difference.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(
        app,
        MENU_TOGGLE,
        "Show / Hide VirtualSSD",
        true,
        None::<&str>,
    )?;
    let diagnostics = MenuItem::with_id(app, MENU_DIAGNOSTICS, "Diagnostics", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "Quit VirtualSSD", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&toggle, &diagnostics, &quit])?;

    // The bundled 32x32 PNG is the same artwork used for the window, the task
    // bar, and the installers, so the tray matches the rest of the product.
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?;

    TrayIconBuilder::with_id("virtualssd-main-tray")
        .icon(icon)
        .tooltip("VirtualSSD — Cloud-Backed Block Storage")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_TOGGLE => toggle_window(app),
            MENU_DIAGNOSTICS => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
                let _ = app.emit(SHOW_DIAGNOSTICS_EVENT, ());
            }
            MENU_QUIT => {
                tracing::info!(target: "virtualssd_desktop::tray", "Quit requested from the system tray");
                app.exit(0);
            }
            _ => {
                tracing::warn!(
                    target: "virtualssd_desktop::tray",
                    "Unhandled tray menu event: {}",
                    event.id().as_ref()
                );
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left click toggles the main window, matching common Windows
            // behaviour for storage and backup utilities.
            if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;

    tracing::info!(target: "virtualssd_desktop::tray", "System tray icon installed");
    Ok(())
}

/// Shows the main window if it is hidden, hides it if it is visible.
fn toggle_window<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        tracing::warn!(
            target: "virtualssd_desktop::tray",
            "Tray action requested but the main window does not exist"
        );
        return;
    };

    let visible = window.is_visible().unwrap_or(false);
    if visible {
        if let Err(e) = window.hide() {
            tracing::warn!(target: "virtualssd_desktop::tray", "Could not hide the main window: {e}");
            return;
        }
        tracing::info!(target: "virtualssd_desktop::tray", "Main window hidden to the system tray");
    } else {
        if let Err(e) = window.show() {
            tracing::warn!(target: "virtualssd_desktop::tray", "Could not show the main window: {e}");
            return;
        }
        let _ = window.unminimize();
        let _ = window.set_focus();
        tracing::info!(target: "virtualssd_desktop::tray", "Main window restored from the system tray");
    }
}
