//! Backup, export and restore.
//!
//! `handoff.md` §17 and §21.5 are the contract:
//!
//! * The archive is a complete package of the project directory — Markdown, SQLite, config, the
//!   git repository (including release tags and metadata), the Astro project, the current
//!   `dist/`, the local attachment cache — and it is **not encrypted**.
//! * No secret ever enters it. A secret scan over the working tree **and** git objects runs
//!   first (`handoff.md` §17.3); a positive finding blocks a silent cloud upload and requires
//!   explicit user confirmation.
//! * Cloud targets must be private: GitHub repositories and S3 buckets.
//! * Retention defaults to 3 per target and is configurable; one failing target never fails
//!   the others.
//! * Restore is "unpack to a temporary location → verify → confirm → replace", never a partial
//!   overwrite. Content-only restore leaves historical releases, git tags, deployment targets,
//!   AI providers, credentials and app settings untouched.
//! * The system never rewrites git history to "clean" a secret.

#![forbid(unsafe_code)]

pub mod archive;
pub mod github;
pub mod restore;
pub mod s3;
pub mod secrets;
pub mod selection;
pub mod targets;

pub use archive::{
    archive_size, read_manifest, safe_entry_path, unpack_to, verify_staging, write_archive,
    ArchiveOutcome, BackupManifest, BackupTrigger, ExcludedEntry, ManifestEntry, ARCHIVE_SCHEMA,
    MANIFEST_NAME,
};
pub use restore::{
    preservation_reason, Restore, RestoreMode, RestoreOutcome, StagedRestore,
    CONTENT_ONLY_PRESERVED,
};
pub use secrets::{
    history_not_scanned, scan_files, scan_history, BackupSecretScan, BlobSource, Finding,
    ScanSurface,
};
pub use selection::{absolute, select, BackupSelection, SkippedPath, EXCLUSIONS};
pub use github::{
    archive_created_at, is_backup_name, GitHubBackupTarget, BACKUP_NAME_PREFIX, BACKUP_NAME_SUFFIX,
};
pub use s3::{s3_config, S3BackupTarget};
pub use targets::{
    adapter_from_target, by_id, is_implemented, prune, prune_detailed, remote_name, run_targets,
    unimplemented_reason, AutoBackupTimer, BackupRun, BackupTargetAdapter, CloudTarget,
    CloudTargetKind, MemoryTarget, PruneOutcome, ReportingTarget, TargetHandle, TargetResult,
    TargetSelection, TargetStatus, DEFAULT_RETENTION, IDLE_BEFORE_AUTOMATIC_BACKUP,
};
