//! The storage targets, as real adapters.
//!
//! `handoff.md` §13.10–13.13 name four targets: a server over SSH/SFTP, an S3-compatible API, Lsky Pro, and one
//! or more GitHub repositories. Until this module existed only `LocalFileStore` and an in-memory store
//! implemented [`StorageAdapter`], so an attachment could be cached and could never actually be uploaded
//! anywhere — which is what §28 means by "the attachment cache pool, reference sets and all storage adapters
//! work by rule".
//!
//! What is here, and what is NOT, is stated rather than implied, because a guessed protocol is worse than a
//! documented gap:
//!
//! * **GitHub (§13.13) is implemented**, over the documented Contents API. It is the target whose protocol is
//!   small enough to get right without a new dependency, and §13.13 asks for it by name.
//! * **SFTP (§13.10) is NOT implemented.** The workspace deliberately has no `ssh` feature because it pulls
//!   OpenSSL onto Windows/MSVC (see the root `Cargo.toml`), so SFTP needs a dependency decision that is not a
//!   storage adapter's to make. [`UnsupportedStore`] reports that in the user's terms.
//! * **S3 (§13.11) and Lsky Pro (§13.12) are NOT implemented**: S3 needs SigV4 signing and Lsky Pro needs its
//!   own upload-token protocol, and both are reported rather than attempted.
//! * **A local directory IS supported**, through the crate's own `LocalFileStore`. It needs no credential and
//!   no network, so it is both genuinely useful (§13.10's "server storage" where the server is this machine)
//!   and the target an upload can be tested END TO END against.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::{
    object_key, public_url, LocalFileStore, PutOutcome, RemoteObject, StorageAdapter,
    StorageCapabilities, StorageConfig, StorageKind, StorageTarget,
};
use veloct_core::http::{HttpClient, HttpMethod, HttpRequest};
use veloct_core::secret::Redacted;
use veloct_core::{Result, VeloctError};

/// The GitHub Contents API's base.
pub const GITHUB_API: &str = "https://api.github.com";

/// A repository, owner and name.
///
/// Parsed HERE rather than through a URL crate: the forms a user writes are `owner/name` and
/// `https://github.com/owner/name`, which are a strip and a split, and a general URL parser would be a
/// dependency for two string operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub owner: String,
    pub name: String,
}

