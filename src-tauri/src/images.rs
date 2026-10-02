//! The network image browser (`handoff.md` §13.14).
//!
//! §13.14's flow is *搜索 → 用户选择 → 直接写入远程 URL → Markdown*, and the model for it existed in
//! `veloct-attach::network` with **no caller above the crate**: there was no way to search a provider,
//! no way to store a key, and no way to write a chosen URL. That is PROCESSING.md §5.9's "remote image"
//! half, and this module is the wiring.
//!
//! **The sources are Unsplash and Pexels, and the third source §13.1 names is a recorded refusal
//! rather than a silent absence.** Pixabay's own terms forbid permanent hotlinking and require images to
//! be downloaded to the caller's own server, which is the opposite of what §13.14 prescribes; so
//! `handoff.md` §20's rule — never fake a success, never invent a vendor behaviour — is met by
//! reporting that, in the provider's own words, from [`network_image_sources`]. See PROCESSING.md §5.11.
//!
//! Three things this module is careful about:
//!
//! * **The key lives in the OS credential store and is read for the length of one request.** No struct
//!   here has a field for it, so it cannot reach a serialised view, a log line or a `Debug` dump. §12.2.
//! * **A key is never returned, only reported as present.** [`NetworkImageSourceView::has_key`] is a
//!   boolean, which is what §12.1 requires of the UI.
//! * **The credit line cannot be switched off where the provider's terms require it.**
//!   `veloct_attach::NetworkImageChoice::from_result` enforces that, not this module, so a second caller
//!   cannot get it wrong.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use veloct_app::CommandError;
use veloct_attach::network::providers;
use veloct_attach::{
    ImageResult, ImageSearch, ImageSource, NetworkImageChoice, SearchQuery, TopicQuery, TopicResult,
};
use veloct_core::http::{HttpClient, ReqwestHttpClient};
use veloct_core::VeloctError;

use crate::commands::command_invalid;

/// The credential handle for an image source's API key.
///
/// Delegates to [`crate::credentials::handle_for_image_source`], so the format has ONE definition: a
/// change there that this function did not follow would silently orphan every saved key.
fn handle_for(source: ImageSource) -> String {
    crate::credentials::handle_for_image_source(source.as_str())
}

/// One source as the window sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkImageSourceView {
    /// The stored spelling, exactly what the search command takes back.
    pub source: String,
    pub label: String,
    /// Whether this build can search it.
    pub implemented: bool,
    /// Why not, in the provider's own terms, when it cannot.
    pub unimplemented_reason: Option<String>,
    /// Whether a key is needed. All offered sources need one.
    pub needs_key: bool,
    /// Whether a key is STORED. Never the value (§12.1).
    pub has_key: bool,
    /// What the provider requires of a page that shows its image.
    pub attribution_rule: String,
    /// The sentence to show before a search, including what a network image costs the user.
    pub notice: String,
    /// `required`, `permitted` or `forbidden` — the provider's stance on embedding its URLs.
    pub hotlink: String,
    /// Whether leaving the credit line OUT of the document is the user's choice.
    ///
    /// The same fact the search view carries, on the source itself, so the panel can state the
    /// obligation BEFORE a search rather than only after one. Derived from the provider's terms here, in
    /// Rust, because "does Unsplash require credit" must not have a second implementation in the window.
    pub attribution_optional: bool,
    pub default_per_page: u16,
    pub max_per_page: u16,
}

fn hotlink_name(policy: providers::HotlinkPolicy) -> &'static str {
    match policy {
        providers::HotlinkPolicy::Required => "required",
        providers::HotlinkPolicy::Permitted => "permitted",
        providers::HotlinkPolicy::Forbidden => "forbidden",
    }
}

/// One source's view, built from the crate's own facts.
///
/// The `implemented` flag and the reason come from `veloct_attach`, not from a list here, so a source
/// cannot be offered by the window while the crate refuses to search it.
fn source_view(source: ImageSource) -> NetworkImageSourceView {
    let facts = providers::facts(source);
    NetworkImageSourceView {
        source: source.as_str().to_string(),
        label: source.label().to_string(),
        implemented: facts.is_ok(),
        unimplemented_reason: providers::unimplemented_reason(source),
        needs_key: source.needs_key(),
        has_key: crate::credentials::exists(&handle_for(source)),
        attribution_rule: facts
            .as_ref()
            .map(|facts| facts.attribution_rule.to_string())
            .unwrap_or_default(),
        notice: providers::source_notice(source),
        hotlink: facts
            .as_ref()
            .map(|facts| hotlink_name(facts.hotlink).to_string())
            .unwrap_or_else(|_| "forbidden".to_string()),
        attribution_optional: facts
            .as_ref()
            .map(|facts| facts.hotlink != providers::HotlinkPolicy::Required)
            .unwrap_or(false),
        default_per_page: facts.as_ref().map(|facts| facts.default_per_page).unwrap_or(0),
        max_per_page: facts.as_ref().map(|facts| facts.max_per_page).unwrap_or(0),
    }
}

/// Every source `handoff.md` §13.1's fifth target names, with its state.
///
/// Takes no `State`, like `storage_kinds`: the answer is a fact about this BUILD rather than about a
/// project, so the window can say a source has no adapter (or no key) before anything exists to
/// configure. That is what lets Pixabay's reason be shown instead of the source simply being missing.
#[tauri::command]
pub fn network_image_sources() -> Vec<NetworkImageSourceView> {
    ImageSource::ALL.into_iter().map(source_view).collect()
}

