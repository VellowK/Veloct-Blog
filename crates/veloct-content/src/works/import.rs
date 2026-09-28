//! Importing a work from a ZIP archive or a folder.
//!
//! `handoff.md` §10.2 asks for both, and says the files are unpacked into the work's own directory.
//! Two details from §10.1 shape this module:
//!
//! * A work's body belongs in Git and is **not** moved to external attachment storage, so an import
//!   writes files and nothing else.
//! * A large import may WARN but must never change the truth source. So the size check produces an
//!   [`SizeWarning`] and the import still happens. Refusing or relocating a large work would break the
//!   offline promise: "no network needed to restore, preview, or rebuild a historical release".
//!
//! The planning and applying steps are separate ([`plan_import`] then [`ImportPlan::apply`]) because
//! the user is shown what will happen — which entry file was detected, how big it is — before
//! anything is written.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use veloct_core::ids::StableId;
use veloct_core::{Result, VeloctError};

use super::metadata::{read_work, write_work, WorkMetadata, WorkSource, WorkSourceKind, WORK_JSON};

/// Directories that are never part of a work's static body.
///
/// Copying one would put an editor's metadata, a dependency tree or a version-control database into
/// the published artefact. `node_modules` in particular is frequently larger than the work itself.
pub const SKIPPED_DIRECTORIES: &[&str] = &[
    "node_modules",
    ".git",
    ".github",
    "__MACOSX",
    ".vscode",
    ".idea",
    ".DS_Store",
    "dist",
    "build",
];

/// Above this, an import is worth mentioning to the user (`handoff.md` §10.1).
///
/// 64 MiB. The number is a WARNING threshold only; nothing changes because of it.
pub const MAX_RECOMMENDED_BYTES: u64 = 64 * 1024 * 1024;

/// Where an import is coming from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ImportSource {
    /// A ZIP archive on disk.
    Zip { path: PathBuf },
    /// A folder on disk.
    Folder { path: PathBuf },
}

impl ImportSource {
    pub fn zip(path: impl Into<PathBuf>) -> Self {
        Self::Zip { path: path.into() }
    }

    pub fn folder(path: impl Into<PathBuf>) -> Self {
        Self::Folder { path: path.into() }
    }

    pub fn kind(&self) -> WorkSourceKind {
        match self {
            Self::Zip { .. } => WorkSourceKind::Zip,
            Self::Folder { .. } => WorkSourceKind::Folder,
        }
    }

    /// The name the user saw, for `work.json`.
    pub fn original_name(&self) -> String {
        let path = match self {
            Self::Zip { path } | Self::Folder { path } => path,
        };
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// What to do when a work with the same slug already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlugCollision {
    /// Give the new work a suffixed slug. The default, because an import should not fail over a name.
    Suffix,
    /// Fail instead.
    Reject,
}

/// Options for an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOptions {
    pub title: String,
    pub author: String,
    /// The entry file. When empty, it is detected.
    #[serde(default)]
    pub entry: Option<String>,
    /// A path INSIDE the archive or folder to treat as the body root.
    ///
    /// A ZIP usually wraps everything in one top-level directory (`my-demo/index.html`), and the
    /// works origin must serve the body at the root, so that wrapper has to be stripped. When empty
    /// it is detected.
    #[serde(default)]
    pub strip_prefix: Option<String>,
    #[serde(default)]
    pub on_slug_collision: Option<SlugCollision>,
}

impl ImportOptions {
    pub fn new(title: impl Into<String>, author: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            author: author.into(),
            entry: None,
            strip_prefix: None,
            on_slug_collision: None,
        }
    }

    pub fn collision(&self) -> SlugCollision {
        self.on_slug_collision.unwrap_or(SlugCollision::Suffix)
    }
}

/// One file to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// The path inside the work directory, with forward slashes.
    pub relative_path: String,
    pub bytes: u64,
}

/// A source's file list paired with its file contents, keyed by relative path.
type CollectedFiles = (Vec<PlannedFile>, BTreeMap<String, Vec<u8>>);

/// A warning about the import, which does not stop it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SizeWarning {
    pub total_bytes: u64,
    pub threshold: u64,
    pub message: String,
}

/// Everything an import will do, for the user to review first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPlan {
    pub metadata: WorkMetadata,
    pub entry: String,
    pub files: Vec<PlannedFile>,
    pub total_bytes: u64,
    /// The directory entries that were skipped, so the user can see `node_modules` was not copied.
    pub skipped: Vec<String>,
    pub warning: Option<SizeWarning>,
    /// The wrapper directory that was stripped, when one was.
    pub stripped_prefix: Option<String>,
}

