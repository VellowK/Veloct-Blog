//! The AI error boundary.
//!
//! `handoff.md` §11.4 draws one distinction that the whole subsystem depends on:
//!
//! > 用户主动点击"停止生成"属于 `CANCELLED`，**不是错误**，不触发 fallback。
//!
//! A user cancelling is a decision, not a fault, so it must not be reported as a failure and must
//! not cause the fallback model to be tried — doing that would answer a request the user just
//! withdrew, and would burn tokens on their behalf. Every other condition, including a provider
//! refusal and a context overflow, is a real failure and does fall back.

use serde::{Deserialize, Serialize};

use veloct_core::VeloctError;

/// What went wrong, in the terms the fallback rule is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    /// The user asked to stop. NOT an error; never triggers fallback.
    Cancelled,
    /// The provider could not be reached.
    Connection,
    /// The request exceeded the configured timeout.
    Timeout,
    /// The provider rejected the request because of its content (4xx).
    ClientError,
    /// The provider failed (5xx).
    ServerError,
    /// The named model does not exist on the provider.
    ModelNotFound,
    /// The request exceeded the model's context window.
    ContextOverflow,
    /// The provider refused to answer.
    Refused,
    /// The provider answered with something that could not be parsed.
    MalformedResponse,
    /// No provider or model is configured for this feature.
    NotConfigured,
}

impl AiErrorKind {
    /// Whether this condition triggers the fallback model.
    ///
    /// This single function is the contract. `Cancelled` is the only kind that does not fall back,
    /// and it is first in the list so the intent is impossible to miss.
    pub fn triggers_fallback(self) -> bool {
        !matches!(self, Self::Cancelled)
    }

    /// Whether the condition is worth telling the user about in the UI.
    pub fn is_user_visible(self) -> bool {
        !matches!(self, Self::Cancelled)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cancelled => "CANCELLED",
            Self::Connection => "CONNECTION",
            Self::Timeout => "TIMEOUT",
            Self::ClientError => "CLIENT_ERROR",
            Self::ServerError => "SERVER_ERROR",
            Self::ModelNotFound => "MODEL_NOT_FOUND",
            Self::ContextOverflow => "CONTEXT_OVERFLOW",
            Self::Refused => "REFUSED",
            Self::MalformedResponse => "MALFORMED_RESPONSE",
            Self::NotConfigured => "NOT_CONFIGURED",
        }
    }
}

/// An AI failure, carrying enough context to explain it and no secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    /// Which provider and model were being used.
    pub provider_id: Option<String>,
    pub model: Option<String>,
    /// True when this happened on the fallback attempt, so the UI does not promise another retry.
    pub was_fallback: bool,
    /// A message safe to show and to log: redacted at construction.
    pub message: String,
    /// The HTTP status, when there was one.
    pub status: Option<u16>,
}

impl AiError {
    pub fn new(kind: AiErrorKind, message: impl Into<String>) -> Self {
        // Redaction happens here rather than at every call site, so a provider error body that
        // echoes a credential back cannot reach a log or the UI (`handoff.md` §18.1).
        Self {
            kind,
            provider_id: None,
            model: None,
            was_fallback: false,
            message: veloct_core::secret::redact(&message.into()),
            status: None,
        }
    }

    /// The cancellation sentinel. Deliberately its own constructor so no call site can accidentally
    /// build a cancellable-looking error with another kind.
    pub fn cancelled() -> Self {
        Self {
            kind: AiErrorKind::Cancelled,
            provider_id: None,
            model: None,
            was_fallback: false,
            message: "the user stopped generation".to_string(),
            status: None,
        }
    }

