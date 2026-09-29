//! Running the real Astro build, and publishing from the window.
//!
//! `handoff.md` §28 asks for the release pipeline to be verifiable and §19.3 fixes the sequence. The
//! sequence itself lives in `veloct_release::publish`; what this module supplies is the two things the
//! library deliberately does not know about: how to RUN the site's build, and how to describe the toolchain
//! that ran it.
//!
//! ## Why output goes through a file rather than a pipe
//!
//! `std::process::Command::output()` reads the child through an anonymous pipe. Under the file sandbox this
//! process runs in, opening a named pipe is refused, so `output()` fails with EPERM for a reason that has
//! nothing to do with the build. Redirecting the child's stdout and stderr to FILES and reading them
//! afterwards works in both modes, and it has a second advantage that matters more: a build's output is
//! wanted for the log, and a `String` captured from a pipe is capped by memory while a file is not.
//!
//! ## Why the Node search differs from the sidecar's
//!
//! The sidecar resolves Node from the sidecar path and NEVER from `PATH` (ADR-0005). That is right for a
//! shipped application. This module runs inside the application but is reached from a build tree too, and a
//! build tree has a `node` on `PATH` and a runtime beside the sidecar. So the search here is: the pinned
//! sidecar runtime first, then `VELOCT_NODE`, then `PATH` — and the LAST is a deliberate, documented
//! difference rather than an oversight. ADR-0005 governs what the application RESOLVES for a user; a build
//! running here is the developer's own build, and refusing to find their Node would make `cargo test`
//! useless.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use veloct_core::{Clock, Result, VeloctError};
use veloct_release::{
    BuildInputs, BuildOutcome, BuildRequest, BuildRunner, DeploymentTargetSnapshot, PublishOutcome,
    PublishRequest, ToolchainSnapshot,
};

/// The environment variable the site template reads for the project root.
pub const PROJECT_ROOT_ENV: &str = "VELOCT_PROJECT_ROOT";

/// The environment variable the site template reads for the frozen build time.
pub const BUILD_TIME_ENV: &str = "VELOCT_BUILD_TIME";

/// The environment variable the site template reads for its output root.
pub const DIST_ROOT_ENV: &str = "VELOCT_DIST_ROOT";

/// The environment variable that disables comments in a build (`handoff.md` §16).
pub const DISABLE_COMMENTS_ENV: &str = "VELOCT_DISABLE_COMMENTS";

/// How much of a build's output to keep for the log.
const LOG_TAIL_BYTES: usize = 8_000;

/// Find the Node runtime to build with.
///
/// Order: the pinned sidecar runtime, `VELOCT_NODE`, then `PATH`. See the module comment for why the last
/// is here on purpose.
pub fn resolve_node(sidecar_directory: Option<&Path>) -> PathBuf {
    if let Some(directory) = sidecar_directory {
        if let Ok(triple) = veloct_sidecar::target_triple() {
            for candidate in [
                directory.join(veloct_sidecar::node_file_name(triple)),
                directory
                    .join("binaries")
                    .join(veloct_sidecar::node_file_name(triple)),
            ] {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }

    if let Some(explicit) = std::env::var_os(veloct_sidecar::NODE_ENV) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return path;
        }
    }

    // `node` rather than an absolute path: the OS resolver finds it, which is the point.
    PathBuf::from("node")
}

/// Run a command, capturing its output through temporary files.
///
/// Returns `(exit_code, combined_output)`. A non-zero exit is NOT an error here: the caller decides, because
/// a build that failed is a result to report rather than an I/O problem.
pub fn run_captured(program: &Path, args: &[String], directory: &Path, env: &BTreeMap<String, String>) -> Result<(i32, String)> {
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    );
    let temp = std::env::temp_dir();
    let stdout_path = temp.join(format!("veloct-build-{stamp}.out"));
    let stderr_path = temp.join(format!("veloct-build-{stamp}.err"));

    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|error| VeloctError::io(&stdout_path, error))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|error| VeloctError::io(&stderr_path, error))?;

    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout_file))
        .stderr(std::process::Stdio::from(stderr_file));
    for (key, value) in env {
        command.env(key, value);
    }

    let status = command.status().map_err(|error| {
        VeloctError::Build(format!(
            "could not run {}: {error}",
            program.display()
        ))
    });

    // Read whatever was written, even when the spawn failed, so a partial run is still explained.
    let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let _ = std::fs::remove_file(&stdout_path);
    let _ = std::fs::remove_file(&stderr_path);

    let status = status?;
    let combined = format!("{stdout}\n{stderr}");
    Ok((status.code().unwrap_or(-1), tail(&combined, LOG_TAIL_BYTES)))
}

