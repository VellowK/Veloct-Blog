//! The single HTTP seam for every network-using crate.
//!
//! Deployment adapters, AI providers, attachment storage adapters and backup cloud targets all
//! need HTTP, and all of them need to be testable without a live vendor. Defining one tiny
//! blocking interface here means:
//!
//! * every crate can be tested against [`FakeHttpClient`] (or a local fake server) instead of
//!   inventing its own mocking story;
//! * no crate has to depend on `reqwest` directly, so the TLS and runtime choice is made once;
//! * cancellation is expressible, which `handoff.md` §11.4/§25 C1 requires for AI streaming.
//!
//! The interface is deliberately blocking. Veloct's service layer is synchronous and Tauri
//! commands run on a worker thread, so a blocking client keeps the filesystem, SQLite and HTTP
//! code in one concurrency model.

use std::collections::HashMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Result, VeloctError};

/// HTTP verbs Veloct uses. A closed enum keeps call sites honest about what they do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
        }
    }
}

/// Request body.
#[derive(Debug, Clone)]
pub enum HttpBody {
    None,
    Bytes(Vec<u8>),
    /// Serialised as JSON with `Content-Type: application/json`.
    Json(serde_json::Value),
    Text(String),
    /// A file streamed from disk, so a large work or attachment never has to be buffered.
    File {
        path: std::path::PathBuf,
        content_type: Option<String>,
    },
}

impl HttpBody {
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::None)
    }

    /// The body as JSON, when it is one.
    ///
    /// Used by tests to assert what a provider actually sent, without making `HttpBody` itself
    /// serialisable: a request body can be a file path, which has no meaningful JSON form.
    pub fn as_json(&self) -> Option<&serde_json::Value> {
        match self {
            Self::Json(value) => Some(value),
            _ => None,
        }
    }

    /// The body as text, when it is text, bytes or JSON.
    pub fn as_text(&self) -> Option<String> {
        match self {
            Self::None => Some(String::new()),
            Self::Text(text) => Some(text.clone()),
            Self::Bytes(bytes) => Some(String::from_utf8_lossy(bytes).to_string()),
            Self::Json(value) => serde_json::to_string(value).ok(),
            Self::File { .. } => None,
        }
    }
}

/// A pending HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: HttpBody,
    pub timeout: Option<Duration>,
}

impl HttpRequest {
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: Vec::new(),
            body: HttpBody::None,
            timeout: None,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Get, url)
    }

    pub fn post(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Post, url)
    }

    pub fn put(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Put, url)
    }

    pub fn delete(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Delete, url)
    }

    /// Insert a header, mutating in place (used by the builder methods that already own `self`).
    fn set_header(&mut self, name: &str, value: &str) {
        self.headers
            .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        self.headers.push((name.to_string(), value.to_string()));
    }

    /// Set a header, replacing any previous value with the same (case-insensitive) name.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        let value = value.into();
        self.set_header(&name, &value);
        self
    }

    /// Set a bearer token. The value is never logged: diagnostics go through
    /// [`crate::secret::redact`].
    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }

    pub fn json(mut self, value: serde_json::Value) -> Self {
        self.body = HttpBody::Json(value);
        self.set_header("Content-Type", "application/json");
        self
    }

    pub fn bytes(mut self, bytes: Vec<u8>) -> Self {
        self.body = HttpBody::Bytes(bytes);
        self
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.body = HttpBody::Text(text.into());
        self
    }

    pub fn file(mut self, path: impl Into<std::path::PathBuf>, content_type: Option<String>) -> Self {
        self.body = HttpBody::File {
            path: path.into(),
            content_type,
        };
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

/// A completed HTTP response. The body is already read.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.body)
    }

    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.body).map_err(|e| {
            VeloctError::Other(format!(
                "response was not valid JSON (status {}): {e}",
                self.status
            ))
        })
    }

    /// Turn a non-2xx response into an error that names the status and a redacted body excerpt.
    ///
    /// The body is redacted because a provider error body can echo a credential back
    /// (`handoff.md` §18.1).
    pub fn error_for_status(self, what: &str) -> Result<Self> {
        if self.is_success() {
            return Ok(self);
        }
        let excerpt: String = crate::secret::redact(&self.text())
            .chars()
            .take(400)
            .collect();
        Err(VeloctError::Other(format!(
            "{what} failed with HTTP {}: {excerpt}",
            self.status
        )))
    }
}

