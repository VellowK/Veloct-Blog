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
//!
//! [`providers`] is the other half: the two providers' request and response contracts, read from their
//! own documentation, and the obligations each one puts on a page that shows its image.

pub mod providers;

use serde::{Deserialize, Serialize};

/// A provider of free network images.
/// `handoff.md` §13.1's fifth target is "free network image sources (such as Pixabay)", and §13.14
/// prescribes *search → the user picks → write the remote URL straight into the Markdown*.
///
/// **Two of the three variants are offered, and the third is a recorded refusal rather than a silent
/// absence.** Reading Pixabay's own documentation showed that its terms forbid the flow §13.14
/// prescribes — it requires images to be downloaded to the caller's own server and forbids permanent
/// hotlinking — so implementing that source here would publish pages that violate its terms.
/// `handoff.md` §20 forbids both a silent failure and an invented vendor behaviour, so the variant
/// stays and [`providers::unimplemented_reason`] answers with the provider's own words. See
/// `PROCESSING.md` §5.11 and `docs/spec/03-upstream-facts.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    /// Not offered by this build. See the type's documentation and
    /// [`providers::unimplemented_reason`].
    Pixabay,
    /// Offered: hotlinking is REQUIRED by their terms.
    Unsplash,
    /// Offered: hotlinking is permitted.
    Pexels,
}

impl ImageSource {
    /// Every source `handoff.md` §13.1's fifth target names, in one place.
    ///
    /// The same device as `StorageKind::ALL`: a canonical list the commands report against, so the
    /// question "which sources does this build offer" has ONE answer rather than a match in each
    /// caller.
    pub const ALL: [ImageSource; 3] = [
        ImageSource::Unsplash,
        ImageSource::Pexels,
        ImageSource::Pixabay,
    ];

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

    /// Whether this build can search this source.
    ///
    /// Derived from [`providers::unimplemented_reason`] rather than stated separately, so a source
    /// cannot be reported as offered while also carrying a reason it is not — the same rule
    /// `StorageKind::is_implemented` follows after the repair that made that split written down once.
    pub fn is_implemented(self) -> bool {
        providers::unimplemented_reason(self).is_none()
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
    /// The author's own profile page, which Unsplash's attribution rule requires a link to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_url: Option<String>,
    /// The provider's OWN description of the image.
    ///
    /// Carried because it is better than anything the caller can invent: both providers return one, and
    /// an image with no alt text is inaccessible. A caller that supplies its own alt wins; this is the
    /// fallback before the generated "Image by X on Y" line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
    /// The endpoint Unsplash requires a caller to hit when a user CHOOSES an image.
    ///
    /// Their API Guidelines make this a condition of API use, so it is data the result has to carry
    /// rather than something a caller can reconstruct. `None` for a source with no such obligation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_location: Option<String>,
}

impl ImageResult {
    /// The alt text for this result, decided in ONE place.
    ///
    /// Order: the caller's, then the PROVIDER's own description, then a line naming the author and the
    /// source. The provider's description beats a generated line because it describes the picture
    /// rather than restating where it came from, and an image with empty alt text is inaccessible, so
    /// there is always something.
    pub fn resolved_alt(&self, alt: Option<&str>) -> String {
        if let Some(text) = alt.map(str::trim).filter(|text| !text.is_empty()) {
            return text.to_string();
        }
        if let Some(text) = self
            .alt
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            return text.to_string();
        }
        if self.author.trim().is_empty() {
            format!("Image from {}", self.source.label())
        } else {
            format!("Image by {} on {}", self.author, self.source.label())
        }
    }

