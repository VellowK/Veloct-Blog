//! Read-only readiness checks for the exact document route, not just the server's home page.
use std::time::Duration;
use tauri::State;
use veloct_app::CommandError;
use veloct_core::http::{HttpClient, HttpRequest, ReqwestHttpClient};
use veloct_process::PreviewRole;
use crate::{commands, state::AppState};

#[tauri::command]
pub async fn preview_document_ready(
    state: State<'_, AppState>, kind: String, id: String, site_url: String,
) -> Result<bool, CommandError> {
    // Do not accept an arbitrary URL from the window: both origin and route come from app state.
    if state.preview_url(PreviewRole::Site).as_deref() != Some(site_url.as_str()) {
        return Ok(false);
    }
    let document = commands::read_document(state, kind, id)?;
    let url = format!("{}{}", site_url.trim_end_matches('/'), document.route);
    tauri::async_runtime::spawn_blocking(move || {
        let client = ReqwestHttpClient::without_redirects(Duration::from_secs(2))?;
        Ok(client.execute(HttpRequest::get(url)).is_ok_and(|response| response.status == 200))
    }).await.map_err(|error| commands::command_invalid(format!("预览就绪检查失败: {error}")))?
}
