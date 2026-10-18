//! The content service: create, read, save, list, and reference repair.
//!
//! This is the surface the desktop commands and the UI call. Three rules are enforced here rather
//! than trusted from a caller:
//!
//! * **Autosave never commits to Git.** `handoff.md` §4.3 says so explicitly, and the way to honour
//!   it is for this crate to have no Git dependency at all: there is no code path that could commit
//!   even by accident. A commit happens only through the release pipeline.
//! * **A save is refused when the file moved underneath the editor.** The check lives in
//!   [`crate::model::save_with_baseline`], so every write path inherits it.
//! * **A repaired reference is rewritten to a target the CALLER names.** There is no code here that
//!   picks a paragraph by similarity or by position, because that is the silent repointing
//!   `handoff.md` §9.3 forbids.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use veloct_core::ids::StableId;
use veloct_core::workdir::FileRevision;
use veloct_core::{Result, VeloctError};
use veloct_markdown::ast::ParagraphId;
use veloct_markdown::frontmatter::{ContentType, Frontmatter, Timestamps};
use veloct_markdown::references::KnowledgeRef;
use veloct_markdown::rewrite;

use crate::index::{self, ContentIndex};
use crate::lifecycle::{self, FinishWipOutcome, WipState, WipTransition};
use crate::model::{
    ContentKind, ContentPaths, DocumentRef, LoadedDocument, SaveOutcome, Workspace,
};
use crate::store;

/// What to set when creating a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateOptions {
    pub kind: ContentKind,
    pub title: String,
    /// The public slug. When empty, it is derived from the title.
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub summary: Option<String>,
    /// The initial body. Paragraph ids are assigned to it.
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// Whether a new article starts as a WIP. Defaults to true for an article
    /// (`handoff.md` §7.1), which is why the field is an `Option` rather than a `bool`.
    #[serde(default)]
    pub start_as_wip: Option<bool>,
}

impl CreateOptions {
    pub fn new(kind: ContentKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            slug: String::new(),
            summary: None,
            body: String::new(),
            tags: Vec::new(),
            category: None,
            start_as_wip: None,
        }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_slug(mut self, slug: impl Into<String>) -> Self {
        self.slug = slug.into();
        self
    }

    /// Choose whether a new article starts as a WIP. Ignored for a page or a knowledge entry, which
    /// have no WIP state.
    pub fn start_as_wip(mut self, wip: bool) -> Self {
        self.start_as_wip = Some(wip);
        self
    }

    /// Whether the created document is a WIP.
    ///
    /// An article defaults to a draft, because publishing something the moment it is created is not
    /// what an author means by "new article". A page or a knowledge entry has no WIP state at all
    /// (§3.2), so the flag is meaningless there and is reported as false.
    pub fn is_wip(&self) -> bool {
        if !self.kind.supports_wip() {
            return false;
        }
        self.start_as_wip.unwrap_or(true)
    }

    /// The slug to use, derived from the title when none was given.
    pub fn resolved_slug(&self) -> String {
        let candidate = if self.slug.trim().is_empty() {
            veloct_core::paths::slugify(&self.title)
        } else {
            self.slug.trim().to_string()
        };
        if candidate.is_empty() {
            // A title of only punctuation would slugify to nothing, which would produce a document
            // whose public address is the site root. A neutral fallback keeps it addressable.
            "untitled".to_string()
        } else {
            candidate
        }
    }
}

/// A row in the sidebar ribbon, one per document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RibbonEntry {
    pub kind: ContentKind,
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub wip_state: WipState,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Whether this document appears in a production build.
    pub is_buildable: bool,
    /// The route it occupies, so the UI can link to the preview.
    pub route: String,
}

/// A reference repair: point a broken reference at a target the user chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceRepair {
    /// The document containing the broken reference.
    pub document_id: String,
    /// The knowledge document the reference currently names.
    pub from_content_id: String,
    /// The paragraph the reference currently names, which may not exist.
    pub from_paragraph_id: String,
    /// The knowledge document to point at. May equal `from_content_id` when only the paragraph is
    /// wrong.
    pub to_content_id: String,
    /// The paragraph to point at. This is the user's choice, never an inference.
    pub to_paragraph_id: String,
}

/// The content service for one project.
pub struct ContentService {
    workspace: Workspace,
}

