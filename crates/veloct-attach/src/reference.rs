//! The reference set.
//!
//! `handoff.md` §13.2.3 is the whole reason this module exists as a set rather than a boolean:
//!
//! > An attachment can be referenced by several articles or pages. The underlying layer must
//! > maintain a set / index relationship between attachments and content references.
//! >
//! > Deleting one of those references must not change the status to unreferenced just because that
//! > reference disappeared; only after the LAST valid reference disappears does it enter the
//! > unreferenced state.
//!
//! So the set is the truth and `reference_status` is derived from it. A boolean would make the bug
//! in that sentence not merely possible but natural, because removing a reference and setting the
//! flag to false is the obvious implementation.
//!
//! §13.8 adds a rule this module enforces by having no code for the opposite: removing a reference
//! deletes the Markdown reference and NOTHING remote. There is no `delete_remote` here at all.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::pool::AttachmentId;
use crate::state::ReferenceStatus;

/// One place an attachment is used.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReferenceTarget {
    /// The referencing document's stable id.
    pub content_id: String,
    /// The document's route, for the UI to link to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    /// Which occurrence within that document, so two images in one article are two references.
    ///
    /// Without this, an article using the same photo twice would count as one reference and removing
    /// one of the two would look like removing the only one.
    pub occurrence: usize,
    /// A hash of the Markdown node that referenced it, so a re-parse can tell whether the reference
    /// is still the same one after an edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_hash: Option<String>,
}

impl ReferenceTarget {
    pub fn new(content_id: impl Into<String>, occurrence: usize) -> Self {
        Self {
            content_id: content_id.into(),
            route: None,
            occurrence,
            node_hash: None,
        }
    }

    pub fn with_route(mut self, route: impl Into<String>) -> Self {
        self.route = Some(route.into());
        self
    }

    pub fn with_node_hash(mut self, hash: impl Into<String>) -> Self {
        self.node_hash = Some(hash.into());
        self
    }

    /// Every reference made by one document, for a re-index of that document.
    pub fn key(&self) -> (String, usize) {
        (self.content_id.clone(), self.occurrence)
    }
}

/// The set of references pointing at one attachment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceSet {
    targets: BTreeSet<ReferenceTarget>,
    /// Whether a reference apply is in flight for this attachment.
    #[serde(default)]
    applying: bool,
    /// Whether the most recent apply attempt failed.
    #[serde(default)]
    last_apply_failed: bool,
}

impl ReferenceSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// The references, in a stable order.
    pub fn targets(&self) -> Vec<&ReferenceTarget> {
        self.targets.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// Add one reference. Idempotent, because re-indexing a document must not multiply its
    /// references.
    pub fn insert(&mut self, target: ReferenceTarget) -> bool {
        self.targets.insert(target)
    }

    /// Remove one reference.
    ///
    /// Returns whether the LAST reference went away — the only event that changes the aggregate to
    /// unreferenced. A caller that acted on every removal would be implementing exactly the bug
    /// `handoff.md` §13.2.3 describes.
    ///
    /// This deletes the Markdown reference only. There is deliberately no remote delete here: §13.8
    /// says a remote file is never deleted automatically.
    pub fn remove(&mut self, target: &ReferenceTarget) -> bool {
        self.targets.remove(target);
        self.targets.is_empty()
    }

    /// Remove every reference made by one document.
    ///
    /// Used when a document is deleted or when its references are re-indexed from scratch.
    pub fn remove_document(&mut self, content_id: &str) -> usize {
        let before = self.targets.len();
        self.targets.retain(|target| target.content_id != content_id);
        before - self.targets.len()
    }

    /// Replace every reference made by one document, which is what a re-index does.
    ///
    /// A re-index must not silently drop the references made by OTHER documents, so only this
    /// document's entries are replaced.
    pub fn replace_document(&mut self, content_id: &str, targets: Vec<ReferenceTarget>) -> usize {
        self.remove_document(content_id);
        let mut added = 0;
        for target in targets {
            if self.insert(target) {
                added += 1;
            }
        }
        added
    }

    /// The document ids that reference this attachment, sorted and de-duplicated.
    pub fn content_ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.targets.iter().map(|t| t.content_id.as_str()).collect();
        ids.dedup();
        ids
    }

    /// Whether more than one document references this attachment.
    pub fn is_shared(&self) -> bool {
        self.content_ids().len() > 1
    }

    pub fn mark_applying(&mut self) {
        self.applying = true;
    }

    /// Finish an apply attempt successfully.
    pub fn mark_applied(&mut self) {
        self.applying = false;
        self.last_apply_failed = false;
    }

    /// Finish an apply attempt with a failure.
    ///
    /// The failure is remembered rather than discarded, because it is what turns a reference-less
    /// attachment into `FAILED` — the state `handoff.md` §13.2.2 says must offer "retry applying the
    /// reference" and must NOT re-upload.
    pub fn mark_apply_failed(&mut self) {
        self.applying = false;
        self.last_apply_failed = true;
    }

    /// Queue this attachment for an apply.
    pub fn mark_apply_pending(&mut self) {
        self.last_apply_failed = false;
    }

    pub fn is_applying(&self) -> bool {
        self.applying
    }

    pub fn last_apply_failed(&self) -> bool {
        self.last_apply_failed
    }

    /// The aggregate status, derived rather than stored.
    pub fn status(&self) -> ReferenceStatus {
        ReferenceStatus::aggregate(self.targets.len(), self.applying, self.last_apply_failed)
    }

    /// Whether this attachment may be treated as having no reference at all.
    pub fn is_unreferenced(&self) -> bool {
        self.targets.is_empty() && !self.applying
    }
}

