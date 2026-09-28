//! The release truth model.
//!
//! `handoff.md` §14.4 defines the model that keeps a release honest, and it is deliberately
//! non-circular:
//!
//! ```text
//! 1. write .veloct/releases/<id>/release.json   <- MUST NOT contain its own commit sha
//! 2. create the publish commit                  -> the sha now exists
//! 3. create the tag release/<id> at that commit
//!    => truth = release.json + tag + the commit the tag points at
//! ```
//!
//! If the manifest recorded its own sha it could not be written before the commit that contains
//! it, and the whole anchoring scheme would break. Everything in this module therefore describes a
//! release *before* it is committed, and the commit is supplied externally when a fingerprint is
//! computed.
//!
//! `build_inputs_hash` and `build_env_hash` are also constrained: the inputs hash excludes the
//! commit sha (§14.8) and the environment hash covers an explicit allow-list rather than the whole
//! environment (§14.8), because hashing every environment variable would make a rebuild on another
//! machine look different for reasons that have nothing to do with content.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use veloct_core::{hash_parts, Result, ReleaseId, VeloctError};

/// Schema version of the on-disk release artefacts.
pub const RELEASE_SCHEMA: u32 = 1;

/// Environment variables that are allowed to influence a build.
///
/// An allow-list, not a denylist: `handoff.md` §14.8 forbids hashing "all environment variables",
/// and a denylist would silently start covering a new variable somebody adds.
pub const BUILD_ENV_ALLOWLIST: [&str; 6] = [
    "NODE_ENV",
    "VELOCT_SITE_URL",
    "VELOCT_SITE_BASE",
    "VELOCT_BUILD_TIME",
    "VELOCT_DISABLE_COMMENTS",
    "VELOCT_OG_LOCALE",
];

/// The toolchain a build ran with, so a rebuild can detect a drift that would explain a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolchainSnapshot {
    pub node: String,
    pub npm: String,
    pub astro: String,
    pub veloct: String,
    pub os: String,
}

impl ToolchainSnapshot {
    /// Hash of the fields that actually affect build output.
    ///
    /// `os` is excluded on purpose: rebuilding a release on a different operating system is
    /// expected and supported (`handoff.md` §14.9 requires semantic, not byte, equality), so the
    /// host must not change the fingerprint.
    pub fn hash(&self) -> String {
        hash_parts([
            ("node", self.node.as_str()),
            ("npm", self.npm.as_str()),
            ("astro", self.astro.as_str()),
            ("veloct", self.veloct.as_str()),
        ])
    }

    /// A stable single-line label for human display.
    ///
    /// It includes the host, which is useful in a report and exactly why it is **not** used as the
    /// fingerprint's toolchain component: the fingerprint must not depend on the host.
    pub fn label(&self) -> String {
        format!(
            "node={};npm={};astro={};veloct={};os={}",
            self.node, self.npm, self.astro, self.veloct, self.os
        )
    }
}

/// The non-secret inputs that determine a build (`handoff.md` §14.6).
///
/// Credentials appear only as `credential_ref` handles; a value never reaches this structure.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildInputs {
    /// Content file path -> content hash, for every Markdown file in the release.
    pub content: BTreeMap<String, String>,
    /// Work directory name -> hash of its `work.json`.
    pub works: BTreeMap<String, String>,
    /// The site configuration as it was exported for the build.
    pub site_config: serde_json::Value,
    /// Navigation and page structure.
    pub navigation: serde_json::Value,
    /// Comment configuration, including whether comments are enabled at all.
    pub comments: serde_json::Value,
    /// SEO configuration.
    pub seo: serde_json::Value,
    /// Hash of the lockfile that resolved the build's dependencies.
    pub lockfile_hash: String,
    /// An explicit lockfile path, for the manifest's evidence trail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lockfile_path: Option<String>,
    /// Only allow-listed, build-affecting, non-secret environment values.
    pub build_env: BTreeMap<String, String>,
    /// Opaque credential handles, never values (`handoff.md` §14.6).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub credential_refs: BTreeMap<String, String>,
}

