//! SQLite persistence: versioned migrations, the v1 schema and repositories.
//!
//! The schema contract is `docs/spec/01-data-model.md` §3 and the status machines are §6.
//! Content bodies are NEVER stored here: `contents` and friends are a derived index that can be
//! rebuilt from Markdown (`handoff.md` §2.3, §3.4, §20.1).
//!
//! Ownership: `core-storage` (team `veloct-complete`).

#![forbid(unsafe_code)]
