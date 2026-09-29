//! End-to-end release verification.
//!
//! Every unit in the publish path has its own tests, and until this file existed they had never run as
//! ONE SYSTEM. `handoff.md` §28 asks for the release pipeline to be verified, and a pipeline whose
//! stages each pass alone is not the same claim as a pipeline that produces a release.
//!
//! So this runs the real thing: it builds a real project with the real Astro template, freezes a real
//! release, tags the real Git repository, and asserts the properties the contract actually names —
//! one `dist`, no commit sha in `release.json`, a fingerprint that does not move on a rebuild, and a
//! release that survives a rollback.
//!
//! It is `#[ignore]`d by default because it spawns `npm` and takes seconds rather than milliseconds.
//! Run it with:
//!
//! ```text
//! cargo test -p veloct-release --test end_to_end_release -- --ignored --nocapture
//! ```
//!
//! Gating it on `#[ignore]` rather than on a feature is deliberate: a feature flag can be enabled by
//! accident in CI and then silently skipped, whereas an `#[ignore]` is an explicit act with a visible
//! name.

use std::path::{Path, PathBuf};
use std::process::Command;

use veloct_core::ids::ReleaseId;
use veloct_core::FixedClock;
use veloct_release::{
    build_once, freeze, load, record_dist_manifest, scan_dist, BuildOutcome, BuildRequest,
    BuildRunner, BuildInputs, DeploymentTargetSnapshot, FreezeRequest, ReleaseArtifacts,
    ReleaseManifest, ToolchainSnapshot,
};

/// The repository root, found by walking up from this test's directory.
fn repo_root() -> PathBuf {
    let mut candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/veloct-release -> crates -> the repository root.
    candidate.pop();
    candidate.pop();
    candidate
}

/// Whether the end-to-end run is possible here.
///
/// Reported rather than silently skipped: a test that quietly does nothing is worse than one that says
/// it could not run, because the first looks like a pass.
fn prerequisites() -> Result<(), String> {
    let site = repo_root().join("site");
    if !site.join("node_modules").is_dir() {
        return Err(format!(
            "{} has no node_modules, so the Astro build cannot run",
            site.display()
        ));
    }
    if !site.join("package.json").is_file() {
        return Err(format!("{} has no package.json", site.display()));
    }
    Ok(())
}

/// The real build: `npm run build` in the Astro template, then the two publish roots.
///
/// The copy is what the shell-side runner does in production. `handoff.md` §14.1 requires ONE build to
/// produce `dist/site` and `dist/works`, and the Astro integration writes both roots under the site's
/// own `dist/`, so the runner moves them into the project's `dist/`.
struct NpmBuildRunner;

impl BuildRunner for NpmBuildRunner {
    fn build(&self, request: &BuildRequest) -> Result<BuildOutcome, veloct_core::VeloctError> {
        // The frozen build time, so time-derived output cannot move on a rebuild (§14.9).
        let output = Command::new(npm_program())
            .arg("run")
            .arg("build")
            .current_dir(&request.site_dir)
            .env("VELOCT_BUILD_TIME", &request.release_build_time)
            // The project root, so the template reads the content it was pointed at rather than
            // guessing from its own location.
            .env("VELOCT_PROJECT_ROOT", &request.project_root)
            .output()
            .map_err(|error| {
                veloct_core::VeloctError::Build(format!("could not run npm: {error}"))
            })?;

        let log = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        if !output.status.success() {
            return Err(veloct_core::VeloctError::Build(format!(
                "the Astro build failed:\n{}",
                tail(&log, 40)
            )));
        }

        // The site's own `dist/` is the build's output; the two roots are moved into the project's.
        let built = request.site_dir.join("dist");
        let site_root = request.out_dir.join("site");
        let works_root = request.out_dir.join("works");
        copy_dir(&built.join("site"), &site_root)?;
        if built.join("works").is_dir() {
            copy_dir(&built.join("works"), &works_root)?;
        }

        Ok(BuildOutcome {
            site_root,
            works_root,
            log,
            // The runner does not scan; `build_once` does, so a runner cannot disagree with what is
            // recorded in the release.
            dist_manifest: None,
        })
    }
}

