//! Knowledge references and the Harvard reference list (`handoff.md` §9).
//!
//! The behaviour here is locked, and the surprising part is what it *refuses* to do:
//!
//! * References are resolved at build time against the current knowledge content, so a knowledge
//!   edit flows into every article that cites it (§9.4).
//! * References are **paragraph-granular** and target a stable paragraph id, never a position or
//!   a text match (§9.2).
//! * A reference whose target does not exist **stays broken** and produces a diagnostic. Veloct
//!   never "helpfully" resolves it to some other paragraph (§9.3).
//! * A duplicate paragraph id is an error, not a silent last-one-wins.
//! * The article's reference list contains every cited knowledge entry, once each, sorted (§9.4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ast::ParagraphId;
use crate::diagnostics::{Diagnostic, DiagnosticCode, Diagnostics};
use crate::frontmatter::Frontmatter;

/// Default excerpt length before the ellipsis.
pub const DEFAULT_EXCERPT_CHARACTERS: usize = 80;

/// A reference extracted from an article's directives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRef {
    /// The referenced document's stable content id.
    pub content_id: String,
    /// The referenced paragraph's stable id.
    pub paragraph_id: ParagraphId,
    /// Where in the referencing document this appears, for repair tooling.
    pub location: String,
    /// The author's own commentary, if the directive carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl KnowledgeRef {
    /// Parse a `knowledgeref` directive's attributes.
    ///
    /// Returns `Ok(None)` when the directive is not a knowledge reference, and an error string
    /// when it is one but malformed, so the caller can report exactly what is wrong.
    pub fn from_directive(
        name: &str,
        attributes: &[(String, String)],
        location: impl Into<String>,
    ) -> std::result::Result<Option<Self>, String> {
        if name != "knowledgeref" {
            return Ok(None);
        }
        let lookup = |key: &str| {
            attributes
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let content_id = lookup("content")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "knowledgeref requires a non-empty `content` attribute".to_string())?;
        let paragraph = lookup("paragraph")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| "knowledgeref requires a non-empty `paragraph` attribute".to_string())?;
        let paragraph_id = paragraph
            .parse::<ParagraphId>()
            .map_err(|_| format!("`paragraph` is not a valid paragraph id: {paragraph:?}"))?;

        Ok(Some(Self {
            content_id: content_id.to_string(),
            paragraph_id,
            location: location.into(),
            note: lookup("note").map(str::to_string),
        }))
    }
}

/// One paragraph of a knowledge document, as the index sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeParagraph {
    pub paragraph_id: ParagraphId,
    /// 1-based position in the document.
    pub order: usize,
    /// Plain text, used for the card excerpt and for change detection.
    pub text: String,
    /// Content hash of the paragraph, so a cached excerpt can be invalidated.
    pub content_hash: String,
}

impl KnowledgeParagraph {
    pub fn new(paragraph_id: ParagraphId, order: usize, text: impl Into<String>) -> Self {
        let text = text.into();
        let content_hash = veloct_core::sha256_hex_str(&text);
        Self {
            paragraph_id,
            order,
            text,
            content_hash,
        }
    }

    /// The excerpt shown on a reference card, with the ellipsis `handoff.md` §9.1 requires.
    pub fn excerpt(&self, limit: usize) -> String {
        excerpt_with_ellipsis(&self.text, limit)
    }
}

/// Truncate text to `limit` characters and append an ellipsis when anything was dropped.
///
/// Truncation is by character, not by byte, so a CJK paragraph cannot be cut mid-character.
pub fn excerpt_with_ellipsis(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    let mut characters = trimmed.char_indices();
    match characters.nth(limit) {
        // Fewer than or exactly `limit` characters: nothing was dropped, so no ellipsis.
        None => trimmed.to_string(),
        Some((byte_index, _)) => {
            let mut out = String::with_capacity(byte_index + 3);
            out.push_str(&trimmed[..byte_index]);
            out.push('…');
            out
        }
    }
}

