//! Optional S3 presentation/upload policy. None preserves every legacy naming decision.
use crate::storage::StorageConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use veloct_core::{Result, VeloctError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessStyle {
    #[default]
    Auto,
    Path,
    VirtualHosted,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rename {
    #[default]
    Legacy,
    Original,
    Uuid,
    Timestamp,
    OriginalRandom,
    DateRandom,
    DateTimeRandom,
    Random,
    Custom,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Duplicate {
    #[default]
    LegacyReuse,
    RandomAlphanumeric,
    RandomAlphabetic,
    Error,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UrlSuffixRule {
    pub extensions: String,
    pub suffix: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct S3Policy {
    pub access_style: AccessStyle,
    pub rename: Rename,
    pub duplicate: Duplicate,
    pub random_length: u8,
    pub filename_template: String,
    pub url_suffixes: Vec<UrlSuffixRule>,
    /// Preview only; never part of the object key or Markdown URL.
    pub thumbnail: String,
}
impl Default for S3Policy {
    fn default() -> Self {
        Self {
            access_style: AccessStyle::Auto,
            rename: Rename::Legacy,
            duplicate: Duplicate::LegacyReuse,
            random_length: 8,
            filename_template: String::new(),
            url_suffixes: Vec::new(),
            thumbnail: String::new(),
        }
    }
}
fn invalid(message: &str) -> VeloctError {
    VeloctError::Invalid(message.into())
}
fn validate_address(value: &str, absolute: bool) -> Result<()> {
    let rest = if let Some((scheme, rest)) = value.split_once("://") {
        if !matches!(scheme, "http" | "https") {
            return Err(invalid("地址协议只能是 HTTP 或 HTTPS。"));
        }
        rest
    } else if absolute {
        return Err(invalid("CDN 地址必须包含 HTTP/HTTPS 协议。"));
    } else {
        value
    };
    if rest.contains(['?', '#', '@', '\\'])
        || rest.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(invalid("地址不能包含登录信息、空白、查询串或片段。"));
    }
    let authority = rest
        .split('/')
        .next()
        .unwrap_or("")
        .replace("{bucket}", "bucket");
    let port_valid = |port: &str| port.parse::<u16>().is_ok();
    let valid = if let Some(ip) = authority.strip_prefix('[') {
        ip.split_once(']').is_some_and(|(ip, tail)| {
            ip.parse::<std::net::Ipv6Addr>().is_ok()
                && (tail.is_empty() || tail.strip_prefix(':').is_some_and(port_valid))
        })
    } else {
        let (host, port) = authority
            .split_once(':')
            .map_or((authority.as_str(), None), |(host, port)| {
                (host, Some(port))
            });
        !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_alphanumeric() || ".-_".contains(c))
            && port.is_none_or(port_valid)
    };
    if !valid {
        return Err(invalid("地址主机或端口无效，请检查 Endpoint / CDN 域名。"));
    }
    Ok(())
}
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', '?', '#'])
        && path.as_bytes().get(1) != Some(&b':')
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn random(seed: &str, token: &str, length: usize, alphabet: &[u8]) -> String {
    (0..length)
        .map(|i| {
            let hash = Sha256::digest(format!("{seed}:{token}:{i}"));
            alphabet[hash[0] as usize % alphabet.len()] as char
        })
        .collect()
}
fn uuid(seed: &str, token: &str, dashed: bool) -> String {
    let mut hash = Sha256::digest(format!("{seed}:{token}"));
    hash[6] = (hash[6] & 15) | 64;
    hash[8] = (hash[8] & 63) | 128;
    let hex = hex::encode(&hash[..16]);
    if dashed {
        format!(
            "{}-{}-{}-{}-{}",
            &hex[..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..]
        )
    } else {
        hex
    }
}
/// Stable attachment seed + its persisted creation time make retries repeat the same path.
/// Dates are UTC. Random placeholders are scoped to this attachment, not freshly regenerated per retry.
fn render(
    template: &str,
    original_stem: Option<&str>,
    seed: &str,
    at: OffsetDateTime,
) -> Result<String> {
    if template.len() > 1024 {
        return Err(invalid("上传目录/文件名模板过长。"));
    }
    let mut out = String::new();
    let mut remaining = template;
    let mut index = 0;
    while let Some(start) = remaining.find("${") {
        out.push_str(&remaining[..start]);
        let tail = &remaining[start + 2..];
        let end = tail
            .find('}')
            .ok_or_else(|| invalid("上传模板占位符缺少 }。"))?;
        let token = &tail[..end];
        let salt = format!("{index}:{token}");
        index += 1;
        let value = match token {
            "origin-filename" => original_stem
                .ok_or_else(|| invalid("上传目录不能使用原文件名占位符。"))?
                .to_string(),
            "uuid-with-dash" => uuid(seed, &salt, true),
            "uuid-no-dash" => uuid(seed, &salt, false),
            "timestamp-sec" => at.unix_timestamp().to_string(),
            "timestamp-ms" => (at.unix_timestamp_nanos() / 1_000_000).to_string(),
            "year" => format!("{:04}", at.year()),
            "month" => format!("{:02}", at.month() as u8),
            "day" => format!("{:02}", at.day()),
            "weekday" => at.weekday().number_from_monday().to_string(),
            "hour" => format!("{:02}", at.hour()),
            "minute" => format!("{:02}", at.minute()),
            "second" => format!("{:02}", at.second()),
            "millisecond" => format!("{:03}", at.millisecond()),
            _ => {
                let (kind, length) = token
                    .split_once(':')
                    .ok_or_else(|| invalid("不支持的上传模板占位符。"))?;
                let length: usize = length
                    .parse()
                    .map_err(|_| invalid("随机占位符长度必须为 1–64。"))?;
                if !(1..=64).contains(&length) {
                    return Err(invalid("随机占位符长度必须为 1–64。"));
                }
                let alphabet: &[u8] = match kind {
                    "random-alphabetic" => b"abcdefghijklmnopqrstuvwxyz",
                    "random-num" => b"0123456789",
                    "random-alphanumeric" => b"abcdefghijklmnopqrstuvwxyz0123456789",
                    _ => return Err(invalid("不支持的上传模板占位符。")),
                };
                random(seed, &salt, length, alphabet)
            }
        };
        out.push_str(&value);
        remaining = &tail[end + 1..];
    }
    out.push_str(remaining);
    if out.contains(['{', '}']) {
        return Err(invalid("上传模板含有无效占位符。"));
    }
    Ok(out)
}
fn split_name(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => (stem, &name[stem.len()..]),
        _ => (name, ""),
    }
}
impl S3Policy {
    pub fn validate(&self, config: &StorageConfig) -> Result<()> {
        if config.access_key.as_deref().unwrap_or("").trim().is_empty() {
            return Err(invalid("请填写 Access Key ID。"));
        }
        if let Some(endpoint) = config.endpoint.as_deref() {
            validate_address(endpoint.trim(), false)?;
        } else if config.region.as_deref() == Some("auto") {
            return Err(invalid(
                "Region 为 auto 时请填写服务商 Endpoint；AWS 请使用实际区域。",
            ));
        }
        if !config.public_url_prefix.trim().is_empty() {
            validate_address(config.public_url_prefix.trim(), true)?;
        }
        if config
            .bucket
            .as_deref()
            .unwrap_or("")
            .chars()
            .any(|c| c.is_whitespace() || "/\\?#@:".contains(c))
        {
            return Err(invalid("Bucket 桶名称不能包含路径、空白或 URL 符号。"));
        }
        if !(1..=64).contains(&self.random_length) {
            return Err(invalid("随机字母长度必须为 1–64。"));
        }
        let now = OffsetDateTime::UNIX_EPOCH;
        self.object_key(
            config,
            "example.png",
            "example-legacy.png",
            "validation",
            now,
        )?;
        if self.url_suffixes.len() > 32 {
            return Err(invalid("网址后缀规则最多 32 项。"));
        }
        for rule in &self.url_suffixes {
            if rule.extensions.trim().is_empty()
                || !rule
                    .extensions
                    .split([',', ' ', ';'])
                    .filter(|s| !s.is_empty())
                    .all(|s| {
                        s == "*"
                            || (!s.trim_start_matches('.').is_empty()
                                && s.trim_start_matches('.')
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric()))
                    })
            {
                return Err(invalid(
                    "网址后缀请填写扩展名，如 jpg,png；* 表示所有类型。",
                ));
            }
            validate_suffix(&rule.suffix, false)?;
        }
        validate_suffix(&self.thumbnail, true)
    }
    pub fn object_key(
        &self,
        config: &StorageConfig,
        original: &str,
        legacy: &str,
        seed: &str,
        at: OffsetDateTime,
    ) -> Result<String> {
        if !crate::is_safe_file_name(original) {
            return Err(invalid("文件名不安全。"));
        }
        let (stem, extension) = split_name(original);
        let random_letters = random(
            seed,
            "filename",
            self.random_length as usize,
            b"abcdefghijklmnopqrstuvwxyz",
        );
        let base = match self.rename {
            Rename::Legacy => return prefixed(config, legacy, seed, at),
            Rename::Original => stem.to_string(),
            Rename::Uuid => uuid(seed, "filename", true),
            Rename::Timestamp => (at.unix_timestamp_nanos() / 1_000_000).to_string(),
            Rename::OriginalRandom => format!("{stem}-{random_letters}"),
            Rename::DateRandom => format!(
                "{}-{:02}-{:02}-{random_letters}",
                at.year(),
                at.month() as u8,
                at.day()
            ),
            Rename::DateTimeRandom => format!(
                "{}-{:02}-{:02}T{:02}-{:02}-{:02}-{random_letters}",
                at.year(),
                at.month() as u8,
                at.day(),
                at.hour(),
                at.minute(),
                at.second()
            ),
            Rename::Random => random_letters,
            Rename::Custom => render(&self.filename_template, Some(stem), seed, at)?,
        };
        if base.contains('/') || base.is_empty() {
            return Err(invalid("文件名模板不能包含目录或为空。"));
        }
        prefixed(config, &format!("{base}{extension}"), seed, at)
    }
}
fn prefixed(config: &StorageConfig, name: &str, seed: &str, at: OffsetDateTime) -> Result<String> {
    let directory = render(
        config.key_prefix.as_deref().unwrap_or("").trim_matches('/'),
        None,
        seed,
        at,
    )?;
    let key = if directory.is_empty() {
        name.to_string()
    } else {
        format!("{directory}/{name}")
    };
    if !safe_path(&key) {
        return Err(invalid(
            "上传路径必须是安全相对路径，不能包含 ..、盘符、反斜线、查询串或空目录。",
        ));
    }
    Ok(key)
}
fn validate_suffix(suffix: &str, thumbnail: bool) -> Result<()> {
    let probe = if thumbnail {
        suffix.replace("{width}", "400")
    } else {
        suffix.to_string()
    };
    if probe.len() > 2048
        || probe.chars().any(|c| c.is_control() || c.is_whitespace())
        || probe.contains(['#', '{', '}'])
        || (!probe.is_empty() && !probe.starts_with(['?', '!', '@']))
    {
        return Err(invalid("网址/缩略图参数需以 ? 或分隔符 !、@ 开头，不能含空白、片段或未知占位符；仅缩略图支持 {width}。"));
    }
    Ok(())
}
pub fn with_suffix(config: &StorageConfig, key: &str, url: String) -> String {
    let Some(policy) = &config.s3_policy else {
        return url;
    };
    let extension = key.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let rule = policy.url_suffixes.iter().find(|rule| {
        rule.extensions.split([',', ' ', ';']).any(|part| {
            part == "*"
                || part
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
        })
    });
    match rule {
        Some(rule) => format!("{url}{}", rule.suffix),
        None => url,
    }
}
pub fn key_for_attachment(
    config: &StorageConfig,
    original: &str,
    legacy: &str,
    seed: &str,
    created_at: &str,
) -> Result<String> {
    let at = OffsetDateTime::parse(created_at, &Rfc3339)
        .map_err(|_| invalid("附件添加时间无效，不能生成稳定上传目录。"))?
        .to_offset(time::UtcOffset::UTC);
    config
        .s3_policy
        .as_ref()
        .ok_or_else(|| invalid("缺少 S3 策略。"))?
        .object_key(config, original, legacy, seed, at)
}
pub fn collision_key(key: &str, attempt: usize, alphabetic: bool) -> String {
    let (parent, name) = key
        .rsplit_once('/')
        .map_or(("", key), |(parent, name)| (parent, name));
    let (stem, ext) = split_name(name);
    let alphabet: &[u8] = if alphabetic {
        b"abcdefghijklmnopqrstuvwxyz"
    } else {
        b"abcdefghijklmnopqrstuvwxyz0123456789"
    };
    let name = format!(
        "{stem}_{}{ext}",
        random(key, &format!("collision:{attempt}"), 4, alphabet)
    );
    if parent.is_empty() {
        name
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
mod tests;
