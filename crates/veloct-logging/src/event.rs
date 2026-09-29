//! The event itself: its level, its category, and the redaction that makes it safe to write.
//!
//! The redaction is the whole point of this module. `handoff.md` §18.1 lists what must never reach the
//! log, and the only way to be sure of that is for the TYPE to be incapable of holding an unredacted
//! value: [`LogEvent`]'s fields are private, its constructors redact, and there is no setter. A caller
//! who wants a value in the log passes it to a constructor and gets whatever the scanner leaves.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use veloct_core::secret::{redact, scan_text};

/// The three levels `handoff.md` §18 names, and their spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Info,
    Warning,
    Error,
}

impl Level {
    /// The bracketed form the log line uses: `[info]`, `[warning]`, `[error]`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    /// The bracketed prefix, which is what a reader greps for.
    pub fn bracket(self) -> String {
        format!("[{}]", self.as_str())
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().trim_matches(|c| c == '[' || c == ']') {
            "info" => Some(Self::Info),
            "warning" | "warn" => Some(Self::Warning),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    /// Whether this level is worth showing in a default-filtered view.
    pub fn is_problem(self) -> bool {
        matches!(self, Self::Warning | Self::Error)
    }
}

/// Which subsystem an event came from.
///
/// `handoff.md` §18 names the list: operations, saves, publishing, builds, deployment, backup, AI and
/// attachments. It is a closed enum rather than a free string so the log page can group by it and a
/// typo cannot create a phantom category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Application start, project open, settings.
    App,
    /// Creating, editing, renaming and deleting content.
    Content,
    /// Autosave and manual save.
    Save,
    /// Publishing: the change check, the commit, the tag.
    Publish,
    /// The Astro build.
    Build,
    /// Deployment targets and sub-targets.
    Deploy,
    /// Backup, export and restore.
    Backup,
    /// AI requests, including the fallback.
    Ai,
    /// The attachment cache pool and the storage targets.
    Attachment,
    /// The preview process lifecycle.
    Preview,
    /// The Git repository.
    Git,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Content => "content",
            Self::Save => "save",
            Self::Publish => "publish",
            Self::Build => "build",
            Self::Deploy => "deploy",
            Self::Backup => "backup",
            Self::Ai => "ai",
            Self::Attachment => "attachment",
            Self::Preview => "preview",
            Self::Git => "git",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "app" => Some(Self::App),
            "content" => Some(Self::Content),
            "save" => Some(Self::Save),
            "publish" => Some(Self::Publish),
            "build" => Some(Self::Build),
            "deploy" => Some(Self::Deploy),
            "backup" => Some(Self::Backup),
            "ai" => Some(Self::Ai),
            "attachment" => Some(Self::Attachment),
            "preview" => Some(Self::Preview),
            "git" => Some(Self::Git),
            _ => None,
        }
    }

    /// Every category, for a filter control.
    pub fn all() -> [Self; 11] {
        [
            Self::App,
            Self::Content,
            Self::Save,
            Self::Publish,
            Self::Build,
            Self::Deploy,
            Self::Backup,
            Self::Ai,
            Self::Attachment,
            Self::Preview,
            Self::Git,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::App => "应用",
            Self::Content => "内容",
            Self::Save => "保存",
            Self::Publish => "发布",
            Self::Build => "构建",
            Self::Deploy => "部署",
            Self::Backup => "备份",
            Self::Ai => "AI",
            Self::Attachment => "附件",
            Self::Preview => "预览",
            Self::Git => "Git",
        }
    }
}

/// A redacted event, ready to be written.
///
/// Fields are private and there is no setter, so an event that exists has already been through the
/// scanner. That is the enforcement, not the convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEvent {
    level: Level,
    category: Category,
    message: String,
    fields: BTreeMap<String, String>,
    /// True when the scanner changed something, so a reader can tell that a value was redacted rather
    /// than being empty.
    redacted: bool,
}

