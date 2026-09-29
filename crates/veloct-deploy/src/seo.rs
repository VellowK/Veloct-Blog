//! The SEO push lifecycle.
//!
//! `handoff.md` §15.2 is short and every clause is a constraint on how this must NOT behave:
//!
//! > An SEO push is an independent task AFTER publishing, not part of generating the static site.
//! > An SEO push failure does not make the whole site release a failure. It is logged. The user may
//! > re-run it manually. Offline may be marked pending, and there is no requirement to retry endlessly
//! > in the background.
//!
//! So the shape here is: a run is a LIST of per-engine records, every engine can end in `pushed`,
//! `failed` or `pending`, and the run's verdict is derived rather than asserted. The important line is
//! [`SeoPushRun::affects_site_release`], which returns `false` unconditionally — a caller cannot make an
//! SEO failure fail a release without deleting that function.
//!
//! Two things are deliberately absent:
//!
//! * **No retry loop.** §15.2 rejects "retrying endlessly in the background", so a failed or pending push
//!   waits for the user. There is no timer here and nowhere to put one.
//! * **No invented endpoint for an unverified engine.** §15.2's mechanism is not an HTTP API for most
//!   engines: Baidu and Bing both accept a plain URL submission, and the ones that need an authenticated
//!   API (IndexNow needs a key file on the origin, Google needs OAuth) are reported as needing the user
//!   rather than called with a guessed request. `handoff.md`'s own principle is that a wrong push is worse
//!   than a documented manual step, and a silently failing guess would look like a success.

use serde::{Deserialize, Serialize};

use veloct_core::{Clock, Result, VeloctError};

/// The engines §15 mentions by name plus the one that is universally supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeoEngine {
    /// Bing, and every engine that consumes the IndexNow protocol (Yandex, Seznam, Naver).
    IndexNow,
    Baidu,
    Google,
}

impl SeoEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IndexNow => "indexnow",
            Self::Baidu => "baidu",
            Self::Google => "google",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "indexnow" | "bing" => Some(Self::IndexNow),
            "baidu" => Some(Self::Baidu),
            "google" => Some(Self::Google),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::IndexNow => "IndexNow(Bing 等)",
            Self::Baidu => "百度",
            Self::Google => "Google",
        }
    }

    /// Why an engine cannot be pushed automatically, when that is the case.
    ///
    /// Reported as a reason rather than attempted: a guessed request would produce a confusing failure,
    /// and a silently swallowed one would look like a success.
    pub fn manual_reason(self) -> Option<&'static str> {
        match self {
            // IndexNow needs a key file served from the site's own origin, so the key is the user's.
            Self::IndexNow => Some(
                "IndexNow 需要在站点根目录提供一个密钥文件,因此密钥由你在设置中提供;未提供时跳过。",
            ),
            Self::Baidu => Some("百度需要账号内的推送密钥,未配置时不会发送请求。"),
            // Google's Indexing API is OAuth-scoped and only officially supports job-posting and
            // livestream markup; sitemap submission is the supported route.
            Self::Google => Some(
                "Google 没有面向普通页面的公开提交接口,请通过 sitemap 与 Search Console 提交。",
            ),
        }
    }

    /// Every engine, in a stable order.
    pub fn all() -> [Self; 3] {
        [Self::IndexNow, Self::Baidu, Self::Google]
    }
}

/// One engine's outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SeoEngineOutcome {
    /// Submitted and accepted.
    Pushed { detail: String },
    /// Attempted and refused. The site release is unaffected.
    Failed { reason: String },
    /// Not attempted: offline, an engine needing the user, or no key configured. §15.2 allows this
    /// instead of an unbounded background retry.
    Pending { reason: String },
    /// Not requested for this run.
    Skipped { reason: String },
}

impl SeoEngineOutcome {
    pub fn status(&self) -> &'static str {
        match self {
            Self::Pushed { .. } => "pushed",
            Self::Failed { .. } => "failed",
            Self::Pending { .. } => "pending",
            Self::Skipped { .. } => "skipped",
        }
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Pushed { detail } => Some(detail),
            Self::Failed { reason } | Self::Pending { reason } | Self::Skipped { reason } => {
                Some(reason)
            }
        }
    }

    /// Whether the user should be offered a manual re-run.
    ///
    /// §15.2 says a failure may be re-run manually, and a PENDING push is exactly the state the user has
    /// to resolve, so both offer it. A skip does not: it was not requested.
    pub fn is_retryable_by_user(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Pending { .. })
    }
}

