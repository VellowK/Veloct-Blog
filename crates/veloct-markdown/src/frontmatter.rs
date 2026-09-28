//! The frontmatter contract from `docs/spec/01-data-model.md` §2.
//!
//! Design decisions worth stating:
//!
//! * **Unknown keys are preserved.** A user's extra frontmatter must survive a load/save cycle,
//!   so anything this crate does not recognise is kept verbatim and re-emitted.
//! * **Timestamps are strings.** They are stored as RFC 3339 text and validated on read rather
//!   than being reinterpreted, so a round trip cannot change the representation of a value the
//!   user or another tool wrote.
//! * **`id` is never derived from `slug`.** `handoff.md` §3.2 makes `id` permanent precisely so
//!   that renaming a slug cannot break a stable `/wip/<id>` URL or a knowledge reference.

use serde::{Deserialize, Serialize};

use veloct_core::{Result, StableId, VeloctError};

/// Which collection a document belongs to (`handoff.md` §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Article,
    Page,
    Knowledge,
}

impl ContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Article => "article",
            Self::Page => "page",
            Self::Knowledge => "knowledge",
        }
    }

    /// The on-disk subdirectory this type lives in, relative to the project root.
    pub fn directory(self) -> &'static str {
        match self {
            Self::Article => "content/articles",
            Self::Page => "content/pages",
            Self::Knowledge => "knowledge",
        }
    }

    /// `wip` and `wip_publish` are only meaningful for articles (`handoff.md` §3.2, §7).
    pub fn supports_wip(self) -> bool {
        matches!(self, Self::Article)
    }
}

impl std::str::FromStr for ContentType {
    type Err = VeloctError;

    fn from_str(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "article" => Ok(Self::Article),
            "page" => Ok(Self::Page),
            "knowledge" => Ok(Self::Knowledge),
            other => Err(VeloctError::Frontmatter {
                path: std::path::PathBuf::from("<frontmatter>"),
                message: format!(
                    "unknown type {other:?}; expected one of article, page, knowledge"
                ),
            }),
        }
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The four timestamp fields the contract requires.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timestamps {
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

/// Parsed frontmatter.
///
/// Serialisation always emits the known keys in the canonical contract order, then any unknown
/// keys, so a round trip is stable and a diff is readable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    /// Permanent internal identity. Never changes when `slug` does.
    pub id: StableId,
    #[serde(rename = "type")]
    pub content_type: ContentType,
    /// Mutable public slug.
    pub slug: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub wip: bool,
    #[serde(default)]
    pub wip_publish: bool,
    /// `handoff.md` §7.2: set the first time this WIP URL is actually emitted into a production
    /// build, and never reset. It is what justifies the permanent redirect after the article is
    /// finished.
    #[serde(default)]
    pub wip_ever_published: bool,
    #[serde(flatten)]
    pub timestamps: Timestamps,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    /// Page-creator block references (`handoff.md` §8). Still expressed in Markdown terms.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    /// Anything this crate does not model. Preserved verbatim.
    #[serde(flatten)]
    pub extras: serde_yaml::Mapping,
}

impl Frontmatter {
    /// A minimal valid record for a brand new document.
    pub fn new(content_type: ContentType, id: StableId, slug: impl Into<String>, title: impl Into<String>, now: &str) -> Self {
        Self {
            id,
            content_type,
            slug: slug.into(),
            title: title.into(),
            summary: None,
            wip: content_type.supports_wip(),
            wip_publish: false,
            wip_ever_published: false,
            timestamps: Timestamps {
                created_at: now.to_string(),
                updated_at: now.to_string(),
                published_at: None,
            },
            tags: Vec::new(),
            category: None,
            cover: None,
            blocks: Vec::new(),
            extras: serde_yaml::Mapping::new(),
        }
    }

    /// Read a free-text extra, used for fields the contract leaves open (author, year, ...).
    pub fn extra_str(&self, key: &str) -> Option<&str> {
        self.extras.get(serde_yaml::Value::String(key.to_string()))?.as_str()
    }

    /// Whether this document is excluded from normal article routing (`handoff.md` §7.1).
    pub fn is_wip(&self) -> bool {
        self.content_type.supports_wip() && self.wip
    }

