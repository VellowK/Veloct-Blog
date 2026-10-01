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
///
/// Every field here is CONFIGURATION or a HANDLE (`handoff.md` §12.2). No field may ever carry a credential
/// VALUE: the SECRET access key and every other secret live in the OS store behind `credential_ref`, and the
/// window learns only whether one exists (`has_credential`).
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
    /// S3: Endpoint, Region and Bucket (`handoff.md` §13.11). Without these the form could not show what a
    /// saved S3 target was saved WITH, so editing one would silently rewrite it to a different target.
    pub endpoint: Option<String>,
    pub region: Option<String>,
    pub bucket: Option<String>,
    /// S3: the access key ID, which is an identifier rather than a secret (§13.11 names both keys).
    pub access_key: Option<String>,
    /// Lsky Pro: the instance address (§13.12's "实例地址 / API 地址").
    ///
    /// It is here for the reason the S3 fields above are: without it the form could not show what a saved
    /// Lsky Pro target was saved WITH, so the next save would submit no instance, `save_storage_target` would
    /// store `None`, and editing a working target would silently break it — the exact defect PROCESSING.md
    /// §5.5 records for `endpoint`/`region`/`bucket`.
    pub instance: Option<String>,
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
            endpoint: config.endpoint.clone(),
            region: config.region.clone(),
            bucket: config.bucket.clone(),
            access_key: config.access_key.clone(),
            instance: config.instance.clone(),
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
    /// Lsky Pro: the instance address (§13.12). The API TOKEN is `credential`, stored by handle.
    #[serde(default)]
    pub instance: Option<String>,
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

/// One storage kind's adapter status, as the window's kind list needs it (`handoff.md` §13.10–§13.13).
///
/// These are facts about the BUILD, not about a project, which is why they carry no target id and no
/// credential handle: this is the answer a user needs before there is any target of that kind to look at.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageKindView {
    /// The stored spelling — exactly `StorageKind::as_str`, which `parse` accepts back and which
    /// `save_storage_target` takes as the `kind`.
    pub kind: String,
    /// Whether this build has an adapter for the kind (`veloct_attach::is_implemented`).
    pub implemented: bool,
    /// Why not, in the user's terms, when it does not (`veloct_attach::unimplemented_reason`).
    /// PROCESSING.md §5.3/§5.4's declared gaps, verbatim — never softened here.
    pub unimplemented_reason: Option<String>,
}

/// Every storage kind this build knows, with its adapter status.
///
/// Deliberately takes NO project state. Whether a KIND has an adapter is a fact about this build rather than
/// about the project, so this answers on a project with zero targets — which is exactly when the window needs
/// it, because a kind with no adapter cannot usefully be saved (an SFTP target needs a host, and nothing can
/// use the target until it has one), so `storage_targets` can never report §5.3's gap for a user who has not
/// already configured the very thing that does not work.
///
/// The kinds come from `StorageKind::ALL` and both answers come from the crate's own `is_implemented` /
/// `unimplemented_reason`: a list or an answer restated here would be a second source of truth for
/// §13.10–§13.13, and the window cannot tell which of two answers is wrong.
#[tauri::command]
pub fn storage_kinds() -> Vec<StorageKindView> {
    veloct_attach::StorageKind::ALL
        .iter()
        .map(|kind| StorageKindView {
            kind: kind.as_str().to_string(),
            implemented: veloct_attach::is_implemented(*kind),
            unimplemented_reason: veloct_attach::unimplemented_reason(*kind).map(str::to_string),
        })
        .collect()
}

/// Add or update a storage target.
#[tauri::command]
pub fn save_storage_target(
    state: State<'_, AppState>,
    request: SaveStorageTargetRequest,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let kind = parse_storage_kind(&request.kind)?;
    let config = config_from_request(&request, kind);

    // A local directory target reads no credential, so a value submitted for one is NOT written to the OS
    // store: storing a secret that nothing references is worse than ignoring an input the kind cannot use.
    // Every other kind keeps the handle, whether or not a value arrives with this save (an omitted value
    // means "leave the stored one alone").
    //
    // The credential VALUE only, when one arrived for a kind that uses one. It is handed to the save path
    // rather than written here, so the order below (validate, then credential, then the list) holds.
    let credential = if kind != veloct_attach::StorageKind::Local {
        request.credential.filter(|value| !value.trim().is_empty())
    } else {
        None
    };

    save_target_config(project.root.as_path(), config, credential)?;

    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Attachment, "storage target saved")
            .with_field("target_id", &request.id)
            .with_field("kind", kind.as_str())
            .with_field("implemented", veloct_attach::is_implemented(kind).to_string()),
    );
    Ok(())
}

