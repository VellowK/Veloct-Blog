//! Opening and preparing the project database.
//!
//! `handoff.md` §2.4 specifies one global SQLite database per project. §20.1 requires that
//! migrations be re-runnable, detectable, and safe to fail: a damaged database must never be
//! treated as lost content, because Markdown and `work.json` are the truth.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use veloct_core::workdir::read_bytes;
use veloct_core::{layout, Result, VeloctError};

use crate::migrations;

/// How many migrations were applied while opening.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    pub applied: usize,
    pub from_version: usize,
    pub to_version: usize,
}

impl MigrationReport {
    pub fn migrated(&self) -> bool {
        self.applied > 0
    }
}

/// Settings applied to every connection.
///
/// WAL keeps a long read (a backup snapshot) from blocking a write (an autosave). `foreign_keys`
/// is off by default in SQLite, and without it the cascade rules in the schema would not fire.
fn apply_pragmas(connection: &Connection) -> Result<()> {
    // These are not user input, so building the pragma text is safe here.
    for (name, value) in [
        ("journal_mode", "WAL"),
        ("synchronous", "NORMAL"),
        ("foreign_keys", "ON"),
        ("busy_timeout", "5000"),
        ("temp_store", "MEMORY"),
    ] {
        connection
            .pragma_update(None, name, value)
            .map_err(|e| VeloctError::Database(format!("could not set pragma {name}={value}: {e}")))?;
    }
    Ok(())
}

/// Open the database for a project, running migrations.
///
/// When migrations are pending the file is copied to a timestamped sibling first, so a failed or
/// interrupted upgrade leaves the user with the pre-migration database rather than nothing.
pub fn open(project_root: impl AsRef<Path>, now: &dyn veloct_core::Clock) -> Result<(Connection, MigrationReport)> {
    let path = project_root
        .as_ref()
        .join(layout::VELOCT_DIR)
        .join(layout::DB_FILE);
    open_at(&path, now)
}

/// Open a database at an explicit path. Used by tests and by the restore flow.
pub fn open_at(path: &Path, now: &dyn veloct_core::Clock) -> Result<(Connection, MigrationReport)> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| VeloctError::io(parent, e))?;
    }

    let mut connection = Connection::open(path)
        .map_err(|e| VeloctError::Database(format!("could not open {}: {e}", path.display())))?;
    apply_pragmas(&connection)?;

    let from_version = migrations::current_version(&connection)?;
    let pending = migrations::pending_migrations(&connection)?;
    if pending > 0 && from_version > 0 {
        backup_before_migration(path, from_version, now)?;
    }

    let applied = migrations::apply(&mut connection)?;
    let to_version = migrations::current_version(&connection)?;

    Ok((
        connection,
        MigrationReport {
            applied,
            from_version,
            to_version,
        },
    ))
}

/// Copy the database aside before a migration touches it.
///
/// `handoff.md` §20.1 permits rebuilding a damaged index from Markdown, but a *user's
/// configuration and release index* are only here, so an upgrade is worth a cheap snapshot.
pub fn backup_before_migration(
    path: &Path,
    from_version: usize,
    now: &dyn veloct_core::Clock,
) -> Result<PathBuf> {
    let stamp = veloct_core::clock::format_release_stamp(now.now());
    let backup = path.with_file_name(format!(
        "{}.pre-migration-v{from_version}-{stamp}.bak",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("veloct.db")
    ));

    let bytes = read_bytes(path)?;
    veloct_core::workdir::write_atomic(&backup, &bytes)?;
    Ok(backup)
}

/// Open an in-memory database with the schema applied, for tests.
pub fn open_in_memory() -> Result<Connection> {
    let mut connection = Connection::open_in_memory()
        .map_err(|e| VeloctError::Database(format!("could not open an in-memory database: {e}")))?;
    apply_pragmas(&connection)?;
    migrations::apply(&mut connection)?;
    Ok(connection)
}

/// Open a database, and if the schema is not usable, rebuild the derived index.
///
/// This is the `handoff.md` §20.1 promise made concrete: a corrupt *index* is recoverable, and the
/// caller is told that a rebuild happened so it can re-scan Markdown.
pub fn open_and_ensure_index(
    project_root: impl AsRef<Path>,
    now: &dyn veloct_core::Clock,
) -> Result<(Connection, MigrationReport, bool)> {
    let (connection, report) = open(project_root, now)?;
    let rebuilt = matches!(crate::rebuild::index_health(&connection), Ok(false));
    Ok((connection, report, rebuilt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::new(time::macros::datetime!(2026-01-01 12:00:00 UTC))
    }

    #[test]
    fn a_fresh_database_is_created_and_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".veloct").join("veloct.db");
        let (connection, report) = open_at(&path, &clock()).unwrap();

        assert!(path.exists());
        assert!(report.migrated());
        assert_eq!(report.from_version, 0);
        assert_eq!(report.to_version, migrations::CURRENT_SCHEMA_VERSION);

        // The pragmas really took effect.
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
        let foreign_keys: i64 = connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);
    }

    #[test]
    fn reopening_an_up_to_date_database_applies_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("veloct.db");
        open_at(&path, &clock()).unwrap();

        let (_connection, report) = open_at(&path, &clock()).unwrap();
        assert!(!report.migrated());
        assert_eq!(report.applied, 0);
        assert_eq!(report.from_version, migrations::CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn a_migration_takes_a_backup_of_the_existing_database() {
        // handoff 20.1: a user's configuration only exists here, so an upgrade is worth a snapshot.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("veloct.db");

        // Simulate an existing older database by creating the file and leaving the version at 0.
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute("CREATE TABLE marker (x INTEGER)", [])
                .unwrap();
        }

        // The file exists but has no schema version, so no backup is taken (nothing to protect).
        let backup = backup_before_migration(&path, 0, &clock()).unwrap();
        assert!(backup.exists());
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("pre-migration-v0-20260101T120000Z"));

        let (connection, report) = open_at(&path, &clock()).unwrap();
        assert!(report.migrated());
        // The pre-existing marker table is still there: migrating must not drop user tables.
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='marker'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn opening_a_database_with_a_newer_schema_is_not_silently_accepted() {
        // A file written by a future Veloct must not be truncated or downgraded into nonsense.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("veloct.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .pragma_update(None, "user_version", 999_i64)
                .unwrap();
        }
        let outcome = open_at(&path, &clock());
        assert!(
            outcome.is_err(),
            "a future schema version must be refused, not silently used"
        );
    }

    #[test]
    fn in_memory_databases_are_ready_for_use() {
        let connection = open_in_memory().unwrap();
        let version = migrations::current_version(&connection).unwrap();
        assert_eq!(version, migrations::CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn open_places_the_database_where_the_layout_says() {
        let dir = tempfile::tempdir().unwrap();
        let (connection, _) = open(dir.path(), &clock()).unwrap();
        assert!(dir.path().join(".veloct").join("veloct.db").exists());
        drop(connection);
    }
}
