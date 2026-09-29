//! The three-axis attachment state.
//!
//! `handoff.md` §13.2.1 is explicit that an attachment's state is **three independent dimensions**,
//! not one large enum:
//!
//! ```text
//! upload_status      PENDING / UPLOADING / SUCCEEDED / FAILED
//! reference_status   NOT_APPLIED / APPLYING / APPLIED / FAILED
//! cache_status       PRESENT / CLEANED
//! ```
//!
//! The reason this matters is §13.2.2's middle case: a remote upload can SUCCEED while applying the
//! resulting URL to the Markdown FAILS. A single enum would have to pick one of those facts and lose
//! the other, and losing the upload fact means the next attempt re-uploads a file that is already
//! remote — the "blind re-upload" §13.2.2 forbids. With three axes the state is exactly
//! `UPLOADING=SUCCEEDED, REFERENCE=FAILED, CACHE=PRESENT`, and the offered action is "retry applying
//! the reference", not "upload again".

use serde::{Deserialize, Serialize};

/// A storage target's identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StorageTargetId(pub String);

impl StorageTargetId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StorageTargetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Whether the bytes have reached the remote target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum UploadStatus {
    /// In the cache pool, not yet attempted.
    Pending,
    /// An upload is in flight. Must never be bulk-deleted (`handoff.md` §13.2.4).
    Uploading,
    /// The remote object exists. Carries the information needed to retry ONLY the reference apply.
    Succeeded,
    /// The attempt failed and may be retried.
    Failed,
}

impl UploadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Uploading => "UPLOADING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
        }
    }

    /// Whether an upload may be started from here.
    ///
    /// False for `SUCCEEDED`: re-uploading a file that is already remote is the blind re-upload
    /// §13.2.2 forbids. False for `UPLOADING` too, because a second concurrent attempt is what
    /// §13.4's concurrency protection exists to prevent.
    pub fn can_start_upload(self) -> bool {
        matches!(self, Self::Pending | Self::Failed)
    }

    /// Whether this state blocks a bulk cache cleanup.
    pub fn blocks_cleanup(self) -> bool {
        matches!(self, Self::Pending | Self::Uploading)
    }

    /// Whether an upload attempt is in flight.
    pub fn is_in_flight(self) -> bool {
        matches!(self, Self::Uploading)
    }
}

/// Whether the remote URL has been written into the content.
///
/// This is an AGGREGATE over a reference set (`handoff.md` §13.2.3), which is why it is computed
/// rather than stored on its own: removing one of three references must leave the attachment
/// APPLIED, because two valid references remain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReferenceStatus {
    /// No valid reference exists right now.
    NotApplied,
    /// A reference apply is in flight.
    Applying,
    /// At least one valid reference exists.
    Applied,
    /// The most recent apply attempt failed.
    Failed,
}

impl ReferenceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotApplied => "NOT_APPLIED",
            Self::Applying => "APPLYING",
            Self::Applied => "APPLIED",
            Self::Failed => "FAILED",
        }
    }

    /// Derive the aggregate status from the reference set and the last apply attempt.
    ///
    /// The order of the checks is the whole rule:
    ///
    /// 1. An in-flight apply means `APPLYING`, whatever else is true.
    /// 2. **Any** valid reference means `APPLIED`. This is the case §13.2.3 protects: a set with one
    ///    reference removed still has the others.
    /// 3. A failed attempt with no references left means `FAILED`, so the retry is offered.
    /// 4. Otherwise there is nothing to apply.
    pub fn aggregate(reference_count: usize, apply_in_flight: bool, last_apply_failed: bool) -> Self {
        if apply_in_flight {
            return Self::Applying;
        }
        if reference_count > 0 {
            return Self::Applied;
        }
        if last_apply_failed {
            return Self::Failed;
        }
        Self::NotApplied
    }

    /// Whether the remote URL has been written somewhere.
    pub fn is_applied(self) -> bool {
        matches!(self, Self::Applied)
    }

    /// Whether retrying the apply — and NOT re-uploading — is the offered action.
    pub fn wants_retry_apply(self) -> bool {
        matches!(self, Self::Failed)
    }
}

