//! Context assembly.
//!
//! `handoff.md` §11.7 defines exactly what may be sent, and what may not:
//!
//! ```text
//! default:   temporary user prompt + current Markdown
//! selection: temporary user prompt + the selected content
//! plus:      other content the user explicitly chose to include
//! ```
//!
//! and then: **不得默认把整个网站发送给 AI** — the whole site must never be sent by default.
//!
//! So the bundle is assembled from explicitly named sources, `ContextLimits` caps it, and a guard
//! reports what was dropped. A silent truncation would produce an answer that ignores part of the
//! document without saying so, which is the kind of quiet wrongness `handoff.md` rules out.

use serde::{Deserialize, Serialize};

/// Where one piece of context came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    /// The document being edited. Always included unless a selection replaces it.
    CurrentDocument,
    /// The user's selected range.
    Selection,
    /// Another article the user chose.
    Article,
    /// A page the user chose.
    Page,
    /// A knowledge entry the user chose.
    Knowledge,
    /// A work's metadata the user chose.
    Work,
}

impl ContextSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CurrentDocument => "current_document",
            Self::Selection => "selection",
            Self::Article => "article",
            Self::Page => "page",
            Self::Knowledge => "knowledge",
            Self::Work => "work",
        }
    }

    /// Whether this source is only present because the user asked for it.
    ///
    /// `handoff.md` §11.7: the current document and the selection are implicit; everything else must
    /// be chosen. The distinction is what makes "we never send the whole site" checkable.
    pub fn requires_explicit_choice(self) -> bool {
        !matches!(self, Self::CurrentDocument | Self::Selection)
    }
}

/// One piece of context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextItem {
    pub source: ContextSource,
    /// A label for the model and for the UI, e.g. the document title.
    pub label: String,
    /// A stable id when the item names a document, so the caller can trace what was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    pub text: String,
}

/// Caps on what is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextLimits {
    /// Maximum characters of the current document or selection.
    pub primary_characters: usize,
    /// Maximum characters of extra context, in total.
    pub extra_characters: usize,
    /// Maximum number of extra items.
    pub extra_items: usize,
}

impl Default for ContextLimits {
    fn default() -> Self {
        // Roughly a few thousand tokens each, which keeps an ordinary article whole while refusing
        // to send an entire knowledge base by accident.
        Self {
            primary_characters: 120_000,
            extra_characters: 60_000,
            extra_items: 12,
        }
    }
}

/// What was assembled, and what was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBundle {
    pub items: Vec<ContextItem>,
    /// Extra items that did not fit, named so the UI can say what was dropped.
    pub dropped: Vec<String>,
    /// Whether any primary content was truncated.
    pub primary_truncated: bool,
    /// Whether any extra content was truncated.
    pub extra_truncated: bool,
}

impl ContextBundle {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The assembled text, ready to be placed in a prompt, in a stable order.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for item in &self.items {
            out.push_str(&format!(
                "<context source=\"{}\" label=\"{}\">\n",
                item.source.as_str(),
                item.label
            ));
            out.push_str(&item.text);
            out.push_str("\n</context>\n\n");
        }
        out
    }

    /// Total characters assembled.
    pub fn characters(&self) -> usize {
        self.items.iter().map(|item| item.text.len()).sum()
    }

    /// Sources present, sorted, so a test can assert exactly what would be sent.
    pub fn sources(&self) -> Vec<ContextSource> {
        let mut sources: Vec<ContextSource> = self.items.iter().map(|item| item.source).collect();
        sources.sort();
        sources.dedup();
        sources
    }

    /// Whether an extra source is present, regardless of where it sits in the order.
    pub fn contains_source(&self, source: ContextSource) -> bool {
        self.items.iter().any(|item| item.source == source)
    }

    /// Whether the bundle includes anything the user did not implicitly authorise.
    ///
    /// A bundle that only carries the current document and the selection satisfies
    /// `handoff.md` §11.7's default with no explicit choice.
    pub fn needs_no_explicit_choice(&self) -> bool {
        self.items
            .iter()
            .all(|item| !item.source.requires_explicit_choice())
    }
}

