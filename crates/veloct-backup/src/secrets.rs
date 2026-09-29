//! The secret scan that runs before a backup.
//!
//! `handoff.md` §17.3 states the problem exactly:
//!
//! > Because the backup contains the Git repository, "the current configuration has no secret" does
//! > not mean the Git history has no old secret.
//!
//! So the scan covers three surfaces — the working tree, the configuration files that are eligible
//! for backup, and the Git history — and a positive finding does **not** block the backup silently. It
//! blocks the AUTOMATIC upload (`handoff.md` §17.3's first bullet and §17.5), requires the user's
//! explicit confirmation for that one run, and is logged.
//!
//! One rule is easy to violate by accident and is therefore called out in the type system:
//! **Veloct must not rewrite Git history to "clean" a secret** (§17.3, last line). There is no such
//! function in this module and no git write call at all; [`GitSecretScan`] only reads.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use veloct_core::secret::{scan_text, SecretScanReport};
use veloct_core::Result;

/// Which surface a finding came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanSurface {
    /// A file in the working tree that would be included in the backup.
    WorkingTree,
    /// A configuration file eligible for backup.
    ConfigFile,
    /// A blob reachable from the Git history.
    GitHistory,
}

impl ScanSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkingTree => "working_tree",
            Self::ConfigFile => "config_file",
            Self::GitHistory => "git_history",
        }
    }

    /// A label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::WorkingTree => "当前工作树文件",
            Self::ConfigFile => "可备份的配置文件",
            Self::GitHistory => "Git 历史 / Git 对象",
        }
    }
}

/// One finding, located.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub surface: ScanSurface,
    /// The file path, or the commit and blob for a history finding.
    pub location: String,
    /// The kind of secret, as `veloct_core::secret` names it.
    pub kind: String,
    /// A REDACTED excerpt. The finding itself must not leak the secret into a log
    /// (`handoff.md` §18.1).
    pub excerpt: String,
    /// Whether the pattern is a high-confidence one, so a UI can order by severity.
    pub high_confidence: bool,
}

/// The whole scan report.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSecretScan {
    pub findings: Vec<Finding>,
    /// Files that were read.
    pub files_scanned: usize,
    /// Blobs that were read from the history.
    pub blobs_scanned: usize,
    /// Commits walked.
    pub commits_scanned: usize,
    /// Whether the Git history scan ran at all.
    ///
    /// A project with no repository is not a failure, but the report has to say the history was not
    /// checked, or "clean" would be a claim about something that never looked.
    pub history_scanned: bool,
    /// Why the history scan did not run, when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_skipped_reason: Option<String>,
}

impl BackupSecretScan {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn high_confidence_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.high_confidence)
            .count()
    }

    /// The distinct surfaces that produced a finding.
    pub fn surfaces(&self) -> Vec<ScanSurface> {
        let mut surfaces: Vec<ScanSurface> = self.findings.iter().map(|f| f.surface).collect();
        surfaces.sort();
        surfaces.dedup();
        surfaces
    }

    /// The distinct secret kinds found.
    pub fn kinds(&self) -> Vec<String> {
        let mut kinds: Vec<String> = self.findings.iter().map(|f| f.kind.clone()).collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    /// Whether a finding is in the Git HISTORY only.
    ///
    /// This is §17.3's whole point: a clean working tree does not mean a clean history, and a UI that
    /// only looked at the tree would tell the user something reassuring and false.
    pub fn has_history_findings(&self) -> bool {
        self.findings
            .iter()
            .any(|finding| finding.surface == ScanSurface::GitHistory)
    }

    /// A summary line for the log and the UI.
    pub fn summary(&self) -> String {
        if self.is_clean() {
            let mut text = format!(
                "no secret pattern found in {} files",
                self.files_scanned
            );
            if self.history_scanned {
                text.push_str(&format!(
                    " or in {} Git blobs across {} commits",
                    self.blobs_scanned, self.commits_scanned
                ));
            } else {
                text.push_str("; the Git history was NOT scanned");
            }
            return text;
        }
        format!(
            "{} possible secret(s) found across {}: {}",
            self.findings.len(),
            self.surfaces()
                .into_iter()
                .map(ScanSurface::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            self.kinds().join(", ")
        )
    }
}

/// A Git blob to scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobSource {
    /// The commit the blob is reachable from, for the finding's location.
    pub commit: String,
    /// The path the blob had in that commit.
    pub path: String,
    /// The blob's bytes.
    pub bytes: Vec<u8>,
}

