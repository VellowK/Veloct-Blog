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
//!
//! Ownership: `systems-ai-attach` (team `veloct-complete`).

#![forbid(unsafe_code)]
