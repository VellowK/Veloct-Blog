//! Opening a project's log, and the events the shell itself records.
//!
//! `handoff.md` §18 asks for a log file, the key events of every subsystem, and a log page. The events
//! that the workspace crates record belong to them; what this module owns is the SINK, the shell's own
//! events, and the one rule that makes the log safe — every event goes through
//! [`veloct_logging::LogEvent`], whose constructors redact and whose fields are private.
//!
//! The sink choice is deliberately forgiving. A project whose log directory cannot be created (a
//! read-only checkout, a full disk) must still be usable for editing, so the failure to open a file sink
//! falls back to an in-memory one AND records why. A log that silently stops existing is worse than one
//! that says it is in memory.

use std::path::Path;

use veloct_logging::{Category, FileSink, LogEvent, LogSink, MemorySink, RotationPolicy};

/// How many entries the in-memory fallback and the log page hold.
pub const LOG_PAGE_LIMIT: usize = 500;

/// How many daily log files to keep.
pub const KEPT_LOG_DAYS: usize = 14;

/// Open the sink for a project, falling back to memory when the directory cannot be used.
///
/// The returned sink is boxed so the caller does not have to know which one it got, and the fallback is
/// recorded in the log itself so the log page can explain why it is showing memory rather than the file.
pub fn open_sink(project: &Path, now: &str) -> Box<dyn LogSink> {
    // Pruning happens at open rather than per event: a project can accumulate a year of daily files, and
    // stat'ing the directory thousands of times a day would be work for no benefit.
    let logs_dir = project.join(".veloct").join("logs");
    let _ = veloct_logging::prune_old_logs(&logs_dir, KEPT_LOG_DAYS);

    match FileSink::open(project, RotationPolicy::default(), now) {
        Ok(sink) => Box::new(sink),
        Err(error) => {
            let fallback = MemorySink::new(LOG_PAGE_LIMIT);
            // The fallback records its own cause. Writing to memory cannot fail in a way that matters,
            // and if it did there would be nowhere left to report it.
            let _ = fallback.write(
                &LogEvent::warning(
                    Category::App,
                    format!("the log file could not be opened, so events are kept in memory: {error}"),
                )
                .with_field("log_directory", logs_dir.to_string_lossy().to_string()),
            );
            Box::new(fallback)
        }
    }
}

/// The events the shell records around a project's lifecycle.
///
/// Grouped here rather than scattered through the commands so the shape of what the log contains is
/// visible in one place, which is what makes "the log records the key events" checkable.
pub mod shell_events {
    use super::*;

    pub fn app_started(version: &str) -> LogEvent {
        LogEvent::info(Category::App, "Veloct started").with_field("version", version)
    }

    pub fn project_opened(path: &str, is_veloct_project: bool, has_git: bool) -> LogEvent {
        LogEvent::info(Category::App, "project opened")
            .with_field("path", path)
            .with_field("is_veloct_project", is_veloct_project.to_string())
            .with_field("has_git", has_git.to_string())
    }

    pub fn project_created(path: &str) -> LogEvent {
        LogEvent::info(Category::App, "project created").with_field("path", path)
    }

    pub fn document_created(kind: &str, id: &str, title: &str) -> LogEvent {
        LogEvent::info(Category::Content, "document created")
            .with_field("kind", kind)
            .with_field("id", id)
            .with_field("title", title)
    }

    pub fn document_deleted(kind: &str, id: &str) -> LogEvent {
        // A warning, not info: deleting content is the one edit a user may want to notice afterwards.
        LogEvent::warning(Category::Content, "document deleted")
            .with_field("kind", kind)
            .with_field("id", id)
    }

    /// A save, at the level its outcome deserves.
    ///
    /// A conflict is a WARNING rather than an error: nothing was lost, and the user has a real choice to
    /// make. Reporting it as an error would inflate the error count until errors stopped meaning
    /// anything.
    pub fn document_saved(kind: &str, id: &str, status: &str, diagnostics: usize) -> LogEvent {
        let level = if status == "conflict" {
            veloct_logging::Level::Warning
        } else {
            veloct_logging::Level::Info
        };
        LogEvent::new(level, Category::Save, format!("save {status}"))
            .with_field("kind", kind)
            .with_field("id", id)
            .with_field("status", status)
            .with_field("diagnostics", diagnostics.to_string())
    }

    pub fn wip_transitioned(id: &str, transition: &str) -> LogEvent {
        LogEvent::info(Category::Content, "WIP transition applied")
            .with_field("id", id)
            .with_field("transition", transition)
    }

    pub fn reference_repaired(document_id: &str, replaced: usize) -> LogEvent {
        LogEvent::info(Category::Content, "reference repaired")
            .with_field("document_id", document_id)
            .with_field("replaced", replaced.to_string())
    }

    pub fn preview_started(role: &str, port: u16, pid: u32) -> LogEvent {
        LogEvent::info(Category::Preview, "preview started")
            .with_field("role", role)
            .with_field("port", port.to_string())
            .with_field("pid", pid.to_string())
    }

    pub fn preview_failed(role: &str, reason: &str) -> LogEvent {
        LogEvent::error(Category::Preview, "preview failed")
            .with_field("role", role)
            .with_field("reason", reason)
    }

