//! The GitHub Contents API, as ONE client.
//!
//! `handoff.md` names GitHub in two contracts that have nothing to do with each other:
//!
//! * **§13.13** — a repository as an **attachment storage target**: mutable files that are served
//!   publicly from `raw.githubusercontent.com`, where dedup and a public URL prefix matter.
//! * **§17.4** — a repository as a **cloud backup target**: a PRIVATE repository holding whole-project
//!   archives, where listing, retention and the §17.2 privacy rule matter.
//!
//! They are different features with different configuration and different policy, but they speak the
//! same protocol: the same contents URL, the same `Authorization`/`User-Agent`/API-version headers, the
//! same base64 body, and the same two-step delete. A second copy of those would be a second place for
//! the protocol to be wrong — the habit this project holds itself to. So the protocol lives here once,
//! and each dependent keeps its own policy: what a key looks like, what a listing means, whether a
//! public URL is derived at all.
//!
//! What is deliberately NOT here:
//!
//! * **Git-over-HTTP.** §13.13 and §17.4 both make a repository a place to PUT an object, not a place
//!   to clone, and a commit per object through a working tree would need one.
//! * **Any public-URL policy.** A backup target is private and serves nothing; an attachment target
//!   derives `raw.githubusercontent.com` or a configured CDN prefix. That difference is the callers'.
//! * **Streaming.** The Contents API takes the object base64-encoded inside a JSON body, so the bytes
//!   must be in memory. [`CONTENTS_API_MAX_BYTES`] is that limit stated as a fact rather than left for
//!   a caller to discover as an out-of-memory crash, and `veloct-backup` refuses above it with a
//!   message that names a target that CAN stream.

#![forbid(unsafe_code)]

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use veloct_core::http::{HttpClient, HttpMethod, HttpRequest};
use veloct_core::secret::Redacted;
use veloct_core::{Result, VeloctError};

/// The GitHub Contents API's base.
pub const GITHUB_API: &str = "https://api.github.com";

/// The largest object the Contents API accepts, in bytes.
///
/// GitHub's own documented ceiling for a file created through this endpoint. Veloct states it because
/// the failure it prevents is not a clean HTTP error: the request body is the object base64-encoded
/// inside JSON, so an object above this limit is roughly 1.37x its size in memory before anything is
/// sent. A caller that needs more must stream, which means a different target.
pub const CONTENTS_API_MAX_BYTES: u64 = 100 * 1024 * 1024;

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
    /// A trailing `.git` and a trailing slash are accepted because both are what a user copies, and
    /// rejecting them would be pedantry that produces a confusing error.
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
        // A third segment means the user pasted a path INSIDE the repository, which is a different thing
        // from the repository. Refusing is better than silently truncating to the wrong place.
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

/// One object, or one directory entry, as the Contents API describes it.
///
/// `sha` is optional because the two callers ask different questions: a listing always carries one (and
/// a delete cannot proceed without it), while "does this exist, and how big is it" does not need one and
/// the API's answer is not required to include it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentsEntry {
    /// The full path inside the repository.
    pub path: String,
    /// The last path segment.
    pub name: String,
    pub size_bytes: u64,
    /// The blob sha, which a delete or an update must send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
}

/// What a write did.
///
/// The Contents API answers 201 for a create and 200 for an update, and §13.4's reuse report depends on
/// telling those two apart, so the distinction is preserved rather than flattened to `Ok(())`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Created,
    Updated,
}

/// Facts about a repository that a caller's POLICY depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryInfo {
    /// Whether the repository is private — the fact `handoff.md` §17.2 turns into a requirement.
    pub private: bool,
    /// The branch the API reports as the default, which a caller may use to replace a guessed one.
    pub default_branch: Option<String>,
}

/// The Contents API for one repository, authenticated with one credential.
///
/// `Debug` is safe to derive: the credential is a [`Redacted`], whose own `Debug` prints the redaction
/// rather than the value, and the client is a trait object that must already be `Debug`.
#[derive(Debug)]
pub struct GitHubContents {
    repository: Repository,
    /// A credential, wrapped so it cannot be printed by accident (§18.1).
    token: Redacted,
    branch: String,
    client: Arc<dyn HttpClient>,
}