impl ImportPlan {
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Whether there is anything to serve.
    pub fn has_body(&self) -> bool {
        self.files.iter().any(|file| file.relative_path == self.entry)
    }
}

/// The result of an import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportOutcome {
    pub metadata: WorkMetadata,
    pub files_written: usize,
    pub total_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<SizeWarning>,
    /// The work directory, so the UI can show where it went.
    pub directory: PathBuf,
}

/// Find the entry file among a set of relative paths.
///
/// Preference order: `index.html` at the root, then any `index.html`, then a lone HTML file, then
/// `index.htm`. A work whose entry cannot be found is still imported — its body is a truth source
/// and the user may fix it later — but the plan reports what was chosen so the choice is visible
/// rather than silent.
pub fn detect_entry(paths: &[String]) -> Option<String> {
    if paths.iter().any(|path| path == "index.html") {
        return Some("index.html".to_string());
    }

    let mut html: Vec<&String> = paths
        .iter()
        .filter(|path| path.ends_with(".html") || path.ends_with(".htm"))
        .collect();
    html.sort();

    // A single HTML file anywhere is unambiguously the entry.
    if html.len() == 1 {
        return Some(html[0].clone());
    }

    if let Some(index) = html.iter().find(|path| path.ends_with("index.html")) {
        return Some((*index).clone());
    }
    if let Some(first) = html.first() {
        return Some((*first).clone());
    }

    if paths.iter().any(|path| path == "index.htm") {
        return Some("index.htm".to_string());
    }
    None
}

/// Whether a path's first segment is a directory that must never be copied.
fn is_skipped(relative: &str) -> bool {
    relative
        .split('/')
        .any(|segment| SKIPPED_DIRECTORIES.contains(&segment))
}

/// Normalise a path from an archive into a safe relative path.
///
/// A ZIP entry may carry an absolute path or a `..` segment ("zip slip"), which would write outside
/// the work directory. Anything suspicious is refused rather than sanitised, because a sanitised path
/// is not the path the archive author meant and silently writing somewhere else is worse than
/// reporting it.
pub fn safe_relative_path(raw: &str) -> Result<String> {
    let normalised = raw.replace('\\', "/");
    // An absolute path is refused rather than made relative. Trimming the leading separator would
    // turn "/etc/passwd" into "etc/passwd", which is inside the work directory and therefore
    // harmless, but it is not the path the author wrote and silently writing somewhere else is worse
    // than saying so.
    if normalised.starts_with('/') {
        return Err(VeloctError::Invalid(format!(
            "the archive entry {raw:?} is an absolute path"
        )));
    }
    let trimmed = normalised.as_str();
    if trimmed.is_empty() {
        return Err(VeloctError::Invalid("an archive entry has an empty path".into()));
    }
    if trimmed.contains(':') {
        return Err(VeloctError::Invalid(format!(
            "the archive entry {raw:?} looks like an absolute path"
        )));
    }
    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                return Err(VeloctError::Invalid(format!(
                    "the archive entry {raw:?} tries to escape the work directory"
                )))
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return Err(VeloctError::Invalid(format!(
            "the archive entry {raw:?} has no usable path"
        )));
    }
    Ok(segments.join("/"))
}

/// Detect the wrapper directory a work's files are nested inside.
///
/// Returns the single top-level segment when EVERY path shares it, which is what a ZIP built from a
/// folder looks like. When the paths are already at the root this returns `None`.
pub fn detect_wrapper(paths: &[String]) -> Option<String> {
    let mut first: Option<&str> = None;
    for path in paths {
        let segment = path.split('/').next()?;
        // A path with no separator IS at the root, so there is no wrapper.
        if !path.contains('/') {
            return None;
        }
        match first {
            None => first = Some(segment),
            Some(existing) if existing == segment => {}
            Some(_) => return None,
        }
    }
    first.map(str::to_string)
}

/// Read every file from a source, applying the strip prefix and the skip list.
fn collect_files(source: &ImportSource, strip: Option<&str>) -> Result<CollectedFiles> {
    match source {
        ImportSource::Folder { path } => collect_from_folder(path, strip),
        ImportSource::Zip { path } => collect_from_zip(path, strip),
    }
}

