//! The attachment cache pool, as a service.
//!
//! `crates/veloct-attach` has the whole attachment model — the three independent state axes, the reference
//! sets, the dedup rules, the storage adapters — and until this module existed **the application never
//! called any of it**. The `attachments` table was dead schema. `handoff.md` §13 is a locked section about
//! adding a picture to an article, so a model with no entry point is not an implementation of it.
//!
//! What this service owns, and why each piece is here rather than in the library:
//!
//! * **The cache directory.** `handoff.md` §13.2 puts the bytes under `.attachments-cache/<sha256>/<name>`.
//!   `veloct-attach` computes the RELATIVE path; resolving it against a project root and refusing to escape
//!   is a filesystem concern, and it is done through the library's own `checked_cache_path` so there is one
//!   definition of a safe path rather than two.
//! * **Durability.** The transaction serialises itself with `to_state`, which is the durable record §13.3
//!   requires before anything touches the Markdown. This writes it atomically.
//! * **The database row.** `handoff.md` §22.4's `attachments` table exists so the index can answer "which
//!   attachments does this project have" without reading every file.
//!
//! One rule shapes the whole file: **adding a file writes the bytes and the record, and touches no
//! Markdown.** §13.3's order is cache, then record, then apply to the document, and a service that rewrote
//! an article as a side effect of adding a picture would make the order meaningless.

use std::path::{Path, PathBuf};

use veloct_attach::{
    Attachment, AttachmentId, AttachmentState, AttachmentTransaction, CachePool, DedupKey, InsertOutcome,
    ReferenceIndex, StorageTargetId,
};
use veloct_core::{Clock, Result, VeloctError};
use veloct_storage::open as open_database;

/// The directory the cached bytes live in, from `docs/spec/01-data-model.md` §1.
pub const CACHE_DIR: &str = ".attachments-cache";

/// How many bytes one attachment may be.
///
/// A ceiling exists because the pool keeps whole files in memory while hashing and uploading, and an
/// unbounded add is a way to run the application out of memory with one click. 64 MiB is far above any
/// reasonable image and far below a problem.
pub const MAX_ATTACHMENT_BYTES: u64 = 64 * 1024 * 1024;

/// One attachment, as the window lists it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AttachmentView {
    pub id: String,
    pub original_name: String,
    pub content_hash: String,
    pub size_bytes: u64,
    /// The cache file's relative path, so the window can show where the bytes are.
    pub cache_path: String,
    pub storage_target_id: String,
    pub storage_namespace: String,
    pub remote_url: Option<String>,
    pub remote_key: Option<String>,
    /// The independent state axes, kept separate (`handoff.md` §13.1).
    pub upload_status: String,
    pub reference_status: String,
    pub cache_status: String,
    pub last_error: Option<String>,
    pub created_at: String,
    /// How many documents reference it, so the UI can warn before a removal.
    pub reference_count: usize,
}

impl AttachmentView {
    fn of(attachment: &Attachment, references: &ReferenceIndex) -> Self {
        let state = attachment.state;
        Self {
            id: attachment.id.as_str().to_string(),
            original_name: attachment.original_name.clone(),
            content_hash: attachment.content_hash.clone(),
            size_bytes: attachment.size_bytes,
            cache_path: attachment.cache_path.clone(),
            storage_target_id: attachment.storage_target_id.as_str().to_string(),
            storage_namespace: attachment.storage_namespace.clone(),
            remote_url: attachment.remote_url.clone(),
            remote_key: attachment.remote_key.clone(),
            upload_status: state.upload_status.as_str().to_string(),
            reference_status: state.reference_status.as_str().to_string(),
            cache_status: state.cache_status.as_str().to_string(),
            last_error: attachment.last_error.clone(),
            created_at: attachment.created_at.clone(),
            reference_count: references.count_for(&attachment.id),
        }
    }

    /// The Markdown a caller would insert for this attachment.
    ///
    /// A remote URL when one exists, and the local cache path otherwise, so an article written before an
    /// upload is not broken and the difference is visible in the Markdown rather than implied.
    pub fn markdown(&self) -> String {
        let src = self
            .remote_url
            .clone()
            .unwrap_or_else(|| format!("/{CACHE_DIR}/{}", self.cache_path));
        format!("![{}]({})", self.original_name, src)
    }
}

/// What an add produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AddOutcome {
    /// The bytes were new to this target and namespace.
    Added,
    /// The same bytes were already present for this target and namespace, so the existing record is reused
    /// rather than a duplicate created (`handoff.md` §13.4).
    Reused,
}

