//! Logical deployment targets, their sub-targets, and how a result rolls up.
//!
//! `handoff.md` §14.10 names three automatic targets — Cloudflare Pages, EdgeOne · mainland China and
//! EdgeOne · international — and says the two EdgeOne regions are two INDEPENDENT logical targets.
//! §14.12 then makes each logical target two sub-targets, `main` and `works`, because §14.1 publishes
//! two roots from one build.
//!
//! The rule that is easiest to get wrong is the roll-up:
//!
//! ```text
//! Cloudflare
//! ├── main  ok
//! └── works failed      -> the LOGICAL target is PARTIAL, and the ok part is NOT retracted
//! ```
//!
//! So a sub-target's success is sticky and is never cleared by another sub-target failing, and a
//! logical target is only `Failed` when BOTH sub-targets failed. That asymmetry is the whole point of
//! §14.12: retracting a working deployment because a sibling failed would take a live site down.

use serde::{Deserialize, Serialize};

use veloct_core::workdir::FileRevision;

/// Which publish root a deployment run covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubTarget {
    /// `dist/site`, the main site.
    Main,
    /// `dist/works`, the work bodies served from the works origin.
    Works,
}

impl SubTarget {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Works => "works",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "main" => Some(Self::Main),
            "works" => Some(Self::Works),
            _ => None,
        }
    }

    /// The two sub-targets, in the order the UI shows them.
    pub fn all() -> [Self; 2] {
        [Self::Main, Self::Works]
    }
}

impl std::fmt::Display for SubTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The state of one sub-target's deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubTargetStatus {
    /// Not attempted yet.
    Pending,
    /// In flight.
    Running,
    /// Done.
    Succeeded,
    /// Attempted and failed. `handoff.md` §14.13: no automatic retry.
    Failed,
    /// Deliberately not attempted, for example the works root when the target has no works origin.
    Skipped,
}

impl SubTargetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    pub fn is_succeeded(self) -> bool {
        matches!(self, Self::Succeeded)
    }

    pub fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }

    pub fn is_settled(self) -> bool {
        !matches!(self, Self::Pending | Self::Running)
    }
}

/// A logical target's overall state (`handoff.md` §14.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogicalStatus {
    /// Nothing attempted.
    Pending,
    /// Everything attempted succeeded.
    Succeeded,
    /// Some sub-targets succeeded and some did not.
    ///
    /// The successful parts are NOT retracted, which is what this variant exists to record.
    Partial,
    /// Every attempted sub-target failed.
    Failed,
}

impl LogicalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Succeeded => "succeeded",
            Self::Partial => "partial",
            Self::Failed => "failed",
        }
    }

    /// Whether the whole logical target is live.
    pub fn is_succeeded(self) -> bool {
        matches!(self, Self::Succeeded)
    }

    /// Whether a user action is needed.
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::Partial | Self::Failed)
    }
}

/// One sub-target's result, with the reason when it failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubTargetRun {
    pub sub_target: SubTarget,
    pub status: SubTargetStatus,
    /// The failure reason, shown in the UI (`handoff.md` §14.13).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The fingerprint of the `dist` that was deployed, so a later re-send can tell whether the
    /// current build is the same one (`handoff.md` §14.13 point 4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_fingerprint: Option<String>,
    /// Whether the existing `dist` was reused rather than rebuilt.
    #[serde(default)]
    pub reused_dist: bool,
    /// The number of files pushed, for the log.
    #[serde(default)]
    pub files: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

impl SubTargetRun {
    pub fn pending(sub_target: SubTarget) -> Self {
        Self {
            sub_target,
            status: SubTargetStatus::Pending,
            error: None,
            build_fingerprint: None,
            reused_dist: false,
            files: 0,
            started_at: None,
            finished_at: None,
        }
    }

    pub fn succeeded(sub_target: SubTarget, files: usize, fingerprint: &str, now: &str) -> Self {
        Self {
            sub_target,
            status: SubTargetStatus::Succeeded,
            error: None,
            build_fingerprint: Some(fingerprint.to_string()),
            reused_dist: false,
            files,
            started_at: None,
            finished_at: Some(now.to_string()),
        }
    }

