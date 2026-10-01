//! The works commands.
//!
//! `handoff.md` §10 is the works system, and until this module existed the whole of it was
//! reachable only from `veloct-content`'s own tests: `import_work`, `list_works` and `export_work`
//! had no caller above the crate, so a user could not import a work from the window, could not see
//! one, and could not give it a poster image. That is PROCESSING.md §5.9 / H10, and it is the gap
//! these commands close.
//!
//! Three choices are deliberate and easy to undo by accident:
//!
//! * **A work's truth source is a file, and these commands write files.** §10.1 makes
//!   `works/<stable-id>/work.json` and the sibling `site/` the truth and SQLite only an index, so
//!   there is no connection in this module and no row to keep in step.
//! * **An import is planned before it is applied.** `plan_work_import` writes nothing, so the
//!   window can show the entry file, the file count, the size warning and the directories that will
//!   be skipped, and a user can decline.
//! * **A poster image is copied, not downloaded or generated.** §10.1's `preview/` belongs to the
//!   work, and Veloct has no headless browser to screenshot a work with. The command therefore
//!   stores the file the user chose and says so in its own doc, rather than pretending to have
//!   captured something.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use veloct_app::CommandError;
use veloct_app::works::{
    self, WorkImportOutcomeView, WorkImportPlanView, WorkView, WorksListView,
};
use veloct_content::works::{ImportOptions, ImportSource, WorkMetadata};
use veloct_core::VeloctError;

use crate::commands::command_invalid;
use crate::state::AppState;

/// The works root of the open project, or the error that says why there is none.
fn works_root(state: &State<'_, AppState>) -> Result<PathBuf, CommandError> {
    let project = state.project()?;
    Ok(project.path().join("works"))
}

/// Every work in the project.
#[tauri::command]
pub fn list_works(state: State<'_, AppState>) -> Result<WorksListView, CommandError> {
    Ok(works::list_works(&works_root(&state)?)?)
}

/// What an import would do, writing nothing.
#[tauri::command]
pub fn plan_work_import(
    state: State<'_, AppState>,
    source_path: String,
    source_kind: String,
    title: String,
    author: String,
) -> Result<WorkImportPlanView, CommandError> {
    let now = state.now();
    let source = import_source(&source_path, &source_kind)?;
    Ok(works::plan_import(
        &works_root(&state)?,
        &source,
        &ImportOptions::new(title, author),
        &now,
    )?)
}

/// Import a work from a ZIP archive or a folder.
#[tauri::command]
pub fn import_work(
    state: State<'_, AppState>,
    source_path: String,
    source_kind: String,
    title: String,
    author: String,
) -> Result<WorkImportOutcomeView, CommandError> {
    let now = state.now();
    let source = import_source(&source_path, &source_kind)?;
    let root = works_root(&state)?;

    let outcome = works::import(
        &root,
        &source,
        &ImportOptions::new(title, author),
        &now,
    )?;

    // The log records what happened to a truth source. A work's body is committed, so "a work was
    // imported" is an event a user will want to find beside the commit that carried it.
    let project = state.project()?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Content, "work imported")
            .with_field("work", &outcome.slug)
            .with_field("files", outcome.files_written.to_string())
            .with_field("bytes", outcome.total_bytes.to_string())
            .with_field("entry", &outcome.work.metadata.entry),
    );

    Ok(outcome)
}

/// Export a work's body as a re-importable ZIP (`handoff.md` §10.5).
#[tauri::command]
pub fn export_work(
    state: State<'_, AppState>,
    id: String,
    destination: String,
) -> Result<usize, CommandError> {
    let written = works::export(
        &works_root(&state)?,
        &id,
        &PathBuf::from(&destination),
    )?;
    Ok(written)
}

/// Replace a work's editable metadata.
#[tauri::command]
pub fn update_work(
    state: State<'_, AppState>,
    id: String,
    metadata: WorkMetadata,
) -> Result<WorkView, CommandError> {
    Ok(works::update_metadata(&works_root(&state)?, &id, metadata)?)
}

/// Remove a work and its body.
#[tauri::command]
pub fn remove_work(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    let project = state.project()?;
    works::remove(&project.path().join("works"), &id)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Content, "work removed")
            .with_field("work", &id),
    );
    Ok(())
}

/// Store a poster image for a work and point its metadata at it.
#[tauri::command]
pub fn set_work_screenshot(
    state: State<'_, AppState>,
    id: String,
    source_path: String,
) -> Result<WorkView, CommandError> {
    Ok(works::set_screenshot(
        &works_root(&state)?,
        &id,
        &PathBuf::from(&source_path),
    )?)
}

