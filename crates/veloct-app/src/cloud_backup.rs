//! The cloud backup targets (`handoff.md` §17.4, §17.6, §17.7) and running a backup against them.
//!
//! `crates/veloct-backup` has the whole model — `CloudTarget`, the `BackupTargetAdapter` trait, the
//! per-target retention policy, the GitHub adapter — and the Tauri layer had none of it wired: an
//! automatic backup wrote the local archive and reported an EMPTY target list. §17.4 names exactly two
//! cloud targets and §17.7 makes them independent, so the missing piece was an entry point plus a place
//! for the configuration to live.
//!
//! Three decisions shape this file:
//!
//! * **The configuration lives in SQLite, in `backup_targets`.** ADR-0008 makes SQLite the truth source for
//!   application configuration, the table already existed in the schema (and was read by nothing), and
//!   §22.7 asks for the run records next to it. A JSON file beside the project would have been a second
//!   answer to a question the schema already answers.
//! * **A run's results are recorded per target, in `backups`.** §22.7 lists exactly the fields this writes,
//!   and §17.7's partial success is only auditable later if each target's own outcome was stored. A
//!   recording failure does NOT discard the run report — the uploads already happened, and a note travels
//!   with the result instead.
//! * **The credentials are resolved by the caller, by HANDLE.** §12.2 keeps credential VALUES out of
//!   SQLite, so a row holds `credential_ref` and the shell supplies the value through the same
//!   `CredentialLookup` callback deployment uses. The service's own tests therefore never touch a keychain.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use veloct_backup::{
    adapter_from_target, is_implemented, run_targets, unimplemented_reason, BackupRun, BackupTrigger,
    CloudTarget, CloudTargetKind, TargetHandle, TargetSelection,
};
use veloct_core::http::HttpClient;
use veloct_core::{Clock, Result, VeloctError};
use veloct_storage::open as open_database;

use crate::autobackup::TargetUploadOutcome;
use crate::deploy::CredentialLookup;

/// The table a target's configuration lives in.
///
/// The schema named it in the first migration and nothing read it until this module.
pub const CLOUD_TARGET_TABLE: &str = "backup_targets";

/// The credential handle for a target (§12.2).
///
/// One function, so the format cannot drift between the code that writes a credential and the code that
/// reads it — which would silently orphan every saved one. `src-tauri`'s store uses the same string.
pub fn credential_handle(id: &str) -> String {
    format!("backup:{id}")
}

fn db_error(error: rusqlite::Error) -> VeloctError {
    VeloctError::Database(format!("the cloud backup targets could not be read: {error}"))
}

/// Every configured cloud target, in a stable order.
///
/// A row whose `config` cannot be parsed is an ERROR rather than a silently skipped target: a target that
/// disappears from the list is one a user cannot see, fix or remove.
pub fn read_targets(project_root: &Path, clock: &dyn Clock) -> Result<Vec<CloudTarget>> {
    let (connection, _) = open_database(project_root, clock)?;
    let mut statement = connection
        .prepare(&format!(
            "SELECT config FROM {CLOUD_TARGET_TABLE} ORDER BY created_at, id"
        ))
        .map_err(db_error)?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_error)?;

    let mut targets = Vec::new();
    for row in rows {
        let text = row.map_err(db_error)?;
        targets.push(serde_json::from_str::<CloudTarget>(&text).map_err(|error| {
            VeloctError::Database(format!(
                "a {CLOUD_TARGET_TABLE} row holds a config this build cannot read: {error}"
            ))
        })?);
    }
    Ok(targets)
}

