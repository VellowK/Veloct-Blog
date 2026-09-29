//! The publish sequence.
//!
//! `handoff.md` §14.4 fixes the order and §19.3 restates it:
//!
//! ```text
//! the user presses publish -> check what actually changed -> commit ONLY if something changed
//!   -> the Astro build -> the single dist -> tag the commit
//! ```
//!
//! Until this module existed, that sequence lived only inside an integration TEST. The stages were all
//! implemented and each had its own tests, and the ordering that makes them a release was asserted rather
//! than executable — which means the application could not perform the thing §28 asks to be verifiable.
//!
//! Three ordering decisions are load-bearing:
//!
//! * **Freeze before the build.** §14.4 step 1 freezes the artefacts before any commit exists, which is
//!   why `release.json` can be written at all: the manifest deliberately cannot contain a commit sha, so
//!   freezing first is safe and freezing after would be a chicken and egg.
//! * **Commit only when something changed.** §19.3 is explicit, and an empty commit on every publish would
//!   make the release history a list of nothing.
//! * **Tag after the build.** A tag must point at a commit that contains the build's inputs, so tagging
//!   before the build would tag a tree that does not describe what was published.
//!
//! Where the commit sha lives: NOT in `release.json`. `ReleaseManifest` is the immutable description of
//! what was built, and §14.4 is what makes it safe to freeze before the commit exists. The sha and the tag
//! go into a separate, smaller record, because a release that cannot say which commit produced it cannot
//! be rolled back to.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::ids::ReleaseId;
use veloct_core::{Clock, Result, VeloctError};

use crate::manifest::{BuildInputs, DeploymentTargetSnapshot, ToolchainSnapshot};
use crate::pipeline::{
    build_once, freeze, BuildOutcome, BuildRunner, FreezeRequest, ReleaseArtifacts,
};

/// What to publish.
#[derive(Debug, Clone)]
pub struct PublishRequest {
    pub project_root: PathBuf,
    /// The Astro template to build and freeze.
    pub site_dir: PathBuf,
    pub release_id: ReleaseId,
    pub build_inputs: BuildInputs,
    pub target_snapshot: DeploymentTargetSnapshot,
    pub toolchain: ToolchainSnapshot,
    pub site_template_version: String,
    /// The frozen build time, so a rebuild cannot move time-derived output (§14.9).
    pub release_build_time: String,
    /// Set when this release came from a re-send or a rollback of another one.
    pub created_from_release_id: Option<ReleaseId>,
    pub commit_message: Option<String>,
}

/// The commit and tag a release is anchored by.
///
/// Written beside the release as `publish-record.json` rather than into `release.json`, which must stay
/// free of a commit sha so it can be frozen before one exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishRecord {
    pub release_id: String,
    /// The commit the release was published from.
    pub commit_sha: String,
    /// `release/<release-id>`.
    pub tag_name: String,
    /// Whether this publish created a commit, or found nothing to commit and tagged the existing head.
    pub committed: bool,
    /// How many files the commit recorded.
    pub changed_files: usize,
    pub created_at: String,
    /// The release this one was produced from, for a re-send or a rollback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_from_release_id: Option<String>,
}

/// The outcome of a publish.
#[derive(Debug, Clone)]
pub struct PublishOutcome {
    /// The frozen artefacts, with the dist manifest recorded by `build_once`.
    pub artifacts: ReleaseArtifacts,
    pub build: BuildOutcome,
    /// The commit and tag, or `None` when the build succeeded but the repository could not be anchored.
    pub record: Option<PublishRecord>,
    /// Why the release could not be anchored, when that happened.
    ///
    /// A release whose files were written but whose commit failed is a REAL state, and reporting it as a
    /// plain error would hide that the artefacts are already on disk. The caller decides whether to retry
    /// the anchoring.
    pub anchor_error: Option<String>,
}

impl PublishOutcome {
    /// Whether the release is fully anchored.
    pub fn is_anchored(&self) -> bool {
        self.record.is_some()
    }