/// The configuration a save request describes, with blank values DROPPED.
///
/// Extracted from the command for the same reason [`save_target_config`] was (PROCESSING.md §5.10.2): the
/// MAPPING is where a field can be lost silently — a form that cannot see an instance submits none, and
/// `Some("")` must never become a configuration value — and a function that takes no `AppState` is one a test
/// can drive. The image round's commands are the model this follows.
fn config_from_request(
    request: &SaveStorageTargetRequest,
    kind: veloct_attach::StorageKind,
) -> veloct_attach::StorageConfig {
    fn non_blank(value: &Option<String>) -> Option<String> {
        value.clone().filter(|value| !value.trim().is_empty())
    }

    let mut config = veloct_attach::StorageConfig::new(&request.id, &request.name, kind);
    config.key_prefix = non_blank(&request.path_prefix);
    config.public_url_prefix = request.public_url_prefix.clone().unwrap_or_default();
    config.repository = non_blank(&request.repository);
    config.branch = non_blank(&request.branch);
    // §13.11's S3 fields. The access key ID is configuration; the SECRET key is `request.credential`, which the
    // save path writes to the OS store and which never reaches this file.
    config.endpoint = non_blank(&request.endpoint);
    config.region = non_blank(&request.region);
    config.bucket = non_blank(&request.bucket);
    config.access_key = non_blank(&request.access_key);
    // §13.12's instance address. `StorageConfig::validate` requires it for the Lsky Pro kind, and that rule is
    // enforced on this path now that the kind has an adapter — so a mistake is refused at the save rather than
    // discovered at the first upload.
    config.instance = non_blank(&request.instance);
    // A kind that needs a credential gets the handle; the local directory kind does not, because a handle to
    // nothing would make the window report a missing credential for a target that needs none.
    config.credential_ref = if kind == veloct_attach::StorageKind::Local {
        None
    } else {
        Some(crate::credentials::handle_for(&format!(
            "storage:{}",
            request.id
        )))
    };
    config
}

/// The save path's own sequence for one target: validate, write the credential, write the list.
///
/// Extracted from the command so a test can drive it without a WebView. That extraction is the point of this
/// function existing at all: `StorageConfig::validate` had NO production caller, so the rules it states — the
/// `Local` drive-letter/UNC prefix rule among them — were enforced by nothing but a unit test and LOOKED
/// enforced, which is exactly the shape PROCESSING.md §2.6 records as dead schema. A user met the mistake at
/// the first upload instead of at save time, which is the opposite of what §28's "key failure paths must have
/// explicit error states" asks for.
///
/// The ORDER is part of the contract:
///
/// 1. **validate**, so a refused configuration writes nothing at all — no row, and no credential either;
/// 2. **the credential**, if a value arrived (§12.2: it goes to the OS store, never to this file);
/// 3. **the list**, replacing this id's entry or appending a new one.
fn save_target_config(
    project_root: &std::path::Path,
    config: veloct_attach::StorageConfig,
    credential: Option<String>,
) -> Result<(), CommandError> {
    // Only a kind this build IMPLEMENTS is validated.
    //
    // An unimplemented kind is EXEMPT on purpose. `validate`'s SFTP arm demands a host and a username, and a
    // half-filled SFTP target is one of the ways §13.10's gap becomes visible in the target list (the window
    // can record "I have no adapter for this" before the protocol fields exist). Refusing the save would
    // delete that path. The arm is deliberately KEPT: it states the shape the adapter will need, so it stays
    // a unit test's subject and must not be softened into accepting anything.
    //
    // SFTP is the ONLY exempt kind now. Lsky Pro's arm — an instance address — stopped being exempt when
    // §5.4's protocol was verified and the kind gained an adapter, so that rule is enforced on the save path;
    // D142 in the matrix said exactly this would happen.
    if veloct_attach::is_implemented(config.kind) {
        config.validate().map_err(command_invalid)?;
    }

    if let (Some(handle), Some(secret)) = (config.credential_ref.as_deref(), credential) {
        crate::credentials::set(handle, secret.trim())?;
    }

    let id = config.id.as_str().to_string();
    let mut list = read_storage_targets(project_root)?;
    match list.iter_mut().find(|existing| existing.id.as_str() == id) {
        Some(existing) => *existing = config,
        None => list.push(config),
    }
    write_storage_targets(project_root, &list)
}