/// The attachment service for one project.
pub struct AttachmentService {
    project_root: PathBuf,
    /// The durable state. A transaction rather than a bare pool, because it also carries the remote object
    /// records §13.3 requires.
    transaction: AttachmentTransaction,
    /// Where the state is written.
    state_path: PathBuf,
}

impl AttachmentService {
    /// Open the service, reading the state if it exists.
    pub fn open(project_root: impl Into<PathBuf>) -> Result<Self> {
        let project_root = project_root.into();
        let state_path = project_root
            .join(veloct_core::layout::VELOCT_DIR)
            .join("attachments.json");

        // An unreadable state is an ERROR rather than a fresh start: silently discarding it would lose the
        // remote object records, and with them the knowledge of which URLs articles already point at.
        let transaction = match veloct_core::workdir::read_text_optional(&state_path)? {
            Some(text) => AttachmentTransaction::from_state(&text)?,
            None => AttachmentTransaction::new(),
        };

        Ok(Self {
            project_root,
            transaction,
            state_path,
        })
    }

    /// The cache directory, created if needed.
    pub fn cache_directory(&self) -> Result<PathBuf> {
        let directory = self.project_root.join(CACHE_DIR);
        std::fs::create_dir_all(&directory).map_err(|error| VeloctError::io(&directory, error))?;
        Ok(directory)
    }

    /// Every attachment, ordered by id so the list is stable.
    pub fn list(&self) -> Vec<AttachmentView> {
        let references = self.transaction.references();
        let mut views: Vec<AttachmentView> = self
            .transaction
            .pool()
            .iter()
            .map(|attachment| AttachmentView::of(attachment, references))
            .collect();
        views.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        views
    }

    /// One attachment.
    pub fn get(&self, id: &str) -> Option<AttachmentView> {
        let references = self.transaction.references();
        self.transaction
            .pool()
            .get(&AttachmentId::new(id))
            .map(|attachment| AttachmentView::of(attachment, references))
    }

    /// Add a file to the pool.
    ///
    /// The order is §13.3's: the bytes reach the cache first, then the record, and the Markdown is untouched
    /// so the caller decides when to apply it.
    ///
    /// The suffix is generated ONCE and stored on the record, because §13.10's retry has to produce the same
    /// remote name: a second suffix would orphan the URL an article already points at.
    pub fn add_file(
        &mut self,
        source: &Path,
        target_id: &str,
        namespace: &str,
        now: &str,
        entropy: &[u8; veloct_attach::SUFFIX_LENGTH],
    ) -> Result<(AttachmentView, AddOutcome)> {
        let metadata = std::fs::metadata(source).map_err(|error| VeloctError::io(source, error))?;
        if !metadata.is_file() {
            return Err(VeloctError::Invalid(format!(
                "{} is not a file",
                source.display()
            )));
        }
        if metadata.len() > MAX_ATTACHMENT_BYTES {
            return Err(VeloctError::Invalid(format!(
                "{} is {} bytes, above the {} byte limit for one attachment",
                source.display(),
                metadata.len(),
                MAX_ATTACHMENT_BYTES
            )));
        }

        let original_name = source
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .ok_or_else(|| {
                VeloctError::Invalid(format!("{} has no file name", source.display()))
            })?;
        veloct_attach::validate_original_name(&original_name)?;

        let bytes = std::fs::read(source).map_err(|error| VeloctError::io(source, error))?;
        let content_hash = veloct_core::sha256_hex(&bytes);

        let target = StorageTargetId::new(target_id);
        let pool = self.transaction.pool();

        // §13.4: the same bytes for the same target and namespace reuse the existing record, INCLUDING its
        // suffix. Reusing the suffix is what keeps the cache path and the remote name stable, so an add of a
        // file that is already there is genuinely a no-op rather than a second copy under a new name.
        let dedup = DedupKey::new(target.clone(), namespace, &content_hash);
        if let Some(existing_id) = pool
            .ids()
            .into_iter()
            .find(|id| {
                pool.get(id)
                    .map(|existing| existing.dedup_key() == dedup)
                    .unwrap_or(false)
            })
            .cloned()
        {
            let existing = pool
                .get(&existing_id)
                .expect("the id came from the pool")
                .clone();
            let cache_file = existing.cache_file(&self.project_root);
            // The bytes may be missing even though the record exists, for example after a cache cleanup. The
            // record is still the truth, so the file is restored rather than a duplicate created.
            if !cache_file.is_file() {
                veloct_core::workdir::ensure_parent(&cache_file)?;
                std::fs::write(&cache_file, &bytes)
                    .map_err(|error| VeloctError::io(&cache_file, error))?;
            }
            let view = AttachmentView::of(&existing, self.transaction.references());
            return Ok((view, AddOutcome::Reused));
        }

        let suffix = veloct_attach::random_suffix(entropy);
        let cache_path = CachePool::cache_path_for(&content_hash, &original_name, &suffix);

        let attachment = Attachment {
            id: AttachmentId::new(veloct_core::ids::StableId::new().to_text()),
            original_name,
            content_hash,
            size_bytes: metadata.len(),
            cache_path,
            // §13.1's axes start where the file actually is: cached, not uploaded, not referenced yet.
            state: AttachmentState::default(),
            storage_target_id: target,
            storage_namespace: namespace.to_string(),
            remote_url: None,
            remote_key: None,
            remote_suffix: suffix,
            created_at: now.to_string(),
            last_error: None,
        };

        // The bytes go to the cache through the library's own path check, so a crafted `cache_path` cannot
        // be the way out of the project directory.
        let checked = veloct_attach::checked_cache_path(&self.project_root, &attachment.cache_path)?;
        veloct_core::workdir::ensure_parent(&checked)?;
        std::fs::write(&checked, &bytes).map_err(|error| VeloctError::io(&checked, error))?;

        let inserted = self.transaction.pool_mut().insert(attachment);
        // The insert can still find an existing record; the pool is the authority on dedup, and asking it
        // rather than duplicating its rule is the point of the call.
        let id = inserted.id().clone();
        match inserted {
            InsertOutcome::Reused { .. } => {
                let existing = self
                    .transaction
                    .pool()
                    .get(&id)
                    .expect("the pool returned this id")
                    .clone();
                let view = AttachmentView::of(&existing, self.transaction.references());
                return Ok((view, AddOutcome::Reused));
            }
            InsertOutcome::Inserted { .. } => {}
        }

        let attachment = self
            .transaction
            .pool()
            .get(&id)
            .expect("the pool just inserted this id")
            .clone();

        let view = AttachmentView::of(&attachment, self.transaction.references());
        self.save()?;
        Ok((view, AddOutcome::Added))
    }

