//! The derived content index.
//!
//! This is DERIVED data and is treated as such (`handoff.md` §3.4): it is built by reading the
//! Markdown, it is never consulted as a source of truth, and deleting it loses nothing. The storage
//! crate can persist it, and `wipe_index` + `rebuild` regenerates it.
//!
//! Two properties the index exists to make checkable:
//!
//! * Every problem is REPORTED, not repaired. A duplicate paragraph id, a reference to a missing
//!   paragraph and a slug claimed twice all become [`IndexIssue`]s. Silently fixing any of them
//!   would hide a real conflict, and silently *repointing* a reference is the specific behaviour
//!   `handoff.md` §9.3 forbids.
//! * The knowledge index is built from document text, so an article's citation resolves against the
//!   CURRENT knowledge content. That is what makes a knowledge edit flow into every article that
//!   cites it without touching those articles (§9.4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use veloct_core::Result;
use veloct_markdown::ast::ParagraphId;
use veloct_markdown::references::{
    KnowledgeDocument, KnowledgeIndex, KnowledgeParagraph, KnowledgeRef,
};
use veloct_markdown::DiagnosticCode;

use crate::model::{ContentKind, ContentPaths, DocumentRef, LoadedDocument};
use crate::paragraphs::extract_paragraphs;
use crate::store;

/// How serious an index issue is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    /// Worth knowing, does not stop a build.
    Info,
    /// A problem the user should fix; the build still runs.
    Warning,
    /// Must stop a build (`handoff.md` §9.3).
    Error,
}

impl IssueSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

impl From<veloct_markdown::diagnostics::Severity> for IssueSeverity {
    fn from(severity: veloct_markdown::diagnostics::Severity) -> Self {
        match severity {
            veloct_markdown::diagnostics::Severity::Info => Self::Info,
            veloct_markdown::diagnostics::Severity::Warning => Self::Warning,
            veloct_markdown::diagnostics::Severity::Error => Self::Error,
        }
    }
}

/// One problem found while indexing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexIssue {
    pub severity: IssueSeverity,
    /// The diagnostic code, so a UI can group and link issues.
    pub code: String,
    pub message: String,
    /// The document the issue is IN, when it is about one document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// The document the issue POINTS AT, when it is about a reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The paragraph involved, for a paragraph-level issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph_id: Option<String>,
}

impl IndexIssue {
    pub fn new(severity: IssueSeverity, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            code: code.into(),
            message: message.into(),
            document: None,
            target: None,
            paragraph_id: None,
        }
    }

    pub fn in_document(mut self, id: impl Into<String>) -> Self {
        self.document = Some(id.into());
        self
    }

    pub fn targeting(mut self, id: impl Into<String>) -> Self {
        self.target = Some(id.into());
        self
    }

    pub fn at_paragraph(mut self, id: impl Into<String>) -> Self {
        self.paragraph_id = Some(id.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == IssueSeverity::Error
    }

    pub fn is_blocking(&self) -> bool {
        self.is_error()
    }

    /// A one-line rendering, for a log.
    pub fn summary(&self) -> String {
        let where_ = self
            .document
            .as_deref()
            .map(|document| format!(" [{document}]"))
            .unwrap_or_default();
        format!("{} {}{where_}: {}", self.severity.as_str(), self.code, self.message)
    }
}

/// One indexed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    pub kind: ContentKind,
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub wip: bool,
    pub wip_publish: bool,
    pub wip_ever_published: bool,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// How many paragraphs carry a stable id, which is how many can be referenced.
    pub identified_paragraphs: usize,
    /// How many paragraphs have no id and therefore cannot be referenced yet.
    pub unidentified_paragraphs: usize,
}

impl IndexEntry {
    /// Whether this document belongs in a production build.
    ///
    /// `handoff.md` §7.1: a WIP with `wip_publish` false is preview only; anything else is built.
    pub fn is_buildable(&self) -> bool {
        !(self.wip && !self.wip_publish)
    }

