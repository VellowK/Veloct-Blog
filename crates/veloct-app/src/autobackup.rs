//! The automatic backup's idle rule.
//!
//! `handoff.md` §17.5 is three conditions and one reset:
//!
//! > 1. the content changed. 2. ten minutes by default pass with no new modification. 3. a backup runs.
//! > A modification in the meantime RESTARTS the timer.
//!
//! That is arithmetic, so it is a pure function here rather than a timer inside this module. A rule that could
//! only be exercised by waiting ten minutes would be a rule nobody exercises, and the reset condition — the part
//! that is easy to get wrong — is exactly the part a test must be able to reach in microseconds.
//!
//! Two things this deliberately does NOT own:
//!
//! * **A timer.** Nothing here sleeps or schedules. The caller decides when to ask, and the answer is a
//!   decision rather than a side effect, so the same function serves a polling loop and a test.
//! * **The upload.** §17.7 allows an automatic backup to reach some targets and fail others, and the success of
//!   one target is independent of the others. That is `veloct_backup::run_targets`, and this only decides
//!   WHETHER a run should happen.

use serde::{Deserialize, Serialize};

use veloct_backup::BackupManifest;

/// Ten minutes, the documented default (`handoff.md` §17.5).
pub const DEFAULT_IDLE_SECONDS: u64 = 600;

/// The state the idle rule needs.
///
/// A WRAPPER, not a second implementation. `veloct_backup::AutoBackupTimer` already had section 17.5's
/// arithmetic, and this module was written without noticing — so for one round there were two copies of the rule
/// and two chances for them to disagree. The timer now owns the arithmetic and this owns what the timer does
/// not have: a THREE-STATE answer (the crate offers a boolean, so "nothing changed" and "not yet" cannot be
/// told apart) and the reasons a caller can show.
///
/// Held by the caller rather than in this module, because it has to survive a restart: a user who edits, closes
/// the application and reopens it ten minutes later should still get a backup, and an in-memory-only timestamp
/// would lose that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdleTracker {
    /// The crate's timer, which is the ONE implementation of the rule.
    inner: veloct_backup::AutoBackupTimer,
}

impl Default for IdleTracker {
    fn default() -> Self {
        Self::new(0)
    }
}

impl IdleTracker {
    /// A tracker with the documented idle period, anchored at `now_seconds`.
    pub fn new(now_seconds: u64) -> Self {
        Self {
            inner: veloct_backup::AutoBackupTimer::new(std::time::Duration::from_secs(
                DEFAULT_IDLE_SECONDS,
            )),
        }
        .anchored_at(now_seconds)
    }

    /// Anchor the timer at an instant without marking the content dirty.
    ///
    /// A tracker read from disk has no change to record, and one created fresh must not report `NoChange` for
    /// the wrong reason — so the anchor sets the STATE directly rather than going through `mark_changed`, which
    /// would also raise the dirty flag. The rule's arithmetic still lives in the crate; this sets a timestamp.
    fn anchored_at(mut self, now_seconds: u64) -> Self {
        self.inner.last_change_ms = now_seconds * 1_000;
        self.inner.dirty = false;
        self
    }

    /// The idle period in seconds, which is the unit the settings use.
    pub fn idle_seconds(&self) -> u64 {
        self.inner.idle_ms / 1_000
    }

    /// When the content last changed, in seconds since the epoch.
    pub fn last_change_seconds(&self) -> u64 {
        // The crate keeps milliseconds, and the caller-facing unit here is seconds.
        self.inner.last_change_ms / 1_000
    }

    /// Whether anything has changed since the last backup.
    pub fn changed_since_backup(&self) -> bool {
        self.inner.is_dirty()
    }

    /// Record that the content changed, which RESTARTS the timer (§17.5).
    pub fn note_change(&mut self, now_seconds: u64) {
        // Through the crate, so a change to the rule cannot leave this copy behind.
        self.inner.mark_changed(now_seconds * 1_000);
    }

    /// Record that a backup ran, clearing the change flag and restarting the timer.
    pub fn note_backup(&mut self, now_seconds: u64) {
        self.inner.mark_backed_up();
        self.inner.last_change_ms = now_seconds * 1_000;
    }

