//! The Lsky Pro attachment target (`handoff.md` §13.12).
//!
//! §13.12 asks for "多个实例、实例地址 / API 地址、独立 Token", so the instance lives in the target's
//! CONFIGURATION and the API token is a CREDENTIAL resolved by handle from the OS store — the same split
//! §13.11 uses for S3's secret access key.
//!
//! **The protocol was read from the vendor's own artifacts, because the vendor publishes no API
//! reference.** `docs.lsky.pro` (now `docs.huohuastudio.com/lsky-pro`) documents installation, storage
//! drivers and upgrades and contains no endpoint; the Halo store page's note that the definitions are
//! visible in the Lsky backend is literally true — the contract ships INSIDE the product. Read on
//! 2026-10-02 from `lsky-org/lsky-pro` at `master` (the free line, latest release 2.1, Laravel 9):
//!
//! * `resources/views/common/api.blade.php` — the vendor's own API page, rendered at `{instance}/api`;
//! * `routes/api.php` — `v1/upload`, `v1/tokens`, `v1/profile`, `v1/images/{key}`;
//! * `app/Http/Controllers/Api/V1/{ImageController,TokenController,UserController}.php`,
//!   `app/Http/Result.php`, `app/Services/ImageService.php`, `app/Models/Image.php` and
//!   `app/Http/Middleware/CheckIsEnableApi.php`.
//!
//! What that reading fixes, and each of these is a rule rather than a preference:
//!
//! * the base is `{instance}/api/v1`, `POST upload` is `multipart/form-data` with a `file` part, and the
//!   `Authorization` header is `Bearer <Sanctum plain-text token>`;
//! * **every request sends `Accept: application/json`.** `CheckIsEnableApi` chooses between a JSON 403 and
//!   an HTML 404 on `expectsJson()`, and Laravel answers an unauthenticated `auth:sanctum` request with a
//!   redirect rather than a 401 unless the client asks for JSON;
//! * **a refusal is usually HTTP 200 with `status:false`.** `Result::response` puts the envelope in the
//!   body and always uses 200, so `is_success()` is NOT the success signal here — `status` is;
//! * **`head` cannot be answered at all.** The free line's only lookup is `GET /images`, whose documented
//!   `keyword` filter the vendor's own `Image::scopeFilter` applies to `origin_name` and `alias_name` and
//!   never to `key`, so this adapter reports that it cannot answer rather than claiming an object is
//!   absent. Nothing in Veloct calls it on the upload path: §13.4's reuse is decided by the RECORDED
//!   object, which is the only place that knows what this target already holds;
//! * the object's KEY and PUBLIC URL are the SERVER's. `data.key` and `data.links.url` are what the record
//!   stores, which is what makes a later `DELETE /images/{key}` name the right object, so
//!   `derives_public_url` is `false` — there is nothing to derive before an upload happens.
//!
//! Two limits are stated rather than hidden. **(1)** The paid line ("Lsky Pro+", 2.x and enterprise 1.x) is
//! the same architecture by the vendor's own documentation, but from 2.3.0 its backend source is ENCRYPTED
//! and no public API reference exists, so this adapter is verified against the FREE line and does not claim
//! the paid one. **(2)** The vendor's delete removes the physical file only when no other record on the same
//! strategy shares its `md5`+`sha1`, and it CATCHES a failure to remove it and only logs it — so a
//! `status:true` "删除成功" does not prove the object is gone, and no Veloct message may say that it does.

use std::sync::Arc;

use serde::Deserialize;
use veloct_core::http::{HttpClient, HttpMethod, HttpRequest, HttpResponse};
use veloct_core::secret::redact_with_known;
use veloct_core::{Result, VeloctError};

use crate::storage::{
    content_type_for, PutOutcome, RemoteObject, StorageAdapter, StorageCapabilities, StorageConfig,
};

/// The API path every instance serves, taken from the vendor's own API page (`{host}/api/v1`).
const API_PATH: &str = "api/v1";

/// A Lsky Pro instance as an attachment target (§13.12).
pub struct LskyStore {
    config: StorageConfig,
    token: String,
    client: Arc<dyn HttpClient>,
}

