//! Live verification of the S3 / S3-compatible CLOUD BACKUP target (`handoff.md` §17.4).
//!
//! **This test makes REAL signed requests to a real bucket and writes a real archive into it**, so it is
//! `#[ignore]`d. It is the backup twin of `crates/veloct-attach/tests/live_s3.rs`: the same shared client
//! (ADR-0015) in its other role, and the two together are what turns "S3 is asserted against `FakeHttpClient`"
//! into a verified claim.
//!
//! Run it explicitly, with the SAME five variables the attachment test uses:
//!
//! ```powershell
//! $env:VELOCT_S3_ACCESS_KEY_ID     = "<access key id>"
//! $env:VELOCT_S3_SECRET_ACCESS_KEY = "<secret access key>"
//! $env:VELOCT_S3_BUCKET            = "your-bucket"
//! $env:VELOCT_S3_REGION            = "cn-chengdu"
//! $env:VELOCT_S3_ENDPOINT          = "https://{bucket}.oss-cn-chengdu.aliyuncs.com"
//! powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test `
//!     -p veloct-backup --test live_s3 -- --ignored --nocapture
//! ```
//!
//! **`{bucket}` in the endpoint chooses the addressing and is REQUIRED by Alibaba Cloud OSS**, which refuses a
//! path-style bucket request with `403 SecondLevelDomainForbidden`. A self-hosted MinIO wants the opposite
//! (`https://host:9000/bucket/key`), which is what an endpoint without the placeholder means. `veloct-s3`
//! records both measurements where they are implemented.
//!
//! **This file is also the regression test for the defect that made it necessary.** Before it, no S3 request
//! this application made could have succeeded against ANY provider: the `x-amz-date` SigV4 signs was built by
//! deleting `-` and `:` from an RFC 3339 instant, which leaves a real clock's sub-second digits behind
//! (`20261002T034316.5888216Z` — twenty-four characters where the format allows sixteen). OSS answered
//! `403 AccessDenied: OSS authentication requires a valid Date`, and every unit test missed it because a FIXED
//! clock renders `...T00:00:00Z`. The first run of this pair of files is what found it.
//!
//! **The archive is written under a dedicated prefix and REMOVED again**, and `list()` only ever considers
//! names matching §17.6's `veloct-backup-…tar.gz` shape, so a run cannot touch anything a user already keeps in
//! the bucket. **The credential is read from the environment, never printed, and never written anywhere.**
//! **A missing variable is a FAILURE, not a skip.**

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use veloct_backup::s3::S3BackupTarget;
use veloct_backup::targets::{BackupTargetAdapter, CloudTarget, CloudTargetKind};
use veloct_backup::{is_backup_name, remote_name};
use veloct_core::http::ReqwestHttpClient;

/// The directory inside the bucket this probe writes to.
const PROBE_PREFIX: &str = "veloct-live-probe";

fn required(variable: &str) -> String {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => panic!(
            "{variable} is not set, so this live test cannot run. It needs VELOCT_S3_ACCESS_KEY_ID, \
             VELOCT_S3_SECRET_ACCESS_KEY, VELOCT_S3_BUCKET, VELOCT_S3_REGION and VELOCT_S3_ENDPOINT — the same \
             five the attachment test uses. A live test that skipped silently would be worse than no test."
        ),
    }
}

/// A REAL gzip-compressed tar archive, however small: the application writes a gzip stream and STREAMS it to
/// S3 (`put_file`), so the bytes must be a real file rather than a string.
fn archive(directory: &Path) -> PathBuf {
    let path = directory.join("veloct-live-probe.tar.gz");
    let file = std::fs::File::create(&path).expect("the archive is created");
    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::fast(),
    ));
    let content = b"# live probe\n\nA real archive, so the streaming upload has real bytes to send.\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, "content/probe.md", &content[..])
        .expect("the entry is appended");
    builder
        .into_inner()
        .expect("the tar stream finishes")
        .finish()
        .expect("the gzip stream finishes");
    path
}