/// Whether the cached bytes are still on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CacheStatus {
    /// The bytes are in `.attachments-cache/`.
    Present,
    /// The cache entry was cleaned up after the reference was applied.
    Cleaned,
}

impl CacheStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => "PRESENT",
            Self::Cleaned => "CLEANED",
        }
    }

    pub fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

/// The three axes together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentState {
    pub upload_status: UploadStatus,
    pub reference_status: ReferenceStatus,
    pub cache_status: CacheStatus,
}

impl AttachmentState {
    pub fn new() -> Self {
        // What a file dropped into the pool looks like: nothing attempted, nothing referenced, bytes
        // present.
        Self {
            upload_status: UploadStatus::Pending,
            reference_status: ReferenceStatus::NotApplied,
            cache_status: CacheStatus::Present,
        }
    }

    /// The state after a remote upload succeeded but the reference has not been applied yet.
    pub fn uploaded_awaiting_reference() -> Self {
        Self {
            upload_status: UploadStatus::Succeeded,
            reference_status: ReferenceStatus::NotApplied,
            cache_status: CacheStatus::Present,
        }
    }

    /// The state §13.2.2 describes: remote SUCCEEDED but the apply FAILED.
    ///
    /// The cache stays PRESENT on purpose: the bytes are still needed if the user decides to change
    /// the target rather than retry, and the remote object must not be re-uploaded.
    pub fn uploaded_reference_failed() -> Self {
        Self {
            upload_status: UploadStatus::Succeeded,
            reference_status: ReferenceStatus::Failed,
            cache_status: CacheStatus::Present,
        }
    }

    /// The terminal success: remote, referenced, cache released.
    pub fn applied_and_cleaned() -> Self {
        Self {
            upload_status: UploadStatus::Succeeded,
            reference_status: ReferenceStatus::Applied,
            cache_status: CacheStatus::Cleaned,
        }
    }

    /// Whether the remote upload has already happened, so it must NOT be repeated.
    pub fn must_not_reupload(&self) -> bool {
        matches!(self.upload_status, UploadStatus::Succeeded | UploadStatus::Uploading)
    }

    /// The single action the UI should offer, derived from the three axes.
    ///
    /// Returning a named action rather than leaving the UI to read three fields is what keeps the
    /// §13.2.2 rule in one place: after a failed apply, the action is `RetryApply`, never `Upload`.
    pub fn next_action(&self) -> NextAction {
        if self.upload_status.is_in_flight() {
            return NextAction::Wait;
        }
        if self.reference_status == ReferenceStatus::Applying {
            return NextAction::Wait;
        }
        match self.upload_status {
            UploadStatus::Succeeded => match self.reference_status {
                ReferenceStatus::Failed => NextAction::RetryApply,
                ReferenceStatus::Applied => {
                    if self.cache_status.is_present() {
                        NextAction::CleanCache
                    } else {
                        NextAction::Nothing
                    }
                }
                _ => NextAction::ApplyReference,
            },
            UploadStatus::Pending | UploadStatus::Failed => NextAction::Upload,
            UploadStatus::Uploading => NextAction::Wait,
        }
    }
}

impl Default for AttachmentState {
    fn default() -> Self {
        Self::new()
    }
}

/// The single next action for an attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    /// Upload the bytes.
    Upload,
    /// Write the already-known remote URL into the content. Never an upload.
    ApplyReference,
    /// Retry an apply that failed. Never an upload.
    RetryApply,
    /// Release the cache entry.
    CleanCache,
    /// An upload or apply is in flight.
    Wait,
    /// Nothing to do.
    Nothing,
}

/// The inputs §13.2.4 lists for deciding whether a cache file may be bulk-cleaned.
///
/// Passed as a struct rather than as four booleans, because four positional booleans are exactly the
/// kind of call site where one gets swapped and a PENDING file gets deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrphanEvidence {
    /// Valid Markdown AST references currently pointing at this attachment.
    pub reference_count: usize,
    /// Whether the attachment is in the queue of references still to be applied.
    pub in_apply_queue: bool,
    /// The upload axis.
    pub upload_status: UploadStatus,
    /// Whether a resume or retry task is waiting for this attachment.
    pub has_pending_task: bool,
}

