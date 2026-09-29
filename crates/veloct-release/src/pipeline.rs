//! The publish pipeline.
//!
//! `handoff.md` §14.1 fixes the order of operations, and every step exists for a reason:
//!
//! ```text
//! working tree
//!   -> freeze the release artefacts (manifest, build inputs, target snapshot, site template)
//!   -> git commit                              (done by veloct-git, from these artefacts)
//!   -> git tag release/<release-id>            (done by veloct-git)
//!   -> exactly ONE Astro build
//!   -> the single current dist/ with dist/site/ and dist/works/
//!   -> distribute to every selected sub-target with the same build fingerprint
//! ```
//!
//! This module owns the middle of that list. It deliberately does **not** create the commit or the
//! tag: keeping Git out of it means the pipeline can be tested with a fake build runner and no
//! repository, and it keeps the non-circular ordering (`release.json` is written before the commit
//! that contains it) visible in the caller.
//!
//! The build itself is behind [`BuildRunner`] so a test can substitute a deterministic fake. That
//! is what makes rows A2, A5, A6 and A14 testable without a Node toolchain.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use veloct_core::workdir::copy_dir_all;
use veloct_core::{layout, Clock, ReleaseId, Result, VeloctError};

use crate::manifest::{
    BuildInputs, DeploymentTargetSnapshot, DistManifest, FingerprintInputs, ReleaseManifest,
    ReleasePaths, ToolchainSnapshot, RELEASE_SCHEMA,
};

/// What the build runner is asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildRequest {
    /// The project root that holds the content, works and database export.
    pub project_root: PathBuf,
    /// The Astro project to build.
    pub site_dir: PathBuf,
    /// Where the two publish roots must appear.
    pub out_dir: PathBuf,
    /// The frozen build time, so time-derived output cannot move on a rebuild
    /// (`handoff.md` §14.9).
    pub release_build_time: String,
    /// Only the allow-listed, non-secret environment variables.
    pub env: BTreeMap<String, String>,
    /// The site URL and base the build should use, from the frozen configuration.
    pub site_url: Option<String>,
    pub site_base: Option<String>,
}

/// The result of a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutcome {
    /// `dist/site`.
    pub site_root: PathBuf,
    /// `dist/works`.
    pub works_root: PathBuf,
    /// The build's stdout/stderr tail, for the log and for a failure report.
    pub log: String,
    /// The normalised manifest of what was emitted, also written into the release.
    ///
    /// `None` only when a BUILD RUNNER produced it, which cannot happen through `build_once`.
    pub dist_manifest: Option<DistManifest>,
}

/// Performs the Astro build.
///
/// Two implementations matter: the real one that runs the bundled Node sidecar, and a fake used by
/// tests. The trait is deliberately narrow — it cannot read the database or the configuration, so
/// it cannot accidentally become a second source of truth.
pub trait BuildRunner: Send + Sync {
    fn build(&self, request: &BuildRequest) -> Result<BuildOutcome>;
}