/// A cancellation handle for a streaming request.
///
/// `handoff.md` §11.4/§25 C1: when the user stops generation the request must be cancellable
/// and treated as `CANCELLED` rather than as a provider error.
#[derive(Debug, Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn new() -> Self {
        Self::default()
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// Return [`VeloctError::Cancelled`] when cancellation was requested.
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(VeloctError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// One increment of a streaming response.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Response headers arrived; the status is known.
    Status(u16),
    /// A chunk of the body.
    Chunk(Vec<u8>),
    /// The body is complete.
    End,
}

/// The HTTP capability. Implemented by [`ReqwestHttpClient`] in production and
/// [`FakeHttpClient`] in tests.
pub trait HttpClient: Send + Sync + std::fmt::Debug + std::any::Any {
    /// The concrete type, so a reference can be shared as an owned handle when a trait object needs one.
    fn as_any(&self) -> &dyn std::any::Any;
    /// Perform a request and read the whole body.
    fn execute(&self, request: HttpRequest) -> Result<HttpResponse>;

    /// Perform a request and stream the body.
    ///
    /// The implementation must stop reading once `cancel` is set and must return
    /// [`VeloctError::Cancelled`] in that case.
    fn stream(
        &self,
        request: HttpRequest,
        cancel: &Cancellation,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamEvent>> + Send>>;
}

/// A shared client is itself a client.
///
/// This is what lets an adapter SHARE its client rather than borrow it. The requirement is concrete: a
/// `Box<dyn DeployAdapter>` is `'static`, so an adapter that holds `&'a dyn HttpClient` cannot be boxed into
/// one — and `veloct-deploy`'s `CloudflarePagesAdapter` did exactly that, which made it impossible for the
/// application to build adapters and keep them. Sharing removes the lifetime instead of working around it.
impl<T: HttpClient + ?Sized> HttpClient for std::sync::Arc<T> {
    fn as_any(&self) -> &dyn std::any::Any {
        // The Arc itself is the concrete type, so a shared client can be shared again.
        self
    }

    fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        (**self).execute(request)
    }

    fn stream(
        &self,
        request: HttpRequest,
        cancel: &Cancellation,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamEvent>> + Send>> {
        (**self).stream(request, cancel)
    }
}

/// Share a client as an owned handle.
///
/// `Box<dyn DeployAdapter>` is `'static`, so an adapter cannot hold `&'a dyn HttpClient`. This turns a borrowed
/// trait object into an `Arc` BY DOWNCASTING TO ITS CONCRETE TYPE, which is why [`HttpClient`] requires `Any`.
///
/// Returns `None` for an implementor that is not itself shareable, so the caller can report that rather than
/// panicking: a client that cannot be shared is a configuration problem, not a crash.
pub fn share(client: &dyn HttpClient) -> Option<std::sync::Arc<dyn HttpClient>> {
    if let Some(shared) = client.as_any().downcast_ref::<std::sync::Arc<dyn HttpClient>>() {
        return Some(shared.clone());
    }
    if let Some(reqwest) = client.as_any().downcast_ref::<ReqwestHttpClient>() {
        return Some(std::sync::Arc::new(reqwest.clone()));
    }
    if let Some(fake) = client.as_any().downcast_ref::<FakeHttpClient>() {
        return Some(std::sync::Arc::new(fake.clone()));
    }
    None
}

/// The production client, backed by a blocking `reqwest` client with rustls.
#[derive(Debug, Clone)]
pub struct ReqwestHttpClient {
    inner: reqwest::blocking::Client,
}

impl ReqwestHttpClient {
    /// Build a client with a default timeout and a user agent identifying Veloct.
    pub fn new() -> Result<Self> {
        Self::with_default_timeout(Duration::from_secs(60))
    }