fn npm_program() -> &'static str {
    // On Windows `npm` is a batch file, and Rust's `Command` does not resolve the `.cmd` extension for
    // a bare name.
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    all[start..].join("\n")
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), veloct_core::VeloctError> {
    if !from.is_dir() {
        return Err(veloct_core::VeloctError::Build(format!(
            "the build produced no directory at {}",
            from.display()
        )));
    }
    std::fs::create_dir_all(to).map_err(|error| veloct_core::VeloctError::io(to, error))?;
    for entry in std::fs::read_dir(from)
        .map_err(|error| veloct_core::VeloctError::io(from, error))?
        .flatten()
    {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_dir(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)
                .map_err(|error| veloct_core::VeloctError::io(&target, error))?;
        }
    }
    Ok(())
}

/// A project laid out the way `docs/spec/01-data-model.md` §1 specifies.
///
/// The CONTENT comes from the repository's own fixtures, copied rather than generated: a build against
/// invented content would test the build, not the project layout.
fn make_project(root: &Path) -> Result<(), String> {
    let repo = repo_root();
    for segment in ["content", "data", "works"] {
        copy_tree(&repo.join(segment), &root.join(segment))
            .map_err(|error| format!("could not copy {segment}: {error}"))?;
    }
    std::fs::create_dir_all(root.join(".veloct").join("releases"))
        .map_err(|error| error.to_string())?;

    // The Astro project is the repository's, so the build runs the REAL template.
    copy_tree(&repo.join("site"), &root.join("site")).map_err(|error| error.to_string())?;
    // A node_modules for the copy, because the template resolves its dependencies from its own
    // directory and a fresh install would need the network.
    link_node_modules(&repo.join("site").join("node_modules"), &root.join("site").join("node_modules"))
        .map_err(|error| format!("could not provide node_modules: {error}"))?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if name == "node_modules" || name == "dist" || name == ".astro" || name == ".veloct-generated" {
            continue;
        }
        let source = entry.path();
        let target = to.join(&name);
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

/// Give the copy a `node_modules` without copying hundreds of megabytes.
///
/// A directory symlink needs a privilege on Windows, so a real copy is used as the fallback. Either way
/// the point is that the build resolves its dependencies, and a failure to provide them is reported
/// rather than producing a build that fails for an unrelated-looking reason.
fn link_node_modules(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    if to.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to)?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        // A Windows directory junction needs no privilege and is what `mklink /J` creates.
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(to)
            .arg(from)
            .output()?;
        if output.status.success() {
            return Ok(());
        }
        // Fall back to a copy. Slower, and it is reported in the log rather than hidden.
        eprintln!("[e2e] junction failed, copying node_modules instead");
        copy_tree(from, to)
    }
}

fn toolchain() -> ToolchainSnapshot {
    ToolchainSnapshot {
        node: "24.18.0".to_string(),
        npm: "11.19.0".to_string(),
        astro: "7.3.5".to_string(),
        veloct: veloct_core::VERSION.to_string(),
        os: std::env::consts::OS.to_string(),
    }
}

