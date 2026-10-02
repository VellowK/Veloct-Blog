//! Live verification of the GitHub Contents API through the ATTACHMENT target (`handoff.md` §13.13).
//!
//! **This test makes REAL requests to api.github.com and writes a real object into a real repository**, so it
//! is `#[ignore]`d: the ordinary suite must never depend on GitHub's uptime or on a credential existing.
//!
//! It exists because `PROCESSING.md` §5.8 records "no live service has ever been contacted" as this
//! repository's biggest honesty gap, and every other GitHub test asserts against `FakeHttpClient`. The
//! protocol lives ONCE, in `veroct-github` (ADR-0015), so what this file proves is that the client's request
//! shapes, its base64 body, its `sha` handling and its list/delete round trip all match the service — through
//! the §13.13 adapter's own code, not through a test-only request.
//!
//! Run it explicitly with your own token in the environment:
//!
//! ```powershell
//! $env:VELOCT_GITHUB_ATTACHMENT_TOKEN = "<a fine-grained PAT with Contents: read and write>"
//! $env:VELOCT_GITHUB_ATTACHMENT_REPO  = "owner/throwaway-repo"
//! powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test `
//!     -p veloct-attach --test live_github -- --ignored --nocapture
//! ```
//!
//! **A PUBLIC repository is expected here, and that is the point of the contrast.** §17.2 requires a PRIVATE
//! repository for a BACKUP, because an unencrypted archive must not be published; §13.13's attachment target is
//! the opposite by design — it exists so a file can be served OUT of the repository — so this target has no
//! privacy check and works on a public one. `crates/veloct-backup/tests/live_github.rs` asserts the other rule.
//!
//! **The token is read from the environment and is never printed**, and nothing here writes it anywhere. The
//! evidence printed below is object names, sizes and statuses only.
//!
//! **The object is REMOVED again before the test ends.** A live test that left litter in a user's repository
//! would be a reason not to run it, so cleanup is part of the evidence: the final `head` proves the delete
//! reached the service.
//!
//! **A missing token or repository is a FAILURE, not a skip**, for the reason the other live file states.

use std::sync::Arc;
use std::time::Duration;

use veloct_attach::{GitHubStore, PutOutcome, Repository, StorageAdapter, StorageConfig, StorageKind};
use veloct_core::http::ReqwestHttpClient;

const TOKEN_ENV: &str = "VELOCT_GITHUB_ATTACHMENT_TOKEN";
const REPO_ENV: &str = "VELOCT_GITHUB_ATTACHMENT_REPO";

/// The directory inside the repository the probe writes to, so a listing is scoped and the object is easy to
/// find if something goes wrong.
const PROBE_PREFIX: &str = "veloct-live-probe";

fn client() -> Arc<dyn veloct_core::http::HttpClient> {
    Arc::new(
        ReqwestHttpClient::with_default_timeout(Duration::from_secs(60))
            .expect("the HTTP client builds"),
    )
}

fn token() -> String {
    match std::env::var(TOKEN_ENV) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => panic!(
            "{TOKEN_ENV} is not set, so this live test cannot run. Set it to a fine-grained PAT with \
             Contents: read and write, e.g.\n  $env:{TOKEN_ENV} = \"<your token>\"\n  \
             $env:{REPO_ENV} = \"owner/throwaway-repo\"\n\
             A live test that skipped silently would be worse than no test: its whole purpose is evidence \
             that the documented contract matches the live service."
        ),
    }
}

fn repository() -> String {
    match std::env::var(REPO_ENV) {
        Ok(value) if !value.trim().is_empty() => {
            let slug = value.trim().to_string();
            println!("repository under test: {slug}");
            slug
        }
        _ => panic!("{REPO_ENV} is not set; see {TOKEN_ENV}'s message for the pair this test needs"),
    }
}

#[test]
#[ignore = "makes REAL requests to api.github.com and writes a real object into a real repository"]
fn the_attachment_target_round_trips_against_the_live_contents_api() {
    let slug = repository();
    let repository = Repository::parse(&slug).expect("owner/repo parses");

    let mut config = StorageConfig::new("live-github", "live probe", StorageKind::GitHub);
    config.repository = Some(slug.clone());
    // No branch is configured, and the adapter's own default covers it — which is itself worth exercising,
    // because the Contents API requires a branch on every write.
    config.key_prefix = Some(PROBE_PREFIX.to_string());

    let store = GitHubStore::new(config, repository, token(), "main", client());

    // ---- 1. The connection test ------------------------------------------------------------------------
    //
    // This reads the repository resource, so it proves the token can SEE the repository — a live 200 from the
    // real API, parsed by the crate's own code, rather than a fake client's queued answer.
    let sentence = store
        .test_connection()
        .expect("the repository is readable with this token");
    println!("connection: {sentence}");
    assert!(sentence.contains(&slug), "it must name the repository: {sentence}");

    // ---- 2. A write, with the bytes and the key the adapter chose --------------------------------------
    //
    // A unique name per run, so a leftover object from an interrupted run cannot make the next one pass for
    // the wrong reason.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after 1970")
        .as_millis();
    let key = format!("{PROBE_PREFIX}/live-probe-{stamp}.webp");
    let bytes = b"VELOCT LIVE PROBE: not a real image, and removed before this test returns.".to_vec();

    match store.put(&key, &bytes).expect("the object is written") {
        PutOutcome::Created(object) => {
            println!("put: created key={} url={} bytes={}", object.key, object.public_url, object.size_bytes);
            assert_eq!(object.key, key);
            assert_eq!(object.size_bytes, bytes.len() as u64);
        }
        // A key that already exists would mean the unique name collided, which the test's own naming makes
        // impossible — reported rather than tolerated, because "it was already there" would hide a failed write.
        PutOutcome::AlreadyPresent(object) => panic!(
            "{} already existed, so this run did not write anything: {}",
            object.key, object.public_url
        ),
    }

    // ---- 3. The object is really there ----------------------------------------------------------------
    //
    // `head` asks the service, and the SIZE it returns comes from the API rather than from the request — so a
    // truncated or empty upload cannot pass.
    let found = store
        .head(&key)
        .expect("the lookup is not an error")
        .unwrap_or_else(|| panic!("{key} must exist after a successful write"));
    println!("head: key={} bytes={} url={}", found.key, found.size_bytes, found.public_url);
    assert_eq!(found.size_bytes, bytes.len() as u64, "the stored object must be whole");

    // ---- 4. A delete, which the API only accepts with the file's current blob sha ----------------------
    //
    // This is the half §13.8's explicit cleanup depends on, and the two-step (read the sha, then delete) is
    // asserted here against the service that requires it.
    store.delete_remote(&key).expect("the object is removed");
    let gone = store.head(&key).expect("the second lookup is not an error");
    assert!(gone.is_none(), "the delete must be visible to a subsequent read: {gone:?}");
    println!("delete: {key} is gone (verified by a second live read)");

    println!("OK: wrote, read and removed {key} on {slug} through the §13.13 attachment target");
}