    /// The public route, which depends on the WIP state.
    pub fn route(&self) -> String {
        match self.kind {
            ContentKind::Article => {
                if self.wip {
                    format!("/wip/{}/", self.id)
                } else {
                    format!("/article/{}/", self.slug)
                }
            }
            ContentKind::Page if self.slug == "home" => "/".to_string(),
            ContentKind::Page => format!("/{}/", self.slug),
            ContentKind::Knowledge => format!("/docs/{}/", self.slug),
        }
    }
}

/// The whole derived index.
#[derive(Debug, Clone, Default)]
pub struct ContentIndex {
    entries: Vec<IndexEntry>,
    issues: Vec<IndexIssue>,
    knowledge: KnowledgeIndex,
    /// References keyed by the id of the document that makes them. Kept beside the entries rather
    /// than inside `IndexEntry` so an entry can be sent to the frontend without dragging every
    /// reference along with it.
    references: BTreeMap<String, Vec<KnowledgeRef>>,
}

impl ContentIndex {
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub fn issues(&self) -> &[IndexIssue] {
        &self.issues
    }

    pub fn knowledge(&self) -> &KnowledgeIndex {
        &self.knowledge
    }

    /// Look up one document.
    pub fn entry(&self, id: &str) -> Option<&IndexEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Only the entries a production build emits.
    pub fn buildable(&self) -> Vec<&IndexEntry> {
        self.entries.iter().filter(|entry| entry.is_buildable()).collect()
    }

    /// The issues that must stop a build.
    pub fn blocking_issues(&self) -> Vec<&IndexIssue> {
        self.issues.iter().filter(|issue| issue.is_blocking()).collect()
    }

    /// Whether a build may proceed (`handoff.md` §9.3: an error stops it).
    pub fn is_buildable(&self) -> bool {
        !self.issues.iter().any(IndexIssue::is_error)
    }

    /// The documents that cite a knowledge entry, for "a knowledge edit rebuilds the affected
    /// content" (`handoff.md` §9.4).
    ///
    /// The knowledge PAGE itself never lists these (§9.4 says the knowledge page does not show which
    /// articles reference it), so this is for the build, not for the site.
    pub fn referencing(&self, content_id: &str) -> Vec<&IndexEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == ContentKind::Article)
            .filter(|entry| {
                self.references_of(&entry.id)
                    .iter()
                    .any(|reference| reference.content_id == content_id)
            })
            .collect()
    }

    /// The references one document makes.
    pub fn references_of(&self, id: &str) -> Vec<KnowledgeRef> {
        self.references
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    /// Every reference made anywhere, keyed by source document id.
    pub fn references_by_document(&self) -> &BTreeMap<String, Vec<KnowledgeRef>> {
        &self.references
    }

    /// Resolve a reference against the current knowledge text.
    pub fn resolve(&self, reference: &KnowledgeRef) -> veloct_markdown::references::ReferenceResolution {
        self.knowledge.resolve(reference, 80)
    }

    /// The reference list one document would render (`handoff.md` §9.4).
    pub fn reference_list(&self, id: &str) -> veloct_markdown::references::ReferenceList {
        self.knowledge.reference_list(&self.references_of(id))
    }

    /// Every paragraph of a knowledge document, in order.
    pub fn knowledge_paragraphs(&self, content_id: &str) -> Vec<KnowledgeParagraph> {
        self.knowledge
            .get(content_id)
            .map(|document| document.paragraphs.clone())
            .unwrap_or_default()
    }
}

