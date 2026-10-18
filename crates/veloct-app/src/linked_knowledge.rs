//! External knowledge links: persist only a directory snapshot, never document bodies or credentials.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use url::Url;
use veloct_core::workdir::{read_text_optional, write_text_if_changed, FileRevision};
use veloct_core::{HttpClient, HttpRequest, ProjectLock, Result, VeloctError};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Yuque,
    Feishu,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutlineNode {
    pub id: String,
    pub title: String,
    pub url: Option<String>,
    #[serde(default)]
    pub children: Vec<OutlineNode>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkedKnowledge {
    pub id: String,
    pub name: String,
    pub provider: Provider,
    pub source_url: String,
    pub nodes: Vec<OutlineNode>,
    pub synced_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkedKnowledgeView {
    pub items: Vec<LinkedKnowledge>,
    pub revision: Option<FileRevision>,
}

fn invalid(message: &str) -> VeloctError {
    VeloctError::Invalid(message.into())
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}
/// Restrict the network authority before attaching any credential. Arbitrary URLs never become API URLs.
pub fn source(provider: Provider, value: &str) -> Result<Url> {
    let mut url = Url::parse(value.trim()).map_err(|_| invalid("请输入完整的 HTTPS 知识库链接"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(invalid(
            "知识库链接必须使用 HTTPS，且不能包含用户名、密码或自定义端口",
        ));
    }
    let host = url.host_str().unwrap_or("");
    let parts: Vec<_> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect();
    let good = match provider {
        Provider::Yuque => {
            (host == "yuque.com" || host.ends_with(".yuque.com"))
                && parts.len() == 2
                && parts.iter().all(|s| identifier(s))
        }
        Provider::Feishu => {
            host.ends_with(".feishu.cn")
                && parts.len() == 2
                && parts[0] == "wiki"
                && identifier(parts[1])
        }
    };
    if !good {
        return Err(invalid(match provider { Provider::Yuque => "语雀请填写 https://www.yuque.com/团队或用户/知识库（不是单篇文档链接）", Provider::Feishu => "飞书请填写知识库根节点链接 https://你的租户.feishu.cn/wiki/节点token（不支持 docx 分享链接）" }));
    }
    url.set_query(None);
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_owned();
    url.set_path(&path);
    Ok(url)
}

pub struct LinkedKnowledgeService {
    root: PathBuf,
}
impl LinkedKnowledgeService {
    pub fn new(root: &Path) -> Self {
        Self { root: root.into() }
    }
    fn path(&self) -> Result<PathBuf> {
        let path = self.root.join("data/linked-knowledge.json");
        for p in [&self.root, &self.root.join("data"), &path] {
            match std::fs::symlink_metadata(p) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(invalid("链接知识库不能写入符号链接"))
                }
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(VeloctError::io(p, e))
                }
                _ => {}
            }
        }
        Ok(path)
    }
    fn load(&self) -> Result<LinkedKnowledgeView> {
        let Some(raw) = read_text_optional(&self.path()?)? else {
            return Ok(LinkedKnowledgeView {
                items: vec![],
                revision: None,
            });
        };
        let items: Vec<LinkedKnowledge> = serde_json::from_str(&raw)?;
        Ok(LinkedKnowledgeView {
            items,
            revision: Some(FileRevision::of_text(&raw)),
        })
    }
    pub fn read(&self) -> Result<LinkedKnowledgeView> {
        let _lock = ProjectLock::shared(&self.root, "read-linked-knowledge")?;
        self.load()
    }
    pub fn upsert(
        &self,
        item: LinkedKnowledge,
        baseline: Option<FileRevision>,
    ) -> Result<LinkedKnowledgeView> {
        if item.name.trim().is_empty() || item.name.len() > 256 || !identifier(&item.id) {
            return Err(invalid("知识库名称不能为空，且不超过 256 字节"));
        }
        source(item.provider, &item.source_url)?;
        let _lock = ProjectLock::exclusive(&self.root, "save-linked-knowledge")?;
        let mut current = self.load()?;
        if current.revision != baseline {
            return Err(VeloctError::ExternalModification { path: self.path()? });
        }
        if let Some(existing) = current.items.iter_mut().find(|i| i.id == item.id) {
            *existing = item;
        } else {
            if current.items.len() >= 100 {
                return Err(invalid("最多链接 100 个知识库"));
            }
            current.items.push(item);
        }
        self.write(current.items)
    }
    pub fn remove(&self, id: &str, baseline: Option<FileRevision>) -> Result<LinkedKnowledgeView> {
        let _lock = ProjectLock::exclusive(&self.root, "remove-linked-knowledge")?;
        let mut current = self.load()?;
        if current.revision != baseline {
            return Err(VeloctError::ExternalModification { path: self.path()? });
        }
        current.items.retain(|i| i.id != id);
        self.write(current.items)
    }
    fn write(&self, items: Vec<LinkedKnowledge>) -> Result<LinkedKnowledgeView> {
        let raw = serde_json::to_string_pretty(&items)? + "\n";
        write_text_if_changed(&self.path()?, &raw)?;
        Ok(LinkedKnowledgeView {
            items,
            revision: Some(FileRevision::of_text(&raw)),
        })
    }
}

