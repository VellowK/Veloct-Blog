use super::*;
use tempfile::TempDir;
use veloct_attach::{StorageConfig, StorageKind};
use veloct_core::http::FakeHttpClient;
const NOW: &str = "2026-10-02T00:00:00Z";

fn fixture(name: &str) -> (TempDir, AttachmentService, String) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(name);
    std::fs::write(&source, b"IMAGE-BYTES").unwrap();
    let mut service = AttachmentService::open(dir.path()).unwrap();
    let (attachment, _) = service.add_file(&source, "local", "default", NOW, &[1,2,3,4,5,6]).unwrap();
    (dir, service, attachment.id)
}
fn target(root: &Path, id: &str) -> StorageTarget {
    let mut config = StorageConfig::new(id, id, StorageKind::Local);
    config.key_prefix = Some("uploads".into());
    config.public_url_prefix = format!("https://{id}.test/");
    veloct_attach::local_target(config, &veloct_attach::local_root(root, id))
}
fn ids() -> Vec<String> { vec!["a".into(), "b".into(), "c".into()] }

#[test]
fn ordered_upload_records_every_replica_but_keeps_the_first_primary_after_restart() {
    let (dir, mut service, id) = fixture("photo.png");
    let article = dir.path().join("article.md");
    std::fs::write(&article, "original content").unwrap();
    let attempts = upload_ordered(dir.path(), &mut service, &id, &ids(), NOW, |id| Ok(target(dir.path(), id))).unwrap();
    assert_eq!(attempts.len(), 3);
    assert!(attempts.iter().all(|a| a.error.is_none()));
    assert_eq!(service.objects().len(), 3);
    let primary = service.get(&id).unwrap().remote_url.unwrap();
    assert!(primary.starts_with("https://a.test/"));
    let mut reopened = AttachmentService::open(dir.path()).unwrap();
    let reversed = vec!["c".into(), "b".into(), "a".into()];
    let retried = upload_ordered(dir.path(), &mut reopened, &id, &reversed, NOW, |id| Ok(target(dir.path(), id))).unwrap();
    assert!(retried.iter().all(|a| a.reused));
    assert_eq!(reopened.get(&id).unwrap().remote_url.as_deref(), Some(primary.as_str()));
    assert_eq!(reopened.objects().len(), 3);
    assert_eq!(std::fs::read_to_string(article).unwrap(), "original content");
}

#[test]
fn a_resolution_failure_does_not_stop_later_hosts_and_retry_does_not_switch_primary() {
    let (dir, mut service, id) = fixture("photo.png");
    let attempts = upload_ordered(dir.path(), &mut service, &id, &ids(), NOW, |id| {
        if id == "a" { Err("missing credential".into()) } else { Ok(target(dir.path(), id)) }
    }).unwrap();
    assert!(attempts[0].error.is_some());
    let primary = service.get(&id).unwrap().remote_url.unwrap();
    assert!(primary.starts_with("https://b.test/"));
    assert_eq!(read_messages(dir.path()).unwrap().upload_errors.len(), 1);
    upload_ordered(dir.path(), &mut service, &id, &ids(), NOW, |id| Ok(target(dir.path(), id))).unwrap();
    assert_eq!(service.get(&id).unwrap().remote_url.as_deref(), Some(primary.as_str()));
    assert!(read_messages(dir.path()).unwrap().upload_errors.is_empty());
}

#[test]
fn an_upload_failure_is_durable_and_preserves_the_primary_and_reference_axis() {
    let (dir, mut service, id) = fixture("photo.png");
    service.upload(&id, &target(dir.path(), "a"), NOW).unwrap();
    service.record_reference(&id, "article", None).unwrap();
    let before = service.get(&id).unwrap();
    let block = dir.path().join("not-a-directory");
    std::fs::write(&block, b"blocked").unwrap();
    let attempts = upload_ordered(dir.path(), &mut service, &id, &["bad".into(), "c".into()], NOW, |id| {
        if id == "bad" {
            let mut config = StorageConfig::new(id, id, StorageKind::Local);
            config.public_url_prefix = "https://bad.test/".into();
            Ok(veloct_attach::local_target(config, &block))
        } else { Ok(target(dir.path(), id)) }
    }).unwrap();
    assert!(attempts[0].error.is_some());
    assert!(attempts[1].error.is_none());
    let reopened = AttachmentService::open(dir.path()).unwrap().get(&id).unwrap();
    assert_eq!(reopened.remote_url, before.remote_url);
    assert_eq!(reopened.reference_status, before.reference_status);
    assert_eq!(reopened.reference_count, 1);
    assert_eq!(reopened.upload_status, "SUCCEEDED");
    assert_eq!(read_messages(dir.path()).unwrap().upload_errors[0].target_id, "bad");
}

