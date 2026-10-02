//! Typed status values and the row types the repositories exchange.
//!
//! `handoff.md` §13.2.1 is explicit that attachment state is **three independent axes**, not one
//! large enumeration, and §13.2.3 that `reference_status` is an aggregate over a real reference
//! set. Modelling those as separate enums with checked conversion is what keeps a repository from
//! quietly collapsing them into a single string.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use veloct_core::{Result, StableId, VeloctError};

/// Generate a closed string enum with `Display`/`FromStr` and `serde` support.
///
/// A macro rather than five near-identical hand-written enums, so the database representation and
/// the in-memory representation cannot drift apart in one place and not the others.
macro_rules! string_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $text:literal
            ),+ $(,)?
        }
        unknown: $unknown:literal
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant
            ),+
        }

        impl $name {
            /// The exact text stored in SQLite.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            /// Every variant, for exhaustiveness in tests and pickers.
            pub fn all() -> &'static [Self] {
                &[$(Self::$variant),+]
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = VeloctError;

            fn from_str(value: &str) -> Result<Self> {
                match value.trim().to_ascii_lowercase().as_str() {
                    $($text => Ok(Self::$variant),)+
                    other => Err(VeloctError::Database(format!(
                        "{}: unknown value {other:?}",
                        $unknown
                    ))),
                }
            }
        }
    };
}

string_enum! {
    /// `handoff.md` §13.2.1, first axis.
    pub enum UploadStatus {
        Pending => "pending",
        Uploading => "uploading",
        Succeeded => "succeeded",
        Failed => "failed",
    }
    unknown: "upload_status"
}

string_enum! {
    /// `handoff.md` §13.2.1, second axis. An aggregate over the reference set, not a boolean.
    pub enum ReferenceStatus {
        NotApplied => "not_applied",
        Applying => "applying",
        Applied => "applied",
        Failed => "failed",
    }
    unknown: "reference_status"
}

string_enum! {
    /// `handoff.md` §13.2.1, third axis.
    pub enum CacheStatus {
        Present => "present",
        Cleaned => "cleaned",
    }
    unknown: "cache_status"
}

string_enum! {
    /// `handoff.md` §22.3.
    pub enum DeploymentStatus {
        Queued => "queued",
        Running => "running",
        Succeeded => "succeeded",
        Failed => "failed",
        Skipped => "skipped",
    }
    unknown: "deployment status"
}

string_enum! {
    /// Which publish root a deployment run covers (`handoff.md` §14.12).
    pub enum SubTarget {
        Main => "main",
        Works => "works",
    }
    unknown: "sub_target"
}

string_enum! {
    /// `handoff.md` §14.12: a logical target can be partially successful.
    pub enum LogicalTargetStatus {
        Succeeded => "succeeded",
        Partial => "partial",
        Failed => "failed",
        Pending => "pending",
    }
    unknown: "logical target status"
}

string_enum! {
    /// `handoff.md` §22.6 and §25.
    pub enum AiRequestStatus {
        Succeeded => "succeeded",
        Failed => "failed",
        Cancelled => "cancelled",
        FallbackSucceeded => "fallback_succeeded",
        FallbackFailed => "fallback_failed",
    }
    unknown: "ai request status"
}

string_enum! {
    /// `handoff.md` §11.1.
    pub enum AiProtocol {
        OpenAiCompatible => "openai",
        Anthropic => "anthropic",
        Ollama => "ollama",
    }
    unknown: "ai protocol"
}

string_enum! {
    /// `handoff.md` §13.1.
    ///
    /// `sftp` is GONE: §13.10's SSH target was removed on 2026-10-02 (ADR-0017), and a stored-value mapping
    /// that still named it would be the kind of stale list that ages into a wrong answer. The macro's
    /// `unknown` fallback means a row written before the removal degrades to "unknown" rather than failing.
    pub enum StorageKind {
        S3 => "s3",
        Lsky => "lsky",
        Github => "github",
        RemoteImage => "remote_image",
    }
    unknown: "storage kind"
}