/// Store an API key for a source.
#[tauri::command]
pub fn save_image_source_key(source: String, api_key: String) -> Result<(), CommandError> {
    let source = parse_source(&source)?;
    if !source.is_implemented() {
        return Err(CommandError::from(VeloctError::Unsupported(
            providers::unimplemented_reason(source).unwrap_or_default(),
        )));
    }
    if api_key.trim().is_empty() {
        return Err(command_invalid(
            "密钥不能为空;如果要去掉已保存的密钥,请用「删除密钥」。",
        ));
    }
    crate::credentials::set(&handle_for(source), api_key.trim())?;
    Ok(())
}

/// Forget a source's stored API key.
#[tauri::command]
pub fn delete_image_source_key(source: String) -> Result<(), CommandError> {
    let source = parse_source(&source)?;
    crate::credentials::remove(&handle_for(source))?;
    Ok(())
}

/// A source string from the window, or the reason it is not one.
fn parse_source(source: &str) -> Result<ImageSource, CommandError> {
    ImageSource::parse(source).ok_or_else(|| {
        VeloctError::Invalid(format!(
            "{source:?} 不是已知的图片来源;可用来源为 {}。",
            ImageSource::ALL
                .iter()
                .map(|source| source.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .into()
    })
}

/// The search results, as the window sees them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSearchView {
    pub source: String,
    pub label: String,
    pub query: String,
    pub results: Vec<ImageResult>,
    pub has_more: bool,
    /// How many results were dropped as unusable, so a short list is explained rather than mysterious.
    pub dropped: usize,
    /// The provider's obligation, shown beside the results so the user knows what a choice commits to.
    pub attribution_rule: String,
    /// The user's own obligation flag, so the panel can say whether the credit line is optional.
    pub attribution_optional: bool,
}

/// A short-timeout client: a search is a small response, and a provider that is merely unreachable must
/// fail rather than hang the window.
fn image_client() -> Result<Arc<ReqwestHttpClient>, CommandError> {
    Ok(Arc::new(ReqwestHttpClient::with_default_timeout(
        Duration::from_secs(30),
    )?))
}

/// Search a provider for a keyword.
#[tauri::command]
pub fn search_network_images(
    source: String,
    query: String,
    page: Option<u16>,
    per_page: Option<u16>,
) -> Result<ImageSearchView, CommandError> {
    let source = parse_source(&source)?;
    let client = image_client()?;
    let key = crate::credentials::get_or_none(&handle_for(source));

    let mut search = SearchQuery::new(source, query)?;
    if let Some(page) = page {
        search.page = page;
    }
    if let Some(per_page) = per_page {
        search.per_page = per_page;
    }

    let found = search_with(&*client, source, key.as_deref(), &search)?;
    Ok(search_view(found))
}

/// The search itself, with the client and the key passed in.
///
/// Extracted so a test can drive the whole body against `FakeHttpClient` — the same reason
/// `save_target_config` and `delete_target_config` were extracted in `attachments.rs`, and the reason
/// PROCESSING.md §5.10.2 lists "a command WRAPPER that needs an `AppState`" as a limit rather than a
/// design. This one takes neither.
fn search_with(
    client: &dyn HttpClient,
    source: ImageSource,
    api_key: Option<&str>,
    query: &SearchQuery,
) -> Result<ImageSearch, CommandError> {
    let key = require_key_for(source, api_key)?;
    Ok(veloct_attach::search_images(client, source, key, query)?)
}

fn search_view(found: ImageSearch) -> ImageSearchView {
    let usable = found.usable().len();
    let dropped = found.results.len().saturating_sub(usable);
    let facts = providers::facts(found.source).ok();
    ImageSearchView {
        source: found.source.as_str().to_string(),
        label: found.source.label().to_string(),
        query: found.query,
        results: found.results,
        has_more: found.has_more,
        dropped,
        attribution_rule: facts
            .as_ref()
            .map(|facts| facts.attribution_rule.to_string())
            .unwrap_or_default(),
        // Whether putting the credit line in the DOCUMENT is the user's choice. False where the
        // provider's terms require the credit, which the choice type enforces.
        attribution_optional: facts
            .map(|facts| facts.hotlink != providers::HotlinkPolicy::Required)
            .unwrap_or(false),
    }
}

/// What a choice produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkImageChoiceView {
    pub choice: NetworkImageChoice,
    /// The Markdown to insert, credit line included when there is one.
    pub markdown: String,
    pub attribution: Option<String>,
    /// `Some(true)`/`Some(false)` for a source that REQUIRES a download report, `None` where the
    /// provider states no such obligation.
    pub download_reported: Option<bool>,
    /// Why the report did not happen, when it did not. Never a silent failure: §20 forbids it, and the
    /// image is still usable, so the user is told rather than blocked.
    pub download_note: Option<String>,
}

/// Turn a chosen result into Markdown, reporting the download where the provider requires it.
#[tauri::command]
pub fn choose_network_image(
    source: String,
    result: ImageResult,
    alt: Option<String>,
    include_attribution: bool,
) -> Result<NetworkImageChoiceView, CommandError> {
    let source = parse_source(&source)?;
    let client = image_client()?;
    let key = crate::credentials::get_or_none(&handle_for(source));
    choose_with(
        &*client,
        source,
        key.as_deref(),
        &result,
        alt.as_deref(),
        include_attribution,
    )
}

/// The choice itself, with the client and the key passed in, so a test can drive the download report.
fn choose_with(
    client: &dyn HttpClient,
    source: ImageSource,
    api_key: Option<&str>,
    result: &ImageResult,
    alt: Option<&str>,
    include_attribution: bool,
) -> Result<NetworkImageChoiceView, CommandError> {
    if result.source != source {
        return Err(command_invalid(format!(
            "这条结果来自 {},与请求的来源 {} 不一致。",
            result.source.label(),
            source.label()
        )));
    }

    // The credit rule lives in the crate, so a caller that passes `false` for a source whose terms
    // require credit still gets one.
    let choice = NetworkImageChoice::from_result(result, alt, include_attribution);

    let mut download_reported = None;
    let mut download_note = None;
    let requires_report = providers::facts(source)
        .map(|facts| facts.hotlink == providers::HotlinkPolicy::Required)
        .unwrap_or(false);

    if requires_report {
        let key = api_key.map(str::trim).filter(|key| !key.is_empty());
        match key {
            Some(key) => match veloct_attach::report_download(client, source, key, result) {
                Ok(Some(_)) => download_reported = Some(true),
                Ok(None) => download_reported = Some(false),
                Err(error) => {
                    // The image is still usable, so this is reported rather than raised: refusing the
                    // choice would lose the user's work over a statistic.
                    download_reported = Some(false);
                    download_note = Some(error.to_string());
                }
            },
            None => {
                download_reported = Some(false);
                download_note = Some(format!(
                    "{} 要求在选择后上报下载,但没有可用的密钥。",
                    source.label()
                ));
            }
        }
    }

    Ok(NetworkImageChoiceView {
        markdown: choice.markdown(),
        attribution: choice.attribution.clone(),
        choice,
        download_reported,
        download_note,
    })
}

/// One topic as the window's strip sees it.
///
/// Mirrors `veloct_attach::TopicResult`. It is a separate type rather than the crate's, for the same
/// reason [`NetworkImageSourceView`] is: the strip needs the stored spelling of the source and a
/// label beside the raw id, and the crate has no business knowing what the window calls things.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkImageTopicView {
    /// Unsplash's stable id, which `browse_network_topic_photos` takes.
    pub id: String,
    /// The addressable form, which the same command also accepts.
    pub slug: String,
    /// What the strip shows.
    pub title: String,
    /// `featured`/`visible`/`hidden`/`removed`.
    pub visibility: String,
    /// How many photos the topic holds, as the provider reports it.
    pub total_photos: u64,
    pub description: Option<String>,
}

