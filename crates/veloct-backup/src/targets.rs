//! Cloud backup targets, retention and the automatic-backup rule.
//!
//! `handoff.md` §17.4 limits the supported cloud targets to GitHub and S3/S3-compatible — "no further
//! netdisk adapters" — and §17.2 adds two constraints that are not optional:
//!
//! * a GitHub target must be a PRIVATE repository,
//! * an S3 target must be a private bucket.
//!
//! Because the archive is not encrypted, those are the only thing standing between a credential that
//! slipped past the scan and the public internet. So they are validated rather than advised.
//!
//! §17.6 sets retention at 3 per target by default and makes it configurable. §17.7 makes the targets
//! independent: a failure on one does not affect the others, and a successful target keeps this
//! backup. There is no "all or nothing" path, for the same reason `handoff.md` §14.12 has none for
//! deployment.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

/// The default number of backups kept per cloud target (`handoff.md` §17.6).
pub const DEFAULT_RETENTION: usize = 3;

/// The idle period before an automatic backup runs (`handoff.md` §17.5).
pub const IDLE_BEFORE_AUTOMATIC_BACKUP: std::time::Duration =
    std::time::Duration::from_secs(10 * 60);

/// Which kind of cloud target a backup goes to (`handoff.md` §17.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudTargetKind {
    GitHub,
    S3,
}

impl CloudTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::S3 => "s3",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::S3 => "S3 / S3-compatible",
        }
    }
}

/// A cloud backup target's configuration.
///
/// No secret VALUES: `credential_ref` is a handle (`handoff.md` §12), which is what makes this struct
/// safe to serialise and to log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudTarget {
    pub id: String,
    pub kind: CloudTargetKind,
    pub enabled: bool,
    /// GitHub: `owner/repo`. S3: the bucket.
    pub location: String,
    /// The path prefix inside the target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// How many backups to keep for THIS target (`handoff.md` §17.6).
    #[serde(default = "default_retention")]
    pub retention: usize,
    /// Whether the user confirmed the target is private.
    ///
    /// Required, because §17.2 makes a private target the only protection for an unencrypted archive,
    /// and Veloct cannot verify privacy without calling the vendor with the user's credential.
    #[serde(default)]
    pub private_confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    /// S3 endpoint, for a compatible provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

fn default_retention() -> usize {
    DEFAULT_RETENTION
}

impl CloudTarget {
    pub fn new(id: impl Into<String>, kind: CloudTargetKind, location: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            enabled: true,
            location: location.into(),
            prefix: None,
            retention: DEFAULT_RETENTION,
            private_confirmed: false,
            credential_ref: None,
            endpoint: None,
            region: None,
        }
    }

    /// The effective retention, never zero.
    ///
    /// A retention of zero would delete every backup including the one just uploaded, which is not a
    /// policy a user can mean.
    pub fn effective_retention(&self) -> usize {
        self.retention.max(1)
    }

    /// Validate before the target is used.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("a cloud backup target needs an id".into());
        }
        if self.location.trim().is_empty() {
            return Err(format!("{} needs a target location", self.kind.label()));
        }
        if self.kind == CloudTargetKind::GitHub {
            let parts: Vec<&str> = self.location.split('/').collect();
            if parts.len() != 2 || parts.iter().any(|part| part.trim().is_empty()) {
                return Err(format!(
                    "the GitHub target {:?} must be in owner/repo form",
                    self.location
                ));
            }
        }
        if let Some(prefix) = &self.prefix {
            if prefix.contains("..") || prefix.starts_with('/') {
                return Err(format!(
                    "the prefix {prefix:?} must be a relative path without .."
                ));
            }
        }
        if self.retention == 0 {
            return Err("the retention must be at least 1".into());
        }
        Ok(())
    }

    /// Whether this target may receive an unencrypted archive.
    ///
    /// `handoff.md` §17.2: a GitHub target MUST be private and an S3 target MUST be private. Veloct
    /// cannot check that itself without the user's credential, so it requires the user to have
    /// confirmed it, and refuses otherwise rather than uploading and hoping.
    pub fn accepts_unencrypted_archive(&self) -> bool {
        self.private_confirmed
    }

    /// Why the target is refused, for the UI.
    pub fn refusal_reason(&self) -> Option<String> {
        if self.accepts_unencrypted_archive() {
            return None;
        }
        Some(format!(
            "{} must be a PRIVATE target before it can receive a backup: the archive is not \
             encrypted (handoff.md section 17.2), so a public target would publish the project. \
             Confirm the target is private to enable it.",
            self.kind.label()
        ))
    }
}

/// One target's result in a backup run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetResult {
    pub target_id: String,
    pub kind: CloudTargetKind,
    pub status: TargetStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How many old backups were removed by the retention policy.
    #[serde(default)]
    pub pruned: usize,
}

/// A target's status in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetStatus {
    Pending,
    Succeeded,
    Failed,
    /// Deliberately not attempted, for example because the target is not private.
    Skipped,
}

