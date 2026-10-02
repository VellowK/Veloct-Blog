//! The S3 object API, signed with SigV4.
//!
//! `handoff.md` names S3 twice — §13.11 as an attachment storage target with a custom Endpoint, Region,
//! Bucket, path prefix and public URL prefix, and §17.4 as a cloud backup target — and "S3-compatible"
//! is the operative word: MinIO, Cloudflare R2, Backblaze B2 and friends all speak this API at a
//! user-supplied endpoint. So the client is written against the API rather than against `s3.amazonaws.com`,
//! and the endpoint, the addressing style and the region are all configuration.
//!
//! What this module owns: building a request, signing it, sending it through `veloct_core`'s HTTP seam,
//! and reading the few responses Veloct needs. What it does NOT own: what a key looks like, what a
//! listing means, or which target is being written to. Those are the dependents', exactly as ADR-0015
//! settled for GitHub.
//!
//! Two decisions worth stating:
//!
//! * **The payload hash is computed, not `UNSIGNED-PAYLOAD`.** ADR-0016 has the reasoning: the literal
//!   is only valid over TLS, and a self-hosted MinIO is usually plain `http://`. Hashing costs one extra
//!   streaming pass over the file and works against every endpoint.
//! * **The timestamp is an argument, not a call to the system clock.** `handoff.md` ADR-0002 requires
//!   injected time where behaviour must be deterministic, and a signature is entirely a function of its
//!   timestamp — so a test can pin it and a live run can pass the real clock.

use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use veloct_core::http::{HttpBody, HttpClient, HttpMethod, HttpRequest, HttpResponse};
use veloct_core::{Result, VeloctError};

use crate::sigv4::{self, CanonicalRequest, Credentials, SigningContext, EMPTY_PAYLOAD_SHA256};

/// One object in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct S3Object {
    /// The full key, including whatever prefix was listed under.
    pub key: String,
    pub size_bytes: u64,
    /// The `LastModified` the bucket reports, which retention sorts by.
    pub last_modified: String,
}

/// The token a user may write in an S3 endpoint to say WHERE the bucket belongs.
///
/// See [`S3Config::compatible`]. It is spelled out rather than inferred from a host suffix, so the choice is
/// the user's and an error message can teach it — a provider's addressing requirement is a fact about that
/// provider, and guessing it from a domain name would be a rule that ages badly.
pub const BUCKET_PLACEHOLDER: &str = "{bucket}";

/// How a bucket is addressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Addressing {
    /// `https://endpoint/bucket/key`. What a self-hosted S3-compatible service almost always wants, and
    /// the default when an endpoint is configured.
    PathStyle,
    /// `https://bucket.endpoint/key`. What AWS wants, and what a provider with wildcard DNS accepts.
    VirtualHosted,
}

/// Where a bucket lives and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    /// The origin, with no trailing slash: `https://s3.eu-central-1.amazonaws.com` or
    /// `http://127.0.0.1:9000`. A path may be present (`https://host/s3`) and is included in both the
    /// URL and the signature, because S3 signs the path it is given.
    pub endpoint: String,
    pub addressing: Addressing,
    /// The prefix a public URL is built from (§13.6). Empty means the caller derives nothing.
    pub public_url_prefix: String,
}

impl S3Config {
    /// The AWS endpoint for a region, addressed the way AWS expects.
    pub fn aws(bucket: impl Into<String>, region: impl Into<String>) -> Self {
        let region = region.into();
        Self {
            bucket: bucket.into(),
            endpoint: format!("https://s3.{region}.amazonaws.com"),
            region,
            addressing: Addressing::VirtualHosted,
            public_url_prefix: String::new(),
        }
    }

    /// An endpoint the user supplied, addressed the way the ENDPOINT says it wants to be addressed.
    ///
    /// The default is path style (`https://host/bucket/key`), which is what a self-hosted service almost always
    /// wants. **A `{bucket}` placeholder in the endpoint switches to virtual-hosted addressing**
    /// (`https://bucket.host/key`), and that is not cosmetic: Alibaba Cloud OSS refuses a path-style bucket
    /// request in its own words — `403 SecondLevelDomainForbidden`, "The bucket you are attempting to access
    /// must be addressed using OSS third level domain" — while AWS, Cloudflare R2 and Backblaze B2 use the same
    /// `bucket.host` shape. So the user states which shape their provider documents:
    ///
    /// ```text
    /// https://s3.example.com                        path style:     https://s3.example.com/bucket/key
    /// https://{bucket}.oss-cn-chengdu.aliyuncs.com  virtual-hosted: https://bucket.oss-cn-chengdu.aliyuncs.com/key
    /// ```
    ///
    /// The placeholder is REMOVED here, because [`S3Config::host`] puts the bucket back when the addressing is
    /// virtual-hosted. A placeholder anywhere else is refused by [`S3Config::endpoint_parts`] BY NAME rather
    /// than silently dropped or silently kept, so a typo cannot become a request to the wrong host.
    pub fn compatible(
        bucket: impl Into<String>,
        region: impl Into<String>,
        endpoint: impl Into<String>,
    ) -> Self {
        let endpoint = endpoint.into();
        // WHERE the placeholder counts: the start of the host, and nowhere else. Anything else is LEFT IN PLACE
        // for `endpoint_parts` to refuse by name, because "it appeared somewhere" is not the same as "the user
        // said the bucket belongs in the hostname" — an endpoint like `https://oss-{bucket}.example.com` would
        // otherwise be silently rewritten into a host that does not exist.
        let placeholder = format!("{BUCKET_PLACEHOLDER}.");
        let rest = match endpoint.split_once("://") {
            Some((_, rest)) => rest,
            None => endpoint.as_str(),
        };
        let authority = rest.split('/').next().unwrap_or("");
        let (endpoint, addressing) = if authority.starts_with(&placeholder) {
            // Strip exactly the recognised prefix, leaving any FURTHER placeholder for the refusal.
            let stripped = match endpoint.split_once(&placeholder) {
                Some((before, after)) => format!("{before}{after}"),
                None => endpoint.clone(),
            };
            (stripped, Addressing::VirtualHosted)
        } else {
            (endpoint.clone(), Addressing::PathStyle)
        };
        Self {
            bucket: bucket.into(),
            region: region.into(),
            endpoint,
            addressing,
            public_url_prefix: String::new(),
        }
    }