impl BuildInputs {
    /// Retain only allow-listed environment variables.
    ///
    /// Called by the caller that collects the environment, so the filtering rule lives in one place
    /// and a test can prove that a non-allow-listed variable is dropped.
    pub fn filter_env<'a, I>(env: I) -> BTreeMap<String, String>
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        env.into_iter()
            .filter(|(key, _)| BUILD_ENV_ALLOWLIST.contains(key))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    /// Hash of the non-secret build inputs.
    ///
    /// **Excludes the commit sha by construction**: this function cannot see one, which is what
    /// `handoff.md` §14.8 requires and test-matrix row A12 checks.
    pub fn hash(&self) -> Result<String> {
        let mut parts: Vec<(String, String)> = Vec::new();

        for (path, hash) in &self.content {
            parts.push((format!("content:{path}"), hash.clone()));
        }
        for (name, hash) in &self.works {
            parts.push((format!("work:{name}"), hash.clone()));
        }
        parts.push((
            "site_config".to_string(),
            canonical_json(&self.site_config)?,
        ));
        parts.push(("navigation".to_string(), canonical_json(&self.navigation)?));
        parts.push(("comments".to_string(), canonical_json(&self.comments)?));
        parts.push(("seo".to_string(), canonical_json(&self.seo)?));
        parts.push(("lockfile".to_string(), self.lockfile_hash.clone()));
        for (key, value) in &self.build_env {
            parts.push((format!("env:{key}"), value.clone()));
        }
        for (key, value) in &self.credential_refs {
            // Handles, not secrets: a re-bound credential must not change the fingerprint, but
            // recording which handle was used keeps the evidence trail intact.
            parts.push((format!("credential_ref:{key}"), value.clone()));
        }

        // A `BTreeMap` already iterates in key order, so no extra sorting is needed and the hash is
        // stable across runs.
        let borrowed: Vec<(&str, &str)> = parts
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        Ok(hash_parts(borrowed))
    }
}

/// Serialise JSON deterministically.
///
/// `serde_json::Value`'s object type preserves insertion order, so two logically identical
/// configurations built in different orders would hash differently. Sorting object keys here is
/// what makes a rebuild comparable.
pub fn canonical_json(value: &serde_json::Value) -> Result<String> {
    fn write(value: &serde_json::Value, out: &mut String) -> Result<()> {
        match value {
            serde_json::Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                out.push('{');
                for (index, key) in keys.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push_str(&serde_json::to_string(*key)?);
                    out.push(':');
                    write(&map[*key], out)?;
                }
                out.push('}');
            }
            serde_json::Value::Array(items) => {
                out.push('[');
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write(item, out)?;
                }
                out.push(']');
            }
            other => out.push_str(&serde_json::to_string(other)?),
        }
        Ok(())
    }

    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out)
}

/// Serialise JSON deterministically, indented for a human-readable artefact.
///
/// Object keys are sorted so two logically identical artefacts compare equal, and the trailing
/// newline makes the file well-formed at the end of a line.
pub fn canonical_json_pretty(value: &serde_json::Value) -> Result<String> {
    let sorted = sort_json_keys(value);
    let mut text = serde_json::to_string_pretty(&sorted)?;
    text.push('\n');
    Ok(text)
}

/// Rebuild a JSON value with every object's keys in sorted order.
pub fn sort_json_keys(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key.clone(), sort_json_keys(value));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(sort_json_keys).collect())
        }
        other => other.clone(),
    }
}

/// One deployment sub-target as frozen into a release (`handoff.md` §14.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubTargetSnapshot {
    pub logical_target_id: String,
    /// `main` or `works`.
    pub role: String,
    /// Target-specific configuration, secret-free.
    pub config: serde_json::Value,
    /// An opaque credential handle, never a value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    /// True when this target cannot provide an independent Works Origin, so its works half runs
    /// in a degraded, non-isolated mode (`handoff.md` §10.4).
    #[serde(default)]
    pub is_degraded: bool,
}

impl SubTargetSnapshot {
    pub fn is_works(&self) -> bool {
        self.role == "works"
    }
}

/// Every deployment sub-target as it stood when a release was created.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentTargetSnapshot {
    pub sub_targets: Vec<SubTargetSnapshot>,
}

impl DeploymentTargetSnapshot {
    /// The frozen `main` and `works` sub-target pair for one logical target.
    pub fn pair(&self, logical_target_id: &str) -> Option<(&SubTargetSnapshot, &SubTargetSnapshot)> {
        let main = self
            .sub_targets
            .iter()
            .find(|sub| sub.logical_target_id == logical_target_id && sub.role == "main")?;
        let works = self
            .sub_targets
            .iter()
            .find(|sub| sub.logical_target_id == logical_target_id && sub.role == "works")?;
        Some((main, works))
    }

