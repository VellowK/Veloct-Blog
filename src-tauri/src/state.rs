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
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

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
        // CREATE_NO_WINDOW hides the console launcher without detaching it from the supervised
        // process tree or changing the stdout/stderr pipes used for preview diagnostics.
        #[cfg(windows)]
        command.creation_flags(0x08000000);
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
            exit_code: AtomicI32::new(EXIT_CODE_UNKNOWN),
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
    if cfg!(windows) && given.extension().is_none() {
        candidates.push(given.with_extension("exe"));
    }
    // externalBin is installed beside the application, not under resources/binaries.
    if let Some(binaries) = given.parent().filter(|path| path.file_name().is_some_and(|name| name == "binaries")) {
        if let (Some(directory), Some(name)) = (binaries.parent(), given.file_name()) {
            let sibling = directory.join(name);
            candidates.push(sibling.clone());
            if cfg!(windows) && sibling.extension().is_none() {
                candidates.push(sibling.with_extension("exe"));
            }
        }
    }
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

/// "No exit code has been observed", as an [`AtomicI32`] sentinel.
///
/// A sentinel rather than an `Option<i32>` in an atomic because `AtomicI32` is what is available
/// lock-free here, and `i32::MIN` is not a code any process exit can produce. It is read through
/// [`ChildHandle::exit_code`], which turns it back into `None`, so the sentinel never escapes this
/// file.
const EXIT_CODE_UNKNOWN: i32 = i32::MIN;

