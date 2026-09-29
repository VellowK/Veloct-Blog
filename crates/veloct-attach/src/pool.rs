//! The local attachment cache pool.
//!
//! `handoff.md` §13.2: **every** local file upload enters the pool first. §13.9 requires two cleanup
//! paths — a bulk sweep of safe orphans and a manual selection — and §13.2.4 is explicit that the
//! bulk path may only take safe orphans while `PENDING`/`UPLOADING` files are never bulk-deleted and
//! `SUCCEEDED + NOT_APPLIED` / `SUCCEEDED + FAILED` files need an explicit manual confirmation.
//!
//! That asymmetry is why the cleanup API takes a [`CleanupRequest`] rather than a list of ids: the
//! request has to say WHICH path the caller is on, so "delete everything with no references" cannot
//! be written by accident and cannot sweep up a file whose upload is in flight.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

use crate::naming::{is_safe_file_name, remote_file_name, DedupKey};
use crate::reference::ReferenceIndex;
use crate::state::{
    is_safe_orphan, AttachmentState, CacheStatus, NextAction, OrphanEvidence, ReferenceStatus,
    StorageTargetId, UploadStatus,
};

/// The directory a project's cache pool lives in.
pub const CACHE_DIRECTORY: &str = ".attachments-cache";

/// An attachment's identity.
///
/// Deliberately NOT the content hash: two attachments with the same bytes on different targets are
/// two records (that is what §13.4's dedup key is for), and an id that WAS the hash would make the
/// records indistinguishable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AttachmentId(pub String);

impl AttachmentId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A queued reference-apply task.
///
/// §13.2.4 makes the presence of one of these a reason a cache file is NOT a safe orphan, so the
/// task is a first-class value rather than a boolean on the attachment: a boolean would lose which
/// document the apply was for, and the retry would then have nothing to retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceTask {
    pub attachment_id: AttachmentId,
    /// The document the URL should be written into.
    pub content_id: String,
    /// The remote URL to write, once the upload has produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// How many attempts have been made, so a UI can show a stalled task.
    pub attempts: u32,
}

/// One attachment in the pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: AttachmentId,
    /// The file name the user supplied, which the remote name is derived from.
    pub original_name: String,
    /// The content hash of the bytes.
    pub content_hash: String,
    pub size_bytes: u64,
    /// The cache file's name inside the cache directory, which is
    /// `<sha256>/<original-name-with-suffix>` per `docs/spec/01-data-model.md` §1.
    pub cache_path: String,
    pub state: AttachmentState,
    /// The target this attachment will be uploaded to, or was uploaded to.
    pub storage_target_id: StorageTargetId,
    /// The namespace inside the target, per `handoff.md` §13.5.
    pub storage_namespace: String,
    /// The remote object, once one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_key: Option<String>,
    /// The suffix inserted into the remote name. Kept so a retry produces the SAME name rather than
    /// orphaning the URL an article already points at.
    pub remote_suffix: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Attachment {
    /// The dedup key: target + namespace + content hash (`handoff.md` §13.4).
    pub fn dedup_key(&self) -> DedupKey {
        DedupKey::new(
            self.storage_target_id.clone(),
            self.storage_namespace.clone(),
            self.content_hash.clone(),
        )
    }

    /// The remote file name this attachment will have.
    pub fn remote_name(&self) -> String {
        remote_file_name(&self.original_name, &self.remote_suffix)
    }

    /// The absolute path of the cache file.
    pub fn cache_file(&self, project_root: &Path) -> PathBuf {
        project_root.join(CACHE_DIRECTORY).join(&self.cache_path)
    }

    /// Whether it is safe for a bulk cleanup to take this attachment.
    pub fn is_safe_orphan(&self, references: &ReferenceIndex, tasks: &[ReferenceTask]) -> bool {
        let reference_count = references.get(&self.id).map(|set| set.len()).unwrap_or(0);
        let reference_status = references
            .get(&self.id)
            .map(|set| set.status())
            .unwrap_or(ReferenceStatus::NotApplied);
        let in_apply_queue = tasks.iter().any(|task| task.attachment_id == self.id);

        is_safe_orphan(
            OrphanEvidence {
                reference_count,
                in_apply_queue,
                upload_status: self.state.upload_status,
                has_pending_task: in_apply_queue,
            },
            reference_status,
            self.state.cache_status,
        )
    }

    /// The next action, with the reference set taken into account.
    ///
    /// The state's own `next_action` is derived from the three axes; this adds the failures recorded
    /// on the attachment itself so the UI has one call to make.
    pub fn next_action(&self, references: &ReferenceIndex) -> NextAction {
        if let Some(set) = references.get(&self.id) {
            if set.is_applying() {
                return NextAction::Wait;
            }
        }
        match self.state.next_action() {
            // A failed apply is a retry of the APPLY, never a re-upload.
            NextAction::ApplyReference if self.state.reference_status == ReferenceStatus::Failed => {
                NextAction::RetryApply
            }
            other => other,
        }
    }

    /// Whether the remote upload has already happened.
    pub fn is_remote(&self) -> bool {
        self.state.upload_status == UploadStatus::Succeeded && self.remote_url.is_some()
    }
}