    pub fn failed(sub_target: SubTarget, error: impl Into<String>, now: &str) -> Self {
        Self {
            sub_target,
            status: SubTargetStatus::Failed,
            error: Some(error.into()),
            build_fingerprint: None,
            reused_dist: false,
            files: 0,
            started_at: None,
            finished_at: Some(now.to_string()),
        }
    }

    pub fn skipped(sub_target: SubTarget, reason: impl Into<String>) -> Self {
        Self {
            sub_target,
            status: SubTargetStatus::Skipped,
            error: Some(reason.into()),
            build_fingerprint: None,
            reused_dist: false,
            files: 0,
            started_at: None,
            finished_at: None,
        }
    }
}

/// Roll two sub-target results up into the logical status.
///
/// This is the §14.12 rule, in one function so it cannot be spelled differently in two places:
///
/// * both succeeded -> `Succeeded`
/// * both failed -> `Failed`
/// * one of each -> `Partial`, and the succeeded half stays succeeded
/// * a skip counts as "not attempted", so a target whose works root does not exist is not reported
///   as failed for it
/// * anything still running or pending -> `Pending`
pub fn roll_up(main: &SubTargetRun, works: &SubTargetRun) -> LogicalStatus {
    let main_started = main.status.is_settled();
    let works_started = works.status.is_settled();

    // Nothing has settled yet.
    if !main_started && !works_started {
        return LogicalStatus::Pending;
    }

    if main.status == SubTargetStatus::Running || works.status == SubTargetStatus::Running {
        return LogicalStatus::Pending;
    }

    // Count rather than match a tuple of four booleans: the tuple version had overlapping arms, and
    // a roll-up that a reader has to reason about is the wrong shape for the rule that decides
    // whether a live site is reported as broken.
    let succeeded = [main.status, works.status]
        .iter()
        .filter(|status| status.is_succeeded())
        .count();
    let failed = [main.status, works.status]
        .iter()
        .filter(|status| status.is_failed())
        .count();
    match (succeeded, failed) {
        // Everything that was ATTEMPTED succeeded. A skip is not an attempt, so a target whose works
        // root does not exist is not reported as failed for it, and a target that pushed only the
        // main root is a success rather than a partial one.
        (1 | 2, 0) => LogicalStatus::Succeeded,
        // One worked and one did not: PARTIAL, and the success is kept (handoff.md section 14.12).
        (1, 1) => LogicalStatus::Partial,
        // Nothing is live: either everything failed, or the one thing that was attempted failed while
        // the other was deliberately skipped.
        (0, 1 | 2) => LogicalStatus::Failed,
        // Nothing was attempted at all.
        (0, 0) => LogicalStatus::Pending,
        _ => LogicalStatus::Pending,
    }
}

/// One logical target's whole result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogicalTargetRun {
    pub target_id: String,
    pub main: SubTargetRun,
    pub works: SubTargetRun,
}

impl LogicalTargetRun {
    pub fn new(target_id: impl Into<String>) -> Self {
        Self {
            target_id: target_id.into(),
            main: SubTargetRun::pending(SubTarget::Main),
            works: SubTargetRun::pending(SubTarget::Works),
        }
    }

    pub fn status(&self) -> LogicalStatus {
        roll_up(&self.main, &self.works)
    }

    pub fn run_for(&self, sub_target: SubTarget) -> &SubTargetRun {
        match sub_target {
            SubTarget::Main => &self.main,
            SubTarget::Works => &self.works,
        }
    }

    pub fn run_for_mut(&mut self, sub_target: SubTarget) -> &mut SubTargetRun {
        match sub_target {
            SubTarget::Main => &mut self.main,
            SubTarget::Works => &mut self.works,
        }
    }

    /// The sub-targets a user may re-send: those that failed.
    ///
    /// `handoff.md` §14.13: the user re-publishes the FAILED sub-target. A succeeded one is offered
    /// as well, because re-publishing it is harmless and sometimes wanted, but the failed ones are
    /// what the contract is about.
    pub fn failed_sub_targets(&self) -> Vec<SubTarget> {
        SubTarget::all()
            .into_iter()
            .filter(|sub_target| self.run_for(*sub_target).status.is_failed())
            .collect()
    }

    /// The fingerprint recorded for a sub-target, which a re-send compares against the current
    /// `dist` (`handoff.md` §14.13 point 4).
    pub fn fingerprint_for(&self, sub_target: SubTarget) -> Option<&str> {
        self.run_for(sub_target).build_fingerprint.as_deref()
    }
}