impl Repository {
    /// Parse `owner/name`, `github.com/owner/name` or `https://github.com/owner/name`.
    ///
    /// A trailing `.git` and a trailing slash are accepted because both are what a user copies, and rejecting
    /// them would be pedantry that produces a confusing error.
    pub fn parse(text: &str) -> Result<Self> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(VeloctError::Invalid(
                "a GitHub target needs a repository, written as owner/name".into(),
            ));
        }

        let without_scheme = trimmed
            .strip_prefix("https://")
            .or_else(|| trimmed.strip_prefix("http://"))
            .unwrap_or(trimmed);
        let without_host = without_scheme
            .strip_prefix("github.com/")
            .or_else(|| without_scheme.strip_prefix("www.github.com/"))
            .unwrap_or(without_scheme);

        let cleaned = without_host.trim_end_matches('/').trim_end_matches(".git");

        let mut parts = cleaned.split('/');
        let owner = parts.next().unwrap_or_default().trim();
        let name = parts.next().unwrap_or_default().trim();
        // A third segment means the user pasted a path INSIDE the repository, which is a different thing from
        // the repository. Refusing is better than silently truncating to the wrong place.
        if parts.next().is_some() {
            return Err(VeloctError::Invalid(format!(
                "{text:?} names a path inside a repository, not a repository; expected owner/name"
            )));
        }
        if owner.is_empty() || name.is_empty() {
            return Err(VeloctError::Invalid(format!("{text:?} is not owner/name")));
        }
        Ok(Self {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }

    /// `owner/name`.
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// A GitHub repository as an attachment target (§13.13).
///
/// The Contents API is used rather than git-over-HTTP: §13.13 makes a repository a place to PUT a file, not a
/// place to clone, and a commit per attachment through git would need a working tree.
pub struct GitHubStore {
    config: StorageConfig,
    repository: Repository,
    /// A credential, wrapped so it cannot be printed by accident (handoff.md section 18.1).
    token: Redacted,
    branch: String,
    client: std::sync::Arc<dyn HttpClient>,
}

impl GitHubStore {
    pub fn new(
        config: StorageConfig,
        repository: Repository,
        token: impl Into<String>,
        branch: impl Into<String>,
        client: std::sync::Arc<dyn HttpClient>,
    ) -> Self {
        // A branch is required by the API's write path, and an empty one would produce a request that fails
        // with a message about a missing ref rather than about the configuration.
        let branch = branch.into();
        Self {
            config,
            repository,
            token: Redacted::new(token.into()),
            branch: if branch.trim().is_empty() {
                "main".to_string()
            } else {
                branch
            },
            client,
        }
    }

    /// The API path for one object.
    pub fn contents_url(&self, key: &str) -> String {
        format!(
            "{GITHUB_API}/repos/{}/{}/contents/{key}",
            self.repository.owner, self.repository.name
        )
    }

    /// The public URL for one object.
    ///
    /// A public file in a public repository is served from `raw.githubusercontent.com`, which is what §13.6's
    /// public URL prefix would otherwise have to be configured for. A configured prefix WINS, because a user
    /// with a CDN in front of the repository means it.
    pub fn public_url_for(&self, key: &str) -> String {
        if !self.config.public_url_prefix.trim().is_empty() {
            return format!("{}{key}", self.config.public_url_prefix.trim_end_matches('/').to_string() + "/");
        }
        format!(
            "https://raw.githubusercontent.com/{}/{}/{key}",
            self.repository.slug(),
            self.branch
        )
    }

    /// A request with the headers every GitHub call needs.
    fn request(&self, method: HttpMethod, url: String) -> HttpRequest {
        HttpRequest::new(method, url)
            .header("Authorization", format!("Bearer {}", self.token.expose()))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            // The API rejects a request with no User-Agent.
            .header("User-Agent", "veloct")
    }

    /// A refusal, as an error that names the status and NOT the body.
    ///
    /// A GitHub error body can echo the token back, and §18.1 keeps credentials out of messages.
    fn refused(&self, what: &str, status: u16) -> VeloctError {
        VeloctError::Attachment(format!(
            "GitHub refused to {what}: HTTP {status}. Check the token's contents-write access and the branch."
        ))
    }
}

impl StorageAdapter for GitHubStore {
    fn config(&self) -> &StorageConfig {
        &self.config
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            verified: true,
            // The URL is derivable from the repository and the branch, so no round trip is needed.
            derives_public_url: true,
            // §13.8: the Contents API can remove an object, and Veloct only calls it on an explicit request.
            can_delete_remotely: true,
            can_test_connection: true,
        }
    }

    fn put(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let body = serde_json::json!({
            "message": format!("Add {key}"),
            "content": base64_encode(bytes),
            "branch": self.branch,
        });
        let request = self
            .request(HttpMethod::Put, self.contents_url(key))
            .json(body);

        let response = self.client.execute(request)?;
        if !response.is_success() {
            return Err(self.refused("upload a file", response.status));
        }

        let object = RemoteObject {
            key: key.to_string(),
            public_url: self.public_url_for(key),
            size_bytes: bytes.len() as u64,
        };
        // The Contents API answers 201 for a create and 200 for an update, so the status distinguishes "wrote
        // it" from "replaced it", which is what §13.4's reuse report is about.
        Ok(if response.status == 201 {
            PutOutcome::Created(object)
        } else {
            PutOutcome::AlreadyPresent(object)
        })
    }

    fn head(&self, key: &str) -> Result<Option<RemoteObject>> {
        let request = self.request(HttpMethod::Get, self.contents_url(key));
        let response = self.client.execute(request)?;
        match response.status {
            200 => {
                // The size comes from the API's answer rather than from the URL, so a caller can compare it.
                let size = response
                    .json::<serde_json::Value>()
                    .ok()
                    .and_then(|value| value.get("size").and_then(|size| size.as_u64()))
                    .unwrap_or(0);
                Ok(Some(RemoteObject {
                    key: key.to_string(),
                    public_url: self.public_url_for(key),
                    size_bytes: size,
                }))
            }
            // A missing file is `None`, not an error: asking whether something is there is not a failure when
            // the answer is no.
            404 => Ok(None),
            other => Err(VeloctError::Attachment(format!(
                "GitHub could not answer whether {key} exists: HTTP {other}"
            ))),
        }
    }

    fn test_connection(&self) -> Result<String> {
        let url = format!("{GITHUB_API}/repos/{}", self.repository.slug());
        let response = self
            .client
            .execute(self.request(HttpMethod::Get, url))?;
        if response.is_success() {
            Ok(format!("{} is reachable", self.repository.slug()))
        } else {
            Err(self.refused("read the repository", response.status))
        }
    }

    fn delete_remote(&self, key: &str) -> Result<()> {
        // §13.8: deleting needs the file's current blob sha, which the API requires. Fetching it first is the
        // documented two-step, and guessing it would fail.
        let lookup = self.request(HttpMethod::Get, self.contents_url(key));
        let response = self.client.execute(lookup)?;
        if response.status == 404 {
            // Already gone, which is what the caller wanted.
            return Ok(());
        }
        if !response.is_success() {
            return Err(self.refused("look up a file", response.status));
        }
        let sha = response
            .json::<serde_json::Value>()
            .ok()
            .and_then(|value| value.get("sha").and_then(|sha| sha.as_str()).map(str::to_string))
            .ok_or_else(|| {
                VeloctError::Attachment(format!("GitHub's answer for {key} carried no sha"))
            })?;

        let request = self
            .request(HttpMethod::Delete, self.contents_url(key))
            .json(serde_json::json!({
                "message": format!("Remove {key}"),
                "sha": sha,
                "branch": self.branch,
            }));
        let response = self.client.execute(request)?;
        if !response.is_success() && response.status != 404 {
            return Err(self.refused("delete a file", response.status));
        }
        Ok(())
    }
}

