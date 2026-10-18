//! Homepage images use the normal attachment cache/upload transaction, never a second image store.
use crate::{AttachmentService, attachment_sync::SyncSettings};
use veloct_attach::{StorageConfig, StorageKind, StorageTarget};
use veloct_core::{Result, VeloctError};

#[derive(Clone, Copy)]
pub enum SiteImageKind {
    Avatar,
    Cover,
}

impl SiteImageKind {
    fn name(self) -> &'static str {
        match self {
            Self::Avatar => "avatar.png",
            Self::Cover => "home-cover.png",
        }
    }
    fn namespace(self) -> &'static str {
        match self {
            Self::Avatar => "site-avatar",
            Self::Cover => "site-home-cover",
        }
    }
    pub fn max_bytes(self) -> usize {
        match self {
            Self::Avatar => crate::site_settings::MAX_AVATAR_BYTES,
            Self::Cover => 12 * 1024 * 1024,
        }
    }
}

/// An explicit sync preference wins even if hidden. Never silently fall back when it is broken.
pub fn preferred_target<'a>(
    configs: &'a [StorageConfig],
    settings: &SyncSettings,
) -> Result<&'a StorageConfig> {
    let cloud = |config: &&StorageConfig| {
        matches!(
            config.kind,
            StorageKind::S3 | StorageKind::GitHub | StorageKind::LskyPro
        )
    };
    let selected = if let Some(id) = settings.target_ids.first() {
        configs
            .iter()
            .find(|config| config.id.as_str() == id)
            .ok_or_else(|| {
                VeloctError::Invalid(format!(
                    "首选附件目标 {id} 不存在，请在附件管理中重新配置。"
                ))
            })?
    } else {
        // A stable sort preserves the saved order when priorities tie.
        let mut available: Vec<_> = configs
            .iter()
            .filter(cloud)
            .filter(|config| !config.hidden_from_upload)
            .collect();
        available.sort_by_key(|config| std::cmp::Reverse(config.upload_priority));
        available.first().copied().ok_or_else(|| {
            VeloctError::Invalid(
                "请先在附件管理中配置一个可见的云存储目标，或设置图片同步的首选目标。".into(),
            )
        })?
    };
    if !cloud(&selected) {
        return Err(VeloctError::Invalid("首页图片的首选附件目标必须是 S3/R2、GitHub 或兰空图床，不能是仅本地缓存或已移除的类型。".into()));
    }
    Ok(selected)
}

/// Does not edit site.json or any Markdown. A failed upload leaves the pool record/cache for retry.
pub fn upload(
    service: &mut AttachmentService,
    kind: SiteImageKind,
    bytes: &[u8],
    target: &StorageTarget,
    now: &str,
    entropy: &[u8; veloct_attach::SUFFIX_LENGTH],
) -> Result<String> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.len() > kind.max_bytes() {
        return Err(VeloctError::Invalid(format!(
            "首页图片必须是 {} MiB 以内的 PNG 图片。",
            kind.max_bytes() / (1024 * 1024)
        )));
    }
    let (attachment, _) = service.add_bytes(
        kind.name(),
        bytes,
        target.config().id.as_str(),
        kind.namespace(),
        now,
        entropy,
    )?;
    if target.config().kind == StorageKind::S3
        && !public_image_url(&target.config().public_url_prefix)
    {
        return Err(VeloctError::Invalid("图片已保留在附件池；请先为首选 S3/R2 目标配置有效的公开域名，再重试上传。原图片地址未更改。".into()));
    }
    let url = if attachment.upload_status == "SUCCEEDED" {
        attachment.remote_url.ok_or_else(|| {
            VeloctError::Invalid("附件已上传但缺少图片地址，请在附件管理中检查。".into())
        })?
    } else {
        service
            .upload(&attachment.id, target, now)
            .map_err(|error| {
                VeloctError::Invalid(format!(
                    "图片已保留在附件池，但上传首选目标失败；原图片地址未更改。{error}"
                ))
            })?
            .remote_url
    };
    if !public_image_url(&url) {
        return Err(VeloctError::Invalid("附件已保存在池内，但首选目标未返回可公开访问的 HTTP(S) 图片地址；原图片地址未更改，请配置公开域名。".into()));
    }
    Ok(url)
}

fn public_image_url(value: &str) -> bool {
    !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

#[cfg(test)]
mod tests;
