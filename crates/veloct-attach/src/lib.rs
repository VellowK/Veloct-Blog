//! The attachment cache pool, its reference set, dedup and the storage adapters.
//!
//! `handoff.md` §13 and §24 are the contract. The rules that are most often got wrong:
//!
//! * State is three independent axes — `upload_status`, `reference_status`, `cache_status` —
//!   never one large enum.
//! * `reference_status` aggregates over a real reference SET. Removing one of several
//!   references must not make the attachment unreferenced.
//! * Only a "safe orphan" may be cleaned in bulk: no reference, no queued apply task, no
//!   in-flight upload, no resumable task. `PENDING`/`UPLOADING` may never be bulk-deleted.
//! * A remote file is NEVER deleted automatically.
//! * Dedup key is `storage_target_id + storage_namespace + content_hash`, so a URL is never
//!   reused across targets.
//!
//! Vendor protocols (S3, Lsky Pro, GitHub, SFTP) are tested against local fakes and the test
//! names say so. A protocol that could not be verified is reported as unconfigured rather than
//! pretended to work.

#![forbid(unsafe_code)]

pub mod naming;
pub mod network;
pub mod pool;
pub mod reference;
pub mod state;
pub mod storage;
pub mod transaction;

pub use naming::{
    dedup_key, dedup_key_string, is_safe_file_name, random_suffix, remote_file_name, DedupKey,
    SUFFIX_LENGTH,
};
pub use network::{
    is_network_image, network_image_notice, ImageResult, ImageSearch, ImageSource,
    NetworkImageChoice,
};
pub use pool::{
    checked_cache_path, validate_original_name, Attachment, AttachmentId, CachePool, CleanupOutcome,
    CleanupRequest, InsertOutcome, PoolListing, ReferenceTask,
};
pub use reference::{ReferenceIndex, ReferenceSet, ReferenceTarget};
pub use state::{
    is_safe_orphan, AttachmentState, CacheStatus, ReferenceStatus, StorageTargetId, UploadStatus,
};
pub use storage::{
    LocalFileStore, MemoryFileStore, ObjectRecord, PutOutcome, RemoteObject, StorageAdapter,
    StorageCapabilities, StorageConfig, StorageKind, StorageTarget,
};
pub use transaction::{
    AddRequest, ApplyOutcome, ApplyRequest, AttachmentTransaction, TransactionResult,
    UploadOutcome,
};
