//! The two providers this build can search, and what their own documentation says about each.
//!
//! **Every URL, header, parameter, response field and obligation in this file was READ FROM the
//! provider's own documentation or published OpenAPI specification on 2026-10-01, and the document is
//! named beside the rule.** That matters more here than anywhere else in this crate: a stock-photo API
//! is external, unversioned data, and `handoff.md` §20 forbids inventing vendor behaviour. The same
//! facts are recorded for the next agent in `docs/spec/03-upstream-facts.md`.
//!
//! # Why these two sources and not the one the contract names
//!
//! `handoff.md` §13.14 prescribes *search → the user picks → write the remote URL straight into the
//! Markdown*, and §13.1's parenthetical names Pixabay. Reading Pixabay's documentation showed that its
//! terms **forbid exactly that**: "permanent hotlinking of images (using Pixabay URLs in your app) is
//! not allowed. If you intend to use the images, please download them to your server first."
//!
//! So the clause is implementable, but not with that source. The two providers below are the ones
//! whose terms make §13.14's flow correct or permitted, and the difference between them is the
//! interesting part:
//!
//! | | Hotlinking | Obligation |
//! |---|---|---|
//! | Unsplash | **required** — re-hosting is not allowed | attribute Unsplash AND the photographer, with links, and report a download when the user picks |
//! | Pexels | permitted | a prominent link to Pexels, and credit the photographer with a link to the photo page |
//!
//! `PROCESSING.md` §5.11 is the decision record; `ImageSource::Pixabay` carries the reason it is not
//! offered, so the absence is stated rather than silent.
//!
//! # What this module deliberately does NOT do
//!
//! It holds no API key and performs no I/O of its own beyond the caller's [`HttpClient`]. A key is
//! passed in for the length of one request, which is what keeps it out of every struct in the crate
//! and therefore out of a `Debug` dump, a serialised view or a log line.

use serde::Deserialize;

use veloct_core::http::{HttpClient, HttpRequest, HttpResponse};
use veloct_core::{Result, VeloctError};

use super::{ImageResult, ImageSearch, ImageSource};

// ---------------------------------------------------------------------------------------------
// Unsplash — from https://unsplash.com/documentation and the OpenAPI spec at
// https://unsplash.com/spec/v1.json (servers[0].url, paths./search/photos,
// responses.SearchAssetFeed, schemas.Asset.Basic / Asset.VeryBasic / Asset.Urls,
// securitySchemes.Auth).
// ---------------------------------------------------------------------------------------------

/// Unsplash's API host (OpenAPI `servers[0].url`).
pub const UNSPLASH_API: &str = "https://api.unsplash.com";

/// Unsplash's photo search path (OpenAPI `paths./search/photos`).
pub const UNSPLASH_SEARCH_PATH: &str = "/search/photos";

/// The version header Unsplash asks every request to carry (documentation, "Version").
pub const UNSPLASH_ACCEPT_VERSION: &str = "v1";

/// Unsplash's `per_page` ceiling (documentation, "Pagination": "up to a maximum of 30").
pub const UNSPLASH_MAX_PER_PAGE: u16 = 30;

/// Unsplash's `per_page` default (OpenAPI `parameters.perPage`: `default: 10`).
pub const UNSPLASH_DEFAULT_PER_PAGE: u16 = 10;

// ---------------------------------------------------------------------------------------------
// Pexels — from https://www.pexels.com/api/documentation/ (Authorization, the Photo Resource,
// Search for Photos, Guidelines, Request Statistics).
// ---------------------------------------------------------------------------------------------

/// Pexels's API host (documentation, "Authorization").
pub const PEXELS_API: &str = "https://api.pexels.com";

/// Pexels's photo search path (documentation, "Search for Photos").
pub const PEXELS_SEARCH_PATH: &str = "/v1/search";

/// Pexels's `per_page` ceiling (documentation: "Default: 15, Max: 80").
pub const PEXELS_MAX_PER_PAGE: u16 = 80;

/// Pexels's `per_page` default (documentation: "Default: 15").
pub const PEXELS_DEFAULT_PER_PAGE: u16 = 15;

/// The `utm_source` value Unsplash's attribution rule requires on every link back to them.
///
/// Their API Guidelines say links "should use utm parameters in the
/// `?utm_source=your_app_name&utm_medium=referral`", so this is the application's name in that slot.
pub const UNSPLASH_UTM_SOURCE: &str = "veloct";

/// The Unsplash link every attribution must carry, with the required `utm` parameters.
pub const UNSPLASH_ATTRIBUTION_LINK: &str =
    "https://unsplash.com/?utm_source=veloct&utm_medium=referral";

/// How a provider wants its key presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthScheme {
    /// `Authorization: Client-ID <key>` (Unsplash OpenAPI `securitySchemes.Auth`: "Value should be in
    /// the format 'Client-ID your-client-id' or 'Bearer your-bearer-token'").
    ClientId,
    /// A bare `Authorization: <key>` (Pexels documentation: `Authorization: YOUR_API_KEY`).
    RawKey,
}

/// What a provider's terms say about embedding its image URLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotlinkPolicy {
    /// The provider REQUIRES its own URLs to be embedded and does not allow re-hosting.
    ///
    /// Unsplash: "Unlike most APIs, we **require** the image URLs returned by the API to be directly
    /// used or embedded in your applications", and their guidelines add that this "applies to all uses
    /// of the image and not just search results".
    Required,
    /// The provider permits embedding its URLs.
    ///
    /// Pexels's documentation prescribes no download step, its photos are served from its own CDN, and
    /// its stated obligations are attribution and a link back.
    Permitted,
    /// The provider forbids embedding, so this build does not offer it as a source.
    Forbidden,
}