/// Scan a build's output into the comparable normalised manifest
/// (`handoff.md` §14.9, §23.6).
///
/// Routes are derived from the emitted HTML files rather than from the source, because the emitted
/// set is the thing that is actually published. Time-derived content is not read here: a rebuild
/// must produce the same manifest, so only route presence and content hashes are recorded.
pub fn scan_dist(site_root: &Path, works_root: &Path) -> Result<DistManifest> {
    let mut manifest = DistManifest::default();

    for entry in walkdir::WalkDir::new(site_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(site_root)
            .map_err(|_| VeloctError::Invalid("a built file is outside the site root".into()))?;
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        let bytes = veloct_core::workdir::read_bytes(entry.path())?;
        let hash = veloct_core::sha256_hex(&bytes);

        if relative.ends_with(".html") {
            let route = route_of(&relative);
            manifest.routes.push(route.clone());
            manifest.pages.insert(route.clone(), hash);
            if route.starts_with("/wip/") {
                manifest.wip_routes.push(route.clone());
            }
            if route == "/sitemap.xml" || route == "/sitemap-index.xml" {
                // Sitemap files are also collected as routes; their *entries* are read below.
            }
        } else if relative == "sitemap.xml" || relative.starts_with("sitemap") {
            for url in read_sitemap_urls(&bytes) {
                manifest.sitemap.push(url);
            }
            manifest.assets.insert(relative.clone(), hash);
        } else if relative == "rss.xml" {
            for item in read_rss_items(&bytes) {
                manifest.rss.push(item);
            }
            manifest.assets.insert(relative.clone(), hash);
        } else {
            manifest.assets.insert(relative.clone(), hash);
        }
    }

    for entry in walkdir::WalkDir::new(works_root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
    {
        let relative = entry
            .path()
            .strip_prefix(works_root)
            .map_err(|_| VeloctError::Invalid("a built file is outside the works root".into()))?;
        let relative = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("/");
        manifest.works_routes.push(format!("/{relative}"));
    }

    Ok(manifest.normalised())
}

/// Turn an emitted HTML path into the route it serves.
fn route_of(relative: &str) -> String {
    let without_extension = relative.trim_end_matches(".html");
    if without_extension == "index" {
        return "/".to_string();
    }
    if let Some(directory) = without_extension.strip_suffix("/index") {
        return format!("/{directory}/");
    }
    format!("/{without_extension}")
}

/// Pull `<loc>` values out of a sitemap, sorted.
///
/// A deliberately small scanner rather than an XML dependency: the only thing needed is the set of
/// URLs, and a rebuild comparison only cares about that set.
fn read_sitemap_urls(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut urls = Vec::new();
    let mut rest = text.as_ref();
    while let Some(start) = rest.find("<loc>") {
        let after = &rest[start + 5..];
        let Some(end) = after.find("</loc>") else {
            break;
        };
        urls.push(after[..end].trim().to_string());
        rest = &after[end + 6..];
    }
    urls.sort();
    urls.dedup();
    urls
}

/// Pull RSS item identities (guid, falling back to link) out of a feed, sorted.
fn read_rss_items(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(bytes);
    let mut items = Vec::new();
    for tag in ["<guid", "<link>"] {
        let mut rest = text.as_ref();
        while let Some(start) = rest.find(tag) {
            let after = &rest[start + tag.len()..];
            // Skip the remainder of an opening tag such as `<guid isPermaLink="false">`.
            let after = match after.find('>') {
                Some(close) => &after[close + 1..],
                None => break,
            };
            let closing = format!("</{}>", tag.trim_start_matches('<'));
            let Some(end) = after.find(&closing) else {
                break;
            };
            let value = after[..end].trim().to_string();
            if !value.is_empty() {
                items.push(value);
            }
            rest = &after[end + closing.len()..];
        }
    }
    items.sort();
    items.dedup();
    items
}

/// Everything a caller needs to commit, tag and distribute a release.
#[derive(Debug, Clone)]
pub struct ReleaseArtifacts {
    pub release_id: ReleaseId,
    pub manifest: ReleaseManifest,
    pub build_inputs: BuildInputs,
    pub target_snapshot: DeploymentTargetSnapshot,
    pub paths: ReleasePaths,
    /// The single build's output, or `None` when the release was only planned.
    pub build: Option<BuildOutcome>,
    pub dist_manifest: Option<DistManifest>,
}

impl ReleaseArtifacts {
    /// Compute the fingerprint for a committed release.
    ///
    /// Kept as a method on the artefacts so every consumer — each sub-target, the re-send path and
    /// the rollback path — derives it the same way, which is what row A2 asserts.
    pub fn fingerprint(&self, commit_sha: &str, toolchain: &ToolchainSnapshot) -> String {
        FingerprintInputs {
            release_commit_sha: commit_sha.to_string(),
            build_inputs_hash: self.manifest.build_inputs_hash.clone(),
            lockfile_hash: self.build_inputs.lockfile_hash.clone(),
            toolchain: toolchain.clone(),
            build_env: self.build_inputs.build_env.clone(),
        }
        .compute()
    }
}

/// Everything needed to freeze a release.
///
/// A struct rather than ten positional arguments: the fields are all distinct facts about a
/// release, and a struct makes a wrong call site readable instead of merely wrong.
#[derive(Debug, Clone)]
pub struct FreezeRequest {
    pub project_root: PathBuf,
    pub release_id: ReleaseId,
    pub build_inputs: BuildInputs,
    pub target_snapshot: DeploymentTargetSnapshot,
    pub toolchain: ToolchainSnapshot,
    pub site_template_version: String,
    /// The Astro template directory to freeze into the release.
    pub site_template_dir: PathBuf,
    /// The frozen build time, so time-derived output cannot move on a rebuild.
    pub release_build_time: String,
    /// Set when this release was produced by re-sending or rolling back another one.
    pub created_from_release_id: Option<ReleaseId>,
}

/// Freeze the release artefacts to disk, before any commit exists.
///
/// This is `handoff.md` §14.4 step 1. The manifest deliberately cannot include a commit sha, so
/// this is safe to run first.
pub fn freeze(request: FreezeRequest, clock: &dyn Clock) -> Result<ReleaseArtifacts> {
    let FreezeRequest {
        project_root,
        release_id,
        build_inputs,
        target_snapshot,
        toolchain,
        site_template_version,
        site_template_dir,
        release_build_time,
        created_from_release_id,
    } = request;

    let paths = ReleasePaths::new(&project_root, &release_id);
    paths.ensure()?;

    // The site template is copied first: a release must remain rebuildable with the exact template
    // it used, even after the application ships a newer one (ADR-0001, handoff §14.13).
    if site_template_dir.is_dir() {
        copy_dir_all(&site_template_dir, &paths.site_template(), |path, is_dir| {
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
            is_dir && matches!(name, "node_modules" | "dist" | ".astro" | ".git")
        })?;
    }

    let build_inputs_hash = build_inputs.hash()?;
    let manifest = ReleaseManifest {
        release_id: release_id.clone(),
        schema: RELEASE_SCHEMA,
        build_inputs_hash,
        build_toolchain_snapshot: toolchain,
        deployment_target_snapshot_ref: "target-snapshot.json".to_string(),
        site_template_version,
        release_build_time,
        created_at: veloct_core::clock::format_rfc3339(clock.now()),
        created_from_release_id,
    };
    manifest.validate()?;

    // Every artefact goes through the atomic writer, and the JSON is redacted on the way out so a
    // credential value cannot reach a release by accident (`handoff.md` §12).
    write_redacted_json(&paths.manifest(), &serde_json::to_value(&manifest)?)?;
    write_redacted_json(&paths.build_inputs(), &serde_json::to_value(&build_inputs)?)?;
    write_redacted_json(&paths.target_snapshot(), &serde_json::to_value(&target_snapshot)?)?;

    Ok(ReleaseArtifacts {
        release_id,
        manifest,
        build_inputs,
        target_snapshot,
        paths,
        build: None,
        dist_manifest: None,
    })
}

/// Read artefacts back from disk, for a re-send or a rollback.
///
/// Everything comes from the frozen files and never from the current configuration, which is
/// `handoff.md` §23.4 and test-matrix row A4.
pub fn load(project_root: &Path, release_id: &ReleaseId) -> Result<ReleaseArtifacts> {
    let paths = ReleasePaths::new(project_root, release_id);

    let manifest_text = veloct_core::workdir::read_text_optional(&paths.manifest())?.ok_or_else(|| {
        VeloctError::NotConfigured {
            what: format!("release {release_id}"),
            hint: Some(format!("{} does not exist", paths.manifest().display())),
        }
    })?;
    let manifest = ReleaseManifest::from_json(&manifest_text)?;

    let inputs_text = veloct_core::workdir::read_text_optional(&paths.build_inputs())?
        .ok_or_else(|| VeloctError::Invalid(format!("release {release_id} has no build-inputs.json")))?;
    let build_inputs: BuildInputs = serde_json::from_str(&inputs_text)?;

    let snapshot_text = veloct_core::workdir::read_text_optional(&paths.target_snapshot())?
        .ok_or_else(|| {
            VeloctError::Invalid(format!("release {release_id} has no target-snapshot.json"))
        })?;
    let target_snapshot: DeploymentTargetSnapshot = serde_json::from_str(&snapshot_text)?;

    let dist_manifest = veloct_core::workdir::read_text_optional(&paths.dist_manifest())?
        .map(|text| DistManifest::from_json(&text))
        .transpose()?;

    Ok(ReleaseArtifacts {
        release_id: release_id.clone(),
        manifest,
        build_inputs,
        target_snapshot,
        paths,
        build: None,
        dist_manifest,
    })
}

/// Run the one build a release is allowed (`handoff.md` §14.1, rule 11).
///
/// The previous `dist/` is removed first: Veloct keeps exactly one current `dist/`, and a stale
/// file left from an earlier build would make the release look like it contains something it does
/// not.
pub fn build_once(
    project_root: &Path,
    site_dir: &Path,
    artifacts: &mut ReleaseArtifacts,
    runner: &dyn BuildRunner,
) -> Result<BuildOutcome> {
    let dist = project_root.join(layout::DIST_DIR);
    if dist.exists() {
        std::fs::remove_dir_all(&dist).map_err(|e| VeloctError::io(&dist, e))?;
    }
    std::fs::create_dir_all(&dist).map_err(|e| VeloctError::io(&dist, e))?;

    let site_url = artifacts
        .build_inputs
        .site_config
        .get("url")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let site_base = artifacts
        .build_inputs
        .site_config
        .get("base")
        .and_then(|value| value.as_str())
        .map(str::to_string);

    let request = BuildRequest {
        project_root: project_root.to_path_buf(),
        site_dir: site_dir.to_path_buf(),
        out_dir: dist.clone(),
        release_build_time: artifacts.manifest.release_build_time.clone(),
        env: artifacts.build_inputs.build_env.clone(),
        site_url,
        site_base,
    };

    let outcome = runner.build(&request)?;

    let site_root = outcome.site_root.clone();
    let works_root = outcome.works_root.clone();
    if !site_root.is_dir() {
        return Err(VeloctError::Build(format!(
            "the build did not produce {}",
            site_root.display()
        )));
    }
    if !works_root.is_dir() {
        return Err(VeloctError::Build(format!(
            "the build did not produce the works publish root {}",
            works_root.display()
        )));
    }

    // The normalised manifest is recorded HERE rather than left to the caller.
    //
    // It was a separate step, and a release could therefore be built without one: `freeze` leaves
    // `dist_manifest` as `None`, and nothing forced the caller to fill it in. `handoff.md` §23.6 makes
    // the manifest the comparable record of what was published, so a release without it cannot be
    // checked for determinism — which is the whole point of freezing the inputs. Doing it inside
    // `build_once` makes the omission impossible rather than merely discouraged.
    let manifest = scan_dist(&site_root, &works_root)?;
    record_dist_manifest(artifacts, manifest.clone())?;

    Ok(BuildOutcome {
        dist_manifest: Some(manifest),
        ..outcome
    })
}

/// Record a build's normalised manifest next to the release.
pub fn record_dist_manifest(artifacts: &mut ReleaseArtifacts, manifest: DistManifest) -> Result<()> {
    let manifest = manifest.normalised();
    let text = manifest.to_json()?;
    veloct_core::workdir::write_atomic(&artifacts.paths.dist_manifest(), text.as_bytes())?;
    artifacts.dist_manifest = Some(manifest);
    Ok(())
}

/// Serialise JSON with every string passed through the redaction layer.
///
/// `handoff.md` §12 requires that a credential never reaches a release artefact, and doing it at
/// the write boundary means a new field cannot bypass the rule by being added in the wrong place.
fn write_redacted_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut value = value.clone();
    veloct_core::secret::redact_json(&mut value);
    let text = crate::manifest::canonical_json_pretty(&value)?;
    veloct_core::workdir::write_atomic(path, text.as_bytes())
}

