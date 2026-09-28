//! The WIP lifecycle.
//!
//! `handoff.md` §7.1 and §7.2 define three states for an article, and the rules that keep a URL
//! stable across them:
//!
//! | `wip` | `wip_publish` | address | in a production build |
//! |---|---|---|---|
//! | `true` | `false` | `/wip/<id>/` | no |
//! | `true` | `true` | `/wip/<id>/` | yes, `noindex`, not in the sitemap |
//! | `false` | — | `/article/<slug>/` | yes |
//!
//! and the transition rule that makes the third state safe:
//!
//! > A WIP URL that has EVER been emitted must keep resolving forever. Once the article is finished
//! > it moves to `/article/<slug>/`, and `/wip/<id>/` becomes a permanent redirect.
//!
//! `wip_ever_published` is what records "has ever been emitted", and it is **never reset**. Clearing
//! it would delete the permanent redirect and break every existing link, which is the one thing
//! §7.2 exists to prevent. So the transitions here are the only place the flag is written, and only
//! one of them ever sets it.

use serde::{Deserialize, Serialize};

use veloct_core::Result;
use veloct_markdown::frontmatter::Frontmatter;
use veloct_markdown::rewrite;

/// A change to an article's WIP state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WipTransition {
    /// Keep it private. Editable and previewable, but never emitted.
    SaveAsDraft,
    /// Publish the WIP at its stable address so the author can share the link.
    PublishWip,
    /// Take a published WIP back off the site.
    ///
    /// `wip_ever_published` survives, so the address keeps resolving.
    UnpublishWip,
    /// Finish it: it becomes an ordinary article at `/article/<slug>/`.
    Finish,
}

impl WipTransition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SaveAsDraft => "save_as_draft",
            Self::PublishWip => "publish_wip",
            Self::UnpublishWip => "unpublish_wip",
            Self::Finish => "finish",
        }
    }

    /// Apply this transition to a frontmatter block, reporting whether anything changed.
    ///
    /// Returns `(changed, outcome)`. Nothing here writes to disk; the caller saves through the
    /// content service so the external-modification check still applies.
    pub fn apply(self, frontmatter: &mut Frontmatter) -> FinishWipOutcome {
        let before = (
            frontmatter.wip,
            frontmatter.wip_publish,
            frontmatter.wip_ever_published,
        );

        match self {
            Self::SaveAsDraft => {
                frontmatter.wip = true;
                frontmatter.wip_publish = false;
            }
            Self::PublishWip => {
                frontmatter.wip = true;
                frontmatter.wip_publish = true;
                // This is the moment the stable address enters a production build, which is what
                // justifies the permanent redirect later.
                frontmatter.wip_ever_published = true;
            }
            Self::UnpublishWip => {
                frontmatter.wip = true;
                frontmatter.wip_publish = false;
                // Deliberately NOT reset: the address was public and must keep resolving.
            }
            Self::Finish => {
                frontmatter.wip = false;
                frontmatter.wip_publish = false;
                // Also not reset, so a finished article that was once a published WIP keeps its
                // redirect.
            }
        }

        let after = (
            frontmatter.wip,
            frontmatter.wip_publish,
            frontmatter.wip_ever_published,
        );
        FinishWipOutcome {
            changed: before != after,
            transition: self,
            needs_redirect: frontmatter.wip_ever_published && !frontmatter.wip,
            published_wip: frontmatter.wip && frontmatter.wip_publish,
            ever_published: frontmatter.wip_ever_published,
        }
    }
}

/// What a transition did, and what the site must do as a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FinishWipOutcome {
    /// Whether the frontmatter actually changed, so an idempotent click does not rewrite the file.
    pub changed: bool,
    pub transition: WipTransition,
    /// Whether `/wip/<id>/` must now permanently redirect to `/article/<slug>/`.
    ///
    /// True exactly when the address was ever public and the article is no longer a WIP. That is the
    /// condition `handoff.md` §7.2 states, and it is computed rather than assumed.
    pub needs_redirect: bool,
    /// Whether the document is currently a published WIP.
    pub published_wip: bool,
    /// The flag as it stands after the transition.
    pub ever_published: bool,
}