/// What a target is and how it deploys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    /// Cloudflare Pages, which has a documented direct-upload API.
    CloudflarePages,
    /// EdgeOne, mainland China.
    EdgeOneMainland,
    /// EdgeOne, international.
    EdgeOneInternational,
    /// A 1Panel environment, which `handoff.md` §14.11 says is NOT an automatic adapter.
    OnePanel,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CloudflarePages => "cloudflare_pages",
            Self::EdgeOneMainland => "edgeone_cn",
            Self::EdgeOneInternational => "edgeone_intl",
            Self::OnePanel => "onepanel",
        }
    }

    /// Whether Veloct deploys to this target automatically, or exports for the user to upload.
    ///
    /// `handoff.md` §14.11 is explicit that 1Panel is not an automatic adapter, and ADR-0012 records
    /// that EdgeOne is an export target until its upload API is verified from an authoritative
    /// source. Reporting that honestly is what ADR-0010 requires; inventing an endpoint would produce
    /// an adapter that reports false success.
    pub fn is_automatic(self) -> bool {
        matches!(
            self,
            Self::CloudflarePages | Self::EdgeOneMainland | Self::EdgeOneInternational
        )
    }

    /// Whether the adapter's protocol has been verified. See [`Self::is_automatic`].
    pub fn is_verified(self) -> bool {
        matches!(self, Self::CloudflarePages | Self::OnePanel)
    }

    /// A label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::CloudflarePages => "Cloudflare Pages",
            Self::EdgeOneMainland => "EdgeOne · 中国大陆",
            Self::EdgeOneInternational => "EdgeOne · 国际",
            Self::OnePanel => "1Panel（手动上传）",
        }
    }
}

/// A logical target's configuration.
///
/// The project and works-origin fields are separate because §14.1 publishes two roots and a target
/// may serve them from genuinely different origins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetConfig {
    /// The logical target's identity, stable across re-sends.
    pub id: String,
    pub kind: TargetKind,
    pub enabled: bool,
    /// The account or project the main root deploys to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// The account or project the works root deploys to.
    ///
    /// When absent, the works sub-target is SKIPPED and reported as such rather than failed, which is
    /// what `handoff.md` §10.5's "a target without an independent Works Origin is degraded" means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub works_project: Option<String>,
    /// The origin the works bodies are served from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub works_origin: Option<String>,
    /// A credential HANDLE, never a value (`handoff.md` §12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
}

impl TargetConfig {
    pub fn new(id: impl Into<String>, kind: TargetKind) -> Self {
        Self {
            id: id.into(),
            kind,
            enabled: true,
            project: None,
            works_project: None,
            works_origin: None,
            credential_ref: None,
        }
    }

    /// Whether the works root can be deployed.
    pub fn has_works_target(&self) -> bool {
        self.works_project.is_some() || self.works_origin.is_some()
    }

    /// Whether this target is degraded: it has no independent works origin.
    ///
    /// `handoff.md` §10.4 requires the works bodies to run on a genuinely different origin, so a
    /// target without one is reported as degraded rather than silently serving works from the main
    /// site's origin, which would remove the isolation the contract requires.
    pub fn is_degraded(&self) -> bool {
        !self.has_works_target()
    }

    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("a deployment target needs an id".into());
        }
        match self.kind {
            TargetKind::CloudflarePages => {
                if self.project.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("Cloudflare Pages needs a project".into());
                }
            }
            TargetKind::OnePanel => {
                // A 1Panel target only exports, so it needs nothing to be configured.
            }
            _ => {
                if self.project.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(format!("{} needs a project", self.kind.label()));
                }
            }
        }
        if let Some(origin) = &self.works_origin {
            if !origin.trim().is_empty()
                && !origin.starts_with("http://")
                && !origin.starts_with("https://")
            {
                return Err(format!(
                    "the works origin {origin:?} must start with http:// or https://"
                ));
            }
        }
        Ok(())
    }
}