/// One engine's record in a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeoEngineRecord {
    pub engine: SeoEngine,
    pub outcome: SeoEngineOutcome,
    pub started_at: String,
    pub finished_at: Option<String>,
    /// True when this record came from the user asking again rather than from the publish.
    pub manual_retry: bool,
}

impl SeoEngineRecord {
    pub fn new(engine: SeoEngine, outcome: SeoEngineOutcome, now: &str, manual_retry: bool) -> Self {
        Self {
            engine,
            outcome,
            started_at: now.to_string(),
            finished_at: Some(now.to_string()),
            manual_retry,
        }
    }
}

/// A whole SEO push for one release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeoPushRun {
    pub release_id: String,
    pub site_url: String,
    pub records: Vec<SeoEngineRecord>,
    pub created_at: String,
}

impl SeoPushRun {
    pub fn new(release_id: impl Into<String>, site_url: impl Into<String>, created_at: &str) -> Self {
        Self {
            release_id: release_id.into(),
            site_url: site_url.into(),
            records: Vec::new(),
            created_at: created_at.to_string(),
        }
    }

    /// The record for one engine.
    pub fn record(&self, engine: SeoEngine) -> Option<&SeoEngineRecord> {
        self.records.iter().find(|record| record.engine == engine)
    }

    /// `handoff.md` §15.2, as a function.
    ///
    /// Always `false`. It exists so that the rule is stated in one place with a name, and so a caller
    /// cannot implement the opposite without deleting it.
    pub fn affects_site_release(&self) -> bool {
        false
    }

    /// Whether any engine failed.
    pub fn has_failure(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.outcome.status() == "failed")
    }

    /// Whether any engine is still waiting on the user or the network.
    pub fn has_pending(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.outcome.status() == "pending")
    }

    /// The engines that were accepted.
    pub fn pushed_engines(&self) -> Vec<SeoEngine> {
        self.records
            .iter()
            .filter(|record| record.outcome.status() == "pushed")
            .map(|record| record.engine)
            .collect()
    }

    /// The engines the user can ask for again.
    pub fn retryable(&self) -> Vec<SeoEngine> {
        self.records
            .iter()
            .filter(|record| record.outcome.is_retryable_by_user())
            .map(|record| record.engine)
            .collect()
    }

    /// A one-line summary for the UI: the site published, the SEO push did or did not.
    ///
    /// §15.2's own illustration is `站点发布 ✅ / SEO 推送 ⚠️`, and this produces that shape so the two
    /// statuses are never conflated by a caller that only reads one field.
    pub fn summary(&self) -> String {
        let site = "站点发布 ✅";
        let engines: Vec<String> = self
            .records
            .iter()
            .map(|record| format!("{} {}", record.engine.label(), symbol(record.outcome.status())))
            .collect();
        if engines.is_empty() {
            return format!("{site} · SEO 推送 未请求");
        }
        format!("{site} · SEO 推送 {}", engines.join(" "))
    }
}

fn symbol(status: &str) -> &'static str {
    match status {
        "pushed" => "✅",
        "failed" => "❌",
        "pending" => "⚠️",
        _ => "—",
    }
}

/// Submits one URL to one engine.
///
/// A trait rather than a concrete client for the same reason `AiService` takes an injected HTTP client:
/// the lifecycle — offline, failure, retry — is what §15.2 constrains, and it is testable with a stub.
pub trait SeoSubmitter: Send + Sync {
    /// Submit `url`. `Ok(None)` means the engine cannot be pushed here and the caller should mark it
    /// pending; `Ok(Some(detail))` means accepted.
    fn submit(&self, engine: SeoEngine, url: &str) -> Result<Option<String>>;
}

/// The submitter that does not guess.
///
/// Every engine reports its reason for needing the user rather than issuing a request that would fail
/// confusingly. Used when no engine credentials are configured, which is the state of a fresh project.
pub struct ConfigurationRequiredSubmitter;

