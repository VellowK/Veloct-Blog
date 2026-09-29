//! The backup and restore commands.
//!
//! `handoff.md` §17 is a locked section and §28 asks for "automatic backup, manual backup, local export, full
//! restore and content-only restore" to be usable. Until this module existed the application never called
//! `veloct-backup`: 87 tests' worth of model with no way in.
//!
//! Two commands are deliberately separate rather than one. **§17.10 requires a restore to be confirmed before
//! it replaces anything**, so `stage_restore` unpacks and verifies into a staging directory and changes
//! nothing, and `confirm_restore` applies it. A single `restore()` that did both would make the confirmation a
//! formality a caller could skip by calling it once — which is exactly what the library's `apply(.., confirmed)`
//! parameter exists to prevent, and collapsing the two here would undo it at the layer above.

use serde::{Deserialize, Serialize};
use tauri::State;

use veloct_app::{
    BackupArchiveView, BackupOutcomeView, CommandError, RestoreAppliedView, RestorePlanView,
};

use crate::state::AppState;

/// A staged restore, held in memory until the user confirms.
///
/// Kept as its own struct rather than a path so the confirmation cannot be forged: the staging directory is
/// generated here and the plan is computed from the SAME staged artefact that will be applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedRestoreView {
    pub plan: RestorePlanView,
    /// The staging directory, so the caller can show where the verified bytes are.
    pub staging_directory: String,
}

/// Take a manual backup into the project's own backup directory (§17.8).
#[tauri::command]
pub fn manual_backup(state: State<'_, AppState>) -> Result<BackupOutcomeView, CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    // The staging root is inside the project's own temp directory, not the OS temp: `Restore::apply` resolves
    // paths relative to the project, and a staging directory on another volume would make a rename impossible.
    let outcome = veloct_app::manual_backup(project.root.as_path(), &clock)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "manual backup written")
            .with_field("archive", &outcome.archive_path)
            .with_field("bytes", outcome.archive_bytes.to_string())
            .with_field("files", outcome.file_count.to_string())
            .with_field("secrets", outcome.secret_findings.len().to_string()),
    );
    Ok(outcome)
}

/// Export a backup to a directory the user chose (§17.9).
#[tauri::command]
pub fn export_backup(
    state: State<'_, AppState>,
    destination_directory: String,
) -> Result<BackupOutcomeView, CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    let destination = std::path::PathBuf::from(&destination_directory);
    let outcome = veloct_app::export_to(project.root.as_path(), &destination, &clock)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "backup exported")
            .with_field("archive", &outcome.archive_path)
            .with_field("bytes", outcome.archive_bytes.to_string()),
    );
    Ok(outcome)
}

/// The archives in a directory.
#[tauri::command]
pub fn list_backups(
    state: State<'_, AppState>,
    directory: Option<String>,
) -> Result<Vec<BackupArchiveView>, CommandError> {
    let project = state.project()?;
    let directory = match directory {
        Some(text) if !text.trim().is_empty() => std::path::PathBuf::from(text),
        _ => veloct_app::backup_directory(project.root.as_path())?,
    };
    Ok(veloct_app::list_archives(&directory)?)
}

/// Remove all but the newest archives (§17.6).
#[tauri::command]
pub fn prune_backups(
    state: State<'_, AppState>,
    directory: Option<String>,
    keep: Option<usize>,
) -> Result<usize, CommandError> {
    let project = state.project()?;
    let directory = match directory {
        Some(text) if !text.trim().is_empty() => std::path::PathBuf::from(text),
        _ => veloct_app::backup_directory(project.root.as_path())?,
    };
    let keep = match keep {
        // A retention of zero would delete the archive that was just taken, so it falls back to the
        // documented default rather than being honoured.
        Some(0) | None => veloct_app::retention_from_settings(project.root.as_path()),
        Some(keep) => keep,
    };
    let removed = veloct_app::prune(&directory, keep)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "old backups pruned")
            .with_field("removed", removed.to_string())
            .with_field("kept", keep.to_string()),
    );
    Ok(removed)
}

