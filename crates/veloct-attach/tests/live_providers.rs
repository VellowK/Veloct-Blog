//! Live verification against the real image providers (`handoff.md` §13.14).
//!
//! **These tests make REAL network requests to third-party services**, so they are `#[ignore]`d: the
//! ordinary suite must never depend on someone else's uptime, and a green `cargo test` must not be able
//! to mean "the vendors were reachable today". They exist because every other §13.14 test asserts against
//! `FakeHttpClient`, and PROCESSING.md §5.8 records "no live service has ever been contacted" as the
//! repository's biggest honesty gap. This is the one place that gap can be closed for these two sources.
//!
//! Run them explicitly, with your own key in the environment:
//!
//! ```powershell
//! $env:VELOCT_UNSPLASH_ACCESS_KEY = "<your access key>"
//! $env:VELOCT_PEXELS_API_KEY      = "<your api key>"     # optional
//! powershell -ExecutionPolicy Bypass -File scripts/cargo.ps1 test `
//!     -p veloct-attach --test live_providers -- --ignored --nocapture
//! ```
//!
//! **Each source is its own test, so ONE key is enough if you filter by name** — `--ignored` runs the
//! whole file and will therefore fail on a source you have no key for, which is deliberate (see below):
//!
//! ```powershell
//! # only the source you have a key for
//! ... --test live_providers -- --ignored unsplash --nocapture
//! # and the offline one that needs neither a key nor the network
//! ... --test live_providers -- pixabay
//! ```
//!
//! **The key is read from the environment and never printed.** The credential store is the application's
//! home for it (§12.2); an environment variable is the test's, because a test that wrote to the real OS
//! store would leave a user's keyring modified and would need the application's handle convention.
//!
//! **A missing key is a FAILURE, not a skip.** A test that quietly passed when it could not run would be
//! the exact dishonesty §20 forbids, and the point of running this file is the evidence.

use std::time::Duration;

use veloct_attach::network::providers;
use veloct_attach::{ImageSource, SearchQuery};
use veloct_core::http::ReqwestHttpClient;

/// The environment variable holding Unsplash's access key.
///
/// Only the ACCESS key is needed: the API's public endpoints authenticate with
/// `Authorization: Client-ID <access key>` (their OpenAPI `securitySchemes.Auth`). The secret key belongs
/// to the user-authentication OAuth flow, which this application does not use — so it is deliberately not
/// read by anything here.
const UNSPLASH_KEY_ENV: &str = "VELOCT_UNSPLASH_ACCESS_KEY";

/// The environment variable holding Pexels's API key.
const PEXELS_KEY_ENV: &str = "VELOCT_PEXELS_API_KEY";

fn client() -> ReqwestHttpClient {
    ReqwestHttpClient::with_default_timeout(Duration::from_secs(30))
        .expect("the HTTP client builds")
}

/// Read a key, or explain exactly what to set.
///
/// Deliberately a panic rather than an early return: an `#[ignore]`d test that passed without doing
/// anything would look like evidence and be the opposite.
fn key(variable: &str) -> String {
    match std::env::var(variable) {
        Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => panic!(
            "{variable} is not set, so this live test cannot run. Set it to a real key, e.g.\n  \
             $env:{variable} = \"<your key>\"\n\
             A live test that skipped silently would be worse than no test: its whole purpose is evidence \
             that the documented contract matches the live service."
        ),
    }
}

/// Print a result WITHOUT its key, so the captured output is safe to keep as evidence.
fn describe(source: ImageSource, result: &veloct_attach::ImageResult) -> String {
    format!(
        "[{}] id={} author={:?} {}x{} url_host={} ixid={} page={} download_location={}",
        source.as_str(),
        result.source_id,
        result.author,
        result.width,
        result.height,
        result
            .url
            .split('/')
            .nth(2)
            .unwrap_or("(no host)"),
        result.url.contains("ixid="),
        result.source_page.is_some(),
        result.download_location.is_some(),
    )
}