    pub fn with_default_timeout(timeout: Duration) -> Result<Self> {
        let inner = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .user_agent(format!("Veloct/{}", crate::VERSION))
            // Redirects are followed, but a deployment upload must never be silently sent
            // somewhere else, so this is kept modest.
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| VeloctError::Other(format!("could not build the HTTP client: {e}")))?;
        Ok(Self { inner })
    }

    fn build(&self, request: &HttpRequest) -> Result<reqwest::blocking::RequestBuilder> {
        let method = match request.method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Head => reqwest::Method::HEAD,
        };
        let mut builder = self.inner.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(timeout) = request.timeout {
            builder = builder.timeout(timeout);
        }
        builder = match &request.body {
            HttpBody::None => builder,
            HttpBody::Bytes(bytes) => builder.body(bytes.clone()),
            HttpBody::Text(text) => builder.body(text.clone()),
            HttpBody::Json(value) => builder.json(value),
            HttpBody::File { path, content_type } => {
                let file = std::fs::File::open(path).map_err(|e| VeloctError::io(path, e))?;
                let length = file
                    .metadata()
                    .map_err(|e| VeloctError::io(path, e))?
                    .len();
                let body = reqwest::blocking::Body::sized(file, length);
                match content_type {
                    Some(ct) => builder.header("Content-Type", ct.as_str()).body(body),
                    None => builder.body(body),
                }
            }
        };
        Ok(builder)
    }

    fn collect(response: reqwest::blocking::Response) -> Result<HttpResponse> {
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    String::from_utf8_lossy(v.as_bytes()).to_string(),
                )
            })
            .collect();
        let body = response
            .bytes()
            .map_err(|e| VeloctError::Other(format!("could not read the response body: {e}")))?
            .to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    fn send_error(error: reqwest::Error, request: &HttpRequest) -> VeloctError {
        // Never include headers in the message: an Authorization header would leak.
        let redacted_url = redact_url(&request.url);
        if error.is_timeout() {
            VeloctError::Other(format!("request to {redacted_url} timed out"))
        } else if error.is_connect() {
            VeloctError::Other(format!("could not connect to {redacted_url}"))
        } else {
            VeloctError::Other(format!("request to {redacted_url} failed: {error}"))
        }
    }
}

/// Strip userinfo and query values from a URL so it is safe to log.
pub fn redact_url(url: &str) -> String {
    let without_query = match url.split_once('?') {
        Some((head, _)) => format!("{head}?[REDACTED]"),
        None => url.to_string(),
    };
    match without_query.split_once("://") {
        Some((scheme, rest)) => match rest.split_once('@') {
            Some((_, host)) => format!("{scheme}://{host}"),
            None => without_query,
        },
        None => without_query,
    }
}

impl HttpClient for ReqwestHttpClient {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let builder = self.build(&request)?;
        let response = builder
            .send()
            .map_err(|e| Self::send_error(e, &request))?;
        Self::collect(response)
    }

    fn stream(
        &self,
        request: HttpRequest,
        cancel: &Cancellation,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamEvent>> + Send>> {
        let builder = self.build(&request)?;
        let mut response = builder
            .send()
            .map_err(|e| Self::send_error(e, &request))?;
        let status = response.status().as_u16();
        let cancellation = cancel.clone();
        let (sender, receiver) = std::sync::mpsc::channel();

        // A dedicated thread performs the blocking reads, so the caller can stop consuming and
        // thereby signal cancellation without waiting for the network.
        std::thread::spawn(move || {
            if sender.send(Ok(StreamEvent::Status(status))).is_err() {
                return;
            }
            let mut buffer = [0u8; 8192];
            loop {
                if cancellation.is_cancelled() {
                    let _ = sender.send(Err(VeloctError::Cancelled));
                    return;
                }
                match response.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        if sender
                            .send(Ok(StreamEvent::Chunk(buffer[..read].to_vec())))
                            .is_err()
                        {
                            // The consumer went away, which is how a caller cancels.
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(VeloctError::Other(format!(
                            "stream read failed: {error}"
                        ))));
                        return;
                    }
                }
            }
            let _ = sender.send(Ok(StreamEvent::End));
        });

        Ok(Box::new(receiver.into_iter()))
    }
}

