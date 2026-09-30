//! The S3 / S3-compatible **cloud backup target** (`handoff.md` §17.4).
//!
//! §17.4 names exactly two cloud targets and §17.2 makes both of them private. The GitHub half lives in
//! [`crate::github`]; this is the other one, over the SigV4 client in `veloct-s3`.
//!
//! Three things are different from the GitHub adapter, and each is a property of the protocol rather
//! than a preference:
//!
//! * **The archive is STREAMED, not read into memory.** S3 takes an object body as-is, so a large project
//!   is uploaded with a file body and a bounded 64 KiB read for the signature's payload hash. The GitHub
//!   Contents API cannot do this, which is why *it* refuses above 100 MB and this adapter does not.
//! * **The listing reports a real `LastModified`.** GitHub reports no per-object creation time, so that
//!   adapter recovers the instant from the archive's own name. Here the bucket states it, and a fact from
//!   the service beats a name Veloct chose.
//! * **Privacy cannot be verified, only declared.** GitHub's API answers with a `private` flag; an S3
//!   bucket's ACL is not readable without bucket-level permissions, which §13.11's configurations usually
//!   do not grant. So §17.2 stays the user's confirmation here, checked by the orchestration, and this
//!   module does not pretend to have verified it.

use std::path::Path;
use std::sync::Arc;

use veloct_core::http::HttpClient;
use veloct_core::{Clock, Result, VeloctError};
use veloct_s3::{amz_timestamp, Credentials, S3Client, S3Config};

use crate::github::is_backup_name;
use crate::targets::{BackupTargetAdapter, CloudTarget, RemoteBackup};

/// An S3 bucket as a cloud backup target.
pub struct S3BackupTarget {
    target: CloudTarget,
    client: S3Client,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for S3BackupTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `S3Client` redacts its own credential; this keeps that guarantee if the struct grows.
        f.debug_struct("S3BackupTarget")
            .field("target", &self.target)
            .finish()
    }
}

impl S3BackupTarget {
    /// Build the adapter for a configured target.
    ///
    /// `clock` is injected rather than read from the system, because a signature is a function of its
    /// timestamp and a test that cannot pin the time cannot check a signature (ADR-0002).
    pub fn new(
        target: CloudTarget,
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        client: Arc<dyn HttpClient>,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        let config = s3_config(&target)?;
        let credentials = Credentials::new(access_key, secret_key);
        Ok(Self {
            target,
            client: S3Client::new(config, credentials, client),
            clock,
        })
    }

    /// The `x-amz-date` value for right now.
    fn timestamp(&self) -> String {
        amz_timestamp(&veloct_core::clock::format_rfc3339(self.clock.now()))
    }

    /// The directory this target's archives live in, which is also the listing prefix.
    fn prefix(&self) -> String {
        match self.target.prefix.as_deref() {
            Some(prefix) if !prefix.trim().is_empty() => {
                format!("{}/", prefix.trim_matches('/'))
            }
            _ => String::new(),
        }
    }
}

/// The S3 client configuration a target describes.
///
/// An endpoint with no scheme is accepted by the client, and an empty endpoint falls back to the AWS
/// endpoint for the region — which is what §13.11's "custom Endpoint" means when it is left blank.
pub fn s3_config(target: &CloudTarget) -> Result<S3Config> {
    let bucket = target.location.trim();
    if bucket.is_empty() {
        return Err(VeloctError::Backup(format!(
            "the S3 backup target {:?} has no bucket",
            target.id
        )));
    }
    let region = target
        .region
        .as_deref()
        .map(str::trim)
        .filter(|region| !region.is_empty())
        .unwrap_or("us-east-1")
        .to_string();
    let endpoint = target
        .endpoint
        .as_deref()
        .map(str::trim)
        .filter(|endpoint| !endpoint.is_empty());

    Ok(match endpoint {
        // A user-supplied endpoint is addressed path-style: that is what a self-hosted S3-compatible
        // service wants, and getting the style wrong is a signature failure rather than a 404.
        Some(endpoint) => S3Config::compatible(bucket, region, endpoint),
        None => S3Config::aws(bucket, region),
    })
}