/// A knowledge document as the reference index needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeDocument {
    pub content_id: String,
    pub slug: String,
    pub title: String,
    pub author: Option<String>,
    pub year: Option<String>,
    pub url: Option<String>,
    pub paragraphs: Vec<KnowledgeParagraph>,
}

impl KnowledgeDocument {
    /// Build a knowledge entry from parsed frontmatter and paragraphs.
    pub fn from_frontmatter(frontmatter: &Frontmatter, paragraphs: Vec<KnowledgeParagraph>) -> Self {
        Self {
            content_id: frontmatter.id.to_text(),
            slug: frontmatter.slug.clone(),
            title: frontmatter.title.clone(),
            author: frontmatter.extra_str("author").map(str::to_string),
            year: frontmatter
                .extra_str("year")
                .map(str::to_string)
                .or_else(|| frontmatter.timestamps.created_at.get(..4).map(str::to_string)),
            url: frontmatter.extra_str("url").map(str::to_string),
            paragraphs,
        }
    }

    /// The citation text for the article reference list (`handoff.md` §9.4).
    ///
    /// Harvard style: `Author (Year) Title. Available at: URL`.
    pub fn citation(&self) -> Citation {
        Citation {
            content_id: self.content_id.clone(),
            author: self
                .author
                .clone()
                .unwrap_or_else(|| "Unknown".to_string()),
            year: self.year.clone().unwrap_or_else(|| "n.d.".to_string()),
            title: self.title.clone(),
            url: self.url.clone(),
        }
    }
}

/// One entry in an article's Harvard reference list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Citation {
    pub content_id: String,
    pub author: String,
    pub year: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Citation {
    /// Render one Harvard-style entry.
    pub fn to_markdown(&self) -> String {
        match &self.url {
            Some(url) if !url.is_empty() => format!(
                "{} ({}) *{}*. Available at: {}",
                self.author, self.year, self.title, url
            ),
            _ => format!("{} ({}) *{}*.", self.author, self.year, self.title),
        }
    }
}

/// The reference list generated at the end of an article.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceList {
    pub citations: Vec<Citation>,
}

impl ReferenceList {
    /// Build from a set of knowledge documents, de-duplicated and deterministically sorted.
    ///
    /// Sorting is what makes a rebuild comparable (`handoff.md` §14.9): the order of the
    /// references must not depend on the order the directives happened to appear in.
    pub fn build<'a, I>(documents: I) -> Self
    where
        I: IntoIterator<Item = &'a KnowledgeDocument>,
    {
        let mut seen: Vec<String> = Vec::new();
        let mut citations: Vec<Citation> = Vec::new();
        for document in documents {
            if seen.contains(&document.content_id) {
                continue;
            }
            seen.push(document.content_id.clone());
            citations.push(document.citation());
        }
        citations.sort();
        Self { citations }
    }

    pub fn is_empty(&self) -> bool {
        self.citations.is_empty()
    }

    /// Render as a Markdown section, or `None` when there is nothing to cite.
    pub fn to_markdown(&self) -> Option<String> {
        if self.citations.is_empty() {
            return None;
        }
        let mut out = String::from("## References\n\n");
        for citation in &self.citations {
            out.push_str("- ");
            out.push_str(&citation.to_markdown());
            out.push('\n');
        }
        Some(out)
    }
}

/// The outcome of resolving one reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ReferenceResolution {
    /// The target exists; the card can be rendered.
    Resolved {
        reference: KnowledgeRef,
        title: String,
        excerpt: String,
        /// Which citation this reference contributes to the article's list.
        citation: Citation,
    },
    /// The target does not exist. Kept broken on purpose (`handoff.md` §9.3).
    Broken {
        reference: KnowledgeRef,
        /// `UNKNOWN_CONTENT_ID` or `BROKEN_REFERENCE`.
        code: DiagnosticCode,
        message: String,
    },
}

impl ReferenceResolution {
    pub fn reference(&self) -> &KnowledgeRef {
        match self {
            Self::Resolved { reference, .. } | Self::Broken { reference, .. } => reference,
        }
    }