#[test]
#[ignore = "makes a real network request; run with --ignored and a key in the environment"]
fn unsplash_returns_usable_results_from_the_live_api() {
    let api_key = key(UNSPLASH_KEY_ENV);
    let client = client();

    let query = SearchQuery::new(ImageSource::Unsplash, "coffee").expect("a query");
    let found = veloct_attach::search_images(&client, ImageSource::Unsplash, &api_key, &query)
        .expect("Unsplash answers a real search");

    println!(
        "Unsplash: {} raw result(s), has_more={}",
        found.results.len(),
        found.has_more
    );

    let usable = found.usable();
    assert!(
        !usable.is_empty(),
        "a live search for a common word returned nothing usable; response had {} raw result(s)",
        found.results.len()
    );

    let first = usable[0];
    println!("{}", describe(ImageSource::Unsplash, first));

    // ---- The parts of the contract the live service must actually satisfy ----------------

    // Their guidelines say the `ixid` parameter must be kept, because it is what reports a view back to
    // the photographer. A live URL without it would mean the documentation and the service disagree.
    assert!(
        first.url.contains("ixid="),
        "the live URL lost its ixid parameter, which their API Guidelines require be kept: {}",
        first.url
    );
    assert!(
        first.url.starts_with("https://"),
        "image URLs must be https: {}",
        first.url
    );
    assert!(
        first.url.contains("w=1080"),
        "the `regular` rendition is the documented 1080 px one: {}",
        first.url
    );
    assert!(
        !first.thumbnail_url.is_empty(),
        "the picker's thumbnail must come from the response"
    );

    // Attribution is a condition of API use (guideline 3), so every one of its parts is asserted
    // against a real payload rather than against a fixture we wrote ourselves.
    assert!(!first.author.trim().is_empty(), "the photographer is named");
    assert!(
        first.author_url.is_some(),
        "guideline 3 requires a link back to the photographer's profile"
    );
    assert!(
        first.source_page.is_some(),
        "the photo's own page is needed for the attribution line"
    );

    let attribution = providers::attribution_markdown(first);
    println!("attribution: {attribution}");
    assert!(attribution.contains("Unsplash"), "{attribution}");
    assert!(attribution.contains(first.author.trim()), "{attribution}");
    assert!(
        attribution.contains("utm_source=veloct") && attribution.contains("utm_medium=referral"),
        "guideline 3 requires utm parameters on every link back to Unsplash: {attribution}"
    );

    // ---- Guideline 2: choosing an image MUST report the download -------------------------
    //
    // This is the compliance step, and it is the one thing the fake-client tests cannot prove: that the
    // endpoint the live service hands back actually accepts the request the crate builds.
    assert!(
        first.download_location.is_some(),
        "Asset.Basic.links.download_location is `required` in their spec"
    );
    let reported = veloct_attach::report_download(&client, ImageSource::Unsplash, &api_key, first)
        .expect("the required download report is accepted");
    println!("download report answered: {:?}", reported.is_some());
    assert!(
        reported.is_some(),
        "the download endpoint returns the JSON body their spec documents"
    );
}

#[test]
#[ignore = "makes a real network request; run with --ignored and a key in the environment"]
fn pexels_returns_usable_results_from_the_live_api() {
    let api_key = key(PEXELS_KEY_ENV);
    let client = client();

    let query = SearchQuery::new(ImageSource::Pexels, "nature").expect("a query");
    let found = veloct_attach::search_images(&client, ImageSource::Pexels, &api_key, &query)
        .expect("Pexels answers a real search");

    println!(
        "Pexels: {} raw result(s), has_more={}",
        found.results.len(),
        found.has_more
    );

    let usable = found.usable();
    assert!(
        !usable.is_empty(),
        "a live search returned nothing usable; response had {} raw result(s)",
        found.results.len()
    );

    let first = usable[0];
    println!("{}", describe(ImageSource::Pexels, first));

    assert!(
        first.url.starts_with("https://"),
        "image URLs must be https: {}",
        first.url
    );
    assert!(
        first.url.contains("w=940"),
        "`src.large` is the documented 940 px rendition: {}",
        first.url
    );
    assert!(!first.author.trim().is_empty(), "the photographer is named");
    assert!(
        first.source_page.is_some(),
        "their guidelines ask for a link to the photo page, so the page URL must be in the response"
    );

    let attribution = providers::attribution_markdown(first);
    println!("attribution: {attribution}");
    assert!(attribution.contains("Pexels"), "{attribution}");
    assert!(attribution.contains(first.author.trim()), "{attribution}");

    // Pexels documents NO download-reporting obligation, so the crate must build no request. Asserting
    // this against the live response is what keeps `download_tracking_request`'s `None` honest: if their
    // API ever started returning a `download_location`, this would notice.
    assert!(
        first.download_location.is_none(),
        "Pexels's Photo resource has no download_location; if that changed, the crate's no-op for this \
         source needs revisiting"
    );
    assert!(
        veloct_attach::download_tracking_request(ImageSource::Pexels, first, &api_key)
            .expect("no obligation means no error")
            .is_none(),
        "no download step for Pexels"
    );
}

/// Prove the refusal is not just a comment: the source §13.1 names is refused by the crate itself.
///
/// Needs no key and no network, but it lives here because it is the other half of "which sources does
/// this build actually talk to" and belongs beside the two that do.
#[test]
fn pixabay_is_refused_by_the_crate_with_its_own_reason() {
    assert!(!ImageSource::Pixabay.is_implemented());
    let reason = providers::unimplemented_reason(ImageSource::Pixabay).expect("a reason");
    assert!(reason.contains("热链"), "{reason}");
    assert!(providers::facts(ImageSource::Pixabay).is_err());
    // And a search cannot even be built for it.
    assert!(SearchQuery::new(ImageSource::Pixabay, "coffee").is_err());
}
