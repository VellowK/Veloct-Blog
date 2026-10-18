//! Preview sidecar supervision.
//!
//! `handoff.md` §5.3 lists what the app must do about the preview process: choose a port
//! dynamically, start it, record its pid and status, show an explicit error when it dies, restart it
//! automatically, clean it up on exit, and never leave a stale instance holding the port.
//!
//! The supervisor owns two roles, because §10.4 requires the work bodies to be served from a
//! genuinely different origin:
//!
//! * [`PreviewRole::Site`] — the Astro dev server for the main site.
//! * [`PreviewRole::Works`] — a static file server for the work bodies, on its own loopback port.
//!
//! Both are launched through [`ChildSpawner`], so the supervision logic is exercised in tests with a
//! fake while production uses the Tauri shell plugin.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

/// Which preview a child process belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewRole {
    /// The Astro dev server serving the main site.
    Site,
    /// The static server serving the work bodies on a separate origin.
    Works,
}

impl PreviewRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Site => "site",
            Self::Works => "works",
        }
    }

    /// A short label for diagnostics.
    pub fn label(self) -> &'static str {
        match self {
            Self::Site => "main site preview",
            Self::Works => "works origin preview",
        }
    }
}

/// What the supervisor knows about one preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PreviewState {
    /// Not started.
    Stopped,
    /// Spawned, waiting for the port to answer.
    Starting { port: u16, pid: u32 },
    /// Answering HTTP.
    Running { port: u16, pid: u32, restarts: u32 },
    /// Exited unexpectedly. Carries the reason so the UI can show it (`handoff.md` §5.3).
    Crashed {
        port: u16,
        reason: String,
        restarts: u32,
        /// Whether the supervisor will try again, so the UI can distinguish "recovering" from
        /// "given up".
        will_retry: bool,
    },
    /// Stopped on purpose.
    Exited { restarts: u32 },
}

impl PreviewState {
    pub fn port(&self) -> Option<u16> {
        match self {
            Self::Starting { port, .. }
            | Self::Running { port, .. }
            | Self::Crashed { port, .. } => Some(*port),
            Self::Stopped | Self::Exited { .. } => None,
        }
    }

    pub fn pid(&self) -> Option<u32> {
        match self {
            Self::Starting { pid, .. } | Self::Running { pid, .. } => Some(*pid),
            _ => None,
        }
    }

    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    /// Whether this state is an error the user must be told about.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Crashed { .. })
    }
}

/// How many times to restart, and how long to wait between attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartPolicy {
    pub max_restarts: u32,
    /// The first backoff interval; each attempt doubles it up to `max_backoff`.
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        // Bounded on purpose. An unbounded restart loop against a project with a syntax error would
        // spin forever, which `handoff.md` forbids: a failure must be reported, not retried silently.
        Self {
            max_restarts: 5,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(30),
        }
    }
}

impl RestartPolicy {
    /// The delay before attempt number `attempt` (1-based), doubled and capped.
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let shift = attempt.saturating_sub(1).min(16);
        let scaled = self
            .initial_backoff
            .saturating_mul(1u32 << shift.min(16));
        scaled.min(self.max_backoff)
    }

    /// Whether another attempt is allowed.
    pub fn allows(&self, attempt: u32) -> bool {
        attempt < self.max_restarts
    }
}

/// Everything needed to launch one preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSpec {
    pub role: PreviewRole,
    /// The program to launch: the preview launcher's path, or a bare name for the OS to resolve.
    ///
    /// The application supplies the resolved `binaries/veloct-astro-sidecar` name Tauri gives the
    /// bundled launcher; the spawner is what turns that into an existing file (a build tree and an
    /// installed bundle name it differently) and what runs it with `args`.
    pub program: String,
    pub args: Vec<String>,
    pub working_directory: std::path::PathBuf,
    pub env: BTreeMap<String, String>,
    pub port: u16,
}

/// A handle to a live child, as the spawner reports it.
pub trait ChildHandle: Send + Sync {
    /// The pid of the process that was spawned.
    fn pid(&self) -> u32;

    /// Whether the process is still alive.
    ///
    /// This is how a crash is detected: the supervisor does not wait on the child, it polls.
    fn is_alive(&self) -> bool;

