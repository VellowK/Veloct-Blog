//! Deployment: the logical targets and their main/works sub-targets.
//!
//! `crates/veloct-deploy` has the whole model — the logical targets, the two sub-targets, the Cloudflare
//! adapter, the manual-export adapters, the roll-up that distinguishes "partial" from "failed", the re-send
//! path — and until this module existed **the application never called any of it**. §28 names the three
//! logical targets and their sub-targets as a completion requirement, so a model with no entry point is not
//! that.
//!
//! Four decisions shape this file:
//!
//! * **The credentials come from the OS store by HANDLE.** §12 keeps credentials out of SQLite, so a target
//!   row holds `credential_ref` and this reads the value through the same `credential` callback the AI service
//!   uses. A target whose credential is absent is reported as needing configuration rather than attempted
//!   with an empty token, because an empty token produces an authentication error that says nothing useful.
//! * **EdgeOne and 1Panel are MANUAL exports, not adapters.** §14.11 says 1Panel is not an automatic adapter
//!   and ADR-0012 records that EdgeOne's upload API was never verified. `ManualExportAdapter` reports what to
//!   upload rather than pretending, which is the honest behaviour and the one the crate already models.
//! * **A target with no works project SKIPS the works sub-target** rather than failing it: §10.5 calls that a
//!   degraded target, and reporting a skip as a failure would make the whole target look broken.
//! * **`dist/` is read once and shared by every target.** §14.1's "one build, many targets" is the reason, and
//!   re-reading the tree per target would also let two targets see different bytes.
//!
//! Deployment is a SEPARATE step from publishing, which is why nothing here builds anything: §14.13's re-send
//! re-uses an existing `dist/` when the fingerprint matches, and a deploy that rebuilt would move the bytes
//! underneath the fingerprint that authorised it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::http::HttpClient;
use veloct_core::{Clock, Result, VeloctError};
use veloct_deploy::{
    CloudflarePagesAdapter, DeployAdapter, DistDecision, FrozenTarget, LogicalStatus, ManualExportAdapter,
    Orchestrator, PublishPayloads, PublishResult, SubTarget, TargetAdapters, TargetConfig, TargetKind,
};

/// Where the target configuration lives inside a project.
pub const TARGETS_FILE: &str = ".veloct/deploy-targets.json";

/// A saved target configuration.
///
/// The same shape as `TargetConfig` so the crate's own `From<&TargetConfig> for FrozenTarget` does the
/// conversion, and there is one definition of what a target is.
pub type StoredTarget = TargetConfig;

/// The credentials a target needs, resolved by the caller.
///
/// A borrowed callback rather than a concrete store, exactly as the AI service takes one: the credential
/// store is the shell's business and the service's tests must not touch a real keychain.
pub type CredentialLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The targets a project deploys to.
///
/// `Default` is implemented by hand rather than derived, because the derive would give `enabled: false` while
/// the serde default is `true` — so an empty JSON object and a `TargetList::default()` would disagree about
/// whether deployment happens at all. A test caught exactly that.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetList {
    pub targets: Vec<StoredTarget>,
    /// Whether deployment is enabled at all. Defaults to true so an existing project keeps deploying.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for TargetList {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            enabled: default_enabled(),
        }
    }
}

impl TargetList {
    /// Read the target list, or an empty one when the file does not exist.
    ///
    /// An unreadable file is an ERROR rather than an empty list: silently deploying to nothing would look
    /// exactly like a successful deployment of nothing.
    pub fn read(project_root: &Path) -> Result<Self> {
        let path = project_root.join(TARGETS_FILE);
        let Some(text) = veloct_core::workdir::read_text_optional(&path)? else {
            return Ok(Self {
                targets: Vec::new(),
                enabled: true,
            });
        };
        serde_json::from_str(&text).map_err(|error| {
            VeloctError::Invalid(format!("{} is not a target list: {error}", path.display()))
        })
    }

    /// Write the target list.
    pub fn write(&self, project_root: &Path) -> Result<()> {
        let path = project_root.join(TARGETS_FILE);
        let text = serde_json::to_vec_pretty(self).map_err(|error| {
            VeloctError::Invalid(format!("could not serialise the target list: {error}"))
        })?;
        veloct_core::workdir::ensure_parent(&path)?;
        veloct_core::workdir::write_atomic(&path, &text)
    }

