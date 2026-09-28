//! Rebuilding the derived content index.
//!
//! `handoff.md` §2.3, §3.4, §19.1 and §20.1 all say the same thing from different angles: the
//! SQLite content tables are a **derived index**, Markdown is the truth, and dropping the index
//! must be recoverable by re-scanning. This module is that promise made executable, and
//! `docs/spec/02-test-matrix.md` row E1 is the test that holds it.
//!
//! The insert path is written so that rebuilding twice, or rebuilding after a wipe, produces
//! byte-identical rows. That matters because the release comparison in `handoff.md` §14.9 must not
//! see a difference that only exists because the index was regenerated.

use rusqlite::{params, Connection, OptionalExtension};

use veloct_core::{Result, VeloctError};

use crate::models::{ContentLinkRow, ContentParagraphRow, ContentRow};

/// Tables that hold derived content data and may be wiped and rebuilt.
pub const DERIVED_TABLES: [&str; 3] = ["content_links", "content_paragraphs", "contents"];

/// What a rebuild did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RebuildReport {
    pub contents: usize,
    pub paragraphs: usize,
    pub links: usize,
}

/// Whether the derived index looks usable.
///
/// Returns `Ok(false)` when a derived table is missing or unreadable, which is the signal to
/// rebuild from Markdown rather than to treat the project as damaged.
pub fn index_health(connection: &Connection) -> Result<bool> {
    for table in DERIVED_TABLES {
        let present: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = ?1",
                params![table],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| VeloctError::Database(format!("could not inspect {table}: {e}")))?;
        if present.is_none() {
            return Ok(false);
        }
        // A missing column or a corrupt page surfaces as an error here.
        if connection
            .prepare(&format!("SELECT * FROM {table} LIMIT 0"))
            .is_err()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A complete description of the index to write.
///
/// Built by the caller from Markdown, so this module never reads a Markdown file itself and never
/// becomes a second source of truth.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexSnapshot {
    pub contents: Vec<ContentRow>,
    pub paragraphs: Vec<ContentParagraphRow>,
    pub links: Vec<ContentLinkRow>,
}

impl IndexSnapshot {
    pub fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    /// Sort every collection into a canonical order.
    ///
    /// Order is what makes a rebuild comparable: two rebuilds of the same content must produce the
    /// same rows in the same positions.
    pub fn normalised(mut self) -> Self {
        self.contents.sort_by_key(|row| row.id);
        self.paragraphs.sort_by(|a, b| {
            a.content_id
                .cmp(&b.content_id)
                .then_with(|| a.paragraph_order.cmp(&b.paragraph_order))
                .then_with(|| a.paragraph_id.cmp(&b.paragraph_id))
        });
        self.links.sort_by(|a, b| {
            a.from_content_id
                .cmp(&b.from_content_id)
                .then_with(|| a.to_content_id.cmp(&b.to_content_id))
                .then_with(|| a.reference_location.cmp(&b.reference_location))
        });
        self
    }
}

/// Delete every derived row.
///
/// Deliberately scoped to the derived tables: configuration, releases, attachments and backups
/// live in the same database and must never be touched by an index rebuild.
pub fn wipe_index(connection: &mut Connection) -> Result<()> {
    let transaction = connection
        .transaction()
        .map_err(|e| VeloctError::Database(format!("could not start a transaction: {e}")))?;
    // Children first, so the foreign keys are satisfied even without cascade.
    for table in DERIVED_TABLES {
        transaction
            .execute(&format!("DELETE FROM {table}"), [])
            .map_err(|e| VeloctError::Database(format!("could not clear {table}: {e}")))?;
    }
    transaction
        .commit()
        .map_err(|e| VeloctError::Database(format!("could not commit the wipe: {e}")))?;
    Ok(())
}

