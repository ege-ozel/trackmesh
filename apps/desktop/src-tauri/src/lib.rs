//! Windows desktop adapter. Core owns project decisions; this adapter owns the window session.
#[cfg(not(target_os = "windows"))]
compile_error!("TrackMesh desktop MVP supports Windows only");
mod clipboard;
mod commands;
mod documents;
mod receive;
mod runtime;
mod tray;
mod uri;

use clap::Parser;
use runtime::{Desktop, Options};
use std::sync::Arc;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

pub fn run() {
    let startup_invite = std::env::args().find(|arg| uri::is_invite_link(arg));
    let options = Options::parse_from(std::env::args().filter(|arg| !uri::is_invite_link(arg)));
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "trackmesh_core=info,trackmesh_transfer=info,trackmesh_network=info".into()
            }),
        )
        .init();
    let desktop = Arc::new(Desktop::new(options));
    let setup_desktop = desktop.clone();
    let application = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if let Some(link) = args.iter().find(|arg| uri::is_invite_link(arg)) {
                app.state::<Arc<Desktop>>()
                    .handle_invite_link(app.clone(), link.clone());
            } else if !args.iter().any(|arg| arg == "--plugin-owner") {
                app.state::<Arc<Desktop>>().reopen(app);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(desktop.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_storage,
            commands::snapshot_changes,
            commands::choose_folder,
            commands::open_project_folder,
            commands::open_projects_location,
            commands::choose_projects_location,
            commands::choose_invite_parent_folder,
            commands::open_invite_parent_folder,
            commands::paste_invite_link,
            commands::cancel_invite_link,
            commands::stop_operation,
            commands::open_previous_version,
            commands::submit_action,
            commands::set_upnp_mapping,
            commands::set_language,
            commands::ask,
            commands::close_app
        ])
        .setup(move |app| {
            uri::register_protocol()?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("TrackMesh")
                .inner_size(1180.0, 820.0)
                .min_inner_size(820.0, 620.0)
                .on_new_window(|_, _| tauri::webview::NewWindowResponse::Deny)
                .on_navigation(|url| {
                    (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
                        || (matches!(url.scheme(), "http" | "https")
                            && url.host_str() == Some("tauri.localhost"))
                })
                .build()?;
            tray::install(app.handle())?;
            setup_desktop.start(app.handle().clone());
            if let Some(link) = startup_invite.clone() {
                setup_desktop.handle_invite_link(app.handle().clone(), link);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let desktop = window.state::<Arc<Desktop>>();
                if !desktop.is_closed() {
                    api.prevent_close();
                    desktop.hide_to_tray(window.app_handle().clone());
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("could not start TrackMesh desktop");
    application.run(move |app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !desktop.is_closed() {
                api.prevent_exit();
                desktop.request_quit(app.clone());
            }
        }
    });
}