impl ContentService {
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace: Workspace::new(root),
        }
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn paths(&self) -> &ContentPaths {
        self.workspace.paths()
    }

    /// Create a document from a COMPLETE Markdown source, re-stamping the fields Veloct owns.
    ///
    /// This is the AI page creator's confirm step (`handoff.md` §8.1). The source already carries
    /// frontmatter — `veloct-ai` built it with `veloct_markdown` rather than inventing a private format
    /// (§8.2) — so this does not re-derive the document from scratch. What it does NOT trust is the
    /// identity: the id, the timestamps and the WIP flags are re-stamped from `new_id` and `now` here.
    ///
    /// Re-stamping rather than validating is deliberate. A caller could hand this method a source whose id
    /// was generated a minute ago, or whose `created_at` came from a model that wrote 1999, and either
    /// would produce a file whose frontmatter disagrees with how it is addressed. Re-stamping makes "every
    /// created document has a fresh identity" true by construction instead of by a check that could be
    /// forgotten.
    pub fn create_from_source(
        &self,
        kind: ContentKind,
        source: &str,
        now: &str,
        new_id: StableId,
    ) -> Result<LoadedDocument> {
        let parsed = veloct_markdown::parse::parse(source)?;
        if parsed.diagnostics.has_errors() {
            let errors: Vec<String> = parsed
                .diagnostics
                .errors()
                .map(|diagnostic| format!("{}: {}", diagnostic.code.as_str(), diagnostic.message))
                .collect();
            return Err(VeloctError::Invalid(format!(
                "the document has {} structural error(s), so it was not created: {}",
                errors.len(),
                errors.join("; ")
            )));
        }

        let reference = DocumentRef::new(kind, new_id);
        let path = self.paths().document_path(kind, new_id);
        if path.exists() {
            return Err(VeloctError::Invalid(format!(
                "{} document {} already exists",
                kind.as_str(),
                new_id.to_text()
            )));
        }

        // A slug the source proposed is honoured, but it is checked exactly as `create` checks one: two
        // documents sharing a public URL would make the build depend on iteration order.
        let mut frontmatter = parsed.frontmatter.clone();
        let slug = if frontmatter.slug.trim().is_empty() {
            veloct_core::paths::slugify(&frontmatter.title)
        } else {
            veloct_core::paths::slugify(&frontmatter.slug)
        };
        let slug = if slug.is_empty() {
            new_id.to_text().to_ascii_lowercase()
        } else {
            slug
        };
        if let Some(owner) = self.slug_owner(kind, &slug, reference)? {
            return Err(VeloctError::Invalid(format!(
                "the slug {slug:?} is already used by another {} document",
                owner.kind.as_str()
            )));
        }

        frontmatter.id = new_id;
        frontmatter.content_type = kind.content_type();
        frontmatter.slug = slug;
        frontmatter.timestamps = Timestamps {
            created_at: now.to_string(),
            updated_at: now.to_string(),
            published_at: None,
        };
        if kind.supports_wip() {
            // A new article starts as a draft, and no redirect is promised until it is published.
            frontmatter.wip = true;
            frontmatter.wip_publish = false;
            frontmatter.wip_ever_published = false;
        } else {
            frontmatter.wip = false;
            frontmatter.wip_publish = false;
            frontmatter.wip_ever_published = false;
        }

        let assembled = crate::model::new_document_source(&frontmatter, &parsed.body)?;
        // Paragraph ids are assigned so a reference can be made to the new document immediately (§9.2).
        let (source, _) = veloct_markdown::rewrite::assign_paragraph_ids(
            &assembled,
            &mut veloct_markdown::ast::ParagraphId::new,
        );
        store::write_document(&path, &source)?;
        store::read_document(self.paths(), reference)
    }

    /// Create a document, refusing to overwrite one that already exists.
    ///
    /// The id is generated fresh, so a create can never collide with an existing file. The check
    /// exists for the case a caller supplies something unexpected rather than for the generated id.
    pub fn create(
        &self,
        options: &CreateOptions,
        now: &str,
        new_id: StableId,
    ) -> Result<LoadedDocument> {
        let reference = DocumentRef::new(options.kind, new_id);
        let path = self.paths().document_path(options.kind, new_id);
        if path.exists() {
            return Err(VeloctError::Invalid(format!(
                "{} document {} already exists",
                options.kind.as_str(),
                new_id.to_text()
            )));
        }

        let slug = options.resolved_slug();
        // Two documents sharing a public URL would make the build depend on iteration order, so the
        // slug is checked before anything is written.
        if let Some(owner) = self.slug_owner(options.kind, &slug, reference)? {
            return Err(VeloctError::Invalid(format!(
                "the slug {slug:?} is already used by another {} document",
                owner.kind.as_str()
            )));
        }

        let mut frontmatter = Frontmatter::new(
            options.kind.content_type(),
            new_id,
            slug,
            options.title.clone(),
            now,
        );
        frontmatter.summary = options.summary.clone();
        frontmatter.tags = options.tags.clone();
        frontmatter.category = options.category.clone();
        frontmatter.timestamps = Timestamps {
            created_at: now.to_string(),
            updated_at: now.to_string(),
            published_at: None,
        };
        // A new article starts as a draft unless the caller says otherwise; a page or a knowledge
        // entry has no WIP state, and `Frontmatter::new` already leaves them unpublished-but-not-WIP.
        if options.kind.supports_wip() {
            frontmatter.wip = options.is_wip();
            frontmatter.wip_publish = false;
            frontmatter.wip_ever_published = false;
        }

        let source = crate::model::new_document_source(&frontmatter, &options.body)?;
        store::write_document(&path, &source)?;
        store::read_document(self.paths(), reference)
    }

    /// Read one document.
    pub fn read(&self, reference: DocumentRef) -> Result<LoadedDocument> {
        self.workspace.read(reference)
    }

    /// Read one document by kind and id text, which is what a command receives.
    pub fn read_by_id(&self, kind: ContentKind, id: &str) -> Result<LoadedDocument> {
        let id = id.parse::<StableId>()?;
        self.read(DocumentRef::new(kind, id))
    }

    /// Read a document, or `None` when it does not exist.
    pub fn read_optional_by_id(&self, kind: ContentKind, id: &str) -> Result<Option<LoadedDocument>> {
        let Ok(id) = id.parse::<StableId>() else {
            return Ok(None);
        };
        self.workspace.read_optional(DocumentRef::new(kind, id))
    }

    /// List the documents of one kind.
    pub fn list(&self, kind: ContentKind) -> Result<Vec<DocumentRef>> {
        self.workspace.list(kind)
    }

    /// List every document.
    pub fn list_all(&self) -> Result<Vec<DocumentRef>> {
        self.workspace.list_all()
    }

    /// Save a document the editor has open.
    ///
    /// `baseline` is the revision the editor read. The document must already exist: a save is not a
    /// create, and treating "the file is missing" as a create would resurrect a document the user
    /// deleted on purpose.
    pub fn save(
        &self,
        kind: ContentKind,
        id: &str,
        baseline: &FileRevision,
        source: &str,
        now: &str,
    ) -> Result<SaveOutcome> {
        let id = id.parse::<StableId>()?;
        let reference = DocumentRef::new(kind, id);
        let path = self.paths().document_path(kind, id);

        if !path.exists() {
            return Err(VeloctError::Invalid(format!(
                "{} document {} does not exist, so it cannot be saved; it may have been deleted",
                kind.as_str(),
                id.to_text()
            )));
        }

        // The revision may have come from a DIFFERENT document by mistake, which would defeat the
        // whole conflict check, so it is verified against the file before anything else.
        let _ = now;
        crate::model::save_with_baseline(self.paths(), reference, baseline, source, now)
    }

    /// Apply a WIP transition and save it.
    ///
    /// The transition is applied to the text the editor holds, so unsaved edits are not lost by
    /// clicking Publish.
    pub fn transition_wip(
        &self,
        id: &str,
        baseline: &FileRevision,
        current_source: &str,
        transition: WipTransition,
        now: &str,
    ) -> Result<(SaveOutcome, FinishWipOutcome)> {
        let (rewritten, outcome) = lifecycle::apply_transition(current_source, transition)?;
        let save = self.save(ContentKind::Article, id, baseline, &rewritten, now)?;
        Ok((save, outcome))
    }

    /// Rewrite a broken reference to the target the user chose.
    ///
    /// Two things this deliberately does NOT do: it does not search for a similar paragraph, and it
    /// does not leave the reference alone "because it probably meant the neighbouring one". Either
    /// would be the silent repointing `handoff.md` §9.3 forbids. If the target is wrong the reference
    /// stays broken, which is a state the user can see and fix again.
    pub fn repair_reference(
        &self,
        repair: &ReferenceRepair,
        baseline: &FileRevision,
        current_source: &str,
        now: &str,
    ) -> Result<(SaveOutcome, usize)> {
        // The target must exist. Rewriting to a paragraph that does not exist would just move the
        // broken state, so it is refused with an explanation instead.
        let target = self
            .read_optional_by_id(ContentKind::Knowledge, &repair.to_content_id)?
            .ok_or_else(|| {
                VeloctError::Invalid(format!(
                    "the knowledge document {} does not exist",
                    repair.to_content_id
                ))
            })?;
        let target_paragraph = target
            .document
            .unique_paragraph_ids()
            .into_iter()
            .find(|id| id.as_stable_id().to_text() == repair.to_paragraph_id)
            .ok_or_else(|| {
                VeloctError::Invalid(format!(
                    "paragraph {} does not exist in {}",
                    repair.to_paragraph_id,
                    target.title()
                ))
            })?;

        let mut rewrites: BTreeMap<String, String> = BTreeMap::new();
        // The content id is only rewritten when it changes, so repairing just the paragraph does not
        // touch the document name.
        if repair.from_content_id != repair.to_content_id {
            rewrites.insert(repair.from_content_id.clone(), repair.to_content_id.clone());
        }
        if repair.from_paragraph_id != repair.to_paragraph_id {
            rewrites.insert(
                repair.from_paragraph_id.clone(),
                target_paragraph.as_stable_id().to_text(),
            );
        }

        if rewrites.is_empty() {
            // Nothing to do, so the file is not rewritten and `updated_at` is not bumped.
            let save = self.save(
                ContentKind::Article,
                &repair.document_id,
                baseline,
                current_source,
                now,
            )?;
            return Ok((save, 0));
        }

        let (rewritten, count) = rewrite::rewrite_references(current_source, &rewrites);
        let save = self.save(
            ContentKind::Article,
            &repair.document_id,
            baseline,
            &rewritten,
            now,
        )?;
        Ok((save, count))
    }

    /// Give paragraph ids to every paragraph that lacks one.
    ///
    /// `handoff.md` §9.2: identity is written as a hidden comment and does not depend on the text, so
    /// doing this never moves an existing reference.
    pub fn assign_missing_paragraph_ids(
        &self,
        kind: ContentKind,
        id: &str,
        baseline: &FileRevision,
        current_source: &str,
        now: &str,
    ) -> Result<(SaveOutcome, usize)> {
        let (rewritten, added) = rewrite::assign_paragraph_ids(current_source, &mut || {
            ParagraphId::new()
        });
        let added_count = added.len();
        let save = self.save(kind, id, baseline, &rewritten, now)?;
        Ok((save, added_count))
    }

    /// Rename a document's public slug.
    ///
    /// Only the slug changes: the file stays where it is and the id is untouched
    /// (`handoff.md` §3.2). For an article that was once a published WIP this is what makes the old
    /// WIP address a redirect and the new article address canonical.
    pub fn rename_slug(
        &self,
        kind: ContentKind,
        id: &str,
        baseline: &FileRevision,
        current_source: &str,
        new_slug: &str,
        now: &str,
    ) -> Result<SaveOutcome> {
        let id_parsed = id.parse::<StableId>()?;
        let reference = DocumentRef::new(kind, id_parsed);
        // An empty rename is refused rather than run through `slugify`, which would turn it into
        // "untitled" and silently give the document an address the user did not ask for. Punctuation
        // still falls back to "untitled", because that IS what `slugify` documents.
        if new_slug.trim().is_empty() {
            return Err(VeloctError::Invalid(
                "a slug must not be empty; give it at least one usable character".to_string(),
            ));
        }
        let slug = veloct_core::paths::slugify(new_slug);
        // The check is what keeps two documents from sharing a public URL.
        if let Some(owner) = self.slug_owner(kind, &slug, reference)? {
            return Err(VeloctError::Invalid(format!(
                "the slug {slug:?} is already used by {}",
                owner.id_text()
            )));
        }

        let document = veloct_markdown::parse::parse(current_source)?;
        let mut frontmatter = document.frontmatter.clone();
        frontmatter.slug = slug;
        let rewritten = rewrite::serialize(&frontmatter, &document.body)?;
        let _ = now;
        self.save(kind, id, baseline, &rewritten, now)
    }

    /// Build the derived index by reading every Markdown file.
    pub fn build_index(&self) -> Result<ContentIndex> {
        index::build(self.paths())
    }

    /// The sidebar ribbon.
    ///
    /// Built from the index so the UI sees the same WIP state the build will act on, rather than a
    /// second interpretation of the frontmatter.
    pub fn ribbon(&self) -> Result<Vec<RibbonEntry>> {
        let index = self.build_index()?;
        let mut rows: Vec<RibbonEntry> = index
            .entries()
            .iter()
            .map(|entry| RibbonEntry {
                kind: entry.kind,
                id: entry.id.clone(),
                slug: entry.slug.clone(),
                title: entry.title.clone(),
                summary: entry.summary.clone(),
                wip_state: if entry.wip {
                    if entry.wip_publish {
                        WipState::PublishedWip
                    } else {
                        WipState::Draft
                    }
                } else if entry.wip_ever_published {
                    WipState::FinishedWithHistory
                } else {
                    WipState::Finished
                },
                created_at: entry.created_at.clone(),
                updated_at: entry.updated_at.clone(),
                category: entry.category.clone(),
                tags: entry.tags.clone(),
                is_buildable: entry.is_buildable(),
                route: entry.route(),
            })
            .collect();

        set_ribbon_routes(&mut rows);
        Ok(rows)
    }

    /// The references a document makes, from the index.
    pub fn references_of(&self, id: &str) -> Result<Vec<KnowledgeRef>> {
        Ok(self.build_index()?.references_of(id).to_vec())
    }

    /// The documents that cite a knowledge entry, so a knowledge edit can trigger the right rebuild
    /// (`handoff.md` §9.4).
    pub fn referencing(&self, content_id: &str) -> Result<Vec<RibbonEntry>> {
        let index = self.build_index()?;
        let mut rows: Vec<RibbonEntry> = index
            .referencing(content_id)
            .into_iter()
            .map(|entry| RibbonEntry {
                kind: entry.kind,
                id: entry.id.clone(),
                slug: entry.slug.clone(),
                title: entry.title.clone(),
                summary: entry.summary.clone(),
                wip_state: if entry.wip {
                    if entry.wip_publish {
                        WipState::PublishedWip
                    } else {
                        WipState::Draft
                    }
                } else if entry.wip_ever_published {
                    WipState::FinishedWithHistory
                } else {
                    WipState::Finished
                },
                created_at: entry.created_at.clone(),
                updated_at: entry.updated_at.clone(),
                category: entry.category.clone(),
                tags: entry.tags.clone(),
                is_buildable: entry.is_buildable(),
                route: entry.route(),
            })
            .collect();
        set_ribbon_routes(&mut rows);
        Ok(rows)
    }

    /// Delete a document's file.
    ///
    /// The file is removed rather than emptied, because an empty Markdown file is not a document: it
    /// has no frontmatter and no id. The absence IS the deletion, and the index stops listing it on
    /// the next rebuild.
    pub fn delete(&self, kind: ContentKind, id: &str) -> Result<()> {
        let id = id.parse::<StableId>()?;
        let path = self.paths().document_path(kind, id);
        if !path.exists() {
            return Err(VeloctError::Invalid(format!(
                "{} document {} does not exist",
                kind.as_str(),
                id.to_text()
            )));
        }
        std::fs::remove_file(&path).map_err(|error| VeloctError::io(&path, error))
    }

    /// Which document of this kind already uses a slug.
    fn slug_owner(
        &self,
        kind: ContentKind,
        slug: &str,
        exclude: DocumentRef,
    ) -> Result<Option<DocumentRef>> {
        for reference in self.list(kind)? {
            if reference == exclude {
                continue;
            }
            let path = self.paths().document_path(kind, reference.id);
            let Some(text) = veloct_core::workdir::read_text_optional(&path)? else {
                continue;
            };
            if let Ok(document) = veloct_markdown::parse::parse(&text) {
                if document.frontmatter.slug == slug {
                    return Ok(Some(reference));
                }
            }
        }
        Ok(None)
    }

    /// The frontmatter type a document of this kind must carry.
    pub fn content_type(&self, kind: ContentKind) -> ContentType {
        kind.content_type()
    }
}