impl GitHubContents {
    /// A client for one repository and branch.
    ///
    /// An empty branch becomes `main`, because the write path requires a branch and an empty one would
    /// produce a request that fails with a message about a missing ref rather than about configuration.
    pub fn new(
        repository: Repository,
        token: impl Into<String>,
        branch: impl Into<String>,
        client: Arc<dyn HttpClient>,
    ) -> Self {
        let branch = branch.into();
        Self {
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

    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The API path for one object.
    pub fn contents_url(&self, key: &str) -> String {
        format!(
            "{GITHUB_API}/repos/{}/{}/contents/{key}",
            self.repository.owner, self.repository.name
        )
    }

    /// The API path for a directory listing.
    ///
    /// The repository root has no path segment, and `contents/` with a trailing slash is a different
    /// request from `contents` for some proxies, so the two forms are built explicitly.
    pub fn directory_url(&self, directory: &str) -> String {
        let trimmed = directory.trim_matches('/');
        if trimmed.is_empty() {
            format!(
                "{GITHUB_API}/repos/{}/{}/contents",
                self.repository.owner, self.repository.name
            )
        } else {
            self.contents_url(trimmed)
        }
    }

    /// The repository resource path, for `test_connection` and the privacy check.
    pub fn repository_url(&self) -> String {
        format!("{GITHUB_API}/repos/{}", self.repository.slug())
    }

    /// A request with the headers every GitHub call needs.
    pub fn request(&self, method: HttpMethod, url: String) -> HttpRequest {
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
    ///
    /// The variant is deliberately feature-neutral. This client serves two contracts that classify their
    /// own failures differently — §13.13 an attachment, §17.4 a backup — so the specific variant is the
    /// dependent's to add at its own boundary, and picking one here would label the other wrongly.
    pub fn refused(&self, what: &str, status: u16) -> VeloctError {
        VeloctError::Other(format!(
            "GitHub refused to {what}: HTTP {status}. Check the token's contents-write access and the branch."
        ))
    }

    /// Create or replace one object on this client's branch.
    pub fn put(&self, key: &str, bytes: &[u8]) -> Result<WriteOutcome> {
        self.put_on_branch(key, bytes, &self.branch)
    }

    /// Create or replace one object on an explicit branch.
    ///
    /// The override exists because the two dependents know the branch differently. §13.13's attachment
    /// target has a user-configured branch, which is this client's. §17.4's backup target has none —
    /// asking a user to name a branch for a backup repository would be inventing configuration — so it
    /// uses the repository's own default branch, which only the API can report.
    pub fn put_on_branch(&self, key: &str, bytes: &[u8], branch: &str) -> Result<WriteOutcome> {
        let body = serde_json::json!({
            "message": format!("Add {key}"),
            "content": base64_encode(bytes),
            "branch": branch,
        });
        let request = self.request(HttpMethod::Put, self.contents_url(key)).json(body);

        let response = self.client.execute(request)?;
        if !response.is_success() {
            return Err(self.refused("upload a file", response.status));
        }
        Ok(if response.status == 201 {
            WriteOutcome::Created
        } else {
            WriteOutcome::Updated
        })
    }

    /// Whether an object exists, and its size and sha when it does.
    ///
    /// A missing object is `None` rather than an error: asking whether something is there is not a
    /// failure when the answer is no.
    pub fn get(&self, key: &str) -> Result<Option<ContentsEntry>> {
        let response = self
            .client
            .execute(self.request(HttpMethod::Get, self.contents_url(key)))?;
        match response.status {
            200 => {
                let value = response.json::<serde_json::Value>().unwrap_or_default();
                Ok(Some(entry_from_value(&value, key)))
            }
            404 => Ok(None),
            other => Err(VeloctError::Other(format!(
                "GitHub could not answer whether {key} exists: HTTP {other}"
            ))),
        }
    }

    /// List the FILES directly inside a directory.
    ///
    /// A directory the API does not know is an EMPTY listing rather than an error, because that is what
    /// a first run against a fresh repository looks like: the directory does not exist until something
    /// is written into it, and reporting that as a failure would make the normal case look broken.
    ///
    /// Sub-directories are omitted rather than returned as zero-byte entries: a caller that asked for
    /// the files in a directory would otherwise try to reason about a size that no file has.
    ///
    /// The API returns at most 1000 entries and does not paginate this endpoint, so a directory with
    /// more than that is truncated — stated here because retention would otherwise silently stop
    /// seeing older objects. Veloct's own backup names sort chronologically (see
    /// `veloct_backup::remote_name`), and its default retention is 3, so this is a bound rather than a
    /// live limitation.
    pub fn list(&self, directory: &str) -> Result<Vec<ContentsEntry>> {
        let response = self
            .client
            .execute(self.request(HttpMethod::Get, self.directory_url(directory)))?;
        match response.status {
            200 => {}
            404 => return Ok(Vec::new()),
            other => {
                return Err(VeloctError::Other(format!(
                    "GitHub could not list the directory {directory:?}: HTTP {other}"
                )))
            }
        }

        let value = response.json::<serde_json::Value>().map_err(|error| {
            VeloctError::Other(format!(
                "GitHub's listing of {directory:?} was not JSON: {error}"
            ))
        })?;
        let array = value.as_array().ok_or_else(|| {
            // An object here means a FILE was requested as though it were a directory. Saying so beats
            // iterating over the object's keys.
            VeloctError::Other(format!(
                "GitHub answered {directory:?} with an object rather than a directory listing"
            ))
        })?;

        Ok(array
            .iter()
            .filter(|item| item.get("type").and_then(|kind| kind.as_str()) == Some("file"))
            .map(|item| {
                let path = item
                    .get("path")
                    .and_then(|path| path.as_str())
                    .unwrap_or_default()
                    .to_string();
                entry_from_value(item, &path)
            })
            .collect())
    }

    /// Remove one object from this client's branch.
    ///
    /// §13.8's two-step: the API requires the current blob sha, so it is fetched first. Guessing it
    /// would fail.
    pub fn delete(&self, key: &str) -> Result<()> {
        self.delete_on_branch(key, &self.branch)
    }

    /// Remove one object from an explicit branch, for the same reason [`Self::put_on_branch`] exists.
    pub fn delete_on_branch(&self, key: &str, branch: &str) -> Result<()> {
        let Some(entry) = self.get(key)? else {
            // Already gone, which is what the caller wanted.
            return Ok(());
        };
        let sha = entry.sha.ok_or_else(|| {
            VeloctError::Other(format!("GitHub's answer for {key} carried no sha"))
        })?;

        let request = self
            .request(HttpMethod::Delete, self.contents_url(key))
            .json(serde_json::json!({
                "message": format!("Remove {key}"),
                "sha": sha,
                "branch": branch,
            }));
        let response = self.client.execute(request)?;
        if !response.is_success() && response.status != 404 {
            return Err(self.refused("delete a file", response.status));
        }
        Ok(())
    }

    /// Whether the repository is readable, and whether it is private.
    ///
    /// §17.2 requires a PRIVATE repository for a backup target. The requirement has to be checkable
    /// rather than merely asserted by the user, and this is the call that makes it a fact.
    pub fn repository_info(&self) -> Result<RepositoryInfo> {
        let response = self
            .client
            .execute(self.request(HttpMethod::Get, self.repository_url()))?;
        if !response.is_success() {
            return Err(self.refused("read the repository", response.status));
        }
        let value = response.json::<serde_json::Value>().map_err(|error| {
            VeloctError::Other(format!("GitHub's repository answer was not JSON: {error}"))
        })?;
        Ok(RepositoryInfo {
            // An answer that omits `private` is treated as NOT private. The default has to be the safe
            // one: §17.2 exists because the archive is unencrypted, so assuming privacy would be the
            // exact mistake the clause is about.
            private: value
                .get("private")
                .and_then(|flag| flag.as_bool())
                .unwrap_or(false),
            default_branch: value
                .get("default_branch")
                .and_then(|branch| branch.as_str())
                .map(str::to_string),
        })
    }
}

/// Read a [`ContentsEntry`] out of an API value, defaulting the fields the caller may not need.
fn entry_from_value(value: &serde_json::Value, fallback_path: &str) -> ContentsEntry {
    let path = value
        .get("path")
        .and_then(|path| path.as_str())
        .unwrap_or(fallback_path)
        .to_string();
    let name = value
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.rsplit('/')
                .next()
                .unwrap_or(path.as_str())
                .to_string()
        });
    ContentsEntry {
        path,
        name,
        size_bytes: value
            .get("size")
            .and_then(|size| size.as_u64())
            .unwrap_or(0),
        sha: value
            .get("sha")
            .and_then(|sha| sha.as_str())
            .map(str::to_string),
    }
}

/// base64, for the Contents API.
///
/// Hand-written rather than a dependency: the Contents API needs standard base64 with padding, which is
/// twenty lines, and a crate for it would be a crate to audit.
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

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::{FakeHttpClient, HttpBody};