/// Whether an attachment is a safe orphan: the ONLY condition under which §13.2.4 allows bulk
/// cleanup.
///
/// All four conditions must hold. The function is named for the property rather than for the
/// conditions so that a caller cannot read it as "probably fine".
pub fn is_safe_orphan(evidence: OrphanEvidence, reference_status: ReferenceStatus, cache_status: CacheStatus) -> bool {
    // The cache entry must exist to be cleaned.
    if !cache_status.is_present() {
        return false;
    }
    // 1. No valid Markdown AST reference.
    if evidence.reference_count > 0 {
        return false;
    }
    // 2. Not waiting in the apply queue.
    if evidence.in_apply_queue {
        return false;
    }
    // 3. Not PENDING or UPLOADING.
    if evidence.upload_status.blocks_cleanup() {
        return false;
    }
    // 4. No resumable or retryable task.
    if evidence.has_pending_task {
        return false;
    }
    // A FAILED apply is a state the user asked to be able to retry, so it is not an orphan either.
    if reference_status.wants_retry_apply() {
        return false;
    }
    // handoff.md section 13.2.4: `SUCCEEDED + NOT_APPLIED` and `SUCCEEDED + FAILED` must go through
    // an explicit manual confirmation. A successful upload whose URL was never written anywhere is
    // exactly that: the bytes exist remotely, but nothing in the project points at them, so sweeping
    // the cache would leave the user with an orphaned object and no record of where it is.
    if evidence.upload_status == UploadStatus::Succeeded
        && !reference_status.is_applied()
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(reference_count: usize) -> OrphanEvidence {
        OrphanEvidence {
            reference_count,
            in_apply_queue: false,
            upload_status: UploadStatus::Succeeded,
            has_pending_task: false,
        }
    }

    #[test]
    fn a_new_attachment_is_pending_with_no_reference_and_present_bytes() {
        let state = AttachmentState::new();
        assert_eq!(state.upload_status, UploadStatus::Pending);
        assert_eq!(state.reference_status, ReferenceStatus::NotApplied);
        assert_eq!(state.cache_status, CacheStatus::Present);
        assert_eq!(state.next_action(), NextAction::Upload);
    }

    #[test]
    fn the_three_axes_are_independent_because_upload_can_succeed_while_apply_fails() {
        // handoff.md section 13.2.2: this is the state a single enum cannot express.
        let state = AttachmentState::uploaded_reference_failed();
        assert_eq!(state.upload_status, UploadStatus::Succeeded);
        assert_eq!(state.reference_status, ReferenceStatus::Failed);
        assert_eq!(state.cache_status, CacheStatus::Present);
    }

    #[test]
    fn after_a_failed_apply_the_offered_action_is_retry_apply_and_never_a_re_upload() {
        // handoff.md section 13.2.2: "must not blindly re-upload the same file".
        let state = AttachmentState::uploaded_reference_failed();
        assert_eq!(state.next_action(), NextAction::RetryApply);
        assert_ne!(state.next_action(), NextAction::Upload);
        assert!(state.must_not_reupload());
    }

    #[test]
    fn a_failed_upload_may_be_retried() {
        let state = AttachmentState {
            upload_status: UploadStatus::Failed,
            reference_status: ReferenceStatus::NotApplied,
            cache_status: CacheStatus::Present,
        };
        assert_eq!(state.next_action(), NextAction::Upload);
        assert!(!state.must_not_reupload());
        assert!(state.upload_status.can_start_upload());
    }

    #[test]
    fn a_succeeded_upload_may_not_be_started_again() {
        // A second attempt would create a duplicate remote object.
        assert!(!UploadStatus::Succeeded.can_start_upload());
        assert!(!UploadStatus::Uploading.can_start_upload());
        assert!(UploadStatus::Pending.can_start_upload());
        assert!(UploadStatus::Failed.can_start_upload());
    }

    #[test]
    fn an_applied_attachment_offers_a_cache_clean_and_then_nothing() {
        let state = AttachmentState::applied_and_cleaned();
        assert_eq!(state.next_action(), NextAction::Nothing);
        assert_eq!(state.cache_status, CacheStatus::Cleaned);

        let still_cached = AttachmentState {
            cache_status: CacheStatus::Present,
            ..state
        };
        assert_eq!(still_cached.next_action(), NextAction::CleanCache);
    }

    #[test]
    fn an_in_flight_upload_or_apply_offers_a_wait_rather_than_a_second_action() {
        let uploading = AttachmentState {
            upload_status: UploadStatus::Uploading,
            ..AttachmentState::new()
        };
        assert_eq!(uploading.next_action(), NextAction::Wait);

        let applying = AttachmentState {
            upload_status: UploadStatus::Succeeded,
            reference_status: ReferenceStatus::Applying,
            cache_status: CacheStatus::Present,
        };
        assert_eq!(applying.next_action(), NextAction::Wait);
    }

    #[test]
    fn a_succeeded_upload_with_no_reference_yet_offers_an_apply_not_an_upload() {
        let state = AttachmentState::uploaded_awaiting_reference();
        assert_eq!(state.next_action(), NextAction::ApplyReference);
        assert_ne!(state.next_action(), NextAction::Upload);
    }

    #[test]
    fn the_reference_status_is_the_aggregate_over_the_set() {
        // handoff.md section 13.2.3: removing ONE of several references must leave it APPLIED.
        assert_eq!(
            ReferenceStatus::aggregate(3, false, false),
            ReferenceStatus::Applied
        );
        assert_eq!(
            ReferenceStatus::aggregate(0, false, false),
            ReferenceStatus::NotApplied
        );
        // The case that matters: two references removed, one left, still applied.
        assert_eq!(
            ReferenceStatus::aggregate(1, false, false),
            ReferenceStatus::Applied
        );
        // Only when the LAST one goes does it become unreferenced.
        assert_eq!(
            ReferenceStatus::aggregate(0, false, false),
            ReferenceStatus::NotApplied
        );
    }

    #[test]
    fn an_in_flight_apply_wins_over_the_reference_count() {
        assert_eq!(
            ReferenceStatus::aggregate(5, true, false),
            ReferenceStatus::Applying
        );
        assert_eq!(
            ReferenceStatus::aggregate(0, true, false),
            ReferenceStatus::Applying
        );
    }

    #[test]
    fn a_failure_with_no_references_left_is_reported_as_failed() {
        assert_eq!(
            ReferenceStatus::aggregate(0, false, true),
            ReferenceStatus::Failed
        );
        assert!(ReferenceStatus::Failed.wants_retry_apply());
        // A failure that left a reference in place is APPLIED, because a valid reference exists.
        assert_eq!(
            ReferenceStatus::aggregate(1, false, true),
            ReferenceStatus::Applied
        );
    }

    #[test]
    fn every_status_has_a_stable_documented_spelling() {
        // The names in handoff.md section 13.2.1 are the wire format.
        for (status, text) in [
            (UploadStatus::Pending, "PENDING"),
            (UploadStatus::Uploading, "UPLOADING"),
            (UploadStatus::Succeeded, "SUCCEEDED"),
            (UploadStatus::Failed, "FAILED"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(serde_json::to_string(&status).unwrap(), format!("\"{text}\""));
        }
        for (status, text) in [
            (ReferenceStatus::NotApplied, "NOT_APPLIED"),
            (ReferenceStatus::Applying, "APPLYING"),
            (ReferenceStatus::Applied, "APPLIED"),
            (ReferenceStatus::Failed, "FAILED"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(serde_json::to_string(&status).unwrap(), format!("\"{text}\""));
        }
        for (status, text) in [
            (CacheStatus::Present, "PRESENT"),
            (CacheStatus::Cleaned, "CLEANED"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(serde_json::to_string(&status).unwrap(), format!("\"{text}\""));
        }
    }

    #[test]
    fn a_clean_orphan_is_safe_to_clean() {
        // A failed upload with no reference and no task is the case with nothing left to lose.
        let evidence = OrphanEvidence {
            upload_status: UploadStatus::Failed,
            ..evidence(0)
        };
        assert!(is_safe_orphan(
            evidence,
            ReferenceStatus::NotApplied,
            CacheStatus::Present
        ));
    }

    #[test]
    fn a_referenced_attachment_is_never_an_orphan() {
        assert!(!is_safe_orphan(
            evidence(1),
            ReferenceStatus::Applied,
            CacheStatus::Present
        ));
    }

    #[test]
    fn a_pending_or_uploading_attachment_is_never_bulk_deleted() {
        // handoff.md section 13.2.4 states this explicitly.
        for status in [UploadStatus::Pending, UploadStatus::Uploading] {
            let evidence = OrphanEvidence {
                upload_status: status,
                ..evidence(0)
            };
            assert!(
                !is_safe_orphan(evidence, ReferenceStatus::NotApplied, CacheStatus::Present),
                "{status:?} must not be a safe orphan"
            );
        }
    }

    #[test]
    fn an_attachment_in_the_apply_queue_is_not_an_orphan() {
        let evidence = OrphanEvidence {
            in_apply_queue: true,
            ..evidence(0)
        };
        assert!(!is_safe_orphan(
            evidence,
            ReferenceStatus::NotApplied,
            CacheStatus::Present
        ));
    }

    #[test]
    fn an_attachment_with_a_resumable_task_is_not_an_orphan() {
        let evidence = OrphanEvidence {
            has_pending_task: true,
            ..evidence(0)
        };
        assert!(!is_safe_orphan(
            evidence,
            ReferenceStatus::NotApplied,
            CacheStatus::Present
        ));
    }

    #[test]
    fn a_failed_apply_is_not_an_orphan_because_the_user_must_be_able_to_retry() {
        // handoff.md section 13.2.4: SUCCEEDED + FAILED needs explicit confirmation, so it is not
        // swept up by a bulk clean.
        assert!(!is_safe_orphan(
            evidence(0),
            ReferenceStatus::Failed,
            CacheStatus::Present
        ));
    }

    #[test]
    fn an_already_cleaned_entry_cannot_be_cleaned_again() {
        assert!(!is_safe_orphan(
            evidence(0),
            ReferenceStatus::NotApplied,
            CacheStatus::Cleaned
        ));
    }

    #[test]
    fn a_succeeded_but_unapplied_attachment_is_not_a_safe_orphan() {
        // handoff.md section 13.2.4: `SUCCEEDED + NOT_APPLIED` needs explicit manual confirmation.
        // The bytes exist remotely but nothing in the project points at them, so sweeping the cache
        // would leave an orphaned object with no recorded whereabouts.
        assert!(!is_safe_orphan(
            evidence(0),
            ReferenceStatus::NotApplied,
            CacheStatus::Present
        ));
        // The same clause covers SUCCEEDED + FAILED.
        assert!(!is_safe_orphan(
            evidence(0),
            ReferenceStatus::Failed,
            CacheStatus::Present
        ));
    }

    #[test]
    fn a_failed_upload_with_no_reference_is_a_safe_orphan() {
        // Nothing is remote, nothing references it, and no task is waiting: there is no reason to
        // keep the bytes.
        let evidence = OrphanEvidence {
            upload_status: UploadStatus::Failed,
            ..evidence(0)
        };
        assert!(is_safe_orphan(
            evidence,
            ReferenceStatus::NotApplied,
            CacheStatus::Present
        ));
    }

    #[test]
    fn the_state_round_trips_through_json_for_the_frontend() {
        let state = AttachmentState::uploaded_reference_failed();
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"upload_status\":\"SUCCEEDED\""), "{json}");
        assert!(json.contains("\"reference_status\":\"FAILED\""), "{json}");
        assert!(json.contains("\"cache_status\":\"PRESENT\""), "{json}");
        assert_eq!(
            serde_json::from_str::<AttachmentState>(&json).unwrap(),
            state
        );
    }

    #[test]
    fn a_storage_target_id_round_trips_and_renders_readably() {
        let id = StorageTargetId::new("s3-main");
        assert_eq!(id.as_str(), "s3-main");
        assert_eq!(id.to_string(), "s3-main");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"s3-main\"");
    }
}
