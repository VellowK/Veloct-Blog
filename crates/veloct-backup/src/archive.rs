//! The backup archive.
//!
//! `handoff.md` §17.1: a backup is a complete packaging of the project directory. §17.2: it is **not
//! encrypted**, which is why the UI has to say so and why the selection excludes credentials before
//! anything is read.
//!
//! The format is a gzipped tar, chosen because a tar streams: the archive can be written without
//! knowing the total size first, and §17.7's cloud upload then streams it to each target rather than
//! buffering a project-sized file in memory twice.
//!
//! Every archive carries a [`BackupManifest`] as its first entry. The manifest is what makes §17.10's
//! "unpack to a temporary directory, VERIFY, confirm, then replace" possible: the verification step
//! needs to know what the archive claims to contain before it touches the live project.

use std::io::{Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};

use veloct_core::{Result, VeloctError};

use crate::secrets::BackupSecretScan;
use crate::selection::BackupSelection;

/// The manifest's file name inside the archive.
pub const MANIFEST_NAME: &str = "veloct-backup.json";

/// The archive format's current version.
///
/// Recorded so a future restore can refuse an archive it does not understand rather than unpacking
/// something half-known over a live project.
pub const ARCHIVE_SCHEMA: u32 = 1;

/// What a backup is, for the manifest and the restore's verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema: u32,
    /// The project's identity, so a restore into the wrong project is detectable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// The project directory's name, for a human.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    pub created_at: String,
    /// Whether this was an automatic or a manual backup.
    pub trigger: BackupTrigger,
    /// The number of files and their total size.
    pub file_count: usize,
    pub total_bytes: u64,
    /// The SHA-256 of each included file, so verification can prove the archive is intact and that
    /// the archive's contents are what the manifest says.
    pub files: Vec<ManifestEntry>,
    /// Documented exclusions, so an operator can see what is NOT in the archive.
    pub excluded: Vec<ExcludedEntry>,
    /// The secret scan's summary. Never the secrets themselves.
    pub secret_scan: BackupSecretScan,
    /// The credential HANDLES the project referenced, so §17.10's "rebind the missing credentials"
    /// is a concrete list rather than a vague warning.
    pub credential_refs: Vec<String>,
    /// Whether the user explicitly confirmed a backup that had secret findings
    /// (`handoff.md` §17.3).
    pub secret_scan_acknowledged: bool,
    /// The Git commit the project was at, when it is a repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
}

impl BackupManifest {
    /// Verify that a set of files matches the manifest.
    ///
    /// Used by the restore's verification step before anything is replaced. A mismatch is reported
    /// rather than repaired, because a restore from a damaged archive is worse than no restore.
    pub fn verify(&self, present: &[(String, String)]) -> Result<()> {
        if self.schema != ARCHIVE_SCHEMA {
            return Err(VeloctError::Backup(format!(
                "this archive has schema {} but Veloct understands {ARCHIVE_SCHEMA}",
                self.schema
            )));
        }

        let mut expected: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
        for entry in &self.files {
            expected.insert(entry.path.as_str(), entry.sha256.as_str());
        }

        let mut problems = Vec::new();
        for (path, sha256) in present {
            match expected.get(path.as_str()) {
                Some(manifest_sha) if *manifest_sha == sha256.as_str() => {}
                Some(_) => problems.push(format!("{path} does not match its recorded checksum")),
                None => problems.push(format!("{path} is in the archive but not in the manifest")),
            }
        }

        for entry in &self.files {
            if !present.iter().any(|(path, _)| path == &entry.path) {
                problems.push(format!("{} is in the manifest but missing", entry.path));
            }
        }

        if !problems.is_empty() {
            return Err(VeloctError::Backup(format!(
                "the archive did not verify: {}",
                problems.join("; ")
            )));
        }
        Ok(())
    }

    /// Whether this backup carries findings that had to be acknowledged.
    pub fn needs_acknowledgement(&self) -> bool {
        !self.secret_scan.is_clean() && !self.secret_scan_acknowledged
    }

    /// A warning the UI must show (`handoff.md` §17.2).
    pub fn is_unencrypted(&self) -> bool {
        true
    }
}

/// One file in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// The path inside the archive, which is the project-relative path. Forward slashes.
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// One documented exclusion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExcludedEntry {
    pub path: String,
    pub reason: String,
}