    pub fn with_provider(mut self, provider_id: impl Into<String>, model: impl Into<String>) -> Self {
        self.provider_id = Some(provider_id.into());
        self.model = Some(model.into());
        self
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    pub fn as_fallback(mut self) -> Self {
        self.was_fallback = true;
        self
    }

    /// Classify an HTTP status code.
    ///
    /// The body is only inspected for the two conditions the user can act on — a wrong model name
    /// and an over-long context — and only on a 4xx. Applying those heuristics to a 5xx would report
    /// "your model name is wrong" when the provider is simply down, sending the user to fix
    /// something that is not broken.
    pub fn from_status(status: u16, body: &str) -> Self {
        let lowered = body.to_ascii_lowercase();
        let kind = match status {
            400..=499 => {
                if status == 404 || (lowered.contains("model") && lowered.contains("not found")) {
                    AiErrorKind::ModelNotFound
                } else if lowered.contains("context")
                    && (lowered.contains("length") || lowered.contains("window"))
                {
                    AiErrorKind::ContextOverflow
                } else {
                    AiErrorKind::ClientError
                }
            }
            500..=599 => AiErrorKind::ServerError,
            _ => AiErrorKind::MalformedResponse,
        };
        Self::new(kind, format!("provider returned HTTP {status}: {body}")).with_status(status)
    }

    /// Classify a transport failure from the core HTTP seam.
    ///
    /// A message mentioning "timed out" is the seam's own wording, checked here so a timeout is not
    /// reported as a generic connection failure. Everything else that failed before a response
    /// arrived is a connection problem, which is what the caller can act on.
    pub fn from_transport(message: &str) -> Self {
        let lowered = message.to_ascii_lowercase();
        let kind = if lowered.contains("timed out") || lowered.contains("timeout") {
            AiErrorKind::Timeout
        } else {
            AiErrorKind::Connection
        };
        Self::new(kind, message)
    }

    /// Convert a cancellation raised by the core seam into the right error.
    pub fn from_core(error: &VeloctError, fallback_kind: AiErrorKind) -> Self {
        match error {
            VeloctError::Cancelled => Self::cancelled(),
            other => Self::new(fallback_kind, other.to_string()),
        }
    }

    /// Whether a fallback attempt should be made for this error.
    pub fn should_fall_back(&self, fallback_available: bool) -> bool {
        // Two conditions, both required: the error must be a real one, AND a fallback must exist and
        // not already have failed. The second is what keeps a fallback failure terminal
        // (`handoff.md` §11.4, §25 C5).
        fallback_available && !self.was_fallback && self.kind.triggers_fallback()
    }

    pub fn is_cancelled(&self) -> bool {
        self.kind == AiErrorKind::Cancelled
    }
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let where_ = match (&self.provider_id, &self.model) {
            (Some(provider), Some(model)) => format!(" [{provider}/{model}]"),
            (Some(provider), None) => format!(" [{provider}]"),
            _ => String::new(),
        };
        let suffix = if self.was_fallback {
            " (fallback attempt)"
        } else {
            ""
        };
        write!(f, "{}{where_}: {}{suffix}", self.kind.as_str(), self.message)
    }
}

