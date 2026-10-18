//! Typed editing of the build-time site configuration, preserving template-specific keys.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use veloct_core::workdir::{
    read_text_optional, write_bytes_if_changed, write_text_if_changed, FileRevision,
};
use veloct_core::{ProjectLock, Result, VeloctError};

/// Where a cropped avatar lives inside a project.
///
/// `site/public/` is Astro's static directory: its contents are copied into a build verbatim and are
/// served from the site root by `astro dev` as well, so one file serves both the preview and the
/// published site. Keeping it in the PROJECT rather than in application data is what makes an avatar
/// part of a backup and part of a release, exactly as a work poster is (`handoff.md` §10.1): the
/// picture the site shows belongs to the site, not to the machine that cropped it.
pub const AVATAR_RELATIVE_PATH: &str = "site/public/assets/avatar.png";

/// The URL `AVATAR_RELATIVE_PATH` is served at, and what `author.avatar` is set to.
pub const AVATAR_URL: &str = "/assets/avatar.png";

/// A cropped avatar is a small picture; anything this large is not one, and is refused rather than
/// written into a directory that gets published.
pub const MAX_AVATAR_BYTES: usize = 4 * 1024 * 1024;

/// The PNG file signature (RFC 2083 §3.1).
const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AliasPolicy {
    #[default]
    Redirect,
    CanonicalOnly,
    Disabled,
}

/// The effect the SITE uses when a reader moves from one page to another.
///
/// This is project configuration rather than a local preference, and the distinction is the whole
/// point: the thing that animates is the visitor's browser, so a value kept in the desktop
/// application's own storage could never reach it. `SiteBeautifyPanel` used to do exactly that, which
/// is why the choice appeared to do nothing in the preview.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTransition {
    /// The new page settles up into place. The default, because it is the one that reads as motion
    /// rather than as a rendering artefact.
    #[default]
    Rise,
    /// A plain cross-fade, for a quiet read.
    Dissolve,
    /// No animation at all. Also what a reader who asked for reduced motion gets regardless of this
    /// value, which the stylesheet enforces rather than this enum.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SeoSettings {
    #[serde(rename = "allow_indexing", alias = "allowIndexing")]
    pub allow_indexing: bool,
    pub engines: Vec<String>,
    pub keywords: String,
    #[serde(rename = "html_injection", alias = "htmlInjection")]
    pub html_injection: String,
}

