//! The release truth model and the publish pipeline.
//!
//! `handoff.md` §14 is the contract. The load-bearing rules:
//!
//! * `release.json` is written BEFORE the commit and must never contain its own commit sha.
//!   The truth source is `release.json` + the git tag `release/<release-id>` + the commit that
//!   tag points to.
//! * One publish builds Astro exactly once and produces one `dist/` with `dist/site/` and
//!   `dist/works/`. Every sub-target of that release gets the same `build_fingerprint`.
//! * Re-publishing a failed sub-target or rolling back uses the original commit and the frozen
//!   snapshots, never the current working tree.
//! * The release freezes the site template files it was built with, so a historical release
//!   stays rebuildable after the app upgrades its bundled template.
//!
//! Ownership: `core-git-release` (team `veloct-complete`).

#![forbid(unsafe_code)]
