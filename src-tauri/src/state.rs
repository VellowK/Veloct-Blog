//! Application state: the open project, the derived index and the preview supervisor.
//!
//! ADR-0002 keeps this crate thin, so the state here holds handles to the workspace crates and does no
//! work of its own. Two choices are worth stating because both are about not paying for things the
//! user has not asked for:
//!
//! * **The index is not kept in memory.** `handoff.md` §3.4 makes the SQLite index derived data, and
//!   a `ContentService` is cheap to build from a root. Keeping an `Arc<ContentIndex>` would mean
//!   deciding when it is stale, which is a bug waiting to happen; rebuilding it on demand is a few
//!   milliseconds on a blog-sized project.
//! * **The preview is started lazily.** §5.3 requires the preview lifecycle to be supervised, and a
//!   machine with no project open should not be running a Node process for nothing.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use veloct_content::ContentService;
use veloct_core::workdir::ProjectRoot;
use veloct_core::{Clock, SystemClock};
use veloct_logging::LogSink;
use veloct_process::{
    kill_process_tree, ChildHandle, ChildSpawner, HttpReadinessProbe, PreviewRole, PreviewSpec,
    PreviewState, PreviewSupervisor, SpawnedChild, SupervisorConfig,
};
use veloct_process::port::reserve_local_port;

/// How the application acquires a preview child process.
///
/// The application uses [`OsSpawner`], which launches the sidecar launcher as an ordinary child
/// process. Tests inject [`NullSpawner`] instead, which is what lets the whole command surface be
/// exercised without a Node runtime or a WebView.
///
/// **`can_spawn` is not decoration.** The shipped application was once wired to [`NullSpawner`],
/// so every "start preview" failed with a message the user could do nothing about, and nothing
/// caught it because the test that exercised the spawner built its own fake input. A hand-built
/// `PreviewSpec` is not proof that the application hands the supervisor something that can spawn,
/// which is why this trait reports the capability and
/// `tests/preview_wiring.rs` asserts on the state the shipped wiring builds.
pub trait ShellSpawner: Send + Sync {
    /// Spawn a preview, returning a handle the supervisor can poll and kill.
    fn spawn_preview(
        &self,
        spec: &PreviewSpec,
    ) -> veloct_core::Result<Arc<dyn ChildHandle>>;

    /// Whether this spawner can start a process at all.
    ///
    /// Defaults to `true`, because a spawner that does not override it is one that tries. A
    /// refusing spawner overrides it with `false` so a wiring mistake is a failing test rather than
    /// a runtime error on the user's first preview attempt.
    fn can_spawn(&self) -> bool {
        true
    }