impl FinishWipOutcome {
    /// The route the article occupies after this transition.
    pub fn route(&self, id: &str, slug: &str) -> String {
        if self.transition == WipTransition::Finish {
            format!("/article/{slug}/")
        } else {
            format!("/wip/{id}/")
        }
    }

    /// Whether the article is emitted in a production build at all.
    pub fn is_buildable(&self) -> bool {
        match self.transition {
            WipTransition::SaveAsDraft | WipTransition::UnpublishWip => false,
            WipTransition::PublishWip | WipTransition::Finish => true,
        }
    }
}

/// Read the WIP state of a document, for the UI to render the right controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WipState {
    /// An ordinary finished article.
    Finished,
    /// A private draft.
    Draft,
    /// A WIP published at its stable address.
    PublishedWip,
    /// A finished article that was once a published WIP, so its old address redirects.
    FinishedWithHistory,
}

impl WipState {
    pub fn of(frontmatter: &Frontmatter) -> Self {
        match (
            frontmatter.wip,
            frontmatter.wip_publish,
            frontmatter.wip_ever_published,
        ) {
            (true, true, _) => Self::PublishedWip,
            (true, false, _) => Self::Draft,
            (false, _, true) => Self::FinishedWithHistory,
            (false, _, false) => Self::Finished,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Draft => "draft",
            Self::PublishedWip => "published_wip",
            Self::FinishedWithHistory => "finished_with_history",
        }
    }

    /// Whether the article appears in a production build.
    pub fn is_buildable(self) -> bool {
        matches!(self, Self::PublishedWip | Self::Finished | Self::FinishedWithHistory)
    }

    /// Whether `/wip/<id>/` is a permanent redirect.
    pub fn is_redirect(self) -> bool {
        matches!(self, Self::FinishedWithHistory)
    }

    /// The address the article occupies.
    pub fn route(self, id: &str, slug: &str) -> String {
        match self {
            Self::Draft | Self::PublishedWip => format!("/wip/{id}/"),
            Self::Finished | Self::FinishedWithHistory => format!("/article/{slug}/"),
        }
    }
}

