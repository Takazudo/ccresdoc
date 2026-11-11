//! Compiles the Tauri host's real settings and appearance modules so their
//! domain behavior is tested on hosts that cannot build Tauri. Native command
//! and window integration stay in `src-tauri` and its macOS verification.

#[path = "../../../src-tauri/src/appearance.rs"]
pub mod appearance;
#[path = "../../../src-tauri/src/settings.rs"]
pub mod settings;