    /// The concrete type, for the wiring test's failure message.
    fn kind(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// A spawner that refuses everything.
///
/// Used by [`AppState::headless`] — tests, and any genuinely headless construction — and by nothing
/// the shipped application builds. It reports rather than pretending, so a preview that cannot start
/// says so instead of showing an empty frame (`handoff.md` §5.3: an explicit error, not an
/// indefinite wait). The application is wired to [`OsSpawner`]; see
/// `tests/preview_wiring.rs`, which fails if that stops being true.
pub struct NullSpawner;

impl ShellSpawner for NullSpawner {
    fn spawn_preview(
        &self,
        spec: &PreviewSpec,
    ) -> veloct_core::Result<Arc<dyn ChildHandle>> {
        // `spec.role.label()` already ends in "preview", so the message names the role as a label
        // rather than interpolating it into a sentence that would read "preview preview". The hint
        // is what the user can act on: this path is a headless or test state, not a broken install.
        Err(veloct_core::VeloctError::Preview(format!(
            "this Veloct state cannot start processes, so the {} cannot start; run the desktop \
             application (not a headless or test state) to preview a project",
            spec.role.label()
        )))
    }

    fn can_spawn(&self) -> bool {
        false
    }

    fn kind(&self) -> &'static str {
        "NullSpawner"
    }
}

/// A real spawner: an OS child process running the preview launcher.
///
/// This is what the shipped application installs in `lib.rs`. It launches the Astro preview sidecar
/// (`crates/veloct-sidecar`) with the arguments [`PreviewSpec`] carries — `dev --port <p> --host
/// 127.0.0.1` for the site, and the works server likewise — and hands the supervisor a handle it can
/// poll and kill.
///
/// Four details are load-bearing:
///
/// * **The launcher, not `npm` and not a `PATH` lookup.** ADR-0005 bundles Node as a sidecar, and
///   the launcher is the thing that resolves it from beside its own executable. `PATH` would make
///   the preview work on the machine that built it and fail on the machine that installed it.
/// * **stdout and stderr are PIPED and drained by two threads.** Piped, because the supervisor
///   turns the tail into the reason it shows for a crash (`handoff.md` §5.3). Drained, because a
///   pipe whose reader stops after one read fills up and then BLOCKS the dev server mid-write: a
///   preview that dies after ~64 KiB of Vite output would look like a freeze rather than a bug.
/// * **`kill_tree`, not `kill`.** The launcher spawns Node, and Node spawns Vite and esbuild; killing
///   only the direct child leaves them holding the port, which is exactly what §5.3 forbids.
/// * **A spawn failure is an error.** `Command::spawn` failing is reported with the program and the
///   working directory, never swallowed into a silent no-op.
pub struct OsSpawner;

/// How much child output is kept for a failure reason.
///
/// The supervisor embeds the tail in its own message, so this is bounded rather than unbounded: a
/// long-running dev server produces megabytes, and the useful part is always at the end.
const OUTPUT_TAIL_CAPACITY: usize = 8 * 1024;

impl ShellSpawner for OsSpawner {
    fn spawn_preview(
        &self,
        spec: &PreviewSpec,
    ) -> veloct_core::Result<Arc<dyn ChildHandle>> {
        let program = resolve_launcher(&spec.program)?;

        let mut command = Command::new(&program);
        command
            .args(&spec.args)
            .current_dir(&spec.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NODE_ENV", "development");
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        let mut child = command.spawn().map_err(|error| {
            veloct_core::VeloctError::Preview(format!(
                "could not start the {}: {} could not be launched from {}: {error}",
                spec.role.label(),
                program.display(),
                spec.working_directory.display()
            ))
        })?;

        let output = Arc::new(Mutex::new(String::new()));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        if let Some(stdout) = stdout {
            spawn_reader(stdout, output.clone());
        }
        if let Some(stderr) = stderr {
            spawn_reader(stderr, output.clone());
        }

        Ok(Arc::new(ShellChild {
            child: Mutex::new(child),
            output,
            liveness: AtomicU8::new(LIVENESS_UNKNOWN),
        }))
    }

    fn kind(&self) -> &'static str {
        "OsSpawner"
    }
}

/// The launcher file a spec's `program` names, or an error listing where it looked.
///
/// Three cases, and the distinction between them matters:
///
/// * **A bare name** (`cmd.exe`, `veloct-astro-sidecar`) is left to the OS to resolve, exactly as a
///   process launched by hand would be. Appending a target triple to a name that is found on `PATH`
///   would be wrong, and this is what keeps the spawner usable for anything that is not the sidecar.
/// * **A path that exists** is used as given.
/// * **A path that does not exist** is reported with every name that was tried, because that is the
///   actionable part.
///
/// The last case is not hypothetical: Tauri RENAMES an `externalBin` when it bundles. A build tree
/// holds `binaries/veloct-astro-sidecar-x86_64-pc-windows-msvc.exe` (the `-<triple>` suffix is what
/// the Cargo build needs) while an installed application holds `veloct-astro-sidecar.exe`, with the
/// triple stripped. The application records the unsuffixed name, so both are tried — the same "a
/// preview that works for the person who built it and fails for the person who installed it" trap
/// that `veloct_sidecar::resolve_node` documents for the runtime applies to the launcher itself.
pub fn resolve_launcher(program: &str) -> veloct_core::Result<PathBuf> {
    let given = Path::new(program);

    // A bare name has no directory component, so whether it exists is `PATH`'s question rather than
    // this function's: `is_file` on it would report "missing" for something the OS can start.
    if given.is_file() || given.components().count() == 1 {
        return Ok(given.to_path_buf());
    }

    let mut candidates: Vec<PathBuf> = vec![given.to_path_buf()];
    // `veloct_sidecar` owns the triple mapping, so this cannot drift from the runtime's.
    if let Ok(triple) = veloct_sidecar::target_triple() {
        let suffixed = if cfg!(windows) {
            format!("{program}-{triple}.exe")
        } else {
            format!("{program}-{triple}")
        };
        let suffixed = PathBuf::from(suffixed);
        if suffixed != given {
            candidates.push(suffixed);
        }
    }

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    Err(veloct_core::VeloctError::NotConfigured {
        what: "the Astro preview launcher".to_string(),
        hint: Some(format!(
            "expected {} to exist; looked in {}. It is installed beside the application, so a \
             build or an install step has to run first",
            program,
            candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    })
}

/// Start a thread that drains one of the child's pipes into the shared tail.
fn spawn_reader<R: Read + Send + 'static>(mut reader: R, output: Arc<Mutex<String>>) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                // EOF: the child closed the pipe, so there is nothing more to drain.
                Ok(0) => break,
                Ok(read) => append_output(&output, &buffer[..read]),
                // A read error means the pipe is gone. Reading is not the operation the supervisor
                // asked for, so it is not reported as one: the missing output is the visible effect.
                Err(_) => break,
            }
        }
    });
}

/// Append output, keeping only the last [`OUTPUT_TAIL_CAPACITY`] bytes.
fn append_output(output: &Arc<Mutex<String>>, bytes: &[u8]) {
    let mut tail = output.lock();
    tail.push_str(&String::from_utf8_lossy(bytes));
    if tail.len() > OUTPUT_TAIL_CAPACITY {
        let mut keep_from = tail.len() - OUTPUT_TAIL_CAPACITY;
        while keep_from < tail.len() && !tail.is_char_boundary(keep_from) {
            keep_from += 1;
        }
        *tail = tail[keep_from..].to_string();
    }
}