    /// The targets that will actually be deployed to.
    pub fn enabled_targets(&self) -> Vec<&StoredTarget> {
        if !self.enabled {
            return Vec::new();
        }
        self.targets.iter().filter(|target| target.enabled).collect()
    }

    /// Add or replace a target by id.
    pub fn upsert(&mut self, target: StoredTarget) {
        match self.targets.iter_mut().find(|existing| existing.id == target.id) {
            Some(existing) => *existing = target,
            None => self.targets.push(target),
        }
    }

    /// Remove a target by id.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.targets.len();
        self.targets.retain(|target| target.id != id);
        self.targets.len() != before
    }
}

/// One target's result, in the user's terms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetResultView {
    pub target_id: String,
    pub kind: String,
    /// `succeeded`, `partial`, `failed` or `skipped`.
    pub status: String,
    /// Whether the main root deployed.
    pub main_status: String,
    pub main_detail: Option<String>,
    /// The works sub-target, when the target has one.
    pub works_status: Option<String>,
    pub works_detail: Option<String>,
    /// Whether the user can re-send this target.
    pub needs_attention: bool,
    /// The origin the works bodies are served from, when configured.
    pub works_origin: Option<String>,
}

/// What a deploy produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployOutcomeView {
    pub release_id: String,
    pub targets: Vec<TargetResultView>,
    /// Whether every target succeeded. A false here with no failures means everything was skipped.
    pub all_succeeded: bool,
    /// The targets the user should re-send.
    pub needing_attention: Vec<String>,
    /// The files each sub-target pushed, so the window can say what happened.
    pub pushed_files: usize,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// §15.2: the SEO task's own state, kept apart from the deployment statuses.
    pub seo_state: String,
}

impl DeployOutcomeView {
    pub fn of(result: &PublishResult, pushed_files: usize) -> Self {
        let targets: Vec<TargetResultView> = result
            .targets
            .iter()
            .map(|run| TargetResultView {
                target_id: run.target_id.clone(),
                kind: String::new(),
                status: run.status().as_str().to_string(),
                main_status: run.main.status.as_str().to_string(),
                main_detail: run.main.error.clone(),
                works_status: Some(run.works.status.as_str().to_string()),
                works_detail: run.works.error.clone(),
                needs_attention: run.status().needs_attention(),
                works_origin: None,
            })
            .collect();

        Self {
            release_id: result.release_id.clone(),
            all_succeeded: result.all_succeeded(),
            needing_attention: result
                .needing_attention()
                .iter()
                .map(|run| run.target_id.clone())
                .collect(),
            targets,
            pushed_files,
            started_at: result.started_at.clone(),
            finished_at: result.finished_at.clone(),
            seo_state: serde_json::to_value(&result.seo)
                .ok()
                .and_then(|value| {
                    value
                        .get("state")
                        .and_then(|state| state.as_str())
                        .map(str::to_string)
                })
                // A payload that does not serialise is reported as unknown rather than invented.
                .unwrap_or_else(|| "unknown".to_string()),
        }
    }
}

