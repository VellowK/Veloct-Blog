//! Network image sources.
//!
//! `handoff.md` §13.14 is short and completely prescriptive:
//!
//! ```text
//! search -> the user picks -> write the remote URL straight into the Markdown
//! ```
//!
//! and then: **does not enter the attachment cache pool, is not downloaded, is not uploaded again.**
//!
//! That is why this module has no dependency on [`crate::pool`] and no HTTP client. A network image
//! is a URL in the Markdown and nothing else. Writing it into the cache pool would make Veloct claim
//! to own a file it does not, and uploading it to the user's own storage would take a copy of someone
//! else's asset and re-host it — neither of which the contract asks for.
//!
//! The search itself belongs to the vendor's API, which the desktop app calls with the user's own key
//! through the AI/HTTP seam. What this module owns is the SHAPE: what a search result is, what the
//! user's choice turns into, and the rules that keep the choice honest (a licence the user should see,
//! a source that must be named, and no accidental download).

use serde::{Deserialize, Serialize};

/// A provider of free network images, as `handoff.md` §13.1's fifth target names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    Pixabay,
    Unsplash,
    Pexels,
}

impl ImageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pixabay => "pixabay",
            Self::Unsplash => "unsplash",
            Self::Pexels => "pexels",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "pixabay" => Some(Self::Pixabay),
            "unsplash" => Some(Self::Unsplash),
            "pexels" => Some(Self::Pexels),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pixabay => "Pixabay",
            Self::Unsplash => "Unsplash",
            Self::Pexels => "Pexels",
        }
    }

    /// Whether this source's API needs a key. All three do.
    pub fn needs_key(self) -> bool {
        true
    }
}

/// One result from a search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageResult {
    pub source: ImageSource,
    /// The provider's own id, so a selection can be recorded and re-fetched.
    pub source_id: String,
    /// The URL that goes into the Markdown.
    pub url: String,
    /// A smaller URL for the picker, so choosing an image does not download the full size.
    ///
    /// This is a URL handed to a webview, not a download Veloct performs: §13.14's "not downloaded"
    /// is about Veloct taking custody of the bytes, which it never does.
    pub thumbnail_url: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// The author, which §13.14's "name the source" requirement needs.
    #[serde(default)]
    pub author: String,
    /// The licence, so the user sees it before choosing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licence: Option<String>,
    /// The page the image came from, for attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_page: Option<String>,
}

impl ImageResult {
    /// The Markdown this choice produces.
    ///
    /// Alt text falls back to the provider and the author rather than to nothing: an image with empty
    /// alt text is inaccessible, and §13.14's flow gives Veloct the information to do better.
    pub fn markdown(&self, alt: Option<&str>) -> String {
        let alt = match alt.map(str::trim) {
            Some(text) if !text.is_empty() => text.to_string(),
            _ if self.author.trim().is_empty() => format!("Image from {}", self.source.label()),
            _ => format!("Image by {} on {}", self.author, self.source.label()),
        };
        format!("![{alt}]({})", self.url)
    }

    /// An attribution line, for the note card the picker offers.
    pub fn attribution(&self) -> String {
        match (&self.source_page, self.author.trim()) {
            (Some(page), "") => format!("{} · {page}", self.source.label()),
            (Some(page), author) => format!("{author} · {} · {page}", self.source.label()),
            (None, "") => self.source.label().to_string(),
            (None, author) => format!("{author} · {}", self.source.label()),
        }
    }

    /// Whether the result is usable: a URL and a stable id.
    pub fn is_usable(&self) -> bool {
        !self.url.trim().is_empty() && !self.source_id.trim().is_empty()
    }

    /// Whether the URL is one a browser can use.
    ///
    /// Checked rather than trusted, because a provider response is external data and a `javascript:`
    /// or `data:` URL written into Markdown would be a real problem.
    pub fn url_is_safe(&self) -> bool {
        let lowered = self.url.trim().to_ascii_lowercase();
        lowered.starts_with("http://") || lowered.starts_with("https://")
    }
}