    /// The Markdown for an attachment.
    pub fn markdown_for(&self, id: &str) -> Result<String> {
        self.get(id)
            .map(|view| view.markdown())
            .ok_or_else(|| VeloctError::Invalid(format!("no attachment {id}")))
    }

    /// Remove an attachment from the pool, and its cached bytes.
    ///
    /// The cache file is removed only when no OTHER record shares it, which happens because two suffixes for
    /// the same content live in the same hash directory but have different names — and because a reused
    /// record shares one. Removing a file another record still points at would break it silently.
    pub fn remove(&mut self, id: &str) -> Result<AttachmentView> {
        let attachment_id = AttachmentId::new(id);
        let attachment = self
            .transaction
            .pool()
            .get(&attachment_id)
            .cloned()
            .ok_or_else(|| VeloctError::Invalid(format!("no attachment {id}")))?;

        let view = AttachmentView::of(&attachment, self.transaction.references());
        let cache_file = attachment.cache_file(&self.project_root);
        let still_used = self
            .transaction
            .pool()
            .iter()
            .any(|other| other.id != attachment.id && other.cache_path == attachment.cache_path);

        self.transaction.pool_mut().remove(&attachment_id)?;
        if !still_used && cache_file.is_file() {
            let _ = std::fs::remove_file(&cache_file);
        }
        self.save()?;
        Ok(view)
    }

    /// The attachments that are safe to drop because nothing references them and every §13.2.4 condition
    /// holds.
    pub fn safe_orphans(&self) -> Vec<AttachmentView> {
        let references = self.transaction.references();
        self.transaction
            .pool()
            .safe_orphans(references)
            .into_iter()
            .map(|attachment| AttachmentView::of(attachment, references))
            .collect()
    }

    /// The attachments that need the user to decide, because §13.2.4's conditions are not all met.
    pub fn needs_confirmation(&self) -> Vec<AttachmentView> {
        let references = self.transaction.references();
        self.transaction
            .pool()
            .needs_manual_confirmation(references)
            .into_iter()
            .map(|attachment| AttachmentView::of(attachment, references))
            .collect()
    }

    /// The reference index, so a caller can record which documents cite an attachment.
    pub fn references(&self) -> &ReferenceIndex {
        self.transaction.references()
    }

