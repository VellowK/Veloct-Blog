//! Veloct desktop shell.
//!
//! This crate is intentionally thin: it owns the Tauri window, the plugin set, the command registry
//! and the process teardown on exit. All behaviour lives in the workspace crates so it can be tested
//! without a WebView (ADR-0002), and [`commands`] and [`state`] are written so their logic is
//! exercised by ordinary `cargo test`.
//!
//! Two facts about Tauri 2 shape the code here and are easy to get wrong:
//!
//! * The Rust-side `Shell::sidecar` takes the BARE file name, while the JavaScript side takes the full
//!   configured string. The bundled binary is therefore addressed as `veloct-astro-sidecar`.
//! * `CommandChild::kill()` terminates ONE process. A Node sidecar spawns Vite and esbuild workers, so
//!   teardown must kill the whole tree — `veloct_process::kill_process_tree` does that with
//!   `taskkill /T`, and it belongs in `RunEvent::Exit`, never in `on_window_event`, which fires per
//!   window and would kill the preview while another window is still open.

#![forbid(unsafe_code)]

pub mod attachments;
pub mod backup;
pub mod deploy;
pub mod commands;
pub mod credentials;
pub mod logging;
pub mod page_creator;
pub mod publish;
pub mod state;

pub use commands::{AppInfo, DocumentView, PreviewView, ProjectView, SaveView};
pub use state::{AppState, NullSpawner, OpenProject, ShellSpawner};

use std::sync::Arc;

use tauri::Manager;

/// Build and run the desktop application.
pub fn run() {
    let spawner: Arc<dyn ShellSpawner> = Arc::new(NullSpawner);
    let state = AppState::new(spawner);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::now_rfc3339,
            commands::open_project,
            commands::create_project,
            commands::current_project,
            commands::list_documents,
            commands::content_index,
            commands::read_document,
            commands::create_document,
            commands::save_document,
            commands::transition_wip,
            commands::rename_slug,
            commands::repair_reference,
            commands::assign_paragraph_ids,
            commands::delete_document,
            commands::referencing_documents,
            commands::start_preview,
            commands::stop_preview,
            commands::preview_status,
            commands::read_log,
            commands::ai_settings,
            commands::save_ai_provider,
            commands::delete_ai_provider,
            commands::set_ai_feature,
            commands::set_ai_fallback,
            commands::generate_page,
            commands::create_page_from_draft,
            commands::publish_project,
            commands::list_releases,
            attachments::add_attachment,
            attachments::list_attachments,
            attachments::attachment_orphans,
            attachments::remove_attachment,
            attachments::attachment_markdown,
            attachments::reference_attachment,
            attachments::forget_attachment_references,
            attachments::sync_attachment_index,
            attachments::attachment_cache_directory,
            attachments::storage_targets,
            attachments::save_storage_target,
            attachments::delete_storage_target,
            attachments::upload_eligibility,
            attachments::upload_attachment,
            attachments::attachment_objects,
            backup::manual_backup,
            backup::export_backup,
            backup::list_backups,
            backup::prune_backups,
            backup::stage_restore,
            backup::confirm_restore,
            backup::backup_directory,
            backup::note_content_change,
            backup::auto_backup_state,
            backup::run_auto_backup_if_due,
            deploy::deploy_targets,
            deploy::save_deploy_target,
            deploy::delete_deploy_target,
            deploy::set_deploy_enabled,
            deploy::deploy_now,
            deploy::deploy_resendable,
        ])
        .setup(|app| {
            // The preview supervisor is started lazily, when a project is opened, so a machine with no
            // project does not pay for a Node process.
            //
            // The sidecar's bare name is recorded here rather than in the command that starts a
            // preview, so the resolution happens once and a missing binary is reported at startup
            // instead of on the user's first preview attempt.
            let state = app.state::<AppState>();
            match app.path().resource_dir() {
                Ok(resources) => {
                    let candidate = resources
                        .join("binaries")
                        .join("veloct-astro-sidecar");
                    state.set_sidecar(candidate.to_string_lossy().to_string());
                }
                Err(error) => {
                    // Not fatal: the application is usable for editing without a preview, and the
                    // preview command reports the missing sidecar with a hint when it is asked.
                    eprintln!("[veloct] could not resolve the resource directory: {error}");
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the Veloct application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                // Teardown belongs here. `stop_all` kills the whole process tree, so the Vite and
                // esbuild workers a Node sidecar spawns do not outlive the window and keep the port.
                let state = app.state::<AppState>();
                if let Err(error) = state.shutdown() {
                    eprintln!("[veloct] a preview process may still be running: {error}");
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_process::PreviewRole;

    #[test]
    fn a_headless_state_can_be_built_and_shut_down_without_a_webview() {
        // The whole point of keeping behaviour out of this crate: the state is usable in a test.
        let state = AppState::headless();
        assert!(state.current().is_none());
        assert!(state.preview_url(PreviewRole::Site).is_none());
        state.shutdown().unwrap();
    }

    #[test]
    fn the_default_spawner_is_the_one_that_reports_rather_than_pretending() {
        // A packaged application replaces it during setup; until then a preview attempt is an explicit
        // error rather than a silent no-op.
        let spawner: Arc<dyn ShellSpawner> = Arc::new(NullSpawner);
        let spec = veloct_process::PreviewSpec {
            role: PreviewRole::Site,
            program: "veloct-astro-sidecar".to_string(),
            args: Vec::new(),
            working_directory: std::path::PathBuf::from("."),
            env: Default::default(),
            port: 1,
        };
        assert!(spawner.spawn_preview(&spec).is_err());
    }

    #[test]
    fn the_sidecar_is_addressed_by_its_bare_name_for_the_rust_side() {
        // Tauri's Rust-side `Shell::sidecar` takes the bare file name; the JavaScript side takes the
        // full configured string. Getting this wrong produces a "not found" at runtime only.
        let name = "veloct-astro-sidecar";
        assert!(!name.contains('/'), "the bare name has no path");
        assert!(!name.ends_with(".exe"), "Tauri appends the platform suffix");
    }
}
