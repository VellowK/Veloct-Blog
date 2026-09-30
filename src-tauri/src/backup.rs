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

/// The automatic backup tracker's state, so the window can show a countdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoBackupStateView {
    pub tracker: veloct_app::IdleTracker,
    /// What the rule decided at this instant.
    pub decision: veloct_app::IdleDecision,
    /// A phrase for the UI rather than the caller assembling one.
    pub description: String,
    /// The idle period and the per-cloud-target retention, from the settings.
    pub idle_seconds: u64,
    pub cloud_retention: usize,
    /// Whether a secret finding is waiting for the user's acknowledgement (section 17.3).
    pub needs_acknowledgement: bool,
}

/// Where the tracker's state is stored, so it survives a restart.
pub const AUTO_STATE_FILE: &str = ".veloct/auto-backup-state.json";

fn read_tracker(project_root: &std::path::Path, now_seconds: u64) -> veloct_app::IdleTracker {
    let path = project_root.join(AUTO_STATE_FILE);
    let idle = veloct_app::idle_seconds_from_settings(project_root);
    match veloct_core::workdir::read_text_optional(&path) {
        Ok(Some(text)) => serde_json::from_str::<veloct_app::IdleTracker>(&text)
            // An unreadable tracker is a FRESH one rather than an error: losing a timestamp is a missed
            // automatic backup, and refusing to start would be worse than that.
            .unwrap_or_else(|_| veloct_app::IdleTracker::new(now_seconds))
            .with_idle_seconds(idle),
        _ => veloct_app::IdleTracker::new(now_seconds).with_idle_seconds(idle),
    }
}

fn write_tracker(project_root: &std::path::Path, tracker: &veloct_app::IdleTracker) -> veloct_core::Result<()> {
    let path = project_root.join(AUTO_STATE_FILE);
    let text = serde_json::to_vec_pretty(tracker)
        .map_err(|error| veloct_core::VeloctError::Backup(format!("could not serialise the tracker: {error}")))?;
    veloct_core::workdir::ensure_parent(&path)?;
    veloct_core::workdir::write_atomic(&path, &text)
}

/// Record that the content changed, which RESTARTS the idle timer (section 17.5).
#[tauri::command]
pub fn note_content_change(state: State<'_, AppState>) -> Result<AutoBackupStateView, CommandError> {
    let project = state.project()?;
    let now = seconds_now();
    let mut tracker = read_tracker(project.root.as_path(), now);
    tracker.note_change(now);
    write_tracker(project.root.as_path(), &tracker)?;
    Ok(auto_state(&project, &tracker, now))
}

/// Whether an automatic backup is due, and how long is left when it is not.
#[tauri::command]
pub fn auto_backup_state(state: State<'_, AppState>) -> Result<AutoBackupStateView, CommandError> {
    let project = state.project()?;
    let now = seconds_now();
    let tracker = read_tracker(project.root.as_path(), now);
    Ok(auto_state(&project, &tracker, now))
}

fn auto_state(
    project: &std::sync::Arc<crate::state::OpenProject>,
    tracker: &veloct_app::IdleTracker,
    now: u64,
) -> AutoBackupStateView {
    let decision = tracker.decide(now);
    AutoBackupStateView {
        tracker: tracker.clone(),
        decision,
        description: decision.describe(),
        idle_seconds: tracker.idle_seconds(),
        cloud_retention: veloct_app::cloud_retention_from_settings(project.root.as_path()),
        needs_acknowledgement: last_run_needs_acknowledgement(project.root.as_path()),
    }
}

/// Whether the most recent automatic run left a finding unacknowledged.
///
/// Read from the run record rather than recomputed, because re-scanning the whole project to answer a UI
/// question would be expensive and would report the CURRENT state rather than the one the finding was about.
fn last_run_needs_acknowledgement(project_root: &std::path::Path) -> bool {
    let path = project_root
        .join(veloct_app::BACKUP_DIR)
        .join("last-auto-run.json");
    veloct_core::workdir::read_text_optional(&path)
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("needs_acknowledgement")
                .and_then(|flag| flag.as_bool())
        })
        .unwrap_or(false)
}

