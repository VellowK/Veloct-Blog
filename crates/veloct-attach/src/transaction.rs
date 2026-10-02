//! The upload transaction.
//!
//! `handoff.md` §13.3 fixes the sequence and the failure rule:
//!
//! ```text
//! cache pool file -> upload -> record the remote object -> apply the Markdown reference
//!   -> build the reference index -> clean the cache
//!
//! Any step failing must leave enough state to continue the operation.
//! ```
//!
//! "Enough state" is the whole design problem, and §13.2.2 already says what it has to make possible:
//! after a failed apply the state is `UPLOADING=SUCCEEDED, REFERENCE=FAILED, CACHE=PRESENT`, and the
//! offered action is to retry the APPLY. So the remote object is recorded as its own durable step,
//! before anything touches the Markdown — if the URL only existed in the return value of an upload
//! function that then failed, the retry would have nothing to apply and would re-upload.
//!
//! §13.3 also says a concurrent edit of the same article must not be silently overwritten: the apply
//! carries the revision it started from and reports a conflict, exactly like a content save.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use veloct_core::workdir::FileRevision;
use veloct_core::{Result, VeloctError};

use crate::naming::{random_suffix, DedupKey};
use crate::pool::{Attachment, AttachmentId, CachePool, ReferenceTask, CACHE_DIRECTORY};
use crate::reference::{ReferenceIndex, ReferenceTarget};
use crate::state::{AttachmentState, CacheStatus, ReferenceStatus, StorageTargetId, UploadStatus};
use crate::storage::{ObjectRecord, StorageTarget};

/// Everything needed to put a file into the pool.
///
/// A struct rather than seven positional arguments: two of them are strings about the same thing, and
/// swapping them would produce a record whose name and hash disagree while every test still ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddRequest {
    pub id: AttachmentId,
    /// The file name the user supplied.
    pub original_name: String,
    /// The content hash of the bytes.
    pub content_hash: String,
    pub size_bytes: u64,
    pub storage_target_id: StorageTargetId,
    /// The namespace inside the target, from the target's configuration.
    pub storage_namespace: String,
    pub now: String,
}

impl AddRequest {
    pub fn new(
        id: AttachmentId,
        original_name: impl Into<String>,
        content_hash: impl Into<String>,
        size_bytes: u64,
        storage_target_id: StorageTargetId,
        storage_namespace: impl Into<String>,
        now: impl Into<String>,
    ) -> Self {
        Self {
            id,
            original_name: original_name.into(),
            content_hash: content_hash.into(),
            size_bytes,
            storage_target_id,
            storage_namespace: storage_namespace.into(),
            now: now.into(),
        }
    }
}

/// A per-dedup-key in-process lock.
///
/// `handoff.md` §13.4 requires concurrency protection so that simultaneous uploads of the same
/// `storage_target_id + storage_namespace + content_hash` do not create several identical remote
/// objects. The protection that actually closes the race is a lock held ACROSS the remote call plus a
/// re-read of the record once it is held, because without the lock two callers both see "nothing
/// uploaded yet" and both PUT.
///
/// The lock is per key rather than global, so uploading two different files still happens in parallel.
/// It is also a `Weak` handle map, so a key that is no longer in use does not accumulate.
#[derive(Debug, Default)]
struct KeyLocks {
    locks: std::sync::Mutex<
        std::collections::BTreeMap<String, std::sync::Weak<std::sync::Mutex<()>>>,
    >,
}

impl KeyLocks {
    /// The lock for a key, creating it on first use.
    fn for_key(&self, key: &str) -> std::sync::Arc<std::sync::Mutex<()>> {
        let mut locks = self.locks.lock().expect("the key lock map is not poisoned");
        // Drop entries whose Arc has been released, so a long-running session does not accumulate one
        // entry per file ever uploaded.
        locks.retain(|_, weak| weak.strong_count() > 0);
        if let Some(existing) = locks.get(key).and_then(std::sync::Weak::upgrade) {
            return existing;
        }
        let created = std::sync::Arc::new(std::sync::Mutex::new(()));
        locks.insert(key.to_string(), std::sync::Arc::downgrade(&created));
        created
    }

    /// How many keys are currently held, for a test.
    fn len(&self) -> usize {
        self.locks.lock().expect("not poisoned").len()
    }
}

/// A request to upload one cached attachment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadRequest {
    pub attachment_id: AttachmentId,
    /// The target to send it to.
    pub storage_target_id: StorageTargetId,
}

/// What an upload produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadOutcome {
    /// The object record, durably recorded before any Markdown was touched
    /// (`handoff.md` §13.3).
    pub object: ObjectRecord,
    /// Whether the remote object was reused because the dedup key already existed.
    pub reused: bool,
    /// The state after the upload: SUCCEEDED with the reference not applied yet.
    pub state: AttachmentState,
}

/// What an explicit remote cleanup produced (`handoff.md` §13.8).
///
/// The attachment is left in the pool with its cached bytes and its reference records: §13.8 removes the
/// REMOTE object, not the user's record of the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDeleteOutcome {
    pub attachment_id: AttachmentId,
    /// The target the object was actually on, read from its record rather than from the caller's claim.
    pub storage_target_id: StorageTargetId,
    /// The object's key inside the target.
    pub key: String,
    pub public_url: String,
    /// The state after the cleanup: no remote object, and an upload may be started again.
    pub state: AttachmentState,
}

/// A request to apply a remote URL into a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyRequest {
    pub attachment_id: AttachmentId,
    /// The document to write the URL into.
    pub content_id: String,
    /// The document's source, as the editor holds it.
    pub source: String,
    /// The revision the editor started from, so a concurrent edit is detected rather than
    /// overwritten (`handoff.md` §13.3).
    pub baseline: FileRevision,
    /// The Markdown node that should receive the URL.
    pub occurrence: usize,
    /// What to replace. When empty, the caller is expected to have already placed a placeholder.
    pub placeholder: String,
}

/// What an apply produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyOutcome {
    /// The new document source, when the apply succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The URL that was written.
    pub url: String,
    /// The reference set after the apply.
    pub reference_status: ReferenceStatus,
    /// Whether the apply was refused because the document changed underneath it.
    pub conflicted: bool,
    /// The reason, when it was refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The result of running the whole transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum TransactionResult {
    /// Everything succeeded: remote, referenced, cache released.
    Complete {
        object: ObjectRecord,
        reference_status: ReferenceStatus,
    },
    /// The upload succeeded but the reference could not be applied. The cache is KEPT and the
    /// retry is an apply, never a re-upload.
    ReferenceFailed {
        object: ObjectRecord,
        message: String,
    },
    /// The upload failed. The cache is kept and a re-upload is allowed.
    UploadFailed { message: String },
    /// The upload was skipped because the remote object already exists. The apply still runs.
    UploadReused {
        object: ObjectRecord,
        reference_status: ReferenceStatus,
    },
}

impl TransactionResult {
    /// Whether the whole transaction completed.
    pub fn is_complete(&self) -> bool {
        matches!(self, Self::Complete { .. } | Self::UploadReused { .. })
    }

    /// Whether the only remaining step is to apply the reference.
    pub fn needs_apply_retry(&self) -> bool {
        matches!(self, Self::ReferenceFailed { .. })
    }

    /// Whether a re-upload is the right next step. False after a failed apply, which is the rule
    /// `handoff.md` §13.2.2 states.
    pub fn allows_reupload(&self) -> bool {
        matches!(self, Self::UploadFailed { .. })
    }

    pub fn object(&self) -> Option<&ObjectRecord> {
        match self {
            Self::Complete { object, .. }
            | Self::ReferenceFailed { object, .. }
            | Self::UploadReused { object, .. } => Some(object),
            Self::UploadFailed { .. } => None,
        }
    }
}

/// The whole attachment lifecycle for one project.
#[derive(Debug)]
pub struct AttachmentTransaction {
    pool: CachePool,
    references: ReferenceIndex,
    /// Recorded remote objects, keyed by dedup key. This is the durable record §13.3 requires before
    /// anything touches the Markdown.
    objects: std::collections::BTreeMap<String, ObjectRecord>,
    /// The suffix each attachment's remote name carries, so a retry produces the SAME name.
    suffixes: std::collections::BTreeMap<AttachmentId, String>,
    /// The per-dedup-key locks that make concurrent uploads of the same content safe
    /// (`handoff.md` §13.4).
    key_locks: KeyLocks,
    /// Where the cached bytes live, so an upload SENDS them.
    ///
    /// This field exists because the upload did not have it, and the bug that produced is worth recording: the
    /// transaction sent `vec![0u8; size_bytes]` — the right NUMBER of bytes, all of them zero — because it had
    /// no way to reach the cache. Every local upload wrote a blank file of the correct size, and the unit tests
    /// did not notice because `MemoryStore` records only the size.
    ///
    /// Optional, because a transaction built by a test with no filesystem has nothing to read, and the recorded
    /// size is then the only thing it can honestly use.
    cache_root: Option<std::path::PathBuf>,
}