    fn client() -> Arc<FakeHttpClient> {
        Arc::new(FakeHttpClient::new())
    }

    fn contents(client: Arc<FakeHttpClient>, branch: &str) -> GitHubContents {
        GitHubContents::new(
            Repository::parse("vellow/private-backups").unwrap(),
            "ghp_example_token",
            branch,
            client as Arc<dyn HttpClient>,
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
        // `owner/name/tree/main/x` is a different thing from the repository, and silently taking the
        // first two segments would upload somewhere the user did not name.
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
    fn the_put_sends_the_expected_request_and_reads_the_status_as_create_or_update() {
        let created_client = Arc::new(FakeHttpClient::new().always(201, "{}"));
        let created = contents(Arc::clone(&created_client), "main");
        assert_eq!(created.put("a.bin", b"PNG").unwrap(), WriteOutcome::Created);
        let requests = created_client.recorded();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, HttpMethod::Put);
        assert!(
            request.url.contains("/repos/vellow/private-backups/contents/a.bin"),
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
            HttpBody::Json(value) => value.clone(),
            other => panic!("expected a JSON body, got {other:?}"),
        };
        assert_eq!(body["content"], "UE5H", "PNG in base64");
        assert_eq!(body["branch"], "main");

        // The Contents API answers 200 for an update, and section 13.4's reuse report depends on telling
        // the two apart.
        let updated = contents(Arc::new(FakeHttpClient::new().always(200, "{}")), "main");
        assert_eq!(updated.put("a.bin", b"PNG").unwrap(), WriteOutcome::Updated);
    }

    #[test]
    fn an_empty_branch_falls_back_rather_than_producing_a_request_about_a_missing_ref() {
        let client = client();
        let api = contents(Arc::clone(&client), "");
        assert_eq!(api.branch(), "main");
    }

    #[test]
    fn a_missing_object_is_absent_rather_than_an_error() {
        let api = contents(Arc::new(FakeHttpClient::new().always(404, "{}")), "main");
        assert!(api.get("gone.bin").unwrap().is_none());

        let api = contents(
            Arc::new(FakeHttpClient::new().always(200, r#"{"size":42,"sha":"deadbeef"}"#)),
            "main",
        );
        let found = api.get("there.bin").unwrap().expect("present");
        assert_eq!(found.size_bytes, 42);
        assert_eq!(found.sha.as_deref(), Some("deadbeef"));
        // The API's answer need not repeat the path, so the key is the fallback.
        assert_eq!(found.path, "there.bin");
    }

    #[test]
    fn deleting_fetches_the_sha_first_because_the_api_requires_it() {
        let client = Arc::new(
            FakeHttpClient::new()
                .on_method_and_url(HttpMethod::Get, "/contents/", 200, r#"{"sha":"deadbeef"}"#)
                .always(200, "{}"),
        );
        let api = contents(Arc::clone(&client), "main");
        api.delete("a.bin").unwrap();

        let requests = client.recorded();
        assert_eq!(requests.len(), 2, "a GET for the sha, then a DELETE");
        assert_eq!(requests[0].method, HttpMethod::Get);
        assert_eq!(requests[1].method, HttpMethod::Delete);
        let body: serde_json::Value = match &requests[1].body {
            HttpBody::Json(value) => value.clone(),
            other => panic!("expected a JSON body, got {other:?}"),
        };
        assert_eq!(body["sha"], "deadbeef");
    }

    #[test]
    fn deleting_something_already_gone_is_not_an_error() {
        let api = contents(Arc::new(FakeHttpClient::new().always(404, "{}")), "main");
        assert!(api.delete("gone.bin").is_ok());
    }

    #[test]
    fn a_delete_without_a_sha_is_refused_rather_than_guessed() {
        // An answer with no sha cannot be turned into the documented two-step, and inventing one would
        // send a delete the API would reject for a reason nobody could read.
        let api = contents(Arc::new(FakeHttpClient::new().always(200, "{}")), "main");
        let error = api.delete("a.bin").unwrap_err();
        assert!(error.to_string().contains("no sha"), "{error}");
    }

    #[test]
    fn a_refused_call_reports_the_status_and_never_the_body() {
        // A GitHub error body can echo the token back, and section 18.1 keeps credentials out of messages.
        //
        // Each call gets its OWN client: `FakeHttpClient` consumes a queued response when it matches, so a
        // shared one would answer the second call with 404 and the test would pass for the wrong reason.
        let refusing = || {
            contents(
                Arc::new(FakeHttpClient::new().always(
                    401,
                    r#"{"message":"Bad credentials","token":"ghp_example_token"}"#,
                )),
                "main",
            )
        };
        for error in [
            refusing().put("a.bin", b"x").unwrap_err(),
            refusing().get("a.bin").unwrap_err(),
            refusing().list("backups").unwrap_err(),
            refusing().repository_info().unwrap_err(),
        ] {
            let text = error.to_string();
            assert!(text.contains("401"), "{text}");
            assert!(
                !text.contains("ghp_example_token"),
                "the token must not reach the message: {text}"
            );
        }
    }

    #[test]
    fn the_root_listing_has_no_path_segment_and_a_subdirectory_does() {
        let api = contents(client(), "main");
        assert_eq!(
            api.directory_url(""),
            "https://api.github.com/repos/vellow/private-backups/contents"
        );
        assert_eq!(
            api.directory_url("veloct-backups"),
            "https://api.github.com/repos/vellow/private-backups/contents/veloct-backups"
        );
        // A user who wrote a leading slash is not asking for a different directory.
        assert_eq!(
            api.directory_url("/veloct-backups/"),
            "https://api.github.com/repos/vellow/private-backups/contents/veloct-backups"
        );
    }

    #[test]
    fn a_directory_the_api_does_not_know_is_an_empty_listing() {
        // A fresh repository has no backup directory until something is written into it, and reporting
        // that as a failure would make the normal first run look broken.
        let api = contents(Arc::new(FakeHttpClient::new().always(404, "{}")), "main");
        assert!(api.list("backups").unwrap().is_empty());
    }

    #[test]
    fn a_listing_keeps_files_and_drops_subdirectories() {
        let api = contents(
            Arc::new(FakeHttpClient::new().on_json(
                "/contents/backups",
                serde_json::json!([
                    { "type": "file", "name": "a.tar.gz", "path": "backups/a.tar.gz", "size": 10, "sha": "s1" },
                    { "type": "dir", "name": "nested", "path": "backups/nested" },
                    { "type": "file", "name": "b.tar.gz", "path": "backups/b.tar.gz", "size": 20, "sha": "s2" }
                ]),
            )),
            "main",
        );

        let entries = api.list("backups").unwrap();
        assert_eq!(entries.len(), 2, "the sub-directory is not a file");
        assert_eq!(entries[0].name, "a.tar.gz");
        assert_eq!(entries[0].size_bytes, 10);
        assert_eq!(entries[0].sha.as_deref(), Some("s1"));
        assert_eq!(entries[1].path, "backups/b.tar.gz");
    }

    #[test]
    fn a_directory_listing_that_is_an_object_is_reported_rather_than_iterated() {
        // An object here means a FILE was asked for as though it were a directory. Iterating its keys
        // would produce nonsense entries.
        let api = contents(
            Arc::new(FakeHttpClient::new().always(200, r#"{"name":"a.bin","size":1}"#)),
            "main",
        );
        let error = api.list("a.bin").unwrap_err();
        assert!(error.to_string().contains("object"), "{error}");
    }

    #[test]
    fn privacy_is_read_from_the_repository_and_an_answer_that_omits_it_is_not_private() {
        // Section 17.2 exists because the archive is unencrypted, so a missing flag must not be read as
        // "private": the safe default is the only acceptable one.
        let api = contents(
            Arc::new(FakeHttpClient::new().on_json(
                "/repos/vellow/private-backups",
                serde_json::json!({ "private": true, "default_branch": "main" }),
            )),
            "main",
        );
        let info = api.repository_info().unwrap();
        assert!(info.private);
        assert_eq!(info.default_branch.as_deref(), Some("main"));

        let api = contents(
            Arc::new(FakeHttpClient::new().on_json(
                "/repos/vellow/private-backups",
                serde_json::json!({ "default_branch": "main" }),
            )),
            "main",
        );
        assert!(
            !api.repository_info().unwrap().private,
            "an answer with no `private` flag is not a private repository"
        );

        let api = contents(
            Arc::new(FakeHttpClient::new().on_json(
                "/repos/vellow/private-backups",
                serde_json::json!({ "private": false }),
            )),
            "main",
        );
        assert!(!api.repository_info().unwrap().private);
    }

    #[test]
    fn the_contents_api_ceiling_is_the_documented_one() {
        // Stated as a fact because the failure it prevents is an out-of-memory crash rather than an HTTP
        // error: the body is the object base64-encoded inside JSON.
        assert_eq!(CONTENTS_API_MAX_BYTES, 100 * 1024 * 1024);
    }
}
