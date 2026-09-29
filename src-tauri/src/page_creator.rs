//! The AI commands, and the page creator's generate/confirm flow.
//!
//! `handoff.md` §11 asks for per-feature model mapping and a single global fallback, and §8.1 for the page
//! creator. The two meet here.
//!
//! The shape of this module follows one rule: **the flow is testable without a network.** Generation takes
//! a `&dyn Fn(&str) -> Result<String>` rather than reaching for HTTP itself, exactly as `AiService` takes
//! an injected client. So the tests drive the whole path — settings, prompt, answer, draft, validation,
//! confirmation — with a stub, and the production path differs only in the closure it passes. A flow that
//! could only be exercised against a live provider would be a flow nobody exercises.

use std::sync::Arc;

use veloct_ai::page::{self, ContentKindChoice, PageDraft, PageRequest};
use veloct_ai::service::AiOutcomeStatus;
use veloct_ai::{AiCall, AiService, ContextBundle, FallbackConfig, ProviderRegistry};
use veloct_core::ids::StableId;
use veloct_core::{Clock, Result, VeloctError};
use veloct_storage::{AiModelRow, AiSettings};

/// The feature id the page creator uses (§11.1's feature list).
pub const PAGE_CREATE_FEATURE: &str = "page_create";

/// What the caller is told when nothing is configured.
///
/// Actionable, because this is the first thing a new project hits and "the AI call failed" would send the
/// user looking in the wrong place.
fn not_configured(reason: &str) -> VeloctError {
    VeloctError::Invalid(format!(
        "{reason}. Configure a provider and choose a model for 页面创建 in AI settings."
    ))
}

/// Build the service the page creator uses, from the stored settings.
///
/// The credential for each provider is fetched from the OS credential store by its HANDLE (§12.2); a
/// provider whose handle has no stored credential is registered without one, so the call fails with the
/// provider's own authentication message rather than with a Veloct invention.
pub fn build_service(
    settings: &AiSettings,
    credential: &dyn Fn(&str) -> Option<String>,
) -> Result<AiService> {
    let mut registry = ProviderRegistry::new();

    for row in &settings.providers {
        if !row.is_usable() {
            continue;
        }
        let Some(protocol) = veloct_ai::ProviderProtocol::parse(&row.protocol) else {
            // An unknown protocol is skipped rather than guessed at: guessing which API to speak would
            // produce a confusing failure instead of an obviously missing provider.
            continue;
        };
        let mut config = veloct_ai::ProviderConfig::new(&row.id, &row.name, protocol);
        // The stored base URL replaces the protocol's default, then validation reports a bad one rather
        // than the call failing somewhere less obvious.
        config.base_url = row.base_url.clone();
        config.credential_ref = row.credential_ref.clone();
        config
            .validate()
            .map_err(|error| VeloctError::Invalid(format!("provider {} is misconfigured: {error}", row.id)))?;
        let secret = row
            .credential_ref
            .as_deref()
            .and_then(credential);
        registry.insert(veloct_ai::Provider::new(config, secret));
    }

    if registry.is_empty() {
        return Err(not_configured("no enabled AI provider is configured"));
    }

    let fallback = if settings.fallback.is_configured() {
        // §11.4: ONE global fallback, used when the feature's own model fails.
        let provider_id = settings.fallback.provider_id.clone().unwrap_or_default();
        let model_id = settings.fallback.model_id.clone().unwrap_or_default();
        FallbackConfig::some(veloct_ai::ModelRef::new(provider_id, model_id))
    } else {
        FallbackConfig::none()
    };

    Ok(AiService::new(registry, fallback))
}

