//! Secret handling.
//!
//! Three locked rules drive this module:
//!
//! * `handoff.md` §12 — credentials never enter SQLite, plain config, logs or backup archives.
//! * `handoff.md` §17.3 — a backup must be preceded by a secret scan over the working tree
//!   *and* git objects, and a positive finding must block a silent cloud upload.
//! * `handoff.md` §18.1 — logs must be redacted.
//!
//! Everything that writes text destined for a log, a snapshot, a manifest or an archive is
//! expected to route that text through [`redact`] (or [`redact_json`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Marker substituted for any value recognised as sensitive.
pub const REDACTION: &str = "[REDACTED]";

/// A string wrapper that never renders or serialises its contents.
///
/// Used for API keys, tokens and secrets that must transit the domain layer on their way to
/// the operating system credential store, without ever being logged or persisted.
#[derive(Clone, PartialEq, Eq)]
pub struct Redacted(String);

impl Redacted {
    /// Wrap an already-resolved secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the real value. Call sites should be limited to the credential-store boundary
    /// and outbound HTTP authorization headers.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Length, allowed for diagnostics that must prove a value exists without revealing it.
    pub fn len(&self) -> usize {
        self.0.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Redacted(<secret>)")
    }
}

impl std::fmt::Display for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(REDACTION)
    }
}

impl Serialize for Redacted {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(REDACTION)
    }
}

impl<'de> Deserialize<'de> for Redacted {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        // Deserialising a redacted placeholder means the real value is gone; callers must
        // re-resolve it from the credential store instead (handoff §17.10).
        let raw = String::deserialize(d)?;
        if raw == REDACTION {
            return Err(serde::de::Error::custom(
                "refusing to deserialize a redacted placeholder as a secret",
            ));
        }
        Ok(Self(raw))
    }
}

impl From<String> for Redacted {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Redacted {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// Pattern-based scanning
// ---------------------------------------------------------------------------------------------

/// What kind of credential a finding looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    PrivateKeyBlock,
    AwsAccessKeyId,
    GithubToken,
    GithubPat,
    OpenAiKey,
    AnthropicKey,
    GoogleApiKey,
    SlackToken,
    Jwt,
    BearerToken,
    GenericAssignment,
}

impl SecretKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::PrivateKeyBlock => "private key block",
            Self::AwsAccessKeyId => "AWS access key id",
            Self::GithubToken => "GitHub token",
            Self::GithubPat => "GitHub personal access token",
            Self::OpenAiKey => "OpenAI API key",
            Self::AnthropicKey => "Anthropic API key",
            Self::GoogleApiKey => "Google API key",
            Self::SlackToken => "Slack token",
            Self::Jwt => "JWT",
            Self::BearerToken => "bearer token",
            Self::GenericAssignment => "credential-looking assignment",
        }
    }
}

/// A single suspicious location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretFinding {
    pub kind: SecretKind,
    /// 1-based line number when the subject is text; `None` for whole-file findings.
    pub line: Option<usize>,
    /// The offending line with the secret portion replaced by [`REDACTION`].
    pub redacted_excerpt: String,
    /// Where the finding came from: a project-relative path, or `git:<object-id>`.
    pub location: String,
    /// True when the match is very likely a genuine credential rather than a placeholder.
    pub high_confidence: bool,
}

/// Aggregate result of a scan, consumed by the backup gate (`handoff.md` §17.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretScanReport {
    pub findings: Vec<SecretFinding>,
    pub files_scanned: usize,
    pub bytes_scanned: u64,
}

impl SecretScanReport {
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn high_confidence_count(&self) -> usize {
        self.findings.iter().filter(|f| f.high_confidence).count()
    }

    /// The finding kinds present, de-duplicated, for a compact UI summary.
    pub fn kinds(&self) -> Vec<SecretKind> {
        let set: BTreeSet<SecretKind> = self.findings.iter().map(|f| f.kind).collect();
        set.into_iter().collect()
    }

    pub fn merge(&mut self, other: SecretScanReport) {
        self.findings.extend(other.findings);
        self.files_scanned += other.files_scanned;
        self.bytes_scanned += other.bytes_scanned;
    }
}