/// The cache pool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CachePool {
    attachments: BTreeMap<AttachmentId, Attachment>,
    /// The dedup index, so the same bytes to the same target and namespace reuse the remote object.
    by_dedup_key: BTreeMap<String, AttachmentId>,
    /// Queued reference applies.
    tasks: Vec<ReferenceTask>,
}

impl CachePool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.attachments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.attachments.is_empty()
    }

    pub fn get(&self, id: &AttachmentId) -> Option<&Attachment> {
        self.attachments.get(id)
    }

    pub fn get_mut(&mut self, id: &AttachmentId) -> Option<&mut Attachment> {
        self.attachments.get_mut(id)
    }

    pub fn ids(&self) -> Vec<&AttachmentId> {
        self.attachments.keys().collect()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Attachment> {
        self.attachments.values()
    }

    pub fn tasks(&self) -> &[ReferenceTask] {
        &self.tasks
    }

    /// The cache path for a piece of content, per the spec's `<sha256>/<name>` layout.
    ///
    /// The suffix is part of the name so two DIFFERENT files with the same original name do not
    /// collide inside one hash directory — which cannot happen for identical content, but does the
    /// moment a user replaces `photo.png` with different bytes.
    pub fn cache_path_for(content_hash: &str, original_name: &str, suffix: &str) -> String {
        format!("{}/{}", content_hash, remote_file_name(original_name, suffix))
    }

    /// Add a file to the pool, or return the existing record when the same bytes are already there
    /// for this target and namespace (`handoff.md` §13.4).
    ///
    /// The existing record is returned rather than a new one so the caller reuses the remote object
    /// and its URL instead of creating a duplicate.
    pub fn insert(&mut self, attachment: Attachment) -> InsertOutcome {
        let key = attachment.dedup_key().as_string();

        // A file already on the same target with the same bytes: reuse it.
        if let Some(existing_id) = self.by_dedup_key.get(&key) {
            if let Some(existing) = self.attachments.get(existing_id) {
                return InsertOutcome::Reused {
                    id: existing.id.clone(),
                    remote_url: existing.remote_url.clone(),
                };
            }
        }

        // The same bytes for a DIFFERENT target or namespace is a new record with its own URL. That
        // is what §13.4 forbids collapsing.
        let id = attachment.id.clone();
        self.by_dedup_key.insert(key, id.clone());
        self.attachments.insert(id.clone(), attachment);
        InsertOutcome::Inserted { id }
    }

    /// Remove an attachment's record from the pool.
    ///
    /// Deliberately does NOT delete anything remote (`handoff.md` §13.8). It also refuses while an
    /// upload is in flight, because removing the record mid-upload would leave the remote object
    /// untracked.
    pub fn remove(&mut self, id: &AttachmentId) -> Result<Attachment> {
        let Some(attachment) = self.attachments.get(id) else {
            return Err(VeloctError::Invalid(format!("attachment {id} is not in the pool")));
        };
        if attachment.state.upload_status == UploadStatus::Uploading {
            return Err(VeloctError::Invalid(format!(
                "attachment {id} has an upload in flight; removing the record now would leave the \
                 remote object untracked"
            )));
        }
        let key = attachment.dedup_key().as_string();
        self.by_dedup_key.remove(&key);
        self.tasks.retain(|task| &task.attachment_id != id);
        Ok(self
            .attachments
            .remove(id)
            .expect("checked immediately above"))
    }

    /// Record a queued reference apply.
    pub fn queue_reference_task(&mut self, task: ReferenceTask) {
        self.tasks
            .retain(|existing| {
                !(existing.attachment_id == task.attachment_id
                    && existing.content_id == task.content_id)
            });
        self.tasks.push(task);
    }

    /// Take the task for an attachment and document, so it can be attempted.
    pub fn take_reference_task(
        &mut self,
        id: &AttachmentId,
        content_id: &str,
    ) -> Option<ReferenceTask> {
        let index = self
            .tasks
            .iter()
            .position(|task| &task.attachment_id == id && task.content_id == content_id)?;
        Some(self.tasks.remove(index))
    }

    /// Complete a reference apply.
    pub fn finish_reference_task(
        &mut self,
        id: &AttachmentId,
        content_id: &str,
        succeeded: bool,
    ) {
        if succeeded {
            self.tasks
                .retain(|task| !(&task.attachment_id == id && task.content_id == content_id));
        } else if let Some(task) = self
            .tasks
            .iter_mut()
            .find(|task| &task.attachment_id == id && task.content_id == content_id)
        {
            task.attempts += 1;
        }
    }

    /// The attachments a bulk cleanup would take, without taking them.
    pub fn safe_orphans(&self, references: &ReferenceIndex) -> Vec<&Attachment> {
        self.attachments
            .values()
            .filter(|attachment| attachment.is_safe_orphan(references, &self.tasks))
            .collect()
    }

    /// Attachments that need an explicit manual confirmation before they can be deleted.
    ///
    /// `handoff.md` §13.2.4: `SUCCEEDED + NOT_APPLIED` and `SUCCEEDED + FAILED`. They are not
    /// orphans, but a user may still want them gone, and the contract says that requires a
    /// deliberate confirmation rather than a sweep.
    pub fn needs_manual_confirmation(&self, references: &ReferenceIndex) -> Vec<&Attachment> {
        self.attachments
            .values()
            .filter(|attachment| {
                if attachment.state.cache_status != CacheStatus::Present {
                    return false;
                }
                if attachment.is_safe_orphan(references, &self.tasks) {
                    return false;
                }
                let reference_count = references.get(&attachment.id).map(|s| s.len()).unwrap_or(0);
                // Protected by the PENDING/UPLOADING clause, not by this one.
                if attachment.state.upload_status.blocks_cleanup() {
                    return false;
                }
                // Referenced: deleting the cache would not break the article, but it is not a
                // cleanup candidate either.
                if reference_count > 0 {
                    return false;
                }
                true
            })
            .collect()
    }

    /// Perform a cleanup.
    ///
    /// The request decides which path is being taken, so a bulk sweep can never take a file that
    /// needs manual confirmation and a manual delete can never take one that is protected outright.
    pub fn clean(
        &mut self,
        request: &CleanupRequest,
        references: &ReferenceIndex,
    ) -> Result<CleanupOutcome> {
        let mut outcome = CleanupOutcome::default();

        match request {
            CleanupRequest::SafeOrphans => {
                let ids: Vec<AttachmentId> = self
                    .safe_orphans(references)
                    .into_iter()
                    .map(|attachment| attachment.id.clone())
                    .collect();
                for id in ids {
                    let attachment = self.remove(&id)?;
                    outcome.cleaned.push(attachment.id);
                }
            }
            CleanupRequest::Manual { ids, confirmed } => {
                for id in ids {
                    let Some(attachment) = self.attachments.get(id) else {
                        outcome.skipped.push((id.clone(), "not in the pool".to_string()));
                        continue;
                    };

                    // PENDING and UPLOADING are never deletable, by either path
                    // (handoff.md section 13.2.4).
                    if attachment.state.upload_status.blocks_cleanup() {
                        outcome.skipped.push((
                            id.clone(),
                            format!(
                                "{} files are never deleted; the upload must finish or be abandoned first",
                                attachment.state.upload_status.as_str()
                            ),
                        ));
                        continue;
                    }

                    let reference_count =
                        references.get(id).map(|set| set.len()).unwrap_or(0);
                    let safe = attachment.is_safe_orphan(references, &self.tasks);

                    if !safe && !confirmed {
                        outcome.skipped.push((
                            id.clone(),
                            "this file needs an explicit confirmation before it can be deleted"
                                .to_string(),
                        ));
                        continue;
                    }
                    // Even with a confirmation, a referenced attachment is refused: deleting the
                    // cache would not break the article, but the request is almost certainly a
                    // mistake and the reference count proves it is still in use.
                    if reference_count > 0 {
                        outcome.skipped.push((
                            id.clone(),
                            format!(
                                "still referenced by {reference_count} place(s); remove the \
                                 references first"
                            ),
                        ));
                        continue;
                    }

                    let attachment = self.remove(id)?;
                    outcome.cleaned.push(attachment.id);
                }
            }
        }

        Ok(outcome)
    }

    /// A snapshot for the UI: every attachment with its derived status and next action.
    pub fn listing(&self, references: &ReferenceIndex) -> PoolListing {
        let items = self
            .attachments
            .values()
            .map(|attachment| {
                let set = references.get(&attachment.id);
                ListedAttachment {
                    id: attachment.id.clone(),
                    original_name: attachment.original_name.clone(),
                    remote_name: attachment.remote_name(),
                    size_bytes: attachment.size_bytes,
                    storage_target_id: attachment.storage_target_id.clone(),
                    storage_namespace: attachment.storage_namespace.clone(),
                    state: attachment.state,
                    reference_count: set.map(|s| s.len()).unwrap_or(0),
                    referenced_by: set
                        .map(|s| s.content_ids().into_iter().map(str::to_string).collect())
                        .unwrap_or_default(),
                    remote_url: attachment.remote_url.clone(),
                    next_action: attachment.next_action(references),
                    is_safe_orphan: attachment.is_safe_orphan(references, &self.tasks),
                    needs_manual_confirmation: self
                        .needs_manual_confirmation(references)
                        .iter()
                        .any(|candidate| candidate.id == attachment.id),
                    last_error: attachment.last_error.clone(),
                }
            })
            .collect();

        PoolListing {
            items,
            pending_tasks: self.tasks.len(),
        }
    }

    /// Rewrite every attachment's public URL prefix after a target's prefix changed
    /// (`handoff.md` §13.6).
    ///
    /// Nothing remote is moved and nothing is deleted: the contract says the prefix change does not
    /// migrate files and does not assume old article references are still valid. What this returns is
    /// the list of planned rewrites for the EXPLICIT rewrite tool, so the user decides.
    pub fn plan_url_prefix_rewrite(
        &self,
        target: &StorageTargetId,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Vec<UrlRewrite> {
        if old_prefix == new_prefix || old_prefix.is_empty() {
            return Vec::new();
        }
        self.attachments
            .values()
            .filter(|attachment| &attachment.storage_target_id == target)
            .filter_map(|attachment| {
                let url = attachment.remote_url.as_ref()?;
                let rest = url.strip_prefix(old_prefix)?;
                Some(UrlRewrite {
                    attachment_id: attachment.id.clone(),
                    from: url.clone(),
                    to: format!("{new_prefix}{rest}"),
                })
            })
            .collect()
    }
}

/// What adding a file to the pool did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum InsertOutcome {
    /// A new record was created.
    Inserted { id: AttachmentId },
    /// The same bytes were already on this target and namespace, so the existing record is reused
    /// and no second remote object is created (`handoff.md` §13.4).
    Reused {
        id: AttachmentId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remote_url: Option<String>,
    },
}

