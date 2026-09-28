//! Veloct desktop shell.
//!
//! This crate is intentionally thin: it owns the Tauri window, the plugin set, the command
//! registry and the process teardown on exit. All behaviour lives in the workspace crates so it
//! can be tested without a WebView (ADR-0002).
//!
//! Ownership: `ui-editor` (team `veloct-complete`). The command surface is filled in by
//! `content-app` as `veloct-app` services land.

use veloct_app::CommandError;
use veloct_core::{Clock, SystemClock};

/// Health/version information the frontend can display on startup.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    /// Version of the bundled Astro site template, used to detect project template upgrades
    /// (ADR-0001).
    pub site_template_version: String,
}

/// Report application identity. Also the simplest possible smoke test that the IPC bridge and
/// the Rust error shape work end to end.
#[tauri::command]
fn app_info() -> Result<AppInfo, CommandError> {
    Ok(AppInfo {
        name: "Veloct".to_string(),
        version: veloct_core::VERSION.to_string(),
        site_template_version: veloct_core::VERSION.to_string(),
    })
}

/// Current time in the storage format, so the frontend never invents its own timestamp format.
#[tauri::command]
fn now_rfc3339() -> String {
    veloct_core::clock::format_rfc3339(SystemClock.now())
}

/// Build and run the desktop application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![app_info, now_rfc3339])
        .setup(|_app| {
            // The Astro preview supervisor is started lazily, when a project is opened, so a
            // machine with no project does not pay for a Node process.
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Veloct application")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // Teardown belongs here, NOT in `on_window_event`, which fires per window and
                // would kill the preview while another window is still open. The supervisor's
                // `stop_all` kills the whole process tree; `CommandChild::kill()` alone would
                // orphan the Vite/esbuild workers.
                //
                // `veloct_process::PreviewSupervisor::stop_all` is wired in here once the
                // process crate lands.
            }
        });
}
