//! Providers: configuration, protocols and the request/response shapes.
//!
//! `handoff.md` §11.1 requires at least OpenAI-compatible, Anthropic and Ollama, each with a
//! user-editable Base URL, a credential reference and a model list. §11.2 requires fetching the
//! model list, adding a model manually and refreshing.
//!
//! Every provider talks through [`veloct_core::http::HttpClient`], so each protocol is tested against
//! a fake rather than a live vendor, and no crate has to know about TLS or a runtime.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use veloct_core::http::{HttpClient, HttpRequest, HttpResponse};
use veloct_core::Result;

use crate::error::{AiError, AiErrorKind};

/// Which wire protocol a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderProtocol {
    /// `POST {base}/chat/completions`, bearer auth. Used by OpenAI and most compatible gateways.
    OpenAiCompatible,
    /// `POST {base}/v1/messages`, `x-api-key` plus an API version header.
    Anthropic,
    /// `POST {base}/api/chat`, no auth by default because it is normally a local daemon.
    Ollama,
}

impl ProviderProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai",
            Self::Anthropic => "anthropic",
            Self::Ollama => "ollama",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" | "openai_compatible" | "openai-compatible" => Some(Self::OpenAiCompatible),
            "anthropic" => Some(Self::Anthropic),
            "ollama" => Some(Self::Ollama),
            _ => None,
        }
    }

    /// The default Base URL, used when the user has not set one.
    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Ollama => "http://127.0.0.1:11434",
        }
    }

    /// Whether a credential is expected. Ollama normally runs locally without one.
    pub fn requires_credential(self) -> bool {
        !matches!(self, Self::Ollama)
    }
}

/// A provider's non-secret configuration.
///
/// `credential_ref` is an opaque handle resolved through the OS credential store
/// (`handoff.md` §12). No secret value is held here, so this struct is safe to serialise into
/// SQLite, a log line or a release snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

impl ProviderConfig {
    /// A new provider with the protocol's default Base URL.
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        protocol: ProviderProtocol,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            protocol,
            base_url: protocol.default_base_url().to_string(),
            credential_ref: None,
            enabled: true,
        }
    }

    /// Validate the configuration before it is used, so a mistake is reported as a configuration
    /// problem rather than as a confusing network error.
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.base_url.trim().is_empty() {
            return Err("Base URL must not be empty".into());
        }
        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            return Err(format!(
                "Base URL must start with http:// or https://, got {:?}",
                self.base_url
            ));
        }
        if self.protocol.requires_credential() && self.credential_ref.is_none() {
            // A warning rather than a hard failure: the user may be mid-setup, and the connection
            // test is what reports the real problem.
            return Ok(());
        }
        Ok(())
    }

    /// Whether a credential handle is recorded.
    pub fn has_credential(&self) -> bool {
        self.credential_ref.is_some()
    }
}

/// One model offered by a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderModel {
    /// The id sent to the provider.
    pub model_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// How the model became known.
    pub source: ModelSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
}

/// Where a model entry came from (`handoff.md` §11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSource {
    /// Returned by the provider's model list endpoint.
    Fetched,
    /// Typed in by the user.
    Manual,
}

/// A provider plus a model, which is what a feature assignment stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

impl ModelRef {
    pub fn new(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        }
    }

    /// Whether a model id looks usable. An empty or whitespace id would produce a request the
    /// provider rejects with a confusing error.
    pub fn is_usable(&self) -> bool {
        !self.provider_id.trim().is_empty() && !self.model_id.trim().is_empty()
    }
}

/// One message in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// `system`, `user` or `assistant`.
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
        }
    }
}

/// A chat request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Whether the caller wants a streamed response (`handoff.md` §11.6).
    #[serde(default)]
    pub stream: bool,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            stream: false,
        }
    }
}

/// A chat response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Token usage, when the provider reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Why generation stopped, when the provider says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

