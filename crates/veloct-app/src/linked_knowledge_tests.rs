use super::*;
use veloct_core::http::{FakeHttpClient, HttpMethod};
fn yuque() -> &'static str {
    "https://www.yuque.com/team/book"
}
fn item(nodes: Vec<OutlineNode>) -> LinkedKnowledge {
    LinkedKnowledge {
        id: "test-id".into(),
        name: "目录".into(),
        provider: Provider::Yuque,
        source_url: yuque().into(),
        nodes,
        synced_at: "2026-01-01T00:00:00Z".into(),
    }
}
fn toc() -> Value {
    json!({"data":[
        {"uuid":"heading", "parent_uuid":"", "title":"分组", "url":"", "doc_id":0},
        {"uuid":"doc-a", "parent_uuid":"heading", "title":"正文 A", "url":"slug-a", "doc_id":123},
        {"uuid":"doc-b", "parent_uuid":"heading", "title":"正文 B", "url":"/team/book/slug-b", "doc_id":456},
        {"uuid":"doc-c", "parent_uuid":"", "title":"正文 C", "url":"https://www.yuque.com/team/book/slug-c", "doc_id":789}
    ]})
}
#[test]
fn validates_sources_before_credentials_and_preserves_space_host() {
    for bad in [
        "http://www.yuque.com/team/book",
        "https://www.yuque.com.evil.test/team/book",
        "https://secret@www.yuque.com/team/book",
        "https://www.yuque.com:8443/team/book",
        "https://www.yuque.com/team/book/doc",
    ] {
        assert!(source(Provider::Yuque, bad).is_err(), "{bad}");
    }
    assert!(source(Provider::Feishu, "https://example.feishu.cn/wiki/root").is_ok());
    assert!(source(Provider::Feishu, "https://evil.test/wiki/root").is_err());
    assert!(source(Provider::Feishu, "https://example.feishu.cn/docx/root").is_err());
    assert_eq!(
        source(
            Provider::Yuque,
            "https://space.yuque.com/team/book/?from=share#x"
        )
        .unwrap()
        .as_str(),
        "https://space.yuque.com/team/book"
    );
    let client = FakeHttpClient::new().always(200, toc().to_string());
    fetch_outline(
        &client,
        Provider::Yuque,
        "https://space.yuque.com/team/book",
        "token",
    )
    .unwrap();
    assert_eq!(
        client.recorded()[0].url,
        "https://space.yuque.com/api/v2/repos/team/book/toc"
    );
}
#[test]
fn yuque_reads_only_toc_and_builds_real_hierarchy_without_type_field() {
    let client = FakeHttpClient::new().always(200, toc().to_string());
    let nodes = fetch_outline(&client, Provider::Yuque, yuque(), "test-token").unwrap();
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].children.len(), 2);
    assert!(nodes[0].url.is_none());
    assert_eq!(
        nodes[0].children[0].url.as_deref(),
        Some("https://www.yuque.com/team/book/slug-a")
    );
    assert_eq!(
        nodes[0].children[1].url.as_deref(),
        Some("https://www.yuque.com/team/book/slug-b")
    );
    assert_eq!(
        nodes[1].url.as_deref(),
        Some("https://www.yuque.com/team/book/slug-c")
    );
    let requests = client.recorded();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.ends_with("/toc"));
    assert!(requests[0]
        .headers
        .iter()
        .any(|(k, v)| k == "X-Auth-Token" && v == "test-token"));
    assert!(!serde_json::to_string(&item(nodes))
        .unwrap()
        .contains("test-token"));
}
#[test]
fn unsafe_or_unknown_yuque_links_stay_unclickable_and_never_guess_ids() {
    let source = source(Provider::Yuque, yuque()).unwrap();
    for bad in [
        "javascript:alert(1)",
        "https://evil.test/x",
        "//evil.test/x",
        "../escape",
        "https://www.yuque.com/other/book/doc",
        "https://secret@www.yuque.com/team/book/doc",
    ] {
        assert!(
            crate::linked_knowledge_links::node_url(Provider::Yuque, &source, bad).is_none(),
            "{bad}"
        );
    }
    let client = FakeHttpClient::new().always(
        200,
        json!({"data":[{"uuid":"x","title":"unknown","doc_id":123,"url":""}]}).to_string(),
    );
    assert!(
        fetch_outline(&client, Provider::Yuque, yuque(), "token").unwrap()[0]
            .url
            .is_none()
    );
}
#[test]
fn duplicate_and_cyclic_yuque_trees_fail_not_silently_lose_nodes() {
    for data in [
        json!([{"uuid":"a"},{"uuid":"a"}]),
        json!([{"uuid":"a","parent_uuid":"b"},{"uuid":"b","parent_uuid":"a"}]),
    ] {
        let client = FakeHttpClient::new().always(200, json!({"data":data}).to_string());
        assert!(fetch_outline(&client, Provider::Yuque, yuque(), "token").is_err());
    }
}
fn feishu_client() -> FakeHttpClient {
    FakeHttpClient::new()
      .on_method_and_url(HttpMethod::Post, "https://accounts.feishu.cn/oauth/v3/token", 200, json!({"access_token":"bearer-secret","expires_in":7200,"token_type":"Bearer"}).to_string())
      .on_url_contains("get_node?", 200, json!({"code":0,"data":{"node":{"space_id":"42","node_token":"root","title":"飞书根","has_child":true}}}).to_string())
}
fn feishu_secret() -> &'static str {
    r#"{"app_id":"cli_test","app_secret":"application-secret"}"#
}
#[test]
fn feishu_uses_current_auth_node_tokens_and_per_parent_pagination_including_empty_pages() {
    let client = feishu_client()
      .on_url_contains("page_token=next", 200, json!({"code":0,"data":{"items":[{"node_token":"child","obj_token":"wrong-do-not-use","title":"子目录","has_child":true}],"has_more":false}}).to_string())
      .on_url_contains("parent_node_token=child", 200, json!({"code":0,"data":{"items":[{"node_token":"leaf","title":"末页","has_child":false}],"has_more":false}}).to_string())
      .on_url_contains("parent_node_token=root", 200, json!({"code":0,"data":{"items":[],"has_more":true,"page_token":"next"}}).to_string());
    let nodes = fetch_outline(
        &client,
        Provider::Feishu,
        "https://tenant.feishu.cn/wiki/root",
        feishu_secret(),
    )
    .unwrap();
    assert_eq!(
        nodes[0].children[0].children[0].url.as_deref(),
        Some("https://tenant.feishu.cn/wiki/leaf")
    );
    let requests = client.recorded();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests[0].body.as_json().unwrap()["grant_type"],
        "client_credentials"
    );
    assert!(requests.iter().skip(1).all(|r| r
        .headers
        .iter()
        .any(|(k, v)| k == "Authorization" && v == "Bearer bearer-secret")));
    assert!(requests
        .iter()
        .all(|r| !r.url.contains("docx") && !r.url.contains("raw_content")));
    assert!(!serde_json::to_string(&item(nodes))
        .unwrap()
        .contains("secret"));
}
#[test]
fn feishu_repeated_pagination_cursor_and_api_acl_failures_are_reported_without_secrets() {
    let client = feishu_client()
        .on_url_contains(
            "/nodes?",
            200,
            json!({"code":0,"data":{"items":[],"has_more":true,"page_token":"same"}}).to_string(),
        )
        .on_url_contains(
            "/nodes?",
            200,
            json!({"code":0,"data":{"items":[],"has_more":true,"page_token":"same"}}).to_string(),
        );
    assert!(fetch_outline(
        &client,
        Provider::Feishu,
        "https://tenant.feishu.cn/wiki/root",
        feishu_secret()
    )
    .unwrap_err()
    .to_string()
    .contains("分页"));
    let client = FakeHttpClient::new().always(
        200,
        json!({"error":"invalid_client","error_description":"application-secret"}).to_string(),
    );
    let error = fetch_outline(
        &client,
        Provider::Feishu,
        "https://tenant.feishu.cn/wiki/root",
        feishu_secret(),
    )
    .unwrap_err()
    .to_string();
    assert!(!error.contains("application-secret"));
    let client = feishu_client().on_url_contains(
        "/nodes?",
        200,
        json!({"code":131006,"msg":"bearer-secret","data":{}}).to_string(),
    );
    let error = fetch_outline(
        &client,
        Provider::Feishu,
        "https://tenant.feishu.cn/wiki/root",
        feishu_secret(),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("权限"));
    assert!(!error.contains("bearer-secret"));
}
#[test]
fn directory_save_is_atomic_conflict_aware_and_unlink_never_touches_local_docs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("knowledge")).unwrap();
    std::fs::write(dir.path().join("knowledge/local.md"), "keep me").unwrap();
    let service = LinkedKnowledgeService::new(dir.path());
    assert!(service.read().unwrap().items.is_empty());
    let saved = service.upsert(item(vec![]), None).unwrap();
    assert!(service.upsert(item(vec![]), None).is_err());
    assert_eq!(service.read().unwrap().revision, saved.revision);
    assert!(service.remove("test-id", None).is_err());
    assert!(service
        .remove("test-id", saved.revision)
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(dir.path().join("knowledge/local.md")).unwrap(),
        "keep me"
    );
}
#[test]
fn network_failure_preserves_existing_snapshot_and_corrupt_data_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let service = LinkedKnowledgeService::new(dir.path());
    let initial = service.upsert(item(vec![]), None).unwrap();
    let path = dir.path().join("data/linked-knowledge.json");
    let before = std::fs::read(&path).unwrap();
    assert!(fetch_outline(
        &FakeHttpClient::new().always(403, "private-response"),
        Provider::Yuque,
        yuque(),
        "secret"
    )
    .is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, "broken").unwrap();
    assert!(service.upsert(item(vec![]), initial.revision).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
}
