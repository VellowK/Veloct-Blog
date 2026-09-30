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

use crate::storage::{
    object_key, public_url, LocalFileStore, PutOutcome, RemoteObject, StorageAdapter,
    StorageCapabilities, StorageConfig, StorageKind, StorageTarget,
};
use veloct_core::http::HttpClient;
use veloct_core::{Result, VeloctError};
use veloct_github::{GitHubContents, WriteOutcome as GitHubWrite};

// The protocol itself lives in `veloct-github`, because `handoff.md` §17.4 makes a repository a CLOUD
// BACKUP target as well as §13.13's attachment target, and one protocol written twice is one protocol
// with two chances to be wrong. Re-exported so this crate's surface — and every caller that already
// names `veloct_attach::Repository` — is unchanged.
pub use veloct_github::{base64_encode, Repository, GITHUB_API};

/// A GitHub repository as an attachment target (§13.13).
///
/// The Contents API is used rather than git-over-HTTP: §13.13 makes a repository a place to PUT a file, not a
/// place to clone, and a commit per attachment through git would need a working tree.
///
/// The protocol is `veloct_github::GitHubContents`; what this type adds is the ATTACHMENT policy — the
/// configured public URL prefix, `raw.githubusercontent.com` as its fallback, and this crate's error
/// wording.
pub struct GitHubStore {
    config: StorageConfig,
    contents: GitHubContents,
}

impl GitHubStore {
    pub fn new(
        config: StorageConfig,
        repository: Repository,
        token: impl Into<String>,
        branch: impl Into<String>,
        client: std::sync::Arc<dyn HttpClient>,
    ) -> Self {
        Self {
            config,
            // An empty branch becomes `main` inside the client, which is the one place that knows the
            // write path requires a branch.
            contents: GitHubContents::new(repository, token, branch, client),
        }
    }

    /// The API path for one object.
    pub fn contents_url(&self, key: &str) -> String {
        self.contents.contents_url(key)
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
            self.contents.repository().slug(),
            self.contents.branch()
        )
    }
}