/// One provider's published facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderFacts {
    pub source: ImageSource,
    pub api: &'static str,
    pub search_path: &'static str,
    pub auth: AuthScheme,
    pub default_per_page: u16,
    pub max_per_page: u16,
    pub hotlink: HotlinkPolicy,
    /// The provider's own words for what showing its image obliges the page to do.
    pub attribution_rule: &'static str,
    /// Which document the facts above were read from, so a later change can be checked against it.
    pub source_of_truth: &'static str,
}

/// The facts for a source, or the reason this build cannot search it.
pub fn facts(source: ImageSource) -> Result<ProviderFacts> {
    let facts = match source {
        ImageSource::Unsplash => ProviderFacts {
            source,
            api: UNSPLASH_API,
            search_path: UNSPLASH_SEARCH_PATH,
            auth: AuthScheme::ClientId,
            default_per_page: UNSPLASH_DEFAULT_PER_PAGE,
            max_per_page: UNSPLASH_MAX_PER_PAGE,
            hotlink: HotlinkPolicy::Required,
            attribution_rule:
                "必须注明 Unsplash 与摄影师,并链接回摄影师的 Unsplash 主页;所有指向 Unsplash 的链接\
                 需带 utm 参数。用户选定图片后还必须请求一次下载接口上报。",
            source_of_truth: "https://unsplash.com/documentation + https://unsplash.com/spec/v1.json",
        },
        ImageSource::Pexels => ProviderFacts {
            source,
            api: PEXELS_API,
            search_path: PEXELS_SEARCH_PATH,
            auth: AuthScheme::RawKey,
            default_per_page: PEXELS_DEFAULT_PER_PAGE,
            max_per_page: PEXELS_MAX_PER_PAGE,
            hotlink: HotlinkPolicy::Permitted,
            attribution_rule:
                "必须显著标明来源 Pexels,并尽可能注明摄影师、链接到图片页。",
            source_of_truth: "https://www.pexels.com/api/documentation/",
        },
        ImageSource::Pixabay => {
            return Err(VeloctError::Unsupported(
                "Pixabay 不接入:它的条款禁止永久热链(「using Pixabay URLs in your app) is not \
                 allowed」,要求先下载到自己的服务器),与本功能写入远程 URL 的方式冲突。\
                 可用来源为 Unsplash 与 Pexels。"
                    .to_string(),
            ))
        }
    };
    Ok(facts)
}

/// Whether this build can search a source.
pub fn is_implemented(source: ImageSource) -> bool {
    facts(source).is_ok()
}

/// Why not, in the provider's own terms, when this build cannot search a source.
pub fn unimplemented_reason(source: ImageSource) -> Option<String> {
    match facts(source) {
        Ok(_) => None,
        Err(error) => Some(error.to_string()),
    }
}

/// Percent-encode one query VALUE.
///
/// Delegated rather than reimplemented: this is RFC 3986, which is the same rule AWS's `UriEncode()`
/// states, so the workspace keeps ONE implementation of "how to percent-encode a UTF-8 string".
/// `encode_slash` is `true` because a search term is a value and not a path — a `/` inside a query
/// must be encoded, where a `/` inside an object key must not.
fn encode_value(value: &str) -> String {
    veloct_s3::uri_encode(value, true)
}

/// One search request's inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub page: u16,
    pub per_page: u16,
}

impl SearchQuery {
    /// A first-page query with the provider's own default page size.
    pub fn new(source: ImageSource, text: impl Into<String>) -> Result<Self> {
        let facts = facts(source)?;
        Ok(Self {
            text: text.into(),
            page: 1,
            per_page: facts.default_per_page,
        })
    }
}

/// Build the search request for a source.
///
/// The key travels in the `Authorization` header and **never in the URL**. Both providers accept a
/// query-parameter form, and using it would put the credential into every diagnostic that prints a
/// URL — including [`veloct_core::http::redact_url`], which redacts a known query parameter but is one
/// more place that has to remember. A header is not printed by anything in this crate.
pub fn search_request(
    source: ImageSource,
    api_key: &str,
    query: &SearchQuery,
) -> Result<HttpRequest> {
    let facts = facts(source)?;
    let term = query.text.trim();
    if term.is_empty() {
        return Err(VeloctError::Invalid(
            "请输入要搜索的关键词。".to_string(),
        ));
    }
    if query.page == 0 {
        return Err(VeloctError::Invalid(
            "page 从 1 开始。".to_string(),
        ));
    }
    if query.per_page == 0 || query.per_page > facts.max_per_page {
        return Err(VeloctError::Invalid(format!(
            "{} 每页最多返回 {} 条。",
            source.label(),
            facts.max_per_page
        )));
    }

    let url = format!(
        "{}{}?query={}&page={}&per_page={}",
        facts.api,
        facts.search_path,
        encode_value(term),
        query.page,
        query.per_page
    );

    let request = match facts.auth {
        AuthScheme::ClientId => HttpRequest::get(url)
            .header("Authorization", format!("Client-ID {api_key}"))
            // Unsplash asks every request to state the version, so a future v2 default cannot change
            // what this parses without a change here.
            .header("Accept-Version", UNSPLASH_ACCEPT_VERSION),
        AuthScheme::RawKey => HttpRequest::get(url).header("Authorization", api_key),
    };

    Ok(request)
}

