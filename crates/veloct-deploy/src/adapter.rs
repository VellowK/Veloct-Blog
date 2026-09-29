//! Deployment adapters.
//!
//! `handoff.md` §14.10 names three automatic targets and §14.12 makes each one two sub-targets, so an
//! adapter's job is to push ONE publish root to ONE place. That shape is deliberate: it makes
//! "the main root succeeded and the works root failed" a state the type system produces naturally,
//! rather than something the caller has to remember to record.
//!
//! ADR-0012 records the honest position on EdgeOne: its upload API could not be verified from an
//! authoritative source, so those two targets are export targets. They are selectable, they produce
//! exactly the payload to upload, and they report `manual` rather than pretending to have pushed.
//! Inventing an endpoint would produce an adapter that reports false success, which ADR-0010 forbids.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::http::{HttpClient, HttpRequest};
use veloct_core::{Result, VeloctError};

use crate::target::{FrozenTarget, SubTarget, TargetKind};

/// One file to push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushFile {
    /// The path inside the root, with forward slashes.
    pub relative_path: String,
    /// The file's bytes.
    pub bytes: Vec<u8>,
    /// The SHA-256 of the bytes, which the Cloudflare direct-upload API needs.
    pub sha256: String,
}

impl PushFile {
    pub fn new(relative_path: impl Into<String>, bytes: Vec<u8>) -> Self {
        let sha256 = veloct_core::sha256_hex(&bytes);
        Self {
            relative_path: relative_path.into(),
            bytes,
            sha256,
        }
    }

    pub fn size(&self) -> usize {
        self.bytes.len()
    }
}

/// The payload for one sub-target: the files of one publish root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushPayload {
    pub sub_target: SubTarget,
    /// The root's absolute path, for the log and for a manual export.
    pub root: PathBuf,
    pub files: Vec<PushFile>,
}

impl PushPayload {
    /// Read a publish root from disk.
    ///
    /// A missing root is an error rather than an empty payload: pushing an empty site would report
    /// success while taking the live site down.
    pub fn read(sub_target: SubTarget, root: &Path) -> Result<Self> {
        if !root.is_dir() {
            return Err(VeloctError::Deploy {
                target: "local".to_string(),
                sub_target: sub_target.as_str().to_string(),
                message: format!(
                    "the {} publish root {} does not exist, so there is nothing to deploy",
                    sub_target.as_str(),
                    root.display()
                ),
            });
        }

        let mut files = Vec::new();
        collect(root, root, &mut files)?;
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

        // An empty root is also refused, for the same reason.
        if files.is_empty() {
            return Err(VeloctError::Deploy {
                target: "local".to_string(),
                sub_target: sub_target.as_str().to_string(),
                message: format!("the {} publish root {} is empty", sub_target.as_str(), root.display()),
            });
        }

        Ok(Self {
            sub_target,
            root: root.to_path_buf(),
            files,
        })
    }

    pub fn total_bytes(&self) -> usize {
        self.files.iter().map(PushFile::size).sum()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// The paths, for a comparison or a manifest.
    pub fn paths(&self) -> Vec<&str> {
        self.files
            .iter()
            .map(|file| file.relative_path.as_str())
            .collect()
    }
}

/// Walk a root, collecting every regular file.
fn collect(root: &Path, current: &Path, out: &mut Vec<PushFile>) -> Result<()> {
    let entries = std::fs::read_dir(current).map_err(|error| VeloctError::Io {
        path: current.to_path_buf(),
        source: error,
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| VeloctError::Io {
            path: current.to_path_buf(),
            source: error,
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| VeloctError::Invalid("a file escaped the publish root".into()))?
            .to_string_lossy()
            .replace('\\', "/");
        let bytes = std::fs::read(&path).map_err(|error| VeloctError::io(&path, error))?;
        out.push(PushFile::new(relative, bytes));
    }
    Ok(())
}

/// What an adapter did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushOutcome {
    /// The number of files the adapter actually sent or installed.
    pub files: usize,
    /// A human-readable summary for the log.
    pub message: String,
    /// The URL the root is now served at, when the adapter knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether the adapter only EXPORTED the payload for a manual upload.
    ///
    /// `handoff.md` §14.11 and ADR-0012: a manual target must say so rather than report a deployment
    /// that did not happen.
    #[serde(default)]
    pub manual: bool,
}

/// Whether an adapter can deploy at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployCapabilities {
    /// Whether the protocol is verified (`crate::target::TargetKind::is_verified`).
    pub verified: bool,
    /// Whether the adapter performs an automatic deployment.
    pub automatic: bool,
    /// Whether it needs a credential to run.
    pub needs_credential: bool,
}

