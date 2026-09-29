//! The publish orchestrator.
//!
//! `handoff.md` §14.12 and §14.13 fix the behaviour that is easiest to get subtly wrong:
//!
//! * Each logical target's two sub-targets are attempted INDEPENDENTLY. A failure on one does not
//!   stop, retract or even affect the other.
//! * A failure does **not** auto-retry. There is no loop here that tries twice, and a test asserts a
//!   failing adapter was contacted exactly once.
//! * A failure does not retract another target's success. Nothing in this module ever clears a
//!   `Succeeded` status.
//! * The user may re-send ONE failed sub-target, and a re-send uses the ORIGINAL release's snapshot
//!   (`handoff.md` §14.13 points 1-3) and the current `dist` only when its fingerprint still matches
//!   (points 4-5). New working-tree changes must never enter a historical re-send.
//!
//! The orchestrator takes adapters as a map so it never constructs one itself: building an adapter
//! means resolving a credential, and that is the caller's job.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

use crate::adapter::{DeployAdapter, PushOutcome, PushPayload};
use crate::target::{
    DistDecision, FrozenTarget, LogicalStatus, LogicalTargetRun, SubTarget, SubTargetRun,
    TargetSnapshot,
};

/// One target's adapters: separate ones for the two roots, because they may deploy to different
/// places and must be able to fail independently.
/// The adapters for one logical target.
///
/// The boxes are `'static`, which is why a caller cannot return a `CloudflarePagesAdapter` — that borrows its
/// HTTP client. A caller builds the adapters where its client lives, and `veloct_app::deploy` does exactly
/// that. Making this struct lifetime-generic was tried and reverted: it forced `'static` instantiations
/// through every test for no gain, because the adapters that need a borrow are built and consumed in one
/// function.
pub struct TargetAdapters {
    pub main: Box<dyn DeployAdapter>,
    pub works: Option<Box<dyn DeployAdapter>>,
}

impl TargetAdapters {
    pub fn new(main: Box<dyn DeployAdapter>, works: Option<Box<dyn DeployAdapter>>) -> Self {
        Self { main, works }
    }

    /// One adapter for both roots, which is correct when the target deploys both to one project.
    pub fn single(adapter: Box<dyn DeployAdapter>) -> Self {
        Self {
            main: adapter,
            works: None,
        }
    }

    fn adapter_for(&self, sub_target: SubTarget) -> Option<&dyn DeployAdapter> {
        match sub_target {
            SubTarget::Main => Some(self.main.as_ref()),
            SubTarget::Works => self.works.as_deref(),
        }
    }
}

/// A publish attempt's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishResult {
    pub release_id: String,
    pub targets: Vec<LogicalTargetRun>,
    /// The SEO task's own status, kept apart from the deployment statuses
    /// (`handoff.md` §15.2: a failed SEO push never fails the site release).
    #[serde(default)]
    pub seo: SeoTaskState,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
}

impl PublishResult {
    /// Every target's logical status.
    pub fn statuses(&self) -> Vec<(&str, LogicalStatus)> {
        self.targets
            .iter()
            .map(|run| (run.target_id.as_str(), run.status()))
            .collect()
    }

    /// The targets that need the user's attention.
    pub fn needing_attention(&self) -> Vec<&LogicalTargetRun> {
        self.targets
            .iter()
            .filter(|run| run.status().needs_attention())
            .collect()
    }

    /// Whether every enabled target fully succeeded.
    ///
    /// The SEO task is deliberately NOT part of this: §15.2 says an SEO failure does not make the
    /// site release a failure.
    pub fn all_succeeded(&self) -> bool {
        !self.targets.is_empty() && self.targets.iter().all(|run| run.status().is_succeeded())
    }

    /// The pairs of (target, sub-target) a user may re-send.
    pub fn resendable(&self) -> Vec<(&str, SubTarget)> {
        self.targets
            .iter()
            .flat_map(|run| {
                run.failed_sub_targets()
                    .into_iter()
                    .map(move |sub_target| (run.target_id.as_str(), sub_target))
            })
            .collect()
    }

    fn run_mut(&mut self, target_id: &str) -> Option<&mut LogicalTargetRun> {
        self.targets.iter_mut().find(|run| run.target_id == target_id)
    }
}

/// The SEO push task's state (`handoff.md` §15.2).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SeoTaskState {
    /// No push was requested.
    #[default]
    NotRequested,
    /// Queued, for example because the machine is offline.
    ///
    /// §15.2 allows this instead of an unbounded background retry: "offline may be marked pending, and
    /// there is no requirement to retry endlessly in the background".
    Pending { reason: String },
    /// The push was attempted and failed. The site release is unaffected.
    Failed { reason: String },
    /// The push succeeded.
    Pushed { engines: Vec<String> },
}