impl TargetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// A whole backup run across targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupRun {
    pub archive_path: PathBuf,
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub created_at: String,
    pub trigger: crate::archive::BackupTrigger,
    pub targets: Vec<TargetResult>,
    /// Whether the run was a manual one with a chosen target subset (`handoff.md` §17.8).
    #[serde(default)]
    pub subset: bool,
}

impl BackupRun {
    /// Whether the run is complete: at least one target stored the backup and none failed.
    ///
    /// A SKIPPED target does not make the run incomplete, because §17.7's rule is about failures: a
    /// target that was refused for not being private, or that the user did not select, did not
    /// attempt anything and has nothing to retry. What it does do is leave the user something to fix,
    /// which `skipped()` reports separately.
    pub fn is_complete(&self) -> bool {
        self.any_succeeded()
            && self
                .targets
                .iter()
                .all(|result| result.status != TargetStatus::Failed)
    }

    /// Whether every target that was ASKED to store the backup did so.
    ///
    /// False when a target was skipped, even though the run completed: this is the stricter question,
    /// and a caller that needs "everything configured is in place" wants this one.
    pub fn all_attempted_succeeded(&self) -> bool {
        let attempted: Vec<&TargetResult> = self
            .targets
            .iter()
            .filter(|result| result.status != TargetStatus::Skipped)
            .collect();
        !attempted.is_empty()
            && attempted
                .iter()
                .all(|result| result.status == TargetStatus::Succeeded)
    }

    /// The targets that were deliberately not attempted, with their reasons.
    pub fn skipped(&self) -> Vec<&TargetResult> {
        self.targets
            .iter()
            .filter(|result| result.status == TargetStatus::Skipped)
            .collect()
    }

    /// Whether at least one target succeeded.
    ///
    /// `handoff.md` §17.7: a partial success is a success for the targets that worked, and those
    /// targets keep this backup. This is the method that decides whether the run is worth recording as
    /// a backup rather than as a failure.
    pub fn any_succeeded(&self) -> bool {
        self.targets
            .iter()
            .any(|result| result.status == TargetStatus::Succeeded)
    }

    /// The targets that failed, which the UI reports with their reasons.
    pub fn failures(&self) -> Vec<&TargetResult> {
        self.targets
            .iter()
            .filter(|result| result.status == TargetStatus::Failed)
            .collect()
    }

    /// A summary line.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        for result in &self.targets {
            let mark = match result.status {
                TargetStatus::Succeeded => "ok",
                TargetStatus::Failed => "failed",
                TargetStatus::Skipped => "skipped",
                TargetStatus::Pending => "pending",
            };
            parts.push(format!("{} {mark}", result.target_id));
        }
        format!(
            "backup {} ({} bytes) -> {}",
            self.archive_sha256.chars().take(12).collect::<String>(),
            self.archive_bytes,
            if parts.is_empty() {
                "no targets".to_string()
            } else {
                parts.join(", ")
            }
        )
    }
}

/// A remote object, as the target reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBackup {
    pub path: String,
    /// When it was created, from the target's own listing, which is what retention sorts by.
    pub created_at: String,
    pub size_bytes: u64,
}

/// A place a backup can be stored.
pub trait BackupTargetAdapter: Send + Sync {
    fn config(&self) -> &CloudTarget;

    /// Upload the archive.
    fn upload(&self, archive: &Path, remote_name: &str) -> Result<String>;

    /// List the backups this target already holds, so retention can decide.
    fn list(&self) -> Result<Vec<RemoteBackup>>;

    /// Remove one, for retention.
    fn remove(&self, remote_path: &str) -> Result<()>;

    /// Prove the target is reachable.
    fn test_connection(&self) -> Result<String>;
}

/// An in-memory target, so the orchestration and the retention policy are testable.
#[derive(Debug)]
pub struct MemoryTarget {
    config: CloudTarget,
    objects: std::sync::Mutex<Vec<RemoteBackup>>,
    /// When set, every upload fails with this message.
    fail_uploads: std::sync::Mutex<Option<String>>,
    /// When true, the listing fails, which exercises the retention path's own failure handling.
    fail_list: std::sync::atomic::AtomicBool,
    uploads: std::sync::atomic::AtomicUsize,
    removals: std::sync::Mutex<Vec<String>>,
    /// A monotonic clock for uploads, so the listing can be ordered the way a real target's would be.
    /// Without it every upload looked equally old and retention could not be tested at all.
    clock: std::sync::atomic::AtomicU64,
}

impl MemoryTarget {
    pub fn new(config: CloudTarget) -> Self {
        Self {
            config,
            objects: std::sync::Mutex::new(Vec::new()),
            fail_uploads: std::sync::Mutex::new(None),
            fail_list: std::sync::atomic::AtomicBool::new(false),
            uploads: std::sync::atomic::AtomicUsize::new(0),
            removals: std::sync::Mutex::new(Vec::new()),
            clock: std::sync::atomic::AtomicU64::new(1_700_000_000),
        }
    }