struct Fetch<'a> {
    client: &'a dyn HttpClient,
    started: Instant,
    requests: usize,
}
impl Fetch<'_> {
    fn json(&mut self, request: HttpRequest) -> Result<Value> {
        self.requests += 1;
        if self.requests > 500 || self.started.elapsed() > Duration::from_secs(180) {
            return Err(invalid(
                "目录过大或接口耗时过长，请改为链接较小的知识库子目录",
            ));
        }
        let response = self
            .client
            .execute(request.timeout(Duration::from_secs(20)))
            .map_err(|_| invalid("连接目录接口失败，请检查网络后重试；旧目录已保留"))?;
        if !response.is_success() {
            return Err(invalid(&format!(
                "目录接口返回 HTTP {}，请检查授权、权限或访问频率；旧目录已保留",
                response.status
            )));
        }
        if response.body.len() > 4 * 1024 * 1024 {
            return Err(invalid("目录接口响应过大"));
        }
        serde_json::from_slice(&response.body)
            .map_err(|_| invalid("目录接口未返回有效 JSON；旧目录已保留"))
    }
}
fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_owned()
}
fn checked_title(v: &Value) -> String {
    text(v, "title").chars().take(512).collect()
}

/// Secret is a Yuque personal token, or JSON {app_id,app_secret} for a Feishu internal app.
pub fn fetch_outline(
    client: &dyn HttpClient,
    provider: Provider,
    source_url: &str,
    secret: &str,
) -> Result<Vec<OutlineNode>> {
    let source = source(provider, source_url)?;
    if secret.trim().is_empty() {
        return Err(invalid("请先配置知识库授权"));
    }
    let mut fetch = Fetch {
        client,
        started: Instant::now(),
        requests: 0,
    };
    match provider {
        Provider::Yuque => fetch_yuque(&mut fetch, &source, secret),
        Provider::Feishu => fetch_feishu(&mut fetch, &source, secret),
    }
}

fn fetch_yuque(fetch: &mut Fetch<'_>, source: &Url, secret: &str) -> Result<Vec<OutlineNode>> {
    let endpoint = format!(
        "{}/api/v2/repos{}/toc",
        source.origin().ascii_serialization(),
        source.path()
    );
    let response = fetch.json(
        HttpRequest::get(endpoint)
            .header("X-Auth-Token", secret)
            .header("User-Agent", "Veloct"),
    )?;
    let items = response
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("语雀目录格式无效，请检查 Token 与知识库权限"))?;
    if items.len() > 5000 {
        return Err(invalid("目录超过 5000 项，请链接更小的知识库"));
    }
    yuque_tree(source, items)
}

fn yuque_tree(source: &Url, items: &[Value]) -> Result<Vec<OutlineNode>> {
    let ids: HashSet<String> = items.iter().map(|v| text(v, "uuid")).collect();
    if ids.len() != items.len() || ids.contains("") {
        return Err(invalid("语雀目录包含重复或缺失节点 ID"));
    }
    fn children(
        source: &Url,
        items: &[Value],
        ids: &HashSet<String>,
        parent: &str,
        depth: usize,
        visited: &mut HashSet<String>,
    ) -> Result<Vec<OutlineNode>> {
        if depth > 32 {
            return Err(invalid("知识库目录层级超过 32 层"));
        }
        let mut out = vec![];
        for v in items {
            let p = text(v, "parent_uuid");
            if if parent.is_empty() {
                !p.is_empty() && ids.contains(&p)
            } else {
                p != parent
            } {
                continue;
            }
            let id = text(v, "uuid");
            if !visited.insert(id.clone()) {
                return Err(invalid("知识库目录包含循环"));
            }
            // Current SDK guarantees doc_id, not type. Accept known same-repository URL forms,
            // leave unknown grouping/external metadata unclickable, and never guess ID-as-slug.
            let url = if text(v, "type") == "DOC"
                || v.get("doc_id")
                    .and_then(Value::as_i64)
                    .is_some_and(|id| id > 0)
            {
                crate::linked_knowledge_links::node_url(Provider::Yuque, source, &text(v, "url"))
            } else {
                None
            };
            let nested = children(source, items, ids, &id, depth + 1, visited)?;
            out.push(OutlineNode {
                id,
                title: checked_title(v),
                url,
                children: nested,
            });
        }
        Ok(out)
    }
    let mut visited = HashSet::new();
    let tree = children(source, items, &ids, "", 0, &mut visited)?;
    if visited.len() != items.len() {
        return Err(invalid("语雀目录包含循环或不可达节点，未覆盖上次同步结果"));
    }
    Ok(tree)
}