/// Deploys one publish root to one place.
///
/// One method, taking one payload. There is no "deploy everything" call, because §14.12 needs the two
/// roots to fail independently and a combined call would have to invent an aggregate error.
pub trait DeployAdapter: Send + Sync {
    fn target(&self) -> &FrozenTarget;

    fn capabilities(&self) -> DeployCapabilities;

    /// Push one root.
    fn push(&self, payload: &PushPayload) -> Result<PushOutcome>;

    /// Prove the target is reachable, or explain why it is not
    /// (`handoff.md` §14.13: the UI shows the failure reason).
    fn test_connection(&self) -> Result<String>;
}

/// An adapter that keeps everything in memory, so the deployment orchestration is testable.
#[derive(Debug)]
pub struct MemoryAdapter {
    target: FrozenTarget,
    /// Every root that was pushed, so a test can prove exactly what happened.
    pushes: std::sync::Mutex<Vec<(SubTarget, usize)>>,
    /// Which sub-targets fail, and why. A SET rather than one entry: "both roots failed" is a real
    /// state, and a single slot made the second `fail_sub_target` silently replace the first.
    fail_on: std::sync::Mutex<Vec<(SubTarget, String)>>,
    /// When true, the adapter reports a manual export instead of deploying.
    manual: bool,
}

impl MemoryAdapter {
    pub fn new(target: FrozenTarget) -> Self {
        Self {
            target,
            pushes: std::sync::Mutex::new(Vec::new()),
            fail_on: std::sync::Mutex::new(Vec::new()),
            manual: false,
        }
    }

    /// A manual adapter, for the export targets.
    pub fn manual(target: FrozenTarget) -> Self {
        Self {
            manual: true,
            ..Self::new(target)
        }
    }

    /// Make every push of this sub-target fail.
    pub fn fail_sub_target(&self, sub_target: SubTarget, message: &str) {
        let mut failures = self.fail_on.lock().unwrap();
        failures.retain(|(existing, _)| *existing != sub_target);
        failures.push((sub_target, message.to_string()));
    }

    /// Make every push fail, for both roots.
    pub fn fail_all(&self, message: &str) {
        self.fail_sub_target(SubTarget::Main, message);
        self.fail_sub_target(SubTarget::Works, message);
    }

    pub fn clear_failures(&self) {
        self.fail_on.lock().unwrap().clear();
    }

    pub fn pushes(&self) -> Vec<(SubTarget, usize)> {
        self.pushes.lock().unwrap().clone()
    }

    pub fn push_count(&self, sub_target: SubTarget) -> usize {
        self.pushes
            .lock()
            .unwrap()
            .iter()
            .filter(|(pushed, _)| *pushed == sub_target)
            .count()
    }
}

impl DeployAdapter for MemoryAdapter {
    fn target(&self) -> &FrozenTarget {
        &self.target
    }

    fn capabilities(&self) -> DeployCapabilities {
        DeployCapabilities {
            verified: self.target.kind.is_verified(),
            automatic: !self.manual && self.target.kind.is_automatic(),
            needs_credential: !self.manual,
        }
    }

