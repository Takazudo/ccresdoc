fn main() {
    const COMMANDS: &[&str] = &[
        "get_browser_bootstrap",
        "update_browser_navigation_state",
        "set_shortcut_capture_active",
        "open_current_page_in_default_browser",
        "reload_documentation",
        "retry_launch",
        "open_settings_window",
        "get_settings_snapshot",
        "update_appearance",
        "preview_appearance",
        "clear_appearance_preview",
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