/// Fill in each row's route from its own state.
fn set_ribbon_routes(rows: &mut [RibbonEntry]) {
    for row in rows.iter_mut() {
        row.route = row.kind.route(&row.id, &row.slug, row.wip_state);
    }
    // Sorted by kind, then newest first, then by title so the order is total and a rebuild is
    // comparable (handoff.md section 14.9).
    rows.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
            .then_with(|| a.title.cmp(&b.title))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn service() -> (TempDir, ContentService) {
        let dir = TempDir::new().unwrap();
        let service = ContentService::open(dir.path());
        (dir, service)
    }

    fn create_article(service: &ContentService, title: &str, seed: u128) -> LoadedDocument {
        service
            .create(
                &CreateOptions::new(ContentKind::Article, title).with_body("Hello body."),
                NOW,
                StableId::from_parts(1_700_000_000_000, seed),
            )
            .unwrap()
    }

    #[test]
    fn creating_an_article_writes_a_file_named_by_its_id() {
        // handoff.md section 3.2: the file is addressed by the stable id, not the slug.
        let (_dir, service) = service();
        let document = create_article(&service, "My First Post", 1);

        assert_eq!(document.slug(), "my-first-post", "the slug is derived from the title");
        assert!(document.path.exists());
        assert!(document
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&document.reference.id_text()));
        assert!(document.is_preview_only(), "a new article starts as a draft");
    }

    #[test]
    fn creating_assigns_paragraph_ids_to_the_body() {
        // handoff.md section 9.2: new paragraphs get identity immediately, so a reference can be
        // made to them right away.
        let (_dir, service) = service();
        let document = service
            .create(
                &CreateOptions::new(ContentKind::Article, "Post")
                    .with_body("First paragraph.\n\nSecond paragraph."),
                NOW,
                StableId::from_parts(1_700_000_000_000, 2),
            )
            .unwrap();

        assert_eq!(
            document.document.unique_paragraph_ids().len(),
            2,
            "both paragraphs were identified: {}",
            document.source
        );
    }

    #[test]
    fn creating_a_knowledge_entry_does_not_make_it_a_wip() {
        // handoff.md section 3.2: wip only has meaning for an article.
        let (_dir, service) = service();
        let document = service
            .create(
                &CreateOptions::new(ContentKind::Knowledge, "Rust Ownership"),
                NOW,
                StableId::from_parts(1_700_000_000_000, 3),
            )
            .unwrap();
        assert!(!document.document.frontmatter.wip);
        assert!(!document.is_preview_only());
        assert!(!document.frontmatter().wip_publish);
    }

    #[test]
    fn a_created_document_appears_in_the_index_and_the_ribbon() {
        let (_dir, service) = service();
        let document = create_article(&service, "Indexed", 4);

        let index = service.build_index().unwrap();
        assert!(index.is_buildable(), "{:?}", index.issues());
        assert!(index.entry(&document.reference.id_text()).is_some());

        let ribbon = service.ribbon().unwrap();
        assert_eq!(ribbon.len(), 1);
        assert_eq!(ribbon[0].wip_state, WipState::Draft);
        assert!(!ribbon[0].is_buildable);
        assert_eq!(ribbon[0].route, format!("/wip/{}/", document.reference.id_text()));
    }

    #[test]
    fn creating_a_second_document_with_the_same_slug_is_refused() {
        // Two documents at the same public URL would make the build depend on iteration order.
        let (_dir, service) = service();
        create_article(&service, "Duplicate", 5);
        let error = service
            .create(
                &CreateOptions::new(ContentKind::Article, "Duplicate"),
                NOW,
                StableId::from_parts(1_700_000_000_000, 6),
            )
            .unwrap_err();
        assert!(error.to_string().contains("already used"), "{error}");
    }

    #[test]
    fn a_title_of_only_punctuation_still_produces_an_addressable_slug() {
        let (_dir, service) = service();
        let document = service
            .create(
                &CreateOptions::new(ContentKind::Page, "!!! ???"),
                NOW,
                StableId::from_parts(1_700_000_000_000, 7),
            )
            .unwrap();
        assert_eq!(document.slug(), "untitled");
    }

    #[test]
    fn saving_changed_text_writes_it_and_bumps_the_revision() {
        let (_dir, service) = service();
        let document = create_article(&service, "Editable", 8);
        let edited = document.source.replace("Hello body.", "Edited body.");

        let outcome = service
            .save(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &edited,
                NOW,
            )
            .unwrap();

        assert!(outcome.is_success());
        assert!(outcome.is_change());
        assert!(outcome.revision.is_some());
        let reread = service.read(document.reference).unwrap();
        assert!(
            reread.body().contains("Edited body."),
            "the edit is on disk: {}",
            reread.body()
        );
    }

    #[test]
    fn saving_identical_text_writes_nothing() {
        // handoff.md section 14.9: an idle autosave must not make every release fingerprint differ.
        let (_dir, service) = service();
        let document = create_article(&service, "Stable", 9);

        let outcome = service
            .save(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &document.source,
                NOW,
            )
            .unwrap();

        assert_eq!(outcome.status, crate::model::SaveStatus::Unchanged);
        assert!(!outcome.is_change());
    }

    #[test]
    fn saving_after_an_external_edit_is_refused_and_returns_the_disk_text() {
        // handoff.md section 4.3 with section 20.2: the user's unsaved text is never silently
        // overwritten, and the disk text comes back so nothing has to be thrown away.
        let (_dir, service) = service();
        let document = create_article(&service, "Contested", 10);

        // Something outside Veloct rewrites the file.
        std::fs::write(&document.path, document.source.replace("Hello body.", "Edited by hand."))
            .unwrap();

        let mine = document.source.replace("Hello body.", "Edited in Veloct.");
        let outcome = service
            .save(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &mine,
                NOW,
            )
            .unwrap();

        assert!(outcome.is_conflict());
        assert!(!outcome.is_success(), "a refused save is not a success");
        let conflict = outcome.conflict.unwrap();
        assert_eq!(conflict.kind, crate::model::ConflictKind::ExternalModification);
        assert!(
            conflict.disk_source.unwrap().contains("Edited by hand."),
            "the disk text is returned for comparison"
        );
        // The external edit is untouched.
        let on_disk = std::fs::read_to_string(&document.path).unwrap();
        assert!(on_disk.contains("Edited by hand."), "the disk file was not overwritten");
        assert!(!on_disk.contains("Edited in Veloct."));
    }

    #[test]
    fn an_edit_that_changes_the_id_is_refused() {
        // handoff.md section 3.2: the id is permanent and is what addresses the file.
        let (_dir, service) = service();
        let document = create_article(&service, "Identity", 11);
        let tampered = document
            .source
            .replace(&document.reference.id_text(), &StableId::from_parts(1, 999).to_text());

        let outcome = service
            .save(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &tampered,
                NOW,
            )
            .unwrap();

        assert!(outcome.is_conflict());
        assert_eq!(
            outcome.conflict.unwrap().kind,
            crate::model::ConflictKind::IdentityChanged
        );
    }

    #[test]
    fn saving_an_unparsable_document_is_refused_with_a_diagnostic() {
        let (_dir, service) = service();
        let document = create_article(&service, "Broken", 12);

        let outcome = service
            .save(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                "no frontmatter at all",
                NOW,
            )
            .unwrap();

        assert!(outcome.is_conflict());
        assert_eq!(
            outcome.conflict.unwrap().kind,
            crate::model::ConflictKind::Unparsable
        );
        assert!(!outcome.diagnostics.is_empty(), "the reason is reported");
    }

    #[test]
    fn saving_a_document_that_was_deleted_is_refused_rather_than_recreating_it() {
        // Treating "the file is gone" as a create would resurrect something the user deleted on
        // purpose.
        let (_dir, service) = service();
        let document = create_article(&service, "Gone", 13);
        std::fs::remove_file(&document.path).unwrap();

        let error = service
            .save(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &document.source,
                NOW,
            )
            .unwrap_err();
        assert!(error.to_string().contains("does not exist"), "{error}");
        assert!(!document.path.exists(), "nothing was recreated");
    }

    #[test]
    fn publishing_a_wip_then_finishing_it_produces_a_redirect() {
        // The whole of handoff.md section 7.2, through the service.
        let (_dir, service) = service();
        let document = create_article(&service, "Lifecycle", 14);
        let id = document.reference.id_text();

        let (save, outcome) = service
            .transition_wip(&id, &document.revision, &document.source, WipTransition::PublishWip, NOW)
            .unwrap();
        assert!(save.is_success());
        assert!(outcome.published_wip);
        assert!(outcome.ever_published);

        let published = service.read(document.reference).unwrap();
        assert_eq!(
            service.ribbon().unwrap()[0].wip_state,
            WipState::PublishedWip
        );
        assert!(service.ribbon().unwrap()[0].is_buildable);

        let (save, outcome) = service
            .transition_wip(
                &id,
                &published.revision,
                &published.source,
                WipTransition::Finish,
                NOW,
            )
            .unwrap();
        assert!(save.is_success());
        assert!(outcome.needs_redirect, "the old WIP address must keep resolving");

        let finished = service.read(document.reference).unwrap();
        assert!(!finished.frontmatter().wip);
        assert!(finished.frontmatter().wip_ever_published);
        let ribbon = service.ribbon().unwrap();
        assert_eq!(ribbon[0].wip_state, WipState::FinishedWithHistory);
        assert_eq!(ribbon[0].route, "/article/lifecycle/");
    }

    #[test]
    fn a_transition_respects_an_external_edit() {
        // Clicking Publish must not overwrite someone else's change either.
        let (_dir, service) = service();
        let document = create_article(&service, "Contested Publish", 15);
        std::fs::write(&document.path, "replaced externally").unwrap();

        let (save, _) = service
            .transition_wip(
                &document.reference.id_text(),
                &document.revision,
                &document.source,
                WipTransition::PublishWip,
                NOW,
            )
            .unwrap();
        assert!(save.is_conflict());
        assert_eq!(
            std::fs::read_to_string(&document.path).unwrap(),
            "replaced externally"
        );
    }

    #[test]
    fn repairing_a_broken_reference_points_it_at_the_named_paragraph() {
        // handoff.md section 9.3: the target is the user's choice, never an inference.
        let (_dir, service) = service();
        let knowledge = service
            .create(
                &CreateOptions::new(ContentKind::Knowledge, "Target").with_body("The real text."),
                NOW,
                StableId::from_parts(1_700_000_000_000, 20),
            )
            .unwrap();
        let real_paragraph = knowledge.document.unique_paragraph_ids()[0];

        let article = service
            .create(
                &CreateOptions::new(ContentKind::Article, "Citing").with_body(format!(
                    ":::knowledgeref{{content=\"{}\" paragraph=\"{}\"}}\n:::",
                    knowledge.reference.id_text(),
                    StableId::from_parts(1, 12345).to_text()
                )),
                NOW,
                StableId::from_parts(1_700_000_000_000, 21),
            )
            .unwrap();

        let index = service.build_index().unwrap();
        assert!(!index.is_buildable(), "the reference is broken to begin with");

        let (save, replaced) = service
            .repair_reference(
                &ReferenceRepair {
                    document_id: article.reference.id_text(),
                    from_content_id: knowledge.reference.id_text(),
                    from_paragraph_id: StableId::from_parts(1, 12345).to_text(),
                    to_content_id: knowledge.reference.id_text(),
                    to_paragraph_id: real_paragraph.as_stable_id().to_text(),
                },
                &article.revision,
                &article.source,
                NOW,
            )
            .unwrap();

        assert!(save.is_success());
        assert_eq!(replaced, 1);
        let repaired = service.build_index().unwrap();
        assert!(
            repaired.is_buildable(),
            "the reference resolves now: {:?}",
            repaired.issues()
        );
    }

    #[test]
    fn repairing_to_a_paragraph_that_does_not_exist_is_refused() {
        // Rewriting to a nonexistent target would just move the broken state.
        let (_dir, service) = service();
        let knowledge = service
            .create(
                &CreateOptions::new(ContentKind::Knowledge, "Target").with_body("Text."),
                NOW,
                StableId::from_parts(1_700_000_000_000, 22),
            )
            .unwrap();
        let article = create_article(&service, "Citing", 23);

        let error = service
            .repair_reference(
                &ReferenceRepair {
                    document_id: article.reference.id_text(),
                    from_content_id: knowledge.reference.id_text(),
                    from_paragraph_id: StableId::from_parts(1, 1).to_text(),
                    to_content_id: knowledge.reference.id_text(),
                    to_paragraph_id: StableId::from_parts(1, 2).to_text(),
                },
                &article.revision,
                &article.source,
                NOW,
            )
            .unwrap_err();
        assert!(error.to_string().contains("does not exist"), "{error}");
    }

    #[test]
    fn repairing_to_a_missing_knowledge_document_is_refused() {
        let (_dir, service) = service();
        let article = create_article(&service, "Citing", 24);
        let error = service
            .repair_reference(
                &ReferenceRepair {
                    document_id: article.reference.id_text(),
                    from_content_id: "missing".to_string(),
                    from_paragraph_id: StableId::from_parts(1, 1).to_text(),
                    to_content_id: "missing".to_string(),
                    to_paragraph_id: StableId::from_parts(1, 2).to_text(),
                },
                &article.revision,
                &article.source,
                NOW,
            )
            .unwrap_err();
        assert!(error.to_string().contains("does not exist"), "{error}");
    }

    #[test]
    fn a_knowledge_edit_makes_the_citing_articles_reachable() {
        // handoff.md section 9.4: a knowledge change must rebuild the affected content.
        let (_dir, service) = service();
        let knowledge = service
            .create(
                &CreateOptions::new(ContentKind::Knowledge, "Cited").with_body("Cited text."),
                NOW,
                StableId::from_parts(1_700_000_000_000, 30),
            )
            .unwrap();
        let paragraph = knowledge.document.unique_paragraph_ids()[0];
        let article = service
            .create(
                &CreateOptions::new(ContentKind::Article, "Citing Article").with_body(format!(
                    ":::knowledgeref{{content=\"{}\" paragraph=\"{}\"}}\n:::",
                    knowledge.reference.id_text(),
                    paragraph.as_stable_id().to_text()
                )),
                NOW,
                StableId::from_parts(1_700_000_000_000, 31),
            )
            .unwrap();

        let referencing = service.referencing(&knowledge.reference.id_text()).unwrap();
        assert_eq!(referencing.len(), 1);
        assert_eq!(referencing[0].id, article.reference.id_text());
        assert!(!referencing[0].is_buildable, "it is still a draft");
    }

    #[test]
    fn assigning_missing_paragraph_ids_is_idempotent() {
        // handoff.md section 9.2: assigning identity never moves an existing reference.
        let (_dir, service) = service();
        let document = service
            .create(
                &CreateOptions::new(ContentKind::Knowledge, "K").with_body("One.\n\nTwo."),
                NOW,
                StableId::from_parts(1_700_000_000_000, 40),
            )
            .unwrap();

        // Strip the ids to simulate a hand-written file.
        let stripped: String = document
            .source
            .lines()
            .filter(|line| !line.trim_start().starts_with("<!--p:"))
            .collect::<Vec<_>>()
            .join("\n");

        let (save, added) = service
            .assign_missing_paragraph_ids(
                ContentKind::Knowledge,
                &document.reference.id_text(),
                &document.revision,
                &stripped,
                NOW,
            )
            .unwrap();
        assert!(save.is_success());
        assert_eq!(added, 2);

        let after = service.read(document.reference).unwrap();
        let again = service
            .assign_missing_paragraph_ids(
                ContentKind::Knowledge,
                &document.reference.id_text(),
                &after.revision,
                &after.source,
                NOW,
            )
            .unwrap();
        assert_eq!(again.1, 0, "nothing was added the second time");
    }

    #[test]
    fn renaming_a_slug_does_not_move_the_file_or_change_the_id() {
        // handoff.md section 3.2: this is the property that makes a rename safe.
        let (_dir, service) = service();
        let document = create_article(&service, "Old Name", 41);
        let original_path = document.path.clone();
        let id = document.reference.id_text();

        let save = service
            .rename_slug(
                ContentKind::Article,
                &id,
                &document.revision,
                &document.source,
                "A Brand New Name",
                NOW,
            )
            .unwrap();
        assert!(save.is_success());

        let renamed = service.read(document.reference).unwrap();
        assert_eq!(renamed.slug(), "a-brand-new-name");
        assert_eq!(renamed.path, original_path, "the file did not move");
        assert_eq!(renamed.reference.id_text(), id, "the id did not change");
    }

    #[test]
    fn renaming_to_a_slug_another_document_uses_is_refused() {
        let (_dir, service) = service();
        create_article(&service, "Taken", 42);
        let second = create_article(&service, "Second", 43);

        let error = service
            .rename_slug(
                ContentKind::Article,
                &second.reference.id_text(),
                &second.revision,
                &second.source,
                "Taken",
                NOW,
            )
            .unwrap_err();
        assert!(error.to_string().contains("already used"), "{error}");
    }

    #[test]
    fn renaming_to_nothing_usable_is_refused() {
        // `slugify` turns `!!!` into nothing, and an empty slug is refused rather than written: a
        // document with no slug would have no public address at all.
        let (_dir, service) = service();
        let document = create_article(&service, "Name", 44);
        let error = service
            .rename_slug(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &document.source,
                "",
                NOW,
            )
            .unwrap_err();
        assert!(error.to_string().contains("must not be empty"), "{error}");
    }

    #[test]
    fn renaming_to_punctuation_falls_back_the_way_slugify_documents() {
        // `slugify` is the single definition of a slug, so the service does not add a second rule:
        // unusable input becomes "untitled" rather than an error.
        let (_dir, service) = service();
        let document = create_article(&service, "Name", 46);
        let save = service
            .rename_slug(
                ContentKind::Article,
                &document.reference.id_text(),
                &document.revision,
                &document.source,
                "!!! ???",
                NOW,
            )
            .unwrap();
        assert!(save.is_success());
        assert_eq!(service.read(document.reference).unwrap().slug(), "untitled");
    }

    #[test]
    fn deleting_removes_the_file_and_the_document_leaves_the_index() {
        let (_dir, service) = service();
        let document = create_article(&service, "Temporary", 45);
        assert!(document.path.exists());

        service
            .delete(ContentKind::Article, &document.reference.id_text())
            .unwrap();
        assert!(!document.path.exists());
        assert!(service.build_index().unwrap().entries().is_empty());
        assert!(service.ribbon().unwrap().is_empty());
    }

    #[test]
    fn deleting_something_that_is_not_there_is_reported() {
        let (_dir, service) = service();
        let error = service
            .delete(
                ContentKind::Article,
                &StableId::from_parts(1, 1).to_text(),
            )
            .unwrap_err();
        assert!(error.to_string().contains("does not exist"), "{error}");
    }

    #[test]
    fn reading_by_id_rejects_something_that_is_not_an_id() {
        let (_dir, service) = service();
        assert!(service.read_by_id(ContentKind::Article, "not-an-id").is_err());
        assert!(service
            .read_optional_by_id(ContentKind::Article, "not-an-id")
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_ribbon_is_sorted_newest_first_within_a_kind() {
        let (_dir, service) = service();
        service
            .create(
                &CreateOptions::new(ContentKind::Article, "Older"),
                "2026-01-01T00:00:00Z",
                StableId::from_parts(1_700_000_000_000, 50),
            )
            .unwrap();
        service
            .create(
                &CreateOptions::new(ContentKind::Article, "Newer"),
                "2026-06-01T00:00:00Z",
                StableId::from_parts(1_700_000_000_000, 51),
            )
            .unwrap();

        let ribbon = service.ribbon().unwrap();
        assert_eq!(ribbon.len(), 2);
        assert_eq!(ribbon[0].title, "Newer");
        assert_eq!(ribbon[1].title, "Older");
    }

    #[test]
    fn create_options_default_an_article_to_a_draft_and_a_page_to_neither() {
        assert!(CreateOptions::new(ContentKind::Article, "x").is_wip());
        assert!(!CreateOptions::new(ContentKind::Article, "x")
            .start_as_wip(false)
            .is_wip());
        assert!(!CreateOptions::new(ContentKind::Page, "x").is_wip());
        assert!(!CreateOptions::new(ContentKind::Knowledge, "x").is_wip());
    }

    #[test]
    fn create_options_derive_a_slug_from_the_title_when_none_is_given() {
        assert_eq!(
            CreateOptions::new(ContentKind::Page, "Hello World").resolved_slug(),
            "hello-world"
        );
        assert_eq!(
            CreateOptions::new(ContentKind::Page, "Hello").with_slug("custom").resolved_slug(),
            "custom"
        );
    }

    #[test]
    fn the_ribbon_entries_round_trip_through_json_for_the_frontend() {
        let (_dir, service) = service();
        create_article(&service, "Serialised", 60);
        let ribbon = service.ribbon().unwrap();
        let json = serde_json::to_string(&ribbon).unwrap();
        assert!(json.contains("\"wip_state\":\"draft\""));
        assert!(json.contains("\"kind\":\"article\""));
        assert_eq!(serde_json::from_str::<Vec<RibbonEntry>>(&json).unwrap(), ribbon);
    }
}
