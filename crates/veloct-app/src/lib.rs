//! Application orchestration: services, DTOs and the Tauri command surface.
//!
//! This is the only layer allowed to know about all the domain crates, and the only place that
//! converts domain errors into a shape the frontend can display. It deliberately does NOT
//! depend on `tauri` itself for the service layer — the `#[tauri::command]` wrappers live
//! behind the `tauri` feature so the services stay unit-testable without a WebView.
//!
//! Project access is serialised by `veloct-core`'s [`veloct_core::ProjectLock`]:
//! Publish, Restore and historical rebuilds take an exclusive lock, while editing, preview and
//! attachment uploads take shared locks (`handoff.md` §20.4, §25.1 D4/D5).
//!
//! Ownership: `content-app` (team `veloct-complete`).

#![forbid(unsafe_code)]

pub mod attachments;
pub mod autobackup;
pub mod backup;
pub mod cloud_backup;
pub mod deploy;
pub mod error;
pub mod project;
pub mod works;

pub use attachments::{
    attachment_ids, mime_type_for, AddOutcome, AttachmentService, AttachmentView, ObjectView,
    RemoteDeleteView, UploadEligibility, UploadView, MAX_ATTACHMENT_BYTES,
};
pub use backup::{
    apply_restore, backup, backup_directory, describe_scan, export_to, history_note, list_archives,
    manual_backup, prune, read_manifest, retention_from_settings, stage_restore, BackupArchiveView,
    BackupOutcomeView, RestoreAppliedView, RestorePlanView, SkippedPathView, BACKUP_DIR,
    DEFAULT_RETENTION,
};
pub use deploy::{
    deploy, deploy_with_client, dist_decision, dist_of, is_manual, kind_label, parse_kind,
    read_payloads, resendable_sub_targets, status_name, CredentialLookup, DeployOutcomeView,
    StoredTarget, TargetListView, TargetList, TargetResultView, TargetView, TARGETS_FILE,
};
pub use autobackup::{
    cloud_retention_from_settings, commit_sha_of, idle_seconds_from_settings,
    manifest_needs_acknowledgement, target_upload_outcome, upload_to_targets, AutoBackupOutcome,
    IdleDecision, IdleTracker, TargetUploadOutcome, DEFAULT_CLOUD_RETENTION, DEFAULT_IDLE_SECONDS,
};
pub use cloud_backup::{
    credential_handle, delete_target, handles_for, parse_cloud_kind, read_targets, record_run,
    run_against_targets, save_target, ArchiveRef, CloudBackupRunView, CloudBackupTargetView,
    ScanNote, CLOUD_TARGET_TABLE,
};
pub use error::CommandError;
pub use project::{CreateProjectOptions, ProjectRecord, ProjectService};
pub use works::{
    export, find as find_work, import as import_work, list_works, plan_import as plan_work_import,
    remove as remove_work, set_screenshot as set_work_screenshot,
    update_metadata as update_work_metadata, work_directory, WorkImportOutcomeView,
    WorkImportPlanView, WorkImportWarningView, WorkView, WorksListView, MAX_SCREENSHOT_BYTES,
    SCREENSHOT_RELATIVE_PATH, SUPPORTED_IMAGE_FORMATS,
};