    /// Terminate the process **and its descendants**.
    ///
    /// Tauri's `CommandChild::kill()` terminates one process, and a Node sidecar spawns Vite and
    /// esbuild workers. An implementation that only kills the direct child would orphan them and
    /// leave the port held, which is exactly what `handoff.md` §5.3 forbids.
    fn kill_tree(&self) -> Result<()>;

    /// Any output the child produced, for the process log.
    fn output_tail(&self) -> String;

    /// The code the child exited with, once its exit has been observed.
    ///
    /// Added for the one child that is not a preview: the one-click `npm install` in
    /// `src-tauri/src/state.rs`. A preview is judged by its port answering, so "it is no longer
    /// alive" is the only question the supervisor ever asks; an installer is judged by whether it
    /// SUCCEEDED, and liveness cannot tell success from failure.
    ///
    /// A defaulted method rather than a new required one, because every existing implementor is
    /// already correct with `None` — no preview asks this question — and a trait that had to be
    /// updated everywhere to say "I do not know" would be a worse trade than one that says it here.
    ///
    /// **The answer is only meaningful AFTER [`ChildHandle::is_alive`] has reported `false`**: that
    /// call is what reaps the child, and a child that has not been reaped yet has no status to read.
    /// `None` therefore means "not observed, or not reportable" — a Unix signal death has no exit
    /// code — and NEVER "exited 0". A caller that treats the two as the same would report a killed
    /// installer as a successful one.
    fn exit_code(&self) -> Option<i32> {
        None
    }
}

/// A child that was successfully launched.
pub struct SpawnedChild {
    pub handle: Arc<dyn ChildHandle>,
}

/// Launches a preview process.
///
/// Implemented in production by the Tauri shell plugin and in tests by a fake, which is what makes
/// the crash and restart paths testable without a Node runtime.
pub trait ChildSpawner: Send + Sync {
    fn spawn(&self, spec: &PreviewSpec) -> Result<SpawnedChild>;
}

/// Supervisor configuration.
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    pub policy: RestartPolicy,
    /// How long to wait for a port to answer before calling the start a failure.
    pub ready_timeout: Duration,
    /// How often to poll readiness.
    pub ready_interval: Duration,
    /// How often to check that a running child is still alive.
    pub liveness_interval: Duration,
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            policy: RestartPolicy::default(),
            // Generous, because a cold Astro dev server on Windows can take several seconds.
            ready_timeout: Duration::from_secs(45),
            ready_interval: Duration::from_millis(100),
            liveness_interval: Duration::from_millis(250),
        }
    }
}

/// A ready probe, injected so tests do not need a real server.
pub trait ReadinessProbe: Send + Sync {
    fn is_ready(&self, port: u16) -> bool;
}

/// A readiness probe that actually connects over TCP and speaks HTTP.
#[derive(Debug, Default, Clone, Copy)]
pub struct HttpReadinessProbe;

impl ReadinessProbe for HttpReadinessProbe {
    fn is_ready(&self, port: u16) -> bool {
        crate::port::is_http_ready(port, Duration::from_millis(500))
    }
}

/// A probe that reports ready as soon as it is asked. Used by tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysReadyProbe;

impl ReadinessProbe for AlwaysReadyProbe {
    fn is_ready(&self, _port: u16) -> bool {
        true
    }
}

/// A probe that never reports ready, so the start timeout path is testable.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeverReadyProbe;

impl ReadinessProbe for NeverReadyProbe {
    fn is_ready(&self, _port: u16) -> bool {
        false
    }
}

/// A child the supervisor is tracking.
///
/// Only the handle and the port are kept: the pid is already part of the reported
/// [`PreviewState`], and duplicating it here would give two places to keep in step.
struct RunningChild {
    handle: Arc<dyn ChildHandle>,
    port: u16,
}

struct SupervisorInner {
    states: BTreeMap<PreviewRole, PreviewState>,
    children: BTreeMap<PreviewRole, RunningChild>,
    restarts: BTreeMap<PreviewRole, u32>,
}