/// A provider failure, with the credential redacted **where it is known**.
///
/// A provider's error body can echo the key back — Unsplash answers an invalid key with a JSON body,
/// and Pexels with a plain-text one. Neither key has a pattern the generic layer could recognise (an
/// Unsplash access key and a Pexels key are both opaque strings), so the exact-value layer is the only
/// one that can catch it. `PROCESSING.md` §2.8 is the round that learned this the hard way.
fn provider_failure(source: ImageSource, response: &HttpResponse, api_key: &str) -> VeloctError {
    let body = veloct_core::secret::redact_with_known(&response.text(), &[api_key]);
    let excerpt: String = body.chars().take(300).collect();
    VeloctError::Attachment(format!(
        "{} 返回 HTTP {}:{}",
        source.label(),
        response.status,
        if excerpt.trim().is_empty() {
            String::new()
        } else {
            format!(" {excerpt}")
        }
    ))
}

// ---------------------------------------------------------------------------------------------
// Response shapes. Field names come from the two documents named above; `#[serde(default)]` is used
// only where a field is not needed for the URL or the attribution, so a provider adding or dropping an
// optional field cannot break a search.
// ---------------------------------------------------------------------------------------------

/// Unsplash's `SearchAssetFeed`: `{ total, total_pages, results }` — all three are `required` in the
/// spec, so none of them is defaulted here.
#[derive(Debug, Deserialize)]
struct UnsplashSearch {
    total_pages: u64,
    results: Vec<UnsplashPhoto>,
}

/// Unsplash's `Asset.Basic` (the fields this crate needs).
#[derive(Debug, Deserialize)]
struct UnsplashPhoto {
    id: String,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    alt_description: Option<String>,
    /// `Asset.VeryBasic.urls`, `required` in the spec: the URL the Markdown must carry.
    urls: UnsplashUrls,
    #[serde(default)]
    user: Option<UnsplashUser>,
    #[serde(default)]
    links: Option<UnsplashLinks>,
}

/// Unsplash's `Asset.Urls` — all five are `required` and every one carries the `ixid` parameter that
/// their guidelines say must be kept. Only the two this crate uses are read: `regular` is the
/// documented size for content and `thumb` is small enough for a result grid. An unread field is not
/// an error, because serde ignores what is not named.
#[derive(Debug, Deserialize)]
struct UnsplashUrls {
    regular: String,
    thumb: String,
}

#[derive(Debug, Deserialize)]
struct UnsplashUser {
    name: String,
    #[serde(default)]
    links: Option<UnsplashLinks>,
}

#[derive(Debug, Deserialize)]
struct UnsplashLinks {
    #[serde(default)]
    html: String,
    /// The download-report endpoint their guideline 2 requires a caller to hit when a user chooses an
    /// image. `required` in `Asset.Basic.links`.
    #[serde(default)]
    download_location: String,
}

/// Pexels's search response: `{ total_results, page, per_page, photos, next_page?, prev_page? }`.
#[derive(Debug, Deserialize)]
struct PexelsSearch {
    #[serde(default)]
    next_page: Option<String>,
    photos: Vec<PexelsPhoto>,
}

/// Pexels's `Photo` resource (the fields this crate needs).
#[derive(Debug, Deserialize)]
struct PexelsPhoto {
    id: u64,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    /// "The Pexels URL where the photo is located" — the page attribution must link to.
    #[serde(default)]
    url: String,
    #[serde(default)]
    photographer: String,
    #[serde(default)]
    photographer_url: Option<String>,
    #[serde(default)]
    alt: Option<String>,
    src: PexelsSrc,
}

/// Pexels's `src` object. `large` is 940×650 at DPR 1 and `small` has a height of 130; both are used
/// verbatim, which is what keeps the CDN's own sizing intact.
#[derive(Debug, Deserialize)]
struct PexelsSrc {
    #[serde(default)]
    original: String,
    #[serde(default)]
    large: String,
    #[serde(default)]
    medium: String,
    #[serde(default)]
    small: String,
    #[serde(default)]
    tiny: String,
}

/// The first non-empty candidate.
fn first_non_empty(candidates: &[&String]) -> String {
    candidates
        .iter()
        .find(|candidate| !candidate.trim().is_empty())
        .map(|candidate| candidate.to_string())
        .unwrap_or_default()
}

/// Parse a search response into the crate's own shape.
pub fn parse_search(
    source: ImageSource,
    query: &str,
    response: &HttpResponse,
) -> Result<ImageSearch> {
    // The guard, so a source this build cannot search is refused with the provider's own reason rather
    // than reaching an arm below.
    facts(source)?;

    match source {
        ImageSource::Unsplash => {
            let parsed: UnsplashSearch = serde_json::from_slice(&response.body).map_err(|error| {
                VeloctError::Attachment(format!(
                    "{} 的搜索响应不是预期的 JSON: {error}",
                    source.label()
                ))
            })?;
            let mut search = ImageSearch::new(
                source,
                query,
                parsed.results.into_iter().map(unsplash_result).collect(),
            );
            // `total_pages` is `required` in the spec, so `has_more` is the document's own answer
            // rather than a guess from the number of results returned.
            search.has_more = 1 < parsed.total_pages;
            Ok(search)
        }
        ImageSource::Pexels => {
            let parsed: PexelsSearch = serde_json::from_slice(&response.body).map_err(|error| {
                VeloctError::Attachment(format!(
                    "{} 的搜索响应不是预期的 JSON: {error}",
                    source.label()
                ))
            })?;
            let mut search = ImageSearch::new(
                source,
                query,
                parsed.photos.into_iter().map(pexels_result).collect(),
            );
            // Pexels states the next page as a URL and only when there is one, so its presence IS the
            // answer.
            search.has_more = parsed.next_page.is_some();
            Ok(search)
        }
        // Dead in practice — `facts` refused it above — but written as an exhaustive arm rather than a
        // wildcard so that ADDING a source is a compile error here instead of a source that silently
        // searches nothing.
        ImageSource::Pixabay => Err(VeloctError::Unsupported(
            unimplemented_reason(source).unwrap_or_default(),
        )),
    }
}