/// A live preview child, as [`ChildHandle`] requires it.
struct ShellChild {
    /// Behind a lock because `try_wait` needs `&mut self` while [`ChildHandle`] takes `&self`.
    child: Mutex<Child>,
    output: Arc<Mutex<String>>,
    liveness: AtomicU8,
    /// The code the child exited with, written by [`ChildHandle::is_alive`] when it reaps the child.
    ///
    /// Recorded THERE rather than read on demand because `try_wait` both reports and reaps: a second
    /// ask about a reaped child is an error, so the answer has to be taken the one time it exists.
    exit_code: AtomicI32,
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
    /// A TERMINAL answer is remembered, because `try_wait` both reports and REAPS: asking a second
    /// time about a reaped child is an error, and treating that error as "alive" would turn a
    /// crashed preview into "starting" forever.
    ///
    /// **"Alive" is deliberately NOT remembered, and that is a fix rather than a detail.** Caching
    /// `LIVENESS_RUNNING` made every later call return `true` without ever asking the child again,
    /// so once something had been observed alive its death was invisible for the rest of the
    /// session — which left the supervisor's crash detection (`poll`) unable to fire, and would have
    /// left the dependency install reporting "running" forever, because the install learns both its
    /// verdict and its exit code from this call. `try_wait` is the non-blocking wait, so asking every
    /// time costs nothing.
    fn is_alive(&self) -> bool {
        match self.liveness.load(Ordering::SeqCst) {
            LIVENESS_EXITED | LIVENESS_UNREPORTABLE => return false,
            _ => {}
        }

        let mut child = self.child.lock();
        let (alive, recorded) = match child.try_wait() {
            Ok(None) => (true, LIVENESS_RUNNING),
            Ok(Some(status)) => {
                // The ONE moment the exit code is available: `try_wait` has reaped the child, and a
                // second call would fail. A signal death has no code on Unix, so the sentinel stands
                // for "not reportable" and `exit_code` reports `None` for it rather than 0.
                self.exit_code
                    .store(status.code().unwrap_or(EXIT_CODE_UNKNOWN), Ordering::SeqCst);
                (false, LIVENESS_EXITED)
            }
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

    /// The code the child exited with, once [`ChildHandle::is_alive`] has observed the exit.
    ///
    /// The dependency install is the caller: it reports a SUCCESS only for a code it actually saw,
    /// so `None` here ("not observed", or a signal death with no code) is never treated as 0. A
    /// cancelled install is `Cancelled` before this is ever asked, which is why `kill_tree` does not
    /// invent a code.
    fn exit_code(&self) -> Option<i32> {
        match self.exit_code.load(Ordering::SeqCst) {
            EXIT_CODE_UNKNOWN => None,
            code => Some(code),
        }
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
    /// Custom-template files that were deliberately not replaced during a safe upgrade.
    pub template_warnings: Mutex<Vec<String>>,
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
    pub fn path(&self) -> &std::path::Path {
        self.root.as_path()
    }

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
            template_warnings: Mutex::new(Vec::new()),
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
            template_warnings: Mutex::new(Vec::new()),
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

    pub fn site_settings(&self) -> veloct_app::SiteSettingsService {
        veloct_app::SiteSettingsService::new(self.path())
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
    /// Resolved by the shell, never supplied by the frontend.
    project_template: Mutex<Option<PathBuf>>,
    /// The spawner the supervisor was built with.
    ///
    /// Kept as well as moved into the supervisor, because "which spawner is the application wired
    /// to" is the question whose wrong answer shipped: a state built with a spawner that cannot
    /// spawn must be detectable (`can_spawn_previews`, and `tests/preview_wiring.rs`).
    spawner: ArcSpawner,
    /// The one-click dependency install: at most ONE at a time, and `None` when none is known.
    ///
    /// **This is not a supervisor role**, and the reason is structural rather than a preference:
    /// `PreviewSupervisor::start` waits for a port to answer HTTP, and `npm install` never binds a
    /// port — it would sit out the full readiness timeout and then report a wedged process. So the
    /// install is spawned through the SAME [`ShellSpawner`] the previews use (output capture, tree
    /// kill, a spawn failure reported with the program and directory) but held here instead of in
    /// the supervisor. [`AppState::start_site_install`] says what happens on a second run, with no
    /// network, and on a cancel.
    ///
    /// **Lock order: the project lock is never held while this lock is taken, and this lock is
    /// never held while the project lock is taken.** `parking_lot::Mutex` is not reentrant, so a
    /// method that locked this and then called [`AppState::project`] would deadlock; the methods
    /// below therefore take the project FIRST, clone the `Arc` and let the guard go.
    install: Mutex<Option<InstallRun>>,
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
            project_template: Mutex::new(None),
            spawner,
            install: Mutex::new(None),
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

    pub fn set_project_template(&self, template: PathBuf) {
        *self.project_template.lock() = Some(template);
    }

    pub fn upgrade_project_template(&self, project: &OpenProject) -> veloct_core::Result<()> {
        let Some(template) = self.project_template.lock().clone() else { return Ok(()); };
        let warnings = veloct_app::ProjectService::upgrade_builtin_templates(project.path(), &template)?;
        for warning in &warnings {
            let _ = project.log(veloct_logging::LogEvent::warning(veloct_logging::Category::App, warning));
        }
        *project.template_warnings.lock() = warnings;
        Ok(())
    }

    pub fn initialize_current_project(&self) -> Result<Arc<OpenProject>, veloct_app::CommandError> {
        let template = self.project_template.lock().clone().ok_or_else(|| {
            veloct_core::VeloctError::not_configured(
                "the bundled project template has not been resolved",
                "repair the Veloct installation or run the development template preparation step",
            )
        })?;
        // Keep the selection stable while writing; a concurrent open cannot redirect this operation.
        let selected = self.project.lock();
        let project = selected.as_ref().ok_or_else(|| {
            veloct_core::VeloctError::not_configured("no project is open", "open a project first")
        })?;
        let name = project.path().file_name().and_then(|value| value.to_str())
            .unwrap_or("My Site").to_string();
        veloct_app::ProjectService::initialize(
            project.path(),
            veloct_app::CreateProjectOptions { name, ..Default::default() },
            &template,
            veloct_core::VERSION,
            &SystemClock,
        )?;
        self.upgrade_project_template(project)?;
        Ok(project.clone())
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
            // A dependency install belongs to the project it was started for. Left running it would
            // keep writing into a `node_modules` nobody is looking at, and its state would be
            // reported against the NEXT project — the same "attachments landing somewhere else"
            // trap the attachment pool is opened fallibly to avoid.
            self.stop_install()?;
        }
        Ok(())
    }

    /// Stop every preview. Called from the Tauri exit handler.
    ///
    /// The dependency install is stopped here too: an `npm` that outlived the window would keep
    /// writing into `node_modules` after the application is gone, which is the half-installed state
    /// the cancel path already warns about. Both are attempted and the first failure is reported,
    /// exactly as [`veloct_process::PreviewSupervisor::stop_all`] does, so one stuck process cannot
    /// leave the other running.
    pub fn shutdown(&self) -> veloct_core::Result<()> {
        let install = self.stop_install();
        let previews = self.previews.stop_all();
        install?;
        previews
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

    /// Start the project's `site/` dependency install: `npm install`, through the OS spawner.
    ///
    /// This is the one-click answer to the error the user actually meets — "no Astro CLI in
    /// `<project>/site/node_modules/astro`; run the site's dependency install first" — which is
    /// correct and actionable and still leaves them to do it in a console. It runs where the build
    /// looks ([`site_install_spec`] uses the same `<project>/site`), through the same machinery the
    /// previews use, and the window polls [`AppState::site_install_view`] for its output tail.
    ///
    /// ## Run twice
    ///
    /// **Refused, by a lock rather than by a check-then-act race.** The install lock is held across
    /// the "is one already running" test, the spawn and the store, so two clicks cannot both pass
    /// and start two npms over the same `node_modules` — which is a real corruption, not a wasted
    /// run. The second caller gets the RUNNING state back, which is what the window renders.
    ///
    /// A run that has already FINISHED does not block a new one: `npm install` is idempotent, and
    /// re-running it is the documented repair after a cancel or a failure.
    ///
    /// ## No network
    ///
    /// npm resolves the registry itself, so an absent network is not an error this code can detect
    /// or should guess at: npm exits non-zero with `ETIMEDOUT`/`ENOTFOUND` in its OWN words, and
    /// that is what [`SiteInstallState::Failed`] carries — the installer's last lines, not a
    /// generic "install failed". Nothing here retries, deliberately: a retry loop against a dead
    /// network is a spinner with no explanation.
    ///
    /// ## Cancel
    ///
    /// [`AppState::cancel_site_install`] kills the WHOLE tree (npm and the node children it
    /// spawned), which is what stops a half-written `node_modules`. That is exactly why the state
    /// it records is `Cancelled` and not `Failed`: the honest description is "interrupted, the tree
    /// may be partial", and the window says so. The repair is another install.
    ///
    /// A spawn failure IS an error (never a silent no-op), and so is a project with no `site/`.
    pub fn start_site_install(&self) -> std::result::Result<SiteInstallView, veloct_app::CommandError> {
        let project = self.project()?;
        let site = project.path().join("site");

        // The lock is held across the check, the spawn and the store. See "Run twice" above.
        let mut slot = self.install.lock();
        self.settle_install(&project, &mut slot);

        if let Some(run) = slot.as_ref() {
            if run.terminal.is_none() {
                return Ok(install_view(&site, Some(run)));
            }
        }

        if !site.is_dir() {
            return Err(veloct_app::CommandError::from(
                veloct_core::VeloctError::not_configured(
                    format!("the project has no site directory at {}", site.display()),
                    "a Veloct project holds the site template in its own site/ directory; open a project that has one",
                ),
            ));
        }

        let npm = resolve_npm().ok_or_else(no_npm_error)?;
        let spec = site_install_spec(&site, &npm);

        // Through the SAME spawner the previews use, so the rules travel with the child:
        // stdout/stderr are piped and DRAINED into a tail (a full pipe would BLOCK npm mid-install
        // and look like a freeze), `kill_tree` reaches npm's whole tree, and a spawn failure is
        // reported with the program and the working directory rather than swallowed.
        let handle = self.spawner.spawn_preview(&spec)?;

        *slot = Some(InstallRun {
            handle,
            npm,
            site_dir: site.clone(),
            started_at: self.now(),
            terminal: None,
        });
        Ok(install_view(&site, slot.as_ref()))
    }

    /// Report the install's state, its output tail and whether the build can find its Astro CLI.
    ///
    /// Reading IS the poll: a run whose process has exited is judged here, once, so a finished
    /// install is never reported as still running. `astro_ready` asks
    /// `veloct_sidecar::astro_entry_in` — the exact check `AstroBuildRunner` makes before a build —
    /// so "ready" here means the build will find the CLI rather than merely that npm exited 0.
    pub fn site_install_view(&self) -> std::result::Result<SiteInstallView, veloct_app::CommandError> {
        let project = self.project()?;
        let site = project.path().join("site");

        let mut slot = self.install.lock();
        self.settle_install(&project, &mut slot);
        Ok(install_view(&site, slot.as_ref()))
    }

    /// Stop the running install, killing npm AND its children.
    ///
    /// Cancelling when nothing is running is not an error: it reports the current state, so a
    /// double click cannot fail with a message about an install the user can see is not there.
    pub fn cancel_site_install(&self) -> std::result::Result<SiteInstallView, veloct_app::CommandError> {
        let project = self.project()?;
        let site = project.path().join("site");

        let mut slot = self.install.lock();
        // A run that already exited is judged first, so "cancel" cannot turn a succeeded install
        // into a cancelled one.
        self.settle_install(&project, &mut slot);

        if let Some(run) = slot.as_mut() {
            if run.terminal.is_none() {
                // The whole tree: `npm` on Windows is a `cmd.exe` shim over node, and killing the
                // shim alone would leave the installer writing into `node_modules`.
                run.handle.kill_tree()?;
                let state = SiteInstallState::Cancelled {
                    finished_at: self.now(),
                };
                let _ = log_install(&project, &state, &run.handle.output_tail());
                run.terminal = Some(state);
            }
        }

        Ok(install_view(&site, slot.as_ref()))
    }

    /// Judge a run whose process has exited, and write its one log event.
    ///
    /// Called before every read, so a dead installer is never reported as running. The verdict is
    /// STORED rather than recomputed because the project log must get exactly one event per run,
    /// and because the failure reason has to survive the next poll.
    ///
    /// Takes the project rather than calling [`AppState::project`]: the install lock is already
    /// held here, and the two locks must not be taken inside one another (see the field's comment).
    fn settle_install(&self, project: &Arc<OpenProject>, slot: &mut Option<InstallRun>) {
        if let Some(run) = slot.as_mut() {
            if run.terminal.is_none() && !run.handle.is_alive() {
                let state = judge_install(run);
                let _ = log_install(project, &state, &run.handle.output_tail());
                run.terminal = Some(state);
            }
        }
    }

    /// Kill and forget any install, so a run cannot outlive the project it belongs to.
    ///
    /// Called when another project replaces this one and on shutdown. Reported rather than
    /// swallowed, for the same reason the previews are: a process the application believes it
    /// stopped but did not is worse than one it never started.
    fn stop_install(&self) -> veloct_core::Result<()> {
        let run = self.install.lock().take();
        match run {
            Some(run) if run.terminal.is_none() => run.handle.kill_tree(),
            _ => Ok(()),
        }
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
        env: [("VELOCT_PROJECT_ROOT".to_string(), project.to_string_lossy().to_string())].into_iter().collect(),
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

/// The environment override naming the npm a dependency install runs.
///
/// # The fact the one-click install turns on: the bundled Node CANNOT run npm
///
/// `npm` is not part of the Node binary. In a Node distribution it is a JavaScript package
/// (`node_modules/npm`, entered through `bin/npm-cli.js`) plus the `npm.cmd`/`npm` shim that sits
/// beside `node.exe` and hands that script to it. Veloct bundles the BINARY only:
/// `scripts/fetch-node.ps1` unpacks exactly ONE member of the official archive —
/// `node-v24.21.0-win-x64/node.exe`, per the pin in `scripts/node-runtime.toml` — and the pin
/// extracts neither `npm.cmd` nor `node_modules/npm`. There is no `npm-cli.js` and no
/// `node_modules/npm` anywhere in this repository. `veloct-node.exe` alone therefore cannot install
/// anything, and ADR-0005 says the same from the other side: "Node.js is a bundled sidecar"; the
/// launcher's own `command_line` documents that it runs the Astro CLI directly, "because `npm`
/// would add ... an `npm` that is not bundled: ADR-0005 ships Node, not npm".
///
/// So this install runs a SYSTEM npm when there is one — the same source
/// `crate::publish::capture_toolchain` already reads its npm version from — and refuses with an
/// explanation when there is not. [`NPM_ENV`] is the seam for the other option, BUNDLING npm: that
/// changes the shipped artefact (a second packaged tree beside `veloct-node.exe`, and the published
/// 1.0.0 installer would not have it), so it is the owner's packaging decision rather than
/// something this code may assume. Pointing this variable at a bundled npm would need no other
/// change here.
pub const NPM_ENV: &str = "VELOCT_NPM";

/// The file names a system npm is found under, in the order they are tried.
///
/// Windows gets all three because npm ships as a `.cmd` shim and only some installers add the
/// `.exe`/`.bat` variants; `npm.cmd` first, because that is what a Node.js install writes.
pub fn npm_file_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["npm.cmd", "npm.exe", "npm.bat"]
    } else {
        vec!["npm"]
    }
}

/// Find the npm a dependency install can run, or `None` when this machine has none.
///
/// Resolution order, and the difference from [`veloct_sidecar::resolve_node`] is deliberate:
///
/// 1. `VELOCT_NPM`, an explicit path — the seam for a bundled npm (see [`NPM_ENV`]) and what the
///    real-process test in `tests/site_install.rs` drives. An override pointing at NOTHING is
///    reported by failing to resolve rather than by silently searching `PATH`, which is
///    `resolve_node`'s rule for the same reason: falling through would hide a typo.
/// 2. `PATH`, scanned by hand rather than left to the OS.
///
/// **Why `PATH` is allowed here when ADR-0005 forbids it for Node.** ADR-0005 governs the runtime
/// the application ships and must therefore find on every machine; npm is not shipped at all, so
/// `PATH` is the only place it can come from. And the scan is done HERE, before the spawn, so "no
/// npm" is one actionable sentence with a hint instead of `Command::spawn`'s file-not-found. A bare
/// name would leave the OS to answer that question, and it would answer it in the spawn error path,
/// where the spawner can only report the program it could not launch.
pub fn resolve_npm() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(NPM_ENV) {
        let path = PathBuf::from(explicit);
        return path.is_file().then_some(path);
    }

    let names = npm_file_names();
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        // A blank `PATH` entry means "the current directory" to some shells and nothing to others,
        // so it is skipped rather than joined into a bogus absolute pair.
        if directory.as_os_str().is_empty() {
            continue;
        }
        for name in &names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Why a dependency install cannot run, in the shape the window already renders.
///
/// The hint is the actionable half: it names what to install, and it says out loud that Veloct's
/// bundled Node is not the missing piece, because "the app bundles Node, why does it need npm" is
/// the first thing a user will think.
fn no_npm_error() -> veloct_core::VeloctError {
    let names = npm_file_names().join(" / ");
    let hint = match std::env::var_os(NPM_ENV) {
        Some(value) => format!(
            "{NPM_ENV} is set to {} but that is not a file, and no {names} was found on PATH",
            PathBuf::from(value).display()
        ),
        None => format!(
            "no {names} was found on PATH. Veloct bundles the Node RUNTIME, not npm, so this step \
             needs a system npm: install Node.js (which includes npm), or point {NPM_ENV} at one"
        ),
    };
    veloct_core::VeloctError::not_configured("npm, for the site's dependency install", hint)
}

/// One dependency install in flight, and its verdict once there is one.
struct InstallRun {
    /// The child. Kept even after it exits, because `output_tail` is where npm's own last lines
    /// live and the failure report is built from them.
    handle: Arc<dyn ChildHandle>,
    /// The npm that was actually started, so the view reports what ran rather than what a fresh
    /// search would find now.
    npm: PathBuf,
    /// The `site/` the install ran in. Also where the artefact check looks, so the verdict does not
    /// have to remember which project this was.
    site_dir: PathBuf,
    started_at: String,
    /// The verdict, once the exit has been observed. `None` means "still running".
    terminal: Option<SiteInstallState>,
}

/// What the one-click dependency install is doing.
///
/// Separate from [`veloct_process::PreviewState`] on purpose: a preview is a SERVER (it is judged
/// by a port answering, and it is restarted when it crashes, because a dev server dying is
/// recoverable). An installer is a ONE-SHOT job: it is judged by its exit code plus whether the
/// Astro CLI is on disk, it is never restarted automatically, and its terminal states are the
/// three ways it can end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SiteInstallState {
    /// Nothing has been installed for this project in this session.
    Idle,
    /// `npm install` is running in `site/`.
    Running { pid: u32, started_at: String },
    /// npm exited 0 AND the Astro CLI is now on disk.
    Succeeded { exit_code: i32, finished_at: String },
    /// npm failed, or it claimed success without installing the Astro CLI.
    Failed {
        /// The code npm exited with. `None` when it was not reportable (a signal death), which is
        /// NOT the same answer as 0 and must never be shown as one.
        exit_code: Option<i32>,
        finished_at: String,
        /// What to tell the user. npm's own output tail travels beside this in
        /// [`SiteInstallView::output_tail`], and the project log keeps both.
        reason: String,
    },
    /// The user stopped it. `node_modules` may be half written — the window says so, because the
    /// difference between "cancelled" and "finished" is exactly the state of that directory.
    Cancelled { finished_at: String },
}

/// The install's state plus everything the window needs to show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SiteInstallView {
    pub state: SiteInstallState,
    /// The installer's output tail, bounded by [`OUTPUT_TAIL_CAPACITY`]. This is what keeps a
    /// several-minute install from being a frozen UI: the window polls the view and prints this.
    pub output_tail: String,
    /// The npm the install uses, or `None` when this machine has none — in which case the window
    /// explains what to install instead of offering a button that cannot work.
    pub npm: Option<String>,
    /// Where the install runs: the same `<project>/site` the build looks in.
    pub site_dir: String,
    /// Whether `<site>/node_modules/astro` is present — the exact condition `AstroBuildRunner`
    /// checks before a build, so this is "the build will find its CLI" rather than a guess.
    pub astro_ready: bool,
}

/// The spec the one-click dependency install runs: the system npm, in the project's `site/`.
///
/// Two fields of [`PreviewSpec`] would otherwise be lies, and both are named here rather than left
/// to be discovered:
///
/// * `role` is [`PreviewRole::Site`] because the process belongs to the site — it installs the
///   site's dependencies — and because `role` is only ever used as a label in a spawn failure. It
///   does NOT register the child with the supervisor: [`AppState::start_site_install`] holds the
///   handle itself, precisely because the supervisor would wait for a port an installer never
///   binds.
/// * `port` is `0`, which means "no port" rather than "port zero": the installer binds nothing, and
///   [`OsSpawner`] never reads it.
///
/// The arguments are `install` and NOT `ci`: `ci` deletes `node_modules` before it starts, so a
/// failure or a cancel would leave the user with LESS than they began with, and it refuses a
/// lockfile that is out of step with `package.json` — the exact "nothing works and the message is
/// about a lockfile" experience this feature exists to remove. `--no-audit --no-fund` only cut
/// noise from the tail the user reads; neither changes what is installed.
pub fn site_install_spec(site: &Path, npm: &Path) -> PreviewSpec {
    PreviewSpec {
        role: PreviewRole::Site,
        program: npm.to_string_lossy().to_string(),
        args: vec![
            "install".to_string(),
            "--no-audit".to_string(),
            "--no-fund".to_string(),
        ],
        working_directory: site.to_path_buf(),
        env: Default::default(),
        port: 0,
    }
}

/// The install view, from an ALREADY-LOCKED run slot.
///
/// A free function rather than a method because every caller holds the install lock and
/// `parking_lot::Mutex` is not reentrant: a method that locked again would deadlock, and the
/// production path would hang while the test suite stayed green.
fn install_view(site: &Path, run: Option<&InstallRun>) -> SiteInstallView {
    SiteInstallView {
        state: run
            .map(|run| {
                run.terminal
                    .clone()
                    .unwrap_or_else(|| SiteInstallState::Running {
                        pid: run.handle.pid(),
                        started_at: run.started_at.clone(),
                    })
            })
            .unwrap_or(SiteInstallState::Idle),
        output_tail: run.map(|run| run.handle.output_tail()).unwrap_or_default(),
        // The run's own npm wins: it is what is actually running, and it needs no `PATH` walk.
        npm: run
            .map(|run| run.npm.clone())
            .or_else(resolve_npm)
            .map(|path| path.to_string_lossy().replace('\\', "/")),
        site_dir: site.to_string_lossy().replace('\\', "/"),
        astro_ready: veloct_sidecar::astro_entry_in(site).is_some(),
    }
}

/// Decide what an exited install means.
///
/// **The exit code is not enough on its own, and the artefact check is not a nicety.** npm exiting
/// 0 while `node_modules/astro` is still absent is a real outcome (a `package.json` that does not
/// depend on Astro, or an install that resolved nothing), and reporting it as success would send
/// the user back to the build to fail again with the very error this feature exists to fix. So
/// success requires BOTH, and every other combination is a failure whose reason names the code.
fn judge_install(run: &InstallRun) -> SiteInstallState {
    let finished_at = veloct_core::clock::format_rfc3339(SystemClock.now());
    // Asked through the sidecar crate, so this is the SAME check the build makes and the two cannot
    // drift: `AstroBuildRunner::astro_entry` fails on exactly this condition.
    let astro_ready = veloct_sidecar::astro_entry_in(&run.site_dir).is_some();
    let package = run.site_dir.join(veloct_sidecar::ASTRO_PACKAGE);

    match (run.handle.exit_code(), astro_ready) {
        (Some(0), true) => SiteInstallState::Succeeded {
            exit_code: 0,
            finished_at,
        },
        (Some(0), false) => SiteInstallState::Failed {
            exit_code: Some(0),
            reason: format!(
                "npm install reported success but there is still no Astro CLI at {}; the site's \
                 package.json may not depend on astro",
                package.display()
            ),
            finished_at,
        },
        (Some(code), _) => SiteInstallState::Failed {
            exit_code: Some(code),
            reason: format!("npm install exited with code {code}"),
            finished_at,
        },
        (None, _) => SiteInstallState::Failed {
            exit_code: None,
            reason: "npm install exited without a reportable exit code".to_string(),
            finished_at,
        },
    }
}

/// How much of npm's tail is carried in the log's one line.
///
/// The log is read by a person, minute by minute; the FULL tail (up to
/// [`OUTPUT_TAIL_CAPACITY`]) is the install panel's job, and the log keeps enough to say what
/// happened without becoming a build transcript.
const INSTALL_LOG_TAIL: usize = 600;

/// npm's last lines as ONE line, bounded, for the log.
///
/// **The tail cannot travel in a log FIELD.** `LogEvent::line` renders fields after ` | ` and
/// `Entry::parse` splits them on spaces, so a multi-word value survives as its first word only —
/// measured: a field holding `npm error code EAI_AGAIN …` came back as `npm`. The message is the
/// one place prose can live, so the tail goes there. The cap keeps the end, which is where npm puts
/// the error.
fn install_log_tail(tail: &str) -> String {
    let flattened: String = tail.split_whitespace().collect::<Vec<_>>().join(" ");
    if flattened.len() <= INSTALL_LOG_TAIL {
        return flattened;
    }
    let mut start = flattened.len() - INSTALL_LOG_TAIL;
    while start < flattened.len() && !flattened.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &flattened[start..])
}

/// Record one install's outcome in the project's log.
///
/// The failure event carries npm's OWN last lines in its message, because the log is the durable
/// place the user can return to; the panel is where they meet it first. A success is one line — the
/// log is not a build transcript, and the interesting failure is the one that did not work.
fn log_install(
    project: &OpenProject,
    state: &SiteInstallState,
    tail: &str,
) -> veloct_core::Result<()> {
    match state {
        SiteInstallState::Succeeded { exit_code, .. } => project.log(
            veloct_logging::LogEvent::info(
                veloct_logging::Category::Build,
                "the site's dependencies were installed",
            )
            .with_field("exit_code", exit_code.to_string()),
        ),
        SiteInstallState::Failed {
            exit_code, reason, ..
        } => {
            let said = install_log_tail(tail);
            let message = if said.is_empty() {
                format!("the site's dependency install failed: {reason}")
            } else {
                format!("the site's dependency install failed: {reason}; npm said: {said}")
            };
            project.log(
                veloct_logging::LogEvent::error(veloct_logging::Category::Build, message).with_field(
                    "exit_code",
                    exit_code.map(|code| code.to_string()).unwrap_or_default(),
                ),
            )
        }
        SiteInstallState::Cancelled { .. } => project.log(veloct_logging::LogEvent::warning(
            veloct_logging::Category::Build,
            "the site's dependency install was cancelled; node_modules may be half written",
        )),
        // Not terminal states: there is nothing that happened yet to record.
        SiteInstallState::Idle | SiteInstallState::Running { .. } => Ok(()),
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
        assert_eq!(spec.env.get("VELOCT_PROJECT_ROOT").map(String::as_str), Some(dir.path().to_string_lossy().as_ref()));
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
    fn launcher_is_found_beside_the_application() {
        let dir = TempDir::new().unwrap();
        let name = if cfg!(windows) { "veloct-astro-sidecar.exe" } else { "veloct-astro-sidecar" };
        let launcher = dir.path().join(name);
        std::fs::write(&launcher, b"pretend launcher").unwrap();
        let requested = dir.path().join("binaries").join("veloct-astro-sidecar");
        assert_eq!(resolve_launcher(&requested.to_string_lossy()).unwrap(), launcher);
    }

    #[cfg(windows)]
    #[test]
    fn launcher_accepts_exe_in_binaries_directory() {
        let dir = TempDir::new().unwrap();
        let directory = dir.path().join("binaries");
        std::fs::create_dir(&directory).unwrap();
        let launcher = directory.join("veloct-astro-sidecar.exe");
        std::fs::write(&launcher, b"pretend launcher").unwrap();
        assert_eq!(resolve_launcher(&directory.join("veloct-astro-sidecar").to_string_lossy()).unwrap(), launcher);
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
        // A RUNNING child has no exit code. "Not observed" must never be readable as 0, because the
        // dependency install reports SUCCESS only for a code it saw.
        assert_eq!(handle.exit_code(), None, "a live child has no exit code");

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

    // ---------------------------------------------------------------------------------------------
    // The one-click dependency install.
    // ---------------------------------------------------------------------------------------------

    /// Serialises the tests that touch `VELOCT_NPM`.
    ///
    /// The requirement `veloct-sidecar`'s tests document for `VELOCT_NODE` applies here for the same
    /// reason: the environment is process-wide while Cargo runs tests in parallel, so a test that
    /// sets the override races every other test that resolves npm. **Every mutation goes through
    /// `with_fake_npm`/`with_missing_npm`**, which hold this lock and remove the variable
    /// themselves, so nothing can strip it outside the guard.
    static NPM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Hold the lock for the duration of `body`.
    fn lock_npm() -> std::sync::MutexGuard<'static, ()> {
        NPM_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Run a closure with `VELOCT_NPM` pointing at an existing file.
    ///
    /// A FILE is all `resolve_npm` has to find, and the fake spawner below never launches it, which
    /// is what keeps these tests off the machine's own npm: a suite that failed on a machine without
    /// Node.js would be an environmental test rather than a unit test.
    fn with_fake_npm<T>(body: impl FnOnce() -> T) -> T {
        let _guard = lock_npm();
        let dir = TempDir::new().unwrap();
        let npm = dir.path().join(npm_file_names()[0]);
        std::fs::write(&npm, b"pretend npm").unwrap();
        std::env::set_var(NPM_ENV, &npm);
        let result = body();
        std::env::remove_var(NPM_ENV);
        result
    }

    /// Run a closure with `VELOCT_NPM` pointing at nothing at all.
    fn with_missing_npm<T>(body: impl FnOnce() -> T) -> T {
        let _guard = lock_npm();
        std::env::set_var(NPM_ENV, "D:/veloct/no/such/npm");
        let result = body();
        std::env::remove_var(NPM_ENV);
        result
    }

    /// An open project whose `site/` exists, so an install has the directory it runs in.
    fn project_with_site(root: &Path) -> Arc<OpenProject> {
        std::fs::create_dir_all(root.join("site")).unwrap();
        OpenProject::with_log(
            root,
            "2026-01-01T00:00:00Z",
            Box::new(veloct_logging::MemorySink::new(64)),
        )
    }

    /// Write the Astro package a build looks for, so "installed" can become true.
    fn install_astro_package(root: &Path) {
        let package = root.join("site").join(veloct_sidecar::ASTRO_PACKAGE);
        std::fs::create_dir_all(package.join("bin")).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{ "name": "astro", "bin": { "astro": "./bin/astro.mjs" } }"#,
        )
        .unwrap();
        std::fs::write(package.join("bin/astro.mjs"), "// astro").unwrap();
    }

    /// A fake install child whose liveness, output and exit code the test controls.
    struct FakeInstallChild {
        pid: u32,
        alive: std::sync::atomic::AtomicBool,
        code: Mutex<Option<i32>>,
        output: Mutex<String>,
        killed_tree: std::sync::atomic::AtomicBool,
    }

    impl FakeInstallChild {
        fn new(pid: u32) -> Arc<Self> {
            Arc::new(Self {
                pid,
                alive: std::sync::atomic::AtomicBool::new(true),
                code: Mutex::new(None),
                // Real npm always says something, so the fake does too: a test that asserted on an
                // empty tail would pass while the tail was never wired up.
                output: Mutex::new("npm warn using --no-audit\n".to_string()),
                killed_tree: std::sync::atomic::AtomicBool::new(false),
            })
        }

        /// Record an exit the way the OS spawner observes one: the tail is complete and the code
        /// becomes knowable at the same moment the child stops being alive.
        fn exit(&self, code: i32, tail: &str) {
            *self.output.lock() = tail.to_string();
            *self.code.lock() = Some(code);
            self.alive
                .store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl ChildHandle for FakeInstallChild {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn is_alive(&self) -> bool {
            self.alive.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn kill_tree(&self) -> veloct_core::Result<()> {
            self.killed_tree
                .store(true, std::sync::atomic::Ordering::SeqCst);
            self.alive
                .store(false, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        fn output_tail(&self) -> String {
            self.output.lock().clone()
        }

        fn exit_code(&self) -> Option<i32> {
            *self.code.lock()
        }
    }

    /// A spawner that records what it was asked to launch and hands back controllable children.
    #[derive(Default)]
    struct FakeInstallerSpawner {
        specs: Mutex<Vec<PreviewSpec>>,
        children: Mutex<Vec<Arc<FakeInstallChild>>>,
        fail_with: Mutex<Option<String>>,
    }

    impl FakeInstallerSpawner {
        fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }

        /// The child handed out last, which is the running install.
        fn last(&self) -> Arc<FakeInstallChild> {
            self.children
                .lock()
                .last()
                .expect("a child was spawned")
                .clone()
        }

        /// How many processes were started. ONE per install is the claim the lock makes.
        fn starts(&self) -> usize {
            self.specs.lock().len()
        }
    }

    impl ShellSpawner for FakeInstallerSpawner {
        fn spawn_preview(&self, spec: &PreviewSpec) -> veloct_core::Result<Arc<dyn ChildHandle>> {
            if let Some(message) = self.fail_with.lock().clone() {
                return Err(veloct_core::VeloctError::Preview(message));
            }
            let pid = 1000 + self.children.lock().len() as u32;
            self.specs.lock().push(spec.clone());
            let child = FakeInstallChild::new(pid);
            self.children.lock().push(child.clone());
            let handle: Arc<dyn ChildHandle> = child;
            Ok(handle)
        }
    }

    /// A state whose project has a `site/`, plus the spawner it is wired to.
    fn state_with_project(root: &Path) -> (AppState, Arc<FakeInstallerSpawner>) {
        let spawner = FakeInstallerSpawner::new();
        let state = AppState::new(spawner.clone());
        state
            .set_project(project_with_site(root))
            .expect("the project is settable");
        (state, spawner)
    }

    #[test]
    fn the_install_spec_runs_npm_install_in_the_projects_site() {
        // The directory is the whole point: the build reads its Astro CLI from `<project>/site`,
        // and an install run anywhere else would report success while the build kept failing.
        let site = Path::new("D:/p/site");
        let npm = Path::new("C:/Program Files/nodejs/npm.cmd");
        let spec = site_install_spec(site, npm);

        assert_eq!(spec.program, npm.to_string_lossy());
        assert_eq!(spec.args, vec!["install", "--no-audit", "--no-fund"]);
        assert_eq!(spec.working_directory, site);
        assert_eq!(
            spec.working_directory.join(veloct_sidecar::ASTRO_PACKAGE),
            site.join("node_modules/astro"),
            "the install and the build must mean the same directory"
        );
        // An installer binds no port; the spawner it goes through never reads this.
        assert_eq!(spec.port, 0);
        assert_eq!(spec.role, PreviewRole::Site);
    }

    #[test]
    fn an_override_that_points_at_a_file_is_the_npm_that_is_used() {
        with_fake_npm(|| {
            let resolved = resolve_npm().expect("the override names a real file");
            assert!(resolved.is_file(), "{}", resolved.display());
        });
    }

    #[test]
    fn an_override_that_points_at_nothing_resolves_to_none_rather_than_searching_path() {
        // Falling through to `PATH` would hide a typo in the override, which is `resolve_node`'s
        // rule for `VELOCT_NODE` and holds here for the same reason.
        with_missing_npm(|| assert!(resolve_npm().is_none()));
    }

    #[test]
    fn a_running_install_refuses_a_second_start_rather_than_starting_two_npms() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());

            let first = state.start_site_install().unwrap();
            assert!(
                matches!(first.state, SiteInstallState::Running { pid: 1000, .. }),
                "{:?}",
                first.state
            );
            assert_eq!(spawner.starts(), 1);

            // The second call is ANSWERED, not refused with an error: the window renders the state
            // it gets back, and "still running" is the truthful thing to render.
            let second = state.start_site_install().unwrap();
            assert_eq!(second.state, first.state);
            assert_eq!(
                spawner.starts(),
                1,
                "two npms over one node_modules is corruption, not a wasted run"
            );
        });
    }

    #[test]
    fn an_install_that_exits_zero_with_the_astro_cli_present_is_a_success() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());
            let project = state.current().expect("a project");

            assert!(
                !state.site_install_view().unwrap().astro_ready,
                "nothing is installed yet"
            );
            state.start_site_install().unwrap();

            install_astro_package(dir.path());
            spawner.last().exit(0, "added 213 packages in 42s\n");

            let view = state.site_install_view().unwrap();
            match view.state {
                SiteInstallState::Succeeded { exit_code, .. } => assert_eq!(exit_code, 0),
                other => panic!("expected Succeeded, got {other:?}"),
            }
            assert!(view.astro_ready, "the build will find its Astro CLI");
            assert!(
                view.output_tail.contains("added 213 packages"),
                "{}",
                view.output_tail
            );

            // One event, recorded once: `settle_install` STORES the verdict, so the next poll does
            // not log the same outcome again.
            let entries = project.recent_log(16).unwrap();
            assert_eq!(
                entries
                    .iter()
                    .filter(|entry| entry.message.contains("dependencies were installed"))
                    .count(),
                1,
                "the outcome is logged exactly once"
            );
            let _ = state.site_install_view().unwrap();
            let entries = project.recent_log(16).unwrap();
            assert_eq!(
                entries
                    .iter()
                    .filter(|entry| entry.message.contains("dependencies were installed"))
                    .count(),
                1,
                "polling again must not log it a second time"
            );
        });
    }

    #[test]
    fn a_failed_install_reports_the_installers_own_last_lines() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());
            let project = state.current().expect("a project");
            state.start_site_install().unwrap();

            // An absent network is what npm actually says when it cannot reach the registry.
            spawner
                .last()
                .exit(1, "npm error code EAI_AGAIN\nnpm error network request failed\n");

            let view = state.site_install_view().unwrap();
            match &view.state {
                SiteInstallState::Failed {
                    exit_code, reason, ..
                } => {
                    assert_eq!(*exit_code, Some(1));
                    assert!(reason.contains('1'), "the reason names the code: {reason}");
                }
                other => panic!("expected Failed, got {other:?}"),
            }
            assert!(
                view.output_tail.contains("EAI_AGAIN"),
                "the installer's own words, not a generic message: {}",
                view.output_tail
            );

            // And the log keeps them, so the reason survives the panel being closed. In the MESSAGE,
            // not a field: the log renders fields after ` | ` and parses them by splitting on
            // spaces, so npm's prose would survive as its first word.
            let entries = project.recent_log(16).unwrap();
            let failed = entries
                .iter()
                .find(|entry| entry.message.contains("install failed"))
                .expect("a failure event");
            assert!(
                failed.message.contains("EAI_AGAIN"),
                "npm's own words travel in the message: {}",
                failed.message
            );
            assert_eq!(
                failed.fields.get("exit_code").map(String::as_str),
                Some("1"),
                "the code is a single token, so a field is the right place for it"
            );
        });
    }

    #[test]
    fn an_install_that_reports_success_without_the_astro_cli_is_still_a_failure() {
        // npm exiting 0 with no `node_modules/astro` leaves the user exactly where they were, and
        // the build would fail with the very message this feature exists to remove. A success
        // reported here has to mean "the next build finds its CLI".
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());
            state.start_site_install().unwrap();

            spawner.last().exit(0, "up to date, audited 1 package\n");

            let view = state.site_install_view().unwrap();
            assert!(
                matches!(view.state, SiteInstallState::Failed { exit_code: Some(0), .. }),
                "{:?}",
                view.state
            );
            assert!(!view.astro_ready);
            match &view.state {
                SiteInstallState::Failed { reason, .. } => {
                    assert!(reason.contains("no Astro CLI"), "{reason}");
                }
                other => panic!("expected Failed, got {other:?}"),
            }
        });
    }

    #[test]
    fn cancelling_kills_the_whole_tree_and_reports_that_node_modules_may_be_partial() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());
            let project = state.current().expect("a project");
            state.start_site_install().unwrap();
            let child = spawner.last();

            let view = state.cancel_site_install().unwrap();
            assert!(
                matches!(view.state, SiteInstallState::Cancelled { .. }),
                "{:?}",
                view.state
            );
            // The TREE, not the direct child: on Windows npm is a `cmd.exe` shim over node, and
            // killing the shim alone leaves the installer writing into `node_modules`.
            assert!(
                child.killed_tree.load(std::sync::atomic::Ordering::SeqCst),
                "kill_tree must be what cancels an install"
            );
            assert!(!child.is_alive());
            // The tail is still there, which is what says how far the install had got.
            assert!(view.output_tail.contains("npm"), "{}", view.output_tail);

            let entries = project.recent_log(16).unwrap();
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.message.contains("cancelled")),
                "the cancellation is recorded"
            );

            // Cancelling again is not an error and does not reclassify a finished run.
            let again = state.cancel_site_install().unwrap();
            assert!(matches!(again.state, SiteInstallState::Cancelled { .. }));
        });
    }

    #[test]
    fn cancelling_with_nothing_running_reports_the_state_rather_than_failing() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, _spawner) = state_with_project(dir.path());
            let view = state.cancel_site_install().unwrap();
            assert_eq!(view.state, SiteInstallState::Idle);
        });
    }

    #[test]
    fn a_machine_with_no_npm_is_refused_with_what_to_install_and_starts_nothing() {
        with_missing_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());

            let error = state.start_site_install().unwrap_err();
            assert_eq!(error.code, "NOT_CONFIGURED", "{}", error.message);
            let hint = error.hint.clone().unwrap_or_default();
            assert!(hint.contains("npm"), "{hint}");
            // The hint names the override, because that is where a BUNDLED npm would be pointed:
            // the follow-up is a one-line change rather than a redesign.
            assert!(hint.contains(NPM_ENV), "{hint}");
            assert!(hint.contains("PATH"), "it says where npm was looked for: {hint}");
            assert_eq!(spawner.starts(), 0, "nothing may be spawned without an npm");

            // And the view says so, so the window can explain instead of offering a dead button.
            let view = state.site_install_view().unwrap();
            assert_eq!(view.state, SiteInstallState::Idle);
            assert!(view.npm.is_none());
        });
    }

    #[test]
    fn a_project_without_a_site_directory_is_refused_rather_than_run_somewhere_else() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let spawner = FakeInstallerSpawner::new();
            let state = AppState::new(spawner.clone());
            state
                .set_project(OpenProject::with_log(
                    dir.path(),
                    "t",
                    Box::new(veloct_logging::MemorySink::new(8)),
                ))
                .unwrap();

            let error = state.start_site_install().unwrap_err();
            assert_eq!(error.code, "NOT_CONFIGURED");
            assert!(error.message.contains("site"), "{}", error.message);
            assert_eq!(spawner.starts(), 0);
        });
    }

    #[test]
    fn a_finished_install_does_not_block_the_next_one_because_re_running_is_the_repair() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());

            state.start_site_install().unwrap();
            spawner.last().exit(1, "npm error code EAI_AGAIN\n");
            assert!(matches!(
                state.site_install_view().unwrap().state,
                SiteInstallState::Failed { .. }
            ));

            state.start_site_install().unwrap();
            assert_eq!(spawner.starts(), 2, "a failed install must be repeatable");
        });
    }

    #[test]
    fn a_spawn_that_cannot_start_is_an_error_and_records_no_run() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());
            *spawner.fail_with.lock() =
                Some("npm could not be launched from the site directory".into());

            let error = state.start_site_install().unwrap_err();
            assert!(
                error.message.contains("could not be launched"),
                "a spawn failure is reported, never swallowed: {}",
                error.message
            );
            assert!(
                matches!(
                    state.site_install_view().unwrap().state,
                    SiteInstallState::Idle
                ),
                "a run that never started must not be reported as one"
            );
        });
    }

    #[test]
    fn opening_another_project_stops_the_install_that_belonged_to_the_first_one() {
        with_fake_npm(|| {
            let first = TempDir::new().unwrap();
            let second = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(first.path());
            state.start_site_install().unwrap();
            let child = spawner.last();

            state.set_project(project_with_site(second.path())).unwrap();

            assert!(
                child.killed_tree.load(std::sync::atomic::Ordering::SeqCst),
                "an install must not keep writing into a project the user has closed"
            );
            let view = state.site_install_view().unwrap();
            assert_eq!(view.state, SiteInstallState::Idle);
            assert_eq!(
                view.site_dir.replace('\\', "/"),
                second.path().join("site").to_string_lossy().replace('\\', "/"),
                "the view describes the project that is open NOW"
            );
        });
    }

    #[test]
    fn shutting_down_stops_a_running_install_as_well_as_the_previews() {
        with_fake_npm(|| {
            let dir = TempDir::new().unwrap();
            let (state, spawner) = state_with_project(dir.path());
            state.start_site_install().unwrap();
            let child = spawner.last();

            state.shutdown().unwrap();

            assert!(
                child.killed_tree.load(std::sync::atomic::Ordering::SeqCst),
                "an npm that outlived the window would keep writing into node_modules"
            );
        });
    }

    #[cfg(windows)]
    #[test]
    fn a_real_childs_death_and_exit_code_are_observed_on_every_poll() {
        // The install reports a SUCCESS only for a code it actually saw, and it learns both the
        // death and the code from `is_alive`: a handle that remembered "alive" from the first poll
        // would report a finished install as running forever. So the child is polled ALIVE first,
        // and only then allowed to die — which is exactly the sequence that used to be invisible.
        let spec = PreviewSpec {
            role: PreviewRole::Site,
            program: "cmd.exe".to_string(),
            args: vec![
                "/C".to_string(),
                "ping -n 3 127.0.0.1 >nul & exit 7".to_string(),
            ],
            working_directory: std::env::temp_dir(),
            env: Default::default(),
            port: 0,
        };
        let handle = OsSpawner.spawn_preview(&spec).expect("a real process starts");
        assert!(handle.is_alive(), "the child is running when it is first asked");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while handle.is_alive() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        assert!(
            !handle.is_alive(),
            "a child that died after being seen alive must be reported dead"
        );
        assert_eq!(
            handle.exit_code(),
            Some(7),
            "the code the process exited with"
        );
    }
}