/// Scan a set of working-tree files and configuration files.
///
/// `files` are project-relative paths that were already SELECTED for backup, so a file the selection
/// excluded is not scanned: there is no point reporting a secret in something that will not be
/// archived, and reporting it would train the user to ignore findings.
pub fn scan_files(root: &Path, files: &[String]) -> Result<BackupSecretScan> {
    let mut report = BackupSecretScan::default();

    for relative in files {
        let path = crate::selection::absolute(root, relative);
        // A binary file is skipped: `scan_text` on compressed or image bytes produces noise, and
        // noise is what makes a real finding ignorable.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if is_probably_binary(&bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        report.files_scanned += 1;

        let surface = if is_config_path(relative) {
            ScanSurface::ConfigFile
        } else {
            ScanSurface::WorkingTree
        };
        let found = scan_text(&text, relative);
        push_findings(&mut report, found, surface, relative);
    }

    Ok(report)
}

/// Scan Git history for secret patterns.
///
/// Reads blobs only. There is no write path here and none is reachable from this module, which is how
/// §17.3's "must not rewrite Git history to clean a secret" is guaranteed rather than promised.
pub fn scan_history(blobs: &[BlobSource]) -> BackupSecretScan {
    let mut report = BackupSecretScan {
        history_scanned: true,
        ..BackupSecretScan::default()
    };

    // The same blob appears in many commits, so it is scanned once. Without this a repository with a
    // thousand commits touching one file would report the same finding a thousand times.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

    for blob in blobs {
        if is_probably_binary(&blob.bytes) {
            continue;
        }
        let text = String::from_utf8_lossy(&blob.bytes);
        let key = (blob.path.clone(), veloct_core::sha256_hex(&blob.bytes));
        if !seen.insert(key) {
            continue;
        }
        report.blobs_scanned += 1;
        let location = format!("{}@{}", blob.path, short(&blob.commit));
        let found = scan_text(&text, &location);
        push_findings(&mut report, found, ScanSurface::GitHistory, &location);
    }

    report
}

/// A report for a project whose history could not be scanned.
///
/// Explicit rather than a silently clean report: §17.3 requires the history to be checked, so "we did
/// not check" has to be distinguishable from "we checked and it was clean".
pub fn history_not_scanned(reason: &str, files: &BackupSecretScan) -> BackupSecretScan {
    BackupSecretScan {
        history_scanned: false,
        history_skipped_reason: Some(reason.to_string()),
        ..files.clone()
    }
}

fn push_findings(
    report: &mut BackupSecretScan,
    found: SecretScanReport,
    surface: ScanSurface,
    location: &str,
) {
    for finding in found.findings {
        report.findings.push(Finding {
            surface,
            location: location.to_string(),
            kind: finding.kind.label().to_string(),
            // The core scanner already redacted this, and it is redacted AGAIN here so a future
            // change to that function cannot make this one leak a secret into a log.
            excerpt: veloct_core::secret::redact(&finding.redacted_excerpt),
            // The scanner's own confidence, not a second guess at it: two places deciding what is
            // high confidence is how they end up disagreeing.
            high_confidence: finding.high_confidence,
        });
    }
}

/// Whether bytes look like a binary file.
///
/// A NUL byte in the first block is the usual heuristic and is enough here: a text file with a NUL is
/// not something a Markdown project contains.
fn is_probably_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8192)];
    sample.contains(&0)
}

/// Whether a path is a configuration file, for the surface label.
fn is_config_path(path: &str) -> bool {
    let lowered = path.replace('\\', "/").to_ascii_lowercase();
    lowered.ends_with(".json")
        || lowered.ends_with(".toml")
        || lowered.ends_with(".yaml")
        || lowered.ends_with(".yml")
        || lowered.ends_with(".ini")
        || lowered.ends_with(".config")
        || lowered.contains("config")
}