    pub fn preview_stopped() -> LogEvent {
        LogEvent::info(Category::Preview, "previews stopped")
    }

    pub fn shutdown_teardown_failed(reason: &str) -> LogEvent {
        LogEvent::error(Category::App, "a preview process may still be running")
            .with_field("reason", reason)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use veloct_logging::Level;

    #[test]
    fn opening_a_sink_in_a_writable_project_uses_the_file() {
        let dir = TempDir::new().unwrap();
        let sink = open_sink(dir.path(), "2026-01-01T12:00:00Z");
        // A file sink can read its own output; the memory fallback can too, but the file exists.
        assert!(dir
            .path()
            .join(".veloct")
            .join("logs")
            .is_dir());
        sink.write(&LogEvent::info(Category::App, "x")).unwrap();
        assert_eq!(sink.recent(10).unwrap().len(), 1);
    }

    #[test]
    fn a_project_whose_log_directory_cannot_be_created_falls_back_to_memory_and_says_why() {
        // A read-only checkout must still be usable, and a log that silently stopped existing would be
        // worse than one that says it is in memory.
        let dir = TempDir::new().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        // A FILE where the log directory needs to go, so `create_dir_all` cannot succeed.
        std::fs::write(project.join(".veloct"), "not a directory").unwrap();

        let sink = open_sink(&project, "2026-01-01T12:00:00Z");
        let entries = sink.recent(10).unwrap();
        assert_eq!(entries.len(), 1, "the fallback recorded its own cause");
        assert_eq!(entries[0].level, Level::Warning);
        assert!(entries[0].message.contains("in memory"), "{}", entries[0].message);
    }

    #[test]
    fn every_shell_event_carries_the_fields_a_reader_needs() {
        let started = shell_events::app_started("0.1.0");
        assert_eq!(started.category(), Category::App);
        assert_eq!(started.fields()["version"], "0.1.0");

        let opened = shell_events::project_opened("D:/proj", true, true);
        assert_eq!(opened.fields()["is_veloct_project"], "true");
        assert_eq!(opened.fields()["has_git"], "true");

        let created = shell_events::document_created("article", "01ABC", "Hello");
        assert_eq!(created.category(), Category::Content);
        assert_eq!(created.fields()["id"], "01ABC");

        let preview = shell_events::preview_started("site", 4321, 42);
        assert_eq!(preview.category(), Category::Preview);
        assert_eq!(preview.fields()["port"], "4321");
    }

    #[test]
    fn a_conflict_is_logged_as_a_warning_and_a_clean_save_as_info() {
        // A conflict is not an error: nothing was lost and the user has a choice. Reporting it as one
        // would inflate the error count until errors stopped meaning anything.
        let conflict = shell_events::document_saved("article", "01ABC", "conflict", 0);
        assert_eq!(conflict.level(), Level::Warning);

        let saved = shell_events::document_saved("article", "01ABC", "saved", 0);
        assert_eq!(saved.level(), Level::Info);
        assert_eq!(saved.fields()["status"], "saved");
    }

    #[test]
    fn a_deletion_is_a_warning_because_it_is_the_edit_a_user_may_want_to_notice() {
        let deleted = shell_events::document_deleted("article", "01ABC");
        assert_eq!(deleted.level(), Level::Warning);
        assert_eq!(deleted.category(), Category::Content);
    }

    #[test]
    fn a_preview_failure_is_an_error_with_its_reason() {
        let failed = shell_events::preview_failed("site", "the sidecar exited before it became ready");
        assert_eq!(failed.level(), Level::Error);
        assert!(failed.message().contains("preview failed"));
    }

    #[test]
    fn no_shell_event_can_leak_a_secret_because_a_secret_in_a_field_is_redacted() {
        // The end-to-end guard. A field named for a credential is replaced whole, so even a short
        // opaque token cannot reach the log.
        let event = shell_events::project_opened("D:/proj", true, true)
            .with_field("token", "shortopaque")
            .with_field("note", "the key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789 failed");
        let line = event.line("2026-01-01T00:00:00Z");
        assert!(!line.contains("shortopaque"), "{line}");
        assert!(
            !line.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"),
            "{line}"
        );
    }

    #[test]
    fn the_rotation_and_retention_defaults_are_bounded() {
        // handoff.md section 18.1: the log must not grow without bound. Asserting a constant is nonzero
        // proves nothing; asserting it is BOUNDED is the property that matters.
        let policy = RotationPolicy::default();
        assert!(policy.max_bytes > 0, "a zero limit would rotate every line");
        assert!(
            policy.max_bytes < u64::MAX,
            "an unbounded file is the thing the limit exists to prevent"
        );
        assert!(policy.max_files > 0, "keeping zero rotated files loses history");
        assert!(
            policy.max_files < usize::MAX,
            "an unbounded count is not a rotation policy"
        );

        // The total the log can occupy is therefore finite and computable, which is the claim.
        let worst_case = policy.max_bytes * (policy.max_files as u64 + 1);
        assert!(worst_case <= 128 * 1024 * 1024, "the ceiling is {worst_case} bytes");
    }
}
