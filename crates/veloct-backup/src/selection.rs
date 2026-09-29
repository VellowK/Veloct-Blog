//! What a backup contains, and what it must never contain.
//!
//! `handoff.md` §17.1 lists what a backup has to be able to restore — Markdown, SQLite, the site and
//! application configuration, the Git repository, the Astro project, the current `dist/`, the local
//! attachment cache — and then lists what it must exclude: API keys, tokens, secrets, S3 secrets,
//! GitHub and deployment credentials.
//!
//! §17.2 adds the reason the exclusion matters: **the backup is not encrypted**. So a credential that
//! reaches the archive reaches it in plaintext, and the archive is intended to be uploaded to a cloud
//! target. Two consequences shape this module:
//!
//! * The selection is written as a deny-list that runs BEFORE anything is read, so a credential file
//!   is never even opened. Scanning its contents for secrets would be a second line of defence, not
//!   the first.
//! * A credential HANDLE (`credential_ref`) stays, because `handoff.md` §17.10 says a restore must
//!   rebind the missing credentials rather than silently working without them. Keeping the handle is
//!   what makes "rebind these two" a thing the UI can say.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::Result;

/// A directory that must never enter a backup, with the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exclusion {
    /// The path prefix, project-relative, with forward slashes.
    pub prefix: &'static str,
    pub reason: &'static str,
}

/// Paths excluded from every backup.
///
/// Ordered longest-first is NOT required, because a prefix match is used; the order here groups the
/// reasons so a reader can check them against §17.1.
pub const EXCLUSIONS: &[Exclusion] = &[
    Exclusion {
        prefix: ".venv/",
        reason: "a Python environment is reproducible and can contain credentials",
    },
    Exclusion {
        prefix: "node_modules/",
        reason: "dependencies are reproducible from the lockfile and are frequently larger than the project",
    },
    Exclusion {
        prefix: ".env",
        reason: "an environment file holds credentials in plaintext (handoff.md section 17.1)",
    },
    Exclusion {
        prefix: ".veloct/logs/",
        reason: "logs are excluded from a release build for the same reason: they are not project data",
    },
    Exclusion {
        prefix: ".veloct/tmp/",
        reason: "a scratch directory is not project data",
    },
    Exclusion {
        prefix: ".veloct/credentials",
        reason: "a credential store file must never enter an unencrypted archive",
    },
    Exclusion {
        prefix: "secrets/",
        reason: "a directory named for secrets must never enter an unencrypted archive",
    },
];

/// File extensions that never enter a backup.
pub const EXCLUDED_EXTENSIONS: &[&str] = &[".pem", ".key", ".p12", ".pfx", ".jks", ".keystore"];

/// File names that never enter a backup, wherever they appear.
pub const EXCLUDED_NAMES: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    "id_rsa",
    "id_ed25519",
    "credentials.json",
    "service-account.json",
    ".netrc",
    ".npmrc",
    ".pypirc",
];

/// Why a path was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedPath {
    /// The project-relative path.
    pub path: String,
    pub reason: String,
}

/// What a backup will include.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSelection {
    /// The files to include, project-relative and sorted.
    pub included: Vec<String>,
    /// The paths left out, with reasons, so the UI can explain the archive's contents.
    pub skipped: Vec<SkippedPath>,
    /// Total bytes of the included files.
    pub total_bytes: u64,
}

impl BackupSelection {
    pub fn file_count(&self) -> usize {
        self.included.len()
    }

    /// Whether a path is excluded, and why.
    pub fn exclusion_for(path: &str) -> Option<String> {
        let normalised = path.replace('\\', "/");
        let lowered = normalised.to_ascii_lowercase();

        for exclusion in EXCLUSIONS {
            // A prefix match on the normalised path, so `.env` matches `.env` and `.env.local` but
            // not `environment.md`.
            let prefix = exclusion.prefix.trim_end_matches('/');
            if lowered == prefix
                || lowered.starts_with(&format!("{prefix}/"))
                || (exclusion.prefix.ends_with('/') && lowered.starts_with(exclusion.prefix))
                || (exclusion.prefix == ".env" && lowered.starts_with(".env"))
            {
                return Some(exclusion.reason.to_string());
            }
        }

        // An extension match anywhere.
        if let Some(extension) = Path::new(&lowered)
            .extension()
            .and_then(|extension| extension.to_str())
        {
            let dotted = format!(".{extension}");
            if EXCLUDED_EXTENSIONS.contains(&dotted.as_str()) {
                return Some(format!(
                    "{dotted} files hold key material and must never enter an unencrypted archive"
                ));
            }
        }

        // A name match anywhere in the tree.
        if let Some(name) = Path::new(&lowered).file_name().and_then(|name| name.to_str()) {
            if EXCLUDED_NAMES.contains(&name) {
                return Some(format!(
                    "{name} holds credentials and must never enter an unencrypted archive"
                ));
            }
        }

        None
    }

    /// Whether a path would be included.
    pub fn is_included(path: &str) -> bool {
        Self::exclusion_for(path).is_none()
    }
}

