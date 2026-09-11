fn main() {
    // Registering app commands here is what makes the capability split real.
    // Without this, every `#[tauri::command]` is callable from EVERY window —
    // including the one running model-authored code.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "load_active_module",
                "save_module",
                "list_versions",
                "load_version",
                "set_active_version",
                "kv_get",
                "kv_set",
                "declare_schema",
                "schema_status",
                "set_api_key",
                "clear_api_key",
                "api_key_status",
                "chat_history",
                "link_chat_version",
                "propose_change",
                "apply_and_verify",
                "list_templates",
                "apply_template",
                "build_identity",
                "get_theme",
                "set_theme",
                "check_for_update",
                "install_update",
                "get_model",
                "set_model",
                "reset_to_version",
                "list_snapshots",
                "restore_snapshot",
                "probe_canvas",
                "reload_canvas",
                "toggle_drawer",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