#[test]
fn all_failed_hosts_keep_a_missing_primary_missing_and_retryable() {
    let (dir, mut service, id) = fixture("photo.png");
    let attempts = upload_ordered(dir.path(), &mut service, &id, &ids(), NOW, |_| Err("unavailable".into())).unwrap();
    assert!(attempts.iter().all(|a| a.error.is_some()));
    assert!(service.get(&id).unwrap().remote_url.is_none());
    assert_eq!(read_messages(dir.path()).unwrap().upload_errors.len(), 3);
    upload_ordered(dir.path(), &mut service, &id, &ids(), NOW, |id| Ok(target(dir.path(), id))).unwrap();
    assert!(service.get(&id).unwrap().remote_url.unwrap().starts_with("https://a.test/"));
}

#[test]
fn removing_a_same_key_backup_through_the_primary_action_is_refused() {
    let (dir, mut service, id) = fixture("photo.png");
    upload_ordered(dir.path(), &mut service, &id, &["a".into(), "b".into()], NOW, |id| Ok(target(dir.path(), id))).unwrap();
    let before = service.get(&id).unwrap().remote_url;
    assert!(service.delete_remote_object(&id, &target(dir.path(), "b")).is_err());
    assert_eq!(service.get(&id).unwrap().remote_url, before);
    assert_eq!(service.objects().len(), 2);
}

#[test]
fn settings_validate_target_ids_and_preserve_priority_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    assert!(read_settings(dir.path()).unwrap().target_ids.is_empty());
    let settings = SyncSettings { target_ids: vec!["c".into(), "a".into()] };
    save_settings(dir.path(), &settings, &ids()).unwrap();
    assert_eq!(read_settings(dir.path()).unwrap(), settings);
    assert!(save_settings(dir.path(), &SyncSettings { target_ids: vec!["a".into(), "a".into()] }, &ids()).is_err());
    assert!(save_settings(dir.path(), &SyncSettings { target_ids: vec!["missing".into()] }, &ids()).is_err());
    assert_eq!(read_settings(dir.path()).unwrap(), settings);
    std::fs::write(dir.path().join(SETTINGS), b"invalid").unwrap();
    assert!(read_settings(dir.path()).is_err());
}

#[test]
fn image_probe_checks_primary_and_backups_without_mutating_the_selected_url() {
    let (dir, mut service, id) = fixture("photo.png");
    upload_ordered(dir.path(), &mut service, &id, &["a".into(), "b".into()], NOW, |id| Ok(target(dir.path(), id))).unwrap();
    let before = service.get(&id).unwrap().remote_url;
    let client = FakeHttpClient::new().on_method_and_url(HttpMethod::Head, "a.test", 404, b"").on_method_and_url(HttpMethod::Head, "b.test", 200, b"");
    let errors = image_health_errors(&service.list(), &service.objects(), &client);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("已失效"));
    assert_eq!(errors[0].target_id, "a");
    assert_eq!(client.request_count(), 2);
    assert!(client.recorded().iter().all(|r| r.method == HttpMethod::Head && r.timeout == Some(Duration::from_secs(10))));
    record_health(dir.path(), errors, std::slice::from_ref(&id), NOW).unwrap();
    assert_eq!(service.get(&id).unwrap().remote_url, before);
    assert_eq!(read_messages(dir.path()).unwrap().health_errors.len(), 1);
    record_health(dir.path(), Vec::new(), &[id], NOW).unwrap();
    assert!(read_messages(dir.path()).unwrap().health_errors.is_empty());
}

#[test]
fn transient_failure_is_not_claimed_as_deletion_and_relative_urls_are_not_probed() {
    let client = FakeHttpClient::new().on_method_and_url(HttpMethod::Head, "a.test", 503, b"");
    assert!(probe(&client, "https://a.test/photo.png").unwrap().contains("不代表文件已删除"));
    assert!(probe(&client, "/uploads/photo.png").is_none());
    assert!(probe(&client, "data:image/png;base64,a").is_none());
    assert_eq!(client.request_count(), 1);
}

#[test]
fn file_attachments_generate_links_not_images_and_are_not_probed() {
    let (dir, mut service, id) = fixture("report.pdf");
    service.upload(&id, &target(dir.path(), "a"), NOW).unwrap();
    assert!(service.markdown_for(&id).unwrap().starts_with("[report.pdf]("));
    let client = FakeHttpClient::new();
    assert!(image_health_errors(&service.list(), &service.objects(), &client).is_empty());
    assert_eq!(client.request_count(), 0);
}

#[test]
fn a_slow_health_check_merges_recent_upload_messages_and_drops_removed_attachments() {
    let (dir, mut service, id) = fixture("photo.png");
    upload_ordered(dir.path(), &mut service, &id, &ids(), NOW, |_| Err("failed".into())).unwrap();
    let report = record_health(dir.path(), Vec::new(), std::slice::from_ref(&id), NOW).unwrap();
    assert_eq!(report.upload_errors.len(), 3);
    assert_eq!(report.checked_at.as_deref(), Some(NOW));
    assert!(record_health(dir.path(), Vec::new(), &[], NOW).unwrap().upload_errors.is_empty());
}