impl From<TopicResult> for NetworkImageTopicView {
    fn from(topic: TopicResult) -> Self {
        Self {
            id: topic.id,
            slug: topic.slug,
            title: topic.title,
            visibility: topic.visibility,
            total_photos: topic.total_photos,
            description: topic.description.filter(|text| !text.trim().is_empty()),
        }
    }
}

/// The curated topics the window's strip lists.
///
/// **Owner-requested enhancement, not a clause of `handoff.md` §13.14.** §13.14 prescribes keyword
/// search; this is a second browse mode beside it, modelled on the reference implementation
/// (`halo-sigs/plugin-unsplash`), which lists topics and then files a non-empty keyword through the
/// SEARCH endpoint even while a topic is selected. That rule lives in the window
/// (`src/lib/networkImageForm.ts`), because it is a choice about which command to call.
///
/// Unsplash only: the endpoint is Unsplash's and no other source this build offers has an equivalent,
/// so a caller naming Pexels is told which provider has topic browsing rather than handed an empty
/// strip — and a source this build cannot search at all keeps the reason its own terms give.
#[tauri::command]
pub fn network_image_topics(source: String) -> Result<Vec<NetworkImageTopicView>, CommandError> {
    let source = parse_source(&source)?;
    require_unsplash_topics(source)?;
    let client = image_client()?;
    let key = crate::credentials::get_or_none(&handle_for(source));
    let topics = list_topics_with(&*client, key.as_deref())?;
    Ok(topics.into_iter().map(NetworkImageTopicView::from).collect())
}

/// The topic list itself, with the client and the key passed in, so a test can drive it.
///
/// Takes the same two arguments as [`search_with`] and for the same reason (PROCESSING.md §5.10.2):
/// the wrapper above is three lines and needs a live credential store, while everything that could be
/// wrong — the refusal, the request, the parse — is below this line and reachable.
fn list_topics_with(
    client: &dyn HttpClient,
    api_key: Option<&str>,
) -> Result<Vec<TopicResult>, CommandError> {
    veloct_attach::list_topics(
        client,
        require_key_for(ImageSource::Unsplash, api_key)?,
        &TopicQuery::first_page(),
    )
    .map_err(CommandError::from)
}