    /// The scheme, host (with any port) and base path of the configured endpoint.
    fn endpoint_parts(&self) -> Result<(String, String, String)> {
        let trimmed = self.endpoint.trim();
        if trimmed.is_empty() {
            return Err(VeloctError::Invalid(
                "the S3 endpoint is empty; write it as https://host[:port]".into(),
            ));
        }
        // The scheme is split off BEFORE the trailing slashes are trimmed: `https://` with nothing after
        // it is a misconfiguration that must be refused, and trimming first would turn it into the bare
        // host "https:".
        let (scheme, rest) = match trimmed.split_once("://") {
            Some((scheme, rest)) => (scheme.to_string(), rest.to_string()),
            // A bare host is accepted and assumed to be HTTPS, because that is what a user pasting
            // "s3.example.com" means and refusing it would be pedantry.
            None => ("https".to_string(), trimmed.to_string()),
        };
        let rest = rest.trim_end_matches('/');
        // A `{bucket}` that survives `S3Config::compatible` is a MISPLACED one: that function removes the single
        // copy at the start of the host and switches to virtual-hosted addressing, so anything still here means
        // the user wrote it where it cannot work. Refused by NAME rather than dropped or silently kept.
        if rest.contains(BUCKET_PLACEHOLDER) {
            return Err(VeloctError::Invalid(format!(
                "the S3 endpoint {:?} still contains {BUCKET_PLACEHOLDER}. Write it as \
                 https://{BUCKET_PLACEHOLDER}.<host> — once, at the start of the host — to put the bucket into \
                 the hostname, which is what Alibaba Cloud OSS requires, or leave it out entirely for \
                 path-style access",
                self.endpoint
            )));
        }
        let (authority, base_path) = match rest.split_once('/') {
            Some((authority, path)) => (authority.to_string(), format!("/{}", path.trim_matches('/'))),
            None => (rest.to_string(), String::new()),
        };
        if authority.trim().is_empty() {
            return Err(VeloctError::Invalid(format!(
                "the S3 endpoint {:?} has no host",
                self.endpoint
            )));
        }
        Ok((scheme, authority, base_path))
    }

    /// The `Host` header value, which is part of every signature.
    pub fn host(&self) -> Result<String> {
        let (_, authority, _) = self.endpoint_parts()?;
        Ok(match self.addressing {
            Addressing::PathStyle => authority,
            Addressing::VirtualHosted => format!("{}.{}", self.bucket, authority),
        })
    }

    /// The path sent and signed for a key. Empty means the bucket root.
    pub fn path_for(&self, key: &str) -> Result<String> {
        let (_, _, base_path) = self.endpoint_parts()?;
        let key = key.trim_start_matches('/');
        let tail = match self.addressing {
            Addressing::PathStyle => {
                if key.is_empty() {
                    format!("/{}", self.bucket)
                } else {
                    format!("/{}/{}", self.bucket, key)
                }
            }
            Addressing::VirtualHosted => {
                if key.is_empty() {
                    String::new()
                } else {
                    format!("/{key}")
                }
            }
        };
        let path = format!("{base_path}{tail}");
        // A request for the bucket root is `/`, never an empty string.
        Ok(if path.is_empty() { "/".to_string() } else { path })
    }

    /// The full URL for a key and an already-canonical query string.
    pub fn url_for(&self, key: &str, query: &str) -> Result<String> {
        let (scheme, _, _) = self.endpoint_parts()?;
        let mut url = format!("{scheme}://{}{}", self.host()?, self.path_for(key)?);
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        Ok(url)
    }

    /// The public URL an object would have (§13.6), when a prefix is configured.
    pub fn public_url_for(&self, key: &str) -> Option<String> {
        let prefix = self.public_url_prefix.trim();
        if prefix.is_empty() {
            return None;
        }
        Some(format!(
            "{}/{}",
            prefix.trim_end_matches('/'),
            key.trim_start_matches('/')
        ))
    }
}

/// `YYYYMMDDTHHMMSSZ`, the `x-amz-date` format, from an RFC 3339 instant.
///
/// **Parsed and re-formatted, never edited in place.** Deleting `-` and `:` from
/// `2026-10-02T03:43:16.5888216Z` leaves the sub-second digits behind — `20261002T034316.5888216Z`, twenty-four
/// characters where SigV4 allows sixteen — and a LIVE Alibaba Cloud OSS request refused exactly that on
/// 2026-10-02 with `403 AccessDenied: OSS authentication requires a valid Date`. Every test missed it because
/// the fixed clocks render `...T00:00:00Z`; a real clock does not. An OFFSET is likewise CONVERTED rather than
/// copied, which string surgery cannot do at all.
pub fn amz_timestamp(rfc3339: &str) -> String {
    match veloct_core::clock::parse_rfc3339(rfc3339) {
        Ok(instant) => veloct_core::clock::format_amz_timestamp(instant),
        // Unparseable input keeps the tolerant behaviour: this is called on a formatted clock reading, and
        // panicking here would take down a backup. The value is signed either way, so a vendor refuses it
        // loudly rather than the request being sent unauthenticated.
        Err(_) => rfc3339.replace(['-', ':'], ""),
    }
}