/// A scriptable in-memory client for tests.
///
/// Vendor protocols are tested through this (or through a local fake HTTP server), never
/// against a live service, and the test name must say so.
/// A test double that can be SHARED.
///
/// The `Arc`s are the point rather than an optimisation. `veloct-deploy`'s Cloudflare adapter must be
/// `Box<dyn DeployAdapter>`, which is `'static`, so an adapter holding `&'a dyn HttpClient` cannot be boxed
/// into one — which is why the APPLICATION could not construct an adapter at all. Making the fake shareable
/// lets the adapter own a clone that writes to the SAME log, so a test still sees the requests the adapter
/// made.
///
/// The first version of this clone deep-copied the responses and gave the clone a fresh, empty request log,
/// and `the_cloudflare_adapter_uploads_then_creates_a_deployment` failed with `[]` — the test was right and
/// the clone was wrong.
#[derive(Debug, Default, Clone)]
pub struct FakeHttpClient {
    responses: std::sync::Arc<Mutex<Vec<FakeResponse>>>,
    pub requests: std::sync::Arc<Mutex<Vec<HttpRequest>>>,
}

#[derive(Debug, Clone)]
struct FakeResponse {
    matcher: FakeMatcher,
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug, Clone)]
enum FakeMatcher {
    AnyUrl,
    UrlContains(String),
    MethodAndUrlContains(HttpMethod, String),
}

impl FakeHttpClient {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue a response for any request.
    pub fn always(self, status: u16, body: impl Into<Vec<u8>>) -> Self {
        self.responses
            .lock()
            .expect("the response lock is not poisoned")
            .push(FakeResponse {
            matcher: FakeMatcher::AnyUrl,
            status,
            headers: Vec::new(),
            body: body.into(),
        });
        self
    }

    /// Queue a response for the first request whose URL contains `fragment`.
    pub fn on_url_contains(
        self,
        fragment: impl Into<String>,
        status: u16,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        self.responses
            .lock()
            .expect("the response lock is not poisoned")
            .push(FakeResponse {
            matcher: FakeMatcher::UrlContains(fragment.into()),
            status,
            headers: Vec::new(),
            body: body.into(),
        });
        self
    }

    /// Queue a response for the first request with this method whose URL contains `fragment`.
    ///
    /// Needed because deployment and storage protocols reuse a path for different verbs (for
    /// example an S3-style `PUT` object versus a `DELETE` object).
    pub fn on_method_and_url(
        self,
        method: HttpMethod,
        fragment: impl Into<String>,
        status: u16,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        self.responses
            .lock()
            .expect("the response lock is not poisoned")
            .push(FakeResponse {
            matcher: FakeMatcher::MethodAndUrlContains(method, fragment.into()),
            status,
            headers: Vec::new(),
            body: body.into(),
        });
        self
    }

    /// Queue a JSON response.
    pub fn on_json(self, fragment: impl Into<String>, value: serde_json::Value) -> Self {
        self.responses
            .lock()
            .expect("the response lock is not poisoned")
            .push(FakeResponse {
            matcher: FakeMatcher::UrlContains(fragment.into()),
            status: 200,
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(&value).expect("serialisable fixture"),
        });
        self
    }

