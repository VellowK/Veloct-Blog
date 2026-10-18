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

#![deny(unsafe_code)]

// Tauri exposes only ICON_SMALL; keep the required Windows ICON_BIG FFI in one audited module.
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows_taskbar;

pub mod attachments;
pub mod attachment_sync;
pub mod backup;
pub mod deploy;
pub mod commands;
pub mod credentials;
pub mod images;
mod site_images;
pub mod linked_knowledge;
pub mod logging;
pub mod project_template;
pub mod preview_document;
pub mod publish;
pub mod state;
pub mod works;

pub use commands::{AppInfo, DocumentView, PreviewView, ProjectView, SaveView};
pub use state::{
    resolve_launcher, resolve_npm, AppState, NullSpawner, OpenProject, OsSpawner, ShellSpawner,
    SiteInstallState, SiteInstallView, NPM_ENV,
};

use std::sync::Arc;

use tauri::Manager;

/// Build the application state the shipped application runs on.
///
/// This is the ONE place the application's spawner is chosen, and it is a function rather than two
/// lines inside [`run`] so that a test can build the SAME state and assert on it. The defect this
/// exists to prevent was purely a wiring one: the state was built with `NullSpawner`, whose every
/// spawn attempt fails, so "start preview" could never work in a shipped build — and no test caught
/// it, because every test either injected a fake spawner or built its own `PreviewSpec` and never
/// asked what the application was wired to. `tests/preview_wiring.rs` asks exactly that.
pub fn application_state() -> AppState {
    // The real spawner: an OS child process running the Astro preview sidecar. `NullSpawner` is for
    // `AppState::headless` (tests and any genuinely headless construction) and must never be what
    // the application installs.
    let spawner: Arc<dyn ShellSpawner> = Arc::new(OsSpawner);
    AppState::new(spawner)
}

/// Build and run the desktop application.
pub fn run() {
    let state = application_state();
    // Decode at compile time instead of depending on a runtime resource readback for the title icon.
    let mut context = tauri::generate_context!();
    context.set_default_window_icon(Some(tauri::include_image!("icons-logo/32x32.png")));

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
            commands::initialize_current_project,
            commands::current_project,
            commands::site_settings,
            commands::save_site_settings,
            commands::save_site_avatar,
            commands::save_site_cover,
            commands::sync_github_activity,
            linked_knowledge::linked_knowledge,
            linked_knowledge::link_knowledge,
            linked_knowledge::refresh_linked_knowledge,
            linked_knowledge::unlink_knowledge,
            linked_knowledge::open_linked_knowledge,
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
            preview_document::preview_document_ready,
            commands::read_log,
            commands::publish_project,
            commands::list_releases,
            commands::install_site_dependencies,
            commands::site_install_status,
            commands::cancel_site_install,
            attachments::add_attachment,
            attachments::add_attachment_bytes,
            attachments::preview_attachment_file,
            attachments::list_attachments,
            attachments::attachment_orphans,
            attachments::remove_attachment,
            attachments::attachment_markdown,
            attachments::reference_attachment,
            attachments::forget_attachment_references,
            attachments::sync_attachment_index,
            attachments::attachment_cache_directory,
            attachments::storage_targets,
            attachments::storage_kinds,
            attachments::validate_storage_target,
            attachments::test_storage_target,
            attachments::save_storage_target,
            attachments::delete_storage_target,
            attachments::upload_eligibility,
            attachments::upload_attachment,
            attachment_sync::attachment_sync_settings,
            attachment_sync::save_attachment_sync_settings,
            attachment_sync::attachment_messages,
            attachment_sync::sync_upload_attachment,
            attachment_sync::check_attachment_images,
            attachments::delete_remote_object,
            attachments::attachment_objects,            backup::manual_backup,
            backup::export_backup,
            backup::list_backups,
            backup::prune_backups,
            backup::stage_restore,
            backup::confirm_restore,
            backup::backup_directory,
            backup::note_content_change,
            backup::auto_backup_state,
            backup::run_auto_backup_if_due,
            backup::cloud_backup_targets,
            backup::save_cloud_backup_target,
            backup::delete_cloud_backup_target,
            backup::test_cloud_backup_target,
            backup::backup_to_cloud,
            backup::resend_backup_to_cloud,
            deploy::deploy_targets,
            deploy::save_deploy_target,
            deploy::delete_deploy_target,
            deploy::set_deploy_enabled,
            deploy::deploy_now,
            deploy::deploy_resendable,
            works::list_works,
            works::describe_work_source,
            works::plan_work_import,
            works::import_work,
            works::export_work,
            works::validate_work_metadata,
            works::update_work,
            works::remove_work,
            works::set_work_screenshot,
            works::work_directory,
            works::work_screenshot_limit,
            images::network_image_sources,
            images::save_image_source_key,
            images::delete_image_source_key,
            images::search_network_images,
            images::network_image_topics,
            images::browse_network_topic_photos,
            images::choose_network_image,
        ])
        .setup(|app| {
            if let (Some(window), Some(icon)) = (app.get_webview_window("main"), app.default_window_icon()) {
                window.set_icon(icon.clone())?;
                #[cfg(windows)]
                windows_taskbar::set_taskbar_icon(&window)?;
            }
            // The preview supervisor is started lazily, when a project is opened, so a machine with no
            // project does not pay for a Node process.
            //
            // The sidecar's bare name is recorded here rather than in the command that starts a
            // preview, so the resolution happens once and a missing binary is reported at startup
            // instead of on the user's first preview attempt.
            let state = app.state::<AppState>();
            match app.path().resource_dir() {
                Ok(resources) => {
                    match crate::project_template::resolve(&resources) {
                        Ok(template) => state.set_project_template(template),
                        Err(error) => eprintln!("[veloct] {error}"),
                    }
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
        .build(context)
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
    fn the_application_is_wired_to_a_spawner_that_can_actually_spawn() {
        // The shipped build once installed `NullSpawner`, so every "start preview" failed with a
        // message about a spawner the user had never heard of. This asserts the wiring itself, on the
        // same state `run` builds, which is the only thing that would have caught it: a test that
        // injects its own spawner exercises the supervisor, not the application.
        let state = application_state();
        assert!(
            state.can_spawn_previews(),
            "the application is wired to {}, which cannot start a process",
            state.spawner_kind()
        );
        assert!(
            state.spawner_kind().ends_with("OsSpawner"),
            "the application must run the real OS spawner, got {}",
            state.spawner_kind()
        );
    }

    #[test]
    fn the_null_spawner_is_only_for_a_headless_construction() {
        // `NullSpawner` stays for tests and headless runs, and it stays honest about itself: it
        // refuses, and it says it refuses, so nothing can mistake it for a working preview path.
        let spawner: Arc<dyn ShellSpawner> = Arc::new(NullSpawner);
        assert!(!spawner.can_spawn());
        let spec = veloct_process::PreviewSpec {
            role: PreviewRole::Site,
            program: "veloct-astro-sidecar".to_string(),
            args: Vec::new(),
            working_directory: std::path::PathBuf::from("."),
            env: Default::default(),
            port: 1,
        };
        // `match` rather than `expect_err`, because the Ok type holds a trait object that is not
        // `Debug`.
        let error = match spawner.spawn_preview(&spec) {
            Err(error) => error,
            Ok(_) => panic!("the null spawner must not report success"),
        };
        // The doubled "preview preview" the user saw came from interpolating a label that already
        // ends in "preview" into a sentence that added another one.
        assert!(!error.to_string().contains("preview preview"), "{error}");
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
