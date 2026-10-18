//! Explicit, credential-free GitHub calendar snapshots. Published sites never fetch remote HTML.
use std::{collections::BTreeMap, path::Path, time::Duration};
use serde::{Deserialize, Serialize};
use veloct_core::{http::{HttpClient, HttpRequest}, ProjectLock, Result, VeloctError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay { pub date: String, pub level: u8 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubActivity { pub schema_version: u8, pub username: String, pub fetched_at: String, pub days: Vec<ActivityDay> }

pub fn validate_username(name: &str) -> Result<()> {
    if name.is_empty() { return Ok(()); }
    if name.len() > 39 || name.starts_with('-') || name.ends_with('-') || name.contains("--")
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
        return Err(VeloctError::Invalid("GitHub 用户名只能包含字母、数字或单个连字符，最多 39 个字符；请勿填写完整 URL。".into()));
    }
    Ok(())
}
fn valid_date(date: &str) -> bool {
    time::Date::parse(date, &time::format_description::well_known::Iso8601::DATE).is_ok()
}

// A small quoted-attribute tokenizer, not an attribute-order regex. Only inert td/rect attributes
// are read. Remote markup is NEVER stored, injected, or passed to a browser renderer.
fn attributes(tag: &str) -> BTreeMap<&str, &str> {
    let mut attrs = BTreeMap::new();
    let mut rest = tag.trim_start();
    while !rest.is_empty() {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        let end = rest.find(|c: char| c == '=' || c.is_ascii_whitespace()).unwrap_or(rest.len());
        let key = &rest[..end];
        rest = rest[end..].trim_start();
        if let Some(after) = rest.strip_prefix('=') {
            rest = after.trim_start();
            let Some(quote @ ('\'' | '"')) = rest.chars().next() else { break; };
            rest = &rest[1..];
            let Some(end) = rest.find(quote) else { break; };
            attrs.insert(key, &rest[..end]);
            rest = &rest[end + 1..];
        } else if end == 0 { break; }
    }
    attrs
}
pub fn parse_calendar(html: &str) -> Result<Vec<ActivityDay>> {
    let mut days = BTreeMap::new();
    for fragment in html.split('<') {
        let Some(tag) = fragment.split('>').next() else { continue; };
        let attrs = if let Some(rest) = tag.strip_prefix("td ").or_else(|| tag.strip_prefix("rect ")) { attributes(rest) } else { continue; };
        let Some(date) = attrs.get("data-date") else { continue; };
        let level = attrs.get("data-level").and_then(|value| value.parse::<u8>().ok());
        if !valid_date(date) || !matches!(level, Some(0..=4)) || days.contains_key(*date) {
            return Err(VeloctError::Invalid("GitHub 贡献日历格式发生变化，已保留上次同步结果。".into()));
        }
        days.insert((*date).to_string(), level.unwrap_or(0));
    }
    if days.is_empty() || days.len() > 371 {
        return Err(VeloctError::Invalid("GitHub 未返回有效贡献日历，已保留上次同步结果。".into()));
    }
    Ok(days.into_iter().map(|(date, level)| ActivityDay { date, level }).collect())
}

pub fn fetch_calendar(client: &dyn HttpClient, username: &str, now: &str) -> Result<GithubActivity> {
    validate_username(username)?;
    if username.is_empty() { return Err(VeloctError::Invalid("请先填写 GitHub 用户名。".into())); }
    let response = client.execute(HttpRequest::get(format!("https://github.com/users/{username}/contributions"))
        .header("User-Agent", "Veloct/1.0 (public contribution calendar)")
        .header("Accept", "text/html").timeout(Duration::from_secs(20)))?;
    if response.status != 200 || response.body.len() > 2 * 1024 * 1024 {
        return Err(VeloctError::Invalid(format!("GitHub 日历同步失败（HTTP {}），已保留上次同步结果。", response.status)));
    }
    let html = std::str::from_utf8(&response.body).map_err(|_| VeloctError::Invalid("GitHub 日历编码无效。".into()))?;
    let days = parse_calendar(html)?;
    if days.len() < 300 || days.windows(2).any(|pair| {
        let format = time::format_description::well_known::Iso8601::DATE;
        let first = time::Date::parse(&pair[0].date, &format).ok();
        let second = time::Date::parse(&pair[1].date, &format).ok();
        first.and_then(|day| day.next_day()) != second
    }) {
        return Err(VeloctError::Invalid("GitHub 贡献日历不完整，已保留上次同步结果。".into()));
    }
    Ok(GithubActivity { schema_version: 1, username: username.to_string(), fetched_at: now.to_string(), days })
}

pub fn save_snapshot(root: &Path, snapshot: &GithubActivity) -> Result<()> {
    let _lock = ProjectLock::exclusive(root, "save-github-calendar")?;
    let path = root.join("data/github-activity.json");
    for candidate in [root.to_path_buf(), root.join("data"), path.clone()] {
        match std::fs::symlink_metadata(&candidate) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(VeloctError::Invalid("GitHub 日历不能写入符号链接。".into())),
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(VeloctError::io(&candidate, error)),
            _ => {}
        }
    }
    veloct_core::workdir::write_text_if_changed(&path, &(serde_json::to_string_pretty(snapshot)? + "\n"))?;
    Ok(())
}

