//! Reading and listing the Markdown truth source.
//!
//! Every read here goes to the file. Nothing is served from SQLite, which is what keeps the index
//! genuinely derived and rebuildable (`handoff.md` §3.4): if the database were ever consulted for a
//! body, deleting it would lose content and the rebuild guarantee would be a claim rather than a
//! property.

use std::path::PathBuf;

use veloct_core::ids::StableId;
use veloct_core::{Result, VeloctError};
use veloct_markdown::frontmatter::ContentType;

use crate::model::{ContentKind, ContentPaths, DocumentRef, LoadedDocument};

/// A document path discovered on disk, with the id parsed from its file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentPath {
    pub reference: DocumentRef,
    pub path: PathBuf,
}

/// Read and parse one document.
pub fn read_document(paths: &ContentPaths, reference: DocumentRef) -> Result<LoadedDocument> {
    let path = paths.document_path(reference.kind, reference.id);
    let source = veloct_core::workdir::read_text(&path).map_err(|error| match error {
        VeloctError::Io { .. } => VeloctError::Invalid(format!(
            "{} document {} does not exist at {}",
            reference.kind.as_str(),
            reference.id.to_text(),
            path.display()
        )),
        other => other,
    })?;

    let document = veloct_markdown::parse::parse(&source)?;
    let revision = veloct_core::workdir::FileRevision::of_text(&source);

    Ok(LoadedDocument {
        reference,
        path,
        source,
        document,
        revision,
    })
}

