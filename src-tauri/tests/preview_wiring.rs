//! The shipped preview wiring, asserted on the state the application actually builds.
//!
//! **This file exists because of a defect that no unit test could have caught.** `lib.rs` built the
//! application state with `state::NullSpawner`, whose every `spawn_preview` returns an error, so
//! "start preview" could never work in a shipped build — and the suite was green, because every test
//! either injected a fake spawner or built its own `PreviewSpec` and never asked what the
//! APPLICATION was wired to. The defect was in the wiring, so the test has to be about the wiring.
//!
//! Two tests, deliberately different in kind:
//!
//! * [`the_shipped_application_is_wired_to_a_spawner_that_spawns`] is the structural one. It builds
//!   `application_state()` — the very function `run` calls, so there is no second code path to drift
//!   — and fails if the spawner in it cannot spawn. It is fast, it runs in the default suite, and it
//!   is the regression net for the exact mistake that shipped.
//! * [`the_real_spawner_starts_the_site_preview_and_the_port_answers`] is the real-process one. It
//!   runs the actual Astro dev server through `PreviewSupervisor` with `OsSpawner`, requires the
//!   port to answer HTTP, and then requires the kill to leave nothing behind. It is `#[ignore]`d
//!   because it needs the built sidecar, the bundled Node runtime and the site's installed
//!   dependencies — a machine without those has an environmental failure, not a code failure. Run it
//!   with `--ignored`.
//!
//! What neither test does is assert a message. A preview that fails with a good message is still a
//! broken preview, which is the whole point of the defect.

use std::path::{Path, PathBuf};
use std::time::Duration;

use veloct_desktop_lib::{application_state, resolve_launcher};
use veloct_process::port::{is_http_ready, reserve_local_port};
use veloct_process::{PreviewRole, PreviewSpec};

/// The structural claim: the application's spawner spawns.
#[test]
fn the_shipped_application_is_wired_to_a_spawner_that_spawns() {
    let state = application_state();

    assert!(
        state.can_spawn_previews(),
        "the shipped application is wired to {}, which reports that it cannot start a process, so \
         `start_preview` would fail for every user. `lib.rs::application_state` must install \
         `OsSpawner`; `NullSpawner` belongs only to `AppState::headless`.",
        state.spawner_kind()
    );
    // Spelled as a suffix rather than compared to `type_name::<OsSpawner>()`: `kind()` returns the
    // concise name on purpose (a failure message should not read `veloct_desktop_lib::state::...`),
    // and the suffix is what still fails if the kind stops being the OS spawner.
    assert!(
        state.spawner_kind().ends_with("OsSpawner"),
        "the application must install the real OS spawner, got {}",
        state.spawner_kind()
    );
}

/// The behavioural claim, on this host: a real Astro dev server answers on its port.
///
/// `#[ignore]`d because it needs the built sidecar (`scripts/build-sidecar.ps1`), the bundled Node
/// runtime and the site's installed dependencies. Nothing about it is mocked: the supervisor runs the
/// same `OsSpawner` the application installs, the launcher is the built binary, and readiness is a
/// real HTTP response.
#[test]
#[ignore = "starts a real Astro dev server; needs the built sidecar and the site's dependencies"]
fn the_real_spawner_starts_the_site_preview_and_the_port_answers() {
    let root = repository_root();
    let launcher = launcher_in(&root);
    let site = root.join("site");
    assert!(
        site.join("node_modules").join("astro").is_dir(),
        "the site template at {} has no installed dependencies; run `npm install` in site/ first",
        site.display()
    );
    let port = reserve_port();

    // The launcher resolves the project from `VELOCT_PROJECT_ROOT` and the template from the project's
    // own `site/` directory first — that is `veloct_sidecar::resolve_site`'s documented order, and it
    // is the same variable the desktop application's child inherits. It MUST be set here: without it
    // the launcher looks for a template beside itself (`src-tauri/binaries/site`), which no development
    // tree has, and reports "the Astro template ... the site needs its dependencies installed" rather
    // than starting anything.
    //
    // Process-wide, which is why this test is `#[ignore]`d and must be run serially
    // (`-- --ignored --test-threads=1`).
    std::env::set_var("VELOCT_PROJECT_ROOT", &root);

    // The application's OWN state, built by the function `run` calls, so the supervisor under test
    // is the one that ships: `OsSpawner`, `HttpReadinessProbe` and `SupervisorConfig::default`.
    let state = application_state();
    let supervisor = state.previews();
    let spec = PreviewSpec {
        role: PreviewRole::Site,
        program: launcher.to_string_lossy().to_string(),
        // Exactly the arguments `AppState::start_site_preview` builds, on a port reserved the way it
        // reserves one.
        args: vec![
            "dev".to_string(),
            "--port".to_string(),
            port.to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
        ],
        working_directory: site,
        env: Default::default(),
        port,
    };

    // The launcher, judged before it is run: `resolve_launcher` is the function the spawner uses, so
    // a name this test accepts is a name the spawner accepts.
    assert!(
        launcher.is_file(),
        "the launcher must be a real file, got {}",
        launcher.display()
    );

    let started = supervisor.start(&spec);
    // Removed before anything is asserted, so a failure cannot leave the variable set for whatever
    // this process runs next.
    std::env::remove_var("VELOCT_PROJECT_ROOT");

    let started = match started {
        Ok(state) => state,
        Err(error) => panic!(
            "the supervisor could not start the preview: {error}\n  launcher: {}",
            launcher.display()
        ),
    };

    // The port answers, which is what `preview_url` promises the iframe. Asserted with the probe's
    // own primitive rather than on the reported state alone, so a supervisor that claimed `Running`
    // without a server behind it could not pass this.
    let answered = is_http_ready(port, Duration::from_millis(500));
    let pid = started.pid();

    // Teardown BEFORE the assertions about the run, so a failing expectation cannot leak a Node
    // process holding the port for the rest of the session.
    let stopped = match supervisor.stop(PreviewRole::Site) {
        Ok(state) => state,
        Err(error) => panic!("the preview could not be stopped: {error}"),
    };

    assert!(
        started.is_running(),
        "the preview must reach Running, got {started:?}"
    );
    assert!(
        answered,
        "the site preview was reported running on port {port} but nothing answered HTTP there; the \
         supervisor's readiness probe and this probe agree, so this should be impossible"
    );
    assert!(
        pid.unwrap_or(0) > 0,
        "the preview must report the pid it started, got {pid:?}"
    );
    assert!(
        !supervisor.has_live_children(),
        "the stopped preview must not leave a child behind"
    );
    assert!(
        !is_http_ready(port, Duration::from_millis(500)),
        "port {port} is still answering after the preview was stopped, so the kill left a server \
         holding it (state: {stopped:?})"
    );
}