    /// The Markdown this result produces, with the alt resolved by [`Self::resolved_alt`].
    pub fn markdown(&self, alt: Option<&str>) -> String {
        format!("![{}]({})", self.resolved_alt(alt), self.url)
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
    ///
    /// **The attribution cannot be switched off for a source whose terms require it.** Unsplash's API
    /// Guidelines make crediting a condition of using the API, so a caller that passed
    /// `include_attribution = false` would produce a page that violates them — a flag is not a licence
    /// to break a provider's terms, so [`providers::HotlinkPolicy::Required`] overrides it. For Pexels,
    /// where the obligation is a prominent link rather than a per-image credit, the flag is honoured.
    ///
    /// The alt is the caller's when it has one, and otherwise the PROVIDER's own description, which is
    /// a better fallback than a generated line because it describes the picture.
    pub fn from_result(result: &ImageResult, alt: Option<&str>, include_attribution: bool) -> Self {
        let must_attribute = providers::facts(result.source)
            .map(|facts| facts.hotlink == providers::HotlinkPolicy::Required)
            .unwrap_or(false);
        let attribution =
            (include_attribution || must_attribute).then(|| providers::attribution_markdown(result));

        Self {
            source: result.source,
            source_id: result.source_id.clone(),
            url: result.url.clone(),
            // Resolved HERE rather than in `markdown()`, because a choice does not carry the author and
            // a later fallback could therefore only ever say "Image from {source}". One decision, made
            // where the information still exists.
            alt: Some(result.resolved_alt(alt)),
            attribution,
        }
    }

    /// The Markdown to insert, with the attribution as a following line when there is one.
    pub fn markdown(&self) -> String {
        let alt = match self.alt.as_deref().map(str::trim) {
            Some(text) if !text.is_empty() => text.to_string(),
            // Only reachable for a choice built by hand rather than by `from_result`, which resolves a
            // non-empty alt for every result.
            _ => format!("Image from {}", self.source.label()),
        };
        let mut out = format!("![{alt}]({})", self.url);
        if let Some(attribution) = &self.attribution {
            // Emphasised as a caption, which is how this codebase has always drawn the credit line.
            // The links inside it are what the providers actually require, and emphasis does not
            // interfere with them.
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

    /// A result for a source whose terms are NOT the ones the providers module knows about.
    ///
    /// These tests are about the SHAPE (what a result is, what a choice turns into), not about a
    /// particular provider's rules, so the fixture is deliberately Pixabay: it keeps the
    /// attribution-must-be-requested behaviour visible, where Unsplash's own tests in
    /// [`providers`] cover the case where the terms require credit.
    fn result(source: ImageSource) -> ImageResult {
        ImageResult {
            source,
            source_id: "12345".to_string(),
            url: "https://cdn.example.com/photo/2026/01/01/demo_1280.jpg".to_string(),
            thumbnail_url: "https://cdn.example.com/photo/2026/01/01/demo_640.jpg".to_string(),
            width: 1280,
            height: 720,
            author: "A Photographer".to_string(),
            licence: Some("Example Content License".to_string()),
            source_page: Some("https://example.com/photos/demo-12345/".to_string()),
            author_url: Some("https://example.com/@photographer".to_string()),
            alt: None,
            download_location: None,
        }
    }

    #[test]
    fn the_three_documented_sources_are_recognised_and_the_two_offered_ones_are_named() {
        // handoff.md section 13.1 names "free network image sources (such as Pixabay)".
        assert_eq!(ImageSource::parse("pixabay"), Some(ImageSource::Pixabay));
        assert_eq!(ImageSource::parse("Unsplash"), Some(ImageSource::Unsplash));
        assert_eq!(ImageSource::parse("pexels"), Some(ImageSource::Pexels));
        assert_eq!(ImageSource::parse("shutterstock"), None);
        for source in ImageSource::ALL {
            assert_eq!(ImageSource::parse(source.as_str()), Some(source));
            assert!(!source.label().is_empty());
        }
        // The offered set is derived, not restated: a source cannot be listed AND carry a reason it is
        // not offered.
        let offered: Vec<&str> = ImageSource::ALL
            .iter()
            .filter(|source| source.is_implemented())
            .map(|source| source.label())
            .collect();
        assert_eq!(offered, vec!["Unsplash", "Pexels"]);
    }

    #[test]
    fn a_search_result_becomes_a_markdown_image_with_the_remote_url() {
        // handoff.md section 13.14: the remote URL goes straight into the Markdown.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pexels), Some("A demo"), false);
        let markdown = choice.markdown();
        assert_eq!(
            markdown,
            "![A demo](https://cdn.example.com/photo/2026/01/01/demo_1280.jpg)"
        );
        assert!(choice.is_usable());
    }

    #[test]
    fn a_choice_has_no_attachment_identity_at_all() {
        // handoff.md section 13.14: it does not enter the cache pool. A type that cannot express an
        // id, a hash or a cache path is how that is guaranteed.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pexels), None, false);
        let json = serde_json::to_string(&choice).unwrap();
        for forbidden in ["content_hash", "cache_path", "storage_target_id", "attachment"] {
            assert!(
                !json.contains(forbidden),
                "a network image choice must not carry {forbidden}: {json}"
            );
        }
    }

    #[test]
    fn alt_text_prefers_the_users_then_the_providers_own_description() {
        // An image with empty alt text is inaccessible. The provider's description is a better
        // fallback than a generated line because it describes the PICTURE rather than restating where
        // it came from.
        let mut described = result(ImageSource::Pexels);
        described.alt = Some("Brown rocks during golden hour".to_string());

        let choice = NetworkImageChoice::from_result(&described, None, false);
        assert!(choice.markdown().starts_with("![Brown rocks during golden hour]"));

        // A caller's alt wins over the provider's.
        let choice = NetworkImageChoice::from_result(&described, Some("My alt"), false);
        assert!(choice.markdown().starts_with("![My alt]"));

        // A blank caller alt is not an alt, so it falls through to the provider's.
        let choice = NetworkImageChoice::from_result(&described, Some("   "), false);
        assert!(choice.markdown().starts_with("![Brown rocks during golden hour]"));
    }

    #[test]
    fn alt_text_falls_back_to_the_source_and_author_rather_than_to_nothing() {
        // Nothing describes the image, so the line says where it came from. Degenerate, but never an
        // empty alt.
        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pexels), None, false);
        assert!(choice.markdown().starts_with("![Image by A Photographer on Pexels]"), "{}", choice.markdown());

        let mut anonymous = result(ImageSource::Pexels);
        anonymous.author = String::new();
        let choice = NetworkImageChoice::from_result(&anonymous, Some("   "), false);
        assert!(choice.markdown().contains("Image from Pexels"), "{}", choice.markdown());
    }

    #[test]
    fn an_attribution_is_included_only_when_asked_for_a_source_whose_terms_allow_that() {
        // Pexels permits hotlinking, so whether the credit line goes into the document is the user's
        // choice — the obligation is a prominent link, which the picker shows either way.
        let with = NetworkImageChoice::from_result(&result(ImageSource::Pexels), None, true);
        let attribution = with.attribution.clone().expect("an attribution");
        assert!(attribution.contains("A Photographer"), "{attribution}");
        assert!(attribution.contains("Pexels"), "{attribution}");
        assert!(with.markdown().contains(&attribution));

        let without = NetworkImageChoice::from_result(&result(ImageSource::Pexels), None, false);
        assert!(without.attribution.is_none());
        // The author's NAME still appears, in the alt text — which is the point of resolving the alt
        // from the result. What is absent is the credit LINE, so the assertion is about the link.
        assert!(!without.markdown().contains("on [Pexels]("), "{}", without.markdown());
    }

    #[test]
    fn an_attribution_cannot_be_switched_off_where_the_terms_require_it() {
        // Unsplash's API Guidelines make crediting a condition of using the API, so a flag is not a
        // licence to publish an uncredited page. This is the rule that keeps §13.14's "write the URL
        // and move on" from quietly becoming a terms violation.
        let mut unsplash = result(ImageSource::Unsplash);
        unsplash.alt = Some("a man drinking coffee".to_string());

        for asked in [false, true] {
            let choice = NetworkImageChoice::from_result(&unsplash, None, asked);
            let attribution = choice
                .attribution
                .clone()
                .unwrap_or_else(|| panic!("include_attribution={asked} must not strip the credit"));
            assert!(attribution.contains("Unsplash"), "{attribution}");
            assert!(attribution.contains("A Photographer"), "{attribution}");
            assert!(attribution.contains("utm_source=veloct"), "{attribution}");
            assert!(choice.markdown().contains("utm_source=veloct"), "{}", choice.markdown());
        }
    }

    #[test]
    fn a_result_without_a_url_or_an_id_is_not_usable() {
        let mut broken = result(ImageSource::Pexels);
        broken.url = String::new();
        assert!(!broken.is_usable());

        let mut no_id = result(ImageSource::Pexels);
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
            let mut unsafe_result = result(ImageSource::Pexels);
            unsafe_result.url = bad.to_string();
            assert!(!unsafe_result.url_is_safe(), "{bad} should be refused");
            assert!(
                !NetworkImageChoice::from_result(&unsafe_result, None, false).is_usable(),
                "{bad} must not produce a usable choice"
            );
        }
        assert!(result(ImageSource::Pexels).url_is_safe());
        assert!(result(ImageSource::Pexels).url.starts_with("https://"));
    }