/// The dedup key of the object that uploading `attachment` to `target` would create.
///
/// `handoff.md` §13.4: `storage_target_id + storage_namespace + content_hash`. The identity is read from
/// the TARGET rather than from the attachment's own record, and it is read in ONE place so the key lock,
/// the reuse lookup and the object record cannot disagree about which object is being written. Recording
/// the attachment's target instead would let the record claim a target the bytes never reached, which is
/// exactly what `delete_remote_object` refuses to trust.
fn object_dedup_key(attachment: &Attachment, target: &StorageTarget) -> DedupKey {
    DedupKey::new(
        target.config().id.clone(),
        target.namespace(),
        attachment.content_hash.clone(),
    )
}

impl AttachmentTransaction {
    pub fn new() -> Self {
        Self {
            pool: CachePool::new(),
            references: ReferenceIndex::new(),
            objects: std::collections::BTreeMap::new(),
            suffixes: std::collections::BTreeMap::new(),
            key_locks: KeyLocks::default(),
            cache_root: None,
        }
    }

    /// Point the transaction at a project root, so an upload reads the real cached bytes.
    ///
    /// A builder rather than a constructor argument so `new()` stays usable by a test with no filesystem, and so
    /// the CALLER is the one that states where the cache is: `veloct_app::AttachmentService` passes its project
    /// root, and a test that wants to prove the bytes are read passes a directory it filled itself.
    pub fn with_cache_root(mut self, project_root: impl Into<std::path::PathBuf>) -> Self {
        self.cache_root = Some(project_root.into());
        self
    }

    /// Where the cached bytes live, when the transaction knows.
    pub fn cache_root(&self) -> Option<&std::path::Path> {
        self.cache_root.as_deref()
    }

    /// The cached bytes for one attachment.
    ///
    /// `None` when there is nowhere to read from or the file is absent, so the caller can report that rather
    /// than sending zeros.
    fn read_cached(&self, cache_path: &str, size_bytes: u64) -> Option<Vec<u8>> {
        let root = self.cache_root.as_ref()?;
        let path = root.join(CACHE_DIRECTORY).join(cache_path);
        let bytes = std::fs::read(&path).ok()?;
        // The recorded size is the caller's own check; this only rejects an obviously wrong read.
        if size_bytes > 0 && bytes.is_empty() {
            return None;
        }
        Some(bytes)
    }

    pub fn pool(&self) -> &CachePool {
        &self.pool
    }

    pub fn pool_mut(&mut self) -> &mut CachePool {
        &mut self.pool
    }

    pub fn references(&self) -> &ReferenceIndex {
        &self.references
    }

    pub fn references_mut(&mut self) -> &mut ReferenceIndex {
        &mut self.references
    }

    pub fn objects(&self) -> &std::collections::BTreeMap<String, ObjectRecord> {
        &self.objects
    }

    /// The recorded object for a dedup key, which is what a retry applies rather than re-uploading.
    pub fn object_for(&self, key: &DedupKey) -> Option<&ObjectRecord> {
        self.objects.get(&key.as_string())
    }

    /// Add a file to the pool, assigning it a remote suffix.
    ///
    /// The suffix is derived from the content hash so it is DETERMINISTIC: the same content always
    /// produces the same remote name, which is what `handoff.md` §13.7's "the same content reuses the
    /// name and does not regenerate a suffix" requires. A random suffix would give the same file two
    /// names on two runs and orphan the first URL.
    pub fn add(&mut self, request: &AddRequest) -> Result<Attachment> {
        let suffix = self.suffix_for(&request.content_hash);
        let attachment = Attachment {
            id: request.id.clone(),
            original_name: request.original_name.clone(),
            content_hash: request.content_hash.clone(),
            size_bytes: request.size_bytes,
            cache_path: CachePool::cache_path_for(
                &request.content_hash,
                &request.original_name,
                &suffix,
            ),
            state: AttachmentState::new(),
            storage_target_id: request.storage_target_id.clone(),
            storage_namespace: request.storage_namespace.clone(),
            remote_url: None,
            remote_key: None,
            remote_suffix: suffix.clone(),
            created_at: request.now.clone(),
            last_error: None,
        };
        self.suffixes.insert(request.id.clone(), suffix);
        // The pool decides whether this was a new record or a reuse, so the caller gets the record
        // that actually EXISTS rather than the id it asked for: a reused record keeps its own id, and
        // returning the new one would send the caller to look up something that was never stored.
        let outcome = self.pool.insert(attachment);
        let stored_id = outcome.id().clone();
        let record = self
            .pool
            .get(&stored_id)
            .cloned()
            .expect("a record exists after insert");
        Ok(record)
    }

    /// The suffix for a content hash, generated once and then reused.
    ///
    /// A random suffix is generated only the first time a hash is seen, and it is remembered, so the
    /// name is stable for the life of the transaction. `handoff.md` §13.7: the same content reuses
    /// the suffix.
    fn suffix_for(&mut self, content_hash: &str) -> String {
        // Derive the entropy from the hash so the name is reproducible for the same content, which
        // is what makes a re-import produce the same remote key.
        let digest = veloct_core::sha256_hex_str(content_hash);
        let bytes = digest.as_bytes();
        let mut entropy = [0u8; crate::naming::SUFFIX_LENGTH];
        for (index, slot) in entropy.iter_mut().enumerate() {
            *slot = bytes[index % bytes.len()];
        }
        random_suffix(&entropy)
    }

    /// Step 2 and 3 of §13.3: upload, then record the remote object.
    ///
    /// The record is inserted BEFORE the function returns, so a failure in the caller's next step
    /// still leaves the object known.
    pub fn upload(
        &mut self,
        id: &AttachmentId,
        target: &StorageTarget,
        now: &str,
    ) -> Result<UploadOutcome> {
        let attachment = self
            .pool
            .get(id)
            .ok_or_else(|| VeloctError::Invalid(format!("attachment {id} is not in the pool")))?
            .clone();

        // handoff.md section 13.2.2: a SUCCEEDED upload must not be repeated. The state machine says
        // so, and this is the check that enforces it.
        if !attachment.state.upload_status.can_start_upload() {
            return Err(VeloctError::Invalid(format!(
                "attachment {id} is {} and must not be uploaded again; \
                 the remaining step is to apply the reference",
                attachment.state.upload_status.as_str()
            )));
        }

        // The object's identity comes from the TARGET the bytes are going to, not from the attachment.
        //
        // The attachment carries the target the user chose when the file was added, and an upload may
        // name a different one. §13.4's dedup key is `storage_target_id + storage_namespace +
        // content_hash`, and the record's job is to say where the object ACTUALLY is: recording the
        // attachment's target here would make the record claim a target the bytes never reached, and
        // `delete_remote_object` trusts this field to refuse a caller who names the wrong target.
        let dedup_key = object_dedup_key(&attachment, target).as_string();

        // The dedup key already has a recorded object, so the remote file is reused
        // (handoff.md section 13.4).
        if let Some(recorded) = self.objects.get(&dedup_key).cloned() {
            let stored = self
                .pool
                .get_mut(id)
                .expect("the attachment was just read");
            stored.remote_url = Some(recorded.public_url.clone());
            stored.remote_key = Some(recorded.key.clone());
            stored.state.upload_status = UploadStatus::Succeeded;
            return Ok(UploadOutcome {
                object: recorded,
                reused: true,
                state: stored.state,
            });
        }

        // handoff.md section 13.4's concurrency protection. The lock is taken BEFORE the remote call
        // and the record is re-read once it is held, because otherwise two callers both see "nothing
        // uploaded yet" and both PUT, producing two identical remote objects with two URLs.
        let key_lock = self.key_locks.for_key(&dedup_key);
        let _guard = key_lock.lock().expect("the per-key lock is not poisoned");

        // Somebody else may have finished the upload while this call waited for the lock.
        if let Some(recorded) = self.objects.get(&dedup_key).cloned() {
            let stored = self
                .pool
                .get_mut(id)
                .expect("the attachment was just read");
            stored.remote_url = Some(recorded.public_url.clone());
            stored.remote_key = Some(recorded.key.clone());
            stored.state.upload_status = UploadStatus::Succeeded;
            return Ok(UploadOutcome {
                object: recorded,
                reused: true,
                state: stored.state,
            });
        }

        let stored = self
            .pool
            .get_mut(id)
            .expect("the attachment was just read");
        stored.state.upload_status = UploadStatus::Uploading;

        let remote_name = stored.remote_name();
        let size_bytes = stored.size_bytes;
        let cache_path = stored.cache_path.clone();

        // Read the CACHED BYTES. The first version of this sent `vec![0u8; size_bytes]`, which is a file of the
        // right length containing nothing — the worst kind of bug, because the upload succeeded, the size
        // matched, and the picture was blank.
        let bytes = match self.read_cached(&cache_path, size_bytes) {
            Some(bytes) => bytes,
            None => {
                // Nothing to read, and sending zeros would be a lie about the content. The attachment is left
                // retryable rather than marked succeeded, and the caller is told what is wrong.
                let stored = self.pool.get_mut(id).expect("the attachment was just read");
                stored.state.upload_status = UploadStatus::Failed;
                stored.last_error = Some(format!(
                    "the cached bytes for {id} are missing, so there was nothing to upload;                      the cache directory must be the project's own"
                ));
                return Err(VeloctError::Attachment(format!(
                    "the cached bytes for attachment {id} could not be read;                      the transaction needs the project root (use `with_cache_root`) to find them"
                )));
            }
        };
        // A size that disagrees with the bytes on disk means the record is stale, and uploading either would be
        // wrong. This is asserted rather than trusted.
        if bytes.len() as u64 != size_bytes {
            let stored = self.pool.get_mut(id).expect("the attachment was just read");
            stored.state.upload_status = UploadStatus::Failed;
            stored.last_error = Some(format!(
                "the cached file has {} bytes but the record says {size_bytes}",
                bytes.len()
            ));
            return Err(VeloctError::Attachment(format!(
                "attachment {id} records {size_bytes} bytes but its cache file has {}",
                bytes.len()
            )));
        }

        // The record's target and namespace are the ones whose key the lock and the lookup above use, so
        // the record cannot describe an object other than the one that was just written.
        let object_target_id = target.config().id.clone();
        let object_namespace = target.namespace();

        // No `let` binding here: both arms diverge, so the match's value would never be read.
        match target.store(&remote_name, &bytes) {
            Ok(put) => {
                let object = ObjectRecord {
                    dedup_key: dedup_key.clone(),
                    storage_target_id: object_target_id,
                    storage_namespace: object_namespace,
                    content_hash: attachment.content_hash.clone(),
                    key: put.object().key.clone(),
                    public_url: put.object().public_url.clone(),
                    size_bytes: put.object().size_bytes,
                    created_at: now.to_string(),
                };
                // Recorded before returning, so the state survives a later failure
                // (handoff.md section 13.3).
                self.objects
                    .insert(object.dedup_key.clone(), object.clone());

                let stored = self.pool.get_mut(id).expect("still present");
                stored.remote_url = Some(object.public_url.clone());
                stored.remote_key = Some(object.key.clone());
                stored.state.upload_status = UploadStatus::Succeeded;
                stored.state.reference_status = ReferenceStatus::NotApplied;
                stored.last_error = None;

                Ok(UploadOutcome {
                    state: stored.state,
                    reused: !put.was_created(),
                    object,
                })
            }
            Err(error) => {
                let stored = self.pool.get_mut(id).expect("still present");
                stored.state.upload_status = UploadStatus::Failed;
                stored.state.reference_status = ReferenceStatus::NotApplied;
                // The MESSAGE is recorded for the UI, and the error ITSELF is returned.
                //
                // This used to wrap every failure as `VeloctError::Attachment(message)`, which threw away the
                // code and the hint and turned "this target kind has no adapter in this build, here is what to
                // use instead" into an opaque ATTACHMENT error. A caller cannot act on a code it was never
                // given, and section 14.13 requires a failure REASON the user can read.
                stored.last_error = Some(match error.hint() {
                    Some(hint) => format!("{error} ({hint})"),
                    None => error.to_string(),
                });
                Err(error)
            }
        }
    }