fn unsplash_result(photo: UnsplashPhoto) -> ImageResult {
    let author = photo
        .user
        .as_ref()
        .map(|user| user.name.clone())
        .unwrap_or_default();
    let author_url = photo
        .user
        .as_ref()
        .and_then(|user| user.links.as_ref())
        .map(|links| links.html.clone())
        .filter(|html| !html.trim().is_empty());
    let source_page = photo
        .links
        .as_ref()
        .map(|links| links.html.clone())
        .filter(|html| !html.trim().is_empty());
    let download_location = photo
        .links
        .as_ref()
        .map(|links| links.download_location.clone())
        .filter(|location| !location.trim().is_empty());
    // The provider's OWN alt text, which is better than anything the caller could invent, and is what
    // makes an image accessible without the user typing a description.
    let alt = photo
        .alt_description
        .clone()
        .filter(|text| !text.trim().is_empty())
        .or_else(|| photo.description.clone().filter(|text| !text.trim().is_empty()));

    ImageResult {
        source: ImageSource::Unsplash,
        source_id: photo.id,
        // `regular` is 1080 px wide, which the documentation recommends for content; `full` is
        // explicitly discouraged for performance. The URL is copied VERBATIM so its `ixid` parameter
        // survives — their guidelines make keeping it a condition of the API's use.
        url: photo.urls.regular,
        // `thumb` is 200 px: small enough that a result grid does not download twenty full images.
        thumbnail_url: photo.urls.thumb.clone(),
        width: photo.width,
        height: photo.height,
        author,
        author_url,
        // Unsplash's own license name is not stated in the documents read here, so this stays `None`
        // rather than asserting a name that was not verified. The obligation is carried by
        // `attribution_rule` and rendered by `attribution_markdown`.
        licence: None,
        source_page,
        alt,
        download_location,
    }
}

fn pexels_result(photo: PexelsPhoto) -> ImageResult {
    // `large` is 940×650 and is the documented size for display; the fallbacks exist because a photo
    // whose `large` rendition is absent must still produce a usable result rather than an empty URL.
    let url = first_non_empty(&[&photo.src.large, &photo.src.original, &photo.src.medium]);
    let thumbnail = first_non_empty(&[&photo.src.small, &photo.src.tiny, &photo.src.medium]);

    ImageResult {
        source: ImageSource::Pexels,
        source_id: photo.id.to_string(),
        thumbnail_url: thumbnail,
        url,
        width: photo.width,
        height: photo.height,
        author: photo.photographer,
        author_url: photo.photographer_url.filter(|url| !url.trim().is_empty()),
        licence: None,
        source_page: Some(photo.url).filter(|url| !url.trim().is_empty()),
        alt: photo.alt.filter(|text| !text.trim().is_empty()),
        // Pexels states no download-reporting obligation, so there is nothing to carry.
        download_location: None,
    }
}

/// The download-report request Unsplash requires when a user CHOOSES one of its images.
///
/// Their API Guidelines, technical rule 2: "When your application performs something similar to a
/// download (like when a user chooses the image to include in a blog post, set as a header, etc.), you
/// must send a request to the download endpoint returned under the `photo.links.download_location`
/// property." Picking an image for an article is exactly that case, so this is not optional — the
/// result carries the endpoint in [`ImageResult::download_location`].
///
/// Returns `Ok(None)` for a source that has no such obligation (Pexels), rather than a request that
/// would do nothing.
pub fn download_tracking_request(
    source: ImageSource,
    result: &ImageResult,
    api_key: &str,
) -> Result<Option<HttpRequest>> {
    let facts = facts(source)?;
    if facts.hotlink != HotlinkPolicy::Required {
        return Ok(None);
    }

    let Some(location) = result
        .download_location
        .as_deref()
        .map(str::trim)
        .filter(|location| !location.is_empty())
    else {
        // A result with no `download_location` cannot be reported, and refusing the choice would be
        // worse than telling the user what is missing: the image itself is still usable.
        return Err(VeloctError::Attachment(
            "Unsplash 的这条结果没有 download_location,无法按要求上报下载。".to_string(),
        ));
    };

    let request = match facts.auth {
        AuthScheme::ClientId => HttpRequest::get(location)
            .header("Authorization", format!("Client-ID {api_key}"))
            .header("Accept-Version", UNSPLASH_ACCEPT_VERSION),
        AuthScheme::RawKey => HttpRequest::get(location).header("Authorization", api_key),
    };
    Ok(Some(request))
}

/// The URL a download report returned, which the caller records rather than parses.
pub fn parse_download_tracking(response: &HttpResponse) -> Result<String> {
    #[derive(Deserialize)]
    struct Body {
        url: String,
    }
    let body: Body = serde_json::from_slice(&response.body).map_err(|error| {
        VeloctError::Attachment(format!("下载上报的响应不是预期的 JSON: {error}"))
    })?;
    Ok(body.url)
}

// ---------------------------------------------------------------------------------------------
// Search, end to end
// ---------------------------------------------------------------------------------------------

