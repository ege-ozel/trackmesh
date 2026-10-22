//! Narrow, main-window-only Tauri entry points.
use crate::{
    documents,
    runtime::{Action, Desktop, DesktopStatus},
};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use uuid::Uuid;

#[tauri::command]
pub async fn get_storage(
    desktop: State<'_, Arc<Desktop>>,
) -> Result<trackmesh_core::StorageSummary, String> {
    desktop
        .storage_summary()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn snapshot_changes(
    desktop: State<'_, Arc<Desktop>>,
    project_id: uuid::Uuid,
    snapshot_id: uuid::Uuid,
) -> Result<trackmesh_core::SnapshotChanges, String> {
    desktop
        .snapshot_changes(project_id, snapshot_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_status(
    app: AppHandle,
    desktop: State<'_, Arc<Desktop>>,
    offset: u32,
) -> Result<DesktopStatus, String> {
    let status = desktop
        .status(offset)
        .await
        .map_err(|error| error.to_string())?;
    desktop
        .try_auto_first_receive(app, &status)
        .await
        .map_err(|error| error.to_string())?;
    desktop
        .try_pairing_approval_status(&status)
        .await
        .map_err(|error| error.to_string())?;
    Ok(status)
}

#[tauri::command]
pub async fn choose_folder(app: AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || documents::choose_folder(&app))
        .await
        .map_err(|_| "Folder picker stopped".to_owned())?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_project_folder(
    desktop: State<'_, Arc<Desktop>>,
    project_id: Uuid,
) -> Result<(), String> {
    desktop
        .open_project_folder(project_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_projects_location(desktop: State<'_, Arc<Desktop>>) -> Result<(), String> {
    desktop
        .open_projects_location()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn choose_projects_location(
    app: AppHandle,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<Option<String>, String> {
    desktop
        .choose_projects_location(app)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn choose_invite_parent_folder(
    app: AppHandle,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<Option<String>, String> {
    let desktop = desktop.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        documents::choose_folder(&app)
            .map_err(|error| error.to_string())?
            .map(|folder| {
                desktop
                    .set_invite_parent_folder(folder.into())
                    .map_err(|error| error.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|_| "Folder picker stopped".to_owned())?
}

#[tauri::command]
pub async fn open_invite_parent_folder(desktop: State<'_, Arc<Desktop>>) -> Result<(), String> {
    desktop
        .open_invite_parent_folder()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn paste_invite_link(
    app: AppHandle,
    desktop: State<'_, Arc<Desktop>>,
) -> Result<(), String> {
    desktop
        .paste_invite_link(&app)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn cancel_invite_link(desktop: State<'_, Arc<Desktop>>) -> Result<(), String> {
    desktop
        .cancel_invite_link()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn stop_operation(
    desktop: State<'_, Arc<Desktop>>,
    project_id: Uuid,
) -> Result<(), String> {
    desktop
        .stop_operation(project_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_previous_version(
    desktop: State<'_, Arc<Desktop>>,
    project_id: Uuid,
    transfer_id: Uuid,
) -> Result<(), String> {
    desktop
        .open_previous_version(project_id, transfer_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn submit_action(
    app: AppHandle,
    desktop: State<'_, Arc<Desktop>>,
    action: Action,
) -> Result<Uuid, String> {
    desktop
        .submit(app, action)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_upnp_mapping(
    desktop: State<'_, Arc<Desktop>>,
    enabled: bool,
) -> Result<(), String> {
    desktop
        .set_upnp_mapping(enabled)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_language(app: AppHandle, desktop: State<'_, Arc<Desktop>>, lang: String) {
    desktop.set_language(&lang);
    // The tray keeps working with its old labels if it can't be relabelled.
    let _ = crate::tray::relabel(&app);
}

/// A native yes/no question for the window, so every confirmation looks the same.
#[tauri::command]
pub async fn ask(
    app: AppHandle,
    title: String,
    message: String,
    ok: String,
    cancel: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .message(message)
            .title(title)
            // Every question confirms something that changes files or access: one warning icon.
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(ok, cancel))
            .blocking_show()
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn close_app(app: AppHandle, desktop: State<'_, Arc<Desktop>>) -> Result<(), String> {
    desktop.request_quit(app);
    Ok(())
}
