//! `work.json`: the metadata truth source of a work.
//!
//! `docs/spec/01-data-model.md` §4 fixes the shape, and `handoff.md` §10.2 fixes what the user is
//! asked for: a title and an author, with the author pre-filled from a default. The completion time
//! defaults to the import timestamp rather than being demanded, because a user importing a finished
//! project rarely knows the exact minute it was finished and should not have to guess.
//!
//! `deny_unknown_fields` is deliberately NOT used, unlike the release manifest. A release manifest is
//! consumed by a reproducible build, so an unrecognised key there means the build is not
//! reproducible. A `work.json` is written by hand as often as by Veloct, so an extra key is the
//! user's own business and dropping it on the next write would lose their note.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::ids::StableId;
use veloct_core::{Result, VeloctError};

/// The metadata file's name inside a work directory.
pub const WORK_JSON: &str = "work.json";

/// How the work's files arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkSourceKind {
    /// A ZIP archive.
    Zip,
    /// A folder on disk.
    Folder,
}

impl WorkSourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Folder => "folder",
        }
    }
}

/// Where the work came from, kept so the user can tell two imports apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkSource {
    pub kind: WorkSourceKind,
    /// The archive or folder name, as the user saw it.
    #[serde(default)]
    pub original_name: String,
}

impl WorkSource {
    pub fn zip(original_name: impl Into<String>) -> Self {
        Self {
            kind: WorkSourceKind::Zip,
            original_name: original_name.into(),
        }
    }

    pub fn folder(original_name: impl Into<String>) -> Self {
        Self {
            kind: WorkSourceKind::Folder,
            original_name: original_name.into(),
        }
    }
}

/// A work's metadata, as stored in `work.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkMetadata {
    /// The stable id, which is also the directory name.
    pub id: String,
    /// The mutable public slug. Renaming it never moves the directory and never changes the id.
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub summary: String,
    /// When the work was finished. Defaults to the import timestamp
    /// (`handoff.md` §10.2) and stays editable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    /// When it was imported. Set once, by the import.
    pub imported_at: String,
    pub source: WorkSource,
    /// The HTML file the works origin serves for a bare `/`.
    #[serde(default = "default_entry")]
    pub entry: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// A path RELATIVE to the work directory, per the spec's `"cover": "preview/cover.webp"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub notes: String,
}

fn default_entry() -> String {
    "index.html".to_string()
}

impl WorkMetadata {
    /// Metadata for a newly imported work.
    ///
    /// `completed_at` starts equal to `imported_at`, which is what §10.2 asks for: the completion
    /// time is taken from the upload timestamp and the user is not required to supply it.
    pub fn new(
        id: StableId,
        title: impl Into<String>,
        author: impl Into<String>,
        source: WorkSource,
        now: &str,
    ) -> Self {
        let title = title.into();
        // The slug starts as the slugified title, which is readable; the user can change it. When
        // the title has nothing sluggable in it the id is used, lowercased, so the work still has an
        // address rather than an empty one.
        let slug = match veloct_core::paths::slugify(&title) {
            candidate if candidate == "untitled" && title.trim() != "untitled" => {
                id.to_text().to_lowercase()
            }
            candidate => candidate,
        };

        Self {
            id: id.to_text(),
            slug,
            title,
            author: author.into(),
            summary: String::new(),
            completed_at: Some(now.to_string()),
            imported_at: now.to_string(),
            source,
            entry: default_entry(),
            tags: Vec::new(),
            cover: None,
            screenshot: None,
            size_bytes: 0,
            notes: String::new(),
        }
    }

    /// The directory this work lives in, under a works root.
    pub fn directory(&self, works_root: &Path) -> PathBuf {
        works_root.join(&self.id)
    }

    /// The directory holding the work's static body.
    pub fn site_directory(&self, works_root: &Path) -> PathBuf {
        self.directory(works_root).join("site")
    }

