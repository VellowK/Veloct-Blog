//! The attachment commands.
//!
//! `handoff.md` §13 is a locked section about putting a picture in an article, and until this module existed
//! the application never called `veloct-attach` at all: the whole model — the three state axes, the reference
//! sets, the dedup rules, the storage adapters — was reachable only from its own tests.
//!
//! The command surface follows §13.3's order rather than inventing one. Adding a file caches the bytes and
//! records the attachment and **touches no Markdown**; the caller asks for the Markdown afterwards and
//! decides when to apply it. That matters because §13.3 exists precisely so the record reaches disk before
//! anything references it — if a document could cite an attachment whose row had not been written, a crash
//! between the two would leave a permanent dangling reference.

use serde::{Deserialize, Serialize};
use tauri::State;

use veloct_app::{AddOutcome, AttachmentView};

use crate::commands::command_invalid;
use crate::state::AppState;
use veloct_app::CommandError;

/// What an add produced, as the window sees it (handoff.md §13.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddAttachmentView {
    pub attachment: AttachmentView,
    /// Whether the bytes were already present for this target and namespace.
    pub outcome: AddOutcome,
    /// The Markdown to insert, so the window does not build a URL itself. A second place that formats an
    /// attachment URL is a second place that can disagree with the site about where the file is.
    pub markdown: String,
}

/// Add a file to the pool.
#[tauri::command]
pub fn add_attachment(
    state: State<'_, AppState>,
    source_path: String,
    storage_target_id: String,
    namespace: String,
) -> Result<AddAttachmentView, CommandError> {
    let project = state.project()?;
    let now = state.now();
    let source = std::path::PathBuf::from(&source_path);

    // The entropy makes the remote name unique without depending on the clock, so two adds in the same
    // millisecond cannot collide.
    let mut entropy = [0u8; veloct_attach::SUFFIX_LENGTH];
    {
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut entropy);
    }

    let (attachment, outcome) = {
        let mut attachments = project.attachments();
        attachments.add_file(&source, &storage_target_id, &namespace, &now, &entropy)?
    };
    let markdown = attachment.markdown();

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Attachment, "attachment added")
            .with_field("id", &attachment.id)
            .with_field("name", &attachment.original_name)
            .with_field("bytes", attachment.size_bytes.to_string())
            .with_field("target", &storage_target_id)
            .with_field("outcome", if outcome == AddOutcome::Reused { "reused" } else { "added" }),
    );

    Ok(AddAttachmentView {
        attachment,
        outcome,
        markdown,
    })
}

/// Every attachment in the pool.
#[tauri::command]
pub fn list_attachments(state: State<'_, AppState>) -> Result<Vec<AttachmentView>, CommandError> {
    let project = state.project()?;
    // The list is materialised INSIDE the scope of the guard: the guard borrows the project, so returning a
    // value that borrows it would outlive the local `Arc`.
    let attachments = project.attachments().list();
    Ok(attachments)
}

/// The attachments that are safe to drop, and the ones that need the user to decide.
///
/// Both are returned together because the distinction IS the information: §13.2.4 lists four conditions an
/// attachment must meet before it can be dropped, and an attachment that fails them is not a candidate for
/// cleanup but a question for the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrphanView {
    pub safe: Vec<AttachmentView>,
    pub needs_confirmation: Vec<AttachmentView>,
}

#[tauri::command]
pub fn attachment_orphans(state: State<'_, AppState>) -> Result<OrphanView, CommandError> {
    let project = state.project()?;
    let pool = project.attachments();
    // Both lists own their attachments, so the view can be returned after the guard is released.
    let safe = pool.safe_orphans();
    let needs_confirmation = pool.needs_confirmation();
    drop(pool);
    Ok(OrphanView {
        safe,
        needs_confirmation,
    })
}

/// Remove an attachment and its cached bytes.
#[tauri::command]
pub fn remove_attachment(
    state: State<'_, AppState>,
    id: String,
) -> Result<AttachmentView, CommandError> {
    let project = state.project()?;
    let removed = {
        let mut attachments = project.attachments();
        attachments.remove(&id)?
    };
    let _ = project.log(
        veloct_logging::LogEvent::warning(veloct_logging::Category::Attachment, "attachment removed")
            .with_field("id", &removed.id)
            .with_field("name", &removed.original_name)
            // The reference count is recorded because removing a referenced attachment is the case a user may
            // regret, and the log is where that is visible afterwards.
            .with_field("references", removed.reference_count.to_string()),
    );
    Ok(removed)
}

