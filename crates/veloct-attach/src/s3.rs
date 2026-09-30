//! The S3 / S3-compatible **attachment storage target** (`handoff.md` §13.11).
//!
//! `handoff.md` names S3 twice — here, as a place attachments live and are served from, and in §17.4 as a
//! cloud backup target. Both are the SigV4 client in `veloct-s3`; what differs is policy, and that is all
//! this module holds:
//!
//! * §13.6 separates the **public URL prefix** from the object's actual path, so the URL comes from
//!   configuration and is not derived from the endpoint.
//! * §13.4's dedup report distinguishes "wrote it" from "it was already there". S3's `PUT` is
//!   idempotent and answers 200 either way, so the distinction cannot be read from the response: this
//!   adapter `HEAD`s first, which costs one small request and keeps the report true.
//! * §13.8's rule that nothing deletes a remote object automatically is the transaction's, and this
//!   adapter simply exposes the documented `DELETE` for the explicit path.
//!
//! The `Content-Type` is set from the file name's extension. That is not cosmetic: a bucket serves the
//! type stored at `PUT` time, so an object written without one is served as `application/octet-stream`
//! and a picture downloads instead of rendering.

use std::sync::Arc;

use veloct_core::http::HttpClient;
use veloct_core::{Clock, Result, VeloctError};
use veloct_s3::{amz_timestamp, Credentials, S3Client, S3Config};

use crate::storage::{
    content_type_for, public_url, PutOutcome, RemoteObject, StorageAdapter, StorageCapabilities,
    StorageConfig,
};

/// An S3 bucket as an attachment target.
pub struct S3Store {
    config: StorageConfig,
    client: S3Client,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for S3Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `S3Client` redacts its own credential, so this cannot print the secret key.
        f.debug_struct("S3Store")
            .field("config", &self.config)
            .finish()
    }
}

impl S3Store {
    /// Build the adapter for a configured target.
    ///
    /// `secret_key` is the value resolved from the OS credential store by HANDLE; `config.access_key` is
    /// the access key ID, which is an identifier and lives in the configuration.
    pub fn new(
        config: StorageConfig,
        secret_key: impl Into<String>,
        client: Arc<dyn HttpClient>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let s3 = s3_config(&config)?;
        let access_key = config
            .access_key
            .clone()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| VeloctError::NotConfigured {
                what: format!("the S3 target {}", config.name),
                hint: Some(
                    "an S3 target needs its access key ID in the configuration and its SECRET access key \
                     in the credential store"
                        .to_string(),
                ),
            })?;
        Ok(Self {
            config,
            client: S3Client::new(s3, Credentials::new(access_key, secret_key), client),
            clock,
        })
    }

    fn timestamp(&self) -> String {
        amz_timestamp(&veloct_core::clock::format_rfc3339(self.clock.now()))
    }

    fn object(&self, key: &str, size_bytes: u64) -> Result<RemoteObject> {
        Ok(RemoteObject {
            key: key.to_string(),
            public_url: public_url(&self.config, key)?,
            size_bytes,
        })
    }
}

/// The S3 client configuration a §13.11 target describes.
///
/// `handoff.md` §13.11 names Endpoint, Region and Bucket; an empty endpoint falls back to AWS's, and an
/// endpoint the user supplied is addressed path-style because that is what a self-hosted service wants.
pub fn s3_config(config: &StorageConfig) -> Result<S3Config> {
    let bucket = config
        .bucket
        .clone()
        .filter(|bucket| !bucket.trim().is_empty())
        .ok_or_else(|| {
            VeloctError::Invalid(format!("the S3 target {} has no bucket configured", config.name))
        })?;
    let region = config
        .region
        .clone()
        .filter(|region| !region.trim().is_empty())
        .unwrap_or_else(|| "us-east-1".to_string());
    let endpoint = config
        .endpoint
        .clone()
        .filter(|endpoint| !endpoint.trim().is_empty());

    let mut s3 = match endpoint {
        Some(endpoint) => S3Config::compatible(bucket, region, endpoint),
        None => S3Config::aws(bucket, region),
    };
    s3.public_url_prefix = config.public_url_prefix.clone();
    Ok(s3)
}