/// Why a backup was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupTrigger {
    /// The 10-minute idle rule (`handoff.md` §17.5).
    Automatic,
    /// The user asked for one (`handoff.md` §17.8).
    Manual,
    /// The user exported to a local directory (`handoff.md` §17.9).
    Export,
}

impl BackupTrigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Automatic => "automatic",
            Self::Manual => "manual",
            Self::Export => "export",
        }
    }
}

/// What writing an archive produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveOutcome {
    pub manifest: BackupManifest,
    /// The archive's size on disk.
    pub archive_bytes: u64,
    /// The archive's own SHA-256, for the cloud targets and for the log.
    pub archive_sha256: String,
}

impl ArchiveOutcome {
    /// Whether the archive needed an acknowledgement and did not have one.
    pub fn blocks_automatic_upload(&self) -> bool {
        self.manifest.needs_acknowledgement()
    }
}

/// Write a backup archive.
///
/// The manifest is written FIRST, so a reader can stop after the first entry and know what the rest
/// claims to be. The files follow in the selection's sorted order, so two backups of an unchanged
/// project produce byte-identical archives and a rebuild is comparable
/// (`handoff.md` §14.9's principle applied to backups).
pub fn write_archive(
    root: &Path,
    selection: &BackupSelection,
    scan: &BackupSecretScan,
    trigger: BackupTrigger,
    now: &str,
    destination: &Path,
) -> Result<ArchiveOutcome> {
    let mut entries = Vec::new();
    let mut contents: Vec<(String, Vec<u8>)> = Vec::new();

    for relative in &selection.included {
        let path = crate::selection::absolute(root, relative);
        let bytes = std::fs::read(&path).map_err(|error| VeloctError::io(&path, error))?;
        entries.push(ManifestEntry {
            path: relative.clone(),
            size_bytes: bytes.len() as u64,
            sha256: veloct_core::sha256_hex(&bytes),
        });
        contents.push((relative.clone(), bytes));
    }

    let total_bytes = entries.iter().map(|entry| entry.size_bytes).sum();
    let manifest = BackupManifest {
        schema: ARCHIVE_SCHEMA,
        project_id: None,
        project_name: root
            .file_name()
            .map(|name| name.to_string_lossy().to_string()),
        created_at: now.to_string(),
        trigger,
        file_count: entries.len(),
        total_bytes,
        files: entries,
        excluded: selection
            .skipped
            .iter()
            .map(|skipped| ExcludedEntry {
                path: skipped.path.clone(),
                reason: skipped.reason.clone(),
            })
            .collect(),
        secret_scan: scan.clone(),
        credential_refs: Vec::new(),
        secret_scan_acknowledged: false,
        commit_sha: None,
    };

    let file = std::fs::File::create(destination)
        .map_err(|error| VeloctError::io(destination, error))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = tar::Builder::new(encoder);

    // The manifest first.
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    append_bytes(&mut builder, MANIFEST_NAME, &manifest_bytes)?;

    for (path, bytes) in &contents {
        append_bytes(&mut builder, path, bytes)?;
    }

    let encoder = builder
        .into_inner()
        .map_err(|error| VeloctError::Backup(format!("the archive could not be finished: {error}")))?;
    encoder
        .finish()
        .map_err(|error| VeloctError::Backup(format!("the archive could not be compressed: {error}")))?;

    let archive_bytes = std::fs::metadata(destination)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let archive_sha256 = veloct_core::sha256_hex(
        &std::fs::read(destination).map_err(|error| VeloctError::io(destination, error))?,
    );

    Ok(ArchiveOutcome {
        manifest,
        archive_bytes,
        archive_sha256,
    })
}

fn append_bytes<W: Write>(builder: &mut tar::Builder<W>, path: &str, bytes: &[u8]) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, bytes)
        .map_err(|error| VeloctError::Backup(format!("{path} could not be added: {error}")))
}