impl std::fmt::Debug for LskyStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The token is deliberately absent: no `{:?}` of this type may be able to print it.
        f.debug_struct("LskyStore")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl LskyStore {
    /// Build the adapter for a configured target.
    ///
    /// `token` is the value resolved from the OS credential store by HANDLE — §13.12's "独立 Token", which
    /// the vendor's own page shows as `"Authorization": "Bearer 1|..."`.
    pub fn new(
        config: StorageConfig,
        token: impl Into<String>,
        client: Arc<dyn HttpClient>,
    ) -> Result<Self> {
        let token = token.into();
        if token.trim().is_empty() {
            return Err(VeloctError::not_configured(
                format!("the Lsky Pro target {}", config.name),
                "an Lsky Pro target needs its API token, stored as the target's credential; the \
                 instance's own 接口 page shows the value as \"Authorization\": \"Bearer 1|...\"",
            ));
        }
        // Checked here as well as on the save path, because a target can be built from a record that was
        // written before the rule existed.
        api_base(&config)?;
        Ok(Self {
            config,
            token,
            client,
        })
    }

    /// The API base, e.g. `https://img.example.com/api/v1`.
    pub fn api_base(&self) -> Result<String> {
        api_base(&self.config)
    }

    /// The URL one endpoint lives at.
    pub fn endpoint(&self, path: &str) -> Result<String> {
        Ok(format!(
            "{}/{}",
            self.api_base()?,
            path.trim_start_matches('/')
        ))
    }

    /// A request carrying the two headers every call needs.
    fn request(&self, method: HttpMethod, url: String) -> HttpRequest {
        HttpRequest::new(method, url)
            .bearer(&self.token)
            // Required rather than cosmetic: Laravel answers an unauthenticated `auth:sanctum` request with
            // a redirect unless the client asks for JSON, and `CheckIsEnableApi` picks 403-vs-404 the same
            // way. Without this a rejected token would look like an HTML page.
            .header("Accept", "application/json")
    }

    fn send(&self, request: HttpRequest, what: &str) -> Result<serde_json::Value> {
        let response = self.client.execute(request)?;
        self.read_envelope(response, what)
    }

    /// Read the documented `{status, message, data}` envelope.
    ///
    /// Two shapes are handled because the vendor really sends both: its own envelope (with `status:false`
    /// for a refusal, at HTTP 200 for most and 403 for the disabled-API switch), and Laravel's bare
    /// `{"message":"Unauthenticated."}` for a rejected token.
    fn read_envelope(&self, response: HttpResponse, what: &str) -> Result<serde_json::Value> {
        let status = response.status;
        match serde_json::from_slice::<Envelope>(&response.body) {
            Ok(envelope) => {
                if envelope.status {
                    Ok(envelope.data)
                } else {
                    // The vendor's own words, because they are the actionable part: "管理员未启用 API"
                    // tells the user which switch to find.
                    Err(VeloctError::Attachment(format!(
                        "{what} was refused by Lsky Pro: {}",
                        self.redact(&envelope.message)
                    )))
                }
            }
            Err(_) => {
                let detail = match serde_json::from_slice::<MessageOnly>(&response.body) {
                    Ok(only) => format!(": {}", self.redact(&only.message)),
                    Err(_) => format!("; the body was {}", self.excerpt(&response)),
                };
                Err(VeloctError::Attachment(format!(
                    "{what} got HTTP {status} from Lsky Pro and it was not the documented \
                     {{\"status\",\"message\",\"data\"}} envelope{detail}"
                )))
            }
        }
    }

    /// Redact with the token KNOWN, not only by pattern.
    ///
    /// §18.1's exact-value layer is the only one that can catch a vendor echoing an opaque credential back,
    /// and a Lsky token (`1|...`) matches no credential pattern (PROCESSING.md §2.8).
    fn redact(&self, text: &str) -> String {
        redact_with_known(text, &[self.token.as_str()])
    }

    fn excerpt(&self, response: &HttpResponse) -> String {
        let text = self.redact(&response.text());
        let excerpt: String = text.chars().take(200).collect();
        format!("{excerpt:?}")
    }

    /// The recorded object, from the upload response.
    fn object_from_upload(&self, data: &serde_json::Value, file_name: &str) -> Result<RemoteObject> {
        let key = data
            .get("key")
            .and_then(serde_json::Value::as_str)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                VeloctError::Attachment(format!(
                    "Lsky Pro answered success for {file_name} but the payload carried no key, so the \
                     object could be neither recorded nor later removed"
                ))
            })?;
        let public_url = data
            .get("links")
            .and_then(|links| links.get("url"))
            .and_then(serde_json::Value::as_str)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| {
                VeloctError::Attachment(format!(
                    "Lsky Pro answered success for {file_name} but links.url was absent, so there is no \
                     URL the document could cite"
                ))
            })?;
        Ok(RemoteObject {
            key: key.to_string(),
            public_url: public_url.to_string(),
            // The vendor documents `size` in KILOBYTES. It is the size of the STORED object, which the
            // instance's own processors (watermark, resize) may have changed, so it is the truthful number
            // to record — the pre-upload length would describe a file the server did not keep.
            size_bytes: data
                .get("size")
                .and_then(serde_json::Value::as_f64)
                .map(kilobytes_to_bytes)
                .unwrap_or(0),
        })
    }
}