/// An adapter for a target whose protocol is not implemented.
///
/// A GUESSED protocol would fail for a reason nobody can read, and a silent no-op would look like a successful
/// upload — which is worse, because the user would then cite a URL that does not exist.
pub struct UnsupportedStore {
    config: StorageConfig,
}

impl UnsupportedStore {
    pub fn new(config: StorageConfig) -> Self {
        Self { config }
    }
}

impl StorageAdapter for UnsupportedStore {
    fn config(&self) -> &StorageConfig {
        &self.config
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            verified: false,
            derives_public_url: false,
            can_delete_remotely: false,
            can_test_connection: false,
        }
    }

    fn put(&self, _key: &str, _bytes: &[u8]) -> Result<PutOutcome> {
        Err(VeloctError::not_configured(
            format!("the {} target {}", self.config.kind.as_str(), self.config.name),
            unimplemented_reason(self.config.kind)
                .unwrap_or("this target kind has no adapter in this build"),
        ))
    }

    fn head(&self, _key: &str) -> Result<Option<RemoteObject>> {
        Ok(None)
    }

    fn test_connection(&self) -> Result<String> {
        Err(VeloctError::not_configured(
            format!("the {} target {}", self.config.kind.as_str(), self.config.name),
            unimplemented_reason(self.config.kind)
                .unwrap_or("this target kind has no adapter in this build"),
        ))
    }

    fn delete_remote(&self, _key: &str) -> Result<()> {
        Ok(())
    }
}