    pub fn is_resolved(&self) -> bool {
        matches!(self, Self::Resolved { .. })
    }
}

/// An index over all knowledge documents in a project.
#[derive(Debug, Clone, Default)]
pub struct KnowledgeIndex {
    documents: BTreeMap<String, KnowledgeDocument>,
}

impl KnowledgeIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_documents(documents: impl IntoIterator<Item = KnowledgeDocument>) -> Self {
        let mut index = Self::new();
        for document in documents {
            index.insert(document);
        }
        index
    }

    pub fn insert(&mut self, document: KnowledgeDocument) {
        self.documents.insert(document.content_id.clone(), document);
    }

    pub fn get(&self, content_id: &str) -> Option<&KnowledgeDocument> {
        self.documents.get(content_id)
    }

    pub fn contains(&self, content_id: &str) -> bool {
        self.documents.contains_key(content_id)
    }

    pub fn len(&self) -> usize {
        self.documents.len()
    }

    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    /// Resolve one reference against this index.
    pub fn resolve(&self, reference: &KnowledgeRef, excerpt_limit: usize) -> ReferenceResolution {
        let Some(document) = self.documents.get(&reference.content_id) else {
            return ReferenceResolution::Broken {
                reference: reference.clone(),
                code: DiagnosticCode::UnknownContentId,
                message: format!(
                    "no knowledge document has the id {}",
                    reference.content_id
                ),
            };
        };

        match document
            .paragraphs
            .iter()
            .find(|paragraph| paragraph.paragraph_id == reference.paragraph_id)
        {
            Some(paragraph) => ReferenceResolution::Resolved {
                reference: reference.clone(),
                title: document.title.clone(),
                excerpt: paragraph.excerpt(excerpt_limit),
                citation: document.citation(),
            },
            None => ReferenceResolution::Broken {
                reference: reference.clone(),
                code: DiagnosticCode::BrokenReference,
                message: format!(
                    "paragraph {} does not exist in knowledge document {} ({})",
                    reference.paragraph_id, document.title, reference.content_id
                ),
            },
        }
    }

    /// Resolve many references, reporting diagnostics for every broken one.
    pub fn resolve_all(
        &self,
        references: &[KnowledgeRef],
        excerpt_limit: usize,
    ) -> (Vec<ReferenceResolution>, Diagnostics) {
        let mut resolutions = Vec::with_capacity(references.len());
        let mut diagnostics = Diagnostics::new();

        for reference in references {
            let resolution = self.resolve(reference, excerpt_limit);
            if let ReferenceResolution::Broken {
                code, message, ..
            } = &resolution
            {
                diagnostics.push(
                    Diagnostic::new(*code, message.clone())
                        .with_content(reference.content_id.clone())
                        .with_paragraph(reference.paragraph_id)
                        .with_detail("location", reference.location.clone()),
                );
            }
            resolutions.push(resolution);
        }

        (resolutions, diagnostics)
    }

    /// The reference list an article should carry, from its own references.
    pub fn reference_list(&self, references: &[KnowledgeRef]) -> ReferenceList {
        let documents = references
            .iter()
            .filter_map(|reference| self.documents.get(&reference.content_id));
        ReferenceList::build(documents)
    }

    /// Content ids present, in stable order.
    pub fn content_ids(&self) -> Vec<&str> {
        self.documents.keys().map(String::as_str).collect()
    }
}