/// Browse one topic's photos.
///
/// The topic is an id or a slug, exactly as Unsplash accepts in the `topicSlug` path parameter. The
/// result is an [`ImageSearchView`] — the SAME type the keyword search returns — so the grid, the
/// paging control, the drop count and the attribution notice are one implementation rather than two.
#[tauri::command]
pub fn browse_network_topic_photos(
    source: String,
    topic: String,
    page: Option<u16>,
    per_page: Option<u16>,
) -> Result<ImageSearchView, CommandError> {
    let source = parse_source(&source)?;
    require_unsplash_topics(source)?;
    let client = image_client()?;
    let key = crate::credentials::get_or_none(&handle_for(source));

    let mut query = TopicQuery::first_page();
    if let Some(page) = page {
        query.page = page;
    }
    // The ceiling is the crate's, so a page size above Unsplash's documented maximum is refused
    // there rather than silently trimmed here.
    if let Some(per_page) = per_page {
        query.per_page = per_page;
    }

    let found = browse_topic_with(&*client, key.as_deref(), &topic, &query)?;
    Ok(search_view(found))
}

/// The topic browse itself, with the client and the key passed in, so a test can drive it.
fn browse_topic_with(
    client: &dyn HttpClient,
    api_key: Option<&str>,
    topic: &str,
    query: &TopicQuery,
) -> Result<ImageSearch, CommandError> {
    veloct_attach::browse_topic(
        client,
        require_key_for(ImageSource::Unsplash, api_key)?,
        topic,
        query,
    )
    .map_err(CommandError::from)
}

/// The key a provider call needs, or the same refusal every image command gives.
///
/// Shared so the search and the two topic commands cannot word the hint differently. The crate
/// refuses a blank key too ([`veloct_attach::network::providers::list_topics`] and friends call their
/// own guard), but deciding it HERE means a command with no key never builds an HTTP client or opens
/// the credential store twice.
fn require_key_for(source: ImageSource, api_key: Option<&str>) -> Result<&str, CommandError> {
    api_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| {
            VeloctError::not_configured(
                format!("{} 的访问密钥", source.label()),
                "在「网络图片浏览」里为这个来源填写密钥,它会存进操作系统凭据库",
            )
            .into()
        })
}

