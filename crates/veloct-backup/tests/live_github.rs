//! Live verification against the REAL GitHub Contents API (`handoff.md` §17.4, §17.2 and §13.13).
//!
//! **This test makes REAL requests to api.github.com and writes to a REAL repository**, so it is `#[ignore]`d:
//! the ordinary suite must never depend on GitHub's uptime or on a credential existing, and a green
//! `cargo test` must not be able to mean "the vendor was reachable today". It exists because every other §17.4
//! test asserts against `FakeHttpClient`, and `PROCESSING.md` §5.8 records "no live service has ever been
//! contacted" as this repository's biggest honesty gap. **This is the second service that gap is closed for**,
//! after Unsplash (D164), and the first CLOUD TARGET — so it is the first time the GitHub Contents API, the
//! `veloct-github` client and §17.2's privacy check have all run against the thing they describe.
//!
//! Run it explicitly, against a THROWAWAY PRIVATE repository, with your own token in the environment:
//!
//! ```powershell
//! $env:VELOCT_GITHUB_BACKUP_TOKEN = "<a fine-grained PAT with Contents: read and write>"
//! $env:VELOCT_GITHUB_BACKUP_REPO  = "owner/throwaway-private-repo"
//! powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test `
//!     -p veloct-backup --test live_github -- --ignored --nocapture
//! ```
//!
//! **The repository MUST be private, and that is asserted rather than assumed.** §17.2 requires it because the
//! archive is not encrypted, and the adapter re-checks GitHub's own `private` flag on every upload — so a
//! public repository makes this test FAIL with the adapter's refusal instead of publishing a project. **That is
//! not hypothetical: the first run of this file was against a PUBLIC repository and was refused, which is
//! recorded as the verification of the rule itself (matrix row D171).** The repository was made private
//! afterwards, and the round trip below then ran (D172).
//!
//! **A note for whoever changes a repository's visibility to run this:** the GitHub API's `permissions.admin`
//! field describes YOU, not the TOKEN. A fine-grained PAT needs its own **Administration: read and write**
//! permission, and without it `PATCH /repos/...` answers **403** — which is how the first attempt here failed.
//!
//! **The token is read from the environment and is never printed.** An environment variable is the test's home
//! for a credential because §12.2 gives the APPLICATION the OS store: a test that wrote to the real keyring
//! would leave a user's machine modified. Nothing here writes the token anywhere, and the evidence printed
//! below is built from repository facts and object names only.
//!
//! **A missing token or repository is a FAILURE, not a skip.** An `#[ignore]`d test that passed without doing
//! anything would look like evidence and be the opposite of it, which is the dishonesty §20 forbids.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use veloct_backup::github::GitHubBackupTarget;
use veloct_backup::targets::{BackupTargetAdapter, CloudTarget, CloudTargetKind};
use veloct_backup::{is_backup_name, remote_name};
use veloct_core::http::ReqwestHttpClient;

/// The environment variable holding a token with Contents read/write on the throwaway repository.
const TOKEN_ENV: &str = "VELOCT_GITHUB_BACKUP_TOKEN";

/// The environment variable holding `owner/repo` of a PRIVATE throwaway repository.
const REPO_ENV: &str = "VELOCT_GITHUB_BACKUP_REPO";

/// The directory inside the repository this probe writes to.
///
/// A prefix is used rather than the repository root on purpose: §17.6's retention lists a DIRECTORY and filters
/// by file name, so a probe that wrote to the root would not exercise the listing path a real backup uses.
const PROBE_DIRECTORY: &str = "veloct-live-probe";

fn client() -> ReqwestHttpClient {
    ReqwestHttpClient::with_default_timeout(Duration::from_secs(60)).expect("the HTTP client builds")
}

/// Read the token, or explain exactly what to set.
///
/// Deliberately a panic rather than an early return: see this file's own documentation.
fn token() -> String {
    match std::env::var(TOKEN_ENV) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => panic!(
            "{TOKEN_ENV} is not set, so this live test cannot run. Set it to a fine-grained PAT with \
             Contents: read and write on a PRIVATE throwaway repository, e.g.\n  \
             $env:{TOKEN_ENV} = \"<your token>\"\n  \
             $env:{REPO_ENV} = \"owner/throwaway-private-repo\"\n\
             A live test that skipped silently would be worse than no test: its whole purpose is evidence \
             that the documented contract matches the live service."
        ),
    }
}

fn repository() -> String {
    match std::env::var(REPO_ENV) {
        Ok(value) if !value.trim().is_empty() => {
            // Printed because it is not a secret and it is what makes captured output attributable.
            let slug = value.trim().to_string();
            println!("repository under test: {slug}");
            slug
        }
        _ => panic!("{REPO_ENV} is not set; see {TOKEN_ENV}'s message for the pair this test needs"),
    }
}

