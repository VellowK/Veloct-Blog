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
//!
//! [`the_works_preview_serves_the_built_works_root_on_its_own_origin`] covers the SAME defect's second
//! half, on the other role. Once the application was wired to a real spawner the works preview reached
//! the launcher and was refused there instead (`expected dev or preview, got "serve"`), because
//! `<project>/dist/works` is build output that no Astro command serves. It goes through
//! `AppState::start_works_preview`, requires the port to answer AND to return the built bytes, and
//! requires `shutdown` to take the server down.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
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

/// The works preview, end to end: `start_works_preview` must reach Running, serve the project's BUILT
/// works files on its own origin, and stop answering after `shutdown`.
///
/// This is the user-facing path for the other half of the preview defect. `handoff.md` §10.4 requires
/// the work bodies on a genuinely different origin, and `dist/works` is BUILD OUTPUT rather than an
/// Astro target, so the launcher has to serve the directory itself. What this asserts is therefore the
/// whole claim: a real process, a real port, the real built bytes, and a kill that frees the port.
///
/// `#[ignore]`d for environmental reasons rather than convenience: it needs the built launcher
/// (`scripts/build-sidecar.ps1`) and a built works root (`npm run build` in `site/`).
#[test]
#[ignore = "starts a real static preview server; needs the built sidecar and a built works root"]
fn the_works_preview_serves_the_built_works_root_on_its_own_origin() {
    let root = repository_root();
    let launcher = launcher_in(&root);

    // `handoff.md` §10.5 puts the two publish roots in the PROJECT's `dist/`, while the Astro build
    // writes them under the site's own `site/dist/`; `veloct-release`'s runner copies one into the
    // other. This stages what a release produces, and removes only what it created.
    let staged = stage_works_root(&root);
    let works_root = root.join("dist").join("works");

    // A file that is really in the built root, and its bytes: "the URL answered" must not be satisfiable
    // by a server that returns 404 for everything, and comparing the body is what says the BUILT file is
    // what is being served.
    let (relative, expected) = first_built_page(&works_root);
    let relative_url = format!("/{relative}");

    // The application's OWN state, built by the function `run` calls, so the supervisor under test is
    // the one that ships. `serve` reads the directory it was given and consults neither
    // `VELOCT_PROJECT_ROOT` nor a Node runtime, which is a deliberate property: the works preview must
    // not depend on the site's installed dependencies.
    let state = application_state();
    state
        .set_project(
            // An in-memory log sink: opening the repository as a project must not write a log into the
            // repository this test is testing.
            veloct_desktop_lib::OpenProject::with_log(
                &root,
                "2026-01-01T00:00:00Z",
                Box::new(veloct_logging::MemorySink::new(64)),
            ),
        )
        .unwrap_or_else(|error| panic!("the project could not be set: {error}"));
    // Exactly what `lib.rs`'s setup does with the resolved resource directory.
    state.set_sidecar(launcher.to_string_lossy().to_string());

    let started = match state.start_works_preview() {
        Ok(state) => state,
        Err(error) => panic!(
            "start_works_preview failed with {}: {} (hint {:?})",
            error.code, error.message, error.hint
        ),
    };
    let port = started
        .port()
        .unwrap_or_else(|| panic!("a running preview must report its port: {started:?}"));
    let url = state.preview_url(PreviewRole::Works);
    let answered = is_http_ready(port, Duration::from_millis(500));
    let fetched = fetch(port, &relative_url);

    // Teardown BEFORE the assertions about the run, so a failing expectation cannot leak a server that
    // holds the port for the rest of the session.
    let stopped = state.shutdown();

    assert!(
        started.is_running(),
        "start_works_preview must reach Running, got {started:?}"
    );
    assert_eq!(
        url.as_deref(),
        Some(format!("http://127.0.0.1:{port}/").as_str()),
        "the iframe URL must point at the port that answers"
    );
    assert!(
        answered,
        "the works preview was reported running on port {port} but nothing answered HTTP there"
    );
    assert_eq!(
        fetched.0, 200,
        "GET {relative_url} must be served from {}",
        works_root.display()
    );
    assert_eq!(
        fetched.1, expected,
        "the works origin must serve the BUILT bytes of {relative_url}, not a substitute"
    );
    assert!(stopped.is_ok(), "shutdown must stop the preview: {stopped:?}");
    assert!(
        !is_http_ready(port, Duration::from_millis(500)),
        "port {port} is still answering after shutdown, so the kill left a server holding it"
    );

    // Dropped here rather than at the end of the function so the staging is undone even if an assertion
    // above failed: the guard removes only what it created.
    drop(staged);
}

