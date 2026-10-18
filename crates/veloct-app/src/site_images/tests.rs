use super::*;
use std::sync::Arc;
use veloct_attach::storage::{MemoryFileStore, MemoryStore};
const NOW: &str = "2026-10-18T05:00:00Z";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\npixels";
fn config(id: &str, priority: i32) -> StorageConfig {
    let mut config = StorageConfig::new(id, id, StorageKind::S3);
    config.upload_priority = priority;
    config.public_url_prefix = "https://cdn.example/images".into();
    config
}
fn target(config: StorageConfig, store: Arc<MemoryFileStore>) -> StorageTarget {
    StorageTarget::new(
        config.clone(),
        Box::new(MemoryStore::with_store(config, store)),
    )
}
#[test]
fn preference_is_explicit_sync_first_otherwise_stable_visible_cloud_priority() {
    let mut hidden = config("hidden", 100);
    hidden.hidden_from_upload = true;
    let mut local = config("local", 999);
    local.kind = StorageKind::Local;
    let configs = vec![
        hidden,
        local,
        config("first", 5),
        config("same", 5),
        config("last", 1),
    ];
    assert_eq!(
        preferred_target(&configs, &SyncSettings::default())
            .unwrap()
            .id
            .as_str(),
        "first"
    );
    let settings = SyncSettings {
        target_ids: vec!["hidden".into(), "first".into()],
    };
    assert_eq!(
        preferred_target(&configs, &settings).unwrap().id.as_str(),
        "hidden"
    );
}
#[test]
fn missing_invalid_or_no_preferred_target_never_silently_switches() {
    let configs = vec![
        config("valid", 1),
        StorageConfig::new("local", "Local", StorageKind::Local),
    ];
    for id in ["missing", "local"] {
        assert!(
            preferred_target(
                &configs,
                &SyncSettings {
                    target_ids: vec![id.into(), "valid".into()]
                }
            )
            .is_err()
        );
    }
    assert!(preferred_target(&[], &SyncSettings::default()).is_err());
    assert!(preferred_target(&configs[1..], &SyncSettings::default()).is_err());
}
#[test]
fn both_kinds_reach_pool_and_only_selected_target_without_touching_old_assets_or_config() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    let raw =
        r#"{"author":{"avatar":"/old.png"},"homepage":{"cover":"/old-cover.png"},"unknown":42}"#;
    std::fs::write(dir.path().join("data/site.json"), raw).unwrap();
    let mut service = AttachmentService::open(dir.path()).unwrap();
    let store = Arc::new(MemoryFileStore::new());
    let target = target(config("preferred", 1), store.clone());
    let avatar = upload(
        &mut service,
        SiteImageKind::Avatar,
        PNG,
        &target,
        NOW,
        &[1; veloct_attach::SUFFIX_LENGTH],
    )
    .unwrap();
    let cover = upload(
        &mut service,
        SiteImageKind::Cover,
        PNG,
        &target,
        NOW,
        &[2; veloct_attach::SUFFIX_LENGTH],
    )
    .unwrap();
    assert!(avatar.starts_with("https://cdn.example/images/avatar-"));
    assert!(cover.starts_with("https://cdn.example/images/home-cover-") || cover == avatar); // Content dedup may reuse an existing remote object.
    assert_eq!(service.list().len(), 2);
    assert!(
        service
            .list()
            .iter()
            .all(|a| a.storage_target_id == "preferred" && a.upload_status == "SUCCEEDED")
    );
    for image in service.list() {
        assert_eq!(
            std::fs::read(dir.path().join(".attachments-cache").join(image.cache_path)).unwrap(),
            PNG
        );
    }
    let puts = store.put_count();
    assert_eq!(
        upload(
            &mut service,
            SiteImageKind::Avatar,
            PNG,
            &target,
            NOW,
            &[9; veloct_attach::SUFFIX_LENGTH]
        )
        .unwrap(),
        avatar
    );
    assert_eq!(store.put_count(), puts);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("data/site.json")).unwrap(),
        raw
    );
    assert!(!dir.path().join("site").exists());
}
#[test]
fn upload_failure_keeps_cache_and_failed_record_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let mut service = AttachmentService::open(dir.path()).unwrap();
    let store = Arc::new(MemoryFileStore::new());
    store.fail_puts_with("offline");
    let target = target(config("first", 1), store.clone());
    let error = upload(
        &mut service,
        SiteImageKind::Cover,
        PNG,
        &target,
        NOW,
        &[1; veloct_attach::SUFFIX_LENGTH],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("已保留在附件池"));
    assert_eq!(service.list().len(), 1);
    assert_eq!(service.list()[0].upload_status, "FAILED");
    store.allow_puts();
    assert!(
        upload(
            &mut service,
            SiteImageKind::Cover,
            PNG,
            &target,
            NOW,
            &[2; veloct_attach::SUFFIX_LENGTH]
        )
        .is_ok()
    );
    assert_eq!(service.list().len(), 1);
}
#[test]
fn invalid_or_oversized_png_never_enters_pool_or_uploads() {
    let dir = tempfile::tempdir().unwrap();
    let mut service = AttachmentService::open(dir.path()).unwrap();
    let store = Arc::new(MemoryFileStore::new());
    let target = target(config("first", 1), store.clone());
    for kind in [SiteImageKind::Avatar, SiteImageKind::Cover] {
        let mut big = PNG.to_vec();
        big.resize(kind.max_bytes() + 1, 0);
        for bytes in [b"not png".as_slice(), big.as_slice()] {
            assert!(
                upload(
                    &mut service,
                    kind,
                    bytes,
                    &target,
                    NOW,
                    &[0; veloct_attach::SUFFIX_LENGTH]
                )
                .is_err()
            );
        }
    }
    assert!(service.list().is_empty());
    assert_eq!(store.put_count(), 0);
}
#[test]
fn unusable_public_url_never_changes_site_settings() {
    let dir = tempfile::tempdir().unwrap();
    let mut service = AttachmentService::open(dir.path()).unwrap();
    let mut config = config("first", 1);
    config.public_url_prefix = "/private".into();
    let store = Arc::new(MemoryFileStore::new());
    let target = target(config, store.clone());
    assert!(
        upload(
            &mut service,
            SiteImageKind::Avatar,
            PNG,
            &target,
            NOW,
            &[0; veloct_attach::SUFFIX_LENGTH]
        )
        .is_err()
    );
    assert!(!dir.path().join("data/site.json").exists());
    assert_eq!(service.list().len(), 1);
    assert_eq!(store.put_count(), 0);
}
#[test]
fn saved_images_are_references_after_reopen_and_replacing_them_preserves_article_citations() {
    let dir = tempfile::tempdir().unwrap();
    let mut service = AttachmentService::open(dir.path()).unwrap();
    let target = target(config("first", 1), Arc::new(MemoryFileStore::new()));
    let url = upload(
        &mut service,
        SiteImageKind::Avatar,
        PNG,
        &target,
        NOW,
        &[0; veloct_attach::SUFFIX_LENGTH],
    )
    .unwrap();
    let id = service.list()[0].id.clone();
    service.record_reference(&id, "article-one", None).unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(
        dir.path().join("data/site.json"),
        serde_json::json!({"author":{"avatar":url},"homepage":{"cover":url}}).to_string(),
    )
    .unwrap();
    drop(service);
    let mut service = AttachmentService::open(dir.path()).unwrap();
    assert_eq!(service.get(&id).unwrap().reference_count, 3);
    assert!(!service.needs_confirmation().iter().any(|a| a.id == id));
    std::fs::write(dir.path().join("data/site.json"), "invalid json").unwrap();
    service.refresh_site_image_references().unwrap();
    assert_eq!(service.get(&id).unwrap().reference_count, 3);
    std::fs::write(
        dir.path().join("data/site.json"),
        r#"{"author":{"avatar":""},"homepage":{"cover":""}}"#,
    )
    .unwrap();
    service.refresh_site_image_references().unwrap();
    assert_eq!(service.get(&id).unwrap().reference_count, 1);
}
