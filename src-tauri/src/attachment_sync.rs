use tauri::State;
use veloct_app::{CommandError, attachment_sync::{self as sync, AttachmentMessages, SyncSettings, SyncAttempt}};
use crate::{AppState, attachments::{read_storage_targets, resolve_stored_target}};

#[tauri::command]
pub fn attachment_sync_settings(state: State<'_, AppState>) -> Result<SyncSettings, CommandError> {
    let project = state.project()?;
    Ok(sync::read_settings(project.root.as_path())?)
}

#[tauri::command]
pub fn save_attachment_sync_settings(state: State<'_, AppState>, settings: SyncSettings) -> Result<(), CommandError> {
    let project = state.project()?;
    let _attachments = project.attachments();
    let known = read_storage_targets(project.root.as_path())?.into_iter().map(|c| c.id.as_str().to_owned()).collect::<Vec<_>>();
    sync::save_settings(project.root.as_path(), &settings, &known)?;
    Ok(())
}

#[tauri::command]
pub fn attachment_messages(state: State<'_, AppState>) -> Result<AttachmentMessages, CommandError> {
    let project = state.project()?;
    Ok(sync::read_messages(project.root.as_path())?)
}

#[tauri::command]
pub async fn sync_upload_attachment(state: State<'_, AppState>, id: String, storage_target_id: String) -> Result<Vec<SyncAttempt>, CommandError> {
    let project = state.project()?;
    let now = state.now();
    tauri::async_runtime::spawn_blocking(move || {
        let mut service = project.attachments();
        let attachment = service.get(&id).ok_or_else(|| crate::commands::command_invalid("附件不存在"))?;
        let settings = sync::read_settings(project.root.as_path())?;
        let is_image = veloct_app::mime_type_for(&attachment.original_name).is_some_and(|m| m.starts_with("image/"));
        let ids = if is_image && !settings.target_ids.is_empty() { settings.target_ids } else { vec![storage_target_id] };
        let configs = read_storage_targets(project.root.as_path())?;
        Ok(sync::upload_ordered(project.root.as_path(), &mut service, &id, &ids, &now, |id| {
            resolve_stored_target(&configs, id, project.root.as_path(), crate::credentials::get_or_none)
                .map_err(|e| e.message)
        })?)
    }).await.map_err(|e| crate::commands::command_invalid(e.to_string()))?
}

#[tauri::command]
pub async fn check_attachment_images(state: State<'_, AppState>) -> Result<AttachmentMessages, CommandError> {
    let project = state.project()?;
    let now = state.now();
    tauri::async_runtime::spawn_blocking(move || {
        let (attachments, objects) = {
            let service = project.attachments();
            (service.list(), service.objects())
        };
        let client = veloct_core::ReqwestHttpClient::with_default_timeout(std::time::Duration::from_secs(10))?;
        let errors = sync::image_health_errors(&attachments, &objects, &client);
        let service = project.attachments();
        let ids = service.list().into_iter().map(|a| a.id).collect::<Vec<_>>();
        Ok(sync::record_health(project.root.as_path(), errors, &ids, &now)?)
    }).await.map_err(|e| crate::commands::command_invalid(e.to_string()))?
}