    /// The path of the frozen release directory.
    pub fn release_dir(&self) -> PathBuf {
        self.artifacts.paths.directory.clone()
    }
}

/// The path of a release's publish record.
pub fn record_path(artifacts: &ReleaseArtifacts) -> PathBuf {
    artifacts.paths.directory.join("publish-record.json")
}

/// Run the publish sequence up to and including the tag.
///
/// The build runner is injected so the sequence is runnable without a build, exactly as `AiService` takes
/// an injected client: the ORDERING is what this function is for, and the ordering is testable with a stub.
/// The repository is optional because a project may not be a Git repository yet, and in that case the
/// artefacts are still written and the failure to anchor is reported rather than thrown away.
pub fn publish(
    request: &PublishRequest,
    runner: &dyn BuildRunner,
    repository: Option<&git2::Repository>,
    clock: &dyn Clock,
) -> Result<PublishOutcome> {
    // Step 1: freeze, before any commit exists (§14.4).
    let mut artifacts = freeze(
        FreezeRequest {
            project_root: request.project_root.clone(),
            release_id: request.release_id.clone(),
            build_inputs: request.build_inputs.clone(),
            target_snapshot: request.target_snapshot.clone(),
            toolchain: request.toolchain.clone(),
            site_template_version: request.site_template_version.clone(),
            site_template_dir: request.site_dir.clone(),
            release_build_time: request.release_build_time.clone(),
            created_from_release_id: request.created_from_release_id.clone(),
        },
        clock,
    )?;

    // Step 2: ONE build producing both publish roots (§14.1), with the dist manifest recorded into the
    // release by `build_once` so a release cannot exist without one.
    let build = build_once(
        &request.project_root,
        &request.site_dir,
        &mut artifacts,
        runner,
    )?;

    // Steps 3 and 4: commit what changed, then tag that commit.
    let now = veloct_core::clock::format_rfc3339(clock.now());
    let Some(repository) = repository else {
        return Ok(PublishOutcome {
            artifacts,
            build,
            record: None,
            anchor_error: Some(
                "the project is not a Git repository, so the release could not be committed or tagged"
                    .to_string(),
            ),
        });
    };

    let anchored = commit_and_tag(repository, &request.release_id, request, &now);
    match anchored {
        Ok(record) => {
            // The record is written AFTER the tag resolves, so a release never claims a commit it does not
            // have.
            let path = record_path(&artifacts);
            let text = serde_json::to_vec_pretty(&record).map_err(|error| {
                VeloctError::Build(format!("could not serialise the publish record: {error}"))
            })?;
            veloct_core::workdir::write_atomic(&path, &text)?;
            Ok(PublishOutcome {
                artifacts,
                build,
                record: Some(record),
                anchor_error: None,
            })
        }
        Err(error) => Ok(PublishOutcome {
            artifacts,
            build,
            record: None,
            anchor_error: Some(error.to_string()),
        }),
    }
}

/// The repository's current head commit.
fn head_sha(repository: &git2::Repository) -> Result<String> {
    let head = repository
        .head()
        .map_err(|error| VeloctError::Git(format!("the repository has no head commit: {error}")))?;
    let commit = head
        .peel_to_commit()
        .map_err(|error| VeloctError::Git(format!("the head is not a commit: {error}")))?;
    Ok(commit.id().to_string())
}