    /// Remove ONE recorded remote object, on the target that actually holds it (`handoff.md` §13.8).
    ///
    /// This is the explicit user request §13.8 leaves room for. It is deliberately NOT reachable from any
    /// other path: nothing in the attachment lifecycle calls it, and `apply`/`remove` do not.
    ///
    /// Every refusal happens BEFORE the adapter is reached, so a call that is refused removes nothing:
    ///
    /// * a target whose `capabilities().can_delete_remotely` is false is refused with its own
    ///   §13.10/§13.12 gap, rather than reporting a cleanup that removed nothing (ADR-0010);
    /// * an attachment with no recorded object has nothing to remove;
    /// * a `target` that is not the one the record names is refused, because the caller's belief about
    ///   where the object is must never be what decides which file is deleted.
    ///
    /// What it does NOT touch: the cached bytes and the reference records. §13.8 removes the remote object,
    /// not the user's record of the file — the attachment stays in the pool, its Markdown falls back to the
    /// cache path, and it may be uploaded again.
    pub fn delete_remote_object(
        &mut self,
        id: &AttachmentId,
        target: &StorageTarget,
    ) -> Result<RemoteDeleteOutcome> {
        let attachment = self
            .pool
            .get(id)
            .cloned()
            .ok_or_else(|| VeloctError::Invalid(format!("attachment {id} is not in the pool")))?;

        let named_target = target.config().id.clone();

        // ADR-0010: a target that cannot delete refuses, with its own reason, rather than reporting a
        // cleanup that removed nothing. This is checked BEFORE anything else about the record, because it
        // is a property of the TARGET the caller named rather than of what is stored on it.
        if !target.capabilities().can_delete_remotely {
            return Err(VeloctError::NotConfigured {
                what: format!(
                    "the {} target {}",
                    target.config().kind.as_str(),
                    target.config().name
                ),
                hint: Some(format!(
                    "this target kind cannot remove an object in this build ({}), so the remote file was \
                     left alone",
                    crate::targets::unimplemented_reason(target.config().kind)
                        .unwrap_or("no adapter for this kind")
                )),
            });
        }

        // The object the caller is asking about is the one this attachment's Markdown points at, so the
        // remote KEY on the record is the link. The record's own `storage_target_id` then decides whether
        // the caller named the right target.
        let Some(remote_key) = attachment.remote_key.clone() else {
            return Err(VeloctError::Invalid(format!(
                "attachment {id} has no recorded remote object, so there is nothing to remove"
            )));
        };
        let Some(record) = self
            .objects
            .values()
            .find(|record| record.key == remote_key && record.storage_target_id == named_target)
            .cloned()
        else {
            // A different target holds an object at this key: say WHICH, so the user can ask for the right
            // one instead of being told that nothing exists.
            if let Some(other) = self
                .objects
                .values()
                .find(|record| record.key == remote_key)
            {
                return Err(VeloctError::Invalid(format!(
                    "the remote object for attachment {id} is recorded on the storage target {}, not \
                     {named_target}; nothing was removed",
                    other.storage_target_id
                )));
            }
            return Err(VeloctError::Invalid(format!(
                "attachment {id} has no recorded remote object on the storage target {named_target}, so \
                 there is nothing to remove"
            )));
        };

        target.delete_remote(&record.key)?;

        // §13.8's cleanup is of the REMOTE object. The record goes with it, and the attachment stops
        // claiming to be remote — remote_url/remote_key are what `Attachment::is_remote()` and
        // `markdown()` read, so the Markdown falls back to the cache path.
        self.objects.remove(&record.dedup_key);
        let stored = self.pool.get_mut(id).expect("the attachment was just read");
        stored.remote_url = None;
        stored.remote_key = None;
        // The upload axis goes back to PENDING, which is `can_start_upload`. It is not FAILED: no attempt
        // failed, and saying one did would be its own small lie. The reference records and the cached bytes
        // are deliberately untouched.
        stored.state.upload_status = UploadStatus::Pending;
        stored.last_error = None;

        Ok(RemoteDeleteOutcome {
            attachment_id: id.clone(),
            storage_target_id: record.storage_target_id.clone(),
            key: record.key,
            public_url: record.public_url,
            state: stored.state,
        })
    }