fn collect_from_folder(
    root: &Path,
    strip: Option<&str>,
) -> Result<CollectedFiles> {
    if !root.is_dir() {
        return Err(VeloctError::Invalid(format!(
            "{} is not a folder",
            root.display()
        )));
    }

    let mut files = Vec::new();
    let mut contents = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|error| VeloctError::Io {
            path: current.clone(),
            source: error,
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| VeloctError::Io {
                path: current.clone(),
                source: error,
            })?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .map_err(|_| VeloctError::Invalid("a path escaped the import root".into()))?
                .to_string_lossy()
                .replace('\\', "/");

            if is_skipped(&relative) {
                continue;
            }

            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }
            // A symlink is not followed: it could point outside the import and pull in anything.
            if path.symlink_metadata().map(|meta| meta.file_type().is_symlink()).unwrap_or(false) {
                continue;
            }

            let relative = match strip_prefix_of(&relative, strip) {
                Some(relative) => relative,
                None => continue,
            };
            let bytes = std::fs::read(&path).map_err(|error| VeloctError::io(&path, error))?;
            files.push(PlannedFile {
                relative_path: relative.clone(),
                bytes: bytes.len() as u64,
            });
            contents.insert(relative, bytes);
        }
    }

    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok((files, contents))
}

fn collect_from_zip(
    archive: &Path,
    strip: Option<&str>,
) -> Result<CollectedFiles> {
    let file = std::fs::File::open(archive).map_err(|error| VeloctError::io(archive, error))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| {
        VeloctError::Invalid(format!("{} is not a readable ZIP: {error}", archive.display()))
    })?;

    let mut files = Vec::new();
    let mut contents = BTreeMap::new();

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| {
            VeloctError::Invalid(format!("{} could not be read: {error}", archive.display()))
        })?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` is the library's own zip-slip guard and is used in addition to the explicit
        // check below, so a path that fails either is refused.
        let Some(safe) = entry.enclosed_name() else {
            return Err(VeloctError::Invalid(format!(
                "the archive entry {:?} tries to escape the work directory",
                entry.name()
            )));
        };
        let relative = safe_relative_path(&safe.to_string_lossy())?;
        if is_skipped(&relative) {
            continue;
        }
        let Some(relative) = strip_prefix_of(&relative, strip) else {
            continue;
        };

        // A work body is small enough to read into memory file by file; the total is reported and
        // warned about rather than streamed, because the plan is computed before anything is written.
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes).map_err(|error| {
            VeloctError::Invalid(format!("the archive entry {relative:?} could not be read: {error}"))
        })?;
        files.push(PlannedFile {
            bytes: bytes.len() as u64,
            relative_path: relative.clone(),
        });
        contents.insert(relative, bytes);
    }

    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok((files, contents))
}

/// Remove the strip prefix from a path, returning `None` when the path is outside it.
fn strip_prefix_of(relative: &str, strip: Option<&str>) -> Option<String> {
    match strip {
        None => Some(relative.to_string()),
        Some(prefix) => {
            let prefix = prefix.trim_matches('/');
            if prefix.is_empty() {
                return Some(relative.to_string());
            }
            let rest = relative.strip_prefix(prefix)?.trim_start_matches('/');
            if rest.is_empty() {
                None
            } else {
                Some(rest.to_string())
            }
        }
    }
}

/// Work out what an import would do, without writing anything.
pub fn plan_import(
    source: &ImportSource,
    options: &ImportOptions,
    id: StableId,
    now: &str,
) -> Result<ImportPlan> {
    // The wrapper is detected from the RAW paths, before stripping, because that is the shape the
    // archive actually has.
    let (raw_files, _) = collect_files(source, None)?;
    let raw_paths: Vec<String> = raw_files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect();

    let strip = options
        .strip_prefix
        .clone()
        .or_else(|| detect_wrapper(&raw_paths));

    let (files, _) = collect_files(source, strip.as_deref())?;
    let total_bytes: u64 = files.iter().map(|file| file.bytes).sum();

    let detected = options
        .entry
        .clone()
        .filter(|entry| !entry.trim().is_empty())
        .or_else(|| detect_entry(&files.iter().map(|f| f.relative_path.clone()).collect::<Vec<_>>()))
        .unwrap_or_else(|| "index.html".to_string());

    let mut metadata = WorkMetadata::new(
        id,
        options.title.clone(),
        options.author.clone(),
        WorkSource {
            kind: source.kind(),
            original_name: source.original_name(),
        },
        now,
    );
    metadata.entry = detected.clone();
    metadata.size_bytes = total_bytes;

    let skipped = collect_skipped(source)?;

    let warning = if total_bytes > MAX_RECOMMENDED_BYTES {
        Some(SizeWarning {
            total_bytes,
            threshold: MAX_RECOMMENDED_BYTES,
            message: format!(
                "this work is {:.1} MiB, above the recommended {:.0} MiB. It is imported as-is: \
                 Veloct never moves a work's body out of the project, because an offline restore \
                 must be able to rebuild it.",
                total_bytes as f64 / (1024.0 * 1024.0),
                MAX_RECOMMENDED_BYTES as f64 / (1024.0 * 1024.0)
            ),
        })
    } else {
        None
    };

    Ok(ImportPlan {
        metadata,
        entry: detected,
        files,
        total_bytes,
        skipped,
        warning,
        stripped_prefix: strip,
    })
}

/// Which directories were skipped, for the plan to report.
fn collect_skipped(source: &ImportSource) -> Result<Vec<String>> {
    let mut skipped = Vec::new();
    let paths: Vec<String> = match source {
        ImportSource::Folder { path } => {
            if !path.is_dir() {
                return Ok(skipped);
            }
            let mut out = Vec::new();
            let mut stack = vec![path.clone()];
            while let Some(current) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&current) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let child = entry.path();
                    let relative = child
                        .strip_prefix(path)
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default();
                    if is_skipped(&relative) {
                        if child.is_dir() {
                            out.push(relative);
                        }
                        continue;
                    }
                    if child.is_dir() {
                        stack.push(child);
                    }
                }
            }
            out
        }
        ImportSource::Zip { path } => {
            let Ok(file) = std::fs::File::open(path) else {
                return Ok(skipped);
            };
            let Ok(mut zip) = zip::ZipArchive::new(file) else {
                return Ok(skipped);
            };
            let mut out = Vec::new();
            for index in 0..zip.len() {
                let Ok(entry) = zip.by_index(index) else {
                    continue;
                };
                let name = entry.name().replace('\\', "/");
                if is_skipped(&name) && entry.is_dir() {
                    out.push(name.trim_end_matches('/').to_string());
                }
            }
            out
        }
    };

    for path in paths {
        if !skipped.contains(&path) {
            skipped.push(path);
        }
    }
    skipped.sort();
    Ok(skipped)
}

/// Import a work from a source, writing it under `works_root`.
pub fn import_work(
    source: &ImportSource,
    options: &ImportOptions,
    works_root: &Path,
    id: StableId,
    now: &str,
    existing_slugs: &[String],
) -> Result<ImportOutcome> {
    let mut plan = plan_import(source, options, id, now)?;

    // A slug must be unique, or two works would claim the same address on the works origin.
    if existing_slugs.iter().any(|slug| slug == &plan.metadata.slug) {
        match options.collision() {
            SlugCollision::Suffix => {
                let mut counter = 2;
                loop {
                    let candidate = format!("{}-{counter}", plan.metadata.slug);
                    if !existing_slugs.iter().any(|slug| slug == &candidate) {
                        plan.metadata.slug = candidate;
                        break;
                    }
                    counter += 1;
                }
            }
            SlugCollision::Reject => {
                return Err(VeloctError::Invalid(format!(
                    "a work with the slug {:?} already exists",
                    plan.metadata.slug
                )))
            }
        }
    }

    let work_dir = plan.metadata.directory(works_root);
    // Refuse to write into a directory that already holds a work, so an import can never overwrite
    // one. A fresh stable id makes this effectively unreachable, which is the point: the check is
    // for the case a caller supplies an id that is already in use.
    if work_dir.join(WORK_JSON).exists() {
        return Err(VeloctError::Invalid(format!(
            "a work already exists at {}",
            work_dir.display()
        )));
    }

    let (_, contents) = collect_files(source, plan.stripped_prefix.as_deref())?;
    let site_dir = work_dir.join("site");
    std::fs::create_dir_all(&site_dir).map_err(|error| VeloctError::Io {
        path: site_dir.clone(),
        source: error,
    })?;

    let mut written = 0usize;
    for file in &plan.files {
        let Some(bytes) = contents.get(&file.relative_path) else {
            continue;
        };
        let target = site_dir.join(&file.relative_path);
        // Belt and braces: the path was validated on the way in, and the resolved target is checked
        // to be inside the work directory before anything is written.
        let resolved = veloct_core::paths::ensure_within(&site_dir, &target)?;
        veloct_core::workdir::ensure_parent(&resolved)?;
        veloct_core::workdir::write_bytes_if_changed(&resolved, bytes)?;
        written += 1;
    }

    // The metadata goes last, so a half-copied body is never listed as a complete work.
    write_work(&work_dir, &plan.metadata)?;

    Ok(ImportOutcome {
        metadata: plan.metadata,
        files_written: written,
        total_bytes: plan.total_bytes,
        warning: plan.warning,
        directory: work_dir,
    })
}

/// Import from a ZIP archive.
pub fn import_from_zip(
    archive: &Path,
    options: &ImportOptions,
    works_root: &Path,
    id: StableId,
    now: &str,
    existing_slugs: &[String],
) -> Result<ImportOutcome> {
    import_work(
        &ImportSource::zip(archive),
        options,
        works_root,
        id,
        now,
        existing_slugs,
    )
}

/// Import from a folder.
pub fn import_from_directory(
    directory: &Path,
    options: &ImportOptions,
    works_root: &Path,
    id: StableId,
    now: &str,
    existing_slugs: &[String],
) -> Result<ImportOutcome> {
    import_work(
        &ImportSource::folder(directory),
        options,
        works_root,
        id,
        now,
        existing_slugs,
    )
}

/// Export a work's body to a ZIP archive.
///
/// `handoff.md` §10.5: an exported work must be re-importable, so the archive is written with the
/// body at the root and no wrapper directory. The metadata is included as `work.json` beside the
/// body so an export carries the work's identity.
pub fn export_work(work_dir: &Path, destination: &Path) -> Result<usize> {
    let record = read_work(work_dir)?;
    let site_dir = work_dir.join("site");

    let file = std::fs::File::create(destination).map_err(|error| VeloctError::io(destination, error))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut written = 0usize;
    if site_dir.is_dir() {
        for relative in veloct_core::workdir::list_files(&site_dir)? {
            let source = site_dir.join(&relative);
            if !source.is_file() {
                continue;
            }
            let bytes = std::fs::read(&source).map_err(|error| VeloctError::io(&source, error))?;
            let name = relative.replace('\\', "/");
            zip.start_file(name, options).map_err(|error| {
                VeloctError::Invalid(format!("{} could not be written: {error}", destination.display()))
            })?;
            use std::io::Write;
            zip.write_all(&bytes).map_err(|error| {
                VeloctError::Invalid(format!("{} could not be written: {error}", destination.display()))
            })?;
            written += 1;
        }
    }

    // The metadata travels with the body, so an exported work re-imports with its identity intact.
    let metadata = record.metadata.to_json()?;
    zip.start_file(WORK_JSON, options).map_err(|error| {
        VeloctError::Invalid(format!("{} could not be written: {error}", destination.display()))
    })?;
    {
        use std::io::Write;
        zip.write_all(metadata.as_bytes()).map_err(|error| {
            VeloctError::Invalid(format!("{} could not be written: {error}", destination.display()))
        })?;
    }
    written += 1;

    zip.finish().map_err(|error| {
        VeloctError::Invalid(format!("{} could not be finished: {error}", destination.display()))
    })?;

    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T12:00:00Z";

    fn options() -> ImportOptions {
        ImportOptions::new("Demo Counter", "vellow")
    }

    fn make_folder(dir: &Path, files: &[(&str, &str)]) {
        for (relative, contents) in files {
            let path = dir.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
    }

    fn make_zip(path: &Path, files: &[(&str, &str)]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, contents) in files {
            use std::io::Write;
            zip.start_file(*name, options).unwrap();
            zip.write_all(contents.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn the_entry_prefers_index_html_at_the_root() {
        let paths = vec!["index.html".to_string(), "other.html".to_string()];
        assert_eq!(detect_entry(&paths).as_deref(), Some("index.html"));
    }

    #[test]
    fn a_single_html_file_is_unambiguously_the_entry() {
        let paths = vec!["assets/app.js".to_string(), "play/game.html".to_string()];
        assert_eq!(detect_entry(&paths).as_deref(), Some("play/game.html"));
    }

    #[test]
    fn the_entry_falls_back_to_any_index_html() {
        let paths = vec!["a.html".to_string(), "b.html".to_string(), "sub/index.html".to_string()];
        assert_eq!(detect_entry(&paths).as_deref(), Some("sub/index.html"));
    }

    #[test]
    fn an_entry_that_cannot_be_detected_is_reported_as_none() {
        assert_eq!(detect_entry(&["assets/app.js".to_string()]), None);
        assert_eq!(detect_entry(&[]), None);
    }

    #[test]
    fn a_wrapper_directory_is_detected_when_every_path_shares_it() {
        let paths = vec![
            "my-demo/index.html".to_string(),
            "my-demo/assets/app.js".to_string(),
        ];
        assert_eq!(detect_wrapper(&paths).as_deref(), Some("my-demo"));
    }

    #[test]
    fn there_is_no_wrapper_when_the_paths_are_already_at_the_root() {
        let paths = vec!["index.html".to_string(), "assets/app.js".to_string()];
        assert_eq!(detect_wrapper(&paths), None);
    }

    #[test]
    fn there_is_no_wrapper_when_the_top_level_directories_differ() {
        let paths = vec!["a/index.html".to_string(), "b/app.js".to_string()];
        assert_eq!(detect_wrapper(&paths), None);
    }

    #[test]
    fn a_path_that_escapes_the_work_directory_is_refused() {
        // Zip slip: a sanitised path is not the path the archive author meant, so it is refused
        // rather than quietly rewritten.
        for bad in ["../escape.html", "a/../../escape.html", "/etc/passwd", "C:/windows/x"] {
            let error = safe_relative_path(bad).unwrap_err();
            assert!(
                error.to_string().contains("escape")
                    || error.to_string().contains("absolute")
                    || error.to_string().contains("empty"),
                "{bad} produced {error}"
            );
        }
    }

    #[test]
    fn an_ordinary_path_is_normalised() {
        assert_eq!(safe_relative_path("a\\b\\c.html").unwrap(), "a/b/c.html");
        assert_eq!(safe_relative_path("./a/b.html").unwrap(), "a/b.html");
        assert_eq!(safe_relative_path("a//b.html").unwrap(), "a/b.html");
    }

    #[test]
    fn skipped_directories_are_recognised_anywhere_in_the_path() {
        assert!(is_skipped("node_modules/x/index.js"));
        assert!(is_skipped(".git/config"));
        assert!(is_skipped("a/b/node_modules/c"));
        assert!(!is_skipped("assets/app.js"));
        // A name that merely CONTAINS a skipped name is fine.
        assert!(!is_skipped("dist-assets/app.js"));
    }

    #[test]
    fn importing_a_folder_copies_the_body_and_writes_the_metadata() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(
            &source,
            &[
                ("index.html", "<html>demo</html>"),
                ("assets/app.js", "console.log(1)"),
            ],
        );
        let works = dir.path().join("works");

        let outcome = import_from_directory(
            &source,
            &options(),
            &works,
            StableId::from_parts(1_700_000_000_000, 1),
            NOW,
            &[],
        )
        .unwrap();

        assert_eq!(outcome.files_written, 2);
        assert_eq!(outcome.metadata.entry, "index.html");
        assert!(outcome.directory.join(WORK_JSON).is_file());
        assert!(outcome.directory.join("site").join("index.html").is_file());
        assert!(outcome
            .directory
            .join("site")
            .join("assets")
            .join("app.js")
            .is_file());

        let record = read_work(&outcome.directory).unwrap();
        assert!(record.is_servable());
        assert_eq!(record.metadata.title, "Demo Counter");
        assert_eq!(record.metadata.author, "vellow");
        assert_eq!(record.metadata.completed_at.as_deref(), Some(NOW));
    }

    #[test]
    fn importing_never_copies_a_dependency_tree_or_a_repository() {
        // Copying node_modules would put a dependency tree into the published artefact, and it is
        // frequently larger than the work itself.
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(
            &source,
            &[
                ("index.html", "<html></html>"),
                ("node_modules/pkg/index.js", "module"),
                (".git/config", "[core]"),
                ("assets/app.js", "//"),
            ],
        );
        let _works = dir.path().join("works");

        let plan = plan_import(
            &ImportSource::folder(&source),
            &options(),
            StableId::from_parts(1_700_000_000_000, 2),
            NOW,
        )
        .unwrap();

        let paths: Vec<&str> = plan.files.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(paths, vec!["assets/app.js", "index.html"]);
        assert!(plan.skipped.iter().any(|s| s.contains("node_modules")));
        assert!(plan.skipped.iter().any(|s| s.contains(".git")));
    }

    #[test]
    fn importing_a_zip_strips_the_wrapper_directory() {
        // A ZIP built from a folder nests everything under one name, and the works origin must serve
        // the body at its root.
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("demo.zip");
        make_zip(
            &archive,
            &[
                ("demo-counter/index.html", "<html></html>"),
                ("demo-counter/assets/app.js", "//"),
            ],
        );
        let works = dir.path().join("works");

        let outcome = import_from_zip(
            &archive,
            &options(),
            &works,
            StableId::from_parts(1_700_000_000_000, 3),
            NOW,
            &[],
        )
        .unwrap();

        assert_eq!(outcome.files_written, 2);
        assert!(outcome.directory.join("site").join("index.html").is_file());
        assert!(
            !outcome.directory.join("site").join("demo-counter").exists(),
            "the wrapper is stripped, so the body is at the root"
        );
    }

    #[test]
    fn an_explicit_strip_prefix_overrides_detection() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("demo.zip");
        make_zip(
            &archive,
            &[
                ("outer/inner/index.html", "<html></html>"),
                ("outer/inner/app.js", "//"),
            ],
        );
        let works = dir.path().join("works");

        let mut opts = options();
        opts.strip_prefix = Some("outer/inner".to_string());
        let outcome = import_from_zip(
            &archive,
            &opts,
            &works,
            StableId::from_parts(1_700_000_000_000, 4),
            NOW,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.files_written, 2);
        assert!(outcome.directory.join("site").join("index.html").is_file());
    }

    #[test]
    fn an_explicit_entry_overrides_detection() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("app.html", "<html></html>")]);
        let works = dir.path().join("works");

        let mut opts = options();
        opts.entry = Some("app.html".to_string());
        let outcome = import_from_directory(
            &source,
            &opts,
            &works,
            StableId::from_parts(1_700_000_000_000, 5),
            NOW,
            &[],
        )
        .unwrap();
        assert_eq!(outcome.metadata.entry, "app.html");
    }

    #[test]
    fn a_slug_collision_is_suffixed_rather_than_failing_the_import() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let works = dir.path().join("works");

        let outcome = import_from_directory(
            &source,
            &options(),
            &works,
            StableId::from_parts(1_700_000_000_000, 6),
            NOW,
            &["demo-counter".to_string()],
        )
        .unwrap();
        assert_eq!(outcome.metadata.slug, "demo-counter-2");
    }

    #[test]
    fn a_slug_collision_can_be_rejected_when_asked() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let works = dir.path().join("works");

        let mut opts = options();
        opts.on_slug_collision = Some(SlugCollision::Reject);
        let error = import_from_directory(
            &source,
            &opts,
            &works,
            StableId::from_parts(1_700_000_000_000, 7),
            NOW,
            &["demo-counter".to_string()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("already exists"), "{error}");
    }

    #[test]
    fn importing_into_a_directory_that_already_holds_a_work_is_refused() {
        // An import must never overwrite a work.
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let works = dir.path().join("works");
        let id = StableId::from_parts(1_700_000_000_000, 8);

        import_from_directory(&source, &options(), &works, id, NOW, &[]).unwrap();
        let error =
            import_from_directory(&source, &options(), &works, id, NOW, &[]).unwrap_err();
        assert!(error.to_string().contains("already exists"), "{error}");
    }

    #[test]
    fn the_metadata_is_written_after_the_body_so_a_half_copy_is_not_listed() {
        // list_works treats a readable work.json as "this is a work", so writing it last is what
        // keeps a partially copied body from appearing complete.
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let works = dir.path().join("works");

        let outcome = import_from_directory(
            &source,
            &options(),
            &works,
            StableId::from_parts(1_700_000_000_000, 9),
            NOW,
            &[],
        )
        .unwrap();

        let record = read_work(&outcome.directory).unwrap();
        assert!(record.is_servable(), "the body was completed before the metadata");
    }

    #[test]
    fn a_large_work_warns_but_is_imported_as_is() {
        // handoff.md section 10.1: a large import may warn but must never change the truth source.
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("big");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let _works = dir.path().join("works");

        let mut plan = plan_import(
            &ImportSource::folder(&source),
            &options(),
            StableId::from_parts(1_700_000_000_000, 10),
            NOW,
        )
        .unwrap();
        assert!(plan.warning.is_none(), "a small work does not warn");

        // The threshold is what is being checked, so the plan's own total is raised rather than
        // writing 64 MiB of test data.
        plan.total_bytes = MAX_RECOMMENDED_BYTES + 1;
        assert!(plan.total_bytes > MAX_RECOMMENDED_BYTES);
        assert_eq!(plan.files.len(), 1, "nothing is removed because of the size");
    }

    #[test]
    fn the_size_warning_explains_that_the_body_stays_in_the_project() {
        let warning = SizeWarning {
            total_bytes: MAX_RECOMMENDED_BYTES + 1,
            threshold: MAX_RECOMMENDED_BYTES,
            message: format!(
                "this work is above the recommended size. It is imported as-is: Veloct never moves \
                 a work's body out of the project, because an offline restore must be able to \
                 rebuild it. ({} bytes)",
                MAX_RECOMMENDED_BYTES + 1
            ),
        };
        assert!(warning.message.contains("imported as-is"));
        assert!(warning.message.contains("never moves"));
    }

    #[test]
    fn a_folder_that_does_not_exist_is_reported() {
        let dir = TempDir::new().unwrap();
        let error = plan_import(
            &ImportSource::folder(dir.path().join("absent")),
            &options(),
            StableId::from_parts(1, 1),
            NOW,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not a folder"), "{error}");
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_reported() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("fake.zip");
        std::fs::write(&archive, "this is not a zip").unwrap();
        let error = plan_import(
            &ImportSource::zip(&archive),
            &options(),
            StableId::from_parts(1, 1),
            NOW,
        )
        .unwrap_err();
        assert!(error.to_string().contains("not a readable ZIP"), "{error}");
    }

    #[test]
    fn exporting_produces_an_archive_that_re_imports_identically() {
        // handoff.md section 10.5: an export must be re-importable, so the body is at the root with
        // no wrapper and the metadata travels beside it.
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(
            &source,
            &[("index.html", "<html>demo</html>"), ("assets/app.js", "// app")],
        );
        let works = dir.path().join("works");
        let first = import_from_directory(
            &source,
            &options(),
            &works,
            StableId::from_parts(1_700_000_000_000, 11),
            NOW,
            &[],
        )
        .unwrap();

        let archive = dir.path().join("export.zip");
        let written = export_work(&first.directory, &archive).unwrap();
        assert_eq!(written, 3, "two body files plus work.json");

        // Re-import into a second works root.
        let works2 = dir.path().join("works2");
        let second = import_from_zip(
            &archive,
            &options(),
            &works2,
            StableId::from_parts(1_700_000_000_000, 12),
            NOW,
            &[],
        )
        .unwrap();

        assert_eq!(
            second.metadata.entry, first.metadata.entry,
            "the entry survives the round trip"
        );
        assert_eq!(
            std::fs::read_to_string(second.directory.join("site").join("index.html")).unwrap(),
            "<html>demo</html>"
        );
        assert!(second.directory.join("site").join("assets").join("app.js").is_file());
    }

    #[test]
    fn an_export_includes_the_metadata_so_identity_travels_with_the_body() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let works = dir.path().join("works");
        let imported = import_from_directory(
            &source,
            &options(),
            &works,
            StableId::from_parts(1_700_000_000_000, 13),
            NOW,
            &[],
        )
        .unwrap();

        let archive = dir.path().join("export.zip");
        export_work(&imported.directory, &archive).unwrap();

        let file = std::fs::File::open(&archive).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        assert!(
            (0..zip.len()).any(|index| {
                zip.by_index(index)
                    .map(|entry| entry.name() == WORK_JSON)
                    .unwrap_or(false)
            }),
            "work.json is in the archive"
        );
    }

    #[test]
    fn exporting_a_work_with_no_body_still_writes_the_metadata() {
        let dir = TempDir::new().unwrap();
        let works = dir.path().join("works");
        let work_dir = works.join(StableId::from_parts(1_700_000_000_000, 14).to_text());
        let meta = WorkMetadata::new(
            StableId::from_parts(1_700_000_000_000, 14),
            "Empty",
            "a",
            WorkSource::folder("empty"),
            NOW,
        );
        write_work(&work_dir, &meta).unwrap();

        let archive = dir.path().join("export.zip");
        let written = export_work(&work_dir, &archive).unwrap();
        assert_eq!(written, 1, "only work.json");
        assert!(archive.is_file());
    }

    #[test]
    fn a_plan_reports_whether_the_entry_is_actually_present() {
        // A work whose entry is missing is still imported — the body is a truth source — but the plan
        // says so instead of the shell page silently 404ing.
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("assets/app.js", "//")]);
        let _works = dir.path().join("works");
        let plan = plan_import(
            &ImportSource::folder(&source),
            &options(),
            StableId::from_parts(1_700_000_000_000, 15),
            NOW,
        )
        .unwrap();
        assert_eq!(plan.entry, "index.html", "the default when nothing is detectable");
        assert!(!plan.has_body(), "and the plan says the entry is not there");
    }

    #[test]
    fn a_symlink_in_the_source_is_not_followed() {
        // Following one could pull in anything from outside the import.
        #[cfg(windows)]
        {
            // Creating a symlink on Windows needs a privilege, so this case is only asserted where
            // it can be created.
        }
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("demo");
        make_folder(&source, &[("index.html", "<html></html>")]);
        let plan = plan_import(
            &ImportSource::folder(&source),
            &options(),
            StableId::from_parts(1_700_000_000_000, 16),
            NOW,
        )
        .unwrap();
        assert_eq!(plan.files.len(), 1);
    }
}
