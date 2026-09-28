//! The optional AI subsystem.
//!
//! AI is never a dependency of editing, building, publishing or backup (`handoff.md` §11): with no
//! provider configured, every other feature must still work.
//!
//! The load-bearing rules, in the order they are easiest to get wrong:
//!
//! * **A user-initiated stop is `CANCELLED` and is NOT an error.** It must never trigger the
//!   fallback model (`handoff.md` §11.4, §25 C1). Every *other* failure — connection, timeout, 4xx,
//!   5xx, missing model, context overflow, provider refusal — does.
//! * **There is exactly one global fallback model**, possibly on a completely independent provider,
//!   and a fallback failure is terminal. It never re-enters the fallback loop (`handoff.md` §11.4,
//!   §25 C5).
//! * **No output rewrites Markdown before the user confirms it**, and a partial apply touches only
//!   the selected range (`handoff.md` §11.9, §25 C6/C7).
//! * **Credentials never reach SQLite, a config file, a log or a backup.** They resolve through the
//!   OS credential store, and only an opaque handle is recorded.
//!
//! The HTTP layer sits behind `veloct_core::http::HttpClient`, so every provider is tested against a
//! fake rather than a live vendor.

#![forbid(unsafe_code)]

pub mod apply;
pub mod cancellation;
pub mod context;
pub mod error;
pub mod feature;
pub mod provider;
pub mod service;

pub use apply::{ApplyMode, ApplyOutcome, ApplyPlan, Selection};
pub use cancellation::CancelToken;
pub use context::{ContextBundle, ContextLimits, ContextSource};
pub use error::{AiError, AiErrorKind};
pub use feature::{builtin_features, feature_spec, FeatureId, FeatureSpec};
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, ConnectionTest, ModelRef, ModelSource, Provider,
    ProviderConfig, ProviderModel, ProviderProtocol,
};
pub use service::{AiCall, AiOutcome, AiService, FallbackConfig, ProviderRegistry};