    /// Steps 4 and 5: apply the URL to the Markdown and record the reference.
    ///
    /// A conflict is REPORTED, never resolved by overwriting: `handoff.md` §13.3 says a concurrent
    /// edit must not be silently overwritten and the user must be asked to re-apply.
    pub fn apply(&mut self, request: &ApplyRequest, now: &str) -> Result<ApplyOutcome> {
        let Some(attachment) = self.pool.get(&request.attachment_id).cloned() else {
            return Err(VeloctError::Invalid(format!(
                "attachment {} is not in the pool",
                request.attachment_id
            )));
        };

        let Some(url) = attachment.remote_url.clone() else {
            return Err(VeloctError::Invalid(format!(
                "attachment {} has no remote URL yet, so there is nothing to apply",
                request.attachment_id
            )));
        };

        // A concurrent edit is a conflict, exactly as it is for a content save.
        if !request.baseline.matches_text(&request.source) {
            let stored = self
                .pool
                .get_mut(&request.attachment_id)
                .expect("read above");
            stored.state.reference_status = ReferenceStatus::Failed;
            stored.last_error = Some("the document changed while the reference was being applied".into());
            self.references
                .entry(&request.attachment_id)
                .mark_apply_failed();

            self.pool.queue_reference_task(ReferenceTask {
                attachment_id: request.attachment_id.clone(),
                content_id: request.content_id.clone(),
                remote_url: Some(url.clone()),
                attempts: 0,
            });

            return Ok(ApplyOutcome {
                source: None,
                url,
                reference_status: ReferenceStatus::Failed,
                conflicted: true,
                message: Some(format!(
                    "{} changed since the apply started; Veloct did not overwrite it. Re-apply the \
                     reference when the document is settled.",
                    request.content_id
                )),
            });
        }

        self.references
            .entry(&request.attachment_id)
            .mark_applying();

        // The URL replaces the placeholder the editor wrote when the file was dropped in. When no
        // placeholder is given the URL is appended as a Markdown image, which is what an image
        // attachment means.
        let new_source = if request.placeholder.is_empty() {
            format!("{}\n\n![{}]({})\n", request.source.trim_end(), attachment.original_name, url)
        } else {
            if !request.source.contains(&request.placeholder) {
                self.references
                    .entry(&request.attachment_id)
                    .mark_apply_failed();
                if let Some(stored) = self.pool.get_mut(&request.attachment_id) {
                    stored.state.reference_status = ReferenceStatus::Failed;
                    stored.last_error = Some(format!(
                        "the placeholder {:?} is no longer in the document",
                        request.placeholder
                    ));
                }
                return Ok(ApplyOutcome {
                    source: None,
                    url,
                    reference_status: ReferenceStatus::Failed,
                    conflicted: true,
                    message: Some(format!(
                        "the placeholder {:?} is no longer in {}; re-apply the reference",
                        request.placeholder, request.content_id
                    )),
                });
            }
            request
                .source
                .replacen(&request.placeholder, &url, 1)
        };

        let set = self.references.entry(&request.attachment_id);
        set.insert(
            ReferenceTarget::new(&request.content_id, request.occurrence)
                .with_node_hash(veloct_core::sha256_hex_str(&url)),
        );
        set.mark_applied();
        let status = set.status();

        let stored = self
            .pool
            .get_mut(&request.attachment_id)
            .expect("read above");
        stored.state.reference_status = status;
        stored.last_error = None;

        // The task is done.
        self.pool
            .finish_reference_task(&request.attachment_id, &request.content_id, true);

        let _ = now;
        Ok(ApplyOutcome {
            source: Some(new_source),
            url,
            reference_status: status,
            conflicted: false,
            message: None,
        })
    }

    /// Step 6: release the cache entry once the reference is applied.
    ///
    /// Only legal when a valid reference exists: releasing the cache before the URL is written
    /// anywhere would leave the file reachable only through a record the user has not seen.
    pub fn release_cache(&mut self, id: &AttachmentId) -> Result<()> {
        let status = self
            .references
            .get(id)
            .map(|set| set.status())
            .unwrap_or(ReferenceStatus::NotApplied);

        if !status.is_applied() {
            return Err(VeloctError::Invalid(format!(
                "attachment {id} has reference status {}; the cache is released only after the URL \
                 has been written somewhere",
                status.as_str()
            )));
        }

        let stored = self
            .pool
            .get_mut(id)
            .ok_or_else(|| VeloctError::Invalid(format!("attachment {id} is not in the pool")))?;
        stored.state.cache_status = CacheStatus::Cleaned;
        Ok(())
    }

    /// Run the whole transaction for one attachment.
    ///
    /// Every step's failure is turned into a [`TransactionResult`] that names the stage, so the
    /// caller can resume rather than start over.
    pub fn run(
        &mut self,
        id: &AttachmentId,
        target: &StorageTarget,
        apply: Option<&ApplyRequest>,
        now: &str,
    ) -> Result<TransactionResult> {
        let upload = match self.upload(id, target, now) {
            Ok(upload) => upload,
            Err(error) => {
                return Ok(TransactionResult::UploadFailed {
                    message: error.to_string(),
                })
            }
        };

        let Some(apply) = apply else {
            return Ok(TransactionResult::UploadReused {
                object: upload.object,
                reference_status: ReferenceStatus::NotApplied,
            });
        };

        let applied = self.apply(apply, now)?;
        if applied.conflicted {
            return Ok(TransactionResult::ReferenceFailed {
                object: upload.object,
                message: applied
                    .message
                    .unwrap_or_else(|| "the reference could not be applied".to_string()),
            });
        }

        // A successful apply releases the cache (step 6).
        self.release_cache(id)?;

        let status = self
            .references
            .get(id)
            .map(|set| set.status())
            .unwrap_or(ReferenceStatus::Applied);

        Ok(if upload.reused {
            TransactionResult::UploadReused {
                object: upload.object,
                reference_status: status,
            }
        } else {
            TransactionResult::Complete {
                object: upload.object,
                reference_status: status,
            }
        })
    }

    /// Retry only the reference apply for an attachment whose upload already succeeded.
    ///
    /// This is the operation `handoff.md` §13.2.2 requires, and it deliberately cannot upload: the
    /// signature has no target, so the re-upload it forbids is not expressible.
    pub fn retry_apply(&mut self, request: &ApplyRequest, now: &str) -> Result<ApplyOutcome> {
        let Some(attachment) = self.pool.get(&request.attachment_id) else {
            return Err(VeloctError::Invalid(format!(
                "attachment {} is not in the pool",
                request.attachment_id
            )));
        };
        if attachment.remote_url.is_none() {
            return Err(VeloctError::Invalid(format!(
                "attachment {} has never been uploaded, so there is nothing to re-apply; upload it \
                 first",
                request.attachment_id
            )));
        }
        self.apply(request, now)
    }

    /// Serialize the durable part, so a restart resumes rather than restarts.
    pub fn to_state(&self) -> Result<String> {
        Ok(serde_json::to_string(&PersistedState {
            pool: self.pool.clone(),
            references: self.references.clone(),
            objects: self.objects.clone(),
        })?)
    }

    /// Restore from a serialized state.
    pub fn from_state(text: &str) -> Result<Self> {
        let state: PersistedState = serde_json::from_str(text)
            .map_err(|error| VeloctError::Invalid(format!("the attachment state is unreadable: {error}")))?;
        let mut transaction = Self::new();
        transaction.pool = state.pool;
        transaction.references = state.references;
        transaction.objects = state.objects;
        // The suffixes are rebuilt from the records, so a resumed transaction produces the same
        // remote names rather than new ones.
        for attachment in transaction.pool.iter() {
            transaction
                .suffixes
                .insert(attachment.id.clone(), attachment.remote_suffix.clone());
        }
        Ok(transaction)
    }
}

impl Default for AttachmentTransaction {
    fn default() -> Self {
        Self::new()
    }
}

/// The serializable part of a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedState {
    pool: CachePool,
    references: ReferenceIndex,
    objects: std::collections::BTreeMap<String, ObjectRecord>,
}

/// A shared transaction, for the Tauri state.
pub type SharedTransaction = Arc<std::sync::Mutex<AttachmentTransaction>>;

/// A transaction that serialises concurrent uploads of the SAME content.
///
/// `handoff.md` §13.4 requires concurrency protection so that simultaneous uploads of the same
/// `storage_target_id + storage_namespace + content_hash` do not create several identical remote
/// objects. [`AttachmentTransaction::upload`] takes `&mut self`, so a caller already has exclusive
/// access by construction — which means the DURABLE protection against two Veloct windows racing is a
/// unique constraint on the object's dedup key, and that is the storage crate's business.
///
/// What this type adds is the protection inside one process, and it is what makes the claim testable:
/// it serialises two uploads of the same key and RE-CHECKS after taking the key, so the second caller
/// returns the object the first one stored instead of issuing a second PUT.
///
/// The transaction mutex is held across the remote call. That limits concurrent uploads to different
/// files to one at a time, which is the deliberate first trade: correctness here is worth more than
/// throughput on a personal publishing tool, and a per-key refinement can come later without changing
/// any caller.
pub struct ConcurrentUploader {
    transaction: Mutex<AttachmentTransaction>,
    locks: KeyLocks,
}

impl ConcurrentUploader {
    pub fn new(transaction: AttachmentTransaction) -> Self {
        Self {
            transaction: Mutex::new(transaction),
            locks: KeyLocks::default(),
        }
    }

    /// Upload one attachment, or return the object an earlier concurrent call already stored for the
    /// same content.
    pub fn upload(
        &self,
        id: &AttachmentId,
        target: &StorageTarget,
        now: &str,
    ) -> Result<UploadOutcome> {
        // The key is read WITHOUT the transfer lock, so two callers for the same content converge on
        // the same lock and the second waits. It is derived from the TARGET by the same helper
        // `AttachmentTransaction::upload` uses, so the lock this call takes is the lock around the object
        // it is about to write.
        let key = {
            let transaction = self.transaction.lock().expect("not poisoned");
            let attachment = transaction.pool().get(id).ok_or_else(|| {
                VeloctError::Invalid(format!("attachment {id} is not in the pool"))
            })?;
            object_dedup_key(attachment, target).as_string()
        };

        let key_lock = self.locks.for_key(&key);
        let _guard = key_lock.lock().expect("the per-key lock is not poisoned");

        let mut transaction = self.transaction.lock().expect("not poisoned");
        transaction.upload(id, target, now)
    }