/// Assembles context bundles.
#[derive(Debug, Clone)]
pub struct ContextAssembly {
    limits: ContextLimits,
}

impl ContextAssembly {
    pub fn new(limits: ContextLimits) -> Self {
        Self { limits }
    }

    pub fn with_defaults() -> Self {
        Self::new(ContextLimits::default())
    }

    /// Assemble the bundle.
    ///
    /// `selection` replaces the current document when present, which is what `handoff.md` §11.7
    /// describes: with a selection the context is the prompt plus the selection, not the whole file.
    pub fn assemble(
        &self,
        prompt: &str,
        document: Option<(&str, &str, Option<String>)>,
        selection: Option<&str>,
        extra: Vec<ContextItem>,
    ) -> ContextBundle {
        let mut items = Vec::new();
        let mut dropped = Vec::new();
        let mut primary_truncated = false;
        let mut extra_truncated = false;

        // The user's own instruction always comes first, so it reads as the task rather than as
        // another piece of data.
        items.push(ContextItem {
            source: ContextSource::CurrentDocument,
            label: "user prompt".to_string(),
            content_id: None,
            text: prompt.to_string(),
        });

        if let Some(selection) = selection {
            let (text, truncated) = truncate(selection, self.limits.primary_characters);
            primary_truncated |= truncated;
            items.push(ContextItem {
                source: ContextSource::Selection,
                label: "selected text".to_string(),
                content_id: None,
                text: text.to_string(),
            });
        } else if let Some((label, text, content_id)) = document {
            let (body, truncated) = truncate(text, self.limits.primary_characters);
            primary_truncated |= truncated;
            items.push(ContextItem {
                source: ContextSource::CurrentDocument,
                label: label.to_string(),
                content_id,
                text: body.to_string(),
            });
        }

        let mut used = 0usize;
        for (index, item) in extra.into_iter().enumerate() {
            if index >= self.limits.extra_items {
                dropped.push(item.label);
                continue;
            }
            // A source that requires an explicit choice is only honoured when it was actually
            // supplied, which it was here; the guard is that nothing else adds items implicitly.
            let remaining = self.limits.extra_characters.saturating_sub(used);
            if remaining == 0 {
                dropped.push(item.label);
                continue;
            }
            let (text, truncated) = truncate(&item.text, remaining);
            extra_truncated |= truncated;
            used += text.len();
            items.push(ContextItem { text: text.to_string(), ..item });
        }

        ContextBundle {
            items,
            dropped,
            primary_truncated,
            extra_truncated,
        }
    }
}