/// Replace the derived index with `snapshot`.
///
/// The whole write is one transaction, so a reader never observes a half-rebuilt index and a
/// failure leaves the previous index intact.
pub fn rebuild(connection: &mut Connection, snapshot: IndexSnapshot) -> Result<RebuildReport> {
    let snapshot = snapshot.normalised();
    let report = RebuildReport {
        contents: snapshot.contents.len(),
        paragraphs: snapshot.paragraphs.len(),
        links: snapshot.links.len(),
    };

    let transaction = connection
        .transaction()
        .map_err(|e| VeloctError::Database(format!("could not start a transaction: {e}")))?;

    for table in DERIVED_TABLES {
        transaction
            .execute(&format!("DELETE FROM {table}"), [])
            .map_err(|e| VeloctError::Database(format!("could not clear {table}: {e}")))?;
    }

    for row in &snapshot.contents {
        transaction
            .execute(
                "INSERT INTO contents (
                    id, type, file_path, slug, title, summary, category, cover,
                    wip, wip_publish, wip_ever_published, tags, blocks,
                    created_at, updated_at, published_at, content_hash,
                    word_count, reading_minutes, indexed_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
                params![
                    row.id.to_text(),
                    row.content_type,
                    row.file_path,
                    row.slug,
                    row.title,
                    row.summary,
                    row.category,
                    row.cover,
                    row.wip as i64,
                    row.wip_publish as i64,
                    row.wip_ever_published as i64,
                    serde_json::to_string(&row.tags).unwrap_or_else(|_| "[]".to_string()),
                    serde_json::to_string(&row.blocks).unwrap_or_else(|_| "[]".to_string()),
                    row.created_at,
                    row.updated_at,
                    row.published_at,
                    row.content_hash,
                    row.word_count as i64,
                    row.reading_minutes as i64,
                    row.indexed_at,
                ],
            )
            .map_err(|e| {
                VeloctError::Database(format!("could not index content {}: {e}", row.id))
            })?;
    }

    for row in &snapshot.paragraphs {
        transaction
            .execute(
                "INSERT INTO content_paragraphs (content_id, paragraph_id, paragraph_order, content_hash)
                 VALUES (?1,?2,?3,?4)",
                params![
                    row.content_id,
                    row.paragraph_id,
                    row.paragraph_order as i64,
                    row.content_hash
                ],
            )
            .map_err(|e| {
                VeloctError::Database(format!(
                    "could not index paragraph {} of {}: {e}",
                    row.paragraph_id, row.content_id
                ))
            })?;
    }

    for row in &snapshot.links {
        transaction
            .execute(
                "INSERT INTO content_links
                    (from_content_id, to_content_id, to_paragraph_id, reference_location, status, diagnostic)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    row.from_content_id,
                    row.to_content_id,
                    row.to_paragraph_id,
                    row.reference_location,
                    row.status.as_str(),
                    row.diagnostic
                ],
            )
            .map_err(|e| VeloctError::Database(format!("could not index a content link: {e}")))?;
    }

    transaction
        .commit()
        .map_err(|e| VeloctError::Database(format!("could not commit the rebuild: {e}")))?;

    Ok(report)
}

