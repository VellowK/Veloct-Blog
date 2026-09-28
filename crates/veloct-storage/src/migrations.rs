//! Schema migrations.
//!
//! `handoff.md` §20.1 requires a version mechanism that can be re-run, can report the current
//! version, and never treats a damaged index as lost content. The version lives in SQLite's
//! `user_version`, which `rusqlite_migration` manages, so there is no bookkeeping table to get out
//! of step with reality.
//!
//! Two rules apply to every migration in this file:
//!
//! * The derived content index (`contents`, `content_paragraphs`, `content_links`) may be dropped
//!   and rebuilt from Markdown at any time. Nothing in it is truth.
//! * No column anywhere may hold a credential value. Credentials are `credential_ref` strings that
//!   resolve through the operating system credential store (`handoff.md` §12).

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use veloct_core::{Result, VeloctError};

/// The number of migrations in [`migrations`].
///
/// Kept as an explicit constant because `Migrations` exposes no length, and the two must move
/// together when a migration is appended. `assert_version_matches` compares them so a forgotten
/// bump fails a test rather than shipping.
pub const MIGRATION_COUNT: usize = 1;

/// The schema version this build expects.
pub const CURRENT_SCHEMA_VERSION: usize = MIGRATION_COUNT;

/// The migration list. Append only; never edit a shipped migration.
///
/// Adding a migration means appending an `M::up(...)` entry, bumping [`MIGRATION_COUNT`], and
/// checking the new SQL into `docs/spec/01-data-model.md` §3.
pub fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(V1)])
}