fn target(location: &str) -> CloudTarget {
    CloudTarget {
        id: "live-github".to_string(),
        kind: CloudTargetKind::GitHub,
        enabled: true,
        location: location.to_string(),
        prefix: Some(PROBE_DIRECTORY.to_string()),
        retention: 3,
        // The declaration §17.2 asks the user for. The adapter still asks GitHub on every upload, which is the
        // half this test is really exercising.
        private_confirmed: true,
        credential_ref: None,
        endpoint: None,
        region: None,
        access_key: None,
    }
}

/// A REAL gzip-compressed tar archive, however small.
///
/// Not a text file wearing a `.tar.gz` name: the application writes a gzip stream, and the point of a live test
/// is to exercise the path that really runs rather than a cheaper stand-in for it.
fn archive(directory: &Path) -> PathBuf {
    let path = directory.join("veloct-live-probe.tar.gz");
    let file = std::fs::File::create(&path).expect("the archive is created");

    let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
        file,
        flate2::Compression::fast(),
    ));
    let content = b"# live probe\n\nThis file exists only so a real archive has real content.\n";
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
#[ignore = "makes REAL requests to api.github.com and writes to a real private repository"]
fn the_github_backup_target_round_trips_against_the_live_api() {
    let repository = repository();
    let adapter = GitHubBackupTarget::new(target(&repository), token(), Arc::new(client()))
        .expect("the adapter builds with a token");

    // ---- 1. The connection test, through the crate's own code -------------------------------------------
    //
    // `test_connection` reads the repository resource, which proves the token can see the repository at all. It
    // prints a sentence the window shows a user, so the assertion is on its content rather than on `is_ok`.
    let sentence = adapter
        .test_connection()
        .expect("the repository is reachable with this token");
    println!("connection: {sentence}");
    assert!(
        sentence.contains(&repository),
        "the sentence must name the repository it proved: {sentence}"
    );

    // ---- 2. §17.2's privacy check, against GitHub's OWN answer ------------------------------------------
    //
    // The upload below re-checks the repository's privacy flag, so it FAILS on a public repository with the
    // adapter's own refusal. That is deliberate: the test asserts the happy path, and a public repository turns
    // it into the failure §17.2 exists to produce.
    let directory = tempfile::TempDir::new().expect("a temp directory");
    let archive_path = archive(directory.path());
    let bytes = std::fs::read(&archive_path).expect("the archive reads back");
    let content_hash = veloct_core::sha256_hex(&bytes);
    // A stamp WITHOUT milliseconds, because that is the shape retention can read back: `remote_name` strips
    // the separators and `archive_created_at` recovers `YYYYMMDD-HHMMSS`, so a `…T00:00:00.000Z` would produce
    // a time field that is not six digits and the listing would fall back to the path instead of the instant.
    let created_at = "2026-10-02T00:00:00Z";

    // The name comes from the application's OWN builder, with the target's own prefix: §17.6's retention
    // filters a listing by `is_backup_name` and sorts by the stamp `archive_created_at` recovers, so a
    // hand-written name would test a file that retention could not manage.
    let name = remote_name(created_at, &content_hash, Some(PROBE_DIRECTORY));
    assert!(
        is_backup_name(name.rsplit('/').next().unwrap_or(&name)),
        "{name} must be a name retention would keep"
    );

    let key = adapter
        .upload(&archive_path, &name)
        .expect("the archive uploads to a private repository");
    println!("uploaded: {key} ({} bytes)", bytes.len());

    // ---- 3. The listing finds it, with the path the API reports -----------------------------------------
    let listed = adapter.list().expect("the target lists");
    let found = listed
        .iter()
        .find(|entry| entry.path == key)
        .unwrap_or_else(|| panic!("the upload must appear in the listing: {listed:#?}"));
    println!(
        "listed: path={} size={} created_at={}",
        found.path, found.size_bytes, found.created_at
    );
    // The API reports the stored size, so a truncating upload cannot pass this.
    assert_eq!(
        found.size_bytes,
        bytes.len() as u64,
        "the stored object must be the whole archive"
    );
    // And the name carries the instant, which is what retention sorts by when the API reports no creation time
    // for a file. Asserted as a LITERAL rather than recomputed from `created_at`: a test that transformed the
    // input the same way the code does would agree with a wrong name.
    assert_eq!(
        found.created_at, "2026-10-02T00:00:00Z",
        "the creation instant is recovered from the archive's own name"
    );

    // ---- 4. It is removed again, so the probe leaves nothing behind --------------------------------------
    //
    // A live test that littered a user's repository would be a reason NOT to run it, so cleanup is part of the
    // evidence rather than an afterthought. The removal is asserted to be VISIBLE, not merely to return `Ok`.
    adapter.remove(&key).expect("the archive is removed");
    let after = adapter.list().expect("the target lists again");
    assert!(
        !after.iter().any(|entry| entry.path == key),
        "the removal must be visible in a listing: {after:#?}"
    );

    println!("OK: uploaded, listed and removed {key} on {repository}");
}