/// Build a target from its configuration.
///
/// The credential arrives separately and is never read from the configuration, because §12.2 keeps credentials
/// out of it. A kind with no adapter yields a target whose adapter REPORTS that, so the failure happens at the
/// upload with a message the user can act on rather than at construction with one they cannot.
pub fn target_from_config(
    config: StorageConfig,
    credential: Option<String>,
    client: std::sync::Arc<dyn HttpClient>,
) -> Result<StorageTarget> {
    let adapter: Box<dyn StorageAdapter> = match config.kind {
        StorageKind::GitHub => {
            let repository_text = config.repository.clone().ok_or_else(|| {
                VeloctError::Invalid(format!(
                    "the GitHub target {} has no repository configured",
                    config.name
                ))
            })?;
            let repository = Repository::parse(&repository_text)?;
            let token = credential.ok_or_else(|| VeloctError::NotConfigured {
                what: format!("the GitHub target {}", config.name),
                hint: Some(
                    "a GitHub target needs a token with contents write access on the repository"
                        .to_string(),
                ),
            })?;
            Box::new(GitHubStore::new(
                config.clone(),
                repository,
                token,
                config.branch.clone().unwrap_or_else(|| "main".to_string()),
                client,
            ))
        }
        // §13.10's SFTP needs SSH, and §13.11's S3 needs SigV4, and §13.12's Lsky needs its own protocol. All
        // three report rather than guess.
        StorageKind::Sftp | StorageKind::S3 | StorageKind::LskyPro => {
            Box::new(UnsupportedStore::new(config.clone()))
        }
    };

    Ok(StorageTarget::new(config, adapter))
}

/// A local directory target, for a project with no remote storage.
///
/// This is the one target that needs no credential and no network, so it is what an upload is tested END TO END
/// against, and it is genuinely useful: §13.10's "server attachment storage" for a user whose server is the
/// machine they are on.
pub fn local_target(config: StorageConfig, root: &Path) -> StorageTarget {
    // The adapter needs its own copy: `StorageAdapter::config` returns a reference, so the adapter cannot borrow
    // the config the target owns.
    let adapter_config = config.clone();
    StorageTarget::new(config, Box::new(LocalFileStore::new(adapter_config, root)))
}

/// The directory a local target writes into.
pub fn local_root(project_root: &Path, target_id: &str) -> PathBuf {
    project_root
        .join(veloct_core::layout::VELOCT_DIR)
        .join("uploads")
        .join(target_id)
}