/// Every attachment's reference set, keyed by attachment id.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReferenceIndex {
    sets: std::collections::BTreeMap<AttachmentId, ReferenceSet>,
}

impl ReferenceIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// The set for one attachment, creating an empty one on demand.
    pub fn entry(&mut self, id: &AttachmentId) -> &mut ReferenceSet {
        self.sets.entry(id.clone()).or_default()
    }

    /// The set for one attachment, or `None` when it has never been referenced.
    pub fn get(&self, id: &AttachmentId) -> Option<&ReferenceSet> {
        self.sets.get(id)
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    pub fn ids(&self) -> Vec<&AttachmentId> {
        self.sets.keys().collect()
    }

    /// Attachments with no reference at all, for the cleanup scan.
    ///
    /// This is only the first of the four conditions `handoff.md` §13.2.4 requires; the pool applies
    /// the other three, because they are about tasks and upload status rather than references.
    pub fn unreferenced(&self) -> Vec<&AttachmentId> {
        self.sets
            .iter()
            .filter(|(_, set)| set.is_unreferenced())
            .map(|(id, _)| id)
            .collect()
    }

    /// Record one reference.
    pub fn insert(&mut self, id: &AttachmentId, target: ReferenceTarget) -> bool {
        self.entry(id).insert(target)
    }

    /// Remove one reference, returning whether it was the last one.
    pub fn remove(&mut self, id: &AttachmentId, target: &ReferenceTarget) -> bool {
        self.entry(id).remove(target)
    }

    /// Which attachments a document references, so deleting a document can update the index.
    /// Forget every reference from one document, across every attachment.
    ///
    /// The per-set `ReferenceSet::remove_document` removes one document from ONE attachment, so nothing
    /// could answer "this article is gone, forget it everywhere" — which is what deleting an article needs,
    /// or its citations would survive it and keep attachments from being orphans.
    ///
    /// Sets left empty are removed, so an attachment that loses its last reference genuinely has no entry
    /// rather than an empty one that reads as unreferenced but present.
    pub fn forget_document(&mut self, content_id: &str) -> usize {
        let mut removed = 0usize;
        let mut emptied: Vec<AttachmentId> = Vec::new();
        for (id, set) in self.sets.iter_mut() {
            removed += set.remove_document(content_id);
            if set.is_empty() {
                emptied.push(id.clone());
            }
        }
        for id in emptied {
            self.sets.remove(&id);
        }
        removed
    }

    /// The number of references one attachment has.
    pub fn count_for(&self, id: &AttachmentId) -> usize {
        self.sets.get(id).map(ReferenceSet::len).unwrap_or(0)
    }

    pub fn referenced_by(&self, content_id: &str) -> Vec<&AttachmentId> {
        self.sets
            .iter()
            .filter(|(_, set)| {
                set.targets
                    .iter()
                    .any(|target| target.content_id == content_id)
            })
            .map(|(id, _)| id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(document: &str, occurrence: usize) -> ReferenceTarget {
        ReferenceTarget::new(document, occurrence)
    }

    #[test]
    fn removing_one_of_several_references_leaves_the_attachment_referenced() {
        // handoff.md section 13.2.3, the sentence this module exists for.
        let mut set = ReferenceSet::new();
        set.insert(target("article-a", 0));
        set.insert(target("article-b", 0));
        set.insert(target("article-c", 0));
        assert_eq!(set.status(), ReferenceStatus::Applied);

        // Remove one. It is still referenced by the other two.
        let last = set.remove(&target("article-a", 0));
        assert!(!last, "one removal of three is not the last");
        assert_eq!(set.len(), 2);
        assert_eq!(set.status(), ReferenceStatus::Applied);

        // Remove the second. Still referenced by one.
        let last = set.remove(&target("article-b", 0));
        assert!(!last);
        assert_eq!(set.status(), ReferenceStatus::Applied);

        // Only the last removal makes it unreferenced.
        let last = set.remove(&target("article-c", 0));
        assert!(last, "removing the last reference is the only event that matters");
        assert_eq!(set.status(), ReferenceStatus::NotApplied);
        assert!(set.is_unreferenced());
    }

    #[test]
    fn two_occurrences_in_one_document_are_two_references() {
        // An article using the same photo twice must not count as one reference, or removing one of
        // the two would look like removing the only one.
        let mut set = ReferenceSet::new();
        set.insert(target("article-a", 0));
        set.insert(target("article-a", 1));
        assert_eq!(set.len(), 2);
        assert_eq!(set.content_ids(), vec!["article-a"]);

        let last = set.remove(&target("article-a", 0));
        assert!(!last, "the second occurrence still references it");
        assert_eq!(set.status(), ReferenceStatus::Applied);
    }

    #[test]
    fn inserting_the_same_reference_twice_does_not_multiply_it() {
        // Re-indexing a document must be idempotent.
        let mut set = ReferenceSet::new();
        assert!(set.insert(target("a", 0)));
        assert!(!set.insert(target("a", 0)), "the second insert changed nothing");
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn removing_a_reference_that_is_not_there_reports_the_current_emptiness() {
        let mut set = ReferenceSet::new();
        let last = set.remove(&target("a", 0));
        assert!(last, "the set is empty either way");
    }

    #[test]
    fn removing_a_document_removes_all_of_its_references() {
        let mut set = ReferenceSet::new();
        set.insert(target("a", 0));
        set.insert(target("a", 1));
        set.insert(target("b", 0));

        let removed = set.remove_document("a");
        assert_eq!(removed, 2);
        assert_eq!(set.len(), 1);
        assert_eq!(set.content_ids(), vec!["b"]);
        assert_eq!(set.status(), ReferenceStatus::Applied);
    }

    #[test]
    fn re_indexing_one_document_does_not_drop_another_documents_references() {
        // The bug a naive "rebuild the whole index from this document" would cause.
        let mut set = ReferenceSet::new();
        set.insert(target("other-article", 0));
        set.insert(target("this-article", 0));

        let added = set.replace_document("this-article", vec![target("this-article", 5)]);
        assert_eq!(added, 1);
        assert_eq!(set.len(), 2, "the other document's reference survives");
        assert_eq!(set.content_ids(), vec!["other-article", "this-article"]);
        assert!(set.targets().iter().any(|t| t.content_id == "this-article" && t.occurrence == 5));
    }

    #[test]
    fn an_attachment_shared_by_two_documents_is_reported_as_shared() {
        let mut set = ReferenceSet::new();
        set.insert(target("a", 0));
        assert!(!set.is_shared());
        set.insert(target("b", 0));
        assert!(set.is_shared());
    }

    #[test]
    fn an_in_flight_apply_is_reported_as_applying() {
        let mut set = ReferenceSet::new();
        set.mark_applying();
        assert_eq!(set.status(), ReferenceStatus::Applying);
    }

    #[test]
    fn a_failed_apply_with_no_references_left_is_reported_as_failed() {
        // handoff.md section 13.2.2: the state that must offer "retry apply" and must NOT re-upload.
        let mut set = ReferenceSet::new();
        set.mark_applying();
        set.mark_apply_failed();
        assert_eq!(set.status(), ReferenceStatus::Failed);
        assert!(set.last_apply_failed());
        assert!(!set.is_applying());
    }

    #[test]
    fn a_successful_apply_clears_the_failure() {
        let mut set = ReferenceSet::new();
        set.mark_apply_failed();
        set.insert(target("a", 0));
        assert_eq!(set.status(), ReferenceStatus::Applied);

        // Even with no references, a successful apply clears the failed flag.
        set.mark_applied();
        assert!(!set.last_apply_failed());
    }

    #[test]
    fn a_failed_apply_that_left_a_reference_is_still_applied() {
        // The aggregate is over the set: a valid reference exists, so it is applied.
        let mut set = ReferenceSet::new();
        set.insert(target("a", 0));
        set.mark_apply_failed();
        assert_eq!(set.status(), ReferenceStatus::Applied);
    }

    #[test]
    fn an_attachment_that_is_mid_apply_is_not_treated_as_unreferenced() {
        // Sweeping it would race the apply that is writing the reference right now.
        let mut set = ReferenceSet::new();
        set.mark_applying();
        assert!(!set.is_unreferenced());
    }

    #[test]
    fn the_index_finds_which_attachments_a_document_references() {
        // Used when a document is deleted, so its references can be released.
        let mut index = ReferenceIndex::new();
        let first = AttachmentId::new("att-1");
        let second = AttachmentId::new("att-2");
        let third = AttachmentId::new("att-3");

        index.insert(&first, target("article-a", 0));
        index.insert(&first, target("article-b", 0));
        index.insert(&second, target("article-a", 0));
        index.insert(&third, target("article-c", 0));

        let mut referenced = index.referenced_by("article-a");
        referenced.sort();
        assert_eq!(referenced, vec![&first, &second]);
        assert_eq!(index.referenced_by("article-c"), vec![&third]);
    }

    #[test]
    fn the_index_lists_unreferenced_attachments_for_the_cleanup_scan() {
        let mut index = ReferenceIndex::new();
        let referenced = AttachmentId::new("att-1");
        let orphan = AttachmentId::new("att-2");
        index.insert(&referenced, target("article-a", 0));
        index.entry(&orphan);

        let unreferenced = index.unreferenced();
        assert_eq!(unreferenced.len(), 1);
        assert_eq!(unreferenced[0], &orphan);
    }

    #[test]
    fn the_index_removes_a_reference_and_reports_the_last_one() {
        let mut index = ReferenceIndex::new();
        let id = AttachmentId::new("att-1");
        index.insert(&id, target("a", 0));
        index.insert(&id, target("b", 0));

        assert!(!index.remove(&id, &target("a", 0)));
        assert!(index.remove(&id, &target("b", 0)));
    }

    #[test]
    fn the_index_tolerates_an_unknown_attachment() {
        let index = ReferenceIndex::new();
        assert!(index.get(&AttachmentId::new("nope")).is_none());
        assert!(index.is_empty());
        assert!(index.unreferenced().is_empty());
    }

    #[test]
    fn a_reference_target_carries_a_route_and_a_node_hash_for_the_ui() {
        let with_details = target("article-a", 0)
            .with_route("/article/hello/")
            .with_node_hash("abc123");
        assert_eq!(with_details.route.as_deref(), Some("/article/hello/"));
        assert_eq!(with_details.node_hash.as_deref(), Some("abc123"));
        assert_eq!(with_details.key(), ("article-a".to_string(), 0));
    }

    #[test]
    fn a_reference_set_round_trips_through_json() {
        let mut set = ReferenceSet::new();
        set.insert(target("a", 0).with_route("/a/"));
        set.insert(target("b", 1));
        set.mark_applying();

        let json = serde_json::to_string(&set).unwrap();
        let parsed: ReferenceSet = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, set);
        assert_eq!(parsed.status(), ReferenceStatus::Applying);
    }

    #[test]
    fn removing_a_reference_has_no_remote_effect_because_there_is_no_such_function() {
        // handoff.md section 13.8: removing a reference deletes the Markdown reference only, and the
        // remote file is never deleted automatically. This test documents that the API has no way to
        // do otherwise: `ReferenceSet` holds ids and text, never a storage adapter.
        let mut set = ReferenceSet::new();
        set.insert(target("a", 0));
        let last = set.remove(&target("a", 0));
        assert!(last);
        // The only effect is on the set itself.
        assert!(set.is_empty());
    }
}