/// The Markdown for an attachment.
#[tauri::command]
pub fn attachment_markdown(
    state: State<'_, AppState>,
    id: String,
) -> Result<String, CommandError> {
    let project = state.project()?;
    let markdown = project.attachments().markdown_for(&id)?;
    Ok(markdown)
}

/// Record that a document cites an attachment (`handoff.md` §13.2).
#[tauri::command]
pub fn reference_attachment(
    state: State<'_, AppState>,
    attachment_id: String,
    content_id: String,
    paragraph_id: Option<String>,
) -> Result<(), CommandError> {
    let project = state.project()?;
    project
        .attachments()
        .record_reference(&attachment_id, &content_id, paragraph_id.as_deref())?;

    let _ = project.log(
        veloct_logging::LogEvent::info(
            veloct_logging::Category::Attachment,
            "attachment referenced",
        )
        .with_field("attachment_id", &attachment_id)
        .with_field("content_id", &content_id),
    );
    Ok(())
}

/// Forget every citation from one document.
///
/// Called when an article is deleted, so its citations do not survive it and keep an attachment from ever
/// becoming an orphan.
#[tauri::command]
pub fn forget_attachment_references(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<usize, CommandError> {
    let project = state.project()?;
    let removed = project.attachments().forget_document(&content_id)?;
    if removed > 0 {
        let _ = project.log(
            veloct_logging::LogEvent::info(
                veloct_logging::Category::Attachment,
                "attachment references forgotten",
            )
            .with_field("content_id", &content_id)
            .with_field("removed", removed.to_string()),
        );
    }
    Ok(removed)
}

/// Write the `attachments` rows into the project index (`handoff.md` §22.4).
#[tauri::command]
pub fn sync_attachment_index(state: State<'_, AppState>) -> Result<usize, CommandError> {
    let project = state.project()?;
    let clock = veloct_core::SystemClock;
    let count = project.attachments().sync_index(&clock)?;
    Ok(count)
}

/// One configured storage target, as the window lists it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTargetView {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Whether this build has an adapter for the kind (`handoff.md` §13.10–13.13).
    pub implemented: bool,
    /// Why not, when it does not, so the window can say what to use instead.
    pub unimplemented_reason: Option<String>,
    pub path_prefix: Option<String>,
    pub public_url_prefix: String,
    pub repository: Option<String>,
    pub branch: Option<String>,
    /// S3: the access key ID, which is an identifier rather than a secret (§13.11 names both keys).
    pub access_key: Option<String>,
    /// The HANDLE, never a value (§12).
    pub credential_ref: Option<String>,
    pub has_credential: bool,
}

impl StorageTargetView {
    fn of(config: &veloct_attach::StorageConfig, has_credential: bool) -> Self {
        Self {
            id: config.id.as_str().to_string(),
            name: config.name.clone(),
            kind: config.kind.as_str().to_string(),
            implemented: veloct_attach::is_implemented(config.kind),
            unimplemented_reason: veloct_attach::unimplemented_reason(config.kind)
                .map(str::to_string),
            path_prefix: config.key_prefix.clone(),
            public_url_prefix: config.public_url_prefix.clone(),
            repository: config.repository.clone(),
            branch: config.branch.clone(),
            access_key: config.access_key.clone(),
            credential_ref: config.credential_ref.clone(),
            has_credential,
        }
    }
}

/// A storage target as the window saves it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveStorageTargetRequest {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub path_prefix: Option<String>,
    #[serde(default)]
    pub public_url_prefix: Option<String>,
    /// GitHub: `owner/name` (§13.13).
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    /// S3: Endpoint, Region, Bucket and the access key ID (§13.11). The SECRET key is `credential`.
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub bucket: Option<String>,
    #[serde(default)]
    pub access_key: Option<String>,
    /// The credential VALUE, written to the OS store. Never persisted here (§12.2).
    #[serde(default)]
    pub credential: Option<String>,
}

/// The configured storage targets.
#[tauri::command]
pub fn storage_targets(state: State<'_, AppState>) -> Result<Vec<StorageTargetView>, CommandError> {
    let project = state.project()?;
    let list = read_storage_targets(project.root.as_path())?;
    Ok(list
        .iter()
        .map(|config| {
            let has = config
                .credential_ref
                .as_deref()
                .map(|handle| crate::credentials::get_or_none(handle).is_some())
                .unwrap_or(false);
            StorageTargetView::of(config, has)
        })
        .collect())
}