fn feishu_data(response: Value) -> Result<Value> {
    if response.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(invalid(
            "飞书接口拒绝访问，请检查应用权限、发布状态以及应用是否为该知识库的成员；旧目录已保留",
        ));
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| invalid("飞书目录响应缺少 data"))
}
fn fetch_feishu(fetch: &mut Fetch<'_>, source: &Url, secret: &str) -> Result<Vec<OutlineNode>> {
    let auth: Value = serde_json::from_str(secret)
        .map_err(|_| invalid("飞书授权格式无效，请重新填写 App ID 和 App Secret"))?;
    let app_id = text(&auth, "app_id");
    let app_secret = text(&auth, "app_secret");
    if !identifier(&app_id) || app_secret.is_empty() {
        return Err(invalid("请填写飞书自建应用的 App ID 和 App Secret"));
    }
    let token_response = fetch.json(HttpRequest::post("https://accounts.feishu.cn/oauth/v3/token").json(json!({"grant_type":"client_credentials","client_id":app_id,"client_secret":app_secret})))?;
    if token_response.get("error").is_some() {
        return Err(invalid(
            "获取飞书访问凭据失败，请检查 App ID、App Secret 和应用发布状态",
        ));
    }
    let token = text(&token_response, "access_token");
    if token.is_empty() {
        return Err(invalid("飞书未返回访问凭据"));
    }
    let root_token = source
        .path_segments()
        .and_then(|mut p| p.nth(1))
        .unwrap_or("");
    let root = feishu_data(
        fetch.json(
            HttpRequest::get(format!(
                "https://open.feishu.cn/open-apis/wiki/v2/spaces/get_node?token={root_token}"
            ))
            .bearer(&token),
        )?,
    )?;
    let node = &root["node"];
    let space = text(node, "space_id");
    let root_id = text(node, "node_token");
    if !identifier(&space) || !identifier(&root_id) {
        return Err(invalid("飞书知识库根节点响应无效"));
    }
    let mut seen = HashSet::from([root_id.clone()]);
    let nested = if node.get("has_child").and_then(Value::as_bool) == Some(false) {
        vec![]
    } else {
        feishu_children(fetch, source, &token, &space, &root_id, 0, &mut seen)?
    };
    Ok(vec![OutlineNode {
        id: root_id,
        title: checked_title(node),
        url: Some(source.to_string()),
        children: nested,
    }])
}
fn feishu_children(
    fetch: &mut Fetch<'_>,
    source: &Url,
    token: &str,
    space: &str,
    parent: &str,
    depth: usize,
    seen: &mut HashSet<String>,
) -> Result<Vec<OutlineNode>> {
    if depth >= 32 {
        return Err(invalid("飞书目录超过 32 层，请链接更小的子目录"));
    }
    let mut out = vec![];
    let mut page = String::new();
    let mut pages = HashSet::new();
    loop {
        let mut endpoint = Url::parse(&format!(
            "https://open.feishu.cn/open-apis/wiki/v2/spaces/{space}/nodes"
        ))
        .expect("static API URL");
        endpoint
            .query_pairs_mut()
            .append_pair("parent_node_token", parent)
            .append_pair("page_size", "50");
        if !page.is_empty() {
            endpoint.query_pairs_mut().append_pair("page_token", &page);
        }
        let data = feishu_data(fetch.json(HttpRequest::get(endpoint.to_string()).bearer(token))?)?;
        let items = data
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("飞书目录响应缺少 items"))?;
        for item in items {
            let id = text(item, "node_token");
            if !identifier(&id) || !seen.insert(id.clone()) {
                return Err(invalid("飞书目录包含重复或无效节点"));
            }
            if seen.len() > 5000 {
                return Err(invalid("目录超过 5000 项，请链接更小的子目录"));
            }
            let children = if item
                .get("has_child")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                feishu_children(fetch, source, token, space, &id, depth + 1, seen)?
            } else {
                vec![]
            };
            let mut target = source.clone();
            target.set_path(&format!("/wiki/{id}"));
            out.push(OutlineNode {
                id,
                title: checked_title(item),
                url: Some(target.to_string()),
                children,
            });
        }
        if !data
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            break;
        }
        page = text(&data, "page_token");
        if page.is_empty() || !pages.insert(page.clone()) {
            return Err(invalid("飞书目录分页游标无效，已停止同步"));
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "linked_knowledge_tests.rs"]
mod tests;