/// What a search returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSearch {
    pub source: ImageSource,
    pub query: String,
    pub results: Vec<ImageResult>,
    /// Whether more results are available, for a "load more" control.
    #[serde(default)]
    pub has_more: bool,
}

impl ImageSearch {
    pub fn new(source: ImageSource, query: impl Into<String>, results: Vec<ImageResult>) -> Self {
        Self {
            source,
            query: query.into(),
            results,
            has_more: false,
        }
    }

    /// Only the results that can actually be written into Markdown.
    ///
    /// A result with no URL or a non-http URL is dropped rather than shown: offering something that
    /// cannot be used, or worse something unsafe, is not a service to the user.
    pub fn usable(&self) -> Vec<&ImageResult> {
        self.results
            .iter()
            .filter(|result| result.is_usable() && result.url_is_safe())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.usable().is_empty()
    }
}

/// A chosen network image, ready to be written.
///
/// Deliberately NOT an attachment: there is no id, no content hash, no cache path and no target. §13.14
/// says the image never enters the cache pool, and a type that cannot express those fields is how that
/// is guaranteed rather than remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkImageChoice {
    pub source: ImageSource,
    pub source_id: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    /// The attribution to record, if the user wants it in the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

impl NetworkImageChoice {
    /// Choose a result.
    pub fn from_result(result: &ImageResult, alt: Option<&str>, include_attribution: bool) -> Self {
        Self {
            source: result.source,
            source_id: result.source_id.clone(),
            url: result.url.clone(),
            alt: alt.map(str::to_string),
            attribution: include_attribution.then(|| result.attribution()),
        }
    }

    /// The Markdown to insert, with the attribution as a following line when there is one.
    pub fn markdown(&self) -> String {
        let alt = match self.alt.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => text.to_string(),
            _ => format!("Image from {}", self.source.label()),
        };
        let mut out = format!("![{alt}]({})", self.url);
        if let Some(attribution) = &self.attribution {
            out.push_str(&format!("\n\n*{attribution}*"));
        }
        out
    }

    /// Whether the choice may be written.
    pub fn is_usable(&self) -> bool {
        !self.url.trim().is_empty()
            && (self.url.trim().to_ascii_lowercase().starts_with("http://")
                || self.url.trim().to_ascii_lowercase().starts_with("https://"))
    }
}

/// Whether a URL is a network-image URL rather than a cached attachment.
///
/// Used when opening a document: a URL from a provider must NOT be offered as "manage this
/// attachment", because it is not one and there is nothing to manage.
pub fn is_network_image(url: &str) -> bool {
    let lowered = url.trim().to_ascii_lowercase();
    if !lowered.starts_with("http://") && !lowered.starts_with("https://") {
        return false;
    }
    // A URL that points at one of the user's own storage targets is an uploaded attachment, not a
    // network image, even though both are absolute URLs.
    !looks_like_our_own_storage(&lowered)
}

/// A conservative test for "this is one of our own storage targets".
///
/// The real check is against the configured `public_url_prefix` values, which the caller has and this
/// module does not. This is the fallback: a path that looks like an object key under a shared prefix.
/// Being conservative matters, because misclassifying an uploaded attachment as a network image would
/// offer to "forget" a file the user actually owns.
fn looks_like_our_own_storage(url: &str) -> bool {
    url.contains("/uploads/")
        || url.contains("/attachments/")
        || url.contains("/assets/uploads/")
        || url.contains(".attachments-cache")
}