/// The last `limit` bytes of a string, cut on a line boundary.
fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut start = text.len() - limit;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    // Move to the next newline so the first line is not a fragment.
    match text[start..].find('\n') {
        Some(index) => format!("…\n{}", &text[start + index + 1..]),
        None => text[start..].to_string(),
    }
}

/// The real build: the bundled Node running the site's own Astro, writing the dist directly.
///
/// `VELOCT_DIST_ROOT` points Astro at `<project>/dist`, so the two publish roots appear exactly where
/// `veloct_release` expects them and no copy step is needed. The build is invoked as `astro build` rather
/// than through npm:
///
/// * npm is not bundled (ADR-0005 ships Node), so requiring it would make the build depend on something the
///   application does not have;
/// * `npm run build` would introduce a package-manager layer between the release and the build, and §14.9's
///   determinism claim is easier to defend the fewer things are in between.
pub struct AstroBuildRunner {
    node: PathBuf,
    /// Where the site's `node_modules` lives, so the Astro entry can be found.
    project_root: PathBuf,
    site_dir: PathBuf,
}

impl AstroBuildRunner {
    pub fn new(node: PathBuf, project_root: PathBuf, site_dir: PathBuf) -> Self {
        Self {
            node,
            project_root,
            site_dir,
        }
    }

    /// The Astro entry point in the site's dependencies.
    fn astro_entry(&self) -> Result<PathBuf> {
        veloct_sidecar::astro_entry_in(&self.site_dir).ok_or_else(|| {
            VeloctError::Build(format!(
                "no Astro CLI in {}; run the site's dependency install first (npm install in site/)",
                self.site_dir.join(veloct_sidecar::ASTRO_PACKAGE).display()
            ))
        })
    }
}

impl BuildRunner for AstroBuildRunner {
    #[allow(clippy::needless_borrow)]
    fn build(&self, request: &BuildRequest) -> Result<BuildOutcome> {
        let entry = self.astro_entry()?;

        let mut env = request.env.clone();
        // The template reads these rather than inferring from its own location, which is what makes a frozen
        // template build the project it was pointed at (ADR-0013).
        env.insert(
            PROJECT_ROOT_ENV.to_string(),
            self.project_root.to_string_lossy().to_string(),
        );
        env.insert(BUILD_TIME_ENV.to_string(), request.release_build_time.clone());
        // `VELOCT_DIST_ROOT` is deliberately NOT set here.
        //
        // `site/astro.config.mjs` resolves it itself from its own location (`resolve(siteDirPath, "dist")`)
        // and ASSIGNS it, so anything this process exports is overwritten before the integration reads it.
        // The first version of this runner set it to the project's `dist/`, and the paths it then expected
        // were not where the build wrote — see `site/astro.config.mjs` and the split-publish-roots
        // integration for why the template owns this decision rather than the caller.

        let args = vec![
            entry.to_string_lossy().to_string(),
            "build".to_string(),
        ];

        let (code, log) = run_captured(&self.node, &args, &self.site_dir, &env)?;
        if code != 0 {
            return Err(VeloctError::Build(format!(
                "the Astro build exited with {code}:\n{log}"
            )));
        }

        // The template writes the two publish roots under the SITE's own dist: `dist/site` for the main
        // site and `dist/works` for the work bodies, split by the `split-publish-roots` integration. There
        // is nothing to move, and the paths are checked rather than assumed.
        let built = self.site_dir.join("dist");
        let site_root = built.join("site");
        let works_root = built.join("works");
        if !site_root.is_dir() {
            return Err(VeloctError::Build(format!(
                "the build produced no {}; the log was:\n{log}",
                site_root.display()
            )));
        }
        if !works_root.is_dir() {
            return Err(VeloctError::Build(format!(
                "the build produced no works root at {}; the split-publish-roots integration writes it\n{log}",
                works_root.display()
            )));
        }

        Ok(BuildOutcome {
            site_root,
            works_root,
            log,
            dist_manifest: None,
        })
    }
}