/// Owns the preview processes.
pub struct PreviewSupervisor {
    spawner: Arc<dyn ChildSpawner>,
    probe: Arc<dyn ReadinessProbe>,
    config: SupervisorConfig,
    inner: Mutex<SupervisorInner>,
}

impl PreviewSupervisor {
    pub fn new(
        spawner: Arc<dyn ChildSpawner>,
        probe: Arc<dyn ReadinessProbe>,
        config: SupervisorConfig,
    ) -> Self {
        Self {
            spawner,
            probe,
            config,
            inner: Mutex::new(SupervisorInner {
                states: BTreeMap::new(),
                children: BTreeMap::new(),
                restarts: BTreeMap::new(),
            }),
        }
    }

    /// Start a preview on the given port and wait until it answers.
    ///
    /// A start that never becomes ready is an explicit failure
    /// ([`PreviewState::Crashed`]) rather than an indefinite wait.
    pub fn start(&self, spec: &PreviewSpec) -> Result<PreviewState> {
        let role = spec.role;
        self.set_state(role, PreviewState::Stopped);

        let spawned = self.spawner.spawn(spec)?;
        let pid = spawned.handle.pid();
        self.set_state(
            role,
            PreviewState::Starting {
                port: spec.port,
                pid,
            },
        );
        {
            let mut inner = self.inner.lock();
            inner.children.insert(
                role,
                RunningChild {
                    handle: spawned.handle.clone(),
                    port: spec.port,
                },
            );
        }

        // Wait for readiness, but stop waiting the moment the process is gone: a child that exited
        // will never answer, and sitting out the full timeout would make a crash look like a slow
        // start.
        let deadline = Instant::now() + self.config.ready_timeout;
        loop {
            if self.probe.is_ready(spec.port) {
                let restarts = self.restart_count(role);
                let state = PreviewState::Running {
                    port: spec.port,
                    pid,
                    restarts,
                };
                self.set_state(role, state.clone());
                return Ok(state);
            }

            if !spawned.handle.is_alive() {
                let reason = format!(
                    "{} exited before it became ready on port {}",
                    role.label(),
                    spec.port
                );
                let tail = spawned.handle.output_tail();
                let reason = if tail.trim().is_empty() { reason } else { format!("{reason}: {}", tail.trim()) };
                return self.record_failure(role, spec.port, reason);
            }

            if Instant::now() >= deadline {
                let reason = format!(
                    "{} did not answer on port {} within {:?}",
                    role.label(),
                    spec.port,
                    self.config.ready_timeout
                );
                // The process may still be alive but wedged; killing it is what keeps the port from
                // being held by a stale instance.
                let _ = spawned.handle.kill_tree();
                return self.record_failure(role, spec.port, reason);
            }

            std::thread::sleep(self.config.ready_interval);
        }
    }

    /// Check whether a running preview is still alive, and record a crash if it is not.
    ///
    /// Called from a polling loop rather than from a signal handler, because the supervisor must not
    /// block the UI thread and a crashed sidecar is not guaranteed to deliver anything.
    pub fn poll(&self, role: PreviewRole) -> PreviewState {
        let existing = self.state(role);
        let (port, restarts) = match &existing {
            PreviewState::Running {
                port, restarts, ..
            } => (*port, *restarts),
            _ => return existing,
        };

        let alive = {
            let inner = self.inner.lock();
            inner
                .children
                .get(&role)
                .map(|child| child.handle.is_alive())
                .unwrap_or(false)
        };

        if alive {
            return existing;
        }

        let reason = {
            let inner = self.inner.lock();
            inner
                .children
                .get(&role)
                .map(|child| {
                    let tail = child.handle.output_tail();
                    if tail.trim().is_empty() {
                        format!("{} exited unexpectedly", role.label())
                    } else {
                        format!("{} exited unexpectedly: {}", role.label(), tail.trim())
                    }
                })
                .unwrap_or_else(|| format!("{} is no longer running", role.label()))
        };

        let _ = restarts;
        match self.record_failure(role, port, reason) {
            Ok(state) => state,
            Err(_) => self.state(role),
        }
    }