/// Whether a source has a topic list at all.
///
/// The refusal names the endpoint's owner, so a caller asking Pexels for topics is told which provider
/// has them rather than receiving an empty strip that looks like "no topics today".
fn require_unsplash_topics(source: ImageSource) -> Result<(), CommandError> {
    // `facts` is asked first so a source this build cannot search at all keeps its own reason — Pixabay
    // must be refused for the terms, not for having no topic endpoint.
    providers::facts(source)?;
    if source == ImageSource::Unsplash {
        return Ok(());
    }
    Err(VeloctError::Unsupported(format!(
        "主题浏览只有 Unsplash 提供;{} 没有对应的接口。",
        source.label()
    ))
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::FakeHttpClient;

    const UNS_KEY: &str = "unsplash-access-key-0123456789";
    const PEX_KEY: &str = "pexels-api-key-0123456789abcdefghijklmnop";

    fn unsplash_result() -> ImageResult {
        ImageResult {
            source: ImageSource::Unsplash,
            source_id: "LBI7cgq3pbM".to_string(),
            url: "https://images.unsplash.com/photo-1?ixid=APPID&w=1080".to_string(),
            thumbnail_url: "https://images.unsplash.com/photo-1?ixid=APPID&w=200".to_string(),
            width: 5245,
            height: 3497,
            author: "Gilbert Kane".to_string(),
            licence: None,
            source_page: Some("https://unsplash.com/photos/LBI7cgq3pbM".to_string()),
            author_url: Some("https://unsplash.com/@poorkane".to_string()),
            alt: Some("a man drinking coffee".to_string()),
            download_location: Some(
                "https://api.unsplash.com/photos/LBI7cgq3pbM/download".to_string(),
            ),
        }
    }

    fn pexels_result() -> ImageResult {
        ImageResult {
            source: ImageSource::Pexels,
            source_id: "3573351".to_string(),
            url: "https://images.pexels.com/photos/1/x.png?h=650&w=940".to_string(),
            thumbnail_url: "https://images.pexels.com/photos/1/x.png?h=130".to_string(),
            width: 3066,
            height: 3968,
            author: "Lukas Rodriguez".to_string(),
            licence: None,
            source_page: Some("https://www.pexels.com/photo/trees-3573351/".to_string()),
            author_url: Some("https://www.pexels.com/@lukas-rodriguez-1845331".to_string()),
            alt: Some("Trees during day".to_string()),
            download_location: None,
        }
    }

    fn unsplash_search_body() -> serde_json::Value {
        serde_json::json!({
            "total": 1,
            "total_pages": 1,
            "results": [{
                "id": "LBI7cgq3pbM",
                "width": 5245,
                "height": 3497,
                "alt_description": "a man drinking coffee",
                "urls": {
                    "regular": "https://images.unsplash.com/photo-1?ixid=APPID&w=1080",
                    "thumb": "https://images.unsplash.com/photo-1?ixid=APPID&w=200"
                },
                "user": { "name": "Gilbert Kane", "links": { "html": "https://unsplash.com/@poorkane" } },
                "links": {
                    "html": "https://unsplash.com/photos/LBI7cgq3pbM",
                    "download_location": "https://api.unsplash.com/photos/LBI7cgq3pbM/download"
                }
            }]
        })
    }

    fn pexels_search_body() -> serde_json::Value {
        serde_json::json!({
            "total_results": 1,
            "page": 1,
            "per_page": 15,
            "photos": [{
                "id": 3573351,
                "width": 3066,
                "height": 3968,
                "url": "https://www.pexels.com/photo/trees-3573351/",
                "photographer": "Lukas Rodriguez",
                "photographer_url": "https://www.pexels.com/@lukas-rodriguez-1845331",
                "alt": "Trees during day",
                "src": {
                    "large": "https://images.pexels.com/photos/1/x.png?h=650&w=940",
                    "small": "https://images.pexels.com/photos/1/x.png?h=130"
                }
            }]
        })
    }

    /// An Unsplash topic list (`components.responses.TopicFeed`): a BARE ARRAY of `Topic.Basic`.
    fn unsplash_topics_body() -> serde_json::Value {
        serde_json::json!([
            {
                "id": "bo8jQKTaE0Y",
                "slug": "wallpapers",
                "title": "Wallpapers",
                "description": "A curated list of wallpapers.",
                "visibility": "featured",
                "total_photos": 1344,
                "links": {
                    "self": "https://api.unsplash.com/topics/wallpapers",
                    "html": "https://unsplash.com/t/wallpapers",
                    "photos": "https://api.unsplash.com/topics/wallpapers/photos"
                }
            },
            {
                "id": "CDwuwXJAbEw",
                "slug": "3d-renders",
                "title": "3D Renders",
                "description": "   ",
                "visibility": "visible",
                "total_photos": 90
            }
        ])
    }

    /// One topic's photos: an `AssetFeed`, i.e. a bare array of the SAME `Asset.Basic` the search
    /// feed returns.
    fn unsplash_topic_photos_body() -> serde_json::Value {
        serde_json::json!([unsplash_search_body()["results"][0].clone()])
    }

    #[test]
    fn the_topic_list_is_the_documented_fields_and_round_trips_through_the_view_type() {
        let client = FakeHttpClient::new().on_json("/topics?", unsplash_topics_body());
        let topics = list_topics_with(&client, Some(UNS_KEY)).unwrap();
        assert_eq!(topics.len(), 2);
        let views: Vec<NetworkImageTopicView> =
            topics.into_iter().map(NetworkImageTopicView::from).collect();

        assert_eq!(views[0].id, "bo8jQKTaE0Y");
        assert_eq!(views[0].slug, "wallpapers");
        assert_eq!(views[0].title, "Wallpapers");
        assert_eq!(views[0].visibility, "featured");
        assert_eq!(views[0].total_photos, 1344);
        assert_eq!(
            views[0].description.as_deref(),
            Some("A curated list of wallpapers.")
        );
        // A description that is present but blank reads as ABSENT, so the strip does not render an
        // empty tooltip. This is the one transformation the view type performs.
        assert!(views[1].description.is_none());
        assert_eq!(views[1].visibility, "visible");

        let json = serde_json::to_string(&views[0]).unwrap();
        assert_eq!(
            serde_json::from_str::<NetworkImageTopicView>(&json).unwrap(),
            views[0]
        );

        // §12.2 again: the key went into the header, and the request the client saw never carried it
        // in the URL. The topics path is Unsplash's, so the version header is on this request too.
        assert_eq!(client.request_count(), 1);
        let recorded = client.recorded();
        assert!(!recorded[0].url.contains(UNS_KEY), "{}", recorded[0].url);
        assert!(recorded[0]
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == &format!("Client-ID {UNS_KEY}")));
        assert!(recorded[0]
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("accept-version") && value == "v1"));
    }

    #[test]
    fn a_topic_browse_returns_the_same_view_type_as_a_keyword_search() {
        // The whole point of reusing `ImageSearchView`: one grid, one paging control, one drop count
        // and one attribution notice, rather than a second implementation for the browse mode.
        let client = FakeHttpClient::new()
            .on_json("/topics/wallpapers/photos", unsplash_topic_photos_body());
        let found = browse_topic_with(&client, Some(UNS_KEY), "wallpapers", &TopicQuery::first_page())
            .unwrap();
        let view = search_view(found);

        assert_eq!(view.source, "unsplash");
        assert_eq!(view.label, "Unsplash");
        assert_eq!(view.query, "wallpapers", "the topic stands in for the keyword");
        assert_eq!(view.results.len(), 1);
        assert_eq!(view.dropped, 0);
        assert!(
            !view.attribution_optional,
            "a topic pick carries the SAME credit obligation as a searched one"
        );
        assert_eq!(
            view.results[0].download_location.as_deref(),
            Some("https://api.unsplash.com/photos/LBI7cgq3pbM/download"),
            "so the download report is still possible from a topic result"
        );

        let recorded = client.recorded();
        assert_eq!(recorded.len(), 1);
        assert!(
            recorded[0].url.contains("/topics/wallpapers/photos"),
            "{}",
            recorded[0].url
        );
        assert!(!recorded[0].url.contains(UNS_KEY), "{}", recorded[0].url);
    }

    #[test]
    fn a_topic_call_without_a_key_sends_nothing_and_says_where_the_key_goes() {
        let client = FakeHttpClient::new();
        for key in [None, Some("   ")] {
            let error = list_topics_with(&client, key).unwrap_err();
            assert_eq!(error.code, "NOT_CONFIGURED");
            assert!(error.hint.is_some(), "the hint says where the key goes");

            let error = browse_topic_with(&client, key, "wallpapers", &TopicQuery::first_page())
                .unwrap_err();
            assert_eq!(error.code, "NOT_CONFIGURED");
        }
        assert_eq!(client.request_count(), 0, "nothing is sent without a key");
    }

    #[test]
    fn topics_belong_to_unsplash_and_another_source_is_told_so_rather_than_given_an_empty_strip() {
        // An empty strip would read as "no topics today". Pexels has no topic endpoint and is told
        // which provider has one.
        let error = require_unsplash_topics(ImageSource::Pexels).unwrap_err();
        assert!(error.message.contains("Unsplash"), "{}", error.message);
        assert!(error.message.contains("Pexels"), "{}", error.message);
        assert_eq!(error.code, "UNSUPPORTED");

        // And a source this build cannot search AT ALL keeps its own reason — Pixabay must be refused
        // for its terms, not for lacking a topic endpoint.
        let error = require_unsplash_topics(ImageSource::Pixabay).unwrap_err();
        assert!(error.message.contains("热链"), "{}", error.message);

        assert!(require_unsplash_topics(ImageSource::Unsplash).is_ok());
    }

    #[test]
    fn a_topic_page_size_above_the_providers_ceiling_is_refused_rather_than_trimmed() {
        // The reference implementation asks for 48, which is above Unsplash's documented maximum of
        // 30. The refusal is the crate's, and it names the number the provider allows.
        let client = FakeHttpClient::new();
        let mut query = TopicQuery::first_page();
        query.per_page = 48;
        let error = browse_topic_with(&client, Some(UNS_KEY), "wallpapers", &query).unwrap_err();
        assert!(error.message.contains("30"), "{}", error.message);
        assert_eq!(client.request_count(), 0, "refused before anything is sent");

        query.page = 0;
        assert!(browse_topic_with(&client, Some(UNS_KEY), "wallpapers", &query).is_err());
        assert_eq!(client.request_count(), 0);
    }

    #[test]
    fn an_unknown_topic_is_refused_by_the_crate_rather_than_requested() {
        let client = FakeHttpClient::new();
        let error =
            browse_topic_with(&client, Some(UNS_KEY), "   ", &TopicQuery::first_page()).unwrap_err();
        assert!(error.message.contains("主题"), "{}", error.message);
        assert_eq!(client.request_count(), 0, "an empty topic is not a request");
    }

    #[test]
    fn the_topic_commands_are_registered_with_the_window() {
        // The commands are three lines each and need a live credential store, so the wrapper half is
        // asserted by registration rather than by a call — the same limit PROCESSING.md §5.10.2
        // records for the other state-needing wrappers, and the same assertion
        // `attachments::the_remote_cleanup_command_is_registered_with_the_window` makes.
        let registry = include_str!("lib.rs");
        assert!(
            registry.contains("images::network_image_topics"),
            "network_image_topics must be in the invoke_handler list"
        );
        assert!(
            registry.contains("images::browse_network_topic_photos"),
            "browse_network_topic_photos must be in the invoke_handler list"
        );
    }

    #[test]
    fn every_source_is_reported_with_its_state_and_the_two_offered_ones_are_named() {
        let _store = crate::credentials::store_guard();
        let views = network_image_sources();
        assert_eq!(views.len(), ImageSource::ALL.len());

        let offered: Vec<&str> = views
            .iter()
            .filter(|view| view.implemented)
            .map(|view| view.source.as_str())
            .collect();
        assert_eq!(offered, vec!["unsplash", "pexels"]);

        // Pixabay is REPORTED rather than missing, in the provider's own terms.
        let pixabay = views
            .iter()
            .find(|view| view.source == "pixabay")
            .expect("Pixabay is listed so its absence is explained");
        assert!(!pixabay.implemented);
        let reason = pixabay.unimplemented_reason.clone().expect("a reason");
        assert!(reason.contains("热链"), "{reason}");
        assert_eq!(pixabay.hotlink, "forbidden");

        for view in views.iter().filter(|view| view.implemented) {
            assert!(view.needs_key);
            assert!(!view.attribution_rule.trim().is_empty(), "{}", view.source);
            assert!(!view.notice.trim().is_empty(), "{}", view.source);
            assert!(view.max_per_page >= view.default_per_page);
            assert!(view.default_per_page > 0);
        }

        // The two providers differ in what they demand, and the window is told which is which.
        let unsplash = views.iter().find(|view| view.source == "unsplash").unwrap();
        assert_eq!(unsplash.hotlink, "required");
        assert!(
            !unsplash.attribution_optional,
            "Unsplash requires the credit, so the window must not offer to omit it"
        );
        let pexels = views.iter().find(|view| view.source == "pexels").unwrap();
        assert_eq!(pexels.hotlink, "permitted");
        assert!(pexels.attribution_optional, "Pexels leaves the credit to the user");
        // A refused source has no terms to honour, so it must not claim the credit is optional.
        assert!(!pixabay.attribution_optional);
    }

    #[test]
    fn a_plain_text_key_never_appears_in_a_source_view() {
        let _store = crate::credentials::store_guard();
        // §12.1: the window reports whether a key exists, not what it is. The view type has no field
        // for a value, and this asserts the serialised shape rather than trusting that.
        let json = serde_json::to_string(&network_image_sources()).unwrap();
        assert!(json.contains("has_key"), "{json}");
        for forbidden in ["api_key", "secret", "token", "credential\""] {
            assert!(!json.contains(forbidden), "a source view must not carry {forbidden}");
        }
    }

    #[test]
    fn an_unknown_source_is_refused_and_the_message_names_the_available_ones() {
        let error = parse_source("shutterstock").unwrap_err();
        assert!(error.message.contains("unsplash"), "{}", error.message);
        assert!(error.message.contains("pexels"), "{}", error.message);
        assert_eq!(error.code, "INVALID_INPUT");
    }

    #[test]
    fn a_key_cannot_be_saved_for_a_source_this_build_cannot_search() {
        // Storing a Pixabay key would suggest the source works.
        let error = save_image_source_key("pixabay".to_string(), "some-key".to_string()).unwrap_err();
        assert!(error.message.contains("热链"), "{}", error.message);
        assert_eq!(error.code, "UNSUPPORTED");
    }

    #[test]
    fn an_empty_key_is_refused_and_says_to_use_the_delete_command() {
        let error =
            save_image_source_key("pexels".to_string(), "   ".to_string()).unwrap_err();
        assert!(error.message.contains("删除密钥"), "{}", error.message);
    }

    #[test]
    fn a_key_round_trips_through_the_os_store_and_a_source_reports_that_it_exists() {
        let _store = crate::credentials::store_guard();
        // The value is never read back out through a view, so the assertion is on `has_key`.
        let handle = handle_for(ImageSource::Pexels);
        let _ = crate::credentials::remove(&handle);
        assert!(!crate::credentials::exists(&handle));
        assert!(!view_for("pexels").has_key);

        save_image_source_key("pexels".to_string(), "a-real-looking-key-0123456789".to_string())
            .unwrap();
        assert!(crate::credentials::exists(&handle), "the OS store holds it");
        assert!(view_for("pexels").has_key, "and the view reports that it does");

        delete_image_source_key("pexels".to_string()).unwrap();
        assert!(!crate::credentials::exists(&handle));
        assert!(!view_for("pexels").has_key);
        // Removing one that is already gone is not an error.
        delete_image_source_key("pexels".to_string()).unwrap();
    }

    fn view_for(source: &str) -> NetworkImageSourceView {
        network_image_sources()
            .into_iter()
            .find(|view| view.source == source)
            .unwrap_or_else(|| panic!("{source} is listed"))
    }

    #[test]
    fn a_search_without_a_stored_key_is_reported_with_a_hint_and_sends_nothing() {
        let client = FakeHttpClient::new();
        let query = SearchQuery::new(ImageSource::Unsplash, "coffee").unwrap();
        let error = search_with(&client, ImageSource::Unsplash, None, &query).unwrap_err();
        assert_eq!(error.code, "NOT_CONFIGURED");
        assert!(error.hint.is_some(), "the hint says where the key goes");
        assert_eq!(client.request_count(), 0);

        // A blank string is not a key either.
        let error = search_with(&client, ImageSource::Unsplash, Some("   "), &query).unwrap_err();
        assert_eq!(error.code, "NOT_CONFIGURED");
        assert_eq!(client.request_count(), 0);
    }

    #[test]
    fn a_search_returns_the_results_and_reports_how_many_were_dropped() {
        let client = FakeHttpClient::new().on_json("/search/photos", unsplash_search_body());
        let query = SearchQuery::new(ImageSource::Unsplash, "coffee").unwrap();
        let found = search_with(&client, ImageSource::Unsplash, Some(UNS_KEY), &query).unwrap();
        let view = search_view(found);

        assert_eq!(view.source, "unsplash");
        assert_eq!(view.label, "Unsplash");
        assert_eq!(view.results.len(), 1);
        assert_eq!(view.dropped, 0);
        assert!(!view.attribution_rule.is_empty());
        assert!(
            !view.attribution_optional,
            "Unsplash requires the credit, so the window must not offer to omit it"
        );

        let pexels = FakeHttpClient::new().on_json("/v1/search", pexels_search_body());
        let query = SearchQuery::new(ImageSource::Pexels, "trees").unwrap();
        let view = search_view(search_with(&pexels, ImageSource::Pexels, Some(PEX_KEY), &query).unwrap());
        assert!(view.attribution_optional, "Pexels leaves the credit line to the user");
    }

    #[test]
    fn a_search_passes_the_key_in_the_header_and_never_in_the_url() {
        let client = FakeHttpClient::new().on_json("/search/photos", unsplash_search_body());
        let query = SearchQuery::new(ImageSource::Unsplash, "coffee").unwrap();
        search_with(&client, ImageSource::Unsplash, Some(UNS_KEY), &query).unwrap();

        let recorded = client.recorded();
        assert_eq!(recorded.len(), 1);
        assert!(!recorded[0].url.contains(UNS_KEY), "{}", recorded[0].url);
        assert!(recorded[0]
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == &format!("Client-ID {UNS_KEY}")));
    }

    #[test]
    fn choosing_an_unsplash_image_reports_the_download_as_their_guidelines_require() {
        // Their API Guidelines, technical rule 2: picking an image for an article is the case that MUST
        // send this request. It is a compliance step, not a statistic, so it is asserted.
        let client = FakeHttpClient::new().on_json(
            "/photos/LBI7cgq3pbM/download",
            serde_json::json!({ "url": "https://images.unsplash.com/photo-1?force=true" }),
        );

        let view = choose_with(
            &client,
            ImageSource::Unsplash,
            Some(UNS_KEY),
            &unsplash_result(),
            None,
            false,
        )
        .unwrap();

        assert_eq!(view.download_reported, Some(true));
        assert!(view.download_note.is_none());
        let recorded = client.recorded();
        assert_eq!(recorded.len(), 1, "exactly the one report");
        assert_eq!(
            recorded[0].url,
            "https://api.unsplash.com/photos/LBI7cgq3pbM/download"
        );
        assert!(recorded[0]
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == &format!("Client-ID {UNS_KEY}")));
    }

    #[test]
    fn choosing_an_unsplash_image_cannot_drop_the_credit_line() {
        // The flag is the user's preference for a source whose terms make it optional; Unsplash's do not.
        let client = FakeHttpClient::new().always(200, "{}");
        let view = choose_with(
            &client,
            ImageSource::Unsplash,
            Some(UNS_KEY),
            &unsplash_result(),
            None,
            false,
        )
        .unwrap();

        let attribution = view.attribution.clone().expect("Unsplash requires a credit");
        assert!(attribution.contains("Gilbert Kane"), "{attribution}");
        assert!(attribution.contains("utm_source=veloct"), "{attribution}");
        assert!(view.markdown.contains("utm_source=veloct"), "{}", view.markdown);
    }

    #[test]
    fn choosing_a_pexels_image_sends_nothing_extra_and_honours_the_flag() {
        // Pexels states no download-reporting obligation, so there is no request to make.
        let client = FakeHttpClient::new();
        let chosen = choose_with(
            &client,
            ImageSource::Pexels,
            Some(PEX_KEY),
            &pexels_result(),
            None,
            true,
        )
        .unwrap();
        assert_eq!(client.request_count(), 0, "Pexels has no report step");
        assert_eq!(chosen.download_reported, None);
        let attribution = chosen.attribution.expect("a credit was asked for");
        assert!(attribution.contains("Lukas Rodriguez"), "{attribution}");
        assert!(attribution.contains("pexels.com"), "{attribution}");

        let without = choose_with(
            &client,
            ImageSource::Pexels,
            Some(PEX_KEY),
            &pexels_result(),
            None,
            false,
        )
        .unwrap();
        assert!(without.attribution.is_none());
        assert_eq!(client.request_count(), 0);
    }

    #[test]
    fn a_failed_download_report_is_stated_and_does_not_lose_the_users_image() {
        // §20: an explicit, explained failure rather than a silent one. Refusing the choice would throw
        // away the user's work over a statistic the image does not depend on.
        let client = FakeHttpClient::new().always(500, "upstream boom");
        let view = choose_with(
            &client,
            ImageSource::Unsplash,
            Some(UNS_KEY),
            &unsplash_result(),
            None,
            true,
        )
        .unwrap();

        assert_eq!(view.download_reported, Some(false));
        let note = view.download_note.clone().expect("the failure is explained");
        assert!(note.contains("500"), "{note}");
        assert!(view.markdown.contains("images.unsplash.com"), "the Markdown is still produced");
        assert!(view.attribution.is_some());
    }

    #[test]
    fn a_download_report_without_a_stored_key_is_reported_rather_than_attempted() {
        let client = FakeHttpClient::new();
        let view = choose_with(
            &client,
            ImageSource::Unsplash,
            None,
            &unsplash_result(),
            None,
            true,
        )
        .unwrap();
        assert_eq!(view.download_reported, Some(false));
        assert!(view
            .download_note
            .as_deref()
            .unwrap_or_default()
            .contains("密钥"));
        assert_eq!(client.request_count(), 0);
    }

    #[test]
    fn a_result_from_one_source_cannot_be_chosen_as_another() {
        let client = FakeHttpClient::new();
        let error = choose_with(
            &client,
            ImageSource::Pexels,
            Some(PEX_KEY),
            &unsplash_result(),
            None,
            true,
        )
        .unwrap_err();
        assert!(error.message.contains("Unsplash"), "{}", error.message);
        assert_eq!(client.request_count(), 0);
    }

    #[test]
    fn the_chosen_markdown_carries_the_remote_url_and_the_alt() {
        let client = FakeHttpClient::new().always(200, "{}");
        let view = choose_with(
            &client,
            ImageSource::Pexels,
            Some(PEX_KEY),
            &pexels_result(),
            Some("My own alt"),
            true,
        )
        .unwrap();
        assert!(view.markdown.starts_with("![My own alt](https://images.pexels.com/"), "{}", view.markdown);

        // With no alt from the user, the provider's own description is used.
        let view = choose_with(&client, ImageSource::Pexels, Some(PEX_KEY), &pexels_result(), None, false)
            .unwrap();
        assert!(view.markdown.starts_with("![Trees during day]"), "{}", view.markdown);
    }

    #[test]
    fn the_pixabay_reason_survives_a_round_trip_through_the_view_type() {
        let _store = crate::credentials::store_guard();
        // The window shows this sentence, so it must be the provider's own words and not a placeholder.
        let views = network_image_sources();
        let pixabay = views.iter().find(|view| view.source == "pixabay").unwrap();
        let json = serde_json::to_string(pixabay).unwrap();
        let parsed: NetworkImageSourceView = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, *pixabay);
        assert!(parsed.unimplemented_reason.unwrap().contains("热链"));
        // And its notice is the refusal, not an instruction to configure something.
        assert!(pixabay.notice.contains("Unsplash"), "{}", pixabay.notice);
    }
}