    fn push(&self, payload: &PushPayload) -> Result<PushOutcome> {
        if self.manual {
            // A manual target says what to upload and where, and does not claim to have done it.
            return Ok(PushOutcome {
                files: 0,
                message: format!(
                    "export {} ({} files) for manual upload to {}",
                    payload.sub_target.as_str(),
                    payload.file_count(),
                    self.target.kind.label()
                ),
                url: self.target.works_origin.clone(),
                manual: true,
            });
        }

        let failure = self
            .fail_on
            .lock()
            .unwrap()
            .iter()
            .find(|(sub_target, _)| *sub_target == payload.sub_target)
            .map(|(_, message)| message.clone());
        if let Some(message) = failure {
            return Err(VeloctError::Deploy {
                target: self.target.id.clone(),
                sub_target: payload.sub_target.as_str().to_string(),
                message,
            });
        }

        self.pushes
            .lock()
            .unwrap()
            .push((payload.sub_target, payload.file_count()));

        Ok(PushOutcome {
            files: payload.file_count(),
            message: format!(
                "pushed {} files to {} ({})",
                payload.file_count(),
                self.target.kind.label(),
                payload.sub_target.as_str()
            ),
            url: if payload.sub_target == SubTarget::Works {
                self.target.works_origin.clone()
            } else {
                None
            },
            manual: false,
        })
    }

    fn test_connection(&self) -> Result<String> {
        if self.manual {
            return Ok(format!(
                "{} takes a manual upload, so there is nothing to connect to",
                self.target.kind.label()
            ));
        }
        Ok(format!("{} is reachable", self.target.kind.label()))
    }
}

/// The Cloudflare Pages adapter, using the documented direct-upload API.
///
/// The sequence is the three documented steps: check which of the file hashes the project already
/// has, upload the missing ones, then create a deployment referencing them. Sending the hashes first
/// is what makes a re-publish of a mostly-unchanged site cheap.
/// The Cloudflare Pages adapter.
///
/// It SHARES its client rather than borrowing it. The first version held `&'a dyn HttpClient`, which made the
/// adapter non-`'static` and therefore impossible to put in a `Box<dyn DeployAdapter>` — so the application
/// could not build its adapters at all, which is what the lifetime error was really saying. An `Arc` is
/// itself an `HttpClient`, so sharing costs nothing at the call site.
pub struct CloudflarePagesAdapter {
    target: FrozenTarget,
    account_id: String,
    /// A credential HANDLE, resolved to a value by the caller before the adapter runs.
    token: String,
    client: std::sync::Arc<dyn HttpClient>,
}

impl CloudflarePagesAdapter {
    pub fn new(
        target: FrozenTarget,
        account_id: impl Into<String>,
        token: impl Into<String>,
        client: std::sync::Arc<dyn HttpClient>,
    ) -> Self {
        Self {
            target,
            account_id: account_id.into(),
            token: token.into(),
            client,
        }
    }