impl BackupTargetAdapter for S3BackupTarget {
    fn config(&self) -> &CloudTarget {
        &self.target
    }

    fn upload(&self, archive: &Path, remote_name: &str) -> Result<String> {
        let timestamp = self.timestamp();
        // Streamed: the body is the file itself, and only its hash is computed in memory.
        self.client
            .put_file(remote_name, archive, Some("application/gzip"), &timestamp)
            .map_err(|error| {
                VeloctError::Backup(format!(
                    "the S3 backup target {:?} could not store {remote_name}: {error}",
                    self.target.id
                ))
            })?;
        // The key the caller records and later passes to `remove`.
        Ok(remote_name.to_string())
    }

    fn list(&self) -> Result<Vec<RemoteBackup>> {
        let timestamp = self.timestamp();
        let prefix = self.prefix();
        let objects = self.client.list(&prefix, &timestamp).map_err(|error| {
            VeloctError::Backup(format!(
                "the S3 backup target {:?} could not list {prefix:?}: {error}",
                self.target.id
            ))
        })?;

        Ok(objects
            .into_iter()
            // Only THIS application's archives: §17.6's retention deletes what it did not create
            // otherwise, and a backup bucket is a place a user may reasonably keep other things.
            .filter(|object| {
                let name = object.key.rsplit('/').next().unwrap_or(&object.key);
                is_backup_name(name)
            })
            .map(|object| RemoteBackup {
                path: object.key,
                // A real fact from the bucket, and ISO 8601, so a lexicographic sort is chronological.
                created_at: object.last_modified,
                size_bytes: object.size_bytes,
            })
            .collect())
    }

    fn remove(&self, remote_path: &str) -> Result<()> {
        let timestamp = self.timestamp();
        self.client
            .delete_object(remote_path, &timestamp)
            .map_err(|error| {
                VeloctError::Backup(format!(
                    "the S3 backup target {:?} could not remove {remote_path}: {error}",
                    self.target.id
                ))
            })
    }

