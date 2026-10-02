//! The four attachment commands that take `tauri::State`, driven through a REAL `tauri::App`.
//!
//! PROCESSING.md §5.10.2 item 2 records that these commands were left untested because "a unit test cannot
//! construct a `tauri::State`". It can. `tauri/test` (enabled for tests only — see the `[dev-dependencies]`
//! entry) builds an `App` on the mock runtime with no WebView and no display, and `app.state::<AppState>()`
//! hands out exactly the `State<'_, AppState>` the command signatures ask for; the commands are then called
//! DIRECTLY, so the layer under test is the command's own mapping, ordering and argument passing rather than
//! the helper behind it, which already has its own tests inside the lib.
//!
//! These live in an INTEGRATION test rather than in `attachments.rs`'s unit tests for one concrete reason:
//! an integration test target is the only kind Cargo gives the application's Windows manifest resource to
//! (`cargo:rustc-link-arg-tests`, see `build.rs`). Without that resource the loader binds `comctl32.dll` to
//! the version 5.82 copy, which does not export the four functions tauri's menu code imports once a real
//! `App` is built, and the test executable dies with `STATUS_ENTRYPOINT_NOT_FOUND` before any test runs.

use std::path::Path;

use tauri::Manager;
use veloct_desktop_lib::attachments::{
    delete_remote_object, delete_storage_target, save_storage_target, upload_attachment,
    SaveStorageTargetRequest, STORAGE_TARGETS_FILE,
};
use veloct_desktop_lib::{AppState, OpenProject};

/// A real Tauri `App` on the mock runtime, with an [`AppState`] whose project is `root`.
///
/// No window and no display: `mock_app` builds from `tauri::test::mock_context`, and the state is MANAGED
/// rather than set up through `setup`, which is what a command needs and all it needs.
fn mock_app(root: &Path) -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_app();
    let state = AppState::headless();
    state
        .set_project(OpenProject::with_log(
            root,
            "2026-01-01T00:00:00Z",
            // An in-memory sink, so a command's logging reaches nothing on disk and cannot be the reason a
            // test fails.
            Box::new(veloct_logging::MemorySink::new(64)),
        ))
        .unwrap();
    app.manage(state);
    app
}

/// The stored target list, read the way the storage path writes it.
fn stored_targets(root: &Path) -> Vec<veloct_attach::StorageConfig> {
    let path = root.join(STORAGE_TARGETS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            panic!("{} is not a storage target list: {error}", path.display())
        }),
        Err(_) => Vec::new(),
    }
}

/// Write the list the way the storage path writes it, so a test can set a stored target up without going
/// through a command and without the command under test also being the thing that built the fixture.
fn write_targets(root: &Path, targets: &[veloct_attach::StorageConfig]) {
    let path = root.join(STORAGE_TARGETS_FILE);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_vec_pretty(targets).unwrap()).unwrap();
}

/// A local directory target, which is the one kind that needs no credential and no network.
fn local_config(id: &str, prefix: &str) -> veloct_attach::StorageConfig {
    let mut config =
        veloct_attach::StorageConfig::new(id, "本机目录", veloct_attach::StorageKind::Local);
    config.key_prefix = Some(prefix.to_string());
    // A local target derives no URL from nothing, so a prefix is what makes `put` succeed rather than
    // write the bytes and then report that no URL can be derived.
    config.public_url_prefix = "https://example.com/files/".to_string();
    config
}

/// Add one file to the mock app's open project the way the add path does, and return the attachment id.
fn add_one_attachment(
    app: &tauri::App<tauri::test::MockRuntime>,
    source: &Path,
    target_id: &str,
) -> String {
    // A FIXED suffix keeps the remote name predictable, which is what lets an assertion name the file an
    // upload wrote rather than matching a pattern.
    let mut entropy = [0u8; veloct_attach::SUFFIX_LENGTH];
    entropy[0] = 7;
    let project = app.state::<AppState>().project().unwrap();
    let (attachment, _outcome) = project
        .attachments()
        .add_file(source, target_id, "default", "2026-01-01T00:00:00Z", &entropy)
        .unwrap();
    attachment.id
}