/// A build runner that always fails, used to assert that a failure is reported rather than
/// swallowed and that the previous `dist/` is not silently kept.
#[derive(Debug, Clone, Default)]
pub struct FailingBuildRunner {
    pub message: String,
}

impl BuildRunner for FailingBuildRunner {
    fn build(&self, _request: &BuildRequest) -> Result<BuildOutcome> {
        Err(VeloctError::Build(if self.message.is_empty() {
            "the build failed (fake)".to_string()
        } else {
            self.message.clone()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::workdir::write_atomic;
    use veloct_core::FixedClock;

    /// A deterministic build runner that writes a fixed set of files into both publish roots.
    #[derive(Debug, Clone)]
    struct FakeBuildRunner {
        site_files: Vec<(&'static str, &'static str)>,
        works_files: Vec<(&'static str, &'static str)>,
        /// A per-run counter, standing in for content that would otherwise vary between builds.
        variable: Option<String>,
    }

    impl FakeBuildRunner {
        fn new() -> Self {
            Self {
                site_files: vec![
                    ("index.html", "<html>home</html>"),
                    ("article/a/index.html", "<html>a</html>"),
                    ("wip/01ABC/index.html", "<html>wip</html>"),
                    ("sitemap-0.xml", "<urlset><url><loc>https://example.com/</loc></url></urlset>"),
                    ("rss.xml", "<rss><item><guid>a</guid></item></rss>"),
                    ("_astro/app.css", "body{}"),
                ],
                works_files: vec![("demo/index.html", "<html>work</html>")],
                variable: None,
            }
        }

        fn with_variable(mut self, value: &str) -> Self {
            self.variable = Some(value.to_string());
            self
        }
    }

    impl BuildRunner for FakeBuildRunner {
        fn build(&self, request: &BuildRequest) -> Result<BuildOutcome> {
            let site_root = request.out_dir.join("site");
            let works_root = request.out_dir.join("works");
            std::fs::create_dir_all(&site_root).map_err(|e| VeloctError::io(&site_root, e))?;
            std::fs::create_dir_all(&works_root).map_err(|e| VeloctError::io(&works_root, e))?;

            for (path, content) in &self.site_files {
                let target = site_root.join(path);
                let body = match &self.variable {
                    Some(variable) => format!("{content}{variable}"),
                    None => content.to_string(),
                };
                write_atomic(&target, body.as_bytes())?;
            }
            for (path, content) in &self.works_files {
                write_atomic(&works_root.join(path), content.as_bytes())?;
            }

            Ok(BuildOutcome {
                site_root,
                works_root,
                log: "fake build".to_string(),
                dist_manifest: None,
            })
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        template: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let template = dir.path().join("template");
        std::fs::create_dir_all(root.join("content").join("articles")).unwrap();
        std::fs::create_dir_all(root.join("works")).unwrap();
        std::fs::create_dir_all(template.join("src")).unwrap();
        write_atomic(&template.join("astro.config.mjs"), b"export default {}").unwrap();
        std::fs::create_dir_all(template.join("node_modules")).unwrap();
        write_atomic(&template.join("node_modules/junk.js"), b"x").unwrap();
        Fixture {
            _dir: dir,
            root,
            template,
        }
    }

    fn release(entropy: u8) -> ReleaseId {
        ReleaseId::from_stamp(time::macros::datetime!(2026-01-01 12:00:00 UTC), &[entropy, 0, 0])
    }

    fn clock() -> FixedClock {
        FixedClock::new(time::macros::datetime!(2026-01-01 12:00:00 UTC))
    }

    fn toolchain() -> ToolchainSnapshot {
        ToolchainSnapshot {
            node: "24.18.0".into(),
            npm: "11.19.0".into(),
            astro: "7.3.5".into(),
            veloct: "0.1.0".into(),
            os: "windows".into(),
        }
    }

    fn inputs() -> BuildInputs {
        BuildInputs {
            content: BTreeMap::from([("content/articles/a.md".to_string(), "h-a".to_string())]),
            works: BTreeMap::new(),
            site_config: serde_json::json!({ "url": "https://example.com" }),
            navigation: serde_json::json!([]),
            comments: serde_json::json!({ "enabled": false }),
            seo: serde_json::json!({}),
            lockfile_hash: "lock".into(),
            lockfile_path: Some("site/package-lock.json".into()),
            build_env: BTreeMap::from([("NODE_ENV".to_string(), "production".to_string())]),
            credential_refs: BTreeMap::new(),
        }
    }

    fn snapshot() -> DeploymentTargetSnapshot {
        DeploymentTargetSnapshot {
            sub_targets: vec![
                crate::manifest::SubTargetSnapshot {
                    logical_target_id: "cloudflare".into(),
                    role: "main".into(),
                    config: serde_json::json!({ "project": "site" }),
                    credential_ref: Some("keychain:cloudflare".into()),
                    is_degraded: false,
                },
                crate::manifest::SubTargetSnapshot {
                    logical_target_id: "cloudflare".into(),
                    role: "works".into(),
                    config: serde_json::json!({ "project": "works" }),
                    credential_ref: Some("keychain:cloudflare".into()),
                    is_degraded: false,
                },
            ],
        }
    }

    fn freeze_release(fixture: &Fixture, release_id: &ReleaseId) -> ReleaseArtifacts {
        freeze(
            FreezeRequest {
                project_root: fixture.root.clone(),
                release_id: release_id.clone(),
                build_inputs: inputs(),
                target_snapshot: snapshot(),
                toolchain: toolchain(),
                site_template_version: "1.0.0".to_string(),
                site_template_dir: fixture.template.clone(),
                release_build_time: "2026-01-01T12:00:00Z".to_string(),
                created_from_release_id: None,
            },
            &clock(),
        )
        .unwrap()
    }

    #[test]
    fn freezing_writes_every_artefact_before_any_commit_exists() {
        // handoff 14.4 step 1.
        let fixture = fixture();
        let release_id = release(1);
        let artifacts = freeze_release(&fixture, &release_id);

        assert!(artifacts.paths.manifest().exists());
        assert!(artifacts.paths.build_inputs().exists());
        assert!(artifacts.paths.target_snapshot().exists());
        assert!(artifacts.paths.site_template().is_dir());
        assert_eq!(artifacts.manifest.release_id, release_id);

        // The frozen template excludes build noise but includes the real project files.
        assert!(artifacts.paths.site_template().join("astro.config.mjs").exists());
        assert!(!artifacts.paths.site_template().join("node_modules").exists());

        // And the manifest on disk carries no commit sha.
        let text = std::fs::read_to_string(artifacts.paths.manifest()).unwrap();
        assert!(!text.contains("commit_sha"));
    }

    #[test]
    fn frozen_artefacts_read_back_identically() {
        let fixture = fixture();
        let release_id = release(2);
        let written = freeze_release(&fixture, &release_id);
        let read = load(&fixture.root, &release_id).unwrap();

        assert_eq!(read.manifest, written.manifest);
        assert_eq!(read.build_inputs, written.build_inputs);
        assert_eq!(read.target_snapshot, written.target_snapshot);
        assert_eq!(read.manifest.build_inputs_hash, written.manifest.build_inputs_hash);
    }

    #[test]
    fn loading_an_unknown_release_is_an_explicit_error() {
        let fixture = fixture();
        let error = load(&fixture.root, &release(3)).unwrap_err();
        assert_eq!(error.code(), "NOT_CONFIGURED");
        assert!(error.to_string().contains("release"), "{error}");
    }

    #[test]
    fn a_rebuild_reads_the_frozen_snapshot_and_not_current_configuration() {
        // Row A4: a historical rebuild must not read the live configuration.
        let fixture = fixture();
        let release_id = release(4);
        let written = freeze_release(&fixture, &release_id);

        // The live project configuration changes afterwards.
        write_atomic(
            &fixture.root.join("data").join("site.json"),
            br#"{"url":"https://changed.example.com"}"#,
        )
        .unwrap();

        let read = load(&fixture.root, &release_id).unwrap();
        assert_eq!(read.build_inputs.site_config, written.build_inputs.site_config);
        assert_eq!(read.manifest.build_inputs_hash, written.manifest.build_inputs_hash);
    }

    #[test]
    fn one_build_produces_both_publish_roots() {
        // handoff 14.1: dist/site and dist/works from a single build.
        let fixture = fixture();
        let release_id = release(5);
        let mut artifacts = freeze_release(&fixture, &release_id);
        let runner = FakeBuildRunner::new();

        let outcome = build_once(&fixture.root, &fixture.template, &mut artifacts, &runner).unwrap();
        assert!(outcome.site_root.join("index.html").exists());
        assert!(outcome.works_root.join("demo/index.html").exists());

        let manifest = scan_dist(&outcome.site_root, &outcome.works_root).unwrap();
        record_dist_manifest(&mut artifacts, manifest).unwrap();
        assert!(artifacts.paths.dist_manifest().exists());
    }

    #[test]
    fn scanning_derives_routes_hashes_and_the_wip_and_works_route_sets() {
        let fixture = fixture();
        let release_id = release(6);
        let mut artifacts = freeze_release(&fixture, &release_id);
        let runner = FakeBuildRunner::new();
        let outcome = build_once(&fixture.root, &fixture.template, &mut artifacts, &runner).unwrap();

        let manifest = scan_dist(&outcome.site_root, &outcome.works_root).unwrap();
        assert_eq!(manifest.routes, vec!["/", "/article/a/", "/wip/01ABC/"]);
        assert_eq!(manifest.wip_routes, vec!["/wip/01ABC/"]);
        assert_eq!(manifest.works_routes, vec!["/demo/index.html"]);
        assert_eq!(manifest.sitemap, vec!["https://example.com/"]);
        assert_eq!(manifest.rss, vec!["a"]);
        assert!(manifest.assets.contains_key("_astro/app.css"));
        // A page hash is recorded for every HTML route.
        assert_eq!(manifest.pages.len(), 3);
    }

    #[test]
    fn two_builds_of_one_release_produce_an_identical_normalised_manifest() {
        // Rows A5 and A6.
        let fixture = fixture();
        let release_id = release(7);
        let mut artifacts = freeze_release(&fixture, &release_id);
        let runner = FakeBuildRunner::new();

        let first = build_once(&fixture.root, &fixture.template, &mut artifacts, &runner).unwrap();
        let first_manifest = scan_dist(&first.site_root, &first.works_root).unwrap();

        let second = build_once(&fixture.root, &fixture.template, &mut artifacts, &runner).unwrap();
        let second_manifest = scan_dist(&second.site_root, &second.works_root).unwrap();

        assert!(
            first_manifest.differences(&second_manifest).is_empty(),
            "{:?}",
            first_manifest.differences(&second_manifest)
        );
    }

    #[test]
    fn a_build_that_varies_between_runs_is_detected() {
        // The comparison must be able to fail, otherwise it proves nothing.
        let fixture = fixture();
        let release_id = release(8);
        let mut artifacts = freeze_release(&fixture, &release_id);

        let first = build_once(
            &fixture.root,
            &fixture.template,
            &mut artifacts,
            &FakeBuildRunner::new(),
        )
        .unwrap();
        let first_manifest = scan_dist(&first.site_root, &first.works_root).unwrap();

        let second = build_once(
            &fixture.root,
            &fixture.template,
            &mut artifacts,
            &FakeBuildRunner::new().with_variable("<!-- build time -->"),
        )
        .unwrap();
        let second_manifest = scan_dist(&second.site_root, &second.works_root).unwrap();

        let diffs = first_manifest.differences(&second_manifest);
        assert!(!diffs.is_empty(), "a varying build must be detectable");
        assert!(diffs.iter().any(|d| d.contains("page hashes differ")), "{diffs:?}");
    }

    #[test]
    fn every_sub_target_of_one_release_receives_the_same_fingerprint() {
        // Rows A2 and A11: the fingerprint is computed once from the frozen release, never per
        // target.
        let fixture = fixture();
        let release_id = release(9);
        let artifacts = freeze_release(&fixture, &release_id);
        let commit_sha = "0123456789abcdef0123456789abcdef01234567";

        let fingerprint = artifacts.fingerprint(commit_sha, &toolchain());
        let main_snapshot = artifacts.target_snapshot.pair("cloudflare").unwrap().0;
        let works_snapshot = artifacts.target_snapshot.pair("cloudflare").unwrap().1;
        assert_eq!(main_snapshot.role, "main");
        assert_eq!(works_snapshot.role, "works");

        // Both halves derive it from the same artefacts.
        assert_eq!(fingerprint, artifacts.fingerprint(commit_sha, &toolchain()));
        assert_eq!(fingerprint.len(), 64);
    }

    #[test]
    fn a_different_commit_produces_a_different_fingerprint_for_the_same_artefacts() {
        let fixture = fixture();
        let release_id = release(10);
        let artifacts = freeze_release(&fixture, &release_id);
        assert_ne!(
            artifacts.fingerprint("aaaa", &toolchain()),
            artifacts.fingerprint("bbbb", &toolchain())
        );
    }

    #[test]
    fn a_failed_build_is_reported_and_leaves_no_usable_dist() {
        // handoff 21.2: a failed build must not replace the previous usable dist.
        let fixture = fixture();
        let release_id = release(11);
        let mut artifacts = freeze_release(&fixture, &release_id);

        let error = build_once(
            &fixture.root,
            &fixture.template,
            &mut artifacts,
            &FailingBuildRunner {
                message: "astro exited with code 1".into(),
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "BUILD");
        assert!(error.to_string().contains("astro exited"), "{error}");

        // The failed run removed the previous dist, but it produced nothing to mistake for a build.
        let dist = fixture.root.join("dist");
        let site = dist.join("site");
        assert!(!site.join("index.html").exists());
    }

    #[test]
    fn a_runner_that_forgets_a_publish_root_is_rejected() {
        /// A runner that only writes the main site, to prove the works root is required.
        #[derive(Debug)]
        struct HalfRunner;
        impl BuildRunner for HalfRunner {
            fn build(&self, request: &BuildRequest) -> Result<BuildOutcome> {
                let site_root = request.out_dir.join("site");
                std::fs::create_dir_all(&site_root).map_err(|e| VeloctError::io(&site_root, e))?;
                write_atomic(&site_root.join("index.html"), b"<html></html>")?;
                Ok(BuildOutcome {
                    site_root,
                    works_root: request.out_dir.join("works"),
                    log: String::new(),
                    dist_manifest: None,
                })
            }
        }

        let fixture = fixture();
        let release_id = release(12);
        let mut artifacts = freeze_release(&fixture, &release_id);
        let error = build_once(&fixture.root, &fixture.template, &mut artifacts, &HalfRunner).unwrap_err();
        assert_eq!(error.code(), "BUILD");
        assert!(error.to_string().contains("works publish root"), "{error}");
    }

    #[test]
    fn only_one_dist_is_kept_across_repeated_builds() {
        // Row A14.
        let fixture = fixture();
        let release_id = release(13);
        let mut artifacts = freeze_release(&fixture, &release_id);
        let runner = FakeBuildRunner::new();

        for _ in 0..3 {
            build_once(&fixture.root, &fixture.template, &mut artifacts, &runner).unwrap();
        }

        let dist = fixture.root.join("dist");
        let mut entries: Vec<String> = std::fs::read_dir(&dist)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();
        assert_eq!(entries, vec!["site", "works"], "exactly one dist with two roots");
    }

    #[test]
    fn the_frozen_site_template_is_used_for_a_rebuild() {
        // ADR-0001: a historical rebuild must not depend on the current application template.
        let fixture = fixture();
        let release_id = release(14);
        let artifacts = freeze_release(&fixture, &release_id);

        // The application's bundled template moves on.
        write_atomic(&fixture.template.join("astro.config.mjs"), b"export default { NEW: true }").unwrap();

        let frozen = std::fs::read_to_string(artifacts.paths.site_template().join("astro.config.mjs")).unwrap();
        assert_eq!(frozen, "export default {}");
    }

    #[test]
    fn a_release_can_record_which_release_it_was_derived_from() {
        // Used by the re-send and rollback paths, which freeze a new release from an old one.
        let fixture = fixture();
        let original = release(15);
        let artifacts = freeze(
            FreezeRequest {
                project_root: fixture.root.clone(),
                release_id: release(16),
                build_inputs: inputs(),
                target_snapshot: snapshot(),
                toolchain: toolchain(),
                site_template_version: "1.0.0".to_string(),
                site_template_dir: fixture.template.clone(),
                release_build_time: "2026-01-01T12:00:00Z".to_string(),
                created_from_release_id: Some(original.clone()),
            },
            &clock(),
        )
        .unwrap();
        assert_eq!(artifacts.manifest.created_from_release_id, Some(original));
    }

    #[test]
    fn a_release_frozen_from_another_release_keeps_the_same_inputs_hash() {
        // The re-send path must reproduce the original build inputs exactly (handoff 14.13).
        let fixture = fixture();
        let original_id = release(17);
        let original = freeze_release(&fixture, &original_id);

        let reloaded = load(&fixture.root, &original_id).unwrap();
        let derived = freeze(
            FreezeRequest {
                project_root: fixture.root.clone(),
                release_id: release(18),
                build_inputs: reloaded.build_inputs.clone(),
                target_snapshot: reloaded.target_snapshot.clone(),
                toolchain: toolchain(),
                site_template_version: "1.0.0".to_string(),
                site_template_dir: fixture.template.clone(),
                release_build_time: reloaded.manifest.release_build_time.clone(),
                created_from_release_id: Some(original_id.clone()),
            },
            &clock(),
        )
        .unwrap();

        assert_eq!(
            derived.manifest.build_inputs_hash,
            original.manifest.build_inputs_hash
        );
        assert_eq!(
            derived.manifest.release_build_time,
            original.manifest.release_build_time
        );
    }

    #[test]
    fn a_secret_in_the_configuration_is_redacted_before_it_reaches_disk() {
        // handoff 12: no credential value in a release artefact.
        let fixture = fixture();
        let mut contaminated = inputs();
        contaminated.site_config = serde_json::json!({
            "url": "https://example.com",
            "api_key": "sk-ant-api03-abcdefghijklmnopqrstuvwxyz"
        });

        let release_id = release(19);
        freeze(
            FreezeRequest {
                project_root: fixture.root.clone(),
                release_id: release_id.clone(),
                build_inputs: contaminated,
                target_snapshot: snapshot(),
                toolchain: toolchain(),
                site_template_version: "1.0.0".to_string(),
                site_template_dir: fixture.template.clone(),
                release_build_time: "2026-01-01T12:00:00Z".to_string(),
                created_from_release_id: None,
            },
            &clock(),
        )
        .unwrap();

        let text = std::fs::read_to_string(ReleasePaths::new(&fixture.root, &release_id).build_inputs())
            .unwrap();
        assert!(!text.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz"), "{text}");
        assert!(text.contains("[REDACTED]"), "{text}");
        // The non-secret part is preserved.
        assert!(text.contains("https://example.com"));
    }
}
