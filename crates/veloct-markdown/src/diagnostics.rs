//! Structured diagnostics.
//!
//! `handoff.md` §9.3 is explicit that a conflict must produce a clear warning or error and must
//! never be resolved by guessing, and §0 rule 9 forbids silent failure generally. So this crate
//! never "fixes" a broken reference: it reports it, keeps it broken, and lets the user repair it.

use serde::{Deserialize, Serialize};

use crate::ast::{ParagraphId, SourceSpan};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational; the build proceeds.
    Info,
    /// The build proceeds but something needs attention.
    Warning,
    /// The build must fail: the document cannot be emitted meaningfully.
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// The conditions this crate can report.
///
/// A closed enum keeps the mapping from condition to severity in one place, so a build can ask
/// "are there any errors?" without string matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiagnosticCode {
    /// Two paragraphs in one document carry the same id (`handoff.md` §9.3).
    DuplicateParagraphId,
    /// A reference names a knowledge document that does not exist.
    UnknownContentId,
    /// A reference names a paragraph that does not exist in the target knowledge document.
    BrokenReference,
    /// A paragraph-marker comment is not followed by any paragraph.
    OrphanParagraphMarker,
    /// A paragraph has no id, so it cannot be referenced yet.
    UnidentifiedParagraph,
    /// The document has no frontmatter block at all.
    MissingFrontmatter,
    /// The frontmatter block is present but does not parse.
    InvalidFrontmatter,
    /// A frontmatter field violates the contract in `docs/spec/01-data-model.md` §2.
    InvalidFrontmatterField,
    /// A directive line could not be parsed.
    MalformedDirective,
    /// A directive uses a name Veloct does not know.
    UnknownDirective,
    /// A directive is missing an attribute it requires.
    MissingDirectiveAttribute,
    /// A paragraph id was dropped by an external edit (`handoff.md` §9.3).
    ParagraphIdLost,
}

impl DiagnosticCode {
    pub fn severity(self) -> Severity {
        match self {
            Self::DuplicateParagraphId
            | Self::UnknownContentId
            | Self::BrokenReference
            | Self::InvalidFrontmatter
            | Self::InvalidFrontmatterField
            | Self::MalformedDirective => Severity::Error,

            Self::OrphanParagraphMarker
            | Self::UnidentifiedParagraph
            | Self::MissingFrontmatter
            | Self::UnknownDirective
            | Self::MissingDirectiveAttribute
            | Self::ParagraphIdLost => Severity::Warning,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateParagraphId => "DUPLICATE_PARAGRAPH_ID",
            Self::UnknownContentId => "UNKNOWN_CONTENT_ID",
            Self::BrokenReference => "BROKEN_REFERENCE",
            Self::OrphanParagraphMarker => "ORPHAN_PARAGRAPH_MARKER",
            Self::UnidentifiedParagraph => "UNIDENTIFIED_PARAGRAPH",
            Self::MissingFrontmatter => "MISSING_FRONTMATTER",
            Self::InvalidFrontmatter => "INVALID_FRONTMATTER",
            Self::InvalidFrontmatterField => "INVALID_FRONTMATTER_FIELD",
            Self::MalformedDirective => "MALFORMED_DIRECTIVE",
            Self::UnknownDirective => "UNKNOWN_DIRECTIVE",
            Self::MissingDirectiveAttribute => "MISSING_DIRECTIVE_ATTRIBUTE",
            Self::ParagraphIdLost => "PARAGRAPH_ID_LOST",
        }
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reported condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    /// The document this was found in, as a project-relative path when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paragraph_id: Option<ParagraphId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
    /// Machine-readable extras, for example the missing attribute name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<(String, String)>,
}

impl Diagnostic {
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: code.severity(),
            message: message.into(),
            content_id: None,
            paragraph_id: None,
            span: None,
            details: Vec::new(),
        }
    }

    pub fn with_content(mut self, content_id: impl Into<String>) -> Self {
        self.content_id = Some(content_id.into());
        self
    }

    pub fn with_paragraph(mut self, paragraph_id: ParagraphId) -> Self {
        self.paragraph_id = Some(paragraph_id);
        self
    }

    pub fn with_span(mut self, span: SourceSpan) -> Self {
        self.span = Some(span);
        self
    }

    pub fn with_line(mut self, line: usize) -> Self {
        self.span = Some(SourceSpan::new(line, line));
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.push((key.into(), value.into()));
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn detail(&self, key: &str) -> Option<&str> {
        self.details
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.severity.as_str(), self.message)
    }
}

/// Collects diagnostics and answers the questions a build needs to ask.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.items.push(diagnostic);
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when at least one item is an error, which must stop a build (`handoff.md` §9.3).
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(Diagnostic::is_error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(|d| d.is_error())
    }

    pub fn warnings(&self) -> impl Iterator<Item = &Diagnostic> {
        self.items
            .iter()
            .filter(|d| d.severity == Severity::Warning)
    }

    /// Items of one kind.
    pub fn of_code(&self, code: DiagnosticCode) -> impl Iterator<Item = &Diagnostic> {
        self.items.iter().filter(move |d| d.code == code)
    }

    /// Append another collection, keeping this one's order.
    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }

    /// Sort by severity then code, so a report is stable across runs (handoff §14.9).
    pub fn sorted(&self) -> Vec<Diagnostic> {
        let mut items = self.items.clone();
        items.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.code.cmp(&b.code))
                .then_with(|| a.message.cmp(&b.message))
        });
        items
    }

    /// A compact human-readable report.
    pub fn summary(&self) -> String {
        if self.items.is_empty() {
            return "no diagnostics".to_string();
        }
        let errors = self.items.iter().filter(|d| d.is_error()).count();
        let warnings = self.items.iter().filter(|d| d.severity == Severity::Warning).count();
        format!(
            "{errors} error(s), {warnings} warning(s), {} total",
            self.items.len()
        )
    }
}

