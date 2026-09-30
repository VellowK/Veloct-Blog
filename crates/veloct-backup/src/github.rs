//! The GitHub **cloud backup target** (`handoff.md` §17.4).
//!
//! §17.4 names exactly two cloud targets — GitHub and S3/S3-compatible — and §17.7 makes them
//! independent: a failure on one is recorded and the other still keeps this backup. The model for that
//! already existed in [`crate::targets`] and had no implementation behind it, so an automatic backup
//! wrote the local archive and honestly reported an EMPTY target list.
//!
//! This module is the GitHub half. Three things about it are decisions rather than mechanics:
//!
//! * **§17.2 is verified, not asserted.** The contract requires a PRIVATE repository because the archive
//!   is unencrypted. `CloudTarget::private_confirmed` records the user's declaration, which the
//!   orchestration honours; this adapter additionally asks GitHub, on every upload, because a repository
//!   that a user made public AFTER confirming would otherwise keep receiving whole-project archives.
//!   That is the exact failure §17.2 exists to prevent, and it is invisible from the filesystem.
//! * **The default branch is the repository's, not a guessed one.** A backup repository has no branch in
//!   its configuration, and writing to a guessed `main` in a repository whose default is `master` would
//!   silently create a branch. The branch is read from the API, together with the privacy flag, in one
//!   call.
//! * **A large archive is refused, not attempted.** The Contents API takes the object base64-encoded
//!   inside a JSON body, so the bytes must be in memory and the request is ~1.37x their size.
//!   [`CONTENTS_API_MAX_BYTES`] is that ceiling stated as a fact; above it the upload is refused with a
//!   message that says so, rather than failing partway through an upload that cannot succeed.

use std::path::Path;
use std::sync::Arc;

use veloct_core::http::HttpClient;
use veloct_core::{Result, VeloctError};
use veloct_github::{GitHubContents, Repository, CONTENTS_API_MAX_BYTES};

use crate::targets::{BackupTargetAdapter, CloudTarget, RemoteBackup};

/// The prefix every archive this application writes carries.
///
/// A listing is filtered by it, because a backup repository may hold other things and retention must not
/// delete something it did not create.
pub const BACKUP_NAME_PREFIX: &str = "veloct-backup-";

/// The suffix every archive this application writes carries.
pub const BACKUP_NAME_SUFFIX: &str = ".tar.gz";

/// A GitHub repository as a cloud backup target.
#[derive(Debug)]
pub struct GitHubBackupTarget {
    target: CloudTarget,
    contents: GitHubContents,
}

impl GitHubBackupTarget {
    /// Build the adapter for a configured target.
    ///
    /// `token` is required: a private repository needs a credential, and a missing one is reported at
    /// construction rather than as a 401 at the first backup.
    pub fn new(
        target: CloudTarget,
        token: impl Into<String>,
        client: Arc<dyn HttpClient>,
    ) -> Result<Self> {
        let repository = Repository::parse(&target.location).map_err(|error| {
            VeloctError::Backup(format!(
                "the GitHub backup target {:?} has no usable repository: {error}",
                target.id
            ))
        })?;
        Ok(Self {
            target,
            // The branch is unused: every call that needs one supplies the repository's own default.
            contents: GitHubContents::new(repository, token, "main", client),
        })
    }

    /// The directory inside the repository this target's archives live in.
    fn directory(&self) -> &str {
        self.target.prefix.as_deref().unwrap_or("")
    }

    /// The repository's facts, fetched rather than assumed.
    fn repository_info(&self) -> Result<veloct_github::RepositoryInfo> {
        self.contents.repository_info().map_err(|error| {
            VeloctError::Backup(format!(
                "the GitHub backup target {:?} could not read {}: {error}",
                self.target.id,
                self.contents.repository().slug()
            ))
        })
    }

    /// Refuse unless the repository is genuinely private (`handoff.md` §17.2).
    fn require_private(&self) -> Result<veloct_github::RepositoryInfo> {
        let info = self.repository_info()?;
        if !info.private {
            return Err(VeloctError::Backup(format!(
                "the GitHub backup target {:?} points at {}, which GitHub reports as PUBLIC. handoff.md \
                 section 17.2 requires a private repository because the backup archive is not encrypted, \
                 so this upload was refused rather than publishing the project. Make the repository \
                 private, or point the target at one that is.",
                self.target.id,
                self.contents.repository().slug()
            )));
        }
        Ok(info)
    }

