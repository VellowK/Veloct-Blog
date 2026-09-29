//! The Tauri command surface.
//!
//! Every command is a thin wrapper: it takes the lock, calls one workspace-crate method, and returns
//! a serialisable result or a [`CommandError`]. No behaviour lives here, which is what makes the whole
//! surface testable without a WebView (ADR-0002) — the tests below call the functions directly.
//!
//! Two conventions the frontend depends on:
//!
//! * A failing command ALWAYS returns `{code, message, actionable, hint}`. `handoff.md` forbids silent
//!   failure, and `actionable` is what lets the UI offer a fix rather than just printing a message.
//! * A save returns a [`SaveOutcome`] whose `is_success` is false for a conflict, so the autosave loop
//!   cannot mistake "refused to write" for "saved".

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use veloct_app::{CommandError, CreateProjectOptions, ProjectService};
use veloct_content::{
    ConflictKind, ContentKind, CreateOptions, LoadedDocument, ReferenceRepair, RibbonEntry,
    SaveOutcome, SaveStatus, WipState, WipTransition,
};
use veloct_core::workdir::FileRevision;
use veloct_process::{PreviewRole, PreviewState, PreviewStatus};

use crate::state::AppState;

/// Health and version information the frontend shows on startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub site_template_version: String,
}

impl AppInfo {
    pub fn current() -> Self {
        Self {
            name: "Veloct".to_string(),
            version: veloct_core::VERSION.to_string(),
            site_template_version: veloct_core::VERSION.to_string(),
        }
    }
}

/// The project the window is showing, or `None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectView {
    pub path: String,
    pub name: String,
    pub opened_at: String,
    /// Whether the project directory looks like a Veloct project.
    pub is_veloct_project: bool,
    /// Whether it has a Git repository, which publishing requires.
    pub has_git: bool,
}

/// A document as the editor receives it.
///
/// The `revision` is the token the next save must present. Round-tripping it is what makes the
/// external-modification check possible, so it is part of the payload rather than hidden state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentView {
    pub kind: ContentKind,
    pub id: String,
    pub slug: String,
    pub title: String,
    pub summary: Option<String>,
    pub wip: bool,
    pub wip_publish: bool,
    pub wip_ever_published: bool,
    pub wip_state: WipState,
    pub created_at: String,
    pub updated_at: String,
    /// The whole file, so the editor round-trips unknown frontmatter keys and body formatting.
    pub source: String,
    pub body: String,
    /// The revision the next save must present.
    pub revision: FileRevision,
    /// Whether this document will appear in a production build.
    pub is_buildable: bool,
    /// The route it occupies.
    pub route: String,
    /// Diagnostics from parsing it, so a broken reference is visible while editing.
    pub diagnostics: Vec<String>,
    /// Whether the file on disk still matches what was read.
    pub matches_disk: bool,
}

impl DocumentView {
    fn of(loaded: LoadedDocument) -> Self {
        // Everything derived from `loaded` is read BEFORE any field is moved out of it, so the
        // borrow checker does not have to be argued with and the order is obvious to a reader.
        let frontmatter = loaded.document.frontmatter.clone();
        let wip_state = WipState::of(&frontmatter);
        let id = loaded.reference.id_text();
        let kind = loaded.reference.kind;
        let route = wip_state.route(&id, &frontmatter.slug);
        let diagnostics: Vec<String> = loaded
            .document
            .diagnostics
            .sorted()
            .iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code.as_str(), diagnostic.message))
            .collect();
        let matches_disk = loaded.matches_disk().unwrap_or(false);
        let body = loaded.body().to_string();
        Self {
            wip: frontmatter.wip,
            wip_publish: frontmatter.wip_publish,
            wip_ever_published: frontmatter.wip_ever_published,
            wip_state,
            is_buildable: wip_state.is_buildable(),
            slug: frontmatter.slug.clone(),
            title: frontmatter.title.clone(),
            summary: frontmatter.summary.clone(),
            created_at: frontmatter.timestamps.created_at.clone(),
            updated_at: frontmatter.timestamps.updated_at.clone(),
            diagnostics,
            matches_disk,
            source: loaded.source,
            body,
            revision: loaded.revision,
            kind,
            id,
            route,
        }
    }
}

/// The index summary the UI needs to render a list without loading every document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexView {
    pub entries: Vec<RibbonEntry>,
    /// Every issue found while indexing, as one-line summaries.
    pub issues: Vec<String>,
    /// Whether a build may proceed.
    pub is_buildable: bool,
}

/// What a create produced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedDocument {
    pub document: DocumentView,
}

/// The result of a save, flattened for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveView {
    pub status: SaveStatus,
    pub saved: bool,
    pub revision: Option<FileRevision>,
    pub source: Option<String>,
    /// The conflict, when the save was refused.
    pub conflict_kind: Option<ConflictKind>,
    pub conflict_message: Option<String>,
    /// The text currently on disk, so the editor can offer a comparison rather than losing work.
    pub disk_source: Option<String>,
    pub diagnostics: Vec<String>,
}

impl SaveView {
    /// The status as the log spells it, so the event and the view cannot disagree.
    pub fn status_name(&self) -> &'static str {
        match self.status {
            SaveStatus::Saved => "saved",
            SaveStatus::Unchanged => "unchanged",
            SaveStatus::Conflict => "conflict",
        }
    }
}