/// Build the adapters for a set of targets.
///
/// A target whose credential is missing is given NO adapter, so the orchestrator reports it as skipped rather
/// than attempting a request with an empty token. That distinction matters: §14.13 requires the UI to show the
/// failure reason, and "no credential configured for cloudflare-main" is a reason the user can act on while
/// "401" from an empty token is not.
/// Build the adapters, consuming them within the caller's frame.
///
/// A closure rather than a function returning a map: the boxes are `'static`, so a borrowing adapter cannot
/// be RETURNED, but it can be built and used inside one frame. The caller supplies `client` and receives
/// whatever the closure produces, which keeps the borrow local.
fn with_adapters<R>(
    targets: &[StoredTarget],
    credentials: CredentialLookup<'_>,
    client: &dyn HttpClient,
    use_adapters: impl FnOnce(&[FrozenTarget], &BTreeMap<String, TargetAdapters>) -> R,
) -> R {
    let mut frozen = Vec::new();
    let mut adapters: BTreeMap<String, TargetAdapters> = BTreeMap::new();

    // A client that cannot be shared is a configuration problem, and it is REPORTED through the target that
    // needed it: no adapter is built, so the orchestrator skips that target with a reason.
    let shared: Option<std::sync::Arc<dyn HttpClient>> = veloct_core::http::share(client);

    for config in targets {
        let target: FrozenTarget = config.into();
        frozen.push(target.clone());

        // The boxes are `'static`, so this function cannot return a borrowing adapter. The client therefore
        // has to outlive the CALLER's use of the map, which is why `deploy_with_client` builds the client, the
        // adapters and the orchestrator call inside one function body.
        let adapter: Option<Box<dyn DeployAdapter>> = match config.kind {
            TargetKind::CloudflarePages => {
                // The account id comes from the configuration and the token from the OS store by handle,
                // because §12 keeps credentials out of SQLite.
                let account = config.project.clone().unwrap_or_default();
                let token = config
                    .credential_ref
                    .as_deref()
                    .and_then(credentials);
                match (token, shared.clone()) {
                    (Some(token), Some(shared)) => Some(Box::new(CloudflarePagesAdapter::new(
                        target.clone(),
                        account,
                        token,
                        // The adapter OWNS a share of the client, because a `Box<dyn DeployAdapter>` is
                        // `'static` and could not hold a borrow.
                        shared,
                    ))),
                    // No credential, or a client that cannot be shared: no adapter, so the orchestrator
                    // skips the target with a reason rather than attempting a request that cannot work.
                    _ => None,
                }
            }
            // ADR-0012: EdgeOne's upload API was never verified, so these are manual exports rather than an
            // invented request. §14.11 says the same about 1Panel.
            TargetKind::EdgeOneMainland
            | TargetKind::EdgeOneInternational
            | TargetKind::OnePanel => Some(Box::new(ManualExportAdapter::new(target.clone()))),
        };

        if let Some(adapter) = adapter {
            // A target with a separate works project gets a second adapter, which is what makes the works
            // bodies live on their own origin (§10.4). Without one, the sub-target is skipped.
            let works: Option<Box<dyn DeployAdapter>> = match &config.works_project {
                Some(_) => match config.kind {
                    TargetKind::CloudflarePages => {
                        let account = config.project.clone().unwrap_or_default();
                        let token = config.credential_ref.as_deref().and_then(credentials);
                        // A works sub-target is only added when both the credential AND a shareable client are
                        // available; otherwise the sub-target is skipped with a reason rather than attempted.
                        match (token, shared.clone()) {
                            (Some(token), Some(works_shared)) => {
                                Some(Box::new(CloudflarePagesAdapter::new(
                                    target.clone(),
                                    account,
                                    token,
                                    works_shared,
                                )) as Box<dyn DeployAdapter>)
                            }
                            _ => None,
                        }
                    }
                    _ => Some(Box::new(ManualExportAdapter::new(target.clone()))
                        as Box<dyn DeployAdapter>),
                },
                None => None,
            };
            adapters.insert(
                config.id.clone(),
                TargetAdapters::new(adapter, works),
            );
        }
    }

    use_adapters(&frozen, &adapters)
}

/// Read the two publish roots, ONCE, so every target sees the same bytes.
pub fn read_payloads(dist: &Path) -> Result<(PublishPayloads, usize)> {
    if !dist.join("site").is_dir() {
        return Err(VeloctError::Invalid(format!(
            "there is no build at {}; publish first, because a deploy does not build (handoff.md §14.1)",
            dist.join("site").display()
        )));
    }
    let payloads = PublishPayloads::read(dist)?;
    let files = payloads.main.as_ref().map(|main| main.files.len()).unwrap_or(0)
        + payloads.works.as_ref().map(|works| works.files.len()).unwrap_or(0);
    Ok((payloads, files))
}

/// Deploy an existing build to every enabled target.
///
/// The client is created here rather than injected, for the reason [`deploy_with_client`] exists: the
/// Cloudflare adapter BORROWS its client, so the client has to outlive the adapters, and a caller cannot hold
/// both. `AiService::call` has the same shape for the same reason.
pub fn deploy(
    project_root: &Path,
    release_id: &str,
    credentials: CredentialLookup<'_>,
    clock: &dyn Clock,
) -> Result<DeployOutcomeView> {
    let client = veloct_core::http::ReqwestHttpClient::with_default_timeout(
        std::time::Duration::from_secs(300),
    )?;
    deploy_with_client(project_root, release_id, credentials, &client, clock)
}

