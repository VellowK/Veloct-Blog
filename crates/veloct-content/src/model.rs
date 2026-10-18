//! The content model: where a document lives, what a save is, and how a conflict is reported.
//!
//! Two design choices here are worth stating, because both are about not losing work:
//!
//! * A save carries the [`FileRevision`] the editor started from. A revision mismatch is reported as
//!   [`SaveStatus::Conflict`] with the current text, never as a silent overwrite. The database has no
//!   say in this: only the file on disk decides whether the editor is still looking at the truth.
//! * [`SaveOutcome::is_success`] is false for a conflict, so a caller cannot accidentally treat
//!   "refused to write" as "saved". An autosave loop that did so would quietly discard the user's
//!   work while reporting success, which is the worst possible failure for an editor.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::ids::StableId;
use veloct_core::paths::RelPath;
use veloct_core::workdir::FileRevision;
use veloct_core::{Result, VeloctError};
use veloct_markdown::diagnostics::{Diagnostic, DiagnosticCode};
use veloct_markdown::frontmatter::ContentType;
use veloct_markdown::parse::Document;
use veloct_markdown::rewrite;

/// Which kind of content a document is.
///
/// `handoff.md` §3.1: WIP is deliberately absent. It is an attribute of an article, not a kind, so a
/// "wip document" is not representable here and cannot be accidentally created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Article,
    Page,
    Knowledge,
}

impl ContentKind {
    /// The directory under `content/` holding this kind.
    ///
    /// Knowledge lives at `content/knowledge`, which is the layout
    /// `docs/spec/01-data-model.md` §1 fixes. A second location would mean the content service and
    /// the site could disagree about where the truth is.
    pub fn directory(self) -> &'static str {
        match self {
            Self::Article => "articles",
            Self::Page => "pages",
            Self::Knowledge => "knowledge",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Page => "page",
            Self::Knowledge => "knowledge",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "article" => Some(Self::Article),
            "page" => Some(Self::Page),
            "knowledge" => Some(Self::Knowledge),
            _ => None,
        }
    }

    /// The frontmatter type, so the two definitions cannot drift.
    pub fn content_type(self) -> ContentType {
        match self {
            Self::Article => ContentType::Article,
            Self::Page => ContentType::Page,
            Self::Knowledge => ContentType::Knowledge,
        }
    }

    /// The kinds a caller can create directly. Work is not a Markdown kind
    /// (`handoff.md` §10.1: a work is a directory with `work.json` and `site/`).
    pub fn all() -> [Self; 3] {
        [Self::Article, Self::Page, Self::Knowledge]
    }

    /// The public route for this kind. Built-in home content owns the site root, not /home/.
    pub fn route(self, id: &str, slug: &str, state: crate::lifecycle::WipState) -> String {
        match self {
            Self::Article => state.route(id, slug),
            Self::Page if slug == "home" => "/".to_string(),
            Self::Page => format!("/{slug}/"),
            Self::Knowledge => format!("/docs/{slug}/"),
        }
    }

    /// Whether WIP has any meaning. `handoff.md` §3.2: `wip` is only meaningful for an article.
    pub fn supports_wip(self) -> bool {
        matches!(self, Self::Article)
    }
}

impl std::fmt::Display for ContentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The project's content directories.
#[derive(Debug, Clone)]
pub struct ContentPaths {
    root: PathBuf,
}

impl ContentPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory holding one kind of Markdown.
    pub fn kind_dir(&self, kind: ContentKind) -> PathBuf {
        self.root.join("content").join(kind.directory())
    }

    /// The file for a document of this kind with this stable id.
    pub fn document_path(&self, kind: ContentKind, id: StableId) -> PathBuf {
        self.kind_dir(kind).join(format!("{}.md", id.to_text()))
    }

    /// The `works/` directory.
    pub fn works_dir(&self) -> PathBuf {
        self.root.join("works")
    }

    /// The directory for one work.
    pub fn work_dir(&self, id: StableId) -> PathBuf {
        self.works_dir().join(id.to_text())
    }

    /// The context bundle for a workspace, so a bundle is built from the same paths as a read.
    pub fn all_kind_dirs(&self) -> Vec<(ContentKind, PathBuf)> {
        ContentKind::all()
            .into_iter()
            .map(|kind| (kind, self.kind_dir(kind)))
            .collect()
    }
}