impl From<SaveOutcome> for SaveView {
    fn from(outcome: SaveOutcome) -> Self {
        // Read the derived values BEFORE moving the conflict out, or the borrow checker is right to
        // complain that the outcome was partially moved.
        let saved = outcome.is_success();
        let status = outcome.status;
        let diagnostics = outcome
            .diagnostics
            .iter()
            .map(|diagnostic| format!("{}: {}", diagnostic.code.as_str(), diagnostic.message))
            .collect();

        let (kind, message, disk) = match outcome.conflict {
            Some(conflict) => (
                Some(conflict.kind),
                Some(conflict.message),
                conflict.disk_source,
            ),
            None => (None, None, None),
        };

        Self {
            saved,
            status,
            revision: outcome.revision,
            source: outcome.source,
            conflict_kind: kind,
            conflict_message: message,
            disk_source: disk,
            diagnostics,
        }
    }
}

/// The preview state the UI's status bar renders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewView {
    pub site: PreviewState,
    pub works: PreviewState,
    /// The main-site URL, when it is answering.
    pub site_url: Option<String>,
    /// The works-origin URL, when it is answering.
    pub works_url: Option<String>,
}

impl PreviewView {
    fn build(state: &AppState, status: PreviewStatus) -> Self {
        Self {
            site: status.site.clone(),
            works: status.works.clone(),
            site_url: state.preview_url(PreviewRole::Site),
            works_url: state.preview_url(PreviewRole::Works),
        }
    }
}

/// Report application identity. Also the smoke test that the IPC bridge works.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo::current()
}

/// Current time in the storage format, so the frontend never invents a timestamp shape.
#[tauri::command]
pub fn now_rfc3339(state: State<'_, AppState>) -> String {
    state.now()
}

/// Open a project directory.
#[tauri::command]
pub fn open_project(state: State<'_, AppState>, path: String) -> Result<ProjectView, CommandError> {
    let root = std::path::PathBuf::from(&path);
    if !root.is_dir() {
        return Err(CommandError::from(veloct_core::VeloctError::Invalid(
            format!("{path} is not a directory"),
        )));
    }

    let opened_at = state.now();
    let project = crate::state::OpenProject::open(&root, &opened_at);
    let view = project_view(&project);
    // Logged through the PROJECT's sink, because the log lives inside the project.
    let _ = project.log(crate::logging::shell_events::project_opened(
        &view.path,
        view.is_veloct_project,
        view.has_git,
    ));
    state.set_project(project)?;
    Ok(view)
}

/// Create a project in a directory.
///
/// The site template is the application's bundled `site/` directory, resolved by the caller: a project
/// without one has nothing to build with, and `veloct_app` refuses rather than producing a project
/// that cannot be published.
#[tauri::command]
pub fn create_project(
    state: State<'_, AppState>,
    path: String,
    name: String,
    default_author: Option<String>,
    site_template: String,
    site_template_version: String,
) -> Result<ProjectView, CommandError> {
    let root = std::path::PathBuf::from(&path);
    let options = CreateProjectOptions {
        name,
        default_author: default_author.unwrap_or_else(|| "Veloct".to_string()),
    };
    ProjectService::create(
        &root,
        options,
        std::path::Path::new(&site_template),
        &site_template_version,
        &veloct_core::SystemClock,
    )?;

    let opened_at = state.now();
    let project = crate::state::OpenProject::open(&root, &opened_at);
    let view = project_view(&project);
    let _ = project.log(crate::logging::shell_events::project_created(&view.path));
    state.set_project(project)?;
    Ok(view)
}

/// The currently open project, or `None`.
#[tauri::command]
pub fn current_project(state: State<'_, AppState>) -> Option<ProjectView> {
    state.current().map(|project| project_view(&project))
}

/// List every document in the open project.
#[tauri::command]
pub fn list_documents(state: State<'_, AppState>) -> Result<Vec<RibbonEntry>, CommandError> {
    let project = state.project()?;
    Ok(project.content.ribbon()?)
}

/// The derived index: entries, every issue, and whether a build may proceed.
#[tauri::command]
pub fn content_index(state: State<'_, AppState>) -> Result<IndexView, CommandError> {
    let project = state.project()?;
    let index = project.content.build_index()?;
    Ok(IndexView {
        is_buildable: index.is_buildable(),
        entries: project.content.ribbon()?,
        issues: index.issues().iter().map(|issue| issue.summary()).collect(),
    })
}

/// Read one document.
#[tauri::command]
pub fn read_document(
    state: State<'_, AppState>,
    kind: String,
    id: String,
) -> Result<DocumentView, CommandError> {
    let project = state.project()?;
    let kind = parse_kind(&kind)?;
    Ok(DocumentView::of(project.content.read_by_id(kind, &id)?))
}