    fn base(&self) -> String {
        format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/pages/projects/{}",
            self.account_id,
            self.target.project.as_deref().unwrap_or_default()
        )
    }

    /// The project name for a sub-target.
    ///
    /// The works root may live in its own project, which is what makes it a genuinely different
    /// origin (`handoff.md` §10.4).
    fn project_or_works(&self) -> String {
        self.target.project.clone().unwrap_or_default()
    }

    /// Which hashes the project already has (`handoff.md` §14.1: one build, two roots).
    pub fn missing_files<'p>(&self, payload: &'p PushPayload) -> Result<Vec<&'p PushFile>> {
        let body = serde_json::json!({
            "hashes": payload.files.iter().map(|file| file.sha256.clone()).collect::<Vec<_>>()
        });
        let request = HttpRequest::post(format!("{}/upload/check-missing", self.base()))
            .bearer(&self.token)
            .json(body);

        let response = self.client.execute(request)?;
        if !response.is_success() {
            return Err(VeloctError::Deploy {
                target: self.target.id.clone(),
                sub_target: payload.sub_target.as_str().to_string(),
                message: format!(
                    "Cloudflare rejected the hash check with HTTP {}: {}",
                    response.status,
                    response.text()
                ),
            });
        }

        // The response lists the hashes that are MISSING, so the files to upload are the ones whose
        // hash appears in that list. A hash we do not recognise is ignored rather than guessed at.
        let value: serde_json::Value = response.json().map_err(|_| VeloctError::Deploy {
            target: self.target.id.clone(),
            sub_target: payload.sub_target.as_str().to_string(),
            message: "Cloudflare returned a hash check that is not JSON".to_string(),
        })?;

        let missing: Vec<String> = value
            .get("result")
            .and_then(|result| result.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        Ok(payload
            .files
            .iter()
            .filter(|file| missing.contains(&file.sha256))
            .collect())
    }

    /// Upload one file's bytes.
    fn upload_file(&self, file: &PushFile) -> Result<()> {
        let request = HttpRequest::post(format!("{}/upload", self.base()))
            .bearer(&self.token)
            .header("Content-Type", "application/octet-stream")
            .bytes(file.bytes.clone());

        let response = self.client.execute(request)?;
        if response.is_success() {
            return Ok(());
        }
        Err(VeloctError::Deploy {
            target: self.target.id.clone(),
            sub_target: self.target.id.clone(),
            message: format!(
                "Cloudflare rejected the upload of {} with HTTP {}: {}",
                file.relative_path,
                response.status,
                response.text()
            ),
        })
    }

    /// Create the deployment that references every hash.
    fn create_deployment(&self, payload: &PushPayload, missing: &[&PushFile]) -> Result<PushOutcome> {
        let mut manifest: BTreeMap<String, String> = BTreeMap::new();
        for file in &payload.files {
            manifest.insert(format!("/{}", file.relative_path), file.sha256.clone());
        }

        let body = serde_json::json!({
            "branch": "main",
            "commit_hash": veloct_core::sha256_hex_str(payload.sub_target.as_str()),
            "manifest": manifest,
        });
        let _ = missing;

        let request = HttpRequest::post(format!("{}/deployments", self.base()))
            .bearer(&self.token)
            .json(body);
        let response = self.client.execute(request)?;
        if !response.is_success() {
            return Err(VeloctError::Deploy {
                target: self.target.id.clone(),
                sub_target: payload.sub_target.as_str().to_string(),
                message: format!(
                    "Cloudflare rejected the deployment with HTTP {}: {}",
                    response.status,
                    response.text()
                ),
            });
        }

        let url = response
            .json()
            .ok()
            .and_then(|value: serde_json::Value| {
                value
                    .get("result")
                    .and_then(|result| result.get("url"))
                    .and_then(|url| url.as_str())
                    .map(str::to_string)
            })
            .or_else(|| {
                Some(format!(
                    "https://{}.pages.dev",
                    self.project_or_works()
                ))
            });

        Ok(PushOutcome {
            files: payload.file_count(),
            message: format!(
                "deployed {} files to Cloudflare Pages project {}",
                payload.file_count(),
                self.project_or_works()
            ),
            url,
            manual: false,
        })
    }
}

impl DeployAdapter for CloudflarePagesAdapter {
    fn target(&self) -> &FrozenTarget {
        &self.target
    }

    fn capabilities(&self) -> DeployCapabilities {
        DeployCapabilities {
            verified: true,
            automatic: true,
            needs_credential: true,
        }
    }

    fn push(&self, payload: &PushPayload) -> Result<PushOutcome> {
        let missing = self.missing_files(payload)?;
        // Only the missing hashes are uploaded, so re-publishing a mostly-unchanged site sends almost
        // nothing.
        for file in &missing {
            self.upload_file(file)?;
        }
        self.create_deployment(payload, &missing)
    }

    fn test_connection(&self) -> Result<String> {
        let request = HttpRequest::get(self.base()).bearer(&self.token);
        let response = self.client.execute(request)?;
        if response.is_success() {
            return Ok(format!(
                "Cloudflare project {} is reachable",
                self.project_or_works()
            ));
        }
        Err(VeloctError::Deploy {
            target: self.target.id.clone(),
            sub_target: "main".to_string(),
            message: format!(
                "Cloudflare rejected the project check with HTTP {}: {}",
                response.status,
                response.text()
            ),
        })
    }
}

