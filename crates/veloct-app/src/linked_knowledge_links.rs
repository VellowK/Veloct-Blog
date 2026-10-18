//! Safe browser links. Never let API metadata become javascript:, credentials, or an API authority.
use super::linked_knowledge::Provider;
use url::Url;
pub fn node_url(provider: Provider, source: &Url, value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let target = if value.starts_with("https://") {
        Url::parse(value).ok()?
    } else if value.starts_with('/') && !value.starts_with("//") {
        source.join(value).ok()?
    } else if value
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        Url::parse(&format!(
            "{}/{value}",
            source.as_str().trim_end_matches('/')
        ))
        .ok()?
    } else {
        return None;
    };
    if target.scheme() != "https"
        || target.origin() != source.origin()
        || !target.username().is_empty()
        || target.password().is_some()
        || target.port().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
    {
        return None;
    }
    let good = match provider {
        Provider::Yuque => target
            .path()
            .starts_with(&format!("{}/", source.path().trim_end_matches('/'))),
        Provider::Feishu => target.path().starts_with("/wiki/"),
    };
    good.then(|| target.to_string())
}