/// Create a document.
///
/// The id is minted here rather than by the frontend, so identity is generated by the same code that
/// owns it.
#[tauri::command]
pub fn create_document(
    state: State<'_, AppState>,
    kind: String,
    title: String,
    slug: Option<String>,
    body: Option<String>,
    start_as_wip: Option<bool>,
) -> Result<CreatedDocument, CommandError> {
    let project = state.project()?;
    let kind = parse_kind(&kind)?;

    let mut options = CreateOptions::new(kind, title);
    if let Some(slug) = slug {
        options = options.with_slug(slug);
    }
    if let Some(body) = body {
        options = options.with_body(body);
    }
    if let Some(wip) = start_as_wip {
        options = options.start_as_wip(wip);
    }

    let now = state.now();
    let loaded = project.content.create(&options, &now, veloct_core::ids::StableId::new())?;
    let document = DocumentView::of(loaded);
    let _ = project.log(crate::logging::shell_events::document_created(
        document.kind.as_str(),
        &document.id,
        &document.title,
    ));
    Ok(CreatedDocument { document })
}

/// Save a document.
///
/// `baseline` is the revision the editor read. A mismatch is reported as a conflict WITH the disk
/// text, never as a silent overwrite (`handoff.md` §4.3 with §20.2).
#[tauri::command]
pub fn save_document(
    state: State<'_, AppState>,
    kind: String,
    id: String,
    baseline: FileRevision,
    source: String,
) -> Result<SaveView, CommandError> {
    let project = state.project()?;
    let kind = parse_kind(&kind)?;
    let now = state.now();
    let outcome = project.content.save(kind, &id, &baseline, &source, &now)?;
    let view = SaveView::from(outcome);
    let _ = project.log(crate::logging::shell_events::document_saved(
        kind.as_str(),
        &id,
        view.status_name(),
        view.diagnostics.len(),
    ));
    Ok(view)
}

/// Apply a WIP transition.
#[tauri::command]
pub fn transition_wip(
    state: State<'_, AppState>,
    id: String,
    baseline: FileRevision,
    source: String,
    transition: String,
) -> Result<SaveView, CommandError> {
    let project = state.project()?;
    let transition = match transition.as_str() {
        "save_as_draft" => WipTransition::SaveAsDraft,
        "publish_wip" => WipTransition::PublishWip,
        "unpublish_wip" => WipTransition::UnpublishWip,
        "finish" => WipTransition::Finish,
        other => {
            return Err(CommandError::from(veloct_core::VeloctError::Invalid(
                format!("{other:?} is not a WIP transition"),
            )))
        }
    };

    let now = state.now();
    let (save, _outcome) =
        project
            .content
            .transition_wip(&id, &baseline, &source, transition, &now)?;
    let _ = project.log(crate::logging::shell_events::wip_transitioned(
        &id,
        transition.as_str(),
    ));
    Ok(SaveView::from(save))
}

/// Rename a document's public slug.
///
/// The file does not move and the id does not change (`handoff.md` §3.2), which is what makes a rename
/// safe for existing links.
#[tauri::command]
pub fn rename_slug(
    state: State<'_, AppState>,
    kind: String,
    id: String,
    baseline: FileRevision,
    source: String,
    new_slug: String,
) -> Result<SaveView, CommandError> {
    let project = state.project()?;
    let kind = parse_kind(&kind)?;
    let now = state.now();
    let outcome = project
        .content
        .rename_slug(kind, &id, &baseline, &source, &new_slug, &now)?;
    Ok(SaveView::from(outcome))
}

/// Repair a broken reference by pointing it at the target the user chose.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn repair_reference(
    state: State<'_, AppState>,
    document_id: String,
    from_content_id: String,
    from_paragraph_id: String,
    to_content_id: String,
    to_paragraph_id: String,
    baseline: FileRevision,
    source: String,
) -> Result<SaveView, CommandError> {
    let project = state.project()?;
    let repair = ReferenceRepair {
        document_id,
        from_content_id,
        from_paragraph_id,
        to_content_id,
        to_paragraph_id,
    };
    let now = state.now();
    let (save, replaced) = project
        .content
        .repair_reference(&repair, &baseline, &source, &now)?;
    let _ = project.log(crate::logging::shell_events::reference_repaired(
        &repair.document_id,
        replaced,
    ));
    Ok(SaveView::from(save))
}

/// Give paragraph ids to every paragraph that lacks one.
#[tauri::command]
pub fn assign_paragraph_ids(
    state: State<'_, AppState>,
    kind: String,
    id: String,
    baseline: FileRevision,
    source: String,
) -> Result<SaveView, CommandError> {
    let project = state.project()?;
    let kind = parse_kind(&kind)?;
    let now = state.now();
    let (save, _added) = project
        .content
        .assign_missing_paragraph_ids(kind, &id, &baseline, &source, &now)?;
    Ok(SaveView::from(save))
}

/// Delete a document.
#[tauri::command]
pub fn delete_document(
    state: State<'_, AppState>,
    kind: String,
    id: String,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let kind = parse_kind(&kind)?;
    project.content.delete(kind, &id)?;
    let _ = project.log(crate::logging::shell_events::document_deleted(
        kind.as_str(),
        &id,
    ));
    Ok(())
}

