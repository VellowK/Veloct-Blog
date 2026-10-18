//! Ordered image replication and read-only availability checks. Never rewrites references.
use std::{collections::BTreeSet, path::Path, time::Duration};
use serde::{Deserialize, Serialize};
use veloct_attach::StorageTarget;
use veloct_core::{HttpClient, HttpMethod, HttpRequest, Result, VeloctError};
use crate::AttachmentService;

#[cfg(test)]
#[path = "attachment_sync_tests.rs"]
mod tests;

const SETTINGS: &str = ".veloct/attachment-sync.json";
const REPORT: &str = ".veloct/attachment-messages.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSettings {
    /// Selected targets in priority order. Empty keeps single-target uploading.
    pub target_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentMessage {
    pub attachment_id: String,
    pub target_id: String,
    pub url: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttachmentMessages {
    pub upload_errors: Vec<AttachmentMessage>,
    pub health_errors: Vec<AttachmentMessage>,
    pub checked_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAttempt {
    pub target_id: String,
    pub url: Option<String>,
    pub reused: bool,
    pub error: Option<String>,
}

fn read_json<T: serde::de::DeserializeOwned + Default>(root: &Path, name: &str) -> Result<T> {
    match veloct_core::workdir::read_text_optional(&root.join(name))? {
        None => Ok(T::default()),
        Some(text) => serde_json::from_str(&text)
            .map_err(|e| VeloctError::Invalid(format!("invalid {name}: {e}"))),
    }
}
fn write_json<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<()> {
    let path = root.join(name);
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| VeloctError::Invalid(e.to_string()))?;
    veloct_core::workdir::ensure_parent(&path)?;
    veloct_core::workdir::write_atomic(&path, &bytes)
}
pub fn read_settings(root: &Path) -> Result<SyncSettings> { read_json(root, SETTINGS) }
pub fn read_messages(root: &Path) -> Result<AttachmentMessages> { read_json(root, REPORT) }
pub fn save_settings(root: &Path, settings: &SyncSettings, known_ids: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for id in &settings.target_ids {
        if !known_ids.contains(id) || !seen.insert(id) {
            return Err(VeloctError::Invalid(format!("invalid or duplicate storage target: {id}")));
        }
    }
    write_json(root, SETTINGS, settings)
}

/// Resolves each target separately: missing credentials or one failing host never stop later backups.
/// Each result is persisted before the next upload, including partial failures.
pub fn upload_ordered(
    root: &Path,
    service: &mut AttachmentService,
    id: &str,
    target_ids: &[String],
    now: &str,
    mut resolve: impl FnMut(&str) -> std::result::Result<StorageTarget, String>,
) -> Result<Vec<SyncAttempt>> {
    if target_ids.is_empty() { return Err(VeloctError::Invalid("请选择存储目标".into())); }
    if service.get(id).is_none() { return Err(VeloctError::Invalid("附件不存在".into())); }
    let mut messages = read_messages(root)?;
    let mut attempts = Vec::new();
    let mut seen = BTreeSet::new();
    for target_id in target_ids {
        if !seen.insert(target_id) { continue; }
        let result = resolve(target_id).and_then(|target| {
            service.upload_replica(id, &target, now).map_err(|e| e.to_string())
        });
        messages.upload_errors.retain(|m| m.attachment_id != id || m.target_id != *target_id);
        let attempt = match result {
            Ok(upload) => SyncAttempt { target_id: target_id.clone(), url: Some(upload.remote_url), reused: upload.reused, error: None },
            Err(error) => {
                messages.upload_errors.push(AttachmentMessage {
                    attachment_id: id.into(), target_id: target_id.clone(), url: None, message: error.clone(),
                });
                SyncAttempt { target_id: target_id.clone(), url: None, reused: false, error: Some(error) }
            }
        };
        attempts.push(attempt);
        write_json(root, REPORT, &messages)?;
    }
    Ok(attempts)
}

/// A failed probe reports unavailability, NOT proof of deletion. Local relative URLs are not probed.
fn probe(client: &dyn HttpClient, url: &str) -> Option<String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) { return None; }
    let mut request = HttpRequest::new(HttpMethod::Head, url);
    request.timeout = Some(Duration::from_secs(10));
    match client.execute(request) {
        Ok(response) if (200..300).contains(&response.status) => None,
        Ok(response) if response.status == 404 || response.status == 410 => Some(format!("图片地址已失效（HTTP {}）；未自动切换备份", response.status)),
        Ok(response) => Some(format!("图片检测未通过（HTTP {}），不代表文件已删除；未自动切换备份", response.status)),
        Err(_) => Some("图片连接检测失败（网络或超时），不代表文件已删除；未自动切换备份".into()),
    }
}

pub fn image_health_errors(attachments: &[crate::AttachmentView], objects: &[crate::ObjectView], client: &dyn HttpClient) -> Vec<AttachmentMessage> {
    let mut errors = Vec::new();
    let mut checked = std::collections::BTreeMap::new();
    for attachment in attachments.iter().filter(|a| crate::mime_type_for(&a.original_name).is_some_and(|m| m.starts_with("image/"))) {
        let mut urls: Vec<(String, String)> = objects.iter().filter(|o| o.content_hash == attachment.content_hash)
            .map(|o| (o.storage_target_id.clone(), o.url.clone())).collect();
        if let Some(url) = &attachment.remote_url {
            if !urls.iter().any(|(_, u)| u == url) { urls.push((attachment.storage_target_id.clone(), url.clone())); }
        }
        for (target_id, url) in urls {
            let error = checked.entry(url.clone()).or_insert_with(|| probe(client, &url));
            if let Some(message) = error {
                errors.push(AttachmentMessage {
                    attachment_id: attachment.id.clone(), target_id, url: Some(url), message: message.clone(),
                });
            }
        }
    }
    errors
}

/// Merge after probing, so an upload that finished during a slow network check keeps its messages.
pub fn record_health(root: &Path, errors: Vec<AttachmentMessage>, current_ids: &[String], now: &str) -> Result<AttachmentMessages> {
    let mut messages = read_messages(root)?;
    messages.upload_errors.retain(|m| current_ids.contains(&m.attachment_id));
    messages.health_errors = errors.into_iter().filter(|m| current_ids.contains(&m.attachment_id)).collect();
    messages.checked_at = Some(now.into());
    write_json(root, REPORT, &messages)?;
    Ok(messages)
}