impl LogEvent {
    /// Build an event, redacting the message.
    pub fn new(level: Level, category: Category, message: impl Into<String>) -> Self {
        let raw: String = message.into();
        let redacted = !scan_text(&raw, "log").is_clean();
        Self {
            level,
            category,
            message: redact(&raw),
            fields: BTreeMap::new(),
            redacted,
        }
    }

    pub fn info(category: Category, message: impl Into<String>) -> Self {
        Self::new(Level::Info, category, message)
    }

    pub fn warning(category: Category, message: impl Into<String>) -> Self {
        Self::new(Level::Warning, category, message)
    }

    pub fn error(category: Category, message: impl Into<String>) -> Self {
        Self::new(Level::Error, category, message)
    }

    /// Attach a field, redacting its value.
    ///
    /// A field whose NAME looks like a credential is replaced entirely rather than scanned: a value
    /// under a key called `token` is a credential whatever it looks like, and a short opaque token can
    /// easily match no pattern.
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let key = key.into();
        let raw: String = value.into();
        if is_sensitive_key(&key) {
            self.fields.insert(key, "[REDACTED]".to_string());
            self.redacted = true;
            return self;
        }
        if !scan_text(&raw, "log").is_clean() {
            self.redacted = true;
        }
        self.fields.insert(key, redact(&raw));
        self
    }

    /// Attach a field only when it has a value.
    pub fn with_optional_field(self, key: impl Into<String>, value: Option<impl Into<String>>) -> Self {
        match value {
            Some(value) => self.with_field(key, value),
            None => self,
        }
    }

    /// Describe an HTTP request WITHOUT its body.
    ///
    /// `handoff.md` §18.1 forbids a complete sensitive request body in the log, so the body's SIZE is
    /// recorded and its CONTENT is not: there is no parameter here that could carry one.
    pub fn http_request(
        level: Level,
        category: Category,
        method: &str,
        url: &str,
        header_names: &[String],
        body_bytes: usize,
    ) -> Self {
        let mut event = Self::new(level, category, format!("{method} {url}"));
        event = event.with_field("method", method);
        // The URL goes through the scanner, because a query string can carry a token.
        event = event.with_field("url", veloct_core::http::redact_url(url));
        // Only the NAMES. A header VALUE is exactly what §18.1 forbids.
        event = event.with_field("headers", header_names.join(","));
        event = event.with_field("body_bytes", body_bytes.to_string());
        event
    }

    /// Describe an HTTP response by status and size, never by body.
    pub fn http_response(
        level: Level,
        category: Category,
        status: u16,
        body_bytes: usize,
    ) -> Self {
        Self::new(level, category, format!("HTTP {status}"))
            .with_field("status", status.to_string())
            .with_field("body_bytes", body_bytes.to_string())
    }

    pub fn level(&self) -> Level {
        self.level
    }

    pub fn category(&self) -> Category {
        self.category
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn fields(&self) -> &BTreeMap<String, String> {
        &self.fields
    }

    /// Whether the scanner changed anything.
    pub fn was_redacted(&self) -> bool {
        self.redacted
    }

    /// One line for the file and for the log page.
    ///
    /// The format is `TIMESTAMP [level] category: message | key=value ...`. Chosen over JSON because
    /// the log is read by a person first, and because a value containing a newline cannot then break a
    /// record: the message and every value are flattened on construction.
    pub fn line(&self, timestamp: &str) -> String {
        let mut out = format!(
            "{timestamp} {} {}: {}",
            self.level.bracket(),
            self.category.as_str(),
            flatten(&self.message)
        );
        if !self.fields.is_empty() {
            out.push_str(" | ");
            let pairs: Vec<String> = self
                .fields
                .iter()
                .map(|(key, value)| format!("{key}={}", flatten(value)))
                .collect();
            out.push_str(&pairs.join(" "));
        }
        out
    }
}