    /// How long the content has been idle at this instant.
    pub fn idle_for(&self, now_seconds: u64) -> u64 {
        now_seconds.saturating_sub(self.last_change_seconds())
    }

    /// Whether an automatic backup should run now, and why not when it should not.
    pub fn decide(&self, now_seconds: u64) -> IdleDecision {
        // The three states exist because the crate's boolean cannot express two of them, and a UI that said
        // "not yet" for an unchanged project would show a countdown toward nothing.
        if !self.inner.is_dirty() {
            return IdleDecision::NoChange;
        }
        if self.inner.should_back_up(now_seconds * 1_000) {
            return IdleDecision::Due;
        }
        IdleDecision::Waiting {
            remaining_seconds: self.inner.remaining_ms(now_seconds * 1_000) / 1_000,
        }
    }

    /// The tracker with a different idle period, for a user setting or a test.
    pub fn with_idle_seconds(mut self, seconds: u64) -> Self {
        // Only the PERIOD changes: the state is what the caller was already tracking, and rebuilding the timer
        // would lose it.
        self.inner.idle_ms = seconds * 1_000;
        self
    }
}

/// What the idle rule decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum IdleDecision {
    /// Nothing changed since the last backup, so there is nothing to do.
    NoChange,
    /// Something changed and the timer has not elapsed.
    Waiting { remaining_seconds: u64 },
    /// §17.5's conditions hold: run a backup.
    Due,
}

impl IdleDecision {
    pub fn is_due(self) -> bool {
        matches!(self, Self::Due)
    }

    /// A phrase for the UI.
    pub fn describe(self) -> String {
        match self {
            Self::NoChange => "内容没有变化,不触发自动备份。".to_string(),
            Self::Waiting { remaining_seconds } => format!(
                "内容空闲中,还有约 {} 分钟触发自动备份。",
                remaining_seconds.div_ceil(60)
            ),
            Self::Due => "内容已空闲足够久,触发自动备份。".to_string(),
        }
    }
}

/// The per-target retention count default (`handoff.md` §17.6).
///
/// THREE, not the ten this module's first version used. §17.6 says "每个云端目标默认保留 **3** 份", and a
/// retention that keeps more than the contract asks is a contract violation, not caution: it means a user's
/// cloud storage fills up with archives they did not ask to keep.
pub const DEFAULT_CLOUD_RETENTION: usize = 3;

/// One target's result in an automatic run (`handoff.md` §17.7).
///
/// Three outcomes, not two. `veloct_backup::TargetStatus` already distinguishes a target that FAILED from
/// one that was deliberately not attempted — it was not selected, it is disabled, or it was refused for
/// not being private — and flattening the two here would report a target with nothing to retry as though
/// it had tried and broken, or a target refused for a fixable reason as though it had succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetUploadOutcome {
    pub target_id: String,
    pub succeeded: bool,
    /// Whether the target was deliberately not attempted, with the reason in `error`.
    #[serde(default)]
    pub skipped: bool,
    /// Why not, when it did not succeed.
    pub error: Option<String>,
    /// How many archives this target now holds after its retention was applied.
    pub kept: usize,
}

impl TargetUploadOutcome {
    pub fn ok(target_id: impl Into<String>, kept: usize) -> Self {
        Self {
            target_id: target_id.into(),
            succeeded: true,
            skipped: false,
            error: None,
            kept,
        }
    }

    pub fn failed(target_id: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            target_id: target_id.into(),
            succeeded: false,
            skipped: false,
            error: Some(error.into()),
            kept: 0,
        }
    }

    /// A target that was not attempted, and why.
    pub fn skipped(target_id: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            target_id: target_id.into(),
            succeeded: false,
            skipped: true,
            error: Some(reason.into()),
            kept: 0,
        }
    }
}

/// One target's result in the shape the window shows.
///
/// `veloct_backup::run_targets` is the orchestration that decides it — §17.7's independence, the
/// per-target retention pass and the refusal of a target that is not private all live there. This only
/// translates its result, so there is no second copy of the rule.
pub fn target_upload_outcome(result: &veloct_backup::TargetResult) -> TargetUploadOutcome {
    use veloct_backup::TargetStatus;
    match result.status {
        TargetStatus::Succeeded => TargetUploadOutcome::ok(&result.target_id, result.kept),
        TargetStatus::Failed => TargetUploadOutcome::failed(
            &result.target_id,
            result
                .error
                .clone()
                .unwrap_or_else(|| "the target failed without reporting a reason".to_string()),
        ),
        TargetStatus::Skipped | TargetStatus::Pending => TargetUploadOutcome::skipped(
            &result.target_id,
            result
                .error
                .clone()
                .unwrap_or_else(|| "the target was not attempted".to_string()),
        ),
    }
}