impl InsertOutcome {
    pub fn id(&self) -> &AttachmentId {
        match self {
            Self::Inserted { id } | Self::Reused { id, .. } => id,
        }
    }

    pub fn was_reused(&self) -> bool {
        matches!(self, Self::Reused { .. })
    }
}

/// Which cleanup path a caller is taking.
///
/// Making this explicit is the point: a bulk sweep and a manual delete have different rules, and a
/// single `clean(ids)` signature would let the sweep's rules be bypassed by passing ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CleanupRequest {
    /// Sweep every safe orphan (`handoff.md` §13.9 first bullet).
    SafeOrphans,
    /// Delete exactly these, having confirmed the ones that need it
    /// (`handoff.md` §13.9 second bullet and §13.2.4).
    Manual { ids: Vec<AttachmentId>, confirmed: bool },
}

/// What a cleanup did, and what it refused to do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupOutcome {
    pub cleaned: Vec<AttachmentId>,
    /// Each refusal with its reason, so a UI can explain rather than silently skipping.
    pub skipped: Vec<(AttachmentId, String)>,
}

impl CleanupOutcome {
    pub fn cleaned_count(&self) -> usize {
        self.cleaned.len()
    }

    pub fn skipped_count(&self) -> usize {
        self.skipped.len()
    }
}

