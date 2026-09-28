//! Cancellation.
//!
//! `handoff.md` §11.6 requires "停止生成" to work, and §11.4 requires that stopping is `CANCELLED`
//! rather than an error. The core HTTP seam already has a [`veloct_core::http::Cancellation`] handle;
//! this type is the AI-level wrapper that makes the intent explicit at every call site, so a
//! provider loop cannot accidentally treat a stop as a retryable failure.

use std::sync::Arc;

use veloct_core::Cancellation;

use crate::error::AiError;

/// A shareable token the UI flips to stop generation.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<Cancellation>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the in-flight request to stop. Safe to call more than once.
    pub fn cancel(&self) {
        self.0.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }

    /// Turn cancellation into the AI error kind, or `Ok(())` when the request may continue.
    ///
    /// Callers use this at every await point so a stop cannot be mistaken for a provider failure.
    pub fn check(&self) -> Result<(), AiError> {
        if self.is_cancelled() {
            Err(AiError::cancelled())
        } else {
            Ok(())
        }
    }

    /// The handle the HTTP seam needs.
    pub fn as_http(&self) -> &Cancellation {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_token_is_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
        assert!(token.check().is_ok());
    }

    #[test]
    fn cancelling_produces_the_cancellation_error_not_a_failure() {
        let token = CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());

        let error = token.check().unwrap_err();
        assert!(error.is_cancelled());
        assert!(!error.should_fall_back(true), "a stop must never fall back");
    }

    #[test]
    fn cancelling_twice_is_harmless() {
        let token = CancelToken::new();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn a_clone_shares_the_same_state() {
        // The UI holds one clone and the request loop another; they must observe the same flag.
        let token = CancelToken::new();
        let observer = token.clone();
        assert!(!observer.is_cancelled());
        token.cancel();
        assert!(observer.is_cancelled());
    }

    #[test]
    fn the_http_handle_reports_the_same_state() {
        let token = CancelToken::new();
        let http = token.as_http().clone();
        assert!(!http.is_cancelled());
        token.cancel();
        assert!(http.is_cancelled());
    }
}