impl SeoSubmitter for ConfigurationRequiredSubmitter {
    fn submit(&self, engine: SeoEngine, _url: &str) -> Result<Option<String>> {
        Err(VeloctError::not_configured(
            engine.label(),
            engine
                .manual_reason()
                .unwrap_or("this engine needs additional configuration"),
        ))
    }
}

/// A submitter that records what it was asked to do, for the tests.
#[derive(Debug, Default)]
pub struct RecordingSubmitter {
    calls: std::sync::Mutex<Vec<(SeoEngine, String)>>,
    /// Engines this stub accepts. Anything else is "needs the user" and becomes pending.
    accepts: Vec<SeoEngine>,
    /// When set, every submission of these engines fails.
    fails: Vec<SeoEngine>,
}

impl RecordingSubmitter {
    pub fn accepting(accepts: Vec<SeoEngine>) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            accepts,
            fails: Vec::new(),
        }
    }

    pub fn failing(fails: Vec<SeoEngine>) -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
            accepts: Vec::new(),
            fails,
        }
    }

    pub fn calls(&self) -> Vec<(SeoEngine, String)> {
        self.calls.lock().map(|calls| calls.clone()).unwrap_or_default()
    }
}

impl SeoSubmitter for RecordingSubmitter {
    fn submit(&self, engine: SeoEngine, url: &str) -> Result<Option<String>> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((engine, url.to_string()));
        }
        if self.fails.contains(&engine) {
            return Err(VeloctError::Deploy {
                target: engine.as_str().to_string(),
                // An SEO push is not a site deployment, so there is no sub-target; naming the main one
                // would imply the two share a fate, which section 15.2 forbids.
                sub_target: "seo".to_string(),
                message: "the engine refused the submission".to_string(),
            });
        }
        if self.accepts.contains(&engine) {
            return Ok(Some("accepted".to_string()));
        }
        Ok(None)
    }
}

/// What the pusher needs to know about the environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeoPushRequest {
    pub release_id: String,
    /// The published site's canonical URL. A push with no URL has nothing to submit.
    pub site_url: String,
    /// Whether the machine is online. §15.2 allows an offline run to be marked pending rather than
    /// retried in the background.
    pub online: bool,
    /// Whether this run came from the user asking again.
    pub manual_retry: bool,
}

impl SeoPushRequest {
    pub fn new(release_id: impl Into<String>, site_url: impl Into<String>) -> Self {
        Self {
            release_id: release_id.into(),
            site_url: site_url.into(),
            online: true,
            manual_retry: false,
        }
    }

    pub fn offline(mut self) -> Self {
        self.online = false;
        self
    }

    pub fn as_manual_retry(mut self) -> Self {
        self.manual_retry = true;
        self
    }

    /// Whether the request can be attempted at all.
    pub fn is_submittable(&self) -> bool {
        !self.site_url.trim().is_empty() && self.site_url.starts_with("http")
    }
}