/// The model the page creator should use, from the feature mapping (§11.3).
pub fn model_for_page_create(settings: &AiSettings) -> Result<veloct_ai::ModelRef> {
    let feature = settings
        .feature(PAGE_CREATE_FEATURE)
        .ok_or_else(|| not_configured("the 页面创建 feature has no model mapped"))?;
    if !feature.is_configured() {
        return Err(not_configured(
            "the 页面创建 feature is mapped to no model",
        ));
    }
    Ok(veloct_ai::ModelRef::new(
        feature.provider_id.clone().unwrap_or_default(),
        feature.model_id.clone().unwrap_or_default(),
    ))
}

/// What an injected generator returns: the answer, which provider and model served it, and whether the
/// global fallback was used (§11.4).
///
/// A named type rather than a four-element tuple, because the two `Option<String>`s are the provider and
/// the model: a caller could transpose them and still compile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedAnswer {
    pub text: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub used_fallback: bool,
}

/// A generator the page creator drives. Production passes one that calls [`AiService`]; the tests pass a
/// stub, which is what makes the whole flow runnable without a network.
pub type Generator<'a> = &'a dyn Fn(&str) -> Result<GeneratedAnswer>;

/// Generate a draft through an injected generator.
///
/// The generator takes the prompt and returns the model's answer. Production passes a closure that calls
/// [`AiService`]; the tests pass one that returns a fixed document. That is what makes the whole flow
/// runnable in a test.
pub fn generate_page_with(
    generate: Generator<'_>,
    request: &PageRequest,
    now: &str,
    id: StableId,
) -> Result<PageDraft> {
    let prompt = request.prompt();
    let generated = generate(&prompt)?;
    let draft = page::build_draft(&generated.text, request, now, id)?;
    Ok(match (generated.provider_id, generated.model_id) {
        (Some(provider), Some(model)) => {
            draft.attributed_to(&provider, &model, generated.used_fallback)
        }
        _ => draft,
    })
}

/// Generate a draft through a real [`AiService`].
pub fn generate_page(
    service: &AiService,
    model: veloct_ai::ModelRef,
    request: &PageRequest,
    now: &str,
    id: StableId,
) -> Result<PageDraft> {
    let generate = |prompt: &str| -> Result<GeneratedAnswer> {
        let call = AiCall::new(
            veloct_ai::FeatureId::PageCreate,
            prompt,
            // A context bundle with no sources: the page is written from the brief alone, and emptying the
            // user's project into the prompt would be both surprising and expensive.
            // No sources: the page is written from the brief alone, and emptying the user's
            // project into the prompt would be both surprising and expensive.
            ContextBundle {
                items: Vec::new(),
                dropped: Vec::new(),
                primary_truncated: false,
                extra_truncated: false,
            },
        )
        .with_model(model.clone());

        let cancel = veloct_ai::CancelToken::new();
        let outcome = service.call(&call, &cancel);

        if outcome.status == AiOutcomeStatus::Cancelled {
            // §11.5: a cancellation is not a failure and must not fall back, so it is reported as its own
            // thing rather than as an error the user should retry.
            return Err(VeloctError::Cancelled);
        }

        let text = outcome.text.clone().ok_or_else(|| match &outcome.error {
            Some(error) => VeloctError::AiProvider {
                provider: outcome.provider_id.clone().unwrap_or_else(|| "unknown".to_string()),
                // The kind IS the code, and whether it triggers the fallback is whether it is worth
                // retrying: handoff.md section 11.5 makes a cancellation the only kind that does not fall
                // back, so the two questions have the same answer.
                code: error.kind.as_str().to_string(),
                message: format!(
                    "the model returned nothing: {error} ({} attempt(s))",
                    outcome.attempts.len()
                ),
                retryable: error.kind.triggers_fallback(),
                status: error.status,
            },
            None => VeloctError::AiProvider {
                provider: outcome.provider_id.clone().unwrap_or_else(|| "unknown".to_string()),
                code: "EMPTY_RESPONSE".to_string(),
                message: "the model returned nothing".to_string(),
                retryable: true,
                status: None,
            },
        })?;

        Ok(GeneratedAnswer {
            text,
            provider_id: outcome.provider_id.clone(),
            model_id: outcome.model_id.clone(),
            used_fallback: outcome.used_fallback,
        })
    };

    generate_page_with(&generate, request, now, id)
}