impl Default for SeoSettings {
    fn default() -> Self {
        Self {
            allow_indexing: true,
            engines: Vec::new(),
            keywords: String::new(),
            html_injection: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FilingSettings {
    pub icp: String,
    pub icp_url: String,
    pub police: String,
    pub police_url: String,
    pub moe: String,
    pub moe_url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthorSettings {
    pub name: String,
    pub bio: String,
    pub avatar: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SocialContact {
    pub label: String,
    pub url: String,
    /// Optional public account/contact ID; never a credential.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub id: String,
    // Arrays are replaced on save, so each contact must carry its template-specific keys.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeroSettings {
    /// An empty title follows the site's name in the template.
    pub title: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HomepageSettings {
    pub cover: String,
    pub github_username: String,
    pub show_blog_heatmap: bool,
    pub show_github_heatmap: bool,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

impl Default for HomepageSettings {
    fn default() -> Self {
        Self { cover: String::new(), github_username: String::new(),
            show_blog_heatmap: true, show_github_heatmap: true, extra: Default::default() }
    }
}

/// Known editable fields. Unrelated and unknown keys stay in the source JSON on save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SiteSettings {
    pub name: String,
    pub description: String,
    pub url: String,
    pub base: String,
    pub language: String,
    pub author: AuthorSettings,
    pub social: Vec<SocialContact>,
    pub hero: HeroSettings,
    pub homepage: HomepageSettings,
    pub article_display_count: u32,
    pub archive_display_count: u32,
    pub category_display_count: u32,
    pub tag_display_count: u32,
    pub alias_policy: AliasPolicy,
    pub page_transition: PageTransition,
    pub seo: SeoSettings,
    pub filing: FilingSettings,
}

impl Default for SiteSettings {
    fn default() -> Self {
        Self {
            name: "Veloct".into(), description: String::new(),
            url: "https://example.com".into(), base: "/".into(), language: "zh-CN".into(),
            author: AuthorSettings::default(), social: Vec::new(), hero: HeroSettings::default(),
            homepage: HomepageSettings::default(),
            article_display_count: 10, archive_display_count: 10, category_display_count: 10,
            tag_display_count: 10, alias_policy: AliasPolicy::default(),
            page_transition: PageTransition::default(),
            seo: SeoSettings::default(), filing: FilingSettings::default(),
        }
    }
}

impl SiteSettings {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(VeloctError::Invalid("site name must not be empty".into()));
        }
        crate::github_activity::validate_username(&self.homepage.github_username)?;
        let cover = &self.homepage.cover;
        if !cover.is_empty() {
            let local = cover.starts_with('/') && !cover.starts_with("//");
            let remote = url::Url::parse(cover).is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some() && url.username().is_empty() && url.password().is_none());
            if (!local && !remote) || cover.contains('\\') || cover.chars().any(char::is_whitespace) || cover.chars().any(char::is_control) {
                return Err(VeloctError::Invalid("首页头图必须是站内路径或有效的 http/https 图片地址。".into()));
            }
        }
        // Old configurations with more contacts remain readable; never silently truncate storage.
        for (name, count) in [
            ("articleDisplayCount", self.article_display_count),
            ("archiveDisplayCount", self.archive_display_count),
            ("categoryDisplayCount", self.category_display_count),
            ("tagDisplayCount", self.tag_display_count),
        ] {
            if !(1..=100).contains(&count) {
                return Err(VeloctError::Invalid(format!("{name} must be between 1 and 100")));
            }
        }
        if !self.url.starts_with("http://") && !self.url.starts_with("https://") {
            return Err(VeloctError::Invalid("site url must use http or https".into()));
        }
        if self.url.contains([' ', '?', '#']) {
            return Err(VeloctError::Invalid("site url must not contain spaces or query fragments".into()));
        }
        if !self.base.starts_with('/') || !self.base.ends_with('/') || self.base.contains(['?', '#', '\\']) {
            return Err(VeloctError::Invalid("site base must be a path beginning and ending with /".into()));
        }
        if self.language.trim().is_empty() {
            return Err(VeloctError::Invalid("site language must not be empty".into()));
        }
        for (name, link) in [
            ("ICP 备案跳转链接", &self.filing.icp_url),
            ("公安备案跳转链接", &self.filing.police_url),
            ("萌备案跳转链接", &self.filing.moe_url),
        ] {
            if link.is_empty() { continue; }
            let valid = url::Url::parse(link).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
                    && url.username().is_empty() && url.password().is_none()
            });
            if !valid || link.chars().any(char::is_whitespace) {
                return Err(VeloctError::Invalid(format!("{name}必须是有效的 http 或 https 地址")));
            }
        }
        if self.seo.engines.iter().any(|engine| engine.trim().is_empty()) {
            return Err(VeloctError::Invalid("SEO engine names must not be empty".into()));
        }
        if self.seo.keywords.split(',').any(|keyword| keyword.trim().is_empty()) && !self.seo.keywords.trim().is_empty() {
            return Err(VeloctError::Invalid("SEO keywords must not contain empty entries".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteSettingsView {
    pub settings: SiteSettings,
    /// None means no configuration file exists yet; reading defaults never creates it.
    pub revision: Option<FileRevision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveSiteSettingsView {
    pub settings: SiteSettings,
    pub revision: FileRevision,
    pub changed: bool,
}

#[derive(Debug)]
pub struct SiteSettingsService {
    root: PathBuf,
}

impl SiteSettingsService {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self { root: root.as_ref().to_path_buf() }
    }

    fn path(&self) -> Result<PathBuf> {
        let path = self.root.join("data/site.json");
        for candidate in [&self.root, &self.root.join("data"), &path] {
            match std::fs::symlink_metadata(candidate) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(VeloctError::Invalid(
                    format!("site settings cannot follow a symbolic link: {}", candidate.display())
                )),
                Ok(_) => {},
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(VeloctError::io(candidate, error)),
            }
        }
        Ok(path)
    }

    fn load(&self) -> Result<(Value, Option<FileRevision>)> {
        let path = self.path()?;
        match read_text_optional(&path)? {
            Some(raw) => {
                let value: Value = serde_json::from_str(&raw)?;
                if !value.is_object() {
                    return Err(VeloctError::Invalid("data/site.json must contain an object".into()));
                }
                Ok((value, Some(FileRevision::of_text(&raw))))
            }
            None => Ok((serde_json::json!({}), None)),
        }
    }

    pub fn read(&self) -> Result<SiteSettingsView> {
        let _lock = ProjectLock::shared(&self.root, "read-site-settings")?;
        let (mut value, revision) = self.load()?;
        normalize_seo_keys(&mut value);
        Ok(SiteSettingsView { settings: serde_json::from_value(value)?, revision })
    }

    pub fn save(&self, settings: SiteSettings, baseline: Option<FileRevision>) -> Result<SaveSiteSettingsView> {
        settings.validate()?;
        // Exclusive for the read/check/write transaction: concurrent app writers cannot lose edits.
        let _lock = ProjectLock::exclusive(&self.root, "save-site-settings")?;
        let (mut value, revision) = self.load()?;
        if revision != baseline {
            return Err(VeloctError::ExternalModification { path: self.path()? });
        }
        let previous = value.clone();
        normalize_seo_keys(&mut value);
        merge_known(&mut value, serde_json::to_value(&settings)?);
        // Do not reformat an unchanged file, including one written by another tool.
        if previous == value {
            if let Some(revision) = revision {
                return Ok(SaveSiteSettingsView { settings, revision, changed: false });
            }
        }
        let raw = serde_json::to_string_pretty(&value)? + "\n";
        let changed = write_text_if_changed(&self.path()?, &raw)?.is_change();
        Ok(SaveSiteSettingsView { settings, revision: FileRevision::of_text(&raw), changed })
    }

    /// Store the avatar the user cropped, and answer the path `author.avatar` must be set to.
    ///
    /// The bytes are the PNG the desktop canvas produced. They are RE-CHECKED here rather than
    /// trusted, for the reason `works::set_screenshot` gives for a poster: the window is the only
    /// caller that proves nothing about what it sends, and this writes a file into a directory whose
    /// whole purpose is to be published — so the file has to actually be the image it claims to be.
    ///
    /// The caller writes `author.avatar` separately, through [`Self::save`]. That is deliberate: the
    /// picture and the configuration that names it are two visible steps, and a save that a conflict
    /// refuses cannot leave a stored picture behind claiming to be in use.
    pub fn save_avatar(&self, bytes: &[u8]) -> Result<String> {
        if !bytes.starts_with(PNG_SIGNATURE) {
            return Err(VeloctError::Invalid(
                "the avatar is not a PNG; a cropped avatar must be re-encoded as PNG".into(),
            ));
        }
        if bytes.len() > MAX_AVATAR_BYTES {
            return Err(VeloctError::Invalid(format!(
                "the avatar is {:.1} MiB, above the {:.0} MiB an avatar may be",
                bytes.len() as f64 / (1024.0 * 1024.0),
                MAX_AVATAR_BYTES as f64 / (1024.0 * 1024.0)
            )));
        }
        // Without a site there is nowhere the picture could be served from, and creating `site/` in
        // an ordinary folder would be this command inventing a project.
        if !self.root.join("site/package.json").is_file() {
            return Err(VeloctError::not_configured(
                "this project has no site template, so it has nowhere to serve an avatar from",
                "initialize the project's site template first",
            ));
        }
        let _lock = ProjectLock::shared(&self.root, "save-site-avatar")?;
        let target = self.root.join(AVATAR_RELATIVE_PATH);
        // `ensure_within` stops a path that escapes the root; this stops one that stays inside it
        // only because a directory on the way is a link to somewhere else.
        self.refuse_symlinks(&[
            self.root.join("site"),
            self.root.join("site/public"),
            self.root.join("site/public/assets"),
            target.clone(),
        ])?;
        let resolved = veloct_core::paths::ensure_within(&self.root, &target)?;
        veloct_core::workdir::ensure_parent(&resolved)?;
        write_bytes_if_changed(&resolved, bytes)?;
        Ok(AVATAR_URL.to_string())
    }

    /// A cover is a user-selected raster re-encoded by the desktop canvas, never arbitrary HTML/SVG.
    pub fn save_cover(&self, bytes: &[u8]) -> Result<String> {
        if !bytes.starts_with(PNG_SIGNATURE) || bytes.len() > 12 * 1024 * 1024 {
            return Err(VeloctError::Invalid("首页头图必须是 12 MiB 以内的 PNG 图片。".into()));
        }
        if !self.root.join("site/package.json").is_file() {
            return Err(VeloctError::Invalid("请先初始化站点模板。".into()));
        }
        let _lock = ProjectLock::shared(&self.root, "save-home-cover")?;
        let target = self.root.join("site/public/assets/home-cover.png");
        self.refuse_symlinks(&[self.root.join("site"), self.root.join("site/public"), self.root.join("site/public/assets"), target.clone()])?;
        let resolved = veloct_core::paths::ensure_within(&self.root, &target)?;
        veloct_core::workdir::ensure_parent(&resolved)?;
        write_bytes_if_changed(&resolved, bytes)?;
        Ok("/assets/home-cover.png".into())
    }

    /// Refuse to write through a symbolic link on the way to a destination.
    ///
    /// The same rule [`Self::path`] applies to the settings file, for the same reason: a write that
    /// follows a link lands outside the project, and `ensure_within` cannot see that because the
    /// link itself is inside it.
    fn refuse_symlinks(&self, candidates: &[PathBuf]) -> Result<()> {
        for candidate in candidates {
            match std::fs::symlink_metadata(candidate) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(VeloctError::Invalid(format!(
                        "the avatar cannot be written through a symbolic link: {}",
                        candidate.display()
                    )))
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(VeloctError::io(candidate, error)),
            }
        }
        Ok(())
    }
}

/// These are recognized legacy aliases, not unknown template keys. Remove duplicates before
/// deserializing and saving; otherwise serde rejects a file containing both spellings after merge.
fn normalize_seo_keys(value: &mut Value) {
    if let Some(seo) = value.get_mut("seo").and_then(Value::as_object_mut) {
        for (legacy, canonical) in [("allowIndexing", "allow_indexing"), ("htmlInjection", "html_injection")] {
            if let Some(value) = seo.remove(legacy) {
                seo.entry(canonical.to_owned()).or_insert(value);
            }
        }
    }
}

fn merge_known(target: &mut Value, update: Value) {
    match (target, update) {
        (Value::Object(target), Value::Object(update)) => {
            for (key, value) in update {
                merge_known(target.entry(key).or_insert(Value::Null), value);
            }
        }
        (target, value) => *target = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_configuration_returns_defaults_without_creating_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let service = SiteSettingsService::new(dir.path());
        let view = service.read().unwrap();
        assert_eq!(view.settings, SiteSettings::default());
        assert!(view.revision.is_none());
        assert!(!dir.path().join("data/site.json").exists());
        let saved = service.save(view.settings, None).unwrap();
        assert!(saved.changed);
        assert_eq!(service.read().unwrap().revision, Some(saved.revision));
    }

    #[test]
    fn settings_roundtrip_and_preserve_unknown_nested_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        let path = dir.path().join("data/site.json");
        std::fs::write(&path, r#"{"hero":{"title":"Custom"},"seo":{"custom":42},"filing":{"custom":"keep"}}"#).unwrap();
        let service = SiteSettingsService::new(dir.path());
        let mut view = service.read().unwrap();
        view.settings.article_display_count = 20;
        view.settings.alias_policy = AliasPolicy::CanonicalOnly;
        view.settings.seo.allow_indexing = false;
        view.settings.seo.engines = vec!["bing".into()];
        view.settings.seo.keywords = "veloct,blog".into();
        view.settings.seo.html_injection = "<meta name=\"baidu-site-verification\" content=\"x\">\n<meta name=\"google-site-verification\" content=\"y\">\n<meta name=\"msvalidate.01\" content=\"z\">".into();
        view.settings.filing.icp = "ICP 123".into();
        view.settings.filing.police = "Police 456".into();
        view.settings.filing.moe = "MOE 789".into();
        view.settings.filing.icp_url = "https://beian.miit.gov.cn/?record=123".into();
        view.settings.filing.police_url = "https://beian.mps.gov.cn/#/query/456".into();
        view.settings.filing.moe_url = "https://icp.gov.moe/?keyword=789".into();
        let saved = service.save(view.settings.clone(), view.revision).unwrap();
        assert_eq!(service.read().unwrap().settings, view.settings);
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["hero"]["title"], "Custom");
        assert_eq!(value["seo"]["custom"], 42);
        assert_eq!(value["filing"]["custom"], "keep");
        assert_eq!(value["articleDisplayCount"], 20);
        assert_eq!(value["aliasPolicy"], "canonical_only");
        assert!(!service.save(saved.settings, Some(saved.revision)).unwrap().changed);
    }

    #[test]
    fn filing_links_refuse_unsafe_urls_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        let service = SiteSettingsService::new(dir.path());
        for link in ["javascript:alert(1)", "//example.com/", "https://", "https://user:password@example.com/", "https://example.com/has space"] {
            let mut settings = SiteSettings::default();
            settings.filing.icp_url = link.into();
            assert!(service.save(settings, None).is_err(), "{link}");
            assert!(!dir.path().join("data/site.json").exists());
        }
        let old: FilingSettings = serde_json::from_str(r#"{"icp":"Old record"}"#).unwrap();
        assert!(old.icp_url.is_empty());
        assert_eq!(serde_json::to_value(old).unwrap()["icpUrl"], "");
    }

    #[test]
    fn site_information_roundtrip_preserves_unknown_keys_and_contact_deletion() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        let path = dir.path().join("data/site.json");
        std::fs::write(&path, r#"{
            "customRoot":{"enabled":true},
            "author":{"name":"Old","bio":"Old bio","avatar":"/old.png","customAuthor":42},
            "hero":{"title":"Old title","subtitle":"Keep subtitle","ctaLabel":"Read","ctaHref":"/article/","customHero":true},
            "social":[{"label":"GitHub","url":"https://github.com/old","icon":"github","customContact":{"color":"blue"}},
                      {"label":"Delete me","url":"/old/"}]
        }"#).unwrap();
        let service = SiteSettingsService::new(dir.path());
        let mut view = service.read().unwrap();
        view.settings.name = "我的博客".into();
        view.settings.description = "新的站点描述".into();
        view.settings.url = "https://blog.example.com".into();
        view.settings.author.name = "新作者".into();
        view.settings.author.bio = "新的作者简介".into();
        view.settings.author.avatar = "/assets/avatar.png".into();
        view.settings.hero.title = "自定义首页标题".into();
        view.settings.social.remove(1);
        view.settings.social[0].url = "https://github.com/new".into();
        view.settings.social[0].id = "new-user".into();
        view.settings.social.push(SocialContact {
            label: "微信".into(), id: "public-id".into(), ..SocialContact::default()
        });
        let saved = service.save(view.settings.clone(), view.revision).unwrap();
        assert_eq!(service.read().unwrap().settings, view.settings);
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["name"], "我的博客");
        assert_eq!(value["author"]["avatar"], "/assets/avatar.png");
        assert_eq!(value["author"]["customAuthor"], 42);
        assert_eq!(value["hero"]["subtitle"], "Keep subtitle");
        assert_eq!(value["hero"]["ctaHref"], "/article/");
        assert_eq!(value["hero"]["customHero"], true);
        assert_eq!(value["customRoot"]["enabled"], true);
        assert_eq!(value["social"].as_array().unwrap().len(), 2);
        assert_eq!(value["social"][0]["icon"], "github");
        assert_eq!(value["social"][0]["customContact"]["color"], "blue");
        assert_eq!(value["social"][0]["id"], "new-user");
        assert_eq!(value["social"][1]["id"], "public-id");
        assert_eq!(value["social"][1]["url"], "");
        assert!(!service.save(saved.settings.clone(), Some(saved.revision.clone())).unwrap().changed);
        let mut settings = saved.settings;
        settings.hero.title.clear();
        settings.social.clear();
        service.save(settings.clone(), Some(saved.revision)).unwrap();
        assert_eq!(service.read().unwrap().settings, settings);
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["social"], serde_json::json!([]));
        assert_eq!(value["hero"]["title"], "");
        assert_eq!(value["hero"]["subtitle"], "Keep subtitle");
    }

