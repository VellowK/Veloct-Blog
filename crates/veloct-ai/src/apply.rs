//! Applying an AI result.
//!
//! `handoff.md` §11.9 and §25 C6/C7 define two rules that are easy to violate accidentally:
//!
//! * AI output **never** rewrites Markdown before the user confirms it. Producing a result is one
//!   step; applying it is a separate, explicit step.
//! * A partial apply touches **only** the selected range. An implementation that hands the model the
//!   whole document and then writes the whole answer back would satisfy neither rule while looking
//!   like it worked.
//!
//! So an [`ApplyPlan`] is computed and returned for review, and only [`ApplyPlan::apply`] mutates
//! text. The plan records the exact byte range it will replace, and a test asserts applying a
//! selection leaves every other byte untouched.

use serde::{Deserialize, Serialize};

use crate::error::{AiError, AiErrorKind};

/// A byte range in the document, on character boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub start: usize,
    pub end: usize,
}

impl Selection {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
        }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Whether the range is usable against a document of this length and lands on character
    /// boundaries.
    pub fn is_valid_for(&self, text: &str) -> bool {
        self.end <= text.len()
            && text.is_char_boundary(self.start)
            && text.is_char_boundary(self.end)
    }
}

/// How the result should be applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ApplyMode {
    /// Replace the whole document body.
    All,
    /// Replace exactly this range.
    Selection(Selection),
    /// Append to the end, for a generated section.
    Append,
}

/// What applying will do, for the user to review before it happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyPlan {
    pub mode: ApplyMode,
    /// The text that will replace the range or the document.
    pub replacement: String,
    /// Characters that will be replaced.
    pub replaced_characters: usize,
    /// A short description for the confirmation dialog.
    pub summary: String,
}

/// The result of applying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyOutcome {
    pub text: String,
    /// The range that actually changed, so an editor can select it.
    pub changed: Option<Selection>,
}

impl ApplyPlan {
    /// Plan a full replacement.
    pub fn all(document: &str, replacement: impl Into<String>) -> Self {
        let replacement = replacement.into();
        Self {
            mode: ApplyMode::All,
            replaced_characters: document.len(),
            summary: format!("replace the whole document ({} characters)", document.len()),
            replacement,
        }
    }

    /// Plan a selection replacement, validating the range first.
    ///
    /// An invalid range is an error rather than a silent fallback to "replace everything": a bad
    /// offset means the document changed underneath the user, and overwriting the whole file in that
    /// situation is exactly the data loss `handoff.md` §20.2 exists to prevent.
    pub fn selection(
        document: &str,
        selection: Selection,
        replacement: impl Into<String>,
    ) -> Result<Self, AiError> {
        if !selection.is_valid_for(document) {
            return Err(AiError::new(
                AiErrorKind::ClientError,
                format!(
                    "the selected range {}..{} is not valid for a document of {} bytes",
                    selection.start,
                    selection.end,
                    document.len()
                ),
            ));
        }
        let replacement = replacement.into();
        Ok(Self {
            mode: ApplyMode::Selection(selection),
            replaced_characters: selection.len(),
            summary: format!(
                "replace the selected {} character(s)",
                selection.len()
            ),
            replacement,
        })
    }

    /// Plan an append.
    pub fn append(replacement: impl Into<String>) -> Self {
        let replacement = replacement.into();
        Self {
            mode: ApplyMode::Append,
            replaced_characters: 0,
            summary: format!("append {} character(s)", replacement.len()),
            replacement,
        }
    }

    /// Whether this plan changes anything at all.
    ///
    /// A no-op plan is reported rather than applied, so an "apply" that changed nothing is not
    /// mistaken for success — and so the editor does not mark the document dirty.
    pub fn is_noop(&self, document: &str) -> bool {
        match &self.mode {
            ApplyMode::All => document == self.replacement,
            ApplyMode::Selection(selection) => {
                selection.is_valid_for(document)
                    && document[selection.start..selection.end] == self.replacement
            }
            ApplyMode::Append => self.replacement.is_empty(),
        }
    }

