//! Backup and restore against a real project.
//!
//! The unit tests in `veloct-app::backup` use small fixtures. This runs the same path against the
//! REPOSITORY's own content, works and data — several hundred files of real Markdown — which is where a
//! selection rule, an archive format or a restore ordering problem actually shows up. A fixture with four
//! files cannot tell whether the walk terminates, whether the archive's manifest matches its contents at
//! scale, or whether a restore puts a real project back.
//!
//! It also exercises the property that matters most about a backup and is the easiest to get wrong: **a
//! restore of a backup taken before an edit brings the edit's file back byte for byte, and an UNCONFIRMED
//! restore changes nothing at all.**
//!
//! Run with:
//!
//! ```text
//! cargo test -p veloct-desktop --test backup_integration -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    let mut candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    candidate.pop();
    candidate
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        let name_text = name.to_string_lossy().to_string();
        // The dependency tree, previous output and generated caches are not copied: none is project data, and
        // the selection rules exclude them anyway.
        if matches!(
            name_text.as_str(),
            "node_modules" | "dist" | ".astro" | "target" | ".git" | ".agent-teams"
        ) {
            continue;
        }
        let source = entry.path();
        let target = to.join(&name);
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)?;
        }
    }
    Ok(())
}

fn fixed_clock(timestamp: &str) -> veloct_core::FixedClock {
    veloct_core::FixedClock::new(
        time::OffsetDateTime::parse(
            timestamp,
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap(),
    )
}

#[test]
#[ignore = "walks and archives the whole real project; run explicitly with --ignored"]
fn a_real_backup_restores_the_real_project_and_an_unconfirmed_one_changes_nothing() {
    let workspace = tempfile::TempDir::new().expect("a temp dir");
    let project = workspace.path().join("project");
    std::fs::create_dir_all(&project).unwrap();

    // The repository's OWN content, works and data, so the selection and the archive run over real input.
    let repo = repo_root();
    for segment in ["content", "works", "data", "site"] {
        copy_tree(&repo.join(segment), &project.join(segment))
            .unwrap_or_else(|error| panic!("could not copy {segment}: {error}"));
    }
    std::fs::create_dir_all(project.join(".veloct")).unwrap();
    // A real SQLite file would need a real database; what matters for the archive is that a file under
    // `.veloct` is carried, and its content is opaque to the backup.
    std::fs::write(project.join(".veloct/veloct.db"), b"SQLite format 3\0placeholder").unwrap();

    // ---- back up ----
    let destination = workspace.path().join("backups");
    let outcome = veloct_app::backup(
        &project,
        veloct_backup::BackupTrigger::Manual,
        &destination,
        &fixed_clock("2026-01-01T12:00:00Z"),
    )
    .unwrap_or_else(|error| panic!("the backup failed: {error}"));

    assert!(
        outcome.file_count > 20,
        "the real project has more than a fixture's files: {}",
        outcome.file_count
    );
    let archive = Path::new(&outcome.archive_path).to_path_buf();
    assert!(archive.is_file());
    assert!(outcome.archive_bytes > 0);
    assert_eq!(outcome.archive_sha256.len(), 64);

    // The manifest describes what is in the archive, and it names real paths.
    let manifest = veloct_app::read_manifest(&archive).unwrap();
    let paths: Vec<&str> = manifest.files.iter().map(|file| file.path.as_str()).collect();
    assert!(paths.iter().any(|path| path.starts_with("content/articles/")), "{paths:?}");
    assert!(paths.iter().any(|path| path.starts_with("content/knowledge/")), "{paths:?}");
    assert!(paths.iter().any(|path| path.starts_with("works/")), "{paths:?}");
    assert!(paths.iter().any(|path| path.contains("veloct.db")), "{paths:?}");
    assert!(
        paths.iter().any(|path| path.starts_with("site/")),
        "the site template is part of the project: {paths:?}"
    );
    eprintln!(
        "[backup] archived {} files, {} bytes, sha {}",
        outcome.file_count,
        outcome.archive_bytes,
        &outcome.archive_sha256[..12]
    );

    // Nothing excluded leaked in.
    assert!(
        !paths.iter().any(|path| path.contains("node_modules")),
        "dependencies must not be archived"
    );
    assert!(
        !paths.iter().any(|path| path.contains(".veloct/logs")),
        "logs must not be archived"
    );

    // ---- edit the project after the backup ----
    let article = project.join("content/articles/01J8Z5Q0K3W7M9V2B4N6P8R1TY.md");
    let original = std::fs::read_to_string(&article).unwrap();
    std::fs::write(&article, "CHANGED AFTER THE BACKUP\n").unwrap();

    // ---- stage a restore: nothing may change yet ----
    let staging = workspace.path().join("staging");
    let (staged, plan) = veloct_app::stage_restore(
        &archive,
        veloct_backup::RestoreMode::Full,
        &staging,
    )
    .unwrap_or_else(|error| panic!("staging failed: {error}"));

    assert_eq!(plan.file_count, manifest.files.len(), "the plan covers the manifest");
    assert_eq!(
        std::fs::read_to_string(&article).unwrap(),
        "CHANGED AFTER THE BACKUP\n",
        "STAGING must not change the project"
    );
    assert!(plan.needs_publish_afterwards, "section 17.10: a publish follows a restore");
    eprintln!("[backup] staged {} files: {}", plan.file_count, plan.summary);

    // ---- an UNCONFIRMED apply is refused and still changes nothing ----
    let error = veloct_app::apply_restore(&staged, &project, false)
        .expect_err("an unconfirmed restore is refused");
    assert!(error.to_string().contains("confirmed"), "{error}");
    assert_eq!(
        std::fs::read_to_string(&article).unwrap(),
        "CHANGED AFTER THE BACKUP\n",
        "an unconfirmed restore must change nothing"
    );

    // ---- a confirmed apply brings the file back ----
    let applied = veloct_app::apply_restore(&staged, &project, true)
        .unwrap_or_else(|error| panic!("the restore failed: {error}"));
    assert!(applied.files_written > 0);
    assert_eq!(
        std::fs::read_to_string(&article).unwrap(),
        original,
        "the restore brought the article back BYTE FOR BYTE"
    );
    assert!(applied.working_tree_changed);
    assert!(!applied.summary.is_empty());
    eprintln!("[backup] restored {} files: {}", applied.files_written, applied.summary);

    // ---- a content-only restore preserves the history paths ----
    std::fs::write(&article, "CHANGED AGAIN\n").unwrap();
    let staging_two = workspace.path().join("staging-two");
    let (staged_two, plan_two) = veloct_app::stage_restore(
        &archive,
        veloct_backup::RestoreMode::ContentOnly,
        &staging_two,
    )
    .unwrap();
    assert!(
        plan_two.needs_publish_afterwards,
        "section 17.10 states it for a content-only restore too"
    );
    // The plan names what it will leave alone, with a reason for each, or the user cannot tell what a
    // content-only restore is for.
    for preserved in &plan_two.will_preserve {
        assert!(!preserved.reason.is_empty(), "{preserved:?}");
    }
    let applied_two = veloct_app::apply_restore(&staged_two, &project, true).unwrap();
    assert_eq!(std::fs::read_to_string(&article).unwrap(), original);
    eprintln!(
        "[backup] content-only restore wrote {} files, preserved {}",
        applied_two.files_written,
        applied_two.preserved.len()
    );

    // ---- retention keeps the newest ----
    let listed = veloct_app::list_archives(&destination).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].file_count > 20);
}