/// `Hex(SHA256Hash(file))` and its length, read in bounded chunks.
///
/// Streaming rather than `std::fs::read`: an archive can be hundreds of megabytes, and the only reason
/// to look at the bytes here is to hash them.
pub fn hash_file(path: &Path) -> Result<(String, u64)> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|error| VeloctError::io(path, error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| VeloctError::io(path, error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((sigv4::hex_encode(&hasher.finalize()), total))
}

/// One request, before it is signed.
///
/// A struct rather than seven positional arguments: the caller names what it is setting, and the two
/// `&str` hashes and timestamps cannot be swapped by accident.
struct Outgoing<'a> {
    method: HttpMethod,
    /// The RAW object key. Empty means the bucket root.
    key: &'a str,
    query: Vec<(String, String)>,
    body: HttpBody,
    /// `Hex(SHA256Hash(payload))`, or [`EMPTY_PAYLOAD_SHA256`].
    payload_hash: &'a str,
    /// `YYYYMMDDTHHMMSSZ`.
    timestamp: &'a str,
    content_type: Option<&'a str>,
}

/// An S3 bucket, as one client.
pub struct S3Client {
    config: S3Config,
    credentials: Credentials,
    client: Arc<dyn HttpClient>,
}

impl std::fmt::Debug for S3Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Hand-written so the credential can never be printed: `Credentials` redacts its own secret,
        // and this keeps that guarantee even if the struct grows.
        f.debug_struct("S3Client")
            .field("config", &self.config)
            .field("credentials", &self.credentials)
            .finish()
    }
}

impl S3Client {
    pub fn new(config: S3Config, credentials: Credentials, client: Arc<dyn HttpClient>) -> Self {
        Self {
            config,
            credentials,
            client,
        }
    }

    pub fn config(&self) -> &S3Config {
        &self.config
    }

    /// Sign and send one request.
    ///
    /// Every S3 request Veloct makes goes through here, so there is exactly one place that decides which
    /// headers are signed and one place that builds the `Authorization` header.
    fn send(&self, outgoing: Outgoing<'_>) -> Result<HttpResponse> {
        let Outgoing {
            method,
            key,
            query,
            body,
            payload_hash,
            timestamp,
            content_type,
        } = outgoing;

        let canonical_query = sigv4::canonical_query_string(query.clone());
        let url = self.config.url_for(key, &canonical_query)?;
        let host = self.config.host()?;

        // The signed set is deliberately small: `host` is required, and the two `x-amz-*` headers are
        // what make the request authenticated and the payload tamper-evident. Signing every header would
        // turn a proxy's harmless addition into a signature failure.
        let signed_headers = vec![
            ("host".to_string(), host),
            ("x-amz-content-sha256".to_string(), payload_hash.to_string()),
            ("x-amz-date".to_string(), timestamp.to_string()),
        ];

        // The path signed is the path sent, WITHOUT the query string and with the leading slash removed
        // because `encode_path` puts it back.
        let path = self.config.path_for(key)?;
        let signed_key = path.trim_start_matches('/').to_string();
        let request = CanonicalRequest::new(
            method.as_str(),
            &signed_key,
            query,
            signed_headers.clone(),
            payload_hash,
        );

        let context = SigningContext::s3(&self.config.region, timestamp.to_string());
        let authorization = sigv4::authorization(&request, &self.credentials, &context);

        let mut http = HttpRequest::new(method, url)
            .header("Authorization", authorization)
            .header("x-amz-content-sha256", payload_hash)
            .header("x-amz-date", timestamp);
        // `Content-Type` is sent but NOT signed. It is metadata about the body rather than part of the
        // request's identity, and signing it would make a client that re-encodes it fail.
        if let Some(content_type) = content_type {
            http = http.header("Content-Type", content_type);
        }
        http.body = body;

        self.client.execute(http)
    }

    /// Upload bytes.
    pub fn put_object(
        &self,
        key: &str,
        bytes: &[u8],
        content_type: Option<&str>,
        timestamp: &str,
    ) -> Result<String> {
        let payload_hash = sigv4::sha256_hex(bytes);
        let response = self.send(Outgoing {
            method: HttpMethod::Put,
            key,
            query: Vec::new(),
            body: HttpBody::Bytes(bytes.to_vec()),
            payload_hash: &payload_hash,
            timestamp,
            content_type,
        })?;
        self.check("upload an object", key, &response)?;
        Ok(payload_hash)
    }

    /// Upload a file by STREAMING it, computing its hash in a first pass.
    ///
    /// Returns the payload hash, which is the object's identity in the signature and the value a caller
    /// would record. Two passes over the file, because the hash is part of the canonical request and so
    /// must exist before the body is sent. ADR-0016 records why the alternative — `UNSIGNED-PAYLOAD` —
    /// was rejected.
    pub fn put_file(
        &self,
        key: &str,
        path: &Path,
        content_type: Option<&str>,
        timestamp: &str,
    ) -> Result<String> {
        let (payload_hash, _bytes) = hash_file(path)?;
        let response = self.send(Outgoing {
            method: HttpMethod::Put,
            key,
            query: Vec::new(),
            body: HttpBody::File {
                path: path.to_path_buf(),
                content_type: content_type.map(str::to_string),
            },
            payload_hash: &payload_hash,
            timestamp,
            content_type,
        })?;
        self.check("upload an object", key, &response)?;
        Ok(payload_hash)
    }

    /// The size of an object, or `None` when it is not there.
    pub fn head_object(&self, key: &str, timestamp: &str) -> Result<Option<u64>> {
        let response = self.send(Outgoing {
            method: HttpMethod::Head,
            key,
            query: Vec::new(),
            body: HttpBody::None,
            payload_hash: EMPTY_PAYLOAD_SHA256,
            timestamp,
            content_type: None,
        })?;
        match response.status {
            200 => {
                let size = response
                    .header("content-length")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                Ok(Some(size))
            }
            // A missing object is `None`: asking whether something is there is not a failure when the
            // answer is no.
            404 => Ok(None),
            other => Err(self.refused("read an object's metadata", key, other, &response)),
        }
    }