/// Deploy with an injected client, so the whole path is testable without a network.
pub fn deploy_with_client(
    project_root: &Path,
    release_id: &str,
    credentials: CredentialLookup<'_>,
    client: &dyn HttpClient,
    clock: &dyn Clock,
) -> Result<DeployOutcomeView> {
    let list = TargetList::read(project_root)?;

    if !list.enabled {
        return Ok(DeployOutcomeView {
            release_id: release_id.to_string(),
            targets: Vec::new(),
            // Nothing was deployed and nothing failed. Reporting `true` here would be a lie about what
            // happened; the empty target list is what says so.
            all_succeeded: false,
            needing_attention: Vec::new(),
            pushed_files: 0,
            started_at: veloct_core::clock::format_rfc3339(clock.now()),
            finished_at: Some(veloct_core::clock::format_rfc3339(clock.now())),
            seo_state: "not_requested".to_string(),
        });
    }

    // §14.1: ONE build, read once, shared by every target.
    let (payloads, pushed_files) = read_payloads(&project_root.join("dist"))?;

    let enabled: Vec<StoredTarget> = list.enabled_targets().into_iter().cloned().collect();

    let now = veloct_core::clock::format_rfc3339(clock.now());
    // The adapters are built AND consumed inside this call, so a borrowing adapter never has to be returned.
    let result = with_adapters(&enabled, credentials, client, |frozen, adapters| {
        Orchestrator::publish(release_id, frozen, adapters, &payloads, &now)
    })?;

    let mut view = DeployOutcomeView::of(&result, pushed_files);
    // The KIND is filled in from the configuration, because the orchestrator works in frozen targets and the
    // window wants to say "Cloudflare Pages" rather than an id.
    for target in &mut view.targets {
        if let Some(config) = enabled.iter().find(|config| config.id == target.target_id) {
            target.kind = config.kind.as_str().to_string();
            target.works_origin = config.works_origin.clone();
        }
    }
    Ok(view)
}

/// Whether an existing `dist/` can be reused for a re-send (§14.13).
///
/// Returned as the crate's own decision enum rather than a boolean so the caller cannot lose the distinction
/// between "reuse" and "must rebuild".
pub fn dist_decision(
    project_root: &Path,
    expected_fingerprint: &str,
    current_fingerprint: Option<&str>,
) -> DistDecision {
    // The crate's own `decide` owns the comparison, so the reason it produces is the crate's and not a
    // second explanation invented here.
    let _ = veloct_deploy::target::root_revision(&project_root.join("dist"));
    DistDecision::decide(expected_fingerprint, current_fingerprint)
}

/// The sub-targets a target can be re-sent to (§14.13).
pub fn resendable_sub_targets(statuses: &[TargetResultView]) -> Vec<(String, SubTarget)> {
    let mut resendable = Vec::new();
    for target in statuses {
        // A main root that is not succeeded can be re-sent; a works root likewise. The crate's own
        // `needing_attention` cannot be consulted here because this works from the views.
        if target.main_status != "succeeded" && target.main_status != "skipped" {
            resendable.push((target.target_id.clone(), SubTarget::Main));
        }
        if let Some(works) = &target.works_status {
            if works != "succeeded" && works != "skipped" {
                resendable.push((target.target_id.clone(), SubTarget::Works));
            }
        }
    }
    resendable
}

/// Parse a target kind.
pub fn parse_kind(text: &str) -> Result<TargetKind> {
    match text.trim().to_ascii_lowercase().as_str() {
        "cloudflare_pages" | "cloudflare" | "cf" => Ok(TargetKind::CloudflarePages),
        "edgeone_mainland" | "edgeone_cn" => Ok(TargetKind::EdgeOneMainland),
        "edgeone_international" | "edgeone_intl" => Ok(TargetKind::EdgeOneInternational),
        "onepanel" | "1panel" => Ok(TargetKind::OnePanel),
        other => Err(VeloctError::Invalid(format!(
            "unknown deploy target kind {other:?}; expected one of cloudflare_pages, edgeone_mainland, edgeone_international, onepanel"
        ))),
    }
}

/// The human name of a target kind, for the window.
pub fn kind_label(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::CloudflarePages => "Cloudflare Pages",
        TargetKind::EdgeOneMainland => "EdgeOne(中国大陆)",
        TargetKind::EdgeOneInternational => "EdgeOne(国际)",
        TargetKind::OnePanel => "1Panel(手动上传)",
    }
}