/// A provider as the settings page shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    /// The HANDLE, never a credential (§12.2).
    pub credential_ref: Option<String>,
    /// Whether a credential is actually stored under that handle.
    pub has_credential: bool,
    pub enabled: bool,
    pub usable: bool,
}

impl ProviderView {
    pub fn of(row: &veloct_storage::AiProviderRow, has_credential: bool) -> Self {
        Self {
            id: row.id.clone(),
            name: row.name.clone(),
            protocol: row.protocol.clone(),
            base_url: row.base_url.clone(),
            credential_ref: row.credential_ref.clone(),
            has_credential,
            enabled: row.enabled,
            usable: row.is_usable(),
        }
    }
}

/// A model as the settings page shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelView {
    pub provider_id: String,
    pub model_id: String,
    pub label: String,
    pub context_window: Option<i64>,
}

impl From<&AiModelRow> for ModelView {
    fn from(row: &AiModelRow) -> Self {
        Self {
            provider_id: row.provider_id.clone(),
            model_id: row.model_id.clone(),
            label: row.label().to_string(),
            context_window: row.context_window,
        }
    }
}

/// A feature mapping as the settings page shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FeatureView {
    pub id: String,
    pub label: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub configured: bool,
}

impl From<&veloct_storage::AiFeatureRow> for FeatureView {
    fn from(row: &veloct_storage::AiFeatureRow) -> Self {
        Self {
            id: row.id.clone(),
            label: row.label.clone(),
            provider_id: row.provider_id.clone(),
            model_id: row.model_id.clone(),
            configured: row.is_configured(),
        }
    }
}

/// The whole AI configuration, for the settings page.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AiSettingsView {
    pub providers: Vec<ProviderView>,
    pub models: Vec<ModelView>,
    pub features: Vec<FeatureView>,
    pub fallback_provider_id: Option<String>,
    pub fallback_model_id: Option<String>,
    /// Whether the page creator has a usable provider AND a mapped model.
    pub page_create_ready: bool,
    /// Why it is not ready, when it is not, so the page can say what to do.
    pub blocked_reason: Option<String>,
}

impl AiSettingsView {
    pub fn build(settings: &AiSettings, credential: &dyn Fn(&str) -> Option<String>) -> Self {
        let providers: Vec<ProviderView> = settings
            .providers
            .iter()
            .map(|row| {
                let has = row
                    .credential_ref
                    .as_deref()
                    .map(|handle| credential(handle).is_some())
                    .unwrap_or(false);
                ProviderView::of(row, has)
            })
            .collect();

        let blocked = if !settings.has_any_provider() {
            Some("还没有启用任何 AI 提供商。".to_string())
        } else {
            match model_for_page_create(settings) {
                Ok(_) => None,
                Err(error) => Some(error.to_string()),
            }
        };

        Self {
            providers,
            models: settings.models.iter().map(ModelView::from).collect(),
            features: settings.features.iter().map(FeatureView::from).collect(),
            fallback_provider_id: settings.fallback.provider_id.clone(),
            fallback_model_id: settings.fallback.model_id.clone(),
            page_create_ready: blocked.is_none(),
            blocked_reason: blocked,
        }
    }
}

/// A draft waiting for confirmation, with the request that produced it.
///
/// Held in memory rather than written anywhere, because §8.1 says a page is not created before the user
/// confirms. There is no file to forget to delete.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PageDraftView {
    pub draft: PageDraft,
    pub prompt_feature: String,
}

impl PageDraftView {
    pub fn new(draft: PageDraft) -> Self {
        Self {
            draft,
            prompt_feature: PAGE_CREATE_FEATURE.to_string(),
        }
    }
}