/// Run one SEO push.
///
/// Returns a run even when nothing could be submitted, because §15.2 wants the failure RECORDED rather
/// than raised: a returned `Err` here would tempt a caller into treating the publish as failed.
pub fn push(
    request: &SeoPushRequest,
    submitter: &dyn SeoSubmitter,
    clock: &dyn Clock,
) -> SeoPushRun {
    let now = veloct_core::clock::format_rfc3339(clock.now());
    let mut run = SeoPushRun::new(&request.release_id, &request.site_url, &now);

    // A site with no URL cannot be submitted, and saying so is more useful than three failures.
    if !request.is_submittable() {
        for engine in SeoEngine::all() {
            run.records.push(SeoEngineRecord::new(
                engine,
                SeoEngineOutcome::Pending {
                    reason: "站点还没有可提交的公开地址,先在设置中填写站点 URL。".to_string(),
                },
                &now,
                request.manual_retry,
            ));
        }
        return run;
    }

    // Offline: every engine is pending, and nothing is attempted. §15.2's "offline may be marked pending".
    if !request.online {
        for engine in SeoEngine::all() {
            run.records.push(SeoEngineRecord::new(
                engine,
                SeoEngineOutcome::Pending {
                    reason: "当前离线,推送已标记为待处理;联网后可以手动重新执行。".to_string(),
                },
                &now,
                request.manual_retry,
            ));
        }
        return run;
    }

    for engine in SeoEngine::all() {
        let outcome = match submitter.submit(engine, &request.site_url) {
            Ok(Some(detail)) => SeoEngineOutcome::Pushed { detail },
            // `None` means the engine cannot be pushed from here, which is PENDING rather than FAILED:
            // nothing went wrong, the user has something to do.
            Ok(None) => SeoEngineOutcome::Pending {
                reason: engine
                    .manual_reason()
                    .unwrap_or("该引擎需要额外配置,已跳过自动推送。")
                    .to_string(),
            },
            Err(error) => match error {
                // A missing configuration is pending, not failed: §15.2 distinguishes "could not try"
                // from "tried and was refused", and only the latter is a failure to report as one.
                VeloctError::NotConfigured { .. } => SeoEngineOutcome::Pending {
                    reason: error.to_string(),
                },
                VeloctError::Cancelled => SeoEngineOutcome::Pending {
                    reason: "推送被取消,可以手动重新执行。".to_string(),
                },
                other => SeoEngineOutcome::Failed {
                    reason: other.to_string(),
                },
            },
        };

        run.records.push(SeoEngineRecord::new(
            engine,
            outcome,
            &now,
            request.manual_retry,
        ));
    }

    run
}

/// The rows a run produces for the `seo_pushes` table.
///
/// A separate function so the mapping from the run to the schema is asserted rather than implied: the
/// table exists and had no writer, which is how a lifecycle goes missing.
pub fn rows_for(run: &SeoPushRun) -> Vec<SeoPushRow> {
    run.records
        .iter()
        .map(|record| SeoPushRow {
            id: format!("{}:{}", run.release_id, record.engine.as_str()),
            release_id: run.release_id.clone(),
            engine: record.engine.as_str().to_string(),
            status: record.outcome.status().to_string(),
            error: match &record.outcome {
                SeoEngineOutcome::Pushed { .. } => None,
                other => other.reason().map(str::to_string),
            },
            created_at: Some(record.started_at.clone()),
            finished_at: record.finished_at.clone(),
            manual_retry: record.manual_retry,
        })
        .collect()
}

/// One row of `seo_pushes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeoPushRow {
    pub id: String,
    pub release_id: String,
    pub engine: String,
    pub status: String,
    pub error: Option<String>,
    pub created_at: Option<String>,
    pub finished_at: Option<String>,
    pub manual_retry: bool,
}