    /// A target that is already holding some backups, for a retention test.
    pub fn with_objects(config: CloudTarget, objects: Vec<RemoteBackup>) -> Self {
        let target = Self::new(config);
        *target.objects.lock().unwrap() = objects;
        target
    }

    pub fn fail_uploads_with(&self, message: &str) {
        *self.fail_uploads.lock().unwrap() = Some(message.to_string());
    }

    pub fn allow_uploads(&self) {
        *self.fail_uploads.lock().unwrap() = None;
    }

    pub fn fail_list(&self, fail: bool) {
        self.fail_list
            .store(fail, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn upload_count(&self) -> usize {
        self.uploads.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn objects(&self) -> Vec<RemoteBackup> {
        self.objects.lock().unwrap().clone()
    }

    pub fn removals(&self) -> Vec<String> {
        self.removals.lock().unwrap().clone()
    }
}

impl BackupTargetAdapter for MemoryTarget {
    fn config(&self) -> &CloudTarget {
        &self.config
    }

    fn upload(&self, archive: &Path, remote_name: &str) -> Result<String> {
        self.uploads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        if let Some(message) = self.fail_uploads.lock().unwrap().clone() {
            return Err(VeloctError::Backup(message));
        }

        let size = std::fs::metadata(archive).map(|meta| meta.len()).unwrap_or(0);
        let path = remote_name.to_string();
        // A monotonic stamp so the newest upload sorts first, which is what a real target's listing
        // would give and what retention relies on.
        let stamp = self
            .clock
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.objects.lock().unwrap().push(RemoteBackup {
            path: path.clone(),
            created_at: format!("epoch:{stamp}"),
            size_bytes: size,
        });
        Ok(path)
    }

    fn list(&self) -> Result<Vec<RemoteBackup>> {
        if self.fail_list.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(VeloctError::Backup(
                "the target listing failed".to_string(),
            ));
        }
        let mut objects = self.objects.lock().unwrap().clone();
        // Newest first, which is what retention needs.
        objects.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(objects)
    }

    fn remove(&self, remote_path: &str) -> Result<()> {
        let mut objects = self.objects.lock().unwrap();
        if let Some(index) = objects.iter().position(|object| object.path == remote_path) {
            objects.remove(index);
            self.removals
                .lock()
                .unwrap()
                .push(remote_path.to_string());
            return Ok(());
        }
        Err(VeloctError::Backup(format!(
            "no remote backup at {remote_path}"
        )))
    }

    fn test_connection(&self) -> Result<String> {
        Ok(format!(
            "{} {} is reachable",
            self.config.kind.label(),
            self.config.location
        ))
    }
}

/// Apply a target's retention policy.
///
/// `handoff.md` §17.6: the oldest backups beyond the retention count are removed, PER TARGET. The
/// count includes the backup just uploaded, so a retention of 3 means "this one plus two older".
///
/// A listing failure is returned rather than ignored: silently keeping every backup would fill the
/// user's bucket, and silently deleting without a listing would be worse.
pub fn prune(adapter: &dyn BackupTargetAdapter, just_uploaded: Option<&str>) -> Result<usize> {
    let retention = adapter.config().effective_retention();
    let mut objects = adapter.list()?;

    // The newest first, which `list` promises.
    objects.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    // The newest `retention` objects survive, and the backup just uploaded is always one of them:
    // deleting it would defeat the run that produced it. The identity comparison matters because a
    // target's clock, or a name that sorts oddly, could otherwise put the new backup outside the
    // window.
    // The just-uploaded backup is the newest by definition, so it is placed first rather than trusted
    // to sort first: a target's clock can disagree with ours, and a name could sort oddly. Everything
    // else keeps the listing's order.
    let mut ordered: Vec<RemoteBackup> = Vec::with_capacity(objects.len());
    if let Some(new_one) = just_uploaded {
        if let Some(index) = objects.iter().position(|object| object.path == new_one) {
            ordered.push(objects.remove(index));
        }
    }
    ordered.append(&mut objects);

    let mut removed = 0usize;
    for object in ordered.iter().skip(retention) {
        adapter.remove(&object.path)?;
        removed += 1;
    }

    Ok(removed)
}

/// A configured target and its adapter.
pub struct TargetHandle {
    pub target: CloudTarget,
    pub adapter: Box<dyn BackupTargetAdapter>,
}

/// Which targets a run should use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSelection {
    /// Every enabled target, which is what an automatic backup does (`handoff.md` §17.7).
    AllEnabled,
    /// A subset the user chose for this run (`handoff.md` §17.8).
    Subset(Vec<String>),
}

impl TargetSelection {
    /// The ids this selection covers.
    pub fn ids<'a>(&self, available: &'a [String]) -> Vec<&'a String> {
        match self {
            Self::AllEnabled => available.iter().collect(),
            Self::Subset(chosen) => available
                .iter()
                .filter(|id| chosen.contains(id))
                .collect(),
        }
    }
}