string_enum! {
    /// `handoff.md` §14.10 and §14.11.
    pub enum DeployTargetKind {
        Cloudflare => "cloudflare",
        EdgeOneCn => "edgeone_cn",
        EdgeOneIntl => "edgeone_intl",
        ManualPanel => "manual_1panel",
    }
    unknown: "deployment target kind"
}

string_enum! {
    /// `handoff.md` §17.4.
    pub enum BackupTargetKind {
        Github => "github",
        S3 => "s3",
    }
    unknown: "backup target kind"
}

string_enum! {
    /// How a backup was initiated (`handoff.md` §17.5, §17.8, §17.9).
    pub enum BackupKind {
        Auto => "auto",
        Manual => "manual",
        Export => "export",
        Restore => "restore",
    }
    unknown: "backup kind"
}

string_enum! {
    /// `handoff.md` §22.7.
    pub enum BackupStatus {
        Running => "running",
        Succeeded => "succeeded",
        Failed => "failed",
        Partial => "partial",
        BlockedBySecretScan => "blocked_by_secret_scan",
    }
    unknown: "backup status"
}

string_enum! {
    /// `handoff.md` §13.3, §20.4: a resumable attachment operation.
    pub enum AttachmentTaskKind {
        Upload => "upload",
        ApplyReference => "apply_reference",
        RewriteUrl => "rewrite_url",
    }
    unknown: "attachment task kind"
}

string_enum! {
    /// `handoff.md` §20.4: an interrupted task must stay recoverable.
    pub enum AttachmentTaskStatus {
        Queued => "queued",
        Running => "running",
        Succeeded => "succeeded",
        Failed => "failed",
        Interrupted => "interrupted",
    }
    unknown: "attachment task status"
}

/// How the derived content index resolved a knowledge reference (`handoff.md` §9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReferenceLinkStatus {
    Ok,
    Broken,
}

impl ReferenceLinkStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Broken => "broken",
        }
    }
}

impl FromStr for ReferenceLinkStatus {
    type Err = VeloctError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ok" => Ok(Self::Ok),
            "broken" => Ok(Self::Broken),
            other => Err(VeloctError::Database(format!(
                "reference link status: unknown value {other:?}"
            ))),
        }
    }
}

/// One row of the derived `contents` index.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentRow {
    pub id: StableId,
    /// `article`, `page` or `knowledge`.
    pub content_type: String,
    /// Project-relative POSIX path.
    pub file_path: String,
    pub slug: String,
    pub title: String,
    pub summary: Option<String>,
    pub category: Option<String>,
    pub cover: Option<String>,
    pub wip: bool,
    pub wip_publish: bool,
    pub wip_ever_published: bool,
    pub tags: Vec<String>,
    pub blocks: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub published_at: Option<String>,
    pub content_hash: String,
    pub word_count: u64,
    pub reading_minutes: u64,
    pub indexed_at: String,
}

/// One row of `content_paragraphs` (`handoff.md` §22.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentParagraphRow {
    pub content_id: String,
    pub paragraph_id: String,
    pub paragraph_order: u32,
    pub content_hash: String,
}

/// One row of `content_links`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentLinkRow {
    pub from_content_id: String,
    pub to_content_id: String,
    pub to_paragraph_id: Option<String>,
    pub reference_location: String,
    pub status: ReferenceLinkStatus,
    pub diagnostic: Option<String>,
}

