//! The native tray is owned by the existing desktop process and network session.
use crate::runtime::{tr, Desktop};
use anyhow::{Context, Result};
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};

fn menu(app: &AppHandle) -> Result<Menu<Wry>> {
    let open = MenuItem::with_id(
        app,
        "trackmesh-open",
        tr("Open TrackMesh", "TrackMesh'i aç"),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        "trackmesh-quit",
        tr("Quit TrackMesh", "TrackMesh'ten çık"),
        true,
        None::<&str>,
    )?;
    Ok(Menu::with_items(app, &[&open, &quit])?)
}

/// Rebuilds the menu after the window changes language.
pub fn relabel(app: &AppHandle) -> Result<()> {
    let tray = app
        .tray_by_id("trackmesh-tray")
        .context("TrackMesh tray icon is unavailable")?;
    tray.set_menu(Some(menu(app)?))?;
    tray.set_tooltip(Some(tr("TrackMesh — running", "TrackMesh — çalışıyor")))?;
    Ok(())
}

pub fn install(app: &AppHandle) -> Result<()> {
    let menu = menu(app)?;
    let icon = app
        .default_window_icon()
        .cloned()
        .context("TrackMesh tray icon is unavailable")?;
    TrayIconBuilder::with_id("trackmesh-tray")
        .icon(icon)
        .tooltip(tr("TrackMesh — running", "TrackMesh — çalışıyor"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let desktop = app.state::<Arc<Desktop>>();
            match event.id.as_ref() {
                "trackmesh-open" => desktop.reopen(app),
                "trackmesh-quit" => desktop.request_quit(app.clone()),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let app = tray.app_handle();
                app.state::<Arc<Desktop>>().reopen(app);
            }
        })
        .build(app)?;
    Ok(())
}