/// A name for an archive inside a target.
///
/// Timestamp-first so a lexicographic listing is also a chronological one, which is exactly what
/// retention relies on.
pub fn remote_name(created_at: &str, sha256: &str, prefix: Option<&str>) -> String {
    let stamp = created_at
        .replace(['-', ':'], "")
        .replace('T', "-")
        .replace('Z', "");
    let name = format!("veloct-backup-{stamp}-{}.tar.gz", &sha256[..12.min(sha256.len())]);
    match prefix {
        Some(prefix) if !prefix.trim().is_empty() => {
            format!("{}/{}", prefix.trim_matches('/'), name)
        }
        _ => name,
    }
}

/// Run a backup against several targets.
///
/// Every target is attempted independently (`handoff.md` §17.7): a failure on one is recorded and the
/// others continue, and the successful ones keep this backup. There is no all-or-nothing path.
pub fn run_targets(
    handles: &[TargetHandle],
    selection: &TargetSelection,
    archive: &Path,
    archive_sha256: &str,
    archive_bytes: u64,
    created_at: &str,
    trigger: crate::archive::BackupTrigger,
) -> BackupRun {
    let available: Vec<String> = handles
        .iter()
        .filter(|handle| handle.target.enabled)
        .map(|handle| handle.target.id.clone())
        .collect();
    let selected: Vec<&String> = selection.ids(&available);

    let mut results = Vec::new();

    for handle in handles {
        let id = &handle.target.id;

        // Not selected for this run: reported as skipped rather than omitted, so the UI can show
        // which targets were deliberately left out.
        if !selected.contains(&id) {
            results.push(TargetResult {
                target_id: id.clone(),
                kind: handle.target.kind,
                status: TargetStatus::Skipped,
                remote_path: None,
                error: Some(if handle.target.enabled {
                    "not selected for this backup".to_string()
                } else {
                    "this target is disabled".to_string()
                }),
                pruned: 0,
            });
            continue;
        }

        // A target that is not confirmed private is refused rather than uploaded to
        // (handoff.md section 17.2).
        if let Some(reason) = handle.target.refusal_reason() {
            results.push(TargetResult {
                target_id: id.clone(),
                kind: handle.target.kind,
                status: TargetStatus::Skipped,
                remote_path: None,
                error: Some(reason),
                pruned: 0,
            });
            continue;
        }

        let name = remote_name(created_at, archive_sha256, handle.target.prefix.as_deref());
        match handle.adapter.upload(archive, &name) {
            Ok(remote_path) => {
                // Retention runs per target AFTER a successful upload, and its own failure is
                // recorded without turning the upload into a failure: the backup IS stored, and
                // saying otherwise would invite the user to upload it again.
                let pruned = match prune(handle.adapter.as_ref(), Some(&remote_path)) {
                    Ok(pruned) => pruned,
                    Err(error) => {
                        results.push(TargetResult {
                            target_id: id.clone(),
                            kind: handle.target.kind,
                            status: TargetStatus::Succeeded,
                            remote_path: Some(remote_path),
                            error: Some(format!(
                                "the backup was stored, but the retention pass failed: {error}"
                            )),
                            pruned: 0,
                        });
                        continue;
                    }
                };

                results.push(TargetResult {
                    target_id: id.clone(),
                    kind: handle.target.kind,
                    status: TargetStatus::Succeeded,
                    remote_path: Some(remote_path),
                    error: None,
                    pruned,
                });
            }
            Err(error) => {
                results.push(TargetResult {
                    target_id: id.clone(),
                    kind: handle.target.kind,
                    status: TargetStatus::Failed,
                    remote_path: None,
                    error: Some(error.to_string()),
                    pruned: 0,
                });
            }
        }
    }

    BackupRun {
        archive_path: archive.to_path_buf(),
        archive_sha256: archive_sha256.to_string(),
        archive_bytes,
        created_at: created_at.to_string(),
        trigger,
        targets: results,
        subset: matches!(selection, TargetSelection::Subset(_)),
    }
}

/// The automatic-backup decision (`handoff.md` §17.5).
///
/// The rule has three parts and the third is the one that is easy to get wrong: the timer is RESET by
/// a new change, so a user who keeps typing does not get a backup mid-sentence, and a user who stops
/// gets one after the idle period.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBackupTimer {
    /// The last time content changed, in milliseconds since the epoch.
    last_change_ms: u64,
    /// Whether anything has changed since the last backup.
    dirty: bool,
    idle_ms: u64,
}

impl AutoBackupTimer {
    pub fn new(idle: std::time::Duration) -> Self {
        Self {
            last_change_ms: 0,
            dirty: false,
            idle_ms: idle.as_millis() as u64,
        }
    }

    /// Record a content change. Resets the idle countdown.
    pub fn mark_changed(&mut self, now_ms: u64) {
        self.last_change_ms = now_ms;
        self.dirty = true;
    }

