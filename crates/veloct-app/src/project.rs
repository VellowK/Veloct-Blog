//! Project lifecycle.
//!
//! A Veloct project is a directory on disk that Veloct manages; the application itself lives
//! elsewhere (ADR-0001). Creating a project materialises the scaffold described in
//! `docs/spec/01-data-model.md` §1, records its identity in `.veloct/project.json`, and takes a
//! shared project lock while it works.
//!
//! Ownership: `content-app`. The full service surface (content, WIP, works, settings,
//! diagnostics, logs) is built on top of this type.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::workdir::{write_text_if_changed, ProjectRoot, WriteOutcome};
use veloct_core::{layout, ProjectLock, Result, StableId, VeloctError};

/// The identity record stored at `.veloct/project.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    /// Stable id of this project, so a move or rename does not confuse the app.
    pub id: StableId,
    /// Display name.
    pub name: String,
    /// Which bundled site template version the scaffold was created from (ADR-0001).
    pub site_template_version: String,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp of the most recent open.
    pub last_opened_at: String,
}

/// Options accepted when creating a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProjectOptions {
    pub name: String,
    /// Default author for imported works; editable per work afterwards.
    pub default_author: String,
}

impl Default for CreateProjectOptions {
    fn default() -> Self {
        Self {
            name: "My Site".to_string(),
            default_author: "Veloct".to_string(),
        }
    }
}

/// Handle to an opened project.
#[derive(Debug)]
pub struct ProjectService {
    root: ProjectRoot,
    record: ProjectRecord,
    _lock: ProjectLock,
}

impl ProjectService {
    /// Absolute path of the project directory.
    pub fn root(&self) -> &ProjectRoot {
        &self.root
    }

    /// The project identity record.
    pub fn record(&self) -> &ProjectRecord {
        &self.record
    }

    /// Open an existing project. Fails when the directory is not a Veloct project, so the UI
    /// never silently treats an arbitrary folder as one.
    pub fn open(path: impl AsRef<Path>, clock: &dyn veloct_core::Clock) -> Result<Self> {
        let root = ProjectRoot::new(path.as_ref());
        if !root.as_path().is_dir() {
            return Err(VeloctError::ProjectNotFound(root.as_path().to_path_buf()));
        }
        let record_path = root.at(&[layout::VELOCT_DIR, layout::PROJECT_FILE]);
        let raw = veloct_core::workdir::read_text_optional(&record_path)?.ok_or_else(|| {
            VeloctError::ProjectNotFound(root.as_path().to_path_buf())
        })?;
        let mut record: ProjectRecord = serde_json::from_str(&raw)?;

        let lock = ProjectLock::shared(root.as_path(), "open-project")?;
        record.last_opened_at = veloct_core::clock::format_rfc3339(clock.now());
        let serialised = serde_json::to_string_pretty(&record)?;
        // Recording the open is a real write, so it goes through the atomic if-changed writer.
        let _ = write_text_if_changed(&record_path, &serialised)?;

        Ok(Self {
            root,
            record,
            _lock: lock,
        })
    }

    /// Create a new project scaffold in `path`.
    ///
    /// `site_template` is the directory of the bundled Astro template in the application. Its
    /// files are copied so the project is self-contained and reproducible.
    pub fn create(
        path: impl AsRef<Path>,
        options: CreateProjectOptions,
        site_template: &Path,
        site_template_version: &str,
        clock: &dyn veloct_core::Clock,
    ) -> Result<Self> {
        let root = ProjectRoot::new(path.as_ref());
        let veloct_dir = root.veloct_dir();

        if veloct_dir.join(layout::PROJECT_FILE).exists() {
            return Err(VeloctError::Invalid(format!(
                "{} is already a Veloct project",
                root.as_path().display()
            )));
        }

        // Exclusive while the scaffold is written: nothing else may touch a half-built project.
        let lock = ProjectLock::exclusive(root.as_path(), "create-project")?;

        for directory in [
            root.content_dir().join("articles"),
            root.content_dir().join("pages"),
            root.knowledge_dir(),
            root.works_dir(),
            root.dist_dir(),
            root.at(&[layout::DATA_DIR]),
            veloct_dir.join(layout::RELEASES_DIR),
            veloct_dir.join(layout::LOGS_DIR),
            root.at(&[layout::CACHE_DIR]),
        ] {
            std::fs::create_dir_all(&directory)
                .map_err(|e| VeloctError::io(&directory, e))?;
        }

        // The Astro project lives inside the managed project so it is versioned with it.
        if site_template.is_dir() {
            veloct_core::workdir::copy_dir_all(site_template, &root.site_dir(), |path, is_dir| {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                is_dir && matches!(name, "node_modules" | "dist" | ".astro" | ".git")
            })?;
        }

        let now = veloct_core::clock::format_rfc3339(clock.now());
        let record = ProjectRecord {
            id: StableId::new(),
            name: options.name.clone(),
            site_template_version: site_template_version.to_string(),
            created_at: now.clone(),
            last_opened_at: now,
        };
        let record_path = root.at(&[layout::VELOCT_DIR, layout::PROJECT_FILE]);
        std::fs::write(&record_path, serde_json::to_string_pretty(&record)?)
            .map_err(|e| VeloctError::io(&record_path, e))?;

        // Site configuration lives in SQLite; `data/site.json` is the build-time export.
        let site_json = serde_json::json!({
            "name": options.name,
            "default_author": options.default_author,
        });
        let _: WriteOutcome = write_text_if_changed(
            &root.at(&[layout::DATA_DIR, "site.json"]),
            &serde_json::to_string_pretty(&site_json)?,
        )?;

        let gitignore = root.as_path().join(".gitignore");
        write_text_if_changed(
            &gitignore,
            &format!(
                "# Build output is regenerated; the current dist is shipped with backups, not commits.\n{}/{}\n{}/{}\n{}/{}\n",
                layout::DIST_DIR, "",
                layout::CACHE_DIR, "",
                layout::VELOCT_DIR, layout::LOGS_DIR,
            ),
        )?;

        Ok(Self {
            root,
            record,
            _lock: lock,
        })
    }