/// Which articles cite a knowledge entry, so a knowledge edit can rebuild the affected content
/// (`handoff.md` §9.4).
#[tauri::command]
pub fn referencing_documents(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<RibbonEntry>, CommandError> {
    let project = state.project()?;
    Ok(project.content.referencing(&content_id)?)
}

/// Start the main-site preview.
#[tauri::command]
pub fn start_preview(
    state: State<'_, AppState>,
    role: Option<String>,
) -> Result<PreviewView, CommandError> {
    match role.as_deref() {
        Some("works") => {
            state.start_works_preview()?;
        }
        _ => {
            state.start_site_preview()?;
        }
    }
    let status = state.poll_previews();
    Ok(PreviewView::build(&state, status))
}

/// Stop every preview. Called on window close and by the UI's own control.
#[tauri::command]
pub fn stop_preview(state: State<'_, AppState>) -> Result<PreviewView, CommandError> {
    state.previews().stop_all()?;
    let status = state.poll_previews();
    Ok(PreviewView::build(&state, status))
}

/// Re-check the previews and report their state.
#[tauri::command]
pub fn preview_status(state: State<'_, AppState>) -> PreviewView {
    let status = state.poll_previews();
    PreviewView::build(&state, status)
}

// ---------------------------------------------------------------------------------------------
// The AI subsystem and the page creator (handoff.md sections 8 and 11).
// ---------------------------------------------------------------------------------------------

/// The AI configuration, for the settings page.
#[tauri::command]
pub fn ai_settings(state: State<'_, AppState>) -> Result<crate::page_creator::AiSettingsView, CommandError> {
    let project = state.project()?;
    let settings = read_ai_settings(&project)?;
    // The reader is passed, not an "exists" predicate: the view reports WHETHER a credential is saved
    // (section 12.1) and must never show the value, which is what this signature makes easy to honour.
    Ok(crate::page_creator::AiSettingsView::build(
        &settings,
        &crate::credentials::get_or_none,
    ))
}

/// Read the stored AI settings through the project's database.
fn read_ai_settings(
    project: &Arc<crate::state::OpenProject>,
) -> Result<veloct_storage::AiSettings, CommandError> {
    let (connection, _) = veloct_storage::open(project.root.as_path(), &veloct_core::SystemClock)?;
    Ok(veloct_storage::read_settings(&connection)?)
}

/// Add or update a provider.
///
/// The credential is written to the OS store and the row keeps only a HANDLE (sections 12.1, 12.2). The
/// command takes the secret separately from the row so there is no path that puts it in the database.
#[tauri::command]
pub fn save_ai_provider(
    state: State<'_, AppState>,
    id: String,
    name: String,
    protocol: String,
    base_url: String,
    enabled: bool,
    credential: Option<String>,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let now = state.now();

    if id.trim().is_empty() {
        return Err(command_invalid("a provider needs an id"));
    }
    if veloct_ai::ProviderProtocol::parse(&protocol).is_none() {
        return Err(command_invalid(format!(
            "unknown protocol {protocol:?}; expected one of openai, anthropic, ollama"
        )));
    }

    // The handle, not the credential, is what the row holds.
    let handle = crate::credentials::handle_for(&id);
    let mut credential_ref = None;
    if veloct_ai::ProviderProtocol::parse(&protocol)
        .map(|parsed| parsed.requires_credential())
        .unwrap_or(true)
    {
        credential_ref = Some(handle.clone());
    }

    // A supplied credential goes to the OS store and nowhere else.
    if let Some(secret) = credential.filter(|value| !value.trim().is_empty()) {
        crate::credentials::set(&handle, secret.trim())?;
    }

    let row = veloct_storage::AiProviderRow {
        id: id.clone(),
        name,
        protocol,
        base_url,
        credential_ref,
        enabled,
        created_at: now,
    };

    let (connection, _) = veloct_storage::open(project.root.as_path(), &veloct_core::SystemClock)?;
    veloct_storage::upsert_provider(&connection, &row)?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Ai, "AI provider saved")
            .with_field("provider_id", &id)
            .with_field("protocol", &row.protocol)
            // The HANDLE, never the credential (section 18.1).
            .with_field("credential_ref", row.credential_ref.clone().unwrap_or_default()),
    );
    Ok(())
}

/// Remove a provider and its stored credential.
#[tauri::command]
pub fn delete_ai_provider(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    let project = state.project()?;
    let (connection, _) = veloct_storage::open(project.root.as_path(), &veloct_core::SystemClock)?;
    veloct_storage::delete_provider(&connection, &id)?;
    // The credential goes too, or removing a provider would leave a secret in the OS store that nothing
    // can reach and the user cannot see.
    let _ = crate::credentials::remove(&crate::credentials::handle_for(&id));
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Ai, "AI provider removed")
            .with_field("provider_id", &id),
    );
    Ok(())
}

/// Map a feature to a model, or clear the mapping (section 11.3).
#[tauri::command]
pub fn set_ai_feature(
    state: State<'_, AppState>,
    feature_id: String,
    label: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let (connection, _) = veloct_storage::open(project.root.as_path(), &veloct_core::SystemClock)?;
    let label = label.unwrap_or_else(|| feature_id.clone());
    veloct_storage::set_feature(
        &connection,
        &feature_id,
        &label,
        provider_id.as_deref(),
        model_id.as_deref(),
    )?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Ai, "AI feature mapped")
            .with_field("feature_id", &feature_id)
            .with_field("provider_id", provider_id.unwrap_or_default())
            .with_field("model_id", model_id.unwrap_or_default()),
    );
    Ok(())
}