impl ChatResponse {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            model: None,
            input_tokens: None,
            output_tokens: None,
            finish_reason: None,
        }
    }
}

/// A provider that can chat and list its models.
pub struct Provider {
    config: ProviderConfig,
    /// Resolved at call time from the credential store, never stored here.
    credential: Option<String>,
}

impl Provider {
    pub fn new(config: ProviderConfig, credential: Option<String>) -> Self {
        Self {
            config,
            credential,
        }
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    /// Mutable access to the configuration, for enabling or disabling a provider and for editing
    /// the Base URL before a connection test.
    pub fn config_mut(&mut self) -> &mut ProviderConfig {
        &mut self.config
    }

    pub fn protocol(&self) -> ProviderProtocol {
        self.config.protocol
    }

    /// Join the Base URL with a path, tolerating a trailing slash.
    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    /// Build the request for one protocol.
    ///
    /// Extracted so a test can assert the exact URL, headers and body each protocol produces,
    /// which is the only way to be sure an Ollama call does not accidentally carry a bearer token or
    /// that Anthropic uses its own header names.
    pub fn build_chat_request(&self, request: &ChatRequest) -> Result<HttpRequest> {
        let mut http = match self.protocol() {
            ProviderProtocol::OpenAiCompatible => {
                let body = serde_json::json!({
                    "model": request.model,
                    "messages": request
                        .messages
                        .iter()
                        .map(|message| serde_json::json!({
                            "role": message.role,
                            "content": message.content
                        }))
                        .collect::<Vec<_>>(),
                    "stream": request.stream,
                    "temperature": request.temperature,
                    "max_tokens": request.max_tokens,
                });
                HttpRequest::post(self.endpoint("chat/completions")).json(body)
            }
            ProviderProtocol::Anthropic => {
                // Anthropic takes the system prompt as a top-level field, not as a message.
                let system: Vec<&str> = request
                    .messages
                    .iter()
                    .filter(|message| message.role == "system")
                    .map(|message| message.content.as_str())
                    .collect();
                let messages: Vec<serde_json::Value> = request
                    .messages
                    .iter()
                    .filter(|message| message.role != "system")
                    .map(|message| {
                        serde_json::json!({
                            "role": message.role,
                            "content": message.content
                        })
                    })
                    .collect();

                let mut body = serde_json::json!({
                    "model": request.model,
                    "messages": messages,
                    "max_tokens": request.max_tokens.unwrap_or(4096),
                    "stream": request.stream,
                });
                if !system.is_empty() {
                    body["system"] = serde_json::json!(system.join("\n\n"));
                }
                if let Some(temperature) = request.temperature {
                    body["temperature"] = serde_json::json!(temperature);
                }

                HttpRequest::post(self.endpoint("v1/messages"))
                    .header("anthropic-version", "2023-06-01")
                    .json(body)
            }
            ProviderProtocol::Ollama => {
                let body = serde_json::json!({
                    "model": request.model,
                    "messages": request
                        .messages
                        .iter()
                        .map(|message| serde_json::json!({
                            "role": message.role,
                            "content": message.content
                        }))
                        .collect::<Vec<_>>(),
                    "stream": request.stream,
                    "options": {
                        "temperature": request.temperature,
                        "num_predict": request.max_tokens,
                    },
                });
                HttpRequest::post(self.endpoint("api/chat")).json(body)
            }
        };

        // Credentials are attached per protocol, and only when one has actually been resolved.
        if let Some(credential) = &self.credential {
            http = match self.protocol() {
                ProviderProtocol::OpenAiCompatible => http.bearer(credential),
                ProviderProtocol::Anthropic => http.header("x-api-key", credential),
                ProviderProtocol::Ollama => http,
            };
        }

        Ok(http)
    }

    /// Build the model-list request.
    pub fn build_models_request(&self) -> HttpRequest {
        let mut http = match self.protocol() {
            ProviderProtocol::OpenAiCompatible => HttpRequest::get(self.endpoint("models")),
            ProviderProtocol::Anthropic => HttpRequest::get(self.endpoint("v1/models"))
                .header("anthropic-version", "2023-06-01"),
            ProviderProtocol::Ollama => HttpRequest::get(self.endpoint("api/tags")),
        };

        if let Some(credential) = &self.credential {
            http = match self.protocol() {
                ProviderProtocol::OpenAiCompatible => http.bearer(credential),
                ProviderProtocol::Anthropic => http.header("x-api-key", credential),
                ProviderProtocol::Ollama => http,
            };
        }

        http
    }

    /// Perform a chat call.
    pub fn chat(&self, client: &dyn HttpClient, request: &ChatRequest) -> Result<ChatResponse> {
        let http = self.build_chat_request(request)?;
        let response = client.execute(http)?;
        self.parse_chat_response(response)
    }

    /// Fetch the model list (`handoff.md` §11.2).
    pub fn list_models(&self, client: &dyn HttpClient) -> Result<Vec<ProviderModel>> {
        let response = client.execute(self.build_models_request())?;
        self.parse_models_response(response)
    }

    /// Turn an HTTP response into a chat response, classifying every failure.
    fn parse_chat_response(&self, response: HttpResponse) -> Result<ChatResponse> {
        if !response.is_success() {
            return Err(AiError::from_status(response.status, &response.text())
                .with_provider(self.config.id.clone(), String::new())
                .into());
        }

        let value: serde_json::Value = response.json().map_err(|_| {
            AiError::new(
                AiErrorKind::MalformedResponse,
                format!(
                    "{} returned a body that is not JSON",
                    self.config.name
                ),
            )
            .with_provider(self.config.id.clone(), String::new())
        })?;

        let text = match self.protocol() {
            ProviderProtocol::OpenAiCompatible => value
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str())
                .map(str::to_string),
            ProviderProtocol::Anthropic => value
                .get("content")
                .and_then(|content| content.as_array())
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.get("text").and_then(|text| text.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                }),
            ProviderProtocol::Ollama => value
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(|content| content.as_str())
                .map(str::to_string),
        };