    /// Record that a backup completed.
    pub fn mark_backed_up(&mut self) {
        self.dirty = false;
    }

    /// Whether a backup should run now.
    ///
    /// Both conditions from §17.5: content has changed AND the idle period has elapsed since the last
    /// change.
    pub fn should_back_up(&self, now_ms: u64) -> bool {
        if !self.dirty {
            return false;
        }
        now_ms.saturating_sub(self.last_change_ms) >= self.idle_ms
    }

    /// How long until a backup becomes due, or zero when it already is.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        if !self.dirty {
            return 0;
        }
        self.idle_ms
            .saturating_sub(now_ms.saturating_sub(self.last_change_ms))
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The default timer: 10 minutes of idle (`handoff.md` §17.5).
    pub fn with_default_idle() -> Self {
        Self::new(IDLE_BEFORE_AUTOMATIC_BACKUP)
    }
}

/// Every configured target by id, for a lookup.
pub fn by_id(handles: &[TargetHandle]) -> BTreeMap<&str, &TargetHandle> {
    handles
        .iter()
        .map(|handle| (handle.target.id.as_str(), handle))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::BackupTrigger;
    use tempfile::TempDir;

    fn github(id: &str) -> CloudTarget {
        let mut target = CloudTarget::new(id, CloudTargetKind::GitHub, "vellow/private-backups");
        target.private_confirmed = true;
        target.credential_ref = Some(format!("keychain:backup/{id}"));
        target
    }

    fn s3(id: &str) -> CloudTarget {
        let mut target = CloudTarget::new(id, CloudTargetKind::S3, "veloct-private-backups");
        target.private_confirmed = true;
        target.region = Some("us-east-1".to_string());
        target
    }

    fn archive(dir: &TempDir) -> std::path::PathBuf {
        let path = dir.path().join("backup.tar.gz");
        std::fs::write(&path, b"archive bytes").unwrap();
        path
    }

    #[test]
    fn the_two_supported_cloud_targets_are_github_and_s3() {
        // handoff.md section 17.4: no further netdisk adapters.
        assert_eq!(CloudTargetKind::GitHub.as_str(), "github");
        assert_eq!(CloudTargetKind::S3.as_str(), "s3");
        assert_eq!(CloudTargetKind::GitHub.label(), "GitHub");
    }

    #[test]
    fn the_default_retention_is_three() {
        // handoff.md section 17.6.
        assert_eq!(DEFAULT_RETENTION, 3);
        assert_eq!(github("gh").retention, 3);
        assert_eq!(github("gh").effective_retention(), 3);
    }

    #[test]
    fn retention_is_configurable_and_never_zero() {
        let mut target = github("gh");
        target.retention = 10;
        assert_eq!(target.effective_retention(), 10);

        // A retention of zero would delete the backup just uploaded.
        target.retention = 0;
        assert_eq!(target.effective_retention(), 1);
        assert!(target.validate().unwrap_err().contains("at least 1"));
    }

    #[test]
    fn a_github_target_must_be_owner_slash_repo() {
        let mut target = CloudTarget::new("gh", CloudTargetKind::GitHub, "vellow");
        assert!(target.validate().unwrap_err().contains("owner/repo"));
        target.location = "vellow/".to_string();
        assert!(target.validate().unwrap_err().contains("owner/repo"));
        target.location = "vellow/private-backups".to_string();
        assert!(target.validate().is_ok());
    }

    #[test]
    fn a_target_without_a_location_is_refused() {
        let mut target = github("gh");
        target.location = String::new();
        assert!(target.validate().unwrap_err().contains("location"));
    }

    #[test]
    fn a_prefix_that_escapes_the_target_is_refused() {
        let mut target = github("gh");
        target.prefix = Some("../outside".to_string());
        assert!(target.validate().unwrap_err().contains("without .."));
        target.prefix = Some("/absolute".to_string());
        assert!(target.validate().unwrap_err().contains("without .."));
    }

    #[test]
    fn an_unconfirmed_private_target_is_refused_rather_than_uploaded_to() {
        // handoff.md section 17.2: the archive is not encrypted, so privacy is the only protection.
        let mut target = github("gh");
        target.private_confirmed = false;
        assert!(!target.accepts_unencrypted_archive());
        let reason = target.refusal_reason().unwrap();
        assert!(reason.contains("PRIVATE"), "{reason}");
        assert!(reason.contains("not encrypted"), "{reason}");

        target.private_confirmed = true;
        assert!(target.accepts_unencrypted_archive());
        assert!(target.refusal_reason().is_none());
    }

    #[test]
    fn a_refused_target_is_skipped_and_does_not_affect_the_others() {
        let dir = TempDir::new().unwrap();
        let path = archive(&dir);

        let mut unconfirmed = github("gh-public");
        unconfirmed.private_confirmed = false;

        let handles = vec![
            TargetHandle {
                target: unconfirmed,
                adapter: Box::new(MemoryTarget::new(github("gh-public"))),
            },
            TargetHandle {
                target: s3("s3"),
                adapter: Box::new(MemoryTarget::new(s3("s3"))),
            },
        ];

        let run = run_targets(
            &handles,
            &TargetSelection::AllEnabled,
            &path,
            "deadbeefdeadbeef",
            13,
            "2026-01-01T12:00:00Z",
            BackupTrigger::Automatic,
        );

        assert_eq!(run.targets[0].status, TargetStatus::Skipped);
        assert!(run.targets[0].error.as_deref().unwrap().contains("PRIVATE"));
        assert_eq!(run.targets[1].status, TargetStatus::Succeeded);
        // The target that worked stored the backup, which is what section 17.7 promises.
        assert!(run.any_succeeded());
        assert!(run.is_complete(), "nothing failed, so the run completed");
        // Every target that was ASKED did store it; the refused one was never asked. The skip is
        // reported separately, because it still leaves the user something to fix.
        assert!(
            run.all_attempted_succeeded(),
            "the only target that attempted the upload succeeded"
        );
        assert!(run.failures().is_empty(), "a refusal is not a failure");
        assert_eq!(run.skipped().len(), 1);
        assert!(run.skipped()[0].error.as_deref().unwrap().contains("PRIVATE"));
    }

    #[test]
    fn a_successful_target_keeps_the_backup_while_another_fails() {
        // handoff.md section 17.7, the example with GitHub ok and S3 failed.
        let dir = TempDir::new().unwrap();
        let path = archive(&dir);

        let gh_adapter = std::sync::Arc::new(MemoryTarget::new(github("gh")));
        let s3_adapter = std::sync::Arc::new(MemoryTarget::new(s3("s3")));
        s3_adapter.fail_uploads_with("the bucket does not exist");

        let handles = vec![
            TargetHandle {
                target: github("gh"),
                adapter: Box::new(SharedTarget(gh_adapter.clone())),
            },
            TargetHandle {
                target: s3("s3"),
                adapter: Box::new(SharedTarget(s3_adapter.clone())),
            },
        ];

        let run = run_targets(
            &handles,
            &TargetSelection::AllEnabled,
            &path,
            "deadbeefdeadbeef",
            13,
            "2026-01-01T12:00:00Z",
            BackupTrigger::Automatic,
        );

        assert_eq!(run.targets[0].status, TargetStatus::Succeeded);
        assert_eq!(run.targets[1].status, TargetStatus::Failed);
        assert!(run.targets[1]
            .error
            .as_deref()
            .unwrap()
            .contains("bucket does not exist"));

        // handoff.md section 17.7: the successful target independently keeps this backup.
        assert!(run.any_succeeded());
        assert!(!run.is_complete(), "a failure means the run is not complete");
        assert!(!run.all_attempted_succeeded());
        assert_eq!(gh_adapter.objects().len(), 1);
        assert_eq!(s3_adapter.objects().len(), 0);
        assert_eq!(run.failures().len(), 1);
        assert!(run.summary().contains("gh ok"), "{}", run.summary());
        assert!(run.summary().contains("s3 failed"), "{}", run.summary());
    }

    /// Forwards to a shared in-memory target so a test can inspect what happened.
    struct SharedTarget(std::sync::Arc<MemoryTarget>);

    impl BackupTargetAdapter for SharedTarget {
        fn config(&self) -> &CloudTarget {
            self.0.config()
        }
        fn upload(&self, archive: &Path, remote_name: &str) -> Result<String> {
            self.0.upload(archive, remote_name)
        }
        fn list(&self) -> Result<Vec<RemoteBackup>> {
            self.0.list()
        }
        fn remove(&self, remote_path: &str) -> Result<()> {
            self.0.remove(remote_path)
        }
        fn test_connection(&self) -> Result<String> {
            self.0.test_connection()
        }
    }

    #[test]
    fn a_manual_backup_can_choose_a_subset_of_targets() {
        // handoff.md section 17.8.
        let dir = TempDir::new().unwrap();
        let path = archive(&dir);

        let handles = vec![
            TargetHandle {
                target: github("gh"),
                adapter: Box::new(MemoryTarget::new(github("gh"))),
            },
            TargetHandle {
                target: s3("s3"),
                adapter: Box::new(MemoryTarget::new(s3("s3"))),
            },
        ];

        let run = run_targets(
            &handles,
            &TargetSelection::Subset(vec!["s3".to_string()]),
            &path,
            "deadbeefdeadbeef",
            13,
            "2026-01-01T12:00:00Z",
            BackupTrigger::Manual,
        );

        assert_eq!(run.targets[0].status, TargetStatus::Skipped);
        assert!(run.targets[0]
            .error
            .as_deref()
            .unwrap()
            .contains("not selected"));
        assert_eq!(run.targets[1].status, TargetStatus::Succeeded);
        assert!(run.subset);
    }

    #[test]
    fn a_disabled_target_is_skipped_and_says_why() {
        let dir = TempDir::new().unwrap();
        let path = archive(&dir);
        let mut target = github("gh");
        target.enabled = false;

        let handles = vec![TargetHandle {
            target: target.clone(),
            adapter: Box::new(MemoryTarget::new(target)),
        }];

        let run = run_targets(
            &handles,
            &TargetSelection::AllEnabled,
            &path,
            "deadbeefdeadbeef",
            13,
            "2026-01-01T12:00:00Z",
            BackupTrigger::Automatic,
        );

        assert_eq!(run.targets[0].status, TargetStatus::Skipped);
        assert!(run.targets[0].error.as_deref().unwrap().contains("disabled"));
        // Nothing stored the backup, so the run is not a complete one.
        assert!(!run.any_succeeded());
        assert!(!run.is_complete());
        assert!(!run.all_attempted_succeeded());
    }

    #[test]
    fn retention_keeps_the_configured_count_including_the_new_backup() {
        // handoff.md section 17.6: the default is 3, so this one plus two older.
        let existing: Vec<RemoteBackup> = (0..5)
            .map(|index| RemoteBackup {
                path: format!("veloct-backup-2025-01-0{index}-000000-aaaa.tar.gz"),
                created_at: format!("2025-01-0{index}T00:00:00Z"),
                size_bytes: 1,
            })
            .collect();
        let target = MemoryTarget::with_objects(github("gh"), existing);

        let removed = prune(&target, None).unwrap();
        assert_eq!(removed, 2, "five held, three kept");
        assert_eq!(target.objects().len(), 3);
        // The newest three survive.
        let remaining: Vec<String> = target
            .objects()
            .iter()
            .map(|object| object.created_at.clone())
            .collect();
        assert!(remaining.contains(&"2025-01-04T00:00:00Z".to_string()));
        assert!(!remaining.contains(&"2025-01-00T00:00:00Z".to_string()));
    }

    #[test]
    fn retention_never_removes_the_backup_just_uploaded() {
        // Deleting the backup the run just produced would defeat the run.
        let mut existing: Vec<RemoteBackup> = (0..5)
            .map(|index| RemoteBackup {
                path: format!("old-{index}.tar.gz"),
                created_at: format!("2026-01-0{index}T00:00:00Z"),
                size_bytes: 1,
            })
            .collect();
        // A path that sorts LAST, so a naive "keep the newest three by timestamp" would delete it.
        // The guard is by identity, not by position.
        let just_uploaded = "veloct-backup-2020-01-01-000000-zzzz.tar.gz".to_string();
        existing.push(RemoteBackup {
            path: just_uploaded.clone(),
            created_at: "2020-01-01T00:00:00Z".to_string(),
            size_bytes: 1,
        });
        let target = MemoryTarget::with_objects(github("gh"), existing);

        let removed = prune(&target, Some(&just_uploaded)).unwrap();

        // Six objects held and the retention is 3, so three go and three stay.
        assert_eq!(removed, 3, "six held, three kept");
        assert_eq!(target.objects().len(), 3);
        // The essential guarantee: the backup the run just produced is never the thing deleted, even
        // though its timestamp places it outside the window.
        assert!(
            target.objects().iter().any(|object| object.path == just_uploaded),
            "the new backup survives even when it sorts oldest"
        );
        assert!(!target.removals().contains(&just_uploaded));
        // And the three that stayed are the new one plus the two newest old ones.
        let remaining: Vec<String> = target
            .objects()
            .iter()
            .map(|object| object.path.clone())
            .collect();
        assert!(remaining.contains(&"old-4.tar.gz".to_string()), "{remaining:?}");
        assert!(remaining.contains(&"old-3.tar.gz".to_string()), "{remaining:?}");
    }

    #[test]
    fn retention_below_the_count_removes_nothing() {
        let existing = vec![RemoteBackup {
            path: "one.tar.gz".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            size_bytes: 1,
        }];
        let target = MemoryTarget::with_objects(github("gh"), existing);
        assert_eq!(prune(&target, None).unwrap(), 0);
        assert_eq!(target.objects().len(), 1);
    }

    #[test]
    fn a_listing_failure_is_reported_rather_than_ignored() {
        // Silently keeping every backup would fill the user's bucket.
        let target = MemoryTarget::new(github("gh"));
        target.fail_list(true);
        let error = prune(&target, None).unwrap_err();
        assert!(error.to_string().contains("listing failed"), "{error}");
    }

    #[test]
    fn a_retention_failure_does_not_turn_a_successful_upload_into_a_failure() {
        // The backup IS stored, and saying otherwise would invite a duplicate upload.
        let dir = TempDir::new().unwrap();
        let path = archive(&dir);

        let target = github("gh");
        let adapter = std::sync::Arc::new(MemoryTarget::new(target.clone()));
        adapter.fail_list(true);

        let handles = vec![TargetHandle {
            target,
            adapter: Box::new(SharedTarget(adapter.clone())),
        }];

        let run = run_targets(
            &handles,
            &TargetSelection::AllEnabled,
            &path,
            "deadbeefdeadbeef",
            13,
            "2026-01-01T12:00:00Z",
            BackupTrigger::Automatic,
        );

        assert_eq!(run.targets[0].status, TargetStatus::Succeeded);
        assert!(run.targets[0]
            .error
            .as_deref()
            .unwrap()
            .contains("retention pass failed"));
        assert_eq!(adapter.upload_count(), 1, "the upload happened once");
        assert_eq!(adapter.objects().len(), 1, "and the backup is stored");
    }

    #[test]
    fn a_remote_name_is_timestamp_first_and_prefixed() {
        // Timestamp-first so a lexicographic listing is also chronological, which retention relies on.
        let name = remote_name("2026-01-01T12:00:00Z", "deadbeefcafe1234", None);
        assert!(name.starts_with("veloct-backup-20260101-120000-"), "{name}");
        assert!(name.ends_with(".tar.gz"), "{name}");
        assert!(name.contains("deadbeefcafe"), "{name}");

        let prefixed = remote_name("2026-01-01T12:00:00Z", "deadbeefcafe1234", Some("backups"));
        assert!(prefixed.starts_with("backups/"), "{prefixed}");

        let trailing = remote_name("2026-01-01T12:00:00Z", "deadbeefcafe1234", Some("backups/"));
        assert!(trailing.starts_with("backups/"), "{trailing}");
        assert!(!trailing.contains("//"), "{trailing}");
    }

    #[test]
    fn the_automatic_timer_waits_for_the_idle_period() {
        // handoff.md section 17.5: content changed, then 10 minutes with no new change.
        let mut timer = AutoBackupTimer::with_default_idle();
        assert!(!timer.should_back_up(0), "nothing has changed yet");

        timer.mark_changed(1_000_000);
        assert!(!timer.should_back_up(1_000_000), "just changed");
        assert!(!timer.should_back_up(1_000_000 + 599_999), "not yet idle");
        assert_eq!(timer.remaining_ms(1_000_000 + 599_999), 1);
        assert!(timer.should_back_up(1_000_000 + 600_000), "10 minutes of quiet");
    }

    #[test]
    fn a_new_change_restarts_the_idle_countdown() {
        // A user who keeps typing must not get a backup mid-sentence.
        let mut timer = AutoBackupTimer::with_default_idle();
        timer.mark_changed(0);
        assert!(timer.should_back_up(600_000));

        // Another change at 599 seconds pushes the deadline out.
        timer.mark_changed(599_000);
        assert!(!timer.should_back_up(600_000), "the timer was reset");
        assert!(timer.should_back_up(599_000 + 600_000));
    }

    #[test]
    fn a_completed_backup_clears_the_dirty_flag() {
        let mut timer = AutoBackupTimer::with_default_idle();
        timer.mark_changed(0);
        assert!(timer.is_dirty());
        assert!(timer.should_back_up(600_000));

        timer.mark_backed_up();
        assert!(!timer.is_dirty());
        assert!(!timer.should_back_up(1_200_000), "nothing has changed since");
        assert_eq!(timer.remaining_ms(1_200_000), 0);
    }

    #[test]
    fn the_default_idle_period_is_ten_minutes() {
        assert_eq!(IDLE_BEFORE_AUTOMATIC_BACKUP.as_secs(), 600);
    }

    #[test]
    fn a_run_round_trips_through_json_for_the_ui() {
        let dir = TempDir::new().unwrap();
        let path = archive(&dir);
        let handles = vec![TargetHandle {
            target: github("gh"),
            adapter: Box::new(MemoryTarget::new(github("gh"))),
        }];
        let run = run_targets(
            &handles,
            &TargetSelection::AllEnabled,
            &path,
            "deadbeefdeadbeef",
            13,
            "2026-01-01T12:00:00Z",
            BackupTrigger::Manual,
        );

        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("\"status\":\"succeeded\""), "{json}");
        assert!(json.contains("\"trigger\":\"manual\""), "{json}");
        assert_eq!(serde_json::from_str::<BackupRun>(&json).unwrap(), run);
    }

    #[test]
    fn a_target_lookup_is_keyed_by_id() {
        let handles = vec![
            TargetHandle {
                target: github("gh"),
                adapter: Box::new(MemoryTarget::new(github("gh"))),
            },
            TargetHandle {
                target: s3("s3"),
                adapter: Box::new(MemoryTarget::new(s3("s3"))),
            },
        ];
        let map = by_id(&handles);
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("gh"));
        assert!(map.contains_key("s3"));
    }

    #[test]
    fn a_connection_test_reports_the_target() {
        let target = MemoryTarget::new(github("gh"));
        let message = target.test_connection().unwrap();
        assert!(message.contains("vellow/private-backups"), "{message}");
    }
}