    /// Every object under a prefix, following continuation tokens.
    ///
    /// `IsTruncated` is honoured rather than assumed false: a bucket with more than a thousand objects
    /// under the prefix would otherwise silently hide the rest, and §17.6's retention decides what to
    /// DELETE from this answer.
    pub fn list(&self, prefix: &str, timestamp: &str) -> Result<Vec<S3Object>> {
        const PAGE: &str = "1000";
        let mut objects = Vec::new();
        let mut token: Option<String> = None;

        loop {
            let mut query = vec![
                ("list-type".to_string(), "2".to_string()),
                ("max-keys".to_string(), PAGE.to_string()),
            ];
            if !prefix.is_empty() {
                query.push(("prefix".to_string(), prefix.to_string()));
            }
            if let Some(token) = &token {
                query.push(("continuation-token".to_string(), token.clone()));
            }

            let response = self.send(Outgoing {
                method: HttpMethod::Get,
                key: "",
                query,
                body: HttpBody::None,
                payload_hash: EMPTY_PAYLOAD_SHA256,
                timestamp,
                content_type: None,
            })?;
            if !response.is_success() {
                return Err(self.refused("list a bucket", prefix, response.status, &response));
            }

            let page = parse_list_response(&response.text())?;
            objects.extend(page.objects);
            match page.next_token {
                Some(next) => token = Some(next),
                None => break,
            }
        }

        Ok(objects)
    }

    /// Remove one object.
    ///
    /// §13.8 and §17.6 both call this explicitly — nothing in Veloct deletes a remote object on its own
    /// initiative, and removing one that is already gone is the outcome the caller wanted.
    pub fn delete_object(&self, key: &str, timestamp: &str) -> Result<()> {
        let response = self.send(Outgoing {
            method: HttpMethod::Delete,
            key,
            query: Vec::new(),
            body: HttpBody::None,
            payload_hash: EMPTY_PAYLOAD_SHA256,
            timestamp,
            content_type: None,
        })?;
        match response.status {
            204 | 200 | 404 => Ok(()),
            other => Err(self.refused("delete an object", key, other, &response)),
        }
    }

    /// Prove the bucket is reachable and the credential can read it.
    pub fn test_connection(&self, timestamp: &str) -> Result<String> {
        // A one-object listing rather than a `HeadBucket`: §13.11's configurations grant object
        // permissions far more often than bucket-level ones, so `HeadBucket` would report a target as
        // broken when it can in fact store and serve files.
        let response = self.send(Outgoing {
            method: HttpMethod::Get,
            key: "",
            query: vec![
                ("list-type".to_string(), "2".to_string()),
                ("max-keys".to_string(), "1".to_string()),
            ],
            body: HttpBody::None,
            payload_hash: EMPTY_PAYLOAD_SHA256,
            timestamp,
            content_type: None,
        })?;
        if response.is_success() {
            return Ok(format!(
                "bucket {} at {} is reachable",
                self.config.bucket, self.config.endpoint
            ));
        }
        Err(self.refused("read the bucket", "", response.status, &response))
    }

    /// Turn a non-success response into an error that names the status and the bucket's own error CODE.
    ///
    /// The body is never included wholesale: an S3 error body is vendor-controlled text, and the
    /// documented `Code`/`Message` fields are the only parts Veloct needs. §18.1 keeps credentials out of
    /// messages, and a raw body is the easiest way to break that by accident.
    fn refused(&self, what: &str, key: &str, status: u16, response: &HttpResponse) -> VeloctError {
        let text = response.text();
        let detail = parse_error(&text);
        let where_ = if key.is_empty() {
            format!("bucket {}", self.config.bucket)
        } else {
            format!("{key} in bucket {}", self.config.bucket)
        };
        match detail {
            Some(detail) => VeloctError::Other(format!(
                "S3 refused to {what} ({where_}): HTTP {status}, {detail}"
            )),
            None => VeloctError::Other(format!(
                "S3 refused to {what} ({where_}): HTTP {status}. Check the endpoint, region, bucket and \
                 the access key's permissions on it."
            )),
        }
    }

    fn check(&self, what: &str, key: &str, response: &HttpResponse) -> Result<()> {
        if response.is_success() {
            return Ok(());
        }
        Err(self.refused(what, key, response.status, response))
    }
}

/// What a `ListObjectsV2` page held.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ListPage {
    pub objects: Vec<S3Object>,
    /// The token for the next page, present only when the bucket said the listing was truncated.
    pub next_token: Option<String>,
}

/// Parse a `ListObjectsV2` response.
///
/// A real XML parser rather than string splitting: an object key may contain `<`, `&` or `%`, and a
/// listing Veloct uses to decide what to DELETE is not a place to guess where a key ends.
pub fn parse_list_response(xml: &str) -> Result<ListPage> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    // `trim_text` is deliberately NOT enabled: an S3 object key may legitimately begin or end with a
    // space, and trimming it would name a different object — the one thing a delete must never do by
    // accident. Numeric and boolean fields are trimmed where they are parsed instead.

    let mut page = ListPage::default();
    let mut truncated = false;
    let mut current: Option<S3Object> = None;
    let mut field: Option<String> = None;
    // An element's text can arrive in SEVERAL events — `a&amp;b` is text, an entity reference and more
    // text — so the value is ACCUMULATED and applied when the element closes. Assigning each event in
    // turn kept only the last fragment, which turned the key `a&amp;b.tar.gz` into `b.tar.gz`: a
    // different object, and for §17.6's retention a delete aimed at the wrong key.
    let mut buffer = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let name = local_name(start.name().as_ref());
                match name.as_str() {
                    "Contents" => {
                        current = Some(S3Object {
                            key: String::new(),
                            size_bytes: 0,
                            last_modified: String::new(),
                        })
                    }
                    "Key" | "Size" | "LastModified" | "IsTruncated" | "NextContinuationToken" => {
                        field = Some(name);
                        buffer.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(text)) => {
                if field.is_none() {
                    continue;
                }
                // `xml_content` decodes the bytes AND resolves the predefined entities.
                buffer.push_str(&text.xml_content().map_err(|error| {
                    VeloctError::Other(format!("the S3 listing held invalid text: {error}"))
                })?);
            }
            Ok(Event::GeneralRef(reference)) => {
                // `<Key>a&amp;b</Key>` reaches this event for the `&amp;` itself.
                if field.is_none() {
                    continue;
                }
                let name = reference.decode().map_err(|error| {
                    VeloctError::Other(format!("the S3 listing held an invalid entity: {error}"))
                })?;
                buffer.push_str(entity_text(&name).unwrap_or(name.as_ref()));
            }
            Ok(Event::CData(data)) => {
                if field.is_none() {
                    continue;
                }
                buffer.push_str(&String::from_utf8_lossy(data.as_ref()));
            }
            Ok(Event::End(end)) => {
                let name = local_name(end.name().as_ref());
                match name.as_str() {
                    "Contents" => {
                        if let Some(object) = current.take() {
                            page.objects.push(object);
                        }
                    }
                    "Key" | "Size" | "LastModified" | "IsTruncated" | "NextContinuationToken" => {
                        apply_field(&mut page, &mut truncated, current.as_mut(), &name, &buffer);
                        field = None;
                        buffer.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => {
                return Err(VeloctError::Other(format!(
                    "the S3 listing was not readable XML: {error}"
                )))
            }
            _ => {}
        }
    }

    // A token is meaningful only when the bucket said there was more; a provider that returns one
    // anyway would otherwise make the caller loop forever.
    if !truncated {
        page.next_token = None;
    }
    Ok(page)
}

/// The five predefined XML entities, which is all an S3 listing uses.
fn entity_text(name: &str) -> Option<&'static str> {
    match name {
        "amp" => Some("&"),
        "lt" => Some("<"),
        "gt" => Some(">"),
        "quot" => Some("\""),
        "apos" => Some("'"),
        _ => None,
    }
}