/// A document identity: its kind and its stable id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DocumentRef {
    pub kind: ContentKind,
    pub id: StableId,
}

impl DocumentRef {
    pub fn new(kind: ContentKind, id: StableId) -> Self {
        Self { kind, id }
    }

    /// The id as stored in frontmatter and used in a URL.
    pub fn id_text(&self) -> String {
        self.id.to_text()
    }
}

impl std::fmt::Display for DocumentRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.id.to_text())
    }
}

/// A document as loaded from disk, with the revision it was read at.
#[derive(Debug, Clone)]
pub struct LoadedDocument {
    pub reference: DocumentRef,
    pub path: PathBuf,
    /// The raw file text, exactly as on disk. The editor round-trips this rather than reassembling
    /// from parts, so unknown frontmatter keys and body formatting survive an edit.
    pub source: String,
    pub document: Document,
    /// The revision this snapshot came from, which the next save must present.
    pub revision: FileRevision,
}

impl LoadedDocument {
    pub fn frontmatter(&self) -> &veloct_markdown::frontmatter::Frontmatter {
        &self.document.frontmatter
    }

    pub fn body(&self) -> &str {
        &self.document.body
    }

    pub fn slug(&self) -> &str {
        &self.document.frontmatter.slug
    }

    pub fn title(&self) -> &str {
        &self.document.frontmatter.title
    }

    /// Whether this document is a WIP that is currently published to the site.
    pub fn is_published_wip(&self) -> bool {
        self.document.frontmatter.wip && self.document.frontmatter.wip_publish
    }

    /// Whether this document must not appear in a production build.
    pub fn is_preview_only(&self) -> bool {
        self.document.frontmatter.wip && !self.document.frontmatter.wip_publish
    }

    /// Whether the loaded source still matches the file on disk.
    pub fn matches_disk(&self) -> Result<bool> {
        self.revision.matches_disk(&self.path)
    }

    /// Save this document back, refusing when the file changed since it was read.
    ///
    /// The source is passed in rather than mutated in place so the caller decides what it is
    /// writing; this function's job is to decide whether writing is safe.
    pub fn save(
        &self,
        paths: &ContentPaths,
        new_source: &str,
        now: &str,
    ) -> Result<SaveOutcome> {
        save_with_baseline(paths, self.reference, &self.revision, new_source, now)
    }
}

/// Why a save was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The file changed on disk since the editor read it.
    ExternalModification,
    /// Another document of the same kind already uses this slug.
    SlugTaken,
    /// The stable id in the new source is not the id this document is stored under.
    ///
    /// `handoff.md` §3.2: the id is permanent and is what addresses the file. An edit that changes
    /// it would orphan the file's identity, so it is refused rather than applied.
    IdentityChanged,
    /// The new source has no usable frontmatter or cannot be parsed.
    Unparsable,
}

impl ConflictKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalModification => "EXTERNAL_MODIFICATION",
            Self::SlugTaken => "SLUG_TAKEN",
            Self::IdentityChanged => "IDENTITY_CHANGED",
            Self::Unparsable => "UNPARSABLE",
        }
    }

    /// A message the editor can show without further interpretation.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::ExternalModification => {
                "the file changed on disk since it was opened; Veloct did not overwrite it"
            }
            Self::SlugTaken => "another document of this kind already uses this slug",
            Self::IdentityChanged => {
                "the id in this document is permanent and cannot be changed by editing"
            }
            Self::Unparsable => "the document could not be parsed",
        }
    }
}

/// A refused save, carrying what the editor needs to resolve it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveConflict {
    pub kind: ConflictKind,
    pub message: String,
    /// The text currently on disk, so the editor can offer a comparison or a reload.
    ///
    /// Present for an external modification. This is the whole point: the user's unsaved text is
    /// still in the editor, and the disk text is here, so nothing has to be thrown away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_source: Option<String>,
}

/// The result of a save attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveStatus {
    /// The file was written.
    ///
    /// The variant is named `Published` in the serialised form for the frontend: a WIP's
    /// `wip_publish` flag is what makes it appear on the site, and the UI shows that as published.
    #[serde(rename = "saved")]
    Saved,
    /// The content was already identical, so nothing was written.
    ///
    /// Distinct from `Saved` because an autosave that fires every 30 seconds must not keep bumping
    /// `updated_at` or rewriting an unchanged file: that would make every release fingerprint differ
    /// for no reason (`handoff.md` §14.9).
    Unchanged,
    /// The save was refused.
    Conflict,
}