/// Persist a run's rows through a connection, replacing any previous rows for the release.
///
/// A replace rather than an append for the reason the table's own primary key implies: the id is
/// `release:engine`, so a re-run updates that engine's row rather than accumulating a history nobody
/// reads. The manual-retry flag is what records that the user asked again.
pub fn record_rows(connection: &rusqlite::Connection, run: &SeoPushRun) -> Result<()> {
    for row in rows_for(run) {
        connection
            .execute(
                "INSERT INTO seo_pushes
                   (id, release_id, engine, status, error, created_at, finished_at, manual_retry)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(id) DO UPDATE SET
                   status = excluded.status,
                   error = excluded.error,
                   finished_at = excluded.finished_at,
                   manual_retry = excluded.manual_retry",
                rusqlite::params![
                    row.id,
                    row.release_id,
                    row.engine,
                    row.status,
                    row.error,
                    row.created_at,
                    row.finished_at,
                    if row.manual_retry { 1 } else { 0 },
                ],
            )
            .map_err(|error| {
                VeloctError::Database(format!("could not record the SEO push: {error}"))
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use veloct_core::FixedClock;

    fn clock() -> FixedClock {
        FixedClock::new(
            time::OffsetDateTime::parse(
                "2026-01-01T12:00:00Z",
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap(),
        )
    }

    fn request() -> SeoPushRequest {
        SeoPushRequest::new("rel-1", "https://example.com")
    }

    #[test]
    fn the_three_engines_section_15_names_round_trip() {
        for engine in SeoEngine::all() {
            assert_eq!(SeoEngine::parse(engine.as_str()), Some(engine));
            assert!(!engine.label().is_empty());
        }
        assert_eq!(SeoEngine::parse("bing"), Some(SeoEngine::IndexNow));
        assert_eq!(SeoEngine::parse("BAIDU"), Some(SeoEngine::Baidu));
        assert_eq!(SeoEngine::parse("duckduckgo"), None);
    }

    #[test]
    fn an_seo_failure_never_fails_the_site_release() {
        // handoff.md section 15.2's central rule, asserted directly so a caller cannot reverse it.
        let mut run = SeoPushRun::new("rel-1", "https://example.com", "2026-01-01T12:00:00Z");
        run.records.push(SeoEngineRecord::new(
            SeoEngine::Baidu,
            SeoEngineOutcome::Failed {
                reason: "the engine refused".to_string(),
            },
            "2026-01-01T12:00:00Z",
            false,
        ));
        assert!(run.has_failure());
        assert!(
            !run.affects_site_release(),
            "section 15.2: an SEO failure does not make the site release a failure"
        );
    }

    #[test]
    fn a_failure_is_recorded_with_its_reason_and_offered_for_a_manual_retry() {
        // "It is logged. The user may re-run it manually."
        let run = push(
            &request(),
            &RecordingSubmitter::failing(vec![SeoEngine::Baidu]),
            &clock(),
        );

        let record = run.record(SeoEngine::Baidu).unwrap();
        assert_eq!(record.outcome.status(), "failed");
        assert!(record
            .outcome
            .reason()
            .unwrap()
            .contains("the engine refused"));
        assert!(record.outcome.is_retryable_by_user());
        assert!(run.retryable().contains(&SeoEngine::Baidu));

        // The other engines are unaffected, which is what makes the failure per-engine.
        assert!(run.record(SeoEngine::IndexNow).is_some());
        assert!(!run.affects_site_release());
    }

    #[test]
    fn a_manual_retry_is_marked_as_one_in_the_record() {
        let run = push(
            &request().as_manual_retry(),
            &RecordingSubmitter::accepting(vec![SeoEngine::IndexNow]),
            &clock(),
        );
        assert!(run.records.iter().all(|record| record.manual_retry));
        assert!(rows_for(&run).iter().all(|row| row.manual_retry));
    }

    #[test]
    fn a_publish_run_is_not_marked_as_a_manual_retry() {
        let run = push(
            &request(),
            &RecordingSubmitter::accepting(vec![SeoEngine::IndexNow]),
            &clock(),
        );
        assert!(run.records.iter().all(|record| !record.manual_retry));
    }

    #[test]
    fn offline_marks_every_engine_pending_and_attempts_nothing() {
        // "Offline may be marked pending, and there is no requirement to retry endlessly in the
        // background." So the run must not touch the network at all.
        let submitter = RecordingSubmitter::accepting(SeoEngine::all().to_vec());
        let run = push(&request().offline(), &submitter, &clock());

        assert!(run.has_pending());
        assert!(!run.has_failure(), "offline is not a failure");
        assert_eq!(run.pushed_engines().len(), 0);
        assert!(
            submitter.calls().is_empty(),
            "nothing may be submitted while offline: {:?}",
            submitter.calls()
        );
        assert!(run.records.iter().all(|record| record
            .outcome
            .reason()
            .unwrap()
            .contains("离线")));
    }

    #[test]
    fn a_pending_engine_is_offered_for_a_manual_retry_because_the_user_has_something_to_do() {
        let run = push(
            &request(),
            &ConfigurationRequiredSubmitter,
            &clock(),
        );
        assert!(run.has_pending());
        assert_eq!(run.retryable().len(), 3, "all three need the user");
        assert!(!run.has_failure());
    }

    #[test]
    fn a_missing_configuration_is_pending_rather_than_failed() {
        // Section 15.2 distinguishes "could not try" from "tried and was refused", and only the latter is
        // a failure to report as one.
        let run = push(&request(), &ConfigurationRequiredSubmitter, &clock());
        assert!(!run.has_failure());
        for engine in SeoEngine::all() {
            assert_eq!(run.record(engine).unwrap().outcome.status(), "pending");
        }
    }

    #[test]
    fn a_cancellation_is_pending_rather_than_failed() {
        struct Cancelling;
        impl SeoSubmitter for Cancelling {
            fn submit(&self, _engine: SeoEngine, _url: &str) -> Result<Option<String>> {
                Err(VeloctError::Cancelled)
            }
        }
        let run = push(&request(), &Cancelling, &clock());
        assert!(!run.has_failure());
        assert!(run.has_pending());
    }

    #[test]
    fn an_accepted_engine_is_recorded_as_pushed_and_is_not_offered_for_a_retry() {
        let run = push(
            &request(),
            &RecordingSubmitter::accepting(vec![SeoEngine::IndexNow]),
            &clock(),
        );
        let record = run.record(SeoEngine::IndexNow).unwrap();
        assert_eq!(record.outcome.status(), "pushed");
        assert!(!record.outcome.is_retryable_by_user());
        assert_eq!(run.pushed_engines(), vec![SeoEngine::IndexNow]);
        assert!(!run.retryable().contains(&SeoEngine::IndexNow));
    }

    #[test]
    fn an_engine_that_cannot_be_pushed_is_pending_and_gets_no_guessed_request() {
        // A wrong push is worse than a documented manual step, and a silently swallowed one would look
        // like a success.
        let submitter = RecordingSubmitter::default();
        let run = push(&request(), &submitter, &clock());
        assert!(!run.has_failure());
        for engine in SeoEngine::all() {
            let reason = run.record(engine).unwrap().outcome.reason().unwrap();
            assert!(!reason.is_empty(), "{engine:?} needs a reason");
        }
        // The engine WAS consulted, so the decision came from the submitter rather than from a skip.
        assert_eq!(submitter.calls().len(), 3);
    }

    #[test]
    fn every_engine_states_why_it_needs_the_user() {
        for engine in SeoEngine::all() {
            assert!(
                engine.manual_reason().is_some(),
                "{engine:?} must explain itself"
            );
            assert!(!engine.manual_reason().unwrap().is_empty());
        }
    }

    #[test]
    fn a_site_with_no_public_url_is_pending_for_every_engine_and_submits_nothing() {
        // Submitting a relative path would send the engines somewhere meaningless.
        let submitter = RecordingSubmitter::accepting(SeoEngine::all().to_vec());
        let run = push(
            &SeoPushRequest::new("rel-1", ""),
            &submitter,
            &clock(),
        );
        assert_eq!(run.records.len(), 3);
        assert!(run.has_pending());
        assert!(submitter.calls().is_empty());

        // A URL that is not http is equally unusable.
        assert!(!SeoPushRequest::new("rel-1", "/article/x").is_submittable());
        assert!(SeoPushRequest::new("rel-1", "https://example.com").is_submittable());
    }

    #[test]
    fn the_summary_shows_the_site_succeeded_independently_of_the_seo_push() {
        // Section 15.2's own illustration is the two statuses side by side, so they cannot be conflated.
        let run = push(
            &request(),
            &RecordingSubmitter::failing(vec![SeoEngine::IndexNow]),
            &clock(),
        );
        let summary = run.summary();
        assert!(summary.contains("站点发布 ✅"), "{summary}");
        assert!(summary.contains("SEO 推送"), "{summary}");
        assert!(summary.contains("❌"), "{summary}");
        assert!(summary.contains("⚠️"), "{summary}");
    }

    #[test]
    fn an_empty_run_says_the_push_was_not_requested_rather_than_claiming_success() {
        let run = SeoPushRun::new("rel-1", "https://example.com", "2026-01-01T12:00:00Z");
        assert!(run.summary().contains("未请求"), "{}", run.summary());
        assert!(!run.affects_site_release());
        assert!(!run.has_failure());
        assert!(!run.has_pending());
    }

    #[test]
    fn the_rows_map_onto_the_schema_columns() {
        // The seo_pushes table existed with NO WRITER, which is how a lifecycle goes missing. The mapping
        // is asserted so that cannot happen again quietly.
        let run = push(
            &request(),
            &RecordingSubmitter::failing(vec![SeoEngine::Baidu]),
            &clock(),
        );
        let rows = rows_for(&run);
        assert_eq!(rows.len(), 3);
        for row in &rows {
            assert_eq!(row.release_id, "rel-1");
            assert!(!row.id.is_empty());
            assert!(row.id.starts_with("rel-1:"));
            assert!(row.created_at.is_some());
            assert!(row.finished_at.is_some());
        }
        let baidu = rows.iter().find(|row| row.engine == "baidu").unwrap();
        assert_eq!(baidu.status, "failed");
        assert!(baidu.error.is_some(), "a failure records its reason");

        let index_now = rows.iter().find(|row| row.engine == "indexnow").unwrap();
        assert_eq!(index_now.status, "pending");
    }

    #[test]
    fn a_row_id_is_stable_so_a_retry_updates_rather_than_accumulates() {
        let first = push(&request(), &ConfigurationRequiredSubmitter, &clock());
        let second = push(
            &request().as_manual_retry(),
            &RecordingSubmitter::accepting(vec![SeoEngine::IndexNow]),
            &clock(),
        );
        let first_ids: Vec<String> = rows_for(&first).into_iter().map(|row| row.id).collect();
        let second_ids: Vec<String> = rows_for(&second).into_iter().map(|row| row.id).collect();
        assert_eq!(first_ids, second_ids, "the same engine updates the same row");

        // And the retry's row says it was a retry.
        let retried = rows_for(&second)
            .into_iter()
            .find(|row| row.engine == "indexnow")
            .unwrap();
        assert_eq!(retried.status, "pushed");
        assert!(retried.manual_retry);
    }

    #[test]
    fn the_rows_persist_into_the_real_schema() {
        let connection = veloct_storage::open_in_memory().unwrap();
        // The foreign key needs a release row.
        connection
            .execute(
                "INSERT INTO releases
                   (release_id, commit_sha, tag_name, build_fingerprint, build_inputs_hash, created_at)
                 VALUES ('rel-1', 'abc', 'v1', 'fp', 'bi', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();

        let run = push(
            &request(),
            &RecordingSubmitter::failing(vec![SeoEngine::Baidu]),
            &clock(),
        );
        record_rows(&connection, &run).unwrap();

        let (status, error): (String, Option<String>) = connection
            .query_row(
                "SELECT status, error FROM seo_pushes WHERE release_id = 'rel-1' AND engine = 'baidu'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert!(error.is_some());

        // A retry updates in place rather than adding a row.
        let retry = push(
            &request().as_manual_retry(),
            &RecordingSubmitter::accepting(vec![SeoEngine::Baidu]),
            &clock(),
        );
        record_rows(&connection, &retry).unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM seo_pushes WHERE release_id = 'rel-1' AND engine = 'baidu'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "a retry updates the row");
        let manual: i64 = connection
            .query_row(
                "SELECT manual_retry FROM seo_pushes WHERE release_id = 'rel-1' AND engine = 'baidu'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(manual, 1, "and records that the user asked again");
    }

    #[test]
    fn a_run_records_the_time_it_was_created_for_the_log() {
        let run = push(&request(), &ConfigurationRequiredSubmitter, &clock());
        assert_eq!(run.created_at, "2026-01-01T12:00:00Z");
        assert_eq!(run.release_id, "rel-1");
        assert_eq!(run.site_url, "https://example.com");
    }

    #[test]
    fn an_engine_outcome_reports_its_status_and_reason() {
        assert_eq!(SeoEngineOutcome::Pushed { detail: "ok".into() }.status(), "pushed");
        assert_eq!(SeoEngineOutcome::Failed { reason: "no".into() }.status(), "failed");
        assert_eq!(SeoEngineOutcome::Pending { reason: "later".into() }.status(), "pending");
        assert_eq!(SeoEngineOutcome::Skipped { reason: "n/a".into() }.status(), "skipped");
        assert!(!SeoEngineOutcome::Skipped { reason: "n/a".into() }.is_retryable_by_user());
    }

    #[test]
    fn a_run_round_trips_through_json_for_the_ui() {
        let run = push(&request(), &ConfigurationRequiredSubmitter, &clock());
        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains("\"status\":\"pending\""), "{json}");
        assert_eq!(serde_json::from_str::<SeoPushRun>(&json).unwrap(), run);
    }
}