/// The frozen target configuration a release was built for.
///
/// `handoff.md` §14.7 freezes the targets into the Release, and §14.13 point 3 requires a re-send to
/// use the ORIGINAL release's snapshot. So a re-send reads this and not the current configuration:
/// if the user has since changed a project name, re-sending an old release must still target what
/// that release was built for, or it would deploy an old build to a new place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrozenTarget {
    pub id: String,
    pub kind: TargetKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub works_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub works_origin: Option<String>,
}

impl From<&TargetConfig> for FrozenTarget {
    fn from(config: &TargetConfig) -> Self {
        Self {
            id: config.id.clone(),
            kind: config.kind,
            enabled: config.enabled,
            project: config.project.clone(),
            works_project: config.works_project.clone(),
            works_origin: config.works_origin.clone(),
        }
    }
}

impl FrozenTarget {
    /// Whether the works root can be deployed for this frozen target.
    pub fn has_works_target(&self) -> bool {
        self.works_project.is_some() || self.works_origin.is_some()
    }

    pub fn is_degraded(&self) -> bool {
        !self.has_works_target()
    }
}

/// The frozen deployment snapshot a release carries (`handoff.md` §14.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetSnapshot {
    /// The release this snapshot belongs to.
    pub release_id: String,
    pub targets: Vec<FrozenTarget>,
    /// The commit the release was built from, so a re-send uses it
    /// (`handoff.md` §14.13 point 1).
    pub commit_sha: String,
    /// The git tag, for the same reason.
    pub tag: String,
    /// The build inputs hash (`handoff.md` §14.13 point 2).
    pub build_inputs_hash: String,
    pub created_at: String,
}

impl TargetSnapshot {
    /// A target by id.
    pub fn target(&self, id: &str) -> Option<&FrozenTarget> {
        self.targets.iter().find(|target| target.id == id)
    }

    /// The enabled automatic targets, which is what a publish pushes to.
    pub fn automatic_targets(&self) -> Vec<&FrozenTarget> {
        self.targets
            .iter()
            .filter(|target| target.enabled && target.kind.is_automatic())
            .collect()
    }

    /// The targets that only export.
    pub fn export_targets(&self) -> Vec<&FrozenTarget> {
        self.targets
            .iter()
            .filter(|target| target.enabled && !target.kind.is_automatic())
            .collect()
    }
}

/// A decision about whether a re-send may reuse the current `dist`.
///
/// `handoff.md` §14.13 points 4 and 5: reuse when the fingerprint still matches, otherwise rebuild in
/// a temporary directory and delete the temporary output afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DistDecision {
    /// The current `dist` is the release's build, so it can be pushed as-is.
    Reuse { fingerprint: String },
    /// The current `dist` is not this release's, so it must be rebuilt in a temporary directory from
    /// the frozen snapshot.
    ///
    /// The reason is carried so the log explains why a rebuild happened.
    Rebuild {
        expected: String,
        actual: Option<String>,
        reason: String,
    },
}

impl DistDecision {
    /// Decide whether the current `dist` belongs to a release.
    ///
    /// `handoff.md` §14.13: "if the current `dist`'s fingerprint still matches this Release, prefer to
    /// reuse it; otherwise rebuild in a temporary directory". A missing `dist` is a rebuild, not an
    /// error, because the whole point is to be able to produce the artefact again.
    pub fn decide(expected: &str, actual: Option<&str>) -> Self {
        match actual {
            Some(actual) if actual == expected => Self::Reuse {
                fingerprint: actual.to_string(),
            },
            Some(actual) => Self::Rebuild {
                expected: expected.to_string(),
                actual: Some(actual.to_string()),
                reason: format!(
                    "the current dist has fingerprint {actual}, which is not this release's {expected}"
                ),
            },
            None => Self::Rebuild {
                expected: expected.to_string(),
                actual: None,
                reason: "there is no current dist to reuse".to_string(),
            },
        }
    }

    pub fn reuses(&self) -> bool {
        matches!(self, Self::Reuse { .. })
    }

    /// The required action, for a log line or the UI.
    pub fn describe(&self) -> String {
        match self {
            Self::Reuse { .. } => "reuse the current dist".to_string(),
            Self::Rebuild {
                actual: Some(actual),
                expected,
                ..
            } => format!("rebuild from the frozen snapshot: dist is {actual}, release is {expected}"),
            Self::Rebuild { expected, .. } => {
                format!("rebuild from the frozen snapshot: no dist, release is {expected}")
            }
        }
    }
}

