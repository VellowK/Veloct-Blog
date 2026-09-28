//! Project-relative path handling.
//!
//! `handoff.md` §22.1 requires content rows to carry a `file_path`, and §14.4/§14.6 require
//! snapshots to reference files. Both must be portable: a Veloct project is expected to be
//! backup-restored onto a different machine, so paths are stored project-relative, in POSIX
//! form, and validated against escaping the project root.

use std::fmt;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Result, VeloctError};

/// A validated path relative to the project root, always using `/` separators.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelPath(String);

impl RelPath {
    /// Validate and normalise a project-relative path given in POSIX or mixed form.
    pub fn new(raw: impl AsRef<str>) -> Result<Self> {
        let raw = raw.as_ref().replace('\\', "/");
        let trimmed = raw.trim_matches('/');
        if trimmed.is_empty() {
            return Err(VeloctError::Invalid("relative path is empty".into()));
        }
        if trimmed.contains(':') && trimmed[1..].starts_with(':') {
            return Err(VeloctError::Invalid(format!(
                "relative path {raw:?} looks like an absolute Windows path"
            )));
        }
        let mut parts = Vec::new();
        for part in trimmed.split('/') {
            match part {
                "" | "." => continue,
                ".." => {
                    return Err(VeloctError::Invalid(format!(
                        "relative path {raw:?} escapes the project root"
                    )))
                }
                other => parts.push(other),
            }
        }
        if parts.is_empty() {
            return Err(VeloctError::Invalid(format!("relative path {raw:?} is empty")));
        }
        Ok(Self(parts.join("/")))
    }

