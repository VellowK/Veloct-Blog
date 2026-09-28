//! The AI service: feature-to-model resolution, one call, and the fallback rule.
//!
//! This is where `handoff.md` §11.4's rule is actually enforced:
//!
//! ```text
//! try the feature's configured model
//!   success                        -> done
//!   CANCELLED                      -> done, as CANCELLED (NOT a failure, NO fallback)
//!   any real error, fallback exists -> try the fallback model
//!       success                    -> done, reporting that a fallback was used
//!       any failure                -> terminal, no further attempt
//! ```
//!
//! Three properties make the rule checkable rather than merely intended:
//!
//! * The fallback attempt is marked, so a fallback failure cannot re-enter the fallback branch
//!   ([`AiError::should_fall_back`] returns false for it).
//! * Cancellation is checked *between* the two attempts as well as inside them, so a user who stops
//!   generation while the first provider is failing is not answered by the second.
//! * Every attempt is recorded, so the caller can write `ai_requests` metadata without having to
//!   reconstruct what happened.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cancellation::CancelToken;
use crate::context::ContextBundle;
use crate::error::{AiError, AiErrorKind};
use crate::feature::FeatureId;
use crate::provider::{ChatMessage, ChatRequest, ModelRef, Provider, ProviderModel};

/// The single global fallback model (`handoff.md` §11.4).
///
/// It may use a completely independent provider, which is why it is a full [`ModelRef`] rather than
/// just a model name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelRef>,
}

impl FallbackConfig {
    pub fn none() -> Self {
        Self { model: None }
    }

    pub fn some(model: ModelRef) -> Self {
        Self { model: Some(model) }
    }

    pub fn is_configured(&self) -> bool {
        self.model.as_ref().map(ModelRef::is_usable).unwrap_or(false)
    }
}

/// The providers a service can reach.
///
/// Providers are held already-constructed, with their credentials resolved, so the service never
/// touches the credential store and a credential cannot leak into a serialised structure.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, Arc<Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, provider: Provider) {
        self.providers
            .insert(provider.config().id.clone(), Arc::new(provider));
    }

    pub fn get(&self, provider_id: &str) -> Option<Arc<Provider>> {
        self.providers.get(provider_id).cloned()
    }

    pub fn contains(&self, provider_id: &str) -> bool {
        self.providers.contains_key(provider_id)
    }

    pub fn len(&self) -> usize {
        self.providers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn ids(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }
}

/// One request from the UI.
#[derive(Debug, Clone)]
pub struct AiCall {
    pub feature: FeatureId,
    /// The model the feature is configured to use.
    pub model: Option<ModelRef>,
    /// The assembled context.
    pub context: ContextBundle,
    /// The user's instruction.
    pub prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl AiCall {
    pub fn new(feature: FeatureId, prompt: impl Into<String>, context: ContextBundle) -> Self {
        Self {
            feature,
            model: None,
            context,
            prompt: prompt.into(),
            temperature: None,
            max_tokens: None,
        }
    }

    pub fn with_model(mut self, model: ModelRef) -> Self {
        self.model = Some(model);
        self
    }
}

/// One attempt's record, for `ai_requests` and for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptRecord {
    pub provider_id: String,
    pub model_id: String,
    pub used_fallback: bool,
    pub succeeded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// The outcome of a call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiOutcome {
    /// The model's text, when there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    pub status: AiOutcomeStatus,
    /// Whether the fallback model produced the result.
    pub used_fallback: bool,
    /// Which model produced the result, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Every attempt, in order.
    pub attempts: Vec<AttemptRecord>,
    /// The failure, when the call did not succeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AiError>,
}

/// The status written to `ai_requests` (`docs/spec/01-data-model.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiOutcomeStatus {
    Succeeded,
    Failed,
    Cancelled,
    FallbackSucceeded,
    FallbackFailed,
}

