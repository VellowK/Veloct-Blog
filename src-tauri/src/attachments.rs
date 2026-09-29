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