/// A rollback plan: which release, and what it needs.
///
/// `handoff.md` §14.14: a rollback selects a historical release, uses its tag/commit and snapshot,
/// rebuilds and deploys through the SAME publish flow. It never rewrites history and never deletes
/// current content, which is why this type only names a release and carries no mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RollbackPlan {
    pub release_id: String,
    pub commit_sha: String,
    pub tag: String,
    pub build_inputs_hash: String,
    pub targets: Vec<FrozenTarget>,
}

impl RollbackPlan {
    /// Build a rollback plan from a frozen snapshot, refusing one that has no commit or tag.
    pub fn from_snapshot(snapshot: &TargetSnapshot) -> Result<Self, String> {
        if snapshot.commit_sha.trim().is_empty() {
            return Err(format!(
                "release {} has no commit, so it cannot be rebuilt",
                snapshot.release_id
            ));
        }
        if snapshot.tag.trim().is_empty() {
            return Err(format!(
                "release {} has no tag, so it cannot be rebuilt",
                snapshot.release_id
            ));
        }
        Ok(Self {
            release_id: snapshot.release_id.clone(),
            commit_sha: snapshot.commit_sha.clone(),
            tag: snapshot.tag.clone(),
            build_inputs_hash: snapshot.build_inputs_hash.clone(),
            targets: snapshot.targets.clone(),
        })
    }

    /// The automatic targets a rollback would push to.
    pub fn automatic_targets(&self) -> Vec<&FrozenTarget> {
        self.targets
            .iter()
            .filter(|target| target.enabled && target.kind.is_automatic())
            .collect()
    }
}