    /// Validate the cross-field rules the contract implies.
    ///
    /// Rejecting this here rather than later means a malformed file is reported at load time,
    /// with the file named, instead of producing a confusing build error.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.slug.trim().is_empty() {
            return Err("slug must not be empty".into());
        }
        if self.title.trim().is_empty() {
            return Err("title must not be empty".into());
        }
        if !self.content_type.supports_wip()
            && (self.wip || self.wip_publish || self.wip_ever_published)
        {
            return Err(format!(
                "wip fields are only meaningful for articles, but type is {}",
                self.content_type
            ));
        }
        if self.wip_publish && !self.wip {
            return Err(
                "wip_publish is true while wip is false; publishable WIP must still be WIP".into(),
            );
        }
        if self.wip_ever_published && !self.content_type.supports_wip() {
            return Err("wip_ever_published is only meaningful for articles".into());
        }
        for (label, value) in [
            ("created_at", Some(&self.timestamps.created_at)),
            ("updated_at", Some(&self.timestamps.updated_at)),
            ("published_at", self.timestamps.published_at.as_ref()),
        ] {
            if let Some(text) = value {
                if veloct_core::clock::parse_rfc3339(text).is_err() {
                    return Err(format!("{label} is not an RFC 3339 timestamp: {text:?}"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Frontmatter {
        Frontmatter {
            id: StableId::from_parts(1_700_000_000_000, 42),
            content_type: ContentType::Article,
            slug: "hello-world".into(),
            title: "Hello World".into(),
            summary: Some("A first post".into()),
            wip: false,
            wip_publish: false,
            wip_ever_published: true,
            timestamps: Timestamps {
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-02T00:00:00Z".into(),
                published_at: Some("2026-01-03T00:00:00Z".into()),
            },
            tags: vec!["rust".into(), "astro".into()],
            category: Some("engineering".into()),
            cover: None,
            blocks: Vec::new(),
            extras: serde_yaml::Mapping::new(),
        }
    }

    #[test]
    fn content_type_directories_match_the_data_model() {
        assert_eq!(ContentType::Article.directory(), "content/articles");
        assert_eq!(ContentType::Page.directory(), "content/pages");
        assert_eq!(ContentType::Knowledge.directory(), "knowledge");
        assert!(ContentType::Article.supports_wip());
        assert!(!ContentType::Knowledge.supports_wip());
    }

    #[test]
    fn content_type_parses_and_rejects_unknown_values() {
        assert_eq!("article".parse::<ContentType>().unwrap(), ContentType::Article);
        assert_eq!("PAGE".parse::<ContentType>().unwrap(), ContentType::Page);
        assert!("draft".parse::<ContentType>().is_err());
    }

    #[test]
    fn frontmatter_round_trips_and_keeps_the_canonical_key_order() {
        let mut fm = sample();
        // An extra key the crate does not model must survive.
        fm.extras.insert(
            serde_yaml::Value::String("author".into()),
            serde_yaml::Value::String("vellow".into()),
        );

        let yaml = serde_yaml::to_string(&fm).unwrap();
        let order: Vec<&str> = yaml
            .lines()
            .filter(|line| !line.starts_with(' ') && line.contains(':'))
            .map(|line| line.split(':').next().unwrap())
            .collect();
        assert_eq!(
            order,
            vec![
                "id",
                "type",
                "slug",
                "title",
                "summary",
                "wip",
                "wip_publish",
                "wip_ever_published",
                "created_at",
                "updated_at",
                "published_at",
                "tags",
                "category",
                "author",
            ],
            "yaml was:\n{yaml}"
        );

        let parsed: Frontmatter = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, fm);
        assert_eq!(parsed.extra_str("author"), Some("vellow"));
    }

    #[test]
    fn omitted_optionals_do_not_appear_in_yaml() {
        let mut fm = sample();
        fm.summary = None;
        fm.category = None;
        fm.timestamps.published_at = None;
        let yaml = serde_yaml::to_string(&fm).unwrap();
        assert!(!yaml.contains("summary"));
        assert!(!yaml.contains("category"));
        assert!(!yaml.contains("published_at"));
        // Booleans that default to false are still written, because their absence is a change
        // in meaning for `wip_ever_published` and readability matters for the others.
        assert!(yaml.contains("wip: false"));
        assert!(yaml.contains("wip_ever_published: true"));
    }

    #[test]
    fn validate_accepts_a_consistent_record() {
        assert!(sample().validate().is_ok());
    }

    #[test]
    fn validate_rejects_publishable_wip_that_is_not_wip() {
        let mut fm = sample();
        fm.wip = false;
        fm.wip_publish = true;
        let error = fm.validate().unwrap_err();
        assert!(error.contains("wip_publish"), "{error}");
    }

    #[test]
    fn validate_rejects_wip_fields_on_a_knowledge_document() {
        // handoff §3.2: wip only has meaning for articles.
        let mut fm = sample();
        fm.content_type = ContentType::Knowledge;
        fm.wip = true;
        fm.wip_publish = false;
        fm.wip_ever_published = false;
        let error = fm.validate().unwrap_err();
        assert!(error.contains("only meaningful for articles"), "{error}");
    }

    #[test]
    fn validate_rejects_an_empty_slug_or_title_and_bad_timestamps() {
        let mut fm = sample();
        fm.slug = "  ".into();
        assert!(fm.validate().unwrap_err().contains("slug"));

        let mut fm = sample();
        fm.title = "".into();
        assert!(fm.validate().unwrap_err().contains("title"));

        let mut fm = sample();
        fm.timestamps.updated_at = "yesterday".into();
        let error = fm.validate().unwrap_err();
        assert!(error.contains("updated_at"), "{error}");
    }

    #[test]
    fn extra_str_only_returns_strings() {
        let mut fm = sample();
        fm.extras.insert(
            serde_yaml::Value::String("count".into()),
            serde_yaml::Value::Number(3.into()),
        );
        assert_eq!(fm.extra_str("count"), None);
        assert_eq!(fm.extra_str("missing"), None);
    }

    #[test]
    fn new_articles_start_as_unpublished_wip() {
        // A new article is WIP by default but is not emitted into a production build until the
        // user publishes it (handoff §7.1).
        let fm = Frontmatter::new(
            ContentType::Article,
            StableId::new(),
            "first-post",
            "First Post",
            "2026-01-01T00:00:00Z",
        );
        assert!(fm.wip);
        assert!(!fm.wip_publish);
        assert!(!fm.wip_ever_published);
        assert!(fm.is_wip());
        assert!(fm.validate().is_ok());
    }

    #[test]
    fn is_wip_is_false_for_non_articles_even_if_the_flag_is_set() {
        let mut fm = sample();
        fm.content_type = ContentType::Knowledge;
        fm.wip = true;
        assert!(!fm.is_wip());
    }
}