/// Add or replace a cloud target.
///
/// The whole [`CloudTarget`] is stored as JSON, and the columns beside it (`kind`, `name`, `credential_ref`,
/// `retention`, `enabled`) are written from that same value in the same statement so they cannot drift; they
/// exist to be QUERIED. `created_at` is set on insert and deliberately left alone on update, so it means
/// "when this target was first configured".
pub fn save_target(project_root: &Path, target: &CloudTarget, clock: &dyn Clock) -> Result<()> {
    target.validate().map_err(VeloctError::Invalid)?;

    let config = serde_json::to_string(target).map_err(|error| {
        VeloctError::Database(format!("could not serialise the cloud backup target: {error}"))
    })?;
    let now = veloct_core::clock::format_rfc3339(clock.now());

    let (connection, _) = open_database(project_root, clock)?;
    connection
        .execute(
            &format!(
                "INSERT INTO {CLOUD_TARGET_TABLE}
                   (id, kind, name, config, credential_ref, retention, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                   kind = excluded.kind,
                   name = excluded.name,
                   config = excluded.config,
                   credential_ref = excluded.credential_ref,
                   retention = excluded.retention,
                   enabled = excluded.enabled"
            ),
            rusqlite::params![
                target.id,
                target.kind.as_str(),
                // The schema requires a name and a cloud target has no separate display name; its
                // location (`owner/repo`, a bucket, an endpoint) is the human-meaningful identity.
                target.location,
                config,
                target.credential_ref,
                target.retention as i64,
                if target.enabled { 1i64 } else { 0i64 },
                now
            ],
        )
        .map_err(db_error)?;
    Ok(())
}

/// Remove a cloud target. Returns whether one was there.
///
/// The `backups` rows are deliberately KEPT. They record what was already sent where, which is history
/// rather than configuration, and erasing it would make "which archives exist on the old target" — the
/// question a user has precisely when removing one — unanswerable.
pub fn delete_target(project_root: &Path, id: &str, clock: &dyn Clock) -> Result<bool> {
    let (connection, _) = open_database(project_root, clock)?;
    let removed = connection
        .execute(
            &format!("DELETE FROM {CLOUD_TARGET_TABLE} WHERE id = ?1"),
            rusqlite::params![id],
        )
        .map_err(db_error)?;
    Ok(removed > 0)
}

/// One configured target, as the window lists it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudBackupTargetView {
    pub id: String,
    pub kind: String,
    pub label: String,
    /// GitHub: `owner/repo`. S3: the bucket.
    pub location: String,
    pub prefix: Option<String>,
    pub retention: usize,
    /// Whether the user has confirmed the target is private (§17.2).
    pub private_confirmed: bool,
    pub enabled: bool,
    /// The HANDLE, never a value (§12).
    pub credential_ref: Option<String>,
    pub has_credential: bool,
    /// S3: the access key ID, which is an identifier and not a secret.
    ///
    /// Reported so the window can show WHICH key is configured; the SECRET key never leaves the OS store.
    pub access_key: Option<String>,
    /// S3: the endpoint, when one is configured.
    pub endpoint: Option<String>,
    /// S3: the region.
    pub region: Option<String>,
    /// Whether this build has an adapter for the kind (§17.4).
    pub implemented: bool,
    /// Why not, when it does not.
    pub unimplemented_reason: Option<String>,
    /// Why the target would be refused before it is used (§17.2).
    pub refusal_reason: Option<String>,
}

impl CloudBackupTargetView {
    pub fn of(target: &CloudTarget, has_credential: bool) -> Self {
        Self {
            id: target.id.clone(),
            kind: target.kind.as_str().to_string(),
            label: target.kind.label().to_string(),
            location: target.location.clone(),
            prefix: target.prefix.clone(),
            retention: target.effective_retention(),
            private_confirmed: target.private_confirmed,
            enabled: target.enabled,
            credential_ref: target.credential_ref.clone(),
            has_credential,
            access_key: target.access_key.clone(),
            endpoint: target.endpoint.clone(),
            region: target.region.clone(),
            implemented: is_implemented(target.kind),
            unimplemented_reason: unimplemented_reason(target.kind).map(str::to_string),
            refusal_reason: target.refusal_reason(),
        }
    }
}

/// What the secret scan said about an archive, for the run record (§17.3, §22.7).
#[derive(Debug, Clone, Copy)]
pub struct ScanNote<'a> {
    /// A word for the scan's outcome, for example `clean` or `findings`.
    pub status: &'a str,
    pub hits: usize,
}

/// The archive a run is storing.
///
/// Its identity is the caller's, because the archive was written by the caller's own backup step and
/// hashing it again here would be a second, potentially different answer.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveRef<'a> {
    pub path: &'a Path,
    pub sha256: &'a str,
    pub bytes: u64,
    pub created_at: &'a str,
    pub trigger: BackupTrigger,
}

/// A cloud backup run, in the user's terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudBackupRunView {
    pub archive_path: String,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub created_at: String,
    pub trigger: String,
    pub targets: Vec<TargetUploadOutcome>,
    /// Whether every target that was attempted stored this backup.
    pub all_succeeded: bool,
    /// Whether no target stored it, whether because they failed or because none was attempted.
    pub nothing_stored: bool,
    /// §17.7's illustration, assembled from the real results.
    pub summary: String,
    /// Whether the run's results could be written to the index, and why not when they could not.
    ///
    /// A recording failure never discards the report: the uploads already happened, and saying otherwise
    /// would invite a user to send the same archive again.
    pub index_note: Option<String>,
}