/// Search a provider through the caller's HTTP client.
///
/// The key is used for the two headers this request needs and is then dropped; nothing here stores it.
pub fn search(
    client: &dyn HttpClient,
    source: ImageSource,
    api_key: &str,
    query: &SearchQuery,
) -> Result<ImageSearch> {
    if api_key.trim().is_empty() {
        return Err(VeloctError::not_configured(
            format!("{} 的访问密钥", source.label()),
            "在「网络图片浏览」里为这个来源填写密钥,它会存进操作系统凭据库",
        ));
    }

    let request = search_request(source, api_key, query)?;
    let response = client.execute(request)?;
    if !response.is_success() {
        return Err(provider_failure(source, &response, api_key));
    }
    parse_search(source, &query.text, &response)
}

/// Report a download, as Unsplash's guidelines require when a user chooses an image.
pub fn report_download(
    client: &dyn HttpClient,
    source: ImageSource,
    api_key: &str,
    result: &ImageResult,
) -> Result<Option<String>> {
    let Some(request) = download_tracking_request(source, result, api_key)? else {
        return Ok(None);
    };
    let response = client.execute(request)?;
    if !response.is_success() {
        return Err(provider_failure(source, &response, api_key));
    }
    Ok(Some(parse_download_tracking(&response)?))
}

// ---------------------------------------------------------------------------------------------
// Attribution
// ---------------------------------------------------------------------------------------------

/// The attribution line a chosen image must carry, per the provider's own rule.
///
/// Rendered as Markdown rather than plain text because both providers require a LINK: Unsplash wants
/// the photographer's profile and Unsplash itself (with `utm` parameters), and Pexels wants a
/// prominent link to Pexels and, where possible, a link to the photo page.
pub fn attribution_markdown(result: &ImageResult) -> String {
    let author = result.author.trim();
    let author_link = match result.author_url.as_deref().map(str::trim) {
        Some(url) if !url.is_empty() && !author.is_empty() => format!("[{author}]({url})"),
        _ if !author.is_empty() => author.to_string(),
        _ => String::new(),
    };

    match result.source {
        ImageSource::Unsplash => {
            let photo_link = result
                .source_page
                .as_deref()
                .map(str::trim)
                .filter(|page| !page.is_empty());
            let subject = match (&author_link.is_empty(), photo_link) {
                (false, Some(page)) => format!("[{author}]({page})"),
                (false, None) => author_link.clone(),
                (true, Some(page)) => format!("[这张照片]({page})"),
                (true, None) => "这张照片".to_string(),
            };
            format!(
                "Photo by {subject} on [Unsplash]({UNSPLASH_ATTRIBUTION_LINK})",
                subject = if author_link.is_empty() {
                    subject
                } else {
                    // Prefer the photographer's PROFILE for the author link when it exists, which is
                    // what the guideline asks for; the photo page is then the image's own link.
                    author_link.clone()
                }
            )
        }
        ImageSource::Pexels => {
            let photo_link = result
                .source_page
                .as_deref()
                .map(str::trim)
                .filter(|page| !page.is_empty());
            let subject = if !author_link.is_empty() {
                author_link.clone()
            } else {
                "摄影师".to_string()
            };
            match photo_link {
                Some(page) => format!("Photo by {subject} on [Pexels]({page})"),
                None => format!("Photo by {subject} on [Pexels](https://www.pexels.com/)"),
            }
        }
        ImageSource::Pixabay => {
            // Unreachable through this build: `facts()` refuses Pixabay before a result can exist.
            // Kept as an exhaustive arm rather than a wildcard so adding a source is a compile error.
            "Pixabay".to_string()
        }
    }
}