/// The application's own preview command, end to end: `start_site_preview` must reach Running and
/// answer on the URL it reports.
///
/// This is the layer the user actually touched. The supervisor test above proves the process starts;
/// this proves the command that was broken — `AppState::start_site_preview`, which selects the launcher
/// from the resolved sidecar path, reserves the port and builds the arguments — produces a live
/// preview. It is `#[ignore]`d for the same environmental reasons as the test above.
#[test]
#[ignore = "starts a real Astro dev server; needs the built sidecar and the site's dependencies"]
fn the_application_preview_command_reaches_running_and_serves_a_url() {
    let root = repository_root();
    let launcher = launcher_in(&root);
    assert!(
        root.join("site").join("node_modules").join("astro").is_dir(),
        "the site template has no installed dependencies; run `npm install` in site/ first"
    );

    // The same variable `lib.rs`'s setup records for the project, and the same one the launcher reads
    // to find the project rather than a template beside itself. Removed before anything is asserted.
    std::env::set_var("VELOCT_PROJECT_ROOT", &root);

    let state = application_state();
    let project_dir = std::env::temp_dir().join(format!(
        "veloct-preview-command-{}",
        std::process::id()
    ));
    // `start_site_preview` runs the launcher with `<project>/site` as its working directory, and
    // `Command` requires that directory to exist. A real project has it; this one has to be given it.
    std::fs::create_dir_all(project_dir.join("site"))
        .unwrap_or_else(|error| panic!("could not create {}: {error}", project_dir.display()));
    state
        .set_project(
            veloct_desktop_lib::OpenProject::open(&project_dir, "2026-01-01T00:00:00Z")
                .unwrap_or_else(|error| panic!("the project could not be opened: {error}")),
        )
        .unwrap_or_else(|error| panic!("the project could not be set: {error}"));
    // Exactly what `lib.rs`'s setup does with the resolved resource directory.
    state.set_sidecar(launcher.to_string_lossy().to_string());

    let started = state.start_site_preview();
    std::env::remove_var("VELOCT_PROJECT_ROOT");

    let started = match started {
        Ok(state) => state,
        Err(error) => panic!(
            "start_site_preview failed with {}: {} (hint {:?})",
            error.code, error.message, error.hint
        ),
    };
    let port = started
        .port()
        .unwrap_or_else(|| panic!("a running preview must report its port: {started:?}"));
    let url = state.preview_url(PreviewRole::Site);
    let answered = is_http_ready(port, Duration::from_millis(500));

    let stopped = state.shutdown();

    assert!(
        started.is_running(),
        "start_site_preview must reach Running, got {started:?}"
    );
    assert_eq!(
        url.as_deref(),
        Some(format!("http://127.0.0.1:{port}/").as_str()),
        "the iframe URL must point at the port that answers"
    );
    assert!(
        answered,
        "the URL {url:?} was reported but nothing answered HTTP on port {port}"
    );
    assert!(
        stopped.is_ok(),
        "shutdown must stop the preview: {stopped:?}"
    );
    assert!(
        !is_http_ready(port, Duration::from_millis(500)),
        "port {port} is still answering after shutdown, so the kill left a server holding it"
    );

    let _ = std::fs::remove_dir_all(&project_dir);
}

/// A port reserved the way the application reserves one.
fn reserve_port() -> u16 {
    reserve_local_port()
        .expect("the OS has a free loopback port")
        .as_u16()
}

/// The repository root, from this test's own location.
fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .to_path_buf()
}

/// The built launcher, resolved through the same function the spawner uses.
fn launcher_in(root: &Path) -> PathBuf {
    let binaries = root.join("src-tauri").join("binaries");
    let plain = binaries.join("veloct-astro-sidecar");
    if plain.is_file() {
        return plain;
    }
    let suffixed = if cfg!(windows) {
        binaries.join("veloct-astro-sidecar-x86_64-pc-windows-msvc.exe")
    } else {
        binaries.join("veloct-astro-sidecar-x86_64-unknown-linux-gnu")
    };
    assert!(
        suffixed.is_file(),
        "no preview launcher at {} or {}; run `scripts/build-sidecar.ps1` first",
        plain.display(),
        suffixed.display()
    );
    // Resolved through the spawner's own resolver, so the test and the spawner cannot disagree about
    // which file that name means.
    resolve_launcher(&suffixed.to_string_lossy())
        .unwrap_or_else(|error| panic!("the launcher that exists did not resolve: {error}"))
}