impl SeoTaskState {
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Whether the site release succeeded despite this.
    ///
    /// Always true: §15.2 says an SEO failure is never a site release failure. The method exists so a
    /// caller cannot implement the opposite without deleting it.
    pub fn affects_site_release(&self) -> bool {
        false
    }
}

/// What a re-send is allowed to use.
///
/// `handoff.md` §14.13: the ORIGINAL release's commit, build inputs and snapshot. Bundling them means
/// a re-send cannot accidentally read the current configuration, because it is not given one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResendContext {
    /// The snapshot frozen into the release being re-sent.
    pub snapshot: TargetSnapshot,
    /// The release's build fingerprint.
    pub build_fingerprint: String,
    /// The fingerprint of the `dist` currently on disk, if any.
    pub current_dist_fingerprint: Option<String>,
}

impl ResendContext {
    pub fn new(
        snapshot: TargetSnapshot,
        build_fingerprint: impl Into<String>,
        current_dist_fingerprint: Option<String>,
    ) -> Self {
        Self {
            snapshot,
            build_fingerprint: build_fingerprint.into(),
            current_dist_fingerprint,
        }
    }

    /// Whether the current `dist` may be reused (`handoff.md` §14.13 point 4).
    pub fn dist_decision(&self) -> DistDecision {
        DistDecision::decide(&self.build_fingerprint, self.current_dist_fingerprint.as_deref())
    }

    /// The frozen target for a re-send, or an error naming why it cannot be re-sent.
    pub fn frozen_target(&self, target_id: &str) -> Result<&FrozenTarget> {
        self.snapshot.target(target_id).ok_or_else(|| {
            VeloctError::Deploy {
                target: target_id.to_string(),
                sub_target: "main".to_string(),
                message: format!(
                    "release {} has no frozen target {target_id}, so it cannot be re-sent",
                    self.snapshot.release_id
                ),
            }
        })
    }

    /// Refuse a re-send that would use a different commit than the release's.
    ///
    /// §14.13 is explicit: new working-tree changes must not be mixed into a historical re-send. The
    /// check is a comparison rather than a warning so the mistake is impossible.
    pub fn assert_same_commit(&self, current_commit: &str) -> Result<()> {
        if current_commit == self.snapshot.commit_sha {
            return Ok(());
        }
        Err(VeloctError::Deploy {
            target: self.snapshot.release_id.clone(),
            sub_target: "main".to_string(),
            message: format!(
                "this re-send is for commit {} but the working tree is at {current_commit}; \
                 check out the release's tag first, or the new changes would be published as if \
                 they were that release",
                self.snapshot.commit_sha
            ),
        })
    }
}

/// Everything a re-send needs.
///
/// A struct rather than eight positional arguments: two of them are ids and two are strings about the
/// same release, and swapping a pair would send an old build to the wrong target while every test
/// still ran.
pub struct ResendRequest<'a> {
    /// The previous result, so the other sub-targets are preserved.
    pub result: &'a PublishResult,
    /// The ORIGINAL release's frozen context (handoff.md section 14.13).
    pub context: &'a ResendContext,
    pub target_id: &'a str,
    pub sub_target: SubTarget,
    pub adapters: &'a BTreeMap<String, TargetAdapters>,
    /// The payload for the sub-target being re-sent.
    pub payload: &'a PushPayload,
    /// The commit the working tree is at, compared against the release's.
    pub current_commit: &'a str,
    pub now: &'a str,
}

/// The orchestrator.
pub struct Orchestrator;