/// What has been learned about the child's liveness.
///
/// Named states rather than a `bool` because "still running" and "already reaped" are different
/// answers: `try_wait` both answers the question and REAPS an exited child, and a second call on a
/// reaped child is an `Err` — which must not be confused with "alive", or a crashed preview would be
/// reported as "starting" forever.
const LIVENESS_UNKNOWN: u8 = 0;
const LIVENESS_RUNNING: u8 = 1;
const LIVENESS_EXITED: u8 = 2;
const LIVENESS_UNREPORTABLE: u8 = 3;

/// A live preview child, as [`ChildHandle`] requires it.
struct ShellChild {
    /// Behind a lock because `try_wait` needs `&mut self` while [`ChildHandle`] takes `&self`.
    child: Mutex<Child>,
    output: Arc<Mutex<String>>,
    liveness: AtomicU8,
}

impl ChildHandle for ShellChild {
    /// The launcher's pid.
    ///
    /// On Unix the launcher replaces itself with Node (`exec`), so this is the server. On Windows it
    /// is the launcher, which waits for Node — and `kill_tree` kills both, which is what makes the
    /// wrapper safe.
    fn pid(&self) -> u32 {
        self.child.lock().id()
    }

    /// Whether the process is still alive.
    ///
    /// The answer is remembered once known, because `try_wait` both reports and REAPS: asking a
    /// second time about a reaped child is an error, and treating that error as "alive" would turn a
    /// crashed preview into "starting" forever.
    fn is_alive(&self) -> bool {
        match self.liveness.load(Ordering::SeqCst) {
            LIVENESS_RUNNING => return true,
            LIVENESS_EXITED | LIVENESS_UNREPORTABLE => return false,
            _ => {}
        }

        let mut child = self.child.lock();
        let (alive, recorded) = match child.try_wait() {
            Ok(None) => (true, LIVENESS_RUNNING),
            Ok(Some(_)) => (false, LIVENESS_EXITED),
            // The child is not reportable, so it is not alive as far as the supervisor is concerned.
            Err(_) => (false, LIVENESS_UNREPORTABLE),
        };
        self.liveness.store(recorded, Ordering::SeqCst);
        alive
    }

    /// Stop the preview **and its descendants**.
    ///
    /// The whole tree, because a Node dev server spawns Vite and esbuild workers that hold the port
    /// on their own. Reported rather than swallowed, so a failure to stop is visible.
    fn kill_tree(&self) -> veloct_core::Result<()> {
        kill_process_tree(self.pid())?;
        // `taskkill /T` on Windows and the signal on Unix reach the tree; this makes sure the direct
        // child is reaped rather than left as a zombie on the platforms where that matters.
        let _ = self.child.lock().kill();
        self.liveness.store(LIVENESS_EXITED, Ordering::SeqCst);
        Ok(())
    }

    /// The tail of what the child printed, which is what a crash reason is built from.
    fn output_tail(&self) -> String {
        self.output.lock().clone()
    }
}

/// The spawner the supervisor sees: it forwards to a [`ShellSpawner`].
///
/// A newtype rather than an `impl ChildSpawner for Arc<dyn ShellSpawner>`, which the orphan rule
/// forbids for a foreign trait and a foreign type.
struct ForwardingSpawner(ArcSpawner);

impl ChildSpawner for ForwardingSpawner {
    fn spawn(&self, spec: &PreviewSpec) -> veloct_core::Result<SpawnedChild> {
        Ok(SpawnedChild {
            handle: self.0.spawn_preview(spec)?,
        })
    }
}

/// Shares one [`ShellSpawner`] between the state and the supervisor.
///
/// Its whole reason to exist is to forward every question to the real spawner, including
/// [`ShellSpawner::can_spawn`] and [`ShellSpawner::kind`], so the answer the state reports is the
/// answer of the object that actually spawns.
#[derive(Clone)]
struct ArcSpawner(Arc<dyn ShellSpawner>);

impl ShellSpawner for ArcSpawner {
    fn spawn_preview(&self, spec: &PreviewSpec) -> veloct_core::Result<Arc<dyn ChildHandle>> {
        self.0.spawn_preview(spec)
    }

    fn can_spawn(&self) -> bool {
        self.0.can_spawn()
    }

    fn kind(&self) -> &'static str {
        self.0.kind()
    }
}

/// An open project.
///
/// Derives no `Debug` automatically: `ContentService` holds paths and is not `Debug`, and a hand-written
/// impl that names only the root is more useful anyway, since dumping a whole service into a test
/// failure message helps nobody.
pub struct OpenProject {
    pub root: ProjectRoot,
    pub content: ContentService,
    /// When the project was opened, so the UI can show it.
    pub opened_at: String,
    /// The project's event log (handoff.md section 18).
    ///
    /// Behind a `Mutex<Box<dyn LogSink>>` rather than a concrete `FileSink` so a test can drive the
    /// whole command surface against an in-memory log, and so a project whose log directory is not
    /// writable can still be used: a log that cannot be written must not stop the application, and that
    /// failure is itself worth an event.
    pub log: Mutex<Box<dyn LogSink>>,
    /// The attachment cache pool (`handoff.md` §13).
    ///
    /// Behind a lock because its operations mutate the pool AND write the state file, so two concurrent adds
    /// could interleave a read and a write of the same state.
    pub attachments: Mutex<veloct_app::AttachmentService>,
}