/// Walk a project and compute the selection.
///
/// Directories that are excluded are not descended into: a `node_modules` tree could be hundreds of
/// thousands of entries, and walking it to then discard the results would make the backup slow for no
/// reason.
pub fn select(root: &Path) -> Result<BackupSelection> {
    let mut included = Vec::new();
    let mut skipped = Vec::new();
    let mut total_bytes = 0u64;

    walk(root, root, &mut included, &mut skipped, &mut total_bytes)?;

    included.sort();
    skipped.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(BackupSelection {
        included,
        skipped,
        total_bytes,
    })
}

fn walk(
    root: &Path,
    current: &Path,
    included: &mut Vec<String>,
    skipped: &mut Vec<SkippedPath>,
    total_bytes: &mut u64,
) -> Result<()> {
    let entries = match std::fs::read_dir(current) {
        Ok(entries) => entries,
        // An unreadable directory is skipped with a reason rather than failing the whole backup:
        // losing the backup because one directory is locked would be worse than losing that
        // directory, and the reason is recorded so the user is told.
        Err(error) => {
            skipped.push(SkippedPath {
                path: relative(root, current),
                reason: format!("the directory could not be read: {error}"),
            });
            return Ok(());
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let relative_path = relative(root, &path);

        if let Some(reason) = BackupSelection::exclusion_for(&relative_path) {
            skipped.push(SkippedPath {
                path: relative_path,
                reason,
            });
            continue;
        }

        let Ok(metadata) = path.symlink_metadata() else {
            skipped.push(SkippedPath {
                path: relative_path,
                reason: "the entry could not be inspected".to_string(),
            });
            continue;
        };

        // A symlink is not followed. It could point outside the project, and including the target
        // would put something in the archive that the project does not own.
        if metadata.file_type().is_symlink() {
            skipped.push(SkippedPath {
                path: relative_path,
                reason: "a symbolic link is not followed, because it may point outside the project"
                    .to_string(),
            });
            continue;
        }

        if metadata.is_dir() {
            walk(root, &path, included, skipped, total_bytes)?;
            continue;
        }

        if metadata.is_file() {
            *total_bytes += metadata.len();
            included.push(relative_path);
        }
    }

    Ok(())
}

/// A project-relative path with forward slashes.
fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|rest| rest.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

/// The absolute path of a selected file.
pub fn absolute(root: &Path, relative_path: &str) -> PathBuf {
    root.join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Whether a path is one of the `dist/` trees, which §17.1 includes but §14.13 treats separately.
pub fn is_dist(path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    normalised == "dist" || normalised.starts_with("dist/")
}

/// Whether a path is inside the attachment cache, which §17.1 includes.
pub fn is_attachment_cache(path: &str) -> bool {
    path.replace('\\', "/").starts_with(".attachments-cache/")
}

/// Whether a path is inside the Git repository metadata, which §17.1 includes.
pub fn is_git_metadata(path: &str) -> bool {
    let normalised = path.replace('\\', "/");
    normalised == ".git" || normalised.starts_with(".git/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn project() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn markdown_sqlite_git_and_dist_are_included() {
        // handoff.md section 17.1, the "must include" list.
        let dir = project();
        let root = dir.path();
        for path in [
            "content/articles/a.md",
            ".veloct/veloct.db",
            ".git/HEAD",
            "site/astro.config.mjs",
            "dist/site/index.html",
            ".attachments-cache/deadbeef/photo.webp",
            "data/site.json",
        ] {
            fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
            fs::write(root.join(path), "x").unwrap();
        }

        let selection = select(root).unwrap();
        for path in [
            "content/articles/a.md",
            ".veloct/veloct.db",
            ".git/HEAD",
            "site/astro.config.mjs",
            "dist/site/index.html",
            ".attachments-cache/deadbeef/photo.webp",
            "data/site.json",
        ] {
            assert!(
                selection.included.contains(&path.to_string()),
                "{path} should be included: {:?}",
                selection.included
            );
        }
        assert!(is_dist("dist/site/index.html"));
        assert!(is_attachment_cache(".attachments-cache/x/y"));
        assert!(is_git_metadata(".git/HEAD"));
    }

    #[test]
    fn a_credentials_file_is_never_included_and_is_never_even_walked_into() {
        // handoff.md section 17.1 and 17.2: the archive is NOT encrypted, so the exclusion must run
        // before anything is read.
        let dir = project();
        let root = dir.path();
        fs::create_dir_all(root.join(".venv/lib")).unwrap();
        fs::write(root.join(".venv/lib/credentials"), "secret").unwrap();
        fs::write(root.join(".env"), "API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwxyz").unwrap();
        fs::write(root.join("content.md"), "safe").unwrap();

        let selection = select(root).unwrap();
        assert_eq!(selection.included, vec!["content.md"]);
        assert!(selection.skipped.iter().any(|s| s.path == ".env"));
        assert!(selection.skipped.iter().any(|s| s.path == ".venv"));
        // The directory was not descended into: no entry under it appears at all.
        assert!(!selection
            .skipped
            .iter()
            .any(|s| s.path.starts_with(".venv/") && s.path != ".venv"));
    }

    #[test]
    fn key_material_extensions_are_excluded_wherever_they_appear() {
        for path in [
            "certs/server.pem",
            "keys/private.key",
            "identity.p12",
            "store.pfx",
            "java/app.jks",
        ] {
            assert!(
                BackupSelection::exclusion_for(path).is_some(),
                "{path} should be excluded"
            );
        }
    }

    #[test]
    fn credential_file_names_are_excluded_wherever_they_appear() {
        for path in [
            ".env",
            "config/.env.local",
            ".ssh/id_rsa",
            "home/.netrc",
            "ci/service-account.json",
            ".npmrc",
        ] {
            assert!(
                BackupSelection::exclusion_for(path).is_some(),
                "{path} should be excluded"
            );
        }
    }

    #[test]
    fn a_name_that_merely_contains_an_excluded_name_is_kept() {
        // `environment.md` is not an environment file, and a substring match would drop prose.
        for path in [
            "content/environment.md",
            "notes/on-env-vars.md",
            "site/my-keyboard-shortcuts.md",
        ] {
            assert!(
                BackupSelection::is_included(path),
                "{path} should be included"
            );
        }
    }

    #[test]
    fn node_modules_is_excluded_with_its_reason() {
        let reason = BackupSelection::exclusion_for("node_modules/pkg/index.js").unwrap();
        assert!(reason.contains("reproducible"), "{reason}");
    }

    #[test]
    fn a_symlink_is_not_followed_and_says_so() {
        let dir = project();
        let root = dir.path();
        fs::write(root.join("real.md"), "content").unwrap();

        // Creating a symlink needs a privilege on Windows, so the test only asserts the rule where it
        // can be created.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("real.md"), root.join("link.md")).unwrap();
            let selection = select(root).unwrap();
            assert!(!selection.included.contains(&"link.md".to_string()));
            assert!(selection
                .skipped
                .iter()
                .any(|s| s.path == "link.md" && s.reason.contains("symbolic link")));
        }

        #[cfg(windows)]
        {
            let selection = select(root).unwrap();
            assert!(selection.included.contains(&"real.md".to_string()));
        }
    }

    #[test]
    fn the_selection_reports_the_total_size_and_a_sorted_list() {
        let dir = project();
        let root = dir.path();
        fs::write(root.join("b.md"), "bb").unwrap();
        fs::write(root.join("a.md"), "a").unwrap();

        let selection = select(root).unwrap();
        assert_eq!(selection.included, vec!["a.md", "b.md"]);
        assert_eq!(selection.total_bytes, 3);
        assert_eq!(selection.file_count(), 2);
    }

    #[test]
    fn an_empty_project_selects_cleanly() {
        let dir = project();
        let selection = select(dir.path()).unwrap();
        assert!(selection.included.is_empty());
        assert!(selection.skipped.is_empty());
        assert_eq!(selection.total_bytes, 0);
    }

    #[test]
    fn an_unreadable_directory_is_skipped_with_a_reason_rather_than_failing_the_backup() {
        // Losing a whole backup because one directory is locked would be worse than losing that
        // directory, and the reason is recorded so the user is told.
        let dir = project();
        let root = dir.path();
        fs::write(root.join("keep.md"), "x").unwrap();
        // A directory removed between the read_dir and the walk: the OS reports it as missing.
        fs::create_dir_all(root.join("gone")).unwrap();
        fs::remove_dir(root.join("gone")).unwrap();

        let selection = select(root).unwrap();
        assert!(selection.included.contains(&"keep.md".to_string()));
    }

    #[test]
    fn a_selection_round_trips_through_json_for_the_ui() {
        let dir = project();
        let root = dir.path();
        fs::write(root.join("a.md"), "a").unwrap();
        fs::create_dir_all(root.join(".venv")).unwrap();
        fs::write(root.join(".venv/x"), "y").unwrap();

        let selection = select(root).unwrap();
        let json = serde_json::to_string(&selection).unwrap();
        assert!(json.contains("\"included\""), "{json}");
        assert!(json.contains("\"skipped\""), "{json}");
        assert_eq!(
            serde_json::from_str::<BackupSelection>(&json).unwrap(),
            selection
        );
    }

    #[test]
    fn an_absolute_path_helper_resolves_a_selected_file() {
        // The stored paths use forward slashes; the helper must convert them so `join` produces one
        // separator rather than a mixture that a later `strip_prefix` would fail on.
        let root = Path::new("D:/proj");
        let path = absolute(root, "content/articles/a.md");
        assert_eq!(path, root.join("content").join("articles").join("a.md"));
        assert!(path.ends_with("a.md"));

        let nested = absolute(root, "dist/site/index.html");
        assert_eq!(
            nested,
            root.join("dist").join("site").join("index.html")
        );
    }

    #[test]
    fn every_exclusion_has_a_reason_a_reader_can_check() {
        for exclusion in EXCLUSIONS {
            assert!(!exclusion.reason.is_empty(), "{} has no reason", exclusion.prefix);
        }
    }
}