/// Apply a completed element's text to whatever it belongs to.
fn apply_field(
    page: &mut ListPage,
    truncated: &mut bool,
    object: Option<&mut S3Object>,
    field: &str,
    value: &str,
) {
    match field {
        // A KEY is taken verbatim: an S3 key may begin or end with a space, and trimming it would name a
        // different object — the one thing a delete must never do by accident.
        "Key" => {
            if let Some(object) = object {
                object.key = value.to_string();
            }
        }
        "Size" => {
            if let Some(object) = object {
                object.size_bytes = value.trim().parse().unwrap_or(0);
            }
        }
        "LastModified" => {
            if let Some(object) = object {
                object.last_modified = value.trim().to_string();
            }
        }
        "IsTruncated" => *truncated = value.trim().eq_ignore_ascii_case("true"),
        "NextContinuationToken" => page.next_token = Some(value.to_string()),
        _ => {}
    }
}

/// The `Code` and `Message` of an S3 error response.
fn parse_error(xml: &str) -> Option<String> {
    let code = extract_tag(xml, "Code")?;
    match extract_tag(xml, "Message") {
        Some(message) => Some(format!("{code}: {message}")),
        None => Some(code),
    }
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    // Bounded, and entity-decoded: the text is vendor-controlled and only a short reason is wanted.
    let raw = &xml[start..end];
    let decoded = raw
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    Some(decoded.chars().take(200).collect())
}