    /// Path of the SQLite database.
    pub fn db_path(&self) -> PathBuf {
        self.root.db_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::new(time::macros::datetime!(2026-01-01 12:00:00 UTC))
    }

    #[test]
    fn create_materialises_the_documented_scaffold() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(template.join("src")).unwrap();
        std::fs::write(template.join("astro.config.mjs"), "export default {}").unwrap();
        std::fs::create_dir_all(template.join("node_modules")).unwrap();
        std::fs::write(template.join("node_modules").join("junk.js"), "x").unwrap();

        let project_dir = dir.path().join("project");
        let service = ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "test-template",
            &clock(),
        )
        .unwrap();

        assert!(project_dir.join(".veloct").join("project.json").exists());
        assert!(project_dir.join("content").join("articles").is_dir());
        assert!(project_dir.join("knowledge").is_dir());
        assert!(project_dir.join("works").is_dir());
        assert!(project_dir.join("data").join("site.json").exists());
        assert!(project_dir.join(".gitignore").exists());
        // The site template is copied, minus build noise.
        assert!(project_dir.join("site").join("astro.config.mjs").exists());
        assert!(!project_dir.join("site").join("node_modules").exists());

        assert_eq!(service.record().name, "My Site");
        assert_eq!(service.record().site_template_version, "test-template");
        assert!(service.db_path().ends_with(".veloct\\veloct.db")
            || service.db_path().ends_with(".veloct/veloct.db"));
    }

    #[test]
    fn creating_over_an_existing_project_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");

        ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap();

        let error = ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already a Veloct project"));
    }

    #[test]
    fn opening_a_foreign_directory_fails_instead_of_guessing() {
        let dir = tempfile::tempdir().unwrap();
        let error = ProjectService::open(dir.path(), &clock()).unwrap_err();
        assert_eq!(error.code(), "PROJECT_NOT_FOUND");
    }

    #[test]
    fn open_then_reopen_detects_the_project_and_updates_last_opened() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");

        ProjectService::create(
            &project_dir,
            CreateProjectOptions {
                name: "Vellow's Blog".into(),
                default_author: "vellow".into(),
            },
            &template,
            "v1",
            &clock(),
        )
        .unwrap();
        drop(ProjectService::open(&project_dir, &clock()).unwrap());

        let later = FixedClock::new(time::macros::datetime!(2026-02-02 08:00:00 UTC));
        let reopened = ProjectService::open(&project_dir, &later).unwrap();
        assert_eq!(reopened.record().name, "Vellow's Blog");
        assert_eq!(reopened.record().last_opened_at, "2026-02-02T08:00:00Z");
        assert_eq!(reopened.record().created_at, "2026-01-01T12:00:00Z");
    }

    #[test]
    fn creating_a_project_takes_an_exclusive_lock_against_publish() {
        // handoff §20.4: a working-tree-replacing operation must not race a scaffold.
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");
        std::fs::create_dir_all(&project_dir).unwrap();

        let publish = ProjectLock::exclusive(&project_dir, "publish").unwrap();
        let error = ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "EXCLUSIVE_BUSY");
        drop(publish);

        ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap();
    }

    #[test]
    fn open_uses_a_shared_lock_so_editing_and_preview_coexist() {
        let dir = tempfile::tempdir().unwrap();
        let template = dir.path().join("template");
        std::fs::create_dir_all(&template).unwrap();
        let project_dir = dir.path().join("project");
        ProjectService::create(
            &project_dir,
            CreateProjectOptions::default(),
            &template,
            "v1",
            &clock(),
        )
        .unwrap();

        let first = ProjectService::open(&project_dir, &clock()).unwrap();
        let second = ProjectService::open(&project_dir, &clock()).unwrap();
        assert_eq!(first.record().id, second.record().id);

        // But an exclusive operation is refused while they are open.
        let error = ProjectLock::exclusive(&project_dir, "publish").unwrap_err();
        assert_eq!(error.code(), "LOCK_CONFLICT");
    }

    #[test]
    fn project_record_round_trips_through_json() {
        let record = ProjectRecord {
            id: StableId::new(),
            name: "Veloct".into(),
            site_template_version: "1.0.0".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            last_opened_at: "2026-01-02T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<ProjectRecord>(&json).unwrap(), record);
    }

    #[test]
    fn shared_lock_mode_permits_concurrent_readers() {
        // handoff §20.4: editing, preview and attachment upload run in parallel.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let a = ProjectLock::acquire(root, veloct_core::LockMode::Shared, "a").unwrap();
        let b = ProjectLock::acquire(root, veloct_core::LockMode::Shared, "b").unwrap();
        assert!(!ProjectLock::is_exclusive_busy(root));
        drop((a, b));
    }
}