/// A manual export adapter.
///
/// `handoff.md` §14.11 says 1Panel is not an automatic adapter, and ADR-0012 records the same for
/// EdgeOne until its upload API is verified. This adapter's whole job is to say so, and to report
/// where the payload is.
pub struct ManualExportAdapter {
    target: FrozenTarget,
}

impl ManualExportAdapter {
    pub fn new(target: FrozenTarget) -> Self {
        Self { target }
    }
}

impl DeployAdapter for ManualExportAdapter {
    fn target(&self) -> &FrozenTarget {
        &self.target
    }

    fn capabilities(&self) -> DeployCapabilities {
        DeployCapabilities {
            // A manual export has no protocol to verify, so there is nothing unverified about it.
            verified: true,
            automatic: false,
            needs_credential: false,
        }
    }

    fn push(&self, payload: &PushPayload) -> Result<PushOutcome> {
        Ok(PushOutcome {
            files: 0,
            message: format!(
                "{}: export dist/{} ({} files, {} bytes) and upload it manually",
                self.target.kind.label(),
                payload.sub_target.as_str(),
                payload.file_count(),
                payload.total_bytes()
            ),
            url: if payload.sub_target == SubTarget::Works {
                self.target.works_origin.clone()
            } else {
                None
            },
            // The contract requires this to be visible rather than reported as a deployment.
            manual: true,
        })
    }

    fn test_connection(&self) -> Result<String> {
        Ok(format!(
            "{} takes a manual upload, so there is nothing to connect to",
            self.target.kind.label()
        ))
    }
}

/// The result of a manual export, listing what the user should upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportManifest {
    pub target_id: String,
    pub kind: TargetKind,
    /// The main root's path and file count.
    pub main: ExportRoot,
    /// The works root, when the target has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub works: Option<ExportRoot>,
    /// Whether the target has no independent works origin
    /// (`handoff.md` §10.5: a target without one is degraded).
    pub degraded: bool,
}

/// One root in an export manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRoot {
    /// The path as the user sees it, relative to the project.
    pub path: String,
    pub files: usize,
    pub bytes: usize,
}

impl ExportManifest {
    /// Build the manifest a 1Panel or EdgeOne user uploads from.
    pub fn build(
        target: &FrozenTarget,
        main: &PushPayload,
        works: Option<&PushPayload>,
    ) -> Self {
        let relative = |root: &Path| {
            root.to_string_lossy()
                .replace('\\', "/")
                .replace("//", "/")
        };
        Self {
            target_id: target.id.clone(),
            kind: target.kind,
            main: ExportRoot {
                path: relative(&main.root),
                files: main.file_count(),
                bytes: main.total_bytes(),
            },
            works: works.map(|payload| ExportRoot {
                path: relative(&payload.root),
                files: payload.file_count(),
                bytes: payload.total_bytes(),
            }),
            degraded: target.is_degraded(),
        }
    }

