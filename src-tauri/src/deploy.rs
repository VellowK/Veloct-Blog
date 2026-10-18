//! The deployment commands.
//!
//! `crates/veloct-deploy` has the whole model — the logical targets, the two sub-targets, the Cloudflare
//! adapter, the manual-export adapters, the roll-up that separates "partial" from "failed" — and until this
//! module existed **the application never called it**. §28 names the three logical targets and their
//! sub-targets as a completion requirement.
//!
//! Two rules from the contract are visible in the command surface rather than merely obeyed:
//!
//! * **A deploy does not build.** §14.13's re-send re-uses an existing `dist/` when the fingerprint matches, and
//!   a deploy that rebuilt would move the bytes underneath the fingerprint that authorised it. `deploy_now`
//!   therefore refuses when there is no build, with a message saying to publish first.
//! * **Only an explicit deploy uploads.** EdgeOne uses the official Makers direct-upload protocol;
//!   publishing/building and saving targets do not send deployment requests. 1Panel remains a manual export.

use serde::{Deserialize, Serialize};
use tauri::State;

use veloct_app::{CommandError, DeployOutcomeView, TargetView};

use crate::state::AppState;

/// The configured targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetListView {
    pub enabled: bool,
    pub targets: Vec<TargetView>,
}

/// The targets a project deploys to.
#[tauri::command]
pub fn deploy_targets(state: State<'_, AppState>) -> Result<TargetListView, CommandError> {
    let project = state.project()?;
    let list = veloct_app::TargetList::read(project.root.as_path())?;
    Ok(TargetListView {
        enabled: list.enabled,
        targets: veloct_app::TargetListView::build(&list, &crate::credentials::get_or_none).targets,
    })
}

/// Add or update a target.
///
/// `credential` is written to the OS store and the row keeps only a HANDLE (§12). The parameter is separate
/// from the rest for the same reason the AI provider command's is: there must be no code path that puts a
/// credential in the configuration file.
/// The fields a target save carries.
///
/// A struct rather than eight parameters, so the credential cannot be transposed with a project name — both are
/// `Option<String>` and the mistake would compile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTargetRequest {
    pub id: String,
    pub kind: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub works_project: Option<String>,
    #[serde(default)]
    pub works_origin: Option<String>,
    /// The credential VALUE, written to the OS store. Never persisted in the configuration (section 12.2).
    #[serde(default)]
    pub credential: Option<String>,
}

fn default_true() -> bool {
    true
}

#[tauri::command]
pub fn save_deploy_target(
    state: State<'_, AppState>,
    request: SaveTargetRequest,
) -> Result<(), CommandError> {
    let SaveTargetRequest {
        id,
        kind,
        enabled,
        project,
        works_project,
        works_origin,
        credential,
    } = request;
    let project_state = state.project()?;
    if id.trim().is_empty() {
        return Err(crate::commands::command_invalid("a target needs an id"));
    }
    let parsed = veloct_app::parse_kind(&kind)?;

    // The handle is derived from the target id, so it is stable across a re-save and the credential is not
    // retyped when nothing changed.
    let handle = crate::credentials::handle_for(&format!("deploy:{id}"));
    let target = veloct_app::StoredTarget {
        id: id.clone(),
        kind: parsed,
        enabled,
        project: project
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        works_project: works_project
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        works_origin: works_origin
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
        credential_ref: Some(handle.clone()),
    };
    target
        .validate()
        .map_err(crate::commands::command_invalid)?;
    // Validate before changing either the OS credential or the project configuration.
    let mut list = veloct_app::TargetList::read(project_state.root.as_path())?;
    if let Some(secret) = credential.filter(|value| !value.trim().is_empty()) {
        crate::credentials::set(&handle, secret.trim())?;
    }
    list.upsert(target);
    list.write(project_state.root.as_path())?;

    let _ = project_state.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Deploy, "deploy target saved")
            .with_field("target_id", &id)
            .with_field("kind", parsed.as_str()),
    );
    Ok(())
}

/// Remove a target, and its stored credential.
#[tauri::command]
pub fn delete_deploy_target(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    let project = state.project()?;
    let mut list = veloct_app::TargetList::read(project.root.as_path())?;
    if !list.remove(&id) {
        return Err(crate::commands::command_invalid(format!(
            "there is no deploy target {id}"
        )));
    }
    list.write(project.root.as_path())?;
    // The credential goes too, or removing a target would leave a secret in the OS store that nothing can
    // reach and the user cannot see.
    let _ = crate::credentials::remove(&crate::credentials::handle_for(&format!("deploy:{id}")));
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Deploy, "deploy target removed")
            .with_field("target_id", &id),
    );
    Ok(())
}