    /// Number of requests recorded so far.
    pub fn request_count(&self) -> usize {
        self.requests.lock().expect("mutex").len()
    }

    /// All recorded requests.
    pub fn recorded(&self) -> Vec<HttpRequest> {
        self.requests.lock().expect("mutex").clone()
    }

    fn take_matching(&self, request: &HttpRequest) -> FakeResponse {
        let mut queue = self.responses.lock().expect("mutex");
        let index = queue.iter().position(|candidate| match &candidate.matcher {
            FakeMatcher::AnyUrl => true,
            FakeMatcher::UrlContains(fragment) => request.url.contains(fragment.as_str()),
            FakeMatcher::MethodAndUrlContains(method, fragment) => {
                *method == request.method && request.url.contains(fragment.as_str())
            }
        });
        match index {
            Some(index) => queue.remove(index),
            None => FakeResponse {
                matcher: FakeMatcher::AnyUrl,
                status: 404,
                headers: Vec::new(),
                body: format!(
                    "FakeHttpClient has no response queued for {} {}",
                    request.method.as_str(),
                    redact_url(&request.url)
                )
                .into_bytes(),
            },
        }
    }
}

impl HttpClient for FakeHttpClient {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        self.requests.lock().expect("mutex").push(request.clone());
        let response = self.take_matching(&request);
        Ok(HttpResponse {
            status: response.status,
            headers: response.headers,
            body: response.body,
        })
    }

    fn stream(
        &self,
        request: HttpRequest,
        cancel: &Cancellation,
    ) -> Result<Box<dyn Iterator<Item = Result<StreamEvent>> + Send>> {
        let response = self.execute(request)?;
        let cancellation = cancel.clone();
        let mut events: Vec<Result<StreamEvent>> = vec![Ok(StreamEvent::Status(response.status))];
        // Split into chunks so a stream consumer has to actually reassemble them.
        for chunk in response.body.chunks(256) {
            events.push(Ok(StreamEvent::Chunk(chunk.to_vec())));
        }
        events.push(Ok(StreamEvent::End));
        Ok(Box::new(CancellableEvents {
            inner: events.into_iter(),
            cancel: cancellation,
        }))
    }
}

struct CancellableEvents {
    inner: std::vec::IntoIter<Result<StreamEvent>>,
    cancel: Cancellation,
}

impl Iterator for CancellableEvents {
    type Item = Result<StreamEvent>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cancel.is_cancelled() {
            return Some(Err(VeloctError::Cancelled));
        }
        self.inner.next()
    }
}

