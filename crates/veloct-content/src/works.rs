//! The works system.
//!
//! `handoff.md` §10.1 fixes a work's truth source: `works/<stable-id>/work.json` holds the metadata
//! and the sibling `site/` holds the static body. SQLite only ever holds an index of either.
//!
//! That choice is what makes the offline promise true (§10.1: "no network needed to restore,
//! preview, or rebuild a historical release"). The consequence is that an import writes files and
//! nothing else, and a preview is served from the directory rather than from a database row.
//!
//! §10.1 also says a work's body is NOT moved to external attachment storage, even when it is large,
//! and that a large import may warn but must never change the truth source. So the size check
//! produces a warning and never a redirect.

pub mod import;
pub mod metadata;

pub use import::{
    detect_entry, detect_wrapper, export_work, import_from_directory, import_from_zip, import_work,
    plan_import, safe_relative_path, ImportOptions, ImportOutcome, ImportPlan, ImportSource,
    PlannedFile, SizeWarning, SlugCollision, MAX_RECOMMENDED_BYTES, SKIPPED_DIRECTORIES,
};
pub use metadata::{
    list_works, read_work, write_work, WorkMetadata, WorkRecord, WorkSource, WorkSourceKind,
    WORK_JSON,
};