#[cfg(test)] mod tests {
    use super::*;
    use veloct_core::http::FakeHttpClient;
    #[test] fn username_is_not_a_url_or_path() {
        for value in ["VellowChire", "a-b", ""] { validate_username(value).unwrap(); }
        for value in ["../x", "https://github.com/x", "a--b", "-ab", "x?x", "x y"] { assert!(validate_username(value).is_err()); }
    }
    #[test] fn calendar_reads_intensity_not_invented_counts() {
        let days = parse_calendar("<td data-level='0' data-date='2026-01-01' class='ContributionCalendar-day'></td><td id=\"x\" data-date=\"2026-01-02\" data-level=\"4\"></td>").unwrap();
        assert_eq!(days, vec![ActivityDay { date: "2026-01-01".into(), level: 0 }, ActivityDay { date: "2026-01-02".into(), level: 4 }]);
        for html in ["<html>login</html>", "<td data-date='2026-02-30' data-level='2'>", "<td data-date='2026-01-01' data-level='9'>", "<td data-date='2026-01-01' data-level='0'><td data-date='2026-01-01' data-level='1'>"] { assert!(parse_calendar(html).is_err()); }
    }
    /// Opt-in network probe: exercises the exact client and fetch path used by the Tauri command,
    /// without opening or changing a user's project. No credentials or third-party API are used.
    #[test]
    #[ignore = "requires access to the official GitHub public calendar"]
    fn live_calendar_uses_production_client() {
        let username = std::env::var("VELOCT_GITHUB_TEST_USERNAME").unwrap_or_else(|_| "octocat".into());
        let client = veloct_core::http::ReqwestHttpClient::without_redirects(Duration::from_secs(20)).unwrap();
        let now = time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap();
        let snapshot = fetch_calendar(&client, &username, &now).unwrap();
        assert_eq!(snapshot.username, username);
        assert!((300..=371).contains(&snapshot.days.len()));
        println!("GitHub official calendar: {} days, {} through {}", snapshot.days.len(), snapshot.days[0].date, snapshot.days.last().unwrap().date);
    }
    fn calendar_html(day_count: usize) -> String {
        let start = time::Date::from_calendar_date(2025, time::Month::January, 1).unwrap();
        (0..day_count).map(|index| {
            let date = start + time::Duration::days(index as i64);
            format!("<td data-date='{date}' data-level='{}'></td>", index % 5)
        }).collect()
    }
    #[test] fn fetch_validates_a_complete_official_calendar() {
        let client = FakeHttpClient::new().always(200, calendar_html(365).into_bytes());
        let now = "2026-01-01T00:00:00Z";
        let snapshot = fetch_calendar(&client, "octocat", now).unwrap();
        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.username, "octocat");
        assert_eq!(snapshot.fetched_at, now);
        assert_eq!(snapshot.days.len(), 365);
        assert_eq!(snapshot.days[4].level, 4);
        assert_eq!(snapshot.days[5].level, 0);
        let requests = client.recorded();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, "https://github.com/users/octocat/contributions");
        assert!(!requests[0].headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("Authorization") || name.eq_ignore_ascii_case("Cookie")));
    }
    #[test] fn invalid_username_never_sends_a_request() {
        let client = FakeHttpClient::new();
        for username in ["", "https://github.com/octocat", "../octocat"] {
            assert!(fetch_calendar(&client, username, "2026-01-01T00:00:00Z").is_err());
        }
        assert_eq!(client.request_count(), 0);
    }
    #[test] fn failed_fetch_cannot_replace_previous_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = GithubActivity { schema_version: 1, username: "octocat".into(), fetched_at: "2026-01-01T00:00:00Z".into(), days: vec![ActivityDay { date: "2025-12-31".into(), level: 3 }] };
        save_snapshot(dir.path(), &snapshot).unwrap();
        let path = dir.path().join("data/github-activity.json");
        let before = std::fs::read(&path).unwrap();
        for status in [500, 502, 503, 504, 403, 404, 429] {
            let client = FakeHttpClient::new().always(status, b"nope".to_vec());
            let error = fetch_calendar(&client, "octocat", "2026-01-02T00:00:00Z").unwrap_err();
            assert!(error.to_string().contains(&format!("HTTP {status}")));
            assert_eq!(before, std::fs::read(&path).unwrap());
        }
        // HTTP 200 is not enough: login pages, incomplete years, gaps, duplicate dates, invalid
        // levels and oversized bodies must not turn a previous good snapshot into an empty one.
        let year = calendar_html(365);
        for body in [
            "<html>login</html>".to_string(),
            calendar_html(299),
            calendar_html(372),
            year.replace("<td data-date='2025-01-02' data-level='1'></td>", ""),
            year.replace("2025-01-02", "2025-01-01"),
            year.replace("data-level='1'", "data-level='9'"),
            " ".repeat(2 * 1024 * 1024 + 1),
        ] {
            let client = FakeHttpClient::new().always(200, body.into_bytes());
            assert!(fetch_calendar(&client, "octocat", "2026-01-02T00:00:00Z").is_err());
            assert_eq!(before, std::fs::read(&path).unwrap());
        }
    }
}