    #[test]
    fn a_search_hides_the_results_that_cannot_be_used() {
        let mut good = result(ImageSource::Pexels);
        good.source_id = "good".to_string();
        let mut bad = result(ImageSource::Pexels);
        bad.url = "javascript:alert(1)".to_string();

        let search = ImageSearch::new(ImageSource::Pexels, "demo", vec![good, bad]);
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
        assert!(is_network_image("https://images.unsplash.com/photo-1?ixid=APPID"));
        assert!(is_network_image("https://images.pexels.com/photos/1/x.png?h=650"));

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
        for source in ImageSource::ALL {
            assert!(source.needs_key());
        }
    }

    #[test]
    fn a_search_and_a_choice_round_trip_through_json() {
        let search = ImageSearch::new(
            ImageSource::Pexels,
            "demo",
            vec![result(ImageSource::Pexels)],
        );
        let json = serde_json::to_string(&search).unwrap();
        assert!(json.contains("\"source\":\"pexels\""), "{json}");
        assert_eq!(serde_json::from_str::<ImageSearch>(&json).unwrap(), search);

        let choice = NetworkImageChoice::from_result(&result(ImageSource::Pexels), Some("x"), true);
        let json = serde_json::to_string(&choice).unwrap();
        assert_eq!(
            serde_json::from_str::<NetworkImageChoice>(&json).unwrap(),
            choice
        );

        // The new optional fields survive, and are OMITTED when absent rather than serialised as
        // null, so a payload from before this round stays readable and a small payload stays small.
        let mut with_location = result(ImageSource::Unsplash);
        with_location.download_location =
            Some("https://api.unsplash.com/photos/1/download".to_string());
        let json = serde_json::to_string(&with_location).unwrap();
        assert!(json.contains("author_url"), "{json}");
        assert!(json.contains("download_location"), "{json}");

        let without = result(ImageSource::Unsplash);
        let json = serde_json::to_string(&without).unwrap();
        assert!(!json.contains("download_location"), "{json}");
        assert!(!json.contains("alt"), "{json}");

        // And a minimal payload — one written before these fields existed — still deserialises.
        let minimal = r#"{
            "source": "pexels",
            "source_id": "1",
            "url": "https://images.pexels.com/photos/1/x.png",
            "thumbnail_url": "https://images.pexels.com/photos/1/x.png?h=130",
            "width": 1,
            "height": 1,
            "author": "A"
        }"#;
        let parsed: ImageResult = serde_json::from_str(minimal).unwrap();
        assert!(parsed.author_url.is_none());
        assert!(parsed.alt.is_none());
        assert!(parsed.download_location.is_none());
    }

    #[test]
    fn a_result_keeps_the_dimensions_and_licence_the_picker_shows() {
        let full = result(ImageSource::Pexels);
        assert_eq!(full.width, 1280);
        assert_eq!(full.height, 720);
        assert_eq!(full.licence.as_deref(), Some("Example Content License"));
        assert!(full.thumbnail_url.contains("_640"), "a thumbnail avoids downloading the full size");
    }
}