/// Whether a field name means "this value is a credential".
///
/// A name-based rule catches the short opaque tokens a pattern cannot: a 20-character hex API key
/// looks like any other string, but nobody puts it under a key called `plain`.
pub fn is_sensitive_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    const MARKERS: &[&str] = &[
        "secret",
        "token",
        "password",
        "passwd",
        "apikey",
        "api_key",
        "access_key",
        "secret_key",
        "authorization",
        "auth",
        "credential",
        "private_key",
        "cookie",
        "session",
        "bearer",
    ];
    MARKERS.iter().any(|marker| lowered.contains(marker))
}

/// Collapse every newline and control character, so one event is one line.
fn flatten(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A parsed log line, for the log page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub timestamp: String,
    pub level: Level,
    pub category: Category,
    pub message: String,
    pub fields: BTreeMap<String, String>,
}

impl Entry {
    /// Parse a line written by [`LogEvent::line`].
    ///
    /// Returns `None` for a line that does not match, rather than guessing: a log with a
    /// half-interpreted entry is worse than a log with an obviously unparsed one, and the log page
    /// shows those verbatim.
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            return None;
        }

        // TIMESTAMP [level] category: message | fields
        let mut rest = line;
        let timestamp_end = rest.find(' ')?;
        let timestamp = rest[..timestamp_end].to_string();
        rest = rest[timestamp_end + 1..].trim_start();

        let close = rest.find(']')?;
        if !rest.starts_with('[') {
            return None;
        }
        let level = Level::parse(&rest[1..close])?;
        rest = rest[close + 1..].trim_start();

        let colon = rest.find(':')?;
        let category = Category::parse(&rest[..colon])?;
        rest = rest[colon + 1..].trim_start();

        let (message, field_text) = match rest.find(" | ") {
            Some(index) => (&rest[..index], Some(&rest[index + 3..])),
            None => (rest, None),
        };

        let mut fields = BTreeMap::new();
        if let Some(field_text) = field_text {
            for pair in field_text.split(' ') {
                if let Some((key, value)) = pair.split_once('=') {
                    if !key.is_empty() {
                        fields.insert(key.to_string(), value.to_string());
                    }
                }
            }
        }

        Some(Self {
            timestamp,
            level,
            category,
            message: message.to_string(),
            fields,
        })
    }

    /// Whether this entry mentions a term, for a search box.
    pub fn matches(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let needle = needle.to_ascii_lowercase();
        self.message.to_ascii_lowercase().contains(&needle)
            || self.category.as_str().contains(&needle)
            || self
                .fields
                .iter()
                .any(|(key, value)| {
                    key.to_ascii_lowercase().contains(&needle)
                        || value.to_ascii_lowercase().contains(&needle)
                })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANTHROPIC_KEY: &str = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";
    const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";

    #[test]
    fn the_three_levels_use_the_documented_bracketed_spelling() {
        // handoff.md section 18: [info] / [warning] / [error].
        assert_eq!(Level::Info.bracket(), "[info]");
        assert_eq!(Level::Warning.bracket(), "[warning]");
        assert_eq!(Level::Error.bracket(), "[error]");
        for level in [Level::Info, Level::Warning, Level::Error] {
            assert_eq!(Level::parse(level.as_str()), Some(level));
            assert_eq!(Level::parse(&level.bracket()), Some(level));
        }
        assert_eq!(Level::parse("warn"), Some(Level::Warning));
        assert_eq!(Level::parse("trace"), None);
    }

    #[test]
    fn every_category_the_contract_names_exists() {
        // handoff.md section 18: operations, saves, publishing, builds, deployment, backup, AI,
        // attachments.
        for category in [
            Category::Content,
            Category::Save,
            Category::Publish,
            Category::Build,
            Category::Deploy,
            Category::Backup,
            Category::Ai,
            Category::Attachment,
        ] {
            assert!(Category::all().contains(&category));
        }
        for category in Category::all() {
            assert_eq!(Category::parse(category.as_str()), Some(category));
            assert!(!category.label().is_empty());
        }
        assert_eq!(Category::parse("nonsense"), None);
    }

    #[test]
    fn a_secret_in_a_message_is_redacted_before_the_event_exists() {
        // handoff.md section 18.1. There is no constructor that skips this.
        let event = LogEvent::info(Category::Ai, format!("using key {ANTHROPIC_KEY}"));
        assert!(!event.message().contains(ANTHROPIC_KEY), "{}", event.message());
        assert!(event.message().contains("REDACTED"), "{}", event.message());
        assert!(event.was_redacted(), "the event says it was redacted");
    }

    #[test]
    fn a_clean_message_is_not_flagged_as_redacted() {
        let event = LogEvent::info(Category::Save, "saved content/articles/a.md");
        assert!(!event.was_redacted());
        assert_eq!(event.message(), "saved content/articles/a.md");
    }

    #[test]
    fn a_secret_in_a_field_value_is_redacted() {
        let event = LogEvent::info(Category::Ai, "response")
            .with_field("note", format!("the key was {AWS_KEY}"));
        assert!(!event.fields()["note"].contains(AWS_KEY));
        assert!(event.was_redacted());
    }

    #[test]
    fn a_field_whose_name_means_credential_is_replaced_whole() {
        // A name-based rule catches a short opaque token that no pattern would match.
        for key in [
            "token",
            "api_key",
            "apiKey",
            "Authorization",
            "secret",
            "password",
            "access_key",
            "credential_ref_value",
            "session",
        ] {
            let event = LogEvent::info(Category::Ai, "request").with_field(key, "shortopaque");
            assert_eq!(
                event.fields()[key],
                "[REDACTED]",
                "the value under {key} must not be logged"
            );
            assert!(event.was_redacted());
        }
    }

    #[test]
    fn a_harmless_field_name_keeps_a_harmless_value() {
        let event = LogEvent::info(Category::Deploy, "pushed")
            .with_field("target", "cloudflare")
            .with_field("files", "12");
        assert_eq!(event.fields()["target"], "cloudflare");
        assert_eq!(event.fields()["files"], "12");
        assert!(!event.was_redacted());
    }

    #[test]
    fn an_optional_field_is_omitted_when_absent() {
        let with_none = LogEvent::info(Category::App, "x").with_optional_field("note", None::<String>);
        assert!(with_none.fields().is_empty());

        let with_some =
            LogEvent::info(Category::App, "x").with_optional_field("note", Some("present"));
        assert_eq!(with_some.fields()["note"], "present");
    }

    #[test]
    fn an_http_request_is_logged_by_shape_and_never_by_body() {
        // handoff.md section 18.1 names "complete sensitive request bodies" specifically, so there is
        // no parameter that could carry one.
        let event = LogEvent::http_request(
            Level::Info,
            Category::Ai,
            "POST",
            "https://api.example.com/v1/messages",
            &["content-type".to_string(), "authorization".to_string()],
            2048,
        );

        assert!(event.message().contains("POST"), "{}", event.message());
        assert_eq!(event.fields()["method"], "POST");
        // The header NAMES, not their values.
        assert!(event.fields()["headers"].contains("authorization"));
        assert!(!event.fields()["headers"].contains("Bearer"));
        // The size, not the content.
        assert_eq!(event.fields()["body_bytes"], "2048");
    }

    #[test]
    fn an_http_request_url_with_a_token_in_the_query_is_redacted() {
        let event = LogEvent::http_request(
            Level::Info,
            Category::Ai,
            "GET",
            "https://api.example.com/v1/models?key=sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789",
            &[],
            0,
        );
        assert!(
            !event.fields()["url"].contains(ANTHROPIC_KEY),
            "{}",
            event.fields()["url"]
        );
    }

    #[test]
    fn an_http_response_is_logged_by_status_and_size() {
        let event = LogEvent::http_response(Level::Warning, Category::Deploy, 503, 512);
        assert_eq!(event.fields()["status"], "503");
        assert_eq!(event.fields()["body_bytes"], "512");
        assert!(event.message().contains("503"));
    }

    #[test]
    fn a_line_has_the_documented_shape() {
        let event = LogEvent::error(Category::Build, "the build failed")
            .with_field("target", "site");
        let line = event.line("2026-01-01T12:00:00Z");
        assert!(line.starts_with("2026-01-01T12:00:00Z [error] build: the build failed"), "{line}");
        assert!(line.contains("| target=site"), "{line}");
    }

    #[test]
    fn a_message_with_a_newline_cannot_break_a_record() {
        // One event must be one line, or a parser sees a record that was never written.
        let event = LogEvent::info(Category::Content, "line one\nline two\r\n[error] forged: entry");
        let line = event.line("2026-01-01T12:00:00Z");
        assert!(!line.contains('\n'), "{line}");
        assert!(!line.contains('\r'), "{line}");
        // And the forged level cannot be mistaken for a real one, because the real one comes first.
        let parsed = Entry::parse(&line).unwrap();
        assert_eq!(parsed.level, Level::Info);
    }

    #[test]
    fn a_line_round_trips_through_parse() {
        let event = LogEvent::warning(Category::Attachment, "upload failed")
            .with_field("target", "s3-main")
            .with_field("files", "1");
        let line = event.line("2026-01-01T12:00:00Z");

        let entry = Entry::parse(&line).unwrap();
        assert_eq!(entry.timestamp, "2026-01-01T12:00:00Z");
        assert_eq!(entry.level, Level::Warning);
        assert_eq!(entry.category, Category::Attachment);
        assert_eq!(entry.message, "upload failed");
        assert_eq!(entry.fields["target"], "s3-main");
    }

    #[test]
    fn a_line_with_no_fields_round_trips() {
        let line = LogEvent::info(Category::App, "started").line("2026-01-01T00:00:00Z");
        let entry = Entry::parse(&line).unwrap();
        assert!(entry.fields.is_empty());
        assert_eq!(entry.message, "started");
    }

    #[test]
    fn a_line_that_does_not_match_is_reported_as_unparsed_rather_than_guessed() {
        // A half-interpreted entry is worse than an obviously unparsed one.
        for bad in [
            "",
            "   ",
            "no timestamp here",
            "2026-01-01T00:00:00Z not-a-level app: x",
            "2026-01-01T00:00:00Z [info] not-a-category: x",
            "2026-01-01T00:00:00Z [info] app no colon",
        ] {
            assert!(Entry::parse(bad).is_none(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn an_entry_can_be_searched_by_message_category_or_field() {
        let entry = Entry::parse(
            &LogEvent::error(Category::Deploy, "the works sub-target failed")
                .with_field("target", "edgeone-cn")
                .line("2026-01-01T00:00:00Z"),
        )
        .unwrap();

        assert!(entry.matches(""));
        assert!(entry.matches("works"));
        assert!(entry.matches("deploy"));
        assert!(entry.matches("edgeone"));
        assert!(entry.matches("TARGET"));
        assert!(!entry.matches("cloudflare"));
    }

    #[test]
    fn is_sensitive_key_is_case_insensitive() {
        assert!(is_sensitive_key("Authorization"));
        assert!(is_sensitive_key("API_KEY"));
        assert!(is_sensitive_key("X-Api-Token"));
        assert!(!is_sensitive_key("target"));
        assert!(!is_sensitive_key("files"));
        assert!(!is_sensitive_key("duration_ms"));
    }

    #[test]
    fn a_field_value_with_a_secret_is_redacted_even_under_a_harmless_name() {
        // The name rule catches what a pattern cannot; the pattern rule catches what a name does not.
        let event = LogEvent::info(Category::Ai, "response")
            .with_field("excerpt", format!("key {ANTHROPIC_KEY} rejected"));
        assert!(!event.fields()["excerpt"].contains(ANTHROPIC_KEY));
        assert!(event.was_redacted());
    }

    #[test]
    fn every_level_reports_whether_it_is_a_problem() {
        assert!(!Level::Info.is_problem());
        assert!(Level::Warning.is_problem());
        assert!(Level::Error.is_problem());
    }
}