/// Describe the toolchain that a build ran with, so the fingerprint can include it.
///
/// Every version is QUERIED rather than assumed: a hardcoded "24.18.0" would make the fingerprint agree
/// across machines that disagreed, which is the opposite of what a fingerprint is for. A missing version is
/// reported as `unknown` rather than invented, and `unknown` changing the fingerprint is correct — an
/// unidentifiable toolchain is not the same input as an identified one.
pub fn capture_toolchain(node: &Path, project_root: &Path) -> ToolchainSnapshot {
    let site = project_root.join("site");
    ToolchainSnapshot {
        node: version_of(node, &["--version".to_string()]),
        npm: version_of(&PathBuf::from(npm_program()), &["--version".to_string()]),
        astro: astro_version(&site),
        veloct: veloct_core::VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
    }
}

fn npm_program() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

/// Ask a program for its version, through a temp file for the reason this module documents.
fn version_of(program: &Path, args: &[String]) -> String {
    let directory = std::env::temp_dir();
    match run_captured(program, args, &directory, &BTreeMap::new()) {
        Ok((0, output)) => {
            let text = output.trim().trim_start_matches('v').to_string();
            if text.is_empty() {
                "unknown".to_string()
            } else {
                text
            }
        }
        _ => "unknown".to_string(),
    }
}