/// Upload a finished archive to the selected cloud targets (`handoff.md` §17.7, §17.8).
///
/// The caller supplies the archive it has ALREADY written, together with the identity it recorded for it.
/// That ordering is the contract's: §17.3's scan and the local archive come first, and an upload is the
/// step that a finding blocks.
pub fn upload_to_targets(
    handles: &[veloct_backup::TargetHandle],
    selection: &veloct_backup::TargetSelection,
    archive: &std::path::Path,
    archive_sha256: &str,
    archive_bytes: u64,
    created_at: &str,
    trigger: veloct_backup::BackupTrigger,
) -> Vec<TargetUploadOutcome> {
    let run = veloct_backup::run_targets(
        handles,
        selection,
        archive,
        archive_sha256,
        archive_bytes,
        created_at,
        trigger,
    );
    run.targets.iter().map(target_upload_outcome).collect()
}

/// A whole automatic run's outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoBackupOutcome {
    pub archive_path: String,
    pub archive_sha256: String,
    pub targets: Vec<TargetUploadOutcome>,
    /// Whether a secret finding needs the user's acknowledgement (§17.3).
    ///
    /// A finding does NOT fail the local archive, and it BLOCKS the upload until the user confirms. That is the
    /// contract's own ordering: lose the backup, and you have made a finding worse than the secret.
    pub needs_acknowledgement: bool,
}

impl AutoBackupOutcome {
    /// The targets that succeeded, so §17.7's "a successful target keeps its own copy" is checkable.
    pub fn succeeded(&self) -> Vec<&str> {
        self.targets
            .iter()
            .filter(|target| target.succeeded)
            .map(|target| target.target_id.as_str())
            .collect()
    }

    /// The targets that failed, whose copies are independent of the successful ones.
    ///
    /// A target that was deliberately not attempted is NOT here: it has nothing to retry, and
    /// [`Self::skipped`] is where a user finds it.
    pub fn failed(&self) -> Vec<&str> {
        self.targets
            .iter()
            .filter(|target| !target.succeeded && !target.skipped)
            .map(|target| target.target_id.as_str())
            .collect()
    }

    /// The targets that were deliberately not attempted, and the reasons.
    pub fn skipped(&self) -> Vec<&TargetUploadOutcome> {
        self.targets.iter().filter(|target| target.skipped).collect()
    }

    /// Whether EVERY target that was attempted failed. Not the same as "the backup failed": the local
    /// archive exists. A run where everything was skipped is also not "all failed" — nothing was tried.
    pub fn all_targets_failed(&self) -> bool {
        !self.targets.is_empty()
            && self
                .targets
                .iter()
                .all(|target| !target.succeeded && !target.skipped)
            && self.targets.iter().any(|target| !target.skipped)
    }

    /// Whether no target stored this backup, whether because they failed or because none was attempted.
    ///
    /// The question a caller asks before telling a user their project is off-machine. §17.7 keeps the
    /// LOCAL archive either way, so this is never "the backup failed".
    pub fn nothing_stored(&self) -> bool {
        !self.targets.is_empty() && self.succeeded().is_empty()
    }

    /// A summary in the shape §17.7 illustrates.
    pub fn summary(&self) -> String {
        let local = "本地备份 ✅";
        if self.targets.is_empty() {
            return format!("{local} · 没有云端目标");
        }
        let lines: Vec<String> = self
            .targets
            .iter()
            .map(|target| {
                let mark = if target.succeeded {
                    "✅"
                } else if target.skipped {
                    "⏭"
                } else {
                    "❌"
                };
                format!("{} {mark}", target.target_id)
            })
            .collect();
        format!("{local} · {}", lines.join(" · "))
    }
}