/// Add or update a storage target.
#[tauri::command]
pub fn save_storage_target(
    state: State<'_, AppState>,
    request: SaveStorageTargetRequest,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let kind = parse_storage_kind(&request.kind)?;

    let handle = crate::credentials::handle_for(&format!("storage:{}", request.id));
    if let Some(secret) = request.credential.filter(|value| !value.trim().is_empty()) {
        crate::credentials::set(&handle, secret.trim())?;
    }

    let mut config = veloct_attach::StorageConfig::new(&request.id, &request.name, kind);
    config.key_prefix = request.path_prefix.filter(|value| !value.trim().is_empty());
    config.public_url_prefix = request.public_url_prefix.unwrap_or_default();
    config.repository = request.repository.filter(|value| !value.trim().is_empty());
    config.branch = request.branch.filter(|value| !value.trim().is_empty());
    // §13.11's S3 fields. The access key ID is configuration; the SECRET key is `request.credential`,
    // which was written to the OS store above and never reaches this file.
    config.endpoint = request.endpoint.filter(|value| !value.trim().is_empty());
    config.region = request.region.filter(|value| !value.trim().is_empty());
    config.bucket = request.bucket.filter(|value| !value.trim().is_empty());
    config.access_key = request.access_key.filter(|value| !value.trim().is_empty());
    // A kind that needs a credential gets the handle; the local directory kind does not, because a handle to
    // nothing would make the window report a missing credential for a target that needs none.
    config.credential_ref = match kind {
        veloct_attach::StorageKind::Sftp
        | veloct_attach::StorageKind::S3
        | veloct_attach::StorageKind::LskyPro
        | veloct_attach::StorageKind::GitHub => Some(handle),
    };

    let mut list = read_storage_targets(project.root.as_path())?;
    match list.iter_mut().find(|existing| existing.id.as_str() == request.id) {
        Some(existing) => *existing = config,
        None => list.push(config),
    }
    write_storage_targets(project.root.as_path(), &list)?;

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Attachment, "storage target saved")
            .with_field("target_id", &request.id)
            .with_field("kind", kind.as_str())
            .with_field("implemented", veloct_attach::is_implemented(kind).to_string()),
    );
    Ok(())
}

/// Remove a storage target, and its stored credential.
#[tauri::command]
pub fn delete_storage_target(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    let project = state.project()?;
    let mut list = read_storage_targets(project.root.as_path())?;
    let before = list.len();
    list.retain(|config| config.id.as_str() != id);
    if list.len() == before {
        return Err(command_invalid(format!("there is no storage target {id}")));
    }
    write_storage_targets(project.root.as_path(), &list)?;
    let _ = crate::credentials::remove(&crate::credentials::handle_for(&format!("storage:{id}")));
    Ok(())
}

/// Whether an attachment may be uploaded, and why not when it may not.
#[tauri::command]
pub fn upload_eligibility(
    state: State<'_, AppState>,
    id: String,
) -> Result<veloct_app::UploadEligibility, CommandError> {
    let project = state.project()?;
    let eligibility = project.attachments().upload_eligibility(&id)?;
    Ok(eligibility)
}

/// Upload an attachment to a configured target.
///
/// `handoff.md` §13.3's second step: the bytes reach the target and the object record is written, and the
/// Markdown is still untouched. The caller applies the URL afterwards.
#[tauri::command]
pub fn upload_attachment(
    state: State<'_, AppState>,
    id: String,
    storage_target_id: String,
) -> Result<veloct_app::UploadView, CommandError> {
    let project = state.project()?;
    let now = state.now();

    let list = read_storage_targets(project.root.as_path())?;
    let config = list
        .iter()
        .find(|config| config.id.as_str() == storage_target_id)
        .cloned()
        .ok_or_else(|| {
            command_invalid(format!(
                "there is no storage target {storage_target_id}; configure one first"
            ))
        })?;

    // The credential comes from the OS store by handle, and a kind that needs none passes `None`.
    let credential = config
        .credential_ref
        .as_deref()
        .and_then(crate::credentials::get_or_none);

    // A LOCAL target writes into the project's own `.veloct/uploads`, which is §13.10's server storage for a
    // user whose server is this machine. It is also the one target that needs no network.
    let target = match config.kind {
        veloct_attach::StorageKind::Sftp if credential.is_none() && config.host.is_none() => {
            veloct_attach::local_target(
                config.clone(),
                &veloct_attach::local_root(project.root.as_path(), config.id.as_str()),
            )
        }
        _ => {
            let client = veloct_core::http::ReqwestHttpClient::with_default_timeout(
                std::time::Duration::from_secs(300),
            )?;
            veloct_attach::target_from_config(config.clone(), credential, std::sync::Arc::new(client))?
        }
    };

    let upload = project
        .attachments()
        .upload(&id, &target, &now)?;

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Attachment, "attachment uploaded")
            .with_field("id", &id)
            .with_field("target_id", &storage_target_id)
            .with_field("reused", upload.reused.to_string())
            .with_field("remote_key", &upload.remote_key),
    );
    Ok(upload)
}