    /// Restart a crashed preview, honouring the policy.
    ///
    /// Returns the new state, or the failure state when the policy is exhausted, so the caller can
    /// surface "given up" instead of looping.
    pub fn restart(&self, spec: &PreviewSpec) -> Result<PreviewState> {
        let role = spec.role;
        let attempts = self.restart_count(role);
        if !self.config.policy.allows(attempts) {
            let reason = format!(
                "{} failed {} times; Veloct will not restart it again",
                role.label(),
                attempts
            );
            return self.record_failure_no_retry(role, spec.port, reason);
        }

        let backoff = self.config.policy.backoff_for(attempts + 1);
        if !backoff.is_zero() {
            std::thread::sleep(backoff);
        }

        // A fresh port is chosen for the retry so a stale holder of the old port cannot make every
        // attempt fail. The caller supplies the port it wants, so this only clears the old child.
        self.clear_child(role);

        let mut retry_spec = spec.clone();
        retry_spec.port = crate::port::reserve_local_port()?.as_u16();
        self.bump_restart(role);
        self.start(&retry_spec)
    }

    /// Stop one preview, killing its whole process tree.
    pub fn stop(&self, role: PreviewRole) -> Result<PreviewState> {
        let child = { self.inner.lock().children.remove(&role) };
        if let Some(child) = child {
            child.handle.kill_tree()?;
        }
        let restarts = self.restart_count(role);
        let state = PreviewState::Exited { restarts };
        self.set_state(role, state.clone());
        Ok(state)
    }

    /// Stop every preview. Called from the Tauri `RunEvent::Exit` handler.
    ///
    /// Deliberately does not abort on the first error: leaving a second sidecar running would hold
    /// its port, so every child is attempted and the first failure is reported afterwards.
    pub fn stop_all(&self) -> Result<()> {
        let roles: Vec<PreviewRole> = { self.inner.lock().children.keys().copied().collect() };
        let mut first_error = None;
        for role in roles {
            if let Err(error) = self.stop(role) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// The current state of one preview.
    pub fn state(&self, role: PreviewRole) -> PreviewState {
        self.inner
            .lock()
            .states
            .get(&role)
            .cloned()
            .unwrap_or(PreviewState::Stopped)
    }

    /// Current states for both roles, so the UI can render them together.
    pub fn status(&self) -> PreviewStatus {
        PreviewStatus {
            site: self.state(PreviewRole::Site),
            works: self.state(PreviewRole::Works),
        }
    }

    /// Whether any preview holds a port.
    pub fn has_live_children(&self) -> bool {
        !self.inner.lock().children.is_empty()
    }

    /// Whether the two previews are on genuinely different origins.
    ///
    /// `handoff.md` §10.4 requires the works preview to be a separate origin, and two different
    /// loopback ports are two different origins. This is asserted rather than assumed because
    /// "different port" is the whole isolation claim in local preview.
    pub fn origins_are_distinct(&self) -> bool {
        let inner = self.inner.lock();
        match (
            inner.children.get(&PreviewRole::Site),
            inner.children.get(&PreviewRole::Works),
        ) {
            (Some(site), Some(works)) => site.port != works.port,
            _ => true,
        }
    }

    fn set_state(&self, role: PreviewRole, state: PreviewState) {
        self.inner.lock().states.insert(role, state);
    }

    fn clear_child(&self, role: PreviewRole) {
        if let Some(child) = self.inner.lock().children.remove(&role) {
            let _ = child.handle.kill_tree();
        }
    }

    fn restart_count(&self, role: PreviewRole) -> u32 {
        self.inner.lock().restarts.get(&role).copied().unwrap_or(0)
    }

    fn bump_restart(&self, role: PreviewRole) {
        let mut inner = self.inner.lock();
        let entry = inner.restarts.entry(role).or_insert(0);
        *entry += 1;
    }

    fn record_failure(&self, role: PreviewRole, port: u16, reason: String) -> Result<PreviewState> {
        self.clear_child(role);
        let restarts = self.restart_count(role);
        let state = PreviewState::Crashed {
            port,
            reason,
            restarts,
            will_retry: self.config.policy.allows(restarts),
        };
        self.set_state(role, state.clone());
        Ok(state)
    }

    fn record_failure_no_retry(
        &self,
        role: PreviewRole,
        port: u16,
        reason: String,
    ) -> Result<PreviewState> {
        self.clear_child(role);
        let restarts = self.restart_count(role);
        let state = PreviewState::Crashed {
            port,
            reason,
            restarts,
            will_retry: false,
        };
        self.set_state(role, state.clone());
        Ok(state)
    }
}

/// Both preview states at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewStatus {
    pub site: PreviewState,
    pub works: PreviewState,
}

impl PreviewStatus {
    /// True when every started preview is answering.
    pub fn is_ready(&self) -> bool {
        let site_ok = matches!(self.site, PreviewState::Running { .. } | PreviewState::Stopped);
        let works_ok = matches!(self.works, PreviewState::Running { .. } | PreviewState::Stopped);
        site_ok && works_ok
    }