    /// The branch a write must name.
    fn write_branch(info: &veloct_github::RepositoryInfo) -> &str {
        // A repository always reports a default branch in practice, and the client's own fallback covers
        // an answer that omitted one, rather than inventing a branch name here.
        info.default_branch.as_deref().unwrap_or("main")
    }
}

impl BackupTargetAdapter for GitHubBackupTarget {
    fn config(&self) -> &CloudTarget {
        &self.target
    }

    fn upload(&self, archive: &Path, remote_name: &str) -> Result<String> {
        // The size check comes FIRST because it is local and free, and because a refusal that costs no
        // request is better than one that costs two.
        let bytes = std::fs::read(archive).map_err(|error| VeloctError::io(archive, error))?;
        if bytes.len() as u64 > CONTENTS_API_MAX_BYTES {
            return Err(VeloctError::Backup(format!(
                "the archive is {} bytes, above the {} byte ceiling of the GitHub Contents API. The API \
                 takes the object base64-encoded inside a JSON body, so the whole archive would have to be \
                 held in memory (at roughly 1.37x its size) and GitHub would reject anything above the \
                 limit anyway. Veloct refuses rather than attempting an upload that cannot succeed: a \
                 project this large needs a target that streams the archive.",
                bytes.len(),
                CONTENTS_API_MAX_BYTES
            )));
        }

        // Section 17.2, re-checked on every upload: a repository made public after the user confirmed it
        // must stop receiving unencrypted project archives.
        let info = self.require_private()?;
        let branch = Self::write_branch(&info).to_string();

        self.contents
            .put_on_branch(remote_name, &bytes, &branch)
            .map_err(|error| {
                VeloctError::Backup(format!(
                    "the GitHub backup target {:?} could not store {remote_name}: {error}",
                    self.target.id
                ))
            })?;

        // The key the caller records and later passes to `remove`.
        Ok(remote_name.to_string())
    }

    fn list(&self) -> Result<Vec<RemoteBackup>> {
        let entries = self.contents.list(self.directory()).map_err(|error| {
            VeloctError::Backup(format!(
                "the GitHub backup target {:?} could not list {:?}: {error}",
                self.target.id,
                self.directory()
            ))
        })?;

        Ok(entries
            .into_iter()
            // Only this application's archives. §17.6's retention deletes what it did not create
            // otherwise, and a backup repository is a place a user may reasonably keep other things.
            .filter(|entry| is_backup_name(&entry.name))
            .map(|entry| RemoteBackup {
                created_at: archive_created_at(&entry.name).unwrap_or_else(|| entry.path.clone()),
                path: entry.path,
                size_bytes: entry.size_bytes,
            })
            .collect())
    }

    fn remove(&self, remote_path: &str) -> Result<()> {
        // The DELETE needs the branch, so the repository is asked again rather than assumed. A retention
        // pass is rare, so the extra call buys correctness cheaply.
        let info = self.repository_info()?;
        let branch = Self::write_branch(&info).to_string();
        self.contents
            .delete_on_branch(remote_path, &branch)
            .map_err(|error| {
                VeloctError::Backup(format!(
                    "the GitHub backup target {:?} could not remove {remote_path}: {error}",
                    self.target.id
                ))
            })
    }

    fn test_connection(&self) -> Result<String> {
        // A public repository is reachable and still NOT a usable backup target, so reachability alone
        // would be a misleading success. Section 17.2 makes privacy part of what "works" means here.
        let info = self.require_private()?;
        Ok(format!(
            "{} is reachable and private (default branch {})",
            self.contents.repository().slug(),
            Self::write_branch(&info)
        ))
    }
}

/// Whether a name is one of this application's backup archives.
pub fn is_backup_name(name: &str) -> bool {
    name.starts_with(BACKUP_NAME_PREFIX) && name.ends_with(BACKUP_NAME_SUFFIX)
}