impl std::fmt::Debug for OpenProject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenProject")
            .field("root", &self.root.as_path())
            .field("opened_at", &self.opened_at)
            .finish()
    }
}

impl OpenProject {
    /// Open a project directory.
    ///
    /// FALLIBLE because the attachment state can genuinely fail to load, and an earlier version of this
    /// function fell back to a pool rooted in the TEMP DIRECTORY when it did. That is worse than failing: the
    /// user would attach pictures to what looked like their project and the records would land somewhere else,
    /// silently. An unreadable state file is reported with its path so it can be repaired.
    pub fn open(path: impl Into<PathBuf>, now: &str) -> veloct_core::Result<Arc<Self>> {
        let root = ProjectRoot::new(path);
        let sink = crate::logging::open_sink(root.as_path(), now);
        let attachments = veloct_app::AttachmentService::open(root.as_path())?;
        Ok(Arc::new(Self {
            content: ContentService::open(root.as_path()),
            root,
            opened_at: now.to_string(),
            log: Mutex::new(sink),
            attachments: Mutex::new(attachments),
        }))
    }

    /// Open a project with a specific log sink, which is what tests use.
    pub fn with_log(path: impl Into<PathBuf>, now: &str, sink: Box<dyn LogSink>) -> Arc<Self> {
        let root = ProjectRoot::new(path);
        let attachments = veloct_app::AttachmentService::open(root.as_path())
            .expect("the attachment state is openable in a test project");
        Arc::new(Self {
            content: ContentService::open(root.as_path()),
            root,
            opened_at: now.to_string(),
            log: Mutex::new(sink),
            attachments: Mutex::new(attachments),
        })
    }

    /// The attachment pool.
    ///
    /// `parking_lot::Mutex` has no poisoning, so this cannot fail and returns the guard directly.
    pub fn attachments(&self) -> parking_lot::MutexGuard<'_, veloct_app::AttachmentService> {
        self.attachments.lock()
    }

    /// Record an event in the project's log.
    ///
    /// The event is already redacted by the time it gets here, because `LogEvent`'s constructors do
    /// that and its fields are private. A failed write is reported to the caller rather than swallowed,
    /// so a broken log is visible instead of looking like a quiet one.
    pub fn log(&self, event: veloct_logging::LogEvent) -> veloct_core::Result<()> {
        self.log
            .lock()
            .write(&event)
    }

    /// The most recent entries, for the log page.
    pub fn recent_log(&self, limit: usize) -> veloct_core::Result<Vec<veloct_logging::Entry>> {
        self.log.lock().recent(limit)
    }

    pub fn path(&self) -> &Path {
        self.root.as_path()
    }
}

/// The whole application state.
pub struct AppState {
    /// The open project, or `None` on a fresh launch.
    project: Mutex<Option<Arc<OpenProject>>>,
    /// The preview supervisor. Created once, because its states outlive a single start.
    previews: PreviewSupervisor,
    /// The configured sidecar program name, from the shell's resource resolution.
    sidecar: Mutex<Option<String>>,
    /// The spawner the supervisor was built with.
    ///
    /// Kept as well as moved into the supervisor, because "which spawner is the application wired
    /// to" is the question whose wrong answer shipped: a state built with a spawner that cannot
    /// spawn must be detectable (`can_spawn_previews`, and `tests/preview_wiring.rs`).
    spawner: ArcSpawner,
}

impl AppState {
    /// Build the state with a spawner.
    pub fn new(spawner: Arc<dyn ShellSpawner>) -> Self {
        let spawner = ArcSpawner(spawner);
        Self {
            project: Mutex::new(None),
            previews: PreviewSupervisor::new(
                Arc::new(ForwardingSpawner(spawner.clone())),
                Arc::new(HttpReadinessProbe),
                SupervisorConfig::default(),
            ),
            sidecar: Mutex::new(None),
            spawner,
        }
    }

    /// The state a test or a headless run uses: no preview can start.
    pub fn headless() -> Self {
        Self::new(Arc::new(NullSpawner))
    }

    /// Whether this state's previews can start a process at all.
    ///
    /// `false` only for [`AppState::headless`], which is a test or a headless construction: the
    /// shipped application is built by [`crate::application_state`], and this returning `false`
    /// there means "start preview" can never work.
    pub fn can_spawn_previews(&self) -> bool {
        self.spawner.can_spawn()
    }