/// The idle period from a project's settings, or the documented default.
///
/// A zero or unreadable setting falls back rather than being honoured: an idle period of zero would back up on
/// every keystroke, which is the opposite of §17.5's intent.
pub fn idle_seconds_from_settings(project_root: &std::path::Path) -> u64 {
    let text = std::fs::read_to_string(project_root.join("data/site.json")).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("backups")
                .and_then(|backups| backups.get("idleMinutes"))
                .and_then(|minutes| minutes.as_u64())
        })
        .map(|minutes| minutes.saturating_mul(60))
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_IDLE_SECONDS)
}

/// The cloud retention from a project's settings, or the documented default.
pub fn cloud_retention_from_settings(project_root: &std::path::Path) -> usize {
    let text = std::fs::read_to_string(project_root.join("data/site.json")).unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|value| {
            value
                .get("backups")
                .and_then(|backups| backups.get("cloudRetention"))
                .and_then(|retention| retention.as_u64())
        })
        .map(|retention| retention as usize)
        .filter(|retention| *retention > 0)
        .unwrap_or(DEFAULT_CLOUD_RETENTION)
}

/// The commit a project is at, for the manifest, when it is a repository.
///
/// A backup records this so a restore can say which commit it came from; an unreadable repository is `None`
/// rather than an error, because a project that is not under Git can still be backed up.
pub fn commit_sha_of(project_root: &std::path::Path) -> Option<String> {
    if !veloct_git::change::is_repository(project_root) {
        return None;
    }
    let repository = veloct_git::change::open(project_root).ok()?;
    let head = repository.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    Some(commit.id().to_string())
}