    /// Whether both roots are present.
    pub fn has_both_roots(&self) -> bool {
        self.works.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::target::TargetConfig;
    use tempfile::TempDir;
    use veloct_core::http::FakeHttpClient;

    fn frozen(kind: TargetKind, project: &str) -> FrozenTarget {
        let mut config = TargetConfig::new("t", kind);
        config.project = Some(project.to_string());
        FrozenTarget::from(&config)
    }

    fn write_root(dir: &Path, files: &[(&str, &str)]) {
        for (relative, contents) in files {
            let path = dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }

    #[test]
    fn reading_a_publish_root_collects_every_file() {
        let dir = TempDir::new().unwrap();
        write_root(
            dir.path(),
            &[("index.html", "<html></html>"), ("assets/app.js", "// app")],
        );

        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();
        assert_eq!(payload.file_count(), 2);
        assert_eq!(payload.paths(), vec!["assets/app.js", "index.html"]);
        assert!(payload.total_bytes() > 0);
    }

    #[test]
    fn reading_a_root_that_does_not_exist_is_an_error_not_an_empty_payload() {
        // Pushing an empty site would report success while taking the live site down.
        let dir = TempDir::new().unwrap();
        let error = PushPayload::read(SubTarget::Main, &dir.path().join("absent")).unwrap_err();
        assert!(error.to_string().contains("does not exist"), "{error}");
    }

    #[test]
    fn reading_an_empty_root_is_refused_for_the_same_reason() {
        let dir = TempDir::new().unwrap();
        let error = PushPayload::read(SubTarget::Main, dir.path()).unwrap_err();
        assert!(error.to_string().contains("is empty"), "{error}");
    }

    #[test]
    fn a_push_file_computes_its_own_hash() {
        let file = PushFile::new("a.txt", b"hello".to_vec());
        assert_eq!(file.sha256, veloct_core::sha256_hex(b"hello"));
        assert_eq!(file.size(), 5);
    }

    #[test]
    fn an_adapter_pushes_one_root_and_reports_it() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();

        let adapter = MemoryAdapter::new(frozen(TargetKind::CloudflarePages, "my-site"));
        let outcome = adapter.push(&payload).unwrap();

        assert!(!outcome.manual);
        assert_eq!(outcome.files, 1);
        assert!(outcome.message.contains("pushed"), "{}", outcome.message);
        assert_eq!(adapter.push_count(SubTarget::Main), 1);
    }

    #[test]
    fn a_failing_sub_target_does_not_affect_the_other_one() {
        // handoff.md section 14.12: the two roots fail independently.
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Works, dir.path()).unwrap();

        let adapter = MemoryAdapter::new(frozen(TargetKind::CloudflarePages, "my-site"));
        adapter.fail_sub_target(SubTarget::Works, "the works project does not exist");

        let error = adapter.push(&payload).unwrap_err();
        assert!(error.to_string().contains("works project does not exist"), "{error}");
        assert_eq!(adapter.push_count(SubTarget::Works), 0);

        // The main root still works, which is the property the roll-up depends on.
        let main = PushPayload::read(SubTarget::Main, dir.path()).unwrap();
        assert!(adapter.push(&main).is_ok());
        assert_eq!(adapter.push_count(SubTarget::Main), 1);
    }

    #[test]
    fn a_failure_can_be_cleared_so_a_re_send_can_succeed() {
        // handoff.md section 14.13: the user re-publishes the failed sub-target.
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Works, dir.path()).unwrap();

        let adapter = MemoryAdapter::new(frozen(TargetKind::CloudflarePages, "s"));
        adapter.fail_sub_target(SubTarget::Works, "temporary");
        assert!(adapter.push(&payload).is_err());