struct Pattern {
    kind: SecretKind,
    regex: regex::Regex,
    high_confidence: bool,
}

fn patterns() -> &'static [Pattern] {
    static PATTERNS: OnceLock<Vec<Pattern>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let build = |kind: SecretKind, pattern: &str, high_confidence: bool| Pattern {
            kind,
            regex: regex::Regex::new(pattern).expect("static secret pattern must compile"),
            high_confidence,
        };
        vec![
            build(
                SecretKind::PrivateKeyBlock,
                r"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----",
                true,
            ),
            build(SecretKind::AwsAccessKeyId, r"\b(?:AKIA|ASIA)[0-9A-Z]{16}\b", true),
            build(SecretKind::GithubToken, r"\bgh[pousr]_[A-Za-z0-9]{36,255}\b", true),
            build(SecretKind::GithubPat, r"\bgithub_pat_[A-Za-z0-9_]{22,255}\b", true),
            build(SecretKind::OpenAiKey, r"\bsk-[A-Za-z0-9_\-]{20,}\b", true),
            build(SecretKind::AnthropicKey, r"\bsk-ant-[A-Za-z0-9_\-]{20,}\b", true),
            build(SecretKind::GoogleApiKey, r"\bAIza[0-9A-Za-z_\-]{35}\b", true),
            build(SecretKind::SlackToken, r"\bxox[abprs]-[A-Za-z0-9\-]{10,}\b", true),
            build(
                SecretKind::Jwt,
                r"\beyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}\b",
                true,
            ),
            build(SecretKind::BearerToken, r"(?i)\bBearer\s+[A-Za-z0-9._\-]{16,}", true),
            build(
                SecretKind::GenericAssignment,
                r#"(?i)\b(?:api[_-]?key|apikey|secret|secret[_-]?key|access[_-]?token|auth[_-]?token|password|passwd|client[_-]?secret|s3[_-]?secret)\b\s*[:=]\s*["']?([A-Za-z0-9/+_\-\.]{16,})["']?"#,
                false,
            ),
        ]
    })
}

/// Values that look like credentials but are obviously placeholders or documentation.
///
/// Note that "example" is deliberately *not* in this list: real reportable tokens can contain
/// that substring, whereas the remaining markers are unambiguous placeholders.
fn is_placeholder(value: &str) -> bool {
    let lowered = value.to_ascii_lowercase();
    const PLACEHOLDERS: [&str; 14] = [
        "redacted",
        "your_",
        "your-",
        "placeholder",
        "changeme",
        "change_me",
        "xxxxx",
        "aaaaa",
        "dummy",
        "test_key",
        "test-key",
        "todo",
        "<",
        "${",
    ];
    PLACEHOLDERS.iter().any(|p| lowered.contains(p)) || lowered.starts_with("${")
}

/// Replace the sensitive span of a matched line with [`REDACTION`].
fn redact_match(line: &str, matched: &str) -> String {
    // Keep a short non-sensitive prefix of the match so the user can recognise which
    // credential was found without the value being usable.
    let keep = if matched.len() > 8 { 4 } else { 0 };
    let visible: String = matched.chars().take(keep).collect();
    line.replace(matched, &format!("{visible}{REDACTION}"))
}

/// Scan a block of text for credential-looking content.
///
/// `location` is recorded verbatim on each finding (a project-relative path, or a git object
/// reference). The scan never mutates its input and never returns a full secret value.
pub fn scan_text(text: &str, location: &str) -> SecretScanReport {
    let mut report = SecretScanReport {
        files_scanned: 1,
        bytes_scanned: text.len() as u64,
        ..Default::default()
    };

    for (index, line) in text.lines().enumerate() {
        for pattern in patterns() {
            let Some(captures) = pattern.regex.captures(line) else {
                continue;
            };
            let whole = captures
                .get(0)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            if whole.is_empty() {
                continue;
            }
            // `GenericAssignment` captures the value separately so the placeholder filter can
            // look at the value rather than the whole `key = value` expression.
            let candidate = captures
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or(whole.as_str());
            if is_placeholder(candidate) {
                continue;
            }
            report.findings.push(SecretFinding {
                kind: pattern.kind,
                line: Some(index + 1),
                redacted_excerpt: redact_match(line.trim(), &whole),
                location: location.to_string(),
                high_confidence: pattern.high_confidence,
            });
        }
    }

    report
}