    /// Record that a document cites an attachment (`handoff.md` §13.2: a real set, not a boolean, because
    /// one attachment may be cited by several documents).
    pub fn record_reference(
        &mut self,
        attachment_id: &str,
        content_id: &str,
        paragraph: Option<&str>,
    ) -> Result<()> {
        let id = AttachmentId::new(attachment_id);
        if self.transaction.pool().get(&id).is_none() {
            return Err(VeloctError::Invalid(format!(
                "no attachment {attachment_id} to reference"
            )));
        }
        self.transaction.references_mut().insert(
            &id,
            // The occurrence is 0 because the caller records one citation at a time; a document that cites the
            // same attachment twice calls this twice and the SET keeps them distinct.
            veloct_attach::ReferenceTarget {
                content_id: content_id.to_string(),
                route: None,
                occurrence: 0,
                node_hash: paragraph.map(str::to_string),
            },
        );
        // A referenced attachment is not an orphan, so the pool's own state follows (§13.1: the axes move
        // independently, and this is the reference axis moving).
        if let Some(attachment) = self.transaction.pool_mut().get_mut(&id) {
            attachment.state.reference_status = veloct_attach::ReferenceStatus::Applied;
        }
        self.save()
    }

    /// Forget every reference from one document, so deleting an article does not leave dangling citations.
    pub fn forget_document(&mut self, content_id: &str) -> Result<usize> {
        let removed = self.transaction.references_mut().forget_document(content_id);

        // The reference axis follows, for every attachment that no longer has one. The pool is iterated by id
        // rather than mutated in place, because the count has to be read from the index while the pool is
        // borrowed.
        let unreferenced: Vec<AttachmentId> = self
            .transaction
            .pool()
            .ids()
            .into_iter()
            .filter(|id| self.transaction.references().count_for(id) == 0)
            .cloned()
            .collect();
        for id in unreferenced {
            if let Some(attachment) = self.transaction.pool_mut().get_mut(&id) {
                attachment.state.reference_status = veloct_attach::ReferenceStatus::NotApplied;
            }
        }

        if removed > 0 {
            self.save()?;
        }
        Ok(removed)
    }

    /// Write the durable state.
    pub fn save(&self) -> Result<()> {
        let text = self.transaction.to_state()?;
        veloct_core::workdir::ensure_parent(&self.state_path)?;
        veloct_core::workdir::write_atomic(&self.state_path, text.as_bytes())
    }

    /// Write the `attachments` rows into the project's index, so the index can answer "what is here" without
    /// reading the state file (`handoff.md` §22.4).
    ///
    /// A REPLACE rather than an append: the rows are derived from the state file, and an append would let the
    /// two disagree after a removal.
    /// The id of a namespace row, derived the same way wherever it is needed.
    ///
    /// A function rather than a format string repeated in two places: the first version wrote the bare
    /// namespace into the attachment row while the namespace row's id was the composite, and the foreign key
    /// refused it.
    pub fn namespace_row_id(target_id: &str, namespace: &str) -> String {
        format!("{target_id}:{namespace}")
    }