fn short(sha: &str) -> String {
    sha.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    const ANTHROPIC_KEY: &str = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
    const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";

    fn project() -> TempDir {
        TempDir::new().unwrap()
    }

    #[test]
    fn a_clean_working_tree_reports_clean_and_says_how_many_files_it_read() {
        let dir = project();
        let root = dir.path();
        fs::write(root.join("a.md"), "ordinary prose").unwrap();
        fs::write(root.join("b.json"), r#"{"name":"site"}"#).unwrap();

        let report = scan_files(root, &["a.md".to_string(), "b.json".to_string()]).unwrap();
        assert!(report.is_clean());
        assert_eq!(report.files_scanned, 2);
        assert!(report.summary().contains("no secret pattern"));
        // A history that was never scanned must not be reported as clean.
        assert!(!report.history_scanned);
        assert!(report.summary().contains("NOT scanned"), "{}", report.summary());
    }

    #[test]
    fn a_secret_in_a_working_tree_file_is_found() {
        let dir = project();
        let root = dir.path();
        fs::write(
            root.join("notes.md"),
            format!("my key is {ANTHROPIC_KEY} keep it safe"),
        )
        .unwrap();

        let report = scan_files(root, &["notes.md".to_string()]).unwrap();
        assert!(!report.is_clean());
        assert_eq!(report.findings[0].surface, ScanSurface::WorkingTree);
        assert_eq!(report.findings[0].location, "notes.md");
        assert!(report.high_confidence_count() >= 1);
    }

    #[test]
    fn a_finding_never_carries_the_secret_into_the_report() {
        // handoff.md section 18.1: the log must not contain a secret, and this report is logged.
        let dir = project();
        let root = dir.path();
        fs::write(root.join("cfg.json"), format!(r#"{{"key":"{ANTHROPIC_KEY}"}}"#)).unwrap();

        let report = scan_files(root, &["cfg.json".to_string()]).unwrap();
        assert!(!report.is_clean());

        let json = serde_json::to_string(&report).unwrap();
        assert!(
            !json.contains(ANTHROPIC_KEY),
            "the report leaked the key: {json}"
        );
        assert!(json.contains("REDACTED"), "{json}");
        assert!(!report.findings[0].excerpt.contains(ANTHROPIC_KEY));
    }

    #[test]
    fn a_configuration_file_is_labelled_as_one() {
        let dir = project();
        let root = dir.path();
        fs::create_dir_all(root.join("data")).unwrap();
        fs::write(root.join("data/site.json"), format!(r#"{{"token":"{AWS_KEY}"}}"#)).unwrap();

        let report = scan_files(root, &["data/site.json".to_string()]).unwrap();
        assert_eq!(report.findings[0].surface, ScanSurface::ConfigFile);
        assert_eq!(report.surfaces(), vec![ScanSurface::ConfigFile]);
    }

    #[test]
    fn a_binary_file_is_not_scanned_because_noise_makes_findings_ignorable() {
        let dir = project();
        let root = dir.path();
        // A PNG header with a NUL, containing a key-like string by chance.
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x00];
        bytes.extend_from_slice(ANTHROPIC_KEY.as_bytes());
        fs::write(root.join("image.png"), &bytes).unwrap();

        let report = scan_files(root, &["image.png".to_string()]).unwrap();
        assert!(report.is_clean());
        assert_eq!(report.files_scanned, 0, "a binary file is not read as text");
    }

    #[test]
    fn a_missing_file_is_skipped_rather_than_failing_the_scan() {
        let dir = project();
        let report = scan_files(dir.path(), &["absent.md".to_string()]).unwrap();
        assert!(report.is_clean());
        assert_eq!(report.files_scanned, 0);
    }

    #[test]
    fn history_only_findings_are_reported_as_history_findings() {
        // handoff.md section 17.3: a clean tree does not mean a clean history. This is the case the
        // whole section exists for.
        let clean_tree = BackupSecretScan {
            files_scanned: 10,
            ..BackupSecretScan::default()
        };

        let blobs = vec![BlobSource {
            commit: "abcdef1234567890".to_string(),
            path: "old/config.json".to_string(),
            bytes: format!(r#"{{"key":"{AWS_KEY}"}}"#).into_bytes(),
        }];
        let report = scan_history(&blobs);

        assert!(report.history_scanned);
        assert!(report.has_history_findings());
        assert!(!report.is_clean());
        assert_eq!(report.blobs_scanned, 1);
        assert!(report.findings[0].location.contains("old/config.json@abcdef12"));
        assert_eq!(report.findings[0].surface, ScanSurface::GitHistory);

        // And the tree's own report is still clean, which is the point.
        assert!(clean_tree.is_clean());
    }

    #[test]
    fn the_same_blob_in_many_commits_is_reported_once() {
        // A repository with a thousand commits touching one file must not produce a thousand
        // identical findings.
        let bytes = format!("key {AWS_KEY}").into_bytes();
        let blobs: Vec<BlobSource> = (0..50)
            .map(|index| BlobSource {
                commit: format!("commit{index:040}"),
                path: "config.json".to_string(),
                bytes: bytes.clone(),
            })
            .collect();

        let report = scan_history(&blobs);
        assert_eq!(report.blobs_scanned, 1, "the identical blob is scanned once");
        assert_eq!(report.findings.len(), 1);
    }

    #[test]
    fn different_blobs_at_the_same_path_are_both_scanned() {
        // A file whose secret was removed later: the old blob still has it, the new one does not.
        let blobs = vec![
            BlobSource {
                commit: "aaaa".to_string(),
                path: "config.json".to_string(),
                bytes: format!("key {AWS_KEY}").into_bytes(),
            },
            BlobSource {
                commit: "bbbb".to_string(),
                path: "config.json".to_string(),
                bytes: b"key REDACTED".to_vec(),
            },
        ];
        let report = scan_history(&blobs);
        assert_eq!(report.blobs_scanned, 2);
        assert_eq!(report.findings.len(), 1);
    }

    #[test]
    fn a_binary_blob_in_the_history_is_not_scanned() {
        let blobs = vec![BlobSource {
            commit: "aaaa".to_string(),
            path: "logo.png".to_string(),
            bytes: vec![0x00, b'x', b'y'],
        }];
        let report = scan_history(&blobs);
        assert!(report.is_clean());
        assert_eq!(report.blobs_scanned, 0);
    }

    #[test]
    fn a_history_that_was_not_scanned_says_so_rather_than_reporting_clean() {
        // "We did not check" must be distinguishable from "we checked and it was clean".
        let files = BackupSecretScan {
            files_scanned: 5,
            ..BackupSecretScan::default()
        };
        let report = history_not_scanned("this project is not a Git repository", &files);

        assert!(!report.history_scanned);
        assert!(report.is_clean(), "there is no finding, but...");
        assert_eq!(
            report.history_skipped_reason.as_deref(),
            Some("this project is not a Git repository")
        );
        assert!(report.summary().contains("NOT scanned"), "{}", report.summary());
    }

    #[test]
    fn a_report_summarises_what_was_found_without_naming_a_secret() {
        let dir = project();
        let root = dir.path();
        fs::write(root.join("a.md"), format!("{ANTHROPIC_KEY}\n{AWS_KEY}")).unwrap();
        let report = scan_files(root, &["a.md".to_string()]).unwrap();

        let summary = report.summary();
        assert!(!summary.contains(ANTHROPIC_KEY), "{summary}");
        assert!(!summary.contains(AWS_KEY), "{summary}");
        assert!(summary.contains("possible secret"), "{summary}");
        assert!(!report.kinds().is_empty());
    }

    #[test]
    fn every_surface_has_a_label_and_a_stable_name() {
        for surface in [
            ScanSurface::WorkingTree,
            ScanSurface::ConfigFile,
            ScanSurface::GitHistory,
        ] {
            assert!(!surface.as_str().is_empty());
            assert!(!surface.label().is_empty());
            let json = serde_json::to_string(&surface).unwrap();
            assert!(json.contains(surface.as_str()), "{json}");
        }
    }

    #[test]
    fn the_report_round_trips_through_json() {
        let blobs = vec![BlobSource {
            commit: "abcdef1234567890".to_string(),
            path: "old.json".to_string(),
            bytes: AWS_KEY.as_bytes().to_vec(),
        }];
        let report = scan_history(&blobs);
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            serde_json::from_str::<BackupSecretScan>(&json).unwrap(),
            report
        );
    }

    #[test]
    fn a_private_key_block_is_a_high_confidence_finding() {
        let blobs = vec![BlobSource {
            commit: "abcdef1234567890".to_string(),
            path: "id_rsa".to_string(),
            bytes: b"-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA\n-----END RSA PRIVATE KEY-----\n".to_vec(),
        }];
        let report = scan_history(&blobs);
        assert!(!report.is_clean());
        assert!(report.high_confidence_count() >= 1);
    }
}