    /// Any failure that must be shown to the user.
    pub fn failures(&self) -> Vec<(PreviewRole, &PreviewState)> {
        let mut out = Vec::new();
        for (role, state) in [
            (PreviewRole::Site, &self.site),
            (PreviewRole::Works, &self.works),
        ] {
            if state.is_failure() {
                out.push((role, state));
            }
        }
        out
    }
}

/// Kill a process tree.
///
/// Windows needs `taskkill /T`, because `TerminateProcess` on the direct child leaves a Node
/// sidecar's Vite and esbuild workers running and holding the port.
pub fn kill_process_tree(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map_err(|e| {
                VeloctError::Preview(format!("could not run taskkill for pid {pid}: {e}"))
            })?;
        if output.status.success() {
            return Ok(());
        }
        // `taskkill` also fails when the process is already gone, which is not an error for us: the
        // goal is "not running", and it is already not running.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") || stderr.contains("没有找到") {
            Ok(())
        } else {
            Err(VeloctError::Preview(format!(
                "taskkill failed for pid {pid}: {}",
                stderr.trim()
            )))
        }
    }

    #[cfg(unix)]
    {
        // A process group is not available without spawning into one, so the direct child is killed
        // and its own cleanup is relied upon. Not a silent no-op: the result is reported.
        let status = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|e| VeloctError::Preview(format!("could not signal pid {pid}: {e}")))?;
        if status.success() {
            return Ok(());
        }
        return Err(VeloctError::Preview(format!(
            "could not signal pid {pid} to stop"
        )));
    }

    #[cfg(not(any(windows, unix)))]
    {
        let _ = pid;
        Err(VeloctError::Unsupported(
            "process tree termination is not implemented on this platform".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    /// A fake child whose liveness and kill behaviour the test controls.
    struct FakeHandle {
        pid: u32,
        alive: AtomicBool,
        killed: AtomicBool,
        killed_tree: AtomicBool,
        output: Mutex<String>,
    }

    impl FakeHandle {
        fn new(pid: u32) -> Self {
            Self {
                pid,
                alive: AtomicBool::new(true),
                killed: AtomicBool::new(false),
                killed_tree: AtomicBool::new(false),
                output: Mutex::new(String::new()),
            }
        }

        fn die(&self) {
            self.alive.store(false, Ordering::SeqCst);
        }

        fn with_output(pid: u32, text: &str) -> Self {
            let handle = Self::new(pid);
            *handle.output.lock() = text.to_string();
            handle
        }
    }

    impl ChildHandle for FakeHandle {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn is_alive(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }

        fn kill_tree(&self) -> Result<()> {
            self.killed_tree.store(true, Ordering::SeqCst);
            self.killed.store(true, Ordering::SeqCst);
            self.alive.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn output_tail(&self) -> String {
            self.output.lock().clone()
        }
    }

    /// A spawner that hands out controllable handles and can be told to fail.
    struct FakeSpawner {
        spawned: Mutex<Vec<Arc<FakeHandle>>>,
        fail_with: Mutex<Option<String>>,
        next_pid: AtomicU32,
    }

    impl FakeSpawner {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                spawned: Mutex::new(Vec::new()),
                fail_with: Mutex::new(None),
                next_pid: AtomicU32::new(1000),
            })
        }

        fn last(&self) -> Arc<FakeHandle> {
            self.spawned.lock().last().expect("a child was spawned").clone()
        }

        fn all(&self) -> Vec<Arc<FakeHandle>> {
            self.spawned.lock().clone()
        }

        fn fail(self: &Arc<Self>, message: &str) {
            *self.fail_with.lock() = Some(message.to_string());
        }

        fn remove_failure(&self) {
            *self.fail_with.lock() = None;
        }
    }

    impl ChildSpawner for FakeSpawner {
        fn spawn(&self, spec: &PreviewSpec) -> Result<SpawnedChild> {
            if let Some(message) = self.fail_with.lock().clone() {
                return Err(VeloctError::Preview(message));
            }
            let _ = spec;
            let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
            let handle = Arc::new(FakeHandle::new(pid));
            self.spawned.lock().push(handle.clone());
            Ok(SpawnedChild { handle })
        }
    }

    fn spec(role: PreviewRole, port: u16) -> PreviewSpec {
        PreviewSpec {
            role,
            program: "veloct-astro-sidecar".to_string(),
            args: vec!["dev".to_string()],
            working_directory: std::path::PathBuf::from("."),
            env: BTreeMap::new(),
            port,
        }
    }

    fn supervisor(spawner: Arc<FakeSpawner>, probe: Arc<dyn ReadinessProbe>) -> PreviewSupervisor {
        PreviewSupervisor::new(
            spawner,
            probe,
            SupervisorConfig {
                policy: RestartPolicy {
                    max_restarts: 3,
                    initial_backoff: Duration::ZERO,
                    max_backoff: Duration::ZERO,
                },
                ready_timeout: Duration::from_millis(300),
                ready_interval: Duration::from_millis(10),
                liveness_interval: Duration::from_millis(10),
            },
        )
    }

    #[test]
    fn starting_a_ready_preview_records_the_port_and_pid() {
        // handoff.md section 5.3: the port and pid must be recorded.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));

        let state = supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        match state {
            PreviewState::Running { port, pid, restarts } => {
                assert_eq!(port, 4321);
                assert_eq!(pid, 1000);
                assert_eq!(restarts, 0);
            }
            other => panic!("expected Running, got {other:?}"),
        }
        assert!(supervisor.status().site.is_running());
        assert!(supervisor.has_live_children());
    }

    #[test]
    fn a_preview_that_never_answers_is_reported_rather_than_waited_on() {
        // handoff.md section 5.3: an explicit error, not an indefinite wait.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(NeverReadyProbe));

        let state = supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        match &state {
            PreviewState::Crashed { port, reason, .. } => {
                assert_eq!(*port, 4321);
                assert!(reason.contains("did not answer"), "{reason}");
            }
            other => panic!("expected Crashed, got {other:?}"),
        }
        assert!(state.is_failure());
        // The wedged process was killed, so the port is not held by a stale instance.
        assert!(spawner.last().killed_tree.load(Ordering::SeqCst));
        assert!(!supervisor.has_live_children());
    }

    #[test]
    fn a_child_that_exits_before_ready_is_reported_with_its_output() {
        /// A probe that kills the child on the first poll, simulating an immediate crash.
        struct CrashOnProbe {
            spawner: Arc<FakeSpawner>,
        }
        impl ReadinessProbe for CrashOnProbe {
            fn is_ready(&self, _port: u16) -> bool {
                self.spawner.last().die();
                false
            }
        }

        let spawner = FakeSpawner::new();
        let probe = Arc::new(CrashOnProbe {
            spawner: spawner.clone(),
        });
        let supervisor = supervisor(spawner.clone(), probe);

        let state = supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        match &state {
            PreviewState::Crashed { reason, .. } => {
                assert!(reason.contains("exited before it became ready"), "{reason}");
            }
            other => panic!("expected Crashed, got {other:?}"),
        }
    }

    #[test]
    fn a_crash_while_running_is_detected_by_polling() {
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();

        // Still alive: polling is a no-op.
        assert!(supervisor.poll(PreviewRole::Site).is_running());

        spawner.last().die();
        let state = supervisor.poll(PreviewRole::Site);
        match &state {
            PreviewState::Crashed {
                reason, will_retry, ..
            } => {
                assert!(reason.contains("exited unexpectedly"), "{reason}");
                assert!(*will_retry, "the policy still allows an attempt");
            }
            other => panic!("expected Crashed, got {other:?}"),
        }
    }

    #[test]
    fn a_crash_reports_the_child_output_so_the_cause_is_visible() {
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();

        // Replace the recorded child with one that has output, then let it die.
        {
            let mut inner = supervisor.inner.lock();
            let dying = Arc::new(FakeHandle::with_output(1000, "Error: bad frontmatter"));
            dying.die();
            inner.children.insert(
                PreviewRole::Site,
                RunningChild {
                    handle: dying,
                    port: 4321,
                },
            );
        }

        let state = supervisor.poll(PreviewRole::Site);
        match &state {
            PreviewState::Crashed { reason, .. } => {
                assert!(reason.contains("bad frontmatter"), "{reason}");
            }
            other => panic!("expected Crashed, got {other:?}"),
        }
    }

    #[test]
    fn a_crashed_preview_is_restarted_automatically() {
        // handoff.md section 5.3: automatic restart.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        spawner.last().die();
        assert!(supervisor.poll(PreviewRole::Site).is_failure());

        let state = supervisor.restart(&spec(PreviewRole::Site, 4321)).unwrap();
        assert!(state.is_running(), "{state:?}");
        assert_eq!(spawner.all().len(), 2, "a second child was spawned");
        match state {
            PreviewState::Running { restarts, .. } => assert_eq!(restarts, 1),
            other => panic!("expected Running, got {other:?}"),
        }
    }

    #[test]
    fn a_restart_uses_a_different_port_so_a_stale_holder_cannot_block_it() {
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 1)).unwrap();
        spawner.last().die();
        supervisor.poll(PreviewRole::Site);

        let state = supervisor.restart(&spec(PreviewRole::Site, 1)).unwrap();
        let port = state.port().unwrap();
        assert_ne!(port, 1, "the retry must not reuse the port that failed");
    }

    #[test]
    fn restarting_gives_up_after_the_policy_is_exhausted() {
        // handoff.md section 5.3 and 21.2: a failure must be reported, not retried forever.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));

        for _ in 0..3 {
            supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
            spawner.last().die();
            supervisor.poll(PreviewRole::Site);
            let _ = supervisor.restart(&spec(PreviewRole::Site, 4321)).unwrap();
        }

        // The fourth attempt is refused with an explicit terminal state.
        spawner.last().die();
        supervisor.poll(PreviewRole::Site);
        let state = supervisor.restart(&spec(PreviewRole::Site, 4321)).unwrap();
        match &state {
            PreviewState::Crashed {
                reason, will_retry, ..
            } => {
                assert!(!will_retry, "the policy is exhausted");
                assert!(reason.contains("will not restart"), "{reason}");
            }
            other => panic!("expected a terminal Crashed, got {other:?}"),
        }
    }

    #[test]
    fn stopping_kills_the_whole_process_tree() {
        // handoff.md section 5.3: cleanup must not leave Vite workers behind.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();

        let child = spawner.last();
        supervisor.stop(PreviewRole::Site).unwrap();

        assert!(child.killed_tree.load(Ordering::SeqCst), "kill_tree must be used");
        assert!(!child.is_alive());
        assert!(!supervisor.has_live_children());
        matches!(supervisor.state(PreviewRole::Site), PreviewState::Exited { .. });
    }

    #[test]
    fn stop_all_stops_every_preview() {
        // This is what the Tauri RunEvent::Exit handler calls.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        supervisor.start(&spec(PreviewRole::Works, 4322)).unwrap();

        supervisor.stop_all().unwrap();

        for child in spawner.all() {
            assert!(child.killed_tree.load(Ordering::SeqCst));
        }
        assert!(!supervisor.has_live_children());
        let status = supervisor.status();
        assert!(matches!(status.site, PreviewState::Exited { .. }));
        assert!(matches!(status.works, PreviewState::Exited { .. }));
    }

    #[test]
    fn the_two_previews_run_on_different_origins() {
        // handoff.md section 10.4: the works preview must be a genuinely different origin, and two
        // different loopback ports are two different origins.
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        supervisor.start(&spec(PreviewRole::Works, 4322)).unwrap();

        assert!(supervisor.origins_are_distinct());

        let status = supervisor.status();
        assert_eq!(status.site.port(), Some(4321));
        assert_eq!(status.works.port(), Some(4322));
        assert!(status.is_ready());
        assert!(status.failures().is_empty());
    }

    #[test]
    fn status_reports_a_failure_for_the_ui_to_show() {
        let spawner = FakeSpawner::new();
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        supervisor.start(&spec(PreviewRole::Works, 4322)).unwrap();

        spawner.last().die();
        supervisor.poll(PreviewRole::Works);

        let status = supervisor.status();
        let failures = status.failures();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].0, PreviewRole::Works);
        assert!(!status.is_ready());
    }

    #[test]
    fn a_handle_that_never_saw_an_exit_code_says_none_rather_than_zero() {
        // "Unknown" and "exited 0" must not be the same answer: the dependency install
        // (`src-tauri/src/state.rs`) reports SUCCESS only for a code it actually saw, and a handle
        // that reported 0 by default would turn "the installer was killed" into "it worked".
        let handle = FakeHandle::new(1);
        assert_eq!(handle.exit_code(), None);
    }

    #[test]
    fn a_spawn_failure_is_propagated_rather_than_swallowed() {
        let spawner = FakeSpawner::new();
        spawner.fail("the sidecar binary is missing");
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));

        let error = supervisor
            .start(&spec(PreviewRole::Site, 4321))
            .expect_err("a spawn failure must surface");
        assert_eq!(error.code(), "PREVIEW");
        assert!(error.to_string().contains("missing"), "{error}");
        assert!(!supervisor.has_live_children());
    }

    #[test]
    fn a_preview_can_be_started_after_the_spawner_recovers() {
        let spawner = FakeSpawner::new();
        spawner.fail("transient");
        let supervisor = supervisor(spawner.clone(), Arc::new(AlwaysReadyProbe));
        assert!(supervisor.start(&spec(PreviewRole::Site, 4321)).is_err());

        spawner.remove_failure();
        let state = supervisor.start(&spec(PreviewRole::Site, 4321)).unwrap();
        assert!(state.is_running());
    }

    #[test]
    fn restart_backoff_grows_and_is_capped() {
        let policy = RestartPolicy {
            max_restarts: 10,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(800),
        };
        assert_eq!(policy.backoff_for(0), Duration::ZERO);
        assert_eq!(policy.backoff_for(1), Duration::from_millis(100));
        assert_eq!(policy.backoff_for(2), Duration::from_millis(200));
        assert_eq!(policy.backoff_for(3), Duration::from_millis(400));
        assert_eq!(policy.backoff_for(4), Duration::from_millis(800));
        // Capped, so a long-running failure does not back off for hours.
        assert_eq!(policy.backoff_for(20), Duration::from_millis(800));
        assert!(policy.allows(9));
        assert!(!policy.allows(10));
    }

    #[test]
    fn state_helpers_describe_the_lifecycle() {
        let stopped = PreviewState::Stopped;
        assert_eq!(stopped.port(), None);
        assert_eq!(stopped.pid(), None);
        assert!(!stopped.is_running());
        assert!(!stopped.is_failure());

        let running = PreviewState::Running {
            port: 1,
            pid: 2,
            restarts: 3,
        };
        assert_eq!(running.port(), Some(1));
        assert_eq!(running.pid(), Some(2));
        assert!(running.is_running());
        assert!(!running.is_failure());

        let crashed = PreviewState::Crashed {
            port: 1,
            reason: "boom".into(),
            restarts: 0,
            will_retry: true,
        };
        assert!(crashed.is_failure());
        assert_eq!(crashed.port(), Some(1));
        assert_eq!(crashed.pid(), None);
    }

    #[test]
    fn states_round_trip_through_json_for_the_frontend() {
        let status = PreviewStatus {
            site: PreviewState::Running {
                port: 4321,
                pid: 42,
                restarts: 1,
            },
            works: PreviewState::Crashed {
                port: 4322,
                reason: "exited".into(),
                restarts: 2,
                will_retry: false,
            },
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"state\":\"running\""));
        assert!(json.contains("\"state\":\"crashed\""));
        assert_eq!(serde_json::from_str::<PreviewStatus>(&json).unwrap(), status);
    }
}