    /// Run a closure with exclusive access, for the operations that are not uploads.
    pub fn with<R>(&self, body: impl FnOnce(&mut AttachmentTransaction) -> R) -> R {
        let mut transaction = self.transaction.lock().expect("not poisoned");
        body(&mut transaction)
    }

    /// How many key locks are currently held.
    pub fn held_locks(&self) -> usize {
        self.locks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{MemoryFileStore, MemoryStore, StorageConfig, StorageKind};

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn config() -> StorageConfig {
        let mut config = StorageConfig::new("s3-main", "S3 main", StorageKind::S3);
        config.bucket = Some("veloct-assets".to_string());
        config.key_prefix = Some("uploads".to_string());
        config.public_url_prefix = "https://cdn.example.com".to_string();
        config
    }

    fn target() -> StorageTarget {
        StorageTarget::new(config(), Box::new(MemoryStore::new(config())))
    }

    fn target_with(store: Arc<MemoryFileStore>) -> StorageTarget {
        StorageTarget::new(config(), Box::new(MemoryStore::with_store(config(), store)))
    }

    /// A fixture with a REAL cache root and the bytes the records claim.
    ///
    /// The upload used to send zeros, so no test needed a cache file. Now that it reads them, a fixture that
    /// does not provide them is testing a state the application never produces — and the tests caught that
    /// immediately, which is the point of fixing the upload rather than working around it.
    struct Fixture {
        transaction: AttachmentTransaction,
        /// Held so the directory outlives the transaction.
        _root: tempfile::TempDir,
    }

    impl std::ops::Deref for Fixture {
        type Target = AttachmentTransaction;
        fn deref(&self) -> &Self::Target {
            &self.transaction
        }
    }

    impl std::ops::DerefMut for Fixture {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.transaction
        }
    }

    fn transaction() -> Fixture {
        let root = tempfile::TempDir::new().expect("a temp project");
        Fixture {
            transaction: AttachmentTransaction::new().with_cache_root(root.path()),
            _root: root,
        }
    }

    /// Write the bytes a record claims, so an upload has something real to send.
    fn fill_cache(fixture: &Fixture, id: &AttachmentId) {
        let attachment = fixture
            .transaction
            .pool()
            .get(id)
            .expect("the attachment was added");
        let path = fixture
            ._root
            .path()
            .join(crate::pool::CACHE_DIRECTORY)
            .join(&attachment.cache_path);
        veloct_core::workdir::ensure_parent(&path).unwrap();
        std::fs::write(&path, vec![7u8; attachment.size_bytes as usize]).unwrap();
    }

    /// Write a cache file for every attachment in a shared transaction.
    fn write_cache_file(
        cache_root: &std::path::Path,
        uploader: &Arc<ConcurrentUploader>,
        size: u64,
    ) {
        uploader.with(|transaction| {
            for id in transaction.pool().ids() {
                let attachment = transaction.pool().get(id).expect("in the pool");
                let path = cache_root
                    .join(crate::pool::CACHE_DIRECTORY)
                    .join(&attachment.cache_path);
                veloct_core::workdir::ensure_parent(&path).unwrap();
                std::fs::write(&path, vec![7u8; size as usize]).unwrap();
            }
        });
    }

    fn add(transaction: &mut Fixture) -> AttachmentId {
        let id = AttachmentId::new("att-1");
        transaction.add(&add_request(&id, "content-hash-1")).unwrap();
        fill_cache(transaction, &id);
        id
    }

    fn add_request(id: &AttachmentId, hash: &str) -> AddRequest {
        AddRequest::new(
            id.clone(),
            "photo.webp",
            hash,
            1024,
            StorageTargetId::new("s3-main"),
            "s3:veloct-assets|uploads",
            NOW,
        )
    }

    fn apply_request(id: &AttachmentId, source: &str) -> ApplyRequest {
        ApplyRequest {
            attachment_id: id.clone(),
            content_id: "article-a".to_string(),
            source: source.to_string(),
            baseline: FileRevision::of_text(source),
            occurrence: 0,
            placeholder: String::new(),
        }
    }

    #[test]
    fn the_whole_sequence_completes_remote_referenced_and_cleaned() {
        // handoff.md section 13.3, the happy path.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();

        let source = "# Article\n";
        let result = transaction
            .run(&id, &target, Some(&apply_request(&id, source)), NOW)
            .unwrap();

        assert!(result.is_complete(), "{result:?}");
        let object = result.object().unwrap();
        assert!(object.public_url.starts_with("https://cdn.example.com/uploads/photo-"));

        let stored = transaction.pool().get(&id).unwrap();
        assert_eq!(stored.state.upload_status, UploadStatus::Succeeded);
        assert_eq!(stored.state.reference_status, ReferenceStatus::Applied);
        assert_eq!(stored.state.cache_status, CacheStatus::Cleaned);
    }

    #[test]
    fn the_remote_object_is_recorded_before_the_reference_is_applied() {
        // handoff.md section 13.3: this is what makes a failed apply resumable rather than requiring
        // a re-upload.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();

        transaction.upload(&id, &target, NOW).unwrap();

        let attachment = transaction.pool().get(&id).unwrap();
        let key = attachment.dedup_key();
        let recorded = transaction.object_for(&key).expect("the object is recorded");
        assert!(recorded.public_url.starts_with("https://cdn.example.com/"));
        assert_eq!(
            transaction.pool().get(&id).unwrap().remote_url.as_deref(),
            Some(recorded.public_url.as_str())
        );
    }

    #[test]
    fn a_failed_upload_keeps_the_cache_and_allows_a_retry() {
        // handoff.md section 13.2.2: FAILED / NOT_APPLIED / PRESENT, and a re-upload is allowed.
        let mut transaction = transaction();
        let id = add(&mut transaction);

        let store = Arc::new(MemoryFileStore::new());
        store.fail_puts_with("the bucket is gone");
        let target = target_with(store.clone());

        let result = transaction
            .run(&id, &target, Some(&apply_request(&id, "# A\n")), NOW)
            .unwrap();

        match &result {
            TransactionResult::UploadFailed { message } => {
                assert!(message.contains("bucket is gone"), "{message}");
            }
            other => panic!("expected UploadFailed, got {other:?}"),
        }
        assert!(result.allows_reupload());
        assert!(!result.needs_apply_retry());

        let stored = transaction.pool().get(&id).unwrap();
        assert_eq!(stored.state.upload_status, UploadStatus::Failed);
        assert_eq!(stored.state.reference_status, ReferenceStatus::NotApplied);
        assert_eq!(stored.state.cache_status, CacheStatus::Present);
        assert!(!transaction.references().get(&id).map(|s| s.is_applying()).unwrap_or(false));

        // The retry succeeds once the target recovers.
        store.allow_puts();
        let result = transaction
            .run(&id, &target, Some(&apply_request(&id, "# A\n")), NOW)
            .unwrap();
        assert!(result.is_complete(), "{result:?}");
    }

    #[test]
    fn a_failed_apply_keeps_the_cache_and_offers_an_apply_retry_not_a_re_upload() {
        // handoff.md section 13.2.2: the state that must not blindly re-upload.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();

        // A request whose baseline does not match its source simulates a concurrent edit.
        let mut request = apply_request(&id, "# Article\n");
        request.baseline = FileRevision::of_text("something else entirely");

        let result = transaction.run(&id, &target, Some(&request), NOW).unwrap();
        match &result {
            TransactionResult::ReferenceFailed { message, .. } => {
                assert!(message.contains("did not overwrite"), "{message}");
            }
            other => panic!("expected ReferenceFailed, got {other:?}"),
        }
        assert!(!result.allows_reupload(), "a re-upload is forbidden here");
        assert!(result.needs_apply_retry());

        let stored = transaction.pool().get(&id).unwrap();
        assert_eq!(stored.state.upload_status, UploadStatus::Succeeded);
        assert_eq!(stored.state.reference_status, ReferenceStatus::Failed);
        assert_eq!(stored.state.cache_status, CacheStatus::Present);
        // The remote object is still recorded, which is what the retry applies.
        assert!(stored.remote_url.is_some());
    }

    #[test]
    fn retrying_the_apply_after_a_failed_upload_attempt_does_not_upload_again() {
        // The core of handoff.md section 13.2.2. `retry_apply` has no target parameter, so a
        // re-upload is not even expressible.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let store = Arc::new(MemoryFileStore::new());
        let target = target_with(store.clone());

        let mut request = apply_request(&id, "# Article\n");
        request.baseline = FileRevision::of_text("stale");
        let failed = transaction.run(&id, &target, Some(&request), NOW).unwrap();
        assert!(failed.needs_apply_retry());
        let puts_after_upload = store.put_count();
        assert_eq!(puts_after_upload, 1, "the upload happened once");

        // Now retry with a matching baseline.
        let good = apply_request(&id, "# Article\n");
        let applied = transaction.retry_apply(&good, NOW).unwrap();
        assert!(!applied.conflicted, "{applied:?}");
        assert!(applied.source.is_some());
        assert_eq!(
            store.put_count(),
            puts_after_upload,
            "the retry did NOT touch the remote target"
        );
    }