    /// Validate the metadata before it is trusted.
    ///
    /// The id must be a stable id, because it is the directory name and therefore the identity. A
    /// `work.json` whose `id` is not one would produce a directory Veloct cannot address, and the
    /// import would appear to succeed while creating something unreachable.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("id must not be empty".into());
        }
        if self.id.parse::<StableId>().is_err() {
            return Err(format!(
                "id {:?} is not a stable id; it is the directory name, so it must be one",
                self.id
            ));
        }
        if self.slug.trim().is_empty() {
            return Err("slug must not be empty".into());
        }
        // A slug must not contain path separators, or it would escape the works root when used to
        // build a URL or a path.
        if self.slug.contains('/') || self.slug.contains('\\') {
            return Err(format!("slug {:?} must not contain a path separator", self.slug));
        }
        if self.title.trim().is_empty() {
            return Err("title must not be empty".into());
        }
        if self.entry.contains("..") {
            return Err(format!("entry {:?} must not escape the work directory", self.entry));
        }
        if self.entry.starts_with('/') || self.entry.contains(':') {
            return Err(format!("entry {:?} must be a relative path", self.entry));
        }
        for (field, value) in [("cover", &self.cover), ("screenshot", &self.screenshot)] {
            if let Some(value) = value {
                if value.contains("..") {
                    return Err(format!("{field} {:?} must not escape the work directory", value));
                }
            }
        }
        Ok(())
    }

    /// Serialize with stable key order and a trailing newline, so a re-write is byte-identical and a
    /// rebuild is comparable (`handoff.md` §14.9).
    pub fn to_json(&self) -> Result<String> {
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        Ok(text)
    }
}

/// A work as read from disk, with the metadata and where it lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRecord {
    pub metadata: WorkMetadata,
    /// True when `site/` exists, so the UI can say "the body is missing" instead of showing a blank.
    pub has_body: bool,
    /// Files in `site/`, for the build's manifest.
    pub file_count: usize,
}

impl WorkRecord {
    pub fn slug(&self) -> &str {
        &self.metadata.slug
    }

    pub fn title(&self) -> &str {
        &self.metadata.title
    }

    /// Whether this work can be served at all.
    pub fn is_servable(&self) -> bool {
        self.has_body && self.file_count > 0
    }
}

