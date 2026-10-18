//! Deployment targets, sub-targets and adapters.
//!
//! `handoff.md` §10.5, §14.7 and §14.10-§14.13 are the contract:
//!
//! * Three logical targets — Cloudflare Pages, EdgeOne · 中国大陆, EdgeOne · 国际 — are
//!   independently selectable.
//! * Each logical target owns a `main` and a `works` sub-target, and each sub-target reports its
//!   own status, so a logical target can be PARTIAL. A successful half is never retracted.
//! * Deployment results live in `DeploymentRun` rows and are never written back into
//!   `release.json`.
//! * A failed sub-target can be re-sent independently, using the original release's commit and
//!   the frozen `main_target` / `works_target` snapshot.
//! * Failure never auto-retries.
//! * A target that cannot provide an independent Works Origin must report itself as degraded
//!   and must never claim strong isolation (`handoff.md` §10.4 last paragraph).
//! * SEO push is an independent post-publish task whose failure never fails the site release.
//! * 1Panel is not an adapter: it is the manual upload of the current `dist/site` and
//!   `dist/works`.
//!
//! Vendor APIs are not invented. Cloudflare Pages is implemented against its documented direct
//! upload API and tested against a local fake; EdgeOne is a manual/export target until a
//! verified API definition exists (ADR-0012).

#![forbid(unsafe_code)]

pub mod adapter;
pub mod edgeone;
pub use edgeone::EdgeOnePagesAdapter;
pub mod pipeline;
pub mod seo;
pub mod target;

pub use adapter::{
    CloudflarePagesAdapter, DeployAdapter, DeployCapabilities, ExportManifest, ExportRoot,
    ManualExportAdapter, MemoryAdapter, PushFile, PushOutcome, PushPayload,
};
pub use seo::{
    push, record_rows, rows_for, ConfigurationRequiredSubmitter, RecordingSubmitter, SeoEngine,
    SeoEngineOutcome, SeoEngineRecord, SeoPushRequest, SeoPushRow, SeoPushRun, SeoSubmitter,
};
pub use pipeline::{
    Orchestrator, PublishPayloads, PublishResult, ResendContext, SeoTaskState, TargetAdapters,
};
pub use target::{
    roll_up, DistDecision, FrozenTarget, LogicalStatus, LogicalTargetRun, RollbackPlan, SubTarget,
    SubTargetRun, SubTargetStatus, TargetConfig, TargetKind, TargetSnapshot,
};
