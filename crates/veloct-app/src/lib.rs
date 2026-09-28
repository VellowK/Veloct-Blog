//! Application orchestration: services, DTOs and the Tauri command surface.
//!
//! This is the only layer allowed to know about all the domain crates, and the only place that
//! converts domain errors into a shape the frontend can display. It deliberately does NOT
//! depend on `tauri` itself for the service layer — the `#[tauri::command]` wrappers live
//! behind the `tauri` feature so the services stay unit-testable without a WebView.
//!
//! Project access is serialised by `veloct-core`'s [`veloct_core::ProjectLock`]:
//! Publish, Restore and historical rebuilds take an exclusive lock, while editing, preview and
//! attachment uploads take shared locks (`handoff.md` §20.4, §25.1 D4/D5).
//!
//! Ownership: `content-app` (team `veloct-complete`).

#![forbid(unsafe_code)]

pub mod error;
pub mod project;

pub use error::CommandError;
pub use project::ProjectService;