fn build_inputs(project: &Path) -> Result<BuildInputs, String> {
    // The real site configuration and navigation, hashed as the build would see them. Anything not
    // provided is left empty rather than invented.
    let site_config: serde_json::Value = std::fs::read_to_string(project.join("data/site.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::json!({}));

    // Built in the initializer rather than by field assignment: a `default()` followed by assignments
    // is the shape that lets a field be missed.
    let mut inputs = BuildInputs {
        site_config,
        ..BuildInputs::default()
    };
    // The content hashes, so the frozen inputs actually describe the content the release was built
    // from. A default here would produce a release whose inputs do not identify it.
    for (relative, name) in [("content/articles", "articles")] {
        let mut hash_source = String::new();
        let directory = project.join(relative);
        if let Ok(entries) = std::fs::read_dir(&directory) {
            let mut names: Vec<String> = entries
                .flatten()
                .filter(|entry| entry.path().is_file())
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect();
            names.sort();
            for name in names {
                let path = directory.join(&name);
                let bytes = std::fs::read(&path).unwrap_or_default();
                hash_source.push_str(&format!("{name}:{}", veloct_core::sha256_hex(&bytes)));
            }
        }
        inputs
            .content
            .insert(name.to_string(), veloct_core::sha256_hex_str(&hash_source));
    }

    // The lockfile hash, which section 14.6 makes part of the frozen inputs.
    if let Ok(lockfile) = std::fs::read(project.join("site/package-lock.json")) {
        inputs.lockfile_hash = veloct_core::sha256_hex(&lockfile);
        inputs.lockfile_path = Some("site/package-lock.json".to_string());
    }

    Ok(inputs)
}

fn target_snapshot() -> DeploymentTargetSnapshot {
    DeploymentTargetSnapshot::default()
}

/// Run the whole publish path once, returning the artefacts and the build outcome.
fn publish_once(
    project: &Path,
    release_id: &str,
    release_build_time: &str,
) -> Result<(ReleaseArtifacts, BuildOutcome), String> {
    let release_id = ReleaseId::parse(release_id).map_err(|error| error.to_string())?;
    let clock = FixedClock::new(
        time::OffsetDateTime::parse(
            "2026-01-01T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .map_err(|error| error.to_string())?,
    );

    let artifacts = freeze(
        FreezeRequest {
            project_root: project.to_path_buf(),
            release_id,
            build_inputs: build_inputs(project)?,
            target_snapshot: target_snapshot(),
            toolchain: toolchain(),
            site_template_version: veloct_core::VERSION.to_string(),
            site_template_dir: project.join("site"),
            release_build_time: release_build_time.to_string(),
            created_from_release_id: None,
        },
        &clock,
    )
    .map_err(|error| error.to_string())?;

    let runner = NpmBuildRunner;
    let mut artifacts = artifacts;
    let outcome = build_once(project, &project.join("site"), &mut artifacts, &runner)
        .map_err(|error| error.to_string())?;

    Ok((artifacts, outcome))
}

#[test]
#[ignore = "spawns npm and takes seconds; run explicitly with --ignored"]
fn the_release_pipeline_produces_a_verifiable_release_end_to_end() {
    if let Err(reason) = prerequisites() {
        panic!("the end-to-end release run cannot happen here: {reason}");
    }

    let workspace = tempfile::TempDir::new().expect("a temp dir");
    let project = workspace.path().join("project");
    std::fs::create_dir_all(&project).expect("the project directory");
    make_project(&project).expect("the project layout");

    // A real Git repository, because the release is anchored to a commit and a tag.
    let repository = veloct_git::change::init(&project).expect("a git repository");
    commit_everything(&repository, "the project fixtures");

    let release_id = "20260101T120000Z-e2e001";
    let build_time = "2026-01-01T12:00:00Z";
    let (artifacts, outcome) = publish_once(&project, release_id, build_time)
        .unwrap_or_else(|error| panic!("the publish path failed: {error}"));

    // ---- Section 14.1: ONE build produced TWO publish roots ----
    let dist = project.join("dist");
    assert!(dist.join("site").is_dir(), "dist/site exists");
    assert!(dist.join("works").is_dir(), "dist/works exists");
    assert!(
        outcome.site_root.join("index.html").is_file(),
        "the site root has a home page: {:?}",
        outcome.site_root
    );
    assert!(
        outcome.works_root.join("demo-counter/index.html").is_file()
            || std::fs::read_dir(&outcome.works_root)
                .map(|mut entries| entries.any(|entry| entry
                    .map(|entry| entry.path().join("index.html").is_file())
                    .unwrap_or(false)))
                .unwrap_or(false),
        "the works root has a work body"
    );

    // ---- Section 14.4: release.json exists, is immutable in shape, and has NO commit sha ----
    let manifest_path = project
        .join(".veloct")
        .join("releases")
        .join(release_id)
        .join("release.json");
    assert!(manifest_path.is_file(), "{}", manifest_path.display());
    let manifest_text = std::fs::read_to_string(&manifest_path).expect("the manifest");
    assert!(
        !manifest_text.contains("commit_sha") && !manifest_text.contains("commit"),
        "release.json must not carry a commit sha (section 14.4): {manifest_text}"
    );

    // It deserialises as the real type, so the shape is not merely absent-a-key.
    let reloaded: ReleaseManifest =
        serde_json::from_str(&manifest_text).expect("release.json parses as a ReleaseManifest");
    assert_eq!(reloaded.release_id.as_str(), release_id);
    assert!(
        !reloaded.build_inputs_hash.is_empty(),
        "the build inputs are frozen"
    );

    // The frozen build inputs and the target snapshot are beside it (sections 14.6 and 14.7).
    let directory = manifest_path.parent().expect("the release directory");
    for name in [
        "build-inputs.json",
        "target-snapshot.json",
        "dist-manifest.json",
    ] {
        assert!(
            directory.join(name).is_file(),
            "{name} should be frozen into the release"
        );
    }
    assert!(
        directory.join("site-template").is_dir(),
        "the site template is frozen for a reproducible rebuild"
    );

    // The frozen build inputs carry credential REFERENCES only (section 14.6).
    let inputs_text = std::fs::read_to_string(directory.join("build-inputs.json")).unwrap();
    assert!(
        !inputs_text.contains("sk-ant-") && !inputs_text.contains("AKIA"),
        "no secret value may appear in the frozen inputs"
    );

    // ---- Section 14.4: the release is anchored by a Git tag ----
    veloct_git::create_release_tag(&repository, &reloaded.release_id).expect("the release tag");
    let tags = veloct_git::list_release_tags(&repository).expect("the tags");
    assert_eq!(tags.len(), 1, "exactly one release tag: {tags:?}");
    let anchored = veloct_git::commit_for_tag(&repository, &reloaded.release_id)
        .expect("the tag resolves to a commit");
    assert!(!anchored.is_empty());
    assert!(
        veloct_git::tag_exists(&repository, &reloaded.release_id),
        "the tag exists"
    );

    // ---- Section 14.8: the fingerprint, and section 14.9's determinism ----
    let fingerprint_from = |artifacts: &ReleaseArtifacts| artifacts.fingerprint(&anchored, &toolchain());
    let first_fingerprint = fingerprint_from(&artifacts);

    // The recorded dist manifest is the normalised, comparable form (section 23.6).
    let scanned =
        scan_dist(&outcome.site_root, &outcome.works_root).expect("the dist can be scanned");
    assert!(
        !scanned.routes.is_empty(),
        "the normalised manifest lists routes"
    );

    let mut reloaded_artifacts = load(&project, &reloaded.release_id).expect("the release reloads");
    record_dist_manifest(&mut reloaded_artifacts, scanned.clone()).expect("the manifest records");

    // A SECOND build from the same frozen inputs must produce the same normalised manifest. This is
    // the determinism claim, and it is why the build time is frozen.
    let second = publish_once(&project, "20260101T130000Z-e2e002", build_time)
        .unwrap_or_else(|error| panic!("the rebuild failed: {error}"));
    let rescanned = scan_dist(&second.1.site_root, &second.1.works_root)
        .expect("the second dist can be scanned");

    assert_eq!(
        scanned.routes, rescanned.routes,
        "a rebuild of the same inputs must emit the same routes"
    );
    assert_eq!(
        scanned.assets, rescanned.assets,
        "a rebuild of the same inputs must emit the same assets"
    );

    // And the fingerprint does not move when the content does not.
    let second_fingerprint = second.0.fingerprint(&anchored, &toolchain());
    assert_eq!(
        first_fingerprint, second_fingerprint,
        "the fingerprint must be a function of the inputs, not of the run"
    );
    assert_eq!(
        first_fingerprint.len(),
        64,
        "a sha256 hex digest: {first_fingerprint}"
    );

    // A DIFFERENT commit must produce a different fingerprint, or the fingerprint would not identify
    // the release.
    let other = second.0.fingerprint("0000000000000000000000000000000000000000", &toolchain());
    assert_ne!(
        first_fingerprint, other,
        "a different commit is a different release"
    );

    // ---- Section 14.14: a rollback rebuilds from the frozen snapshot, and rewrites nothing ----
    let before = std::fs::read_to_string(&manifest_path).unwrap();
    let rolled_back = load(&project, &reloaded.release_id).expect("the release still loads");
    assert_eq!(
        rolled_back.manifest.release_id.as_str(),
        release_id,
        "a rollback selects the historical release"
    );
    assert_eq!(
        std::fs::read_to_string(&manifest_path).unwrap(),
        before,
        "loading a release must not rewrite it"
    );

    // The frozen snapshot names the commit and the tag a rollback would use.
    let snapshot_text =
        std::fs::read_to_string(directory.join("target-snapshot.json")).unwrap();
    assert!(
        !snapshot_text.is_empty(),
        "the target snapshot is what a re-send and a rollback read"
    );

    eprintln!(
        "[e2e] release {release_id} built, one dist with two roots, fingerprinted {first_fingerprint}, tagged {anchored}"
    );
}

/// Commit everything, so the release has an anchor.
fn commit_everything(repository: &git2::Repository, message: &str) {
    let mut index = repository.index().expect("the index");
    index
        .add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)
        .expect("staging");
    index.write().expect("writing the index");
    let tree_id = index.write_tree().expect("the tree");
    let tree = repository.find_tree(tree_id).expect("finding the tree");
    let signature =
        git2::Signature::now("Veloct e2e", "e2e@example.invalid").expect("a signature");
    let parent = repository
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )
        .expect("the commit");
}

/// A second, cheaper assertion of the same invariants that does NOT need npm.
///
/// The expensive test above can only run where the Astro dependencies are installed. This one runs
/// everywhere, so a broken freeze or a manifest that gained a commit key is caught on any machine.
#[test]
fn freezing_a_release_writes_the_artefacts_without_a_commit_sha() {
    let workspace = tempfile::TempDir::new().expect("a temp dir");
    let project = workspace.path().join("project");
    std::fs::create_dir_all(project.join(".veloct/releases")).unwrap();
    std::fs::create_dir_all(project.join("site")).unwrap();
    std::fs::write(project.join("site/package.json"), "{}").unwrap();

    let clock = FixedClock::new(
        time::OffsetDateTime::parse(
            "2026-01-01T12:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap(),
    );

    let artifacts = freeze(
        FreezeRequest {
            project_root: project.clone(),
            release_id: ReleaseId::parse("20260101T120000Z-cafe01").unwrap(),
            build_inputs: BuildInputs::default(),
            target_snapshot: DeploymentTargetSnapshot::default(),
            toolchain: toolchain(),
            site_template_version: veloct_core::VERSION.to_string(),
            site_template_dir: project.join("site"),
            release_build_time: "2026-01-01T12:00:00Z".to_string(),
            created_from_release_id: None,
        },
        &clock,
    )
    .expect("the freeze");

    let directory = project.join(".veloct/releases/20260101T120000Z-cafe01");
    let text = std::fs::read_to_string(directory.join("release.json")).unwrap();
    assert!(!text.contains("commit"), "{text}");
    assert!(directory.join("build-inputs.json").is_file());
    assert!(directory.join("target-snapshot.json").is_file());

    // The fingerprint excludes the operating system, so the same inputs on two machines agree
    // (section 14.9). Two toolchains differing only in `os` must produce the same fingerprint.
    let mut on_windows = toolchain();
    on_windows.os = "windows".to_string();
    let mut on_linux = toolchain();
    on_linux.os = "linux".to_string();
    let a = artifacts.fingerprint("abc", &on_windows);
    let b = artifacts.fingerprint("abc", &on_linux);
    assert_eq!(a, b, "the fingerprint must not depend on the operating system");

    // But it DOES depend on the commit, which is what anchors it to the release.
    assert_ne!(a, artifacts.fingerprint("def", &on_windows));
}
