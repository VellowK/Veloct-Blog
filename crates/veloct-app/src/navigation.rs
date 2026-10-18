//! Blog sidebar navigation, independently persisted from site information.
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use veloct_core::{ProjectLock, Result, VeloctError};
use veloct_core::workdir::{read_text_optional, write_text_if_changed, FileRevision};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationItem {
    #[serde(default)] pub id: String,
    pub label: String,
    pub href: String,
    #[serde(default = "default_icon")] pub icon: String,
    #[serde(default = "shown")] pub visible: bool,
    #[serde(default)] pub builtin: bool,
    #[serde(flatten)] pub extra: serde_json::Map<String, serde_json::Value>,
}
fn default_icon() -> String { "links".into() }
fn shown() -> bool { true }
pub fn default_navigation() -> Vec<NavigationItem> {
    [("home", "首页", "/", "home"), ("article", "文章", "/article/", "article"),
     ("docs", "知识库", "/docs/", "docs"), ("works", "作品", "/works/", "works"),
     ("archive", "归档", "/archive/", "archive"), ("links", "友链", "/links/", "links"),
     ("about", "关于", "/about/", "about")].into_iter().map(|(id, label, href, icon)| NavigationItem {
        id: format!("builtin-{id}"), label: label.into(), href: href.into(), icon: icon.into(), visible: true, builtin: true, extra: Default::default(),
    }).collect()
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationView { pub items: Vec<NavigationItem>, pub revision: Option<FileRevision> }
#[derive(Debug)]
pub struct NavigationService { root: PathBuf }
impl NavigationService {
    pub fn new(root: impl AsRef<Path>) -> Self { Self { root: root.as_ref().into() } }
    fn path(&self) -> Result<PathBuf> {
        let path = self.root.join("data/nav.json");
        for candidate in [&self.root, &self.root.join("data"), &path] {
            match std::fs::symlink_metadata(candidate) {
                Ok(meta) if meta.file_type().is_symlink() => return Err(VeloctError::Invalid("navigation cannot follow symbolic links".into())),
                Ok(_) => {},
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(VeloctError::io(candidate, error)),
            }
        }
        Ok(path)
    }
    fn load(&self) -> Result<NavigationView> {
        let Some(raw) = read_text_optional(&self.path()?)? else { return Ok(NavigationView { items: default_navigation(), revision: None }); };
        let mut items: Vec<NavigationItem> = serde_json::from_str(&raw)?;
        // Migrate the old label/href/icon array without losing custom entries or visibility.
        for (position, item) in items.iter_mut().enumerate() {
            if let Some(preset) = default_navigation().into_iter().find(|preset| preset.href == item.href) {
                item.id = preset.id; item.builtin = true;
            } else {
                item.builtin = false;
                if item.id.is_empty() { item.id = format!("custom-legacy-{position}"); }
            }
        }
        for preset in default_navigation() {
            if !items.iter().any(|item| item.id == preset.id) { items.push(preset); }
        }
        validate(&items)?;
        Ok(NavigationView { items, revision: Some(FileRevision::of_text(&raw)) })
    }
    pub fn read(&self) -> Result<NavigationView> {
        let _lock = ProjectLock::shared(&self.root, "read-navigation")?;
        self.load()
    }
    pub fn save(&self, items: Vec<NavigationItem>, baseline: Option<FileRevision>) -> Result<NavigationView> {
        validate(&items)?;
        let _lock = ProjectLock::exclusive(&self.root, "save-navigation")?;
        let current = self.load()?;
        if current.revision != baseline { return Err(VeloctError::ExternalModification { path: self.path()? }); }
        if current.items == items && current.revision.is_some() { return Ok(current); }
        let raw = serde_json::to_string_pretty(&items)? + "\n";
        write_text_if_changed(&self.path()?, &raw)?;
        Ok(NavigationView { items, revision: Some(FileRevision::of_text(&raw)) })
    }
}
fn validate(items: &[NavigationItem]) -> Result<()> {
    if items.len() > 100 { return Err(VeloctError::Invalid("最多允许 100 个菜单".into())); }
    let mut ids = std::collections::HashSet::new();
    let presets = default_navigation();
    for preset in &presets {
        if !items.iter().any(|item| item.id == preset.id && item.builtin && item.href == preset.href) {
            return Err(VeloctError::Invalid("预置菜单不可删除或修改链接，请使用隐藏开关".into()));
        }
    }
    for item in items {
        if item.id.trim().is_empty() || !ids.insert(&item.id) || item.label.trim().is_empty() {
            return Err(VeloctError::Invalid("菜单名称不能为空，菜单 ID 必须唯一".into()));
        }
        if item.builtin != presets.iter().any(|preset| preset.id == item.id) {
            return Err(VeloctError::Invalid("菜单类型无效".into()));
        }
        if !["home", "article", "docs", "works", "archive", "links", "about"].contains(&item.icon.as_str()) {
            return Err(VeloctError::Invalid("菜单图标无效".into()));
        }
        let href = &item.href;
        if href.chars().any(|c| c.is_whitespace() || c.is_control()) || href.contains('\\') || href.starts_with("//") ||
            !(href.starts_with('/') || href.starts_with("https://") && href.len() > 8 || href.starts_with("http://") && href.len() > 7) {
            return Err(VeloctError::Invalid("菜单链接必须是站内路径或 HTTP(S) 地址".into()));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_hidden_presets_and_custom_items_and_detects_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let service = NavigationService::new(dir.path());
        let mut view = service.read().unwrap();
        assert!(view.revision.is_none());
        view.items[0].visible = false;
        let mut custom = view.items[0].clone();
        custom.id = "custom-one".into(); custom.builtin = false; custom.label = "项目".into(); custom.href = "https://example.com".into();
        view.items.push(custom);
        let saved = service.save(view.items, view.revision).unwrap();
        assert_eq!(service.read().unwrap().items, saved.items);
        assert!(service.save(saved.items.clone(), None).is_err());
        let mut removed = saved.items.clone(); removed.remove(0);
        assert!(service.save(removed, saved.revision.clone()).is_err());
        let mut deleted_custom = saved.items; deleted_custom.pop();
        assert!(service.save(deleted_custom, saved.revision).is_ok());
    }
    #[test]
    fn migrates_legacy_and_rejects_unsafe_links() {
        let dir = tempfile::tempdir().unwrap(); std::fs::create_dir(dir.path().join("data")).unwrap();
        std::fs::write(dir.path().join("data/nav.json"), r#"[{"label":"主页","href":"/","icon":"home"},{"label":"自定义","href":"/custom/"}]"#).unwrap();
        let service = NavigationService::new(dir.path()); let view = service.read().unwrap();
        assert_eq!(view.items.len(), 8); assert!(view.items[0].builtin); assert!(!view.items[1].builtin);
        for href in ["javascript:alert(1)", "//evil.com", "/\\evil.com", "https://example.com\n"] {
            let mut items = view.items.clone(); items[1].href = href.into(); assert!(service.save(items, view.revision.clone()).is_err());
        }
    }
}