/// Whether a manifest says the upload needs acknowledgement.
pub fn manifest_needs_acknowledgement(manifest: &BackupManifest) -> bool {
    manifest.needs_acknowledgement()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_project_never_triggers_a_backup() {
        // Section 17.5's first condition. Without it an application left open on an unchanged project would
        // write the same archive every ten minutes forever.
        let tracker = IdleTracker::new(1_000);
        assert_eq!(tracker.decide(99_999), IdleDecision::NoChange);
        assert!(!tracker.decide(99_999).is_due());
    }

    #[test]
    fn a_change_starts_a_ten_minute_timer_and_the_backup_is_due_at_the_end() {
        // Section 17.5's second and third conditions.
        let mut tracker = IdleTracker::new(1_000);
        tracker.note_change(1_000);

        // Nothing due yet.
        assert_eq!(
            tracker.decide(1_000),
            IdleDecision::Waiting {
                remaining_seconds: 600
            }
        );
        // Still waiting one second short.
        assert_eq!(
            tracker.decide(1_599),
            IdleDecision::Waiting {
                remaining_seconds: 1
            }
        );
        // Due exactly at ten minutes.
        assert_eq!(tracker.decide(1_600), IdleDecision::Due);
        assert!(tracker.decide(1_600).is_due());
        // And afterwards.
        assert!(tracker.decide(9_999).is_due());
    }

    #[test]
    fn another_change_restarts_the_timer() {
        // Section 17.5's last line: "a modification in the meantime restarts the timer". This is the part that
        // is easy to get wrong, and the part a timer-based implementation could not test quickly.
        let mut tracker = IdleTracker::new(1_000);
        tracker.note_change(1_000);

        // Five minutes in, nine minutes after the start, a new edit arrives.
        tracker.note_change(1_300);

        // The original deadline has passed, and nothing is due because the timer restarted.
        assert_eq!(
            tracker.decide(1_600),
            IdleDecision::Waiting {
                remaining_seconds: 300
            }
        );
        // Ten minutes after the SECOND edit it is due.
        assert!(tracker.decide(1_900).is_due());
    }

    #[test]
    fn a_backup_clears_the_change_flag_so_it_does_not_run_again_immediately() {
        let mut tracker = IdleTracker::new(1_000);
        tracker.note_change(1_000);
        assert!(tracker.decide(1_600).is_due());

        tracker.note_backup(1_600);
        // Nothing changed since, so no second run.
        assert_eq!(tracker.decide(1_600), IdleDecision::NoChange);
        assert_eq!(tracker.decide(99_999), IdleDecision::NoChange);

        // Until something changes.
        tracker.note_change(2_000);
        assert_eq!(
            tracker.decide(2_100),
            IdleDecision::Waiting {
                remaining_seconds: 500
            }
        );
    }

    #[test]
    fn the_idle_period_is_configurable_and_a_custom_one_is_honoured() {
        let clock = 1_000u64;
        let mut tracker = IdleTracker::new(clock).with_idle_seconds(60);
        tracker.note_change(clock);
        assert_eq!(
            tracker.decide(1_030),
            IdleDecision::Waiting {
                remaining_seconds: 30
            }
        );
        assert!(tracker.decide(1_060).is_due());
    }

    #[test]
    fn the_idle_period_is_read_from_the_settings_and_falls_back_rather_than_to_zero() {
        // An idle period of zero would back up on every keystroke, which is the opposite of section 17.5's
        // intent.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        assert_eq!(idle_seconds_from_settings(dir.path()), DEFAULT_IDLE_SECONDS);

        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "backups": { "idleMinutes": 2 } }"#,
        )
        .unwrap();
        assert_eq!(idle_seconds_from_settings(dir.path()), 120);

        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "backups": { "idleMinutes": 0 } }"#,
        )
        .unwrap();
        assert_eq!(idle_seconds_from_settings(dir.path()), DEFAULT_IDLE_SECONDS);

        std::fs::write(dir.path().join("data/site.json"), "{ not json").unwrap();
        assert_eq!(idle_seconds_from_settings(dir.path()), DEFAULT_IDLE_SECONDS);
    }

    #[test]
    fn the_cloud_retention_default_is_three_as_section_17_6_requires() {
        // The first version of this module used ten, which was wrong: section 17.6 says three per cloud target.
        // Keeping more than the contract asks is a violation rather than caution, because it fills a user's
        // cloud storage with archives they did not ask to keep.
        assert_eq!(DEFAULT_CLOUD_RETENTION, 3);
    }

    #[test]
    fn the_cloud_retention_is_read_from_the_settings_and_falls_back_rather_than_to_zero() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        assert_eq!(
            cloud_retention_from_settings(dir.path()),
            DEFAULT_CLOUD_RETENTION
        );

        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "backups": { "cloudRetention": 7 } }"#,
        )
        .unwrap();
        assert_eq!(cloud_retention_from_settings(dir.path()), 7);

        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "backups": { "cloudRetention": 0 } }"#,
        )
        .unwrap();
        assert_eq!(
            cloud_retention_from_settings(dir.path()),
            DEFAULT_CLOUD_RETENTION
        );
    }

    #[test]
    fn a_partly_successful_run_keeps_the_successful_targets() {
        // Section 17.7: "a successful target keeps its own copy; a failed target does not affect the successful
        // ones". The illustration in the contract is exactly this shape.
        let outcome = AutoBackupOutcome {
            archive_path: "a.tar.gz".to_string(),
            archive_sha256: "x".repeat(64),
            targets: vec![
                TargetUploadOutcome::ok("github", 3),
                TargetUploadOutcome::failed("s3", "the bucket refused the upload"),
            ],
            needs_acknowledgement: false,
        };

        assert_eq!(outcome.succeeded(), vec!["github"]);
        assert_eq!(outcome.failed(), vec!["s3"]);
        assert!(!outcome.all_targets_failed());
        assert!(outcome.summary().contains("本地备份 ✅"), "{}", outcome.summary());
        assert!(outcome.summary().contains("github ✅"), "{}", outcome.summary());
        assert!(outcome.summary().contains("s3 ❌"), "{}", outcome.summary());
    }

    #[test]
    fn every_target_failing_is_still_not_a_failed_backup() {
        // The local archive exists, which is the whole reason section 17.3 refuses to let a finding — or a target
        // — destroy it.
        let outcome = AutoBackupOutcome {
            archive_path: "a.tar.gz".to_string(),
            archive_sha256: "x".repeat(64),
            targets: vec![
                TargetUploadOutcome::failed("github", "auth"),
                TargetUploadOutcome::failed("s3", "network"),
            ],
            needs_acknowledgement: false,
        };
        assert!(outcome.all_targets_failed());
        assert!(outcome.succeeded().is_empty());
        assert!(outcome.summary().contains("本地备份 ✅"), "{}", outcome.summary());
    }

    #[test]
    fn a_run_with_no_cloud_targets_says_so_rather_than_listing_nothing() {
        let outcome = AutoBackupOutcome {
            archive_path: "a.tar.gz".to_string(),
            archive_sha256: "x".repeat(64),
            targets: Vec::new(),
            needs_acknowledgement: false,
        };
        assert!(!outcome.all_targets_failed(), "no targets is not all-failed");
        assert!(outcome.summary().contains("没有云端目标"), "{}", outcome.summary());
    }

    #[test]
    fn a_secret_finding_is_recorded_as_needing_acknowledgement_and_does_not_fail_the_run() {
        // Section 17.3 and 17.7: losing the local archive because of a finding would make the finding worse
        // than the secret.
        let outcome = AutoBackupOutcome {
            archive_path: "a.tar.gz".to_string(),
            archive_sha256: "x".repeat(64),
            targets: vec![TargetUploadOutcome::ok("github", 3)],
            needs_acknowledgement: true,
        };
        assert!(outcome.needs_acknowledgement);
        assert_eq!(outcome.succeeded(), vec!["github"]);
    }

    #[test]
    fn the_tracker_round_trips_through_json_because_it_must_survive_a_restart() {
        // A user who edits, closes the application and reopens it ten minutes later should still get a backup,
        // which an in-memory-only timestamp would lose.
        let mut tracker = IdleTracker::new(1_000);
        tracker.note_change(1_234);
        let json = serde_json::to_string(&tracker).unwrap();
        let restored: IdleTracker = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, tracker);
        assert_eq!(restored.decide(1_834), IdleDecision::Due);
    }

    #[test]
    fn the_decision_serialises_with_a_tag_so_a_ui_can_branch_on_it() {
        let due = serde_json::to_string(&IdleDecision::Due).unwrap();
        assert!(due.contains("\"decision\":\"due\""), "{due}");
        let waiting = serde_json::to_string(&IdleDecision::Waiting {
            remaining_seconds: 30,
        })
        .unwrap();
        assert!(waiting.contains("\"decision\":\"waiting\""), "{waiting}");
        assert!(waiting.contains("\"remaining_seconds\":30"), "{waiting}");
    }

    #[test]
    fn every_decision_describes_itself_in_the_users_terms() {
        assert!(IdleDecision::NoChange.describe().contains("没有变化"));
        let waiting = IdleDecision::Waiting {
            remaining_seconds: 300,
        };
        assert!(waiting.describe().contains('5'), "{}", waiting.describe());
        assert!(IdleDecision::Due.describe().contains("触发"), "{}", IdleDecision::Due.describe());
    }

    #[test]
    fn a_tracker_that_has_never_been_told_about_a_change_reports_no_change() {
        // The default must not start a backup on a project nobody has touched.
        assert_eq!(IdleTracker::default().decide(u64::MAX), IdleDecision::NoChange);
        assert!(!IdleTracker::default().changed_since_backup());
        assert_eq!(IdleTracker::default().idle_seconds(), DEFAULT_IDLE_SECONDS);
    }

    #[test]
    fn a_commit_sha_is_recorded_for_a_repository_and_is_absent_otherwise() {
        // A backup records the commit so a restore can say which one it came from, and a project that is not
        // under Git can still be backed up.
        let dir = tempfile::TempDir::new().unwrap();
        assert!(commit_sha_of(dir.path()).is_none());

        let repository = veloct_git::change::init(dir.path()).unwrap();
        // A repository with no commit has no sha, and reporting `None` is honest.
        assert!(commit_sha_of(dir.path()).is_none());

        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let mut index = repository.index().unwrap();
        index
            .add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("t", "t@example.invalid").unwrap();
        let sha = repository
            .commit(Some("HEAD"), &signature, &signature, "first", &tree, &[])
            .unwrap()
            .to_string();

        assert_eq!(commit_sha_of(dir.path()).as_deref(), Some(sha.as_str()));
    }

    #[test]
    fn the_remaining_time_is_reported_as_whole_minutes_rounded_up() {
        // A countdown that said "0 minutes" while still waiting would look broken.
        let remaining = IdleDecision::Waiting {
            remaining_seconds: 1,
        };
        assert!(remaining.describe().contains('1'), "{}", remaining.describe());
    }
}
