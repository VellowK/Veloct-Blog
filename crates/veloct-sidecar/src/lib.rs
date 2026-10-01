//! Resolving and running the bundled Node runtime.
//!
//! `docs/spec/00-architecture.md` ADR-0005: "Node.js is a bundled sidecar, never a user prerequisite. A
//! pinned Node runtime is bundled as a Tauri `externalBin` sidecar together with the site template, and the
//! app resolves the node binary from the sidecar path, **never from `PATH`**."
//!
//! The last clause is the one that shapes this module. Falling back to `PATH` would make the application
//! work on a developer's machine and fail on a user's, which is the worst possible distribution of a bug.
//! So [`resolve_node`] returns an error explaining what is missing rather than a runtime that happens to be
//! installed.
//!
//! What IS resolvable from the environment is the case where the sidecar is being run from a Cargo build
//! tree rather than from an installed bundle: Tauri places sidecars beside the executable under their
//! target-triple name, and a `cargo run` has no bundle at all. `VELOCT_NODE` exists for that, and it is
//! named like the other `VELOCT_*` overrides the site already honours (`VELOCT_PROJECT_ROOT`,
//! `VELOCT_DIST_ROOT`, `VELOCT_BUILD_TIME`), so there is one convention rather than three.

use std::path::{Path, PathBuf};

use veloct_core::{Result, VeloctError};

/// The name Tauri gives the bundled runtime, without the target triple.
pub const NODE_SIDECAR_STEM: &str = "veloct-node";

/// The environment override, for a build tree with no bundle.
pub const NODE_ENV: &str = "VELOCT_NODE";

/// The directory the Astro package occupies inside the site's `node_modules`.
pub const ASTRO_PACKAGE: &str = "node_modules/astro";

/// The Astro CLI's entry point, RELATIVE TO THE PACKAGE.
///
/// Taken from the package's own `bin` field in `package.json` (`./bin/astro.mjs`) rather than guessed. The
/// first version of this module hardcoded `node_modules/astro/astro.js`, which does not exist, and the
/// launcher reported a missing template for a site whose dependencies were installed perfectly well. The
/// older `astro.js` is still tried as a fallback because a site may have an older Astro pinned.
pub const ASTRO_BIN_FALLBACKS: [&str; 2] = ["bin/astro.mjs", "astro.js"];