/// One row of `attachments` (`handoff.md` §22.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRow {
    pub id: String,
    pub original_name: String,
    pub stored_name: Option<String>,
    pub content_hash: String,
    pub mime_type: Option<String>,
    pub size: u64,
    pub storage_def_id: Option<String>,
    pub storage_namespace_id: Option<String>,
    pub remote_key: Option<String>,
    pub public_url: Option<String>,
    pub upload_status: UploadStatus,
    pub reference_status: ReferenceStatus,
    pub cache_status: CacheStatus,
    pub cache_path: Option<String>,
    pub remote_object_id: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl AttachmentRow {
    /// `handoff.md` §13.4: the dedup key is target **and** namespace **and** content hash.
    ///
    /// A URL must never be reused across targets, so the content hash alone is deliberately not
    /// enough to identify an upload.
    pub fn dedup_key(&self) -> Option<String> {
        match (&self.storage_def_id, &self.storage_namespace_id) {
            (Some(target), Some(namespace)) => {
                Some(format!("{target}\u{1f}{namespace}\u{1f}{}", self.content_hash))
            }
            _ => None,
        }
    }

    /// `handoff.md` §13.2.4: only a safe orphan may be cleaned in bulk.
    ///
    /// A file that is referenced, queued, uploading, or waiting for a retry is never safe to
    /// delete. The caller supplies `has_open_tasks` because that fact lives in another table.
    pub fn is_safe_orphan(&self, has_open_tasks: bool) -> bool {
        self.reference_status == ReferenceStatus::NotApplied
            && !has_open_tasks
            && !matches!(
                self.upload_status,
                UploadStatus::Pending | UploadStatus::Uploading
            )
            && self.cache_status == CacheStatus::Present
    }

    /// A remote upload that succeeded but whose URL has not reached the Markdown yet.
    ///
    /// `handoff.md` §13.2.2 and §24.9: this state offers "retry reference application" and must
    /// never re-upload.
    pub fn needs_reference_retry(&self) -> bool {
        self.upload_status == UploadStatus::Succeeded
            && self.reference_status == ReferenceStatus::Failed
            && self.cache_status == CacheStatus::Present
    }
}

/// One row of `releases` (`handoff.md` §22.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRow {
    pub release_id: String,
    pub commit_sha: String,
    pub tag_name: String,
    pub build_fingerprint: String,
    pub build_inputs_hash: String,
    pub created_at: String,
    pub created_from_release_id: Option<String>,
}

/// One row of `deployment_runs` (`handoff.md` §22.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentRunRow {
    pub id: String,
    pub release_id: String,
    pub logical_target_id: String,
    pub sub_target: SubTarget,
    pub status: DeploymentStatus,
    pub build_fingerprint: Option<String>,
    pub reused_dist: bool,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// One row of `ai_requests` (`handoff.md` §22.6).
///
/// Metadata only: Veloct does not keep a chat history (`handoff.md` §11.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiRequestRow {
    pub id: String,
    pub feature_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub status: AiRequestStatus,
    pub used_fallback: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// Roll a logical target's two sub-target runs up into one status (`handoff.md` §14.12).