impl CloudBackupRunView {
    pub fn of(run: &BackupRun, index_note: Option<String>) -> Self {
        let targets: Vec<TargetUploadOutcome> =
            run.targets.iter().map(crate::autobackup::target_upload_outcome).collect();
        let observed = crate::autobackup::AutoBackupOutcome {
            archive_path: run.archive_path.to_string_lossy().to_string(),
            archive_sha256: run.archive_sha256.clone(),
            targets: targets.clone(),
            // The scan's verdict belongs to the backup step that produced the archive, not here.
            needs_acknowledgement: false,
        };
        Self {
            archive_path: run.archive_path.to_string_lossy().to_string(),
            archive_sha256: run.archive_sha256.clone(),
            archive_bytes: run.archive_bytes,
            created_at: run.created_at.clone(),
            trigger: run.trigger.as_str().to_string(),
            targets,
            all_succeeded: observed.succeeded().len() == run.targets.len() && !run.targets.is_empty(),
            nothing_stored: observed.nothing_stored(),
            summary: observed.summary(),
            index_note,
        }
    }
}

/// Build the handles a run will use.
///
/// Every configured target becomes a handle, including one that cannot be built: §17.7 makes the targets
/// independent, and a target that vanished from the run would be a target a user cannot see failing.
///
/// `clock` reaches the S3 adapter because a SigV4 signature is a function of its timestamp, so an injected
/// clock is what makes a signature checkable in a test at all (ADR-0002).
pub fn handles_for(
    targets: &[CloudTarget],
    credential: CredentialLookup<'_>,
    client: Arc<dyn HttpClient>,
    clock: Arc<dyn Clock>,
) -> Vec<TargetHandle> {
    targets
        .iter()
        .map(|target| {
            let value = target
                .credential_ref
                .as_deref()
                .and_then(credential);
            adapter_from_target(target.clone(), value, Arc::clone(&client), Arc::clone(&clock))
        })
        .collect()
}

/// Run a finished archive against the configured cloud targets, and record the result.
pub fn run_against_targets(
    project_root: &Path,
    archive: ArchiveRef<'_>,
    selection: &TargetSelection,
    credential: CredentialLookup<'_>,
    client: Arc<dyn HttpClient>,
    scan: ScanNote<'_>,
    clock: Arc<dyn Clock>,
) -> Result<CloudBackupRunView> {
    let targets = read_targets(project_root, clock.as_ref())?;
    let handles = handles_for(&targets, credential, client, Arc::clone(&clock));

    let run = run_targets(
        &handles,
        selection,
        archive.path,
        archive.sha256,
        archive.bytes,
        archive.created_at,
        archive.trigger,
    );

    // §22.7's rows, one per target. A failure here is reported in the view rather than raised, because
    // the uploads have already happened and the report is the only evidence of them.
    let index_note = record_run(project_root, &run, scan, clock.as_ref())
        .err()
        .map(|error| format!("备份结果未能写入索引:{error}"));

    Ok(CloudBackupRunView::of(&run, index_note))
}

