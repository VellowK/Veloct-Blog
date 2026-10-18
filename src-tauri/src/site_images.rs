use crate::{
    AppState,
    attachments::{read_storage_targets, resolve_stored_target},
    commands::command_invalid,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rand::RngCore;
use tauri::State;
use veloct_app::{
    CommandError,
    site_images::{self, SiteImageKind},
};

/// Captures the open project before any await. Never writes site.json or the old public asset.
pub(crate) async fn upload(
    state: State<'_, AppState>,
    data: String,
    kind: SiteImageKind,
) -> Result<String, CommandError> {
    let project = state.project()?;
    let now = state.now();
    if data.len() > kind.max_bytes().div_ceil(3) * 4 {
        return Err(command_invalid("首页图片数据过大。"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = STANDARD
            .decode(data.trim())
            .map_err(|_| command_invalid("首页图片不是有效的 base64。"))?;
        let configs = read_storage_targets(project.root.as_path())?;
        let settings = veloct_app::attachment_sync::read_settings(project.root.as_path())?;
        let config = site_images::preferred_target(&configs, &settings)?;
        let target = resolve_stored_target(
            &configs,
            config.id.as_str(),
            project.root.as_path(),
            crate::credentials::get_or_none,
        )?;
        let mut entropy = [0; veloct_attach::SUFFIX_LENGTH];
        rand::thread_rng().fill_bytes(&mut entropy);
        let url = site_images::upload(
            &mut project.attachments(),
            kind,
            &bytes,
            &target,
            &now,
            &entropy,
        )?;
        let _ = project.log(
            veloct_logging::LogEvent::info(
                veloct_logging::Category::Attachment,
                "homepage image uploaded through attachment pool",
            )
            .with_field("target_id", config.id.as_str()),
        );
        Ok(url)
    })
    .await
    .map_err(|_| command_invalid("首页图片上传任务未能完成；请在附件池检查后重试。"))?
}