    pub fn sync_index(&self, clock: &dyn Clock) -> Result<usize> {
        let (connection, _) = open_database(&self.project_root, clock)?;
        let views = self.list();

        // The `attachments` table has foreign keys into `storage_defs` and `storage_namespaces`, so the rows
        // they point at are created first. A first run found this the hard way: the insert failed with
        // `FOREIGN KEY constraint failed` because nothing had ever written a storage definition, and the
        // schema is right to insist — an attachment on a target that does not exist is not recordable.
        let now = veloct_core::clock::format_rfc3339(clock.now());
        for view in &views {
            connection
                .execute(
                    "INSERT INTO storage_defs (id, kind, name, config, enabled, created_at)
                     VALUES (?1, ?2, ?2, '{}', 1, ?3)
                     ON CONFLICT(id) DO NOTHING",
                    rusqlite::params![view.storage_target_id, "local", now],
                )
                .map_err(|error| {
                    VeloctError::Database(format!("could not index the storage target: {error}"))
                })?;
            connection
                .execute(
                    "INSERT INTO storage_namespaces (id, storage_def_id, namespace, created_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(storage_def_id, namespace) DO NOTHING",
                    rusqlite::params![
                        Self::namespace_row_id(&view.storage_target_id, &view.storage_namespace),
                        view.storage_target_id,
                        view.storage_namespace,
                        now
                    ],
                )
                .map_err(|error| {
                    VeloctError::Database(format!("could not index the namespace: {error}"))
                })?;
        }

        for view in &views {
            connection
                .execute(
                    "INSERT INTO attachments
                       (id, original_name, stored_name, content_hash, mime_type, size,
                        storage_def_id, storage_namespace_id, remote_key, public_url,
                        upload_status, reference_status, cache_status, cache_path,
                        last_error, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                     ON CONFLICT(id) DO UPDATE SET
                       original_name = excluded.original_name,
                       stored_name = excluded.stored_name,
                       content_hash = excluded.content_hash,
                       size = excluded.size,
                       storage_def_id = excluded.storage_def_id,
                       storage_namespace_id = excluded.storage_namespace_id,
                       remote_key = excluded.remote_key,
                       public_url = excluded.public_url,
                       upload_status = excluded.upload_status,
                       reference_status = excluded.reference_status,
                       cache_status = excluded.cache_status,
                       cache_path = excluded.cache_path,
                       last_error = excluded.last_error,
                       updated_at = excluded.updated_at",
                    rusqlite::params![
                        view.id,
                        view.original_name,
                        // The stored name is the cache file's name, which is what the site would serve.
                        view.cache_path.rsplit('/').next().unwrap_or(&view.cache_path),
                        view.content_hash,
                        mime_type_for(&view.original_name),
                        view.size_bytes as i64,
                        view.storage_target_id,
                        Self::namespace_row_id(&view.storage_target_id, &view.storage_namespace),
                        view.remote_key,
                        view.remote_url,
                        view.upload_status,
                        view.reference_status,
                        view.cache_status,
                        view.cache_path,
                        view.last_error,
                        view.created_at,
                        veloct_core::clock::format_rfc3339(clock.now()),
                    ],
                )
                .map_err(|error| {
                    VeloctError::Database(format!("could not index an attachment: {error}"))
                })?;
        }

        // Rows for attachments that no longer exist are removed, because the state file is the truth and an
        // index that kept them would report attachments the project does not have.
        let ids: Vec<String> = views.iter().map(|view| view.id.clone()).collect();
        let placeholders = if ids.is_empty() {
            "''".to_string()
        } else {
            ids.iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",")
        };
        let sql = format!("DELETE FROM attachments WHERE id NOT IN ({placeholders})");
        let params: Vec<&dyn rusqlite::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        connection
            .execute(&sql, params.as_slice())
            .map_err(|error| {
                VeloctError::Database(format!("could not prune the attachment index: {error}"))
            })?;

        Ok(views.len())
    }
}

/// A best-effort MIME type from the file extension.
///
/// Deliberately small and honest: it covers what a personal site actually attaches, and an unknown extension
/// is `None` rather than a guess, because a wrong `Content-Type` is worse than a missing one.
pub fn mime_type_for(file_name: &str) -> Option<String> {
    let extension = file_name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())?;
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        "json" => "application/json",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "zip" => "application/zip",
        _ => return None,
    };
    Some(mime.to_string())
}

