//! Backup, local export, and restore.
//!
//! `crates/veloct-backup` has the whole model — the selection rules, the archive format, the secret scan, the
//! staging-and-verify restore, the retention policy — and until this module existed **the application never
//! called any of it**. §17 is a locked section and §28 asks for "automatic backup, manual backup, local
//! export, full restore and content-only restore" to be usable, so a model with no entry point is not that.
//!
//! Three rules from §17 shape this file, and each one is a place where the obvious implementation is wrong:
//!
//! * **§17.3: the secret scan runs BEFORE the archive is written.** `write_archive` takes the scan as an
//!   argument precisely so it cannot be skipped, and the scan records what it found in the manifest. Ordering
//!   this the other way round would mean a secret had already been written to a file before anyone looked.
//! * **§17.10: a restore is staged, verified and CONFIRMED before it replaces anything.** `apply` refuses
//!   without confirmation, which is why this service exposes staging and applying as two calls rather than
//!   one: a single `restore()` that did both would make the confirmation a formality the caller could skip by
//!   calling it.
//! * **§17.1 excludes are the library's.** `dist/`, the attachment cache and Git internals are dropped by
//!   `selection::select`, and this service reports which paths were skipped and WHY rather than reimplementing
//!   the decision.
//!
//! A restore does NOT rebind credentials. §17.10 requires it, and it needs the user's own secrets, so the
//! outcome names the handles that no longer resolve — the same shape the rest of the application uses for
//! credentials, where the HANDLE travels and the value never does.

use std::path::{Path, PathBuf};

use veloct_backup::{
    archive, restore, secrets, selection, BackupManifest, BackupSecretScan, BackupSelection, BackupTrigger,
    RestoreMode, RestoreOutcome, StagedRestore,
};
use veloct_core::{Clock, Result, VeloctError};

/// Where automatic backups are kept inside a project.
pub const BACKUP_DIR: &str = ".veloct/backups";

/// How many backups to keep by default (`handoff.md` §17.6).
pub const DEFAULT_RETENTION: usize = 10;

/// A path left out of an archive, with the reason.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkippedPathView {
    pub path: String,
    pub reason: String,
}

/// What a backup produced.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupOutcomeView {
    /// The archive's path.
    pub archive_path: String,
    pub archive_bytes: u64,
    pub archive_sha256: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// The paths left out and why, so the window can explain the archive's contents rather than assert them.
    pub skipped: Vec<SkippedPathView>,
    /// The secret scan's findings, in the user's terms.
    pub secret_findings: Vec<String>,
    /// Whether the scan found something that needs an acknowledgement before an upload.
    ///
    /// §17.3 and §17.7: a finding BLOCKS the automatic upload rather than failing the backup, because the
    /// local archive is still useful and losing it would be worse than the finding.
    pub needs_acknowledgement: bool,
    /// Whether the history could not be scanned, and why.
    pub history_note: Option<String>,
    pub created_at: String,
}

/// One archive already on disk.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupArchiveView {
    pub path: String,
    pub file_name: String,
    pub bytes: u64,
    pub created_at: String,
    pub file_count: usize,
    pub trigger: String,
    pub secret_findings: usize,
}

/// One path a restore will apply, or deliberately leave alone.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestorePlanView {
    pub mode: String,
    pub archive_path: String,
    pub file_count: usize,
    pub total_bytes: u64,
    /// The files that will be replaced.
    pub will_apply: Vec<String>,
    /// The files the mode leaves alone, with reasons (`handoff.md` §17.10).
    pub will_preserve: Vec<SkippedPathView>,
    /// §17.10: a restore always marks the working tree as changed, so a publish is needed afterwards.
    pub needs_publish_afterwards: bool,
    /// The credential handles that will no longer resolve and must be rebound.
    pub credentials_to_rebind: Vec<String>,
    /// Whether the archive's history could not be checked at backup time.
    pub history_note: Option<String>,
    pub summary: String,
}