    #[test]
    fn a_second_upload_of_an_already_uploaded_attachment_is_refused() {
        // The state machine and this check both say so.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();

        transaction.upload(&id, &target, NOW).unwrap();
        let error = transaction.upload(&id, &target, NOW).unwrap_err();
        assert!(error.to_string().contains("must not be uploaded again"), "{error}");
    }

    #[test]
    fn the_same_bytes_on_the_same_target_reuse_the_remote_object_without_a_second_put() {
        // handoff.md section 13.4.
        let mut transaction = transaction();
        let store = Arc::new(MemoryFileStore::new());
        let target = target_with(store.clone());

        let first = add(&mut transaction);
        transaction.upload(&first, &target, NOW).unwrap();
        assert_eq!(store.put_count(), 1);

        // A second attachment with the same content hash, target and namespace. The pool reports
        // the EXISTING record, because the same bytes on the same target are one remote object.
        let second = transaction
            .add(&add_request(&AttachmentId::new("att-2"), "content-hash-1"))
            .unwrap();
        assert_eq!(second.id, first, "the existing record is reused, not duplicated");
        assert_eq!(transaction.pool().len(), 1);

        // A SUCCEEDED attachment must not be uploaded again at all, which is the stronger form of
        // "the object is reused": the second upload attempt is refused outright.
        let error = transaction.upload(&second.id, &target, NOW).unwrap_err();
        assert!(error.to_string().contains("must not be uploaded again"), "{error}");
        assert_eq!(store.put_count(), 1, "no second PUT was attempted");
        assert!(
            transaction.pool().get(&second.id).unwrap().remote_url.is_some(),
            "the existing record already carries its URL, which is what the reuse gives the caller"
        );

        // And `run` with no apply reports the reuse rather than attempting a PUT.
        let _ = transaction.run(&second.id, &target, None, NOW).unwrap();
        assert_eq!(store.put_count(), 1, "still one PUT");
    }

