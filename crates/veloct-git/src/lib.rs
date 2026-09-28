//! Git integration for a managed project.
//!
//! `handoff.md` §2.5, §14.3, §19.2, §20.3 and §23 (row A7) define the contract. The rules that
//! shape this module:
//!
//! * **Only publishing creates a commit.** Autosave must not be *able* to create one, so the commit
//!   API lives in [`publish`] behind an explicit call that first proves there is a real change.
//! * **No change means no commit.** A publish with nothing to record returns
//!   [`CommitOutcome::NoChange`] rather than creating an empty commit, which would produce a
//!   meaningless release.
//! * **Unresolved conflicts block a publish.** A merge conflict must be reported with the file
//!   names so the user can resolve it, never committed silently.
//! * **The release tag is the source anchor.** `release/<release-id>` points at the publish commit,
//!   and that pairing is the release truth (`handoff.md` §14.4).
//!
//! The git2 `https` and `ssh` features are deliberately disabled: on Windows/MSVC they pull in
//! `openssl-sys`, which has no system provider there. Veloct's Git use is entirely local.

#![forbid(unsafe_code)]

pub mod change;
pub mod publish;

pub use change::{is_ignored, ChangeKind, ChangeSet, FileChange, IGNORED_PREFIXES};
pub use publish::{
    commit_for_tag, commit_publish, create_release_tag, create_release_tag_at, list_release_tags,
    publish_message, remove_worktree, tag_exists, worktree_at, CommitOutcome, PublishCommit,
    ReleaseTag, TagRef, TAG_PREFIX,
};