/// Remove a storage target, and its stored credential.
#[tauri::command]
pub fn delete_storage_target(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    let project = state.project()?;
    delete_target_config(project.root.as_path(), &id)
}

/// The delete path's own sequence for one target: drop the row, write the list, forget the credential.
///
/// Extracted from the command for the same reason `save_target_config` is: the credential removal had no test
/// that could reach it, and "the OS store no longer resolves the handle" is the half a user cannot see.
///
/// An id that is not in the list is an explicit error and the list is left EXACTLY as it was — nothing is
/// written and no credential is removed, because a typo must not be able to forget a real target's secret.
fn delete_target_config(project_root: &std::path::Path, id: &str) -> Result<(), CommandError> {
    let mut list = read_storage_targets(project_root)?;
    let before = list.len();
    list.retain(|config| config.id.as_str() != id);
    if list.len() == before {
        return Err(command_invalid(format!("there is no storage target {id}")));
    }
    write_storage_targets(project_root, &list)?;
    // `delete_storage_target` also removes the credential, which is what the window's confirmation says. §13.8
    // means the remote FILES are untouched by this.
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

    // A LOCAL target writes into the project's own `.veloct/uploads/<target id>`, which is §13.10's server
    // storage for a user whose server is this machine.
    //
    // An SFTP target is NOT routed to a local directory, whatever it is configured with. Doing that used to
    // make an unconfigured `sftp` target upload to the machine it was running on and report a URL no server
    // serves; an SFTP target now reaches `UnsupportedStore`, which reports the §13.10 gap (PROCESSING.md §5.3).
    let target = storage_target_for(&config, credential, project.root.as_path())?;

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

/// Remove ONE remote object, on the target that actually holds it (`handoff.md` §13.8).
///
/// `remove_attachment` is the other half and deliberately leaves the remote file alone, so this is the only
/// way a user can ask for a remote object to be tidied. The attachment, its cached bytes and its reference
/// records stay: §13.8 removes the remote object, not the user's record of the file.
///
/// Refusals, all BEFORE anything is removed:
///
/// * a target whose `capabilities().can_delete_remotely` is false is refused with its own §13.10/§13.12 gap,
///   rather than reporting a cleanup that removed nothing (ADR-0010);
/// * a target other than the one the object's record names is refused, because the caller's belief about
///   where the object is must not decide what is deleted;
/// * an attachment with no recorded object has nothing to remove.
#[tauri::command]
pub fn delete_remote_object(
    state: State<'_, AppState>,
    id: String,
    storage_target_id: String,
) -> Result<veloct_app::RemoteDeleteView, CommandError> {
    let project = state.project()?;

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

    let credential = config
        .credential_ref
        .as_deref()
        .and_then(crate::credentials::get_or_none);

    // Built the same way an upload builds it, so the target a delete talks to is the target an upload
    // wrote to.
    let target = storage_target_for(&config, credential, project.root.as_path())?;

    let deleted = {
        let mut attachments = project.attachments();
        attachments.delete_remote_object(&id, &target)?
    };

    let _ = project.log(
        veloct_logging::LogEvent::warning(
            veloct_logging::Category::Attachment,
            "remote attachment object removed",
        )
        .with_field("id", &id)
        .with_field("target_id", &deleted.storage_target_id)
        .with_field("remote_key", &deleted.key),
    );
    Ok(deleted)
}

/// Parse a storage kind.
///
/// The crate owns the names (`StorageKind::parse`), so the window cannot accept a spelling the model does
/// not; `server` is kept here because this command surface accepted it as the §13.10 spelling before the
/// `local` kind existed, and it means the REMOTE server, not a directory on this machine.
pub fn parse_storage_kind(text: &str) -> Result<veloct_attach::StorageKind, CommandError> {
    let normalized = text.trim().to_ascii_lowercase();
    if normalized == "server" {
        return Ok(veloct_attach::StorageKind::Sftp);
    }
    veloct_attach::StorageKind::parse(&normalized).ok_or_else(|| {
        command_invalid(format!(
            "unknown storage kind {normalized:?}; expected one of sftp, local (dir/directory), s3, lsky, github"
        ))
    })
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

/// The storage target an upload or a remote cleanup talks to, built from a stored configuration.
///
/// Extracted from the two commands so the ROUTING is testable without a WebView: the defect it closes was a
/// routing decision — an SFTP target with no host and no credential was handed a LOCAL directory adapter, so
/// it "uploaded" to the machine it was running on — and a routing decision that nothing can exercise is how
/// that survived. Both commands call this, so an upload and a delete can never disagree about which target
/// they mean.
fn storage_target_for(
    config: &veloct_attach::StorageConfig,
    credential: Option<String>,
    project_root: &std::path::Path,
) -> Result<veloct_attach::StorageTarget, CommandError> {
    match config.kind {
        // The one target with no protocol and no network: a directory inside the project, which
        // `local_root` derives from the project root and the target id. It needs no credential, so none is
        // read for it.
        veloct_attach::StorageKind::Local => Ok(veloct_attach::local_target(
            config.clone(),
            &veloct_attach::local_root(project_root, config.id.as_str()),
        )),
        _ => {
            let client = veloct_core::http::ReqwestHttpClient::with_default_timeout(
                std::time::Duration::from_secs(300),
            )?;
            Ok(veloct_attach::target_from_config(
                config.clone(),
                credential,
                std::sync::Arc::new(client),
                Some(project_root),
            )?)
        }
    }
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

    #[test]
    fn the_window_parses_every_kind_including_the_local_directory() {
        // The command surface must accept the kind names the model does, and `local` / `dir` / `directory`
        // must reach the LOCAL kind rather than the SFTP one — that conflation is the bug this closes.
        assert_eq!(
            parse_storage_kind("local").unwrap(),
            veloct_attach::StorageKind::Local
        );
        assert_eq!(
            parse_storage_kind(" dir ").unwrap(),
            veloct_attach::StorageKind::Local,
            "a user naming a directory means the directory target"
        );
        assert_eq!(
            parse_storage_kind("DIRECTORY").unwrap(),
            veloct_attach::StorageKind::Local
        );
        assert_eq!(
            parse_storage_kind("sftp").unwrap(),
            veloct_attach::StorageKind::Sftp
        );
        // `server` was this surface's spelling for the §13.10 remote server and keeps meaning that.
        assert_eq!(
            parse_storage_kind("server").unwrap(),
            veloct_attach::StorageKind::Sftp
        );
        assert_eq!(
            parse_storage_kind("s3").unwrap(),
            veloct_attach::StorageKind::S3
        );
        assert_eq!(
            parse_storage_kind("github").unwrap(),
            veloct_attach::StorageKind::GitHub
        );
        assert_eq!(
            parse_storage_kind("lskypro").unwrap(),
            veloct_attach::StorageKind::LskyPro
        );

        let error = parse_storage_kind("webdav").unwrap_err();
        assert!(error.to_string().contains("webdav"), "{error}");
        assert!(error.to_string().contains("local"), "{error}");
    }

    #[test]
    fn an_unconfigured_sftp_target_is_never_routed_to_a_local_directory() {
        // THE regression test for the defect: a `sftp` target with no host and no credential used to be handed
        // a local directory adapter by the upload command, so it uploaded to the machine it was running on and
        // reported a remote URL. It must now reach the store that REPORTS the section 13.10 gap.
        let project = tempfile::TempDir::new().unwrap();
        let config = veloct_attach::StorageConfig::new("server", "my server", veloct_attach::StorageKind::Sftp);
        let target = storage_target_for(&config, None, project.path()).unwrap();

        assert_eq!(target.config().kind, veloct_attach::StorageKind::Sftp);
        assert!(
            !target.capabilities().verified,
            "the window can see the gap before an upload is attempted"
        );
        assert!(
            !target.capabilities().can_delete_remotely,
            "and an explicit remote cleanup is refused there too"
        );
        let error = target.store("photo.png", b"BYTES").unwrap_err();
        assert_eq!(error.code(), "NOT_CONFIGURED");
        assert!(
            error.hint().unwrap_or_default().contains("SSH"),
            "{error} / {:?}",
            error.hint()
        );
        // And nothing was written into the project.
        assert!(!veloct_attach::local_root(project.path(), "server").exists());
    }

    #[test]
    fn a_local_target_is_built_inside_the_projects_upload_directory_and_needs_no_credential() {
        let project = tempfile::TempDir::new().unwrap();
        let mut config =
            veloct_attach::StorageConfig::new("on-this-machine", "Local", veloct_attach::StorageKind::Local);
        config.key_prefix = Some("uploads".to_string());
        config.public_url_prefix = "https://example.com/files/".to_string();
        // No credential is passed, which is the point: a local target must not need one.
        let target = storage_target_for(&config, None, project.path()).unwrap();

        assert!(target.capabilities().verified);
        assert!(target.capabilities().can_delete_remotely);
        let outcome = target.store("photo.png", b"BYTES").unwrap();
        let written = veloct_attach::local_root(project.path(), "on-this-machine")
            .join(&outcome.object().key);
        assert_eq!(
            std::fs::read(&written).unwrap(),
            b"BYTES",
            "{} exists with the uploaded bytes",
            written.display()
        );

        // And the same helper reaches the real bytes for an explicit cleanup.
        target.delete_remote(&outcome.object().key).unwrap();
        assert!(!written.exists(), "{} is gone", written.display());
    }

    #[test]
    fn an_s3_targets_view_carries_the_fields_a_form_needs_and_no_credential_value() {
        // A form that cannot see endpoint/region/bucket cannot edit what it saved: saving again would
        // silently rewrite the target.
        let mut config =
            veloct_attach::StorageConfig::new("s3-main", "S3 main", veloct_attach::StorageKind::S3);
        config.endpoint = Some("https://s3.example.com".to_string());
        config.region = Some("us-east-1".to_string());
        config.bucket = Some("veloct-assets".to_string());
        config.key_prefix = Some("uploads".to_string());
        config.public_url_prefix = "https://cdn.example.com".to_string();
        config.access_key = Some("AKIAIOSFODNN7EXAMPLE".to_string());
        config.credential_ref = Some("keychain:storage/s3-main".to_string());

        let view = StorageTargetView::of(&config, true);
        assert_eq!(view.endpoint.as_deref(), Some("https://s3.example.com"));
        assert_eq!(view.region.as_deref(), Some("us-east-1"));
        assert_eq!(view.bucket.as_deref(), Some("veloct-assets"));
        assert_eq!(view.path_prefix.as_deref(), Some("uploads"));
        assert_eq!(view.access_key.as_deref(), Some("AKIAIOSFODNN7EXAMPLE"));

        // Section 12.2: the SECRET access key lives in the OS store, and this view carries only the HANDLE and
        // the boolean. The field list is asserted EXACTLY, so a second field that carried a value would have to
        // change this test on purpose rather than slip in.
        let json = serde_json::to_value(&view).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("a view is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "access_key",
                "branch",
                "bucket",
                "credential_ref",
                "endpoint",
                "has_credential",
                "id",
                "implemented",
                "instance",
                "kind",
                "name",
                "path_prefix",
                "public_url_prefix",
                "region",
                "repository",
                "unimplemented_reason",
            ],
            "the view's fields are configuration and a HANDLE, never a credential value"
        );
        assert_eq!(json["credential_ref"], "keychain:storage/s3-main");
        assert_eq!(json["has_credential"], true);
    }

    #[test]
    fn a_lsky_targets_view_carries_its_instance_so_an_edit_cannot_lose_it() {
        // The §5.5 defect, for §13.12: without this field the form could not see the instance it saved, would
        // submit none on the next save, and a working target would silently stop working.
        let mut config =
            veloct_attach::StorageConfig::new("img", "兰空图床", veloct_attach::StorageKind::LskyPro);
        config.instance = Some("https://img.example.com".to_string());
        let view = StorageTargetView::of(&config, true);

        assert_eq!(view.kind, "lsky");
        assert_eq!(view.instance.as_deref(), Some("https://img.example.com"));
        // §5.4's protocol was read from the vendor's own artifacts and an adapter exists, so the view no longer
        // reports a gap for this kind.
        assert!(view.implemented);
        assert!(view.unimplemented_reason.is_none());
    }

    #[test]
    fn a_blank_field_never_becomes_configuration_and_the_instance_survives_the_mapping() {
        // The mapping is DRIVEN rather than inspected, which is why it is a function that takes no `AppState`.
        let request = SaveStorageTargetRequest {
            id: "img".to_string(),
            name: "兰空图床".to_string(),
            kind: "lsky".to_string(),
            path_prefix: Some("   ".to_string()),
            public_url_prefix: None,
            repository: None,
            branch: None,
            endpoint: None,
            region: None,
            bucket: None,
            access_key: None,
            instance: Some("https://img.example.com".to_string()),
            credential: Some("1|a-secret-token".to_string()),
        };
        let config = config_from_request(&request, veloct_attach::StorageKind::LskyPro);

        assert_eq!(config.instance.as_deref(), Some("https://img.example.com"));
        assert!(
            config.key_prefix.is_none(),
            "a blank must be dropped rather than stored as an empty prefix"
        );
        // The kind's own rule now accepts what the form sent, because the arm is enforced on this path.
        assert!(config.validate().is_ok(), "{:?}", config.validate());

        // §12.2: the view carries a HANDLE, and the credential VALUE is nowhere in the configuration.
        let handle = config
            .credential_ref
            .as_deref()
            .expect("a kind that needs a credential gets a handle");
        assert!(!handle.contains("1|a-secret-token"), "{handle}");

        // And the rule is real: the same request without an instance is refused by the crate.
        let mut without = request;
        without.instance = None;
        let missing = config_from_request(&without, veloct_attach::StorageKind::LskyPro);
        assert!(missing.instance.is_none());
        assert!(
            missing.validate().unwrap_err().contains("instance"),
            "{:?}",
            missing.validate()
        );
    }

    #[test]
    fn a_local_targets_view_is_implemented_and_carries_no_credential() {
        // The window must be able to offer the local directory kind instead of an SFTP target that silently
        // became one, and it must not ask for a credential the kind cannot use.
        let config =
            veloct_attach::StorageConfig::new("on-this-machine", "Local", veloct_attach::StorageKind::Local);
        let view = StorageTargetView::of(&config, config.has_credential());

        assert_eq!(view.kind, "local");
        assert!(view.implemented);
        assert!(view.unimplemented_reason.is_none());
        assert!(view.credential_ref.is_none());
        assert!(
            !view.has_credential,
            "a local directory needs no credential, and the window must not report a missing one"
        );
    }

    #[test]
    fn the_remote_cleanup_command_is_registered_with_the_window() {
        // Section 13.8's cleanup was missing at BOTH layers: `StorageAdapter::delete_remote` existed and no
        // command exposed it. This asserts the registration the way the shell declares it, so the command
        // cannot be written and then left unreachable.
        let registry = include_str!("lib.rs");
        assert!(
            registry.contains("attachments::delete_remote_object"),
            "delete_remote_object must be in the invoke_handler list"
        );
    }

    #[test]
    fn storage_kinds_reports_every_kind_and_agrees_with_the_crate_for_all_of_them() {
        // The command must DELEGATE, not restate: for every kind in the canonical list its answer is compared
        // against the crate function the window's whole notion of "does this kind work" comes from. A second
        // copy of that knowledge is the defect this command exists to remove, and this is the assertion that
        // would catch it — a restated list would drift the moment one side is edited.
        let views = storage_kinds();
        assert_eq!(views.len(), veloct_attach::StorageKind::ALL.len());

        for view in &views {
            let kind = veloct_attach::StorageKind::parse(&view.kind)
                .unwrap_or_else(|| panic!("{} is not a spelling `parse` accepts back", view.kind));
            assert_eq!(
                view.implemented,
                veloct_attach::is_implemented(kind),
                "{}: implemented must be the crate's answer",
                view.kind
            );
            assert_eq!(
                view.unimplemented_reason.as_deref(),
                veloct_attach::unimplemented_reason(kind),
                "{}: the gap reason must be the crate's own words",
                view.kind
            );
        }

        // Every kind the crate knows is reported exactly once, in the canonical list's order.
        let spellings: Vec<&str> = views.iter().map(|view| view.kind.as_str()).collect();
        let expected: Vec<&str> = veloct_attach::StorageKind::ALL
            .iter()
            .map(|kind| kind.as_str())
            .collect();
        assert_eq!(spellings, expected);
    }

    #[test]
    fn storage_kinds_answers_without_any_project_state() {
        // The whole point: a kind-level fact about the BUILD, so a project with no target at all still learns
        // which kinds have an adapter. This function takes no `State`, which is what makes that structural
        // rather than a promise.
        let views = storage_kinds();
        assert!(
            !views.is_empty(),
            "the window needs the kind list before any target exists"
        );
        for view in &views {
            assert!(!view.kind.trim().is_empty());
            if !view.implemented {
                assert!(
                    view.unimplemented_reason
                        .as_deref()
                        .map(|reason| !reason.trim().is_empty())
                        .unwrap_or(false),
                    "{} is not implemented, so it must say why",
                    view.kind
                );
            }
        }
    }

    #[test]
    fn storage_kinds_reports_sftp_as_the_gap_and_every_other_kind_as_working() {
        // PROCESSING.md §5.3 keeps SFTP as a reported gap; §5.5's window has to be able to say so BEFORE a
        // target of that kind is saved, which is the case a target list alone cannot answer.
        //
        // Lsky Pro was in this gap loop until §5.4's protocol was read from the vendor's own artifacts and
        // `lsky::LskyStore` implemented it, so it moved to the working loop. The command's own answer is what
        // is asserted here, not a restatement of the crate's.
        let views = storage_kinds();
        let spelling = "sftp";
        let view = views
            .iter()
            .find(|view| view.kind == spelling)
            .unwrap_or_else(|| panic!("{spelling} must be reported"));
        assert!(!view.implemented, "{spelling} has no adapter in this build");
        let reason = view
            .unimplemented_reason
            .as_deref()
            .unwrap_or_else(|| panic!("{spelling} must report its gap"));
        assert!(!reason.trim().is_empty(), "{spelling}");

        for spelling in ["local", "s3", "github", "lsky"] {
            let view = views
                .iter()
                .find(|view| view.kind == spelling)
                .unwrap_or_else(|| panic!("{spelling} must be reported"));
            assert!(view.implemented, "{spelling} has an adapter");
            assert!(view.unimplemented_reason.is_none(), "{spelling}");
        }
    }

    #[test]
    fn the_storage_kinds_command_is_registered_with_the_window() {
        // A command the window cannot invoke is the gap this task exists to close, so the registration is
        // asserted where the shell declares it.
        assert!(
            include_str!("lib.rs").contains("attachments::storage_kinds"),
            "storage_kinds must be in the invoke_handler list"
        );
    }

    // ---------------------------------------------------------------------------------------------
    // The SAVE and DELETE paths, driven through the same functions the commands call. These go through
    // `save_target_config` / `delete_target_config` rather than calling `validate()` themselves: the defect
    // being closed is precisely that `validate()` had no caller, so a test that calls it directly would have
    // missed it.
    // ---------------------------------------------------------------------------------------------

    fn local_config(id: &str, prefix: &str) -> veloct_attach::StorageConfig {
        let mut config =
            veloct_attach::StorageConfig::new(id, "本机目录", veloct_attach::StorageKind::Local);
        config.key_prefix = Some(prefix.to_string());
        config
    }

    #[test]
    fn a_local_target_with_a_drive_letter_prefix_is_refused_when_it_is_saved_and_writes_nothing() {
        // The rule t2 added to `validate`'s `Local` arm, enforced where a user meets it: at SAVE time. Before
        // this task the only reader of that rule was a unit test, so the window accepted the prefix and the
        // escaping write was refused later, by `ensure_within`, at the first upload.
        let project = tempfile::TempDir::new().unwrap();

        let error = save_target_config(
            project.path(),
            local_config("on-this-machine", "D:/images"),
            None,
        )
        .expect_err("a drive-letter prefix must be refused");
        assert!(
            error.to_string().contains("D:/images"),
            "the message must name the prefix: {error}"
        );

        // A UNC root is the same mistake spelled differently.
        let error = save_target_config(
            project.path(),
            local_config("on-this-machine", "\\\\server\\share"),
            None,
        )
        .expect_err("a UNC prefix must be refused");
        assert!(error.to_string().contains("relative path"), "{error}");

        // NOTHING was written by either refusal — the rule runs before the list is touched.
        assert!(
            !project.path().join(STORAGE_TARGETS_FILE).exists(),
            "a refused save must not create the target list"
        );
        assert!(read_storage_targets(project.path()).unwrap().is_empty());
    }

    #[test]
    fn an_accepted_local_target_is_written_so_the_refusal_is_a_rule_and_not_a_blanket_no() {
        let project = tempfile::TempDir::new().unwrap();
        save_target_config(
            project.path(),
            local_config("on-this-machine", "uploads/2026"),
            None,
        )
        .unwrap();

        let list = read_storage_targets(project.path()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id.as_str(), "on-this-machine");
        assert_eq!(list[0].kind, veloct_attach::StorageKind::Local);
        assert_eq!(list[0].key_prefix.as_deref(), Some("uploads/2026"));
    }

    #[test]
    fn a_host_less_sftp_target_still_saves_and_the_list_reports_it_as_a_gap() {
        // The DECISION this exemption implements: `validate` is called only for a kind this build IMPLEMENTS.
        // A half-filled SFTP target is one of the ways §13.10's gap becomes visible in the target list, so it
        // must still save — which is why the arm that would refuse it is deliberately not consulted.
        let project = tempfile::TempDir::new().unwrap();
        let config =
            veloct_attach::StorageConfig::new("server", "my server", veloct_attach::StorageKind::Sftp);
        assert!(!veloct_attach::is_implemented(config.kind), "the exemption's premise");

        // The arm is still there and still refuses: it states the shape the adapter will need, so it is kept
        // rather than softened, and this asserts that keeping it is not the same as obeying it.
        let problem = config
            .validate()
            .expect_err("the SFTP arm still demands a host and a username");
        assert!(problem.contains("host"), "{problem}");

        // And the save path ACCEPTS it, because this build has no SFTP adapter.
        save_target_config(project.path(), config, None).unwrap();

        let saved = read_storage_targets(project.path()).unwrap();
        assert_eq!(saved.len(), 1, "an unimplemented kind is not refused");
        let view = StorageTargetView::of(&saved[0], saved[0].has_credential());
        assert_eq!(view.kind, "sftp");
        assert!(
            !view.implemented,
            "the target list still reports the gap, which is what the exemption preserves"
        );
        assert!(
            !view.unimplemented_reason.unwrap_or_default().trim().is_empty(),
            "and it reports WHY"
        );
    }

    #[test]
    fn an_implemented_kind_is_still_validated_where_an_unimplemented_one_is_exempt() {
        // The other half of the exemption, so it cannot be read as "nothing is validated any more": S3 IS
        // implemented, so its rule (a bucket) is enforced on the save path.
        let project = tempfile::TempDir::new().unwrap();
        let mut config =
            veloct_attach::StorageConfig::new("assets", "S3 附件", veloct_attach::StorageKind::S3);
        let error = save_target_config(project.path(), config.clone(), None)
            .expect_err("S3 needs a bucket");
        assert!(error.to_string().contains("bucket"), "{error}");
        assert!(!project.path().join(STORAGE_TARGETS_FILE).exists());

        config.bucket = Some("veloct-assets".to_string());
        save_target_config(project.path(), config, None).unwrap();
        assert_eq!(read_storage_targets(project.path()).unwrap().len(), 1);
    }

    #[test]
    fn deleting_a_target_forgets_its_stored_credential_and_an_unknown_id_changes_nothing() {
        // The OS credential store is process-wide and this test asserts against the REAL one, so it is
        // serialised with every other test that reads or writes it. See `credentials::store_guard`: with
        // the tests running in parallel, `exists` was observed returning a stale `true` immediately after
        // a `remove`, which failed this assertion.
        let _store = crate::credentials::store_guard();
        let project = tempfile::TempDir::new().unwrap();
        let id = "t9-credential-target";
        let handle = crate::credentials::handle_for(&format!("storage:{id}"));

        let mut config =
            veloct_attach::StorageConfig::new(id, "S3 附件", veloct_attach::StorageKind::S3);
        config.bucket = Some("veloct-assets".to_string());
        config.credential_ref = Some(handle.clone());
        // The value goes to the OS store, not to the file (§12.2).
        save_target_config(project.path(), config.clone(), Some("s3-secret".to_string())).unwrap();
        assert_eq!(read_storage_targets(project.path()).unwrap().len(), 1);

        // The credential really is in the store — otherwise "it was removed" would be vacuous. A machine
        // whose credential store refuses to keep a value cannot make that claim, and the test says so instead
        // of passing quietly; the removal path itself is asserted below whatever happens.
        let stored = crate::credentials::set(&handle, "s3-secret");
        assert!(
            stored.is_ok(),
            "this test needs a working credential store: {stored:?}"
        );
        assert!(crate::credentials::exists(&handle), "a value is stored under the handle");

        delete_target_config(project.path(), id).unwrap();

        assert!(
            read_storage_targets(project.path()).unwrap().is_empty(),
            "the target is gone from the list"
        );
        assert!(
            !crate::credentials::exists(&handle),
            "and its stored credential no longer resolves: the OS store is where §12.2 keeps it"
        );

        // Removing an id that does not exist is an EXPLICIT error, and the list is left exactly as it was.
        save_target_config(project.path(), config, None).unwrap();
        // A credential for the SURVIVING target, so the refusal can be shown not to touch it: a typo must not
        // be able to forget a real target's secret.
        crate::credentials::set(&handle, "kept").unwrap();
        assert!(crate::credentials::exists(&handle));

        let error = delete_target_config(project.path(), "not-a-target")
            .expect_err("an unknown id is an error rather than a silent no-op");
        assert!(error.to_string().contains("no storage target"), "{error}");
        let list = read_storage_targets(project.path()).unwrap();
        assert_eq!(list.len(), 1, "the list is unchanged by the refusal");
        assert_eq!(list[0].id.as_str(), id);
        assert!(
            crate::credentials::exists(&handle),
            "a refused delete must not forget the credential of the target it did not delete"
        );
        let _ = crate::credentials::remove(&handle);
    }
}
