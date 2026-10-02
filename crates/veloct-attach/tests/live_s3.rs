//! Live verification of the S3 / S3-compatible ATTACHMENT target (`handoff.md` §13.11).
//!
//! **This test makes REAL signed requests to a real object store and writes a real object into it**, so it is
//! `#[ignore]`d: the ordinary suite must never depend on a bucket, a credential or a vendor's uptime. It exists
//! because every other §13.11 test asserts against `FakeHttpClient`, and `PROCESSING.md` §5.8 records "no live
//! service has ever been contacted" as this repository's biggest honesty gap.
//!
//! Run it explicitly:
//!
//! ```powershell
//! $env:VELOCT_S3_ACCESS_KEY_ID     = "<access key id>"
//! $env:VELOCT_S3_SECRET_ACCESS_KEY = "<secret access key>"
//! $env:VELOCT_S3_BUCKET            = "your-bucket"
//! $env:VELOCT_S3_REGION            = "cn-chengdu"
//! $env:VELOCT_S3_ENDPOINT          = "https://{bucket}.oss-cn-chengdu.aliyuncs.com"
//! $env:VELOCT_S3_PUBLIC_URL_PREFIX = "https://your-bucket.oss-cn-chengdu.aliyuncs.com"
//! powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test `
//!     -p veloct-attach --test live_s3 -- --ignored --nocapture
//! ```
//!
//! **`{bucket}` in the endpoint is how the ADDRESSING is chosen, and for OSS it is required.** Alibaba Cloud OSS
//! refuses a path-style bucket request in its own words — `403 SecondLevelDomainForbidden`, "The bucket you are
//! attempting to access must be addressed using OSS third level domain" — so the bucket has to go into the
//! hostname. A self-hosted MinIO, by contrast, wants `https://host:9000/bucket/key`, which is what an endpoint
//! WITHOUT the placeholder means. Both were measured against the real services, not assumed:
//!
//! * the same LIST as the bucket subdomain with SigV4's service scope reading **`s3`** answers **200** (which is
//!   the scope `veloct-s3` signs with), while the scope **`oss`** answers **400 InvalidArgument**;
//! * the `-internal` OSS endpoint (`oss-cn-<region>-internal.aliyuncs.com`) is **unreachable** from outside
//!   Alibaba's network, so a test run from a workstation must use the public one.
//!
//! **The access key ID is configuration and the secret is a credential** (§12.2's distinction, recorded in
//! ADR-0016): the ID goes into the target's configuration, the secret is read from the environment here and is
//! never printed or written anywhere.
//!
//! **A missing variable is a FAILURE, not a skip.** The whole point of running this file is the evidence.
//!
//! **The object is written under a dedicated prefix and REMOVED again**, so a run leaves a user's bucket as it
//! found it — a live test that littered real storage would be a reason not to run it.

use std::sync::Arc;
use std::time::Duration;

use veloct_attach::{PutOutcome, S3Store, StorageAdapter, StorageConfig, StorageKind};
use veloct_core::http::ReqwestHttpClient;

/// The directory inside the bucket this probe writes to. Deliberately its own prefix, so nothing a user already
/// keeps in the bucket is touched.
const PROBE_PREFIX: &str = "veloct-live-probe";

fn required(variable: &str) -> String {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => panic!(
            "{variable} is not set, so this live test cannot run. See this file's documentation for the six \
             variables it needs (the endpoint may contain `{{bucket}}`, which is how virtual-hosted addressing \
             is chosen — Alibaba Cloud OSS requires it). A live test that skipped silently would be worse than \
             no test: its whole purpose is evidence that the documented contract matches the live service."
        ),
    }
}