/// Scan one file on disk.
pub fn scan_path(path: &Path, location: &str) -> Result<SecretScanReport> {
    let bytes = std::fs::read(path).map_err(|e| crate::VeloctError::io(path, e))?;
    // Binary files are not line-scannable in a useful way; skip them but still count them.
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Ok(SecretScanReport {
            files_scanned: 1,
            bytes_scanned: bytes.len() as u64,
            ..Default::default()
        });
    }
    let text = String::from_utf8_lossy(&bytes);
    Ok(scan_text(&text, location))
}

/// Convenience wrapper matching the naming used in the architecture notes.
pub fn scan_for_secrets(text: &str, location: &str) -> SecretScanReport {
    scan_text(text, location)
}

/// Redact known secret values and credential patterns out of a log line.
///
/// Two layers, because either alone is insufficient:
/// 1. exact-value substitution for every secret the caller currently holds;
/// 2. pattern substitution for credentials the caller has never seen (for example a token
///    echoed back by a provider in an error body — `handoff.md` §18.1).
pub fn redact_with_known(message: &str, known_values: &[&str]) -> String {
    let mut out = message.to_string();
    for value in known_values {
        if value.len() >= 8 && out.contains(value) {
            out = out.replace(value, REDACTION);
        }
    }
    for pattern in patterns() {
        out = pattern
            .regex
            .replace_all(&out, |caps: &regex::Captures<'_>| {
                let whole = caps.get(0).map(|m| m.as_str()).unwrap_or_default();
                let candidate = caps.get(1).map(|m| m.as_str()).unwrap_or(whole);
                if is_placeholder(candidate) {
                    whole.to_string()
                } else {
                    redact_match(whole, whole)
                }
            })
            .into_owned();
    }
    out
}

/// Redact a log line using the pattern layer only.
pub fn redact(message: &str) -> String {
    redact_with_known(message, &[])
}

/// Redact every string value in a JSON document in place.
///
/// Used before any JSON artefact is written to disk (release snapshots, deployment records,
/// diagnostics) so that a stray secret cannot be persisted even by mistake.
pub fn redact_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => {
            let redacted = redact(s);
            if redacted != *s {
                *s = redacted;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(redact_json),
        serde_json::Value::Object(map) => {
            let sensitive_keys: Vec<String> = map
                .keys()
                .filter(|k| {
                    let lowered = k.to_ascii_lowercase();
                    lowered.contains("secret")
                        || lowered.contains("password")
                        || lowered.contains("passwd")
                        || lowered.contains("token")
                        || lowered == "apikey"
                        || lowered == "api_key"
                        || lowered.contains("credential")
                        || lowered.contains("access_key")
                        || lowered.contains("private_key")
                        || lowered == "authorization"
                })
                .cloned()
                .collect();
            for key in sensitive_keys {
                if let Some(slot) = map.get_mut(&key) {
                    // A `credential_ref` is an opaque handle, not a secret value; keep it so
                    // historical releases can be re-bound (handoff §14.6, §17.10).
                    let keep = key.to_ascii_lowercase().ends_with("_ref")
                        && slot.as_str().map(|s| !looks_like_secret(s)).unwrap_or(true);
                    if !keep {
                        *slot = serde_json::Value::String(REDACTION.to_string());
                    }
                }
            }
            for (_, v) in map.iter_mut() {
                redact_json(v);
            }
        }
        _ => {}
    }
}

/// Heuristic used to distinguish an opaque credential handle from an actual secret value.
fn looks_like_secret(value: &str) -> bool {
    if value.len() < 20 {
        return false;
    }
    patterns().iter().any(|p| p.regex.is_match(value))
}

