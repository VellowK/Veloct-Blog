//! Child-process supervision for the Astro preview sidecars.
//!
//! `handoff.md` §5.2 and §5.3 require Veloct to pick a free port, start the Astro preview, track the
//! pid and status, surface a crash as an explicit error, restart automatically, clean up on exit and
//! never leave a stale instance holding the port.
//!
//! The design keeps the OS interaction behind [`ChildSpawner`] so the supervision logic — port
//! allocation, ready detection, crash reporting, restart with backoff, and tree teardown — is
//! testable without spawning a real Node process. The production implementation is the Tauri shell
//! plugin, which is the only supported sidecar path in Tauri 2.
//!
//! Two facts drive the implementation and are easy to get wrong:
//!
//! * `CommandChild::kill()` from `tauri-plugin-shell` terminates **one** process. A Node sidecar
//!   spawns Vite/esbuild children, so teardown must kill the whole tree
//!   (`taskkill /PID <pid> /T /F` on Windows).
//! * The port is chosen by binding `127.0.0.1:0` and reading back the assigned port. Do not use
//!   `portpicker`; it was last published in 2021 and is unmaintained.
//!
//! Ownership: `verify-deploy` (team `veloct-complete`).

#![forbid(unsafe_code)]

pub mod port;
pub mod supervisor;

pub use port::{pick_free_port, reserve_local_port, PortReservation};
pub use supervisor::{
    kill_process_tree, AlwaysReadyProbe, ChildHandle, ChildSpawner, HttpReadinessProbe,
    NeverReadyProbe, PreviewRole, PreviewSpec, PreviewState, PreviewStatus, PreviewSupervisor,
    ReadinessProbe, RestartPolicy, SpawnedChild, SupervisorConfig,
};