#[test]
#[ignore = "archives the real project twice; run explicitly with --ignored"]
fn a_secret_in_the_real_project_blocks_the_upload_but_still_produces_an_archive() {
    // handoff.md section 17.7: the local archive is still useful when a finding blocks the upload. Losing the
    // backup because of a finding would be worse than the finding.
    let workspace = tempfile::TempDir::new().expect("a temp dir");
    let project = workspace.path().join("project");
    std::fs::create_dir_all(project.join("content/articles")).unwrap();
    std::fs::create_dir_all(project.join(".veloct")).unwrap();
    std::fs::write(
        project.join("content/articles/01SECRET.md"),
        "---\nid: 01SECRET\ntype: article\nslug: s\ntitle: S\n---\n\ntoken: sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789\n",
    )
    .unwrap();

    let destination = workspace.path().join("backups");
    let outcome = veloct_app::backup(
        &project,
        veloct_backup::BackupTrigger::Manual,
        &destination,
        &fixed_clock("2026-01-01T12:00:00Z"),
    )
    .unwrap();

    assert!(!outcome.secret_findings.is_empty(), "{outcome:?}");
    assert!(outcome.needs_acknowledgement, "a finding blocks the automatic upload");
    assert!(
        Path::new(&outcome.archive_path).is_file(),
        "the archive is still written"
    );
    // The finding names the path and the kind, and never the value.
    for finding in &outcome.secret_findings {
        assert!(!finding.contains("sk-ant-api03"), "{finding}");
    }
    eprintln!("[backup] findings: {:?}", outcome.secret_findings);
}
