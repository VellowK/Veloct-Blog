//! Git integration for a managed project.
//!
//! Only publishing creates a commit (`handoff.md` §0 rule 10, §2.5, §14.3, §19.2). Autosave
//! must not be able to create one at all, so the commit API lives behind the publish path.
//!
//! The `https` and `ssh` features of git2 are deliberately disabled: on Windows/MSVC they pull
//! in `openssl-sys`, which has no system provider. Network git traffic, if it is ever needed,
//! goes through the HTTP seam instead.
//!
//! Ownership: `core-git-release` (team `veloct-complete`).

#![forbid(unsafe_code)]