/// Read the index back, for a test or a comparison against a freshly built snapshot.
pub fn read_index(connection: &Connection) -> Result<IndexSnapshot> {
    let mut snapshot = IndexSnapshot::default();

    let mut statement = connection
        .prepare(
            "SELECT id, type, file_path, slug, title, summary, category, cover,
                    wip, wip_publish, wip_ever_published, tags, blocks,
                    created_at, updated_at, published_at, content_hash,
                    word_count, reading_minutes, indexed_at
             FROM contents ORDER BY id",
        )
        .map_err(|e| VeloctError::Database(format!("could not read contents: {e}")))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ContentRow {
                id: row.get::<_, String>(0)?.parse().map_err(|_| {
                    rusqlite::Error::InvalidColumnType(
                        0,
                        "id".into(),
                        rusqlite::types::Type::Text,
                    )
                })?,
                content_type: row.get(1)?,
                file_path: row.get(2)?,
                slug: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                category: row.get(6)?,
                cover: row.get(7)?,
                wip: row.get::<_, i64>(8)? != 0,
                wip_publish: row.get::<_, i64>(9)? != 0,
                wip_ever_published: row.get::<_, i64>(10)? != 0,
                tags: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
                blocks: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
                published_at: row.get(15)?,
                content_hash: row.get(16)?,
                word_count: row.get::<_, i64>(17)?.max(0) as u64,
                reading_minutes: row.get::<_, i64>(18)?.max(0) as u64,
                indexed_at: row.get(19)?,
            })
        })
        .map_err(|e| VeloctError::Database(format!("could not read contents: {e}")))?;
    for row in rows {
        snapshot
            .contents
            .push(row.map_err(|e| VeloctError::Database(format!("could not read a content row: {e}")))?);
    }

    let mut statement = connection
        .prepare(
            "SELECT content_id, paragraph_id, paragraph_order, content_hash
             FROM content_paragraphs ORDER BY content_id, paragraph_order, paragraph_id",
        )
        .map_err(|e| VeloctError::Database(format!("could not read paragraphs: {e}")))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ContentParagraphRow {
                content_id: row.get(0)?,
                paragraph_id: row.get(1)?,
                paragraph_order: row.get::<_, i64>(2)?.max(0) as u32,
                content_hash: row.get(3)?,
            })
        })
        .map_err(|e| VeloctError::Database(format!("could not read paragraphs: {e}")))?;
    for row in rows {
        snapshot.paragraphs.push(
            row.map_err(|e| VeloctError::Database(format!("could not read a paragraph row: {e}")))?,
        );
    }

    let mut statement = connection
        .prepare(
            "SELECT from_content_id, to_content_id, to_paragraph_id, reference_location, status, diagnostic
             FROM content_links ORDER BY from_content_id, to_content_id, reference_location",
        )
        .map_err(|e| VeloctError::Database(format!("could not read links: {e}")))?;
    let rows = statement
        .query_map([], |row| {
            Ok(ContentLinkRow {
                from_content_id: row.get(0)?,
                to_content_id: row.get(1)?,
                to_paragraph_id: row.get(2)?,
                reference_location: row.get(3)?,
                status: row
                    .get::<_, String>(4)?
                    .parse()
                    .unwrap_or(crate::models::ReferenceLinkStatus::Broken),
                diagnostic: row.get(5)?,
            })
        })
        .map_err(|e| VeloctError::Database(format!("could not read links: {e}")))?;
    for row in rows {
        snapshot
            .links
            .push(row.map_err(|e| VeloctError::Database(format!("could not read a link row: {e}")))?);
    }

    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ContentRow, ReferenceLinkStatus};
    use veloct_core::StableId;

    fn content(id: StableId, slug: &str) -> ContentRow {
        ContentRow {
            id,
            content_type: "article".into(),
            file_path: format!("content/articles/{slug}.md"),
            slug: slug.into(),
            title: slug.to_uppercase(),
            summary: None,
            category: None,
            cover: None,
            wip: false,
            wip_publish: false,
            wip_ever_published: false,
            tags: vec!["rust".into()],
            blocks: Vec::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-02T00:00:00Z".into(),
            published_at: None,
            content_hash: format!("hash-{slug}"),
            word_count: 10,
            reading_minutes: 1,
            indexed_at: "2026-01-02T00:00:00Z".into(),
        }
    }

    fn snapshot() -> IndexSnapshot {
        let a = StableId::from_parts(1_700_000_000_000, 1);
        let b = StableId::from_parts(1_700_000_000_000, 2);
        IndexSnapshot {
            contents: vec![content(b, "beta"), content(a, "alpha")],
            paragraphs: vec![
                ContentParagraphRow {
                    content_id: b.to_text(),
                    paragraph_id: "p2".into(),
                    paragraph_order: 2,
                    content_hash: "h2".into(),
                },
                ContentParagraphRow {
                    content_id: a.to_text(),
                    paragraph_id: "p1".into(),
                    paragraph_order: 1,
                    content_hash: "h1".into(),
                },
            ],
            links: vec![ContentLinkRow {
                from_content_id: b.to_text(),
                to_content_id: a.to_text(),
                to_paragraph_id: Some("p1".into()),
                reference_location: "body:block:3".into(),
                status: ReferenceLinkStatus::Ok,
                diagnostic: None,
            }],
        }
    }

    #[test]
    fn a_fresh_database_has_a_usable_empty_index() {
        let connection = crate::db::open_in_memory().unwrap();
        assert!(index_health(&connection).unwrap());
        let index = read_index(&connection).unwrap();
        assert!(index.is_empty());
    }

    #[test]
    fn rebuilding_populates_every_derived_table() {
        let mut connection = crate::db::open_in_memory().unwrap();
        let report = rebuild(&mut connection, snapshot()).unwrap();
        assert_eq!(report.contents, 2);
        assert_eq!(report.paragraphs, 2);
        assert_eq!(report.links, 1);

        let index = read_index(&connection).unwrap();
        assert_eq!(index.contents.len(), 2);
        assert_eq!(index.paragraphs.len(), 2);
        assert_eq!(index.links.len(), 1);
        // Sorted by id, so the order does not depend on insertion order.
        assert_eq!(index.contents[0].slug, "alpha");
        assert_eq!(index.contents[1].slug, "beta");
    }

    #[test]
    fn rebuild_is_idempotent_and_produces_identical_rows() {
        // Row E1: the index must be rebuildable without the result depending on history.
        let mut connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut connection, snapshot()).unwrap();
        let first = read_index(&connection).unwrap();

        rebuild(&mut connection, snapshot()).unwrap();
        let second = read_index(&connection).unwrap();
        assert_eq!(first, second);

        // And after an explicit wipe, which is the damaged-index recovery path.
        wipe_index(&mut connection).unwrap();
        assert!(read_index(&connection).unwrap().is_empty());
        rebuild(&mut connection, snapshot()).unwrap();
        assert_eq!(read_index(&connection).unwrap(), first);
    }

    #[test]
    fn insertion_order_does_not_change_the_stored_index() {
        let mut first_connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut first_connection, snapshot()).unwrap();

        let mut reversed = snapshot();
        reversed.contents.reverse();
        reversed.paragraphs.reverse();
        let mut second_connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut second_connection, reversed).unwrap();

        assert_eq!(
            read_index(&first_connection).unwrap(),
            read_index(&second_connection).unwrap()
        );
    }

    #[test]
    fn a_rebuild_does_not_touch_anything_outside_the_derived_tables() {
        // Configuration, releases and attachments share the database and must survive.
        let mut connection = crate::db::open_in_memory().unwrap();
        connection
            .execute(
                "INSERT INTO site_config (key, value, updated_at) VALUES ('site.name','Veloct','now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO deploy_targets (id, kind, name, enabled, created_at)
                 VALUES ('t','cloudflare','CF',1,'now')",
                [],
            )
            .unwrap();

        rebuild(&mut connection, snapshot()).unwrap();
        wipe_index(&mut connection).unwrap();

        let name: String = connection
            .query_row(
                "SELECT value FROM site_config WHERE key='site.name'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "Veloct");
        let targets: i64 = connection
            .query_row("SELECT COUNT(*) FROM deploy_targets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(targets, 1);
    }

    #[test]
    fn a_failed_rebuild_leaves_the_previous_index_intact() {
        // A duplicate primary key is the realistic failure; the transaction must roll back.
        let mut connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut connection, snapshot()).unwrap();
        let good = read_index(&connection).unwrap();

        let mut broken = snapshot();
        broken.contents.push(broken.contents[0].clone());
        let error = rebuild(&mut connection, broken).unwrap_err();
        assert_eq!(error.code(), "DATABASE");

        assert_eq!(read_index(&connection).unwrap(), good);
    }

    #[test]
    fn index_health_reports_a_missing_table_as_unhealthy() {
        // The damaged-index path from handoff 20.1.
        let mut connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut connection, snapshot()).unwrap();
        connection
            .execute("DROP TABLE content_paragraphs", [])
            .unwrap();
        assert!(!index_health(&connection).unwrap());
    }

    #[test]
    fn foreign_keys_remove_paragraphs_with_their_content() {
        let mut connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut connection, snapshot()).unwrap();

        let victim = read_index(&connection).unwrap().contents[0].id.to_text();
        connection
            .execute("DELETE FROM contents WHERE id = ?1", params![victim])
            .unwrap();

        let remaining: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM content_paragraphs WHERE content_id = ?1",
                params![victim],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0, "cascade must remove orphaned paragraphs");
    }

    #[test]
    fn flags_and_json_columns_round_trip_faithfully() {
        let mut connection = crate::db::open_in_memory().unwrap();
        let mut one = snapshot();
        one.contents.truncate(1);
        one.paragraphs.clear();
        one.links.clear();
        let row = &mut one.contents[0];
        row.wip = true;
        row.wip_publish = true;
        row.wip_ever_published = true;
        row.tags = vec!["a".into(), "b".into()];
        row.blocks = vec!["block:1".into()];
        row.summary = Some("a summary".into());
        row.published_at = Some("2026-02-01T00:00:00Z".into());

        let expected = row.clone();
        rebuild(&mut connection, one).unwrap();
        let index = read_index(&connection).unwrap();
        assert_eq!(index.contents[0], expected);
    }

    #[test]
    fn links_round_trip_with_their_status_and_diagnostic() {
        let mut connection = crate::db::open_in_memory().unwrap();
        let mut one = snapshot();
        one.links[0].status = ReferenceLinkStatus::Broken;
        one.links[0].diagnostic = Some("paragraph missing".into());
        let expected = one.links[0].clone();

        rebuild(&mut connection, one).unwrap();
        let index = read_index(&connection).unwrap();
        assert_eq!(index.links[0], expected);
    }

    #[test]
    fn a_content_hash_change_is_visible_in_the_index() {
        // The editor uses this to detect an external modification (handoff 20.2).
        let mut connection = crate::db::open_in_memory().unwrap();
        rebuild(&mut connection, snapshot()).unwrap();
        let before = read_index(&connection).unwrap();

        let mut changed = snapshot();
        changed.contents[0].content_hash = "different".into();
        rebuild(&mut connection, changed).unwrap();
        let after = read_index(&connection).unwrap();

        assert_ne!(before, after);
    }
}