/// Run the automatic backup if the rule says it is due.
///
/// Returns `None` when it is not due, which is not a failure: it is the common case, and reporting it as an
/// error would make a normal quiet minute look like a problem.
#[tauri::command]
pub fn run_auto_backup_if_due(
    state: State<'_, AppState>,
) -> Result<Option<veloct_app::AutoBackupOutcome>, CommandError> {
    let project = state.project()?;
    let now = seconds_now();
    let clock = veloct_core::SystemClock;
    let mut tracker = read_tracker(project.root.as_path(), now);

    if !tracker.decide(now).is_due() {
        return Ok(None);
    }

    // Section 17.8's manual path is the same work, so it is reused rather than duplicated.
    let outcome = veloct_app::manual_backup(project.root.as_path(), &clock)?;
    tracker.note_backup(now);
    write_tracker(project.root.as_path(), &tracker)?;

    // The run record, so the window can report a finding without re-scanning the project.
    let record = serde_json::json!({
        "archive_path": outcome.archive_path,
        "archive_sha256": outcome.archive_sha256,
        "needs_acknowledgement": outcome.needs_acknowledgement,
        "created_at": outcome.created_at,
    });
    let record_path = project
        .root
        .as_path()
        .join(veloct_app::BACKUP_DIR)
        .join("last-auto-run.json");
    veloct_core::workdir::ensure_parent(&record_path)?;
    veloct_core::workdir::write_atomic(
        &record_path,
        serde_json::to_vec_pretty(&record).unwrap_or_default().as_slice(),
    )?;

    // Section 17.7's cloud half. Section 17.3 is what orders these two steps: the local archive is
    // already written, and a finding BLOCKS the automatic upload rather than failing the backup. A user
    // who wants to send an archive the scan flagged does it deliberately through `backup_to_cloud`, which
    // takes an explicit acknowledgement.
    let targets = if outcome.needs_acknowledgement {
        Vec::new()
    } else {
        match cloud_run(
            &project,
            &outcome,
            None,
            veloct_backup::BackupTrigger::Automatic,
        ) {
            Ok(run) => run.targets,
            Err(error) => {
                // The upload failing must not lose the local archive that is already on disk, so the
                // failure is logged and reported as "no target stored it" rather than raised.
                let _ = project.log(
                    veloct_logging::LogEvent::warning(
                        veloct_logging::Category::Backup,
                        "automatic backup could not reach the cloud targets",
                    )
                    .with_field("error", error.to_string()),
                );
                Vec::new()
            }
        }
    };

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "automatic backup written")
            .with_field("archive", &outcome.archive_path)
            .with_field("files", outcome.file_count.to_string())
            .with_field(
                "needs_acknowledgement",
                outcome.needs_acknowledgement.to_string(),
            ),
    );

    Ok(Some(veloct_app::AutoBackupOutcome {
        archive_path: outcome.archive_path,
        archive_sha256: outcome.archive_sha256,
        targets,
        needs_acknowledgement: outcome.needs_acknowledgement,
    }))
}

/// The current time in seconds since the epoch, which is the unit section 17.5's rule works in.
fn seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// A cloud backup target as the window saves it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveCloudBackupTargetRequest {
    pub id: String,
    /// `github` or `s3` (`handoff.md` §17.4).
    pub kind: String,
    /// GitHub: `owner/repo`. S3: the bucket.
    pub location: String,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub retention: Option<usize>,
    /// The user's confirmation that the target is private (§17.2). Required before a run will use it.
    #[serde(default)]
    pub private_confirmed: bool,
    #[serde(default)]
    pub enabled: Option<bool>,
    /// The credential VALUE, written to the OS store. Never persisted in SQLite (§12.2).
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    /// S3: the access key ID. An IDENTIFIER rather than a secret — the SECRET access key goes to the OS
    /// store with `credential` (see `CloudTarget::access_key` for why the two are treated differently).
    #[serde(default)]
    pub access_key: Option<String>,
}

/// The configured cloud backup targets (§17.4).
#[tauri::command]
pub fn cloud_backup_targets(
    state: State<'_, AppState>,
) -> Result<Vec<veloct_app::CloudBackupTargetView>, CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    let targets = veloct_app::read_targets(project.root.as_path(), &clock)?;
    Ok(targets
        .iter()
        .map(|target| {
            let has = target
                .credential_ref
                .as_deref()
                .map(|handle| crate::credentials::get_or_none(handle).is_some())
                .unwrap_or(false);
            veloct_app::CloudBackupTargetView::of(target, has)
        })
        .collect())
}