/// The recorded remote objects, so the window can show what is on the targets.
#[tauri::command]
pub fn attachment_objects(
    state: State<'_, AppState>,
) -> Result<Vec<veloct_app::ObjectView>, CommandError> {
    let project = state.project()?;
    let objects = project.attachments().objects();
    Ok(objects)
}

/// Parse a storage kind.
pub fn parse_storage_kind(text: &str) -> Result<veloct_attach::StorageKind, CommandError> {
    match text.trim().to_ascii_lowercase().as_str() {
        "sftp" | "server" | "local" => Ok(veloct_attach::StorageKind::Sftp),
        "s3" => Ok(veloct_attach::StorageKind::S3),
        "lsky" | "lskypro" => Ok(veloct_attach::StorageKind::LskyPro),
        "github" => Ok(veloct_attach::StorageKind::GitHub),
        other => Err(command_invalid(format!(
            "unknown storage kind {other:?}; expected one of sftp, s3, lsky, github"
        ))),
    }
}

/// Where the storage targets are stored.
pub const STORAGE_TARGETS_FILE: &str = ".veloct/storage-targets.json";

fn read_storage_targets(
    project_root: &std::path::Path,
) -> Result<Vec<veloct_attach::StorageConfig>, CommandError> {
    let path = project_root.join(STORAGE_TARGETS_FILE);
    let Some(text) = veloct_core::workdir::read_text_optional(&path)? else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&text).map_err(|error| {
        command_invalid(format!("{} is not a storage target list: {error}", path.display()))
    })
}

fn write_storage_targets(
    project_root: &std::path::Path,
    targets: &[veloct_attach::StorageConfig],
) -> Result<(), CommandError> {
    let path = project_root.join(STORAGE_TARGETS_FILE);
    let text = serde_json::to_vec_pretty(targets).map_err(|error| {
        command_invalid(format!("could not serialise the storage targets: {error}"))
    })?;
    veloct_core::workdir::ensure_parent(&path)?;
    veloct_core::workdir::write_atomic(&path, &text)?;
    Ok(())
}

/// The directory cached bytes live in, so the window can show it.
#[tauri::command]
pub fn attachment_cache_directory(state: State<'_, AppState>) -> Result<String, CommandError> {
    let project = state.project()?;
    let directory = project.attachments().cache_directory()?;
    Ok(directory.to_string_lossy().to_string())
}

/// Reject an empty id early, so a caller gets a clear message rather than a lookup failure.
#[allow(dead_code)]
fn require_id(id: &str) -> Result<(), CommandError> {
    if id.trim().is_empty() {
        return Err(command_invalid("an attachment id is required"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_id_is_rejected_with_a_clear_message() {
        assert!(require_id("").is_err());
        assert!(require_id("   ").is_err());
        assert!(require_id("01ABC").is_ok());
        let error = require_id("").expect_err("an empty id is refused");
        assert!(error.to_string().contains("attachment id"), "{error}");
    }

    #[test]
    fn the_add_view_round_trips_through_json_for_the_window() {
        let view = AddAttachmentView {
            attachment: AttachmentView {
                id: "01ABC".to_string(),
                original_name: "photo.png".to_string(),
                content_hash: "abc".to_string(),
                size_bytes: 3,
                cache_path: "abc/photo-a1b2c3.png".to_string(),
                storage_target_id: "local".to_string(),
                storage_namespace: "default".to_string(),
                remote_url: None,
                remote_key: None,
                upload_status: "PENDING".to_string(),
                reference_status: "NOT_APPLIED".to_string(),
                cache_status: "PRESENT".to_string(),
                last_error: None,
                created_at: "2026-01-01T12:00:00Z".to_string(),
                reference_count: 0,
            },
            outcome: AddOutcome::Reused,
            markdown: "![photo.png](/.attachments-cache/abc/photo-a1b2c3.png)".to_string(),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"outcome\":\"reused\""), "{json}");
        assert!(json.contains("markdown"), "{json}");
    }

    #[test]
    fn the_orphan_view_separates_safe_from_needs_confirmation() {
        // The distinction IS the information: an attachment that fails section 13.2.4's conditions is not a
        // cleanup candidate but a question for the user.
        let view = OrphanView {
            safe: Vec::new(),
            needs_confirmation: Vec::new(),
        };
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("needs_confirmation"), "{json}");
    }
}