    /// Build from a path that is already known to be clean.
    pub fn from_parts(parts: &[&str]) -> Result<Self> {
        Self::new(parts.join("/"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// File name component.
    pub fn file_name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or(&self.0)
    }

    /// Parent directory as a relative path, if any.
    pub fn parent(&self) -> Option<RelPath> {
        self.0.rsplit_once('/').map(|(head, _)| RelPath(head.to_string()))
    }

    /// Extension without the dot, lowercased.
    pub fn extension(&self) -> Option<String> {
        self.file_name()
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
    }

    /// Join a child segment onto this path.
    pub fn join(&self, child: &str) -> Result<RelPath> {
        RelPath::new(format!("{}/{}", self.0, child))
    }

    /// Whether this path is inside (or equal to) `other`.
    pub fn starts_with(&self, other: &RelPath) -> bool {
        self.0 == other.0 || self.0.starts_with(&format!("{}/", other.0))
    }

    /// The OS path for this relative path under `root`.
    ///
    /// `root` is trusted because it comes from the project record; the relative part has
    /// already been validated by the constructor, so this cannot escape the root.
    pub fn to_absolute(&self, root: impl AsRef<Path>) -> PathBuf {
        let mut out = root.as_ref().to_path_buf();
        for part in self.0.split('/') {
            out.push(part);
        }
        out
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RelPath({})", self.0)
    }
}

impl Serialize for RelPath {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RelPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        RelPath::new(raw).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for RelPath {
    type Err = VeloctError;

    fn from_str(s: &str) -> Result<Self> {
        RelPath::new(s)
    }
}

/// Convert an absolute path into a project-relative path.
///
/// Returns `Ok(None)` when `absolute` is outside `root`, which callers must treat as "not part
/// of this project" rather than silently relativising with `..`.
pub fn to_relative(root: impl AsRef<Path>, absolute: impl AsRef<Path>) -> Result<Option<RelPath>> {
    let root = normalise(root.as_ref());
    let absolute = normalise(absolute.as_ref());
    let rel = match absolute.strip_prefix(&root) {
        Ok(rel) => rel,
        Err(_) => return Ok(None),
    };
    let text: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    if text.is_empty() {
        return Ok(None);
    }
    RelPath::new(text.join("/")).map(Some)
}

/// Lexically normalise a path without touching the filesystem.
///
/// Used instead of `canonicalize` so that a path to a file that does not exist yet can still be
/// compared, and so that symlinked project roots are not resolved behind the user's back.
pub fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Guard used by every filesystem write in Veloct.
///
/// Rejects `..` segments and absolute paths, then confirms the resolved target stays under the
/// project root. Content, attachments and release artefacts are all written through this.
pub fn ensure_within(root: impl AsRef<Path>, candidate: impl AsRef<Path>) -> Result<PathBuf> {
    let root = normalise(root.as_ref());
    let candidate = normalise(candidate.as_ref());
    if candidate.starts_with(&root) {
        return Ok(candidate);
    }
    // The candidate may itself be a relative path; interpret it against the root.
    let joined = normalise(&root.join(&candidate));
    if joined.starts_with(&root) {
        return Ok(joined);
    }
    Err(VeloctError::Invalid(format!(
        "path {} escapes project root {}",
        candidate.display(),
        root.display()
    )))
}

/// Sanitise an arbitrary title into a URL slug.
///
/// CJK characters are preserved (Veloct is aimed at Chinese-language personal sites); only
/// characters that are unsafe in a URL path segment are replaced.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        let keep = ch.is_alphanumeric() || ch == '_';
        if keep {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_normalised_to_posix() {
        let p = RelPath::new(r"content\articles\a.md").unwrap();
        assert_eq!(p.as_str(), "content/articles/a.md");
        assert_eq!(p.file_name(), "a.md");
        assert_eq!(p.extension().as_deref(), Some("md"));
        assert_eq!(p.parent().unwrap().as_str(), "content/articles");
    }

    #[test]
    fn escaping_the_project_root_is_rejected() {
        // handoff §22.1: file_path is project-relative, so escaping must be impossible.
        assert!(RelPath::new("../outside.md").is_err());
        assert!(RelPath::new("content/../../outside.md").is_err());
        assert!(RelPath::new("").is_err());
        assert!(RelPath::new("/").is_err());
        assert!(RelPath::new("C:/windows/system32").is_err());
    }

    #[test]
    fn relative_paths_round_trip_through_json() {
        let p = RelPath::new("works/01ABC/site/index.html").unwrap();
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "\"works/01ABC/site/index.html\"");
        assert_eq!(serde_json::from_str::<RelPath>(&json).unwrap(), p);
        assert!(serde_json::from_str::<RelPath>("\"../boom\"").is_err());
    }

    #[test]
    fn absolute_paths_convert_back_to_relative_ones() {
        let root = PathBuf::from(r"D:\Veloct-Blog\project");
        let rel = to_relative(&root, root.join("content").join("articles").join("a.md")).unwrap();
        assert_eq!(rel.unwrap().as_str(), "content/articles/a.md");
    }

    #[test]
    fn paths_outside_the_root_are_reported_not_relativised() {
        let root = PathBuf::from(r"D:\Veloct-Blog\project");
        let outside = PathBuf::from(r"D:\Somewhere\else\a.md");
        assert!(to_relative(&root, outside).unwrap().is_none());
        assert!(ensure_within(&root, r"D:\Somewhere\else\a.md").is_err());
    }

    #[test]
    fn ensure_within_accepts_relative_inputs_under_the_root() {
        let root = PathBuf::from(r"D:\Veloct-Blog\project");
        let resolved = ensure_within(&root, "content/articles/a.md").unwrap();
        assert!(resolved.starts_with(&root));
        assert!(ensure_within(&root, "../escape.md").is_err());
    }

    #[test]
    fn prefix_matching_respects_directory_boundaries() {
        let parent = RelPath::new("content/articles").unwrap();
        assert!(RelPath::new("content/articles/a.md").unwrap().starts_with(&parent));
        assert!(RelPath::new("content/articles").unwrap().starts_with(&parent));
        assert!(!RelPath::new("content/articles-archive/a.md").unwrap().starts_with(&parent));
    }

    #[test]
    fn slugify_keeps_cjk_and_removes_unsafe_characters() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("我的第一篇文章"), "我的第一篇文章");
        assert_eq!(slugify("Rust + Astro: 一起用"), "rust-astro-一起用");
        assert_eq!(slugify("!!!"), "untitled");
        assert_eq!(slugify(""), "untitled");
    }
}