/// base64, for the GitHub Contents API.
///
/// Hand-written rather than a dependency: the Contents API needs standard base64 with padding, which is twenty
/// lines, and a crate for it would be a crate to audit.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((triple >> 18) & 0x3f) as usize] as char);
        out.push(ALPHABET[((triple >> 12) & 0x3f) as usize] as char);
        // The padding rules: one `=` when two bytes were available, two when only one.
        if chunk.len() > 1 {
            out.push(ALPHABET[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(triple & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Whether a kind's adapter is implemented in this build.
pub fn is_implemented(kind: StorageKind) -> bool {
    matches!(kind, StorageKind::GitHub)
}

/// Why a kind has no adapter, in the user's terms.
pub fn unimplemented_reason(kind: StorageKind) -> Option<&'static str> {
    match kind {
        StorageKind::GitHub => None,
        StorageKind::Sftp => Some(
            "SFTP 需要 SSH 实现,本构建出于依赖原因没有引入;请改用 GitHub 或服务器目录目标。",
        ),
        StorageKind::S3 => Some("S3 需要 SigV4 签名,本构建尚未实现;请先使用 GitHub 或服务器目录目标。"),
        StorageKind::LskyPro => {
            Some("Lsky Pro 需要其专用上传令牌协议,本构建尚未实现;请先使用 GitHub 或服务器目录目标。")
        }
    }
}

/// The key an object would have, so a caller can show it before uploading.
pub fn key_for(config: &StorageConfig, file_name: &str) -> Result<String> {
    object_key(config, file_name)
}

/// The public URL an object would have.
pub fn url_for(config: &StorageConfig, key: &str) -> Result<String> {
    public_url(config, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::FakeHttpClient;

    /// Only the fields a test cares about are set; the rest take their defaults.
    fn github_config(repo: Option<&str>, branch: Option<&str>, prefix: Option<&str>) -> StorageConfig {
        // `StorageConfig` has no `Default`: `new` is the one place that knows a target is enabled and has an
        // empty public URL prefix, so the tests use it rather than a literal.
        let mut config = StorageConfig::new("my-repo", "my repo", StorageKind::GitHub);
        config.repository = repo.map(str::to_string);
        config.branch = branch.map(str::to_string);
        config.public_url_prefix = prefix.map(str::to_string).unwrap_or_default();
        config
    }

    fn github_store(repo: &str, client: std::sync::Arc<FakeHttpClient>) -> GitHubStore {
        GitHubStore::new(
            github_config(Some(repo), Some("main"), None),
            Repository::parse(repo).unwrap(),
            "ghp_example_token",
            "main",
            client as std::sync::Arc<dyn HttpClient>,
        )
    }

    #[test]
    fn a_repository_is_parsed_from_every_form_a_user_writes() {
        let expected = Repository {
            owner: "vellow".to_string(),
            name: "assets".to_string(),
        };
        for text in [
            "vellow/assets",
            "github.com/vellow/assets",
            "https://github.com/vellow/assets",
            "https://github.com/vellow/assets/",
            "https://github.com/vellow/assets.git",
            "  vellow/assets  ",
        ] {
            assert_eq!(Repository::parse(text).unwrap(), expected, "{text:?}");
        }
        assert_eq!(
            Repository::parse("vellow/assets").unwrap().slug(),
            "vellow/assets"
        );
    }

    #[test]
    fn a_path_inside_a_repository_is_refused_rather_than_truncated() {
        // `owner/name/tree/main/x` is a different thing from the repository, and silently taking the first two
        // segments would upload somewhere the user did not name.
        let error =
            Repository::parse("https://github.com/vellow/assets/tree/main/images").unwrap_err();
        assert!(error.to_string().contains("path inside a repository"), "{error}");
    }

    #[test]
    fn a_repository_with_a_missing_half_is_refused() {
        for text in ["", "   ", "vellow", "vellow/", "/assets"] {
            assert!(Repository::parse(text).is_err(), "{text:?} should be refused");
        }
    }

    #[test]
    fn base64_matches_the_known_vectors_including_padding() {
        // The padding rules are the part that is easy to get wrong, so all three lengths are asserted.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        // And bytes above 0x7f, which a naive implementation gets wrong.
        assert_eq!(base64_encode(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn the_github_put_sends_the_expected_request() {
        let client = std::sync::Arc::new(FakeHttpClient::new().always(201, "{}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&client));

        let outcome = store.put("a.png", b"PNG").unwrap();
        assert_eq!(outcome.object().key, "a.png");
        assert_eq!(outcome.object().size_bytes, 3);
        assert_eq!(
            outcome.object().public_url,
            "https://raw.githubusercontent.com/vellow/assets/main/a.png"
        );
        assert!(matches!(outcome, PutOutcome::Created(_)));

        let requests = client.recorded();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Put);
        assert!(
            request.url.contains("/repos/vellow/assets/contents/a.png"),
            "{}",
            request.url
        );
        let auth = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .expect("an Authorization header");
        assert!(auth.starts_with("Bearer "), "{auth}");
        assert!(
            request
                .headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("user-agent")),
            "the API requires a User-Agent"
        );

        let body: serde_json::Value = match &request.body {
            veloct_core::http::HttpBody::Json(value) => value.clone(),
            other => panic!("expected a JSON body, got {other:?}"),
        };
        assert_eq!(body["content"], "UE5H", "PNG in base64");
        assert_eq!(body["branch"], "main");
    }

    #[test]
    fn an_updated_file_is_reported_as_already_present_rather_than_created() {
        // The Contents API answers 200 for an update, and section 13.4's reuse report depends on telling the
        // two apart.
        let client = std::sync::Arc::new(FakeHttpClient::new().always(200, "{}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&client));
        assert!(matches!(
            store.put("a.png", b"x").unwrap(),
            PutOutcome::AlreadyPresent(_)
        ));
    }

    #[test]
    fn a_refused_upload_reports_the_status_and_not_the_body() {
        // A GitHub error body can echo the token back, and section 18.1 keeps credentials out of messages.
        let client = std::sync::Arc::new(FakeHttpClient::new().always(
            401,
            r#"{"message":"Bad credentials","token":"ghp_example_token"}"#,
        ));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&client));

        let error = store.put("a.png", b"x").unwrap_err();
        let text = error.to_string();
        assert!(text.contains("401"), "{text}");
        assert!(
            !text.contains("ghp_example_token"),
            "the token must not reach the message: {text}"
        );
    }

    #[test]
    fn a_missing_file_is_reported_as_absent_rather_than_as_an_error() {
        let client = std::sync::Arc::new(FakeHttpClient::new().always(404, "{}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&client));
        assert!(store.head("gone.png").unwrap().is_none());

        let present = std::sync::Arc::new(FakeHttpClient::new().always(200, "{\"size\":42}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&present));
        let found = store.head("there.png").unwrap().expect("present");
        assert_eq!(found.size_bytes, 42, "the size comes from the API's answer");
    }

    #[test]
    fn deleting_fetches_the_sha_first_because_the_api_requires_it() {
        // Section 13.8: the two-step is the documented path, and guessing the sha would fail.
        let client = std::sync::Arc::new(
            FakeHttpClient::new()
                .on_method_and_url(HttpMethod::Get, "/contents/", 200, r#"{"sha":"deadbeef"}"#)
                .always(200, "{}"),
        );
        let store = github_store("vellow/assets", std::sync::Arc::clone(&client));

        store.delete_remote("a.png").unwrap();
        let requests = client.recorded();
        assert_eq!(requests.len(), 2, "a GET for the sha, then a DELETE");
        assert_eq!(requests[0].method, HttpMethod::Get);
        assert_eq!(requests[1].method, HttpMethod::Delete);
        let body: serde_json::Value = match &requests[1].body {
            veloct_core::http::HttpBody::Json(value) => value.clone(),
            other => panic!("expected a JSON body, got {other:?}"),
        };
        assert_eq!(body["sha"], "deadbeef");
    }

    #[test]
    fn deleting_something_already_gone_is_not_an_error() {
        // The caller wanted it absent, and it is.
        let client = std::sync::Arc::new(FakeHttpClient::new().always(404, "{}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&client));
        assert!(store.delete_remote("gone.png").is_ok());
    }

    #[test]
    fn testing_the_connection_proves_the_repository_is_readable() {
        let ok = std::sync::Arc::new(FakeHttpClient::new().always(200, "{}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&ok));
        assert!(store.test_connection().unwrap().contains("vellow/assets"));

        let denied = std::sync::Arc::new(FakeHttpClient::new().always(403, "{}"));
        let store = github_store("vellow/assets", std::sync::Arc::clone(&denied));
        assert!(store.test_connection().is_err());
    }

    #[test]
    fn a_github_target_without_a_repository_or_token_is_reported_rather_than_attempted() {
        let client = std::sync::Arc::new(FakeHttpClient::new()) as std::sync::Arc<dyn HttpClient>;

        let error = target_from_config(
            github_config(None, None, None),
            Some("t".to_string()),
            std::sync::Arc::clone(&client),
        )
        .err()
        .expect("a repository is required");
        assert!(error.to_string().contains("no repository"), "{error}");

        let error = target_from_config(
            github_config(Some("vellow/assets"), None, None),
            None,
            std::sync::Arc::clone(&client),
        )
        .err()
        .expect("a token is required");
        // The guidance lives in the HINT, which `Display` deliberately omits.
        assert!(
            error.hint().unwrap_or_default().contains("token"),
            "{error} / {:?}",
            error.hint()
        );
    }

    #[test]
    fn sftp_s3_and_lsky_report_that_they_have_no_adapter_instead_of_failing_obscurely() {
        // A guessed protocol would fail for a reason nobody can read, and a silent no-op would look like a
        // successful upload.
        for kind in [StorageKind::Sftp, StorageKind::S3, StorageKind::LskyPro] {
            assert!(!is_implemented(kind), "{kind:?} is not implemented");
            let reason = unimplemented_reason(kind).expect("a reason");
            assert!(!reason.is_empty(), "{kind:?}");

            let config = StorageConfig {
                kind,
                name: "t".to_string(),
                ..StorageConfig::new("t", "t", StorageKind::GitHub)
            };
            let client = std::sync::Arc::new(FakeHttpClient::new()) as std::sync::Arc<dyn HttpClient>;
            let target =
                target_from_config(config, None, std::sync::Arc::clone(&client)).unwrap();
            let error = target.store("a.png", b"x").unwrap_err();
            assert!(
                matches!(error, VeloctError::NotConfigured { .. }),
                "{kind:?}: {error}"
            );
            // And the capabilities say so, so a caller can tell before trying.
            assert!(!target.capabilities().verified, "{kind:?}");
        }
        assert!(is_implemented(StorageKind::GitHub));
        assert!(unimplemented_reason(StorageKind::GitHub).is_none());
    }

    #[test]
    fn a_local_target_writes_a_real_file_and_serves_a_url() {
        // The one target that needs no credential and no network, so an upload can be tested end to end.
        let dir = tempfile::TempDir::new().unwrap();
        let config = StorageConfig {
            kind: StorageKind::Sftp,
            name: "on this machine".to_string(),
            key_prefix: Some("uploads".to_string()),
            public_url_prefix: "https://example.com/files/".to_string(),
            ..StorageConfig::new("t", "t", StorageKind::GitHub)
        };
        let target = local_target(config, dir.path());
        let outcome = target.store("photo.png", b"BYTES").unwrap();

        assert_eq!(outcome.object().size_bytes, 5);
        let written = dir.path().join(&outcome.object().key);
        assert!(written.is_file(), "{}", written.display());
        assert_eq!(std::fs::read(&written).unwrap(), b"BYTES");
        assert!(
            outcome.object().public_url.starts_with("https://example.com/files/"),
            "{}",
            outcome.object().public_url
        );
    }

    #[test]
    fn a_configured_public_url_prefix_wins_over_the_repositorys_raw_host() {
        // A user with a CDN in front of the repository means it.
        let client = std::sync::Arc::new(FakeHttpClient::new().always(201, "{}"));
        let store = GitHubStore::new(
            github_config(Some("vellow/assets"), Some("main"), Some("https://cdn.example.com/")),
            Repository::parse("vellow/assets").unwrap(),
            "t",
            "main",
            client as std::sync::Arc<dyn HttpClient>,
        );
        assert_eq!(
            store.public_url_for("a.png"),
            "https://cdn.example.com/a.png"
        );
    }

    #[test]
    fn an_empty_branch_falls_back_rather_than_producing_a_request_about_a_missing_ref() {
        let client = std::sync::Arc::new(FakeHttpClient::new().always(201, "{}"));
        let store = GitHubStore::new(
            github_config(Some("vellow/assets"), None, None),
            Repository::parse("vellow/assets").unwrap(),
            "t",
            "",
            client as std::sync::Arc<dyn HttpClient>,
        );
        assert!(store.public_url_for("a.png").contains("/main/"));
    }

    #[test]
    fn the_key_and_url_come_from_the_configurations_prefixes() {
        let config = StorageConfig {
            kind: StorageKind::GitHub,
            name: "t".to_string(),
            key_prefix: Some("assets".to_string()),
            public_url_prefix: "https://cdn.example.com/".to_string(),
            ..StorageConfig::new("t", "t", StorageKind::GitHub)
        };
        let key = key_for(&config, "photo.png").unwrap();
        assert!(key.starts_with("assets/"), "{key}");
        let url = url_for(&config, &key).unwrap();
        assert!(url.starts_with("https://cdn.example.com/"), "{url}");
    }

    #[test]
    fn local_root_is_inside_the_projects_veloct_directory() {
        let root = local_root(Path::new("D:/proj"), "my-target");
        let text = root.to_string_lossy().replace('\\', "/");
        assert!(text.contains(".veloct/uploads/my-target"), "{text}");
    }
}