/// Write one run's per-target results into `backups` (`handoff.md` §22.7).
pub fn record_run(
    project_root: &Path,
    run: &BackupRun,
    scan: ScanNote<'_>,
    clock: &dyn Clock,
) -> Result<usize> {
    let now = veloct_core::clock::format_rfc3339(clock.now());
    let (connection, _) = open_database(project_root, clock)?;

    let mut written = 0usize;
    for result in &run.targets {
        connection
            .execute(
                "INSERT INTO backups
                   (id, target_id, kind, created_at, status, artifact_identifier, size,
                    snapshot_ref, secret_scan_status, secret_scan_hits, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                rusqlite::params![
                    veloct_core::ids::StableId::new().to_text(),
                    result.target_id,
                    result.kind.as_str(),
                    now,
                    result.status.as_str(),
                    // The remote object when one was written; otherwise the local archive is the only
                    // artefact that exists, and naming it is more useful than naming nothing.
                    result
                        .remote_path
                        .clone()
                        .unwrap_or_else(|| run.archive_path.to_string_lossy().to_string()),
                    run.archive_bytes as i64,
                    // The archive's own digest identifies the exact bytes this run sent.
                    run.archive_sha256,
                    scan.status,
                    scan.hits as i64,
                    result.error,
                ],
            )
            .map_err(|error| {
                VeloctError::Database(format!("could not record a backup run: {error}"))
            })?;
        written += 1;
    }
    Ok(written)
}

/// Parse a cloud target kind, for a caller that has it as text.
///
/// Named `parse_cloud_kind` rather than `parse_kind` because deployment has its own, and two functions
/// with the same name in one crate's exports is a name a caller cannot rely on.
pub fn parse_cloud_kind(text: &str) -> Result<CloudTargetKind> {
    match text.trim().to_ascii_lowercase().as_str() {
        "github" => Ok(CloudTargetKind::GitHub),
        "s3" => Ok(CloudTargetKind::S3),
        other => Err(VeloctError::Invalid(format!(
            "unknown cloud backup target kind {other:?}; expected `github` or `s3`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::clock::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::from_rfc3339("2026-01-01T12:00:00Z").unwrap()
    }

    fn target(id: &str) -> CloudTarget {
        let mut target = CloudTarget::new(id, CloudTargetKind::GitHub, "vellow/private-backups");
        target.prefix = Some("veloct-backups".to_string());
        target.retention = 5;
        target.private_confirmed = true;
        target.credential_ref = Some(credential_handle(id));
        target
    }

    #[test]
    fn the_credential_handle_is_stable_and_namespaced() {
        // A changed format would orphan every saved credential, so it is asserted rather than assumed.
        assert_eq!(credential_handle("gh"), "backup:gh");
    }

    #[test]
    fn a_saved_target_round_trips_through_the_index_with_every_field_intact() {
        // The `config` column carries the whole target and the columns beside it are a projection. A field
        // lost in the round trip would be a setting the window shows but the run ignores.
        let dir = tempfile::TempDir::new().unwrap();
        let mut saved = target("gh");
        saved.enabled = false;
        saved.private_confirmed = true;
        saved.endpoint = Some("https://s3.example.invalid".to_string());
        saved.region = Some("eu-central-1".to_string());

        save_target(dir.path(), &saved, &clock()).unwrap();
        let read = read_targets(dir.path(), &clock()).unwrap();
        assert_eq!(read, vec![saved]);
    }

    #[test]
    fn saving_the_same_id_twice_replaces_rather_than_duplicating() {
        let dir = tempfile::TempDir::new().unwrap();
        save_target(dir.path(), &target("gh"), &clock()).unwrap();

        let mut edited = target("gh");
        edited.location = "vellow/other-backups".to_string();
        save_target(dir.path(), &edited, &clock()).unwrap();

        let read = read_targets(dir.path(), &clock()).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].location, "vellow/other-backups");
    }

    #[test]
    fn an_unusable_target_is_refused_at_save_time_rather_than_stored() {
        // A stored target that can never run is a setting the window would show as configured.
        let dir = tempfile::TempDir::new().unwrap();
        let mut broken = target("gh");
        broken.location = "not-a-repository".to_string();
        let error = save_target(dir.path(), &broken, &clock()).unwrap_err();
        assert!(error.to_string().contains("owner/repo"), "{error}");
        assert!(read_targets(dir.path(), &clock()).unwrap().is_empty());
    }

    #[test]
    fn removing_a_target_keeps_the_history_of_what_was_already_sent_there() {
        // Section 22.7's rows are history, not configuration: "which archives exist on the old target" is
        // exactly the question a user has when removing one.
        let dir = tempfile::TempDir::new().unwrap();
        save_target(dir.path(), &target("gh"), &clock()).unwrap();
        assert!(delete_target(dir.path(), "gh", &clock()).unwrap());
        assert!(read_targets(dir.path(), &clock()).unwrap().is_empty());
        // Removing something that is not there is reported as "nothing was removed", not as a failure.
        assert!(!delete_target(dir.path(), "gh", &clock()).unwrap());
    }

    #[test]
    fn a_target_that_is_not_confirmed_private_carries_its_refusal_reason() {
        // Section 17.2 is enforced by the orchestration; the view has to be able to say WHY before a run.
        let mut unconfirmed = target("gh");
        unconfirmed.private_confirmed = false;
        let view = CloudBackupTargetView::of(&unconfirmed, true);
        assert!(!view.private_confirmed);
        assert!(view.refusal_reason.unwrap().contains("PRIVATE"));

        // And a confirmed one has no reason to refuse.
        assert!(CloudBackupTargetView::of(&target("gh"), true).refusal_reason.is_none());
    }

    #[test]
    fn both_of_section_17_4s_targets_are_implemented_and_the_view_carries_what_an_s3_one_needs() {
        // This test asserted the OPPOSITE until the SigV4 client landed: `!view.implemented` for S3, with
        // the reason naming the missing adapter. Section 17.4 names exactly two cloud targets, and both
        // are real now, so the assertion is inverted deliberately rather than deleted — a future round
        // that breaks S3 has to notice.
        let mut s3 = CloudTarget::new("s3", CloudTargetKind::S3, "veloct-private-backups");
        s3.private_confirmed = true;
        s3.region = Some("eu-central-1".to_string());
        s3.endpoint = Some("http://127.0.0.1:9000".to_string());
        s3.access_key = Some("AKIAIOSFODNN7EXAMPLE".to_string());

        let view = CloudBackupTargetView::of(&s3, true);
        assert!(view.implemented);
        assert!(view.unimplemented_reason.is_none());
        // The window shows WHICH key and WHICH endpoint; the secret key never leaves the OS store.
        assert_eq!(view.access_key.as_deref(), Some("AKIAIOSFODNN7EXAMPLE"));
        assert_eq!(view.endpoint.as_deref(), Some("http://127.0.0.1:9000"));
        assert_eq!(view.region.as_deref(), Some("eu-central-1"));
        assert!(view.credential_ref.is_none(), "an S3 target with no handle set");
        assert!(view.has_credential);

        let github = CloudBackupTargetView::of(&target("gh"), true);
        assert!(github.implemented);
        assert!(github.unimplemented_reason.is_none());
        assert!(github.access_key.is_none(), "the access key belongs to S3 alone");
    }

    #[test]
    fn a_kind_is_parsed_or_refused_rather_than_defaulted() {
        assert_eq!(parse_cloud_kind("github").unwrap(), CloudTargetKind::GitHub);
        assert_eq!(parse_cloud_kind("  S3 ").unwrap(), CloudTargetKind::S3);
        let error = parse_cloud_kind("dropbox").unwrap_err();
        assert!(error.to_string().contains("unknown cloud backup target kind"), "{error}");
    }

    #[test]
    fn a_run_records_one_row_per_target_including_the_ones_that_did_not_succeed() {
        // Section 22.7 wants the run recorded; section 17.7's partial success is only auditable later if
        // each target's own outcome was stored.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("a.tar.gz"), b"ARCHIVE").unwrap();

        let run = BackupRun {
            archive_path: dir.path().join("a.tar.gz"),
            archive_sha256: "a".repeat(64),
            archive_bytes: 7,
            created_at: "2026-01-01T12:00:00Z".to_string(),
            trigger: BackupTrigger::Manual,
            targets: vec![
                veloct_backup::TargetResult {
                    target_id: "gh".to_string(),
                    kind: CloudTargetKind::GitHub,
                    status: veloct_backup::TargetStatus::Succeeded,
                    remote_path: Some("veloct-backups/a.tar.gz".to_string()),
                    error: None,
                    pruned: 0,
                    kept: 1,
                },
                veloct_backup::TargetResult {
                    target_id: "s3".to_string(),
                    kind: CloudTargetKind::S3,
                    status: veloct_backup::TargetStatus::Failed,
                    remote_path: None,
                    error: Some("the bucket refused the upload".to_string()),
                    pruned: 0,
                    kept: 0,
                },
            ],
            subset: false,
        };

        let written = record_run(
            dir.path(),
            &run,
            ScanNote { status: "clean", hits: 0 },
            &clock(),
        )
        .unwrap();
        assert_eq!(written, 2);

        let (connection, _) = open_database(dir.path(), &clock()).unwrap();
        let rows: Vec<(String, String, String, Option<String>)> = connection
            .prepare("SELECT target_id, status, kind, error FROM backups ORDER BY target_id")
            .unwrap()
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "gh");
        assert_eq!(rows[0].1, "succeeded");
        assert_eq!(rows[0].3, None);
        assert_eq!(rows[1].0, "s3");
        assert_eq!(rows[1].1, "failed");
        assert_eq!(rows[1].3.as_deref(), Some("the bucket refused the upload"));
    }

    #[test]
    fn a_run_view_reports_partial_success_in_the_contracts_own_shape() {
        let run = BackupRun {
            archive_path: std::path::PathBuf::from(".veloct/backups/a.tar.gz"),
            archive_sha256: "b".repeat(64),
            archive_bytes: 10,
            created_at: "2026-01-01T12:00:00Z".to_string(),
            trigger: BackupTrigger::Automatic,
            targets: vec![
                veloct_backup::TargetResult {
                    target_id: "github".to_string(),
                    kind: CloudTargetKind::GitHub,
                    status: veloct_backup::TargetStatus::Succeeded,
                    remote_path: Some("a.tar.gz".to_string()),
                    error: None,
                    pruned: 1,
                    kept: 3,
                },
                veloct_backup::TargetResult {
                    target_id: "s3".to_string(),
                    kind: CloudTargetKind::S3,
                    status: veloct_backup::TargetStatus::Failed,
                    remote_path: None,
                    error: Some("the bucket refused the upload".to_string()),
                    pruned: 0,
                    kept: 0,
                },
            ],
            subset: false,
        };

        let view = CloudBackupRunView::of(&run, None);
        assert!(!view.all_succeeded);
        assert!(!view.nothing_stored, "one target stored it");
        assert!(view.summary.contains("本地备份 ✅"), "{}", view.summary);
        assert!(view.summary.contains("github ✅"), "{}", view.summary);
        assert!(view.summary.contains("s3 ❌"), "{}", view.summary);
        assert_eq!(view.targets[0].kept, 3);
    }

    #[test]
    fn a_skipped_target_is_not_a_failure_and_is_shown_as_skipped() {
        // A target the user did not select, or one refused for not being private, has nothing to retry.
        // Calling it a failure would put it in the same list as a target that broke.
        let run = BackupRun {
            archive_path: std::path::PathBuf::from("a.tar.gz"),
            archive_sha256: "c".repeat(64),
            archive_bytes: 1,
            created_at: "2026-01-01T12:00:00Z".to_string(),
            trigger: BackupTrigger::Manual,
            targets: vec![
                veloct_backup::TargetResult {
                    target_id: "gh".to_string(),
                    kind: CloudTargetKind::GitHub,
                    status: veloct_backup::TargetStatus::Succeeded,
                    remote_path: Some("a.tar.gz".to_string()),
                    error: None,
                    pruned: 0,
                    kept: 1,
                },
                veloct_backup::TargetResult {
                    target_id: "private".to_string(),
                    kind: CloudTargetKind::GitHub,
                    status: veloct_backup::TargetStatus::Skipped,
                    remote_path: None,
                    error: Some("GitHub must be a PRIVATE target".to_string()),
                    pruned: 0,
                    kept: 0,
                },
            ],
            subset: true,
        };

        let view = CloudBackupRunView::of(&run, None);
        assert!(view.targets[1].skipped);
        assert!(!view.targets[1].succeeded);
        assert!(!view.all_succeeded);
        assert!(view.summary.contains("private ⏭"), "{}", view.summary);
    }

    #[test]
    fn handlers_are_built_for_every_configured_target_even_when_one_cannot_be() {
        // Section 17.7's independence, at the point where the handles are assembled: a target that cannot
        // be built must still appear in the run, so a user sees it fail rather than sees it vanish.
        let client = Arc::new(veloct_core::http::FakeHttpClient::new()) as Arc<dyn HttpClient>;
        let mut with_credential = target("gh");
        with_credential.credential_ref = Some(credential_handle("gh"));
        let without_credential = target("no-credential");

        let stored = vec![
            CloudTarget {
                credential_ref: None,
                ..with_credential
            },
            CloudTarget {
                credential_ref: Some(credential_handle("no-credential")),
                ..without_credential
            },
        ];

        let lookup = |handle: &str| -> Option<String> {
            if handle == credential_handle("gh") {
                Some("ghp_example_token".to_string())
            } else {
                None
            }
        };
        let handles = handles_for(&stored, &lookup, client, Arc::new(clock()));
        assert_eq!(handles.len(), 2, "both targets appear in the run");

        // The one with a token can attempt a call; the one without reports why it cannot.
        let archive = std::path::Path::new("a.tar.gz");
        assert!(handles[1]
            .adapter
            .upload(archive, "n")
            .unwrap_err()
            .hint()
            .unwrap_or_default()
            .contains("token"));
    }
}