/// List the documents of one kind that exist on disk.
///
/// The id comes from the FILE NAME and is then cross-checked against the frontmatter id. A mismatch
/// means the file was renamed or copied, and silently trusting either side would make references
/// resolve to the wrong document, so it is reported.
pub fn list_documents(paths: &ContentPaths, kind: ContentKind) -> Result<Vec<DocumentRef>> {
    let directory = paths.kind_dir(kind);
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let entries = std::fs::read_dir(&directory).map_err(|error| {
        VeloctError::Io {
            path: directory.clone(),
            source: error,
        }
    })?;

    for entry in entries {
        let entry = entry.map_err(|error| VeloctError::Io {
            path: directory.clone(),
            source: error,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if let Ok(id) = stem.parse::<StableId>() {
            out.push(DocumentRef::new(kind, id));
        }
    }

    out.sort();
    Ok(out)
}

/// List every document path, including ones whose file name is not a stable id.
///
/// Used by the index so a mis-named file is REPORTED rather than invisible: a file the user created
/// by hand would otherwise simply not appear anywhere.
pub fn scan_document_paths(paths: &ContentPaths, kind: ContentKind) -> Result<Vec<(PathBuf, Option<DocumentRef>)>> {
    let directory = paths.kind_dir(kind);
    if !directory.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let entries = std::fs::read_dir(&directory).map_err(|error| VeloctError::Io {
        path: directory.clone(),
        source: error,
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| VeloctError::Io {
            path: directory.clone(),
            source: error,
        })?;
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let reference = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<StableId>().ok())
            .map(|id| DocumentRef::new(kind, id));
        out.push((path, reference));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Write a document's source to disk.
pub fn write_document(path: &std::path::Path, source: &str) -> Result<bool> {
    veloct_core::workdir::ensure_parent(path)?;
    let outcome = veloct_core::workdir::write_text_if_changed(path, source)?;
    Ok(outcome.is_change())
}

/// Check that a file name and its frontmatter agree about the id.
///
/// Returns a message describing the mismatch, or `None` when they agree.
pub fn identity_mismatch(path: &std::path::Path, document: &veloct_markdown::parse::Document) -> Option<String> {
    let stem = path.file_stem().and_then(|stem| stem.to_str())?;
    match stem.parse::<StableId>() {
        Ok(file_id) if file_id == document.frontmatter.id => None,
        Ok(file_id) => Some(format!(
            "the file name says {} but the frontmatter says {}",
            file_id.to_text(),
            document.frontmatter.id.to_text()
        )),
        Err(_) => Some(format!(
            "the file name {stem:?} is not a stable id; Veloct addresses documents by id"
        )),
    }
}

/// The frontmatter type a kind must carry, for a caller validating a hand-written file.
pub fn expected_type(kind: ContentKind) -> ContentType {
    kind.content_type()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContentPaths;
    use std::fs;
    use tempfile::TempDir;

    fn paths() -> (TempDir, ContentPaths) {
        let dir = TempDir::new().unwrap();
        let paths = ContentPaths::new(dir.path());
        (dir, paths)
    }

    fn sample_source(kind: ContentKind, id: StableId, slug: &str) -> String {
        // `wip: true` is what Frontmatter::new gives a new article, so the fixture states it rather
        // than relying on a default.
        format!(
            "---\nid: {}\ntype: {}\nslug: {}\ntitle: A Document\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\nwip: {}\nwip_publish: false\n---\n\nBody text.\n",
            id.to_text(),
            kind.as_str(),
            slug,
            kind.supports_wip()
        )
    }

    #[test]
    fn a_missing_document_reports_which_one_is_missing() {
        let (_dir, paths) = paths();
        let id = StableId::from_parts(1, 1);
        let error = read_document(&paths, DocumentRef::new(ContentKind::Article, id)).unwrap_err();
        assert_eq!(error.code(), "INVALID_INPUT");
        assert!(error.to_string().contains(&id.to_text()), "{error}");
    }

    #[test]
    fn a_document_is_read_from_the_file_with_the_id_in_its_name() {
        let (_dir, paths) = paths();
        let id = StableId::from_parts(1_700_000_000_000, 5);
        let reference = DocumentRef::new(ContentKind::Article, id);
        fs::create_dir_all(paths.kind_dir(ContentKind::Article)).unwrap();
        fs::write(
            paths.document_path(ContentKind::Article, id),
            sample_source(ContentKind::Article, id, "hello"),
        )
        .unwrap();

        let loaded = read_document(&paths, reference).unwrap();
        assert_eq!(loaded.slug(), "hello");
        assert_eq!(loaded.title(), "A Document");
        assert_eq!(loaded.body().trim(), "Body text.");
        assert!(loaded.matches_disk().unwrap());
        assert!(loaded.is_preview_only(), "a new article starts as a WIP");
    }

    #[test]
    fn listing_ignores_files_that_are_not_markdown_or_not_named_by_id() {
        let (_dir, paths) = paths();
        let directory = paths.kind_dir(ContentKind::Article);
        fs::create_dir_all(&directory).unwrap();
        let id = StableId::from_parts(1_700_000_000_000, 9);
        fs::write(
            directory.join(format!("{}.md", id.to_text())),
            sample_source(ContentKind::Article, id, "a"),
        )
        .unwrap();
        // None of these should be listed as documents.
        fs::write(directory.join("notes.txt"), "not markdown").unwrap();
        fs::write(directory.join("draft.md"), "no id in the name").unwrap();

        let listed = list_documents(&paths, ContentKind::Article).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
    }

    #[test]
    fn listing_an_absent_directory_is_empty_rather_than_an_error() {
        // A project where the user has not written any pages yet is normal, not a failure.
        let (_dir, paths) = paths();
        assert!(list_documents(&paths, ContentKind::Page).unwrap().is_empty());
        assert!(scan_document_paths(&paths, ContentKind::Page).unwrap().is_empty());
    }

    #[test]
    fn scanning_reports_a_file_whose_name_is_not_an_id() {
        // A file the user created by hand must be visible as a problem, not invisible.
        let (_dir, paths) = paths();
        let directory = paths.kind_dir(ContentKind::Knowledge);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("hand-written.md"), "---\nid: x\n---\n").unwrap();

        let scanned = scan_document_paths(&paths, ContentKind::Knowledge).unwrap();
        assert_eq!(scanned.len(), 1);
        assert!(scanned[0].1.is_none(), "an unusable name yields no reference");
    }

    #[test]
    fn an_identity_mismatch_between_the_name_and_the_frontmatter_is_detected() {
        let (_dir, paths) = paths();
        let name_id = StableId::from_parts(1_700_000_000_000, 11);
        let body_id = StableId::from_parts(1_700_000_000_000, 12);
        let source = sample_source(ContentKind::Article, body_id, "x");
        let document = veloct_markdown::parse::parse(&source).unwrap();
        let path = paths.document_path(ContentKind::Article, name_id);

        let mismatch = identity_mismatch(&path, &document).expect("a mismatch is reported");
        assert!(mismatch.contains(&name_id.to_text()), "{mismatch}");
        assert!(mismatch.contains(&body_id.to_text()), "{mismatch}");
    }

    #[test]
    fn a_matching_identity_reports_no_mismatch() {
        let (_dir, paths) = paths();
        let id = StableId::from_parts(1_700_000_000_000, 13);
        let source = sample_source(ContentKind::Article, id, "x");
        let document = veloct_markdown::parse::parse(&source).unwrap();
        let path = paths.document_path(ContentKind::Article, id);
        assert!(identity_mismatch(&path, &document).is_none());
    }

    #[test]
    fn writing_reports_whether_the_file_actually_changed() {
        let (_dir, paths) = paths();
        let id = StableId::from_parts(1_700_000_000_000, 14);
        let path = paths.document_path(ContentKind::Article, id);
        let source = sample_source(ContentKind::Article, id, "x");

        assert!(write_document(&path, &source).unwrap(), "the first write changes");
        assert!(!write_document(&path, &source).unwrap(), "a repeat does not");
        assert!(write_document(&path, "different").unwrap());
    }

    #[test]
    fn the_expected_type_matches_the_kind() {
        assert_eq!(
            expected_type(ContentKind::Knowledge).as_str(),
            ContentKind::Knowledge.as_str()
        );
    }
}
