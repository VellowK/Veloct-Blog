//! The shell's publish path, against a real Astro build.
//!
//! `crates/veloct-release/tests/end_to_end_release.rs` proves the LIBRARY sequence works, and it does so
//! with a stub runner. This proves the thing the window actually calls: `crate::publish::publish_project`,
//! with the real `AstroBuildRunner` launching the real Astro through the real bundled Node.
//!
//! The distinction is the whole point. The library test says "the ordering is right given a build that
//! behaves"; this says "the build behaves", which is the part that has been wrong twice — once because the
//! runner expected the publish roots in the project's `dist/` when the template writes them under the
//! SITE's `dist/`, and once because the Astro entry point was guessed at instead of read from the package.
//!
//! `#[ignore]`d because it spawns Node and takes seconds. Run it with:
//!
//! ```text
//! cargo test -p veloct-desktop --test publish_integration -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

/// The repository root, found by walking up from this test's directory.
fn repo_root() -> PathBuf {
    let mut candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // src-tauri -> the repository root.
    candidate.pop();
    candidate
}

/// Whether the run is possible here, REPORTED rather than silently skipped: a test that quietly does
/// nothing looks like a pass.
fn prerequisites() -> Result<(), String> {
    let root = repo_root();
    if !root.join("site/node_modules").is_dir() {
        return Err(format!(
            "{} has no node_modules, so the Astro build cannot run",
            root.join("site").display()
        ));
    }
    if !root.join("content/articles").is_dir() {
        return Err("the repository has no content/articles fixtures".to_string());
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        let name_text = name.to_string_lossy().to_string();
        // The dependency tree, previous output and the Astro cache are not copied: the first is linked, and
        // the others must not exist or the build would reuse another project's output.
        if matches!(
            name_text.as_str(),
            "node_modules" | "dist" | ".astro" | ".veloct-generated"
        ) {
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

/// Give the copied project a dependency tree without copying hundreds of megabytes.
fn provide_node_modules(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() || to.exists() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(from, to)?;
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        // `mklink` cannot create the link's own parent directory, so it is created here rather than assumed.
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // `New-Item -ItemType Junction` rather than `cmd /C mklink`: a temp path contains spaces, and `cmd`
        // splits its arguments on them however they are quoted, which `mklink` reports as
        // `Invalid switch - "node_modules"`. PowerShell takes the paths as parameters.
        let output = Command::new("powershell")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null",
                to.display(),
                from.display()
            ))
            .output()?;
        if output.status.success() {
            return Ok(());
        }
        // A junction is an optimisation for the test; a real copy is the fallback and is REPORTED rather than
        // hidden, because a silent fallback to a slow path is how a suite becomes unusable without anyone
        // noticing why.
        eprintln!(
            "[publish] junction failed ({}), copying node_modules instead",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        copy_tree(from, to)
    }
}

/// A project laid out the way `docs/spec/01-data-model.md` §1 specifies, using the repository's OWN
/// fixtures and the real Astro template.
fn make_project(root: &Path) -> Result<(), String> {
    let repo = repo_root();
    for segment in ["content", "data", "works", "site"] {
        copy_tree(&repo.join(segment), &root.join(segment))
            .map_err(|error| format!("could not copy {segment}: {error}"))?;
    }
    std::fs::create_dir_all(root.join(".veloct/releases")).map_err(|error| error.to_string())?;
    provide_node_modules(
        &repo.join("site/node_modules"),
        &root.join("site/node_modules"),
    )
    .map_err(|error| format!("could not provide node_modules: {error}"))?;
    Ok(())
}

fn commit_everything(repository: &git2::Repository, message: &str) -> String {
    let mut index = repository.index().unwrap();
    index
        .add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repository.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Veloct integration", "integration@example.invalid").unwrap();
    let parent = repository.head().ok().and_then(|head| head.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.iter().collect();
    repository
        .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)
        .unwrap()
        .to_string()
}

#[test]
#[ignore = "spawns the real Astro build through Node; run explicitly with --ignored"]
fn the_shell_publishes_a_real_site_through_the_real_build_runner() {
    if let Err(reason) = prerequisites() {
        panic!("the publish integration run cannot happen here: {reason}");
    }

    let workspace = tempfile::TempDir::new().expect("a temp dir");
    let project = workspace.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    make_project(&project).expect("the project layout");

    let repository = veloct_git::change::init(&project).expect("a git repository");
    let first_commit = commit_everything(&repository, "the project fixtures");

    // The bundled runtime, which is what the application would use.
    let node = {
        let pin = repo_root().join("src-tauri/binaries");
        let triple = veloct_sidecar::target_triple().unwrap();
        let candidate = pin.join(veloct_sidecar::node_file_name(triple));
        if candidate.is_file() {
            candidate
        } else {
            // The pinned runtime is fetched, not committed, so a machine without it uses its own Node. That
            // is the documented difference in `src-tauri/src/publish.rs`.
            PathBuf::from("node")
        }
    };
    eprintln!("[publish] using node at {}", node.display());

    let release_id = veloct_core::ids::ReleaseId::parse("20260101T120000Z-f00d01").unwrap();
    let clock = veloct_core::SystemClock;
    let outcome = veloct_desktop_lib::publish::publish_project(
        &project,
        release_id.clone(),
        None,
        Some(&repo_root().join("src-tauri/binaries")),
        &clock,
        Some(&repository),
    )
    .unwrap_or_else(|error| panic!("the publish failed: {error}"));

    // ---- the build actually ran and produced the template's own two roots ----
    let built = project.join("site/dist");
    assert!(
        built.join("site/index.html").is_file(),
        "the main site root exists at {}",
        built.join("site").display()
    );
    assert!(
        built.join("works").is_dir(),
        "the works root exists at {}",
        built.join("works").display()
    );
    assert!(
        outcome.build.site_root.join("index.html").is_file(),
        "the outcome reports the main root"
    );
    assert!(
        !outcome.build.log.is_empty(),
        "the build's output was captured through a temp file"
    );
    eprintln!("[publish] build log tail:\n{}", &outcome.build.log[outcome.build.log.len().saturating_sub(400)..]);

    // ---- the release was frozen and anchored ----
    assert!(
        outcome.artifacts.paths.manifest().is_file(),
        "release.json is on disk"
    );
    let manifest = std::fs::read_to_string(outcome.artifacts.paths.manifest()).unwrap();
    assert!(!manifest.contains("commit"), "release.json carries no commit sha:\n{manifest}");
    assert!(
        outcome.artifacts.paths.dist_manifest().is_file(),
        "the normalised dist manifest was recorded by the build"
    );
    assert!(
        outcome.is_anchored(),
        "the release was anchored: {:?}",
        outcome.anchor_error
    );

    let record = outcome.record.clone().expect("a publish record");
    assert_eq!(record.tag_name, "release/20260101T120000Z-f00d01");
    assert!(
        record.committed,
        "the release artefact is a truth source, so it was committed"
    );
    assert_eq!(
        veloct_git::commit_for_tag(&repository, &release_id).unwrap(),
        record.commit_sha,
        "the tag resolves to the commit the record names"
    );

    // ---- the manifest describes what was actually emitted ----
    let dist_manifest = outcome.artifacts.dist_manifest.clone().expect("a dist manifest");
    assert!(
        !dist_manifest.routes.is_empty(),
        "the normalised manifest lists routes"
    );
    let has_home = dist_manifest
        .routes
        .iter()
        .any(|route| route == "/" || route == "/index.html");
    assert!(has_home, "the home route is in the manifest: {:?}", dist_manifest.routes);
    eprintln!(
        "[publish] release {} built {} routes, fingerprinted {}, tagged {}",
        record.release_id,
        dist_manifest.routes.len(),
        outcome
            .artifacts
            .fingerprint(&record.commit_sha, &outcome.artifacts.manifest.build_toolchain_snapshot),
        record.commit_sha
    );

    // ---- the toolchain was QUERIED, not assumed ----
    let toolchain = &outcome.artifacts.manifest.build_toolchain_snapshot;
    assert_ne!(toolchain.node, "unknown", "the node version was read: {toolchain:?}");
    assert_ne!(
        toolchain.astro, "unknown",
        "the installed astro version was read from its package: {toolchain:?}"
    );
    assert_eq!(toolchain.veloct, veloct_core::VERSION);

    // ---- the frozen inputs describe the build and carry no secret ----
    let inputs = std::fs::read_to_string(outcome.artifacts.paths.build_inputs()).unwrap();
    assert!(inputs.contains("\"url\""), "the site url was frozen:\n{inputs}");
    assert!(
        !inputs.contains("sk-ant-") && !inputs.contains("AKIA"),
        "no key material in the frozen inputs"
    );
    assert!(
        inputs.contains("lockfile"),
        "the lockfile that resolved the dependencies is part of the inputs"
    );

    // ---- section 19.3: the publish commit is what the tag points at ----
    assert_ne!(record.commit_sha, first_commit, "the publish created its own commit");
}