/// Header map helper used by adapters that need case-insensitive lookup.
pub fn header_map(headers: &[(String, String)]) -> HashMap<String, String> {
    headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_client_records_requests_and_matches_by_url() {
        let client = FakeHttpClient::new()
            .on_json("/user/repos", serde_json::json!({ "ok": true }))
            .always(500, "nope");

        let response = client
            .execute(HttpRequest::get("https://api.example.com/user/repos").bearer("ghp_secret"))
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.json::<serde_json::Value>().unwrap()["ok"], true);
        assert_eq!(client.request_count(), 1);

        // The recorded request keeps the real header, but nothing logs it.
        let recorded = client.recorded();
        assert!(recorded[0]
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v.starts_with("Bearer ")));

        let second = client.execute(HttpRequest::get("https://api.example.com/other")).unwrap();
        assert_eq!(second.status, 500);
    }

    #[test]
    fn unqueued_requests_fail_loudly_instead_of_silently_succeeding() {
        let client = FakeHttpClient::new();
        let response = client.execute(HttpRequest::get("https://api.example.com/x")).unwrap();
        assert_eq!(response.status, 404);
        assert!(response.text().contains("no response queued"));
    }

    #[test]
    fn error_for_status_redacts_credentials_from_the_body() {
        let body = "unauthorized: token sk-ant-api03-abcdefghijklmnopqrstuvwxyz rejected";
        let response = HttpResponse {
            status: 401,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        };
        let error = response.error_for_status("provider call").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("HTTP 401"));
        assert!(!message.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz"), "{message}");
    }

    #[test]
    fn success_status_passes_through_error_for_status() {
        let response = HttpResponse {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
        };
        assert!(response.error_for_status("upload").is_ok());
    }

    #[test]
    fn cancellation_stops_a_stream_and_reports_cancelled() {
        // handoff §11.4 / §25 C1.
        let client = FakeHttpClient::new().always(200, "hello world");
        let cancel = Cancellation::new();
        let mut events = client
            .stream(HttpRequest::get("https://example.com"), &cancel)
            .unwrap();
        assert!(matches!(events.next(), Some(Ok(StreamEvent::Status(200)))));
        cancel.cancel();
        let next = events.next().unwrap();
        assert!(matches!(next, Err(VeloctError::Cancelled)), "{next:?}");
    }

    #[test]
    fn stream_reassembles_chunked_bodies() {
        let payload = "x".repeat(1000);
        let client = FakeHttpClient::new().always(200, payload.clone());
        let cancel = Cancellation::new();
        let mut collected = Vec::new();
        let mut chunks = 0;
        for event in client
            .stream(HttpRequest::get("https://example.com"), &cancel)
            .unwrap()
        {
            match event.unwrap() {
                StreamEvent::Status(200) => {}
                StreamEvent::Chunk(bytes) => {
                    chunks += 1;
                    collected.extend_from_slice(&bytes);
                }
                StreamEvent::Status(other) => panic!("unexpected status {other}"),
                StreamEvent::End => break,
            }
        }
        assert_eq!(String::from_utf8(collected).unwrap(), payload);
        assert!(chunks > 1, "the fake must actually chunk the body");
    }

    #[test]
    fn urls_are_redacted_before_logging() {
        // handoff §18.1: no credentials in logs.
        assert_eq!(
            redact_url("https://user:pass@example.com/path?token=abc"),
            "https://example.com/path?[REDACTED]"
        );
        assert_eq!(redact_url("https://example.com/plain"), "https://example.com/plain");
    }

    #[test]
    fn request_builder_sets_headers_without_duplicates() {
        let request = HttpRequest::get("https://example.com")
            .header("X-A", "1")
            .header("x-a", "2");
        assert_eq!(request.headers.len(), 1);
        assert_eq!(request.headers[0].1, "2");
    }

    #[test]
    fn json_body_sets_the_content_type() {
        let request = HttpRequest::post("https://example.com").json(serde_json::json!({ "a": 1 }));
        assert!(request
            .headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/json"));
        assert!(!request.body.is_empty());
    }

    #[test]
    fn header_map_is_case_insensitive() {
        let map = header_map(&[("Content-Type".into(), "text/plain".into())]);
        assert_eq!(map.get("content-type").map(String::as_str), Some("text/plain"));
    }

    #[test]
    fn responses_can_be_matched_on_method_as_well_as_url() {
        // Storage protocols reuse one path across verbs, so the fake has to be able to tell a
        // PUT apart from a DELETE on the same key.
        let client = FakeHttpClient::new()
            .on_method_and_url(HttpMethod::Put, "/object/key", 200, "stored")
            .on_method_and_url(HttpMethod::Delete, "/object/key", 204, "");

        let upload = client
            .execute(HttpRequest::put("https://s3.example.com/object/key"))
            .unwrap();
        assert_eq!(upload.status, 200);

        let removal = client
            .execute(HttpRequest::delete("https://s3.example.com/object/key"))
            .unwrap();
        assert_eq!(removal.status, 204);

        // The queue is consumed in order, so a third call finds nothing queued.
        let extra = client
            .execute(HttpRequest::get("https://s3.example.com/object/key"))
            .unwrap();
        assert_eq!(extra.status, 404);
    }
}