/// Build the index by reading every Markdown file in the project.
pub fn build(paths: &ContentPaths) -> Result<ContentIndex> {
    let mut index = ContentIndex::default();
    let mut references = BTreeMap::new();
    let mut issues = Vec::new();
    let mut knowledge_documents = Vec::new();
    let mut documents: Vec<(DocumentRef, LoadedDocument)> = Vec::new();

    for kind in ContentKind::all() {
        // A file whose NAME is not a stable id is reported rather than skipped: a hand-created file
        // would otherwise be invisible, and the user would have no idea why it never appears.
        for (path, reference) in store::scan_document_paths(paths, kind)? {
            let Some(reference) = reference else {
                issues.push(
                    IndexIssue::new(
                        IssueSeverity::Error,
                        DiagnosticCode::InvalidFrontmatter.as_str(),
                        format!(
                            "{} is not named by a stable id, so Veloct cannot address it",
                            path.file_name()
                                .map(|name| name.to_string_lossy().to_string())
                                .unwrap_or_default()
                        ),
                    )
                    .in_document(path.to_string_lossy().to_string()),
                );
                continue;
            };

            let loaded = match store::read_document(paths, reference) {
                Ok(loaded) => loaded,
                Err(error) => {
                    issues.push(
                        IndexIssue::new(
                            IssueSeverity::Error,
                            DiagnosticCode::InvalidFrontmatter.as_str(),
                            format!("could not be read: {error}"),
                        )
                        .in_document(reference.id_text()),
                    );
                    continue;
                }
            };

            // The file name and the frontmatter must agree; trusting either alone would make a
            // rename silently change a document's identity.
            if let Some(mismatch) = store::identity_mismatch(&loaded.path, &loaded.document) {
                issues.push(
                    IndexIssue::new(
                        IssueSeverity::Error,
                        DiagnosticCode::InvalidFrontmatterField.as_str(),
                        mismatch,
                    )
                    .in_document(reference.id_text()),
                );
            }

            documents.push((reference, loaded));
        }
    }

    // A slug must be unique within a kind, because two documents sharing a public URL would make
    // the build's output depend on iteration order.
    let mut slugs: BTreeMap<(ContentKind, String), String> = BTreeMap::new();

    for (reference, loaded) in &documents {
        let frontmatter = &loaded.document.frontmatter;

        if let Some(existing) = slugs.insert(
            (reference.kind, frontmatter.slug.clone()),
            reference.id_text(),
        ) {
            issues.push(
                IndexIssue::new(
                    IssueSeverity::Error,
                    DiagnosticCode::InvalidFrontmatterField.as_str(),
                    format!(
                        "the slug {:?} is already used by {} in the same content type; \
                         two documents cannot share a public URL",
                        frontmatter.slug, existing
                    ),
                )
                .in_document(reference.id_text())
                .targeting(existing),
            );
        }

        // The document's own diagnostics: duplicate paragraph id, unidentified paragraph, broken
        // directive, and the rest.
        for diagnostic in loaded.document.diagnostics.sorted() {
            issues.push(
                IndexIssue::new(
                    diagnostic.severity.into(),
                    diagnostic.code.as_str(),
                    diagnostic.message.clone(),
                )
                .in_document(reference.id_text()),
            );
        }

        let paragraphs = extract_paragraphs(loaded.body());
        let identified = paragraphs.iter().filter(|p| p.id.is_some()).count();
        let unidentified = paragraphs.len() - identified;

        if reference.kind == ContentKind::Knowledge {
            let knowledge_paragraphs: Vec<KnowledgeParagraph> = paragraphs
                .iter()
                .filter_map(|paragraph| {
                    paragraph.id.map(|id| {
                        KnowledgeParagraph::new(id, paragraph.order, paragraph.text.clone())
                    })
                })
                .collect();

            let document =
                KnowledgeDocument::from_frontmatter(frontmatter, knowledge_paragraphs);

            // A duplicate paragraph id inside one knowledge document makes a citation ambiguous, so
            // it is reported rather than resolved to the first match.
            for diagnostic in veloct_markdown::references::check_duplicates(&document).sorted() {
                issues.push(
                    IndexIssue::new(
                        diagnostic.severity.into(),
                        diagnostic.code.as_str(),
                        diagnostic.message.clone(),
                    )
                    .in_document(reference.id_text()),
                );
            }

            knowledge_documents.push(document);
        }

        index.entries.push(IndexEntry {
            kind: reference.kind,
            id: reference.id_text(),
            slug: frontmatter.slug.clone(),
            title: frontmatter.title.clone(),
            summary: frontmatter.summary.clone(),
            wip: frontmatter.wip,
            wip_publish: frontmatter.wip_publish,
            wip_ever_published: frontmatter.wip_ever_published,
            created_at: frontmatter.timestamps.created_at.clone(),
            updated_at: frontmatter.timestamps.updated_at.clone(),
            published_at: frontmatter.timestamps.published_at.clone(),
            tags: frontmatter.tags.clone(),
            category: frontmatter.category.clone(),
            identified_paragraphs: identified,
            unidentified_paragraphs: unidentified,
        });
    }

    index.knowledge = KnowledgeIndex::from_documents(knowledge_documents);

    // References are collected per document, then resolved against the finished knowledge index so a
    // reference to a document that appears later in the scan still resolves.
    for (reference, loaded) in &documents {
        // The directive scanner is the same code the parser uses, so "what counts as a reference"
        // has one definition rather than two.
        let scan = veloct_markdown::directives::scan(loaded.body());
        let mut found = Vec::new();
        for directive in &scan.directives {
            // A malformed directive is already a diagnostic on the document; it is not a reference,
            // so it is skipped here rather than invented.
            if let Ok(Some(knowledge_ref)) = KnowledgeRef::from_directive(
                &directive.name,
                &directive.attributes,
                format!("line {}", directive.start_line),
            ) {
                found.push(knowledge_ref);
            }
        }
        if !found.is_empty() {
            references.insert(reference.id_text(), found);
        }
    }

    for (document_id, found) in &references {
        for knowledge_ref in found {
            match index.knowledge.resolve(knowledge_ref, 80) {
                veloct_markdown::references::ReferenceResolution::Resolved { .. } => {}
                veloct_markdown::references::ReferenceResolution::Broken { message, .. } => {
                    // handoff.md section 9.3: the reference stays broken and waits for the user. It
                    // is never repointed at a different paragraph.
                    issues.push(
                        IndexIssue::new(
                            IssueSeverity::Error,
                            DiagnosticCode::BrokenReference.as_str(),
                            format!(
                                "the reference to {} paragraph {} cannot be resolved ({message}); \
                                 it stays broken until repaired",
                                knowledge_ref.content_id,
                                knowledge_ref.paragraph_id.as_stable_id().to_text()
                            ),
                        )
                        .in_document(document_id.clone())
                        .targeting(knowledge_ref.content_id.clone())
                        .at_paragraph(knowledge_ref.paragraph_id.as_stable_id().to_text()),
                    );
                }
            }
        }
    }

    index
        .entries
        .sort_by(|a, b| (a.kind, a.id.as_str()).cmp(&(b.kind, b.id.as_str())));
    index.issues = issues;
    index.references = references;
    Ok(index)
}