/// Where a work's files live, so the window can open the directory rather than guess at it.
#[tauri::command]
pub fn work_directory(state: State<'_, AppState>, id: String) -> Result<String, CommandError> {
    let directory = works::work_directory(&works_root(&state)?, &id)?;
    Ok(directory.to_string_lossy().replace('\\', "/"))
}

/// The maximum poster size the crate will store, so the window can say it before a failed attempt.
#[tauri::command]
pub fn work_screenshot_limit() -> u64 {
    works::MAX_SCREENSHOT_BYTES
}

/// Build an import source from what the window sent.
///
/// The kind is an explicit argument rather than being guessed from the path: a folder whose name
/// ends in `.zip` is a folder, and asking a ZIP reader to open a directory reports the wrong
/// problem.
fn import_source(path: &str, kind: &str) -> Result<ImportSource, CommandError> {
    let path = Path::new(path);
    if path.as_os_str().is_empty() {
        return Err(command_invalid("no source path was given to import"));
    }

    match kind.trim().to_ascii_lowercase().as_str() {
        "zip" => {
            if !path.is_file() {
                return Err(VeloctError::Invalid(format!(
                    "{} is not a file, so it cannot be imported as a ZIP archive",
                    path.display()
                ))
                .into());
            }
            Ok(ImportSource::zip(path))
        }
        "folder" => {
            if !path.is_dir() {
                return Err(VeloctError::Invalid(format!(
                    "{} is not a folder",
                    path.display()
                ))
                .into());
            }
            Ok(ImportSource::folder(path))
        }
        other => Err(command_invalid(format!(
            "{other:?} is not an import kind; expected zip or folder"
        ))),
    }
}

/// One import candidate the window can offer, so a folder is not mistaken for an archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkImportSourceView {
    /// `zip` or `folder`, exactly what the import commands accept back.
    pub kind: String,
    pub path: String,
    pub name: String,
}

/// Decide how a chosen path should be imported.
#[tauri::command]
pub fn describe_work_source(path: String) -> Result<WorkImportSourceView, CommandError> {
    let candidate = Path::new(&path);
    let kind = if candidate.is_dir() {
        "folder"
    } else if candidate.is_file() {
        "zip"
    } else {
        return Err(VeloctError::Invalid(format!(
            "{} does not exist, so there is nothing to import",
            candidate.display()
        ))
        .into());
    };
    Ok(WorkImportSourceView {
        kind: kind.to_string(),
        name: candidate
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: candidate.to_string_lossy().replace('\\', "/"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_folder_is_never_mistaken_for_an_archive() {
        let dir = TempDir::new().unwrap();
        // A directory whose name looks like an archive: the kind is decided by what the path IS,
        // not by how it reads.
        let archive_looking = dir.path().join("demo.zip");
        std::fs::create_dir_all(&archive_looking).unwrap();

        let described = describe_work_source(archive_looking.to_string_lossy().to_string()).unwrap();
        assert_eq!(described.kind, "folder");
        assert_eq!(described.name, "demo.zip");

        // And the explicit kind is checked against the filesystem rather than trusted.
        let error = import_source(&archive_looking.to_string_lossy(), "zip").unwrap_err();
        assert!(error.message.contains("is not a file"), "{}", error.message);
    }

    #[test]
    fn a_file_is_a_zip_source_and_a_relative_kind_is_accepted_case_insensitively() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("demo.zip");
        std::fs::write(&archive, b"not really a zip").unwrap();

        let described = describe_work_source(archive.to_string_lossy().to_string()).unwrap();
        assert_eq!(described.kind, "zip");
        // The kind arrives from the window as it was described, but the command does not depend on
        // the spelling.
        assert!(import_source(&archive.to_string_lossy(), " ZIP ").is_ok());
    }

    #[test]
    fn an_unknown_import_kind_is_refused_rather_than_defaulted() {
        let dir = TempDir::new().unwrap();
        let error = import_source(&dir.path().to_string_lossy(), "tar").unwrap_err();
        assert!(error.message.contains("not an import kind"), "{}", error.message);
        assert_eq!(error.code, "INVALID");
    }

    #[test]
    fn a_path_that_does_not_exist_is_reported_rather_than_treated_as_an_empty_folder() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope");
        let error = describe_work_source(missing.to_string_lossy().to_string()).unwrap_err();
        assert!(error.message.contains("does not exist"), "{}", error.message);
    }

    #[test]
    fn the_poster_limit_is_the_crates_own_number() {
        // The window reads it before an attempt, so it must be the same number the crate enforces.
        assert_eq!(work_screenshot_limit(), veloct_app::MAX_SCREENSHOT_BYTES);
    }
}