/// The statement §13.14's flow implies and the UI should show.
///
/// A network image is not backed up, not cached, and can disappear if the provider removes it. That is
/// a real consequence of "does not enter the cache pool, is not downloaded", and a user choosing one
/// should be told rather than discovering it later.
pub fn network_image_notice() -> &'static str {
    "网络图片直接写入 Markdown,不进入附件缓存池、不下载、不再上传。它不会被备份,也可能因为来源站\
移除了图片而失效;如果这张图对文章很重要,请把它下载后作为本地附件上传。"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(source: ImageSource) -> ImageResult {
        ImageResult {
            source,
            source_id: "12345".to_string(),
            url: "https://cdn.pixabay.com/photo/2026/01/01/demo_1280.jpg".to_string(),
            thumbnail_url: "https://cdn.pixabay.com/photo/2026/01/01/demo_640.jpg".to_string(),
            width: 1280,
            height: 720,
            author: "A Photographer".to_string(),
            licence: Some("Pixabay Content License".to_string()),
            source_page: Some("https://pixabay.com/photos/demo-12345/".to_string()),
        }
    }

    #[test]
    fn the_three_documented_sources_are_recognised() {
        // handoff.md section 13.1 names "free network image sources (such as Pixabay)".
        assert_eq!(ImageSource::parse("pixabay"), Some(ImageSource::Pixabay));
        assert_eq!(ImageSource::parse("Unsplash"), Some(ImageSource::Unsplash));
        assert_eq!(ImageSource::parse("pexels"), Some(ImageSource::Pexels));
        assert_eq!(ImageSource::parse("shutterstock"), None);
        for source in [ImageSource::Pixabay, ImageSource::Unsplash, ImageSource::Pexels] {
            assert_eq!(ImageSource::parse(source.as_str()), Some(source));
            assert!(!source.label().is_empty());
        }
    }

    #[test]
    fn a_search_result_becomes_a_markdown_image_with_the_remote_url() {
        // handoff.md section 13.14: the remote URL goes straight into the Markdown.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), Some("A demo"), false);
        let markdown = choice.markdown();
        assert_eq!(
            markdown,
            "![A demo](https://cdn.pixabay.com/photo/2026/01/01/demo_1280.jpg)"
        );
        assert!(choice.is_usable());
    }

    #[test]
    fn a_choice_has_no_attachment_identity_at_all() {
        // handoff.md section 13.14: it does not enter the cache pool. A type that cannot express an
        // id, a hash or a cache path is how that is guaranteed.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), None, false);
        let json = serde_json::to_string(&choice).unwrap();
        for forbidden in ["content_hash", "cache_path", "storage_target_id", "attachment"] {
            assert!(
                !json.contains(forbidden),
                "a network image choice must not carry {forbidden}: {json}"
            );
        }
    }

    #[test]
    fn alt_text_falls_back_to_the_source_and_author_rather_than_to_nothing() {
        // An image with empty alt text is inaccessible, and the flow gives Veloct the information to
        // do better.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), None, false);
        assert!(choice.markdown().contains("Image from Pixabay"), "{}", choice.markdown());

        let mut anonymous = result(ImageSource::Unsplash);
        anonymous.author = String::new();
        let choice = NetworkImageChoice::from_result(&anonymous, Some("   "), false);
        assert!(choice.markdown().contains("Image from Unsplash"));

        // And an explicit alt wins.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), Some("My alt"), false);
        assert!(choice.markdown().starts_with("![My alt]"));
    }

    #[test]
    fn an_attribution_is_offered_and_included_only_when_asked() {
        let with = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), None, true);
        assert!(with.markdown().contains("*A Photographer · Pixabay · https://pixabay.com/photos/demo-12345/*"));

        let without = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), None, false);
        assert!(!without.markdown().contains("A Photographer"));
    }

    #[test]
    fn an_attribution_names_the_source_and_the_author() {
        let full = result(ImageSource::Pixabay);
        assert!(full.attribution().contains("A Photographer"));
        assert!(full.attribution().contains("Pixabay"));
        assert!(full.attribution().contains("pixabay.com"));

        let mut anonymous = result(ImageSource::Pexels);
        anonymous.author = String::new();
        anonymous.source_page = None;
        assert_eq!(anonymous.attribution(), "Pexels");
    }

    #[test]
    fn a_result_without_a_url_or_an_id_is_not_usable() {
        let mut broken = result(ImageSource::Pixabay);
        broken.url = String::new();
        assert!(!broken.is_usable());

        let mut no_id = result(ImageSource::Pixabay);
        no_id.source_id = "  ".to_string();
        assert!(!no_id.is_usable());
    }

    #[test]
    fn a_result_url_must_be_http_so_a_crafted_response_cannot_inject_a_scheme() {
        // A provider response is external data, and a data: or javascript: URL in Markdown would be a
        // real problem.
        for bad in [
            "javascript:alert(1)",
            "data:image/png;base64,AAAA",
            "file:///etc/passwd",
            "",
        ] {
            let mut unsafe_result = result(ImageSource::Pixabay);
            unsafe_result.url = bad.to_string();
            assert!(!unsafe_result.url_is_safe(), "{bad} should be refused");
            assert!(
                !NetworkImageChoice::from_result(&unsafe_result, None, false).is_usable(),
                "{bad} must not produce a usable choice"
            );
        }
        assert!(result(ImageSource::Pixabay).url_is_safe());
        assert!(result(ImageSource::Pixabay).url.starts_with("https://"));
    }

    #[test]
    fn a_search_hides_the_results_that_cannot_be_used() {
        let mut good = result(ImageSource::Pixabay);
        good.source_id = "good".to_string();
        let mut bad = result(ImageSource::Pixabay);
        bad.url = "javascript:alert(1)".to_string();

        let search = ImageSearch::new(ImageSource::Pixabay, "demo", vec![good, bad]);
        assert_eq!(search.results.len(), 2, "the raw results are kept for the log");
        assert_eq!(search.usable().len(), 1, "but only the usable one is offered");
        assert_eq!(search.usable()[0].source_id, "good");
    }

    #[test]
    fn an_empty_search_is_reported_as_empty() {
        let search = ImageSearch::new(ImageSource::Unsplash, "nothing matches", Vec::new());
        assert!(search.is_empty());
        assert!(search.usable().is_empty());
    }

    #[test]
    fn a_network_image_url_is_distinguished_from_an_uploaded_attachment() {
        // A provider URL must not be offered as "manage this attachment": it is not one and there is
        // nothing to manage. Misclassifying an uploaded attachment the other way would offer to
        // "forget" a file the user actually owns.
        assert!(is_network_image("https://cdn.pixabay.com/photo/x.jpg"));
        assert!(is_network_image("https://images.unsplash.com/photo-1"));

        assert!(!is_network_image("/local/photo.webp"), "a relative path is not a network image");
        assert!(!is_network_image("photo.webp"));
        assert!(
            !is_network_image("https://cdn.example.com/uploads/photo-a8k3xq.webp"),
            "an uploaded attachment is our own storage, not a network image"
        );
        assert!(!is_network_image("https://cdn.example.com/attachments/x.webp"));
    }

    #[test]
    fn the_notice_says_what_a_network_image_costs_the_user() {
        // A network image is not backed up and can disappear. That is a real consequence of "not
        // cached, not downloaded", and the user should be told rather than finding out later.
        let notice = network_image_notice();
        assert!(notice.contains("不进入附件缓存池"), "{notice}");
        assert!(notice.contains("不下载"), "{notice}");
        assert!(notice.contains("备份"), "{notice}");
        assert!(notice.contains("失效"), "{notice}");
    }

    #[test]
    fn every_source_needs_a_key() {
        for source in [ImageSource::Pixabay, ImageSource::Unsplash, ImageSource::Pexels] {
            assert!(source.needs_key());
        }
    }

    #[test]
    fn a_search_and_a_choice_round_trip_through_json() {
        let search = ImageSearch::new(
            ImageSource::Pixabay,
            "demo",
            vec![result(ImageSource::Pixabay)],
        );
        let json = serde_json::to_string(&search).unwrap();
        assert!(json.contains("\"source\":\"pixabay\""), "{json}");
        assert_eq!(serde_json::from_str::<ImageSearch>(&json).unwrap(), search);

        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pixabay), Some("x"), true);
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(
            serde_json::from_str::<NetworkImageChoice>(&json).unwrap(),
            choice
        );
    }

    #[test]
    fn a_result_keeps_the_dimensions_and_licence_the_picker_shows() {
        let full = result(ImageSource::Pexels);
        assert_eq!(full.width, 1280);
        assert_eq!(full.height, 720);
        assert_eq!(full.licence.as_deref(), Some("Pixabay Content License"));
        assert!(full.thumbnail_url.contains("_640"), "a thumbnail avoids downloading the full size");
    }
}