    /// The concrete spawner's type name, for diagnostics and for the wiring test's failure message.
    pub fn spawner_kind(&self) -> &'static str {
        self.spawner.kind()
    }

    /// Record the resolved sidecar path, once Tauri has found it.
    pub fn set_sidecar(&self, program: impl Into<String>) {
        *self.sidecar.lock() = Some(program.into());
    }

    pub fn sidecar(&self) -> Option<String> {
        self.sidecar.lock().clone()
    }

    pub fn now(&self) -> String {
        veloct_core::clock::format_rfc3339(SystemClock.now())
    }

    pub fn previews(&self) -> &PreviewSupervisor {
        &self.previews
    }

    /// The open project, or an error naming what to do about it.
    pub fn project(&self) -> Result<Arc<OpenProject>, veloct_app::CommandError> {
        self.project
            .lock()
            .clone()
            .ok_or_else(|| veloct_app::CommandError::from(veloct_core::VeloctError::not_configured(
                "no project is open",
                "open or create a project first",
            )))
    }

    /// The open project, or `None`.
    pub fn current(&self) -> Option<Arc<OpenProject>> {
        self.project.lock().clone()
    }

    /// Set the open project, stopping any preview that belonged to the previous one.
    ///
    /// Stopping first matters: a preview holds a port and a Node process, and leaving it running
    /// against a directory the user has closed would leak both.
    pub fn set_project(&self, project: Arc<OpenProject>) -> veloct_core::Result<()> {
        let previous = self.project.lock().replace(project);
        if previous.is_some() {
            self.previews.stop_all()?;
        }
        Ok(())
    }

    /// Stop every preview. Called from the Tauri exit handler.
    pub fn shutdown(&self) -> veloct_core::Result<()> {
        self.previews.stop_all()
    }

    /// Start the main-site preview on a free port, or return the running one.
    ///
    /// `handoff.md` §5.3: the port is chosen dynamically, the pid and status are tracked, and the URL
    /// is reported so the frontend can point its iframe at it.
    pub fn start_site_preview(&self) -> Result<PreviewState, veloct_app::CommandError> {
        let project = self.project()?;
        let existing = self.previews.state(PreviewRole::Site);
        if existing.is_running() {
            return Ok(existing);
        }

        let program = self.sidecar().ok_or_else(|| {
            veloct_app::CommandError::from(veloct_core::VeloctError::not_configured(
                "the Astro sidecar has not been resolved",
                "the packaged application resolves it during setup",
            ))
        })?;

        let port = reserve_local_port()?.as_u16();
        let spec = site_preview_spec(project.path(), program, port);

        Ok(self.previews.start(&spec)?)
    }

    /// Start the works-origin preview on its own free port, or return the running one.
    ///
    /// `handoff.md` §10.4 requires the works bodies on a GENUINELY different origin, and two loopback
    /// ports are two origins. What is served is the project's built `dist/works` — build output rather
    /// than an Astro target — so the launcher's `serve` mode answers it as static files; see
    /// [`works_preview_spec`] for why neither Astro command is the right tool and what that fix was.
    pub fn start_works_preview(&self) -> Result<PreviewState, veloct_app::CommandError> {
        let project = self.project()?;
        let existing = self.previews.state(PreviewRole::Works);
        if existing.is_running() {
            return Ok(existing);
        }

        let program = self.sidecar().ok_or_else(|| {
            veloct_app::CommandError::from(veloct_core::VeloctError::not_configured(
                "the Astro sidecar has not been resolved",
                "the packaged application resolves it during setup",
            ))
        })?;

        let port = reserve_local_port()?.as_u16();
        let spec = works_preview_spec(project.path(), program, port);

        Ok(self.previews.start(&spec)?)
    }

    /// The URL the preview iframe should use, or `None` when nothing is running.
    pub fn preview_url(&self, role: PreviewRole) -> Option<String> {
        self.previews
            .state(role)
            .port()
            .filter(|_| self.previews.state(role).is_running())
            .map(|port| format!("http://127.0.0.1:{port}/"))
    }

    /// Re-check the previews and report their state, for the UI's status bar.
    pub fn poll_previews(&self) -> veloct_process::PreviewStatus {
        self.previews.poll(PreviewRole::Site);
        self.previews.poll(PreviewRole::Works);
        self.previews.status()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::headless()
    }
}

/// The spec the main-site preview runs: the project's own `site/` through the Astro CLI's `dev`.
///
/// A function rather than an inline literal so the command the application hands the launcher is
/// asserted WITHOUT starting a process, in the default suite
/// (`the_site_preview_still_runs_the_astro_dev_server_against_the_projects_site`). `handoff.md` §5.4
/// wants HMR, which is what `dev` gives: a production `astro build` is never run just to preview.
pub fn site_preview_spec(project: &Path, program: String, port: u16) -> PreviewSpec {
    PreviewSpec {
        role: PreviewRole::Site,
        program,
        args: vec![
            "dev".to_string(),
            "--port".to_string(),
            port.to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
        ],
        working_directory: project.join("site"),
        env: Default::default(),
        port,
    }
}