/// Turn deployment on or off for the whole project.
#[tauri::command]
pub fn set_deploy_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), CommandError> {
    let project = state.project()?;
    let mut list = veloct_app::TargetList::read(project.root.as_path())?;
    list.enabled = enabled;
    list.write(project.root.as_path())?;
    Ok(())
}

/// Deploy the current build to every enabled target.
///
/// A SEPARATE step from publishing, because §14.13's re-send works from an existing `dist/`. The command
/// refuses when there is no build rather than building one.
#[tauri::command]
pub async fn deploy_now(
    state: State<'_, AppState>,
    release_id: Option<String>,
) -> Result<DeployOutcomeView, CommandError> {
    let project = state.project()?;
    // ZIP creation, upload and status polling must not block the WebView command thread.
    tauri::async_runtime::spawn_blocking(move || -> Result<DeployOutcomeView, CommandError> {
        let clock = veloct_core::SystemClock;

        // The release id names the deployment's provenance. Falling back to the most recent release on disk is
        // better than inventing one, and an empty string would produce deployment rows that reference nothing.
        let release_id = match release_id.filter(|value| !value.trim().is_empty()) {
            Some(id) => id,
            None => {
                // The release DIRECTORIES are the releases, and the newest name is the most recent because the id
                // carries its timestamp — so this needs no clock and cannot disagree with the listing.
                newest_release_id(project.root.as_path())
                    .unwrap_or_else(|| "unreleased".to_string())
            }
        };

        let outcome = veloct_app::deploy(
            project.root.as_path(),
            &release_id,
            &crate::credentials::get_or_none,
            &clock,
        )?;

        let _ = project.log(
            if outcome.all_succeeded {
                veloct_logging::LogEvent::info(
                    veloct_logging::Category::Deploy,
                    "deployment finished",
                )
            } else {
                veloct_logging::LogEvent::warning(
                    veloct_logging::Category::Deploy,
                    "deployment finished with targets needing attention",
                )
            }
            .with_field("release_id", &outcome.release_id)
            .with_field("targets", outcome.targets.len().to_string())
            .with_field("pushed_files", outcome.pushed_files.to_string())
            .with_field("needing_attention", outcome.needing_attention.join(",")),
        );

        Ok(outcome)
    })
    .await
    .map_err(|_| {
        crate::commands::command_invalid(
            "部署后台任务中断，不能确认结果；请先检查控制台，勿盲目重复部署。",
        )
    })?
}

/// The newest release id on disk, by name.
///
/// The id's own timestamp prefix makes the newest name the most recent release, so this does not need a clock
/// and cannot disagree with the directory listing.
pub fn newest_release_id(project_root: &std::path::Path) -> Option<String> {
    let directory = project_root
        .join(veloct_core::layout::VELOCT_DIR)
        .join(veloct_core::layout::RELEASES_DIR);
    let mut ids: Vec<String> = std::fs::read_dir(directory)
        .ok()?
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        // Only names that parse as release ids, so a stray directory cannot become the "newest release".
        .filter(|name| veloct_core::ids::ReleaseId::parse(name).is_ok())
        .collect();
    ids.sort();
    ids.pop()
}

/// Which sub-targets a completed deployment can re-send (§14.13).
#[tauri::command]
pub fn deploy_resendable(
    state: State<'_, AppState>,
    release_id: Option<String>,
) -> Result<Vec<String>, CommandError> {
    let project = state.project()?;
    let _ = release_id;
    // A re-send needs a recorded outcome, and the outcome of a finished deployment lives in the log rather than
    // in the database. Until a deployment-run table is written, the honest answer is what the targets can do,
    // so this reports the MANUAL targets as needing the user and nothing else.
    let list = veloct_app::TargetList::read(project.root.as_path())?;
    let manual: Vec<String> = list
        .targets
        .iter()
        .filter(|target| veloct_app::is_manual(target.kind))
        .map(|target| target.id.clone())
        .collect();
    Ok(manual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_release_id_is_the_newest_name_and_skips_a_stray_directory() {
        // The id carries its timestamp, so the newest name is the most recent release and a clock is not needed.
        let dir = tempfile::TempDir::new().unwrap();
        let releases = dir
            .path()
            .join(veloct_core::layout::VELOCT_DIR)
            .join(veloct_core::layout::RELEASES_DIR);
        std::fs::create_dir_all(&releases).unwrap();
        for id in ["20260101T120000Z-aaaaaa", "20260102T120000Z-bbbbbb"] {
            std::fs::create_dir_all(releases.join(id)).unwrap();
        }
        // A directory that is not a release id must not become the newest release.
        std::fs::create_dir_all(releases.join("not-a-release")).unwrap();

        assert_eq!(
            newest_release_id(dir.path()).as_deref(),
            Some("20260102T120000Z-bbbbbb")
        );
    }

    #[test]
    fn a_project_with_no_releases_has_no_newest_release() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(newest_release_id(dir.path()).is_none());
    }
}
