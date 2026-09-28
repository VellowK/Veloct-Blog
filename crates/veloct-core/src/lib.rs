//! Veloct domain primitives.
//!
//! This crate is deliberately dependency-light and Tauri-free: everything else in the
//! workspace builds on it, and the rules encoded here (stable ids, secret redaction, path
//! safety, project-level locking) are the ones `handoff.md` marks 【锁定】.

#![forbid(unsafe_code)]

pub mod clock;
pub mod error;
pub mod http;
pub mod ids;
pub mod lock;
pub mod paths;
pub mod secret;
pub mod workdir;

pub use clock::{Clock, FixedClock, SystemClock};
pub use error::{Result, VeloctError};
pub use http::{Cancellation, HttpClient, HttpMethod, HttpRequest, HttpResponse, ReqwestHttpClient};
pub use ids::{ReleaseId, StableId};
pub use lock::{LockMode, ProjectLock};
pub use paths::RelPath;
pub use secret::{redact, redact_json, Redacted, SecretFinding, SecretKind, SecretScanReport, scan_for_secrets};

/// Crate version, surfaced in diagnostics and release toolchain snapshots.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Lowercase hex encoding, used for content hashes and random filename suffixes.
pub fn hex_lower(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

/// SHA-256 over an arbitrary byte slice, returned as lowercase hex.
///
/// This is *the* hash function for content hashes, dedup keys and build fingerprints, so it
/// is defined once here rather than re-implemented per crate.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// SHA-256 over a UTF-8 string.
pub fn sha256_hex_str(value: &str) -> String {
    sha256_hex(value.as_bytes())
}

/// Produce a canonical hash over an ordered list of `(label, value)` pairs.
///
/// Used for `build_inputs_hash` and `build_fingerprint` (`handoff.md` §14.8): the component
/// name is hashed together with its value with an unambiguous separator so that two different
/// input sets can never collide by concatenation ambiguity.
pub fn hash_parts<'a, I>(parts: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut buf = Vec::new();
    for (label, value) in parts {
        buf.extend_from_slice(label.as_bytes());
        buf.push(0x1f); // unit separator
        buf.extend_from_slice(value.as_bytes());
        buf.push(0x1e); // record separator
    }
    sha256_hex(&buf)
}

/// Default on-disk locations inside a Veloct project, relative to the project root.
pub mod layout {
    /// Project metadata directory (contains the SQLite DB and release snapshots).
    pub const VELOCT_DIR: &str = ".veloct";
    /// SQLite database file name.
    pub const DB_FILE: &str = "veloct.db";
    /// Project identity file.
    pub const PROJECT_FILE: &str = "project.json";
    /// Per-release snapshot directory.
    pub const RELEASES_DIR: &str = "releases";
    /// Rotated log directory.
    pub const LOGS_DIR: &str = "logs";
    /// Markdown content root.
    pub const CONTENT_DIR: &str = "content";
    /// Knowledge base root (a sibling of `content`, not a child: `handoff.md` §9).
    pub const KNOWLEDGE_DIR: &str = "knowledge";
    /// Work truth root.
    pub const WORKS_DIR: &str = "works";
    /// The Astro project inside a managed project.
    pub const SITE_DIR: &str = "site";
    /// Build output root; holds `site/` and `works/` publish roots.
    pub const DIST_DIR: &str = "dist";
    /// Main site publish root inside `dist/`.
    pub const DIST_SITE: &str = "dist/site";
    /// Works origin publish root inside `dist/`.
    pub const DIST_WORKS: &str = "dist/works";
    /// Local attachment cache pool.
    pub const CACHE_DIR: &str = ".attachments-cache";
    /// Non-Markdown build inputs exported from SQLite before a build.
    pub const DATA_DIR: &str = "data";
}