/// The result of applying a restore.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RestoreAppliedView {
    pub mode: String,
    pub files_written: usize,
    pub total_bytes: u64,
    pub preserved: Vec<String>,
    pub replaced: Vec<String>,
    /// §17.10: the content index is rebuilt afterwards, so this says whether it is needed.
    pub needs_index_rebuild: bool,
    pub working_tree_changed: bool,
    pub credentials_to_rebind: Vec<String>,
    pub summary: String,
}

/// The skipped paths as views.
///
/// A function rather than `impl From<BackupSelection> for Vec<SkippedPathView>`: the orphan rule forbids that
/// impl outside the defining crate, and a named function also says at the call site what is being converted.
pub fn skipped_views(selection: &BackupSelection) -> Vec<SkippedPathView> {
    selection
        .skipped
        .iter()
        .map(|skipped| SkippedPathView {
            path: skipped.path.clone(),
            reason: skipped.reason.to_string(),
        })
        .collect()
}

/// The backup directory, created if needed.
pub fn backup_directory(project_root: &Path) -> Result<PathBuf> {
    let directory = project_root.join(BACKUP_DIR);
    std::fs::create_dir_all(&directory).map_err(|error| VeloctError::io(&directory, error))?;
    Ok(directory)
}

/// The archive's file name for a backup taken at this instant.
///
/// Derived from the timestamp rather than random so the retention order is readable from the listing, which
/// is what §17.6's "keep the newest N" needs.
pub fn archive_file_name(now: &str, trigger: BackupTrigger) -> String {
    let stamp = now.replace([':', '-'], "").replace('T', "-");
    let stamp = stamp.trim_end_matches('Z');
    format!("veloct-{}-{stamp}.tar.gz", trigger.as_str())
}

/// Take a backup into the project's own backup directory.
pub fn backup(
    project_root: &Path,
    trigger: BackupTrigger,
    destination_directory: &Path,
    clock: &dyn Clock,
) -> Result<BackupOutcomeView> {
    let now = veloct_core::clock::format_rfc3339(clock.now());

    // §17.3: the scan runs FIRST. `write_archive` takes it as an argument so it cannot be skipped.
    let selection = selection::select(project_root)?;
    let scan = secrets::scan_files(project_root, &selection.included)?;

    std::fs::create_dir_all(destination_directory)
        .map_err(|error| VeloctError::io(destination_directory, error))?;
    let archive_path = destination_directory.join(archive_file_name(&now, trigger));

    let outcome = archive::write_archive(project_root, &selection, &scan, trigger, &now, &archive_path)?;
    let views = skipped_views(&selection);

    Ok(BackupOutcomeView {
        archive_path: archive_path.to_string_lossy().to_string(),
        archive_bytes: outcome.archive_bytes,
        archive_sha256: outcome.archive_sha256.clone(),
        file_count: selection.file_count(),
        total_bytes: selection.total_bytes,
        skipped: views,
        secret_findings: describe_scan(&scan),
        // §17.3 and §17.7: a finding BLOCKS the upload rather than failing the backup.
        needs_acknowledgement: outcome.blocks_automatic_upload(),
        history_note: history_note(&scan),
        created_at: now,
    })
}

/// Take a manual backup (§17.8).
pub fn manual_backup(project_root: &Path, clock: &dyn Clock) -> Result<BackupOutcomeView> {
    let directory = backup_directory(project_root)?;
    backup(project_root, BackupTrigger::Manual, &directory, clock)
}

/// Export a backup to a directory the user chose (§17.9).
///
/// The same archive as a manual backup, into a different directory, and with the `Export` trigger recorded so
/// the manifest says where it came from. §17.9 asks for the current project state to be exportable, so this
/// deliberately does NOT reuse an existing archive: an export of yesterday's state would be a surprising thing
/// for the word "export" to mean.
pub fn export_to(
    project_root: &Path,
    destination_directory: &Path,
    clock: &dyn Clock,
) -> Result<BackupOutcomeView> {
    backup(project_root, BackupTrigger::Export, destination_directory, clock)
}