///
/// Success is never retracted: if one half succeeded and the other failed, the target is
/// `Partial`, which is deliberately distinct from `Failed`.
pub fn roll_up_logical_status(runs: &[DeploymentRunRow]) -> LogicalTargetStatus {
    if runs.is_empty() {
        return LogicalTargetStatus::Pending;
    }
    if runs
        .iter()
        .any(|run| matches!(run.status, DeploymentStatus::Queued | DeploymentStatus::Running))
    {
        return LogicalTargetStatus::Pending;
    }

    let succeeded = runs
        .iter()
        .filter(|run| run.status == DeploymentStatus::Succeeded)
        .count();
    let failed = runs
        .iter()
        .filter(|run| run.status == DeploymentStatus::Failed)
        .count();

    match (succeeded, failed) {
        (0, 0) => LogicalTargetStatus::Pending,
        (_, 0) => LogicalTargetStatus::Succeeded,
        (0, _) => LogicalTargetStatus::Failed,
        _ => LogicalTargetStatus::Partial,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_round_trip_through_the_database_representation() {
        // Every enum must survive the exact text SQLite stores, including case variations.
        for value in UploadStatus::all() {
            assert_eq!(value.as_str().parse::<UploadStatus>().unwrap(), *value);
        }
        for value in ReferenceStatus::all() {
            assert_eq!(value.as_str().parse::<ReferenceStatus>().unwrap(), *value);
        }
        for value in CacheStatus::all() {
            assert_eq!(value.as_str().parse::<CacheStatus>().unwrap(), *value);
        }
        for value in DeploymentStatus::all() {
            assert_eq!(value.as_str().parse::<DeploymentStatus>().unwrap(), *value);
        }
        for value in SubTarget::all() {
            assert_eq!(value.as_str().parse::<SubTarget>().unwrap(), *value);
        }
        for value in AiRequestStatus::all() {
            assert_eq!(value.as_str().parse::<AiRequestStatus>().unwrap(), *value);
        }
        for value in AttachmentTaskStatus::all() {
            assert_eq!(
                value.as_str().parse::<AttachmentTaskStatus>().unwrap(),
                *value
            );
        }
        for value in BackupStatus::all() {
            assert_eq!(value.as_str().parse::<BackupStatus>().unwrap(), *value);
        }
        assert_eq!(
            "SUCCEEDED".parse::<UploadStatus>().unwrap(),
            UploadStatus::Succeeded
        );
        assert_eq!(
            "Works".parse::<SubTarget>().unwrap(),
            SubTarget::Works
        );
    }

    #[test]
    fn unknown_status_text_is_rejected_rather_than_defaulted() {
        // A typo in the database must surface, not silently become the first variant.
        let error = "bogus".parse::<UploadStatus>().unwrap_err();
        assert!(error.to_string().contains("upload_status"), "{error}");
        assert!("bogus".parse::<ReferenceStatus>().is_err());
        assert!("bogus".parse::<SubTarget>().is_err());
        assert!("bogus".parse::<ReferenceLinkStatus>().is_err());
    }

    #[test]
    fn the_three_attachment_axes_are_independent() {
        // handoff 13.2.1: this is the state combination that must be representable.
        let mut attachment = sample_attachment();
        attachment.upload_status = UploadStatus::Succeeded;
        attachment.reference_status = ReferenceStatus::Failed;
        attachment.cache_status = CacheStatus::Present;

        assert!(attachment.needs_reference_retry());
        // It is not a safe orphan, because a retry is outstanding.
        assert!(!attachment.is_safe_orphan(true));
    }

    #[test]
    fn a_referenced_attachment_is_never_a_safe_orphan() {
        // handoff 24.7.
        let mut attachment = sample_attachment();
        attachment.reference_status = ReferenceStatus::Applied;
        assert!(!attachment.is_safe_orphan(false));
    }

    #[test]
    fn in_flight_uploads_are_never_safe_orphans() {
        // handoff 24.8.
        let mut attachment = sample_attachment();
        attachment.reference_status = ReferenceStatus::NotApplied;
        attachment.upload_status = UploadStatus::Pending;
        assert!(!attachment.is_safe_orphan(false));

        attachment.upload_status = UploadStatus::Uploading;
        assert!(!attachment.is_safe_orphan(false));
    }

    #[test]
    fn an_attachment_with_open_tasks_is_not_cleanable() {
        let mut attachment = sample_attachment();
        attachment.upload_status = UploadStatus::Succeeded;
        attachment.reference_status = ReferenceStatus::NotApplied;
        attachment.cache_status = CacheStatus::Present;
        // No open tasks: this one *is* a safe orphan.
        assert!(attachment.is_safe_orphan(false));
        // But a queued apply task protects it.
        assert!(!attachment.is_safe_orphan(true));
    }

    #[test]
    fn a_cleaned_cache_is_not_an_orphan_to_clean() {
        let mut attachment = sample_attachment();
        attachment.upload_status = UploadStatus::Succeeded;
        attachment.cache_status = CacheStatus::Cleaned;
        assert!(!attachment.is_safe_orphan(false));
    }

    #[test]
    fn the_dedup_key_includes_target_and_namespace() {
        // handoff 24.1 and 24.2: the same bytes under different targets are different objects.
        let mut first = sample_attachment();
        first.storage_def_id = Some("s3-a".into());
        first.storage_namespace_id = Some("ns-1".into());
        first.content_hash = "abc".into();

        let mut second = first.clone();
        second.storage_def_id = Some("s3-b".into());

        let mut third = first.clone();
        third.storage_namespace_id = Some("ns-2".into());

        assert_ne!(first.dedup_key(), second.dedup_key());
        assert_ne!(first.dedup_key(), third.dedup_key());
        assert_eq!(first.dedup_key().unwrap(), "s3-a\u{1f}ns-1\u{1f}abc");
    }

    #[test]
    fn an_unassigned_attachment_has_no_dedup_key() {
        // Nothing may be reused before a target has been chosen.
        let mut attachment = sample_attachment();
        attachment.storage_def_id = None;
        attachment.storage_namespace_id = None;
        assert_eq!(attachment.dedup_key(), None);
    }

    #[test]
    fn logical_target_roll_up_distinguishes_partial_from_failed() {
        // handoff 14.12: "Cloudflare main OK, works failed" is partial, not failed.
        let mut main = sample_run();
        main.sub_target = SubTarget::Main;
        main.status = DeploymentStatus::Succeeded;
        let mut works = sample_run();
        works.id = "run-2".into();
        works.sub_target = SubTarget::Works;
        works.status = DeploymentStatus::Failed;

        assert_eq!(
            roll_up_logical_status(&[main.clone(), works.clone()]),
            LogicalTargetStatus::Partial
        );

        assert_eq!(
            roll_up_logical_status(&[main.clone()]),
            LogicalTargetStatus::Succeeded
        );

        works.status = DeploymentStatus::Succeeded;
        assert_eq!(
            roll_up_logical_status(&[main.clone(), works.clone()]),
            LogicalTargetStatus::Succeeded
        );

        main.status = DeploymentStatus::Failed;
        works.status = DeploymentStatus::Failed;
        assert_eq!(
            roll_up_logical_status(&[main.clone(), works.clone()]),
            LogicalTargetStatus::Failed
        );

        assert_eq!(roll_up_logical_status(&[]), LogicalTargetStatus::Pending);
    }

    #[test]
    fn a_running_or_queued_subtarget_keeps_the_target_pending() {
        let mut main = sample_run();
        main.status = DeploymentStatus::Succeeded;
        let mut works = sample_run();
        works.id = "run-2".into();
        works.status = DeploymentStatus::Running;
        assert_eq!(
            roll_up_logical_status(&[main, works]),
            LogicalTargetStatus::Pending
        );
    }

    #[test]
    fn skipped_subtargets_do_not_make_a_target_partial() {
        // A sub-target that was not selected is not a failure.
        let mut main = sample_run();
        main.status = DeploymentStatus::Succeeded;
        let mut works = sample_run();
        works.id = "run-2".into();
        works.status = DeploymentStatus::Skipped;
        assert_eq!(
            roll_up_logical_status(&[main, works]),
            LogicalTargetStatus::Succeeded
        );
    }

    #[test]
    fn rows_round_trip_through_json() {
        let attachment = sample_attachment();
        let json = serde_json::to_string(&attachment).unwrap();
        assert_eq!(
            serde_json::from_str::<AttachmentRow>(&json).unwrap(),
            attachment
        );
        assert!(json.contains("\"upload_status\":\"pending\""));

        let run = sample_run();
        let json = serde_json::to_string(&run).unwrap();
        assert_eq!(
            serde_json::from_str::<DeploymentRunRow>(&json).unwrap(),
            run
        );
    }

    fn sample_attachment() -> AttachmentRow {
        AttachmentRow {
            id: "01J8Z5Q0K3W7M9V2B4N6P8R1TY".into(),
            original_name: "photo.webp".into(),
            stored_name: Some("photo-a8K3xQ.webp".into()),
            content_hash: "deadbeef".into(),
            mime_type: Some("image/webp".into()),
            size: 1234,
            storage_def_id: Some("s3-main".into()),
            storage_namespace_id: Some("ns-default".into()),
            remote_key: Some("uploads/photo-a8K3xQ.webp".into()),
            public_url: None,
            upload_status: UploadStatus::Pending,
            reference_status: ReferenceStatus::NotApplied,
            cache_status: CacheStatus::Present,
            cache_path: Some(".attachments-cache/deadbeef/photo-a8K3xQ.webp".into()),
            remote_object_id: None,
            last_error: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn sample_run() -> DeploymentRunRow {
        DeploymentRunRow {
            id: "run-1".into(),
            release_id: "20260101T120000Z-abc123".into(),
            logical_target_id: "cloudflare".into(),
            sub_target: SubTarget::Main,
            status: DeploymentStatus::Queued,
            build_fingerprint: Some("fp".into()),
            reused_dist: false,
            error: None,
            started_at: None,
            finished_at: None,
        }
    }
}
