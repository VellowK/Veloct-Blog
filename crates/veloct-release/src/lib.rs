//! The release truth model and the publish pipeline.
//!
//! `handoff.md` §14 is the contract. The load-bearing rules:
//!
//! * `release.json` is written BEFORE the commit and must never contain its own commit sha.
//!   The truth source is `release.json` + the git tag `release/<release-id>` + the commit that
//!   tag points to ([`manifest::ReleaseManifest`] enforces the absence, and a test proves it).
//! * One publish builds Astro exactly once and produces one `dist/` with `dist/site/` and
//!   `dist/works/`. Every sub-target of that release gets the same `build_fingerprint`, computed
//!   once from the frozen artefacts rather than per target.
//! * Re-publishing a failed sub-target or rolling back uses the original commit and the frozen
//!   snapshots, never the current working tree ([`pipeline::load`] reads only frozen files).
//! * The release freezes the site template files it was built with, so a historical release stays
//!   rebuildable after the app upgrades its bundled template.
//!
//! The build itself sits behind [`pipeline::BuildRunner`], so the whole pipeline is testable with a
//! deterministic fake and no Node toolchain. That is what makes rows A2, A4, A5, A6, A14, A12 and
//! A13 of the test matrix provable rather than asserted.

#![forbid(unsafe_code)]

pub mod manifest;
pub mod pipeline;
pub mod publish;

pub use manifest::{
    canonical_json, canonical_json_pretty, sort_json_keys, BuildInputs, DeploymentTargetSnapshot,
    DistManifest, FingerprintInputs, ReleaseManifest, ReleasePaths, SubTargetSnapshot,
    ToolchainSnapshot, BUILD_ENV_ALLOWLIST, RELEASE_SCHEMA,
};
pub use publish::{
    publish, read_record, record_path, PublishOutcome, PublishRecord, PublishRequest,
};
pub use pipeline::{
    build_once, freeze, load, record_dist_manifest, scan_dist, BuildOutcome, BuildRequest,
    BuildRunner, FailingBuildRunner, FreezeRequest, ReleaseArtifacts,
};
