//! SQLite persistence: versioned migrations, the v1 schema and the rebuildable content index.
//!
//! The schema contract is `docs/spec/01-data-model.md` §3 and the status machines are §6. Three
//! rules shape everything here:
//!
//! * **Content bodies are never stored.** The `contents`, `content_paragraphs` and `content_links`
//!   tables are a derived index that can be wiped and rebuilt from Markdown
//!   (`handoff.md` §2.3, §3.4, §20.1). [`rebuild`] is that operation and row E1 of the test matrix
//!   proves it is lossless.
//! * **No column may hold a secret value.** Credentials are `credential_ref` strings resolved
//!   through the operating system credential store (`handoff.md` §12). A test scans every column
//!   name in the schema for anything that looks like a stored secret.
//! * **State is modelled as it is described.** Attachment state is three independent axes
//!   (`handoff.md` §13.2.1), `reference_status` is an aggregate over a real reference set (§13.2.3),
//!   and a logical deployment target can be partially successful (§14.12). Those are separate types
//!   in [`models`], not one string.
//!
//! Configuration, releases, attachments and backups share this one database (`handoff.md` §2.4) and
//! are never touched by an index rebuild.

#![forbid(unsafe_code)]

pub mod ai_settings;
pub mod db;
pub mod migrations;
pub mod models;
pub mod rebuild;

pub use ai_settings::{
    read_fallback, read_features, read_models, read_providers, read_settings, record_request,
    replace_models, set_fallback, set_feature, upsert_provider, delete_provider, AiFallbackRow,
    AiFeatureRow, AiModelRow, AiProviderRow, AiRequestRecord, AiSettings,
};
pub use db::{open, open_at, open_in_memory, MigrationReport};
// Re-exported because most of this crate's public API takes a `&Connection`: a caller that can call
// `read_settings` must be able to NAME the type it passes, without taking a direct dependency on the
// database library the shell has no other reason to know about.
pub use rusqlite::Connection;
pub use migrations::CURRENT_SCHEMA_VERSION;
pub use models::{
    roll_up_logical_status, AiProtocol, AiRequestRow, AiRequestStatus, AttachmentRow,
    AttachmentTaskKind, AttachmentTaskStatus, BackupKind, BackupStatus, BackupTargetKind,
    CacheStatus, ContentLinkRow, ContentParagraphRow, ContentRow, DeployTargetKind,
    DeploymentRunRow, DeploymentStatus, LogicalTargetStatus, ReferenceLinkStatus, ReferenceStatus,
    ReleaseRow, StorageKind, SubTarget, UploadStatus,
};
pub use rebuild::{index_health, read_index, rebuild, wipe_index, IndexSnapshot, RebuildReport};