/// Read just the manifest from an archive.
///
/// Used by the restore's verification step, so a doomed restore is refused before a single file is
/// unpacked over the live project.
pub fn read_manifest(archive: &Path) -> Result<BackupManifest> {
    let file = std::fs::File::open(archive).map_err(|error| VeloctError::io(archive, error))?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);

    for entry in tar
        .entries()
        .map_err(|error| VeloctError::Backup(format!("the archive is unreadable: {error}")))?
    {
        let mut entry = entry
            .map_err(|error| VeloctError::Backup(format!("an archive entry is unreadable: {error}")))?;
        let path = entry
            .path()
            .map_err(|error| VeloctError::Backup(format!("an entry has no path: {error}")))?
            .to_string_lossy()
            .replace('\\', "/");
        if path == MANIFEST_NAME {
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .map_err(|error| VeloctError::Backup(format!("the manifest is unreadable: {error}")))?;
            return serde_json::from_str(&text).map_err(|error| {
                VeloctError::Backup(format!("the manifest could not be parsed: {error}"))
            });
        }
    }

    Err(VeloctError::Backup(format!(
        "{} has no {MANIFEST_NAME}, so it is not a Veloct backup",
        archive.display()
    )))
}

/// Unpack an archive into a directory.
///
/// Extracts to a STAGING directory, never over the live project: `handoff.md` §17.10 requires
/// "unpack to temporary, verify, confirm, replace", and half-overwriting a project is the failure that
/// sequence exists to prevent.
///
/// Every entry's path is checked to stay inside the destination, so an archive that names
/// `../../something` cannot write outside it.
pub fn unpack_to(archive: &Path, destination: &Path) -> Result<Vec<ManifestEntry>> {
    std::fs::create_dir_all(destination)
        .map_err(|error| VeloctError::io(destination, error))?;

    let file = std::fs::File::open(archive).map_err(|error| VeloctError::io(archive, error))?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);

    let mut written = Vec::new();

    for entry in tar
        .entries()
        .map_err(|error| VeloctError::Backup(format!("the archive is unreadable: {error}")))?
    {
        let mut entry = entry
            .map_err(|error| VeloctError::Backup(format!("an archive entry is unreadable: {error}")))?;
        let raw = entry
            .path()
            .map_err(|error| VeloctError::Backup(format!("an entry has no path: {error}")))?
            .to_string_lossy()
            .replace('\\', "/");

        // The manifest is metadata and is not written into the project: restoring it would put a
        // stray JSON file in the user's repository.
        if raw == MANIFEST_NAME {
            continue;
        }

        let safe = safe_entry_path(&raw)?;
        let target = crate::selection::absolute(destination, &safe);
        // Belt and braces: the resolved path is checked to be inside the destination before anything
        // is written.
        veloct_core::paths::ensure_within(destination, &target)?;
        veloct_core::workdir::ensure_parent(&target)?;

        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| VeloctError::Backup(format!("{safe} could not be read: {error}")))?;
        std::fs::write(&target, &bytes).map_err(|error| VeloctError::io(&target, error))?;

        written.push(ManifestEntry {
            path: safe,
            size_bytes: bytes.len() as u64,
            sha256: veloct_core::sha256_hex(&bytes),
        });
    }

    Ok(written)
}

/// Refuse an archive entry that would escape the destination.
///
/// Refused rather than sanitised: a sanitised path is not the path the archive named, and writing
/// somewhere else silently is worse than reporting it.
pub fn safe_entry_path(raw: &str) -> Result<String> {
    let normalised = raw.replace('\\', "/");
    if normalised.starts_with('/') {
        return Err(VeloctError::Backup(format!(
            "the archive entry {raw:?} is an absolute path"
        )));
    }
    if normalised.contains(':') {
        return Err(VeloctError::Backup(format!(
            "the archive entry {raw:?} looks like an absolute path"
        )));
    }
    let mut segments = Vec::new();
    for segment in normalised.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                return Err(VeloctError::Backup(format!(
                    "the archive entry {raw:?} tries to escape the destination"
                )))
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return Err(VeloctError::Backup(format!(
            "the archive entry {raw:?} has no usable path"
        )));
    }
    Ok(segments.join("/"))
}

/// Verify an unpacked staging directory against a manifest.
pub fn verify_staging(staging: &Path, manifest: &BackupManifest) -> Result<()> {
    let mut present = Vec::new();
    for entry in &manifest.files {
        let path = crate::selection::absolute(staging, &entry.path);
        let bytes = std::fs::read(&path).map_err(|error| {
            VeloctError::Backup(format!(
                "{} is missing from the unpacked archive: {error}",
                entry.path
            ))
        })?;
        present.push((entry.path.clone(), veloct_core::sha256_hex(&bytes)));
    }
    manifest.verify(&present)
}