/// Commit what changed and tag the commit.
fn commit_and_tag(
    repository: &git2::Repository,
    release_id: &ReleaseId,
    request: &PublishRequest,
    now: &str,
) -> Result<PublishRecord> {
    let outcome = veloct_git::commit_publish(
        repository,
        release_id,
        request.commit_message.as_deref(),
    )?;

    // `commit_publish` reports whether it created a commit or found nothing to commit, which is §19.3's
    // "commit only when something actually changed".
    let (sha, committed, changed_files) = match outcome.commit() {
        Some(commit) => (commit.sha.clone(), true, commit.changes.files.len()),
        // §19.3's "commit only when something actually changed": nothing changed, so the tag will point at
        // the existing head. The head is read from the repository because `NoChange` carries nothing.
        None => (head_sha(repository)?, false, 0),
    };

    let tag = veloct_git::create_release_tag(repository, release_id)?;

    // The tag must resolve to the commit this record names, or the record would claim an anchor that does
    // not exist.
    let resolved = veloct_git::commit_for_tag(repository, release_id)?;
    if resolved != tag.commit_sha {
        return Err(VeloctError::Git(format!(
            "the release tag {} resolved to {resolved} but was created at {}",
            tag.name, tag.commit_sha
        )));
    }

    // The commit sha the record stores is the TAG's, and the sha the commit outcome reported must agree
    // with it: a mismatch would mean the tag was created somewhere other than where the commit landed.
    debug_assert_eq!(sha, tag.commit_sha, "the tag must point at the committed sha");

    Ok(PublishRecord {
        release_id: release_id.as_str().to_string(),
        commit_sha: tag.commit_sha.clone(),
        tag_name: tag.name.clone(),
        committed,
        changed_files,
        created_at: now.to_string(),
        created_from_release_id: request
            .created_from_release_id
            .as_ref()
            .map(|id| id.as_str().to_string()),
    })
}