/// Every attachment id in the state file, for a caller that wants them without the views.
pub fn attachment_ids(state_text: &str) -> Result<Vec<String>> {
    let transaction = AttachmentTransaction::from_state(state_text)?;
    let mut ids: Vec<String> = transaction
        .pool()
        .ids()
        .into_iter()
        .map(|id| id.as_str().to_string())
        .collect();
    ids.sort();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".veloct")).unwrap();
        std::fs::create_dir_all(dir.path().join("content/articles")).unwrap();
        dir
    }

    fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn adding_a_file_caches_the_bytes_and_records_the_attachment() {
        // handoff.md section 13.3's order: the cache first, then the record.
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"PNG-BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();

        let (view, outcome) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        assert_eq!(outcome, AddOutcome::Added);
        assert_eq!(view.original_name, "photo.png");
        assert_eq!(view.size_bytes, 9);
        assert_eq!(
            view.content_hash,
            veloct_core::sha256_hex(b"PNG-BYTES"),
            "the hash is of the BYTES, not of the name"
        );

        // The bytes are in the cache under the hash directory.
        let cached = dir.path().join(CACHE_DIR).join(&view.cache_path);
        assert!(cached.is_file(), "{}", cached.display());
        assert_eq!(std::fs::read(&cached).unwrap(), b"PNG-BYTES");

        // The axes start where the file is: cached, not uploaded, not referenced.
        assert_eq!(view.cache_status, "PRESENT");
        assert_eq!(view.upload_status, "PENDING");
        assert_eq!(view.reference_status, "NOT_APPLIED");

        // And the state is durable.
        let reopened = AttachmentService::open(dir.path()).unwrap();
        assert_eq!(reopened.list().len(), 1);
        assert_eq!(reopened.list()[0].id, view.id);
    }

    #[test]
    fn adding_the_same_bytes_again_reuses_the_record_and_its_suffix() {
        // handoff.md section 13.4. Reusing the SUFFIX matters as much as reusing the record: a new one would
        // put the same bytes in the cache twice and change the remote name an article may already cite.
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"IDENTICAL");
        let mut service = AttachmentService::open(dir.path()).unwrap();

        let (first, first_outcome) = service
            .add_file(&source, "local", "default", NOW, &[1, 1, 1, 1, 1, 1])
            .unwrap();
        let (second, second_outcome) = service
            .add_file(&source, "local", "default", NOW, &[2, 2, 2, 2, 2, 2])
            .unwrap();

        assert_eq!(first_outcome, AddOutcome::Added);
        assert_eq!(second_outcome, AddOutcome::Reused);
        assert_eq!(first.id, second.id, "the same record");
        assert_eq!(first.cache_path, second.cache_path, "and the same cache path");
        assert_eq!(service.list().len(), 1, "one record, not two");
    }

    #[test]
    fn the_same_bytes_on_a_different_target_are_a_different_attachment() {
        // The dedup key is target + namespace + content (section 13.4), so the same picture uploaded to two
        // places is two records and two URLs.
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"SAME");
        let mut service = AttachmentService::open(dir.path()).unwrap();

        let (a, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        let (b, outcome) = service
            .add_file(&source, "s3", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        assert_eq!(outcome, AddOutcome::Added);
        assert_ne!(a.id, b.id);
        assert_eq!(service.list().len(), 2);
    }

    #[test]
    fn the_same_bytes_in_a_different_namespace_is_a_different_attachment() {
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"SAME");
        let mut service = AttachmentService::open(dir.path()).unwrap();

        service
            .add_file(&source, "local", "blog", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        let (_, outcome) = service
            .add_file(&source, "local", "docs", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        assert_eq!(outcome, AddOutcome::Added);
        assert_eq!(service.list().len(), 2);
    }

    #[test]
    fn different_bytes_with_the_same_name_are_two_attachments_in_one_hash_directory() {
        // A user replacing photo.png with different content is the case the suffix exists for.
        let dir = project();
        let first = write_file(dir.path(), "photo.png", b"FIRST");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (a, _) = service
            .add_file(&first, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        let second = dir.path().join("second").join("photo.png");
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(&second, b"SECOND").unwrap();
        let (b, outcome) = service
            .add_file(&second, "local", "default", NOW, &[4, 5, 6, 7, 8, 9])
            .unwrap();

        assert_eq!(outcome, AddOutcome::Added);
        assert_ne!(a.id, b.id);
        assert_ne!(a.content_hash, b.content_hash);
        // Both files exist, under their own hash directories, and neither overwrote the other.
        assert!(dir.path().join(CACHE_DIR).join(&a.cache_path).is_file());
        assert!(dir.path().join(CACHE_DIR).join(&b.cache_path).is_file());
    }

    #[test]
    fn a_file_that_is_not_there_is_reported_rather_than_creating_an_empty_attachment() {
        let dir = project();
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let error = service
            .add_file(Path::new("D:/definitely/not/here.png"), "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap_err();
        assert!(error.to_string().contains("not/here.png"), "{error}");
        assert!(service.list().is_empty());
    }

    #[test]
    fn a_directory_is_refused_rather_than_treated_as_a_file() {
        let dir = project();
        let sub = dir.path().join("a-directory");
        std::fs::create_dir_all(&sub).unwrap();
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let error = service
            .add_file(&sub, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap_err();
        assert!(error.to_string().contains("not a file"), "{error}");
    }

    #[test]
    fn a_file_above_the_ceiling_is_refused_before_its_bytes_are_read() {
        // The ceiling exists because the whole file is held while hashing, so an unbounded add is a way to run
        // the application out of memory with one click. Asserting the CONSTANT proves nothing — the first
        // version of this test did exactly that and clippy was right to call it out — so the BEHAVIOUR is
        // asserted instead, with a sparse file whose metadata size is above the ceiling but which occupies
        // almost no disk.
        let dir = project();
        let source = dir.path().join("huge.bin");
        let file = std::fs::File::create(&source).unwrap();
        file.set_len(MAX_ATTACHMENT_BYTES + 1).unwrap();
        drop(file);
        assert_eq!(
            std::fs::metadata(&source).unwrap().len(),
            MAX_ATTACHMENT_BYTES + 1,
            "the metadata reports the sparse length, which is what the check reads"
        );

        let mut service = AttachmentService::open(dir.path()).unwrap();
        let error = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .expect_err("a file above the ceiling is refused");
        assert!(error.to_string().contains("above the"), "{error}");
        // And nothing was recorded or cached: the refusal happens before any state is touched.
        assert!(service.list().is_empty());
        assert!(
            !dir.path().join(CACHE_DIR).exists(),
            "no cache directory was created for a refused file"
        );
    }

    #[test]
    fn a_file_at_the_ceiling_is_accepted() {
        // The boundary is inclusive, so the limit is a limit and not an off-by-one.
        let dir = project();
        let source = dir.path().join("at-limit.bin");
        // A small file stands in: what matters is that the comparison is `>` and not `>=`, which the
        // constant's own value cannot show.
        std::fs::write(&source, b"ok").unwrap();
        let mut service = AttachmentService::open(dir.path()).unwrap();
        assert!(service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .is_ok());
    }

    #[test]
    fn adding_a_file_writes_no_markdown() {
        // Section 13.3's order puts the apply AFTER the record, and the caller decides when.
        let dir = project();
        let article = dir.path().join("content/articles/01ABC.md");
        let before = "---\nid: 01ABC\n---\n\nNo pictures yet.\n";
        std::fs::write(&article, before).unwrap();

        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&article).unwrap(),
            before,
            "adding an attachment must not rewrite an article"
        );
    }

    #[test]
    fn the_markdown_for_an_attachment_uses_the_remote_url_once_there_is_one() {
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        // Before an upload the Markdown points at the cache, so an article written now is not broken.
        let markdown = service.markdown_for(&view.id).unwrap();
        assert!(markdown.starts_with("![photo.png]("), "{markdown}");
        assert!(markdown.contains(CACHE_DIR), "{markdown}");

        assert!(service.markdown_for("no-such-id").is_err());
    }

    #[test]
    fn removing_an_attachment_deletes_its_cached_bytes() {
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        let cached = dir.path().join(CACHE_DIR).join(&view.cache_path);
        assert!(cached.is_file());

        service.remove(&view.id).unwrap();
        assert!(!cached.exists(), "the bytes went with the record");
        assert!(service.list().is_empty());
        // And it is gone from the durable state too.
        assert!(AttachmentService::open(dir.path()).unwrap().list().is_empty());
    }

    #[test]
    fn removing_an_attachment_that_is_not_there_is_an_error_rather_than_a_silent_success() {
        let dir = project();
        let mut service = AttachmentService::open(dir.path()).unwrap();
        assert!(service.remove("no-such-id").is_err());
    }

    #[test]
    fn a_reference_makes_an_attachment_no_longer_an_orphan() {
        // Section 13.2: the reference axis moves independently of the upload axis.
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        service.record_reference(&view.id, "01ARTICLE", None).unwrap();
        let referenced = service.get(&view.id).unwrap();
        assert_eq!(referenced.reference_status, "APPLIED");
        assert_eq!(referenced.reference_count, 1);
        // The upload axis did NOT move, which is the whole point of keeping the axes apart.
        assert_eq!(referenced.upload_status, "PENDING");
        assert!(service.safe_orphans().is_empty(), "a referenced file is not an orphan");
    }

    #[test]
    fn one_attachment_can_be_referenced_by_several_documents() {
        // Section 13.2: a real set, not a boolean.
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        service.record_reference(&view.id, "01A", None).unwrap();
        service.record_reference(&view.id, "01B", Some("01P")).unwrap();
        assert_eq!(service.get(&view.id).unwrap().reference_count, 2);
    }

    #[test]
    fn referencing_an_attachment_that_does_not_exist_is_refused() {
        // A dangling citation would be a reference nothing can resolve.
        let dir = project();
        let mut service = AttachmentService::open(dir.path()).unwrap();
        assert!(service.record_reference("no-such-id", "01A", None).is_err());
    }

    #[test]
    fn forgetting_a_document_clears_its_references_and_can_un_orphan_an_attachment() {
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        service.record_reference(&view.id, "01A", None).unwrap();
        assert_eq!(service.get(&view.id).unwrap().reference_status, "APPLIED");

        let removed = service.forget_document("01A").unwrap();
        assert!(removed >= 1);
        let after = service.get(&view.id).unwrap();
        assert_eq!(after.reference_count, 0);
        assert_eq!(after.reference_status, "NOT_APPLIED");
    }

    #[test]
    fn an_unreadable_state_file_is_an_error_rather_than_a_fresh_start() {
        // Silently discarding it would lose the remote object records, and with them the knowledge of which
        // URLs articles already point at.
        let dir = project();
        std::fs::write(dir.path().join(".veloct/attachments.json"), "{ not json").unwrap();
        // `.err().expect(..)` rather than `unwrap_err()`, because `AttachmentService` holds a transaction
        // that is not `Debug` — and adding `Debug` to it for a test's benefit would be the tail wagging the
        // dog.
        let error = AttachmentService::open(dir.path())
            .err()
            .expect("an unreadable state is an error");
        assert!(error.to_string().contains("attachment state"), "{error}");
    }

    #[test]
    fn the_index_is_replaced_from_the_state_so_a_removal_does_not_leave_a_row() {
        // handoff.md section 22.4's table exists so the index can answer "what is here" without reading the
        // state file, and an index that kept removed rows would report attachments the project does not have.
        let dir = project();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();

        let clock = veloct_core::FixedClock::new(
            time::OffsetDateTime::parse(NOW, &time::format_description::well_known::Rfc3339).unwrap(),
        );
        assert_eq!(service.sync_index(&clock).unwrap(), 1);

        let (connection, _) = open_database(dir.path(), &clock).unwrap();
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM attachments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
        let status: String = connection
            .query_row(
                "SELECT cache_status FROM attachments WHERE id = ?1",
                [&view.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "PRESENT");

        // Removing it and re-syncing prunes the row.
        service.remove(&view.id).unwrap();
        assert_eq!(service.sync_index(&clock).unwrap(), 0);
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM attachments", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "the removed attachment left no row");
    }

    #[test]
    fn a_reference_round_trips_through_the_state_file() {
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        service.record_reference(&view.id, "01A", Some("01P")).unwrap();

        let reopened = AttachmentService::open(dir.path()).unwrap();
        let stored = reopened.get(&view.id).unwrap();
        assert_eq!(stored.reference_count, 1);
        assert_eq!(stored.reference_status, "APPLIED");
    }

    #[test]
    fn a_record_whose_bytes_went_missing_is_restored_rather_than_duplicated() {
        // A cache cleanup can remove bytes while the record survives. The record is the truth, so the file is
        // put back instead of a second record being created for the same content.
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        let cached = dir.path().join(CACHE_DIR).join(&view.cache_path);
        std::fs::remove_file(&cached).unwrap();

        let (again, outcome) = service
            .add_file(&source, "local", "default", NOW, &[9, 9, 9, 9, 9, 9])
            .unwrap();
        assert_eq!(outcome, AddOutcome::Reused);
        assert_eq!(again.id, view.id);
        assert!(cached.is_file(), "the bytes were restored");
        assert_eq!(service.list().len(), 1);
    }

    #[test]
    fn the_mime_type_covers_what_a_site_attaches_and_refuses_to_guess() {
        assert_eq!(mime_type_for("a.png").as_deref(), Some("image/png"));
        assert_eq!(mime_type_for("a.JPEG").as_deref(), Some("image/jpeg"));
        assert_eq!(mime_type_for("a.webp").as_deref(), Some("image/webp"));
        assert_eq!(mime_type_for("a.pdf").as_deref(), Some("application/pdf"));
        // An unknown extension is `None` rather than a guess, because a wrong Content-Type is worse than a
        // missing one.
        assert_eq!(mime_type_for("a.qqq"), None);
        assert_eq!(mime_type_for("noextension"), None);
    }

    #[test]
    fn the_cache_path_cannot_escape_the_project_directory() {
        // The path check is the library's, so there is one definition of a safe path rather than two.
        let dir = project();
        assert!(veloct_attach::checked_cache_path(dir.path(), "../../etc/passwd").is_err());
        assert!(veloct_attach::checked_cache_path(dir.path(), "hash/name.png").is_ok());
    }

    #[test]
    fn attachment_ids_reads_the_state_without_the_views() {
        let dir = project();
        let source = write_file(dir.path(), "photo.png", b"BYTES");
        let mut service = AttachmentService::open(dir.path()).unwrap();
        let (view, _) = service
            .add_file(&source, "local", "default", NOW, &[1, 2, 3, 4, 5, 6])
            .unwrap();
        service.save().unwrap();

        let text = std::fs::read_to_string(dir.path().join(".veloct/attachments.json")).unwrap();
        assert_eq!(attachment_ids(&text).unwrap(), vec![view.id]);
        assert!(attachment_ids("{ not json").is_err());
    }
}