/// Whether a kind is deployed automatically or exported for the user.
///
/// ADR-0012 and §14.11: EdgeOne's API was never verified and 1Panel is explicitly not an automatic adapter, so
/// the window has to say which is which rather than presenting all four as equivalent.
pub fn is_manual(kind: TargetKind) -> bool {
    !matches!(kind, TargetKind::CloudflarePages)
}

/// The status of a target list, for the window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetListView {
    pub enabled: bool,
    pub targets: Vec<TargetView>,
}

/// One configured target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetView {
    pub id: String,
    pub kind: String,
    pub kind_label: String,
    pub enabled: bool,
    /// Whether this kind needs the user to upload by hand.
    pub manual: bool,
    pub project: Option<String>,
    pub works_project: Option<String>,
    pub works_origin: Option<String>,
    /// The HANDLE, never a value (§12).
    pub credential_ref: Option<String>,
    /// Whether a credential is actually stored under that handle.
    pub has_credential: bool,
}

impl TargetListView {
    pub fn build(list: &TargetList, credentials: CredentialLookup<'_>) -> Self {
        Self {
            enabled: list.enabled,
            targets: list
                .targets
                .iter()
                .map(|target| {
                    let has = target
                        .credential_ref
                        .as_deref()
                        .map(|handle| credentials(handle).is_some())
                        .unwrap_or(false);
                    TargetView {
                        id: target.id.clone(),
                        kind: target.kind.as_str().to_string(),
                        kind_label: kind_label(target.kind).to_string(),
                        enabled: target.enabled,
                        manual: is_manual(target.kind),
                        project: target.project.clone(),
                        works_project: target.works_project.clone(),
                        works_origin: target.works_origin.clone(),
                        credential_ref: target.credential_ref.clone(),
                        has_credential: has,
                    }
                })
                .collect(),
        }
    }
}

/// A target's logical status, for a caller that wants it by name.
pub fn status_name(status: LogicalStatus) -> &'static str {
    status.as_str()
}