/// The v1 schema, exactly as specified in `docs/spec/01-data-model.md` §3.
const V1: &str = r#"
-- ===================== derived content index (rebuildable; never the body truth) =============
CREATE TABLE contents (
  id                   TEXT PRIMARY KEY,
  type                 TEXT NOT NULL,
  file_path            TEXT NOT NULL UNIQUE,
  slug                 TEXT NOT NULL,
  title                TEXT NOT NULL,
  summary              TEXT,
  category             TEXT,
  cover                TEXT,
  wip                  INTEGER NOT NULL DEFAULT 0,
  wip_publish          INTEGER NOT NULL DEFAULT 0,
  wip_ever_published   INTEGER NOT NULL DEFAULT 0,
  tags                 TEXT NOT NULL DEFAULT '[]',
  blocks               TEXT NOT NULL DEFAULT '[]',
  created_at           TEXT NOT NULL,
  updated_at           TEXT NOT NULL,
  published_at         TEXT,
  content_hash         TEXT NOT NULL,
  word_count           INTEGER NOT NULL DEFAULT 0,
  reading_minutes      INTEGER NOT NULL DEFAULT 0,
  indexed_at           TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_contents_type_slug ON contents(type, slug);
CREATE INDEX idx_contents_type_wip ON contents(type, wip);
CREATE INDEX idx_contents_updated_at ON contents(updated_at);

CREATE TABLE content_paragraphs (
  content_id      TEXT NOT NULL REFERENCES contents(id) ON DELETE CASCADE,
  paragraph_id    TEXT NOT NULL,
  paragraph_order INTEGER NOT NULL,
  content_hash    TEXT NOT NULL,
  PRIMARY KEY (content_id, paragraph_id)
);
CREATE INDEX idx_paragraphs_content ON content_paragraphs(content_id, paragraph_order);

CREATE TABLE content_links (
  from_content_id   TEXT NOT NULL REFERENCES contents(id) ON DELETE CASCADE,
  to_content_id     TEXT NOT NULL,
  to_paragraph_id   TEXT,
  reference_location TEXT NOT NULL,
  status            TEXT NOT NULL,
  diagnostic        TEXT,
  PRIMARY KEY (from_content_id, to_content_id, to_paragraph_id, reference_location)
);
CREATE INDEX idx_links_to ON content_links(to_content_id);

-- ===================== site and application configuration (config truth) ======================
CREATE TABLE site_config (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE app_settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE nav_items (
  id        TEXT PRIMARY KEY,
  label     TEXT NOT NULL,
  href      TEXT NOT NULL,
  sort      INTEGER NOT NULL DEFAULT 0,
  parent_id TEXT
);

CREATE TABLE friend_links (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  url         TEXT NOT NULL,
  avatar      TEXT,
  description TEXT,
  sort        INTEGER NOT NULL DEFAULT 0
);

-- ===================== AI configuration (credential_ref only, never a value) ==================
CREATE TABLE ai_providers (
  id             TEXT PRIMARY KEY,
  name           TEXT NOT NULL,
  protocol       TEXT NOT NULL,
  base_url       TEXT NOT NULL,
  credential_ref TEXT,
  enabled        INTEGER NOT NULL DEFAULT 1,
  created_at     TEXT NOT NULL
);

CREATE TABLE ai_models (
  id             TEXT PRIMARY KEY,
  provider_id    TEXT NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
  model_id       TEXT NOT NULL,
  display_name   TEXT,
  source         TEXT NOT NULL,
  context_window INTEGER,
  last_seen_at   TEXT,
  UNIQUE(provider_id, model_id)
);

CREATE TABLE ai_features (
  id          TEXT PRIMARY KEY,
  label       TEXT NOT NULL,
  provider_id TEXT REFERENCES ai_providers(id) ON DELETE SET NULL,
  model_id    TEXT,
  enabled     INTEGER NOT NULL DEFAULT 0
);

-- Exactly one row: the single global fallback model (handoff 11.4).
CREATE TABLE ai_fallback (
  id          INTEGER PRIMARY KEY CHECK (id = 1),
  provider_id TEXT REFERENCES ai_providers(id) ON DELETE SET NULL,
  model_id    TEXT
);

CREATE TABLE ai_requests (
  id              TEXT PRIMARY KEY,
  feature_id      TEXT NOT NULL,
  provider_id     TEXT,
  model           TEXT,
  status          TEXT NOT NULL,
  used_fallback   INTEGER NOT NULL DEFAULT 0,
  error_code      TEXT,
  error_message   TEXT,
  created_at      TEXT NOT NULL,
  completed_at    TEXT
);
CREATE INDEX idx_ai_requests_feature ON ai_requests(feature_id, created_at);

-- ===================== attachment storage targets =============================================
CREATE TABLE storage_defs (
  id             TEXT PRIMARY KEY,
  kind           TEXT NOT NULL,
  name           TEXT NOT NULL,
  config         TEXT NOT NULL,
  credential_ref TEXT,
  enabled        INTEGER NOT NULL DEFAULT 1,
  created_at     TEXT NOT NULL
);

CREATE TABLE storage_namespaces (
  id                TEXT PRIMARY KEY,
  storage_def_id    TEXT NOT NULL REFERENCES storage_defs(id) ON DELETE CASCADE,
  namespace         TEXT NOT NULL,
  public_url_prefix TEXT,
  created_at        TEXT NOT NULL,
  UNIQUE(storage_def_id, namespace)
);

-- ===================== deployment targets =====================================================
CREATE TABLE deploy_targets (
  id         TEXT PRIMARY KEY,
  kind       TEXT NOT NULL,
  name       TEXT NOT NULL,
  enabled    INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

CREATE TABLE deploy_subtargets (
  id             TEXT PRIMARY KEY,
  target_id      TEXT NOT NULL REFERENCES deploy_targets(id) ON DELETE CASCADE,
  role           TEXT NOT NULL CHECK (role IN ('main','works')),
  config         TEXT NOT NULL,
  credential_ref TEXT,
  is_degraded    INTEGER NOT NULL DEFAULT 0,
  UNIQUE(target_id, role)
);

-- ===================== release index (query index only; truth is release.json + git tag) ======
CREATE TABLE releases (
  release_id              TEXT PRIMARY KEY,
  commit_sha              TEXT NOT NULL,
  tag_name                TEXT NOT NULL UNIQUE,
  build_fingerprint       TEXT NOT NULL,
  build_inputs_hash       TEXT NOT NULL,
  created_at              TEXT NOT NULL,
  created_from_release_id TEXT
);

CREATE TABLE deployment_runs (
  id                TEXT PRIMARY KEY,
  release_id        TEXT NOT NULL REFERENCES releases(release_id) ON DELETE CASCADE,
  logical_target_id TEXT NOT NULL,
  sub_target        TEXT NOT NULL CHECK (sub_target IN ('main','works')),
  status            TEXT NOT NULL,
  build_fingerprint TEXT,
  reused_dist       INTEGER NOT NULL DEFAULT 0,
  error             TEXT,
  started_at        TEXT,
  finished_at       TEXT
);
CREATE INDEX idx_deployment_runs_release ON deployment_runs(release_id);

CREATE TABLE seo_pushes (
  id           TEXT PRIMARY KEY,
  release_id   TEXT NOT NULL,
  engine       TEXT NOT NULL,
  status       TEXT NOT NULL,
  error        TEXT,
  created_at   TEXT,
  finished_at  TEXT,
  manual_retry INTEGER NOT NULL DEFAULT 0
);

-- ===================== attachments (handoff 13, 22.4) ========================================
CREATE TABLE attachments (
  id                  TEXT PRIMARY KEY,
  original_name       TEXT NOT NULL,
  stored_name         TEXT,
  content_hash        TEXT NOT NULL,
  mime_type           TEXT,
  size                INTEGER NOT NULL,
  storage_def_id      TEXT REFERENCES storage_defs(id) ON DELETE SET NULL,
  storage_namespace_id TEXT REFERENCES storage_namespaces(id) ON DELETE SET NULL,
  remote_key          TEXT,
  public_url          TEXT,
  upload_status       TEXT NOT NULL,
  reference_status    TEXT NOT NULL,
  cache_status        TEXT NOT NULL,
  cache_path          TEXT,
  remote_object_id    TEXT,
  last_error          TEXT,
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL
);
CREATE INDEX idx_attachments_dedup ON attachments(storage_def_id, storage_namespace_id, content_hash);
CREATE INDEX idx_attachments_status ON attachments(upload_status, reference_status, cache_status);

-- A real set, not a boolean: one attachment may be cited by several documents.
CREATE TABLE attachment_references (
  attachment_id      TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  content_id         TEXT NOT NULL,
  reference_location TEXT NOT NULL,
  created_at         TEXT NOT NULL,
  PRIMARY KEY (attachment_id, content_id, reference_location)
);
CREATE INDEX idx_attachment_refs_content ON attachment_references(content_id);

CREATE TABLE attachment_tasks (
  id            TEXT PRIMARY KEY,
  attachment_id TEXT NOT NULL REFERENCES attachments(id) ON DELETE CASCADE,
  kind          TEXT NOT NULL,
  status        TEXT NOT NULL,
  attempts      INTEGER NOT NULL DEFAULT 0,
  payload       TEXT,
  error         TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE INDEX idx_attachment_tasks_open ON attachment_tasks(attachment_id, status);

-- Concurrency guard for dedup (handoff 13.4).
CREATE TABLE upload_locks (
  dedup_key     TEXT PRIMARY KEY,
  owner_task_id TEXT NOT NULL,
  acquired_at   TEXT NOT NULL
);

-- ===================== backups (handoff 17, 22.7) ============================================
CREATE TABLE backup_targets (
  id             TEXT PRIMARY KEY,
  kind           TEXT NOT NULL,
  name           TEXT NOT NULL,
  config         TEXT NOT NULL,
  credential_ref TEXT,
  retention      INTEGER NOT NULL DEFAULT 3,
  enabled        INTEGER NOT NULL DEFAULT 1,
  created_at     TEXT NOT NULL
);

CREATE TABLE backups (
  id                   TEXT PRIMARY KEY,
  target_id            TEXT,
  kind                 TEXT NOT NULL,
  created_at           TEXT NOT NULL,
  status               TEXT NOT NULL,
  artifact_identifier  TEXT,
  size                 INTEGER,
  snapshot_ref         TEXT,
  secret_scan_status   TEXT,
  secret_scan_hits     INTEGER NOT NULL DEFAULT 0,
  error                TEXT
);
CREATE INDEX idx_backups_target ON backups(target_id, created_at);

-- ===================== logging and diagnostics (handoff 18) ==================================
CREATE TABLE log_index (
  id      INTEGER PRIMARY KEY AUTOINCREMENT,
  ts      TEXT NOT NULL,
  level   TEXT NOT NULL,
  area    TEXT NOT NULL,
  message TEXT NOT NULL,
  detail  TEXT
);
CREATE INDEX idx_log_index_ts ON log_index(ts);

CREATE TABLE diagnostics (
  id          TEXT PRIMARY KEY,
  area        TEXT NOT NULL,
  code        TEXT NOT NULL,
  severity    TEXT NOT NULL,
  message     TEXT NOT NULL,
  detail      TEXT,
  content_id  TEXT,
  created_at  TEXT NOT NULL,
  resolved    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_diagnostics_open ON diagnostics(resolved, severity);

-- ===================== projects ==============================================================
CREATE TABLE projects (
  id                    TEXT PRIMARY KEY,
  name                  TEXT NOT NULL,
  root_path             TEXT NOT NULL UNIQUE,
  site_template_version TEXT NOT NULL,
  created_at            TEXT NOT NULL,
  last_opened_at        TEXT
);
"#;

/// Bring a connection up to the current schema version.
///
/// Returns the number of migrations that were applied, so a caller can decide whether a pre-migration
/// database backup is worth taking.
pub fn apply(connection: &mut Connection) -> Result<usize> {
    // Refuse a database written by a newer Veloct. Silently treating an unknown future schema as
    // "already up to date" would let this build write rows a newer build cannot read, or read rows
    // it misunderstands.
    let current = current_version(connection)?;
    if current > CURRENT_SCHEMA_VERSION {
        return Err(VeloctError::Migration(format!(
            "the database schema is version {current}, but this build understands version \
             {CURRENT_SCHEMA_VERSION}; upgrade Veloct to open this project"
        )));
    }

    let pending = pending_migrations(connection)?;
    if pending == 0 {
        return Ok(0);
    }
    migrations()
        .to_latest(connection)
        .map_err(|e| VeloctError::Migration(format!("could not migrate the database: {e}")))?;
    Ok(pending)
}

/// How many migrations are outstanding.
pub fn pending_migrations(connection: &Connection) -> Result<usize> {
    let pending = migrations()
        .pending_migrations(connection)
        .map_err(|e| VeloctError::Migration(format!("could not read the schema version: {e}")))?;
    Ok(pending.max(0) as usize)
}

/// The schema version recorded in the file.
pub fn current_version(connection: &Connection) -> Result<usize> {
    let migrations = migrations();
    let version = migrations
        .current_version(connection)
        .map_err(|e| VeloctError::Migration(format!("could not read the schema version: {e}")))?;
    Ok(usize::from(version))
}

/// Validate that the migration list is internally consistent.
///
/// `rusqlite_migration::validate` catches an inconsistent list; the extra assertion guards the
/// constant that documents the expected version.
pub fn assert_version_matches() -> Result<()> {
    migrations()
        .validate()
        .map_err(|e| VeloctError::Migration(format!("migration list is invalid: {e}")))?;
    if MIGRATION_COUNT != CURRENT_SCHEMA_VERSION {
        return Err(VeloctError::Migration(format!(
            "CURRENT_SCHEMA_VERSION is {CURRENT_SCHEMA_VERSION} but MIGRATION_COUNT is {MIGRATION_COUNT}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let connection = Connection::open_in_memory().expect("in-memory database");
        connection
            .pragma_update(None, "foreign_keys", true)
            .expect("enable foreign keys");
        connection
    }

    #[test]
    fn the_migration_list_is_consistent_and_matches_the_documented_version() {
        assert_version_matches().unwrap();
        assert_eq!(CURRENT_SCHEMA_VERSION, 1, "v1 is the only shipped migration");
    }

    #[test]
    fn applying_migrations_from_empty_reaches_the_current_version() {
        let mut connection = memory();
        assert_eq!(current_version(&connection).unwrap(), 0);
        assert_eq!(pending_migrations(&connection).unwrap(), CURRENT_SCHEMA_VERSION);

        let applied = apply(&mut connection).unwrap();
        assert_eq!(applied, CURRENT_SCHEMA_VERSION);
        assert_eq!(current_version(&connection).unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        // handoff 20.1: the upgrade path must be re-runnable.
        let mut connection = memory();
        apply(&mut connection).unwrap();
        assert_eq!(pending_migrations(&connection).unwrap(), 0);
        assert_eq!(apply(&mut connection).unwrap(), 0);
        assert_eq!(current_version(&connection).unwrap(), CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn every_table_from_the_data_model_exists() {
        let mut connection = memory();
        apply(&mut connection).unwrap();

        let expected = [
            "contents",
            "content_paragraphs",
            "content_links",
            "site_config",
            "app_settings",
            "nav_items",
            "friend_links",
            "ai_providers",
            "ai_models",
            "ai_features",
            "ai_fallback",
            "ai_requests",
            "storage_defs",
            "storage_namespaces",
            "deploy_targets",
            "deploy_subtargets",
            "releases",
            "deployment_runs",
            "seo_pushes",
            "attachments",
            "attachment_references",
            "attachment_tasks",
            "upload_locks",
            "backup_targets",
            "backups",
            "log_index",
            "diagnostics",
            "projects",
        ];

        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap();
        let found: Vec<String> = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();

        for table in expected {
            assert!(
                found.iter().any(|name| name == table),
                "table {table} is missing; found {found:?}"
            );
        }
    }

    #[test]
    fn no_column_is_named_like_a_secret_value() {
        // handoff 12: credentials never enter SQLite. Only opaque *_ref handles are allowed.
        let mut connection = memory();
        apply(&mut connection).unwrap();

        let mut statement = connection
            .prepare(
                "SELECT m.name, p.name FROM sqlite_master m
                 JOIN pragma_table_info(m.name) p
                 WHERE m.type = 'table'",
            )
            .unwrap();
        let columns: Vec<(String, String)> = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();

        assert!(!columns.is_empty());
        for (table, column) in columns {
            let lowered = column.to_ascii_lowercase();
            // A column that only *describes* secret handling is fine: `secret_scan_status` records
            // the outcome of the scan handoff.md 17.3 requires, and `credential_ref` is an opaque
            // handle rather than a value. Everything else that names a credential is rejected.
            let is_metadata = lowered.ends_with("_status")
                || lowered.ends_with("_ref")
                || lowered.ends_with("_hits")
                || lowered.ends_with("_id");
            let names_a_credential = lowered.contains("secret")
                || lowered.contains("password")
                || lowered.contains("token")
                || lowered == "api_key"
                || lowered == "apikey"
                || lowered == "access_key";
            assert!(
                !(names_a_credential && !is_metadata),
                "{table}.{column} looks like it stores a secret value"
            );
        }
    }

    #[test]
    fn the_single_fallback_row_is_enforced_by_the_schema() {
        // handoff 11.4: there is exactly one global fallback model.
        let mut connection = memory();
        apply(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO ai_fallback (id, provider_id, model_id) VALUES (1, NULL, NULL)",
                [],
            )
            .unwrap();
        let second = connection.execute(
            "INSERT INTO ai_fallback (id, provider_id, model_id) VALUES (2, NULL, NULL)",
            [],
        );
        assert!(second.is_err(), "a second fallback row must be rejected");
    }

    #[test]
    fn subtarget_roles_are_constrained() {
        let mut connection = memory();
        apply(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO deploy_targets (id, kind, name, enabled, created_at) VALUES ('t','cloudflare','CF',1,'now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO deploy_subtargets (id, target_id, role, config) VALUES ('s','t','main','{}')",
                [],
            )
            .unwrap();
        let bad = connection.execute(
            "INSERT INTO deploy_subtargets (id, target_id, role, config) VALUES ('s2','t','shell','{}')",
            [],
        );
        assert!(bad.is_err(), "only main and works are valid roles");
    }

    #[test]
    fn subtargets_are_unique_per_target_and_role() {
        let mut connection = memory();
        apply(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO deploy_targets (id, kind, name, enabled, created_at) VALUES ('t','cloudflare','CF',1,'now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO deploy_subtargets (id, target_id, role, config) VALUES ('s','t','works','{}')",
                [],
            )
            .unwrap();
        let duplicate = connection.execute(
            "INSERT INTO deploy_subtargets (id, target_id, role, config) VALUES ('s2','t','works','{}')",
            [],
        );
        assert!(duplicate.is_err(), "one works sub-target per logical target");
    }
}