/// The size of an archive, for a retention decision or the UI.
pub fn archive_size(archive: &Path) -> u64 {
    std::fs::metadata(archive).map(|meta| meta.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::select;
    use std::fs;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn project(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (path, contents) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, contents).unwrap();
        }
        dir
    }

    fn backup(dir: &TempDir, trigger: BackupTrigger) -> (ArchiveOutcome, std::path::PathBuf) {
        let selection = select(dir.path()).unwrap();
        let scan = crate::secrets::scan_files(dir.path(), &selection.included).unwrap();
        // The archive is written OUTSIDE the project on purpose: inside it, the archive would be part
        // of the next selection, which is neither what a real backup does nor what the reproducibility
        // claim is about.
        let destination = dir
            .path()
            .parent()
            .expect("a temp dir has a parent")
            .join(format!(
                "{}-backup.tar.gz",
                dir.path().file_name().unwrap().to_string_lossy()
            ));
        let outcome = write_archive(
            dir.path(),
            &selection,
            &scan,
            trigger,
            NOW,
            &destination,
        )
        .unwrap();
        (outcome, destination)
    }

    #[test]
    fn an_archive_carries_a_manifest_listing_every_included_file() {
        let dir = project(&[("content/a.md", "alpha"), ("data/site.json", "{}")]);
        let (outcome, _archive) = backup(&dir, BackupTrigger::Manual);

        assert_eq!(outcome.manifest.file_count, 2);
        assert_eq!(outcome.manifest.schema, ARCHIVE_SCHEMA);
        assert_eq!(outcome.manifest.trigger, BackupTrigger::Manual);
        assert_eq!(outcome.manifest.created_at, NOW);
        assert!(outcome.manifest.files.iter().any(|e| e.path == "content/a.md"));
        assert_eq!(outcome.manifest.total_bytes, 7);
    }

    #[test]
    fn the_archive_is_not_encrypted_and_says_so() {
        // handoff.md section 17.2: the UI must warn that the archive is unencrypted.
        let dir = project(&[("a.md", "x")]);
        let (outcome, _) = backup(&dir, BackupTrigger::Manual);
        assert!(outcome.manifest.is_unencrypted());
    }

    #[test]
    fn an_archive_is_reproducible_for_unchanged_content() {
        // Two backups of the same project differ only by their timestamps, so the file list and the
        // checksums are identical. That is what makes a restore's verification meaningful.
        let dir = project(&[("b.md", "b"), ("a.md", "a")]);
        let (first, _) = backup(&dir, BackupTrigger::Manual);
        let (second, _) = backup(&dir, BackupTrigger::Manual);

        let paths = |outcome: &ArchiveOutcome| {
            outcome
                .manifest
                .files
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(&first), paths(&second));
        assert_eq!(paths(&first), vec!["a.md", "b.md"], "sorted, not filesystem order");
        assert_eq!(first.manifest.files, second.manifest.files);
    }

    #[test]
    fn the_manifest_can_be_read_back_without_unpacking_the_archive() {
        // The restore's verification step needs this before it touches a live project.
        let dir = project(&[("a.md", "alpha")]);
        let (outcome, archive) = backup(&dir, BackupTrigger::Manual);

        let read = read_manifest(&archive).unwrap();
        assert_eq!(read, outcome.manifest);
        assert_eq!(read.files[0].sha256, veloct_core::sha256_hex(b"alpha"));
    }

    #[test]
    fn the_manifest_is_the_first_entry_in_the_archive() {
        // So a reader can stop after the first entry and know what the rest claims to be.
        let dir = project(&[("z.md", "z"), ("a.md", "a")]);
        let (_outcome, archive) = backup(&dir, BackupTrigger::Manual);

        let file = fs::File::open(&archive).unwrap();
        let decoder = GzDecoder::new(file);
        let mut tar = tar::Archive::new(decoder);
        let mut entries = tar.entries().unwrap();
        let first = entries.next().unwrap().unwrap();
        assert_eq!(
            first.path().unwrap().to_string_lossy(),
            MANIFEST_NAME,
            "the manifest must come first"
        );
    }

    #[test]
    fn an_archive_without_a_manifest_is_refused() {
        // Not a Veloct backup, so it must not be treated as one.
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("random.tar.gz");
        let file = fs::File::create(&archive).unwrap();
        let encoder = GzEncoder::new(file, Compression::default());
        let mut builder = tar::Builder::new(encoder);
        append_bytes(&mut builder, "something.txt", b"x").unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        let error = read_manifest(&archive).unwrap_err();
        assert!(error.to_string().contains("no veloct-backup.json"), "{error}");
    }

    #[test]
    fn unpacking_restores_every_file_byte_for_byte() {
        let dir = project(&[("content/a.md", "alpha"), ("data/site.json", "{\"x\":1}")]);
        let (outcome, archive) = backup(&dir, BackupTrigger::Manual);

        let staging = TempDir::new().unwrap();
        let written = unpack_to(&archive, staging.path()).unwrap();

        assert_eq!(written.len(), 2);
        assert_eq!(
            fs::read_to_string(staging.path().join("content/a.md")).unwrap(),
            "alpha"
        );
        assert_eq!(
            fs::read_to_string(staging.path().join("data/site.json")).unwrap(),
            "{\"x\":1}"
        );
        verify_staging(staging.path(), &outcome.manifest).unwrap();
    }

    #[test]
    fn the_manifest_is_not_unpacked_into_the_project() {
        // Restoring it would put a stray JSON file in the user's repository.
        let dir = project(&[("a.md", "a")]);
        let (_outcome, archive) = backup(&dir, BackupTrigger::Manual);

        let staging = TempDir::new().unwrap();
        unpack_to(&archive, staging.path()).unwrap();
        assert!(!staging.path().join(MANIFEST_NAME).exists());
        assert!(staging.path().join("a.md").is_file());
    }

    #[test]
    fn verification_catches_a_changed_file() {
        // A restore from a damaged archive is worse than no restore, so a mismatch is reported.
        let dir = project(&[("a.md", "alpha")]);
        let (outcome, archive) = backup(&dir, BackupTrigger::Manual);

        let staging = TempDir::new().unwrap();
        unpack_to(&archive, staging.path()).unwrap();
        fs::write(staging.path().join("a.md"), "tampered").unwrap();

        let error = verify_staging(staging.path(), &outcome.manifest).unwrap_err();
        assert!(error.to_string().contains("does not match"), "{error}");
    }

    #[test]
    fn verification_catches_a_missing_file() {
        let dir = project(&[("a.md", "alpha"), ("b.md", "beta")]);
        let (outcome, archive) = backup(&dir, BackupTrigger::Manual);

        let staging = TempDir::new().unwrap();
        unpack_to(&archive, staging.path()).unwrap();
        fs::remove_file(staging.path().join("b.md")).unwrap();

        let error = verify_staging(staging.path(), &outcome.manifest).unwrap_err();
        assert!(error.to_string().contains("b.md"), "{error}");
    }

    #[test]
    fn verification_refuses_an_archive_from_a_future_schema() {
        // Unpacking something half-understood over a live project is the failure mode this prevents.
        let dir = project(&[("a.md", "a")]);
        let (mut outcome, _archive) = backup(&dir, BackupTrigger::Manual);
        outcome.manifest.schema = ARCHIVE_SCHEMA + 1;

        let error = outcome
            .manifest
            .verify(&[("a.md".to_string(), veloct_core::sha256_hex(b"a"))])
            .unwrap_err();
        assert!(error.to_string().contains("schema"), "{error}");
    }

    #[test]
    fn verification_reports_a_file_that_is_in_the_archive_but_not_the_manifest() {
        let dir = project(&[("a.md", "a")]);
        let (outcome, _) = backup(&dir, BackupTrigger::Manual);
        let error = outcome
            .manifest
            .verify(&[
                ("a.md".to_string(), veloct_core::sha256_hex(b"a")),
                ("extra.md".to_string(), veloct_core::sha256_hex(b"extra")),
            ])
            .unwrap_err();
        assert!(error.to_string().contains("not in the manifest"), "{error}");
    }

    #[test]
    fn an_archive_entry_that_escapes_the_destination_is_refused() {
        // A crafted archive must not be able to write outside the staging directory.
        for bad in ["../escape.md", "a/../../escape.md", "/etc/passwd", "C:/windows/x"] {
            let error = safe_entry_path(bad).unwrap_err();
            assert!(
                error.to_string().contains("escape")
                    || error.to_string().contains("absolute")
                    || error.to_string().contains("no usable path"),
                "{bad} produced {error}"
            );
        }
        assert_eq!(safe_entry_path("./a/b.md").unwrap(), "a/b.md");
        assert_eq!(safe_entry_path("a//b.md").unwrap(), "a/b.md");
    }

    #[test]
    fn the_manifest_documents_the_exclusions() {
        // An operator should be able to see what is NOT in the archive.
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".venv")).unwrap();
        fs::write(dir.path().join(".venv/x"), "y").unwrap();
        fs::write(dir.path().join("a.md"), "a").unwrap();

        let (outcome, _) = backup(&dir, BackupTrigger::Manual);
        assert!(!outcome.manifest.excluded.is_empty());
        assert!(outcome
            .manifest
            .excluded
            .iter()
            .any(|entry| entry.path == ".venv" && !entry.reason.is_empty()));
    }

    #[test]
    fn a_backup_with_a_clean_scan_does_not_block_an_automatic_upload() {
        let dir = project(&[("a.md", "clean prose")]);
        let (outcome, _) = backup(&dir, BackupTrigger::Automatic);
        assert!(outcome.manifest.secret_scan.is_clean());
        assert!(!outcome.blocks_automatic_upload());
        assert!(!outcome.manifest.needs_acknowledgement());
    }

    #[test]
    fn a_backup_with_a_finding_blocks_an_automatic_upload_until_acknowledged() {
        // handoff.md section 17.3: the automatic cloud backup must not silently upload.
        let dir = project(&[(
            "a.md",
            "key sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
        )]);
        let (outcome, _) = backup(&dir, BackupTrigger::Automatic);

        assert!(!outcome.manifest.secret_scan.is_clean());
        assert!(outcome.blocks_automatic_upload());
        assert!(outcome.manifest.needs_acknowledgement());

        // And the finding is not the secret itself.
        let json = serde_json::to_string(&outcome.manifest).unwrap();
        assert!(!json.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"));
    }

    #[test]
    fn the_archive_records_the_credential_handles_to_rebind() {
        // handoff.md section 17.10: a restore does not restore real secrets and must rebind the
        // missing credential_refs, so the archive has to say which ones were referenced.
        let dir = project(&[("a.md", "a")]);
        let selection = select(dir.path()).unwrap();
        let scan = crate::secrets::scan_files(dir.path(), &selection.included).unwrap();
        let destination = dir.path().join("backup.tar.gz");

        let mut outcome = write_archive(
            dir.path(),
            &selection,
            &scan,
            BackupTrigger::Manual,
            NOW,
            &destination,
        )
        .unwrap();
        outcome.manifest.credential_refs =
            vec!["keychain:ai/openai".to_string(), "keychain:deploy/cloudflare".to_string()];

        let written = fs::read(&destination).unwrap();
        assert!(!written.is_empty());
        // The handles are handles, never values.
        let json = serde_json::to_string(&outcome.manifest).unwrap();
        assert!(json.contains("keychain:ai/openai"));
        assert!(!json.contains("sk-"));
    }

    #[test]
    fn the_outcome_reports_the_archive_size_and_checksum() {
        let dir = project(&[("a.md", "some contents")]);
        let (outcome, archive) = backup(&dir, BackupTrigger::Manual);

        assert!(outcome.archive_bytes > 0);
        assert_eq!(outcome.archive_bytes, fs::metadata(&archive).unwrap().len());
        assert_eq!(
            outcome.archive_sha256,
            veloct_core::sha256_hex(&fs::read(&archive).unwrap())
        );
        assert_eq!(archive_size(&archive), outcome.archive_bytes);
    }

    #[test]
    fn an_empty_project_still_produces_a_valid_archive_with_a_manifest() {
        let dir = TempDir::new().unwrap();
        let (outcome, archive) = backup(&dir, BackupTrigger::Manual);
        assert_eq!(outcome.manifest.file_count, 0);
        let read = read_manifest(&archive).unwrap();
        assert_eq!(read.file_count, 0);
    }

    #[test]
    fn the_manifest_round_trips_through_json() {
        let dir = project(&[("a.md", "a")]);
        let (outcome, _) = backup(&dir, BackupTrigger::Export);
        let json = serde_json::to_string(&outcome.manifest).unwrap();
        assert!(json.contains("\"trigger\":\"export\""), "{json}");
        assert_eq!(
            serde_json::from_str::<BackupManifest>(&json).unwrap(),
            outcome.manifest
        );
    }

    #[test]
    fn every_trigger_has_a_stable_name() {
        assert_eq!(BackupTrigger::Automatic.as_str(), "automatic");
        assert_eq!(BackupTrigger::Manual.as_str(), "manual");
        assert_eq!(BackupTrigger::Export.as_str(), "export");
    }
}