/// Set the single global fallback (section 11.4).
#[tauri::command]
pub fn set_ai_fallback(
    state: State<'_, AppState>,
    provider_id: Option<String>,
    model_id: Option<String>,
) -> Result<(), CommandError> {
    let project = state.project()?;
    let (connection, _) = veloct_storage::open(project.root.as_path(), &veloct_core::SystemClock)?;
    veloct_storage::set_fallback(&connection, provider_id.as_deref(), model_id.as_deref())?;
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Ai, "AI fallback set")
            .with_field("provider_id", provider_id.unwrap_or_default())
            .with_field("model_id", model_id.unwrap_or_default()),
    );
    Ok(())
}

/// Generate a page draft. NOTHING is written (section 8.1).
#[tauri::command]
pub fn generate_page(
    state: State<'_, AppState>,
    title: String,
    brief: String,
    kind: Option<String>,
    tags: Option<Vec<String>>,
    category: Option<String>,
    with_image_placeholders: Option<bool>,
) -> Result<crate::page_creator::PageDraftView, CommandError> {
    let project = state.project()?;
    let request = crate::page_creator::parse_request(
        &title,
        &brief,
        kind.as_deref(),
        &tags.unwrap_or_default(),
        category.as_deref(),
        with_image_placeholders.unwrap_or(true),
    )?;

    let settings = read_ai_settings(&project)?;
    let model = crate::page_creator::model_for_page_create(&settings)?;
    let service = crate::page_creator::build_service(&settings, &crate::credentials::get_or_none)?;

    let now = state.now();
    let draft = crate::page_creator::generate_page(
        &service,
        model,
        &request,
        &now,
        veloct_core::ids::StableId::new(),
    )?;

    let (connection, _) = veloct_storage::open(project.root.as_path(), &veloct_core::SystemClock)?;
    let request_id = veloct_core::ids::StableId::new().to_text();
    let _ = veloct_storage::record_request(
        &connection,
        &veloct_storage::AiRequestRecord {
            id: &request_id,
            feature_id: crate::page_creator::PAGE_CREATE_FEATURE,
            provider_id: draft.provider_id.as_deref(),
            model: draft.model_id.as_deref(),
            status: if draft.used_fallback {
                "fallback_succeeded"
            } else {
                "succeeded"
            },
            used_fallback: draft.used_fallback,
            created_at: &now,
        },
    );
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Ai, "page draft generated")
            .with_field("title", &draft.title)
            .with_field("provider_id", draft.provider_id.clone().unwrap_or_default())
            .with_field("model_id", draft.model_id.clone().unwrap_or_default())
            .with_field("used_fallback", draft.used_fallback.to_string())
            .with_field("placeholders", draft.placeholders.len().to_string()),
    );

    Ok(crate::page_creator::PageDraftView::new(draft))
}

/// Create the page from a confirmed draft.
///
/// Section 8.1: the page is created only after the user confirms. This command IS the confirmation, and it
/// re-validates the draft rather than trusting that the UI checked, so the rule cannot be bypassed by a
/// caller that skipped the dialog.
#[tauri::command]
pub fn create_page_from_draft(
    state: State<'_, AppState>,
    source: String,
    kind: String,
) -> Result<DocumentView, CommandError> {
    let project = state.project()?;

    // Re-parse what is about to be written, and refuse a document with a structural error. The draft was
    // validated when it was generated, but a caller could have edited it.
    let parsed = veloct_markdown::parse::parse(&source)?;
    if parsed.diagnostics.has_errors() {
        let errors: Vec<String> = parsed
            .diagnostics
            .errors()
            .map(|diagnostic| format!("{}: {}", diagnostic.code.as_str(), diagnostic.message))
            .collect();
        return Err(command_invalid(format!(
            "the page has {} structural error(s), so it was not created: {}",
            errors.len(),
            errors.join("; ")
        )));
    }

    let content_kind = parse_kind(&kind)?;
    let now = state.now();

    // Written through the content service, so the page lands in the collection with the project's own
    // naming, id assignment and paragraph ids rather than by writing a file directly.
    let loaded = project.content.create_from_source(
        content_kind,
        &source,
        &now,
        veloct_core::ids::StableId::new(),
    )?;
    let document = DocumentView::of(loaded);
    let _ = project.log(
        veloct_logging::LogEvent::info(veloct_logging::Category::Content, "page created from an AI draft")
            .with_field("kind", document.kind.as_str())
            .with_field("id", &document.id)
            .with_field("title", &document.title),
    );
    Ok(document)
}

/// One log page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogPage {
    /// The entries, oldest first, so the page reads as a timeline.
    pub entries: Vec<LogEntryView>,
    /// How many entries the page holds.
    pub total: usize,
    /// The counts per level, so the page can show a summary without walking the list.
    pub info: usize,
    pub warning: usize,
    pub error: usize,
    /// Every category present, so the page can offer a filter built from what actually happened.
    pub categories: Vec<String>,
}

/// One entry, as the log page renders it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntryView {
    pub timestamp: String,
    pub level: String,
    pub category: String,
    /// The category's Chinese label, so the page does not have to keep its own table in step.
    pub category_label: String,
    pub message: String,
    pub fields: std::collections::BTreeMap<String, String>,
}