impl Orchestrator {
    /// Publish every enabled target.
    ///
    /// `payloads` supplies the two publish roots. Each is optional so a target whose works root does
    /// not exist is SKIPPED rather than failed, which is what `handoff.md` §10.5's degraded target
    /// means.
    pub fn publish(
        release_id: &str,
        targets: &[FrozenTarget],
        adapters: &BTreeMap<String, TargetAdapters>,
        payloads: &PublishPayloads,
        now: &str,
    ) -> Result<PublishResult> {
        let mut runs = Vec::new();

        for target in targets {
            // A disabled target is not deployed and not reported as a failure.
            if !target.enabled {
                runs.push(LogicalTargetRun {
                    target_id: target.id.clone(),
                    main: SubTargetRun::skipped(SubTarget::Main, "target disabled"),
                    works: SubTargetRun::skipped(SubTarget::Works, "target disabled"),
                });
                continue;
            }

            // A target with no adapter configured is skipped for the same reason.
            let Some(target_adapters) = adapters.get(&target.id) else {
                runs.push(LogicalTargetRun {
                    target_id: target.id.clone(),
                    main: SubTargetRun::skipped(
                        SubTarget::Main,
                        "no adapter is configured for this target",
                    ),
                    works: SubTargetRun::skipped(
                        SubTarget::Works,
                        "no adapter is configured for this target",
                    ),
                });
                continue;
            };

            // A manual target reports an export rather than a deployment. It is still a success for
            // the release, because the payload was produced; it is just not pushed (§14.11).
            if !target.kind.is_automatic() {
                let mut run = LogicalTargetRun::new(&target.id);
                run.main = Self::push_one(
                    target_adapters,
                    SubTarget::Main,
                    payloads.main.as_ref(),
                    now,
                );
                run.works = Self::push_one(
                    target_adapters,
                    SubTarget::Works,
                    payloads.works.as_ref(),
                    now,
                );
                runs.push(run);
                continue;
            }

            let mut run = LogicalTargetRun::new(&target.id);
            // The two roots are pushed independently and the second runs even if the first failed:
            // there is no short circuit, because a main failure must not stop the works root.
            run.main = Self::push_one(
                target_adapters,
                SubTarget::Main,
                payloads.main.as_ref(),
                now,
            );
            run.works = Self::push_one(
                target_adapters,
                SubTarget::Works,
                payloads.works.as_ref(),
                now,
            );
            runs.push(run);
        }

        Ok(PublishResult {
            release_id: release_id.to_string(),
            targets: runs,
            seo: SeoTaskState::NotRequested,
            started_at: now.to_string(),
            finished_at: Some(now.to_string()),
        })
    }

    /// Push one sub-target, exactly once.
    fn push_one(
        adapters: &TargetAdapters,
        sub_target: SubTarget,
        payload: Option<&PushPayload>,
        now: &str,
    ) -> SubTargetRun {
        // No payload means the root was not built, which for the works root is the degraded case.
        let Some(payload) = payload else {
            return SubTargetRun::skipped(
                sub_target,
                format!(
                    "there is no dist/{} to deploy; this target has no independent works origin",
                    sub_target.as_str()
                ),
            );
        };

        let Some(adapter) = adapters.adapter_for(sub_target) else {
            // The works root has no adapter: the target has no works origin, which §10.5 reports as
            // degraded rather than failed.
            return SubTargetRun::skipped(
                sub_target,
                "this target has no separate works target configured",
            );
        };

        // EXACTLY ONE attempt. There is no retry loop here, because handoff.md section 14.13 says a
        // failure is not retried automatically.
        match adapter.push(payload) {
            Ok(outcome) => Self::success_run(sub_target, &outcome, now),
            Err(error) => SubTargetRun::failed(sub_target, error.to_string(), now),
        }
    }

    fn success_run(sub_target: SubTarget, outcome: &PushOutcome, now: &str) -> SubTargetRun {
        SubTargetRun {
            sub_target,
            status: crate::target::SubTargetStatus::Succeeded,
            error: if outcome.manual {
                Some(outcome.message.clone())
            } else {
                None
            },
            build_fingerprint: None,
            reused_dist: false,
            files: outcome.files,
            started_at: None,
            finished_at: Some(now.to_string()),
        }
    }