impl StorageAdapter for S3Store {
    fn config(&self) -> &StorageConfig {
        &self.config
    }

    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities {
            verified: true,
            // §13.6 separates the public URL prefix from the object's path, so a URL is only derivable
            // when the user configured one.
            derives_public_url: !self.config.public_url_prefix.trim().is_empty(),
            can_delete_remotely: true,
            can_test_connection: true,
        }
    }

    fn put(&self, key: &str, bytes: &[u8]) -> Result<PutOutcome> {
        let timestamp = self.timestamp();

        // S3 answers 200 for a create AND for an overwrite, so the reuse report §13.4 asks for cannot be
        // read from the response: the object is looked up first.
        if let Some(existing) = self.head(key)? {
            return Ok(PutOutcome::AlreadyPresent(existing));
        }

        let content_type = content_type_for(key);
        self.client
            .put_object(key, bytes, content_type.as_deref(), &timestamp)?;
        Ok(PutOutcome::Created(self.object(key, bytes.len() as u64)?))
    }

    fn head(&self, key: &str) -> Result<Option<RemoteObject>> {
        let timestamp = self.timestamp();
        match self.client.head_object(key, &timestamp)? {
            // The size comes from the service, so a caller can compare it with what it meant to send.
            Some(size) => Ok(Some(self.object(key, size)?)),
            // A missing object is `None`, not an error: asking whether something is there is not a
            // failure when the answer is no.
            None => Ok(None),
        }
    }

    fn test_connection(&self) -> Result<String> {
        let timestamp = self.timestamp();
        self.client.test_connection(&timestamp)
    }

    fn delete_remote(&self, key: &str) -> Result<()> {
        let timestamp = self.timestamp();
        self.client.delete_object(key, &timestamp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StorageKind;
    use veloct_core::clock::FixedClock;
    use veloct_core::http::{FakeHttpClient, HttpMethod};

    fn fixed_clock() -> Arc<dyn Clock> {
        Arc::new(FixedClock::from_rfc3339("2013-05-24T00:00:00Z").unwrap())
    }

    fn s3_config_fixture() -> StorageConfig {
        let mut config = StorageConfig::new("assets", "my bucket", StorageKind::S3);
        config.bucket = Some("veloct-assets".to_string());
        config.region = Some("us-east-1".to_string());
        config.access_key = Some("AKIAIOSFODNN7EXAMPLE".to_string());
        config.public_url_prefix = "https://cdn.example.com/".to_string();
        config
    }

    fn build_store(fake: Arc<FakeHttpClient>, config: StorageConfig) -> S3Store {
        S3Store::new(
            config,
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            fake as Arc<dyn HttpClient>,
            fixed_clock(),
        )
        .unwrap()
    }

    #[test]
    fn an_aws_target_is_virtual_hosted_and_a_custom_endpoint_is_path_style() {
        let aws = s3_config(&s3_config_fixture()).unwrap();
        assert_eq!(aws.host().unwrap(), "veloct-assets.s3.us-east-1.amazonaws.com");
        assert_eq!(aws.path_for("a.png").unwrap(), "/a.png");

        let mut minio = s3_config_fixture();
        minio.endpoint = Some("http://127.0.0.1:9000".to_string());
        let compatible = s3_config(&minio).unwrap();
        assert_eq!(compatible.host().unwrap(), "127.0.0.1:9000");
        assert_eq!(compatible.path_for("a.png").unwrap(), "/veloct-assets/a.png");
    }

    #[test]
    fn a_bucket_that_is_missing_is_reported_with_what_to_configure() {
        let mut config = s3_config_fixture();
        config.bucket = None;
        let error = s3_config(&config).unwrap_err();
        assert!(error.to_string().contains("no bucket"), "{error}");

        // And a target with no access key ID is reported at construction rather than as a 403.
        let mut keyless = s3_config_fixture();
        keyless.access_key = None;
        let fake = Arc::new(FakeHttpClient::new()) as Arc<dyn HttpClient>;
        let error = S3Store::new(keyless, "secret", fake, fixed_clock()).unwrap_err();
        assert!(error.to_string().contains("not configured"), "{error}");
        assert!(
            error.hint().unwrap_or_default().contains("access key ID"),
            "{:?}",
            error.hint()
        );
    }

    #[test]
    fn a_new_object_reports_created_and_carries_its_content_type() {
        // The HEAD is answered 404 (nothing queued on the fake), then the PUT is answered 200.
        let fake = Arc::new(
            FakeHttpClient::new()
                .on_method_and_url(HttpMethod::Head, "/a.png", 404, "")
                .on_method_and_url(HttpMethod::Put, "/a.png", 200, ""),
        );
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        let outcome = store.put("a.png", b"PNG").unwrap();

        assert!(matches!(outcome, PutOutcome::Created(_)));
        assert_eq!(outcome.object().size_bytes, 3);
        assert_eq!(outcome.object().public_url, "https://cdn.example.com/a.png");

        let requests = fake.recorded();
        assert_eq!(requests.len(), 2, "a HEAD to check, then the PUT");
        assert_eq!(requests[0].method, HttpMethod::Head);
        assert_eq!(requests[1].method, HttpMethod::Put);
        // Without this header the bucket serves the picture as octet-stream and it downloads.
        assert!(
            requests[1]
                .headers
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("content-type") && value == "image/png"),
            "{:?}",
            requests[1].headers
        );
    }

    #[test]
    fn an_object_that_is_already_there_is_reported_as_present_without_a_second_put() {
        // Section 13.4's reuse report: S3 answers 200 for an overwrite too, so the distinction has to
        // come from the lookup.
        let fake = Arc::new(
            FakeHttpClient::new()
                .on_method_and_url(HttpMethod::Head, "/a.png", 200, "")
                .on_method_and_url(HttpMethod::Put, "/a.png", 200, ""),
        );
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        let outcome = store.put("a.png", b"PNG").unwrap();

        assert!(matches!(outcome, PutOutcome::AlreadyPresent(_)));
        let puts = fake
            .recorded()
            .into_iter()
            .filter(|request| request.method == HttpMethod::Put)
            .count();
        assert_eq!(puts, 0, "an object that is already there is not uploaded again");
    }

    #[test]
    fn an_unknown_extension_still_stores_the_object_it_just_has_no_content_type() {
        // A missing Content-Type is honest; a GUESSED one would make a browser render the wrong thing.
        let fake = Arc::new(
            FakeHttpClient::new()
                .on_method_and_url(HttpMethod::Head, "/archive.zzz", 404, "")
                .on_method_and_url(HttpMethod::Put, "/archive.zzz", 200, ""),
        );
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        assert!(matches!(
            store.put("archive.zzz", b"X").unwrap(),
            PutOutcome::Created(_)
        ));
        let put = fake
            .recorded()
            .into_iter()
            .find(|request| request.method == HttpMethod::Put)
            .unwrap();
        assert!(
            !put.headers
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("content-type")),
            "{:?}",
            put.headers
        );
    }

    #[test]
    fn a_missing_object_is_absent_rather_than_an_error() {
        let fake =
            Arc::new(FakeHttpClient::new().on_method_and_url(HttpMethod::Head, "/gone", 404, ""));
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        assert!(store.head("gone").unwrap().is_none());

        // A present one reports the size the SERVICE gave, not a number this side invented.
        let present =
            Arc::new(FakeHttpClient::new().on_method_and_url(HttpMethod::Head, "/there", 200, ""));
        let store = build_store(Arc::clone(&present), s3_config_fixture());
        // The fake sends no content-length header, so the honest answer is 0.
        assert_eq!(store.head("there").unwrap().unwrap().size_bytes, 0);
    }

    #[test]
    fn a_public_url_is_only_derivable_when_a_prefix_is_configured() {
        // Section 13.6 keeps the prefix separate from the object's path, so a target without one cannot
        // promise a URL — and `capabilities` reports that rather than producing a plausible guess.
        let mut without = s3_config_fixture();
        without.public_url_prefix = String::new();
        let fake = Arc::new(FakeHttpClient::new()) as Arc<dyn HttpClient>;
        let store = S3Store::new(without, "secret", fake, fixed_clock()).unwrap();
        assert!(!store.capabilities().derives_public_url);

        let fake = Arc::new(FakeHttpClient::new()) as Arc<dyn HttpClient>;
        let store = S3Store::new(s3_config_fixture(), "secret", fake, fixed_clock()).unwrap();
        assert!(store.capabilities().derives_public_url);
        assert!(store.capabilities().verified);
        assert!(store.capabilities().can_delete_remotely);
    }

    #[test]
    fn a_delete_reaches_the_documented_endpoint_and_a_missing_object_is_not_an_error() {
        let fake = Arc::new(FakeHttpClient::new().on_method_and_url(
            HttpMethod::Delete,
            "/a.png",
            204,
            "",
        ));
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        store.delete_remote("a.png").unwrap();
        let request = fake.recorded().into_iter().next().unwrap();
        assert_eq!(request.method, HttpMethod::Delete);
        assert_eq!(
            request.url,
            "https://veloct-assets.s3.us-east-1.amazonaws.com/a.png"
        );

        // Section 13.8 says the caller wanted it absent, and it is.
        let gone = Arc::new(FakeHttpClient::new().on_method_and_url(
            HttpMethod::Delete,
            "/gone.png",
            404,
            "",
        ));
        let store = build_store(gone, s3_config_fixture());
        assert!(store.delete_remote("gone.png").is_ok());
    }

    #[test]
    fn a_connection_test_lists_rather_than_asking_for_bucket_metadata() {
        // Section 13.11's configurations normally grant object permissions and not bucket-level ones.
        let fake = Arc::new(FakeHttpClient::new().always(
            200,
            "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>",
        ));
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        let said = store.test_connection().unwrap();
        assert!(said.contains("veloct-assets"), "{said}");

        let requests = fake.recorded();
        assert_eq!(requests[0].method, HttpMethod::Get);
        assert!(requests[0].url.contains("max-keys=1"), "{}", requests[0].url);
    }

    #[test]
    fn a_refusal_names_the_buckets_own_error_code_and_never_the_secret() {
        let fake = Arc::new(FakeHttpClient::new().on_method_and_url(
            HttpMethod::Head,
            "/a.png",
            403,
            "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
        ));
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        let text = store.head("a.png").unwrap_err().to_string();
        assert!(text.contains("403"), "{text}");
        assert!(text.contains("AccessDenied"), "{text}");
        assert!(
            !text.contains("wJalrXUtnFEMI"),
            "the secret must not reach a message: {text}"
        );
    }

    #[test]
    fn the_signature_uses_the_access_key_id_and_the_injected_clock() {
        let fake = Arc::new(FakeHttpClient::new().always(200, ""));
        let store = build_store(Arc::clone(&fake), s3_config_fixture());
        store.test_connection().unwrap();
        let request = fake.recorded().into_iter().next().unwrap();
        let authorization = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .unwrap();
        assert!(
            authorization
                .contains("Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request"),
            "{authorization}"
        );
        assert!(request.headers.iter().any(
            |(name, value)| name.eq_ignore_ascii_case("x-amz-date") && value == "20130524T000000Z"
        ));
    }
}