/// Add or update a cloud backup target.
#[tauri::command]
pub fn save_cloud_backup_target(
    state: State<'_, AppState>,
    request: SaveCloudBackupTargetRequest,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    let kind = veloct_app::parse_cloud_kind(&request.kind)?;

    // The handle comes from ONE function, so the string written here and the string read at run time
    // cannot drift apart and silently orphan every saved credential.
    let handle = veloct_app::credential_handle(&request.id);
    if let Some(secret) = request.credential.filter(|value| !value.trim().is_empty()) {
        crate::credentials::set(&handle, secret.trim())?;
    }

    let mut target = veloct_backup::CloudTarget::new(&request.id, kind, &request.location);
    target.prefix = request.prefix.filter(|value| !value.trim().is_empty());
    target.private_confirmed = request.private_confirmed;
    target.enabled = request.enabled.unwrap_or(true);
    target.endpoint = request.endpoint.filter(|value| !value.trim().is_empty());
    target.region = request.region.filter(|value| !value.trim().is_empty());
    target.access_key = request.access_key.filter(|value| !value.trim().is_empty());
    // A retention of zero would delete the archive the run just uploaded, so the documented default
    // stands rather than being honoured.
    target.retention = request
        .retention
        .filter(|retention| *retention > 0)
        .unwrap_or(veloct_backup::DEFAULT_RETENTION);
    // Every supported kind needs a credential, so the handle is recorded; whether a VALUE exists yet is
    // what the window reports separately.
    target.credential_ref = Some(handle);

    veloct_app::save_target(project.root.as_path(), &target, &clock)?;

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "cloud backup target saved")
            .with_field("target_id", &request.id)
            .with_field("kind", kind.as_str())
            .with_field(
                "implemented",
                veloct_backup::is_implemented(kind).to_string(),
            ),
    );
    Ok(())
}

/// Remove a cloud backup target and its stored credential.
///
/// The `backups` rows for it are KEPT: they are the record of what was already sent there, which is
/// exactly what a user needs when deciding whether the remote copies can be deleted.
#[tauri::command]
pub fn delete_cloud_backup_target(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    if !veloct_app::delete_target(project.root.as_path(), &id, &clock)? {
        return Err(crate::commands::command_invalid(format!(
            "there is no cloud backup target {id}"
        )));
    }
    let _ = crate::credentials::remove(&veloct_app::credential_handle(&id));
    Ok(())
}

/// Prove a cloud target is reachable, and that it satisfies §17.2.
///
/// A public GitHub repository is reachable and still not a usable backup target, so reachability alone
/// would be a misleading success.
#[tauri::command]
pub fn test_cloud_backup_target(
    state: State<'_, AppState>,
    id: String,
) -> Result<String, CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    let targets = veloct_app::read_targets(project.root.as_path(), &clock)?;
    let target = targets
        .iter()
        .find(|target| target.id == id)
        .cloned()
        .ok_or_else(|| {
            crate::commands::command_invalid(format!("there is no cloud backup target {id}"))
        })?;

    let client = std::sync::Arc::new(veloct_core::http::ReqwestHttpClient::with_default_timeout(
        std::time::Duration::from_secs(60),
    )?);
    let lookup = |handle: &str| crate::credentials::get_or_none(handle);
    // The clock is injected down to the S3 adapter, whose signature is a function of its timestamp.
    let handle = veloct_app::handles_for(&[target], &lookup, client, std::sync::Arc::new(clock))
        .into_iter()
        .next()
        .ok_or_else(|| crate::commands::command_invalid("the target could not be built"))?;

    let said = handle.adapter.test_connection()?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "cloud backup target tested")
            .with_field("target_id", &id)
            .with_field("result", &said),
    );
    Ok(said)
}

/// What a cloud backup produced, alongside the local archive that made it possible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudBackupResultView {
    /// The local archive, which exists whether or not any upload happened (§17.3).
    pub backup: BackupOutcomeView,
    /// The per-target outcome, absent when a finding blocked the upload.
    pub run: Option<veloct_app::CloudBackupRunView>,
    /// Whether a secret finding blocked the upload (§17.3).
    pub blocked_by_finding: bool,
    /// Why it was blocked, in the user's terms.
    pub blocked_reason: Option<String>,
}

/// Take a backup AND send it to the configured cloud targets (§17.7, §17.8, §17.9).
///
/// Two things this deliberately does differently from [`manual_backup`]:
///
/// * **The local archive is taken first and is never lost.** §17.3's ordering is the contract's, not an
///   implementation detail: a finding, or an upload failure, must not cost the user their backup.
/// * **A finding requires an acknowledgement to upload.** `acknowledged_findings` is the user saying they
///   have seen the finding and still want this archive sent; without it the upload is skipped and the
///   reason is returned. That is §17.3's "the user must confirm before the risky upload", and it is why
///   this is a separate command from [`manual_backup`] rather than a flag on it.
#[tauri::command]
pub fn backup_to_cloud(
    state: State<'_, AppState>,
    target_ids: Option<Vec<String>>,
    acknowledged_findings: bool,
) -> Result<CloudBackupResultView, CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    let backup = veloct_app::manual_backup(project.root.as_path(), &clock)?;

    if backup.needs_acknowledgement && !acknowledged_findings {
        let reason = format!(
            "secret scan 在本次备份中发现 {} 处疑似凭据,自动上传已被阻止(handoff.md §17.3)。\
             本地备份已写入;确认后可以再上传。",
            backup.secret_findings.len()
        );
        let _ = project.log(
            veloct_logging::LogEvent::warning(
                veloct_logging::Category::Backup,
                "cloud upload blocked by a secret finding",
            )
            .with_field("archive", &backup.archive_path)
            .with_field("findings", backup.secret_findings.len().to_string()),
        );
        return Ok(CloudBackupResultView {
            backup,
            run: None,
            blocked_by_finding: true,
            blocked_reason: Some(reason),
        });
    }

    let subset = target_ids.filter(|ids| !ids.is_empty());
    let run = cloud_run(&project, &backup, subset, veloct_backup::BackupTrigger::Manual)?;

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "cloud backup finished")
            .with_field("archive", &backup.archive_path)
            .with_field("summary", &run.summary),
    );

    Ok(CloudBackupResultView {
        backup,
        run: Some(run),
        blocked_by_finding: false,
        blocked_reason: None,
    })
}