    #[test]
    fn a_key_lock_serialises_two_callers_and_is_shared_per_key() {
        // handoff.md section 13.4's concurrency protection. Two threads racing the SAME key must not
        // both be inside the critical section, and two DIFFERENT keys must not block each other.
        let locks = std::sync::Arc::new(KeyLocks::default());
        let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));

        let inner = locks.for_key("same-key");
        let guard = inner.lock().expect("not poisoned");

        let waiter = {
            let locks = locks.clone();
            let order = order.clone();
            std::thread::spawn(move || {
                // This blocks until the guard is released, which is what makes the race safe.
                let inner = locks.for_key("same-key");
                let _guard = inner.lock().expect("not poisoned");
                order.lock().unwrap().push("second");
            })
        };

        // The other key is not affected by the first one being held.
        let other = locks.for_key("other-key");
        assert!(
            other.try_lock().is_ok(),
            "a different key must not be blocked by the first"
        );
        order.lock().unwrap().push("first");

        std::thread::sleep(std::time::Duration::from_millis(80));
        assert_eq!(
            order.lock().unwrap().len(),
            1,
            "the second caller must still be waiting"
        );

        drop(guard);
        waiter.join().unwrap();
        assert_eq!(*order.lock().unwrap(), vec!["first", "second"]);
    }

    #[test]
    fn a_released_key_lock_is_reused_rather_than_accumulating() {
        // A long session must not keep one entry per file ever uploaded.
        let locks = KeyLocks::default();
        {
            let _held = locks.for_key("a");
            assert_eq!(locks.len(), 1);
        }
        // The Arc is gone, so the next call prunes the entry.
        let _again = locks.for_key("b");
        assert_eq!(locks.len(), 1, "the released key was pruned");
    }

    #[test]
    fn concurrent_uploads_of_the_same_content_never_issue_a_second_put() {
        // handoff.md section 13.4: "simultaneous uploads of the same target + namespace + hash must
        // have concurrency protection so several identical remote objects are not created".
        //
        // The store counts PUTs, so a second PUT is visible rather than inferred.
        //
        // Note what the pool already does: `add` REUSES a record for content it already holds for the
        // same target and namespace, so two attachments holding identical bytes are one record with
        // one id. The race the contract is about therefore cannot arise from two `add` calls, and this
        // test exercises the reachable form: the same attachment uploaded from two threads at once.
        let store = Arc::new(MemoryFileStore::new());
        let target = Arc::new(target_with(store.clone()));

        // A real cache root with the bytes the record claims, because the upload now READS them. The directory
        // is leaked deliberately: it must outlive the uploader, and a `TempDir` dropped at the end of the test
        // would be fine but threading the guard through the closure is noise for a test fixture.
        let root = tempfile::TempDir::new().unwrap();
        let cache_root = root.path().to_path_buf();
        let uploader = Arc::new(ConcurrentUploader::new(
            AttachmentTransaction::new().with_cache_root(cache_root.clone()),
        ));
        let id = uploader.with(|transaction| {
            transaction
                .add(&AddRequest::new(
                    AttachmentId::new("att-1"),
                    "photo.webp",
                    "content-hash-1",
                    1024,
                    StorageTargetId::new("s3-main"),
                    "s3:veloct-assets|uploads",
                    NOW,
                ))
                .unwrap();
            AttachmentId::new("att-1")
        });
        write_cache_file(&cache_root, &uploader, 1024);

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let uploader = uploader.clone();
                let target = target.clone();
                let barrier = barrier.clone();
                let id = id.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    uploader.upload(&id, &target, NOW)
                })
            })
            .collect();

        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();

        // Exactly one PUT reached the store, which is the guarantee. The other caller either waited
        // and reused the recorded object, or was refused because the upload had already SUCCEEDED —
        // both are correct, and neither creates a second remote object.
        assert_eq!(
            store.put_count(),
            1,
            "a second identical remote object must not be created: {outcomes:?}"
        );
        assert_eq!(store.keys().len(), 1);

        let succeeded = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
        let refused = outcomes.iter().filter(|outcome| outcome.is_err()).count();
        assert_eq!(succeeded + refused, 2);
        assert!(succeeded >= 1, "one caller must succeed: {outcomes:?}");
        for outcome in &outcomes {
            if let Err(error) = outcome {
                // A refusal is the state machine saying "already uploaded", not a spurious failure.
                assert!(
                    error.to_string().contains("must not be uploaded again"),
                    "an unexpected error: {error}"
                );
            }
        }
        // The two callers converged on one key, so the lock was shared rather than duplicated.
        assert_eq!(uploader.held_locks(), 1);
    }

    #[test]
    fn the_pool_reuses_one_record_for_identical_bytes_so_the_race_cannot_arise_from_two_adds() {
        // The reason the test above takes the form it does. Documented as a test because it is the
        // first line of defence and a reader should be able to see it.
        let uploader = ConcurrentUploader::new(AttachmentTransaction::new());
        let (first, second) = uploader.with(|transaction| {
            let first = transaction
                .add(&AddRequest::new(
                    AttachmentId::new("att-1"),
                    "photo.webp",
                    "content-hash-1",
                    1024,
                    StorageTargetId::new("s3-main"),
                    "s3:veloct-assets|uploads",
                    NOW,
                ))
                .unwrap();
            let second = transaction
                .add(&AddRequest::new(
                    AttachmentId::new("att-2"),
                    "photo.webp",
                    "content-hash-1",
                    1024,
                    StorageTargetId::new("s3-main"),
                    "s3:veloct-assets|uploads",
                    NOW,
                ))
                .unwrap();
            (first, second)
        });

        assert_eq!(first.id, second.id, "identical bytes are one record");
        assert_eq!(uploader.with(|transaction| transaction.pool().len()), 1);
    }

    #[test]
    fn two_concurrent_uploads_of_different_content_both_go_through() {
        // The per-key lock must not serialise unrelated files into a failure.
        let store = Arc::new(MemoryFileStore::new());
        let target = Arc::new(target_with(store.clone()));
        // A real cache root, because the upload now reads the bytes rather than sending zeros.
        let root = tempfile::TempDir::new().unwrap();
        let cache_root = root.path().to_path_buf();
        let uploader = Arc::new(ConcurrentUploader::new(
            AttachmentTransaction::new().with_cache_root(cache_root.clone()),
        ));

        for (id, hash) in [("att-1", "hash-1"), ("att-2", "hash-2")] {
            uploader.with(|transaction| {
                transaction
                    .add(&AddRequest::new(
                        AttachmentId::new(id),
                        "photo.webp",
                        hash,
                        512,
                        StorageTargetId::new("s3-main"),
                        "s3:veloct-assets|uploads",
                        NOW,
                    ))
                    .unwrap();
            });
        }
        write_cache_file(&cache_root, &uploader, 512);

        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = ["att-1", "att-2"]
            .into_iter()
            .map(|id| {
                let uploader = uploader.clone();
                let target = target.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    uploader.upload(&AttachmentId::new(id), &target, NOW)
                })
            })
            .collect();

        let outcomes: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert!(outcomes.iter().all(|outcome| outcome.is_ok()), "{outcomes:?}");
        assert_eq!(store.put_count(), 2, "two different files are two objects");
        assert_eq!(store.keys().len(), 2);
    }

    #[test]
    fn the_same_bytes_on_a_different_target_does_not_reuse_the_object() {
        // handoff.md section 13.4: a URL must never be reused across targets.
        let mut transaction = transaction();
        let target = target();
        let first = add(&mut transaction);
        let first_object = transaction.upload(&first, &target, NOW).unwrap().object;

        let mut other_config = config();
        other_config.id = StorageTargetId::new("github-main");
        other_config.kind = StorageKind::GitHub;
        other_config.repository = Some("vellow/assets".to_string());
        // A genuinely different target has its own public URL, which is what makes the assertion
        // meaningful rather than a comparison of two identical strings.
        other_config.public_url_prefix = "https://raw.githubusercontent.com/vellow/assets".to_string();
        let other = StorageTarget::new(
            other_config.clone(),
            Box::new(MemoryStore::new(other_config.clone())),
        );

        let added = transaction
            .add(
                &AddRequest::new(
                    AttachmentId::new("att-2"),
                    "photo.webp",
                    "content-hash-1",
                    1024,
                    StorageTargetId::new("github-main"),
                    other_config.storage_namespace(),
                    NOW,
                ),
            )
            .unwrap();
        assert_ne!(added.id, first, "a different target is a different record");
        assert_eq!(transaction.pool().len(), 2);

        let second = transaction.upload(&added.id, &other, NOW).unwrap();
        assert!(!second.reused, "a different target is a different object");
        assert_ne!(
            second.object.public_url, first_object.public_url,
            "each target has its own URL"
        );
    }

    #[test]
    fn the_remote_name_is_stable_for_the_same_content() {
        // handoff.md section 13.7: the same content reuses the suffix, so a retry produces the SAME
        // remote key instead of orphaning the URL an article points at.
        let mut first = transaction();
        let a = add(&mut first);
        let name_a = first.pool().get(&a).unwrap().remote_name();

        let mut second = transaction();
        let b = add(&mut second);
        let name_b = second.pool().get(&b).unwrap().remote_name();

        assert_eq!(name_a, name_b, "the same content gives the same remote name");
        assert!(name_a.starts_with("photo-"), "{name_a}");
        assert!(name_a.ends_with(".webp"), "{name_a}");
    }

    #[test]
    fn different_content_gets_a_different_suffix() {
        // handoff.md section 13.4: the same name with different content gets a new suffix.
        let mut transaction = transaction();
        let first = add(&mut transaction);
        transaction
            .add(
                &AddRequest::new(
                    AttachmentId::new("att-2"),
                    "photo.webp",
                    "content-hash-2",
                    1024,
                    StorageTargetId::new("s3-main"),
                    "uploads",
                    NOW,
                ),
            )
            .unwrap();

        let name_a = transaction.pool().get(&first).unwrap().remote_name();
        let name_b = transaction
            .pool()
            .get(&AttachmentId::new("att-2"))
            .unwrap()
            .remote_name();
        assert_ne!(name_a, name_b);
    }

    #[test]
    fn an_apply_that_appends_a_url_records_a_reference() {
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let outcome = transaction
            .apply(&apply_request(&id, "# Article\n"), NOW)
            .unwrap();
        assert!(!outcome.conflicted);
        let source = outcome.source.unwrap();
        assert!(source.contains("![photo.webp](https://cdn.example.com/"), "{source}");

        let set = transaction.references().get(&id).unwrap();
        assert_eq!(set.len(), 1);
        assert_eq!(set.status(), ReferenceStatus::Applied);
        assert_eq!(set.content_ids(), vec!["article-a"]);
    }

    #[test]
    fn an_apply_replaces_a_placeholder_exactly_once() {
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let source = "# A\n\n![pending](placeholder://one)\n\nand `placeholder://one` as text\n";
        let mut request = apply_request(&id, source);
        request.placeholder = "placeholder://one".to_string();

        let outcome = transaction.apply(&request, NOW).unwrap();
        let new_source = outcome.source.unwrap();
        assert_eq!(
            new_source.matches("https://cdn.example.com").count(),
            1,
            "only the first occurrence is replaced: {new_source}"
        );
        assert!(new_source.contains("`placeholder://one` as text"), "the second survives");
    }

    #[test]
    fn an_apply_whose_placeholder_is_gone_reports_a_conflict_rather_than_guessing() {
        // Overwriting something else, or appending blindly, would put the URL in the wrong place.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let mut request = apply_request(&id, "# A\n");
        request.placeholder = "placeholder://gone".to_string();

        let outcome = transaction.apply(&request, NOW).unwrap();
        assert!(outcome.conflicted);
        assert!(outcome.source.is_none(), "nothing was written");
        assert!(outcome.message.unwrap().contains("no longer in"));
        assert_eq!(
            transaction.pool().get(&id).unwrap().state.reference_status,
            ReferenceStatus::Failed
        );
    }

    #[test]
    fn an_apply_without_an_upload_is_refused() {
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let error = transaction
            .apply(&apply_request(&id, "# A\n"), NOW)
            .unwrap_err();
        assert!(error.to_string().contains("no remote URL"), "{error}");
    }

    #[test]
    fn applying_the_same_url_to_two_documents_keeps_both_references() {
        // handoff.md section 13.2.3: an attachment can be referenced by several documents.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        transaction
            .apply(&apply_request(&id, "# One\n"), NOW)
            .unwrap();

        let mut second = apply_request(&id, "# Two\n");
        second.content_id = "article-b".to_string();
        transaction.apply(&second, NOW).unwrap();

        let set = transaction.references().get(&id).unwrap();
        assert_eq!(set.len(), 2);
        assert!(set.is_shared());
        assert_eq!(set.status(), ReferenceStatus::Applied);
    }

    #[test]
    fn the_cache_is_released_only_after_a_reference_exists() {
        // Releasing it earlier would leave the file reachable only through a record the user has not
        // been shown.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let error = transaction.release_cache(&id).unwrap_err();
        assert!(error.to_string().contains("NOT_APPLIED"), "{error}");
        assert_eq!(
            transaction.pool().get(&id).unwrap().state.cache_status,
            CacheStatus::Present
        );

        transaction
            .apply(&apply_request(&id, "# A\n"), NOW)
            .unwrap();
        transaction.release_cache(&id).unwrap();
        assert_eq!(
            transaction.pool().get(&id).unwrap().state.cache_status,
            CacheStatus::Cleaned
        );
    }

    #[test]
    fn an_upload_only_run_leaves_the_attachment_awaiting_a_reference() {
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        let result = transaction.run(&id, &target, None, NOW).unwrap();
        assert!(matches!(result, TransactionResult::UploadReused { .. }));
        assert_eq!(result.object().unwrap().size_bytes, 1024);

        let stored = transaction.pool().get(&id).unwrap();
        assert_eq!(stored.state.upload_status, UploadStatus::Succeeded);
        assert_eq!(stored.state.reference_status, ReferenceStatus::NotApplied);
    }

    #[test]
    fn a_conflicted_apply_queues_a_reference_task_for_the_retry() {
        // So the retry has something to work from after a restart.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let mut request = apply_request(&id, "# A\n");
        request.baseline = FileRevision::of_text("stale");
        transaction.apply(&request, NOW).unwrap();

        assert_eq!(transaction.pool().tasks().len(), 1);
        let task = &transaction.pool().tasks()[0];
        assert_eq!(task.content_id, "article-a");
        assert!(task.remote_url.is_some(), "the URL is kept so the retry can apply it");
    }

    #[test]
    fn the_state_survives_a_serialize_and_restore_so_a_restart_resumes() {
        // handoff.md section 13.3: any failure must leave enough state to continue.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let saved = transaction.to_state().unwrap();
        assert!(saved.contains("SUCCEEDED"), "{saved}");

        let mut restored = AttachmentTransaction::from_state(&saved).unwrap();
        assert_eq!(restored.pool().len(), 1);

        // The restored transaction can finish the apply, and its remote name is unchanged.
        let before = transaction.pool().get(&id).unwrap().remote_name();
        let after = restored.pool().get(&id).unwrap().remote_name();
        assert_eq!(before, after, "the remote name is stable across a restart");

        let outcome = restored
            .retry_apply(&apply_request(&id, "# A\n"), NOW)
            .unwrap();
        assert!(!outcome.conflicted);
    }

    #[test]
    fn a_restored_transaction_does_not_re_upload_because_the_object_is_recorded() {
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let target = target();
        transaction.upload(&id, &target, NOW).unwrap();

        let mut restored =
            AttachmentTransaction::from_state(&transaction.to_state().unwrap()).unwrap();
        let outcome = restored.upload(&id, &target, NOW);
        assert!(outcome.is_err(), "an already-uploaded attachment is not uploaded again");
        assert!(outcome.unwrap_err().to_string().contains("must not be uploaded again"));
    }

    #[test]
    fn unreadable_persisted_state_is_reported() {
        let error = AttachmentTransaction::from_state("{ not json").unwrap_err();
        assert!(error.to_string().contains("unreadable"), "{error}");
    }

    #[test]
    fn a_transaction_result_reports_exactly_one_next_step() {
        let object = ObjectRecord {
            dedup_key: "k".to_string(),
            storage_target_id: StorageTargetId::new("t"),
            storage_namespace: "n".to_string(),
            content_hash: "h".to_string(),
            key: "uploads/x.webp".to_string(),
            public_url: "https://cdn/x.webp".to_string(),
            size_bytes: 1,
            created_at: NOW.to_string(),
        };

        let complete = TransactionResult::Complete {
            object: object.clone(),
            reference_status: ReferenceStatus::Applied,
        };
        assert!(complete.is_complete());
        assert!(!complete.allows_reupload());
        assert!(!complete.needs_apply_retry());

        let reference_failed = TransactionResult::ReferenceFailed {
            object: object.clone(),
            message: "conflict".to_string(),
        };
        assert!(!reference_failed.is_complete());
        assert!(reference_failed.needs_apply_retry());
        assert!(!reference_failed.allows_reupload());

        let upload_failed = TransactionResult::UploadFailed {
            message: "down".to_string(),
        };
        assert!(upload_failed.allows_reupload());
        assert!(!upload_failed.needs_apply_retry());
        assert!(upload_failed.object().is_none());
    }

    #[test]
    fn the_transaction_result_round_trips_through_json_for_the_ui() {
        let result = TransactionResult::UploadFailed {
            message: "down".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"stage\":\"upload_failed\""), "{json}");
        assert_eq!(
            serde_json::from_str::<TransactionResult>(&json).unwrap(),
            result
        );
    }

    // ---------------------------------------------------------------------------------------------
    // Section 13.8's EXPLICIT remote cleanup, and the record that decides where it may happen.
    // ---------------------------------------------------------------------------------------------

    /// A target that is genuinely different from `config()`: its own id, its own store, its own URL. The
    /// path prefix is the same, so an object's KEY looks the same on both — which is exactly why the record's
    /// target, and not the key, has to decide.
    fn other_target(store: Arc<MemoryFileStore>) -> StorageTarget {
        let mut other_config = config();
        other_config.id = StorageTargetId::new("github-main");
        other_config.kind = StorageKind::GitHub;
        other_config.public_url_prefix = "https://raw.example.com".to_string();
        StorageTarget::new(
            other_config.clone(),
            Box::new(MemoryStore::with_store(other_config, store)),
        )
    }

    #[test]
    fn the_object_record_names_the_target_the_bytes_were_uploaded_to() {
        // The record's job is to say where the object ACTUALLY is. The attachment was added for `s3-main`,
        // and this upload names a different target: the record must name the one whose adapter received the
        // bytes, because a remote cleanup refuses to trust a caller's claim instead of this field.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let store = Arc::new(MemoryFileStore::new());
        let other = other_target(store.clone());

        let outcome = transaction.upload(&id, &other, NOW).unwrap();

        assert_eq!(outcome.object.storage_target_id.as_str(), "github-main");
        assert_eq!(outcome.object.storage_namespace, other.namespace());
        assert!(
            outcome.object.public_url.starts_with("https://raw.example.com/"),
            "{}",
            outcome.object.public_url
        );
        assert_eq!(store.keys().len(), 1, "the bytes reached the named target");
        let recorded = transaction.objects().values().next().unwrap();
        assert_eq!(recorded.storage_target_id.as_str(), "github-main");
    }

    #[test]
    fn deleting_the_remote_object_clears_only_the_way_back_to_the_remote_file() {
        // Section 13.8: the REMOTE object goes, and the user's own record of the file stays. The attachment
        // no longer claims to be remote, its cached bytes are still there, its reference records are
        // untouched, and it may be uploaded again.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        transaction.references_mut().insert(
            &id,
            ReferenceTarget {
                content_id: "article-a".to_string(),
                route: None,
                occurrence: 0,
                node_hash: None,
            },
        );
        let store = Arc::new(MemoryFileStore::new());
        let target = target_with(store.clone());
        transaction.upload(&id, &target, NOW).unwrap();
        let record = transaction.objects().values().next().unwrap().clone();
        assert_eq!(store.keys().len(), 1);

        let outcome = transaction.delete_remote_object(&id, &target).unwrap();

        assert_eq!(outcome.storage_target_id, StorageTargetId::new("s3-main"));
        assert_eq!(outcome.key, record.key);
        assert_eq!(outcome.public_url, record.public_url);

        assert!(store.keys().is_empty(), "the remote object is gone");
        assert!(transaction.objects().is_empty(), "and its record with it");

        let stored = transaction.pool().get(&id).unwrap();
        assert!(!stored.is_remote(), "the attachment no longer claims to be remote");
        assert!(stored.remote_url.is_none());
        assert!(stored.remote_key.is_none());
        assert_eq!(stored.state.upload_status, UploadStatus::Pending);
        assert!(
            stored.state.upload_status.can_start_upload(),
            "a deleted object must not leave the attachment stuck (section 13.2.2)"
        );
        assert_eq!(
            stored.state.cache_status,
            CacheStatus::Present,
            "the cached bytes are left alone"
        );
        assert_eq!(
            transaction.references().count_for(&id),
            1,
            "the reference records are left alone"
        );
    }

    #[test]
    fn a_remote_delete_naming_a_different_target_removes_nothing() {
        // The caller's belief about where the object is must never be what decides which file is deleted.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let store = Arc::new(MemoryFileStore::new());
        let target = target_with(store.clone());
        transaction.upload(&id, &target, NOW).unwrap();
        assert_eq!(store.keys().len(), 1);

        let other_store = Arc::new(MemoryFileStore::new());
        let other = other_target(Arc::clone(&other_store));

        let error = transaction
            .delete_remote_object(&id, &other)
            .expect_err("a target that does not hold the object is refused");
        assert!(error.to_string().contains("s3-main"), "{error}");

        assert_eq!(store.keys().len(), 1, "the object on the real target survives");
        assert!(other_store.keys().is_empty(), "and nothing appeared on the named one");
        assert_eq!(transaction.objects().len(), 1, "the record is still there");
        assert!(
            transaction.pool().get(&id).unwrap().is_remote(),
            "and the attachment still claims to be remote"
        );
    }

    #[test]
    fn a_target_that_cannot_delete_remotely_is_refused_with_its_own_reason() {
        // ADR-0010: a silent success here would tell the user a remote object was cleaned up when nothing was.
        let mut transaction = transaction();
        let id = add(&mut transaction);
        let config = StorageConfig::new("server", "my server", StorageKind::Sftp);
        let unsupported = StorageTarget::new(
            config.clone(),
            Box::new(crate::targets::UnsupportedStore::new(config)),
        );

        let error = transaction
            .delete_remote_object(&id, &unsupported)
            .expect_err("a target with no adapter cannot delete");
        assert_eq!(error.code(), "NOT_CONFIGURED");
        // §13.10's SSH target was REMOVED (ADR-0017), so the reason names the removal rather than a protocol
        // gap: the caller has to be told the target no longer exists, not that an adapter is missing.
        assert!(
            error.hint().unwrap_or_default().contains("移除"),
            "{error} / {:?}",
            error.hint()
        );
        assert!(!unsupported.capabilities().can_delete_remotely);
        assert!(transaction.objects().is_empty());
    }

    #[test]
    fn remote_delete_outcome_is_reachable_from_the_crate_root() {
        // The crate-root `pub use` block IS this crate's public API, so a caller of `delete_remote_object`
        // must be able to name its outcome as `veloct_attach::RemoteDeleteOutcome` instead of reaching
        // through `veloct_attach::transaction::…`. Spelling the type through `crate::` — and `crate::` IS
        // what `veloct_attach::` resolves to for a caller — makes the re-export a COMPILE-time requirement:
        // drop it from that block and this test stops building, so the hole cannot return silently.
        let outcome: crate::RemoteDeleteOutcome = crate::RemoteDeleteOutcome {
            attachment_id: AttachmentId::new("att-1"),
            storage_target_id: StorageTargetId::new("s3-main"),
            key: "uploads/photo-a8k3xq.webp".to_string(),
            public_url: "https://cdn.example.com/uploads/photo-a8k3xq.webp".to_string(),
            state: AttachmentState::new(),
        };
        assert_eq!(outcome.attachment_id.as_str(), "att-1");
        assert_eq!(outcome.storage_target_id.as_str(), "s3-main");

        // And the root-exported name is the SAME type the delete path returns, not a second declaration
        // that merely looks like it.
        let delete: fn(
            &mut AttachmentTransaction,
            &AttachmentId,
            &StorageTarget,
        ) -> Result<crate::RemoteDeleteOutcome> = AttachmentTransaction::delete_remote_object;
        let _ = delete;
    }
}