#[test]
#[ignore = "makes REAL signed requests to a real object store and writes a real object into it"]
fn the_s3_attachment_target_round_trips_against_the_live_service() {
    let bucket = required("VELOCT_S3_BUCKET");
    let region = required("VELOCT_S3_REGION");
    let endpoint = required("VELOCT_S3_ENDPOINT");
    let access_key_id = required("VELOCT_S3_ACCESS_KEY_ID");
    let secret = required("VELOCT_S3_SECRET_ACCESS_KEY");
    let public_url_prefix = required("VELOCT_S3_PUBLIC_URL_PREFIX");

    // Printed because none of it is a credential, and because it is what makes captured output attributable.
    println!("bucket={bucket} region={region} endpoint={endpoint}");

    let mut config = StorageConfig::new("live-s3", "live probe", StorageKind::S3);
    config.bucket = Some(bucket.clone());
    config.region = Some(region.clone());
    config.endpoint = Some(endpoint.clone());
    // §13.11 and ADR-0016: the ID is configuration, the SECRET is the credential.
    config.access_key = Some(access_key_id);
    // §13.6 keeps the public URL prefix apart from the object's path, and the adapter builds the object's URL
    // from it — so a target without one cannot report where a file landed. Hence a required variable.
    config.public_url_prefix = public_url_prefix;

    let client = Arc::new(
        ReqwestHttpClient::with_default_timeout(Duration::from_secs(60))
            .expect("the HTTP client builds"),
    ) as Arc<dyn veloct_core::http::HttpClient>;
    let store = S3Store::new(
        config,
        secret,
        client,
        Arc::new(veloct_core::SystemClock),
    )
    .expect("the adapter builds from the configured target");

    // ---- 1. The connection test ----------------------------------------------------------------------
    //
    // A one-object LIST rather than a HeadBucket, because §13.11's configurations grant object permissions far
    // more often than bucket-level ones. This is also the request that proves the ENDPOINT and the ADDRESSING:
    // OSS answers a path-style bucket request with 403 SecondLevelDomainForbidden, so a wrong shape fails here.
    let sentence = store
        .test_connection()
        .expect("the bucket is reachable with this credential");
    println!("connection: {sentence}");
    assert!(sentence.contains(&bucket), "it must name the bucket: {sentence}");

    // ---- 2. A write, under a unique key ---------------------------------------------------------------
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_millis();
    let key = format!("{PROBE_PREFIX}/live-probe-{stamp}.bin");
    let bytes = b"VELOCT LIVE PROBE: not real content, and removed before this test returns.".to_vec();

    match store.put(&key, &bytes).expect("the object is written") {
        PutOutcome::Created(object) => {
            println!(
                "put: created key={} url={} bytes={}",
                object.key, object.public_url, object.size_bytes
            );
            assert_eq!(object.key, key);
            assert_eq!(object.size_bytes, bytes.len() as u64);
            // The public URL comes from the configured prefix (§13.6) and is BUILT, not fetched — so this
            // asserts the shape the Markdown would carry rather than that the object is publicly readable.
            assert!(
                object.public_url.starts_with("http"),
                "the object needs a URL to cite: {}",
                object.public_url
            );
        }
        PutOutcome::AlreadyPresent(object) => panic!(
            "{} already existed, so this run did not write anything: {}",
            object.key, object.public_url
        ),
    }

    // ---- 3. The object is really there, at the size the SERVICE reports -------------------------------
    //
    // `head` on S3 returns `Content-Length`, so this compares what the bucket stored against what was sent —
    // a truncated or empty upload cannot pass.
    let found = store
        .head(&key)
        .expect("the lookup is not an error")
        .unwrap_or_else(|| panic!("{key} must exist after a successful write"));
    println!("head: key={} bytes={}", found.key, found.size_bytes);
    assert_eq!(found.size_bytes, bytes.len() as u64, "the stored object must be whole");

    // ---- 4. A delete, which §13.8's explicit cleanup is the only caller of ----------------------------
    store.delete_remote(&key).expect("the object is removed");
    let gone = store.head(&key).expect("the second lookup is not an error");
    assert!(gone.is_none(), "the delete must be visible to a subsequent read: {gone:?}");
    println!("delete: {key} is gone (verified by a second live read)");

    println!("OK: wrote, read and removed {key} in bucket {bucket}");
}