/// Read a release's publish record, when it has one.
///
/// A release published before this module existed has no record, and that is reported as `None` rather
/// than inferred from the tag: an inferred commit would be a guess presented as a fact.
pub fn read_record(project_root: &Path, release_id: &ReleaseId) -> Result<Option<PublishRecord>> {
    let path = crate::manifest::ReleasePaths::new(project_root, release_id)
        .directory
        .join("publish-record.json");
    let Some(text) = veloct_core::workdir::read_text_optional(&path)? else {
        return Ok(None);
    };
    let record: PublishRecord = serde_json::from_str(&text).map_err(|error| {
        VeloctError::Build(format!("{} is not a publish record: {error}", path.display()))
    })?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{BuildRequest, FailingBuildRunner};
    use std::sync::Mutex;
    use veloct_core::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::new(
            time::OffsetDateTime::parse(
                "2026-01-01T12:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        )
    }

    fn release_id(text: &str) -> ReleaseId {
        ReleaseId::parse(text).expect("a valid release id")
    }

    /// A build runner that writes both publish roots and records what it was asked to do.
    struct StubRunner {
        calls: Mutex<Vec<BuildRequest>>,
    }

    impl StubRunner {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<BuildRequest> {
            self.calls.lock().map(|calls| calls.clone()).unwrap_or_default()
        }
    }

    impl BuildRunner for StubRunner {
        fn build(&self, request: &BuildRequest) -> Result<BuildOutcome> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push(request.clone());
            }
            let site_root = request.out_dir.join("site");
            let works_root = request.out_dir.join("works");
            for (root, file, body) in [
                (&site_root, "index.html", "<h1>home</h1>"),
                (&site_root, "article/x/index.html", "<h1>article</h1>"),
                (&works_root, "addition/index.html", "<h1>addition</h1>"),
            ] {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, body).unwrap();
            }
            Ok(BuildOutcome {
                site_root,
                works_root,
                log: "stub build".to_string(),
                dist_manifest: None,
            })
        }
    }

    /// A project laid out enough to freeze and build into.
    fn project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".veloct/releases")).unwrap();
        std::fs::create_dir_all(dir.path().join("site")).unwrap();
        std::fs::write(dir.path().join("site/package.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join("content/articles")).unwrap();
        std::fs::write(dir.path().join("content/articles/a.md"), "# A\n").unwrap();
        dir
    }

    fn request(root: &Path, id: &str) -> PublishRequest {
        PublishRequest {
            project_root: root.to_path_buf(),
            site_dir: root.join("site"),
            release_id: release_id(id),
            build_inputs: BuildInputs::default(),
            target_snapshot: DeploymentTargetSnapshot::default(),
            toolchain: ToolchainSnapshot {
                node: "24".to_string(),
                npm: "11".to_string(),
                astro: "7".to_string(),
                veloct: veloct_core::VERSION.to_string(),
                os: "test".to_string(),
            },
            site_template_version: veloct_core::VERSION.to_string(),
            release_build_time: "2026-01-01T12:00:00Z".to_string(),
            created_from_release_id: None,
            commit_message: None,
        }
    }

    /// A repository with the project committed, so a publish has something to commit against.
    fn repository(root: &Path) -> git2::Repository {
        let repository = veloct_git::change::init(root).unwrap();
        commit_all(&repository, "initial");
        repository
    }

    fn commit_all(repository: &git2::Repository, message: &str) {
        let mut index = repository.index().unwrap();
        index
            .add_all(["."].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Veloct test", "test@example.invalid").unwrap();
        let parent = repository.head().ok().and_then(|head| head.peel_to_commit().ok());
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        repository
            .commit(Some("HEAD"), &signature, &signature, message, &tree, &parents)
            .unwrap();
    }

    #[test]
    fn a_publish_freezes_builds_commits_and_tags_in_that_order() {
        let dir = project();
        let repo = repository(dir.path());
        let runner = StubRunner::new();

        let outcome = publish(
            &request(dir.path(), "20260101T120000Z-abc001"),
            &runner,
            Some(&repo),
            &clock(),
        )
        .unwrap();

        // Frozen first: the manifest exists even though it cannot name a commit (§14.4).
        assert!(outcome.artifacts.paths.manifest().is_file());
        let manifest = std::fs::read_to_string(outcome.artifacts.paths.manifest()).unwrap();
        assert!(!manifest.contains("commit"), "{manifest}");

        // One build, producing both roots (§14.1), and the dist manifest recorded by the build.
        assert_eq!(runner.calls().len(), 1, "exactly one build");
        assert!(outcome.build.site_root.join("index.html").is_file());
        assert!(outcome.build.works_root.join("addition/index.html").is_file());
        assert!(
            outcome.artifacts.paths.dist_manifest().is_file(),
            "the build records the dist manifest into the release"
        );

        // Anchored: a commit and a tag that resolves to it.
        assert!(outcome.is_anchored());
        let record = outcome.record.unwrap();
        assert_eq!(record.release_id, "20260101T120000Z-abc001");
        assert!(record.committed, "the dist is new, so there was something to commit");
        assert!(record.changed_files > 0);
        assert_eq!(record.tag_name, "release/20260101T120000Z-abc001");
        assert_eq!(
            veloct_git::commit_for_tag(&repo, &release_id("20260101T120000Z-abc001")).unwrap(),
            record.commit_sha,
            "the tag resolves to the recorded commit"
        );
    }

    #[test]
    fn the_publish_record_is_written_beside_the_release_and_not_into_the_manifest() {
        // release.json must stay free of a commit sha so it can be frozen before one exists.
        let dir = project();
        let repo = repository(dir.path());
        let outcome = publish(
            &request(dir.path(), "20260101T120000Z-abc002"),
            &StubRunner::new(),
            Some(&repo),
            &clock(),
        )
        .unwrap();

        let path = record_path(&outcome.artifacts);
        assert!(path.is_file(), "{}", path.display());
        let manifest = std::fs::read_to_string(outcome.artifacts.paths.manifest()).unwrap();
        assert!(!manifest.contains("commit_sha"), "{manifest}");

        // And it reads back.
        let stored = read_record(dir.path(), &release_id("20260101T120000Z-abc002"))
            .unwrap()
            .expect("a record");
        assert_eq!(stored, outcome.record.unwrap());
    }

    #[test]
    fn a_publish_with_nothing_to_commit_tags_the_existing_head_rather_than_making_an_empty_commit() {
        // handoff.md section 19.3: commit only when something actually changed.
        //
        // This test's PREVIOUS version was wrong, and the failure was instructive rather than a nuisance:
        // it asserted that two publishes in a row produce only one commit. They do not, and should not.
        // `docs/spec/01-data-model.md` line 1732 makes `.veloct/releases/<id>/release.json` a TRUTH SOURCE,
        // so publishing a second release really does add a file that belongs in the repository, and
        // committing it is the correct behaviour rather than churn.
        //
        // So the no-change path is exercised where it genuinely occurs: against a clean repository.
        let dir = project();
        let repo = repository(dir.path());
        let id = release_id("20260101T120000Z-abc004");
        let head_before = head_sha(&repo).unwrap();

        let record = commit_and_tag(&repo, &id, &request(dir.path(), "20260101T120000Z-abc004"), "2026-01-01T12:00:00Z")
            .unwrap();

        assert!(!record.committed, "nothing changed, so no empty commit was made");
        assert_eq!(record.changed_files, 0);
        assert_eq!(
            record.commit_sha, head_before,
            "the tag points at the existing head"
        );
        assert_eq!(head_sha(&repo).unwrap(), head_before, "and the head did not move");
        assert!(veloct_git::tag_exists(&repo, &id));
        assert_eq!(
            veloct_git::commit_for_tag(&repo, &id).unwrap(),
            head_before,
            "the tag resolves to the head it was created at"
        );
    }

    #[test]
    fn publishing_a_second_release_commits_the_new_release_artefact_because_it_is_a_truth_source() {
        // docs/spec/01-data-model.md line 1732 lists `.veloct/releases/<id>/release.json` beside the Git tag
        // as a truth source, so a new release directory is a real change that belongs in the repository.
        let dir = project();
        let repo = repository(dir.path());
        let runner = StubRunner::new();

        let first = publish(
            &request(dir.path(), "20260101T120000Z-abc003"),
            &runner,
            Some(&repo),
            &clock(),
        )
        .unwrap();
        let first_sha = first.record.as_ref().unwrap().commit_sha.clone();
        assert!(first.record.as_ref().unwrap().committed);

        let second = publish(
            &request(dir.path(), "20260101T120000Z-abc00d"),
            &runner,
            Some(&repo),
            &clock(),
        )
        .unwrap();
        let record = second.record.unwrap();
        assert!(
            record.committed,
            "the new release.json is a truth source, so it is committed"
        );
        assert_ne!(record.commit_sha, first_sha, "which is a new commit");
        // The first release's tag still points where it did: a later publish does not move it.
        assert_eq!(
            veloct_git::commit_for_tag(&repo, &release_id("20260101T120000Z-abc003")).unwrap(),
            first_sha
        );
        assert_eq!(veloct_git::list_release_tags(&repo).unwrap().len(), 2);
    }

    #[test]
    fn a_project_that_is_not_a_repository_still_produces_the_artefacts_and_reports_why_it_is_unanchored() {
        // Reporting this as a plain error would hide that the artefacts are already on disk.
        let dir = project();
        let outcome = publish(
            &request(dir.path(), "20260101T120000Z-abc005"),
            &StubRunner::new(),
            None,
            &clock(),
        )
        .unwrap();

        assert!(outcome.artifacts.paths.manifest().is_file());
        assert!(outcome.build.site_root.join("index.html").is_file());
        assert!(!outcome.is_anchored());
        assert!(outcome
            .anchor_error
            .as_deref()
            .unwrap()
            .contains("not a Git repository"));
        // And no record was written, because there is no commit to name.
        assert!(read_record(dir.path(), &release_id("20260101T120000Z-abc005"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_failed_build_writes_no_tag() {
        // A tag on a failed build would claim a release that does not exist.
        let dir = project();
        let repo = repository(dir.path());
        let error = publish(
            &request(dir.path(), "20260101T120000Z-abc006"),
            &FailingBuildRunner {
                message: "the build failed".to_string(),
            },
            Some(&repo),
            &clock(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("build"), "{error}");
        assert!(
            veloct_git::list_release_tags(&repo).unwrap().is_empty(),
            "no tag may exist after a failed build"
        );
        assert!(!veloct_git::tag_exists(&repo, &release_id("20260101T120000Z-abc006")));
    }

    #[test]
    fn a_re_send_records_the_release_it_came_from() {
        // handoff.md section 14.13: a re-send is anchored to the release it re-sends.
        let dir = project();
        let repo = repository(dir.path());

        let mut resend = request(dir.path(), "20260101T130000Z-abc007");
        resend.created_from_release_id = Some(release_id("20260101T120000Z-abc003"));
        let outcome = publish(&resend, &StubRunner::new(), Some(&repo), &clock()).unwrap();

        let record = outcome.record.unwrap();
        assert_eq!(
            record.created_from_release_id.as_deref(),
            Some("20260101T120000Z-abc003")
        );
        // And the freeze carried it too, so the release itself knows.
        assert_eq!(
            outcome
                .artifacts
                .manifest
                .created_from_release_id
                .as_ref()
                .map(|id| id.as_str()),
            Some("20260101T120000Z-abc003")
        );
    }

    #[test]
    fn the_frozen_build_time_reaches_the_build() {
        // Section 14.9: time-derived output cannot move on a rebuild, so the time the build sees is the
        // frozen one rather than the clock.
        let dir = project();
        let repo = repository(dir.path());
        let runner = StubRunner::new();
        let outcome = publish(
            &request(dir.path(), "20260101T120000Z-abc008"),
            &runner,
            Some(&repo),
            &clock(),
        )
        .unwrap();

        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].release_build_time, "2026-01-01T12:00:00Z");
        // And the build is pointed at the project, not at the template's own directory.
        assert_eq!(calls[0].project_root, dir.path());
        assert!(outcome.build.dist_manifest.is_some());
    }

    #[test]
    fn a_release_with_no_record_reports_none_rather_than_inferring_one_from_the_tag() {
        // An inferred commit would be a guess presented as a fact.
        let dir = project();
        let path = crate::manifest::ReleasePaths::new(dir.path(), &release_id("20260101T120000Z-abc009")).directory;
        std::fs::create_dir_all(&path).unwrap();
        assert!(read_record(dir.path(), &release_id("20260101T120000Z-abc009"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn a_corrupt_record_is_reported_rather_than_silently_ignored() {
        let dir = project();
        let id = release_id("20260101T120000Z-abc00a");
        let path = crate::manifest::ReleasePaths::new(dir.path(), &id).directory;
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("publish-record.json"), "{ not json").unwrap();
        let error = read_record(dir.path(), &id).unwrap_err();
        assert!(error.to_string().contains("not a publish record"), "{error}");
    }

    #[test]
    fn the_record_round_trips_through_json() {
        let record = PublishRecord {
            release_id: "20260101T120000Z-abc00b".to_string(),
            commit_sha: "abc123".to_string(),
            tag_name: "release/20260101T120000Z-abc00b".to_string(),
            committed: true,
            changed_files: 3,
            created_at: "2026-01-01T12:00:00Z".to_string(),
            created_from_release_id: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        // An absent origin is omitted rather than written as null.
        assert!(!json.contains("created_from_release_id"), "{json}");
        assert_eq!(serde_json::from_str::<PublishRecord>(&json).unwrap(), record);
    }

    #[test]
    fn the_outcome_reports_the_release_directory() {
        let dir = project();
        let outcome = publish(
            &request(dir.path(), "20260101T120000Z-abc00c"),
            &StubRunner::new(),
            None,
            &clock(),
        )
        .unwrap();
        assert!(outcome.release_dir().ends_with("20260101T120000Z-abc00c"));
        assert!(outcome.release_dir().is_dir());
    }
}