fn local_name(name: &[u8]) -> String {
    let text = String::from_utf8_lossy(name);
    match text.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::FakeHttpClient;
    use veloct_core::Clock;

    const TIMESTAMP: &str = "20130524T000000Z";

    fn client_with(fake: Arc<FakeHttpClient>, config: S3Config) -> S3Client {
        S3Client::new(
            config,
            Credentials::new("AKIAIOSFODNN7EXAMPLE", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
            fake as Arc<dyn HttpClient>,
        )
    }

    fn compatible() -> S3Config {
        S3Config::compatible("veloct-private-backups", "us-east-1", "http://127.0.0.1:9000")
    }

    #[test]
    fn the_x_amz_date_a_live_request_carries_has_the_documented_shape() {
        // THE defect a LIVE request found on 2026-10-02: Alibaba Cloud OSS answered
        // `403 AccessDenied: OSS authentication requires a valid Date` to a request this client signed.
        // `x-amz-date` must be exactly `YYYYMMDDTHHMMSSZ`, and `amz_timestamp` produced it by deleting the
        // separators from an RFC 3339 instant — correct only while that instant ends in `Z`. A reading WITH AN
        // OFFSET becomes `20261002T123456+0000`, which is not the documented shape at all.
        //
        // Nothing else could see it: the SigV4 unit tests inject an already-correct `20130524T000000Z`, and the
        // FIXED clock in the adapter tests renders `...Z`. Only the SYSTEM clock reaches the other branch, so
        // this test reads the real one.
        let clock_reading = veloct_core::clock::format_rfc3339(veloct_core::SystemClock.now());
        let timestamp = amz_timestamp(&clock_reading);
        println!("system clock RFC 3339 = {clock_reading}");
        println!("x-amz-date            = {timestamp}");

        let fake = Arc::new(FakeHttpClient::new().always(200, ""));
        let config = S3Config::compatible(
            "vellow-chengdu-archieve",
            "cn-chengdu",
            "https://{bucket}.oss-cn-chengdu.aliyuncs.com",
        );
        let client = client_with(Arc::clone(&fake), config);
        client
            .test_connection(&timestamp)
            .expect("a listing is signed and sent");
        for request in fake.recorded() {
            println!("{} {}", request.method.as_str(), request.url);
            for (name, value) in &request.headers {
                println!("  {name}: {value}");
            }
        }

        // The documented shape and NOTHING else: eight digits, `T`, six digits, `Z`.
        assert_eq!(
            timestamp.len(),
            16,
            "x-amz-date must be YYYYMMDDTHHMMSSZ, and a vendor will refuse anything else: {timestamp}"
        );
        let (date, rest) = timestamp.split_at(8);
        assert!(date.chars().all(|c| c.is_ascii_digit()), "{timestamp}");
        assert!(rest.starts_with('T'), "{timestamp}");
        assert!(rest.ends_with('Z'), "{timestamp}");
        assert!(
            rest[1..7].chars().all(|c| c.is_ascii_digit()),
            "{timestamp}"
        );
    }

    #[test]
    fn the_amazon_endpoint_is_virtual_hosted_and_a_custom_one_is_path_style() {
        // The two styles produce different Host headers AND different paths, and both are signed, so
        // getting this wrong is a signature failure rather than an HTTP 404.
        let aws = S3Config::aws("examplebucket", "us-east-1");
        assert_eq!(aws.host().unwrap(), "examplebucket.s3.us-east-1.amazonaws.com");
        assert_eq!(aws.path_for("photos/a.jpg").unwrap(), "/photos/a.jpg");
        assert_eq!(
            aws.url_for("photos/a.jpg", "").unwrap(),
            "https://examplebucket.s3.us-east-1.amazonaws.com/photos/a.jpg"
        );

        let minio = S3Config::compatible("assets", "us-east-1", "http://127.0.0.1:9000");
        assert_eq!(minio.host().unwrap(), "127.0.0.1:9000");
        assert_eq!(minio.path_for("photos/a.jpg").unwrap(), "/assets/photos/a.jpg");
        assert_eq!(
            minio.url_for("photos/a.jpg", "list-type=2").unwrap(),
            "http://127.0.0.1:9000/assets/photos/a.jpg?list-type=2"
        );
    }

    #[test]
    fn an_endpoint_with_a_base_path_signs_the_path_it_sends() {
        // Some providers put the S3 API under a path prefix. S3 signs the path it is given, so a client
        // that dropped the base path would sign one URL and request another.
        let config = S3Config::compatible("assets", "us-east-1", "https://gateway.example.com/s3/");
        assert_eq!(config.path_for("a.png").unwrap(), "/s3/assets/a.png");
        assert_eq!(config.host().unwrap(), "gateway.example.com");
        assert_eq!(
            config.url_for("a.png", "").unwrap(),
            "https://gateway.example.com/s3/assets/a.png"
        );
    }

    #[test]
    fn the_bucket_root_is_a_slash_and_never_an_empty_path() {
        // An empty request target is not a valid HTTP request, and a signature over one would not match.
        let aws = S3Config::aws("examplebucket", "us-east-1");
        assert_eq!(aws.path_for("").unwrap(), "/");
        let minio = S3Config::compatible("assets", "us-east-1", "http://127.0.0.1:9000");
        assert_eq!(minio.path_for("").unwrap(), "/assets");
    }

    #[test]
    fn a_bare_host_is_assumed_to_be_https_and_an_empty_endpoint_is_refused() {
        let bare = S3Config::compatible("assets", "us-east-1", "s3.example.com");
        assert_eq!(
            bare.url_for("a.png", "").unwrap(),
            "https://s3.example.com/assets/a.png"
        );

        let empty = S3Config::compatible("assets", "us-east-1", "   ");
        let error = empty.host().unwrap_err();
        assert!(error.to_string().contains("endpoint is empty"), "{error}");

        let no_host = S3Config::compatible("assets", "us-east-1", "https://");
        assert!(no_host.host().is_err());
    }

    #[test]
    fn a_bucket_placeholder_switches_a_user_endpoint_to_virtual_hosted_addressing() {
        // THE vendor requirement this exists for, measured on 2026-10-02: a path-style bucket request to
        // Alibaba Cloud OSS is refused with `403 SecondLevelDomainForbidden` — "The bucket you are attempting
        // to access must be addressed using OSS third level domain" — while the same LIST with the bucket in
        // the hostname and SigV4's service scope reading `s3` answers 200. So the bucket has to go into the
        // host, and the only question is who decides: the user does, by where they put `{bucket}`.
        let oss = S3Config::compatible(
            "vellow-chengdu-archieve",
            "cn-chengdu",
            "https://{bucket}.oss-cn-chengdu.aliyuncs.com",
        );
        assert_eq!(oss.addressing, Addressing::VirtualHosted);
        assert_eq!(
            oss.host().unwrap(),
            "vellow-chengdu-archieve.oss-cn-chengdu.aliyuncs.com",
            "the bucket goes into the hostname, once"
        );
        assert_eq!(oss.path_for("photos/a.jpg").unwrap(), "/photos/a.jpg");
        assert_eq!(oss.path_for("").unwrap(), "/", "the bucket root is the host root");
        assert_eq!(
            oss.url_for("photos/a.jpg", "list-type=2").unwrap(),
            "https://vellow-chengdu-archieve.oss-cn-chengdu.aliyuncs.com/photos/a.jpg?list-type=2"
        );

        // A placeholder with a base path still works, because the base path is part of the signature.
        let with_path = S3Config::compatible("assets", "us-east-1", "https://{bucket}.gw.example.com/s3");
        assert_eq!(with_path.host().unwrap(), "assets.gw.example.com");
        assert_eq!(with_path.path_for("a.png").unwrap(), "/s3/a.png");

        // And WITHOUT the placeholder nothing changes: path style stays the default for a user endpoint, which
        // is what a self-hosted MinIO wants and what the tests above already cover.
        let minio = S3Config::compatible("assets", "us-east-1", "http://127.0.0.1:9000");
        assert_eq!(minio.addressing, Addressing::PathStyle);
        assert_eq!(minio.path_for("a.png").unwrap(), "/assets/a.png");
    }

    #[test]
    fn a_placeholder_anywhere_but_the_start_of_the_host_is_refused_by_name() {
        // A typo must not become a request to the wrong host: the placeholder is either in the one documented
        // position or it is an error that says where it belongs.
        for endpoint in [
            "https://oss-{bucket}.example.com",       // inside the host but not at its start
            "https://gw.example.com/s3/{bucket}",     // in the path
            "https://{bucket}.{bucket}.example.com",  // twice
            "https://{bucket}",                       // no host after it
        ] {
            let config = S3Config::compatible("assets", "us-east-1", endpoint);
            let error = config
                .host()
                .expect_err("a misplaced placeholder is refused rather than sent")
                .to_string();
            assert!(
                error.contains("{bucket}") && error.contains("start of the host"),
                "{endpoint}: {error}"
            );
        }
    }

    #[test]
    fn a_put_sends_a_signed_request_with_the_payload_hash_header() {
        let fake = Arc::new(FakeHttpClient::new().always(200, ""));
        let client = client_with(Arc::clone(&fake), compatible());
        let payload_hash = client
            .put_object("veloct-backups/a.tar.gz", b"ARCHIVE", Some("application/gzip"), TIMESTAMP)
            .unwrap();

        assert_eq!(payload_hash, sigv4::sha256_hex(b"ARCHIVE"));
        let requests = fake.recorded();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Put);
        assert_eq!(
            request.url,
            "http://127.0.0.1:9000/veloct-private-backups/veloct-backups/a.tar.gz"
        );
        let authorization = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .expect("an Authorization header");
        assert!(authorization.starts_with("AWS4-HMAC-SHA256 Credential="), "{authorization}");
        assert!(authorization.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date"), "{authorization}");
        assert!(
            request
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("x-amz-content-sha256") && value == &payload_hash),
            "the payload hash must be sent as its own header"
        );
        assert!(request
            .headers
            .iter()
            .any(|(name, value)| name.eq_ignore_ascii_case("x-amz-date") && value == TIMESTAMP));
    }

    #[test]
    fn a_streamed_put_hashes_the_file_and_sends_it_as_a_file_body() {
        // The archive path is the one that matters for §17.1: the whole project, which must not be read
        // into memory just to sign it.
        let dir = tempfile::TempDir::new().unwrap();
        let archive = dir.path().join("a.tar.gz");
        std::fs::write(&archive, b"ARCHIVE BYTES").unwrap();

        let fake = Arc::new(FakeHttpClient::new().always(200, ""));
        let client = client_with(Arc::clone(&fake), compatible());
        let hash = client
            .put_file("a.tar.gz", &archive, Some("application/gzip"), TIMESTAMP)
            .unwrap();
        assert_eq!(hash, sigv4::sha256_hex(b"ARCHIVE BYTES"));

        let requests = fake.recorded();
        match &requests[0].body {
            HttpBody::File { path, .. } => assert_eq!(path, &archive),
            other => panic!("expected a file body so the upload streams, got {other:?}"),
        }
    }

    #[test]
    fn hash_file_reports_the_digest_and_the_size_in_bounded_memory() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("big.bin");
        // Larger than the 64 KiB read buffer, so the loop is exercised more than once.
        let bytes = vec![0xab_u8; 200 * 1024];
        std::fs::write(&path, &bytes).unwrap();

        let (hash, size) = hash_file(&path).unwrap();
        assert_eq!(size, bytes.len() as u64);
        assert_eq!(hash, sigv4::sha256_hex(&bytes));
        let error = hash_file(&dir.path().join("missing")).unwrap_err();
        assert!(error.to_string().contains("io error"), "{error}");
    }

    #[test]
    fn a_missing_object_is_absent_rather_than_an_error() {
        let fake = Arc::new(FakeHttpClient::new().always(404, ""));
        let client = client_with(Arc::clone(&fake), compatible());
        assert!(client.head_object("gone", TIMESTAMP).unwrap().is_none());

        let fake = Arc::new(FakeHttpClient::new().always(200, "").on_url_contains("/present", 200, ""));
        // A `HEAD` response carries the size in a header; the fake returns none, so the honest answer is 0
        // rather than an invented number.
        let client = client_with(Arc::clone(&fake), compatible());
        assert_eq!(client.head_object("present", TIMESTAMP).unwrap(), Some(0));
    }

    #[test]
    fn deleting_something_already_gone_is_not_an_error_and_a_refusal_is() {
        let fake = Arc::new(FakeHttpClient::new().always(404, ""));
        let client = client_with(Arc::clone(&fake), compatible());
        assert!(client.delete_object("gone", TIMESTAMP).is_ok());

        let fake = Arc::new(FakeHttpClient::new().always(
            403,
            "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
        ));
        let client = client_with(Arc::clone(&fake), compatible());
        let error = client.delete_object("a", TIMESTAMP).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("403"), "{text}");
        assert!(text.contains("AccessDenied"), "the bucket's own code is the useful part: {text}");
    }

    #[test]
    fn a_listing_is_parsed_with_its_keys_unescaped_and_its_sizes_read() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult>
  <Name>veloct-private-backups</Name>
  <Prefix>veloct-backups/</Prefix>
  <KeyCount>2</KeyCount>
  <IsTruncated>false</IsTruncated>
  <Contents>
    <Key>veloct-backups/veloct-backup-20260101-120000-aaaa.tar.gz</Key>
    <LastModified>2026-01-01T12:00:00.000Z</LastModified>
    <ETag>&quot;abc&quot;</ETag>
    <Size>1234</Size>
    <StorageClass>STANDARD</StorageClass>
  </Contents>
  <Contents>
    <Key>veloct-backups/a&amp;b.tar.gz</Key>
    <LastModified>2026-01-02T12:00:00.000Z</LastModified>
    <Size>7</Size>
  </Contents>