/// Serialises the tests in THIS file that touch the real OS credential store.
///
/// The store is process-wide, which is why the lib's own tests take `credentials::store_guard`; that helper
/// is `#[cfg(test)]` and so is not visible from an integration test, and test binaries run one after
/// another, so a lock covering this process is the whole job.
fn store_guard() -> std::sync::MutexGuard<'static, ()> {
    static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    STORE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn the_save_command_writes_an_accepted_target_into_the_open_project() {
    let project = tempfile::TempDir::new().unwrap();
    let app = mock_app(project.path());

    let request = SaveStorageTargetRequest {
        id: "on-this-machine".to_string(),
        name: "本机目录".to_string(),
        kind: "local".to_string(),
        path_prefix: Some("uploads/2026".to_string()),
        public_url_prefix: Some("https://example.com/files/".to_string()),
        repository: None,
        branch: None,
        endpoint: None,
        region: None,
        bucket: None,
        access_key: None,
        instance: None,
        // Submitted ON PURPOSE: the local kind reads no credential, so a value for one must not be stored.
        // Storing a secret nothing references is worse than dropping an input the kind cannot use.
        credential: Some("a-secret-no-local-target-can-use".to_string()),
    };

    save_storage_target(app.state::<AppState>(), request).expect("a valid local target saves");

    let list = stored_targets(project.path());
    assert_eq!(list.len(), 1, "the command wrote exactly one target");
    assert_eq!(list[0].id.as_str(), "on-this-machine");
    assert_eq!(list[0].kind, veloct_attach::StorageKind::Local);
    assert_eq!(list[0].key_prefix.as_deref(), Some("uploads/2026"));
    assert_eq!(list[0].public_url_prefix, "https://example.com/files/");
    assert!(
        list[0].credential_ref.is_none(),
        "the window must not be told a local target is missing a credential it cannot use"
    );

    // §12.2: the submitted value is nowhere in the configuration the command wrote.
    let text = std::fs::read_to_string(project.path().join(STORAGE_TARGETS_FILE)).unwrap();
    assert!(
        !text.contains("a-secret-no-local-target-can-use"),
        "a credential value must never reach the target list: {text}"
    );
}

#[test]
fn the_save_command_stores_neither_a_refused_target_nor_its_credential() {
    // The helper-level twin of this test lives in `attachments.rs`
    // (`a_refused_configuration_stores_neither_the_target_nor_its_credential`); this one drives the COMMAND,
    // so the credential mapping the command does — and the order it hands the steps to the save path in — is
    // covered too.
    let _store = store_guard();
    let project = tempfile::TempDir::new().unwrap();
    let app = mock_app(project.path());
    let id = "t11-command-refused";
    let handle = veloct_desktop_lib::credentials::handle_for(&format!("storage:{id}"));
    const SECRET: &str = "a-command-secret-that-must-not-be-stored";

    let request = SaveStorageTargetRequest {
        id: id.to_string(),
        name: "S3 附件".to_string(),
        kind: "s3".to_string(),
        path_prefix: None,
        public_url_prefix: None,
        repository: None,
        branch: None,
        endpoint: Some("https://s3.example.com".to_string()),
        region: Some("us-east-1".to_string()),
        // The crate's S3 rule requires a bucket, and this is the configuration it refuses.
        bucket: None,
        access_key: Some("AKIAIOSFODNN7EXAMPLE".to_string()),
        instance: None,
        credential: Some(SECRET.to_string()),
    };

    let error = save_storage_target(app.state::<AppState>(), request)
        .expect_err("the crate refuses an S3 target with no bucket");
    assert!(error.to_string().contains("Bucket"), "{error}");

    // 1. No row, and the file was never created — not merely "the list is empty after an edit".
    assert!(
        !project.path().join(STORAGE_TARGETS_FILE).exists(),
        "a refused save must not write the target list at all"
    );
    // 2. And no credential, which is the half a reordered command would leak.
    assert!(
        !veloct_desktop_lib::credentials::exists(&handle),
        "a refused save must not store the credential either"
    );
}

#[test]
fn the_delete_command_forgets_a_stored_credential_and_refuses_an_unknown_id() {
    let _store = store_guard();
    let project = tempfile::TempDir::new().unwrap();
    let app = mock_app(project.path());
    let id = "t12-command-delete";
    let handle = veloct_desktop_lib::credentials::handle_for(&format!("storage:{id}"));

    let mut config =
        veloct_attach::StorageConfig::new(id, "S3 附件", veloct_attach::StorageKind::S3);
    config.bucket = Some("veloct-assets".to_string());
    config.credential_ref = Some(handle.clone());
    write_targets(project.path(), &[config.clone()]);

    // The credential must really be in the store, or "the command forgot it" would be vacuous.
    let stored = veloct_desktop_lib::credentials::set(&handle, "s3-secret");
    assert!(
        stored.is_ok(),
        "this test needs a working credential store: {stored:?}"
    );
    assert!(veloct_desktop_lib::credentials::exists(&handle));

    delete_storage_target(app.state::<AppState>(), id.to_string())
        .expect("the stored target is deleted");

    assert!(
        stored_targets(project.path()).is_empty(),
        "the target is gone from the list"
    );
    assert!(
        !veloct_desktop_lib::credentials::exists(&handle),
        "and its stored credential no longer resolves"
    );

    // An id that names nothing is an explicit error, and nothing at all is touched: a typo must not be able
    // to forget a real target's secret.
    write_targets(project.path(), &[config]);
    veloct_desktop_lib::credentials::set(&handle, "kept").unwrap();
    assert!(veloct_desktop_lib::credentials::exists(&handle));

    let error = delete_storage_target(app.state::<AppState>(), "not-a-target".to_string())
        .expect_err("an unknown id is reported rather than silently ignored");
    assert!(error.to_string().contains("no storage target"), "{error}");
    let list = stored_targets(project.path());
    assert_eq!(list.len(), 1, "the list is unchanged by the refusal");
    assert_eq!(list[0].id.as_str(), id);
    assert!(
        veloct_desktop_lib::credentials::exists(&handle),
        "a refused delete must not forget the credential of the target it did not delete"
    );
    let _ = veloct_desktop_lib::credentials::remove(&handle);
}

