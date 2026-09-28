//! Feature-level model assignment.
//!
//! `handoff.md` §11.3: every built-in AI capability maps to a provider and a model, and the user
//! cannot create new capabilities or maintain a prompt-template system. The feature list is
//! therefore a closed set defined in code, and the UI only chooses the model for each one.
//!
//! `handoff.md` §11.10 names the minimum set: article summary, article polish/rewrite, and page
//! creation.

use serde::{Deserialize, Serialize};

/// A built-in AI capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureId {
    /// `handoff.md` §11.11: generate or regenerate an article summary.
    ArticleSummary,
    /// Rewrite or polish the current selection.
    ArticlePolish,
    /// `handoff.md` §8.1: generate a whole page, structure and copy included.
    PageCreate,
    /// Shorten or expand a passage without changing its meaning.
    ArticleRewrite,
    /// Suggest tags for a document.
    ArticleTags,
}

impl FeatureId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ArticleSummary => "article.summary",
            Self::ArticlePolish => "article.polish",
            Self::PageCreate => "page.create",
            Self::ArticleRewrite => "article.rewrite",
            Self::ArticleTags => "article.tags",
        }
    }

    /// Parse a stored feature id, so a value in the database cannot drift from this list.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "article.summary" => Some(Self::ArticleSummary),
            "article.polish" => Some(Self::ArticlePolish),
            "page.create" => Some(Self::PageCreate),
            "article.rewrite" => Some(Self::ArticleRewrite),
            "article.tags" => Some(Self::ArticleTags),
            _ => None,
        }
    }
}

impl std::fmt::Display for FeatureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a feature needs, so the UI can present it correctly and the service can assemble context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeatureSpec {
    pub id: FeatureId,
    /// The label shown in the settings list.
    pub label: String,
    /// Whether the feature can work on a selection rather than the whole document.
    pub supports_selection: bool,
    /// Whether the feature's output replaces text the user selected (`handoff.md` §11.9).
    pub applies_to_document: bool,
    /// Whether the user must confirm before anything is written (`handoff.md` §11.9).
    ///
    /// Always true in Veloct. The field exists so the rule is visible in the data rather than only in
    /// the code that honours it, and a test asserts no feature opts out.
    pub requires_confirmation: bool,
}

/// The built-in feature list.
pub fn builtin_features() -> Vec<FeatureSpec> {
    vec![
        FeatureSpec {
            id: FeatureId::ArticleSummary,
            label: "文章摘要".to_string(),
            // A summary is derived from the whole document, so a selection is not meaningful.
            supports_selection: false,
            applies_to_document: true,
            requires_confirmation: true,
        },
        FeatureSpec {
            id: FeatureId::ArticlePolish,
            label: "文章润色".to_string(),
            supports_selection: true,
            applies_to_document: true,
            requires_confirmation: true,
        },
        FeatureSpec {
            id: FeatureId::PageCreate,
            label: "创建页面".to_string(),
            supports_selection: false,
            applies_to_document: false,
            requires_confirmation: true,
        },
        FeatureSpec {
            id: FeatureId::ArticleRewrite,
            label: "改写片段".to_string(),
            supports_selection: true,
            applies_to_document: true,
            requires_confirmation: true,
        },
        FeatureSpec {
            id: FeatureId::ArticleTags,
            label: "推荐标签".to_string(),
            supports_selection: false,
            applies_to_document: true,
            requires_confirmation: true,
        },
    ]
}

/// Look up one feature's specification.
pub fn feature_spec(id: FeatureId) -> FeatureSpec {
    builtin_features()
        .into_iter()
        .find(|spec| spec.id == id)
        .expect("every FeatureId has a specification")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feature_set_is_closed_and_every_id_round_trips() {
        // handoff.md section 11.3: the user cannot create new features, so this list is the whole
        // set and its stored form must be stable.
        for spec in builtin_features() {
            assert_eq!(FeatureId::parse(spec.id.as_str()), Some(spec.id));
            assert_eq!(spec.id.to_string(), spec.id.as_str());
        }
        assert_eq!(FeatureId::parse("something.else"), None);
    }

    #[test]
    fn the_documented_minimum_features_are_present() {
        // handoff.md section 11.10.
        let ids: Vec<FeatureId> = builtin_features().into_iter().map(|spec| spec.id).collect();
        assert!(ids.contains(&FeatureId::ArticleSummary));
        assert!(ids.contains(&FeatureId::ArticlePolish));
        assert!(ids.contains(&FeatureId::PageCreate));
    }

    #[test]
    fn every_feature_requires_confirmation_before_it_writes() {
        // handoff.md section 11.9 and 25 C6: AI never rewrites the body before the user agrees.
        for spec in builtin_features() {
            assert!(
                spec.requires_confirmation,
                "{} would apply without confirmation",
                spec.id.as_str()
            );
        }
    }

    #[test]
    fn each_feature_has_a_non_empty_label() {
        for spec in builtin_features() {
            assert!(!spec.label.trim().is_empty(), "{} has no label", spec.id.as_str());
        }
    }

    #[test]
    fn feature_specs_round_trip_through_json() {
        let specs = builtin_features();
        let json = serde_json::to_string(&specs).unwrap();
        assert!(json.contains("\"article_summary\""));
        assert_eq!(serde_json::from_str::<Vec<FeatureSpec>>(&json).unwrap(), specs);
    }
}