impl From<AiError> for VeloctError {
    fn from(error: AiError) -> Self {
        if error.is_cancelled() {
            return VeloctError::Cancelled;
        }
        VeloctError::AiProvider {
            provider: error
                .provider_id
                .clone()
                .unwrap_or_else(|| "unconfigured".to_string()),
            code: error.kind.as_str().to_string(),
            message: error.message,
            // Every classified kind except cancellation is a condition a retry or a fallback could
            // change, so it is marked retryable.
            retryable: true,
            // Carried across so the status is not lost on the way out of this crate.
            status: error.status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_is_not_an_error_and_never_falls_back() {
        // handoff.md section 11.4 and 25 C1: the single most important rule in this crate.
        let error = AiError::cancelled();
        assert!(error.is_cancelled());
        assert_eq!(error.kind, AiErrorKind::Cancelled);
        assert!(!error.kind.triggers_fallback());
        assert!(!error.should_fall_back(true), "a fallback must not be attempted");
        assert!(!error.kind.is_user_visible(), "a user's own decision is not a fault");
    }

    #[test]
    fn every_other_failure_does_fall_back() {
        // handoff.md section 11.4: "有错误就切换".
        for kind in [
            AiErrorKind::Connection,
            AiErrorKind::Timeout,
            AiErrorKind::ClientError,
            AiErrorKind::ServerError,
            AiErrorKind::ModelNotFound,
            AiErrorKind::ContextOverflow,
            AiErrorKind::Refused,
            AiErrorKind::MalformedResponse,
            AiErrorKind::NotConfigured,
        ] {
            assert!(
                kind.triggers_fallback(),
                "{kind:?} must trigger the fallback model"
            );
            assert!(kind.is_user_visible(), "{kind:?} must be shown to the user");
        }
    }

    #[test]
    fn a_fallback_failure_is_terminal() {
        // handoff.md section 11.4 and 25 C5: no loop back into the fallback.
        let first = AiError::new(AiErrorKind::ServerError, "500");
        assert!(first.should_fall_back(true), "the first real failure falls back");

        let second = AiError::new(AiErrorKind::ServerError, "500").as_fallback();
        assert!(
            !second.should_fall_back(true),
            "a fallback failure must not fall back again"
        );
    }

    #[test]
    fn no_fallback_configured_means_no_fallback() {
        let error = AiError::new(AiErrorKind::Connection, "refused");
        assert!(!error.should_fall_back(false));
    }

    #[test]
    fn http_statuses_are_classified_into_the_documented_kinds() {
        assert_eq!(
            AiError::from_status(500, "boom").kind,
            AiErrorKind::ServerError
        );
        assert_eq!(
            AiError::from_status(503, "unavailable").kind,
            AiErrorKind::ServerError
        );
        assert_eq!(
            AiError::from_status(400, "bad request").kind,
            AiErrorKind::ClientError
        );
        assert_eq!(
            AiError::from_status(404, "no such route").kind,
            AiErrorKind::ModelNotFound
        );
        assert_eq!(
            AiError::from_status(400, "the model gpt-9 was not found").kind,
            AiErrorKind::ModelNotFound
        );
        assert_eq!(
            AiError::from_status(400, "context length exceeded").kind,
            AiErrorKind::ContextOverflow
        );
        assert_eq!(AiError::from_status(200, "?").kind, AiErrorKind::MalformedResponse);
    }

    #[test]
    fn transport_failures_distinguish_a_timeout_from_a_refusal() {
        assert_eq!(
            AiError::from_transport("request to https://x timed out").kind,
            AiErrorKind::Timeout
        );
        assert_eq!(
            AiError::from_transport("could not connect to https://x").kind,
            AiErrorKind::Connection
        );
    }

    #[test]
    fn the_core_cancellation_sentinel_maps_to_cancellation() {
        // The core HTTP seam reports cancellation as VeloctError::Cancelled; it must not become a
        // provider error here.
        let error = AiError::from_core(&VeloctError::Cancelled, AiErrorKind::Connection);
        assert!(error.is_cancelled());
        assert!(!error.should_fall_back(true));
    }

    #[test]
    fn a_credential_in_a_provider_error_body_is_redacted() {
        // handoff.md section 18.1: a provider can echo a key back in an error body.
        let error = AiError::new(
            AiErrorKind::ClientError,
            "invalid key sk-ant-api03-abcdefghijklmnopqrstuvwxyz for this endpoint",
        );
        assert!(!error.message.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz"));
        assert!(error.message.contains("[REDACTED]"));
        assert_eq!(error.kind, AiErrorKind::ClientError, "classification is unaffected");
    }

    #[test]
    fn the_error_display_names_the_provider_and_the_attempt() {
        let error = AiError::new(AiErrorKind::Timeout, "took too long")
            .with_provider("openai-main", "gpt-x")
            .as_fallback();
        let text = error.to_string();
        assert!(text.contains("TIMEOUT"), "{text}");
        assert!(text.contains("openai-main/gpt-x"), "{text}");
        assert!(text.contains("fallback attempt"), "{text}");
    }

    #[test]
    fn conversion_to_a_core_error_keeps_the_code_and_the_cancellation() {
        let cancelled: VeloctError = AiError::cancelled().into();
        assert_eq!(cancelled.code(), "CANCELLED");

        let failed: VeloctError = AiError::new(AiErrorKind::ServerError, "500")
            .with_provider("p", "m")
            .into();
        assert_eq!(failed.code(), "AI_PROVIDER");
        // The AI kind is carried in the provider error's `code` field, so the classification is not
        // lost when the error crosses into the rest of the app.
        match &failed {
            VeloctError::AiProvider { code, provider, .. } => {
                assert_eq!(code, "SERVER_ERROR");
                assert_eq!(provider, "p");
            }
            other => panic!("expected AiProvider, got {other:?}"),
        }
        assert!(failed.to_string().contains("SERVER_ERROR"), "{failed}");
    }

    #[test]
    fn errors_round_trip_through_json_for_the_frontend() {
        let error = AiError::from_status(429, "rate limited").with_provider("p", "m");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"kind\":\"client_error\""));
        assert_eq!(serde_json::from_str::<AiError>(&json).unwrap(), error);
    }

    #[test]
    fn the_http_status_survives_the_round_trip_through_a_core_error() {
        // Regression: the status used to be dropped when the error crossed into VeloctError, so a UI
        // could not show "429" without string matching the message. Both the field and the message
        // must carry it.
        let original = AiError::from_status(429, "rate limited").with_provider("p", "m");
        assert_eq!(original.status, Some(429));

        let converted: VeloctError = original.into();
        match &converted {
            VeloctError::AiProvider {
                status,
                code,
                message,
                ..
            } => {
                assert_eq!(*status, Some(429), "the status must not be lost");
                assert_eq!(code, "CLIENT_ERROR");
                assert!(message.contains("429"));
            }
            other => panic!("expected AiProvider, got {other:?}"),
        }
    }

    #[test]
    fn a_classification_is_never_silently_widened() {
        // Regression: a 500 was reported as MODEL_NOT_FOUND because the model-name heuristic was
        // applied to every status. A caller would then send the user to fix a model name while the
        // provider was simply down.
        assert_eq!(
            AiError::from_status(500, "the model backend is not found").kind,
            AiErrorKind::ServerError,
            "a 5xx must stay a server error regardless of its body"
        );
        assert_eq!(
            AiError::from_status(503, "context length exceeded upstream").kind,
            AiErrorKind::ServerError
        );
    }
}