/// One raw HTTP GET, returning the status code and the body BYTES.
///
/// The app's own readiness primitive cannot say which bytes were served, and "something answered" is not
/// the claim this test makes.
fn fetch(port: u16, path: &str) -> (u16, Vec<u8>) {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let mut stream = TcpStream::connect_timeout(&address.into(), Duration::from_secs(5))
        .unwrap_or_else(|error| panic!("could not connect to 127.0.0.1:{port}: {error}"));
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap_or_else(|error| panic!("could not bound the read: {error}"));

    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .unwrap_or_else(|error| panic!("the request could not be written: {error}"));

    // `Connection: close` is what the server states, so the response ends at the end of the stream.
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .unwrap_or_else(|error| panic!("the response could not be read: {error}"));

    let head_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap_or_else(|| {
            panic!(
                "no response head in {:?}",
                String::from_utf8_lossy(&response)
            )
        });
    let head = String::from_utf8_lossy(&response[..head_end]).to_string();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("no status code in {head:?}"));

    (status, response[head_end + 4..].to_vec())
}

/// The directory this test created, removed when it is dropped.
///
/// Only what was created is removed: a project that already has a `dist/works` is left exactly as it was
/// found.
struct StagedDist {
    created: Option<PathBuf>,
}

impl Drop for StagedDist {
    fn drop(&mut self) {
        if let Some(path) = &self.created {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// Make sure the project has a works root, the way a release does.
///
/// `handoff.md` §10.5 puts `dist/works` in the PROJECT and the Astro integration writes it under the
/// site's own `site/dist/`, so `veloct-release`'s runner copies one into the other. A preview before
/// any build has nothing to serve and a test cannot run a whole release, so this does the same copy —
/// rather than inventing a file and then asserting that the invented file is served.
fn stage_works_root(root: &Path) -> StagedDist {
    let dist = root.join("dist");
    let works = dist.join("works");
    if works.is_dir() {
        return StagedDist { created: None };
    }

    let built = root.join("site").join("dist").join("works");
    assert!(
        built.is_dir(),
        "neither {} nor {} exists; run `npm run build` in site/ first, because the works preview serves \
         the BUILT works root and has nothing to serve before a build",
        works.display(),
        built.display()
    );

    // What to remove afterwards is what this test creates: the whole `dist/` when there was none, or just
    // `dist/works` when the project already had a dist.
    let created = if dist.is_dir() { works.clone() } else { dist.clone() };
    copy_tree(&built, &works);
    StagedDist {
        created: Some(created),
    }
}

/// Copy a directory tree. Test-local, so staging a fixture does not import the release pipeline.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to)
        .unwrap_or_else(|error| panic!("could not create {}: {error}", to.display()));
    for entry in std::fs::read_dir(from)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", from.display()))
        .flatten()
    {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target);
        } else {
            std::fs::copy(&source, &target)
                .unwrap_or_else(|error| panic!("could not copy {}: {error}", source.display()));
        }
    }
}

/// The first built page under the works root: its path relative to the root, with `/` separators, and its
/// bytes.
///
/// The works root holds `<slug>/index.html` (`handoff.md` §10.1's layout), but this walks the tree rather
/// than assuming a slug, because what matters is that the file it asks for is one THIS project actually
/// built.
fn first_built_page(works_root: &Path) -> (String, Vec<u8>) {
    let mut directories = vec![works_root.to_path_buf()];
    let mut pages: Vec<PathBuf> = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("could not read {}: {error}", directory.display()))
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some("html") {
                pages.push(path);
            }
        }
    }
    // Sorted so the file this test fetches is the same on every run.
    pages.sort();
    let page = pages.into_iter().next().unwrap_or_else(|| {
        panic!(
            "{} holds no built .html file; run `npm run build` in site/ so there is something to serve",
            works_root.display()
        )
    });

    let relative = page
        .strip_prefix(works_root)
        .expect("the page is inside the works root")
        .to_string_lossy()
        .replace('\\', "/");
    let bytes = std::fs::read(&page)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", page.display()));
    (relative, bytes)
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