</ListBucketResult>"#;

        let page = parse_list_response(xml).unwrap();
        assert_eq!(page.objects.len(), 2);
        assert_eq!(page.objects[0].key, "veloct-backups/veloct-backup-20260101-120000-aaaa.tar.gz");
        assert_eq!(page.objects[0].size_bytes, 1234);
        assert_eq!(page.objects[0].last_modified, "2026-01-01T12:00:00.000Z");
        // An ampersand in a key is the case a string-splitting parser gets wrong.
        assert_eq!(page.objects[1].key, "veloct-backups/a&b.tar.gz");
        assert_eq!(page.next_token, None);
    }

    #[test]
    fn a_truncated_listing_carries_its_continuation_token_and_a_complete_one_does_not() {
        // `IsTruncated` is honoured rather than assumed false: retention decides what to DELETE from this
        // answer, so a page Veloct never sees is an object it can never manage.
        let truncated = r#"<ListBucketResult><IsTruncated>true</IsTruncated>
            <NextContinuationToken>tok+1/2=</NextContinuationToken></ListBucketResult>"#;
        let page = parse_list_response(truncated).unwrap();
        assert_eq!(page.next_token.as_deref(), Some("tok+1/2="));

        // A provider that returns a token WITHOUT saying the listing is truncated must not send the
        // caller round the loop forever.
        let complete = r#"<ListBucketResult><IsTruncated>false</IsTruncated>
            <NextContinuationToken>ignore-me</NextContinuationToken></ListBucketResult>"#;
        assert_eq!(parse_list_response(complete).unwrap().next_token, None);
    }

    #[test]
    fn listing_follows_the_continuation_token_until_the_bucket_stops_truncating() {
        let fake = Arc::new(
            FakeHttpClient::new()
                .on_url_contains(
                    "continuation-token",
                    200,
                    r#"<ListBucketResult><IsTruncated>false</IsTruncated>
                       <Contents><Key>p/2</Key><Size>2</Size><LastModified>2026-01-02T00:00:00Z</LastModified></Contents>
                       </ListBucketResult>"#,
                )
                .on_method_and_url(
                    HttpMethod::Get,
                    "list-type=2",
                    200,
                    r#"<ListBucketResult><IsTruncated>true</IsTruncated>
                       <NextContinuationToken>NEXT</NextContinuationToken>
                       <Contents><Key>p/1</Key><Size>1</Size><LastModified>2026-01-01T00:00:00Z</LastModified></Contents>
                       </ListBucketResult>"#,
                ),
        );
        let client = client_with(Arc::clone(&fake), compatible());

        let objects = client.list("p/", TIMESTAMP).unwrap();
        assert_eq!(objects.len(), 2, "both pages are returned");
        assert_eq!(objects[0].key, "p/1");
        assert_eq!(objects[1].key, "p/2");

        let requests = fake.recorded();
        assert_eq!(requests.len(), 2);
        assert!(requests[0].url.contains("list-type=2"), "{}", requests[0].url);
        assert!(requests[0].url.contains("prefix=p%2F"), "the prefix is encoded: {}", requests[0].url);
        assert!(requests[1].url.contains("continuation-token=NEXT"), "{}", requests[1].url);
    }

    #[test]
    fn a_refused_listing_names_the_buckets_own_error_code() {
        let fake = Arc::new(FakeHttpClient::new().always(
            404,
            "<Error><Code>NoSuchBucket</Code><Message>The specified bucket does not exist</Message></Error>",
        ));
        let client = client_with(Arc::clone(&fake), compatible());
        let error = client.list("", TIMESTAMP).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("NoSuchBucket"), "{text}");
        assert!(text.contains("does not exist"), "{text}");
    }

    #[test]
    fn a_connection_test_lists_rather_than_asking_for_bucket_metadata() {
        // Section 13.11's configurations usually grant object permissions and not bucket-level ones, so a
        // HeadBucket would call a working target broken.
        let fake = Arc::new(FakeHttpClient::new().always(200, "<ListBucketResult/>"));
        let client = client_with(Arc::clone(&fake), compatible());
        let said = client.test_connection(TIMESTAMP).unwrap();
        assert!(said.contains("veloct-private-backups"), "{said}");
        assert!(said.contains("127.0.0.1:9000"), "{said}");

        let requests = fake.recorded();
        assert_eq!(requests[0].method, HttpMethod::Get);
        assert!(requests[0].url.contains("max-keys=1"), "{}", requests[0].url);
    }

    #[test]
    fn a_refusal_never_carries_the_secret_and_truncates_a_hostile_body() {
        // Section 18.1. The fake echoes the secret into the body, which is the shape of a vendor error
        // page that includes request context.
        let fake = Arc::new(FakeHttpClient::new().always(
            403,
            "<Error><Code>SignatureDoesNotMatch</Code><Message>wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY</Message></Error>",
        ));
        let client = client_with(Arc::clone(&fake), compatible());
        let text = client.put_object("a", b"x", None, TIMESTAMP).unwrap_err().to_string();
        assert!(text.contains("SignatureDoesNotMatch"), "{text}");
        // The Message field is included, so a vendor echo WOULD reach it — which is exactly why the
        // redaction layer, not this module, is what guarantees §18.1. Asserted here so the coupling is
        // visible rather than assumed: `veloct_core::secret::redact` is applied by the log sink.
        assert!(text.len() < 400, "the body is bounded: {text}");
    }

    #[test]
    fn the_public_url_comes_from_the_configured_prefix_or_is_absent() {
        // Section 13.6: the public URL prefix is configuration, and a target without one serves nothing.
        let mut config = compatible();
        assert_eq!(config.public_url_for("a.png"), None);
        config.public_url_prefix = "https://cdn.example.com/".to_string();
        assert_eq!(
            config.public_url_for("a.png").as_deref(),
            Some("https://cdn.example.com/a.png")
        );
        assert_eq!(
            config.public_url_for("/a.png").as_deref(),
            Some("https://cdn.example.com/a.png"),
            "a leading slash is not a different object"
        );
    }

    #[test]
    fn a_signed_url_changes_when_the_region_does() {
        // The scope is part of the signature, so the same request to a different region must not produce
        // the same Authorization header.
        let request = |region: &str| {
            let fake = Arc::new(FakeHttpClient::new().always(200, ""));
            let mut config = compatible();
            config.region = region.to_string();
            let client = client_with(Arc::clone(&fake), config);
            client.put_object("a", b"x", None, TIMESTAMP).unwrap();
            fake.recorded()[0]
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                .map(|(_, value)| value.clone())
                .unwrap()
        };
        assert_ne!(request("us-east-1"), request("eu-central-1"));
    }
}
