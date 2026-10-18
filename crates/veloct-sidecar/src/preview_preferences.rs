//! Disable Astro's own toolbar for every Veloct-launched preview, including older/custom projects.
//! Astro 7's PreferenceStore reads <site>/.astro/settings.json, with project values before global ones.
use std::path::Path;
use veloct_core::{Result, VeloctError};

pub fn disable_dev_toolbar(site: &Path) -> Result<()> {
    let directory = site.join(".astro");
    let path = directory.join("settings.json");
    for candidate in [&directory, &path] {
        match std::fs::symlink_metadata(candidate) {
            Ok(meta) if meta.file_type().is_symlink() => return Err(VeloctError::Invalid("Astro preferences cannot follow symbolic links".into())),
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(VeloctError::io(candidate, e)),
            _ => {}
        }
    }
    let mut value = match veloct_core::workdir::read_text_optional(&path)? {
        Some(raw) => serde_json::from_str::<serde_json::Value>(&raw)?,
        None => serde_json::json!({}),
    };
    let object = value.as_object_mut().ok_or_else(|| VeloctError::Invalid("Astro settings must be a JSON object".into()))?;
    let toolbar = object.entry("devToolbar").or_insert_with(|| serde_json::json!({}));
    let toolbar = toolbar.as_object_mut().ok_or_else(|| VeloctError::Invalid("Astro devToolbar preferences must be an object".into()))?;
    if toolbar.get("enabled") == Some(&serde_json::json!(false)) { return Ok(()); }
    toolbar.insert("enabled".into(), serde_json::json!(false));
    veloct_core::workdir::ensure_parent(&path)?;
    veloct_core::workdir::write_atomic(&path, serde_json::to_string_pretty(&value)?.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disables_toolbar_without_touching_config_or_other_preferences() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".astro")).unwrap();
        let path = dir.path().join(".astro/settings.json");
        std::fs::write(&path, r#"{"devToolbar":{"enabled":true,"placement":"top-left"},"other":42}"#).unwrap();
        std::fs::write(dir.path().join("astro.config.mjs"), "// custom config").unwrap();
        disable_dev_toolbar(dir.path()).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(json["devToolbar"]["enabled"], false);
        assert_eq!(json["devToolbar"]["placement"], "top-left");
        assert_eq!(json["other"], 42);
        assert_eq!(std::fs::read_to_string(dir.path().join("astro.config.mjs")).unwrap(), "// custom config");
        disable_dev_toolbar(dir.path()).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
    }
    #[test]
    fn creates_missing_preferences_but_preserves_corrupt_files() {
        let dir = tempfile::tempdir().unwrap();
        disable_dev_toolbar(dir.path()).unwrap();
        let path = dir.path().join(".astro/settings.json");
        std::fs::write(&path, "broken").unwrap();
        assert!(disable_dev_toolbar(dir.path()).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
    }
}