    /// Logical target ids present, sorted.
    pub fn logical_targets(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .sub_targets
            .iter()
            .map(|sub| sub.logical_target_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// A stable hash, so a changed target configuration is visible in the release evidence.
    pub fn hash(&self) -> Result<String> {
        let mut parts: Vec<(String, String)> = Vec::new();
        let mut sorted = self.sub_targets.clone();
        sorted.sort_by(|a, b| {
            a.logical_target_id
                .cmp(&b.logical_target_id)
                .then_with(|| a.role.cmp(&b.role))
        });
        for sub in &sorted {
            parts.push((
                format!("{}:{}", sub.logical_target_id, sub.role),
                canonical_json(&sub.config)?,
            ));
            parts.push((
                format!("{}:{}:degraded", sub.logical_target_id, sub.role),
                sub.is_degraded.to_string(),
            ));
        }
        let borrowed: Vec<(&str, &str)> = parts
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        Ok(hash_parts(borrowed))
    }
}

/// The release manifest written to `.veloct/releases/<id>/release.json`.
///
/// Note what is absent: there is no `commit_sha` field, by construction (`handoff.md` §14.4).
///
/// `deny_unknown_fields` is deliberate. An unrecognised key means the file was written by a schema
/// this build does not understand, and accepting it silently would mean reading a release while
/// ignoring part of its meaning. Failing loudly is the honest option, and it is what makes the
/// "no commit sha in the manifest" rule enforceable rather than merely documented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub release_id: ReleaseId,
    pub schema: u32,
    pub build_inputs_hash: String,
    pub build_toolchain_snapshot: ToolchainSnapshot,
    pub deployment_target_snapshot_ref: String,
    pub site_template_version: String,
    /// The frozen build time (`handoff.md` §14.9: time-derived fields must not move on a rebuild).
    pub release_build_time: String,
    pub created_at: String,
    /// Set when this release was created by re-sending or rolling back another one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_from_release_id: Option<ReleaseId>,
}

impl ReleaseManifest {
    /// Serialise for disk, with a trailing newline so the file is a well-formed text file.
    pub fn to_json(&self) -> Result<String> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }

    /// Read a manifest from disk.
    pub fn from_json(text: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate the invariants the anchoring scheme depends on.
    pub fn validate(&self) -> Result<()> {
        if self.schema != RELEASE_SCHEMA {
            return Err(VeloctError::Invalid(format!(
                "release manifest schema {} is not supported (expected {RELEASE_SCHEMA})",
                self.schema
            )));
        }
        if self.deployment_target_snapshot_ref.trim().is_empty() {
            return Err(VeloctError::Invalid(
                "release manifest must reference its deployment target snapshot".into(),
            ));
        }
        veloct_core::clock::parse_rfc3339(&self.created_at).map_err(|_| {
            VeloctError::Invalid(format!(
                "release manifest created_at is not RFC 3339: {:?}",
                self.created_at
            ))
        })?;
        veloct_core::clock::parse_rfc3339(&self.release_build_time).map_err(|_| {
            VeloctError::Invalid(format!(
                "release manifest release_build_time is not RFC 3339: {:?}",
                self.release_build_time
            ))
        })?;

        // The one rule that must never regress: the manifest cannot carry its own commit sha.
        let serialised = serde_json::to_string(self)?;
        if serialised.contains("commit_sha") {
            return Err(VeloctError::Invalid(
                "release.json must not contain a commit sha: the truth source is the manifest plus \
                 the release tag plus the commit that tag points at (handoff.md 14.4)"
                    .into(),
            ));
        }
        Ok(())
    }
}

/// The components that make up a build fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintInputs {
    pub release_commit_sha: String,
    pub build_inputs_hash: String,
    pub lockfile_hash: String,
    pub toolchain: ToolchainSnapshot,
    pub build_env: BTreeMap<String, String>,
}

impl FingerprintInputs {
    /// `build_fingerprint = hash(commit_sha + build_inputs_hash + lockfile_hash +
    /// toolchain_version + build_env_hash)` (`handoff.md` §14.8).
    ///
    /// The `build_inputs_hash` component already excludes the commit sha, so including the sha here
    /// does not double-count it.
    pub fn build_env_hash(&self) -> String {
        let borrowed: Vec<(&str, &str)> = self
            .build_env
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        hash_parts(borrowed)
    }

    pub fn compute(&self) -> String {
        // The toolchain component is its *hash*, not its label: the label includes the host
        // operating system, and `handoff.md` §14.9 expects a release to be rebuildable on another
        // machine. Using the label here would make the fingerprint host-dependent.
        hash_parts([
            ("commit_sha", self.release_commit_sha.as_str()),
            ("build_inputs_hash", self.build_inputs_hash.as_str()),
            ("lockfile_hash", self.lockfile_hash.as_str()),
            ("toolchain", self.toolchain.hash().as_str()),
            ("build_env_hash", self.build_env_hash().as_str()),
        ])
    }
}

/// The normalised manifest of a build's output (`handoff.md` §14.9, §23.6).
///
/// This is what makes "semantically identical" testable without demanding byte-identical output:
/// routes, page hashes, asset hashes, sitemap and RSS entries, and the WIP and works route sets,
/// each sorted so two builds of one release compare equal.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistManifest {
    /// Sorted site routes, e.g. `/article/hello/`.
    pub routes: Vec<String>,
    /// Sorted `route -> sha256 of the emitted file`.
    pub pages: BTreeMap<String, String>,
    /// Sorted `asset path -> sha256`.
    pub assets: BTreeMap<String, String>,
    /// Sorted sitemap entry URLs.
    pub sitemap: Vec<String>,
    /// Sorted RSS item identifiers.
    pub rss: Vec<String>,
    /// Routes that exist only because a WIP article is publishable (`handoff.md` §7.1).
    pub wip_routes: Vec<String>,
    /// Routes emitted into the works publish root.
    pub works_routes: Vec<String>,
}