    /// Apply the plan, returning the new text.
    ///
    /// The caller is responsible for having shown the user the plan first; this is the explicit
    /// second step, not the confirmation itself.
    pub fn apply(&self, document: &str) -> Result<ApplyOutcome, AiError> {
        match &self.mode {
            ApplyMode::All => Ok(ApplyOutcome {
                text: self.replacement.clone(),
                changed: Some(Selection::new(0, self.replacement.len())),
            }),
            ApplyMode::Selection(selection) => {
                if !selection.is_valid_for(document) {
                    return Err(AiError::new(
                        AiErrorKind::ClientError,
                        format!(
                            "the selected range {}..{} is no longer valid; the document changed",
                            selection.start, selection.end
                        ),
                    ));
                }
                let mut text = String::with_capacity(
                    document.len() - selection.len() + self.replacement.len(),
                );
                text.push_str(&document[..selection.start]);
                text.push_str(&self.replacement);
                text.push_str(&document[selection.end..]);
                Ok(ApplyOutcome {
                    text,
                    changed: Some(Selection::new(
                        selection.start,
                        selection.start + self.replacement.len(),
                    )),
                })
            }
            ApplyMode::Append => {
                let mut text = document.to_string();
                text.push_str(&self.replacement);
                Ok(ApplyOutcome {
                    text,
                    changed: Some(Selection::new(
                        document.len(),
                        document.len() + self.replacement.len(),
                    )),
                })
            }
        }
    }

    /// Whether every byte outside the planned range is preserved.
    ///
    /// This is the property `handoff.md` §25 C7 asserts, exposed so a caller (and a test) can check
    /// it without duplicating the reasoning.
    pub fn preserves_outside(&self, document: &str, outcome: &ApplyOutcome) -> bool {
        match &self.mode {
            // A full replacement deliberately preserves nothing.
            ApplyMode::All => true,
            ApplyMode::Selection(selection) => {
                outcome.text.starts_with(&document[..selection.start])
                    && outcome.text.ends_with(&document[selection.end..])
            }
            ApplyMode::Append => outcome.text.starts_with(document),
        }
    }
}