/// The API base for a configured instance.
///
/// §13.12 says "实例地址 / API 地址" — either spelling is accepted, so an instance that already names the
/// API path keeps it and one that names only the host gets `/{API_PATH}` appended.
pub fn api_base(config: &StorageConfig) -> Result<String> {
    let raw = config
        .instance
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            VeloctError::Invalid(format!(
                "the Lsky Pro target {} has no instance address",
                config.name
            ))
        })?;
    let trimmed = raw.trim_end_matches('/');
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(VeloctError::Invalid(format!(
            "the Lsky Pro instance {raw:?} must be an absolute http(s) URL, because the endpoint is built \
             by appending /{API_PATH} to it"
        )));
    }
    if trimmed.ends_with(API_PATH) {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{trimmed}/{API_PATH}"))
    }
}

/// The documented unit: the API reports `size` in kilobytes.
fn kilobytes_to_bytes(kilobytes: f64) -> u64 {
    if !kilobytes.is_finite() || kilobytes <= 0.0 {
        return 0;
    }
    (kilobytes * 1024.0).round() as u64
}

/// The multipart boundary for one upload.
///
/// Derived from a seed and then EXTENDED until it cannot occur inside the payload: a boundary that appears
/// in the bytes would split the part in two, which is the one failure a hand-built multipart body can have.
/// The seed is separate from the payload so that collision path is testable — a test can build a payload
/// that contains the candidate.
fn boundary_for(seed: &str, payload: &[u8]) -> String {
    let mut boundary = format!(
        "----VeloctLskyBoundary{}",
        &veloct_core::sha256_hex_str(seed)[..24]
    );
    while contains(payload, boundary.as_bytes()) {
        boundary.push('X');
    }
    boundary
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

/// The `multipart/form-data` body for one `file` part, which is the only part §13.12 needs.
///
/// `filename` is what the instance records as `origin_name`, so the attachment keeps a name a user can
/// recognise in Lsky's own gallery.
fn multipart_body(
    boundary: &str,
    file_name: &str,
    content_type: Option<&str>,
    payload: &[u8],
) -> Vec<u8> {
    let mut body = Vec::with_capacity(payload.len() + 256);
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{}\"\r\n",
            quoted(file_name)
        )
        .as_bytes(),
    );
    body.extend_from_slice(
        format!(
            "Content-Type: {}\r\n\r\n",
            content_type.unwrap_or("application/octet-stream")
        )
        .as_bytes(),
    );
    body.extend_from_slice(payload);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// Strip anything that could break out of the quoted `filename` parameter.
fn quoted(value: &str) -> String {
    value.replace(['"', '\\', '\r', '\n'], "_")
}