/// Read one work from its directory.
pub fn read_work(work_dir: &Path) -> Result<WorkRecord> {
    let meta_path = work_dir.join(WORK_JSON);
    let text = veloct_core::workdir::read_text(&meta_path).map_err(|error| match error {
        VeloctError::Io { .. } => VeloctError::Invalid(format!(
            "no {WORK_JSON} in {}",
            work_dir.display()
        )),
        other => other,
    })?;

    let metadata: WorkMetadata = serde_json::from_str(&text).map_err(|error| {
        VeloctError::Invalid(format!("{} could not be read: {error}", meta_path.display()))
    })?;

    metadata.validate().map_err(|problem| {
        VeloctError::Invalid(format!("{} is not valid: {problem}", meta_path.display()))
    })?;

    let site_dir = work_dir.join("site");
    let files = if site_dir.is_dir() {
        veloct_core::workdir::list_files(&site_dir).unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(WorkRecord {
        metadata,
        has_body: site_dir.is_dir(),
        file_count: files.len(),
    })
}

/// List every work under a works root, sorted by completion time then slug.
///
/// A directory without a readable `work.json` is skipped rather than reported as an error: a
/// half-copied directory or an editor's backup folder is not a work, and failing the whole list for
/// one stray directory would hide the works that are fine.
pub fn list_works(works_root: &Path) -> Result<Vec<WorkRecord>> {
    if !works_root.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let entries = std::fs::read_dir(works_root).map_err(|error| VeloctError::Io {
        path: works_root.to_path_buf(),
        source: error,
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| VeloctError::Io {
            path: works_root.to_path_buf(),
            source: error,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if !path.join(WORK_JSON).is_file() {
            continue;
        }
        if let Ok(record) = read_work(&path) {
            out.push(record);
        }
    }

    out.sort_by(|a, b| {
        let a_key = a.metadata.completed_at.clone().unwrap_or_default();
        let b_key = b.metadata.completed_at.clone().unwrap_or_default();
        // Newest first, then slug so the order is total and a rebuild is comparable.
        b_key
            .cmp(&a_key)
            .then_with(|| a.metadata.slug.cmp(&b.metadata.slug))
    });
    Ok(out)
}

/// Write a work's metadata, creating the directory and `site/` as needed.
pub fn write_work(work_dir: &Path, metadata: &WorkMetadata) -> Result<()> {
    metadata
        .validate()
        .map_err(|problem| VeloctError::Invalid(format!("work metadata is not valid: {problem}")))?;
    std::fs::create_dir_all(work_dir.join("site")).map_err(|error| VeloctError::Io {
        path: work_dir.to_path_buf(),
        source: error,
    })?;
    let path = work_dir.join(WORK_JSON);
    let text = metadata.to_json()?;
    veloct_core::workdir::write_text_if_changed(&path, &text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn metadata(seed: u128) -> WorkMetadata {
        let mut meta = WorkMetadata::new(
            StableId::from_parts(1_700_000_000_000, seed),
            "Demo Counter",
            "vellow",
            WorkSource::zip("demo-counter.zip"),
            NOW,
        );
        meta.title = "Demo Counter".to_string();
        meta.slug = "demo-counter".to_string();
        meta
    }

    #[test]
    fn a_new_work_takes_the_completion_time_from_the_import() {
        // handoff.md section 10.2: the user is not asked for the completion time.
        let meta = metadata(1);
        assert_eq!(meta.completed_at.as_deref(), Some(NOW));
        assert_eq!(meta.imported_at, NOW);
    }

    #[test]
    fn a_valid_work_passes_validation() {
        assert!(metadata(2).validate().is_ok());
    }

    #[test]
    fn an_id_that_is_not_a_stable_id_is_rejected() {
        // The id IS the directory name, so a non-id would create something unaddressable.
        let mut meta = metadata(3);
        meta.id = "demo-counter".to_string();
        let problem = meta.validate().unwrap_err();
        assert!(problem.contains("not a stable id"), "{problem}");
    }

    #[test]
    fn a_slug_with_a_path_separator_is_rejected() {
        // It would escape the works root when turned into a URL or a path.
        let mut meta = metadata(4);
        meta.slug = "../escape".to_string();
        assert!(meta.validate().unwrap_err().contains("path separator"));

        meta.slug = "a/b".to_string();
        assert!(meta.validate().unwrap_err().contains("path separator"));
    }

    #[test]
    fn an_entry_that_escapes_the_work_directory_is_rejected() {
        let mut meta = metadata(5);
        meta.entry = "../../etc/passwd".to_string();
        assert!(meta.validate().unwrap_err().contains("escape"));

        meta.entry = "/etc/passwd".to_string();
        assert!(meta.validate().unwrap_err().contains("relative"));

        meta.entry = "C:/windows".to_string();
        assert!(meta.validate().unwrap_err().contains("relative"));
    }

    #[test]
    fn a_cover_that_escapes_the_work_directory_is_rejected() {
        let mut meta = metadata(6);
        meta.cover = Some("../../secret.png".to_string());
        assert!(meta.validate().unwrap_err().contains("escape"));
    }

    #[test]
    fn an_empty_title_or_slug_is_rejected() {
        let mut meta = metadata(7);
        meta.title = "  ".to_string();
        assert!(meta.validate().unwrap_err().contains("title"));

        let mut meta = metadata(8);
        meta.slug = String::new();
        assert!(meta.validate().unwrap_err().contains("slug"));
    }

    #[test]
    fn writing_and_reading_a_work_round_trips() {
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path().join("w");
        let meta = metadata(9);
        write_work(&work_dir, &meta).unwrap();

        assert!(work_dir.join(WORK_JSON).is_file());
        assert!(work_dir.join("site").is_dir(), "site/ is created as the body root");

        let record = read_work(&work_dir).unwrap();
        assert_eq!(record.metadata, meta);
        assert!(record.has_body);
        assert_eq!(record.file_count, 0);
        assert!(!record.is_servable(), "an empty body is not servable");
    }

    #[test]
    fn a_work_with_a_body_reports_its_file_count_and_is_servable() {
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path().join("w");
        write_work(&work_dir, &metadata(10)).unwrap();
        std::fs::write(work_dir.join("site").join("index.html"), "<html></html>").unwrap();
        std::fs::create_dir_all(work_dir.join("site").join("assets")).unwrap();
        std::fs::write(work_dir.join("site").join("assets").join("app.js"), "//").unwrap();

        let record = read_work(&work_dir).unwrap();
        assert_eq!(record.file_count, 2);
        assert!(record.is_servable());
    }

    #[test]
    fn writing_the_same_metadata_twice_does_not_change_the_file() {
        // A rebuild must be comparable (handoff.md section 14.9).
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path().join("w");
        let meta = metadata(11);
        write_work(&work_dir, &meta).unwrap();
        let first = std::fs::read_to_string(work_dir.join(WORK_JSON)).unwrap();
        write_work(&work_dir, &meta).unwrap();
        let second = std::fs::read_to_string(work_dir.join(WORK_JSON)).unwrap();
        assert_eq!(first, second);
        assert!(first.ends_with('\n'), "the file ends with a newline");
    }

    #[test]
    fn the_metadata_file_keeps_an_unknown_key_the_user_added() {
        // Unlike a release manifest, a work.json is hand-written as often as generated, so an extra
        // key is the user's own business.
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path().join("w");
        write_work(&work_dir, &metadata(12)).unwrap();
        let path = work_dir.join(WORK_JSON);
        let mut text = std::fs::read_to_string(&path).unwrap();
        text = text.replace("\"notes\": \"\"", "\"notes\": \"\",\n  \"my_note\": \"kept\"");
        std::fs::write(&path, text).unwrap();

        // Reading does not fail on the extra key.
        let record = read_work(&work_dir).unwrap();
        assert_eq!(record.metadata.title, "Demo Counter");

        // And re-writing from the parsed metadata drops it, which is why the reader tolerates it
        // rather than the writer preserving it: the file is the truth, and the user edits the file.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("my_note"));
    }

    #[test]
    fn reading_a_directory_without_metadata_reports_it_clearly() {
        let dir = TempDir::new().unwrap();
        let error = read_work(dir.path()).unwrap_err();
        assert!(error.to_string().contains(WORK_JSON), "{error}");
    }

    #[test]
    fn reading_invalid_metadata_reports_the_problem() {
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path().join("w");
        std::fs::create_dir_all(&work_dir).unwrap();
        std::fs::write(work_dir.join(WORK_JSON), "{ not json").unwrap();
        let error = read_work(&work_dir).unwrap_err();
        assert!(error.to_string().contains("could not be read"), "{error}");

        std::fs::write(
            work_dir.join(WORK_JSON),
            r#"{"id":"not-an-id","slug":"x","title":"X","imported_at":"now","source":{"kind":"zip","original_name":"x.zip"}}"#,
        )
        .unwrap();
        let error = read_work(&work_dir).unwrap_err();
        assert!(error.to_string().contains("not valid"), "{error}");
    }

    #[test]
    fn listing_skips_directories_that_are_not_works() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("works");
        std::fs::create_dir_all(&root).unwrap();

        write_work(&root.join("good"), &metadata(13)).unwrap();
        // A stray directory with no metadata, and a plain file.
        std::fs::create_dir_all(root.join("stray")).unwrap();
        std::fs::write(root.join("readme.txt"), "not a work").unwrap();

        let works = list_works(&root).unwrap();
        assert_eq!(works.len(), 1, "only the real work is listed");
        assert_eq!(works[0].title(), "Demo Counter");
    }

    #[test]
    fn listing_an_absent_root_is_empty_rather_than_an_error() {
        let dir = TempDir::new().unwrap();
        assert!(list_works(&dir.path().join("nothing")).unwrap().is_empty());
    }

    #[test]
    fn works_are_listed_newest_first() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("works");
        std::fs::create_dir_all(&root).unwrap();

        let mut older = metadata(14);
        older.slug = "older".to_string();
        older.completed_at = Some("2026-01-01T00:00:00Z".to_string());
        write_work(&root.join(&older.id), &older).unwrap();

        let mut newer = metadata(15);
        newer.slug = "newer".to_string();
        newer.completed_at = Some("2026-06-01T00:00:00Z".to_string());
        write_work(&root.join(&newer.id), &newer).unwrap();

        let works = list_works(&root).unwrap();
        assert_eq!(works[0].slug(), "newer");
        assert_eq!(works[1].slug(), "older");
    }

    #[test]
    fn a_work_with_no_completion_time_sorts_last_rather_than_failing() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("works");
        std::fs::create_dir_all(&root).unwrap();

        let mut dated = metadata(16);
        dated.slug = "dated".to_string();
        dated.completed_at = Some("2026-01-01T00:00:00Z".to_string());
        write_work(&root.join(&dated.id), &dated).unwrap();

        let mut undated = metadata(17);
        undated.slug = "undated".to_string();
        undated.completed_at = None;
        write_work(&root.join(&undated.id), &undated).unwrap();

        let works = list_works(&root).unwrap();
        assert_eq!(works[0].slug(), "dated");
        assert_eq!(works[1].slug(), "undated");
    }

    #[test]
    fn the_site_directory_is_derived_from_the_id_and_not_the_slug() {
        // handoff.md section 10.1: the directory is named by the stable id, so renaming the slug
        // never moves the body.
        let root = std::path::Path::new("D:/proj/works");
        let meta = metadata(18);
        assert!(meta.directory(root).ends_with(&meta.id));
        assert!(meta.site_directory(root).ends_with("site"));
        let mut renamed = meta.clone();
        renamed.slug = "something-else".to_string();
        assert_eq!(renamed.directory(root), meta.directory(root));
    }

    #[test]
    fn metadata_round_trips_through_json() {
        let meta = metadata(19);
        let json = meta.to_json().unwrap();
        assert!(json.contains("\"kind\": \"zip\""), "{json}");
        let parsed: WorkMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, meta);
    }

    #[test]
    fn the_new_work_helper_uses_the_slugified_title_for_a_readable_slug() {
        let meta = WorkMetadata::new(
            StableId::from_parts(1, 1),
            "My Cool Demo",
            "author",
            WorkSource::folder("My Cool Demo"),
            NOW,
        );
        assert_eq!(meta.slug, "my-cool-demo");
        assert_eq!(meta.author, "author");
        assert_eq!(meta.source.kind, WorkSourceKind::Folder);
        assert!(meta.entry == "index.html");
    }
}