        adapter.clear_failures();
        assert!(adapter.push(&payload).is_ok());
        assert_eq!(adapter.push_count(SubTarget::Works), 1);
    }

    #[test]
    fn a_manual_adapter_reports_an_export_rather_than_a_deployment() {
        // handoff.md section 14.11 and ADR-0012.
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();

        let adapter = ManualExportAdapter::new(frozen(TargetKind::OnePanel, "panel"));
        let outcome = adapter.push(&payload).unwrap();

        assert!(outcome.manual, "a manual target must say so");
        assert_eq!(outcome.files, 0, "nothing was sent");
        assert!(outcome.message.contains("manual"), "{}", outcome.message);
        assert!(!adapter.capabilities().automatic);
    }

    #[test]
    fn a_memory_adapter_can_stand_in_for_a_manual_target_too() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();

        let adapter = MemoryAdapter::manual(frozen(TargetKind::EdgeOneMainland, "cn"));
        let outcome = adapter.push(&payload).unwrap();
        assert!(outcome.manual);
        assert!(!adapter.capabilities().automatic);
        // ADR-0012: an unverified protocol reports that it is unverified.
        assert!(!adapter.capabilities().verified);
    }

    #[test]
    fn capabilities_report_honestly_for_each_kind() {
        let memory = MemoryAdapter::new(frozen(TargetKind::CloudflarePages, "s"));
        assert!(memory.capabilities().verified);
        assert!(memory.capabilities().automatic);

        let edgeone = MemoryAdapter::new(frozen(TargetKind::EdgeOneInternational, "intl"));
        assert!(!edgeone.capabilities().verified, "ADR-0012: not verified");
    }

    #[test]
    fn the_cloudflare_adapter_only_uploads_the_missing_hashes() {
        // The documented sequence sends the hashes first, which is what makes a re-publish of a
        // mostly-unchanged site cheap.
        let dir = TempDir::new().unwrap();
        write_root(
            dir.path(),
            &[("index.html", "<html>"), ("assets/app.js", "// app")],
        );
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();
        let known = payload.files[0].sha256.clone();

        let client = FakeHttpClient::new()
            .on_url_contains("/upload/check-missing", 200, format!(r#"{{"result":["{known}"]}}"#))
            .on_url_contains("/deployments", 200, r#"{"result":{"url":"https://x.pages.dev"}}"#);

        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );

        let missing = adapter.missing_files(&payload).unwrap();
        assert_eq!(missing.len(), 1, "only the hash Cloudflare does not have");
        assert_eq!(missing[0].sha256, known);
    }

    #[test]
    fn the_cloudflare_adapter_uploads_then_creates_a_deployment() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();
        let hash = payload.files[0].sha256.clone();

        let client = FakeHttpClient::new()
            .on_url_contains("/upload/check-missing", 200, format!(r#"{{"result":["{hash}"]}}"#))
            .on_url_contains("/upload", 200, "{}")
            .on_url_contains("/deployments", 200, r#"{"result":{"url":"https://my-site.pages.dev"}}"#);

        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );

        let outcome = adapter.push(&payload).unwrap();
        assert!(!outcome.manual);
        assert_eq!(outcome.files, 1);
        assert_eq!(outcome.url.as_deref(), Some("https://my-site.pages.dev"));

        let requests = client.recorded();
        let urls: Vec<&str> = requests.iter().map(|r| r.url.as_str()).collect();
        assert!(urls.iter().any(|u| u.ends_with("/upload/check-missing")), "{urls:?}");
        assert!(urls.iter().any(|u| u.ends_with("/upload")), "{urls:?}");
        assert!(urls.iter().any(|u| u.ends_with("/deployments")), "{urls:?}");
    }

    #[test]
    fn the_cloudflare_adapter_sends_its_credential_as_a_bearer_token() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();

        let client = FakeHttpClient::new()
            .on_url_contains("/upload/check-missing", 200, r#"{"result":[]}"#)
            .on_url_contains("/deployments", 200, "{}");

        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "secret-token",
            std::sync::Arc::new(client.clone()),
        );
        adapter.push(&payload).unwrap();

        for request in client.recorded() {
            assert!(
                request
                    .headers
                    .iter()
                    .any(|(name, value)| name == "Authorization" && value == "Bearer secret-token"),
                "every Cloudflare call must carry the token"
            );
        }
    }

    #[test]
    fn the_cloudflare_adapter_reports_a_rejected_hash_check() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();

        let client = FakeHttpClient::new().on_url_contains(
            "/upload/check-missing",
            403,
            "the token lacks Pages:Edit",
        );
        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );

        let error = adapter.push(&payload).unwrap_err();
        assert!(error.to_string().contains("403"), "{error}");
        assert!(error.to_string().contains("Pages:Edit"), "{error}");
    }

    #[test]
    fn the_cloudflare_adapter_reports_a_rejected_upload() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();
        let hash = payload.files[0].sha256.clone();

        let client = FakeHttpClient::new()
            .on_url_contains("/upload/check-missing", 200, format!(r#"{{"result":["{hash}"]}}"#))
            .on_url_contains("/upload", 500, "internal error");

        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );
        let error = adapter.push(&payload).unwrap_err();
        assert!(error.to_string().contains("500"), "{error}");
        assert!(error.to_string().contains("index.html"), "{error}");
    }

    #[test]
    fn the_cloudflare_adapter_does_not_upload_when_every_hash_is_known() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let payload = PushPayload::read(SubTarget::Main, dir.path()).unwrap();

        let client = FakeHttpClient::new()
            .on_url_contains("/upload/check-missing", 200, r#"{"result":[]}"#)
            .on_url_contains("/deployments", 200, "{}");

        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );
        adapter.push(&payload).unwrap();

        let uploads = client
            .recorded()
            .iter()
            .filter(|request| request.url.ends_with("/upload"))
            .count();
        assert_eq!(uploads, 0, "nothing needed uploading");
    }

    #[test]
    fn a_connection_test_reports_success_and_uses_the_project_endpoint() {
        let client = FakeHttpClient::new().on_url_contains("/pages/projects", 200, "{}");
        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );
        let message = adapter.test_connection().unwrap();
        assert!(message.contains("my-site"), "{message}");
    }

    #[test]
    fn a_connection_test_reports_the_failure_reason() {
        // handoff.md section 14.13: the UI shows the failure reason.
        let client = FakeHttpClient::new().on_url_contains("/pages/projects", 404, "project not found");
        let adapter = CloudflarePagesAdapter::new(
            frozen(TargetKind::CloudflarePages, "my-site"),
            "acct",
            "token",
            std::sync::Arc::new(client.clone()),
        );
        let error = adapter.test_connection().unwrap_err();
        assert!(error.to_string().contains("project not found"), "{error}");
    }

    #[test]
    fn a_manual_adapter_has_nothing_to_connect_to() {
        let adapter = ManualExportAdapter::new(frozen(TargetKind::OnePanel, "panel"));
        let message = adapter.test_connection().unwrap();
        assert!(message.contains("manual"), "{message}");
        assert!(adapter.capabilities().verified);
    }

    #[test]
    fn an_export_manifest_names_both_roots_and_marks_a_degraded_target() {
        // handoff.md section 14.11: the user uploads dist/site and dist/works themselves.
        let dir = TempDir::new().unwrap();
        let main_root = dir.path().join("dist/site");
        let works_root = dir.path().join("dist/works");
        write_root(&main_root, &[("index.html", "<html>")]);
        write_root(&works_root, &[("demo/index.html", "<html>")]);

        let main = PushPayload::read(SubTarget::Main, &main_root).unwrap();
        let works = PushPayload::read(SubTarget::Works, &works_root).unwrap();

        let full = FrozenTarget::from(&{
            let mut config = TargetConfig::new("panel", TargetKind::OnePanel);
            config.works_origin = Some("https://works.example.com".to_string());
            config
        });
        let manifest = ExportManifest::build(&full, &main, Some(&works));
        assert!(manifest.has_both_roots());
        assert!(!manifest.degraded);
        assert_eq!(manifest.main.files, 1);
        assert_eq!(manifest.works.unwrap().files, 1);

        // A target with no works origin is degraded and has one root.
        let degraded = FrozenTarget::from(&TargetConfig::new("edgeone", TargetKind::EdgeOneMainland));
        let manifest = ExportManifest::build(&degraded, &main, None);
        assert!(!manifest.has_both_roots());
        assert!(manifest.degraded);
    }

    #[test]
    fn an_export_manifest_round_trips_through_json() {
        let dir = TempDir::new().unwrap();
        write_root(dir.path(), &[("index.html", "<html>")]);
        let main = PushPayload::read(SubTarget::Main, dir.path()).unwrap();
        let manifest = ExportManifest::build(
            &frozen(TargetKind::OnePanel, "panel"),
            &main,
            None,
        );
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"one_panel\""), "{json}");
        assert_eq!(serde_json::from_str::<ExportManifest>(&json).unwrap(), manifest);
    }
}