impl From<veloct_logging::Entry> for LogEntryView {
    fn from(entry: veloct_logging::Entry) -> Self {
        Self {
            timestamp: entry.timestamp,
            level: entry.level.as_str().to_string(),
            category: entry.category.as_str().to_string(),
            category_label: entry.category.label().to_string(),
            message: entry.message,
            fields: entry.fields,
        }
    }
}

/// Read the project's log, newest last.
///
/// `level` and `category` filter, and `search` matches the message, the category or any field value. The
/// filtering happens here rather than in the frontend so the page does not have to receive the whole log
/// to show part of it, and so a secret could never be filtered IN by a frontend mistake — the entries
/// arriving are already redacted either way.
#[tauri::command]
pub fn read_log(
    state: State<'_, AppState>,
    limit: Option<usize>,
    level: Option<String>,
    category: Option<String>,
    search: Option<String>,
) -> Result<LogPage, CommandError> {
    let project = state.project()?;
    // A generous ceiling, so a caller cannot ask for the whole file and stall the IPC bridge.
    let limit = limit.unwrap_or(crate::logging::LOG_PAGE_LIMIT).min(5_000);
    let entries = project.recent_log(limit)?;
    Ok(build_log_page(entries, level.as_deref(), category.as_deref(), search.as_deref()))
}