/// The Astro version from the site's installed package, not from the manifest's range.
///
/// The installed version is what actually runs the build; a range like `^7.3.5` in `package.json` describes
/// what MAY be installed, which is not the same input.
fn astro_version(site: &Path) -> String {
    let manifest = site
        .join(veloct_sidecar::ASTRO_PACKAGE)
        .join("package.json");
    std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("version")
                .and_then(|version| version.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

/// The environment a build is allowed to see.
///
/// `BuildInputs::filter_env` is the single definition of the allow list, so this does not restate it.
pub fn build_environment() -> BTreeMap<String, String> {
    // Collect the pairs first: `std::env::vars` yields owned `String`s and `filter_env` borrows `&str`s, so
    // the owned values have to outlive the call.
    let pairs: Vec<(String, String)> = std::env::vars().collect();
    BuildInputs::filter_env(
        pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    )
}

/// What a publish request needs from the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublishSettings {
    /// The site's canonical URL, or empty when it is not published yet.
    pub site_url: String,
    /// Whether comments are enabled for the build.
    pub comments_enabled: bool,
}

impl PublishSettings {
    /// Read the settings the build needs from the project's site configuration.
    ///
    /// Read from `data/site.json`, which is the same file the template reads, so the release's frozen inputs
    /// and the build cannot disagree about the URL.
    pub fn read(project_root: &Path) -> Self {
        let text = std::fs::read_to_string(project_root.join("data/site.json")).unwrap_or_default();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
        Self {
            site_url: value
                .get("url")
                .and_then(|url| url.as_str())
                .unwrap_or_default()
                .to_string(),
            comments_enabled: value
                .get("comments")
                .and_then(|comments| comments.get("enabled"))
                .and_then(|enabled| enabled.as_bool())
                .unwrap_or(false),
        }
    }
}

/// Assemble the frozen build inputs for a release.
pub fn collect_build_inputs(project_root: &Path, settings: &PublishSettings) -> Result<BuildInputs> {
    let text = std::fs::read_to_string(project_root.join("data/site.json")).unwrap_or_default();
    let site_config: serde_json::Value =
        serde_json::from_str(&text).unwrap_or(serde_json::json!({}));

    let navigation = std::fs::read_to_string(project_root.join("data/navigation.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::json!([]));

    let comments = serde_json::json!({
        "enabled": settings.comments_enabled,
    });

    let mut inputs = BuildInputs {
        site_config,
        navigation,
        comments,
        build_env: build_environment(),
        ..BuildInputs::default()
    };

    // The lockfile that resolved the build's dependencies is part of what produced the output (§14.6).
    let lockfile = project_root.join("site/package-lock.json");
    if let Ok(bytes) = std::fs::read(&lockfile) {
        inputs.lockfile_hash = veloct_core::sha256_hex(&bytes);
        inputs.lockfile_path = Some("site/package-lock.json".to_string());
    }

    Ok(inputs)
}

/// Run a full publish for a project.
pub fn publish_project(
    project_root: &Path,
    release_id: veloct_core::ids::ReleaseId,
    created_from: Option<veloct_core::ids::ReleaseId>,
    sidecar_directory: Option<&Path>,
    clock: &dyn Clock,
    repository: Option<&git2::Repository>,
) -> Result<PublishOutcome> {
    let site_dir = project_root.join("site");
    let node = resolve_node(sidecar_directory);
    let settings = PublishSettings::read(project_root);
    let inputs = collect_build_inputs(project_root, &settings)?;
    let toolchain = capture_toolchain(&node, project_root);

    // The frozen build time. §14.9 requires time-derived output not to move on a rebuild, so this is chosen
    // ONCE here and passed to both the release and the build.
    let build_time = veloct_core::clock::format_rfc3339(clock.now());

    let runner = AstroBuildRunner::new(node, project_root.to_path_buf(), site_dir.clone());
    veloct_release::publish(
        &PublishRequest {
            project_root: project_root.to_path_buf(),
            site_dir,
            release_id,
            build_inputs: inputs,
            target_snapshot: DeploymentTargetSnapshot::default(),
            toolchain,
            site_template_version: veloct_core::VERSION.to_string(),
            release_build_time: build_time,
            created_from_release_id: created_from,
            commit_message: None,
        },
        &runner,
        repository,
        clock,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_tail_is_cut_on_a_line_boundary() {
        let text = "one\ntwo\nthree\nfour\nfive\n";
        let cut = tail(text, 10);
        assert!(!cut.starts_with("hree"), "{cut}");
        assert!(cut.ends_with("five\n"), "{cut}");
        // A short text is returned whole.
        assert_eq!(tail("short", 100), "short");
    }

    #[test]
    fn a_log_tail_of_multibyte_text_does_not_panic() {
        // Cutting at a byte offset can land inside a character, and this project is full of Chinese.
        let text = "一二三四五六七八九十".repeat(20);
        let cut = tail(&text, 25);
        assert!(!cut.is_empty());
        // The assertion is that it did not panic on a boundary; the content is whatever survived.
        assert!(cut.chars().count() > 0);
    }

    #[test]
    fn a_missing_node_falls_back_to_the_name_rather_than_failing() {
        // The build runs the developer's own build in a build tree, so finding their node on PATH is correct
        // here in a way it is NOT for the shipped sidecar (ADR-0005). See the module comment.
        let dir = tempfile::TempDir::new().unwrap();
        let resolved = resolve_node(Some(dir.path()));
        assert_eq!(resolved, PathBuf::from("node"));
    }

    #[test]
    fn a_present_runtime_beside_the_sidecar_is_preferred_over_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let triple = veloct_sidecar::target_triple().unwrap();
        let node = dir.path().join(veloct_sidecar::node_file_name(triple));
        std::fs::write(&node, b"fake").unwrap();
        assert_eq!(resolve_node(Some(dir.path())), node);
    }

    #[test]
    fn a_runtime_in_a_binaries_subdirectory_is_also_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let triple = veloct_sidecar::target_triple().unwrap();
        let node = dir
            .path()
            .join("binaries")
            .join(veloct_sidecar::node_file_name(triple));
        std::fs::create_dir_all(node.parent().unwrap()).unwrap();
        std::fs::write(&node, b"fake").unwrap();
        assert_eq!(resolve_node(Some(dir.path())), node);
    }

    #[test]
    fn the_build_environment_is_the_allow_list_and_not_the_whole_environment() {
        // A build that inherited everything would put a credential into the frozen inputs' environment.
        let env = build_environment();
        for key in env.keys() {
            assert!(
                veloct_release::BUILD_ENV_ALLOWLIST.contains(&key.as_str()),
                "{key} is not allow-listed"
            );
        }
    }

    #[test]
    fn toolchain_versions_are_queried_and_a_missing_one_is_unknown_rather_than_invented() {
        // A hardcoded version would make two disagreeing machines fingerprint the same, which is the
        // opposite of what a fingerprint is for.
        let snapshot = capture_toolchain(Path::new("definitely-not-a-program"), Path::new("D:/nowhere"));
        assert_eq!(snapshot.node, "unknown");
        assert_eq!(snapshot.astro, "unknown");
        assert_eq!(snapshot.veloct, veloct_core::VERSION);
        assert!(!snapshot.os.is_empty());
    }

    #[test]
    fn the_astro_version_comes_from_the_installed_package_and_not_the_manifest_range() {
        // `^7.3.5` in package.json describes what MAY be installed; the installed version is the input.
        let dir = tempfile::TempDir::new().unwrap();
        let package = dir.path().join(veloct_sidecar::ASTRO_PACKAGE);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{ "name": "astro", "version": "7.3.5", "bin": { "astro": "./bin/astro.mjs" } }"#,
        )
        .unwrap();
        assert_eq!(astro_version(dir.path()), "7.3.5");

        // And an absent package is `unknown` rather than a guess.
        assert_eq!(astro_version(Path::new("D:/nowhere")), "unknown");
    }

    #[test]
    fn the_publish_settings_are_read_from_the_same_file_the_template_reads() {
        // Reading them separately would let the frozen inputs and the build disagree about the URL.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "url": "https://example.com", "comments": { "enabled": true } }"#,
        )
        .unwrap();

        let settings = PublishSettings::read(dir.path());
        assert_eq!(settings.site_url, "https://example.com");
        assert!(settings.comments_enabled);

        // A project with no configuration is empty rather than an error: it can still be published.
        let empty = PublishSettings::read(Path::new("D:/nowhere"));
        assert!(empty.site_url.is_empty());
        assert!(!empty.comments_enabled);
    }

    #[test]
    fn the_build_inputs_carry_the_site_configuration_navigation_and_comments() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::create_dir_all(dir.path().join("site")).unwrap();
        std::fs::write(
            dir.path().join("data/site.json"),
            r#"{ "url": "https://example.com", "name": "Veloct" }"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("data/navigation.json"), r#"[{ "label": "Home" }]"#).unwrap();
        std::fs::write(dir.path().join("site/package-lock.json"), r#"{ "lockfileVersion": 3 }"#)
            .unwrap();

        let settings = PublishSettings::read(dir.path());
        let inputs = collect_build_inputs(dir.path(), &settings).unwrap();

        assert_eq!(inputs.site_config["url"], "https://example.com");
        assert_eq!(inputs.navigation[0]["label"], "Home");
        assert_eq!(inputs.comments["enabled"], false);
        // The lockfile is hashed, because it resolved the dependencies that produced the output.
        assert!(!inputs.lockfile_hash.is_empty());
        assert_eq!(inputs.lockfile_path.as_deref(), Some("site/package-lock.json"));
    }

    #[test]
    fn a_project_with_no_lockfile_is_published_without_one_rather_than_failing() {
        let dir = tempfile::TempDir::new().unwrap();
        let settings = PublishSettings::read(dir.path());
        let inputs = collect_build_inputs(dir.path(), &settings).unwrap();
        assert!(inputs.lockfile_hash.is_empty());
        assert!(inputs.lockfile_path.is_none());
        // The environment is still filtered, so the frozen inputs are never the whole environment.
        for key in inputs.build_env.keys() {
            assert!(veloct_release::BUILD_ENV_ALLOWLIST.contains(&key.as_str()));
        }
    }

    #[test]
    fn a_runner_with_no_astro_reports_what_to_install_rather_than_failing_obscurely() {
        // "Cannot find module astro" from Node would send the user looking in the wrong place.
        let dir = tempfile::TempDir::new().unwrap();
        let runner = AstroBuildRunner::new(
            PathBuf::from("node"),
            dir.path().to_path_buf(),
            dir.path().join("site"),
        );
        let error = runner.astro_entry().unwrap_err();
        assert!(error.to_string().contains("dependency install"), "{error}");
    }

    #[test]
    fn running_a_program_that_does_not_exist_is_reported_with_the_program_named() {
        let dir = tempfile::TempDir::new().unwrap();
        let error = run_captured(
            Path::new("veloct-a-program-that-does-not-exist"),
            &[],
            dir.path(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("veloct-a-program-that-does-not-exist"), "{error}");
    }
}