    #[test]
    fn external_modification_and_deletion_are_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let service = SiteSettingsService::new(dir.path());
        let saved = service.save(SiteSettings::default(), None).unwrap();
        let path = dir.path().join("data/site.json");
        std::fs::write(&path, "{}").unwrap();
        assert!(matches!(service.save(saved.settings.clone(), Some(saved.revision.clone())), Err(VeloctError::ExternalModification { .. })));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(service.save(saved.settings, Some(saved.revision)), Err(VeloctError::ExternalModification { .. })));
        assert!(!path.exists());
    }

    #[test]
    fn malformed_or_non_object_configuration_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        let service = SiteSettingsService::new(dir.path());
        for raw in ["{broken", "[]", "null", r#"{"articleDisplayCount":"ten"}"#] {
            std::fs::write(dir.path().join("data/site.json"), raw).unwrap();
            assert!(service.read().is_err(), "accepted {raw}");
        }
    }

    #[test]
    fn seo_injection_uses_the_frontend_dto_key_and_accepts_legacy_configuration() {
        let legacy: SeoSettings = serde_json::from_value(serde_json::json!({
            "htmlInjection": "<meta name=\"legacy\">"
        })).unwrap();
        assert_eq!(legacy.html_injection, "<meta name=\"legacy\">");
        let value = serde_json::to_value(legacy).unwrap();
        assert_eq!(value["html_injection"], "<meta name=\"legacy\">");
        assert!(value.get("htmlInjection").is_none());
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("data")).unwrap();
        let path = dir.path().join("data/site.json");
        std::fs::write(&path, r#"{"seo":{"allowIndexing":true,"allow_indexing":false,"htmlInjection":"legacy","html_injection":"canonical","custom":"keep"}}"#).unwrap();
        let service = SiteSettingsService::new(dir.path());
        let view = service.read().unwrap();
        assert!(!view.settings.seo.allow_indexing);
        assert_eq!(view.settings.seo.html_injection, "canonical");
        let saved = service.save(view.settings, view.revision).unwrap();
        assert_eq!(service.read().unwrap().settings, saved.settings);
        let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(value["seo"].get("allowIndexing").is_none());
        assert!(value["seo"].get("htmlInjection").is_none());
        assert_eq!(value["seo"]["custom"], "keep");
    }

    #[test]
    fn the_page_transition_is_the_key_the_template_reads_and_defaults_to_rise() {
        // The template reads `pageTransition` from data/site.json, so the DTO key is part of the
        // contract between the two languages rather than an implementation detail.
        let settings = SiteSettings::default();
        assert_eq!(settings.page_transition, PageTransition::Rise);
        let value = serde_json::to_value(&settings).unwrap();
        assert_eq!(value["pageTransition"], "rise");
        assert!(value.get("page_transition").is_none(), "the template cannot read a snake_case key");
        for (name, expected) in [("dissolve", PageTransition::Dissolve), ("none", PageTransition::None), ("rise", PageTransition::Rise)] {
            let parsed: SiteSettings = serde_json::from_value(serde_json::json!({ "pageTransition": name })).unwrap();
            assert_eq!(parsed.page_transition, expected);
            assert_eq!(serde_json::to_value(parsed).unwrap()["pageTransition"], name);
        }
        // A configuration written before this field existed must keep working, and must not silently
        // become "no animation" — the default is the moving one.
        let legacy: SiteSettings = serde_json::from_value(serde_json::json!({ "name": "Old site" })).unwrap();
        assert_eq!(legacy.page_transition, PageTransition::Rise);
        // An unknown effect is refused rather than quietly replaced, the same rule the numeric fields
        // already follow: a site that renders differently from what its configuration says is the
        // failure this project treats as worse than a clear error.
        assert!(serde_json::from_value::<SiteSettings>(serde_json::json!({ "pageTransition": "zoom" })).is_err());
    }

    #[test]
    fn the_page_transition_round_trips_through_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let service = SiteSettingsService::new(dir.path());
        let mut settings = SiteSettings::default();
        settings.page_transition = PageTransition::Dissolve;
        let saved = service.save(settings, None).unwrap();
        assert_eq!(service.read().unwrap().settings.page_transition, PageTransition::Dissolve);
        let value: Value = serde_json::from_str(&std::fs::read_to_string(dir.path().join("data/site.json")).unwrap()).unwrap();
        assert_eq!(value["pageTransition"], "dissolve");
        assert!(!service.save(saved.settings, Some(saved.revision)).unwrap().changed);
    }

    #[test]
    fn invalid_settings_do_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let service = SiteSettingsService::new(dir.path());
        for count in [0, 101] {
            let settings = SiteSettings { article_display_count: count, ..SiteSettings::default() };
            assert!(service.save(settings, None).is_err());
        }
        assert!(!dir.path().join("data/site.json").exists());
    }

    /// A minimal but REAL PNG: signature, IHDR, IDAT and IEND. The avatar check reads the signature
    /// only, and using a well-formed file keeps this fixture honest about what the window sends.
    fn png_bytes() -> Vec<u8> {
        let mut bytes = PNG_SIGNATURE.to_vec();
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x0d]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x1f, 0x15, 0xc4, 0x89]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x0a]);
        bytes.extend_from_slice(b"IDAT");
        bytes.extend_from_slice(&[0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01]);
        bytes.extend_from_slice(&[0x0d, 0x0a, 0x2d, 0xb4]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(b"IEND");
        bytes.extend_from_slice(&[0xae, 0x42, 0x60, 0x82]);
        bytes
    }

    fn service_with_site(dir: &Path) -> SiteSettingsService {
        std::fs::create_dir_all(dir.join("site")).unwrap();
        std::fs::write(dir.join("site/package.json"), "{}").unwrap();
        SiteSettingsService::new(dir)
    }

    /// Create a directory symbolic link, so the guard is exercised on both platforms that can run
    /// these tests rather than only on the one this machine happens to be.
    #[cfg(windows)]
    fn make_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    #[cfg(unix)]
    fn make_dir_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[test]
    fn a_cropped_avatar_is_stored_in_the_site_and_answers_the_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_with_site(dir.path());
        let bytes = png_bytes();
        let url = service.save_avatar(&bytes).unwrap();
        assert_eq!(url, AVATAR_URL);
        assert_eq!(url, "/assets/avatar.png");
        // The bytes stored are the bytes sent: a size check could not catch a re-encoded or
        // truncated file, which is exactly the failure a wrong-bytes upload produces.
        let stored = dir.path().join(AVATAR_RELATIVE_PATH);
        assert_eq!(std::fs::read(&stored).unwrap(), bytes);
        // Rewriting the same picture must not touch the file, so an unrelated save cannot make an
        // avatar look modified to a backup or a release.
        let before = std::fs::metadata(&stored).unwrap().modified().unwrap();
        assert_eq!(service.save_avatar(&bytes).unwrap(), AVATAR_URL);
        assert_eq!(std::fs::metadata(&stored).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn an_avatar_that_is_not_a_png_is_refused_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_with_site(dir.path());
        // A JPEG and a text file: both are plausible things for a bug to send, and neither is a PNG.
        let jpeg = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00];
        assert!(service.save_avatar(&jpeg).is_err());
        assert!(service.save_avatar(b"not an image at all").is_err());
        assert!(service.save_avatar(b"").is_err());
        assert!(!dir.path().join(AVATAR_RELATIVE_PATH).exists());
    }

    #[test]
    fn an_oversized_avatar_is_refused_rather_than_written_into_a_published_directory() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_with_site(dir.path());
        let mut bytes = png_bytes();
        bytes.resize(MAX_AVATAR_BYTES + 1, 0);
        assert!(service.save_avatar(&bytes).is_err());
        assert!(!dir.path().join(AVATAR_RELATIVE_PATH).exists());
        // The boundary itself is accepted, so the limit is not off by one.
        let mut at_limit = png_bytes();
        at_limit.resize(MAX_AVATAR_BYTES, 0);
        assert!(service.save_avatar(&at_limit).is_ok());
    }

    #[test]
    fn a_project_with_no_site_is_told_so_instead_of_having_one_created() {
        let dir = tempfile::tempdir().unwrap();
        let service = SiteSettingsService::new(dir.path());
        let error = service.save_avatar(&png_bytes()).unwrap_err();
        assert!(matches!(error, VeloctError::NotConfigured { .. }), "got {error:?}");
        assert!(!dir.path().join("site").exists());
    }

    #[test]
    fn profile_settings_roundtrip_without_conflating_bio_or_truncating_legacy_contacts() {
        let mut settings: SiteSettings = serde_json::from_value(serde_json::json!({
            "description": "site description", "author": { "bio": "independent bio" },
            "homepage": { "githubUsername": "octocat", "custom": 42 },
            "social": (0..18).map(|i| serde_json::json!({"label": format!("contact-{i}"), "id": "public-id"})).collect::<Vec<_>>()
        })).unwrap();
        settings.validate().unwrap();
        assert_eq!(settings.social.len(), 18);
        assert_eq!(settings.author.bio, "independent bio");
        assert_eq!(settings.description, "site description");
        assert!(settings.homepage.show_blog_heatmap && settings.homepage.show_github_heatmap);
        let encoded = serde_json::to_value(&settings).unwrap();
        assert_eq!(encoded["homepage"]["custom"], 42);
        for cover in ["javascript:alert(1)", "//evil.example/x.png", "https://user:secret@example.com/x.png"] {
            settings.homepage.cover = cover.into(); assert!(settings.validate().is_err());
        }
        settings.homepage.cover = "/assets/home-cover.png".into(); settings.validate().unwrap();
    }

    #[test]
    fn an_avatar_cannot_be_written_through_a_symbolic_link() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_with_site(dir.path());
        let outside = tempfile::tempdir().unwrap();
        // `site/public` is the link, so `ensure_within` sees a path inside the project while the
        // write would land outside it. Creating the link needs privileges on Windows, so a failure
        // to create it SKIPS rather than passing silently.
        if make_dir_symlink(outside.path(), &dir.path().join("site/public")).is_err() {
            eprintln!("skipped: this machine cannot create a directory symlink");
            return;
        }
        assert!(service.save_avatar(&png_bytes()).is_err());
        assert!(!outside.path().join("assets/avatar.png").exists());
    }
}