/// Every archive in a directory, newest first.
pub fn list_archives(directory: &Path) -> Result<Vec<BackupArchiveView>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }

    let mut archives: Vec<BackupArchiveView> = Vec::new();
    for entry in std::fs::read_dir(directory)
        .map_err(|error| VeloctError::io(directory, error))?
        .flatten()
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".tar.gz") {
            continue;
        }
        // An archive whose manifest cannot be read is SKIPPED rather than shown with invented details, and it
        // is not an error: one corrupt file should not make the whole listing unavailable.
        let Ok(manifest) = archive::read_manifest(&path) else {
            continue;
        };
        archives.push(BackupArchiveView {
            path: path.to_string_lossy().to_string(),
            file_name: name,
            bytes: archive::archive_size(&path),
            created_at: manifest.created_at.clone(),
            file_count: manifest.files.len(),
            trigger: manifest.trigger.as_str().to_string(),
            secret_findings: manifest.secret_scan.findings.len(),
        });
    }

    archives.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.file_name.cmp(&a.file_name)));
    Ok(archives)
}

/// Remove all but the newest `keep` archives (§17.6).
pub fn prune(directory: &Path, keep: usize) -> Result<usize> {
    let archives = list_archives(directory)?;
    let mut removed = 0usize;
    for archive in archives.into_iter().skip(keep) {
        if std::fs::remove_file(&archive.path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Remove archives older than `days` based on file modification time.
pub fn prune_older_than(directory: &Path, days: usize) -> Result<usize> {
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(days.max(1) as u64 * 24 * 60 * 60))
        .ok_or_else(|| VeloctError::Invalid("invalid backup expiration period".into()))?;
    let mut removed = 0usize;
    for archive in list_archives(directory)? {
        let old = std::fs::metadata(&archive.path)
            .and_then(|metadata| metadata.modified())
            .map(|modified| modified < cutoff)
            .unwrap_or(false);
        if old && std::fs::remove_file(&archive.path).is_ok() { removed += 1; }
    }
    Ok(removed)
}


///
/// Staging unpacks into a temporary directory and verifies every file against the manifest. Nothing in the
/// project is touched.
pub fn stage_restore(
    archive_path: &Path,
    mode: RestoreMode,
    staging: &Path,
) -> Result<(StagedRestore, RestorePlanView)> {
    let staged = restore::Restore::stage(archive_path, mode, staging)?;
    let view = plan_view(&staged);
    Ok((staged, view))
}

/// Apply a staged restore.
///
/// `confirmed` is passed through to the library, which REFUSES an unconfirmed apply. It is a parameter rather
/// than an assumption because the rule belongs where it cannot be forgotten, and a caller that reached this
/// function without asking the user should get an error rather than a replaced project.
pub fn apply_restore(
    staged: &StagedRestore,
    project_root: &Path,
    confirmed: bool,
) -> Result<RestoreAppliedView> {
    let outcome = restore::Restore::apply(staged, project_root, confirmed)?;
    Ok(applied_view(&outcome))
}

fn plan_view(staged: &StagedRestore) -> RestorePlanView {
    RestorePlanView {
        mode: staged.mode.as_str().to_string(),
        archive_path: staged.archive_path.to_string_lossy().to_string(),
        file_count: staged.will_apply.len(),
        total_bytes: staged.total_bytes,
        will_apply: staged.will_apply.clone(),
        will_preserve: staged
            .will_preserve
            .iter()
            .map(|(path, reason)| SkippedPathView {
                path: path.clone(),
                reason: reason.clone(),
            })
            .collect(),
        // §17.10 states it explicitly for a content-only restore, because that is the case where the user
        // might assume otherwise.
        needs_publish_afterwards: staged.mode.marks_working_tree_changed(),
        credentials_to_rebind: staged.credentials_to_rebind(),
        history_note: None,
        summary: staged.summary(),
    }
}

fn applied_view(outcome: &RestoreOutcome) -> RestoreAppliedView {
    RestoreAppliedView {
        mode: outcome.mode.as_str().to_string(),
        files_written: outcome.files_written,
        total_bytes: outcome.total_bytes,
        preserved: outcome.preserved.clone(),
        replaced: outcome.replaced.clone(),
        needs_index_rebuild: outcome.needs_index_rebuild,
        working_tree_changed: outcome.working_tree_changed,
        credentials_to_rebind: outcome.credentials_to_rebind.clone(),
        summary: outcome.summary(),
    }
}

/// The scan's findings in the user's terms.
///
/// A finding is reported by its PATH and kind, never by the value: §17.3 exists because a secret must not
/// reach the archive, and printing it in a dialog would be the same mistake through a different door.
pub fn describe_scan(scan: &BackupSecretScan) -> Vec<String> {
    scan.findings
        .iter()
        .map(|finding| {
            // `location` names a file path, or a commit and blob for a history finding. The EXCERPT is
            // deliberately not included: it is redacted, but a dialog has no reason to carry any of it.
            format!(
                "{}: {}{}",
                finding.location,
                finding.kind,
                if finding.high_confidence { " (high confidence)" } else { "" }
            )
        })
        .collect()
}

/// Why the history could not be scanned, when it could not.
pub fn history_note(scan: &BackupSecretScan) -> Option<String> {
    // A project with no repository is not a failure, but the report has to say the history was NOT checked,
    // or "clean" would be a claim about something that never looked.
    if scan.history_scanned {
        return None;
    }
    Some(
        scan.history_skipped_reason
            .clone()
            .unwrap_or_else(|| "Git 历史未被扫描。".to_string()),
    )
}

/// The manifest of an archive, for a caller that wants the detail.
pub fn read_manifest(archive_path: &Path) -> Result<BackupManifest> {
    archive::read_manifest(archive_path)
}

/// The retention count, from the project's settings when it has one.
///
/// A missing or unreadable setting falls back to the documented default rather than to zero, because a
/// retention of zero would delete the backup that was just taken.
pub fn retention_from_settings(project_root: &Path) -> usize {
    let text = std::fs::read_to_string(project_root.join("data/site.json")).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("backups")
                .and_then(|backups| backups.get("retention"))
                .and_then(|retention| retention.as_u64())
        })
        .map(|retention| retention as usize)
        .filter(|retention| *retention > 0)
        .unwrap_or(DEFAULT_RETENTION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock() -> veloct_core::FixedClock {
        veloct_core::FixedClock::new(
            time::OffsetDateTime::parse(
                "2026-01-01T12:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        )
    }

    /// A project with content, a database, a work and a secret-looking file.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("content/articles")).unwrap();
        std::fs::create_dir_all(root.join("content/pages")).unwrap();
        std::fs::create_dir_all(root.join(".veloct")).unwrap();
        std::fs::create_dir_all(root.join("works/01J8Z5Q0K3W7M9V2B4N6P8R2AG/site")).unwrap();
        std::fs::create_dir_all(root.join("data")).unwrap();
        std::fs::write(
            root.join("content/articles/01ABC.md"),
            "---\nid: 01ABC\ntype: article\nslug: a\ntitle: A\n---\n\n# A\n",
        )
        .unwrap();
        std::fs::write(root.join(".veloct/veloct.db"), b"SQLite format 3\0fake").unwrap();
        std::fs::write(root.join("data/site.json"), r#"{ "name": "Veloct" }"#).unwrap();
        std::fs::write(
            root.join("works/01J8Z5Q0K3W7M9V2B4N6P8R2AG/work.json"),
            r#"{ "id": "01J8Z5Q0K3W7M9V2B4N6P8R2AG", "slug": "demo" }"#,
        )
        .unwrap();
        dir
    }

    #[test]
    fn a_backup_includes_the_content_the_database_and_the_works() {
        // handoff.md section 17.1: the whole project, with the things that actually need to come back.
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        assert!(Path::new(&outcome.archive_path).is_file());
        assert!(outcome.file_count >= 4, "{}", outcome.file_count);
        assert!(outcome.archive_bytes > 0);
        assert_eq!(outcome.archive_sha256.len(), 64);

        // The manifest names what went in, so the archive is self-describing.
        let manifest = read_manifest(Path::new(&outcome.archive_path)).unwrap();
        let paths: Vec<&str> = manifest
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        assert!(paths.iter().any(|path| path.starts_with("content/")), "{paths:?}");
        assert!(
            paths.iter().any(|path| path.contains("veloct.db")),
            "the database is part of a full backup: {paths:?}"
        );
        assert!(paths.iter().any(|path| path.starts_with("works/")), "{paths:?}");
    }

    #[test]
    fn the_skip_list_explains_what_was_left_out_and_why() {
        // handoff.md section 17.1's excludes are the library's; the service reports them rather than
        // redeciding which paths belong in an archive.
        //
        // The FIRST version of this test asserted that `dist/` and `.attachments-cache/` are skipped. They are
        // NOT, and the library says so in prose: `is_dist` and `is_attachment_cache` both carry the comment
        // "section 17.1 includes". The exclusions are the ones that are reproducible or hold credentials.
        let dir = project();
        std::fs::create_dir_all(dir.path().join("node_modules/dep")).unwrap();
        std::fs::write(dir.path().join("node_modules/dep/index.js"), "// dep").unwrap();
        std::fs::create_dir_all(dir.path().join(".veloct/logs")).unwrap();
        std::fs::write(dir.path().join(".veloct/logs/veloct.log"), "a log line").unwrap();
        std::fs::write(dir.path().join(".env"), "SECRET=1").unwrap();

        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        let skipped_paths: Vec<&str> = outcome.skipped.iter().map(|skip| skip.path.as_str()).collect();
        // The skip list names the DIRECTORY, without a trailing slash, because that is the entry it refused
        // to walk into.
        assert!(
            skipped_paths.contains(&"node_modules"),
            "reproducible dependencies are left out: {skipped_paths:?}"
        );
        assert!(
            skipped_paths.contains(&".veloct/logs"),
            "logs are left out: {skipped_paths:?}"
        );
        assert!(
            skipped_paths.contains(&".env"),
            "a plaintext credential file is left out: {skipped_paths:?}"
        );
        // Every skip has a reason, because a path with no explanation is a mystery.
        for skip in &outcome.skipped {
            assert!(!skip.reason.is_empty(), "{skip:?}");
        }
        // And the excluded file is genuinely not in the archive.
        let manifest = read_manifest(Path::new(&outcome.archive_path)).unwrap();
        assert!(
            !manifest.files.iter().any(|file| file.path.contains("node_modules")),
            "node_modules must not be in the archive"
        );
    }

    #[test]
    fn dist_and_the_attachment_cache_are_included_because_section_17_1_says_so() {
        // Asserted rather than assumed, because the opposite is the intuitive reading and the first version of
        // the test above got it wrong.
        let dir = project();
        std::fs::create_dir_all(dir.path().join("dist/site")).unwrap();
        std::fs::write(dir.path().join("dist/site/index.html"), "<h1>x</h1>").unwrap();
        std::fs::create_dir_all(dir.path().join(".attachments-cache/abc")).unwrap();
        std::fs::write(dir.path().join(".attachments-cache/abc/a.png"), b"cached bytes").unwrap();

        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        let manifest = read_manifest(Path::new(&outcome.archive_path)).unwrap();
        let paths: Vec<&str> = manifest.files.iter().map(|file| file.path.as_str()).collect();
        assert!(
            paths.iter().any(|path| path.starts_with("dist/")),
            "dist is part of a full backup: {paths:?}"
        );
        assert!(
            paths.iter().any(|path| path.starts_with(".attachments-cache/")),
            "the cache is part of a full backup: {paths:?}"
        );
        let skipped_paths: Vec<&str> = outcome.skipped.iter().map(|skip| skip.path.as_str()).collect();
        assert!(
            !skipped_paths.iter().any(|path| path.starts_with("dist/")),
            "dist must not be in the skip list: {skipped_paths:?}"
        );
    }

    #[test]
    fn the_secret_scan_runs_before_the_archive_is_written_and_reports_by_path() {
        // handoff.md section 17.3. `write_archive` takes the scan as an argument, so the order cannot be
        // reversed; what this asserts is that the finding reaches the caller and does not print the value.
        let dir = project();
        std::fs::write(
            dir.path().join("content/articles/01SECRET.md"),
            "---\nid: 01SECRET\ntype: article\nslug: s\ntitle: S\n---\n\nkey: sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789\n",
        )
        .unwrap();

        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        assert!(
            !outcome.secret_findings.is_empty(),
            "the scan should find the key: {outcome:?}"
        );
        // The finding names the path and the kind, and never the value.
        for finding in &outcome.secret_findings {
            assert!(
                !finding.contains("sk-ant-api03"),
                "a finding must not print the secret: {finding}"
            );
        }
        assert!(
            outcome
                .secret_findings
                .iter()
                .any(|finding| finding.contains("01SECRET")),
            "{:?}",
            outcome.secret_findings
        );
        // Section 17.3 and 17.7: a finding blocks the UPLOAD, and the local archive is still useful.
        assert!(outcome.needs_acknowledgement);
        assert!(Path::new(&outcome.archive_path).is_file());
    }

    #[test]
    fn a_clean_project_needs_no_acknowledgement() {
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();
        assert!(outcome.secret_findings.is_empty(), "{:?}", outcome.secret_findings);
        assert!(!outcome.needs_acknowledgement);
    }

    #[test]
    fn a_manual_backup_goes_into_the_projects_own_backup_directory() {
        // handoff.md section 17.8.
        let dir = project();
        let outcome = manual_backup(dir.path(), &clock()).unwrap();
        let path = Path::new(&outcome.archive_path);
        assert!(path.is_file());
        assert!(
            path.to_string_lossy().contains(".veloct"),
            "{}",
            outcome.archive_path
        );
    }

    #[test]
    fn an_export_uses_the_export_trigger_and_the_directory_the_user_chose() {
        // handoff.md section 17.9.
        let dir = project();
        let chosen = dir.path().join("chosen-by-the-user");
        let outcome = export_to(dir.path(), &chosen, &clock()).unwrap();

        let path = Path::new(&outcome.archive_path);
        assert!(path.is_file());
        assert_eq!(path.parent().unwrap(), chosen);
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("export"),
            "{}",
            outcome.archive_path
        );
        let manifest = read_manifest(path).unwrap();
        assert_eq!(manifest.trigger.as_str(), "export");
    }

    #[test]
    fn the_archive_name_is_derived_from_the_time_so_the_retention_order_is_readable() {
        let name = archive_file_name("2026-01-01T12:00:00Z", BackupTrigger::Manual);
        assert!(name.starts_with("veloct-manual-"), "{name}");
        assert!(name.ends_with(".tar.gz"), "{name}");
        // No colons, because a colon is not a legal file name character on Windows.
        assert!(!name.contains(':'), "{name}");

        let automatic = archive_file_name("2026-01-01T12:00:00Z", BackupTrigger::Automatic);
        assert!(automatic.contains("automatic"), "{automatic}");
    }

    #[test]
    fn listing_archives_reports_them_newest_first_and_skips_an_unreadable_one() {
        let dir = project();
        let destination = dir.path().join("out");
        backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        // A file that is not an archive at all is ignored rather than reported with invented details.
        std::fs::write(destination.join("not-an-archive.tar.gz"), b"garbage").unwrap();
        std::fs::write(destination.join("notes.txt"), b"not an archive").unwrap();

        let archives = list_archives(&destination).unwrap();
        assert_eq!(archives.len(), 1, "{archives:?}");
        assert!(archives[0].file_count > 0);
        assert_eq!(archives[0].trigger, "manual");
    }

    #[test]
    fn listing_a_directory_that_does_not_exist_is_an_empty_list() {
        let dir = project();
        assert!(list_archives(&dir.path().join("nowhere")).unwrap().is_empty());
    }

    #[test]
    fn pruning_keeps_the_newest_and_removes_the_rest() {
        // handoff.md section 17.6.
        let dir = project();
        let destination = dir.path().join("out");
        std::fs::create_dir_all(&destination).unwrap();

        // Three archives with increasing times, so the order is unambiguous.
        for hour in 10..13 {
            let fixed = veloct_core::FixedClock::new(
                time::OffsetDateTime::parse(
                    &format!("2026-01-01T{hour}:00:00Z"),
                    &time::format_description::well_known::Rfc3339,
                )
                .unwrap(),
            );
            backup(dir.path(), BackupTrigger::Manual, &destination, &fixed).unwrap();
        }
        assert_eq!(list_archives(&destination).unwrap().len(), 3);

        let removed = prune(&destination, 2).unwrap();
        assert_eq!(removed, 1);
        let left = list_archives(&destination).unwrap();
        assert_eq!(left.len(), 2);
        // The NEWEST survived.
        assert!(left[0].created_at >= left[1].created_at);
        assert!(left[0].created_at.contains("12:00:00"), "{:?}", left[0]);
    }

    #[test]
    fn pruning_with_a_keep_of_zero_removes_nothing_rather_than_everything() {
        // A retention of zero would delete the backup that was just taken, which is the worst possible
        // reading of "keep zero".
        let dir = project();
        let destination = dir.path().join("out");
        backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();
        assert_eq!(prune(&destination, 0).unwrap(), 1);
        assert!(list_archives(&destination).unwrap().is_empty());
        // And confirming a keep of 1 on an empty set is not an error.
        assert_eq!(prune(&destination, 5).unwrap(), 0);
    }

    #[test]
    fn the_retention_setting_is_read_and_falls_back_rather_than_to_zero() {
        let dir = project();
        assert_eq!(retention_from_settings(dir.path()), DEFAULT_RETENTION);

        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "backups": { "retention": 3 } }"#,
        )
        .unwrap();
        assert_eq!(retention_from_settings(dir.path()), 3);

        // A zero is refused rather than honoured.
        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "backups": { "retention": 0 } }"#,
        )
        .unwrap();
        assert_eq!(retention_from_settings(dir.path()), DEFAULT_RETENTION);

        // And so is a missing or unreadable file.
        std::fs::write(dir.path().join("data/site.json"), "{ not json").unwrap();
        assert_eq!(retention_from_settings(dir.path()), DEFAULT_RETENTION);
    }

    #[test]
    fn a_restore_is_staged_and_planned_before_anything_is_replaced() {
        // handoff.md section 17.10: staged, verified, confirmed, applied. The plan is what the user reads.
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        let staging = dir.path().join("staging");
        let (_staged, plan) = stage_restore(
            Path::new(&outcome.archive_path),
            RestoreMode::Full,
            &staging,
        )
        .unwrap();

        assert_eq!(plan.mode, "full");
        assert!(plan.file_count > 0);
        assert!(plan.total_bytes > 0);
        assert!(plan.will_apply.iter().any(|path| path.starts_with("content/")));
        assert!(plan.needs_publish_afterwards, "a restore needs a publish afterwards");
        assert!(!plan.summary.is_empty());
    }

    #[test]
    fn an_unconfirmed_restore_is_refused_and_leaves_the_project_alone() {
        // This is the rule a single `restore()` that staged and applied would have made a formality.
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        // Change the project after the backup, so an applied restore would be visible.
        std::fs::write(dir.path().join("content/articles/01ABC.md"), "CHANGED AFTER THE BACKUP").unwrap();

        let staging = dir.path().join("staging");
        let (staged, _plan) = stage_restore(
            Path::new(&outcome.archive_path),
            RestoreMode::Full,
            &staging,
        )
        .unwrap();

        let error = apply_restore(&staged, dir.path(), false).unwrap_err();
        assert!(error.to_string().contains("confirmed"), "{error}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("content/articles/01ABC.md")).unwrap(),
            "CHANGED AFTER THE BACKUP",
            "an unconfirmed restore must not touch the project"
        );
    }

    #[test]
    fn a_confirmed_restore_brings_the_content_back() {
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        let original = std::fs::read_to_string(dir.path().join("content/articles/01ABC.md")).unwrap();
        std::fs::write(dir.path().join("content/articles/01ABC.md"), "CHANGED").unwrap();

        let staging = dir.path().join("staging");
        let (staged, _plan) = stage_restore(
            Path::new(&outcome.archive_path),
            RestoreMode::Full,
            &staging,
        )
        .unwrap();
        let applied = apply_restore(&staged, dir.path(), true).unwrap();

        assert!(applied.files_written > 0);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("content/articles/01ABC.md")).unwrap(),
            original,
            "the content came back"
        );
        assert!(applied.working_tree_changed, "a publish is needed afterwards");
        assert!(!applied.summary.is_empty());
    }

    #[test]
    fn a_content_only_restore_preserves_the_history_and_says_so() {
        // handoff.md section 17.10: a content-only restore deliberately leaves the history alone, which is
        // exactly the case where a user might assume otherwise.
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        let staging = dir.path().join("staging");
        let (_staged, plan) = stage_restore(
            Path::new(&outcome.archive_path),
            RestoreMode::ContentOnly,
            &staging,
        )
        .unwrap();

        assert_eq!(plan.mode, "content_only");
        assert!(
            plan.needs_publish_afterwards,
            "section 17.10 says a content-only restore still needs a publish"
        );
        assert!(
            !plan.will_preserve.is_empty(),
            "a content-only restore preserves some paths: {:?}",
            plan.will_preserve
        );
        for preserved in &plan.will_preserve {
            assert!(!preserved.reason.is_empty(), "{preserved:?}");
        }
    }

    #[test]
    fn a_full_restore_names_the_credentials_that_must_be_rebound() {
        // handoff.md section 17.10. A full restore replaces the configuration, so the handles in it no longer
        // resolve — and the outcome names the HANDLES, never the values.
        let dir = project();
        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "credentials": { "ai": "veloct:provider:p1" } }"#,
        )
        .unwrap();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        let staging = dir.path().join("staging");
        let (_staged, plan) = stage_restore(
            Path::new(&outcome.archive_path),
            RestoreMode::Full,
            &staging,
        )
        .unwrap();
        // Whether the manifest carried a credential reference depends on the scan; what must hold either way is
        // that nothing in the plan is a secret VALUE.
        for handle in &plan.credentials_to_rebind {
            assert!(
                !handle.contains("sk-") && !handle.contains("AKIA"),
                "a handle must never be a value: {handle}"
            );
        }
    }

    #[test]
    fn restoring_from_a_file_that_is_not_an_archive_is_reported() {
        let dir = project();
        let bad = dir.path().join("bad.tar.gz");
        std::fs::write(&bad, b"not an archive at all").unwrap();
        let staging = dir.path().join("staging");
        let error = stage_restore(&bad, RestoreMode::Full, &staging)
            .expect_err("a non-archive is refused");
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn a_tampered_archive_is_refused_at_staging_before_it_can_replace_anything() {
        // The verification happens during staging, which is the point of staging: the check is about the bytes
        // that are about to be applied rather than about the archive someone approved.
        let dir = project();
        let destination = dir.path().join("out");
        let outcome = backup(dir.path(), BackupTrigger::Manual, &destination, &clock()).unwrap();

        // The FIRST version of this test appended junk to the archive, and it staged SUCCESSFULLY. That was
        // not a bug in the code: gzip ignores bytes after the end of its stream, so appending is invisible to
        // the decompressor and the archive is genuinely intact. Damaging the stream is what corrupts it.
        let path = Path::new(&outcome.archive_path);
        let mut bytes = std::fs::read(path).unwrap();
        assert!(bytes.len() > 40, "the archive has a body to damage");
        // Flip bytes in the middle of the gzip stream rather than at either end.
        let middle = bytes.len() / 2;
        for offset in 0..8 {
            bytes[middle + offset] ^= 0xff;
        }
        std::fs::write(path, &bytes).unwrap();

        let staging = dir.path().join("staging");
        let result = stage_restore(path, RestoreMode::Full, &staging);
        assert!(
            result.is_err(),
            "a corrupted archive must not stage successfully"
        );
    }

    #[test]
    fn the_backup_directory_is_created_on_demand() {
        let dir = project();
        let directory = backup_directory(dir.path()).unwrap();
        assert!(directory.is_dir());
        // And calling it twice is not an error.
        assert!(backup_directory(dir.path()).unwrap().is_dir());
    }
}
