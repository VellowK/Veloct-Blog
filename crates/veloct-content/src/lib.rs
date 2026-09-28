//! The content model and truth-source I/O.
//!
//! `handoff.md` §3.4 fixes the direction of the data flow: Markdown is the truth, everything else is
//! derived. So every write in this crate targets a Markdown file (or `work.json`), and nothing here
//! ever writes a body into SQLite. That is the "Markdown ←→ SQLite two-way competition" the section
//! forbids, and it is avoided by construction rather than by discipline: the storage crate only ever
//! receives documents to index.
//!
//! The three rules this crate exists to enforce:
//!
//! * **A stable id never changes.** Files are addressed by id, so renaming a slug never moves a file
//!   (`handoff.md` §3.2).
//! * **An external edit is never silently overwritten.** Every save carries the revision the editor
//!   started from, and a change on disk turns the save into an explicit conflict
//!   (`handoff.md` §4.3 with §20.2).
//! * **A broken reference stays broken.** Repair rewrites a reference only to a target the caller
//!   names, never to a guess, because guessing a different paragraph is exactly what §9.3 forbids.

#![forbid(unsafe_code)]

pub mod index;
pub mod lifecycle;
pub mod model;
pub mod paragraphs;
pub mod service;
pub mod store;

pub use index::{ContentIndex, IndexEntry, IndexIssue, IssueSeverity};
pub use lifecycle::{FinishWipOutcome, WipTransition};
pub use model::{
    ConflictKind, ContentKind, ContentPaths, DocumentRef, ExistingFile, LoadedDocument,
    SaveConflict, SaveOutcome, SaveStatus, Workspace,
};
pub use paragraphs::{excerpt, extract_paragraphs, ExtractedParagraph};
pub use service::{ContentService, CreateOptions, ReferenceRepair, RibbonEntry};
pub use store::{list_documents, read_document, write_document, DocumentPath};