/// The Astro entry point inside a site directory, or `None` when there is none.
///
/// Resolution order: the package's declared `bin`, then the two known file names. A missing entry is `None`
/// so the caller can report where it looked.
pub fn astro_entry_in(site_dir: &Path) -> Option<PathBuf> {
    let package = site_dir.join(ASTRO_PACKAGE);
    if !package.is_dir() {
        return None;
    }

    // The package's own declaration is authoritative, because it is what npm would run.
    if let Ok(text) = std::fs::read_to_string(package.join("package.json")) {
        if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&text) {
            let declared = manifest
                .get("bin")
                .and_then(|bin| match bin {
                    // `bin` is either a string or a map of name to path.
                    serde_json::Value::String(path) => Some(path.clone()),
                    serde_json::Value::Object(map) => map
                        .get("astro")
                        .and_then(|value| value.as_str())
                        .map(str::to_string),
                    _ => None,
                });
            if let Some(relative) = declared {
                let candidate = package.join(relative.trim_start_matches("./"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    for fallback in ASTRO_BIN_FALLBACKS {
        let candidate = package.join(fallback);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The target triple this build was compiled for.
///
/// `std::env::consts` cannot produce it (it has the architecture and the OS but not the vendor or the ABI),
/// so the few triples a release supports are named. An unknown triple is reported rather than guessed,
/// because guessing would produce a path that silently does not exist.
pub fn target_triple() -> Result<&'static str> {
    #[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"))]
    {
        return Ok("x86_64-pc-windows-msvc");
    }
    #[cfg(all(target_arch = "aarch64", target_os = "windows", target_env = "msvc"))]
    {
        return Ok("aarch64-pc-windows-msvc");
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        return Ok("x86_64-apple-darwin");
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        return Ok("aarch64-apple-darwin");
    }
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
    {
        return Ok("x86_64-unknown-linux-gnu");
    }
    #[allow(unreachable_code)]
    Err(VeloctError::NotConfigured {
        what: "the bundled Node runtime".to_string(),
        hint: Some(format!(
            "this build targets {}-{}, which has no pinned runtime; add one to the release pipeline",
            std::env::consts::ARCH,
            std::env::consts::OS
        )),
    })
}

/// The bundled runtime's file name, `veloct-node-<triple>.exe` on Windows.
///
/// This is the name a Cargo build tree and a Tauri bundle SOURCE use. An installed bundle renames it
/// — see [`node_bundled_file_name`] — so both names travel together through
/// [`node_file_names`].
pub fn node_file_name(triple: &str) -> String {
    if cfg!(windows) {
        format!("{NODE_SIDECAR_STEM}-{triple}.exe")
    } else {
        format!("{NODE_SIDECAR_STEM}-{triple}")
    }
}

/// The name an INSTALLED Tauri bundle gives the runtime, with the target triple stripped.
pub fn node_bundled_file_name() -> String {
    if cfg!(windows) {
        format!("{NODE_SIDECAR_STEM}.exe")
    } else {
        NODE_SIDECAR_STEM.to_string()
    }
}

/// Both names a bundled runtime is found under, in the order they should be tried.
pub fn node_file_names(triple: &str) -> Vec<String> {
    let mut names = vec![node_file_name(triple)];
    let bundled = node_bundled_file_name();
    if !names.contains(&bundled) {
        names.push(bundled);
    }
    names
}

/// Find the bundled Node runtime.
///
/// The search order is deliberate and has no `PATH` in it:
///
/// 1. `VELOCT_NODE`, an explicit path, for a build tree or a test.
/// 2. Beside the sidecar executable, which is where Tauri puts an `externalBin` at runtime.
/// 3. One directory up, which is where Tauri puts resources relative to a binary in `binaries/`.
///
/// **BOTH names are tried, and the second one is not a nicety.** A Tauri bundle RENAMES an
/// `externalBin`: the file is built and shipped as `binaries/veloct-node-x86_64-pc-windows-msvc.exe`
/// (the `-<triple>` suffix is what a Cargo build tree needs) but it is INSTALLED as
/// `veloct-node.exe`, with the triple stripped. Measured against a real
/// `Veloct_0.1.0_x64-setup.exe`: the installed directory holds `veloct-astro-sidecar.exe`,
/// `veloct-desktop.exe` and `veloct-node.exe`. Searching only the triple-suffixed name therefore
/// finds the runtime in a development tree and NEVER in an installed application — a preview that
/// works for the person who built it and fails for the person who installed it, which ADR-0005's
/// "never from `PATH`" rule exists to prevent in the other direction.
pub fn resolve_node(beside: &Path) -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os(NODE_ENV) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Ok(path);
        }
        return Err(VeloctError::NotConfigured {
            what: "the bundled Node runtime".to_string(),
            hint: Some(format!(
                "{NODE_ENV} is set to {} but that is not a file",
                path.display()
            )),
        });
    }

    let triple = target_triple()?;
    let names = node_file_names(triple);

    let mut candidates: Vec<PathBuf> = Vec::new();
    // Beside the executable.
    if let Some(directory) = beside.parent() {
        for name in &names {
            candidates.push(directory.join(name));
        }
        // One level up: a `binaries/` subdirectory beside resources.
        if let Some(parent) = directory.parent() {
            for name in &names {
                candidates.push(parent.join(name));
                candidates.push(parent.join("binaries").join(name));
            }
        }
    }
    for name in &names {
        candidates.push(beside.join(name));
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(VeloctError::NotConfigured {
        what: "the bundled Node runtime".to_string(),
        hint: Some(format!(
            "expected {} beside the application; looked in {}",
            names.join(" or "),
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    })
}

/// The working directory a preview should run in, and the Astro entry point inside it.
///
/// The project root comes from `VELOCT_PROJECT_ROOT`, which is the same override the site template already
/// reads for its own content, so the launcher and the site agree about where the project is by construction
/// rather than by two independent guesses.
pub fn resolve_site(project_root: Option<&Path>, sidecar_directory: &Path) -> Result<(PathBuf, PathBuf)> {
    // The project's own `site/`, which is where a user's template lives.
    let project_site = project_root.map(|root| root.join("site"));
    if let Some(site) = &project_site {
        if let Some(entry) = astro_entry_in(site) {
            return Ok((site.clone(), entry));
        }
    }

    // The template shipped beside the application, for a project that has none.
    let bundled = sidecar_directory.join("site");
    if let Some(entry) = astro_entry_in(&bundled) {
        return Ok((bundled, entry));
    }

    Err(VeloctError::NotConfigured {
        what: "the Astro template".to_string(),
        hint: Some(format!(
            "no {ASTRO_PACKAGE}/bin/astro.mjs was found under {} or {}; the site needs its dependencies installed",
            project_site
                .map(|site| site.display().to_string())
                .unwrap_or_else(|| "(no VELOCT_PROJECT_ROOT)".to_string()),
            bundled.display()
        )),
    })
}

/// The full command line for a preview.
///
/// Node runs the Astro CLI directly rather than through `npm`, because `npm` would add a shell, a package
/// manifest lookup and an `npm` that is not bundled: ADR-0005 ships Node, not npm.
pub fn command_line(node: &Path, astro_entry: &Path, args: &[String]) -> (PathBuf, Vec<String>) {
    let mut full = vec![astro_entry.to_string_lossy().to_string()];
    full.extend(args.iter().cloned());
    (node.to_path_buf(), full)
}

/// Whether the arguments name a preview subcommand this launcher supports.
pub fn is_preview_command(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("dev" | "preview"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests that touch `VELOCT_NODE`.
    ///
    /// This is a REAL requirement rather than tidiness: Cargo runs tests in parallel within one binary, and
    /// the environment is process-wide, so a test that sets the variable races a test that removes it. The
    /// first version of this module had no lock and `a_runtime_beside_the_executable_is_found` failed with a
    /// path from a different test's temp directory.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Run a closure with the environment variable cleared, holding the lock.
    fn without_env<T>(body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::remove_var(NODE_ENV);
        body()
    }

    /// Run a closure with the environment variable set, holding the lock.
    fn with_env<T>(value: &Path, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        std::env::set_var(NODE_ENV, value);
        let result = body();
        std::env::remove_var(NODE_ENV);
        result
    }

    #[test]
    fn the_target_triple_is_known_on_this_host() {
        // The test host is one of the supported triples, so this asserts the mapping exists rather than
        // asserting which one.
        let triple = target_triple().expect("a known triple");
        assert!(triple.contains('-'), "{triple}");
        assert!(triple.contains(std::env::consts::ARCH), "{triple}");
    }

    #[test]
    fn the_node_file_name_carries_the_triple() {
        let name = node_file_name("x86_64-pc-windows-msvc");
        assert!(name.starts_with("veloct-node-"));
        assert!(name.contains("x86_64-pc-windows-msvc"));
        if cfg!(windows) {
            assert!(name.ends_with(".exe"), "{name}");
        }
    }

    #[test]
    fn an_explicit_override_is_honoured_when_it_points_at_a_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let fake = dir.path().join("node");
        std::fs::write(&fake, b"not really node").unwrap();
        let resolved = with_env(&fake, || resolve_node(&dir.path().join("sidecar")).unwrap());
        assert_eq!(resolved, fake);
    }

    #[test]
    fn an_override_pointing_at_nothing_is_reported_rather_than_ignored() {
        // Silently falling through to a search would hide a typo in the override.
        let error = with_env(Path::new("D:/definitely/not/here/node.exe"), || {
            resolve_node(Path::new("D:/app/sidecar")).unwrap_err()
        });
        assert!(matches!(error, VeloctError::NotConfigured { .. }));
        // The hint is a separate field rather than part of `Display`, so it is read through the accessor.
        // Checking `to_string()` here was the first version of this test and it was wrong for that reason.
        let hint = error.hint().expect("a hint");
        assert!(hint.contains(NODE_ENV), "{hint}");
        std::env::remove_var(NODE_ENV);
    }

    #[test]
    fn a_missing_runtime_names_where_it_looked_and_never_falls_back_to_path() {
        // ADR-0005: the app resolves node from the sidecar path, NEVER from PATH. A PATH fallback would make
        // the application work on a developer's machine and fail on a user's.
        let dir = tempfile::TempDir::new().unwrap();
        let error = without_env(|| resolve_node(&dir.path().join("veloct-astro-sidecar")).unwrap_err());
        assert_eq!(error.code(), "NOT_CONFIGURED");
        let hint = error.hint().expect("a hint");
        assert!(hint.contains("beside the application"), "{hint}");
        assert!(hint.contains("veloct-node-"), "{hint}");
        // And it says where it looked, so the user can act on it.
        assert!(hint.contains(&dir.path().display().to_string()), "{hint}");
    }

    #[test]
    fn a_runtime_beside_the_executable_is_found() {
        let dir = tempfile::TempDir::new().unwrap();
        let triple = target_triple().unwrap();
        let sidecar = dir.path().join("veloct-astro-sidecar");
        let node = dir.path().join(node_file_name(triple));
        std::fs::write(&node, b"fake").unwrap();
        let resolved = without_env(|| resolve_node(&sidecar).unwrap());
        assert_eq!(resolved, node);
    }

    #[test]
    fn a_runtime_under_the_name_an_installed_bundle_gives_it_is_found() {
        // Measured, not assumed: a Tauri NSIS bundle installs the runtime as `veloct-node.exe`, with the
        // target triple STRIPPED — `binaries/veloct-node-x86_64-pc-windows-msvc.exe` in the build tree
        // becomes `veloct-node.exe` in the install directory, beside `veloct-desktop.exe`. Searching only
        // the triple-suffixed name found the runtime in a development tree and NEVER in an installed app.
        let dir = tempfile::TempDir::new().unwrap();
        let sidecar = dir.path().join("veloct-astro-sidecar");
        let node = dir.path().join(node_bundled_file_name());
        std::fs::write(&node, b"fake").unwrap();
        let resolved = without_env(|| resolve_node(&sidecar).unwrap());
        assert_eq!(resolved, node);
    }

    #[test]
    fn both_runtime_names_are_tried_and_the_triple_suffixed_one_comes_first() {
        let triple = target_triple().unwrap();
        let names = node_file_names(triple);
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(names[0], node_file_name(triple));
        assert_eq!(names[1], node_bundled_file_name());
        // A build tree uses the suffixed name, so it must win when both exist: two runtimes in one
        // directory is a mistake, and picking the wrong one silently is worse than picking a defined one.
        let dir = tempfile::TempDir::new().unwrap();
        let sidecar = dir.path().join("veloct-astro-sidecar");
        let suffixed = dir.path().join(node_file_name(triple));
        let bundled = dir.path().join(node_bundled_file_name());
        std::fs::write(&suffixed, b"suffixed").unwrap();
        std::fs::write(&bundled, b"bundled").unwrap();
        assert_eq!(without_env(|| resolve_node(&sidecar).unwrap()), suffixed);
    }

    /// Install a fake Astro package whose `package.json` declares `bin`.
    fn install_astro(site: &Path, declared: &str) -> PathBuf {
        let package = site.join(ASTRO_PACKAGE);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            format!(r#"{{ "name": "astro", "bin": {{ "astro": "./{declared}" }} }}"#),
        )
        .unwrap();
        let entry = package.join(declared);
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "// astro").unwrap();
        entry
    }

    #[test]
    fn the_astro_entry_comes_from_the_packages_own_bin_field() {
        // The first version of this module hardcoded `node_modules/astro/astro.js`, which does not exist:
        // the real entry is `bin/astro.mjs`, and a site with its dependencies installed perfectly well was
        // reported as missing its template.
        let dir = tempfile::TempDir::new().unwrap();
        let entry = install_astro(dir.path(), "dist/cli.js");
        assert_eq!(astro_entry_in(dir.path()).unwrap(), entry);
    }

    #[test]
    fn a_package_with_a_string_bin_field_is_understood() {
        let dir = tempfile::TempDir::new().unwrap();
        let package = dir.path().join(ASTRO_PACKAGE);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{ "name": "astro", "bin": "./bin/astro.mjs" }"#,
        )
        .unwrap();
        let entry = package.join("bin/astro.mjs");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, "// astro").unwrap();
        assert_eq!(astro_entry_in(dir.path()).unwrap(), entry);
    }

    #[test]
    fn a_package_whose_declared_bin_is_missing_falls_back_to_the_known_names() {
        // A site may have an older Astro pinned, or a manifest that names a file it does not ship.
        let dir = tempfile::TempDir::new().unwrap();
        let package = dir.path().join(ASTRO_PACKAGE);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{ "name": "astro", "bin": { "astro": "./bin/gone.mjs" } }"#,
        )
        .unwrap();
        let entry = package.join("astro.js");
        std::fs::write(&entry, "// older astro").unwrap();
        assert_eq!(astro_entry_in(dir.path()).unwrap(), entry);
    }

    #[test]
    fn a_package_directory_with_no_entry_at_all_reports_none() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(ASTRO_PACKAGE)).unwrap();
        assert!(astro_entry_in(dir.path()).is_none());
        assert!(astro_entry_in(&dir.path().join("nowhere")).is_none());
    }

    #[test]
    fn the_site_is_taken_from_the_project_when_it_has_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let project = dir.path().join("project");
        let entry = install_astro(&project.join("site"), "bin/astro.mjs");
        let (site, resolved) = resolve_site(Some(&project), dir.path()).unwrap();
        assert_eq!(site, project.join("site"));
        assert_eq!(resolved, entry);
    }

    #[test]
    fn a_project_with_no_template_falls_back_to_the_bundled_one() {
        let dir = tempfile::TempDir::new().unwrap();
        let bundled_entry = install_astro(&dir.path().join("site"), "bin/astro.mjs");

        let project = dir.path().join("empty-project");
        std::fs::create_dir_all(&project).unwrap();
        let (site, resolved) = resolve_site(Some(&project), dir.path()).unwrap();
        assert_eq!(site, dir.path().join("site"));
        assert_eq!(resolved, bundled_entry);
    }

    #[test]
    fn a_site_with_no_dependencies_is_reported_with_what_to_do() {
        // "Cannot find module astro" from Node would send the user looking in the wrong place.
        let dir = tempfile::TempDir::new().unwrap();
        let error = resolve_site(None, dir.path()).unwrap_err();
        assert!(error.to_string().contains("Astro template"), "{error}");
        let hint = error.hint().expect("a hint");
        assert!(hint.contains("dependencies installed"), "{hint}");
    }

    #[test]
    fn the_command_runs_the_astro_cli_through_the_bundled_node_and_not_through_npm() {
        // ADR-0005 ships Node, not npm, so going through npm would need something that is not bundled.
        let (program, args) = command_line(
            Path::new("D:/app/veloct-node.exe"),
            Path::new("D:/p/site/node_modules/astro/astro.js"),
            &[
                "dev".to_string(),
                "--port".to_string(),
                "4321".to_string(),
            ],
        );
        assert!(program.ends_with("veloct-node.exe"), "{}", program.display());
        assert_eq!(args[0], "D:/p/site/node_modules/astro/astro.js");
        assert_eq!(args[1], "dev");
        assert_eq!(args[3], "4321");
        // No shell, no npm.
        assert!(!args.iter().any(|arg| arg.contains("npm")));
    }

    #[test]
    fn the_preview_commands_are_the_two_astro_serves_the_shell_starts() {
        let dev = vec!["dev".to_string(), "--port".to_string()];
        assert!(is_preview_command(&dev));
        assert!(is_preview_command(&["preview".to_string()]));
        assert!(!is_preview_command(&["build".to_string()]));
        assert!(!is_preview_command(&[]));
    }
}
