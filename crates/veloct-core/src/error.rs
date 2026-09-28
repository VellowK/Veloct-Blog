//! Error taxonomy.
//!
//! `handoff.md` forbids silent failure and false success claims, so the error type is
//! intentionally explicit: each variant names a condition the UI is expected to explain.

use std::path::PathBuf;

/// Shorthand for a `VeloctError` result.
pub type Result<T, E = VeloctError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum VeloctError {
    #[error("not configured: {what}")]
    NotConfigured {
        what: String,
        hint: Option<String>,
    },

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("project not found at {0}")]
    ProjectNotFound(PathBuf),

    #[error("project lock conflict: {0}")]
    LockConflict(String),

    #[error("another exclusive operation is running: {holder}")]
    ExclusiveBusy { holder: String },

    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("database error: {0}")]
    Database(String),

    #[error("migration failed: {0}")]
    Migration(String),

    #[error("markdown parse error in {path}: {message}")]
    MarkdownParse { path: PathBuf, message: String },

    #[error("frontmatter error in {path}: {message}")]
    Frontmatter { path: PathBuf, message: String },

    #[error("git error: {0}")]
    Git(String),

    #[error("unresolved merge conflicts in: {0}")]
    MergeConflict(String),

    #[error("external modification detected for {path}")]
    ExternalModification { path: PathBuf },

    #[error("reference error: {0}")]
    Reference(String),

    #[error("build failed: {0}")]
    Build(String),

    #[error("deployment failed for {target}/{sub_target}: {message}")]
    Deploy {
        target: String,
        sub_target: String,
        message: String,
    },

    #[error("preview process error: {0}")]
    Preview(String),

    // The classification code is part of the message on purpose. It is the difference between a
    // timeout and a bad model name, and a caller that only sees the Display form needs it to decide
    // what to do next.
    #[error("AI provider error {code} ({provider}): {message}")]
    AiProvider {
        provider: String,
        code: String,
        message: String,
        retryable: bool,
        /// The HTTP status, when the provider returned one. Carried as a field rather than parsed
        /// back out of the message, so a UI can show "429" without string matching.
        status: Option<u16>,
    },

    #[error("AI request cancelled by the user")]
    Cancelled,

    #[error("attachment error: {0}")]
    Attachment(String),

    #[error("secret scan blocked the operation: {0}")]
    SecretBlocked(String),

    #[error("backup error: {0}")]
    Backup(String),

    #[error("restore error: {0}")]
    Restore(String),

    #[error("os credential store error: {0}")]
    Credential(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("not implemented yet: {0}")]
    NotImplemented(String),

    #[error("serialization error: {0}")]
    Serde(String),

    #[error("{0}")]
    Other(String),
}

impl VeloctError {
    /// Build a `NotConfigured` error with an operator-facing hint.
    pub fn not_configured(what: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::NotConfigured {
            what: what.into(),
            hint: Some(hint.into()),
        }
    }

    /// Wrap a `std::io::Error` with the path it happened at.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// A stable machine-readable code, used for AI request metadata, diagnostics and logs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured { .. } => "NOT_CONFIGURED",
            Self::Invalid(_) => "INVALID_INPUT",
            Self::ProjectNotFound(_) => "PROJECT_NOT_FOUND",
            Self::LockConflict(_) => "LOCK_CONFLICT",
            Self::ExclusiveBusy { .. } => "EXCLUSIVE_BUSY",
            Self::Io { .. } => "IO",
            Self::Database(_) => "DATABASE",
            Self::Migration(_) => "MIGRATION",
            Self::MarkdownParse { .. } => "MARKDOWN_PARSE",
            Self::Frontmatter { .. } => "FRONTMATTER",
            Self::Git(_) => "GIT",
            Self::MergeConflict(_) => "MERGE_CONFLICT",
            Self::ExternalModification { .. } => "EXTERNAL_MODIFICATION",
            Self::Reference(_) => "REFERENCE",
            Self::Build(_) => "BUILD",
            Self::Deploy { .. } => "DEPLOY",
            Self::Preview(_) => "PREVIEW",
            Self::AiProvider { .. } => "AI_PROVIDER",
            Self::Cancelled => "CANCELLED",
            Self::Attachment(_) => "ATTACHMENT",
            Self::SecretBlocked(_) => "SECRET_BLOCKED",
            Self::Backup(_) => "BACKUP",
            Self::Restore(_) => "RESTORE",
            Self::Credential(_) => "CREDENTIAL",
            Self::Unsupported(_) => "UNSUPPORTED",
            Self::NotImplemented(_) => "NOT_IMPLEMENTED",
            Self::Serde(_) => "SERDE",
            Self::Other(_) => "OTHER",
        }
    }

    /// Whether this error should be explained to the user as an actionable state versus a bug.
    pub fn is_actionable(&self) -> bool {
        matches!(
            self,
            Self::NotConfigured { .. }
                | Self::ProjectNotFound(_)
                | Self::LockConflict(_)
                | Self::ExclusiveBusy { .. }
                | Self::MergeConflict(_)
                | Self::ExternalModification { .. }
                | Self::SecretBlocked(_)
                | Self::Cancelled
        )
    }
}

impl From<serde_json::Error> for VeloctError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value.to_string())
    }
}

impl From<time::error::Format> for VeloctError {
    fn from(value: time::error::Format) -> Self {
        Self::Other(format!("time format error: {value}"))
    }
}

impl From<time::error::Parse> for VeloctError {
    fn from(value: time::error::Parse) -> Self {
        Self::Other(format!("time parse error: {value}"))
    }
}

/// Serialise errors for Tauri command boundaries.
impl serde::Serialize for VeloctError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("VeloctError", 4)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.serialize_field("actionable", &self.is_actionable())?;
        st.serialize_field(
            "hint",
            &match self {
                Self::NotConfigured { hint, .. } => hint.clone(),
                _ => None,
            },
        )?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_is_actionable_and_not_a_failure_code() {
        // handoff §11.4: user cancel is CANCELLED, never an error that triggers fallback.
        let err = VeloctError::Cancelled;
        assert_eq!(err.code(), "CANCELLED");
        assert!(err.is_actionable());
    }

    #[test]
    fn error_serializes_with_a_stable_code() {
        let err = VeloctError::not_configured("works origin", "set a Works Origin in the target");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "NOT_CONFIGURED");
        assert_eq!(json["actionable"], true);
        assert!(json["hint"].as_str().unwrap().contains("Works Origin"));
    }

    #[test]
    fn io_error_keeps_the_path() {
        let err = VeloctError::io(
            PathBuf::from("content/articles/a.md"),
            std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
        );
        assert!(err.to_string().contains("content/articles/a.md"));
    }
}