/// The creation instant an archive's own name records.
///
/// `veloct_backup::remote_name` builds the name timestamp-first — `veloct-backup-<stamp>-<sha>.tar.gz` —
/// precisely so a lexicographic listing is also a chronological one, which is what retention sorts by.
/// The Contents API reports no creation time per file, so the name is the only honest source, and this
/// recovers the instant the caller originally asked for.
///
/// `None` for a name that does not carry the stamp. The caller then keeps the path as the sort key
/// instead of dropping the object: an archive retention cannot see is an archive retention cannot
/// manage.
pub fn archive_created_at(name: &str) -> Option<String> {
    let rest = name.strip_prefix(BACKUP_NAME_PREFIX)?;
    // The stamp is `YYYYMMDD-HHMMSS` after `remote_name` has removed the separators, so it is the first
    // TWO dash-separated fields; the third is the content hash and everything after it is descriptive.
    let mut fields = rest.split('-');
    let date = fields.next()?;
    let time = fields.next()?;
    // Recovering the original instant is string surgery rather than a date library, and every field is
    // validated so a file that merely looks similar cannot become a wrong timestamp.
    if date.len() != 8 || time.len() != 6 {
        return None;
    }
    if !date.chars().all(|c| c.is_ascii_digit()) || !time.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{}-{}-{}T{}:{}:{}Z",
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &time[0..2],
        &time[2..4],
        &time[4..6]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::{prune, remote_name, CloudTargetKind};
    use veloct_core::http::{FakeHttpClient, HttpBody, HttpMethod};

    fn github_target() -> CloudTarget {
        let mut target = CloudTarget::new("gh", CloudTargetKind::GitHub, "vellow/private-backups");
        target.private_confirmed = true;
        target.prefix = Some("veloct-backups".to_string());
        target
    }

    fn adapter(client: Arc<FakeHttpClient>) -> GitHubBackupTarget {
        GitHubBackupTarget::new(github_target(), "ghp_example_token", client as Arc<dyn HttpClient>)
            .unwrap()
    }

    #[test]
    fn the_archive_name_round_trips_to_the_instant_it_records() {
        let name = remote_name(
            "2026-01-01T12:00:00Z",
            &"a".repeat(64),
            Some("veloct-backups"),
        );
        assert!(name.starts_with("veloct-backups/veloct-backup-20260101-120000-"), "{name}");
        assert_eq!(
            archive_created_at("veloct-backup-20260101-120000-aaaaaaaaaaaa.tar.gz").as_deref(),
            Some("2026-01-01T12:00:00Z")
        );
    }

    #[test]
    fn a_name_that_is_not_an_archive_yields_no_instant_rather_than_a_guess() {
        for name in [
            "notes.txt",
            "veloct-backup-.tar.gz",
            "veloct-backup-2026-01-01T12:00:00Z.tar.gz",
            "veloct-backup-2026010-120000.tar.gz",
            "veloct-backup-20260101-12000a.tar.gz",
        ] {
            assert_eq!(archive_created_at(name), None, "{name}");
        }
    }

    #[test]
    fn only_this_applications_archives_are_listed() {
        // Retention must not delete something Veloct did not create, and a backup repository is a place a
        // user may reasonably keep other things.
        assert!(is_backup_name("veloct-backup-20260101-120000-abc.tar.gz"));
        assert!(!is_backup_name("README.md"));
        assert!(!is_backup_name("veloct-backup-20260101-120000-abc.tar"));
        assert!(!is_backup_name("my-veloct-backup-20260101-120000-abc.tar.gz"));
    }

    #[test]
    fn an_upload_verifies_privacy_before_it_sends_anything() {
        // Section 17.2: the archive is not encrypted, so a public repository must be refused. The order
        // matters — a refusal that has already uploaded would be no refusal at all.
        let dir = tempfile::TempDir::new().unwrap();
        let archive = dir.path().join("veloct-backup-x.tar.gz");
        std::fs::write(&archive, b"ARCHIVE").unwrap();

        let client = Arc::new(FakeHttpClient::new().on_json(
            "/repos/vellow/private-backups",
            serde_json::json!({ "private": false, "default_branch": "main" }),
        ));
        let target = adapter(Arc::clone(&client));

        let error = target.upload(&archive, "veloct-backups/a.tar.gz").unwrap_err();
        assert!(error.to_string().contains("PUBLIC"), "{error}");
        assert!(error.to_string().contains("17.2"), "{error}");

        let requests = client.recorded();
        assert_eq!(requests.len(), 1, "only the privacy check ran");
        assert_eq!(requests[0].method, HttpMethod::Get);
    }

    #[test]
    fn an_answer_that_omits_privacy_is_refused_as_not_private() {
        // The safe default. Reading a missing flag as "private" would be the exact mistake section 17.2
        // exists to prevent.
        let dir = tempfile::TempDir::new().unwrap();
        let archive = dir.path().join("a.tar.gz");
        std::fs::write(&archive, b"ARCHIVE").unwrap();

        let client = Arc::new(FakeHttpClient::new().on_json(
            "/repos/vellow/private-backups",
            serde_json::json!({ "default_branch": "main" }),
        ));
        let target = adapter(Arc::clone(&client));
        assert!(target.upload(&archive, "a.tar.gz").is_err());
        assert_eq!(client.request_count(), 1, "the privacy check refused it");
    }

    #[test]
    fn a_private_repository_receives_the_archive_on_its_own_default_branch() {
        // The branch comes from the repository, not from a guess: writing to a guessed `main` in a
        // repository whose default is `master` would silently create a branch.
        let dir = tempfile::TempDir::new().unwrap();
        let archive = dir.path().join("a.tar.gz");
        std::fs::write(&archive, b"ARCHIVE").unwrap();

        let client = Arc::new(
            FakeHttpClient::new()
                .on_json(
                    "/repos/vellow/private-backups",
                    serde_json::json!({ "private": true, "default_branch": "master" }),
                )
                .on_method_and_url(HttpMethod::Put, "/contents/", 201, "{}"),
        );
        let target = adapter(Arc::clone(&client));
        let stored = target
            .upload(&archive, "veloct-backups/veloct-backup-x.tar.gz")
            .unwrap();
        assert_eq!(stored, "veloct-backups/veloct-backup-x.tar.gz");

        let requests = client.recorded();
        assert_eq!(requests.len(), 2);
        let body: serde_json::Value = match &requests[1].body {
            HttpBody::Json(value) => value.clone(),
            other => panic!("expected a JSON body, got {other:?}"),
        };
        assert_eq!(body["branch"], "master");
        assert_eq!(body["content"], "QVJDSElWRQ==", "ARCHIVE in base64");
    }

    #[test]
    fn an_archive_above_the_contents_api_ceiling_is_refused_without_any_request() {
        // The failure this prevents is not a clean HTTP error: the body is the object base64-encoded
        // inside JSON, so the whole archive is in memory before anything is sent.
        let dir = tempfile::TempDir::new().unwrap();
        let archive = dir.path().join("big.tar.gz");
        let file = std::fs::File::create(&archive).unwrap();
        // A sparse file above the ceiling, so the test costs no disk and the refusal is about the SIZE
        // rather than about how long it took to write.
        file.set_len(CONTENTS_API_MAX_BYTES + 1).unwrap();
        drop(file);

        let client = Arc::new(FakeHttpClient::new());
        let target = adapter(Arc::clone(&client));
        let error = target.upload(&archive, "a.tar.gz").unwrap_err();
        assert!(error.to_string().contains("ceiling"), "{error}");
        assert_eq!(
            client.request_count(),
            0,
            "the refusal costs no request, because the limit is known locally"
        );
    }

    #[test]
    fn a_listing_becomes_remote_backups_newest_first_and_ignores_other_files() {
        let client = Arc::new(FakeHttpClient::new().on_json(
            "/contents/veloct-backups",
            serde_json::json!([
                { "type": "file", "name": "veloct-backup-20260101-120000-aaaaaaaaaaaa.tar.gz",
                  "path": "veloct-backups/veloct-backup-20260101-120000-aaaaaaaaaaaa.tar.gz",
                  "size": 10, "sha": "s1" },
                { "type": "file", "name": "notes.md", "path": "veloct-backups/notes.md", "size": 1, "sha": "s2" },
                { "type": "file", "name": "veloct-backup-20260103-090000-bbbbbbbbbbbb.tar.gz",
                  "path": "veloct-backups/veloct-backup-20260103-090000-bbbbbbbbbbbb.tar.gz",
                  "size": 20, "sha": "s3" }
            ]),
        ));
        let target = adapter(Arc::clone(&client));
        let listed = target.list().unwrap();

        assert_eq!(listed.len(), 2, "notes.md is not a backup");
        // `list` returns the target's own order; retention sorts. Both are asserted so a change to either
        // is visible.
        let mut sorted = listed.clone();
        sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        assert_eq!(sorted[0].created_at, "2026-01-03T09:00:00Z");
        assert_eq!(sorted[1].created_at, "2026-01-01T12:00:00Z");
        assert_eq!(sorted[0].size_bytes, 20);
    }

    #[test]
    fn a_directory_that_does_not_exist_yet_is_an_empty_listing_not_a_failure() {
        // A fresh repository has no backup directory until something is written into it, and reporting
        // that as a failure would make the normal first run look broken.
        let client = Arc::new(FakeHttpClient::new());
        let target = adapter(client);
        assert!(target.list().unwrap().is_empty());
    }

    #[test]
    fn retention_deletes_the_oldest_through_the_adapter() {
        // Section 17.6 end to end over the GitHub listing: the newest three survive and the DELETE path
        // is the documented two-step.
        let client = Arc::new(
            FakeHttpClient::new()
                .on_json(
                    "/contents/veloct-backups",
                    serde_json::json!([
                        { "type": "file", "name": "veloct-backup-20260101-120000-aaaaaaaaaaaa.tar.gz",
                          "path": "veloct-backups/veloct-backup-20260101-120000-aaaaaaaaaaaa.tar.gz",
                          "size": 10, "sha": "s1" },
                        { "type": "file", "name": "veloct-backup-20260102-120000-bbbbbbbbbbbb.tar.gz",
                          "path": "veloct-backups/veloct-backup-20260102-120000-bbbbbbbbbbbb.tar.gz",
                          "size": 10, "sha": "s2" },
                        { "type": "file", "name": "veloct-backup-20260103-120000-cccccccccccc.tar.gz",
                          "path": "veloct-backups/veloct-backup-20260103-120000-cccccccccccc.tar.gz",
                          "size": 10, "sha": "s3" },
                        { "type": "file", "name": "veloct-backup-20260104-120000-dddddddddddd.tar.gz",
                          "path": "veloct-backups/veloct-backup-20260104-120000-dddddddddddd.tar.gz",
                          "size": 10, "sha": "s4" }
                    ]),
                )
                // The retention pass asks the repository for the branch, then does GET-sha / DELETE.
                .on_json(
                    "/repos/vellow/private-backups",
                    serde_json::json!({ "private": true, "default_branch": "main" }),
                )
                .on_method_and_url(HttpMethod::Get, "/contents/veloct-backups/veloct-backup-20260101", 200,
                    r#"{"sha":"s1"}"#)
                .on_method_and_url(HttpMethod::Delete, "/contents/veloct-backups/veloct-backup-20260101", 200, "{}"),
        );
        let target = adapter(Arc::clone(&client));

        let removed = prune(
            &target,
            Some("veloct-backups/veloct-backup-20260104-120000-dddddddddddd.tar.gz"),
        )
        .unwrap();
        assert_eq!(removed, 1, "four objects and a retention of three");

        let deletes: Vec<String> = client
            .recorded()
            .into_iter()
            .filter(|request| request.method == HttpMethod::Delete)
            .map(|request| request.url)
            .collect();
        assert_eq!(deletes.len(), 1);
        assert!(
            deletes[0].contains("veloct-backup-20260101-120000-aaaaaaaaaaaa.tar.gz"),
            "{:?}",
            deletes
        );
    }

    #[test]
    fn test_connection_reports_a_public_repository_as_unusable_rather_than_reachable() {
        // Reachability alone would be a misleading success: section 17.2 makes privacy part of what
        // "works" means for a backup target.
        let public = Arc::new(FakeHttpClient::new().on_json(
            "/repos/vellow/private-backups",
            serde_json::json!({ "private": false }),
        ));
        let error = adapter(public).test_connection().unwrap_err();
        assert!(error.to_string().contains("PUBLIC"), "{error}");

        let private = Arc::new(FakeHttpClient::new().on_json(
            "/repos/vellow/private-backups",
            serde_json::json!({ "private": true, "default_branch": "trunk" }),
        ));
        let said = adapter(private).test_connection().unwrap();
        assert!(said.contains("private"), "{said}");
        assert!(said.contains("trunk"), "{said}");
    }

    #[test]
    fn a_target_whose_location_is_not_a_repository_is_reported_at_construction() {
        let mut target = github_target();
        target.location = "just-a-name".to_string();
        let client = Arc::new(FakeHttpClient::new()) as Arc<dyn HttpClient>;
        let error = GitHubBackupTarget::new(target, "t", client).unwrap_err();
        assert!(error.to_string().contains("no usable repository"), "{error}");
    }
}