/// The `dist` directory of a project.
pub fn dist_of(project_root: &Path) -> PathBuf {
    project_root.join("dist")
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::FakeHttpClient;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".veloct")).unwrap();
        // A minimal build: the two publish roots with one file each.
        std::fs::create_dir_all(dir.path().join("dist/site")).unwrap();
        std::fs::create_dir_all(dir.path().join("dist/works")).unwrap();
        std::fs::write(dir.path().join("dist/site/index.html"), "<h1>home</h1>").unwrap();
        std::fs::write(dir.path().join("dist/works/index.html"), "<h1>works</h1>").unwrap();
        dir
    }

    fn target(id: &str, kind: TargetKind) -> StoredTarget {
        StoredTarget {
            id: id.to_string(),
            kind,
            enabled: true,
            project: Some("proj".to_string()),
            works_project: None,
            works_origin: None,
            credential_ref: Some(format!("veloct:deploy:{id}")),
        }
    }

    fn clock() -> veloct_core::FixedClock {
        veloct_core::FixedClock::new(
            time::OffsetDateTime::parse(
                "2026-01-01T12:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        )
    }

    #[test]
    fn a_target_list_round_trips_and_a_missing_file_is_an_empty_list() {
        let dir = project();
        let empty = TargetList::read(dir.path()).unwrap();
        assert!(empty.targets.is_empty());
        assert!(empty.enabled);

        let mut list = TargetList::read(dir.path()).unwrap();
        list.upsert(target("cf-main", TargetKind::CloudflarePages));
        list.write(dir.path()).unwrap();

        let reloaded = TargetList::read(dir.path()).unwrap();
        assert_eq!(reloaded.targets.len(), 1);
        assert_eq!(reloaded.targets[0].id, "cf-main");
        assert_eq!(reloaded.targets[0].kind, TargetKind::CloudflarePages);
    }

    #[test]
    fn an_unreadable_target_list_is_an_error_not_an_empty_one() {
        // Silently deploying to nothing would look exactly like a successful deployment of nothing.
        let dir = project();
        std::fs::write(dir.path().join(TARGETS_FILE), "{ not json").unwrap();
        let error = TargetList::read(dir.path()).unwrap_err();
        assert!(error.to_string().contains("not a target list"), "{error}");
    }

    #[test]
    fn upsert_replaces_by_id_rather_than_adding_a_duplicate() {
        let mut list = TargetList::default();
        list.upsert(target("cf", TargetKind::CloudflarePages));
        let mut changed = target("cf", TargetKind::OnePanel);
        changed.project = Some("other".to_string());
        list.upsert(changed);

        assert_eq!(list.targets.len(), 1, "one target, not two");
        assert_eq!(list.targets[0].kind, TargetKind::OnePanel);
        assert_eq!(list.targets[0].project.as_deref(), Some("other"));
    }

    #[test]
    fn removing_a_target_reports_whether_it_was_there() {
        let mut list = TargetList::default();
        list.upsert(target("cf", TargetKind::CloudflarePages));
        assert!(list.remove("cf"));
        assert!(!list.remove("cf"), "removing twice reports the second as absent");
        assert!(list.targets.is_empty());
    }

    #[test]
    fn the_whole_list_can_be_disabled_so_no_target_is_deployed() {
        let mut list = TargetList::default();
        list.upsert(target("cf", TargetKind::CloudflarePages));
        assert_eq!(list.enabled_targets().len(), 1);
        list.enabled = false;
        assert!(list.enabled_targets().is_empty());
    }

    #[test]
    fn a_disabled_target_is_not_in_the_enabled_list() {
        let mut list = TargetList::default();
        let mut off = target("cf", TargetKind::CloudflarePages);
        off.enabled = false;
        list.upsert(off);
        assert!(list.enabled_targets().is_empty());
    }

    #[test]
    fn the_publish_roots_are_read_once_with_both_roots() {
        // handoff.md section 14.1: one build, and every target sees the SAME bytes.
        let dir = project();
        let (payloads, files) = read_payloads(&dir.path().join("dist")).unwrap();
        assert!(payloads.main.is_some());
        assert!(payloads.works.is_some());
        assert_eq!(files, 2);
    }

    #[test]
    fn a_deploy_with_no_build_is_refused_with_what_to_do() {
        // A deploy does not build. Section 14.13's re-send re-uses an existing dist, and a deploy that
        // rebuilt would move the bytes underneath the fingerprint that authorised it.
        let dir = project();
        std::fs::remove_dir_all(dir.path().join("dist")).unwrap();
        let error = read_payloads(&dir.path().join("dist")).unwrap_err();
        assert!(error.to_string().contains("publish first"), "{error}");
    }

    #[test]
    fn a_missing_works_root_is_tolerated_because_it_is_a_normal_degraded_state() {
        // handoff.md section 10.5: a target without an independent Works Origin is degraded, not broken.
        let dir = project();
        std::fs::remove_dir_all(dir.path().join("dist/works")).unwrap();
        let (payloads, files) = read_payloads(&dir.path().join("dist")).unwrap();
        assert!(payloads.main.is_some());
        assert!(payloads.works.is_none());
        assert_eq!(files, 1);
    }

    #[test]
    fn a_cloudflare_target_with_no_credential_gets_no_adapter_so_it_is_skipped_with_a_reason() {
        // An empty token produces a 401 that says nothing useful. "No credential configured" is a reason the
        // user can act on.
        let dir = project();
        let client = FakeHttpClient::new();
        with_adapters(
            &[target("cf", TargetKind::CloudflarePages)],
            &|_| None,
            &client,
            |frozen, adapters| {
                assert_eq!(frozen.len(), 1, "the target is still known");
                assert!(
                    adapters.is_empty(),
                    "but it has no adapter, so the orchestrator skips it"
                );
            },
        );
        let _ = dir;
    }

    #[test]
    fn a_cloudflare_target_with_a_credential_gets_an_adapter_whose_token_came_from_the_handle() {
        let client = FakeHttpClient::new();
        let asked: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        with_adapters(
            &[target("cf", TargetKind::CloudflarePages)],
            &|handle: &str| {
                asked.borrow_mut().push(handle.to_string());
                Some("token-from-the-store".to_string())
            },
            &client,
            |_frozen, adapters| {
                assert_eq!(adapters.len(), 1);
            },
        );
        assert_eq!(*asked.borrow(), vec!["veloct:deploy:cf".to_string()]);
    }

    #[test]
    fn the_manual_kinds_get_a_manual_adapter_rather_than_an_invented_request() {
        // ADR-0012: EdgeOne's upload API was never verified. Section 14.11 says 1Panel is not an automatic
        // adapter. Both report what to upload.
        let client = FakeHttpClient::new();
        for kind in [
            TargetKind::EdgeOneMainland,
            TargetKind::EdgeOneInternational,
            TargetKind::OnePanel,
        ] {
            with_adapters(
                &[target("manual", kind)],
                &|_| Some("unused".to_string()),
                &client,
                |_frozen, adapters| {
                    assert_eq!(adapters.len(), 1, "{kind:?} gets an adapter");
                },
            );
            assert!(is_manual(kind), "{kind:?} is manual");
        }
        assert!(!is_manual(TargetKind::CloudflarePages));
    }

    #[test]
    fn a_target_with_a_works_project_gets_a_second_adapter_so_the_works_live_elsewhere() {
        // handoff.md section 10.4: the works bodies on their own origin is what makes a sub-target real.
        let client = FakeHttpClient::new();
        let mut with_works = target("cf", TargetKind::CloudflarePages);
        with_works.works_project = Some("works-proj".to_string());
        with_works.works_origin = Some("https://works.example.com".to_string());

        with_adapters(
            &[with_works],
            &|_| Some("token".to_string()),
            &client,
            |_frozen, adapters| {
                assert_eq!(adapters.len(), 1);
                assert!(
                    adapters["cf"].works.is_some(),
                    "a separate works project means a second adapter, not a skipped sub-target"
                );
            },
        );
    }

    #[test]
    fn a_target_with_no_works_project_has_no_works_adapter_so_the_sub_target_is_skipped() {
        let client = FakeHttpClient::new();
        with_adapters(
            &[target("cf", TargetKind::CloudflarePages)],
            &|_| Some("token".to_string()),
            &client,
            |_frozen, adapters| {
                assert!(
                    adapters["cf"].works.is_none(),
                    "no works project means the sub-target is skipped rather than attempted"
                );
            },
        );
    }

    #[test]
    fn a_deploy_with_deployment_disabled_does_nothing_and_does_not_claim_success() {
        // Reporting `all_succeeded` for a run that deployed nothing would be a lie about what happened.
        let dir = project();
        let mut list = TargetList::read(dir.path()).unwrap();
        list.enabled = false;
        list.upsert(target("cf", TargetKind::CloudflarePages));
        list.write(dir.path()).unwrap();

        let client = FakeHttpClient::new();
        let outcome =
            deploy_with_client(dir.path(), "rel-1", &|_| Some("t".to_string()), &client, &clock())
                .unwrap();
        assert!(outcome.targets.is_empty());
        assert!(!outcome.all_succeeded);
        assert!(outcome.needing_attention.is_empty());
    }

    #[test]
    fn deploying_with_no_targets_configured_reports_an_empty_run_rather_than_failing() {
        let dir = project();
        let client = FakeHttpClient::new();
        let outcome = deploy_with_client(dir.path(), "rel-1", &|_| None, &client, &clock()).unwrap();
        assert!(outcome.targets.is_empty());
        assert!(!outcome.all_succeeded, "nothing succeeded because nothing ran");
    }

    #[test]
    fn a_manual_target_deploys_as_an_export_and_reports_what_to_upload() {
        // The manual adapter reports rather than pretending (§14.11 and ADR-0012).
        let dir = project();
        let mut list = TargetList::read(dir.path()).unwrap();
        list.upsert(target("onepanel", TargetKind::OnePanel));
        list.write(dir.path()).unwrap();

        let client = FakeHttpClient::new();
        let outcome = deploy_with_client(dir.path(), "rel-1", &|_| None, &client, &clock()).unwrap();
        assert_eq!(outcome.targets.len(), 1, "a manual target still runs");
        assert_eq!(outcome.targets[0].kind, "onepanel");
        assert!(outcome.pushed_files > 0, "the payload was read");
    }

    #[test]
    fn the_resendable_sub_targets_name_only_what_actually_failed() {
        // handoff.md section 14.13: the user re-sends the FAILED sub-target, not the whole release.
        let statuses = vec![
            TargetResultView {
                target_id: "cf".to_string(),
                kind: "cloudflare_pages".to_string(),
                status: "partial".to_string(),
                main_status: "succeeded".to_string(),
                main_detail: None,
                works_status: Some("failed".to_string()),
                works_detail: Some("the works project refused".to_string()),
                needs_attention: true,
                works_origin: None,
            },
            TargetResultView {
                target_id: "edgeone".to_string(),
                kind: "edgeone_mainland".to_string(),
                status: "skipped".to_string(),
                main_status: "skipped".to_string(),
                main_detail: Some("target disabled".to_string()),
                works_status: Some("skipped".to_string()),
                works_detail: None,
                needs_attention: false,
                works_origin: None,
            },
        ];
        let resendable = resendable_sub_targets(&statuses);
        assert_eq!(resendable, vec![("cf".to_string(), SubTarget::Works)]);
    }

    #[test]
    fn a_fully_deployed_target_has_nothing_to_re_send() {
        let statuses = vec![TargetResultView {
            target_id: "cf".to_string(),
            kind: "cloudflare_pages".to_string(),
            status: "succeeded".to_string(),
            main_status: "succeeded".to_string(),
            main_detail: None,
            works_status: Some("succeeded".to_string()),
            works_detail: None,
            needs_attention: false,
            works_origin: None,
        }];
        assert!(resendable_sub_targets(&statuses).is_empty());
    }

    #[test]
    fn a_target_kind_is_parsed_or_refused_rather_than_defaulted() {
        // A typo that silently became Cloudflare would deploy to the wrong place.
        assert_eq!(parse_kind("cloudflare_pages").unwrap(), TargetKind::CloudflarePages);
        assert_eq!(parse_kind("cf").unwrap(), TargetKind::CloudflarePages);
        assert_eq!(parse_kind("edgeone_cn").unwrap(), TargetKind::EdgeOneMainland);
        assert_eq!(parse_kind("1panel").unwrap(), TargetKind::OnePanel);
        assert_eq!(parse_kind("  ONEPANEL  ").unwrap(), TargetKind::OnePanel);
        let error = parse_kind("netlify").unwrap_err();
        assert!(error.to_string().contains("unknown deploy target kind"), "{error}");
    }

    #[test]
    fn every_kind_has_a_label_and_a_manual_flag() {
        for kind in [
            TargetKind::CloudflarePages,
            TargetKind::EdgeOneMainland,
            TargetKind::EdgeOneInternational,
            TargetKind::OnePanel,
        ] {
            assert!(!kind_label(kind).is_empty(), "{kind:?}");
            // And the label is round-trippable through the parser.
            assert_eq!(parse_kind(kind.as_str()).unwrap(), kind);
        }
    }

    #[test]
    fn the_target_view_reports_the_handle_and_whether_a_credential_exists_but_never_the_value() {
        let mut list = TargetList::default();
        list.upsert(target("cf", TargetKind::CloudflarePages));
        let view = TargetListView::build(&list, &|_| Some("SUPER-SECRET".to_string()));
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("SUPER-SECRET"), "{json}");
        assert!(json.contains("has_credential"), "{json}");
        assert!(json.contains("veloct:deploy:cf"), "{json}");
        assert!(view.targets[0].has_credential);
        assert!(!view.targets[0].manual);

        let missing = TargetListView::build(&list, &|_| None);
        assert!(!missing.targets[0].has_credential);
    }

    #[test]
    fn the_deploy_outcome_carries_the_seo_state_apart_from_the_target_statuses() {
        // handoff.md section 15.2: a failed SEO push never fails the site release, so the two are separate
        // fields rather than one.
        let result = PublishResult {
            release_id: "rel-1".to_string(),
            targets: Vec::new(),
            seo: veloct_deploy::SeoTaskState::Pending {
                reason: "offline".to_string(),
            },
            started_at: "2026-01-01T12:00:00Z".to_string(),
            finished_at: None,
        };
        let view = DeployOutcomeView::of(&result, 5);
        assert_eq!(view.seo_state, "pending");
        assert_eq!(view.pushed_files, 5);
        assert!(view.targets.is_empty());
    }

    #[test]
    fn the_dist_decision_reuses_only_when_the_fingerprint_matches() {
        // Section 14.13: a re-send re-uses the existing dist when nothing changed, and rebuilds when it did.
        let dir = project();
        assert!(matches!(
            dist_decision(dir.path(), "same", Some("same")),
            DistDecision::Reuse { .. }
        ));
        assert!(matches!(
            dist_decision(dir.path(), "old", Some("new")),
            DistDecision::Rebuild { .. }
        ));
        // With no current dist recorded, a rebuild is the only honest answer.
        assert!(matches!(
            dist_decision(dir.path(), "same", None),
            DistDecision::Rebuild { .. }
        ));
    }

    #[test]
    fn the_dist_of_a_project_is_where_the_build_writes() {
        assert!(dist_of(Path::new("D:/proj")).ends_with("dist"));
    }
}