#[test]
#[ignore = "makes REAL signed requests to a real bucket and writes a real archive into it"]
fn the_s3_backup_target_round_trips_against_the_live_service() {
    let bucket = required("VELOCT_S3_BUCKET");
    let region = required("VELOCT_S3_REGION");
    let endpoint = required("VELOCT_S3_ENDPOINT");
    let access_key_id = required("VELOCT_S3_ACCESS_KEY_ID");
    let secret = required("VELOCT_S3_SECRET_ACCESS_KEY");
    println!("bucket={bucket} region={region} endpoint={endpoint}");

    let target = CloudTarget {
        id: "live-s3-backup".to_string(),
        kind: CloudTargetKind::S3,
        enabled: true,
        location: bucket.clone(),
        prefix: Some(PROBE_PREFIX.to_string()),
        retention: 3,
        // §17.2's declaration. S3 has no API that answers "is this private", which is exactly why the user's
        // declaration is honoured by the orchestration and the bucket's own policy is the real protection.
        private_confirmed: true,
        credential_ref: None,
        endpoint: Some(endpoint.clone()),
        region: Some(region.clone()),
        access_key: Some(access_key_id),
    };

    let client = Arc::new(
        ReqwestHttpClient::with_default_timeout(Duration::from_secs(120))
            .expect("the HTTP client builds"),
    ) as Arc<dyn veloct_core::http::HttpClient>;
    let adapter = S3BackupTarget::new(
        target,
        // The ID is configuration and the secret is the credential (§12.2); both go to the signer, and only
        // the sentence below is printed.
        required("VELOCT_S3_ACCESS_KEY_ID"),
        secret,
        client,
        Arc::new(veloct_core::SystemClock),
    )
    .expect("the adapter builds from the configured target");

    // ---- 1. The connection test ----------------------------------------------------------------------
    let sentence = adapter
        .test_connection()
        .expect("the bucket is reachable with this credential");
    println!("connection: {sentence}");
    assert!(sentence.contains(&bucket), "it must name the bucket: {sentence}");

    // ---- 2. A real archive is STREAMED up under the application's own name ---------------------------
    let directory = tempfile::TempDir::new().expect("a temp directory");
    let archive_path = archive(directory.path());
    let bytes = std::fs::read(&archive_path).expect("the archive reads back");
    let content_hash = veloct_core::sha256_hex(&bytes);
    // The shape `remote_name` itself generates: §17.6's retention recognises the name, and the bucket reports
    // the timestamp for the LIST below — the S3 path takes `LastModified` from the service rather than parsing
    // the name, which is the difference between this file and the GitHub one.
    let name = remote_name("2026-10-02T00:00:00Z", &content_hash, Some(PROBE_PREFIX));
    assert!(
        is_backup_name(name.rsplit('/').next().unwrap_or(&name)),
        "{name} must be a name retention would keep"
    );

    let key = adapter
        .upload(&archive_path, &name)
        .expect("the archive is streamed to the bucket");
    println!("uploaded: {key} ({} bytes)", bytes.len());
    assert_eq!(key, name);

    // ---- 3. The listing finds it, and the timestamp is the BUCKET's own fact -------------------------
    let listed = adapter.list().expect("the target lists");
    let found = listed
        .iter()
        .find(|entry| entry.path == key)
        .unwrap_or_else(|| panic!("the upload must appear in the listing: {listed:#?}"));
    println!(
        "listed: path={} size={} created_at={}",
        found.path, found.size_bytes, found.created_at
    );
    assert_eq!(
        found.size_bytes,
        bytes.len() as u64,
        "the stored object must be the whole archive"
    );
    // S3 reports a timestamp per object, so this asserts a REAL one arrived rather than a placeholder. It is
    // the bucket's `LastModified`, which is why nothing here parses a name to sort by — §17.6's retention does.
    assert!(
        found.created_at.len() >= 20 && found.created_at.starts_with("20"),
        "the bucket must report when it stored the object: {:?}",
        found.created_at
    );

    // ---- 4. Removed again, so the probe leaves the bucket as it found it -----------------------------
    adapter.remove(&key).expect("the archive is removed");
    let after = adapter.list().expect("the target lists again");
    assert!(
        !after.iter().any(|entry| entry.path == key),
        "the removal must be visible in a listing: {after:#?}"
    );
    println!("delete: {key} is gone (verified by a second live listing)");

    println!("OK: streamed, listed and removed {key} in bucket {bucket}");
}