    fn test_connection(&self) -> Result<String> {
        let timestamp = self.timestamp();
        self.client.test_connection(&timestamp).map_err(|error| {
            VeloctError::Backup(format!(
                "the S3 backup target {:?} is not reachable: {error}",
                self.target.id
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::{prune, CloudTargetKind};
    use veloct_core::clock::FixedClock;
    use veloct_core::http::{FakeHttpClient, HttpMethod};

    const TIMESTAMP: &str = "20130524T000000Z";

    fn clock() -> Arc<dyn Clock> {
        Arc::new(FixedClock::from_rfc3339("2013-05-24T00:00:00Z").unwrap())
    }

    fn s3_target() -> CloudTarget {
        let mut target =
            CloudTarget::new("s3", CloudTargetKind::S3, "veloct-private-backups");
        target.private_confirmed = true;
        target.region = Some("us-east-1".to_string());
        target.access_key = Some("AKIAIOSFODNN7EXAMPLE".to_string());
        target.prefix = Some("veloct-backups".to_string());
        target
    }

    fn adapter(fake: Arc<FakeHttpClient>, target: CloudTarget) -> S3BackupTarget {
        S3BackupTarget::new(
            target,
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            fake as Arc<dyn HttpClient>,
            clock(),
        )
        .unwrap()
    }

    #[test]
    fn aws_is_virtual_hosted_and_a_custom_endpoint_is_path_style() {
        // The addressing style changes both the Host header and the signed path, so choosing wrongly is a
        // signature failure the user cannot diagnose from the message.
        let aws = s3_config(&s3_target()).unwrap();
        assert_eq!(aws.host().unwrap(), "veloct-private-backups.s3.us-east-1.amazonaws.com");
        assert_eq!(aws.path_for("a.tar.gz").unwrap(), "/a.tar.gz");

        let mut minio = s3_target();
        minio.endpoint = Some("http://127.0.0.1:9000".to_string());
        let compatible = s3_config(&minio).unwrap();
        assert_eq!(compatible.host().unwrap(), "127.0.0.1:9000");
        assert_eq!(
            compatible.path_for("a.tar.gz").unwrap(),
            "/veloct-private-backups/a.tar.gz"
        );
    }

    #[test]
    fn a_missing_region_still_produces_a_usable_endpoint() {
        // A self-hosted service often has no meaningful region, and refusing the target over it would be
        // pedantry: SigV4 needs SOME region string, and `us-east-1` is what MinIO's own docs suggest.
        let mut target = s3_target();
        target.region = None;
        target.endpoint = Some("http://127.0.0.1:9000".to_string());
        let config = s3_config(&target).unwrap();
        assert_eq!(config.region, "us-east-1");
    }

    #[test]
    fn an_upload_streams_the_archive_and_names_the_stored_key() {
        let dir = tempfile::TempDir::new().unwrap();
        let archive = dir.path().join("a.tar.gz");
        std::fs::write(&archive, b"ARCHIVE").unwrap();

        let fake = Arc::new(FakeHttpClient::new().always(200, ""));
        let target = adapter(Arc::clone(&fake), s3_target());
        let stored = target
            .upload(&archive, "veloct-backups/veloct-backup-20130524-000000-aaaaaaaaaaaa.tar.gz")
            .unwrap();
        assert_eq!(stored, "veloct-backups/veloct-backup-20130524-000000-aaaaaaaaaaaa.tar.gz");

        let requests = fake.recorded();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, HttpMethod::Put);
        // The path style is virtual-hosted with no custom endpoint, so the bucket is in the host.
        assert_eq!(
            requests[0].url,
            "https://veloct-private-backups.s3.us-east-1.amazonaws.com/veloct-backups/veloct-backup-20130524-000000-aaaaaaaaaaaa.tar.gz"
        );
        // A FILE body, so a large project is not read into memory to sign it.
        assert!(matches!(
            requests[0].body,
            veloct_core::http::HttpBody::File { .. }
        ));
        let authorization = requests[0]
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            .map(|(_, value)| value.clone())
            .unwrap();
        assert!(authorization.contains("Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request"), "{authorization}");
    }

    #[test]
    fn a_listing_keeps_only_this_applications_archives_and_uses_the_buckets_own_timestamps() {
        // Section 17.6's retention decides what to DELETE from this answer, so a stray object must never
        // appear — and `LastModified` is the service's fact, unlike GitHub where the name is all there is.
        let fake = Arc::new(FakeHttpClient::new().on_method_and_url(
            HttpMethod::Get,
            "list-type=2",
            200,
            r#"<ListBucketResult><IsTruncated>false</IsTruncated>
               <Contents><Key>veloct-backups/veloct-backup-20130524-000000-aaaaaaaaaaaa.tar.gz</Key>
                 <LastModified>2013-05-24T00:00:00.000Z</LastModified><Size>111</Size></Contents>
               <Contents><Key>veloct-backups/notes.md</Key>
                 <LastModified>2013-05-25T00:00:00.000Z</LastModified><Size>5</Size></Contents>
               </ListBucketResult>"#,
        ));
        let target = adapter(Arc::clone(&fake), s3_target());
        let listed = target.list().unwrap();

        assert_eq!(listed.len(), 1, "notes.md is not a backup");
        assert_eq!(listed[0].size_bytes, 111);
        assert_eq!(listed[0].created_at, "2013-05-24T00:00:00.000Z");
        // The listing was scoped to the target's own prefix.
        let requests = fake.recorded();
        assert!(requests[0].url.contains("prefix=veloct-backups%2F"), "{}", requests[0].url);
    }

    #[test]
    fn retention_deletes_the_oldest_through_the_adapter() {
        let fake = Arc::new(
            FakeHttpClient::new()
                .on_method_and_url(
                    HttpMethod::Get,
                    "list-type=2",
                    200,
                    r#"<ListBucketResult><IsTruncated>false</IsTruncated>
                       <Contents><Key>veloct-backups/veloct-backup-20130521-000000-aaaaaaaaaaaa.tar.gz</Key><LastModified>2013-05-21T00:00:00.000Z</LastModified><Size>1</Size></Contents>
                       <Contents><Key>veloct-backups/veloct-backup-20130522-000000-bbbbbbbbbbbb.tar.gz</Key><LastModified>2013-05-22T00:00:00.000Z</LastModified><Size>1</Size></Contents>
                       <Contents><Key>veloct-backups/veloct-backup-20130523-000000-cccccccccccc.tar.gz</Key><LastModified>2013-05-23T00:00:00.000Z</LastModified><Size>1</Size></Contents>
                       <Contents><Key>veloct-backups/veloct-backup-20130524-000000-dddddddddddd.tar.gz</Key><LastModified>2013-05-24T00:00:00.000Z</LastModified><Size>1</Size></Contents>
                       </ListBucketResult>"#,
                )
                .on_method_and_url(HttpMethod::Delete, "/veloct-backup-20130521", 204, ""),
        );
        let target = adapter(Arc::clone(&fake), s3_target());

        let removed = prune(
            &target,
            Some("veloct-backups/veloct-backup-20130524-000000-dddddddddddd.tar.gz"),
        )
        .unwrap();
        assert_eq!(removed, 1, "four objects and a retention of three");

        let deletes: Vec<String> = fake
            .recorded()
            .into_iter()
            .filter(|request| request.method == HttpMethod::Delete)
            .map(|request| request.url)
            .collect();
        assert_eq!(deletes.len(), 1);
        assert!(deletes[0].contains("veloct-backup-20130521"), "{deletes:?}");
    }

    #[test]
    fn a_missing_backup_directory_is_an_empty_listing_not_a_failure() {
        // A fresh bucket has no objects under the prefix, and reporting that as a failure would make the
        // normal first run look broken.
        let fake = Arc::new(FakeHttpClient::new().always(
            200,
            "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>",
        ));
        let target = adapter(fake, s3_target());
        assert!(target.list().unwrap().is_empty());
    }

    #[test]
    fn a_connection_test_names_the_bucket_and_the_endpoint() {
        let fake = Arc::new(FakeHttpClient::new().always(
            200,
            "<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>",
        ));
        let target = adapter(Arc::clone(&fake), s3_target());
        let said = target.test_connection().unwrap();
        assert!(said.contains("veloct-private-backups"), "{said}");
    }

    #[test]
    fn a_refusal_is_reported_with_the_buckets_own_error_code() {
        let fake = Arc::new(FakeHttpClient::new().always(
            403,
            "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
        ));
        let target = adapter(Arc::clone(&fake), s3_target());
        let error = target.test_connection().unwrap_err();
        let text = error.to_string();
        assert!(text.contains("AccessDenied"), "{text}");
        assert!(text.contains("\"s3\""), "the failing target is named: {text}");
    }

    #[test]
    fn the_timestamp_comes_from_the_injected_clock_and_not_the_system() {
        // A signature is a function of its timestamp, so a test that cannot pin the time cannot check one.
        let fake = Arc::new(FakeHttpClient::new().always(200, ""));
        let target = adapter(Arc::clone(&fake), s3_target());
        target.test_connection().unwrap();
        let requests = fake.recorded();
        let sent = requests[0]
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("x-amz-date"))
            .map(|(_, value)| value.clone())
            .unwrap();
        assert_eq!(sent, TIMESTAMP);
        assert!(requests[0]
            .headers
            .iter()
            .any(|(_, value)| value.contains("20130524/us-east-1/s3/aws4_request")));
    }

    #[test]
    fn a_target_without_a_bucket_is_reported_rather_than_attempted() {
        let mut broken = s3_target();
        broken.location = "   ".to_string();
        let fake = Arc::new(FakeHttpClient::new()) as Arc<dyn HttpClient>;
        let error = S3BackupTarget::new(broken, "k", "s", fake, clock()).unwrap_err();
        assert!(error.to_string().contains("no bucket"), "{error}");
    }
}