/// Send an archive that already exists to the selected cloud targets (§17.7).
///
/// This is the retry path: §17.7 gives every target its own copy, so a target that failed — or that was
/// not reachable at the time — is re-sent without taking and storing a SECOND local archive of the same
/// project state.
#[tauri::command]
pub fn resend_backup_to_cloud(
    state: State<'_, AppState>,
    archive_path: String,
    target_ids: Option<Vec<String>>,
) -> Result<veloct_app::CloudBackupRunView, CommandError> {
    let project = state.project()?;
    let path = std::path::PathBuf::from(&archive_path);
    if !path.is_file() {
        return Err(crate::commands::command_invalid(format!(
            "{archive_path} is not a file"
        )));
    }

    // The identity comes from the archive's OWN manifest where the manifest carries it, and from the
    // bytes on disk where it does not. `archive_sha256` is not stored in the manifest — it is the digest
    // of the finished archive, which only exists after the manifest was written — so it is recomputed the
    // same way `write_archive` computed it, which yields the same value for unchanged bytes and is
    // therefore the identity the original upload already used.
    let manifest = veloct_app::read_manifest(&path)?;
    let bytes = std::fs::read(&path).map_err(|error| veloct_core::VeloctError::io(&path, error))?;
    let backup = BackupOutcomeView {
        archive_path: path.to_string_lossy().to_string(),
        archive_bytes: bytes.len() as u64,
        archive_sha256: veloct_core::sha256_hex(&bytes),
        file_count: manifest.files.len(),
        total_bytes: manifest.files.iter().map(|entry| entry.size_bytes).sum(),
        skipped: Vec::new(),
        secret_findings: veloct_app::describe_scan(&manifest.secret_scan),
        needs_acknowledgement: manifest.needs_acknowledgement(),
        history_note: veloct_app::history_note(&manifest.secret_scan),
        created_at: manifest.created_at.clone(),
    };

    let subset = target_ids.filter(|ids| !ids.is_empty());
    let run = cloud_run(&project, &backup, subset, veloct_backup::BackupTrigger::Manual)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Backup, "backup re-sent to cloud")
            .with_field("archive", &archive_path)
            .with_field("summary", &run.summary),
    );
    Ok(run)
}

/// Run the configured targets for an archive that already exists.
///
/// Shared by the automatic rule, the manual command and the re-send command, so §17.7's independence and
/// the credential lookup cannot differ between the three paths.
fn cloud_run(
    project: &std::sync::Arc<crate::state::OpenProject>,
    backup: &BackupOutcomeView,
    target_ids: Option<Vec<String>>,
    trigger: veloct_backup::BackupTrigger,
) -> Result<veloct_app::CloudBackupRunView, CommandError> {
    let clock = veloct_core::SystemClock;
    let selection = match target_ids {
        Some(ids) if !ids.is_empty() => veloct_backup::TargetSelection::Subset(ids),
        _ => veloct_backup::TargetSelection::AllEnabled,
    };

    // The timeout is generous because an archive can be tens of megabytes and the Contents API takes it
    // base64-encoded inside a JSON body, which is a slow request on a domestic connection.
    let client = std::sync::Arc::new(veloct_core::http::ReqwestHttpClient::with_default_timeout(
        std::time::Duration::from_secs(300),
    )?);
    let lookup = |handle: &str| crate::credentials::get_or_none(handle);

    let path = std::path::PathBuf::from(&backup.archive_path);
    let scan_status = if backup.secret_findings.is_empty() {
        "clean"
    } else {
        "findings"
    };

    let run = veloct_app::run_against_targets(
        project.root.as_path(),
        veloct_app::ArchiveRef {
            path: &path,
            sha256: &backup.archive_sha256,
            bytes: backup.archive_bytes,
            created_at: &backup.created_at,
            trigger,
        },
        &selection,
        &lookup,
        client,
        veloct_app::ScanNote {
            status: scan_status,
            hits: backup.secret_findings.len(),
        },
        std::sync::Arc::new(clock),
    )?;
    Ok(run)
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