/// The content kind a draft was created for, so the confirm step knows the collection.
pub fn kind_of(draft: &PageDraft) -> ContentKindChoice {
    draft.kind
}

/// Parse a request from the command layer.
pub fn parse_request(
    title: &str,
    brief: &str,
    kind: Option<&str>,
    tags: &[String],
    category: Option<&str>,
    with_image_placeholders: bool,
) -> Result<PageRequest> {
    if title.trim().is_empty() {
        return Err(VeloctError::Invalid("a page needs a title".into()));
    }
    if brief.trim().is_empty() {
        return Err(VeloctError::Invalid(
            "a page needs a brief describing what it is for".into(),
        ));
    }

    let mut request = PageRequest::new(title.trim(), brief.trim());
    if let Some(kind) = kind {
        request.kind = ContentKindChoice::parse(kind).ok_or_else(|| {
            VeloctError::Invalid(format!(
                "unknown page kind {kind:?}; expected one of page, article, knowledge"
            ))
        })?;
    }
    request.tags = tags.to_vec();
    request.category = category
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    request.with_image_placeholders = with_image_placeholders;
    Ok(request)
}

/// The clock, as the commands use it.
pub fn now(clock: &dyn Clock) -> String {
    veloct_core::clock::format_rfc3339(clock.now())
}

/// A brand new id for a draft, from the monotonic source the rest of the project uses.
pub fn new_id() -> StableId {
    StableId::new()
}

