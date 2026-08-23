fn main() {
    const COMMANDS: &[&str] = &[
        "retry_launch",
        "open_settings_window",
        "get_settings_snapshot",
        "validate_settings_draft",
        "save_and_apply_settings",
        "rebase_stale_settings",
        "replace_malformed_settings",
        "pick_source_directory",
        "open_config_file",
        "reveal_config_file",
    ];
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS));
    tauri_build::try_build(attributes).expect("failed to build Tauri application manifest")
}
