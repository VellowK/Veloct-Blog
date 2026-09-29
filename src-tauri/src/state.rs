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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use veloct_content::ContentService;
use veloct_core::workdir::ProjectRoot;
use veloct_core::{Clock, SystemClock};
use veloct_process::{
    ChildSpawner, HttpReadinessProbe, PreviewRole, PreviewSpec, PreviewState, PreviewSupervisor,
    SupervisorConfig,
};
use veloct_logging::LogSink;
use veloct_process::port::reserve_local_port;

/// How the application acquires a preview child process.
///
/// The Tauri shell plugin is the only supported sidecar path, but the shell's spawn API needs the
/// running `AppHandle`, so the spawner is created during Tauri setup and injected here. Tests inject
/// [`NullSpawner`] instead, which is what lets the whole command surface be exercised without a
/// WebView.
pub trait ShellSpawner: Send + Sync {
    /// Spawn a preview, returning a handle the supervisor can poll and kill.
    fn spawn_preview(
        &self,
        spec: &PreviewSpec,
    ) -> veloct_core::Result<Arc<dyn veloct_process::ChildHandle>>;
}

/// A spawner that refuses everything.
///
/// Used when the sidecar is not configured, and in tests. It reports rather than pretending, so a
/// preview that cannot start says so instead of showing an empty frame
/// (`handoff.md` §5.3: an explicit error, not an indefinite wait).
pub struct NullSpawner;

impl ShellSpawner for NullSpawner {
    fn spawn_preview(
        &self,
        spec: &PreviewSpec,
    ) -> veloct_core::Result<Arc<dyn veloct_process::ChildHandle>> {
        Err(veloct_core::VeloctError::Preview(format!(
            "no preview spawner is configured, so the {} preview cannot start",
            spec.role.label()
        )))
    }
}

/// The spawner the supervisor sees: it forwards to a [`ShellSpawner`].
struct ForwardingSpawner(Arc<dyn ShellSpawner>);

impl ChildSpawner for ForwardingSpawner {
    fn spawn(
        &self,
        spec: &PreviewSpec,
    ) -> veloct_core::Result<veloct_process::SpawnedChild> {
        Ok(veloct_process::SpawnedChild {
            handle: self.0.spawn_preview(spec)?,
        })
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
    pub fn open(path: impl Into<PathBuf>, now: &str) -> Arc<Self> {
        let root = ProjectRoot::new(path);
        let sink = crate::logging::open_sink(root.as_path(), now);
        Arc::new(Self {
            content: ContentService::open(root.as_path()),
            root,
            opened_at: now.to_string(),
            log: Mutex::new(sink),
        })
    }

    /// Open a project with a specific log sink, which is what tests use.
    pub fn with_log(path: impl Into<PathBuf>, now: &str, sink: Box<dyn LogSink>) -> Arc<Self> {
        let root = ProjectRoot::new(path);
        Arc::new(Self {
            content: ContentService::open(root.as_path()),
            root,
            opened_at: now.to_string(),
            log: Mutex::new(sink),
        })
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
}

impl AppState {
    /// Build the state with a spawner.
    pub fn new(spawner: Arc<dyn ShellSpawner>) -> Self {
        Self {
            project: Mutex::new(None),
            previews: PreviewSupervisor::new(
                Arc::new(ForwardingSpawner(spawner)),
                Arc::new(HttpReadinessProbe),
                SupervisorConfig::default(),
            ),
            sidecar: Mutex::new(None),
        }
    }

    /// The state a test or a headless run uses: no preview can start.
    pub fn headless() -> Self {
        Self::new(Arc::new(NullSpawner))
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
        let site_dir = project.path().join("site");
        let spec = PreviewSpec {
            role: PreviewRole::Site,
            program,
            args: vec![
                "dev".to_string(),
                "--port".to_string(),
                port.to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
            ],
            working_directory: site_dir,
            env: Default::default(),
            port,
        };

        Ok(self.previews.start(&spec)?)
    }

    /// Start the works-origin preview on its own free port.
    ///
    /// `handoff.md` §10.4 requires the works bodies on a GENUINELY different origin, and two loopback
    /// ports are two origins.
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
        let spec = PreviewSpec {
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
            working_directory: project.path().to_path_buf(),
            env: Default::default(),
            port,
        };

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
        let project = OpenProject::open(dir.path(), "2026-01-01T00:00:00Z");

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
            .set_project(OpenProject::open(first.path(), "t"))
            .unwrap();
        state
            .set_project(OpenProject::open(second.path(), "t"))
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
            .set_project(OpenProject::open(dir.path(), "t"))
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
            Err(error) => assert!(error.to_string().contains("cannot start"), "{error}"),
            Ok(_) => panic!("the null spawner must not report success"),
        }
    }

    #[test]
    fn an_open_project_exposes_a_content_service_for_its_root() {
        let dir = TempDir::new().unwrap();
        let project = OpenProject::open(dir.path(), "t");
        assert_eq!(project.content.paths().root(), dir.path());
        // And the service reads an empty project cleanly rather than failing.
        let index = project.content.build_index().unwrap();
        assert!(index.entries().is_empty());
    }
}
