//! Change detection.
//!
//! `handoff.md` §14.3 requires that a publish computes the workspace change first and does nothing
//! when there is none, and row A7 of the test matrix proves it. §23.7 states it again as a
//! testable requirement.
//!
//! Two details matter and are both easy to get wrong:
//!
//! * Ignored paths must not count as changes. `dist/` is regenerated on every build and
//!   `.attachments-cache/` churns constantly, so treating them as content would make every publish
//!   look dirty — and would put the build output into every release commit.
//! * `.veloct/releases/` must **count** as a change. A release manifest and its frozen build inputs
//!   are part of the release and have to be committed, or a historical release stops being
//!   reproducible (`handoff.md` §14.4).

use std::path::Path;

use git2::{Repository, Status, StatusOptions};
use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

/// What happened to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChanged,
    Conflicted,
    Untracked,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
            Self::TypeChanged => "type_changed",
            Self::Conflicted => "conflicted",
            Self::Untracked => "untracked",
        }
    }

    /// Whether this change blocks a publish (`handoff.md` §20.3).
    pub fn is_conflict(self) -> bool {
        matches!(self, Self::Conflicted)
    }
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Project-relative POSIX path.
    pub path: String,
    /// For a rename, the previous path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub kind: ChangeKind,
    /// True when the change is staged in the index.
    pub staged: bool,
}

impl FileChange {
    pub fn is_conflict(&self) -> bool {
        self.kind.is_conflict()
    }
}

/// Everything that differs from the last commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub files: Vec<FileChange>,
}

impl ChangeSet {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Paths with an unresolved merge conflict.
    pub fn conflicts(&self) -> Vec<&FileChange> {
        self.files.iter().filter(|file| file.is_conflict()).collect()
    }

    /// Whether publishing is blocked by an unresolved conflict.
    pub fn has_conflicts(&self) -> bool {
        self.files.iter().any(FileChange::is_conflict)
    }

    /// A stable, human-readable one-line summary for a commit subject.
    pub fn summary(&self) -> String {
        if self.files.is_empty() {
            return "no changes".to_string();
        }
        let mut added = 0usize;
        let mut modified = 0usize;
        let mut deleted = 0usize;
        let mut other = 0usize;
        for file in &self.files {
            match file.kind {
                ChangeKind::Added | ChangeKind::Untracked => added += 1,
                ChangeKind::Modified | ChangeKind::Renamed | ChangeKind::TypeChanged => modified += 1,
                ChangeKind::Deleted => deleted += 1,
                ChangeKind::Conflicted => other += 1,
            }
        }
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("{added} added"));
        }
        if modified > 0 {
            parts.push(format!("{modified} modified"));
        }
        if deleted > 0 {
            parts.push(format!("{deleted} deleted"));
        }
        if other > 0 {
            parts.push(format!("{other} conflicted"));
        }
        format!("{} ({})", parts.join(", "), self.files.len())
    }

    /// Paths, sorted, for a deterministic commit message or a test assertion.
    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<String> = self.files.iter().map(|file| file.path.clone()).collect();
        paths.sort();
        paths
    }
}

/// Paths that must never be committed, relative to the project root.
///
/// `dist/` is regenerated and is shipped with backups rather than commits; the attachment cache is
/// working state; the log directory churns on every action.
pub const IGNORED_PREFIXES: [&str; 4] = ["dist/", ".attachments-cache/", ".veloct/logs/", "node_modules/"];

/// Whether a project-relative path is one Veloct ignores.
///
/// The comparison is on a path boundary, so `dist/` matches both `dist` and `dist/site/index.html`
/// but not `distribution/notes.md`.
pub fn is_ignored(path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    IGNORED_PREFIXES.iter().any(|prefix| {
        let bare = prefix.trim_end_matches('/');
        normalised == bare || normalised.starts_with(prefix)
    })
}

