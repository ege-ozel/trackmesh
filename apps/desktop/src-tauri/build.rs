fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_status",
            "get_storage",
            "snapshot_changes",
            "choose_folder",
            "open_project_folder",
            "open_projects_location",
            "choose_projects_location",
            "choose_invite_parent_folder",
            "open_invite_parent_folder",
            "paste_invite_link",
            "cancel_invite_link",
            "stop_operation",
            "open_previous_version",
            "submit_action",
            "set_upnp_mapping",
            "set_language",
            "ask",
            "close_app",
        ]),
    ))
    .expect("TrackMesh desktop build configuration is invalid");
}