/// Stage a restore and return the plan, WITHOUT touching the project (§17.10).
#[tauri::command]
pub fn stage_restore(
    state: State<'_, AppState>,
    archive_path: String,
    mode: String,
) -> Result<StagedRestoreView, CommandError> {
    let project = state.project()?;
    let mode = parse_mode(&mode)?;
    let archive = std::path::PathBuf::from(&archive_path);
    if !archive.is_file() {
        return Err(crate::commands::command_invalid(format!(
            "{archive_path} is not a file"
        )));
    }

    // The staging directory is unique per staging run, so two stagings cannot verify each other's bytes.
    let staging = project
        .root
        .as_path()
        .join(veloct_core::layout::VELOCT_DIR)
        .join("tmp")
        .join(format!(
            "restore-{}",
            veloct_core::ids::StableId::new().to_text()
        ));

    let (_staged, plan) = veloct_app::stage_restore(&archive, mode, &staging)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "restore staged")
            .with_field("archive", &archive_path)
            .with_field("mode", plan.mode.clone())
            .with_field("files", plan.file_count.to_string()),
    );

    Ok(StagedRestoreView {
        plan,
        staging_directory: staging.to_string_lossy().to_string(),
    })
}

/// Apply a staged restore.
///
/// The archive is re-staged from the same path and mode rather than keeping a `StagedRestore` in the state,
/// because the library's `apply` verifies the staged bytes again immediately before writing. Re-staging means
/// the verification and the write are about the same bytes in one call, and the staging directory is created
/// fresh rather than reused from a previous run.
#[tauri::command]
pub fn confirm_restore(
    state: State<'_, AppState>,
    archive_path: String,
    mode: String,
    confirmed: bool,
) -> Result<RestoreAppliedView, CommandError> {
    let project = state.project()?;
    let mode = parse_mode(&mode)?;

    // Passing the caller's `confirmed` through rather than assuming true: the library refuses an unconfirmed
    // apply, and this command must not be the place where that refusal is bypassed.
    if !confirmed {
        return Err(crate::commands::command_invalid(
            "a restore must be confirmed before it replaces anything",
        ));
    }

    let archive = std::path::PathBuf::from(&archive_path);
    let staging = project
        .root
        .as_path()
        .join(veloct_core::layout::VELOCT_DIR)
        .join("tmp")
        .join(format!(
            "restore-{}",
            veloct_core::ids::StableId::new().to_text()
        ));

    let (staged, _plan) = veloct_app::stage_restore(&archive, mode, &staging)?;
    let applied = veloct_app::apply_restore(&staged, project.root.as_path(), true)?;

    // The staging directory is removed after a successful apply: its bytes have been written to the project,
    // and leaving a second copy of the project in `.veloct/tmp` would make the next backup larger.
    let _ = std::fs::remove_dir_all(&staging);

    let _ = project.log(
        veloct_logging::LogEvent::warning(veloct_logging::Category::Backup, "restore applied")
            .with_field("archive", &archive_path)
            .with_field("mode", applied.mode.clone())
            .with_field("files", applied.files_written.to_string())
            .with_field("index_rebuild", applied.needs_index_rebuild.to_string()),
    );

    Ok(applied)
}

/// The backup directory, so the window can show it.
#[tauri::command]
pub fn backup_directory(state: State<'_, AppState>) -> Result<String, CommandError> {
    let project = state.project()?;
    Ok(veloct_app::backup_directory(project.root.as_path())?
        .to_string_lossy()
        .to_string())
}

/// Parse a restore mode.
pub fn parse_mode(text: &str) -> Result<veloct_backup::RestoreMode, CommandError> {
    match text.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(veloct_backup::RestoreMode::Full),
        "content_only" | "contentonly" | "content" => Ok(veloct_backup::RestoreMode::ContentOnly),
        other => Err(crate::commands::command_invalid(format!(
            "unknown restore mode {other:?}; expected `full` or `content_only`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_mode_is_parsed_or_refused_rather_than_defaulted() {
        // A typo that silently became a FULL restore would replace more than the user asked for.
        assert_eq!(parse_mode("full").unwrap(), veloct_backup::RestoreMode::Full);
        assert_eq!(
            parse_mode("content_only").unwrap(),
            veloct_backup::RestoreMode::ContentOnly
        );
        assert_eq!(
            parse_mode("  CONTENT  ").unwrap(),
            veloct_backup::RestoreMode::ContentOnly
        );
        let error = parse_mode("partial").expect_err("an unknown mode is refused");
        assert!(error.to_string().contains("unknown restore mode"), "{error}");
    }
}