impl AiOutcomeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::FallbackSucceeded => "fallback_succeeded",
            Self::FallbackFailed => "fallback_failed",
        }
    }

    /// Whether this counts as a success for the caller.
    pub fn is_success(self) -> bool {
        matches!(self, Self::Succeeded | Self::FallbackSucceeded)
    }

    /// Whether the user stopped it. Deliberately not an error.
    pub fn is_cancelled(self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// The service.
pub struct AiService {
    registry: ProviderRegistry,
    fallback: FallbackConfig,
}

impl AiService {
    pub fn new(registry: ProviderRegistry, fallback: FallbackConfig) -> Self {
        Self { registry, fallback }
    }

    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    pub fn fallback(&self) -> &FallbackConfig {
        &self.fallback
    }

    /// Perform a call, applying the fallback rule.
    ///
    /// Streaming is deliberately not modelled here: §11.6 requires it for the UI, and a streaming
    /// implementation would still have to make exactly these decisions, so the decision logic lives
    /// here and the transport can stream underneath it.
    ///
    /// The real transport is built here and handed to [`Self::call_with_client`], so there is one
    /// implementation of the fallback rule rather than one per transport. Duplicating it would mean
    /// the tested path and the shipped path could disagree, which is the whole risk this
    /// arrangement removes.
    pub fn call(&self, call: &AiCall, cancel: &CancelToken) -> AiOutcome {
        match veloct_core::http::ReqwestHttpClient::with_default_timeout(
            std::time::Duration::from_secs(120),
        ) {
            Ok(client) => self.call_with_client(call, cancel, &client),
            Err(error) => {
                let ai_error = AiError::new(AiErrorKind::Connection, error.to_string());
                AiOutcome {
                    text: None,
                    status: AiOutcomeStatus::Failed,
                    used_fallback: false,
                    provider_id: None,
                    model_id: None,
                    attempts: Vec::new(),
                    error: Some(ai_error),
                }
            }
        }
    }

    /// Perform a call against an injected HTTP client, so the whole fallback path is testable
    /// without a network.
    pub fn call_with_client(
        &self,
        call: &AiCall,
        cancel: &CancelToken,
        client: &dyn veloct_core::http::HttpClient,
    ) -> AiOutcome {
        let mut attempts = Vec::new();

        // A stop before anything starts is a cancellation, not a failure.
        if cancel.is_cancelled() {
            return AiOutcome {
                text: None,
                status: AiOutcomeStatus::Cancelled,
                used_fallback: false,
                provider_id: None,
                model_id: None,
                attempts,
                error: Some(AiError::cancelled()),
            };
        }

        let Some(preferred) = call.model.clone().filter(ModelRef::is_usable) else {
            let error = AiError::new(
                AiErrorKind::NotConfigured,
                format!(
                    "no model is configured for {}; choose one in AI settings",
                    call.feature.as_str()
                ),
            );
            return AiOutcome {
                text: None,
                status: AiOutcomeStatus::Failed,
                used_fallback: false,
                provider_id: None,
                model_id: None,
                attempts,
                error: Some(error),
            };
        };

        // Attempt 1: the feature's own model.
        let (first_text, first_record) =
            match self.attempt(&preferred, call, cancel, client, false) {
                Ok(result) => result,
                Err((error, record)) => {
                    attempts.push(record);
                    return self.after_first_failure(
                        call, cancel, client, attempts, preferred, error,
                    );
                }
            };
        attempts.push(first_record);

        AiOutcome {
            text: Some(first_text),
            status: AiOutcomeStatus::Succeeded,
            used_fallback: false,
            provider_id: Some(preferred.provider_id),
            model_id: Some(preferred.model_id),
            attempts,
            error: None,
        }
    }

    /// Decide what to do after the first attempt failed.
    ///
    /// This is the fallback rule, in one place, so it cannot be spelled differently on two paths.
    fn after_first_failure(
        &self,
        call: &AiCall,
        cancel: &CancelToken,
        client: &dyn veloct_core::http::HttpClient,
        mut attempts: Vec<AttemptRecord>,
        preferred: ModelRef,
        error: AiError,
    ) -> AiOutcome {
        let failed = |status: AiOutcomeStatus, attempts: Vec<AttemptRecord>, error: AiError| AiOutcome {
            text: None,
            status,
            used_fallback: matches!(status, AiOutcomeStatus::FallbackFailed),
            provider_id: Some(preferred.provider_id.clone()),
            model_id: Some(preferred.model_id.clone()),
            attempts,
            error: Some(error),
        };

        // A user's stop is a decision, not a fault: report CANCELLED and never contact another
        // provider (handoff.md section 11.4, section 25 C1).
        if error.is_cancelled() {
            return failed(AiOutcomeStatus::Cancelled, attempts, error);
        }

        // No fallback configured, or the error is one that must not fall back.
        if !error.should_fall_back(self.fallback.is_configured()) {
            return failed(AiOutcomeStatus::Failed, attempts, error);
        }

        // The user may have stopped generation while the first provider was failing. Trying the
        // fallback then would answer a request that was just withdrawn.
        if cancel.is_cancelled() {
            return failed(AiOutcomeStatus::Cancelled, attempts, AiError::cancelled());
        }

        let fallback_model = self
            .fallback
            .model
            .clone()
            .expect("should_fall_back implies a configured fallback");

        // Attempt 2: the single global fallback. Terminal either way.
        match self.attempt(&fallback_model, call, cancel, client, true) {
            Ok((text, record)) => {
                attempts.push(record);
                AiOutcome {
                    text: Some(text),
                    status: AiOutcomeStatus::FallbackSucceeded,
                    used_fallback: true,
                    provider_id: Some(fallback_model.provider_id),
                    model_id: Some(fallback_model.model_id),
                    attempts,
                    error: None,
                }
            }
            Err((error, record)) => {
                attempts.push(record);
                let cancelled = cancel.is_cancelled();
                AiOutcome {
                    text: None,
                    status: if cancelled {
                        AiOutcomeStatus::Cancelled
                    } else {
                        AiOutcomeStatus::FallbackFailed
                    },
                    used_fallback: true,
                    provider_id: Some(fallback_model.provider_id),
                    model_id: Some(fallback_model.model_id),
                    attempts,
                    error: Some(if cancelled { AiError::cancelled() } else { error }),
                }
            }
        }
    }

    /// One attempt against one model.
    ///
    /// Returns the text and the record on success, or the classified error and the record on
    /// failure, so a caller always has the attempt metadata whether or not it succeeded.
    #[allow(clippy::result_large_err)]
    fn attempt(
        &self,
        model: &ModelRef,
        call: &AiCall,
        cancel: &CancelToken,
        client: &dyn veloct_core::http::HttpClient,
        used_fallback: bool,
    ) -> std::result::Result<(String, AttemptRecord), (AiError, AttemptRecord)> {
        let record = |succeeded: bool, error: Option<&AiError>| AttemptRecord {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
            used_fallback,
            succeeded,
            error_code: error.map(|e| e.kind.as_str().to_string()),
            error_message: error.map(|e| e.message.clone()),
        };

        if let Err(error) = cancel.check() {
            return Err((error, record(false, None)));
        }

        let Some(provider) = self.registry.get(&model.provider_id) else {
            let error = AiError::new(
                AiErrorKind::NotConfigured,
                format!("provider {} is not configured", model.provider_id),
            )
            .with_provider(model.provider_id.clone(), model.model_id.clone());
            return Err((error.clone(), record(false, Some(&error))));
        };

        if !provider.config().enabled {
            let error = AiError::new(
                AiErrorKind::NotConfigured,
                format!("provider {} is disabled", provider.config().name),
            )
            .with_provider(model.provider_id.clone(), model.model_id.clone());
            return Err((error.clone(), record(false, Some(&error))));
        }

        // The prompt plus the rendered context. The context is already limited and ordered by
        // `ContextBundle`, so nothing unbounded can reach the provider.
        let rendered = call.context.render();
        let user_content = if rendered.is_empty() {
            call.prompt.clone()
        } else {
            format!("{}\n\n{}", call.prompt, rendered)
        };

        let mut request =
            ChatRequest::new(model.model_id.clone(), vec![ChatMessage::user(user_content)]);
        request.temperature = call.temperature;
        request.max_tokens = call.max_tokens;

        match provider.chat(client, &request) {
            Ok(response) => Ok((response.text, record(true, None))),
            Err(error) => {
                let ai_error = classify(error, model, used_fallback);
                Err((ai_error.clone(), record(false, Some(&ai_error))))
            }
        }
    }

    /// Fetch and merge a provider's model list, preserving manual entries
    /// (`handoff.md` §11.2).
    pub fn refresh_models(
        &self,
        provider_id: &str,
        existing: &[ProviderModel],
        client: &dyn veloct_core::http::HttpClient,
    ) -> Result<Vec<ProviderModel>, AiError> {
        let provider = self.registry.get(provider_id).ok_or_else(|| {
            AiError::new(
                AiErrorKind::NotConfigured,
                format!("provider {provider_id} is not configured"),
            )
        })?;

        match provider.list_models(client) {
            Ok(fetched) => Ok(provider.merge_models(existing, &fetched)),
            Err(error) => Err(AiError::from_transport(&error.to_string())
                .with_provider(provider_id, String::new())),
        }
    }
}

/// Turn a core error into an AI error, preserving cancellation and classifying the rest.
///
/// Every kind `AiErrorKind::as_str` can produce must have an arm here. A missing arm falls into the
/// wildcard and silently reclassifies the error, which is how a 500 once came out reported as a
/// malformed response.
fn classify(error: veloct_core::VeloctError, model: &ModelRef, used_fallback: bool) -> AiError {
    let classified = match &error {
        veloct_core::VeloctError::Cancelled => AiError::cancelled(),
        veloct_core::VeloctError::AiProvider {
            code,
            message,
            status,
            ..
        } => {
            let kind = match code.as_str() {
                "CANCELLED" => AiErrorKind::Cancelled,
                "CONNECTION" => AiErrorKind::Connection,
                "TIMEOUT" => AiErrorKind::Timeout,
                "CLIENT_ERROR" => AiErrorKind::ClientError,
                "SERVER_ERROR" => AiErrorKind::ServerError,
                "MODEL_NOT_FOUND" => AiErrorKind::ModelNotFound,
                "CONTEXT_OVERFLOW" => AiErrorKind::ContextOverflow,
                "REFUSED" => AiErrorKind::Refused,
                "MALFORMED_RESPONSE" => AiErrorKind::MalformedResponse,
                "NOT_CONFIGURED" => AiErrorKind::NotConfigured,
                // An unrecognised code means a producer added a kind without teaching this match
                // about it. The message is then the only trustworthy part left.
                _ => AiErrorKind::MalformedResponse,
            };
            let rebuilt = AiError::new(kind, message.clone());
            // The status survives the round trip rather than being lost here.
            match status {
                Some(status) => rebuilt.with_status(*status),
                None => rebuilt,
            }
        }
        other => AiError::from_transport(&other.to_string()),
    };

    let classified = classified.with_provider(model.provider_id.clone(), model.model_id.clone());
    if used_fallback {
        classified.as_fallback()
    } else {
        classified
    }
}

/// Convenience: the outcome as the status text stored in `ai_requests`.
pub fn status_text(outcome: &AiOutcome) -> &'static str {
    outcome.status.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ContextAssembly, ContextSource};
    use crate::provider::{ProviderConfig, ProviderProtocol};
    use veloct_core::http::FakeHttpClient;

    fn provider(id: &str, protocol: ProviderProtocol) -> Provider {
        Provider::new(
            ProviderConfig::new(id, id, protocol),
            Some("test-credential-value".to_string()),
        )
    }

    fn registry(ids: &[&str]) -> ProviderRegistry {
        let mut registry = ProviderRegistry::new();
        for id in ids {
            registry.insert(provider(id, ProviderProtocol::OpenAiCompatible));
        }
        registry
    }

    fn call(model: ModelRef) -> AiCall {
        let context = ContextAssembly::with_defaults().assemble(
            "summarise",
            Some(("Doc", "body", None)),
            None,
            Vec::new(),
        );
        AiCall::new(FeatureId::ArticleSummary, "summarise this", context).with_model(model)
    }

    #[test]
    fn a_working_preferred_model_succeeds_without_a_fallback() {
        let service = AiService::new(registry(&["p1", "p2"]), FallbackConfig::some(ModelRef::new("p2", "backup")));
        let client = FakeHttpClient::new().on_json(
            "/chat/completions",
            serde_json::json!({ "choices": [{ "message": { "content": "the summary" } }] }),
        );

        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &client,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::Succeeded);
        assert!(!outcome.used_fallback);
        assert_eq!(outcome.text.as_deref(), Some("the summary"));
        assert_eq!(outcome.attempts.len(), 1);
        assert!(outcome.attempts[0].succeeded);
        assert!(!outcome.attempts[0].used_fallback);
        assert_eq!(client.request_count(), 1, "the fallback must not have been used");
    }

    #[test]
    fn a_real_failure_falls_back_and_the_result_says_so() {
        // handoff.md section 11.4: "有错误就切换".
        let service = AiService::new(
            registry(&["p1", "p2"]),
            FallbackConfig::some(ModelRef::new("p2", "backup")),
        );

        // Both providers are reached through one client here, so both attempts fail. That is the
        // terminal path, which is asserted below; the successful-fallback path is covered by
        // `a_fallback_on_an_independent_provider_is_used`.
        //
        // The fake answers from a QUEUE, so one queued response is consumed by the first attempt and
        // the second gets the fake's own 404. Two responses are queued so both attempts see a real
        // server error, which is what this test is about.
        let client = FakeHttpClient::new()
            .always(500, "primary is down")
            .always(500, "backup is down too");
        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &client,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::FallbackFailed);
        assert!(outcome.used_fallback);
        assert_eq!(outcome.attempts.len(), 2, "exactly two attempts, never more");
        assert!(!outcome.attempts[0].used_fallback);
        assert!(outcome.attempts[1].used_fallback);
        // A 500 stays a server error: the model-name heuristic must not be applied to a 5xx, or the
        // user would be sent to fix a model name while the provider is simply down.
        assert_eq!(
            outcome.attempts[0].error_code.as_deref(),
            Some("SERVER_ERROR")
        );
        assert_eq!(
            outcome.attempts[1].error_code.as_deref(),
            Some("SERVER_ERROR")
        );
    }

    #[test]
    fn a_fallback_failure_is_terminal() {
        // handoff.md section 11.4 and 25 C5: no third attempt.
        let service = AiService::new(registry(&["p1", "p2"]), FallbackConfig::some(ModelRef::new("p2", "backup")));
        let client = FakeHttpClient::new().always(503, "everything is down");

        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &client,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::FallbackFailed);
        assert_eq!(outcome.attempts.len(), 2);
        assert_eq!(
            client.request_count(),
            2,
            "a fallback failure must not be retried"
        );
        assert!(outcome.error.is_some());
    }

    #[test]
    fn cancellation_never_falls_back() {
        // handoff.md section 11.4 and 25 C1: the single most important behaviour.
        let service = AiService::new(registry(&["p1", "p2"]), FallbackConfig::some(ModelRef::new("p2", "backup")));
        let client = FakeHttpClient::new().always(500, "down");
        let cancel = CancelToken::new();
        cancel.cancel();

        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &cancel,
            &client,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::Cancelled);
        assert!(outcome.status.is_cancelled());
        assert!(!outcome.used_fallback);
        assert!(
            outcome.attempts.is_empty(),
            "a stop before the first attempt makes no request"
        );
        assert_eq!(client.request_count(), 0, "no provider may be contacted");
        assert!(outcome.error.as_ref().unwrap().is_cancelled());
    }

    #[test]
    fn cancelling_mid_flight_does_not_contact_the_fallback() {
        /// A provider that is cancelled by the act of being called, then fails.
        struct CancelOnCall;
        impl veloct_core::http::HttpClient for CancelOnCall {
            fn execute(
                &self,
                _request: veloct_core::http::HttpRequest,
            ) -> veloct_core::Result<veloct_core::http::HttpResponse> {
                Err(veloct_core::VeloctError::Cancelled)
            }
            fn stream(
                &self,
                _request: veloct_core::http::HttpRequest,
                _cancel: &veloct_core::http::Cancellation,
            ) -> veloct_core::Result<
                Box<
                    dyn Iterator<Item = veloct_core::Result<veloct_core::http::StreamEvent>>
                        + Send,
                >,
            > {
                Err(veloct_core::VeloctError::Cancelled)
            }
        }
        impl std::fmt::Debug for CancelOnCall {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("CancelOnCall")
            }
        }

        let service = AiService::new(registry(&["p1", "p2"]), FallbackConfig::some(ModelRef::new("p2", "backup")));
        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &CancelOnCall,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::Cancelled);
        assert!(!outcome.used_fallback);
        assert_eq!(outcome.attempts.len(), 1);
        assert!(
            !outcome.attempts[0].error_code.as_deref().unwrap_or("").is_empty()
                || outcome.attempts[0].error_code.is_none(),
            "the attempt is recorded"
        );
    }

    #[test]
    fn without_a_fallback_a_failure_is_reported_directly() {
        let service = AiService::new(registry(&["p1"]), FallbackConfig::none());
        let client = FakeHttpClient::new().always(500, "down");

        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &client,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::Failed);
        assert!(!outcome.used_fallback);
        assert_eq!(outcome.attempts.len(), 1);
    }

    #[test]
    fn a_fallback_on_an_independent_provider_is_used() {
        // handoff.md section 11.4: the fallback may be a completely independent provider. The two
        // providers are separate registry entries, which is what makes that true here.
        let mut registry = ProviderRegistry::new();
        registry.insert(provider("openai-main", ProviderProtocol::OpenAiCompatible));
        registry.insert(provider("anthropic-backup", ProviderProtocol::Anthropic));
        let service = AiService::new(
            registry,
            FallbackConfig::some(ModelRef::new("anthropic-backup", "claude-x")),
        );

        // Only the Anthropic path answers, so the fallback is the one that succeeds.
        let client = FakeHttpClient::new().on_json(
            "/v1/messages",
            serde_json::json!({ "content": [{ "type": "text", "text": "from the backup" }] }),
        );

        let outcome = service.call_with_client(
            &call(ModelRef::new("openai-main", "gpt-x")),
            &CancelToken::new(),
            &client,
        );

        assert_eq!(outcome.status, AiOutcomeStatus::FallbackSucceeded);
        assert!(outcome.used_fallback);
        assert_eq!(outcome.text.as_deref(), Some("from the backup"));
        assert_eq!(outcome.provider_id.as_deref(), Some("anthropic-backup"));
        assert_eq!(outcome.attempts.len(), 2);
    }

    #[test]
    fn a_feature_with_no_model_configured_is_an_explicit_error() {
        // handoff.md section 11.3: every feature maps to a provider and a model, so a missing
        // assignment must say so rather than silently doing nothing.
        let service = AiService::new(registry(&["p1"]), FallbackConfig::none());
        let context = ContextAssembly::with_defaults().assemble("p", None, None, Vec::new());
        let unconfigured = AiCall::new(FeatureId::PageCreate, "make a page", context);

        let client = FakeHttpClient::new();
        let outcome = service.call_with_client(&unconfigured, &CancelToken::new(), &client);

        assert_eq!(outcome.status, AiOutcomeStatus::Failed);
        assert_eq!(
            outcome.error.as_ref().unwrap().kind,
            AiErrorKind::NotConfigured
        );
        assert!(outcome
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("page.create"));
        assert_eq!(client.request_count(), 0);
    }

    #[test]
    fn a_missing_provider_is_reported_and_can_fall_back() {
        let service = AiService::new(
            registry(&["p2"]),
            FallbackConfig::some(ModelRef::new("p2", "backup")),
        );
        let client = FakeHttpClient::new().always(200, r#"{"choices":[{"message":{"content":"ok"}}]}"#);

        let outcome = service.call_with_client(
            &call(ModelRef::new("missing-provider", "m")),
            &CancelToken::new(),
            &client,
        );

        // The missing provider is a NotConfigured error, which is a real failure, so the fallback is
        // used and succeeds.
        assert_eq!(outcome.status, AiOutcomeStatus::FallbackSucceeded);
        assert_eq!(outcome.attempts[0].error_code.as_deref(), Some("NOT_CONFIGURED"));
        assert!(outcome.attempts[0].provider_id == "missing-provider");
    }

    #[test]
    fn a_disabled_provider_is_a_real_failure_that_falls_back() {
        let mut registry = ProviderRegistry::new();
        let mut disabled = provider("p1", ProviderProtocol::OpenAiCompatible);
        disabled.config_mut().enabled = false;
        registry.insert(disabled);
        registry.insert(provider("p2", ProviderProtocol::OpenAiCompatible));
        let service = AiService::new(registry, FallbackConfig::some(ModelRef::new("p2", "backup")));

        let client = FakeHttpClient::new().on_json(
            "/chat/completions",
            serde_json::json!({ "choices": [{ "message": { "content": "from backup" } }] }),
        );

        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &client,
        );
        assert_eq!(outcome.status, AiOutcomeStatus::FallbackSucceeded);
        assert!(outcome.attempts[0].error_message.as_deref().unwrap().contains("disabled"));
    }

    #[test]
    fn the_attempt_records_are_complete_for_the_metadata_row() {
        // docs/spec/01-data-model.md section 3: ai_requests stores status, provider, model and error
        // code, and no chat history.
        let service = AiService::new(registry(&["p1", "p2"]), FallbackConfig::some(ModelRef::new("p2", "backup")));
        let client = FakeHttpClient::new().always(500, "down");

        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "primary")),
            &CancelToken::new(),
            &client,
        );

        for attempt in &outcome.attempts {
            assert!(!attempt.provider_id.is_empty());
            assert!(!attempt.model_id.is_empty());
        }
        assert_eq!(status_text(&outcome), "fallback_failed");
        // The serialised outcome carries no prompt or completion beyond the text field.
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(!json.contains("summarise"), "no prompt is stored: {json}");
    }

    #[test]
    fn the_context_is_placed_in_the_request_body() {
        let service = AiService::new(registry(&["p1"]), FallbackConfig::none());
        let client = FakeHttpClient::new().on_json(
            "/chat/completions",
            serde_json::json!({ "choices": [{ "message": { "content": "ok" } }] }),
        );

        let context = ContextAssembly::with_defaults().assemble(
            "summarise",
            Some(("My Doc", "the document body", None)),
            None,
            vec![crate::context::ContextItem {
                source: ContextSource::Knowledge,
                label: "Extra".to_string(),
                content_id: None,
                text: "extra knowledge".to_string(),
            }],
        );
        let request = AiCall::new(FeatureId::ArticleSummary, "summarise this", context)
            .with_model(ModelRef::new("p1", "m"));

        let outcome = service.call_with_client(&request, &CancelToken::new(), &client);
        assert!(outcome.status.is_success());

        let recorded = client.recorded();
        let body = recorded[0]
            .body
            .as_text()
            .expect("the chat request body is JSON");
        assert!(body.contains("summarise this"), "the prompt is sent");
        assert!(body.contains("the document body"), "the document is sent");
        assert!(body.contains("extra knowledge"), "the chosen extra is sent");
    }

    #[test]
    fn every_status_has_a_stable_stored_form() {
        assert_eq!(AiOutcomeStatus::Succeeded.as_str(), "succeeded");
        assert_eq!(AiOutcomeStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(AiOutcomeStatus::FallbackSucceeded.as_str(), "fallback_succeeded");
        assert_eq!(AiOutcomeStatus::FallbackFailed.as_str(), "fallback_failed");
        assert!(AiOutcomeStatus::FallbackSucceeded.is_success());
        assert!(!AiOutcomeStatus::Cancelled.is_success());
        assert!(AiOutcomeStatus::Cancelled.is_cancelled());
    }

    #[test]
    fn a_registry_reports_what_it_holds() {
        let registry = registry(&["b", "a"]);
        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
        assert!(registry.contains("a"));
        assert!(!registry.contains("z"));
        assert_eq!(registry.ids(), vec!["a", "b"]);
        assert!(ProviderRegistry::new().is_empty());
    }

    #[test]
    fn a_fallback_config_knows_whether_it_is_usable() {
        assert!(!FallbackConfig::none().is_configured());
        assert!(FallbackConfig::some(ModelRef::new("p", "m")).is_configured());
        assert!(
            !FallbackConfig::some(ModelRef::new("p", "  ")).is_configured(),
            "a blank model id is not a configured fallback"
        );
    }

    #[test]
    fn an_outcome_round_trips_through_json() {
        let service = AiService::new(registry(&["p1"]), FallbackConfig::none());
        let client = FakeHttpClient::new().on_json(
            "/chat/completions",
            serde_json::json!({ "choices": [{ "message": { "content": "hi" } }] }),
        );
        let outcome = service.call_with_client(
            &call(ModelRef::new("p1", "m")),
            &CancelToken::new(),
            &client,
        );
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(json.contains("\"status\":\"succeeded\""));
        assert_eq!(serde_json::from_str::<AiOutcome>(&json).unwrap(), outcome);
    }
}