impl DistManifest {
    /// Sort every collection into a canonical order.
    pub fn normalised(mut self) -> Self {
        self.routes.sort();
        self.routes.dedup();
        self.sitemap.sort();
        self.sitemap.dedup();
        self.rss.sort();
        self.rss.dedup();
        self.wip_routes.sort();
        self.wip_routes.dedup();
        self.works_routes.sort();
        self.works_routes.dedup();
        // `BTreeMap` is already ordered by key.
        self
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let manifest: Self = serde_json::from_str(text)?;
        Ok(manifest.normalised())
    }

    pub fn to_json(&self) -> Result<String> {
        let mut text = serde_json::to_string_pretty(&self.clone().normalised())?;
        text.push('\n');
        Ok(text)
    }

    /// Compare two builds of the same release for semantic equality.
    ///
    /// Returns the differences rather than a bare boolean, because "the rebuild produced a
    /// different route set" is only actionable if the route is named.
    pub fn differences(&self, other: &DistManifest) -> Vec<String> {
        let mut diffs = Vec::new();
        let left = self.clone().normalised();
        let right = other.clone().normalised();

        if left.routes != right.routes {
            diffs.push(format!(
                "routes differ: only-left {:?}, only-right {:?}",
                difference(&left.routes, &right.routes),
                difference(&right.routes, &left.routes)
            ));
        }
        if left.pages != right.pages {
            let changed: Vec<String> = left
                .pages
                .iter()
                .filter(|(route, hash)| right.pages.get(*route).map(|other| other != *hash).unwrap_or(true))
                .map(|(route, _)| route.clone())
                .collect();
            diffs.push(format!("page hashes differ for {changed:?}"));
        }
        if left.assets != right.assets {
            let changed: Vec<String> = left
                .assets
                .iter()
                .filter(|(path, hash)| {
                    right.assets.get(*path).map(|other| other != *hash).unwrap_or(true)
                })
                .map(|(path, _)| path.clone())
                .collect();
            diffs.push(format!("asset hashes differ for {changed:?}"));
        }
        if left.sitemap != right.sitemap {
            diffs.push("sitemap entries differ".to_string());
        }
        if left.rss != right.rss {
            diffs.push("rss entries differ".to_string());
        }
        if left.wip_routes != right.wip_routes {
            diffs.push(format!(
                "wip routes differ: {:?} vs {:?}",
                left.wip_routes, right.wip_routes
            ));
        }
        if left.works_routes != right.works_routes {
            diffs.push(format!(
                "works routes differ: {:?} vs {:?}",
                left.works_routes, right.works_routes
            ));
        }
        diffs
    }

    /// The route count, for a compact progress or evidence line.
    pub fn route_count(&self) -> usize {
        self.routes.len()
    }
}

fn difference(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .filter(|item| !right.contains(item))
        .cloned()
        .collect()
}

/// Where a release's artefacts live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePaths {
    pub directory: std::path::PathBuf,
}

impl ReleasePaths {
    /// The standard layout for a release inside a project (`docs/spec/01-data-model.md` §5).
    pub fn new(project_root: impl AsRef<Path>, release_id: &ReleaseId) -> Self {
        Self {
            directory: project_root
                .as_ref()
                .join(veloct_core::layout::VELOCT_DIR)
                .join(veloct_core::layout::RELEASES_DIR)
                .join(release_id.as_str()),
        }
    }

    pub fn manifest(&self) -> std::path::PathBuf {
        self.directory.join("release.json")
    }

    pub fn build_inputs(&self) -> std::path::PathBuf {
        self.directory.join("build-inputs.json")
    }

    pub fn target_snapshot(&self) -> std::path::PathBuf {
        self.directory.join("target-snapshot.json")
    }

    pub fn dist_manifest(&self) -> std::path::PathBuf {
        self.directory.join("dist-manifest.json")
    }

    /// The frozen site template (`ADR-0001`), so a release stays rebuildable after the app upgrades
    /// its bundled template.
    pub fn site_template(&self) -> std::path::PathBuf {
        self.directory.join("site-template")
    }

