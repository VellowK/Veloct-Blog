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
//! Vendor protocols (S3, Lsky Pro, GitHub) are tested against local fakes and the test names say so. A
//! protocol that could not be verified is reported as unconfigured rather than pretended to work, and a kind
//! the CONTRACT no longer names — §13.10's SSH/SFTP target, removed on 2026-10-02 (ADR-0017) — is reported as
//! removed, because a target saved before that decision must still load and must not look usable.

#![forbid(unsafe_code)]

pub mod lsky;
pub mod naming;
pub mod network;
pub mod pool;
pub mod reference;
pub mod s3;
pub mod state;
pub mod storage;
pub mod targets;
pub mod transaction;

pub use naming::{
    dedup_key, dedup_key_string, is_safe_file_name, random_suffix, remote_file_name, DedupKey,
    SUFFIX_LENGTH,
};
pub use network::{
    is_network_image, network_image_notice, ImageResult, ImageSearch, ImageSource,
    NetworkImageChoice,
};
/// The provider contracts, re-exported for the command surface.
///
/// `veloct_attach::network::providers` stays public too, so a caller that needs
/// `providers::search_request`, `providers::parse_search`, `providers::facts` or
/// `providers::is_implemented` reaches them by module path. Those four are deliberately NOT hoisted to
/// the crate root: `is_implemented` and `unimplemented_reason` already exist there for
/// `StorageKind`, and two free functions of the same name meaning different kinds would be a trap.
pub use network::providers::{
    attribution_markdown, download_tracking_request, report_download, search as search_images,
    source_notice, AuthScheme, HotlinkPolicy, ProviderFacts, SearchQuery, PEXELS_API,
    PEXELS_MAX_PER_PAGE, PEXELS_SEARCH_PATH, UNSPLASH_API, UNSPLASH_ATTRIBUTION_LINK,
    UNSPLASH_MAX_PER_PAGE, UNSPLASH_SEARCH_PATH, UNSPLASH_UTM_SOURCE,
};
pub use pool::{
    checked_cache_path, validate_original_name, Attachment, AttachmentId, CachePool, CleanupOutcome,
    CleanupRequest, InsertOutcome, PoolListing, ReferenceTask,
};
pub use reference::{ReferenceIndex, ReferenceSet, ReferenceTarget};
pub use state::{
    is_safe_orphan, AttachmentState, CacheStatus, ReferenceStatus, StorageTargetId, UploadStatus,
};
pub use lsky::{api_base as lsky_api_base, LskyStore};
pub use s3::{s3_config, S3Store};
pub use targets::{
    base64_encode, is_implemented, key_for, local_root, local_target, target_from_config,
    unimplemented_reason, url_for, GitHubStore, Repository, UnsupportedStore, GITHUB_API,
};
pub use storage::{
    content_type_for, LocalFileStore, MemoryFileStore, ObjectRecord, PutOutcome, RemoteObject,
    StorageAdapter, StorageCapabilities, StorageConfig, StorageKind, StorageTarget,
};
pub use transaction::{
    AddRequest, ApplyOutcome, ApplyRequest, AttachmentTransaction, RemoteDeleteOutcome,
    TransactionResult, UploadOutcome,
};