/// What a user should know before choosing an image from this source.
pub fn source_notice(source: ImageSource) -> String {
    match facts(source) {
        Ok(facts) => format!(
            "{}:{}。图片为免费素材,直接写入 Markdown 的远程地址,不进入附件缓存池、不下载、不再上传;\
             因此它不会被备份,也可能因为来源站移除而失效。",
            source.label(),
            facts.attribution_rule
        ),
        Err(error) => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::FakeHttpClient;

    const UNS_KEY: &str = "unsplash-access-key-0123456789";
    const PEX_KEY: &str = "pexels-api-key-0123456789abcdefghijklmnop";

    /// An Unsplash search response with the fields the spec marks `required`.
    fn unsplash_body() -> serde_json::Value {
        serde_json::json!({
            "total": 133,
            "total_pages": 5,
            "results": [{
                "id": "LBI7cgq3pbM",
                "created_at": "2016-05-03T11:00:28-04:00",
                "updated_at": "2016-07-10T11:00:01-05:00",
                "width": 5245,
                "height": 3497,
                "blur_hash": "LoC%a7IoIVxZ_NM|M{s:%hRjWAo0",
                "description": "A man drinking a coffee.",
                "alt_description": "a man drinking coffee",
                "urls": {
                    "raw": "https://images.unsplash.com/photo-1?ixid=APPID",
                    "full": "https://images.unsplash.com/photo-1?ixid=APPID&fm=jpg&q=80",
                    "regular": "https://images.unsplash.com/photo-1?ixid=APPID&fm=jpg&w=1080",
                    "small": "https://images.unsplash.com/photo-1?ixid=APPID&fm=jpg&w=400",
                    "thumb": "https://images.unsplash.com/photo-1?ixid=APPID&fm=jpg&w=200"
                },
                "user": {
                    "name": "Gilbert Kane",
                    "links": { "html": "https://unsplash.com/@poorkane" }
                },
                "links": {
                    "html": "https://unsplash.com/photos/LBI7cgq3pbM",
                    "download": "https://unsplash.com/photos/LBI7cgq3pbM/download",
                    "self": "https://api.unsplash.com/photos/LBI7cgq3pbM",
                    "download_location": "https://api.unsplash.com/photos/LBI7cgq3pbM/download"
                }
            }]
        })
    }

    /// A Pexels search response in the shape their documentation prints.
    fn pexels_body() -> serde_json::Value {
        serde_json::json!({
            "total_results": 10000,
            "page": 1,
            "per_page": 15,
            "next_page": "https://api.pexels.com/v1/search/?page=2&per_page=15&query=nature",
            "photos": [{
                "id": 3573351,
                "width": 3066,
                "height": 3968,
                "url": "https://www.pexels.com/photo/trees-during-day-3573351/",
                "photographer": "Lukas Rodriguez",
                "photographer_url": "https://www.pexels.com/@lukas-rodriguez-1845331",
                "photographer_id": 1845331,
                "avg_color": "#374824",
                "src": {
                    "original": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png",
                    "large2x": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&dpr=2&h=650&w=940",
                    "large": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&h=650&w=940",
                    "medium": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&h=350",
                    "small": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?auto=compress&cs=tinysrgb&h=130",
                    "portrait": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?fit=crop&h=1200&w=800",
                    "landscape": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?fit=crop&h=627&w=1200",
                    "tiny": "https://images.pexels.com/photos/3573351/pexels-photo-3573351.png?dpr=1&fit=crop&h=200&w=280"
                },
                "liked": false,
                "alt": "Brown Rocks During Golden Hour"
            }]
        })
    }

    #[test]
    fn only_the_two_sources_whose_terms_allow_the_flow_are_implemented() {
        assert!(is_implemented(ImageSource::Unsplash));
        assert!(is_implemented(ImageSource::Pexels));
        // Pixabay is REFUSED rather than silently absent, and the reason is its own words.
        assert!(!is_implemented(ImageSource::Pixabay));
        let reason = unimplemented_reason(ImageSource::Pixabay).expect("a reason");
        assert!(reason.contains("热链"), "{reason}");
        assert!(reason.contains("Unsplash"), "{reason}");
        assert!(unimplemented_reason(ImageSource::Unsplash).is_none());
    }

    #[test]
    fn the_two_providers_authenticate_differently_and_the_difference_is_the_point() {
        // Unsplash's OpenAPI `securitySchemes.Auth` says the value is `Client-ID <id>`; Pexels's
        // documentation says `Authorization: YOUR_API_KEY` with no prefix. Copying one to the other
        // produces a 401 that looks like a bad key.
        let unsplash = search_request(
            ImageSource::Unsplash,
            UNS_KEY,
            &SearchQuery::new(ImageSource::Unsplash, "coffee").unwrap(),
        )
        .unwrap();
        let auth = unsplash
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .expect("an Authorization header");
        assert_eq!(auth, format!("Client-ID {UNS_KEY}"));
        assert_eq!(
            unsplash
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("accept-version"))
                .map(|(_, value)| value.as_str()),
            Some("v1"),
            "Unsplash asks every request to state the version"
        );

        let pexels = search_request(
            ImageSource::Pexels,
            PEX_KEY,
            &SearchQuery::new(ImageSource::Pexels, "coffee").unwrap(),
        )
        .unwrap();
        let auth = pexels
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .expect("an Authorization header");
        assert_eq!(auth, PEX_KEY, "Pexels takes the bare key, with no prefix");
        assert!(
            pexels
                .headers
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("accept-version")),
            "the version header is Unsplash's, not a shared convention"
        );
    }

    #[test]
    fn the_key_never_enters_the_url() {
        // Both providers accept a query-parameter form. Using it would put the credential into every
        // URL that reaches a log, a diagnostic or an error message.
        for source in [ImageSource::Unsplash, ImageSource::Pexels] {
            let key = "distinctive-key-material-0123456789";
            let request = search_request(
                source,
                key,
                &SearchQuery::new(source, "coffee").unwrap(),
            )
            .unwrap();
            assert!(!request.url.contains(key), "{}: {}", source.label(), request.url);
            assert!(!request.url.contains("client_id"), "{}", request.url);
            assert!(!request.url.contains("api_key"), "{}", request.url);
        }
    }

    #[test]
    fn the_search_url_is_the_documented_one_with_an_encoded_query() {
        let mut query = SearchQuery::new(ImageSource::Pexels, "a b/c&d").unwrap();
        query.page = 2;
        query.per_page = 40;
        let request = search_request(ImageSource::Pexels, PEX_KEY, &query).unwrap();

        assert!(request.url.starts_with("https://api.pexels.com/v1/search?"), "{}", request.url);
        // A space is %20 and a `/` inside a VALUE is encoded: a search term is not a path.
        assert!(request.url.contains("query=a%20b%2Fc%26d"), "{}", request.url);
        assert!(request.url.contains("page=2"), "{}", request.url);
        assert!(request.url.contains("per_page=40"), "{}", request.url);

        let mut uns = SearchQuery::new(ImageSource::Unsplash, "计数器").unwrap();
        uns.per_page = 30;
        let request = search_request(ImageSource::Unsplash, UNS_KEY, &uns).unwrap();
        assert!(request.url.starts_with("https://api.unsplash.com/search/photos?"), "{}", request.url);
        // UTF-8 is encoded per byte, so a CJK term survives the round trip.
        assert!(request.url.contains("query=%E8%AE%A1%E6%95%B0%E5%99%A8"), "{}", request.url);
    }

    #[test]
    fn an_empty_term_and_an_out_of_range_page_size_are_refused_locally() {
        let source = ImageSource::Unsplash;
        let mut query = SearchQuery::new(source, "coffee").unwrap();

        query.text = "   ".to_string();
        assert!(search_request(source, UNS_KEY, &query).is_err());
        query.text = "coffee".to_string();

        query.page = 0;
        assert!(search_request(source, UNS_KEY, &query).is_err());
        query.page = 1;

        // The ceiling is the provider's own: 30 for Unsplash, 80 for Pexels.
        query.per_page = UNSPLASH_MAX_PER_PAGE + 1;
        let error = search_request(source, UNS_KEY, &query).unwrap_err();
        assert!(error.to_string().contains("30"), "{error}");

        let mut pexels = SearchQuery::new(ImageSource::Pexels, "coffee").unwrap();
        pexels.per_page = PEXELS_MAX_PER_PAGE;
        assert!(search_request(ImageSource::Pexels, PEX_KEY, &pexels).is_ok());
        pexels.per_page = PEXELS_MAX_PER_PAGE + 1;
        assert!(search_request(ImageSource::Pexels, PEX_KEY, &pexels).is_err());
    }

    #[test]
    fn the_default_page_size_is_each_providers_own() {
        assert_eq!(
            SearchQuery::new(ImageSource::Unsplash, "x").unwrap().per_page,
            10
        );
        assert_eq!(SearchQuery::new(ImageSource::Pexels, "x").unwrap().per_page, 15);
    }

    #[test]
    fn an_unsplash_result_keeps_the_ixid_parameter_and_uses_the_regular_rendition() {
        // Their guidelines make keeping `ixid` a condition of API use, so a result that stripped it
        // would be non-compliant while still working.
        let body = serde_json::to_vec(&unsplash_body()).unwrap();
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body,
        };
        let search = parse_search(ImageSource::Unsplash, "coffee", &response).unwrap();
        let usable = search.usable();
        assert_eq!(usable.len(), 1);
        let result = usable[0];

        assert!(result.url.contains("w=1080"), "regular is the documented display size: {}", result.url);
        assert!(result.url.contains("ixid="), "ixid must survive: {}", result.url);
        assert!(
            result.thumbnail_url.contains("w=200"),
            "the picker's thumbnail is the 200 px rendition: {}",
            result.thumbnail_url
        );
        assert_eq!(result.source_id, "LBI7cgq3pbM");
        assert_eq!(result.author, "Gilbert Kane");
        assert_eq!(
            result.author_url.as_deref(),
            Some("https://unsplash.com/@poorkane")
        );
        assert_eq!(
            result.source_page.as_deref(),
            Some("https://unsplash.com/photos/LBI7cgq3pbM")
        );
        assert_eq!(result.width, 5245);
        assert_eq!(result.alt.as_deref(), Some("a man drinking coffee"));
        assert!(search.has_more, "1 < total_pages 5");
    }

    #[test]
    fn a_pexels_result_uses_the_large_rendition_and_the_documented_fields() {
        let body = serde_json::to_vec(&pexels_body()).unwrap();
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body,
        };
        let search = parse_search(ImageSource::Pexels, "nature", &response).unwrap();
        let usable = search.usable();
        assert_eq!(usable.len(), 1);
        let result = usable[0];

        assert!(result.url.contains("w=940"), "large is 940 wide: {}", result.url);
        assert!(result.thumbnail_url.contains("h=130"), "{}", result.thumbnail_url);
        assert_eq!(result.source_id, "3573351");
        assert_eq!(result.author, "Lukas Rodriguez");
        assert_eq!(
            result.source_page.as_deref(),
            Some("https://www.pexels.com/photo/trees-during-day-3573351/")
        );
        assert_eq!(result.alt.as_deref(), Some("Brown Rocks During Golden Hour"));
        assert!(search.has_more, "next_page is present");
    }

    #[test]
    fn a_last_page_reports_no_more_rather_than_offering_a_button_that_returns_nothing() {
        let mut body = pexels_body();
        body.as_object_mut().unwrap().remove("next_page");
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: serde_json::to_vec(&body).unwrap(),
        };
        assert!(!parse_search(ImageSource::Pexels, "nature", &response).unwrap().has_more);

        let mut uns = unsplash_body();
        uns["total_pages"] = serde_json::json!(1);
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: serde_json::to_vec(&uns).unwrap(),
        };
        assert!(!parse_search(ImageSource::Unsplash, "coffee", &response).unwrap().has_more);
    }

    #[test]
    fn a_response_that_is_not_the_documented_shape_is_reported_rather_than_read_as_empty() {
        // "no results" and "this is not the API I think it is" must not look the same.
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: b"this is not json".to_vec(),
        };
        let error = parse_search(ImageSource::Unsplash, "coffee", &response).unwrap_err();
        assert!(error.to_string().contains("JSON"), "{error}");
    }

    #[test]
    fn an_empty_result_set_is_successful_and_empty_rather_than_an_error() {
        let body = serde_json::json!({ "total": 0, "total_pages": 0, "results": [] });
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: serde_json::to_vec(&body).unwrap(),
        };
        let search = parse_search(ImageSource::Unsplash, "zzz", &response).unwrap();
        assert!(search.is_empty());
        assert!(!search.has_more);
    }

    #[test]
    fn a_provider_error_body_cannot_leak_the_key() {
        // §18.1 and PROCESSING.md §2.8: redact where the secret is KNOWN. Neither provider's key
        // matches a credential PATTERN, so the exact-value layer is the only one that can catch it.
        for (source, key) in [(ImageSource::Unsplash, UNS_KEY), (ImageSource::Pexels, PEX_KEY)] {
            let echoing = format!(
                "{{\"errors\":[\"Invalid access token: {key}\"]}}"
            );
            let response = HttpResponse {
                status: 401,
                headers: Vec::new(),
                body: echoing.into_bytes(),
            };
            let error = provider_failure(source, &response, key);
            let message = error.to_string();
            assert!(
                !message.contains(key),
                "{} leaked the key: {message}",
                source.label()
            );
            assert!(message.contains("401"), "{message}");
            assert!(message.contains(source.label()), "{message}");
        }
    }

    #[test]
    fn a_search_with_no_key_is_reported_as_unconfigured_with_a_hint() {
        let client = FakeHttpClient::new();
        let error = search(
            &client,
            ImageSource::Unsplash,
            "  ",
            &SearchQuery::new(ImageSource::Unsplash, "coffee").unwrap(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "NOT_CONFIGURED");
        assert!(error.hint().is_some(), "the hint says where to put the key");
        assert_eq!(client.request_count(), 0, "no request is sent without a key");
    }

    #[test]
    fn a_search_through_the_client_returns_the_parsed_results() {
        let client = FakeHttpClient::new()
            .on_json("/search/photos", unsplash_body());
        let search_result = search(
            &client,
            ImageSource::Unsplash,
            UNS_KEY,
            &SearchQuery::new(ImageSource::Unsplash, "coffee").unwrap(),
        )
        .unwrap();

        assert_eq!(search_result.usable().len(), 1);
        assert_eq!(client.request_count(), 1);
        // The request the client actually saw carried the key in the header, not the URL.
        let recorded = client.recorded();
        let sent = &recorded[0];
        assert!(!sent.url.contains(UNS_KEY), "{}", sent.url);
        assert!(sent
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == &format!("Client-ID {UNS_KEY}")));
    }

    #[test]
    fn unsplash_requires_a_download_report_and_pexels_does_not() {
        let body = serde_json::to_vec(&unsplash_body()).unwrap();
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body,
        };
        let search = parse_search(ImageSource::Unsplash, "coffee", &response).unwrap();
        let result = search.usable()[0].clone();

        let request = download_tracking_request(ImageSource::Unsplash, &result, UNS_KEY)
            .unwrap()
            .expect("Unsplash's guideline 2 requires this call");
        assert_eq!(
            request.url,
            "https://api.unsplash.com/photos/LBI7cgq3pbM/download"
        );
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("authorization")
                && value == &format!("Client-ID {UNS_KEY}")));

        // Pexels has no such obligation, so this is `None` rather than a request that does nothing.
        let pexels_body = serde_json::to_vec(&pexels_body()).unwrap();
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: pexels_body,
        };
        let search = parse_search(ImageSource::Pexels, "nature", &response).unwrap();
        let result = search.usable()[0].clone();
        assert!(download_tracking_request(ImageSource::Pexels, &result, PEX_KEY)
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_download_report_reads_the_url_the_endpoint_returns() {
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: br#"{"url":"https://images.unsplash.com/photo-1?ixid=APPID&force=true"}"#.to_vec(),
        };
        assert_eq!(
            parse_download_tracking(&response).unwrap(),
            "https://images.unsplash.com/photo-1?ixid=APPID&force=true"
        );
    }

    #[test]
    fn the_unsplash_attribution_names_unsplash_the_photographer_and_the_utm_parameters() {
        // Their API Guidelines, technical rule 3, verbatim: attribute Unsplash, attribute the
        // photographer, link back to their profile, and put utm parameters on links to Unsplash.
        let body = serde_json::to_vec(&unsplash_body()).unwrap();
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body,
        };
        let result = parse_search(ImageSource::Unsplash, "coffee", &response)
            .unwrap()
            .usable()[0]
            .clone();

        let attribution = attribution_markdown(&result);
        assert!(attribution.contains("Unsplash"), "{attribution}");
        assert!(attribution.contains("Gilbert Kane"), "{attribution}");
        assert!(attribution.contains("https://unsplash.com/@poorkane"), "{attribution}");
        assert!(attribution.contains("utm_source=veloct"), "{attribution}");
        assert!(attribution.contains("utm_medium=referral"), "{attribution}");
    }

    #[test]
    fn the_pexels_attribution_links_the_photo_page_as_its_guidelines_ask() {
        // Their guidelines: a prominent link to Pexels, and "Photo by John Doe on Pexels" with a link
        // to the photo page.
        let body = serde_json::to_vec(&pexels_body()).unwrap();
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body,
        };
        let result = parse_search(ImageSource::Pexels, "nature", &response)
            .unwrap()
            .usable()[0]
            .clone();

        let attribution = attribution_markdown(&result);
        assert!(attribution.contains("Pexels"), "{attribution}");
        assert!(attribution.contains("Lukas Rodriguez"), "{attribution}");
        assert!(
            attribution.contains("https://www.pexels.com/photo/trees-during-day-3573351/"),
            "{attribution}"
        );
    }

    #[test]
    fn an_attribution_survives_a_result_that_is_missing_its_author() {
        // A result with no `user` still has a usable URL, so the line degrades instead of panicking or
        // printing an empty link.
        let mut body = unsplash_body();
        body["results"][0].as_object_mut().unwrap().remove("user");
        let response = HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: serde_json::to_vec(&body).unwrap(),
        };
        let result = parse_search(ImageSource::Unsplash, "coffee", &response)
            .unwrap()
            .usable()[0]
            .clone();
        assert!(result.author.is_empty());
        let attribution = attribution_markdown(&result);
        assert!(attribution.contains("Unsplash"), "{attribution}");
        assert!(!attribution.contains("[]("), "no empty link: {attribution}");
    }

    #[test]
    fn every_source_has_a_notice_a_user_can_read_before_choosing() {
        for source in ImageSource::ALL {
            let notice = source_notice(source);
            assert!(!notice.trim().is_empty(), "{source:?}");
            if is_implemented(source) {
                assert!(notice.contains(source.label()), "{notice}");
            }
        }
    }
}