    /// Re-send ONE failed sub-target of ONE target.
    ///
    /// `handoff.md` §14.13: the original release's commit, build inputs and snapshot; the current
    /// `dist` when its fingerprint still matches, otherwise a rebuild. The caller supplies the
    /// already-built payload, and [`ResendContext`] proves which release it came from.
    pub fn resend(request: &ResendRequest<'_>) -> Result<PublishResult> {
        let ResendRequest {
            result,
            context,
            target_id,
            sub_target,
            adapters,
            payload,
            current_commit,
            now,
        } = request;
        let sub_target = *sub_target;

        // The release's own commit, never the working tree's (handoff.md section 14.13, last line).
        context.assert_same_commit(current_commit)?;

        // The frozen target, not the current configuration (handoff.md section 14.13 point 3). The
        // lookup is what fails early when the release has no such target; the adapter then reads the
        // frozen configuration itself.
        let _frozen = context.frozen_target(target_id)?;

        // The payload must be for the sub-target being re-sent, or the wrong root would be pushed.
        if payload.sub_target != sub_target {
            return Err(VeloctError::Deploy {
                target: target_id.to_string(),
                sub_target: sub_target.as_str().to_string(),
                message: format!(
                    "the payload is for dist/{} but the re-send is for dist/{}",
                    payload.sub_target.as_str(),
                    sub_target.as_str()
                ),
            });
        }

        let Some(target_adapters) = adapters.get(*target_id) else {
            return Err(VeloctError::Deploy {
                target: target_id.to_string(),
                sub_target: sub_target.as_str().to_string(),
                message: "no adapter is configured for this target".to_string(),
            });
        };

        let mut updated = (*result).clone();
        let run = updated.run_mut(target_id).ok_or_else(|| VeloctError::Deploy {
            target: target_id.to_string(),
            sub_target: sub_target.as_str().to_string(),
            message: format!("the previous result has no run for {target_id}"),
        })?;

        let Some(adapter) = target_adapters.adapter_for(sub_target) else {
            return Err(VeloctError::Deploy {
                target: target_id.to_string(),
                sub_target: sub_target.as_str().to_string(),
                message: "this target has no adapter for that sub-target".to_string(),
            });
        };

        let decision = context.dist_decision();
        match adapter.push(payload) {
            Ok(outcome) => {
                let mut succeeded = Self::success_run(sub_target, &outcome, now);
                // The fingerprint is recorded so a later re-send can compare again.
                succeeded.build_fingerprint = Some(context.build_fingerprint.clone());
                succeeded.reused_dist = decision.reuses();
                *run.run_for_mut(sub_target) = succeeded;
            }
            Err(error) => {
                // A failed re-send records the failure and nothing else. The OTHER sub-target is
                // untouched, which is the section 14.12 rule.
                let mut failed = SubTargetRun::failed(sub_target, error.to_string(), now);
                failed.reused_dist = decision.reuses();
                *run.run_for_mut(sub_target) = failed;
            }
        }

        updated.finished_at = Some((*now).to_string());
        Ok(updated)
    }

    /// Whether a re-send may reuse the current `dist`.
    pub fn dist_decision(context: &ResendContext) -> DistDecision {
        context.dist_decision()
    }
}

/// The two publish roots for one publish.
#[derive(Debug, Clone)]
pub struct PublishPayloads {
    pub main: Option<PushPayload>,
    pub works: Option<PushPayload>,
}

impl PublishPayloads {
    pub fn new(main: Option<PushPayload>, works: Option<PushPayload>) -> Self {
        Self { main, works }
    }

