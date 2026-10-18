//! Knowledge directory IPC. Network work captures one project and runs outside project locks.
use crate::{credentials, state::AppState};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::State;
use veloct_app::{
    linked_knowledge::{self, LinkedKnowledge, LinkedKnowledgeService, Provider},
    CommandError,
};
use veloct_core::{sha256_hex as sha256, workdir::FileRevision, VeloctError};

#[derive(Serialize)]
pub struct LinkView {
    #[serde(flatten)]
    pub item: LinkedKnowledge,
    pub has_credential: bool,
}
#[derive(Serialize)]
pub struct LinksView {
    pub items: Vec<LinkView>,
    pub revision: Option<FileRevision>,
}
#[derive(Deserialize)]
pub struct LinkInput {
    pub name: String,
    pub provider: Provider,
    pub source_url: String,
    pub token: Option<String>,
    pub app_id: Option<String>,
    pub app_secret: Option<String>,
}
fn handle(root: &std::path::Path, id: &str) -> String {
    format!(
        "linked-knowledge:{}:{id}",
        sha256(root.to_string_lossy().as_bytes())
    )
}
fn view(root: &std::path::Path, result: linked_knowledge::LinkedKnowledgeView) -> LinksView {
    LinksView {
        items: result
            .items
            .into_iter()
            .map(|item| LinkView {
                has_credential: credentials::exists(&handle(root, &item.id)),
                item,
            })
            .collect(),
        revision: result.revision,
    }
}
fn invalid(text: &str) -> CommandError {
    VeloctError::Invalid(text.into()).into()
}
fn find_url<'a>(nodes: &'a [linked_knowledge::OutlineNode], id: &str) -> Option<&'a str> {
    for node in nodes {
        if node.id == id {
            return node.url.as_deref();
        }
        if let Some(url) = find_url(&node.children, id) {
            return Some(url);
        }
    }
    None
}
#[tauri::command]
pub fn open_linked_knowledge(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
    node_id: Option<String>,
) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    let project = state.project()?;
    let item = LinkedKnowledgeService::new(project.root.as_path())
        .read()?
        .items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| invalid("链接知识库不存在"))?;
    let source = linked_knowledge::source(item.provider, &item.source_url)?;
    let url = match node_id {
        None => source.to_string(),
        Some(node) => {
            let target = find_url(&item.nodes, &node)
                .ok_or_else(|| invalid("该目录项没有可用的源文档链接"))?;
            veloct_app::linked_knowledge_links::node_url(item.provider, &source, target)
                .ok_or_else(|| invalid("该源文档链接不安全，已拒绝打开"))?
        }
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| invalid("无法打开默认浏览器"))
}
#[tauri::command]
pub fn linked_knowledge(state: State<'_, AppState>) -> Result<LinksView, CommandError> {
    let project = state.project()?;
    Ok(view(
        project.root.as_path(),
        LinkedKnowledgeService::new(project.root.as_path()).read()?,
    ))
}
#[tauri::command]
pub async fn link_knowledge(
    state: State<'_, AppState>,
    input: LinkInput,
    baseline: Option<FileRevision>,
) -> Result<LinksView, CommandError> {
    let project = state.project()?;
    let now = state.now();
    tauri::async_runtime::spawn_blocking(move || {
        let source = linked_knowledge::source(input.provider, &input.source_url)?;
        let source_url = source.to_string();
        let id = sha256(source_url.as_bytes())[..24].to_string();
        let key = handle(project.root.as_path(), &id);
        let supplied = match input.provider {
            Provider::Yuque => input
                .token
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_owned()),
            Provider::Feishu => match (
                input.app_id.filter(|s| !s.trim().is_empty()),
                input.app_secret.filter(|s| !s.trim().is_empty()),
            ) {
                (None, None) => None,
                (Some(app_id), Some(app_secret)) => Some(
                    serde_json::json!({"app_id":app_id.trim(),"app_secret":app_secret.trim()})
                        .to_string(),
                ),
                _ => return Err(invalid("更新飞书授权时必须同时填写 App ID 和 App Secret")),
            },
        };
        if input.name.trim().is_empty() || input.name.len() > 256 {
            return Err(invalid("请填写知识库名称（不超过 256 字节）"));
        }
        let secret = match supplied.as_ref() {
            Some(value) => value.clone(),
            None => credentials::get(&key)?
                .ok_or_else(|| invalid("请先填写语雀 Token 或飞书应用授权"))?,
        };
        let client =
            veloct_core::http::ReqwestHttpClient::without_redirects(Duration::from_secs(20))?;
        let nodes = linked_knowledge::fetch_outline(&client, input.provider, &source_url, &secret)?;
        // Save authorization separately in the OS store, never in the public snapshot or logs.
        if supplied.is_some() {
            credentials::set(&key, &secret)?;
        }
        let result = LinkedKnowledgeService::new(project.root.as_path()).upsert(
            LinkedKnowledge {
                id,
                name: input.name.trim().into(),
                provider: input.provider,
                source_url,
                nodes,
                synced_at: now,
            },
            baseline,
        )?;
        Ok(view(project.root.as_path(), result))
    })
    .await
    .map_err(|_| invalid("目录同步任务意外结束；原目录未被覆盖"))?
}
#[tauri::command]
pub async fn refresh_linked_knowledge(
    state: State<'_, AppState>,
    id: String,
    baseline: Option<FileRevision>,
) -> Result<LinksView, CommandError> {
    let project = state.project()?;
    let now = state.now();
    tauri::async_runtime::spawn_blocking(move || {
        let service = LinkedKnowledgeService::new(project.root.as_path());
        let mut item = service
            .read()?
            .items
            .into_iter()
            .find(|i| i.id == id)
            .ok_or_else(|| invalid("链接知识库已被移除，请刷新列表"))?;
        let secret = credentials::get(&handle(project.root.as_path(), &id))?
            .ok_or_else(|| invalid("请重新填写授权；跨设备恢复后需要重新授权"))?;
        let client =
            veloct_core::http::ReqwestHttpClient::without_redirects(Duration::from_secs(20))?;
        item.nodes =
            linked_knowledge::fetch_outline(&client, item.provider, &item.source_url, &secret)?;
        item.synced_at = now;
        Ok(view(
            project.root.as_path(),
            service.upsert(item, baseline)?,
        ))
    })
    .await
    .map_err(|_| invalid("目录同步任务意外结束；原目录未被覆盖"))?
}
#[tauri::command]
pub fn unlink_knowledge(
    state: State<'_, AppState>,
    id: String,
    baseline: Option<FileRevision>,
) -> Result<LinksView, CommandError> {
    let project = state.project()?;
    let service = LinkedKnowledgeService::new(project.root.as_path());
    // Only permit a credential handle associated with an actual current record.
    if !service.read()?.items.iter().any(|i| i.id == id) {
        return Err(invalid("链接知识库不存在"));
    }
    let result = service.remove(&id, baseline)?;
    credentials::remove(&handle(project.root.as_path(), &id))?;
    Ok(view(project.root.as_path(), result))
}