impl FromIterator<Diagnostic> for Diagnostics {
    fn from_iter<T: IntoIterator<Item = Diagnostic>>(iter: T) -> Self {
        Self {
            items: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ParagraphId {
        ParagraphId::from_parts(1_700_000_000_000, 3)
    }

    #[test]
    fn severity_defaults_match_the_handoff_rules() {
        // handoff §9.3: a duplicate id, an unknown target and a broken reference are errors.
        assert_eq!(
            DiagnosticCode::DuplicateParagraphId.severity(),
            Severity::Error
        );
        assert_eq!(DiagnosticCode::UnknownContentId.severity(), Severity::Error);
        assert_eq!(DiagnosticCode::BrokenReference.severity(), Severity::Error);
        // A paragraph that simply lacks an id yet is only a warning.
        assert_eq!(
            DiagnosticCode::UnidentifiedParagraph.severity(),
            Severity::Warning
        );
    }

    #[test]
    fn code_strings_are_stable_and_displayable() {
        assert_eq!(DiagnosticCode::BrokenReference.as_str(), "BROKEN_REFERENCE");
        assert_eq!(
            DiagnosticCode::DuplicateParagraphId.to_string(),
            "DUPLICATE_PARAGRAPH_ID"
        );
    }

    #[test]
    fn has_errors_ignores_warnings() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::UnidentifiedParagraph,
            "paragraph 3 has no id",
        ));
        assert!(!diagnostics.has_errors());
        assert_eq!(diagnostics.warnings().count(), 1);

        diagnostics.push(Diagnostic::new(
            DiagnosticCode::BrokenReference,
            "reference target missing",
        ));
        assert!(diagnostics.has_errors());
        assert_eq!(diagnostics.errors().count(), 1);
    }

    #[test]
    fn diagnostics_carry_their_location_and_details() {
        let diagnostic = Diagnostic::new(
            DiagnosticCode::MissingDirectiveAttribute,
            "knowledgeref requires a paragraph attribute",
        )
        .with_content("01ABC")
        .with_paragraph(id())
        .with_span(SourceSpan::new(12, 12))
        .with_detail("attribute", "paragraph");

        assert_eq!(diagnostic.content_id.as_deref(), Some("01ABC"));
        assert_eq!(diagnostic.paragraph_id, Some(id()));
        assert_eq!(diagnostic.detail("attribute"), Some("paragraph"));
        assert_eq!(diagnostic.span.unwrap().start_line, 12);
        assert!(diagnostic.to_string().starts_with("[warning]"));
    }

    #[test]
    fn sorted_orders_errors_first_and_is_deterministic() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::UnidentifiedParagraph,
            "b warning",
        ));
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::BrokenReference,
            "an error",
        ));
        diagnostics.push(Diagnostic::new(
            DiagnosticCode::DuplicateParagraphId,
            "another error",
        ));

        let sorted = diagnostics.sorted();
        assert!(sorted[0].is_error());
        assert!(sorted[1].is_error());
        assert!(!sorted[2].is_error());
        // Same collection, same order, every time.
        assert_eq!(sorted, diagnostics.sorted());
        assert_eq!(diagnostics.items[0].message, "b warning", "the original order is untouched");
    }

    #[test]
    fn summary_reports_counts() {
        let mut diagnostics = Diagnostics::new();
        assert_eq!(diagnostics.summary(), "no diagnostics");
        diagnostics.push(Diagnostic::new(DiagnosticCode::BrokenReference, "x"));
        diagnostics.push(Diagnostic::new(DiagnosticCode::UnidentifiedParagraph, "y"));
        assert_eq!(diagnostics.summary(), "1 error(s), 1 warning(s), 2 total");
    }

    #[test]
    fn of_code_filters_by_kind() {
        let mut diagnostics = Diagnostics::new();
        diagnostics.push(Diagnostic::new(DiagnosticCode::BrokenReference, "one"));
        diagnostics.push(Diagnostic::new(DiagnosticCode::BrokenReference, "two"));
        diagnostics.push(Diagnostic::new(DiagnosticCode::UnidentifiedParagraph, "three"));
        assert_eq!(diagnostics.of_code(DiagnosticCode::BrokenReference).count(), 2);
        assert_eq!(diagnostics.of_code(DiagnosticCode::UnknownContentId).count(), 0);
    }

    #[test]
    fn diagnostics_round_trip_through_json() {
        let diagnostics: Diagnostics = vec![Diagnostic::new(
            DiagnosticCode::ParagraphIdLost,
            "id disappeared",
        )
        .with_line(4)]
        .into_iter()
        .collect();
        let json = serde_json::to_string(&diagnostics).unwrap();
        assert_eq!(serde_json::from_str::<Diagnostics>(&json).unwrap(), diagnostics);
        assert!(json.contains("\"code\":\"PARAGRAPH_ID_LOST\""));
    }
}