/// Rewrite a document's source with a transition applied, without touching disk.
///
/// The source is round-tripped through the frontmatter serialiser rather than patched with a
/// regular expression, so an unknown frontmatter key the user added survives
/// (`handoff.md` §3.2 allows extra fields).
pub fn apply_transition(source: &str, transition: WipTransition) -> Result<(String, FinishWipOutcome)> {
    let document = veloct_markdown::parse::parse(source)?;
    let mut frontmatter = document.frontmatter.clone();
    let outcome = transition.apply(&mut frontmatter);
    let rewritten = rewrite::serialize(&frontmatter, &document.body)?;
    Ok((rewritten, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::ids::StableId;
    use veloct_markdown::frontmatter::ContentType;

    fn frontmatter() -> Frontmatter {
        Frontmatter::new(
            ContentType::Article,
            StableId::from_parts(1_700_000_000_000, 1),
            "my-post",
            "My Post",
            "2026-01-01T00:00:00Z",
        )
    }

    fn source_with(wip: bool, wip_publish: bool, ever: bool) -> String {
        format!(
            "---\nid: 01J8Z5Q0K3W7M9V2B4N6P8R1TY\ntype: article\nslug: my-post\ntitle: My Post\ncreated_at: 2026-01-01T00:00:00Z\nupdated_at: 2026-01-01T00:00:00Z\nwip: {wip}\nwip_publish: {wip_publish}\nwip_ever_published: {ever}\ncustom_field: kept\n---\n\nBody text.\n"
        )
    }

    #[test]
    fn a_new_article_starts_as_a_draft() {
        // Frontmatter::new sets wip for an article and leaves wip_publish false.
        let frontmatter = frontmatter();
        assert_eq!(WipState::of(&frontmatter), WipState::Draft);
        assert!(!WipState::of(&frontmatter).is_buildable());
    }

    #[test]
    fn publishing_a_wip_records_that_it_was_ever_public() {
        // This is the moment the stable address enters a build, which is what justifies the
        // permanent redirect later.
        let mut frontmatter = frontmatter();
        let outcome = WipTransition::PublishWip.apply(&mut frontmatter);

        assert!(outcome.changed);
        assert!(outcome.published_wip);
        assert!(outcome.ever_published, "the flag must be set on first publish");
        assert!(!outcome.needs_redirect, "it is still a WIP");
        assert_eq!(WipState::of(&frontmatter), WipState::PublishedWip);
        assert!(outcome.is_buildable());
        assert_eq!(outcome.route("01ABC", "my-post"), "/wip/01ABC/");
    }

    #[test]
    fn finishing_a_published_wip_requires_a_permanent_redirect() {
        // handoff.md section 7.2: the old address must keep resolving forever.
        let mut frontmatter = frontmatter();
        WipTransition::PublishWip.apply(&mut frontmatter);
        let outcome = WipTransition::Finish.apply(&mut frontmatter);

        assert!(outcome.changed);
        assert!(outcome.needs_redirect, "a once-public address needs a redirect");
        assert!(outcome.ever_published);
        assert_eq!(WipState::of(&frontmatter), WipState::FinishedWithHistory);
        assert!(outcome.is_buildable());
        assert_eq!(outcome.route("01ABC", "my-post"), "/article/my-post/");
        assert!(WipState::of(&frontmatter).is_redirect());
    }

    #[test]
    fn finishing_a_never_published_draft_needs_no_redirect() {
        // Nothing was ever public, so there is no link to preserve.
        let mut frontmatter = frontmatter();
        WipTransition::SaveAsDraft.apply(&mut frontmatter);
        let outcome = WipTransition::Finish.apply(&mut frontmatter);

        assert!(!outcome.needs_redirect);
        assert!(!outcome.ever_published);
        assert_eq!(WipState::of(&frontmatter), WipState::Finished);
        assert!(!WipState::of(&frontmatter).is_redirect());
    }

    #[test]
    fn unpublishing_keeps_the_ever_published_flag() {
        // Clearing it would delete the permanent redirect and break every existing link, which is
        // the one thing section 7.2 exists to prevent.
        let mut frontmatter = frontmatter();
        WipTransition::PublishWip.apply(&mut frontmatter);
        let outcome = WipTransition::UnpublishWip.apply(&mut frontmatter);

        assert!(outcome.ever_published, "the flag survives unpublishing");
        assert!(!outcome.published_wip);
        assert!(!outcome.is_buildable());
        assert_eq!(WipState::of(&frontmatter), WipState::Draft);

        // And a later finish still produces the redirect.
        let finished = WipTransition::Finish.apply(&mut frontmatter);
        assert!(finished.needs_redirect);
    }

    #[test]
    fn republishing_after_unpublishing_still_needs_the_redirect_later() {
        let mut frontmatter = frontmatter();
        WipTransition::PublishWip.apply(&mut frontmatter);
        WipTransition::UnpublishWip.apply(&mut frontmatter);
        WipTransition::PublishWip.apply(&mut frontmatter);
        WipTransition::UnpublishWip.apply(&mut frontmatter);
        let outcome = WipTransition::Finish.apply(&mut frontmatter);
        assert!(outcome.needs_redirect);
    }

    #[test]
    fn an_idempotent_transition_reports_no_change() {
        // So clicking the same control twice does not rewrite the file and bump updated_at.
        let mut frontmatter = frontmatter();
        WipTransition::SaveAsDraft.apply(&mut frontmatter);
        let again = WipTransition::SaveAsDraft.apply(&mut frontmatter);
        assert!(!again.changed, "the second draft save changed nothing");

        WipTransition::PublishWip.apply(&mut frontmatter);
        let again = WipTransition::PublishWip.apply(&mut frontmatter);
        assert!(!again.changed);

        WipTransition::Finish.apply(&mut frontmatter);
        let again = WipTransition::Finish.apply(&mut frontmatter);
        assert!(!again.changed, "finishing an ordinary article changes nothing");
    }

    #[test]
    fn every_state_has_a_route_and_a_build_decision() {
        for state in [
            WipState::Draft,
            WipState::PublishedWip,
            WipState::Finished,
            WipState::FinishedWithHistory,
        ] {
            let route = state.route("01ABC", "my-post");
            assert!(route.starts_with('/'), "{state:?} has no route");
            assert!(!state.as_str().is_empty());
            // The redirect state is exactly the one that was ever public and is no longer a WIP.
            assert_eq!(
                state.is_redirect(),
                state == WipState::FinishedWithHistory
            );
        }
    }

    #[test]
    fn a_draft_address_is_never_the_article_address() {
        // The WIP address is the stable id, so renaming the slug cannot move it.
        assert_eq!(WipState::Draft.route("01ABC", "a-slug"), "/wip/01ABC/");
        assert_eq!(
            WipState::PublishedWip.route("01ABC", "a-slug"),
            "/wip/01ABC/"
        );
        assert_eq!(
            WipState::Finished.route("01ABC", "a-slug"),
            "/article/a-slug/"
        );
    }

    #[test]
    fn applying_a_transition_round_trips_the_source_and_keeps_unknown_fields() {
        // handoff.md section 3.2 allows extra frontmatter fields, so a transition must not drop one.
        let source = source_with(false, false, false);
        let (rewritten, outcome) = apply_transition(&source, WipTransition::PublishWip).unwrap();

        assert!(outcome.changed);
        assert!(rewritten.contains("custom_field: kept"), "{rewritten}");
        assert!(rewritten.contains("wip: true"), "{rewritten}");
        assert!(rewritten.contains("wip_publish: true"), "{rewritten}");
        assert!(rewritten.contains("wip_ever_published: true"), "{rewritten}");
        assert!(rewritten.contains("Body text."), "the body survives");
    }

    #[test]
    fn a_transition_preserves_the_stable_id_and_the_slug() {
        // handoff.md section 3.2: the id is permanent, and a transition is not a rename.
        let source = source_with(true, true, true);
        let (rewritten, _) = apply_transition(&source, WipTransition::Finish).unwrap();
        assert!(rewritten.contains("id: 01J8Z5Q0K3W7M9V2B4N6P8R1TY"));
        assert!(rewritten.contains("slug: my-post"));
        assert!(rewritten.contains("wip: false"));
    }

    #[test]
    fn a_full_lifecycle_ends_with_exactly_one_redirect() {
        // The whole point of the section 7.2 rule, walked end to end.
        let source = source_with(true, false, false);
        let (published, publish_outcome) =
            apply_transition(&source, WipTransition::PublishWip).unwrap();
        assert!(publish_outcome.ever_published);
        assert!(!publish_outcome.needs_redirect);

        let (finished, finish_outcome) =
            apply_transition(&published, WipTransition::Finish).unwrap();
        assert!(finish_outcome.needs_redirect);
        assert!(finished.contains("wip: false"));
        assert!(finished.contains("wip_ever_published: true"));

        // A later edit that touches nothing about WIP does not resurrect or clear the flag.
        let (again, again_outcome) = apply_transition(&finished, WipTransition::Finish).unwrap();
        assert!(!again_outcome.changed);
        assert!(again.contains("wip_ever_published: true"));
    }

    #[test]
    fn states_round_trip_through_json_for_the_frontend() {
        assert_eq!(
            serde_json::to_string(&WipState::PublishedWip).unwrap(),
            "\"published_wip\""
        );
        assert_eq!(
            serde_json::to_string(&WipState::FinishedWithHistory).unwrap(),
            "\"finished_with_history\""
        );
        assert_eq!(
            serde_json::to_string(&WipTransition::Finish).unwrap(),
            "\"finish\""
        );
    }
}