/// One attachment as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListedAttachment {
    pub id: AttachmentId,
    pub original_name: String,
    pub remote_name: String,
    pub size_bytes: u64,
    pub storage_target_id: StorageTargetId,
    pub storage_namespace: String,
    pub state: AttachmentState,
    pub reference_count: usize,
    pub referenced_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    pub next_action: NextAction,
    pub is_safe_orphan: bool,
    pub needs_manual_confirmation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// The pool listing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolListing {
    pub items: Vec<ListedAttachment>,
    pub pending_tasks: usize,
}

/// A planned public-URL rewrite (`handoff.md` §13.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlRewrite {
    pub attachment_id: AttachmentId,
    pub from: String,
    pub to: String,
}

/// Check that a cache path is safe to write.
pub fn checked_cache_path(project_root: &Path, cache_path: &str) -> Result<PathBuf> {
    let root = project_root.join(CACHE_DIRECTORY);
    let candidate = root.join(cache_path);
    // A cache path comes from a content hash and an original name, either of which could be crafted,
    // so the resolved target is checked to be inside the cache directory.
    veloct_core::paths::ensure_within(&root, &candidate)
}

/// Validate an original file name before it enters the pool.
pub fn validate_original_name(name: &str) -> Result<()> {
    if !is_safe_file_name(name) {
        return Err(VeloctError::Invalid(format!(
            "{name:?} is not a usable file name: it must not be empty, contain a path separator, \
             or start with a dot"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reference::ReferenceTarget;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn attachment(id: &str, hash: &str, target: &str) -> Attachment {
        Attachment {
            id: AttachmentId::new(id),
            original_name: "photo.webp".to_string(),
            content_hash: hash.to_string(),
            size_bytes: 1024,
            cache_path: CachePool::cache_path_for(hash, "photo.webp", "a8k3xq"),
            state: AttachmentState::new(),
            storage_target_id: StorageTargetId::new(target),
            storage_namespace: "uploads".to_string(),
            remote_url: None,
            remote_key: None,
            remote_suffix: "a8k3xq".to_string(),
            created_at: NOW.to_string(),
            last_error: None,
        }
    }

    fn referenced(references: &mut ReferenceIndex, id: &AttachmentId, document: &str) {
        references.insert(id, ReferenceTarget::new(document, 0));
    }

    #[test]
    fn a_new_attachment_enters_the_pool_as_pending() {
        let mut pool = CachePool::new();
        let outcome = pool.insert(attachment("a1", "hash1", "s3-main"));
        assert!(!outcome.was_reused());
        assert_eq!(pool.len(), 1);
        let stored = pool.get(&AttachmentId::new("a1")).unwrap();
        assert_eq!(stored.state.upload_status, UploadStatus::Pending);
        assert_eq!(stored.next_action(&ReferenceIndex::new()), NextAction::Upload);
    }

    #[test]
    fn the_cache_path_is_the_hash_directory_then_the_remote_name() {
        // docs/spec/01-data-model.md section 1: .attachments-cache/<sha256>/<original-name>.
        let path = CachePool::cache_path_for("deadbeef", "photo.webp", "a8k3xq");
        assert_eq!(path, "deadbeef/photo-a8k3xq.webp");
    }

    #[test]
    fn the_same_bytes_on_the_same_target_and_namespace_are_reused() {
        // handoff.md section 13.4: reuse the existing remote file and URL.
        let mut pool = CachePool::new();
        let first = pool.insert(attachment("a1", "hash1", "s3-main"));
        assert!(!first.was_reused());

        // Mark the first as uploaded so the reuse carries a URL.
        let id = first.id().clone();
        pool.get_mut(&id).unwrap().remote_url = Some("https://cdn.example.com/photo.webp".into());
        pool.get_mut(&id).unwrap().state.upload_status = UploadStatus::Succeeded;

        let second = pool.insert(attachment("a2", "hash1", "s3-main"));
        assert!(second.was_reused(), "the same content is not uploaded twice");
        assert_eq!(second.id(), &id, "the existing record is reused");
        match second {
            InsertOutcome::Reused { remote_url, .. } => {
                assert_eq!(remote_url.as_deref(), Some("https://cdn.example.com/photo.webp"));
            }
            other => panic!("expected a reuse, got {other:?}"),
        }
        assert_eq!(pool.len(), 1, "no second record was created");
    }

    #[test]
    fn the_same_bytes_on_a_different_target_is_a_new_record() {
        // handoff.md section 13.4: a URL must never be reused across targets.
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "hash1", "s3-main"));
        let second = pool.insert(attachment("a2", "hash1", "github-main"));
        assert!(!second.was_reused());
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn the_same_bytes_in_a_different_namespace_is_a_new_record() {
        // handoff.md section 13.5: changing the prefix forms a new namespace.
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "hash1", "s3-main"));

        let mut other = attachment("a2", "hash1", "s3-main");
        other.storage_namespace = "uploads/v2".to_string();
        let second = pool.insert(other);
        assert!(!second.was_reused());
        assert_eq!(pool.len(), 2);
    }

    #[test]
    fn different_content_with_the_same_name_is_a_new_record() {
        // handoff.md section 13.4: same name but different content gets a new suffix.
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "hash1", "s3-main"));
        let mut second = attachment("a2", "hash2", "s3-main");
        second.remote_suffix = "zzzzzz".to_string();
        let outcome = pool.insert(second);
        assert!(!outcome.was_reused());
        assert_eq!(pool.len(), 2);
        assert_eq!(
            pool.get(&AttachmentId::new("a2")).unwrap().remote_name(),
            "photo-zzzzzz.webp"
        );
    }

    #[test]
    fn a_safe_orphan_is_the_only_thing_a_bulk_sweep_takes() {
        let mut pool = CachePool::new();
        pool.insert(attachment("orphan", "h1", "s3-main"));
        pool.insert(attachment("referenced", "h2", "s3-main"));

        let mut references = ReferenceIndex::new();
        referenced(&mut references, &AttachmentId::new("referenced"), "article-a");

        // A FAILED upload with nothing referencing it is the sweepable case; a PENDING one is
        // protected outright and a SUCCEEDED one needs manual confirmation.
        pool.get_mut(&AttachmentId::new("orphan")).unwrap().state.upload_status =
            UploadStatus::Failed;

        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &references)
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 1);
        assert_eq!(outcome.cleaned[0], AttachmentId::new("orphan"));
        assert_eq!(pool.len(), 1, "the referenced attachment survives");
    }

    #[test]
    fn a_pending_file_is_never_swept_even_with_no_reference() {
        // handoff.md section 13.2.4: PENDING files must not be bulk-deleted.
        let mut pool = CachePool::new();
        pool.insert(attachment("pending", "h1", "s3-main"));

        let references = ReferenceIndex::new();
        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &references)
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0);
        assert_eq!(pool.len(), 1);
        assert!(pool.get(&AttachmentId::new("pending")).is_some());
    }

    #[test]
    fn an_uploading_file_is_never_swept() {
        let mut pool = CachePool::new();
        pool.insert(attachment("uploading", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("uploading"))
            .unwrap()
            .state
            .upload_status = UploadStatus::Uploading;

        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &ReferenceIndex::new())
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0);
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn a_queued_apply_keeps_a_file_out_of_the_sweep() {
        let mut pool = CachePool::new();
        pool.insert(attachment("queued", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("queued")).unwrap().state.upload_status =
            UploadStatus::Succeeded;
        pool.queue_reference_task(ReferenceTask {
            attachment_id: AttachmentId::new("queued"),
            content_id: "article-a".to_string(),
            remote_url: Some("https://cdn/x.webp".to_string()),
            attempts: 0,
        });

        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &ReferenceIndex::new())
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0, "a queued task protects the file");
    }

    #[test]
    fn a_failed_apply_needs_manual_confirmation_and_is_not_swept() {
        // handoff.md section 13.2.4: SUCCEEDED + FAILED requires explicit confirmation.
        let mut pool = CachePool::new();
        pool.insert(attachment("failed", "h1", "s3-main"));
        {
            let stored = pool.get_mut(&AttachmentId::new("failed")).unwrap();
            stored.state = AttachmentState::uploaded_reference_failed();
        }

        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &ReferenceIndex::new())
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0);

        let needs = pool.needs_manual_confirmation(&ReferenceIndex::new());
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].id, AttachmentId::new("failed"));
    }

    #[test]
    fn a_succeeded_but_unapplied_file_is_not_swept_and_needs_manual_confirmation() {
        // handoff.md section 13.2.4: SUCCEEDED + NOT_APPLIED requires explicit confirmation, so the
        // bulk sweep leaves it alone.
        let mut pool = CachePool::new();
        pool.insert(attachment("unapplied", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("unapplied")).unwrap().state.upload_status =
            UploadStatus::Succeeded;

        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &ReferenceIndex::new())
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0, "the sweep must not take it");

        let needs = pool.needs_manual_confirmation(&ReferenceIndex::new());
        assert_eq!(needs.len(), 1);
        assert_eq!(needs[0].id, AttachmentId::new("unapplied"));

        // With an explicit confirmation it can go.
        let confirmed = pool
            .clean(
                &CleanupRequest::Manual {
                    ids: vec![AttachmentId::new("unapplied")],
                    confirmed: true,
                },
                &ReferenceIndex::new(),
            )
            .unwrap();
        assert_eq!(confirmed.cleaned_count(), 1);
    }

    #[test]
    fn a_failed_upload_with_no_reference_is_swept() {
        // Nothing remote, nothing referencing it, no task waiting: the bytes serve no purpose.
        let mut pool = CachePool::new();
        pool.insert(attachment("failed", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("failed")).unwrap().state.upload_status =
            UploadStatus::Failed;

        let outcome = pool
            .clean(&CleanupRequest::SafeOrphans, &ReferenceIndex::new())
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 1);
    }

    #[test]
    fn a_manual_delete_without_confirmation_is_refused_when_one_is_needed() {
        let mut pool = CachePool::new();
        pool.insert(attachment("failed", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("failed")).unwrap().state =
            AttachmentState::uploaded_reference_failed();

        let outcome = pool
            .clean(
                &CleanupRequest::Manual {
                    ids: vec![AttachmentId::new("failed")],
                    confirmed: false,
                },
                &ReferenceIndex::new(),
            )
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0);
        assert_eq!(outcome.skipped_count(), 1);
        assert!(outcome.skipped[0].1.contains("explicit confirmation"));
        assert!(pool.get(&AttachmentId::new("failed")).is_some());
    }

    #[test]
    fn a_manual_delete_with_confirmation_removes_the_file() {
        let mut pool = CachePool::new();
        pool.insert(attachment("failed", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("failed")).unwrap().state =
            AttachmentState::uploaded_reference_failed();

        let outcome = pool
            .clean(
                &CleanupRequest::Manual {
                    ids: vec![AttachmentId::new("failed")],
                    confirmed: true,
                },
                &ReferenceIndex::new(),
            )
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 1);
        assert!(pool.is_empty());
    }

    #[test]
    fn a_manual_delete_still_refuses_a_pending_file_even_when_confirmed() {
        // Confirmation cannot override the PENDING/UPLOADING clause: those are never deletable.
        let mut pool = CachePool::new();
        pool.insert(attachment("pending", "h1", "s3-main"));

        let outcome = pool
            .clean(
                &CleanupRequest::Manual {
                    ids: vec![AttachmentId::new("pending")],
                    confirmed: true,
                },
                &ReferenceIndex::new(),
            )
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0);
        assert!(outcome.skipped[0].1.contains("never deleted"));
    }

    #[test]
    fn a_manual_delete_refuses_a_referenced_file_even_when_confirmed() {
        // Deleting the cache would not break the article, but the request is almost certainly a
        // mistake and the reference count proves it is in use.
        let mut pool = CachePool::new();
        pool.insert(attachment("used", "h1", "s3-main"));
        // A PENDING file is protected by its own clause, so the upload is marked succeeded to reach
        // the referenced-file rule this test is about.
        pool.get_mut(&AttachmentId::new("used")).unwrap().state.upload_status =
            UploadStatus::Succeeded;
        let mut references = ReferenceIndex::new();
        referenced(&mut references, &AttachmentId::new("used"), "article-a");

        let outcome = pool
            .clean(
                &CleanupRequest::Manual {
                    ids: vec![AttachmentId::new("used")],
                    confirmed: true,
                },
                &references,
            )
            .unwrap();
        assert_eq!(outcome.cleaned_count(), 0);
        assert!(
            outcome.skipped[0].1.contains("still referenced by 1 place"),
            "{}",
            outcome.skipped[0].1
        );
    }

    #[test]
    fn removing_a_record_refuses_while_an_upload_is_in_flight() {
        // Removing it mid-upload would leave the remote object untracked.
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("a1")).unwrap().state.upload_status =
            UploadStatus::Uploading;

        let error = pool.remove(&AttachmentId::new("a1")).unwrap_err();
        assert!(error.to_string().contains("in flight"), "{error}");
    }

    #[test]
    fn removing_a_record_has_no_remote_effect() {
        // handoff.md section 13.8: there is no code path here that deletes a remote file.
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        let removed = pool.remove(&AttachmentId::new("a1")).unwrap();
        assert_eq!(removed.id, AttachmentId::new("a1"));
        assert_eq!(removed.remote_url, None);
    }

    #[test]
    fn a_queued_task_round_trips_and_retries_keep_their_attempt_count() {
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        pool.queue_reference_task(ReferenceTask {
            attachment_id: AttachmentId::new("a1"),
            content_id: "article-a".to_string(),
            remote_url: None,
            attempts: 0,
        });

        let task = pool
            .take_reference_task(&AttachmentId::new("a1"), "article-a")
            .unwrap();
        assert_eq!(task.attempts, 0);
        pool.queue_reference_task(task);
        pool.finish_reference_task(&AttachmentId::new("a1"), "article-a", false);
        assert_eq!(pool.tasks()[0].attempts, 1, "the failure is recorded for the retry");

        pool.finish_reference_task(&AttachmentId::new("a1"), "article-a", true);
        assert!(pool.tasks().is_empty(), "a success clears the task");
    }

    #[test]
    fn queueing_the_same_task_twice_does_not_duplicate_it() {
        let mut pool = CachePool::new();
        for _ in 0..3 {
            pool.queue_reference_task(ReferenceTask {
                attachment_id: AttachmentId::new("a1"),
                content_id: "article-a".to_string(),
                remote_url: None,
                attempts: 0,
            });
        }
        assert_eq!(pool.tasks().len(), 1);
    }

    #[test]
    fn the_listing_derives_the_status_and_the_next_action_for_the_ui() {
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("a1")).unwrap().state =
            AttachmentState::uploaded_reference_failed();

        let listing = pool.listing(&ReferenceIndex::new());
        assert_eq!(listing.items.len(), 1);
        let item = &listing.items[0];
        assert_eq!(item.next_action, NextAction::RetryApply);
        assert_eq!(item.reference_count, 0);
        assert!(item.needs_manual_confirmation);
        assert!(!item.is_safe_orphan);
        assert_eq!(item.remote_name, "photo-a8k3xq.webp");
    }

    #[test]
    fn the_listing_names_which_documents_reference_an_attachment() {
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        let mut references = ReferenceIndex::new();
        referenced(&mut references, &AttachmentId::new("a1"), "article-a");
        referenced(&mut references, &AttachmentId::new("a1"), "article-b");

        let listing = pool.listing(&references);
        assert_eq!(listing.items[0].reference_count, 2);
        assert_eq!(
            listing.items[0].referenced_by,
            vec!["article-a".to_string(), "article-b".to_string()]
        );
    }

    #[test]
    fn a_prefix_change_plans_a_rewrite_without_moving_or_deleting_anything() {
        // handoff.md section 13.6: the prefix change does not migrate remote files and does not
        // assume old references are still valid. It offers an EXPLICIT rewrite tool.
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("a1")).unwrap().remote_url =
            Some("https://old.example.com/uploads/photo-a8k3xq.webp".to_string());

        let plan = pool.plan_url_prefix_rewrite(
            &StorageTargetId::new("s3-main"),
            "https://old.example.com/",
            "https://new.example.com/",
        );
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].from, "https://old.example.com/uploads/photo-a8k3xq.webp");
        assert_eq!(plan[0].to, "https://new.example.com/uploads/photo-a8k3xq.webp");
        // Nothing changed: the plan is a plan.
        assert_eq!(
            pool.get(&AttachmentId::new("a1")).unwrap().remote_url.as_deref(),
            Some("https://old.example.com/uploads/photo-a8k3xq.webp")
        );
    }

    #[test]
    fn a_prefix_rewrite_plan_is_empty_when_nothing_matches() {
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        pool.get_mut(&AttachmentId::new("a1")).unwrap().remote_url =
            Some("https://other.example.com/x.webp".to_string());

        assert!(pool
            .plan_url_prefix_rewrite(
                &StorageTargetId::new("s3-main"),
                "https://old.example.com/",
                "https://new.example.com/"
            )
            .is_empty());
        // A different target is not touched either.
        assert!(pool
            .plan_url_prefix_rewrite(
                &StorageTargetId::new("github-main"),
                "https://other.example.com/",
                "https://new.example.com/"
            )
            .is_empty());
    }

    #[test]
    fn a_cache_path_that_escapes_the_cache_directory_is_refused() {
        // The cache root is `<project>/.attachments-cache`, and a path is joined onto it, so the
        // escape has to climb out of the cache directory.
        let root = Path::new("D:/proj");
        for escape in ["../../../escape.webp", "..", "../.."] {
            let error = checked_cache_path(root, escape).unwrap_err();
            assert!(error.to_string().contains("escapes"), "{escape} produced {error}");
        }
    }

    #[test]
    fn an_ordinary_cache_path_is_accepted() {
        let root = Path::new("D:/proj");
        let path = checked_cache_path(root, "deadbeef/photo-a8k3xq.webp").unwrap();
        assert!(path.ends_with("photo-a8k3xq.webp"));
        assert!(path.starts_with(root.join(CACHE_DIRECTORY)));
    }

    #[test]
    fn an_unsafe_original_name_is_refused_before_entering_the_pool() {
        for bad in ["", "a/b.png", "../x.png", ".hidden", "a\\b.png"] {
            assert!(validate_original_name(bad).is_err(), "{bad:?} should be refused");
        }
        assert!(validate_original_name("photo.webp").is_ok());
        assert!(validate_original_name("my file.png").is_ok());
    }

    #[test]
    fn the_pool_and_its_records_round_trip_through_json() {
        let mut pool = CachePool::new();
        pool.insert(attachment("a1", "h1", "s3-main"));
        let json = serde_json::to_string(&pool).unwrap();
        assert!(json.contains("\"upload_status\":\"PENDING\""), "{json}");
        let parsed: CachePool = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(
            parsed.get(&AttachmentId::new("a1")).unwrap().original_name,
            "photo.webp"
        );
    }

    #[test]
    fn removing_an_unknown_attachment_is_reported() {
        let mut pool = CachePool::new();
        let error = pool.remove(&AttachmentId::new("nope")).unwrap_err();
        assert!(error.to_string().contains("not in the pool"), "{error}");
    }

    #[test]
    fn a_manual_delete_of_something_absent_is_skipped_with_a_reason() {
        let mut pool = CachePool::new();
        let outcome = pool
            .clean(
                &CleanupRequest::Manual {
                    ids: vec![AttachmentId::new("nope")],
                    confirmed: true,
                },
                &ReferenceIndex::new(),
            )
            .unwrap();
        assert_eq!(outcome.skipped_count(), 1);
        assert!(outcome.skipped[0].1.contains("not in the pool"));
    }
}
