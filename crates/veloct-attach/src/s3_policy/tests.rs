use super::*;
use crate::storage::{StorageConfig, StorageKind};
fn config() -> StorageConfig {
    let mut config = StorageConfig::new("r2", "R2", StorageKind::S3);
    config.bucket = Some("picbed".into());
    config.region = Some("auto".into());
    config.endpoint = Some("https://account.r2.cloudflarestorage.com".into());
    config.access_key = Some("fixture-id".into());
    config.public_url_prefix = "https://cdn.example.com".into();
    config.s3_policy = Some(S3Policy {
        access_style: AccessStyle::Path,
        rename: Rename::Uuid,
        duplicate: Duplicate::RandomAlphanumeric,
        ..S3Policy::default()
    });
    config
}
fn key(config: &StorageConfig, original: &str, seed: &str) -> Result<String> {
    key_for_attachment(
        config,
        original,
        "photo_legacy.PNG",
        seed,
        "2026-01-02T09:04:05.123+08:00",
    )
}
#[test]
fn old_json_is_legacy_and_new_optional_fields_roundtrip() {
    let old: StorageConfig =
        serde_json::from_str(r#"{"id":"r2","name":"R2","kind":"s3"}"#).unwrap();
    assert!(old.s3_policy.is_none());
    assert!(!old.hidden_from_upload);
    assert_eq!(old.upload_priority, 0);
    let mut new = config();
    new.hidden_from_upload = true;
    new.upload_priority = 42;
    let again: StorageConfig = serde_json::from_str(&serde_json::to_string(&new).unwrap()).unwrap();
    assert_eq!(again.s3_policy, new.s3_policy);
    assert!(again.hidden_from_upload);
    assert_eq!(again.upload_priority, 42);
}
#[test]
fn directory_placeholders_use_persisted_utc_and_retry_stable_randoms() {
    let mut config = config();
    config.key_prefix = Some("${year}/${month}/${day}/${hour}/${minute}/${second}/${millisecond}/${weekday}/${random-num:4}/${random-alphanumeric:5}/${uuid-no-dash}".into());
    let first = key(&config, "照片.PNG", "stable-seed").unwrap();
    assert!(first.starts_with("2026/01/02/01/04/05/123/5/"), "{first}");
    assert_eq!(first, key(&config, "照片.PNG", "stable-seed").unwrap());
    assert_ne!(first, key(&config, "照片.PNG", "another-seed").unwrap());
    assert!(first.ends_with(".PNG"));
}
#[test]
fn uuid_names_are_shaped_correctly_and_root_is_really_root() {
    let config = config();
    let value = key(&config, "照片.PNG", "seed").unwrap();
    assert!(!value.contains('/'));
    let base = value.strip_suffix(".PNG").unwrap();
    assert_eq!(
        base.split('-').map(str::len).collect::<Vec<_>>(),
        [8, 4, 4, 4, 12]
    );
    assert_eq!(&base[14..15], "4");
}
#[test]
fn all_renaming_modes_preserve_extension_and_custom_original_name() {
    let mut config = config();
    for mode in [
        Rename::Original,
        Rename::Uuid,
        Rename::Timestamp,
        Rename::OriginalRandom,
        Rename::DateRandom,
        Rename::DateTimeRandom,
        Rename::Random,
        Rename::Custom,
        Rename::Legacy,
    ] {
        let policy = config.s3_policy.as_mut().unwrap();
        policy.rename = mode;
        policy.filename_template = "${origin-filename}-${uuid-no-dash}".into();
        let value = key(&config, "photo.PNG", "seed").unwrap();
        assert!(value.ends_with(".PNG"), "{mode:?}: {value}");
        if mode == Rename::Original {
            assert_eq!(value, "photo.PNG");
        }
        if mode == Rename::Legacy {
            assert_eq!(value, "photo_legacy.PNG");
        }
        if mode == Rename::Timestamp {
            assert_eq!(value, "1767315845123.PNG");
        }
        if mode == Rename::Custom {
            assert!(value.starts_with("photo-"));
        }
    }
}
#[test]
fn invalid_templates_and_unsafe_keys_fail_before_any_upload() {
    let mut config = config();
    for prefix in [
        "../escape",
        "a/../b",
        "a//b",
        "C:/escape",
        "a\\b",
        "a?x",
        "${unknown}",
        "${origin-filename}",
        "${random-num:0}",
        "${random-num:65}",
        "${year",
        "{year}",
    ] {
        config.key_prefix = Some(prefix.into());
        assert!(config.validate().is_err(), "accepted {prefix}");
    }
    config.key_prefix = None;
    for template in ["", "../escape", "a/b", "${unknown}", "${random-num:999}"] {
        let policy = config.s3_policy.as_mut().unwrap();
        policy.rename = Rename::Custom;
        policy.filename_template = template.into();
        assert!(config.validate().is_err(), "accepted filename {template}");
    }
    assert!(key(&config, "../photo.png", "seed").is_err());
}
#[test]
fn missing_key_invalid_bucket_and_endpoint_are_rejected() {
    let good = config();
    assert!(good.validate().is_ok());
    for endpoint in [
        "ftp://host",
        "https://user:password@host",
        "https://host?query",
        "https://host#fragment",
        "https://bad host",
    ] {
        let mut bad = good.clone();
        bad.endpoint = Some(endpoint.into());
        assert!(bad.validate().is_err(), "{endpoint}");
    }
    let mut bad = good.clone();
    bad.access_key = None;
    assert!(bad.validate().is_err());
    bad = good.clone();
    bad.bucket = Some("folder/bucket".into());
    assert!(bad.validate().is_err());
    bad = good;
    bad.endpoint = None;
    assert!(bad.validate().is_err());
}
#[test]
fn endpoint_and_cdn_hosts_are_validated_but_local_ports_ipv6_and_bucket_placeholders_work() {
    for endpoint in [
        "http://localhost:9000/base",
        "http://[::1]:9000",
        "https://{bucket}.objects.example.com",
        "account.r2.cloudflarestorage.com",
    ] {
        let mut config = config();
        config.endpoint = Some(endpoint.into());
        assert!(config.validate().is_ok(), "{endpoint}");
    }
    for address in [
        "https://",
        "https:///host",
        "https://ftp://host",
        "https://host:bad",
        "https://user:secret@host",
        "https://host?key=secret",
    ] {
        let mut config = config();
        config.endpoint = Some(address.into());
        assert!(config.validate().is_err(), "{address}");
        config = self::config();
        config.public_url_prefix = address.into();
        assert!(config.validate().is_err(), "{address}");
    }
}
#[test]
fn first_matching_suffix_wins_and_thumbnail_never_enters_public_url() {
    let mut config = config();
    let policy = config.s3_policy.as_mut().unwrap();
    policy.url_suffixes = vec![
        UrlSuffixRule {
            extensions: ".jpg,PNG".into(),
            suffix: "?format=webp".into(),
        },
        UrlSuffixRule {
            extensions: "*".into(),
            suffix: "!fallback".into(),
        },
    ];
    policy.thumbnail = "?width={width}".into();
    assert!(config.validate().is_ok());
    assert_eq!(
        crate::storage::public_url(&config, "folder/照片.PNG").unwrap(),
        "https://cdn.example.com/folder/%E7%85%A7%E7%89%87.PNG?format=webp"
    );
    assert_eq!(
        crate::storage::public_url(&config, "a.pdf").unwrap(),
        "https://cdn.example.com/a.pdf!fallback"
    );
    assert!(!crate::storage::public_url(&config, "a.png")
        .unwrap()
        .contains("width"));
}
#[test]
fn unknown_suffix_variables_and_too_many_rules_are_rejected() {
    let mut config = config();
    for suffix in [
        "javascript:bad",
        "?width={width}",
        "?x=a b",
        "?x=1#fragment",
    ] {
        config.s3_policy.as_mut().unwrap().url_suffixes = vec![UrlSuffixRule {
            extensions: "png".into(),
            suffix: suffix.into(),
        }];
        assert!(config.validate().is_err(), "{suffix}");
    }
    config.s3_policy.as_mut().unwrap().url_suffixes = vec![
        UrlSuffixRule {
            extensions: "*".into(),
            suffix: "!test".into()
        };
        33
    ];
    assert!(config.validate().is_err());
}
#[test]
fn collisions_preserve_dotted_directories_extension_and_attempt_identity() {
    let key = "dir.with.dot/photo.png";
    let first = collision_key(key, 1, false);
    assert!(first.starts_with("dir.with.dot/photo_"));
    assert!(first.ends_with(".png"));
    assert_eq!(first, collision_key(key, 1, false));
    assert_ne!(first, collision_key(key, 2, false));
    assert!(collision_key("dir.with.dot/README", 1, true).starts_with("dir.with.dot/README_"));
}
#[test]
fn display_and_url_policy_changes_do_not_change_namespace_or_legacy_default_paths() {
    let mut config = config();
    let namespace = config.storage_namespace();
    config.hidden_from_upload = true;
    config.upload_priority = 99;
    config.s3_policy.as_mut().unwrap().rename = Rename::Timestamp;
    config.s3_policy.as_mut().unwrap().thumbnail = "?w={width}".into();
    assert_eq!(config.storage_namespace(), namespace);
}