/// Detect duplicate paragraph ids inside one knowledge document (`handoff.md` §9.3).
pub fn check_duplicates(document: &KnowledgeDocument) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    let mut seen: Vec<ParagraphId> = Vec::new();
    for paragraph in &document.paragraphs {
        if seen.contains(&paragraph.paragraph_id) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::DuplicateParagraphId,
                    format!(
                        "paragraph id {} appears twice in knowledge document {}",
                        paragraph.paragraph_id, document.title
                    ),
                )
                .with_content(document.content_id.clone())
                .with_paragraph(paragraph.paragraph_id),
            );
        } else {
            seen.push(paragraph.paragraph_id);
        }
    }
    diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_body;

    fn knowledge(content_id: &str, title: &str, paragraphs: &[(&str, &str)]) -> KnowledgeDocument {
        KnowledgeDocument {
            content_id: content_id.to_string(),
            slug: title.to_lowercase().replace(' ', "-"),
            title: title.to_string(),
            author: Some("Vellow".to_string()),
            year: Some("2026".to_string()),
            url: Some(format!("https://example.com/docs/{content_id}")),
            paragraphs: paragraphs
                .iter()
                .enumerate()
                .map(|(index, (id, text))| {
                    KnowledgeParagraph::new(
                        id.parse::<ParagraphId>().unwrap(),
                        index + 1,
                        *text,
                    )
                })
                .collect(),
        }
    }

    fn pid(timestamp: u64, randomness: u128) -> ParagraphId {
        ParagraphId::from_parts(timestamp, randomness)
    }

    #[test]
    fn knowledgeref_attributes_are_parsed_strictly() {
        let reference = KnowledgeRef::from_directive(
            "knowledgeref",
            &[
                ("content".to_string(), "01ABC".to_string()),
                ("paragraph".to_string(), pid(1_700_000_000_000, 1).to_string()),
            ],
            "body:block:2",
        )
        .unwrap()
        .unwrap();
        assert_eq!(reference.content_id, "01ABC");
        assert_eq!(reference.paragraph_id, pid(1_700_000_000_000, 1));
        assert_eq!(reference.location, "body:block:2");

        // A different directive is simply not a reference.
        assert!(KnowledgeRef::from_directive("download", &[], "x").unwrap().is_none());

        // Malformed references report why, rather than yielding a reference with empty fields.
        let missing = KnowledgeRef::from_directive(
            "knowledgeref",
            &[("content".to_string(), "01ABC".to_string())],
            "x",
        )
        .unwrap_err();
        assert!(missing.contains("paragraph"), "{missing}");

        let bad_id = KnowledgeRef::from_directive(
            "knowledgeref",
            &[
                ("content".to_string(), "01ABC".to_string()),
                ("paragraph".to_string(), "not-a-ulid".to_string()),
            ],
            "x",
        )
        .unwrap_err();
        assert!(bad_id.contains("valid paragraph id"), "{bad_id}");
    }

    #[test]
    fn a_resolvable_reference_produces_a_card_with_an_excerpt() {
        let paragraph_id = pid(1_700_000_000_000, 2);
        let index = KnowledgeIndex::from_documents([knowledge(
            "01ABC",
            "Rust Ownership",
            &[(&paragraph_id.to_string(), "Ownership is Rust's central idea.")],
        )]);

        let reference = KnowledgeRef {
            content_id: "01ABC".to_string(),
            paragraph_id,
            location: "body:block:1".to_string(),
            note: None,
        };

        match index.resolve(&reference, DEFAULT_EXCERPT_CHARACTERS) {
            ReferenceResolution::Resolved {
                title, excerpt, ..
            } => {
                assert_eq!(title, "Rust Ownership");
                assert!(excerpt.starts_with("Ownership is Rust"));
            }
            other => panic!("expected a resolved reference, got {other:?}"),
        }
    }

    #[test]
    fn a_reference_to_a_missing_paragraph_stays_broken_and_never_guesses() {
        // handoff §9.3: the single most important rule in this module.
        let existing = pid(1_700_000_000_000, 3);
        let missing = pid(1_700_000_000_000, 4);
        let index = KnowledgeIndex::from_documents([knowledge(
            "01ABC",
            "Rust Ownership",
            &[(&existing.to_string(), "The only paragraph.")],
        )]);

        let reference = KnowledgeRef {
            content_id: "01ABC".to_string(),
            paragraph_id: missing,
            location: "body:block:9".to_string(),
            note: None,
        };

        match index.resolve(&reference, DEFAULT_EXCERPT_CHARACTERS) {
            ReferenceResolution::Broken { code, message, .. } => {
                assert_eq!(code, DiagnosticCode::BrokenReference);
                assert!(message.contains(&missing.to_string()));
                assert!(message.contains("Rust Ownership"));
            }
            other => panic!("a missing paragraph must stay broken, got {other:?}"),
        }
    }

    #[test]
    fn a_reference_to_an_unknown_document_reports_the_unknown_content_id() {
        let index = KnowledgeIndex::new();
        let reference = KnowledgeRef {
            content_id: "01ZZZ".to_string(),
            paragraph_id: pid(1_700_000_000_000, 5),
            location: "body:block:1".to_string(),
            note: None,
        };
        match index.resolve(&reference, 80) {
            ReferenceResolution::Broken { code, message, .. } => {
                assert_eq!(code, DiagnosticCode::UnknownContentId);
                assert!(message.contains("01ZZZ"));
            }
            other => panic!("expected a broken reference, got {other:?}"),
        }
    }

    #[test]
    fn resolve_all_reports_every_broken_reference() {
        let good = pid(1_700_000_000_000, 6);
        let index = KnowledgeIndex::from_documents([knowledge(
            "01ABC",
            "Known",
            &[(&good.to_string(), "text")],
        )]);

        let references = vec![
            KnowledgeRef {
                content_id: "01ABC".to_string(),
                paragraph_id: good,
                location: "body:block:1".to_string(),
                note: None,
            },
            KnowledgeRef {
                content_id: "01ABC".to_string(),
                paragraph_id: pid(1_700_000_000_000, 7),
                location: "body:block:2".to_string(),
                note: None,
            },
            KnowledgeRef {
                content_id: "01NOPE".to_string(),
                paragraph_id: pid(1_700_000_000_000, 8),
                location: "body:block:3".to_string(),
                note: None,
            },
        ];

        let (resolutions, diagnostics) = index.resolve_all(&references, 80);
        assert_eq!(resolutions.len(), 3);
        assert!(resolutions[0].is_resolved());
        assert!(!resolutions[1].is_resolved());
        assert!(!resolutions[2].is_resolved());
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.has_errors());
        // The location is carried through so the editor can offer a repair.
        assert_eq!(
            diagnostics.items[0].detail("location"),
            Some("body:block:2")
        );
    }

    #[test]
    fn the_reference_list_contains_each_cited_entry_once_and_is_sorted() {
        // handoff §9.4.
        let index = KnowledgeIndex::from_documents([
            knowledge("01B", "Beta", &[]),
            knowledge("01A", "Alpha", &[]),
        ]);
        let references = vec![
            KnowledgeRef {
                content_id: "01B".to_string(),
                paragraph_id: pid(1_700_000_000_000, 9),
                location: "a".to_string(),
                note: None,
            },
            KnowledgeRef {
                content_id: "01A".to_string(),
                paragraph_id: pid(1_700_000_000_000, 10),
                location: "b".to_string(),
                note: None,
            },
            // A second citation of the same document must not duplicate the entry.
            KnowledgeRef {
                content_id: "01B".to_string(),
                paragraph_id: pid(1_700_000_000_000, 11),
                location: "c".to_string(),
                note: None,
            },
        ];

        let list = index.reference_list(&references);
        assert_eq!(list.citations.len(), 2);
        // Sorted by content id, so the order does not depend on directive order.
        assert_eq!(list.citations[0].content_id, "01A");
        assert_eq!(list.citations[1].content_id, "01B");

        let markdown = list.to_markdown().unwrap();
        assert!(markdown.starts_with("## References\n\n"));
        assert!(markdown.contains("Vellow (2026) *Alpha*. Available at: https://example.com/docs/01A"));
    }

    #[test]
    fn an_article_with_no_references_has_no_reference_section() {
        let list = ReferenceList::build(std::iter::empty());
        assert!(list.is_empty());
        assert!(list.to_markdown().is_none());
    }

    #[test]
    fn duplicate_paragraph_ids_in_one_document_are_errors() {
        let duplicate = pid(1_700_000_000_000, 12);
        let document = knowledge(
            "01ABC",
            "Dupes",
            &[(&duplicate.to_string(), "one"), (&duplicate.to_string(), "two")],
        );
        let diagnostics = check_duplicates(&document);
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics.has_errors());
        assert_eq!(
            diagnostics.items[0].code,
            DiagnosticCode::DuplicateParagraphId
        );
    }

    #[test]
    fn excerpts_are_truncated_by_character_with_an_ellipsis() {
        assert_eq!(excerpt_with_ellipsis("short", 80), "short");
        assert_eq!(excerpt_with_ellipsis("abcdefghij", 5), "abcde…");
        // CJK must not be cut mid-character.
        let excerpt = excerpt_with_ellipsis("你好世界再见", 3);
        assert_eq!(excerpt, "你好世…");
        assert_eq!(excerpt.chars().count(), 4);
    }

    #[test]
    fn a_citation_without_a_url_still_renders() {
        let citation = Citation {
            content_id: "01A".to_string(),
            author: "Someone".to_string(),
            year: "n.d.".to_string(),
            title: "Untitled".to_string(),
            url: None,
        };
        assert_eq!(citation.to_markdown(), "Someone (n.d.) *Untitled*.");
    }

    #[test]
    fn paragraph_indexes_and_hashes_are_recorded_for_repair_tooling() {
        // handoff §22.5: the index needs order and a revision marker.
        let paragraph = KnowledgeParagraph::new(pid(1_700_000_000_000, 13), 3, "some text");
        assert_eq!(paragraph.order, 3);
        assert_eq!(paragraph.content_hash, veloct_core::sha256_hex_str("some text"));
        assert_ne!(
            paragraph.content_hash,
            KnowledgeParagraph::new(pid(1_700_000_000_000, 13), 3, "other text").content_hash
        );
    }

    #[test]
    fn a_knowledge_document_can_be_built_from_parsed_frontmatter() {
        let source = r#"---
id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY
type: knowledge
slug: ownership
title: Rust Ownership
created_at: 2024-05-01T00:00:00Z
updated_at: 2024-05-01T00:00:00Z
author: Vellow
---

<!--p:01J8Z5Q0K3W7M9V2B4N6P8R1TZ-->

Ownership text.
"#;
        let document = crate::parse::parse(source).unwrap();
        let paragraphs: Vec<KnowledgeParagraph> = document
            .ast
            .paragraphs()
            .into_iter()
            .enumerate()
            .filter_map(|(index, (id, content, _))| {
                id.map(|id| {
                    KnowledgeParagraph::new(id, index + 1, crate::ast::plain_text(&content))
                })
            })
            .collect();
        assert_eq!(paragraphs.len(), 1);

        let entry = KnowledgeDocument::from_frontmatter(&document.frontmatter, paragraphs);
        assert_eq!(entry.title, "Rust Ownership");
        assert_eq!(entry.author.as_deref(), Some("Vellow"));
        // The year falls back to the creation timestamp.
        assert_eq!(entry.year.as_deref(), Some("2024"));
    }

    #[test]
    fn a_paragraph_without_an_id_is_skipped_rather_than_given_a_made_up_one() {
        let parsed = parse_body("A paragraph with no marker.\n");
        let ids: Vec<_> = parsed
            .ast
            .paragraphs()
            .into_iter()
            .filter_map(|(id, _, _)| id)
            .collect();
        assert!(ids.is_empty());
    }

    #[test]
    fn the_index_is_deterministic_in_its_content_id_order() {
        let index = KnowledgeIndex::from_documents([
            knowledge("01C", "C", &[]),
            knowledge("01A", "A", &[]),
            knowledge("01B", "B", &[]),
        ]);
        assert_eq!(index.content_ids(), vec!["01A", "01B", "01C"]);
        assert_eq!(index.len(), 3);
        assert!(index.contains("01B"));
        assert!(!index.is_empty());
    }
}