        let text = text.ok_or_else(|| {
            AiError::new(
                AiErrorKind::MalformedResponse,
                format!("{} returned a response with no text content", self.config.name),
            )
            .with_provider(self.config.id.clone(), String::new())
        })?;

        Ok(ChatResponse {
            text,
            model: value
                .get("model")
                .and_then(|model| model.as_str())
                .map(str::to_string),
            input_tokens: value
                .get("usage")
                .and_then(|usage| usage.get("prompt_tokens").or_else(|| usage.get("input_tokens")))
                .and_then(|tokens| tokens.as_u64()),
            output_tokens: value
                .get("usage")
                .and_then(|usage| {
                    usage
                        .get("completion_tokens")
                        .or_else(|| usage.get("output_tokens"))
                })
                .and_then(|tokens| tokens.as_u64()),
            finish_reason: value
                .get("stop_reason")
                .or_else(|| {
                    value
                        .get("choices")
                        .and_then(|choices| choices.get(0))
                        .and_then(|choice| choice.get("finish_reason"))
                })
                .and_then(|reason| reason.as_str())
                .map(str::to_string),
        })
    }

    /// Parse a model list, tolerating each protocol's envelope.
    fn parse_models_response(&self, response: HttpResponse) -> Result<Vec<ProviderModel>> {
        if !response.is_success() {
            return Err(AiError::from_status(response.status, &response.text())
                .with_provider(self.config.id.clone(), String::new())
                .into());
        }

        let value: serde_json::Value = response.json().map_err(|_| {
            AiError::new(
                AiErrorKind::MalformedResponse,
                format!("{} returned a model list that is not JSON", self.config.name),
            )
            .with_provider(self.config.id.clone(), String::new())
        })?;

        // OpenAI-compatible uses `data`, Ollama uses `models` with a `name` field.
        let entries = value
            .get("data")
            .and_then(|data| data.as_array())
            .or_else(|| value.get("models").and_then(|models| models.as_array()))
            .cloned()
            .unwrap_or_default();

        let mut models = Vec::new();
        for entry in entries {
            let model_id = entry
                .get("id")
                .or_else(|| entry.get("name"))
                .or_else(|| entry.get("model"))
                .and_then(|id| id.as_str())
                .map(str::to_string);
            let Some(model_id) = model_id else { continue };
            let context_window = entry
                .get("context_window")
                .or_else(|| entry.get("context_length"))
                .and_then(|value| value.as_u64());
            models.push(ProviderModel {
                display_name: None,
                model_id,
                source: ModelSource::Fetched,
                context_window,
            });
        }

        models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
        models.dedup_by(|a, b| a.model_id == b.model_id);
        Ok(models)
    }

    /// A connection test (`handoff.md` §11.5): the real configuration is exercised and both outcomes
    /// are reported with a reason.
    pub fn test_connection(&self, client: &dyn HttpClient) -> ConnectionTest {
        if let Err(problem) = self.config.validate() {
            return ConnectionTest {
                ok: false,
                models: 0,
                message: problem,
            };
        }

        match self.list_models(client) {
            Ok(models) => ConnectionTest {
                ok: true,
                models: models.len(),
                message: format!(
                    "{} responded with {} model{}",
                    self.config.name,
                    models.len(),
                    if models.len() == 1 { "" } else { "s" }
                ),
            },
            Err(error) => ConnectionTest {
                ok: false,
                models: 0,
                // The error is already redacted, so it is safe to show.
                message: error.to_string(),
            },
        }
    }

    /// Merge a fetched model list with the user's manual entries (`handoff.md` §11.2).
    ///
    /// A manual entry is preserved even when the refresh does not return it: the user added it on
    /// purpose, and dropping it would silently break a feature assignment that uses it.
    pub fn merge_models(
        &self,
        existing: &[ProviderModel],
        fetched: &[ProviderModel],
    ) -> Vec<ProviderModel> {
        let mut merged: BTreeMap<String, ProviderModel> = BTreeMap::new();
        for model in fetched {
            merged.insert(model.model_id.clone(), model.clone());
        }
        for model in existing {
            if model.source == ModelSource::Manual {
                merged
                    .entry(model.model_id.clone())
                    .or_insert_with(|| model.clone());
            }
        }
        merged.into_values().collect()
    }
}