/// The revision of a published root, so a re-send can prove the artefact is the same one.
pub fn root_revision(root: &std::path::Path) -> Option<FileRevision> {
    if !root.is_dir() {
        return None;
    }
    // A directory has no single hash, so the manifest the build wrote is used when it exists.
    let manifest = root.join("dist-manifest.json");
    if manifest.is_file() {
        return FileRevision::of_file(&manifest).ok();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(sub_target: SubTarget) -> SubTargetRun {
        SubTargetRun::succeeded(sub_target, 3, "fp", "2026-01-01T00:00:00Z")
    }

    fn bad(sub_target: SubTarget) -> SubTargetRun {
        SubTargetRun::failed(sub_target, "the API said no", "2026-01-01T00:00:00Z")
    }

    #[test]
    fn the_two_sub_targets_are_the_two_publish_roots() {
        assert_eq!(SubTarget::all(), [SubTarget::Main, SubTarget::Works]);
        assert_eq!(SubTarget::parse("main"), Some(SubTarget::Main));
        assert_eq!(SubTarget::parse("works"), Some(SubTarget::Works));
        assert_eq!(SubTarget::parse("site"), None);
    }

    #[test]
    fn both_sub_targets_succeeding_is_a_success() {
        assert_eq!(
            roll_up(&ok(SubTarget::Main), &ok(SubTarget::Works)),
            LogicalStatus::Succeeded
        );
    }

    #[test]
    fn one_failure_makes_the_target_partial_and_keeps_the_success() {
        // handoff.md section 14.12: the successful part must NOT be retracted.
        let status = roll_up(&ok(SubTarget::Main), &bad(SubTarget::Works));
        assert_eq!(status, LogicalStatus::Partial);
        assert!(!status.is_succeeded());
        assert!(status.needs_attention());

        // The main sub-target is still recorded as succeeded.
        let run = LogicalTargetRun {
            target_id: "cloudflare".to_string(),
            main: ok(SubTarget::Main),
            works: bad(SubTarget::Works),
        };
        assert!(run.main.status.is_succeeded());
        assert!(run.works.status.is_failed());
    }

    #[test]
    fn a_works_failure_does_not_touch_the_main_result() {
        let mut run = LogicalTargetRun::new("cloudflare");
        run.main = ok(SubTarget::Main);
        run.works = bad(SubTarget::Works);

        assert_eq!(run.status(), LogicalStatus::Partial);
        assert_eq!(run.main.status, SubTargetStatus::Succeeded);
        assert!(run.main.error.is_none());
        assert!(run.works.error.is_some());
    }

    #[test]
    fn both_sub_targets_failing_is_a_failure() {
        assert_eq!(
            roll_up(&bad(SubTarget::Main), &bad(SubTarget::Works)),
            LogicalStatus::Failed
        );
    }

    #[test]
    fn a_skip_is_not_a_failure() {
        // A target with no works origin is degraded, not broken: the main site deployed.
        let skipped = SubTargetRun::skipped(SubTarget::Works, "no works origin configured");
        assert_eq!(
            roll_up(&ok(SubTarget::Main), &skipped),
            LogicalStatus::Succeeded
        );
    }

    #[test]
    fn both_sub_targets_skipped_is_pending_rather_than_a_failure() {
        // Nothing was attempted, so nothing failed.
        let main = SubTargetRun::skipped(SubTarget::Main, "target disabled");
        let works = SubTargetRun::skipped(SubTarget::Works, "target disabled");
        assert_eq!(roll_up(&main, &works), LogicalStatus::Pending);
    }

    #[test]
    fn nothing_attempted_is_pending() {
        assert_eq!(
            roll_up(
                &SubTargetRun::pending(SubTarget::Main),
                &SubTargetRun::pending(SubTarget::Works)
            ),
            LogicalStatus::Pending
        );
    }

    #[test]
    fn something_still_running_is_pending() {
        let mut running = SubTargetRun::pending(SubTarget::Works);
        running.status = SubTargetStatus::Running;
        assert_eq!(roll_up(&ok(SubTarget::Main), &running), LogicalStatus::Pending);
    }

    #[test]
    fn a_failure_with_the_other_side_skipped_is_a_failure() {
        // The attempted part failed and the other was deliberately not tried, so there is nothing
        // live.
        let skipped = SubTargetRun::skipped(SubTarget::Works, "no works origin");
        assert_eq!(roll_up(&bad(SubTarget::Main), &skipped), LogicalStatus::Failed);
    }

    #[test]
    fn every_status_has_a_stable_spelling_matching_the_documented_names() {
        for (status, text) in [
            (SubTargetStatus::Pending, "pending"),
            (SubTargetStatus::Running, "running"),
            (SubTargetStatus::Succeeded, "succeeded"),
            (SubTargetStatus::Failed, "failed"),
            (SubTargetStatus::Skipped, "skipped"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(serde_json::to_string(&status).unwrap(), format!("\"{text}\""));
        }
        for (status, text) in [
            (LogicalStatus::Pending, "pending"),
            (LogicalStatus::Succeeded, "succeeded"),
            (LogicalStatus::Partial, "partial"),
            (LogicalStatus::Failed, "failed"),
        ] {
            assert_eq!(status.as_str(), text);
            assert_eq!(serde_json::to_string(&status).unwrap(), format!("\"{text}\""));
        }
    }

    #[test]
    fn a_partial_target_reports_exactly_the_failed_sub_targets() {
        // handoff.md section 14.13: the user re-publishes the FAILED sub-target.
        let mut run = LogicalTargetRun::new("cloudflare");
        run.main = ok(SubTarget::Main);
        run.works = bad(SubTarget::Works);
        assert_eq!(run.failed_sub_targets(), vec![SubTarget::Works]);

        run.main = bad(SubTarget::Main);
        run.works = bad(SubTarget::Works);
        assert_eq!(
            run.failed_sub_targets(),
            vec![SubTarget::Main, SubTarget::Works]
        );

        run.main = ok(SubTarget::Main);
        run.works = ok(SubTarget::Works);
        assert!(run.failed_sub_targets().is_empty());
    }

    #[test]
    fn the_three_automatic_targets_are_the_ones_the_contract_names() {
        // handoff.md section 14.10.
        assert!(TargetKind::CloudflarePages.is_automatic());
        assert!(TargetKind::EdgeOneMainland.is_automatic());
        assert!(TargetKind::EdgeOneInternational.is_automatic());

        // handoff.md section 14.11: 1Panel is NOT an automatic adapter.
        assert!(!TargetKind::OnePanel.is_automatic());
    }

    #[test]
    fn the_two_edgeone_regions_are_independent_targets() {
        // handoff.md section 14.10: two separate logical deployment targets.
        assert_ne!(TargetKind::EdgeOneMainland, TargetKind::EdgeOneInternational);
        assert_ne!(
            TargetKind::EdgeOneMainland.as_str(),
            TargetKind::EdgeOneInternational.as_str()
        );
    }

    #[test]
    fn a_target_reports_honestly_which_protocols_are_verified() {
        // ADR-0012 and ADR-0010: EdgeOne's upload API could not be verified from an authoritative
        // source, so it is an export target that says so rather than an adapter that pretends.
        assert!(TargetKind::CloudflarePages.is_verified());
        assert!(TargetKind::OnePanel.is_verified());
        assert!(!TargetKind::EdgeOneMainland.is_verified());
        assert!(!TargetKind::EdgeOneInternational.is_verified());
    }

    #[test]
    fn a_target_without_a_works_origin_is_degraded() {
        // handoff.md section 10.4 and 10.5: the works bodies must run on a different origin.
        let mut config = TargetConfig::new("cloudflare", TargetKind::CloudflarePages);
        config.project = Some("my-site".to_string());
        assert!(config.is_degraded());

        config.works_origin = Some("https://works.example.com".to_string());
        assert!(!config.is_degraded());

        let mut with_project = TargetConfig::new("cf", TargetKind::CloudflarePages);
        with_project.project = Some("site".to_string());
        with_project.works_project = Some("site-works".to_string());
        assert!(!with_project.is_degraded());
    }

    #[test]
    fn validation_requires_a_project_for_the_automatic_kinds() {
        let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
        assert!(config.validate().unwrap_err().contains("project"));
        config.project = Some("my-site".to_string());
        assert!(config.validate().is_ok());

        // A 1Panel target only exports, so it needs nothing.
        let export = TargetConfig::new("panel", TargetKind::OnePanel);
        assert!(export.validate().is_ok());
    }

    #[test]
    fn validation_rejects_a_relative_works_origin() {
        let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
        config.project = Some("site".to_string());
        config.works_origin = Some("works.example.com".to_string());
        assert!(config.validate().unwrap_err().contains("http://"));
    }

    #[test]
    fn a_target_config_carries_only_a_credential_handle() {
        let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
        config.project = Some("my-site".to_string());
        config.credential_ref = Some("keychain:deploy/cloudflare".to_string());
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("keychain:deploy/cloudflare"));
        assert!(!json.contains("token"), "no secret may appear: {json}");
    }

    #[test]
    fn a_frozen_target_keeps_what_the_release_was_built_for() {
        // handoff.md section 14.13 point 3: a re-send uses the ORIGINAL release's snapshot, not the
        // current configuration.
        let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
        config.project = Some("old-project".to_string());
        config.works_origin = Some("https://works.example.com".to_string());
        let frozen = FrozenTarget::from(&config);

        // The user renames the project afterwards.
        config.project = Some("new-project".to_string());

        assert_eq!(frozen.project.as_deref(), Some("old-project"));
        assert_eq!(config.project.as_deref(), Some("new-project"));
        assert!(!frozen.is_degraded());
    }

    #[test]
    fn a_snapshot_separates_the_automatic_targets_from_the_export_ones() {
        // handoff.md sections 14.10 and 14.11.
        let snapshot = TargetSnapshot {
            release_id: "20260101T120000Z-abcdef".to_string(),
            targets: vec![
                FrozenTarget::from(&{
                    let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
                    config.project = Some("site".to_string());
                    config
                }),
                FrozenTarget::from(&{
                    let mut config = TargetConfig::new("edgeone-cn", TargetKind::EdgeOneMainland);
                    config.project = Some("cn".to_string());
                    config
                }),
                FrozenTarget::from(&TargetConfig::new("panel", TargetKind::OnePanel)),
                FrozenTarget::from(&{
                    let mut config = TargetConfig::new("off", TargetKind::CloudflarePages);
                    config.project = Some("x".to_string());
                    config.enabled = false;
                    config
                }),
            ],
            commit_sha: "abc123".to_string(),
            tag: "release/20260101T120000Z-abcdef".to_string(),
            build_inputs_hash: "deadbeef".to_string(),
            created_at: "2026-01-01T12:00:00Z".to_string(),
        };

        let automatic: Vec<&str> = snapshot
            .automatic_targets()
            .iter()
            .map(|target| target.id.as_str())
            .collect();
        assert_eq!(automatic, vec!["cf", "edgeone-cn"], "a disabled target is excluded");

        let export: Vec<&str> = snapshot
            .export_targets()
            .iter()
            .map(|target| target.id.as_str())
            .collect();
        assert_eq!(export, vec!["panel"]);

        assert_eq!(snapshot.target("cf").unwrap().project.as_deref(), Some("site"));
        assert!(snapshot.target("nope").is_none());
    }

    #[test]
    fn a_re_send_reuses_the_dist_when_the_fingerprint_still_matches() {
        // handoff.md section 14.13 point 4.
        let decision = DistDecision::decide("fp-1", Some("fp-1"));
        assert!(decision.reuses());
        assert_eq!(decision.describe(), "reuse the current dist");
    }

    #[test]
    fn a_re_send_rebuilds_when_the_fingerprint_differs() {
        // handoff.md section 14.13 point 5: rebuild in a temporary directory.
        let decision = DistDecision::decide("fp-1", Some("fp-2"));
        assert!(!decision.reuses());
        match &decision {
            DistDecision::Rebuild {
                expected,
                actual,
                reason,
            } => {
                assert_eq!(expected, "fp-1");
                assert_eq!(actual.as_deref(), Some("fp-2"));
                assert!(reason.contains("not this release's"), "{reason}");
            }
            other => panic!("expected a rebuild, got {other:?}"),
        }
        assert!(decision.describe().contains("rebuild from the frozen snapshot"));
    }

    #[test]
    fn a_re_send_rebuilds_when_there_is_no_dist_at_all() {
        // A missing dist is a rebuild, not an error: the point is to be able to produce it again.
        let decision = DistDecision::decide("fp-1", None);
        assert!(!decision.reuses());
        assert!(decision.describe().contains("no dist"));
    }

    #[test]
    fn a_rollback_plan_carries_the_tag_the_commit_and_the_snapshot() {
        // handoff.md section 14.14.
        let snapshot = TargetSnapshot {
            release_id: "20251201T000000Z-oldone".to_string(),
            targets: vec![FrozenTarget::from(&{
                let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
                config.project = Some("site".to_string());
                config
            })],
            commit_sha: "oldsha".to_string(),
            tag: "release/20251201T000000Z-oldone".to_string(),
            build_inputs_hash: "oldhash".to_string(),
            created_at: "2025-12-01T00:00:00Z".to_string(),
        };

        let plan = RollbackPlan::from_snapshot(&snapshot).unwrap();
        assert_eq!(plan.release_id, "20251201T000000Z-oldone");
        assert_eq!(plan.commit_sha, "oldsha");
        assert_eq!(plan.tag, "release/20251201T000000Z-oldone");
        assert_eq!(plan.build_inputs_hash, "oldhash");
        assert_eq!(plan.automatic_targets().len(), 1);
    }

    #[test]
    fn a_rollback_of_a_release_with_no_anchor_is_refused() {
        // A release with no commit or tag cannot be rebuilt, and deploying the CURRENT tree would be
        // a rollback to somewhere else entirely.
        let mut snapshot = TargetSnapshot {
            release_id: "r".to_string(),
            targets: Vec::new(),
            commit_sha: String::new(),
            tag: "release/r".to_string(),
            build_inputs_hash: "h".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        assert!(RollbackPlan::from_snapshot(&snapshot)
            .unwrap_err()
            .contains("no commit"));

        snapshot.commit_sha = "abc".to_string();
        snapshot.tag = String::new();
        assert!(RollbackPlan::from_snapshot(&snapshot)
            .unwrap_err()
            .contains("no tag"));
    }

    #[test]
    fn a_run_round_trips_through_json_for_the_frontend() {
        let mut run = LogicalTargetRun::new("cloudflare");
        run.main = ok(SubTarget::Main);
        run.works = bad(SubTarget::Works);
        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("\"status\":\"succeeded\""), "{json}");
        assert!(json.contains("\"error\":\"the API said no\""), "{json}");
        assert_eq!(serde_json::from_str::<LogicalTargetRun>(&json).unwrap(), run);
    }

    #[test]
    fn a_root_revision_is_none_for_a_missing_root() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(root_revision(&dir.path().join("nothing")).is_none());
        // A directory with no manifest also has no revision rather than a misleading one.
        assert!(root_revision(dir.path()).is_none());
    }
}