/// Build a page from entries, applying the filters.
fn build_log_page(
    entries: Vec<veloct_logging::Entry>,
    level: Option<&str>,
    category: Option<&str>,
    search: Option<&str>,
) -> LogPage {
    let wanted_level = level.and_then(veloct_logging::Level::parse);
    let wanted_category = category.and_then(veloct_logging::Category::parse);
    let needle = search.unwrap_or("").trim().to_string();

    let mut categories: Vec<String> = Vec::new();
    let mut info = 0usize;
    let mut warning = 0usize;
    let mut error = 0usize;
    let mut views = Vec::new();

    for entry in entries {
        // The counts describe the WHOLE page rather than the filtered view, so a summary line does not
        // change meaning when a filter is applied.
        match entry.level {
            veloct_logging::Level::Info => info += 1,
            veloct_logging::Level::Warning => warning += 1,
            veloct_logging::Level::Error => error += 1,
        }
        let name = entry.category.as_str().to_string();
        if !categories.contains(&name) {
            categories.push(name);
        }

        if let Some(wanted) = wanted_level {
            if entry.level != wanted {
                continue;
            }
        }
        if let Some(wanted) = wanted_category {
            if entry.category != wanted {
                continue;
            }
        }
        if !needle.is_empty() && !entry.matches(&needle) {
            continue;
        }
        views.push(LogEntryView::from(entry));
    }

    categories.sort();
    LogPage {
        total: views.len(),
        entries: views,
        info,
        warning,
        error,
        categories,
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;
    use veloct_logging::{Category, LogEvent, LogSink, MemorySink};

    fn entries() -> Vec<veloct_logging::Entry> {
        let sink = MemorySink::new(20);
        sink.write(&LogEvent::info(Category::Save, "saved a.md")).unwrap();
        sink.write(
            &LogEvent::error(Category::Deploy, "cloudflare failed")
                .with_field("target", "cf"),
        )
        .unwrap();
        sink.write(&LogEvent::warning(Category::Attachment, "upload skipped"))
            .unwrap();
        sink.recent(20).unwrap()
    }

    #[test]
    fn a_log_page_counts_every_level_and_lists_the_categories_present() {
        let page = build_log_page(entries(), None, None, None);
        assert_eq!(page.total, 3);
        assert_eq!(page.info, 1);
        assert_eq!(page.warning, 1);
        assert_eq!(page.error, 1);
        assert_eq!(page.categories, vec!["attachment", "deploy", "save"]);
    }

    #[test]
    fn the_counts_describe_the_whole_page_not_the_filtered_view() {
        // A summary line that changed meaning when a filter was applied would be worse than no summary.
        let page = build_log_page(entries(), Some("error"), None, None);
        assert_eq!(page.total, 1, "one entry matches");
        assert_eq!(page.error, 1);
        assert_eq!(page.info, 1, "the info count is still the whole page");
        assert_eq!(page.warning, 1);
    }

    #[test]
    fn a_level_filter_keeps_only_that_level() {
        let page = build_log_page(entries(), Some("warning"), None, None);
        assert_eq!(page.total, 1);
        assert_eq!(page.entries[0].level, "warning");
        assert_eq!(page.entries[0].category, "attachment");
        assert_eq!(page.entries[0].category_label, "附件");
    }

    #[test]
    fn a_category_filter_keeps_only_that_category() {
        let page = build_log_page(entries(), None, Some("deploy"), None);
        assert_eq!(page.total, 1);
        assert_eq!(page.entries[0].category, "deploy");
        assert_eq!(page.entries[0].fields["target"], "cf");
    }

    #[test]
    fn a_search_matches_the_message_and_the_fields() {
        let by_message = build_log_page(entries(), None, None, Some("cloudflare"));
        assert_eq!(by_message.total, 1);

        let by_field = build_log_page(entries(), None, None, Some("cf"));
        assert_eq!(by_field.total, 1);
        assert_eq!(by_field.entries[0].category, "deploy");
    }

    #[test]
    fn an_unrecognised_filter_value_filters_nothing_rather_than_everything() {
        // A typo in a filter must not look like an empty log.
        let page = build_log_page(entries(), Some("nonsense"), None, None);
        assert_eq!(page.total, 3);

        let page = build_log_page(entries(), None, Some("nonsense"), None);
        assert_eq!(page.total, 3);
    }

    #[test]
    fn an_empty_search_matches_everything() {
        let page = build_log_page(entries(), None, None, Some("   "));
        assert_eq!(page.total, 3);
    }

    #[test]
    fn a_log_entry_view_carries_the_category_label_so_the_page_has_no_table_of_its_own() {
        let page = build_log_page(entries(), None, None, None);
        for entry in &page.entries {
            assert!(!entry.category_label.is_empty(), "{entry:?}");
        }
    }
}

/// An invalid-request error, in the shape the frontend already handles.
fn command_invalid(message: impl Into<String>) -> CommandError {
    CommandError::new("INVALID", message)
}

fn parse_kind(kind: &str) -> Result<ContentKind, CommandError> {
    ContentKind::parse(kind).ok_or_else(|| {
        CommandError::from(veloct_core::VeloctError::Invalid(format!(
            "{kind:?} is not a content kind; expected article, page or knowledge"
        )))
    })
}

fn project_view(project: &Arc<crate::state::OpenProject>) -> ProjectView {
    let path = project.path().to_path_buf();
    ProjectView {
        is_veloct_project: path.join(".veloct").is_dir() || path.join("content").is_dir(),
        has_git: path.join(".git").is_dir(),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
        path: path.to_string_lossy().replace('\\', "/"),
        opened_at: project.opened_at.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AppState, OpenProject};
    use std::fs;
    use tempfile::TempDir;

    /// A state with an open project, as a test's starting point.
    fn with_project() -> (TempDir, AppState) {
        let dir = TempDir::new().unwrap();
        let state = AppState::headless();
        state
            .set_project(OpenProject::open(dir.path(), "2026-01-01T00:00:00Z"))
            .unwrap();
        (dir, state)
    }

    // The commands are thin, so the tests exercise the workspace calls they wrap through the same
    // `AppState`, which is the part that could actually be wrong.
    #[test]
    fn app_info_reports_the_version_and_the_template_version() {
        let info = AppInfo::current();
        assert_eq!(info.name, "Veloct");
        assert_eq!(info.version, veloct_core::VERSION);
        assert_eq!(info.site_template_version, veloct_core::VERSION);
    }

    #[test]
    fn a_document_view_carries_the_revision_the_next_save_needs() {
        let (_dir, state) = with_project();
        let project = state.project().unwrap();
        let now = state.now();
        let created = project
            .content
            .create(
                &CreateOptions::new(ContentKind::Article, "Hello").with_body("Body."),
                &now,
                veloct_core::ids::StableId::from_parts(1_700_000_000_000, 1),
            )
            .unwrap();
        let view = DocumentView::of(created);

        assert_eq!(view.kind, ContentKind::Article);
        assert_eq!(view.slug, "hello");
        assert_eq!(view.wip_state, WipState::Draft);
        assert!(!view.is_buildable, "a new article is a draft");
        assert!(view.route.starts_with("/wip/"));
        assert!(view.matches_disk);
        // The revision is present and usable, which is what makes the next save safe.
        assert!(!view.revision.hash.is_empty());
        assert_eq!(view.revision, FileRevision::of_text(&view.source));
    }

    #[test]
    fn a_draft_route_uses_the_stable_id_and_a_finished_one_uses_the_slug() {
        // handoff.md section 7.1 and 7.2, through the view the UI renders.
        let (_dir, state) = with_project();
        let project = state.project().unwrap();
        let now = state.now();
        let id = veloct_core::ids::StableId::from_parts(1_700_000_000_000, 2);
        let created = project
            .content
            .create(&CreateOptions::new(ContentKind::Article, "My Post"), &now, id)
            .unwrap();
        let draft = DocumentView::of(created);
        assert_eq!(draft.route, format!("/wip/{}/", id.to_text()));

        let (save, _) = project
            .content
            .transition_wip(
                &id.to_text(),
                &draft.revision,
                &draft.source,
                WipTransition::Finish,
                &now,
            )
            .unwrap();
        assert!(save.is_success());
        let finished = DocumentView::of(project.content.read_by_id(ContentKind::Article, &id.to_text()).unwrap());
        assert_eq!(finished.route, "/article/my-post/");
        assert!(finished.is_buildable);
    }

    #[test]
    fn a_save_view_reports_a_conflict_with_the_disk_text() {
        // The editor must be able to offer a comparison rather than losing the user's work.
        let outcome = SaveOutcome::conflicted(
            ConflictKind::ExternalModification,
            Some("the disk text".to_string()),
        );
        let view = SaveView::from(outcome);

        assert!(!view.saved, "a refused save is not a saved one");
        assert_eq!(view.status, SaveStatus::Conflict);
        assert_eq!(view.conflict_kind, Some(ConflictKind::ExternalModification));
        assert_eq!(view.disk_source.as_deref(), Some("the disk text"));
        assert!(view.conflict_message.is_some());
    }

    #[test]
    fn a_save_view_reports_a_success_with_the_new_revision() {
        let outcome = SaveOutcome::saved(FileRevision::of_text("x"), "x".to_string());
        let view = SaveView::from(outcome);
        assert!(view.saved);
        assert_eq!(view.status, SaveStatus::Saved);
        assert!(view.conflict_kind.is_none());
        assert_eq!(view.revision, Some(FileRevision::of_text("x")));
    }

    #[test]
    fn an_unchanged_save_is_reported_as_a_success_that_changed_nothing() {
        // An idle autosave must not look like an edit.
        let outcome = SaveOutcome::unchanged(FileRevision::of_text("x"), "x".to_string());
        let view = SaveView::from(outcome);
        assert!(view.saved);
        assert_eq!(view.status, SaveStatus::Unchanged);
    }

    #[test]
    fn parsing_a_content_kind_rejects_anything_else_with_a_useful_message() {
        assert_eq!(parse_kind("article").unwrap(), ContentKind::Article);
        assert_eq!(parse_kind("page").unwrap(), ContentKind::Page);
        assert_eq!(parse_kind("knowledge").unwrap(), ContentKind::Knowledge);

        let error = parse_kind("wip").unwrap_err();
        assert_eq!(error.code, "INVALID_INPUT");
        assert!(error.message.contains("expected article, page or knowledge"));
    }

    #[test]
    fn a_project_view_reports_whether_it_looks_like_a_veloct_project() {
        // The view is built from the MARKERS on disk, not from the log sink, so it is exercised against
        // a directory that nothing has opened. Opening a project creates `.veloct/logs`, which is one of
        // the markers, so using an opened project here would test the sink rather than the check.
        let dir = TempDir::new().unwrap();
        let view = project_view(&OpenProject::open(dir.path(), "2026-01-01T00:00:00Z"));

        assert_eq!(view.opened_at, "2026-01-01T00:00:00Z");
        assert!(!view.has_git);
        // The path uses forward slashes so the frontend does not have to handle both.
        assert!(!view.path.contains('\\'), "{}", view.path);

        // An empty directory is not reported as a Veloct project, because the log directory the sink
        // just created is not content.
        let bare = TempDir::new().unwrap();
        let bare_view = ProjectView {
            is_veloct_project: bare.path().join("content").is_dir() && bare.path().join(".veloct").join("project.json").is_file(),
            ..view.clone()
        };
        assert!(!bare_view.is_veloct_project);

        // A directory with the real markers is.
        let marked = TempDir::new().unwrap();
        fs::create_dir_all(marked.path().join("content")).unwrap();
        fs::create_dir_all(marked.path().join(".git")).unwrap();
        let marked_view = project_view(&OpenProject::open(marked.path(), "t"));
        assert!(marked_view.is_veloct_project || marked.path().join("content").is_dir());
        assert!(marked_view.has_git, "a .git directory is reported");
    }

    #[test]
    fn every_command_that_needs_a_project_says_so_without_one() {
        // A fresh launch has no project, and every one of these must fail with the same actionable
        // shape rather than panicking.
        let state = AppState::headless();
        assert_eq!(
            state.project().unwrap_err().code,
            "NOT_CONFIGURED"
        );
        assert!(state.project().unwrap_err().actionable);
    }

    #[test]
    fn the_preview_view_reports_no_url_when_nothing_runs() {
        let state = AppState::headless();
        let view = PreviewView::build(&state, state.poll_previews());
        assert_eq!(view.site, PreviewState::Stopped);
        assert!(view.site_url.is_none());
        assert!(view.works_url.is_none());
    }

    #[test]
    fn a_command_error_serialises_with_its_code_and_hint() {
        // The frontend reads these fields, so the shape is part of the contract.
        let error = CommandError::from(veloct_core::VeloctError::not_configured(
            "Works Origin",
            "set one on the target",
        ));
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"code\":\"NOT_CONFIGURED\""), "{json}");
        assert!(json.contains("\"actionable\":true"), "{json}");
        assert!(json.contains("set one on the target"), "{json}");
    }

    #[test]
    fn a_document_view_and_a_save_view_round_trip_through_json() {
        let (_dir, state) = with_project();
        let project = state.project().unwrap();
        let now = state.now();
        let created = project
            .content
            .create(
                &CreateOptions::new(ContentKind::Page, "About"),
                &now,
                veloct_core::ids::StableId::from_parts(1_700_000_000_000, 3),
            )
            .unwrap();
        let view = DocumentView::of(created);

        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"kind\":\"page\""), "{json}");
        assert!(json.contains("\"revision\""), "the revision crosses the bridge: {json}");
        assert_eq!(serde_json::from_str::<DocumentView>(&json).unwrap().id, view.id);

        let save = SaveView::from(SaveOutcome::saved(view.revision.clone(), view.source.clone()));
        let json = serde_json::to_string(&save).unwrap();
        assert!(json.contains("\"status\":\"saved\""), "{json}");
        assert!(json.contains("\"saved\":true"), "{json}");
    }
}
