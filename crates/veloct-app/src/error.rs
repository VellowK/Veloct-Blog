//! Frontend-facing error shape.
//!
//! `handoff.md` forbids silent failure, so a command error always carries a stable machine code
//! and a message the UI can show, plus a flag saying whether it is a user-actionable state
//! (not configured, locked, conflict) rather than a defect.

use serde::{Deserialize, Serialize};

use veloct_core::VeloctError;

/// The serialised form every failing Tauri command returns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandError {
    /// Stable machine-readable code, e.g. `EXCLUSIVE_BUSY`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// True when the user can resolve it (configure something, wait, respond to a conflict).
    pub actionable: bool,
    /// Optional remediation hint.
    pub hint: Option<String>,
}

impl From<VeloctError> for CommandError {
    fn from(error: VeloctError) -> Self {
        let hint = match &error {
            VeloctError::NotConfigured { hint, .. } => hint.clone(),
            _ => None,
        };
        Self {
            code: error.code().to_string(),
            message: error.to_string(),
            actionable: error.is_actionable(),
            hint,
        }
    }
}

impl CommandError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            actionable: false,
            hint: None,
        }
    }
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// Shorthand used by every command wrapper.
pub type CommandResult<T> = std::result::Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_configured_errors_keep_their_hint_and_are_actionable() {
        let error = VeloctError::not_configured("Works Origin", "set one on the target");
        let command: CommandError = error.into();
        assert_eq!(command.code, "NOT_CONFIGURED");
        assert!(command.actionable);
        assert_eq!(command.hint.as_deref(), Some("set one on the target"));
    }
}