/// Shared so the command layer does not have to name the trait object's lifetime.
pub type CredentialLookup = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The error code a caller should show for a generation failure.
///
/// A cancellation is reported as `CANCELLED` rather than as the provider's code, because §11.5 makes a stop
/// its own outcome and a UI that showed it as a failure would invite a pointless retry.
pub fn generation_error_code(error: &VeloctError) -> &'static str {
    match error {
        VeloctError::Cancelled => "CANCELLED",
        other => other.code(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_storage::{
        AiFallbackRow, AiFeatureRow, AiProviderRow, AiSettings,
    };

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn id() -> StableId {
        StableId::from_parts(1_700_000_000_000, 9)
    }

    fn provider(id: &str, enabled: bool) -> AiProviderRow {
        AiProviderRow {
            id: id.to_string(),
            name: format!("Provider {id}"),
            protocol: "openai".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            credential_ref: Some(format!("veloct:{id}:key")),
            enabled,
            created_at: NOW.to_string(),
        }
    }

    /// Settings with one provider and the page creator mapped to a model.
    fn configured() -> AiSettings {
        AiSettings {
            providers: vec![provider("p1", true)],
            models: vec![AiModelRow {
                id: "m1".to_string(),
                provider_id: "p1".to_string(),
                model_id: "gpt-4o".to_string(),
                display_name: Some("GPT-4o".to_string()),
                source: "discovered".to_string(),
                context_window: Some(128_000),
                last_seen_at: Some(NOW.to_string()),
            }],
            features: vec![AiFeatureRow {
                id: PAGE_CREATE_FEATURE.to_string(),
                label: "页面创建".to_string(),
                provider_id: Some("p1".to_string()),
                model_id: Some("gpt-4o".to_string()),
                enabled: true,
            }],
            fallback: AiFallbackRow {
                provider_id: Some("p1".to_string()),
                model_id: Some("gpt-4o-mini".to_string()),
            },
        }
    }

    const ANSWER: &str = "---\ntitle: About Us\nslug: about-us\nsummary: Who we are.\n---\n\n# About Us\n\nWe are a team.\n";

    #[test]
    fn a_configured_feature_yields_the_model_to_use() {
        // handoff.md section 11.3: the model comes from the per-feature mapping, not from a global default.
        let model = model_for_page_create(&configured()).unwrap();
        assert_eq!(model.provider_id, "p1");
        assert_eq!(model.model_id, "gpt-4o");
    }

    #[test]
    fn an_unmapped_feature_is_refused_with_an_actionable_message() {
        // "The AI call failed" would send the user looking in the wrong place.
        let mut settings = configured();
        settings.features.clear();
        let error = model_for_page_create(&settings).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("页面创建"), "{text}");
        assert!(text.contains("AI settings"), "{text}");
    }

    #[test]
    fn a_feature_mapped_to_a_model_but_disabled_is_refused() {
        let mut settings = configured();
        settings.features[0].enabled = false;
        assert!(model_for_page_create(&settings).is_err());
    }

    #[test]
    fn no_enabled_provider_is_refused_before_any_request_is_attempted() {
        let mut settings = configured();
        settings.providers[0].enabled = false;
        let error = build_service(&settings, &|_| Some("k".to_string()))
            .err()
            .expect("an unknown protocol leaves no usable provider");
        assert!(error.to_string().contains("no enabled AI provider"), "{error}");

        let empty = AiSettings::default();
        assert!(build_service(&empty, &|_| None).is_err());
    }

    #[test]
    fn the_service_is_built_from_the_settings_and_fetches_the_credential_by_handle() {
        // handoff.md section 12.2: the row names the credential; the value comes from the store.
        let settings = configured();
        // A `Fn` closure cannot mutate a capture, and widening the signature to `FnMut` would complicate
        // every caller for a test's benefit, so the record uses interior mutability.
        let asked: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
        let service = build_service(&settings, &|handle: &str| {
            asked.borrow_mut().push(handle.to_string());
            Some("secret-from-the-store".to_string())
        })
        .unwrap();

        assert_eq!(*asked.borrow(), vec!["veloct:p1:key".to_string()]);
        assert!(service.registry().contains("p1"));
        assert!(service.fallback().is_configured());
    }

    #[test]
    fn a_provider_whose_credential_is_missing_is_still_registered_so_the_error_is_the_providers_own() {
        // Reporting "no credential configured" instead of the provider's authentication message would hide
        // which provider rejected the request.
        let settings = configured();
        let service = build_service(&settings, &|_| None).unwrap();
        assert!(service.registry().contains("p1"));
    }

    #[test]
    fn an_unknown_protocol_is_skipped_rather_than_guessed_at() {
        // Guessing which API to speak would produce a confusing failure instead of an obviously missing
        // provider.
        let mut settings = configured();
        settings.providers[0].protocol = "telepathy".to_string();
        assert!(build_service(&settings, &|_| Some("k".to_string())).is_err());
    }

    #[test]
    fn a_configured_provider_with_no_fallback_builds_a_service_without_one() {
        let mut settings = configured();
        settings.fallback = AiFallbackRow::default();
        let service = build_service(&settings, &|_| Some("k".to_string())).unwrap();
        assert!(!service.fallback().is_configured());
    }

    #[test]
    fn the_whole_flow_runs_with_an_injected_generator_and_writes_nothing() {
        // This is the point of the injection: sections 8.1's flow is exercised end to end with no network.
        let request = PageRequest::new("About Us", "describe the team");
        let draft = generate_page_with(
            &|prompt: &str| {
                assert!(prompt.contains("describe the team"), "the brief reaches the model: {prompt}");
                Ok(GeneratedAnswer {
                    text: ANSWER.to_string(),
                    provider_id: Some("p1".to_string()),
                    model_id: Some("gpt-4o".to_string()),
                    used_fallback: false,
                })
            },
            &request,
            NOW,
            id(),
        )
        .unwrap();

        assert_eq!(draft.title, "About Us");
        assert_eq!(draft.slug, "about-us");
        assert_eq!(draft.provider_id.as_deref(), Some("p1"));
        assert_eq!(draft.model_id.as_deref(), Some("gpt-4o"));
        assert!(!draft.used_fallback);
        // A draft is a VALUE. Nothing has been written anywhere, which is what makes section 8.1's
        // "without confirmation, a page must not be created" a property of the design.
        assert!(draft.is_creatable());
    }

    #[test]
    fn a_generation_that_used_the_fallback_says_so() {
        let request = PageRequest::new("T", "b");
        let draft = generate_page_with(
            &|_| {
                Ok(GeneratedAnswer {
                    text: ANSWER.to_string(),
                    provider_id: Some("p2".to_string()),
                    model_id: Some("m2".to_string()),
                    used_fallback: true,
                })
            },
            &request,
            NOW,
            id(),
        )
        .unwrap();
        assert!(draft.used_fallback);
        assert_eq!(draft.provider_id.as_deref(), Some("p2"));
    }

    #[test]
    fn a_generation_with_no_attribution_still_produces_a_draft() {
        let request = PageRequest::new("T", "b");
        let draft = generate_page_with(&|_| {
                Ok(GeneratedAnswer {
                    text: ANSWER.to_string(),
                    provider_id: None,
                    model_id: None,
                    used_fallback: false,
                })
            }, &request, NOW, id())
            .unwrap();
        assert!(draft.provider_id.is_none());
    }

    #[test]
    fn a_generator_failure_reaches_the_caller_unchanged() {
        let request = PageRequest::new("T", "b");
        let error = generate_page_with(
            &|_| {
                Err(VeloctError::AiProvider {
                    provider: "p1".to_string(),
                    code: "AUTH".to_string(),
                    message: "the provider refused".to_string(),
                    retryable: false,
                    status: Some(401),
                })
            },
            &request,
            NOW,
            id(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("the provider refused"), "{error}");
    }

    #[test]
    fn a_cancellation_is_reported_as_a_cancellation_and_not_as_a_failure() {
        // handoff.md section 11.5: a stop is not an error, and the two must not be conflated or a cancelled
        // request would trigger the fallback.
        let request = PageRequest::new("T", "b");
        let error = generate_page_with(&|_| Err(VeloctError::Cancelled), &request, NOW, id()).unwrap_err();
        assert!(matches!(error, VeloctError::Cancelled));
    }

    #[test]
    fn an_answer_with_no_body_is_refused_rather_than_producing_an_empty_page() {
        let request = PageRequest::new("T", "b");
        let error = generate_page_with(
            &|_| {
                Ok(GeneratedAnswer {
                    text: String::new(),
                    provider_id: None,
                    model_id: None,
                    used_fallback: false,
                })
            },
            &request,
            NOW,
            id(),
        )
            .unwrap_err();
        assert!(error.to_string().contains("no document body"), "{error}");
    }

    #[test]
    fn the_request_parser_insists_on_a_title_and_a_brief() {
        // Without a brief the model has nothing to write from, and a page with no title cannot be
        // addressed.
        assert!(parse_request("", "b", None, &[], None, true).is_err());
        assert!(parse_request("T", "   ", None, &[], None, true).is_err());
        assert!(parse_request("  T  ", "  b  ", None, &[], None, true).is_ok());
    }

    #[test]
    fn the_request_parser_reads_the_kind_and_the_extras() {
        let request = parse_request(
            "T",
            "b",
            Some("article"),
            &["a".to_string()],
            Some("news"),
            false,
        )
        .unwrap();
        assert_eq!(request.kind, ContentKindChoice::Article);
        assert_eq!(request.tags, vec!["a"]);
        assert_eq!(request.category.as_deref(), Some("news"));
        assert!(!request.with_image_placeholders);

        // An empty category is absent rather than an empty string.
        let blank = parse_request("T", "b", None, &[], Some("  "), true).unwrap();
        assert!(blank.category.is_none());
    }

    #[test]
    fn the_request_parser_refuses_an_unknown_kind_rather_than_defaulting() {
        let error = parse_request("T", "b", Some("wip"), &[], None, true).unwrap_err();
        assert!(error.to_string().contains("unknown page kind"), "{error}");
    }

    #[test]
    fn the_kind_travels_with_the_draft_so_the_confirm_step_knows_the_collection() {
        let request = parse_request("T", "b", Some("knowledge"), &[], None, true).unwrap();
        let draft = generate_page_with(&|_| {
                Ok(GeneratedAnswer {
                    text: ANSWER.to_string(),
                    provider_id: None,
                    model_id: None,
                    used_fallback: false,
                })
            }, &request, NOW, id())
            .unwrap();
        assert_eq!(kind_of(&draft), ContentKindChoice::Knowledge);
        assert_eq!(draft.kind, ContentKindChoice::Knowledge);
    }

    #[test]
    fn the_settings_view_reports_what_is_missing_rather_than_only_that_something_is() {
        let empty = AiSettings::default();
        let view = AiSettingsView::build(&empty, &|_| None);
        assert!(!view.page_create_ready);
        assert!(view.blocked_reason.as_deref().unwrap().contains("提供商"));

        let ready = AiSettingsView::build(&configured(), &|_| Some("k".to_string()));
        assert!(ready.page_create_ready);
        assert!(ready.blocked_reason.is_none());
        assert_eq!(ready.providers.len(), 1);
        assert!(ready.providers[0].has_credential);
        assert_eq!(ready.providers[0].credential_ref.as_deref(), Some("veloct:p1:key"));
        assert_eq!(ready.models[0].label, "GPT-4o");
        assert!(ready.features[0].configured);
        assert_eq!(ready.fallback_model_id.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn the_settings_view_never_carries_a_credential_value() {
        // The view is serialised to the frontend, so a secret in it would be a secret in the UI.
        let view = AiSettingsView::build(&configured(), &|_| Some("SUPER-SECRET-VALUE".to_string()));
        let json = serde_json::to_string(&view).unwrap();
        assert!(!json.contains("SUPER-SECRET-VALUE"), "{json}");
        assert!(json.contains("has_credential")); // and it says one exists
    }

    #[test]
    fn the_settings_view_reports_a_provider_whose_credential_is_missing() {
        let view = AiSettingsView::build(&configured(), &|_| None);
        assert!(!view.providers[0].has_credential);
        // But the provider is still usable, and the page creator still ready: the credential is checked by
        // the provider when the call is made.
        assert!(view.providers[0].usable);
    }

    #[test]
    fn the_page_create_readiness_needs_both_a_provider_and_a_mapped_model() {
        let mut no_provider = configured();
        no_provider.providers.clear();
        assert!(!AiSettingsView::build(&no_provider, &|_| None).page_create_ready);

        let mut no_model = configured();
        no_model.features.clear();
        let view = AiSettingsView::build(&no_model, &|_| None);
        assert!(!view.page_create_ready);
        assert!(view.blocked_reason.as_deref().unwrap().contains("页面创建"));
    }

    #[test]
    fn a_draft_view_round_trips_for_the_preview_dialog() {
        let request = PageRequest::new("T", "b");
        let draft = generate_page_with(&|_| {
                Ok(GeneratedAnswer {
                    text: ANSWER.to_string(),
                    provider_id: None,
                    model_id: None,
                    used_fallback: false,
                })
            }, &request, NOW, id())
            .unwrap();
        let view = PageDraftView::new(draft.clone());
        let json = serde_json::to_string(&view).unwrap();
        assert!(json.contains("\"prompt_feature\":\"page_create\""), "{json}");
        assert_eq!(serde_json::from_str::<PageDraftView>(&json).unwrap().draft, draft);
    }

    #[test]
    fn the_generation_error_code_distinguishes_a_cancellation_from_a_failure() {
        assert_eq!(generation_error_code(&VeloctError::Cancelled), "CANCELLED");
        let failure = VeloctError::AiProvider {
            provider: "p1".to_string(),
            code: "SERVER_ERROR".to_string(),
            message: "boom".to_string(),
            retryable: true,
            status: Some(500),
        };
        assert_eq!(generation_error_code(&failure), failure.code());
    }
}
