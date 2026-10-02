//! The one-click dependency install, end to end.
//!
//! Four tests, deliberately different in kind, and the difference is what each one is worth:
//!
//! * [`a_resolved_npm_is_a_program_the_spawner_can_launch`] is structural and instant. Two
//!   independent steps have to agree for a click to work at all: `resolve_npm` finds a file and
//!   `OsSpawner`'s `resolve_launcher` accepts it as the program to run. A path the resolver returned
//!   and the spawner refused would fail only at the user's first click.
//! * [`the_one_click_install_runs_a_real_process_and_reports_its_output`] and
//!   [`cancelling_the_install_kills_the_tree_it_started`] run a REAL process through the real
//!   `OsSpawner`, with a STUB npm (a generated `.cmd`). What they exercise is exactly the machinery
//!   the feature depends on: a Windows `.cmd` launched through `std::process::Command`, stdout and
//!   stderr drained into the tail, the death and the exit code observed by polling, the artefact
//!   check that decides success, and `taskkill /T` reaching the descendants. They need no network,
//!   which is why they are NOT ignored — a test that cannot run in the default suite protects
//!   nothing. What they deliberately do NOT cover is npm itself.
//! * [`the_real_npm_install_puts_the_astro_cli_where_the_build_looks`] is the real thing: the system
//!   npm, the real registry, the real `astro`. It is `#[ignore]`d because it is slow and needs the
//!   network — an environmental dependency, not a convenience. Run it with
//!   `cargo test -p veloct-desktop --test site_install -- --ignored`.
//!
//! These tests set `VELOCT_NPM`, which is process-wide, so every one of them holds [`ENV_LOCK`]; and
//! they use `application_state()`, the very function `run` calls, so what they drive is the wiring
//! the application ships rather than a state built for a test.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use veloct_desktop_lib::{
    application_state, resolve_launcher, resolve_npm, AppState, OpenProject, SiteInstallState,
    SiteInstallView, NPM_ENV,
};

/// Serialises the tests in this binary that set `VELOCT_NPM`.
///
/// Cargo runs the tests in one binary on parallel threads while the environment is process-wide, so
/// without this two tests would overwrite each other's stub npm. Every mutation below happens while
/// this guard is held.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Write a stub "npm" (a `.cmd`, which is what a real npm is on Windows) and return its path.
///
/// A `.cmd` and not an `.exe` on purpose: npm IS a batch shim over `node`, so this is the same
/// launch path a real install takes — including whether `std::process::Command` can run a batch file
/// at all, which is a genuine question with a whole feature riding on the answer.
fn write_stub_npm(root: &Path, body: &str) -> PathBuf {
    let path = root.join("stub-npm.cmd");
    std::fs::write(&path, body).unwrap_or_else(|error| {
        panic!("could not write the stub npm at {}: {error}", path.display())
    });
    path
}

/// Open a project whose `site/` exists, with an in-memory log so the test writes nothing anywhere.
fn project_with_site(root: &Path) -> std::sync::Arc<OpenProject> {
    std::fs::create_dir_all(root.join("site")).expect("the site directory is creatable");
    // `with_log` is infallible by construction: it expects the attachment state to open, which is
    // what makes it the test constructor rather than the fallible one.
    OpenProject::with_log(
        root,
        "2026-01-01T00:00:00Z",
        Box::new(veloct_logging::MemorySink::new(64)),
    )
}