/// Compute the workspace change set for a repository.
///
/// Untracked files count as changes: a newly written Markdown file is real content and must be
/// committed by the publish that follows. Ignored paths do not count.
pub fn changes(repository: &Repository) -> Result<ChangeSet> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        // Recurse into untracked directories so a new `works/<id>/site/` tree is reported as its
        // files rather than as one opaque directory entry.
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .exclude_submodules(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);

    let statuses = repository
        .statuses(Some(&mut options))
        .map_err(|e| VeloctError::Git(format!("could not read the working tree status: {e}")))?;

    let mut files = Vec::new();
    for entry in statuses.iter() {
        let Ok(path) = entry.path() else {
            continue;
        };
        let path = path.replace('\\', "/");
        if is_ignored(&path) {
            continue;
        }

        let status = entry.status();
        let staged = status.intersects(
            Status::INDEX_NEW
                | Status::INDEX_MODIFIED
                | Status::INDEX_DELETED
                | Status::INDEX_RENAMED
                | Status::INDEX_TYPECHANGE,
        );

        // A path can carry several status bits; report the most significant one, so a file that is
        // both modified and conflicted is reported as conflicted.
        let kind = if status.contains(Status::CONFLICTED) {
            ChangeKind::Conflicted
        } else if status.contains(Status::WT_NEW) || status.contains(Status::INDEX_NEW) {
            ChangeKind::Added
        } else if status.contains(Status::WT_DELETED) || status.contains(Status::INDEX_DELETED) {
            ChangeKind::Deleted
        } else if status.contains(Status::WT_RENAMED) || status.contains(Status::INDEX_RENAMED) {
            ChangeKind::Renamed
        } else if status.contains(Status::WT_TYPECHANGE) || status.contains(Status::INDEX_TYPECHANGE) {
            ChangeKind::TypeChanged
        } else if status.contains(Status::WT_MODIFIED) || status.contains(Status::INDEX_MODIFIED) {
            ChangeKind::Modified
        } else {
            ChangeKind::Untracked
        };

        files.push(FileChange {
            path,
            previous_path: None,
            kind,
            staged,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ChangeSet { files })
}

/// Open a repository, searching parent directories.
pub fn open(root: impl AsRef<Path>) -> Result<Repository> {
    Repository::discover(root.as_ref()).map_err(|e| {
        VeloctError::Git(format!(
            "{} is not a Git repository: {e}",
            root.as_ref().display()
        ))
    })
}

/// Open a repository at exactly this path, without searching upwards.
pub fn open_exact(root: impl AsRef<Path>) -> Result<Repository> {
    Repository::open(root.as_ref()).map_err(|e| {
        VeloctError::Git(format!(
            "{} is not a Git repository: {e}",
            root.as_ref().display()
        ))
    })
}

/// Whether a directory is already a repository.
pub fn is_repository(root: impl AsRef<Path>) -> bool {
    Repository::open(root.as_ref()).is_ok()
}

/// Initialise a repository with a Veloct-shaped `.gitignore`.
///
/// The ignore list keeps regenerable output out of the index while keeping `.veloct/releases/` in
/// it, because a release manifest and its frozen inputs are part of the release.
pub fn init(root: impl AsRef<Path>) -> Result<Repository> {
    let root = root.as_ref();
    std::fs::create_dir_all(root).map_err(|e| VeloctError::io(root, e))?;

    std::fs::create_dir_all(root.join(".veloct").join("releases"))
        .map_err(|e| VeloctError::io(root.join(".veloct"), e))?;

    let repository = Repository::init(root)
        .map_err(|e| VeloctError::Git(format!("could not initialise {}: {e}", root.display())))?;

    // Name the initial branch explicitly. Without this the repository has no `refs/heads/*` until
    // the first commit, so anything that resolves HEAD by branch name fails.
    if repository.head().is_err() {
        let _ = repository.set_head("refs/heads/main");
    }

    let gitignore = root.join(".gitignore");
    let contents = veloct_gitignore();
    veloct_core::workdir::write_text_if_changed(&gitignore, &contents)?;

    Ok(repository)
}

/// The `.gitignore` Veloct writes into a new project.
pub fn veloct_gitignore() -> String {
    let mut out = String::from(
        "# Written by Veloct. Regenerable build output is not committed; the current dist is\n\
         # included in backups instead. Release manifests ARE committed, because a historical\n\
         # release must stay rebuildable from its own commit.\n",
    );
    for prefix in IGNORED_PREFIXES {
        out.push_str(prefix.trim_end_matches('/'));
        out.push_str("/\n");
    }
    out.push_str("*.log\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::workdir::write_atomic;

    struct Fixture {
        _dir: tempfile::TempDir,
        repository: Repository,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let repository = init(dir.path()).unwrap();
        // A signature is required to commit; set it locally so a machine without a global identity
        // still works.
        {
            let mut config = repository.config().unwrap();
            config.set_str("user.name", "Veloct Test").unwrap();
            config.set_str("user.email", "test@veloct.invalid").unwrap();
        }
        Fixture {
            _dir: dir,
            repository,
        }
    }

    fn commit_all(repository: &Repository, message: &str) {
        let mut index = repository.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = repository.signature().unwrap();
        let parents: Vec<git2::Commit> = match repository.head().ok().and_then(|h| h.peel_to_commit().ok()) {
            Some(parent) => vec![parent],
            None => Vec::new(),
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repository
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parent_refs,
            )
            .unwrap();
    }

    #[test]
    fn a_fresh_repository_has_no_changes() {
        let fixture = fixture();
        // The generated .gitignore is itself untracked at first; committing it clears the workspace.
        commit_all(&fixture.repository, "initial");
        let detected = changes(&fixture.repository).unwrap();
        assert!(detected.is_empty(), "{:?}", detected.files);
        assert_eq!(detected.summary(), "no changes");
    }

    #[test]
    fn a_new_markdown_file_is_reported_as_an_addition() {
        // Row A7 needs this: real content must count as a change.
        let fixture = fixture();
        commit_all(&fixture.repository, "initial");

        let path = fixture.repository.workdir().unwrap().join("content/articles/a.md");
        write_atomic(&path, b"# A\n").unwrap();

        let changes = changes(&fixture.repository).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes.files[0].kind, ChangeKind::Added);
        assert_eq!(changes.files[0].path, "content/articles/a.md");
        assert!(changes.summary().contains("1 added"));
    }

    #[test]
    fn regenerable_output_does_not_count_as_a_change() {
        // dist/ and the attachment cache churn constantly; counting them would make every publish
        // look dirty and would put build output into release commits.
        let fixture = fixture();
        commit_all(&fixture.repository, "initial");

        let workdir = fixture.repository.workdir().unwrap();
        write_atomic(&workdir.join("dist/site/index.html"), b"<html></html>").unwrap();
        write_atomic(&workdir.join(".attachments-cache/hash/a.png"), b"png").unwrap();
        write_atomic(&workdir.join(".veloct/logs/veloct.log"), b"log").unwrap();
        write_atomic(&workdir.join("node_modules/pkg/index.js"), b"js").unwrap();

        let changes = changes(&fixture.repository).unwrap();
        assert!(changes.is_empty(), "ignored paths leaked in: {:?}", changes.files);
    }

    #[test]
    fn a_release_manifest_does_count_as_a_change() {
        // handoff 14.4: the manifest and frozen build inputs are part of the release.
        let fixture = fixture();
        commit_all(&fixture.repository, "initial");

        let workdir = fixture.repository.workdir().unwrap();
        write_atomic(
            &workdir.join(".veloct/releases/20260101T120000Z-abc123/release.json"),
            br#"{"release_id":"20260101T120000Z-abc123"}"#,
        )
        .unwrap();

        let changes = changes(&fixture.repository).unwrap();
        assert_eq!(changes.len(), 1);
        assert!(changes.files[0].path.starts_with(".veloct/releases/"));
    }

    #[test]
    fn modified_and_deleted_files_are_classified_correctly() {
        let fixture = fixture();
        let workdir = fixture.repository.workdir().unwrap().to_path_buf();
        write_atomic(&workdir.join("a.md"), b"one").unwrap();
        write_atomic(&workdir.join("b.md"), b"two").unwrap();
        commit_all(&fixture.repository, "initial");

        write_atomic(&workdir.join("a.md"), b"changed").unwrap();
        std::fs::remove_file(workdir.join("b.md")).unwrap();

        let changes = changes(&fixture.repository).unwrap();
        let by_path: Vec<(String, ChangeKind)> = changes
            .files
            .iter()
            .map(|file| (file.path.clone(), file.kind))
            .collect();
        assert!(by_path.contains(&("a.md".to_string(), ChangeKind::Modified)), "{by_path:?}");
        assert!(by_path.contains(&("b.md".to_string(), ChangeKind::Deleted)), "{by_path:?}");
        assert!(changes.summary().contains("1 modified"));
        assert!(changes.summary().contains("1 deleted"));
    }

    #[test]
    fn an_untracked_directory_is_reported_as_its_files() {
        // A newly imported work is a directory tree; reporting one opaque entry would hide it.
        let fixture = fixture();
        commit_all(&fixture.repository, "initial");
        let workdir = fixture.repository.workdir().unwrap();

        write_atomic(&workdir.join("works/01ABC/work.json"), b"{}").unwrap();
        write_atomic(&workdir.join("works/01ABC/site/index.html"), b"<html>").unwrap();

        let changes = changes(&fixture.repository).unwrap();
        let paths = changes.paths();
        assert!(paths.contains(&"works/01ABC/work.json".to_string()), "{paths:?}");
        assert!(paths.contains(&"works/01ABC/site/index.html".to_string()), "{paths:?}");
    }

    #[test]
    fn a_merge_conflict_is_detected_and_names_the_files() {
        // handoff 20.3: a conflict must be locatable to a file and must block a publish.
        let fixture = fixture();
        let workdir = fixture.repository.workdir().unwrap().to_path_buf();
        write_atomic(&workdir.join("shared.md"), b"base\n").unwrap();
        commit_all(&fixture.repository, "base");
        write_atomic(&workdir.join("other.md"), b"unrelated\n").unwrap();
        commit_all(&fixture.repository, "second");

        // Two divergent branches that each change the same line of the same file. Leaving the
        // working tree out of it keeps this a pure git-level conflict.
        let repository = &fixture.repository;
        let base = repository
            .revparse_single("HEAD~1")
            .unwrap()
            .peel_to_commit()
            .unwrap();

        // `main` changes shared.md one way.
        repository
            .branch("mainline", &base, true)
            .unwrap();
        repository.set_head("refs/heads/mainline").unwrap();
        repository
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        write_atomic(&workdir.join("shared.md"), b"main change\n").unwrap();
        commit_all(repository, "main changes shared.md");

        // `feature` changes the same line a different way, from the same base.
        repository
            .branch("feature", &base, true)
            .unwrap();
        repository.set_head("refs/heads/feature").unwrap();
        repository
            .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
            .unwrap();
        write_atomic(&workdir.join("shared.md"), b"feature change\n").unwrap();
        commit_all(repository, "feature changes shared.md");

        // Merge mainline into feature: both sides changed the same line, so this conflicts.
        let main = repository
            .find_reference("refs/heads/mainline")
            .unwrap()
            .peel_to_commit()
            .unwrap();
        let annotated = repository.find_annotated_commit(main.id()).unwrap();
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force();
        let merge_result = repository.merge(&[&annotated], None, Some(&mut checkout));
        assert!(merge_result.is_ok(), "merge setup failed: {merge_result:?}");

        let changes = changes(repository).unwrap();
        let conflicts = changes.conflicts();
        assert!(!conflicts.is_empty(), "expected a conflict, got {:?}", changes.files);
        assert_eq!(conflicts[0].path, "shared.md");
        assert!(changes.has_conflicts());
        assert!(changes.summary().contains("conflicted"));
    }

    #[test]
    fn ignored_prefixes_are_matched_on_path_boundaries() {
        assert!(is_ignored("dist/site/index.html"));
        assert!(is_ignored("dist"));
        assert!(is_ignored(".attachments-cache/x/y.png"));
        assert!(!is_ignored("content/dist.md"));
        // A directory whose name merely starts with an ignored one is not ignored.
        assert!(!is_ignored("distribution/a.md"));
    }

    #[test]
    fn opening_a_non_repository_fails_with_a_clear_message() {
        let dir = tempfile::tempdir().unwrap();
        match open_exact(dir.path()) {
            Ok(_) => panic!("a plain directory must not open as a repository"),
            Err(error) => {
                assert_eq!(error.code(), "GIT");
                assert!(error.to_string().contains("not a Git repository"));
            }
        }
        assert!(!is_repository(dir.path()));
    }

    #[test]
    fn init_writes_an_ignore_file_that_keeps_releases_and_drops_build_output() {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();
        let contents = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(contents.contains("dist/"));
        assert!(contents.contains(".attachments-cache/"));
        assert!(contents.contains(".veloct/logs/"));
        // Releases are deliberately absent from the ignore list.
        assert!(!contents.contains(".veloct/releases/"));
        assert!(dir.path().join(".veloct").join("releases").is_dir());
    }

    #[test]
    fn change_sets_round_trip_through_json() {
        let fixture = fixture();
        commit_all(&fixture.repository, "initial");
        write_atomic(
            &fixture.repository.workdir().unwrap().join("new.md"),
            b"x",
        )
        .unwrap();

        let changes = changes(&fixture.repository).unwrap();
        let json = serde_json::to_string(&changes).unwrap();
        assert_eq!(serde_json::from_str::<ChangeSet>(&json).unwrap(), changes);
        assert!(json.contains("\"kind\":\"added\""));
    }

    #[test]
    fn open_discovers_a_repository_from_a_subdirectory() {
        let fixture = fixture();
        let nested = fixture
            .repository
            .workdir()
            .unwrap()
            .join("content")
            .join("articles");
        std::fs::create_dir_all(&nested).unwrap();
        let discovered = open(&nested).unwrap();
        assert!(discovered.workdir().is_some());
    }
}