impl StorageAdapter for LskyStore {
    fn config(&self) -> &StorageConfig {
        &self.config
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            // Verified against the vendor's own shipped API page and source, for the FREE line. The paid
            // line is deliberately not claimed; see this module's documentation.
            verified: true,
            // The server names the object and returns its URL, so nothing can be derived before an upload.
            derives_public_url: false,
            // `DELETE /images/{key}` is documented, and its own source hard-deletes the record.
            can_delete_remotely: true,
            // `GET /profile` proves the token without touching an image.
            can_test_connection: true,
        }
    }

    fn put(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let file_name = key.rsplit('/').next().unwrap_or(key);
        let boundary = boundary_for(file_name, bytes);
        let content_type = content_type_for(file_name);
        let body = multipart_body(&boundary, file_name, content_type.as_deref(), bytes);
        let request = self
            .request(HttpMethod::Post, self.endpoint("upload")?)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .bytes(body);

        let data = self.send(request, &format!("uploading {file_name}"))?;
        let object = self.object_from_upload(&data, file_name)?;
        // Lsky cannot be asked whether this object was already there (see `head`), so a write is always a
        // create. §13.4's reuse is decided by the RECORDED object before an adapter is reached, which is
        // the only correct place for a target whose key the server chooses.
        Ok(PutOutcome::Created(object))
    }

    fn head(&self, key: &str) -> Result<Option<RemoteObject>> {
        Err(VeloctError::Attachment(format!(
            "Lsky Pro cannot be asked whether {key} exists: its API has no lookup by key, and the \
             documented `keyword` filter matches a file's name rather than its key. Answering \"absent\" \
             would be a guess, so this reports that the answer is unavailable — nothing in Veloct needs it \
             for an upload, because §13.4's reuse is decided by the recorded object"
        )))
    }

    fn test_connection(&self) -> Result<String> {
        let data = self.send(
            self.request(HttpMethod::Get, self.endpoint("profile")?),
            "checking the Lsky Pro credentials",
        )?;
        let who = data
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|name| !name.trim().is_empty())
            .unwrap_or("the account");
        Ok(format!("{} answered for {who}", self.api_base()?))
    }

    fn delete_remote(&self, key: &str) -> Result<()> {
        // §13.8's explicit path only. The key is a path SEGMENT, so it is percent-encoded: today's keys are
        // alphanumeric, and encoding keeps that an implementation detail rather than an assumption.
        let request = self.request(
            HttpMethod::Delete,
            self.endpoint(&format!("images/{}", veloct_s3::uri_encode(key, true)))?,
        );
        self.send(request, &format!("removing {key} from Lsky Pro"))?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct Envelope {
    status: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    data: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct MessageOnly {
    message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageKind;
    use veloct_core::http::FakeHttpClient;

    fn config() -> StorageConfig {
        let mut config = StorageConfig::new("lsky", "my lsky", StorageKind::LskyPro);
        config.instance = Some("https://img.example.com".to_string());
        config
    }

    fn store(fake: Arc<FakeHttpClient>) -> LskyStore {
        LskyStore::new(config(), "1|abc123token", fake as Arc<dyn HttpClient>).unwrap()
    }

    fn uploaded(key: &str, url: &str, size_kb: f64) -> serde_json::Value {
        serde_json::json!({
            "status": true,
            "message": "上传成功",
            "data": {
                "key": key,
                "name": "photo-a8k3xq.webp",
                "pathname": "uploads/2026/10/02/photo-a8k3xq.webp",
                "origin_name": "photo-a8k3xq.webp",
                "size": size_kb,
                "mimetype": "image/webp",
                "extension": "webp",
                "md5": "0".repeat(32),
                "sha1": "0".repeat(40),
                "links": {
                    "url": url,
                    "html": "&lt;img src=\"...\" /&gt;",
                    "bbcode": "[img]...[/img]",
                    "markdown": "![](...)",
                    "markdown_with_link": "[![](...)](...)",
                    "thumbnail_url": url
                }
            }
        })
    }

    #[test]
    fn the_instance_becomes_the_documented_api_base_and_a_pasted_api_address_is_kept() {
        assert_eq!(
            api_base(&config()).unwrap(),
            "https://img.example.com/api/v1"
        );

        // A trailing slash, and the "API 地址" spelling §13.12 also allows, both land on one base.
        let mut trailing = config();
        trailing.instance = Some("https://img.example.com/".to_string());
        assert_eq!(
            api_base(&trailing).unwrap(),
            "https://img.example.com/api/v1"
        );

        let mut full = config();
        full.instance = Some("https://img.example.com/api/v1/".to_string());
        assert_eq!(api_base(&full).unwrap(), "https://img.example.com/api/v1");
    }

    #[test]
    fn an_instance_that_is_not_an_absolute_url_is_refused() {
        let mut relative = config();
        relative.instance = Some("img.example.com".to_string());
        let error = api_base(&relative).unwrap_err();
        assert!(
            error.to_string().contains("absolute http(s) URL"),
            "{error}"
        );

        let mut missing = config();
        missing.instance = None;
        assert!(api_base(&missing).unwrap_err().to_string().contains("instance"));
    }

    #[test]
    fn a_missing_token_is_refused_with_the_instances_own_header_shape() {
        let error = LskyStore::new(
            config(),
            "",
            Arc::new(FakeHttpClient::new()) as Arc<dyn HttpClient>,
        )
        .unwrap_err();
        // PROCESSING.md §2.3: a hint is NOT part of `Display`, so the two halves are asserted separately
        // rather than through one string that would silently pass on either.
        assert!(
            error.to_string().contains("Lsky Pro target"),
            "{}",
            error
        );
        let hint = error.hint().unwrap_or_default();
        assert!(hint.contains("API token"), "{hint}");
        // §18.1: neither half may print a credential VALUE. The hint quotes the vendor's header SHAPE
        // (`Bearer 1|...`), which is documentation rather than a secret, so what is asserted is that no
        // value could have been substituted into it.
        assert!(!error.to_string().contains("Bearer 1|abc"), "{error}");
        assert!(!hint.contains("abc123"), "{hint}");
    }

    #[test]
    fn the_multipart_body_is_the_documented_file_part() {
        let body = multipart_body(
            "BOUND",
            "photo-a8k3xq.webp",
            Some("image/webp"),
            b"PNGDATA",
        );
        let text = String::from_utf8_lossy(&body).to_string();
        assert!(text.starts_with("--BOUND\r\n"), "{text:?}");
        assert!(
            text.contains("Content-Disposition: form-data; name=\"file\"; filename=\"photo-a8k3xq.webp\""),
            "{text:?}"
        );
        assert!(text.contains("Content-Type: image/webp"), "{text:?}");
        assert!(text.contains("\r\n\r\nPNGDATA\r\n--BOUND--\r\n"), "{text:?}");
    }

    #[test]
    fn a_quoted_file_name_cannot_break_out_of_the_disposition_header() {
        let body = multipart_body("B", "a\"b\r\nX: y", None, b"x");
        let text = String::from_utf8_lossy(&body).to_string();
        assert!(text.contains("filename=\"a_b__X: y\""), "{text:?}");
        // The injected CRLF is gone, so nothing after it can become a header of its own.
        assert!(!text.contains("filename=\"a\"b"), "{text:?}");
    }

    #[test]
    fn a_boundary_that_occurs_in_the_payload_is_extended_until_it_cannot() {
        // The candidate is computable from the seed, so the collision path is reachable in a test rather
        // than being a branch nothing covers.
        let candidate = format!(
            "----VeloctLskyBoundary{}",
            &veloct_core::sha256_hex_str("photo.webp")[..24]
        );
        let payload = format!("before{candidate}after").into_bytes();
        let boundary = boundary_for("photo.webp", &payload);
        assert_ne!(boundary, candidate);
        assert!(!contains(&payload, boundary.as_bytes()));
        assert!(boundary.starts_with(&candidate), "{boundary}");

        // And the ordinary case is stable for the same input.
        assert_eq!(boundary_for("photo.webp", b"x"), candidate);
    }

    #[test]
    fn upload_against_fake_api_sends_the_documented_request_and_records_the_servers_key_and_url() {
        let fake = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/upload",
            uploaded(
                "Kx3o6m1Yv8",
                "https://img.example.com/i/2026/10/02/photo-a8k3xq.webp",
                12.5,
            ),
        ));
        let store = store(fake.clone());

        let outcome = store.put("photo-a8k3xq.webp", b"PNGDATA").unwrap();
        assert!(outcome.was_created(), "Lsky cannot report a reuse");
        let object = outcome.object();
        // The SERVER's key and URL, because that is what a later DELETE must name.
        assert_eq!(object.key, "Kx3o6m1Yv8");
        assert_eq!(
            object.public_url,
            "https://img.example.com/i/2026/10/02/photo-a8k3xq.webp"
        );
        // `size` is documented in kilobytes: 12.5 KB is 12800 bytes.
        assert_eq!(object.size_bytes, 12_800);

        let recorded = fake.recorded();
        assert_eq!(recorded.len(), 1);
        let request = &recorded[0];
        assert_eq!(request.method, HttpMethod::Post);
        assert_eq!(request.url, "https://img.example.com/api/v1/upload");
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "Authorization" && value == "Bearer 1|abc123token"));
        // Without this header Laravel answers a rejected token with a redirect instead of a 401.
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name == "Accept" && value == "application/json"));
        let content_type = request
            .headers
            .iter()
            .find(|(name, _)| name == "Content-Type")
            .map(|(_, value)| value.clone())
            .expect("the multipart content type");
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .expect("the documented multipart content type");
        let body = request.body.as_text().expect("a byte body");
        // The declared boundary is the one the body actually uses.
        assert!(body.starts_with(&format!("--{boundary}\r\n")), "{body:?}");
        assert!(body.contains("PNGDATA"), "{body:?}");
    }

    #[test]
    fn a_status_false_response_is_a_failure_even_though_the_http_status_is_200() {
        // The vendor's own refusal shape: HTTP 200 with the envelope saying no.
        let fake = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/upload",
            serde_json::json!({ "status": false, "message": "不支持的文件类型", "data": {} }),
        ));
        let error = store(fake).put("photo.exe", b"x").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("不支持的文件类型"), "{message}");
        assert!(message.contains("uploading photo.exe"), "{message}");
    }

    #[test]
    fn the_admins_api_switch_is_reported_in_the_vendors_words() {
        let fake = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/upload",
            serde_json::json!({ "status": false, "message": "管理员未启用 API", "data": {} }),
        ));
        let error = store(fake).put("a.png", b"x").unwrap_err();
        // The message names the switch, which is the actionable part, rather than "HTTP 403".
        assert!(error.to_string().contains("管理员未启用 API"), "{error}");
    }

    #[test]
    fn an_invalid_token_reports_the_status_and_laravels_message_rather_than_the_envelope() {
        // Sanctum's unauthenticated answer is NOT the envelope, which is why it needs its own path.
        let fake = Arc::new(FakeHttpClient::new().always(
            401,
            r#"{"message":"Unauthenticated."}"#,
        ));
        let error = store(fake).put("a.png", b"x").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("HTTP 401"), "{message}");
        assert!(message.contains("Unauthenticated."), "{message}");
    }

    #[test]
    fn a_response_that_is_not_the_api_is_reported_rather_than_read_as_success() {
        // A login page, a proxy error or a different product behind the same host.
        let fake = Arc::new(FakeHttpClient::new().always(200, "<!doctype html><html>login</html>"));
        let error = store(fake).put("a.png", b"x").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("not the documented"), "{message}");
        assert!(message.contains("login"), "{message}");
    }

    #[test]
    fn a_key_or_url_the_vendor_did_not_return_is_refused_rather_than_recorded_half_formed() {
        let no_key = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/upload",
            serde_json::json!({ "status": true, "message": "ok", "data": { "links": { "url": "https://x/y.png" } } }),
        ));
        assert!(store(no_key)
            .put("a.png", b"x")
            .unwrap_err()
            .to_string()
            .contains("no key"));

        let no_url = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/upload",
            serde_json::json!({ "status": true, "message": "ok", "data": { "key": "K" } }),
        ));
        assert!(store(no_url)
            .put("a.png", b"x")
            .unwrap_err()
            .to_string()
            .contains("links.url was absent"));
    }

    #[test]
    fn a_size_that_is_not_a_number_is_recorded_as_zero_rather_than_guessed() {
        let fake = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/upload",
            serde_json::json!({
                "status": true,
                "message": "ok",
                "data": { "key": "K", "size": "twelve", "links": { "url": "https://x/y.png" } }
            }),
        ));
        assert_eq!(store(fake).put("a.png", b"x").unwrap().object().size_bytes, 0);
    }

    #[test]
    fn head_reports_that_it_cannot_be_answered_instead_of_claiming_the_object_is_absent() {
        let fake = Arc::new(FakeHttpClient::new());
        let error = store(fake.clone()).head("Kx3o6m1Yv8").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("no lookup by key"), "{message}");
        assert!(message.contains("Kx3o6m1Yv8"), "{message}");
        // It must not have asked the instance anything: there is no endpoint to ask.
        assert_eq!(fake.request_count(), 0);
    }

    #[test]
    fn delete_against_fake_api_uses_the_servers_key_and_checks_status() {
        let fake = Arc::new(FakeHttpClient::new().on_method_and_url(
            HttpMethod::Delete,
            "/api/v1/images/",
            200,
            r#"{"status":true,"message":"删除成功","data":{}}"#,
        ));
        store(fake.clone()).delete_remote("Kx3o6m1Yv8").unwrap();

        let recorded = fake.recorded();
        assert_eq!(
            recorded[0].url,
            "https://img.example.com/api/v1/images/Kx3o6m1Yv8"
        );
        assert_eq!(recorded[0].method, HttpMethod::Delete);
    }

    #[test]
    fn a_delete_the_api_refuses_is_reported_rather_than_counted_as_a_cleanup() {
        let fake = Arc::new(FakeHttpClient::new().on_method_and_url(
            HttpMethod::Delete,
            "/api/v1/images/",
            200,
            r#"{"status":false,"message":"图片不存在","data":{}}"#,
        ));
        let error = store(fake).delete_remote("gone").unwrap_err();
        assert!(error.to_string().contains("图片不存在"), "{error}");
    }

    #[test]
    fn checking_the_connection_proves_the_token_against_the_profile_endpoint() {
        let fake = Arc::new(FakeHttpClient::new().on_json(
            "/api/v1/profile",
            serde_json::json!({
                "status": true,
                "message": "success",
                "data": { "name": "veloct", "capacity": 1024, "url": "https://img.example.com" }
            }),
        ));
        let sentence = store(fake.clone()).test_connection().unwrap();
        assert!(sentence.contains("https://img.example.com/api/v1"), "{sentence}");
        assert!(sentence.contains("veloct"), "{sentence}");
        assert_eq!(
            fake.recorded()[0].url,
            "https://img.example.com/api/v1/profile"
        );
        assert_eq!(fake.recorded()[0].method, HttpMethod::Get);

        // A rejected token is a failure rather than a connection.
        let refused = Arc::new(FakeHttpClient::new().always(401, r#"{"message":"Unauthenticated."}"#));
        assert!(store(refused).test_connection().is_err());
    }

    #[test]
    fn the_capabilities_say_the_public_url_comes_from_the_response() {
        let capabilities = store(Arc::new(FakeHttpClient::new())).capabilities();
        assert!(capabilities.verified, "the free line's contract was read");
        assert!(
            !capabilities.derives_public_url,
            "the server returns the URL, so nothing can be derived before an upload"
        );
        assert!(capabilities.can_delete_remotely);
        assert!(capabilities.can_test_connection);
    }

    #[test]
    fn the_token_never_appears_in_an_error() {
        // §18.1 at the boundary: a body that echoes the token back must not reach a message or a log.
        let fake = Arc::new(FakeHttpClient::new().always(
            500,
            format!("upstream failed with Authorization: Bearer {}", "1|abc123token"),
        ));
        let error = store(fake).put("a.png", b"x").unwrap_err();
        let message = error.to_string();
        assert!(!message.contains("1|abc123token"), "{message}");
        assert!(message.contains("[REDACTED]"), "{message}");
    }
}