/// Frequently used operation name so call sites read the same in tests and in the UI.
pub type ScanLocation = PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    /// A syntactically valid AWS access key id (20 chars: `AKIA` + 16 uppercase alphanumerics).
    const AWS_KEY: &str = "AKIAIOSFODNN7ABCDEFG";
    const GITHUB_TOKEN: &str = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";

    #[test]
    fn redacted_never_exposes_its_value() {
        let secret = Redacted::new("sk-ant-api03-REALSECRETVALUE123456");
        assert_eq!(format!("{secret}"), REDACTION);
        assert_eq!(format!("{secret:?}"), "Redacted(<secret>)");
        assert_eq!(serde_json::to_string(&secret).unwrap(), format!("\"{REDACTION}\""));
        assert!(secret.expose().starts_with("sk-ant-"));
    }

    #[test]
    fn redacted_refuses_to_deserialize_a_placeholder() {
        let json = format!("\"{REDACTION}\"");
        assert!(serde_json::from_str::<Redacted>(&json).is_err());
    }

    #[test]
    fn pattern_layer_catches_provider_keys() {
        // handoff §25 C8: secrets must never reach the log.
        let line = "calling provider with Authorization: Bearer sk-ant-api03-abcdefghijklmnopqrstuvwxyz";
        let redacted = redact(line);
        assert!(!redacted.contains("sk-ant-api03-abcdefghijklmnopqrstuvwxyz"), "{redacted}");
        assert!(redacted.contains(REDACTION));
    }

    #[test]
    fn known_value_layer_catches_arbitrary_secrets() {
        let known = "hunter2-but-long-enough-to-matter";
        let line = format!("deploy token is {known} ok");
        let redacted = redact_with_known(&line, &[known]);
        assert!(!redacted.contains(known));
        assert!(redacted.contains(REDACTION));
    }

    #[test]
    fn placeholder_values_are_not_reported() {
        let text = "api_key = \"your_api_key_here\"\nsecret: changeme000000000000";
        let report = scan_text(text, "config.json");
        assert!(report.is_clean(), "false positives: {:?}", report.findings);
    }

    #[test]
    fn scan_reports_line_numbers_without_leaking_values() {
        let text = format!("line one\naws_key = {AWS_KEY}\nline three");
        let report = scan_text(&text, "content/articles/a.md");
        assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
        let finding = &report.findings[0];
        assert_eq!(finding.kind, SecretKind::AwsAccessKeyId);
        assert_eq!(finding.line, Some(2));
        assert!(finding.high_confidence);
        assert!(!finding.redacted_excerpt.contains(AWS_KEY));
    }

    #[test]
    fn private_key_blocks_and_jwts_are_detected() {
        let text = "-----BEGIN OPENSSH PRIVATE KEY-----\n";
        assert_eq!(scan_text(text, "id_rsa").findings[0].kind, SecretKind::PrivateKeyBlock);

        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(scan_text(jwt, "token.txt").findings[0].kind, SecretKind::Jwt);
    }

    #[test]
    fn redact_json_keeps_credential_refs_but_removes_values() {
        // handoff §14.6: the snapshot stores credential_ref only.
        let mut doc = serde_json::json!({
            "credential_ref": "keychain:cloudflare/prod",
            "api_key": "sk-abcdefghijklmnopqrstuvwxyz012345",
            "nested": { "github": { "token": "ghp_abcdefghijklmnopqrstuvwxyz0123456789" } }
        });
        redact_json(&mut doc);
        assert_eq!(doc["credential_ref"], "keychain:cloudflare/prod");
        assert_eq!(doc["api_key"], REDACTION);
        assert_eq!(doc["nested"]["github"]["token"], REDACTION);
    }

    #[test]
    fn high_confidence_count_separates_likely_credentials_from_keywords() {
        let text = format!("password = someLongValueHere123456\n{AWS_KEY}");
        let report = scan_text(&text, "x.env");
        assert_eq!(report.findings.len(), 2, "{:?}", report.findings);
        assert_eq!(report.high_confidence_count(), 1);
    }

    #[test]
    fn merge_accumulates_counts() {
        let mut a = scan_text(AWS_KEY, "a");
        let b = scan_text(GITHUB_TOKEN, "b");
        a.merge(b);
        assert_eq!(a.findings.len(), 2);
        assert_eq!(a.files_scanned, 2);
        assert_eq!(a.kinds().len(), 2);
        assert!(!a.is_clean());
    }
}