/// Resolve one paragraph id to its 1-based ordinal in a knowledge document.
///
/// Used by repair: the caller names the TARGET paragraph id, and its ordinal is what the site links
/// to. Deriving the ordinal here rather than in the UI keeps one definition of "which paragraph is
/// the third one".
pub fn paragraph_ordinal(paragraphs: &[KnowledgeParagraph], paragraph_id: ParagraphId) -> Option<usize> {
    paragraphs
        .iter()
        .find(|paragraph| paragraph.paragraph_id == paragraph_id)
        .map(|paragraph| paragraph.order)
}

/// The knowledge documents currently on disk.
pub fn knowledge_documents(paths: &ContentPaths) -> Result<Vec<KnowledgeDocument>> {
    let mut out = Vec::new();
    for reference in store::list_documents(paths, ContentKind::Knowledge)? {
        let loaded = store::read_document(paths, reference)?;
        let paragraphs: Vec<KnowledgeParagraph> = extract_paragraphs(loaded.body())
            .iter()
            .filter_map(|paragraph| {
                paragraph
                    .id
                    .map(|id| KnowledgeParagraph::new(id, paragraph.order, paragraph.text.clone()))
            })
            .collect();
        out.push(KnowledgeDocument::from_frontmatter(
            &loaded.document.frontmatter,
            paragraphs,
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn workspace() -> (TempDir, ContentPaths) {
        let dir = TempDir::new().unwrap();
        let paths = ContentPaths::new(dir.path());
        (dir, paths)
    }

    fn write(
        paths: &ContentPaths,
        kind: ContentKind,
        id: veloct_core::ids::StableId,
        slug: &str,
        title: &str,
        body: &str,
        extra: &str,
    ) {
        let directory = paths.kind_dir(kind);
        fs::create_dir_all(&directory).unwrap();
        let source = format!(
            "---\nid: {}\ntype: {}\nslug: {slug}\ntitle: {title}\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-02T00:00:00Z\n{extra}---\n\n{body}\n",
            id.to_text(),
            kind.as_str()
        );
        fs::write(paths.document_path(kind, id), source).unwrap();
    }

    fn knowledge_body(ids: &[ParagraphId]) -> String {
        ids.iter()
            .enumerate()
            .map(|(index, id)| format!("<!--p:{}-->\n\nParagraph {}.", id.as_stable_id().to_text(), index + 1))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    #[test]
    fn an_empty_project_indexes_cleanly() {
        let (_dir, paths) = workspace();
        let index = build(&paths).unwrap();
        assert!(index.entries().is_empty());
        assert!(index.issues().is_empty());
        assert!(index.is_buildable());
        assert!(index.knowledge().is_empty());
    }

    #[test]
    fn every_kind_is_indexed_with_its_own_route() {
        let (_dir, paths) = workspace();
        let article = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 1);
        let page = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 2);
        let knowledge = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 3);
        write(&paths, ContentKind::Article, article, "hello", "Hello", "Body.", "wip: false\n");
        write(&paths, ContentKind::Page, page, "about", "About", "Body.", "");
        write(&paths, ContentKind::Knowledge, knowledge, "rust", "Rust", "Body.", "");

        let index = build(&paths).unwrap();
        assert_eq!(index.entries().len(), 3);

        let article_entry = index.entry(&article.to_text()).unwrap();
        assert_eq!(article_entry.route(), "/article/hello/");
        let page_entry = index.entry(&page.to_text()).unwrap();
        assert_eq!(page_entry.route(), "/about/");
        let knowledge_entry = index.entry(&knowledge.to_text()).unwrap();
        assert_eq!(knowledge_entry.route(), "/docs/rust/");
    }

    #[test]
    fn an_unpublished_wip_is_not_buildable_but_is_still_indexed() {
        // handoff.md section 7.1: it exists and is editable, it just does not reach a production
        // build.
        let (_dir, paths) = workspace();
        let id = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 4);
        write(
            &paths,
            ContentKind::Article,
            id,
            "draft",
            "Draft",
            "Body.",
            "wip: true\nwip_publish: false\n",
        );

        let index = build(&paths).unwrap();
        let entry = index.entry(&id.to_text()).unwrap();
        assert!(entry.wip);
        assert!(!entry.is_buildable());
        assert_eq!(entry.route(), format!("/wip/{}/", id.to_text()));
        assert!(index.buildable().is_empty());
        assert!(index.is_buildable(), "no error was found");
    }

    #[test]
    fn a_published_wip_keeps_its_stable_id_address() {
        let (_dir, paths) = workspace();
        let id = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 5);
        write(
            &paths,
            ContentKind::Article,
            id,
            "shared",
            "Shared",
            "Body.",
            "wip: true\nwip_publish: true\n",
        );

        let index = build(&paths).unwrap();
        let entry = index.entry(&id.to_text()).unwrap();
        assert!(entry.is_buildable());
        // The address is the id, not the slug, so renaming the slug cannot move it.
        assert_eq!(entry.route(), format!("/wip/{}/", id.to_text()));
    }

    #[test]
    fn a_reference_resolves_against_the_current_knowledge_text() {
        // handoff.md section 9.4: a knowledge edit flows into every article that cites it.
        let (_dir, paths) = workspace();
        let knowledge = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 6);
        let article = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 7);
        let paragraph = ParagraphId::from_parts(1_700_000_000_000, 100);

        write(
            &paths,
            ContentKind::Knowledge,
            knowledge,
            "sqlite",
            "SQLite is only an index",
            &knowledge_body(&[paragraph]),
            "author: Vellow\n",
        );
        write(
            &paths,
            ContentKind::Article,
            article,
            "post",
            "Post",
            &format!(
                ":::knowledgeref{{content=\"{}\" paragraph=\"{}\"}}\n:::",
                knowledge.to_text(),
                paragraph.as_stable_id().to_text()
            ),
            "wip: false\n",
        );

        let index = build(&paths).unwrap();
        assert!(index.is_buildable(), "{:?}", index.issues());
        let found = index.references_of(&article.to_text());
        assert_eq!(found.len(), 1);
        let list = index.reference_list(&article.to_text());
        assert!(!list.is_empty());
        assert_eq!(index.referencing(&knowledge.to_text()).len(), 1);
    }

    #[test]
    fn a_reference_to_a_missing_paragraph_is_an_error_and_stays_broken() {
        // handoff.md section 9.3: never silently guess a different paragraph.
        let (_dir, paths) = workspace();
        let knowledge = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 8);
        let article = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 9);
        let real = ParagraphId::from_parts(1_700_000_000_000, 200);
        let missing = ParagraphId::from_parts(1_700_000_000_000, 201);

        write(
            &paths,
            ContentKind::Knowledge,
            knowledge,
            "k",
            "K",
            &knowledge_body(&[real]),
            "",
        );
        write(
            &paths,
            ContentKind::Article,
            article,
            "a",
            "A",
            &format!(
                ":::knowledgeref{{content=\"{}\" paragraph=\"{}\"}}\n:::",
                knowledge.to_text(),
                missing.as_stable_id().to_text()
            ),
            "wip: false\n",
        );

        let index = build(&paths).unwrap();
        assert!(!index.is_buildable(), "a broken reference must stop a build");
        let broken: Vec<&IndexIssue> = index
            .issues()
            .iter()
            .filter(|issue| issue.code == DiagnosticCode::BrokenReference.as_str())
            .collect();
        assert_eq!(broken.len(), 1, "{:?}", index.issues());
        assert!(broken[0].is_error());
        assert_eq!(broken[0].target.as_deref(), Some(knowledge.to_text().as_str()));
        assert_eq!(
            broken[0].paragraph_id.as_deref(),
            Some(missing.as_stable_id().to_text().as_str())
        );
    }

    #[test]
    fn two_documents_sharing_a_slug_in_one_kind_is_an_error() {
        // Two documents at the same public URL would make the output depend on iteration order.
        let (_dir, paths) = workspace();
        let first = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 10);
        let second = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 11);
        write(&paths, ContentKind::Page, first, "about", "One", "A.", "");
        write(&paths, ContentKind::Page, second, "about", "Two", "B.", "");

        let index = build(&paths).unwrap();
        assert!(!index.is_buildable());
        assert!(index
            .blocking_issues()
            .iter()
            .any(|issue| issue.message.contains("already used")));
    }

    #[test]
    fn the_same_slug_in_different_kinds_is_allowed() {
        // A page at /about/ and a knowledge entry at /docs/about/ do not collide.
        let (_dir, paths) = workspace();
        let page = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 12);
        let knowledge = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 13);
        write(&paths, ContentKind::Page, page, "about", "About", "A.", "");
        write(&paths, ContentKind::Knowledge, knowledge, "about", "About", "A.", "");

        let index = build(&paths).unwrap();
        assert!(index.is_buildable(), "{:?}", index.issues());
    }

    #[test]
    fn a_file_not_named_by_an_id_is_reported_rather_than_invisible() {
        let (_dir, paths) = workspace();
        let directory = paths.kind_dir(ContentKind::Article);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("hand-written.md"),
            "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: article\nslug: x\ntitle: X\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\nBody.\n",
        )
        .unwrap();

        let index = build(&paths).unwrap();
        assert!(index.entries().is_empty());
        assert!(!index.issues().is_empty());
        assert!(index.issues()[0].message.contains("not named by a stable id"));
    }

    #[test]
    fn a_name_and_frontmatter_id_mismatch_is_reported() {
        let (_dir, paths) = workspace();
        let name_id = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 14);
        let body_id = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 15);
        let directory = paths.kind_dir(ContentKind::Article);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            paths.document_path(ContentKind::Article, name_id),
            format!(
                "---\nid: {}\ntype: article\nslug: x\ntitle: X\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\n---\n\nBody.\n",
                body_id.to_text()
            ),
        )
        .unwrap();

        let index = build(&paths).unwrap();
        assert!(!index.is_buildable());
        assert!(index
            .issues()
            .iter()
            .any(|issue| issue.message.contains("frontmatter says")));
    }

    #[test]
    fn paragraph_counts_distinguish_identified_from_unidentified() {
        let (_dir, paths) = workspace();
        let knowledge = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 16);
        let identified = ParagraphId::from_parts(1_700_000_000_000, 300);
        let body = format!(
            "<!--p:{}-->\n\nIdentified one.\n\nIdentified two without a marker.\n",
            identified.as_stable_id().to_text()
        );
        write(&paths, ContentKind::Knowledge, knowledge, "k", "K", &body, "");

        let index = build(&paths).unwrap();
        let entry = index.entry(&knowledge.to_text()).unwrap();
        assert_eq!(entry.identified_paragraphs, 1);
        assert_eq!(entry.unidentified_paragraphs, 1);
    }

    #[test]
    fn a_knowledge_documents_paragraphs_are_available_in_order() {
        let (_dir, paths) = workspace();
        let knowledge = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 17);
        let first = ParagraphId::from_parts(1_700_000_000_000, 400);
        let second = ParagraphId::from_parts(1_700_000_000_000, 401);
        write(
            &paths,
            ContentKind::Knowledge,
            knowledge,
            "k",
            "K",
            &knowledge_body(&[first, second]),
            "",
        );

        let index = build(&paths).unwrap();
        let paragraphs = index.knowledge_paragraphs(&knowledge.to_text());
        assert_eq!(paragraphs.len(), 2);
        assert_eq!(paragraph_ordinal(&paragraphs, first), Some(1));
        assert_eq!(paragraph_ordinal(&paragraphs, second), Some(2));
        let absent = ParagraphId::from_parts(1_700_000_000_000, 999);
        assert_eq!(paragraph_ordinal(&paragraphs, absent), None);
    }

    #[test]
    fn entries_and_issues_round_trip_through_json() {
        let (_dir, paths) = workspace();
        let id = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 18);
        write(&paths, ContentKind::Article, id, "x", "X", "Body.", "wip: false\n");
        let index = build(&paths).unwrap();

        let entries = serde_json::to_string(index.entries()).unwrap();
        assert!(entries.contains("\"kind\":\"article\""));
        let issues = serde_json::to_string(index.issues()).unwrap();
        assert!(issues.starts_with('['));
    }

    #[test]
    fn an_issue_summary_names_the_severity_the_code_and_the_document() {
        let issue = IndexIssue::new(IssueSeverity::Error, "BROKEN_REFERENCE", "cannot resolve")
            .in_document("01ABC");
        let summary = issue.summary();
        assert!(summary.contains("error"));
        assert!(summary.contains("BROKEN_REFERENCE"));
        assert!(summary.contains("01ABC"));
        assert!(issue.is_blocking());
    }
}