/// The spec the works-origin preview runs: the project's BUILT works root, served as static files.
///
/// **`serve dist/works`, and deliberately not an Astro command.** `handoff.md` §10.4 requires the work
/// bodies on a genuinely different origin, and ADR-0004 says what serves them: "a second sidecar that
/// serves the work bodies as plain static files". `dist/works` is BUILD OUTPUT — the
/// `splitPublishRoots` integration emits it during `astro build` — so there is no Astro target for it:
///
/// * `dev` compiles the site's SOURCES and serves the project's routes. The works root has no Astro
///   project, no pages and no sources, so `astro dev` there would serve the wrong thing or fail.
/// * `preview` serves the site PROJECT's `outDir`, which `site/astro.config.mjs` sets to `dist/site`.
///   The work bodies are the sibling `dist/works`, and Astro has exactly one `outDir` per project.
///
/// The launcher's `serve` mode is what answers it: it reads this directory and serves those bytes, from
/// its own process on its own port, which is the independent origin §10.4 requires. The directory is
/// relative to the spec's `working_directory` (the project root), so the pair means
/// `<project>/dist/works` exactly as §10.5 lays the publish roots out.
///
/// **This is the second half of the works-preview defect.** The application already sent `serve`, the
/// launcher accepted only `dev` and `preview`, and the works preview was refused with `expected dev or
/// preview, got "serve"` before it ever listened — so the work bodies had no independent origin at all.
/// The unit test that guards it asks [`veloct_sidecar::is_served_command`] as well as comparing the
/// arguments, so a future edit that reintroduces a command the launcher refuses fails THERE and not on
/// a user's first hover.
pub fn works_preview_spec(project: &Path, program: String, port: u16) -> PreviewSpec {
    PreviewSpec {
        role: PreviewRole::Works,
        program,
        args: vec![
            "serve".to_string(),
            "dist/works".to_string(),
            "--port".to_string(),
            port.to_string(),
            "--host".to_string(),
            "127.0.0.1".to_string(),
        ],
        working_directory: project.to_path_buf(),
        env: Default::default(),
        port,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_fresh_state_has_no_project_and_says_so() {
        let state = AppState::headless();
        assert!(state.current().is_none());
        let error = state.project().unwrap_err();
        assert_eq!(error.code, "NOT_CONFIGURED");
        assert!(error.message.contains("no project is open"));
        assert!(error.hint.is_some(), "the error tells the user what to do");
    }

    #[test]
    fn setting_a_project_makes_it_current_and_records_when_it_was_opened() {
        let dir = TempDir::new().unwrap();
        let state = AppState::headless();
        let project = OpenProject::open(dir.path(), "2026-01-01T00:00:00Z").unwrap();

        state.set_project(project.clone()).unwrap();
        let current = state.project().unwrap();
        assert_eq!(current.path(), dir.path());
        assert_eq!(current.opened_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn opening_a_second_project_stops_the_previous_previews() {
        // A preview holds a port and a Node process, so leaving one running against a directory the
        // user has closed would leak both.
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let state = AppState::headless();

        state
            .set_project(OpenProject::open(first.path(), "t").unwrap())
            .unwrap();
        state
            .set_project(OpenProject::open(second.path(), "t").unwrap())
            .unwrap();

        assert_eq!(state.project().unwrap().path(), second.path());
        assert!(!state.previews().has_live_children());
    }

    #[test]
    fn a_preview_cannot_start_without_a_sidecar_and_reports_why() {
        // handoff.md section 5.3: an explicit error, not an indefinite wait or a blank frame.
        let dir = TempDir::new().unwrap();
        let state = AppState::headless();
        state
            .set_project(OpenProject::open(dir.path(), "t").unwrap())
            .unwrap();

        let error = state.start_site_preview().unwrap_err();
        assert_eq!(error.code, "NOT_CONFIGURED");
        assert!(error.message.contains("sidecar"), "{}", error.message);
    }

    #[test]
    fn a_preview_cannot_start_without_a_project() {
        let state = AppState::headless();
        state.set_sidecar("binaries/veloct-astro-sidecar");
        let error = state.start_site_preview().unwrap_err();
        assert_eq!(error.code, "NOT_CONFIGURED");
        assert!(error.message.contains("no project is open"));
    }

    #[test]
    fn no_preview_url_is_reported_when_nothing_is_running() {
        // Reporting a URL for a preview that is not up would point the iframe at a refused connection.
        let state = AppState::headless();
        assert!(state.preview_url(PreviewRole::Site).is_none());
        assert!(state.preview_url(PreviewRole::Works).is_none());
    }

    #[test]
    fn the_works_preview_hands_the_launcher_a_command_it_actually_serves() {
        // **This is the test for the second half of the works-preview defect.** The application built
        // `serve dist/works ...`, the launcher accepted only `dev` and `preview`, and the works preview
        // failed with `expected dev or preview, got "serve"` — so the work bodies had no independent
        // origin at all (`handoff.md` §10.4). Comparing the arguments to a literal would only assert what
        // this file happens to say; asking the launcher's own predicate is what makes a command the
        // launcher refuses fail HERE, in the default suite, rather than on a user's first hover.
        let dir = TempDir::new().unwrap();
        let spec = works_preview_spec(dir.path(), "binaries/veloct-astro-sidecar".to_string(), 4322);

        assert_eq!(spec.role, PreviewRole::Works);
        assert_eq!(spec.program, "binaries/veloct-astro-sidecar");
        assert_eq!(
            spec.args,
            vec!["serve", "dist/works", "--port", "4322", "--host", "127.0.0.1"]
        );
        assert_eq!(spec.port, 4322);
        // The directory is relative, so the working directory is half of what it means: the pair is
        // `<project>/dist/works`, which is where `handoff.md` §10.5 puts the works publish root.
        assert_eq!(spec.working_directory, dir.path());
        assert_eq!(
            spec.working_directory.join(&spec.args[1]),
            dir.path().join("dist").join("works")
        );
        assert!(
            veloct_sidecar::is_served_command(&spec.args),
            "the launcher does not serve {:?}, so the works preview would fail before it ever listened \
             (the defect this test exists for)",
            spec.args
        );
    }

    #[test]
    fn the_site_preview_still_runs_the_astro_dev_server_against_the_projects_site() {
        // The other half of the same contract: fixing the works command must not move the site preview
        // off `dev` against `<project>/site`, which is the command that already works.
        let dir = TempDir::new().unwrap();
        let spec = site_preview_spec(dir.path(), "binaries/veloct-astro-sidecar".to_string(), 4321);

        assert_eq!(spec.role, PreviewRole::Site);
        assert_eq!(
            spec.args,
            vec!["dev", "--port", "4321", "--host", "127.0.0.1"]
        );
        assert_eq!(spec.working_directory, dir.path().join("site"));
        assert!(veloct_sidecar::is_served_command(&spec.args));
    }

    #[test]
    fn polling_with_nothing_running_reports_stopped_rather_than_failing() {
        let state = AppState::headless();
        let status = state.poll_previews();
        assert_eq!(status.site, PreviewState::Stopped);
        assert_eq!(status.works, PreviewState::Stopped);
        assert!(status.is_ready());
        assert!(status.failures().is_empty());
    }

    #[test]
    fn shutting_down_with_nothing_running_is_not_an_error() {
        // The exit handler calls this unconditionally.
        let state = AppState::headless();
        state.shutdown().unwrap();
    }

    #[test]
    fn the_sidecar_program_is_recorded_once_resolved() {
        let state = AppState::headless();
        assert!(state.sidecar().is_none());
        state.set_sidecar("binaries/veloct-astro-sidecar");
        assert_eq!(
            state.sidecar().as_deref(),
            Some("binaries/veloct-astro-sidecar")
        );
    }

    #[test]
    fn now_returns_the_storage_timestamp_format() {
        // The frontend never invents its own timestamp format, so this asserts the SHAPE rather than a
        // fixed length: the clock emits sub-second precision, which is valid RFC 3339 and sorts
        // correctly as a string, so pinning a 20-character expectation would be testing the wrong
        // thing and would break the moment the clock got more precise.
        let state = AppState::headless();
        let now = state.now();

        assert!(now.ends_with('Z'), "{now}");
        assert!(now.contains('T'), "{now}");
        assert_eq!(now.matches('-').count(), 2, "a date and its separators: {now}");
        // It parses as the storage format, which is the claim that matters.
        let parsed = time::OffsetDateTime::parse(
            &now,
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap_or_else(|error| panic!("{now} is not RFC 3339: {error}"));
        // And it is close to now, so the clock is actually the system clock.
        let drift = (time::OffsetDateTime::now_utc() - parsed).abs();
        assert!(drift < time::Duration::seconds(5), "the clock drifted by {drift}");
    }

    #[test]
    fn a_null_spawner_reports_rather_than_pretending() {
        let spec = PreviewSpec {
            role: PreviewRole::Site,
            program: "x".to_string(),
            args: Vec::new(),
            working_directory: PathBuf::from("."),
            env: Default::default(),
            port: 1,
        };
        // `match` rather than `unwrap_err`, because the Ok type holds a trait object that is not
        // `Debug`, and the error is what this test is about.
        match NullSpawner.spawn_preview(&spec) {
            Err(error) => {
                let message = error.to_string();
                // The role label already ends in "preview", so a message that interpolates it into
                // "the {} preview cannot start" read "preview preview" — the bug the user saw.
                assert!(!message.contains("preview preview"), "{message}");
                assert!(message.contains("cannot start"), "{message}");
                assert!(message.contains("main site preview"), "{message}");
                // And it says what a user can do about it.
                assert!(message.contains("desktop application"), "{message}");
            }
            Ok(_) => panic!("the null spawner must not report success"),
        }
    }

    #[test]
    fn a_headless_state_reports_that_it_cannot_spawn_a_preview() {
        // The capability, not the error message, is what makes a wiring mistake detectable: a
        // headless state must say so, and the shipped state must not.
        let state = AppState::headless();
        assert!(!state.can_spawn_previews());
        assert!(
            state.spawner_kind().ends_with("NullSpawner"),
            "{}",
            state.spawner_kind()
        );
    }

    #[test]
    fn a_real_spawner_reports_that_it_can_spawn() {
        let state = AppState::new(Arc::new(OsSpawner));
        assert!(state.can_spawn_previews());
        assert!(
            state.spawner_kind().ends_with("OsSpawner"),
            "the capability must be reported by the spawner that actually spawns, got {}",
            state.spawner_kind()
        );
        // And the null spawner is a genuinely different answer, so the two cannot be confused.
        assert_ne!(state.spawner_kind(), AppState::headless().spawner_kind());
    }

    #[test]
    fn a_bare_program_name_is_left_to_the_operating_system_to_resolve() {
        // `PATH` resolution is the OS's job. Answering "missing" for `cmd.exe` because it is not
        // under the working directory would make the spawner unusable for anything but the sidecar.
        assert_eq!(
            resolve_launcher("cmd.exe").unwrap(),
            PathBuf::from("cmd.exe")
        );
    }

    #[test]
    fn a_launcher_that_does_not_exist_is_reported_with_every_path_that_was_tried() {
        // "The preview did not start" is not actionable. A missing launcher names the file it wanted
        // and every place it looked, so an install that dropped the sidecar can be diagnosed.
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("binaries").join("veloct-astro-sidecar");
        let error = resolve_launcher(&missing.to_string_lossy()).unwrap_err();

        assert_eq!(error.code(), "NOT_CONFIGURED");
        let hint = error.hint().expect("a hint");
        assert!(
            hint.contains(&missing.display().to_string()),
            "the hint names the file: {hint}"
        );
        // The triple-suffixed name is what a Cargo build tree holds, and it is the second candidate.
        assert!(hint.contains("veloct-astro-sidecar-"), "{hint}");
    }

    #[test]
    fn an_existing_launcher_is_returned_exactly_as_given() {
        let dir = TempDir::new().unwrap();
        let launcher = dir.path().join("veloct-astro-sidecar");
        std::fs::write(&launcher, b"pretend launcher").unwrap();
        assert_eq!(resolve_launcher(&launcher.to_string_lossy()).unwrap(), launcher);
    }

    #[test]
    fn a_spawn_that_cannot_launch_is_an_error_rather_than_a_silent_no_op() {
        // handoff.md section 5.3: an explicit error, not an indefinite wait. A spawner that returns
        // a handle for a process it never started would make the supervisor report "starting" and
        // then time out, blaming the project instead of the missing launcher.
        let dir = TempDir::new().unwrap();
        let spec = PreviewSpec {
            role: PreviewRole::Site,
            program: dir
                .path()
                .join("veloct-astro-sidecar")
                .to_string_lossy()
                .to_string(),
            args: vec!["dev".to_string()],
            working_directory: dir.path().to_path_buf(),
            env: Default::default(),
            port: 1,
        };

        // `match` rather than `unwrap_err`, because the Ok type holds a trait object that is not
        // `Debug`.
        let error = match OsSpawner.spawn_preview(&spec) {
            Err(error) => error,
            Ok(_) => panic!("a launcher that does not exist must not report success"),
        };
        assert_eq!(error.code(), "NOT_CONFIGURED", "{error}");
        assert!(error.to_string().contains("Astro preview launcher"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn a_real_spawner_starts_a_process_keeps_its_output_and_kills_its_tree() {
        // This is the part that cannot be faked with a handle: an actual OS process, an actual
        // output pipe, and an actual tree kill. The three claims `ChildHandle` makes are all
        // exercised here because each one has a failure mode the supervisor cannot detect on its
        // own: a fake "alive", output that was never drained (a full pipe then BLOCKS the child), and
        // a kill that leaves the descendants holding the port.
        let spec = PreviewSpec {
            role: PreviewRole::Site,
            program: "cmd.exe".to_string(),
            args: vec![
                "/C".to_string(),
                "echo veloct-preview-probe-ok 1>&2 & ping -n 30 127.0.0.1 >nul".to_string(),
            ],
            working_directory: std::env::temp_dir(),
            env: Default::default(),
            port: 1,
        };

        let handle = match OsSpawner.spawn_preview(&spec) {
            Ok(handle) => handle,
            Err(error) => panic!("a real process must be started, got an error: {error}"),
        };
        assert!(handle.pid() > 0, "the child reports its pid");
        assert!(handle.is_alive(), "the child is running");

        // The reader threads are separate from the spawn, so the output is waited for rather than
        // assumed. `ping -n 30` keeps the child alive while it arrives.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut output = handle.output_tail();
        while output.trim().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
            output = handle.output_tail();
        }
        assert!(
            output.contains("veloct-preview-probe-ok"),
            "the child's stderr must be captured, got {output:?}"
        );

        handle.kill_tree().expect("the tree is killable");
        // `is_alive` is polled until the reaped state is visible: `try_wait` reports the exit, and a
        // kill that did not land must fail here rather than leak a process holding a port.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while handle.is_alive() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(!handle.is_alive(), "the killed child is no longer alive");
    }

    #[test]
    fn an_open_project_exposes_a_content_service_for_its_root() {
        let dir = TempDir::new().unwrap();
        let project = OpenProject::open(dir.path(), "t").unwrap();
        assert_eq!(project.content.paths().root(), dir.path());
        // And the service reads an empty project cleanly rather than failing.
        let index = project.content.build_index().unwrap();
        assert!(index.entries().is_empty());
    }
}