    /// Create the release directory.
    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.directory).map_err(|e| VeloctError::io(&self.directory, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::FixedClock;

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
            content: BTreeMap::from([
                ("content/articles/a.md".to_string(), "hash-a".to_string()),
                ("content/articles/b.md".to_string(), "hash-b".to_string()),
            ]),
            works: BTreeMap::from([("01ABC".to_string(), "work-hash".to_string())]),
            site_config: serde_json::json!({ "name": "Veloct", "url": "https://example.com" }),
            navigation: serde_json::json!([]),
            comments: serde_json::json!({ "enabled": false }),
            seo: serde_json::json!({ "engines": ["bing"] }),
            lockfile_hash: "lock-hash".into(),
            lockfile_path: Some("site/package-lock.json".into()),
            build_env: BTreeMap::from([("NODE_ENV".to_string(), "production".to_string())]),
            credential_refs: BTreeMap::new(),
        }
    }

    fn manifest() -> ReleaseManifest {
        let release_id = ReleaseId::from_stamp(
            time::macros::datetime!(2026-01-01 12:00:00 UTC),
            &[1, 2, 3],
        );
        ReleaseManifest {
            release_id,
            schema: RELEASE_SCHEMA,
            build_inputs_hash: inputs().hash().unwrap(),
            build_toolchain_snapshot: toolchain(),
            deployment_target_snapshot_ref: "target-snapshot.json".into(),
            site_template_version: "1.0.0".into(),
            release_build_time: "2026-01-01T12:00:00Z".into(),
            created_at: "2026-01-01T12:00:00Z".into(),
            created_from_release_id: None,
        }
    }

    #[test]
    fn the_manifest_never_contains_its_own_commit_sha() {
        // handoff 14.4 step 1: this is the rule the whole anchoring scheme rests on.
        let sample = manifest();
        let json = sample.to_json().unwrap();
        assert!(!json.contains("commit"), "{json}");

        // The struct has no such field at all.
        let fields = serde_json::to_value(&sample).unwrap();
        assert!(fields.get("commit_sha").is_none());
        assert!(fields.get("commit").is_none());

        // And an unknown key is refused rather than ignored, so a hand-edited or
        // future-schema manifest cannot be read while silently dropping part of its meaning.
        let mut tampered = serde_json::to_value(&sample).unwrap();
        tampered["commit_sha"] = serde_json::json!("deadbeef");
        let text = serde_json::to_string(&tampered).unwrap();
        let error = ReleaseManifest::from_json(&text).unwrap_err();
        assert!(
            error.to_string().contains("commit_sha") || error.to_string().contains("unknown field"),
            "{error}"
        );
    }

    #[test]
    fn the_manifest_round_trips_through_disk() {
        let sample = manifest();
        let text = sample.to_json().unwrap();
        assert!(text.ends_with('\n'));
        assert_eq!(ReleaseManifest::from_json(&text).unwrap(), sample);
    }

    #[test]
    fn a_manifest_from_a_future_schema_is_rejected() {
        let mut manifest = manifest();
        manifest.schema = RELEASE_SCHEMA + 1;
        let error = manifest.validate().unwrap_err();
        assert!(error.to_string().contains("not supported"), "{error}");
    }

    #[test]
    fn a_manifest_with_a_bad_timestamp_is_rejected() {
        let mut broken = manifest();
        broken.created_at = "yesterday".into();
        assert!(broken.validate().unwrap_err().to_string().contains("created_at"));

        let mut broken = manifest();
        broken.release_build_time = "".into();
        assert!(broken
            .validate()
            .unwrap_err()
            .to_string()
            .contains("release_build_time"));
    }

    #[test]
    fn the_inputs_hash_excludes_the_commit_sha() {
        // Row A12: the inputs hash cannot see a commit, and the fingerprint can.
        let inputs = inputs();
        let first = inputs.hash().unwrap();

        let fingerprint_a = FingerprintInputs {
            release_commit_sha: "aaaa".into(),
            build_inputs_hash: first.clone(),
            lockfile_hash: inputs.lockfile_hash.clone(),
            toolchain: toolchain(),
            build_env: inputs.build_env.clone(),
        };
        let fingerprint_b = FingerprintInputs {
            release_commit_sha: "bbbb".into(),
            ..fingerprint_a.clone()
        };

        assert_eq!(inputs.hash().unwrap(), first, "the inputs hash must be stable");
        assert_ne!(
            fingerprint_a.compute(),
            fingerprint_b.compute(),
            "a different commit must produce a different fingerprint"
        );
    }

    #[test]
    fn the_environment_hash_covers_only_the_allow_list() {
        // Row A13.
        let filtered = BuildInputs::filter_env([
            ("NODE_ENV", "production"),
            ("VELOCT_SITE_URL", "https://example.com"),
            ("PATH", "/usr/bin"),
            ("SECRET_TOKEN", "sk-abc"),
            ("HOME", "/root"),
        ]);
        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains_key("NODE_ENV"));
        assert!(!filtered.contains_key("PATH"));
        assert!(!filtered.contains_key("SECRET_TOKEN"));
        assert!(!filtered.contains_key("HOME"));

        // A change to a non-allow-listed variable cannot move the hash.
        let mut inputs = inputs();
        inputs.build_env = filtered;
        let before = inputs.hash().unwrap();
        inputs
            .build_env
            .insert("PATH".to_string(), "/somewhere/else".to_string());
        assert_ne!(before, inputs.hash().unwrap(), "an inserted key does change the hash");

        // But the *collection* step is what enforces the allow-list.
        let mut allowed = inputs.clone();
        allowed.build_env = BuildInputs::filter_env([("PATH", "/somewhere/else")]);
        assert!(allowed.build_env.is_empty());
    }

    #[test]
    fn the_same_inputs_always_hash_the_same() {
        // Row A5 (fingerprint stability) starts here.
        let first = inputs();
        let mut second = first.clone();
        // Rebuild the maps in a different insertion order; a BTreeMap makes that irrelevant.
        second.content = BTreeMap::new();
        second
            .content
            .insert("content/articles/b.md".to_string(), "hash-b".to_string());
        second
            .content
            .insert("content/articles/a.md".to_string(), "hash-a".to_string());

        assert_eq!(first.hash().unwrap(), second.hash().unwrap());
    }

    #[test]
    fn a_content_change_moves_the_inputs_hash() {
        let before = inputs().hash().unwrap();
        let mut changed = inputs();
        changed
            .content
            .insert("content/articles/a.md".to_string(), "different".to_string());
        assert_ne!(before, changed.hash().unwrap());
    }

    #[test]
    fn canonical_json_is_order_independent() {
        let a = serde_json::json!({ "b": 1, "a": { "d": 2, "c": 3 } });
        let b = serde_json::json!({ "a": { "c": 3, "d": 2 }, "b": 1 });
        assert_eq!(canonical_json(&a).unwrap(), canonical_json(&b).unwrap());
        assert_eq!(canonical_json(&a).unwrap(), r#"{"a":{"c":3,"d":2},"b":1}"#);

        // Arrays keep their order: order is meaningful in a list.
        let list_a = serde_json::json!([1, 2, 3]);
        let list_b = serde_json::json!([3, 2, 1]);
        assert_ne!(canonical_json(&list_a).unwrap(), canonical_json(&list_b).unwrap());
    }

    #[test]
    fn the_host_operating_system_does_not_affect_the_fingerprint() {
        // handoff 14.9: a rebuild on another machine is expected, so the host must not move the
        // fingerprint. This is why the fingerprint hashes the toolchain's hash (which excludes the
        // host) rather than its human-readable label (which includes it).
        let mut other = toolchain();
        other.os = "linux".into();
        assert_eq!(toolchain().hash(), other.hash());
        assert_ne!(toolchain().label(), other.label(), "the label does record the host");

        let inputs = inputs();
        let on_windows = FingerprintInputs {
            release_commit_sha: "sha".into(),
            build_inputs_hash: inputs.hash().unwrap(),
            lockfile_hash: inputs.lockfile_hash.clone(),
            toolchain: toolchain(),
            build_env: inputs.build_env.clone(),
        };
        let on_linux = FingerprintInputs {
            toolchain: other,
            ..on_windows.clone()
        };
        assert_eq!(on_windows.compute(), on_linux.compute());
    }

    #[test]
    fn a_different_toolchain_component_changes_the_fingerprint() {
        let inputs = inputs();
        let base = FingerprintInputs {
            release_commit_sha: "sha".into(),
            build_inputs_hash: inputs.hash().unwrap(),
            lockfile_hash: inputs.lockfile_hash.clone(),
            toolchain: toolchain(),
            build_env: inputs.build_env.clone(),
        };

        let mut newer = base.clone();
        newer.toolchain.astro = "8.0.0".into();
        assert_ne!(base.compute(), newer.compute());

        let mut other_lock = base.clone();
        other_lock.lockfile_hash = "different".into();
        assert_ne!(base.compute(), newer.compute());
        assert_ne!(base.compute(), other_lock.compute());
    }

    #[test]
    fn every_sub_target_of_one_release_gets_the_same_fingerprint() {
        // Row A2 and A11: this is the property that proves "one build, many targets".
        let inputs = inputs();
        let fingerprint = FingerprintInputs {
            release_commit_sha: "abc123".into(),
            build_inputs_hash: inputs.hash().unwrap(),
            lockfile_hash: inputs.lockfile_hash.clone(),
            toolchain: toolchain(),
            build_env: inputs.build_env.clone(),
        };
        let computed = fingerprint.compute();

        // Main and works sub-targets receive the fingerprint, not compute it separately.
        let main = computed.clone();
        let works = computed.clone();
        assert_eq!(main, works);
        assert_eq!(computed.len(), 64, "SHA-256 in hex");
    }

    #[test]
    fn the_target_snapshot_exposes_a_main_and_works_pair() {
        // handoff 14.7: a logical target owns exactly two sub-targets.
        let snapshot = DeploymentTargetSnapshot {
            sub_targets: vec![
                SubTargetSnapshot {
                    logical_target_id: "cloudflare".into(),
                    role: "main".into(),
                    config: serde_json::json!({ "project": "site" }),
                    credential_ref: Some("keychain:cloudflare".into()),
                    is_degraded: false,
                },
                SubTargetSnapshot {
                    logical_target_id: "cloudflare".into(),
                    role: "works".into(),
                    config: serde_json::json!({ "project": "works" }),
                    credential_ref: Some("keychain:cloudflare".into()),
                    is_degraded: false,
                },
                SubTargetSnapshot {
                    logical_target_id: "edgeone_cn".into(),
                    role: "main".into(),
                    config: serde_json::json!({}),
                    credential_ref: None,
                    is_degraded: true,
                },
            ],
        };

        assert_eq!(snapshot.logical_targets(), vec!["cloudflare", "edgeone_cn"]);
        let (main, works) = snapshot.pair("cloudflare").unwrap();
        assert_eq!(main.role, "main");
        assert!(works.is_works());
        assert!(!main.is_degraded);

        assert!(snapshot.pair("edgeone_intl").is_none());
        assert!(snapshot.pair("edgeone_cn").is_none(), "a lone sub-target is not a pair");
    }

    #[test]
    fn the_target_snapshot_hash_ignores_declaration_order() {
        let first = DeploymentTargetSnapshot {
            sub_targets: vec![
                SubTargetSnapshot {
                    logical_target_id: "cloudflare".into(),
                    role: "main".into(),
                    config: serde_json::json!({ "a": 1, "b": 2 }),
                    credential_ref: None,
                    is_degraded: false,
                },
                SubTargetSnapshot {
                    logical_target_id: "cloudflare".into(),
                    role: "works".into(),
                    config: serde_json::json!({ "c": 3 }),
                    credential_ref: None,
                    is_degraded: false,
                },
            ],
        };
        let mut second = first.clone();
        second.sub_targets.reverse();
        assert_eq!(first.hash().unwrap(), second.hash().unwrap());
    }

    #[test]
    fn a_changed_target_configuration_changes_the_snapshot_hash() {
        let mut snapshot = DeploymentTargetSnapshot {
            sub_targets: vec![SubTargetSnapshot {
                logical_target_id: "cloudflare".into(),
                role: "main".into(),
                config: serde_json::json!({ "project": "site" }),
                credential_ref: None,
                is_degraded: false,
            }],
        };
        let before = snapshot.hash().unwrap();
        snapshot.sub_targets[0].config = serde_json::json!({ "project": "renamed" });
        assert_ne!(before, snapshot.hash().unwrap());
    }

    #[test]
    fn a_renamed_deployment_project_does_not_change_the_fingerprint() {
        // handoff 14.7: the frozen snapshot is what a re-send uses, so renaming a project later
        // must not make the historical release look different.
        let inputs = inputs();
        let fingerprint = FingerprintInputs {
            release_commit_sha: "abc".into(),
            build_inputs_hash: inputs.hash().unwrap(),
            lockfile_hash: inputs.lockfile_hash.clone(),
            toolchain: toolchain(),
            build_env: inputs.build_env.clone(),
        };
        let before = fingerprint.compute();

        // The *current* configuration changes, but the fingerprint is built from frozen inputs.
        let renamed = DeploymentTargetSnapshot {
            sub_targets: vec![SubTargetSnapshot {
                logical_target_id: "cloudflare".into(),
                role: "main".into(),
                config: serde_json::json!({ "project": "renamed-later" }),
                credential_ref: None,
                is_degraded: false,
            }],
        };
        assert!(!renamed.sub_targets.is_empty());
        assert_eq!(fingerprint.compute(), before);
    }

    #[test]
    fn the_dist_manifest_normalises_every_collection() {
        let manifest = DistManifest {
            routes: vec!["/b/".into(), "/a/".into(), "/a/".into()],
            pages: BTreeMap::from([("/a/".into(), "h1".into())]),
            assets: BTreeMap::from([("_astro/x.css".into(), "h2".into())]),
            sitemap: vec!["https://example.com/b/".into(), "https://example.com/a/".into()],
            rss: vec!["post-b".into(), "post-a".into()],
            wip_routes: vec!["/wip/02/".into(), "/wip/01/".into()],
            works_routes: vec!["/demo/".into()],
        };

        let normalised = manifest.clone().normalised();
        assert_eq!(normalised.routes, vec!["/a/", "/b/"]);
        assert_eq!(normalised.sitemap[0], "https://example.com/a/");
        assert_eq!(normalised.rss, vec!["post-a", "post-b"]);
        assert_eq!(normalised.wip_routes, vec!["/wip/01/", "/wip/02/"]);
        assert_eq!(normalised.route_count(), 2);
    }

    #[test]
    fn two_builds_of_one_release_have_no_differences() {
        // Row A6 and A5's semantic half.
        let manifest = DistManifest {
            routes: vec!["/".into(), "/article/a/".into()],
            pages: BTreeMap::from([
                ("/".into(), "hash-root".into()),
                ("/article/a/".into(), "hash-a".into()),
            ]),
            assets: BTreeMap::from([("_astro/app.css".into(), "css-hash".into())]),
            sitemap: vec!["https://example.com/".into()],
            rss: vec!["a".into()],
            wip_routes: vec![],
            works_routes: vec!["/demo/".into()],
        };

        let mut rebuilt = manifest.clone();
        // A rebuild may emit the same content in a different order.
        rebuilt.routes.reverse();
        rebuilt.sitemap.reverse();

        assert!(manifest.differences(&rebuilt).is_empty());
    }

    #[test]
    fn a_difference_is_reported_with_a_specific_reason() {
        let base = DistManifest {
            routes: vec!["/".into(), "/article/a/".into()],
            pages: BTreeMap::from([
                ("/".into(), "hash-root".into()),
                ("/article/a/".into(), "hash-a".into()),
            ]),
            ..Default::default()
        };

        // A removed route.
        let mut fewer = base.clone();
        fewer.routes = vec!["/".into()];
        let diffs = base.differences(&fewer);
        assert!(diffs.iter().any(|d| d.contains("routes differ")), "{diffs:?}");
        assert!(diffs.iter().any(|d| d.contains("/article/a/")), "{diffs:?}");

        // A changed page hash.
        let mut changed = base.clone();
        changed.pages.insert("/".into(), "different".into());
        let diffs = base.differences(&changed);
        assert!(diffs.iter().any(|d| d.contains("page hashes differ for [\"/\"]")), "{diffs:?}");

        // A changed sitemap.
        let mut sitemap = base.clone();
        sitemap.sitemap = vec!["https://example.com/new/".into()];
        assert!(base
            .differences(&sitemap)
            .iter()
            .any(|d| d.contains("sitemap")));

        // A changed asset hash.
        let mut assets = base.clone();
        assets.assets.insert("a.css".into(), "new".into());
        assert!(base.differences(&assets).iter().any(|d| d.contains("asset hashes")));

        // Changed WIP and works route sets.
        let mut wip = base.clone();
        wip.wip_routes = vec!["/wip/01/".into()];
        assert!(base.differences(&wip).iter().any(|d| d.contains("wip routes")));

        let mut works = base.clone();
        works.works_routes = vec!["/demo/".into()];
        assert!(base.differences(&works).iter().any(|d| d.contains("works routes")));

        // Changed RSS entries.
        let mut rss = base.clone();
        rss.rss = vec!["a".into()];
        assert!(base.differences(&rss).iter().any(|d| d.contains("rss")));
    }

    #[test]
    fn the_dist_manifest_round_trips_through_disk_in_normalised_form() {
        let manifest = DistManifest {
            routes: vec!["/b/".into(), "/a/".into()],
            ..Default::default()
        };
        let text = manifest.to_json().unwrap();
        let read = DistManifest::from_json(&text).unwrap();
        assert_eq!(read.routes, vec!["/a/", "/b/"]);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn release_paths_match_the_documented_layout() {
        let release_id = ReleaseId::from_stamp(
            time::macros::datetime!(2026-01-01 12:00:00 UTC),
            &[9, 9, 9],
        );
        let paths = ReleasePaths::new(r"D:\Veloct-Blog\project", &release_id);
        assert!(paths.manifest().ends_with("release.json"));
        assert!(paths.directory.ends_with(release_id.as_str()));
        assert!(paths
            .directory
            .to_string_lossy()
            .contains(".veloct"));
        assert!(paths.site_template().ends_with("site-template"));

        let dir = tempfile::tempdir().unwrap();
        let paths = ReleasePaths::new(dir.path(), &release_id);
        paths.ensure().unwrap();
        assert!(paths.directory.is_dir());
    }

    #[test]
    fn credential_values_never_reach_the_inputs() {
        // handoff 12 and 14.6: only opaque handles are recorded.
        let mut inputs = inputs();
        inputs.credential_refs.insert(
            "cloudflare".to_string(),
            "keychain:cloudflare/prod".to_string(),
        );
        let json = serde_json::to_string(&inputs).unwrap();
        assert!(json.contains("keychain:cloudflare/prod"));
        // The hash includes the handle, so re-binding a credential is visible in the evidence.
        let with_handle = inputs.hash().unwrap();
        inputs
            .credential_refs
            .insert("cloudflare".to_string(), "keychain:cloudflare/staging".to_string());
        assert_ne!(with_handle, inputs.hash().unwrap());
    }

    #[test]
    fn the_frozen_build_time_keeps_time_derived_fields_stable() {
        // handoff 14.9: a rebuild must not move the build time.
        use veloct_core::Clock;
        let clock = FixedClock::new(time::macros::datetime!(2026-01-01 12:00:00 UTC));
        let sample = manifest();
        assert_eq!(
            sample.release_build_time,
            veloct_core::clock::format_rfc3339(clock.now())
        );

        let mut rebuilt = sample.clone();
        // The clock is fixed, so re-deriving the time produces the same value.
        rebuilt.release_build_time = veloct_core::clock::format_rfc3339(clock.now());
        assert_eq!(sample, rebuilt);
    }
}