#[test]
fn the_upload_command_reaches_the_target_it_was_given_and_reports_an_unknown_one() {
    let project = tempfile::TempDir::new().unwrap();
    let app = mock_app(project.path());
    let root = project.path();
    write_targets(root, &[local_config("on-this-machine", "uploads")]);

    let source = root.join("photo.png");
    std::fs::write(&source, b"PNG-BYTES").unwrap();
    let id = add_one_attachment(&app, &source, "on-this-machine");

    let uploaded = upload_attachment(
        app.state::<AppState>(),
        id.clone(),
        "on-this-machine".to_string(),
    )
    .expect("a local target uploads");

    // The bytes reached the target the COMMAND resolved, measured where a local target writes them.
    let written = veloct_attach::local_root(root, "on-this-machine").join(&uploaded.remote_key);
    assert_eq!(
        std::fs::read(&written).unwrap(),
        b"PNG-BYTES",
        "{} holds the uploaded bytes",
        written.display()
    );
    assert!(
        uploaded.remote_key.contains("photo"),
        "the key names the file: {}",
        uploaded.remote_key
    );
    assert_eq!(
        uploaded.remote_url,
        format!("https://example.com/files/{}", uploaded.remote_key)
    );
    assert!(!uploaded.reused, "the first upload writes a new object");
    assert_eq!(uploaded.attachment.upload_status, "SUCCEEDED");

    // The command passes the caller's target id THROUGH: an id that names nothing — including a blank one —
    // is reported rather than defaulted to the stored target or to the attachment's own.
    for missing in ["no-such-target", "", "   "] {
        let error = upload_attachment(app.state::<AppState>(), id.clone(), missing.to_string())
            .expect_err("an unknown target id is reported rather than ignored");
        assert!(
            error.to_string().contains("no storage target"),
            "{missing:?}: {error}"
        );
    }
}

#[test]
fn the_remote_delete_command_removes_the_recorded_object_and_refuses_another_target() {
    let project = tempfile::TempDir::new().unwrap();
    let app = mock_app(project.path());
    let root = project.path();
    write_targets(
        root,
        &[
            local_config("on-this-machine", "uploads"),
            local_config("elsewhere", "uploads"),
        ],
    );

    let source = root.join("photo.png");
    std::fs::write(&source, b"PNG-BYTES").unwrap();
    let id = add_one_attachment(&app, &source, "on-this-machine");

    // The setup upload goes through the attachment service, so a failure below can only be the delete
    // command's.
    let target = veloct_attach::local_target(
        local_config("on-this-machine", "uploads"),
        &veloct_attach::local_root(root, "on-this-machine"),
    );
    let uploaded = {
        let project = app.state::<AppState>().project().unwrap();
        let mut attachments = project.attachments();
        attachments
            .upload(&id, &target, "2026-01-01T00:00:00Z")
            .unwrap()
    };
    let written = veloct_attach::local_root(root, "on-this-machine").join(&uploaded.remote_key);
    assert!(written.is_file(), "the setup really wrote a remote object");

    // A target other than the one the object's RECORD names is refused, and the remote file survives: the
    // caller's belief about where the object is must not decide what is deleted.
    let error = delete_remote_object(app.state::<AppState>(), id.clone(), "elsewhere".to_string())
        .expect_err("the other target does not hold this object");
    assert!(error.to_string().contains("elsewhere"), "{error}");
    assert!(
        written.is_file(),
        "a refused cleanup removes nothing: {} is still there",
        written.display()
    );

    // An id that names no stored target is reported rather than silently ignored.
    let error =
        delete_remote_object(app.state::<AppState>(), id.clone(), "no-such-target".to_string())
            .expect_err("an unknown target id is reported");
    assert!(error.to_string().contains("no storage target"), "{error}");
    assert!(written.is_file(), "and it removed nothing either");

    let deleted = delete_remote_object(
        app.state::<AppState>(),
        id.clone(),
        "on-this-machine".to_string(),
    )
    .expect("the target that holds the object removes it");
    assert_eq!(deleted.storage_target_id, "on-this-machine");
    assert_eq!(deleted.key, uploaded.remote_key);
    assert_eq!(
        deleted.public_url,
        format!("https://example.com/files/{}", uploaded.remote_key)
    );
    assert!(!written.exists(), "{} is gone", written.display());

    // And a second call reports that there is no longer a recorded object rather than reporting success.
    let error = delete_remote_object(
        app.state::<AppState>(),
        id.clone(),
        "on-this-machine".to_string(),
    )
    .expect_err("there is nothing left to remove");
    assert!(
        error.to_string().contains("no recorded remote object"),
        "{error}"
    );
}