/// Poll the install until `done` says so, or the deadline expires.
///
/// The report is returned either way: a test that timed out asserts on what it GOT, tail included,
/// rather than on "it did not finish".
fn wait_until(
    state: &AppState,
    timeout: Duration,
    done: impl Fn(&SiteInstallView) -> bool,
) -> SiteInstallView {
    let deadline = Instant::now() + timeout;
    loop {
        let view = state
            .site_install_view()
            .expect("the install state is readable");
        if done(&view) || Instant::now() >= deadline {
            return view;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Poll until the install leaves `Running`.
fn wait_for_terminal(state: &AppState, timeout: Duration) -> SiteInstallView {
    wait_until(state, timeout, |view| {
        !matches!(view.state, SiteInstallState::Running { .. })
    })
}

#[test]
fn a_resolved_npm_is_a_program_the_spawner_can_launch() {
    let _guard = lock_env();
    let dir = tempfile::TempDir::new().unwrap();
    let npm = write_stub_npm(dir.path(), "@echo off\r\nexit /b 0\r\n");
    std::env::set_var(NPM_ENV, &npm);

    let resolved = resolve_npm().expect("the override names a real file");
    let launched = resolve_launcher(&resolved.to_string_lossy());

    std::env::remove_var(NPM_ENV);

    let launched = launched.unwrap_or_else(|error| {
        panic!(
            "the spawner refused the npm the resolver found at {}: {error}",
            resolved.display()
        )
    });
    assert_eq!(
        launched, resolved,
        "the two steps must agree about which file is npm"
    );
}

#[test]
fn the_one_click_install_runs_a_real_process_and_reports_its_output() {
    let _guard = lock_env();
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let project = project_with_site(root);

    // The stub writes the Astro package the BUILD looks for, says something on stdout, and exits 0 —
    // the same contract npm has. Nothing about it is mocked below this line: the `.cmd` is launched
    // by `OsSpawner`, its output is drained by the reader threads, and its death is discovered by
    // polling the handle.
    let npm = write_stub_npm(
        root,
        "@echo off\r\n\
         echo veloct-install-probe-start\r\n\
         if not exist node_modules\\astro\\bin mkdir node_modules\\astro\\bin\r\n\
         echo { \"name\": \"astro\", \"bin\": { \"astro\": \"./bin/astro.mjs\" } }> node_modules\\astro\\package.json\r\n\
         echo // astro> node_modules\\astro\\bin\\astro.mjs\r\n\
         echo veloct-install-probe-finish added 213 packages\r\n\
         exit /b 0\r\n",
    );
    std::env::set_var(NPM_ENV, &npm);

    let state = application_state();
    state
        .set_project(project)
        .expect("the project is settable");

    let started = state.start_site_install().expect("the install starts");
    let pid = match started.state {
        SiteInstallState::Running { pid, .. } => pid,
        ref other => {
            std::env::remove_var(NPM_ENV);
            panic!("expected Running, got {other:?}");
        }
    };

    let view = wait_for_terminal(&state, Duration::from_secs(60));
    std::env::remove_var(NPM_ENV);

    assert!(pid > 0, "the install reports the pid it started");
    assert!(
        view.site_dir.ends_with("site"),
        "the install runs in the project's site: {}",
        view.site_dir
    );
    match view.state {
        SiteInstallState::Succeeded { exit_code, .. } => {
            assert_eq!(exit_code, 0, "see the tail:\n{}", view.output_tail)
        }
        ref other => panic!(
            "expected Succeeded, got {other:?}; the installer said:\n{}",
            view.output_tail
        ),
    }
    assert!(
        view.astro_ready,
        "the build must find its Astro CLI under {}",
        view.site_dir
    );
    // Both ends of the output, so "the tail is captured" cannot be satisfied by one late line.
    assert!(
        view.output_tail.contains("veloct-install-probe-start"),
        "the beginning of the output is kept: {}",
        view.output_tail
    );
    assert!(
        view.output_tail.contains("veloct-install-probe-finish"),
        "the end of the output is kept: {}",
        view.output_tail
    );
}

#[test]
fn cancelling_the_install_kills_the_tree_it_started() {
    let _guard = lock_env();
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let site = root.join("site");
    let project = project_with_site(root);

    // A DESCENDANT writes the Astro package, after its own sleep, and leaves a marker. That is what
    // makes this test about the TREE rather than about the direct child: killing only the `cmd.exe`
    // running the stub would let the grandchild finish and leave exactly the half-installed
    // `node_modules` the cancel path warns about.
    std::fs::write(
        root.join("late-writer.cmd"),
        "@echo off\r\n\
         ping -n 6 127.0.0.1 >nul\r\n\
         if not exist node_modules\\astro\\bin mkdir node_modules\\astro\\bin\r\n\
         echo { \"name\": \"astro\", \"bin\": { \"astro\": \"./bin/astro.mjs\" } }> node_modules\\astro\\package.json\r\n\
         echo late> late-marker.txt\r\n",
    )
    .unwrap();

    let npm = write_stub_npm(
        root,
        "@echo off\r\n\
         echo veloct-install-probe-sleeping\r\n\
         start /b \"\" \"%~dp0late-writer.cmd\"\r\n\
         ping -n 6 127.0.0.1 >nul\r\n\
         echo veloct-install-probe-finish\r\n\
         exit /b 0\r\n",
    );
    std::env::set_var(NPM_ENV, &npm);

    let state = application_state();
    state
        .set_project(project)
        .expect("the project is settable");

    state.start_site_install().expect("the install starts");
    let running = wait_until(&state, Duration::from_secs(30), |view| {
        view.output_tail.contains("veloct-install-probe-sleeping")
    });
    assert!(
        running.output_tail.contains("veloct-install-probe-sleeping"),
        "the stub install must have started before it is cancelled: {}",
        running.output_tail
    );

    let cancelled = state.cancel_site_install().expect("cancelling is not an error");
    std::env::remove_var(NPM_ENV);

    assert!(
        matches!(cancelled.state, SiteInstallState::Cancelled { .. }),
        "{:?}",
        cancelled.state
    );
    assert!(
        !cancelled.astro_ready,
        "a cancelled install has not installed anything"
    );

    // Longer than the writers' remaining sleep, so a descendant that survived the kill WOULD have
    // written by now.
    std::thread::sleep(Duration::from_secs(8));
    assert!(
        !site.join("node_modules").exists(),
        "the killed tree still produced node_modules, so the cancel did not reach it"
    );
    assert!(
        !root.join("late-marker.txt").exists(),
        "a DESCENDANT survived the kill, which is the half-installed mess a cancel must prevent"
    );
}

#[test]
#[ignore = "downloads astro from the real npm registry; needs a system npm and the network"]
fn the_real_npm_install_puts_the_astro_cli_where_the_build_looks() {
    let _guard = lock_env();
    // No `VELOCT_NPM`: the point of this test is the SYSTEM npm the feature falls back to.
    std::env::remove_var(NPM_ENV);

    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let site = root.join("site");
    let project = project_with_site(root);
    std::fs::write(
        site.join("package.json"),
        "{\n  \"name\": \"veloct-install-probe\",\n  \"version\": \"1.0.0\",\n  \"private\": true,\n  \"dependencies\": { \"astro\": \"7.3.5\" }\n}\n",
    )
    .unwrap();

    let npm = resolve_npm().unwrap_or_else(|| {
        panic!("no system npm was found on PATH; this test needs Node.js installed ({NPM_ENV} can name one)")
    });

    let state = application_state();
    state
        .set_project(project)
        .expect("the project is settable");

    let started = state.start_site_install().expect("the install starts");
    assert!(
        matches!(started.state, SiteInstallState::Running { .. }),
        "{:?}",
        started.state
    );
    assert_eq!(
        started.npm.as_deref(),
        Some(npm.to_string_lossy().replace('\\', "/").as_str()),
        "the view reports the npm that is actually running"
    );

    // npm over a cold cache is minutes, not seconds: the timeout is generous on purpose, and the
    // failure message carries the installer's own last lines.
    let view = wait_for_terminal(&state, Duration::from_secs(900));
    match view.state {
        SiteInstallState::Succeeded { exit_code, .. } => {
            assert_eq!(exit_code, 0, "see the tail:\n{}", view.output_tail)
        }
        ref other => panic!(
            "npm install did not succeed ({other:?}); the installer said:\n{}",
            view.output_tail
        ),
    }
    assert!(
        view.astro_ready,
        "the build's own check must find an Astro CLI at {}",
        site.join("node_modules").join("astro").display()
    );
    assert!(
        veloct_sidecar::astro_entry_in(&site).is_some(),
        "the build's resolver, asked directly, must find the CLI the install produced"
    );
}