/// What happened when a document was saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveOutcome {
    pub status: SaveStatus,
    /// The revision after a successful write, for the next save to present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<FileRevision>,
    /// The text that is now on disk, after frontmatter normalisation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict: Option<SaveConflict>,
    /// Diagnostics from parsing what was written, so a broken reference is visible immediately.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<veloct_markdown::diagnostics::Diagnostic>,
}

impl SaveOutcome {
    pub fn saved(revision: FileRevision, source: String) -> Self {
        Self {
            status: SaveStatus::Saved,
            revision: Some(revision),
            source: Some(source),
            conflict: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn unchanged(revision: FileRevision, source: String) -> Self {
        Self {
            status: SaveStatus::Unchanged,
            revision: Some(revision),
            source: Some(source),
            conflict: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn conflicted(kind: ConflictKind, disk_source: Option<String>) -> Self {
        Self {
            status: SaveStatus::Conflict,
            revision: None,
            source: None,
            conflict: Some(SaveConflict {
                kind,
                message: kind.explanation().to_string(),
                disk_source,
            }),
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<veloct_markdown::diagnostics::Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    /// Whether the document is now durably stored.
    ///
    /// False for a conflict, deliberately, so an autosave loop cannot report success after refusing
    /// to write.
    pub fn is_success(&self) -> bool {
        matches!(self.status, SaveStatus::Saved | SaveStatus::Unchanged)
    }

    pub fn is_conflict(&self) -> bool {
        self.status == SaveStatus::Conflict
    }

    /// Whether the file itself changed on disk.
    pub fn is_change(&self) -> bool {
        self.status == SaveStatus::Saved
    }
}

/// Whether a file already exists at a path, for a create that must not overwrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExistingFile {
    /// Nothing is there.
    Absent,
    /// A file is there.
    Present,
}

/// The workspace: a project root plus the paths derived from it.
#[derive(Debug, Clone)]
pub struct Workspace {
    paths: ContentPaths,
}

impl Workspace {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            paths: ContentPaths::new(root),
        }
    }

    pub fn paths(&self) -> &ContentPaths {
        &self.paths
    }

    pub fn root(&self) -> &Path {
        self.paths.root()
    }

    /// Read a document, or report that it does not exist.
    pub fn read(&self, reference: DocumentRef) -> Result<LoadedDocument> {
        crate::store::read_document(&self.paths, reference)
    }

    /// Read a document, returning `None` when its file is absent.
    pub fn read_optional(&self, reference: DocumentRef) -> Result<Option<LoadedDocument>> {
        let path = self.paths.document_path(reference.kind, reference.id);
        if !path.exists() {
            return Ok(None);
        }
        self.read(reference).map(Some)
    }

    /// Whether a document's file exists.
    pub fn exists(&self, reference: DocumentRef) -> bool {
        self.paths
            .document_path(reference.kind, reference.id)
            .exists()
    }

    /// Every document of one kind, sorted by id.
    pub fn list(&self, kind: ContentKind) -> Result<Vec<DocumentRef>> {
        crate::store::list_documents(&self.paths, kind)
    }

    /// Every document in the project, sorted.
    pub fn list_all(&self) -> Result<Vec<DocumentRef>> {
        let mut out = Vec::new();
        for kind in ContentKind::all() {
            out.extend(self.list(kind)?);
        }
        Ok(out)
    }
}

/// Write a document, refusing when the baseline no longer matches.
///
/// This is the only write path for Markdown, so the conflict rule cannot be bypassed. A caller with
/// no baseline (a fresh create) passes `None` and is then responsible for having checked absence.
pub fn save_with_baseline(
    paths: &ContentPaths,
    reference: DocumentRef,
    baseline: &FileRevision,
    new_source: &str,
    now: &str,
) -> Result<SaveOutcome> {
    let path = paths.document_path(reference.kind, reference.id);

    // The identity check comes first, because it is a property of the text rather than of the disk
    // and does not need the file to exist.
    let parsed = match veloct_markdown::parse::parse(new_source) {
        Ok(document) => document,
        Err(error) => {
            return Ok(SaveOutcome::conflicted(ConflictKind::Unparsable, None).with_diagnostics(
                vec![Diagnostic::new(
                    DiagnosticCode::InvalidFrontmatter,
                    format!("the document could not be parsed: {error}"),
                )],
            ))
        }
    };

    if parsed.frontmatter.id != reference.id {
        return Ok(SaveOutcome::conflicted(ConflictKind::IdentityChanged, None));
    }
    if parsed.frontmatter.content_type != reference.kind.content_type() {
        return Ok(SaveOutcome::conflicted(ConflictKind::IdentityChanged, None));
    }

    // The conflict check reads the CURRENT file rather than trusting a cached revision, so a change
    // made between the editor's last read and this save is caught.
    if path.exists() && !baseline.matches_disk(&path)? {
        let disk_source = veloct_core::workdir::read_text_optional(&path)?;
        return Ok(SaveOutcome::conflicted(
            ConflictKind::ExternalModification,
            disk_source,
        ));
    }

    // A slug must not collide with another document of the same kind: two documents sharing a public
    // URL would make the build's output depend on iteration order.
    if let Some(other) = find_slug_owner(paths, reference.kind, &parsed.frontmatter.slug, reference)? {
        return Ok(SaveOutcome::conflicted(
            ConflictKind::SlugTaken,
            Some(other),
        ));
    }

    // `updated_at` is only bumped when the text actually changes, so an idle autosave does not make
    // every document look modified (`handoff.md` §14.9).
    let on_disk = veloct_core::workdir::read_text_optional(&path)?;
    if let Some(existing) = &on_disk {
        if existing == new_source {
            let revision = FileRevision::of_text(new_source);
            return Ok(
                SaveOutcome::unchanged(revision, new_source.to_string())
                    .with_diagnostics(parsed.diagnostics.items.clone()),
            );
        }
    }

    let _ = now;
    veloct_core::workdir::ensure_parent(&path)?;
    let outcome = veloct_core::workdir::write_text_if_changed(&path, new_source)?;
    let revision = FileRevision::of_file(&path)?;

    let status = if outcome.is_change() {
        SaveStatus::Saved
    } else {
        SaveStatus::Unchanged
    };
    let mut result = if outcome.is_change() {
        SaveOutcome::saved(revision, new_source.to_string())
    } else {
        SaveOutcome::unchanged(revision, new_source.to_string())
    };
    debug_assert_eq!(result.status, status);
    result.diagnostics = parsed.diagnostics.items;
    Ok(result)
}

/// Find the text of a document already using this slug, if any.
///
/// Returns `None` when the slug is free. The owner's source is returned so the conflict message can
/// name the document that holds it, which is more useful than "the slug is taken".
fn find_slug_owner(
    paths: &ContentPaths,
    kind: ContentKind,
    slug: &str,
    exclude: DocumentRef,
) -> Result<Option<String>> {
    for reference in crate::store::list_documents(paths, kind)? {
        if reference == exclude {
            continue;
        }
        let path = paths.document_path(kind, reference.id);
        let Some(text) = veloct_core::workdir::read_text_optional(&path)? else {
            continue;
        };
        let Ok(document) = veloct_markdown::parse::parse(&text) else {
            continue;
        };
        if document.frontmatter.slug == slug {
            return Ok(Some(text));
        }
    }
    Ok(None)
}

/// Build a document from parts, with paragraph ids assigned.
///
/// `handoff.md` §9.2: a paragraph id is written as a hidden comment and does not depend on the
/// paragraph text, so editing the words never moves an existing reference. New paragraphs get one
/// immediately, so a reference can be made to them right away.
pub fn new_document_source(
    frontmatter: &veloct_markdown::frontmatter::Frontmatter,
    body: &str,
) -> Result<String> {
    let source = rewrite::serialize(frontmatter, body)?;
    let (with_ids, _) = rewrite::assign_paragraph_ids(&source, &mut veloct_markdown::ast::ParagraphId::new);
    Ok(with_ids)
}

/// A path that is safe to write inside the project.
///
/// Used for the paths a user supplies, so a crafted slug or filename cannot escape the project.
pub fn checked_relative(root: &Path, candidate: &Path) -> Result<PathBuf> {
    veloct_core::paths::ensure_within(root, candidate)
}

/// Turn a filesystem path into a project-relative path, or report that it is outside.
pub fn project_relative(root: &Path, path: &Path) -> Result<RelPath> {
    match veloct_core::paths::to_relative(root, path)? {
        Some(relative) => Ok(relative),
        None => Err(VeloctError::Invalid(format!(
            "{} is outside the project at {}",
            path.display(),
            root.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_directory_and_a_type() {
        for kind in ContentKind::all() {
            assert!(!kind.directory().is_empty());
            assert!(!kind.as_str().is_empty());
            assert_eq!(ContentKind::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.content_type().as_str(), kind.as_str());
            // The frontmatter type's directory is its location under `content/`; the kind's own
            // `directory()` is the leaf name, which `kind_dir` joins on. They agree on the leaf.
            assert!(
                kind.content_type().directory().ends_with(kind.directory()),
                "{} vs {}",
                kind.content_type().directory(),
                kind.directory()
            );
        }
    }

    #[test]
    fn only_an_article_supports_wip() {
        // handoff.md section 3.2: wip is only meaningful for an article.
        assert!(ContentKind::Article.supports_wip());
        assert!(!ContentKind::Page.supports_wip());
        assert!(!ContentKind::Knowledge.supports_wip());
    }

    #[test]
    fn there_is_no_wip_content_kind() {
        // handoff.md section 3.1: WIP is not an independent content type, so it must not be
        // representable as one.
        assert_eq!(ContentKind::parse("wip"), None);
        assert_eq!(ContentKind::parse("work"), None, "a work is not a Markdown kind");
    }

    #[test]
    fn knowledge_lives_under_content() {
        // docs/spec/01-data-model.md section 1: content/knowledge, so the service and the site
        // cannot disagree about where the truth is.
        let paths = ContentPaths::new("D:/proj");
        assert!(paths
            .kind_dir(ContentKind::Knowledge)
            .ends_with("content\\knowledge")
            || paths
                .kind_dir(ContentKind::Knowledge)
                .ends_with("content/knowledge"));
    }

    #[test]
    fn a_document_path_is_named_by_its_id_not_its_slug() {
        // handoff.md section 3.2: renaming a slug must never move the file.
        let paths = ContentPaths::new("D:/proj");
        let id = StableId::from_parts(1_700_000_000_000, 42);
        let path = paths.document_path(ContentKind::Article, id);
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{}.md", id.to_text())
        );
        assert!(path.parent().unwrap().ends_with("articles"));
    }

    #[test]
    fn a_save_conflict_is_not_a_success() {
        // The autosave loop must not report success after refusing to write.
        let outcome = SaveOutcome::conflicted(ConflictKind::ExternalModification, Some("disk".into()));
        assert!(!outcome.is_success());
        assert!(outcome.is_conflict());
        assert!(!outcome.is_change());
        assert!(outcome.conflict.is_some());
        assert_eq!(
            outcome.conflict.unwrap().kind,
            ConflictKind::ExternalModification
        );
    }

    #[test]
    fn an_unchanged_save_is_a_success_but_not_a_change() {
        let outcome = SaveOutcome::unchanged(FileRevision::of_text("x"), "x".to_string());
        assert!(outcome.is_success());
        assert!(!outcome.is_change());
        assert!(!outcome.is_conflict());
    }

    #[test]
    fn every_conflict_kind_explains_itself_and_has_a_code() {
        for kind in [
            ConflictKind::ExternalModification,
            ConflictKind::SlugTaken,
            ConflictKind::IdentityChanged,
            ConflictKind::Unparsable,
        ] {
            assert!(!kind.explanation().is_empty(), "{kind:?} has no explanation");
            assert!(!kind.as_str().is_empty());
        }
    }

    #[test]
    fn save_statuses_have_stable_serialised_forms() {
        assert_eq!(
            serde_json::to_string(&SaveStatus::Saved).unwrap(),
            "\"saved\""
        );
        assert_eq!(
            serde_json::to_string(&SaveStatus::Unchanged).unwrap(),
            "\"unchanged\""
        );
        assert_eq!(
            serde_json::to_string(&SaveStatus::Conflict).unwrap(),
            "\"conflict\""
        );
    }

    #[test]
    fn a_document_reference_round_trips_and_renders_readably() {
        let id = StableId::from_parts(1_700_000_000_000, 7);
        let reference = DocumentRef::new(ContentKind::Page, id);
        let json = serde_json::to_string(&reference).unwrap();
        assert!(json.contains("\"kind\":\"page\""));
        assert_eq!(serde_json::from_str::<DocumentRef>(&json).unwrap(), reference);
        assert!(reference.to_string().starts_with("page:"));
        assert_eq!(reference.id_text(), id.to_text());
    }
}