/// Add this crate's context to a shared client's failure.
///
/// `veloct-github` is feature-neutral on purpose, because it serves §13.13 (an attachment) and §17.4 (a
/// backup) and could not name either without mislabelling the other. The attachment wording is therefore
/// added HERE, at the boundary that knows what it is. A condition that already names its own kind is passed
/// through rather than wrapped twice.
fn attachment_error(error: VeloctError) -> VeloctError {
    match error {
        VeloctError::Other(message) => VeloctError::Attachment(message),
        other => other,
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
        let outcome = self.contents.put(key, bytes).map_err(attachment_error)?;

        let object = RemoteObject {
            key: key.to_string(),
            public_url: self.public_url_for(key),
            size_bytes: bytes.len() as u64,
        };
        // The Contents API answers 201 for a create and 200 for an update, so the status distinguishes "wrote
        // it" from "replaced it", which is what §13.4's reuse report is about.
        Ok(match outcome {
            GitHubWrite::Created => PutOutcome::Created(object),
            GitHubWrite::Updated => PutOutcome::AlreadyPresent(object),
        })
    }

    fn head(&self, key: &str) -> Result<Option<RemoteObject>> {
        let found = self.contents.get(key).map_err(attachment_error)?;
        Ok(found.map(|entry| RemoteObject {
            key: key.to_string(),
            public_url: self.public_url_for(key),
            // The size comes from the API's answer rather than from the URL, so a caller can compare it.
            size_bytes: entry.size_bytes,
        }))
    }

    fn test_connection(&self) -> Result<String> {
        // The repository resource is the call that proves the token can read the repository at all. The
        // privacy flag it also returns belongs to §17.2's BACKUP rule and is ignored here: an attachment
        // repository is public by design, because §13.13 serves files out of it.
        self.contents.repository_info().map_err(attachment_error)?;
        Ok(format!("{} is reachable", self.contents.repository().slug()))
    }

    fn delete_remote(&self, key: &str) -> Result<()> {
        // §13.8: deleting needs the file's current blob sha, which the API requires. The client owns the
        // documented two-step, so guessing is impossible here rather than merely discouraged.
        self.contents.delete(key).map_err(attachment_error)
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
        StorageKind::S3 => {
            // §13.11: Endpoint, Region, Bucket and the access key pair. The SECRET arrives as the
            // resolved credential; the access key ID is configuration, which is why it is not here.
            let secret = credential.ok_or_else(|| VeloctError::NotConfigured {
                what: format!("the S3 target {}", config.name),
                hint: Some(
                    "an S3 target needs its SECRET access key stored as the target's credential; the \
                     access key ID belongs in the target's configuration"
                        .to_string(),
                ),
            })?;
            // The clock is the system's here because this factory's job is to assemble an adapter; a test
            // that checks a signature builds `S3Store` directly with a pinned clock instead.
            Box::new(crate::s3::S3Store::new(
                config.clone(),
                secret,
                client,
                std::sync::Arc::new(veloct_core::SystemClock),
            )?)
        }
        // §13.10's SFTP needs SSH and §13.12's Lsky needs its own protocol. Both report rather than
        // guess, and ADR-0010 forbids replacing that with a plausible-looking request.
        StorageKind::Sftp | StorageKind::LskyPro => {
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

/// Whether a kind's adapter is implemented in this build.
pub fn is_implemented(kind: StorageKind) -> bool {
    matches!(kind, StorageKind::GitHub | StorageKind::S3)
}

/// Why a kind has no adapter, in the user's terms.
pub fn unimplemented_reason(kind: StorageKind) -> Option<&'static str> {
    match kind {
        StorageKind::GitHub | StorageKind::S3 => None,
        StorageKind::Sftp => Some(
            "SFTP 需要 SSH 实现,本构建出于依赖原因没有引入;请改用 GitHub、S3 或服务器目录目标。",
        ),
        StorageKind::LskyPro => Some(
            "Lsky Pro 需要其专用上传令牌协议,本构建尚未实现;请先使用 GitHub、S3 或服务器目录目标。",
        ),
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
    use veloct_core::http::{FakeHttpClient, HttpMethod};

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
    fn sftp_and_lsky_report_that_they_have_no_adapter_instead_of_failing_obscurely() {
        // A guessed protocol would fail for a reason nobody can read, and a silent no-op would look like a
        // successful upload. S3 USED to be in this list and is no longer: §13.11's adapter landed with the
        // SigV4 client, so the loop now covers the two kinds that genuinely have none.
        for kind in [StorageKind::Sftp, StorageKind::LskyPro] {
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
        assert!(is_implemented(StorageKind::S3));
        assert!(unimplemented_reason(StorageKind::S3).is_none());
    }

    #[test]
    fn an_s3_target_without_its_secret_is_reported_rather_than_sending_an_unsigned_request() {
        // Section 13.11 names the access key AND the secret key. The ID is configuration; the secret is a
        // credential, so an S3 target assembled with no credential must say so.
        let mut config = StorageConfig::new("assets", "my bucket", StorageKind::S3);
        config.bucket = Some("veloct-assets".to_string());
        config.region = Some("us-east-1".to_string());
        config.access_key = Some("AKIAIOSFODNN7EXAMPLE".to_string());
        config.public_url_prefix = "https://cdn.example.com/".to_string();

        let client = std::sync::Arc::new(FakeHttpClient::new()) as std::sync::Arc<dyn HttpClient>;
        let error = target_from_config(config.clone(), None, std::sync::Arc::clone(&client))
            .err()
            .expect("a secret is required");
        assert!(matches!(error, VeloctError::NotConfigured { .. }), "{error}");
        assert!(
            error.hint().unwrap_or_default().contains("SECRET"),
            "{:?}",
            error.hint()
        );

        // A bucket that is missing is reported too, and before any request is attempted.
        let mut bucketless = config.clone();
        bucketless.bucket = None;
        let error = target_from_config(
            bucketless,
            Some("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string()),
            std::sync::Arc::clone(&client),
        )
        .err()
        .expect("a bucket is required");
        assert!(error.to_string().contains("no bucket"), "{error}");

        // And with both in place the target is BUILT, with the capabilities §13.11 promises.
        let target = target_from_config(
            config,
            Some("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".to_string()),
            client,
        )
        .unwrap();
        let capabilities = target.capabilities();
        assert!(capabilities.verified);
        assert!(capabilities.can_delete_remotely);
        assert!(capabilities.can_test_connection);
        assert!(capabilities.derives_public_url, "a prefix was configured");
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