/// Truncate on a character boundary, reporting whether anything was cut.
fn truncate(text: &str, limit: usize) -> (&str, bool) {
    if text.len() <= limit {
        return (text, false);
    }
    // `floor_char_boundary` is not stable across the supported toolchain range, so the cut point is
    // found by walking down from the limit.
    let mut end = limit;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extra(source: ContextSource, label: &str, text: &str) -> ContextItem {
        ContextItem {
            source,
            label: label.to_string(),
            content_id: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn the_default_bundle_is_the_prompt_plus_the_document() {
        // handoff.md section 11.7, first case.
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble(
            "summarise this",
            Some(("My Article", "the body", Some("01ABC".to_string()))),
            None,
            Vec::new(),
        );

        assert_eq!(bundle.items.len(), 2);
        assert_eq!(bundle.items[0].text, "summarise this");
        assert_eq!(bundle.items[1].source, ContextSource::CurrentDocument);
        assert_eq!(bundle.items[1].label, "My Article");
        assert_eq!(bundle.items[1].content_id.as_deref(), Some("01ABC"));
        assert!(bundle.needs_no_explicit_choice());
        assert!(bundle.dropped.is_empty());
    }

    #[test]
    fn a_selection_replaces_the_document_rather_than_adding_to_it() {
        // handoff.md section 11.7, second case: prompt plus selection.
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble(
            "polish this",
            Some(("My Article", "the whole body", None)),
            Some("just this paragraph"),
            Vec::new(),
        );

        assert_eq!(bundle.sources(), vec![ContextSource::CurrentDocument, ContextSource::Selection]);
        assert!(
            !bundle.items.iter().any(|item| item.text == "the whole body"),
            "the whole document must not be sent when a selection is given"
        );
        assert!(bundle.needs_no_explicit_choice());
    }

    #[test]
    fn extra_context_is_included_in_a_stable_order() {
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble(
            "prompt",
            Some(("Doc", "body", None)),
            None,
            vec![
                extra(ContextSource::Knowledge, "Rust Ownership", "knowledge text"),
                extra(ContextSource::Article, "Other Article", "article text"),
            ],
        );

        let labels: Vec<&str> = bundle.items.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(
            labels,
            vec!["user prompt", "Doc", "Rust Ownership", "Other Article"],
            "the caller's order is preserved"
        );
        assert!(!bundle.needs_no_explicit_choice(), "extras were explicitly chosen");
    }

    #[test]
    fn the_sources_are_reported_so_a_caller_can_confirm_what_was_sent() {
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble(
            "p",
            Some(("Doc", "b", None)),
            None,
            vec![
                extra(ContextSource::Knowledge, "k", "x"),
                extra(ContextSource::Page, "pg", "y"),
            ],
        );
        assert_eq!(
            bundle.sources(),
            // Sorted by the enum's own order (article, page, knowledge, work), so the result is a
            // property of the type rather than of how the caller happened to pass the items.
            vec![
                ContextSource::CurrentDocument,
                ContextSource::Page,
                ContextSource::Knowledge
            ]
        );
        assert!(bundle.contains_source(ContextSource::Knowledge));
        assert!(bundle.contains_source(ContextSource::Page));
        assert!(!bundle.contains_source(ContextSource::Work));
    }

    #[test]
    fn an_oversized_document_is_truncated_and_says_so() {
        // A silent truncation would produce an answer that ignores part of the document.
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 10,
            extra_characters: 100,
            extra_items: 5,
        });
        let bundle = assembly.assemble(
            "p",
            Some(("Doc", "0123456789ABCDEF", None)),
            None,
            Vec::new(),
        );

        assert!(bundle.primary_truncated);
        assert_eq!(bundle.items[1].text.len(), 10);
    }

    #[test]
    fn truncation_respects_character_boundaries() {
        // Cutting a multi-byte character in half would panic or corrupt the text.
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 5,
            extra_characters: 100,
            extra_items: 5,
        });
        // Each of these is three bytes, so a byte-limit of 5 must cut after the first character.
        let bundle = assembly.assemble("p", Some(("Doc", "你好世界", None)), None, Vec::new());
        assert!(bundle.primary_truncated);
        assert_eq!(bundle.items[1].text, "你");
    }

    #[test]
    fn too_many_extras_are_dropped_and_named() {
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 1000,
            extra_characters: 1000,
            extra_items: 2,
        });
        let bundle = assembly.assemble(
            "p",
            None,
            None,
            vec![
                extra(ContextSource::Article, "one", "a"),
                extra(ContextSource::Article, "two", "b"),
                extra(ContextSource::Article, "three", "c"),
            ],
        );

        assert_eq!(bundle.dropped, vec!["three"]);
        assert!(!bundle.items.iter().any(|item| item.label == "three"));
    }

    #[test]
    fn the_extra_budget_is_shared_across_items() {
        // ASCII, so the byte budget is consumed exactly.
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 1000,
            extra_characters: 10,
            extra_items: 5,
        });
        let bundle = assembly.assemble(
            "p",
            None,
            None,
            vec![
                extra(ContextSource::Article, "one", "0123456789"),
                extra(ContextSource::Article, "two", "abcdefghij"),
            ],
        );

        // The first item fits exactly, so nothing was cut.
        assert!(!bundle.extra_truncated);
        // The first item consumes the whole budget, so the second is dropped rather than sent
        // partially, and it is named so the UI can say what was left out.
        assert_eq!(
            bundle.items.iter().filter(|i| i.source == ContextSource::Article).count(),
            1
        );
        assert_eq!(bundle.dropped, vec!["two"]);
    }

    #[test]
    fn an_oversized_extra_is_truncated_and_says_so() {
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 1000,
            extra_characters: 10,
            extra_items: 5,
        });
        let bundle = assembly.assemble(
            "p",
            None,
            None,
            vec![extra(ContextSource::Article, "one", "0123456789ABCDEF")],
        );

        assert!(bundle.extra_truncated);
        assert_eq!(
            bundle
                .items
                .iter()
                .find(|item| item.source == ContextSource::Article)
                .unwrap()
                .text
                .len(),
            10
        );
        assert!(bundle.dropped.is_empty(), "a truncated item is still sent");
    }

    #[test]
    fn a_multibyte_extra_does_not_overrun_the_budget() {
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 1000,
            extra_characters: 10,
            extra_items: 5,
        });
        let bundle = assembly.assemble(
            "p",
            None,
            None,
            vec![
                // Three bytes each, so only one character fits in a ten-byte budget. The point of
                // this case is that the character boundary is respected and the reported total
                // stays inside the limit.
                extra(ContextSource::Article, "one", "你好世界"),
                extra(ContextSource::Article, "two", "abcdefghij"),
            ],
        );

        assert!(bundle.extra_truncated);
        let extra_characters: usize = bundle
            .items
            .iter()
            .filter(|item| item.source.requires_explicit_choice())
            .map(|item| item.text.len())
            .sum();
        assert!(
            extra_characters <= 10,
            "the extra budget must not be exceeded, got {extra_characters}"
        );
    }

    #[test]
    fn extras_beyond_the_item_cap_are_dropped_and_named() {
        let assembly = ContextAssembly::new(ContextLimits {
            primary_characters: 1000,
            extra_characters: 1000,
            extra_items: 1,
        });
        let bundle = assembly.assemble(
            "p",
            None,
            None,
            vec![
                extra(ContextSource::Article, "kept", "a"),
                extra(ContextSource::Article, "dropped", "b"),
            ],
        );
        assert_eq!(bundle.dropped, vec!["dropped"]);
        assert!(bundle.contains_source(ContextSource::Article));
    }

    #[test]
    fn only_a_selection_or_the_document_counts_as_implicit() {
        assert!(!ContextSource::CurrentDocument.requires_explicit_choice());
        assert!(!ContextSource::Selection.requires_explicit_choice());
        for source in [
            ContextSource::Article,
            ContextSource::Page,
            ContextSource::Knowledge,
            ContextSource::Work,
        ] {
            assert!(
                source.requires_explicit_choice(),
                "{source:?} must require an explicit choice"
            );
        }
    }

    #[test]
    fn an_empty_bundle_is_possible_and_harmless() {
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble("", None, None, Vec::new());
        assert!(!bundle.is_empty(), "the prompt item is always present");
        assert_eq!(bundle.characters(), 0);
    }

    #[test]
    fn rendering_marks_each_source_so_the_model_can_tell_them_apart() {
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble(
            "p",
            Some(("Doc", "body text", None)),
            None,
            vec![extra(ContextSource::Knowledge, "K", "knowledge text")],
        );
        let rendered = bundle.render();
        assert!(rendered.contains("source=\"current_document\""), "{rendered}");
        assert!(rendered.contains("source=\"knowledge\""), "{rendered}");
        assert!(rendered.contains("label=\"Doc\""), "{rendered}");
        assert!(rendered.contains("body text"));
    }

    #[test]
    fn a_bundle_round_trips_through_json() {
        let assembly = ContextAssembly::with_defaults();
        let bundle = assembly.assemble("p", Some(("D", "b", None)), None, Vec::new());
        let json = serde_json::to_string(&bundle).unwrap();
        assert_eq!(serde_json::from_str::<ContextBundle>(&json).unwrap(), bundle);
    }
}