/// Compute the plan for a feature's result, honouring the feature's own capabilities.
///
/// A feature that does not support a selection cannot be given one, which is checked here rather
/// than trusted from the UI.
pub fn plan_for(
    document: &str,
    replacement: impl Into<String>,
    selection: Option<Selection>,
    supports_selection: bool,
) -> Result<ApplyPlan, AiError> {
    match selection {
        Some(selection) if supports_selection => ApplyPlan::selection(document, selection, replacement),
        Some(_) => Err(AiError::new(
            AiErrorKind::ClientError,
            "this feature works on the whole document, not on a selection",
        )),
        None => Ok(ApplyPlan::all(document, replacement)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_apply_touches_only_the_selected_range() {
        // handoff.md section 25 C7, the property this module exists for.
        let document = "alpha BETA gamma";
        let selection = Selection::new(6, 10); // "BETA"
        assert_eq!(&document[selection.start..selection.end], "BETA");

        let plan = ApplyPlan::selection(document, selection, "delta").unwrap();
        let outcome = plan.apply(document).unwrap();

        assert_eq!(outcome.text, "alpha delta gamma");
        assert!(plan.preserves_outside(document, &outcome));
        // Everything before and after is byte-identical.
        assert_eq!(&outcome.text[..6], &document[..6]);
        assert_eq!(&outcome.text[6 + "delta".len()..], &document[10..]);
    }

    #[test]
    fn a_selection_apply_reports_the_changed_range() {
        let document = "alpha BETA gamma";
        let plan = ApplyPlan::selection(document, Selection::new(6, 10), "d").unwrap();
        let outcome = plan.apply(document).unwrap();
        assert_eq!(outcome.changed, Some(Selection::new(6, 7)));
        assert_eq!(&outcome.text[6..7], "d");
    }

    #[test]
    fn an_apply_across_a_multibyte_boundary_preserves_the_rest() {
        let document = "前言 BETA 后记";
        let start = document.find("BETA").unwrap();
        let selection = Selection::new(start, start + 4);

        let plan = ApplyPlan::selection(document, selection, "内容").unwrap();
        let outcome = plan.apply(document).unwrap();

        assert_eq!(outcome.text, "前言 内容 后记");
        assert!(plan.preserves_outside(document, &outcome));
    }

    #[test]
    fn an_invalid_range_is_an_error_not_a_fallback_to_replacing_everything() {
        // Overwriting the whole document because an offset went stale is the data loss
        // handoff.md section 20.2 exists to prevent.
        let document = "short";
        let error = ApplyPlan::selection(document, Selection::new(0, 999), "x").unwrap_err();
        assert!(error.message.contains("not valid"), "{}", error.message);

        let error = ApplyPlan::selection("你好", Selection::new(1, 2), "x").unwrap_err();
        assert!(
            error.message.contains("not valid"),
            "a mid-character range must be rejected: {}",
            error.message
        );
    }

    #[test]
    fn applying_a_range_that_became_invalid_is_refused() {
        let document = "alpha BETA gamma";
        let plan = ApplyPlan::selection(document, Selection::new(6, 10), "d").unwrap();
        // The document shrank underneath the plan.
        let error = plan.apply("alpha").unwrap_err();
        assert!(error.message.contains("no longer valid"), "{}", error.message);
    }

    #[test]
    fn applying_everything_replaces_the_document() {
        let plan = ApplyPlan::all("old body", "new body");
        let outcome = plan.apply("old body").unwrap();
        assert_eq!(outcome.text, "new body");
        assert_eq!(plan.summary, "replace the whole document (8 characters)");
    }

    #[test]
    fn appending_leaves_the_document_intact_and_adds_to_the_end() {
        let document = "existing";
        let plan = ApplyPlan::append("\n\n## New section");
        let outcome = plan.apply(document).unwrap();
        assert_eq!(outcome.text, "existing\n\n## New section");
        assert!(plan.preserves_outside(document, &outcome));
        assert_eq!(outcome.changed, Some(Selection::new(8, 8 + 16)));
    }

    #[test]
    fn a_noop_plan_is_detectable() {
        // An "apply" that changes nothing must not be reported as a successful edit, and must not
        // dirty the document.
        let document = "same";
        assert!(ApplyPlan::all(document, "same").is_noop(document));
        assert!(!ApplyPlan::all(document, "different").is_noop(document));
        assert!(ApplyPlan::selection(document, Selection::new(0, 4), "same")
            .unwrap()
            .is_noop(document));
        assert!(ApplyPlan::append("").is_noop(document));
        assert!(!ApplyPlan::append("x").is_noop(document));
    }

    #[test]
    fn the_summary_describes_the_change_for_the_confirmation_dialog() {
        let document = "alpha BETA gamma";
        let plan = ApplyPlan::selection(document, Selection::new(6, 10), "x").unwrap();
        assert!(plan.summary.contains("selected 4"), "{}", plan.summary);
        assert_eq!(plan.replaced_characters, 4);
    }

    #[test]
    fn a_selection_is_normalised_so_a_reversed_range_still_works() {
        let selection = Selection::new(10, 4);
        assert_eq!(selection.start, 4);
        assert_eq!(selection.end, 10);
        assert_eq!(selection.len(), 6);
        assert!(!selection.is_empty());
        assert!(Selection::new(3, 3).is_empty());
    }

    #[test]
    fn a_feature_that_does_not_support_a_selection_refuses_one() {
        // Checked here rather than trusted from the UI.
        let document = "alpha beta";
        let error = plan_for(document, "x", Some(Selection::new(0, 5)), false).unwrap_err();
        assert!(error.message.contains("whole document"), "{}", error.message);

        let plan = plan_for(document, "x", Some(Selection::new(0, 5)), true).unwrap();
        assert!(matches!(plan.mode, ApplyMode::Selection(_)));

        let plan = plan_for(document, "x", None, false).unwrap();
        assert_eq!(plan.mode, ApplyMode::All);
    }

    #[test]
    fn a_plan_round_trips_through_json_for_the_frontend_to_review() {
        let plan = ApplyPlan::selection("alpha beta", Selection::new(0, 5), "gamma").unwrap();
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("\"mode\":\"selection\""));
        assert!(json.contains("\"start\":0"));
        assert_eq!(serde_json::from_str::<ApplyPlan>(&json).unwrap(), plan);
    }

    #[test]
    fn an_empty_selection_apply_is_valid_and_inserts_text() {
        let document = "ab";
        let plan = ApplyPlan::selection(document, Selection::new(1, 1), "XY").unwrap();
        let outcome = plan.apply(document).unwrap();
        assert_eq!(outcome.text, "aXYb");
        assert!(plan.preserves_outside(document, &outcome));
    }
}
