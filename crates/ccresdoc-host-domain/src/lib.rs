//! Compiles the Tauri host's real settings, appearance and runtime-policy
//! modules so their domain behavior is tested on hosts that cannot build Tauri.
//! `runtime` covers the Tauri-independent policy (port allocation, origin and
//! navigation checks, readiness, supersession); native command and window
//! integration stay in `src-tauri` and its macOS verification.

#[path = "../../../src-tauri/src/appearance.rs"]
pub mod appearance;
#[path = "../../../src-tauri/src/runtime.rs"]
pub mod runtime;
#[path = "../../../src-tauri/src/settings.rs"]
pub mod settings;