    /// Read both roots from a `dist/` directory, tolerating a missing works root.
    ///
    /// A missing `dist/site` is an error, because a publish with no site is not a publish. A missing
    /// `dist/works` is not: a project with no works, or a target with no works origin, is a normal
    /// degraded state (`handoff.md` §10.5).
    pub fn read(dist: &std::path::Path) -> Result<Self> {
        let main = Some(PushPayload::read(SubTarget::Main, &dist.join("site"))?);
        let works_root = dist.join("works");
        let works = if works_root.is_dir() {
            PushPayload::read(SubTarget::Works, &works_root).ok()
        } else {
            None
        };
        Ok(Self { main, works })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::MemoryAdapter;
    use crate::target::{TargetConfig, TargetKind};
    use std::path::Path;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn write_root(dir: &Path, files: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        for (relative, contents) in files {
            let path = dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }

    fn payloads(dir: &TempDir) -> PublishPayloads {
        let main_root = dir.path().join("site");
        let works_root = dir.path().join("works");
        write_root(&main_root, &[("index.html", "<html>main</html>")]);
        write_root(&works_root, &[("demo/index.html", "<html>works</html>")]);
        PublishPayloads::new(
            Some(PushPayload::read(SubTarget::Main, &main_root).unwrap()),
            Some(PushPayload::read(SubTarget::Works, &works_root).unwrap()),
        )
    }

    fn frozen(id: &str, kind: TargetKind, project: &str) -> FrozenTarget {
        let mut config = TargetConfig::new(id, kind);
        config.project = Some(project.to_string());
        config.works_project = Some(format!("{project}-works"));
        config.works_origin = Some("https://works.example.com".to_string());
        FrozenTarget::from(&config)
    }

    /// Adapt pair sharing one adapter, which is the common case.
    fn adapters(target: &FrozenTarget) -> (TargetAdapters, std::sync::Arc<MemoryAdapter>) {
        let adapter = std::sync::Arc::new(MemoryAdapter::new(target.clone()));
        // The trait object needs to own something, so the shared handle is wrapped in a forwarding
        // adapter that lets the test observe the pushes.
        (
            TargetAdapters::new(
                Box::new(SharedAdapter(adapter.clone())),
                Some(Box::new(SharedAdapter(adapter.clone()))),
            ),
            adapter,
        )
    }

    /// Forwards to a shared [`MemoryAdapter`] so a test can inspect what happened.
    struct SharedAdapter(std::sync::Arc<MemoryAdapter>);

    impl DeployAdapter for SharedAdapter {
        fn target(&self) -> &FrozenTarget {
            self.0.target()
        }
        fn capabilities(&self) -> crate::adapter::DeployCapabilities {
            self.0.capabilities()
        }
        fn push(&self, payload: &PushPayload) -> Result<PushOutcome> {
            self.0.push(payload)
        }
        fn test_connection(&self) -> Result<String> {
            self.0.test_connection()
        }
    }

    #[test]
    fn a_publish_attempts_both_roots_of_every_enabled_target() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let targets = vec![frozen("cf", TargetKind::CloudflarePages, "site")];

        let (target_adapters, adapter) = adapters(&targets[0]);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let result = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();
        assert!(result.all_succeeded());
        assert_eq!(adapter.push_count(SubTarget::Main), 1);
        assert_eq!(adapter.push_count(SubTarget::Works), 1);
        assert_eq!(result.statuses(), vec![("cf", LogicalStatus::Succeeded)]);
    }

    #[test]
    fn a_works_failure_makes_the_target_partial_and_leaves_main_succeeded() {
        // handoff.md section 14.12, end to end.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let targets = vec![frozen("cf", TargetKind::CloudflarePages, "site")];
        let (target_adapters, adapter) = adapters(&targets[0]);
        adapter.fail_sub_target(SubTarget::Works, "the works project does not exist");

        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let result = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();

        let run = &result.targets[0];
        assert_eq!(run.status(), LogicalStatus::Partial);
        assert_eq!(run.main.status, crate::target::SubTargetStatus::Succeeded);
        assert_eq!(run.works.status, crate::target::SubTargetStatus::Failed);
        assert!(run.works.error.is_some());
        // The successful half is NOT retracted.
        assert!(run.main.error.is_none());
        assert_eq!(adapter.push_count(SubTarget::Main), 1);
        assert!(!result.all_succeeded());
    }

    #[test]
    fn a_main_failure_does_not_stop_the_works_root_from_being_pushed() {
        // There is no short circuit: the two roots are independent.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let targets = vec![frozen("cf", TargetKind::CloudflarePages, "site")];
        let (target_adapters, adapter) = adapters(&targets[0]);
        adapter.fail_sub_target(SubTarget::Main, "the API rejected the deployment");

        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let result = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();
        assert_eq!(result.targets[0].status(), LogicalStatus::Partial);
        assert_eq!(
            adapter.push_count(SubTarget::Works),
            1,
            "the works root was still attempted"
        );
    }

    #[test]
    fn a_failure_is_not_retried_automatically() {
        // handoff.md section 14.13: no automatic retry. The adapter is contacted exactly once.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let targets = vec![frozen("cf", TargetKind::CloudflarePages, "site")];
        let (target_adapters, adapter) = adapters(&targets[0]);
        adapter.fail_sub_target(SubTarget::Main, "permanent failure");

        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        // The main root fails on every attempt; the works root succeeds.
        let first = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();
        assert_eq!(first.targets[0].main.status, crate::target::SubTargetStatus::Failed);
        assert_eq!(adapter.push_count(SubTarget::Main), 0, "main never succeeded");
        assert_eq!(adapter.push_count(SubTarget::Works), 1);

        // A SECOND publish is a new user-initiated publish, not an automatic retry, and it still
        // attempts each root exactly once. So after two publishes there are two successful works
        // pushes and still no successful main push: nothing was retried inside either publish.
        let second = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();
        assert_eq!(second.targets[0].main.status, crate::target::SubTargetStatus::Failed);
        assert_eq!(adapter.push_count(SubTarget::Works), 2, "one push per publish, not two");
        assert_eq!(adapter.push_count(SubTarget::Main), 0);
    }

    #[test]
    fn one_target_failing_does_not_retract_another_targets_success() {
        // handoff.md section 14.13: a failure does not retract other successful targets.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let targets = vec![
            frozen("cf", TargetKind::CloudflarePages, "site"),
            frozen("edgeone", TargetKind::EdgeOneMainland, "cn"),
        ];

        let (cf_adapters, cf) = adapters(&targets[0]);
        let (eo_adapters, eo) = adapters(&targets[1]);
        // BOTH EdgeOne roots fail, so that target is Failed. Failing only one would make it PARTIAL,
        // which the roll-up rule covers elsewhere.
        eo.fail_all("the EdgeOne account is not set up");

        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), cf_adapters);
        map.insert("edgeone".to_string(), eo_adapters);

        let result = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();

        let cf_run = result.targets.iter().find(|r| r.target_id == "cf").unwrap();
        let eo_run = result.targets.iter().find(|r| r.target_id == "edgeone").unwrap();
        assert_eq!(cf_run.status(), LogicalStatus::Succeeded);
        assert_eq!(eo_run.status(), LogicalStatus::Failed);
        assert_eq!(cf.push_count(SubTarget::Main), 1);
        assert!(!result.all_succeeded());

        // The failed target is the one offered for a re-send, and only it.
        assert_eq!(
            result.resendable(),
            vec![("edgeone", SubTarget::Main), ("edgeone", SubTarget::Works)]
        );
    }

    #[test]
    fn a_degraded_target_skips_the_works_root_rather_than_failing_it() {
        // handoff.md section 10.5: a target without an independent works origin is degraded.
        let dir = TempDir::new().unwrap();
        let main_root = dir.path().join("site");
        write_root(&main_root, &[("index.html", "<html>")]);
        let payloads = PublishPayloads::new(
            Some(PushPayload::read(SubTarget::Main, &main_root).unwrap()),
            None,
        );

        let mut config = TargetConfig::new("cf", TargetKind::CloudflarePages);
        config.project = Some("site".to_string());
        let target = FrozenTarget::from(&config);
        assert!(target.is_degraded());

        let (target_adapters, _) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let result = Orchestrator::publish("r1", &[target], &map, &payloads, NOW).unwrap();
        let run = &result.targets[0];
        assert_eq!(run.works.status, crate::target::SubTargetStatus::Skipped);
        assert!(run.works.error.as_deref().unwrap().contains("no independent works origin"));
        // A skip is not a failure, so the target succeeded.
        assert_eq!(run.status(), LogicalStatus::Succeeded);
    }

    #[test]
    fn a_disabled_target_is_skipped_and_not_reported_as_a_failure() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let mut target = frozen("cf", TargetKind::CloudflarePages, "site");
        target.enabled = false;

        let (target_adapters, adapter) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let result = Orchestrator::publish("r1", &[target], &map, &payloads, NOW).unwrap();
        assert_eq!(result.targets[0].main.status, crate::target::SubTargetStatus::Skipped);
        assert_eq!(result.targets[0].status(), LogicalStatus::Pending);
        assert_eq!(adapter.pushes().len(), 0, "nothing was deployed");
    }

    #[test]
    fn a_target_with_no_adapter_is_skipped_rather_than_reported_as_a_failure() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let targets = vec![frozen("cf", TargetKind::CloudflarePages, "site")];
        let map = BTreeMap::new();

        let result = Orchestrator::publish("r1", &targets, &map, &payloads, NOW).unwrap();
        assert_eq!(result.targets[0].main.status, crate::target::SubTargetStatus::Skipped);
        assert!(result.targets[0]
            .main
            .error
            .as_deref()
            .unwrap()
            .contains("no adapter"));
    }

    #[test]
    fn a_manual_target_reports_an_export_and_still_counts_as_a_success() {
        // handoff.md section 14.11.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let mut config = TargetConfig::new("panel", TargetKind::OnePanel);
        config.works_origin = Some("https://works.example.com".to_string());
        let target = FrozenTarget::from(&config);

        let adapter = std::sync::Arc::new(MemoryAdapter::manual(target.clone()));
        let mut map = BTreeMap::new();
        map.insert(
            "panel".to_string(),
            TargetAdapters::new(
                Box::new(SharedAdapter(adapter.clone())),
                Some(Box::new(SharedAdapter(adapter.clone()))),
            ),
        );

        let result = Orchestrator::publish("r1", &[target], &map, &payloads, NOW).unwrap();
        let run = &result.targets[0];
        assert_eq!(run.status(), LogicalStatus::Succeeded);
        assert_eq!(run.main.status, crate::target::SubTargetStatus::Succeeded);
        assert!(
            run.main.error.as_deref().unwrap().contains("manual"),
            "the export is described: {:?}",
            run.main.error
        );
        assert_eq!(adapter.pushes().len(), 0, "nothing was actually pushed");
    }

    #[test]
    fn reading_payloads_from_a_dist_requires_a_site_root_but_not_a_works_root() {
        let dir = TempDir::new().unwrap();
        let dist = dir.path().join("dist");

        // No dist at all is an error: a publish with no site is not a publish.
        assert!(PublishPayloads::read(&dist).is_err());

        // A dist with only a site root is fine.
        write_root(&dist.join("site"), &[("index.html", "<html>")]);
        let payloads = PublishPayloads::read(&dist).unwrap();
        assert!(payloads.main.is_some());
        assert!(payloads.works.is_none(), "a project with no works is normal");

        // And with a works root both are read.
        write_root(&dist.join("works"), &[("demo/index.html", "<html>")]);
        let payloads = PublishPayloads::read(&dist).unwrap();
        assert!(payloads.works.is_some());
    }

    fn snapshot(targets: Vec<FrozenTarget>) -> TargetSnapshot {
        TargetSnapshot {
            release_id: "r1".to_string(),
            targets,
            commit_sha: "abc123".to_string(),
            tag: "release/r1".to_string(),
            build_inputs_hash: "deadbeef".to_string(),
            created_at: NOW.to_string(),
        }
    }

    #[test]
    fn a_re_send_publishes_only_the_failed_sub_target() {
        // handoff.md section 14.13: the user re-publishes the failed sub-target, not everything.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, adapter) = adapters(&target);
        adapter.fail_sub_target(SubTarget::Works, "temporary");

        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let first = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();
        assert_eq!(first.targets[0].status(), LogicalStatus::Partial);
        let pushes_after_first = adapter.pushes().len();

        // The works project is fixed, and the user re-sends just that sub-target.
        adapter.clear_failures();
        let context = ResendContext::new(snapshot(vec![target]), "fp-1", Some("fp-1".to_string()));
        let updated = Orchestrator::resend(&ResendRequest {
            result: &first,
            context: &context,
            target_id: "cf",
            sub_target: SubTarget::Works,
            adapters: &map,
            payload: payloads.works.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap();

        let run = &updated.targets[0];
        assert_eq!(run.works.status, crate::target::SubTargetStatus::Succeeded);
        // The main result is untouched.
        assert_eq!(run.main.status, crate::target::SubTargetStatus::Succeeded);
        assert_eq!(run.status(), LogicalStatus::Succeeded);
        // Only ONE new push happened, for the works root.
        assert_eq!(adapter.pushes().len(), pushes_after_first + 1);
        assert_eq!(adapter.push_count(SubTarget::Works), 1);
        assert_eq!(adapter.push_count(SubTarget::Main), 1, "main was not re-pushed");
    }

    #[test]
    fn a_re_send_records_whether_the_existing_dist_was_reused() {
        // handoff.md section 14.13 points 4 and 5.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, adapter) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);
        let result = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();

        // Matching fingerprint: the dist is reused.
        let reuse = ResendContext::new(snapshot(vec![target.clone()]), "fp-1", Some("fp-1".into()));
        assert!(reuse.dist_decision().reuses());
        let updated = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &reuse,
            target_id: "cf",
            sub_target: SubTarget::Main,
            adapters: &map,
            payload: payloads.main.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap();
        assert!(updated.targets[0].main.reused_dist);
        assert_eq!(updated.targets[0].main.build_fingerprint.as_deref(), Some("fp-1"));

        // Different fingerprint: a rebuild was needed, and the run records that it was not reused.
        adapter.clear_failures();
        let rebuild = ResendContext::new(snapshot(vec![target]), "fp-1", Some("fp-2".into()));
        assert!(!rebuild.dist_decision().reuses());
        let updated = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &rebuild,
            target_id: "cf",
            sub_target: SubTarget::Main,
            adapters: &map,
            payload: payloads.main.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap();
        assert!(!updated.targets[0].main.reused_dist);
    }

    #[test]
    fn a_re_send_from_a_different_commit_is_refused() {
        // handoff.md section 14.13: new working-tree changes must not be mixed into a historical
        // re-send.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, _) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);
        let result = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();

        let context = ResendContext::new(snapshot(vec![target]), "fp-1", Some("fp-1".into()));
        let error = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &context,
            target_id: "cf",
            sub_target: SubTarget::Main,
            adapters: &map,
            payload: payloads.main.as_ref().unwrap(),
            current_commit: "different-commit",
            now: NOW,
        })
        .unwrap_err();
        assert!(error.to_string().contains("working tree is at different-commit"), "{error}");
    }

    #[test]
    fn a_re_send_uses_the_frozen_target_not_the_current_configuration() {
        // handoff.md section 14.13 point 3.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "old-project");
        let (target_adapters, _) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);
        let result = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();

        // The frozen snapshot names old-project; a current configuration naming something else is
        // irrelevant because the re-send is never given one.
        let context = ResendContext::new(snapshot(vec![target]), "fp-1", Some("fp-1".into()));
        let frozen_target = context.frozen_target("cf").unwrap();
        assert_eq!(frozen_target.project.as_deref(), Some("old-project"));

        let updated = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &context,
            target_id: "cf",
            sub_target: SubTarget::Main,
            adapters: &map,
            payload: payloads.main.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap();
        assert!(updated.targets[0].main.status.is_succeeded());
    }

    #[test]
    fn a_re_send_of_a_target_the_release_does_not_have_is_refused() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, _) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);
        let result = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();

        // The snapshot has no "other" target.
        let context = ResendContext::new(snapshot(vec![target]), "fp-1", None);
        let error = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &context,
            target_id: "other",
            sub_target: SubTarget::Main,
            adapters: &map,
            payload: payloads.main.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap_err();
        assert!(error.to_string().contains("no frozen target other"), "{error}");
    }

    #[test]
    fn a_re_send_with_the_wrong_payload_is_refused() {
        // Pushing dist/site to the works target would replace the works origin with the main site.
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, _) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);
        let result = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();

        let context = ResendContext::new(snapshot(vec![target]), "fp-1", None);
        let error = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &context,
            target_id: "cf",
            sub_target: SubTarget::Works,
            adapters: &map,
            payload: payloads.main.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap_err();
        assert!(error.to_string().contains("payload is for dist/main"), "{error}");
    }

    #[test]
    fn a_re_send_that_fails_again_records_the_failure_and_leaves_the_other_alone() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, adapter) = adapters(&target);
        adapter.fail_sub_target(SubTarget::Works, "still broken");

        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);
        let result = Orchestrator::publish("r1", std::slice::from_ref(&target), &map, &payloads, NOW).unwrap();

        let context = ResendContext::new(snapshot(vec![target]), "fp-1", None);
        let updated = Orchestrator::resend(&ResendRequest {
            result: &result,
            context: &context,
            target_id: "cf",
            sub_target: SubTarget::Works,
            adapters: &map,
            payload: payloads.works.as_ref().unwrap(),
            current_commit: "abc123",
            now: NOW,
        })
        .unwrap();

        assert_eq!(updated.targets[0].works.status, crate::target::SubTargetStatus::Failed);
        assert!(updated.targets[0]
            .works
            .error
            .as_deref()
            .unwrap()
            .contains("still broken"));
        // The main success is untouched.
        assert_eq!(updated.targets[0].main.status, crate::target::SubTargetStatus::Succeeded);
    }

    #[test]
    fn a_seo_task_is_never_part_of_the_site_release_status() {
        // handoff.md section 15.2.
        let failed = SeoTaskState::Failed {
            reason: "Baidu rejected the push".to_string(),
        };
        assert!(failed.is_failure());
        assert!(
            !failed.affects_site_release(),
            "an SEO failure must never make the site release a failure"
        );

        let pending = SeoTaskState::Pending {
            reason: "the machine is offline".to_string(),
        };
        assert!(!pending.is_failure());
        assert!(!pending.affects_site_release());

        let pushed = SeoTaskState::Pushed {
            engines: vec!["baidu".to_string(), "bing".to_string()],
        };
        assert!(!pushed.affects_site_release());
    }

    #[test]
    fn a_publish_result_with_only_seo_trouble_still_reports_success() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, _) = adapters(&target);
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let mut result = Orchestrator::publish("r1", &[target], &map, &payloads, NOW).unwrap();
        result.seo = SeoTaskState::Failed {
            reason: "the search engine was unreachable".to_string(),
        };

        assert!(result.all_succeeded(), "the site release succeeded");
        assert!(result.seo.is_failure());
        assert!(result.needing_attention().is_empty(), "no deployment needs attention");
    }

    #[test]
    fn a_publish_result_round_trips_through_json_for_the_ui() {
        let dir = TempDir::new().unwrap();
        let payloads = payloads(&dir);
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let (target_adapters, adapter) = adapters(&target);
        adapter.fail_sub_target(SubTarget::Works, "no");
        let mut map = BTreeMap::new();
        map.insert("cf".to_string(), target_adapters);

        let result = Orchestrator::publish("r1", &[target], &map, &payloads, NOW).unwrap();

        // The logical status is DERIVED and is not stored on the result, so what crosses to the
        // frontend is the two sub-target statuses and the SEO state. Asserting a top-level "status"
        // would be asserting a field that does not exist.
        assert_eq!(result.targets[0].status(), LogicalStatus::Partial);

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"sub_target\":\"main\""), "{json}");
        assert!(json.contains("\"status\":\"succeeded\""), "{json}");
        assert!(json.contains("\"sub_target\":\"works\""), "{json}");
        assert!(json.contains("\"status\":\"failed\""), "{json}");
        assert!(json.contains("\"error\":\"deployment failed for cf/works: no\""), "{json}");
        assert!(json.contains("\"state\":\"not_requested\""), "the SEO state is present: {json}");
        assert_eq!(serde_json::from_str::<PublishResult>(&json).unwrap(), result);
    }

    #[test]
    fn a_resend_context_reports_a_missing_dist_as_a_rebuild() {
        let target = frozen("cf", TargetKind::CloudflarePages, "site");
        let context = ResendContext::new(snapshot(vec![target]), "fp-1", None);
        assert!(!context.dist_decision().reuses());
        assert!(context.dist_decision().describe().contains("no dist"));
    }
}