/// The result of a connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionTest {
    pub ok: bool,
    pub models: usize,
    /// A success summary or a failure reason. Never contains a credential.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::http::{FakeHttpClient, HttpBody};

    fn provider(protocol: ProviderProtocol) -> Provider {
        Provider::new(
            ProviderConfig::new("p1", "Test Provider", protocol),
            Some("secret-value".to_string()),
        )
    }

    #[test]
    fn the_three_documented_protocols_are_supported() {
        // handoff.md section 11.1.
        assert_eq!(
            ProviderProtocol::parse("openai"),
            Some(ProviderProtocol::OpenAiCompatible)
        );
        assert_eq!(
            ProviderProtocol::parse("openai-compatible"),
            Some(ProviderProtocol::OpenAiCompatible)
        );
        assert_eq!(
            ProviderProtocol::parse("Anthropic"),
            Some(ProviderProtocol::Anthropic)
        );
        assert_eq!(ProviderProtocol::parse("ollama"), Some(ProviderProtocol::Ollama));
        assert_eq!(ProviderProtocol::parse("gemini"), None);
    }

    #[test]
    fn a_base_url_must_be_an_absolute_http_url() {
        let mut config = ProviderConfig::new("p", "P", ProviderProtocol::OpenAiCompatible);
        assert!(config.validate().is_ok());

        config.base_url = "api.example.com".into();
        assert!(config.validate().unwrap_err().contains("http://"));

        config.base_url = "  ".into();
        assert!(config.validate().unwrap_err().contains("empty"));
    }

    #[test]
    fn a_custom_base_url_is_used_verbatim() {
        // handoff.md section 11.1: the Base URL must be fully user-configurable.
        let mut p = provider(ProviderProtocol::OpenAiCompatible);
        p.config.base_url = "https://gateway.internal/v1/".into();
        let request = p
            .build_chat_request(&ChatRequest::new("m", vec![ChatMessage::user("hi")]))
            .unwrap();
        assert_eq!(request.url, "https://gateway.internal/v1/chat/completions");
    }

    #[test]
    fn openai_compatible_uses_bearer_auth_and_its_own_path() {
        let p = provider(ProviderProtocol::OpenAiCompatible);
        let request = p
            .build_chat_request(&ChatRequest::new("gpt-x", vec![ChatMessage::user("hi")]))
            .unwrap();

        assert!(request.url.ends_with("/chat/completions"), "{}", request.url);
        assert!(request
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer secret-value"));
        match &request.body {
            HttpBody::Json(value) => {
                assert_eq!(value["model"], "gpt-x");
                assert_eq!(value["messages"][0]["role"], "user");
            }
            other => panic!("expected a JSON body, got {other:?}"),
        }
    }

    #[test]
    fn anthropic_uses_its_own_header_and_moves_the_system_prompt_out_of_messages() {
        // Getting this wrong is a 400 from the provider, so it is asserted rather than assumed.
        let p = provider(ProviderProtocol::Anthropic);
        let request = p
            .build_chat_request(&ChatRequest::new(
                "claude-x",
                vec![
                    ChatMessage::system("be terse"),
                    ChatMessage::user("summarise this"),
                ],
            ))
            .unwrap();

        assert!(request.url.ends_with("/v1/messages"), "{}", request.url);
        assert!(request
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "secret-value"));
        assert!(request
            .headers
            .iter()
            .any(|(k, _)| k == "anthropic-version"));
        assert!(
            !request.headers.iter().any(|(k, _)| k == "Authorization"),
            "Anthropic does not take a bearer token"
        );

        match &request.body {
            HttpBody::Json(value) => {
                assert_eq!(value["system"], "be terse");
                assert_eq!(value["messages"].as_array().unwrap().len(), 1);
                assert_eq!(value["messages"][0]["role"], "user");
                // Anthropic requires max_tokens, so a default is supplied.
                assert!(value["max_tokens"].as_u64().unwrap() > 0);
            }
            other => panic!("expected a JSON body, got {other:?}"),
        }
    }

    #[test]
    fn anthropic_without_a_system_prompt_omits_the_field() {
        let p = provider(ProviderProtocol::Anthropic);
        let request = p
            .build_chat_request(&ChatRequest::new("c", vec![ChatMessage::user("hi")]))
            .unwrap();
        match &request.body {
            HttpBody::Json(value) => assert!(value.get("system").is_none()),
            other => panic!("expected a JSON body, got {other:?}"),
        }
    }

    #[test]
    fn ollama_uses_its_own_path_and_sends_no_credential() {
        let p = provider(ProviderProtocol::Ollama);
        let request = p
            .build_chat_request(&ChatRequest::new("llama3", vec![ChatMessage::user("hi")]))
            .unwrap();

        assert!(request.url.ends_with("/api/chat"), "{}", request.url);
        // A local daemon has no credential, and sending one would be leaking it to a local process.
        assert!(!request
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("authorization") || k == "x-api-key"));
    }

    #[test]
    fn a_provider_without_a_credential_sends_no_auth_header() {
        let p = Provider::new(
            ProviderConfig::new("p", "P", ProviderProtocol::OpenAiCompatible),
            None,
        );
        let request = p
            .build_chat_request(&ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap();
        assert!(!request.headers.iter().any(|(k, _)| k == "Authorization"));
    }

    #[test]
    fn a_chat_response_is_parsed_for_every_protocol() {
        let openai = FakeHttpClient::new().on_json(
            "/chat/completions",
            serde_json::json!({
                "model": "gpt-x",
                "choices": [{ "message": { "content": "hello" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 3, "completion_tokens": 2 }
            }),
        );
        let response = provider(ProviderProtocol::OpenAiCompatible)
            .chat(&openai, &ChatRequest::new("gpt-x", vec![ChatMessage::user("hi")]))
            .unwrap();
        assert_eq!(response.text, "hello");
        assert_eq!(response.input_tokens, Some(3));
        assert_eq!(response.output_tokens, Some(2));
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));

        let anthropic = FakeHttpClient::new().on_json(
            "/v1/messages",
            serde_json::json!({
                "content": [{ "type": "text", "text": "hello " }, { "type": "text", "text": "there" }],
                "stop_reason": "end_turn"
            }),
        );
        let response = provider(ProviderProtocol::Anthropic)
            .chat(&anthropic, &ChatRequest::new("c", vec![ChatMessage::user("hi")]))
            .unwrap();
        assert_eq!(response.text, "hello there", "multiple content blocks are concatenated");

        let ollama = FakeHttpClient::new().on_json(
            "/api/chat",
            serde_json::json!({ "message": { "content": "local hello" } }),
        );
        let response = provider(ProviderProtocol::Ollama)
            .chat(&ollama, &ChatRequest::new("llama3", vec![ChatMessage::user("hi")]))
            .unwrap();
        assert_eq!(response.text, "local hello");
    }

    #[test]
    fn a_provider_error_status_becomes_a_classified_failure() {
        let client = FakeHttpClient::new().always(429, "rate limited");
        let error = provider(ProviderProtocol::OpenAiCompatible)
            .chat(&client, &ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap_err();
        assert_eq!(error.code(), "AI_PROVIDER");
        assert!(error.to_string().contains("429"), "{error}");

        let client = FakeHttpClient::new().always(500, "upstream boom");
        let error = provider(ProviderProtocol::OpenAiCompatible)
            .chat(&client, &ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap_err();
        assert!(error.to_string().contains("SERVER_ERROR"), "{error}");
    }

    #[test]
    fn an_unparseable_body_is_reported_as_a_malformed_response() {
        let client = FakeHttpClient::new().always(200, "this is not json");
        let error = provider(ProviderProtocol::OpenAiCompatible)
            .chat(&client, &ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap_err();
        assert!(error.to_string().contains("not JSON"), "{error}");
    }

    #[test]
    fn a_well_formed_body_with_no_text_is_a_malformed_response() {
        let client = FakeHttpClient::new().on_json("/chat/completions", serde_json::json!({ "choices": [] }));
        let error = provider(ProviderProtocol::OpenAiCompatible)
            .chat(&client, &ChatRequest::new("m", vec![ChatMessage::user("x")]))
            .unwrap_err();
        assert!(error.to_string().contains("no text content"), "{error}");
    }

    #[test]
    fn model_lists_are_parsed_from_both_envelopes() {
        let openai = FakeHttpClient::new().on_json(
            "/models",
            serde_json::json!({ "data": [{ "id": "b" }, { "id": "a" }, { "id": "a" }] }),
        );
        let models = provider(ProviderProtocol::OpenAiCompatible)
            .list_models(&openai)
            .unwrap();
        assert_eq!(
            models.iter().map(|m| m.model_id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"],
            "sorted and de-duplicated"
        );
        assert!(models.iter().all(|m| m.source == ModelSource::Fetched));

        let ollama = FakeHttpClient::new().on_json(
            "/api/tags",
            serde_json::json!({ "models": [{ "name": "llama3" }, { "name": "qwen" }] }),
        );
        let models = provider(ProviderProtocol::Ollama).list_models(&ollama).unwrap();
        assert_eq!(models.len(), 2);
    }

    #[test]
    fn a_manual_model_survives_a_refresh_that_does_not_return_it() {
        // handoff.md section 11.2: the user added it on purpose, and dropping it would silently
        // break whichever feature is assigned to it.
        let p = provider(ProviderProtocol::OpenAiCompatible);
        let existing = vec![ProviderModel {
            model_id: "my-private-model".to_string(),
            display_name: None,
            source: ModelSource::Manual,
            context_window: None,
        }];
        let fetched = vec![ProviderModel {
            model_id: "gpt-x".to_string(),
            display_name: None,
            source: ModelSource::Fetched,
            context_window: None,
        }];

        let merged = p.merge_models(&existing, &fetched);
        let ids: Vec<&str> = merged.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(ids, vec!["gpt-x", "my-private-model"]);
    }

    #[test]
    fn a_fetched_model_is_updated_by_a_later_refresh() {
        let p = provider(ProviderProtocol::OpenAiCompatible);
        let existing = vec![ProviderModel {
            model_id: "gpt-x".to_string(),
            display_name: None,
            source: ModelSource::Fetched,
            context_window: Some(1000),
        }];
        let fetched = vec![ProviderModel {
            model_id: "gpt-x".to_string(),
            display_name: Some("GPT X".to_string()),
            source: ModelSource::Fetched,
            context_window: Some(2000),
        }];

        let merged = p.merge_models(&existing, &fetched);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].context_window, Some(2000));
    }

    #[test]
    fn a_connection_test_reports_success_with_a_model_count() {
        // handoff.md section 11.5.
        let client = FakeHttpClient::new().on_json(
            "/models",
            serde_json::json!({ "data": [{ "id": "a" }, { "id": "b" }] }),
        );
        let result = provider(ProviderProtocol::OpenAiCompatible).test_connection(&client);
        assert!(result.ok);
        assert_eq!(result.models, 2);
        assert!(result.message.contains("2 models"), "{}", result.message);
    }

    #[test]
    fn a_connection_test_reports_the_failure_reason_without_a_credential() {
        let client = FakeHttpClient::new().always(401, "invalid key sk-ant-api03-abcdefghijklmnopqrstuvwxyz");
        let result = provider(ProviderProtocol::OpenAiCompatible).test_connection(&client);
        assert!(!result.ok);
        assert!(result.message.contains("401"), "{}", result.message);
        // The provider echoed the key back; it must not reach the UI or a log.
        assert!(
            !result.message.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz"),
            "{}",
            result.message
        );
    }

    #[test]
    fn a_connection_test_reports_a_configuration_problem_without_a_request() {
        let mut p = provider(ProviderProtocol::OpenAiCompatible);
        p.config.base_url = "not-a-url".into();
        let client = FakeHttpClient::new();
        let result = p.test_connection(&client);
        assert!(!result.ok);
        assert_eq!(client.request_count(), 0, "no request should be attempted");
    }

    #[test]
    fn the_provider_config_carries_only_a_credential_handle() {
        // handoff.md section 12: this struct is what reaches SQLite, a log and a release snapshot.
        let config = ProviderConfig {
            credential_ref: Some("keychain:ai/openai".to_string()),
            ..ProviderConfig::new("p", "P", ProviderProtocol::OpenAiCompatible)
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("keychain:ai/openai"));
        assert!(!json.contains("sk-"), "no key material may be present: {json}");
        assert!(config.has_credential());
    }

    #[test]
    fn a_model_reference_must_have_both_parts() {
        assert!(ModelRef::new("p", "m").is_usable());
        assert!(!ModelRef::new("p", "  ").is_usable());
        assert!(!ModelRef::new("", "m").is_usable());
    }

    #[test]
    fn the_anthropic_model_list_request_carries_its_version_header() {
        let p = provider(ProviderProtocol::Anthropic);
        let request = p.build_models_request();
        assert!(request.url.ends_with("/v1/models"));
        assert!(request.headers.iter().any(|(k, _)| k == "anthropic-version"));
    }
}
