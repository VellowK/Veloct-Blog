//! Safe filesystem primitives.
//!
//! Every durable write in Veloct goes through here, because several locked rules depend on it:
//!
//! * `handoff.md` §4.3/§14.3 — a save that changes nothing must not rewrite the file, otherwise
//!   the publish change-detection would see phantom modifications.
//! * `handoff.md` §20.2 — the editor compares a baseline content hash with the hash on disk;
//!   [`FileRevision`] is that comparison.
//! * `handoff.md` §20.5/§25.1 D3 — a backup must never capture a half-written file, which is
//!   why writes are atomic (temp file + rename in the same directory).

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Result, VeloctError};

/// The observed state of one file: its content hash and, when available, its modification time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRevision {
    pub hash: String,
    pub size: u64,
}

impl FileRevision {
    /// Hash a file's bytes.
    pub fn of_file(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).map_err(|e| VeloctError::io(path, e))?;
        Ok(Self {
            hash: crate::sha256_hex(&bytes),
            size: bytes.len() as u64,
        })
    }

    /// Revision of in-memory content, used as the editor's baseline before saving.
    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self {
            hash: crate::sha256_hex(bytes),
            size: bytes.len() as u64,
        }
    }

    pub fn of_text(text: &str) -> Self {
        Self::of_bytes(text.as_bytes())
    }

    /// Whether the file on disk differs from this revision. `None` means the file is gone.
    pub fn matches_disk(&self, path: &Path) -> Result<bool> {
        match Self::of_file(path) {
            Ok(current) => Ok(current.hash == self.hash),
            Err(VeloctError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(false)
            }
            Err(other) => Err(other),
        }
    }

    /// Whether these bytes are this revision's bytes.
    ///
    /// The in-memory counterpart of `matches_disk`, for a caller that holds text and wants to know
    /// whether the file it came from is still the same. The size is compared first because it is free
    /// and rules out most mismatches without hashing.
    pub fn matches_text(&self, text: &str) -> bool {
        text.len() as u64 == self.size && crate::sha256_hex_str(text) == self.hash
    }
}

/// Outcome of a write attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// The file did not exist and was created.
    Created,
    /// The file existed with different content and was replaced atomically.
    Modified,
    /// The file already had exactly this content; nothing was written.
    Unchanged,
}

impl WriteOutcome {
    /// Whether the write touched the filesystem in a way that changes an archive or commit.
    pub fn is_change(self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

/// Read a file as UTF-8 text.
pub fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| VeloctError::io(path, e))
}

/// Read a file as bytes.
pub fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| VeloctError::io(path, e))
}

/// Create parent directories as needed.
pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| VeloctError::io(parent, e))?;
    }
    Ok(())
}

/// Write `bytes` to `path` atomically.
///
/// The temporary file is created in the destination directory so the final `rename` cannot
/// cross a filesystem boundary, and a reader (including the backup walker) therefore only ever
/// observes either the old file or the complete new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    ensure_parent(path)?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));

    let mut temp = tempfile::Builder::new()
        .prefix(".veloct-tmp-")
        .suffix(".part")
        .tempfile_in(directory)
        .map_err(|e| VeloctError::io(directory, e))?;

    temp.write_all(bytes).map_err(|e| VeloctError::io(path, e))?;
    temp.flush().map_err(|e| VeloctError::io(path, e))?;
    // Durability before the rename: on a crash the destination must not point at empty content.
    temp.as_file().sync_all().map_err(|e| VeloctError::io(path, e))?;

    temp.persist(path)
        .map_err(|e| VeloctError::io(path, e.error))?;
    Ok(())
}

/// Write text only when the content actually differs.
///
/// This is the primitive behind autosave and behind "no content change ⇒ no publish commit"
/// (`handoff.md` §14.3, §23.7).
pub fn write_text_if_changed(path: &Path, contents: &str) -> Result<WriteOutcome> {
    if path.exists() {
        let existing = read_bytes(path)?;
        if existing == contents.as_bytes() {
            return Ok(WriteOutcome::Unchanged);
        }
        write_atomic(path, contents.as_bytes())?;
        return Ok(WriteOutcome::Modified);
    }
    write_atomic(path, contents.as_bytes())?;
    Ok(WriteOutcome::Created)
}

/// Write bytes only when the content actually differs.
pub fn write_bytes_if_changed(path: &Path, contents: &[u8]) -> Result<WriteOutcome> {
    if path.exists() {
        let existing = read_bytes(path)?;
        if existing == contents {
            return Ok(WriteOutcome::Unchanged);
        }
        write_atomic(path, contents)?;
        return Ok(WriteOutcome::Modified);
    }
    write_atomic(path, contents)?;
    Ok(WriteOutcome::Created)
}

/// Read `path`, returning `None` when it does not exist.
pub fn read_text_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(VeloctError::io(path, e)),
    }
}

/// Recursively copy a directory tree, preserving file contents.
///
/// `skip` receives the absolute source path and whether it is a directory; when it returns
/// `true` the entry (and its subtree) is not copied. Used to materialise the site template into
/// a project and to freeze a template into a Release snapshot.
pub fn copy_dir_all<F>(from: &Path, to: &Path, skip: F) -> Result<u64>
where
    F: FnMut(&Path, bool) -> bool,
{
    // The recursion goes through a `&mut` reference so the closure is not re-instantiated at
    // every level (which would otherwise hit the monomorphisation recursion limit).
    let mut skip = skip;
    copy_dir_inner(from, to, &mut skip)
}

fn copy_dir_inner<F>(from: &Path, to: &Path, skip: &mut F) -> Result<u64>
where
    F: FnMut(&Path, bool) -> bool,
{
    let mut copied = 0u64;
    fs::create_dir_all(to).map_err(|e| VeloctError::io(to, e))?;

    let entries = fs::read_dir(from).map_err(|e| VeloctError::io(from, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| VeloctError::io(from, e))?;
        let file_type = entry.file_type().map_err(|e| VeloctError::io(entry.path(), e))?;
        let target = to.join(entry.file_name());
        let is_dir = file_type.is_dir();

        if skip(&entry.path(), is_dir) {
            continue;
        }

        if is_dir {
            copied += copy_dir_inner(&entry.path(), &target, skip)?;
        } else if file_type.is_file() {
            ensure_parent(&target)?;
            fs::copy(entry.path(), &target).map_err(|e| VeloctError::io(&target, e))?;
            copied += 1;
        }
        // Symlinks are deliberately not followed: a project must not be able to make the app
        // write outside its own directory.
    }
    Ok(copied)
}

/// Total size of a directory tree in bytes, ignoring unreadable entries.
pub fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// List files under `root`, returning project-relative POSIX paths, sorted.
pub fn list_files(root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        if let Ok(rel) = entry.path().strip_prefix(root) {
            let text = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/");
            out.push(text);
        }
    }
    out.sort();
    Ok(out)
}

/// Read a file into a string, tolerating a UTF-8 BOM and normalising CRLF to LF.
///
/// Markdown authored on Windows must hash identically to the same content authored elsewhere,
/// otherwise a Release rebuild on another machine would report spurious changes.
pub fn read_text_normalised(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(|e| VeloctError::io(path, e))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|e| VeloctError::io(path, e))?;
    Ok(normalise_text(&bytes))
}

/// Normalise raw bytes into comparable Markdown text.
pub fn normalise_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The absolute location of a Veloct project on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRoot(PathBuf);

impl ProjectRoot {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(crate::paths::normalise(&path.into()))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Absolute path of a project-relative location.
    pub fn join(&self, rel: &crate::paths::RelPath) -> PathBuf {
        rel.to_absolute(&self.0)
    }

    /// Absolute path built from raw segments, e.g. `.veloct/releases/<id>`.
    pub fn at(&self, segments: &[&str]) -> PathBuf {
        let mut out = self.0.clone();
        for segment in segments {
            out.push(segment);
        }
        out
    }

    /// The project metadata directory.
    pub fn veloct_dir(&self) -> PathBuf {
        self.at(&[crate::layout::VELOCT_DIR])
    }

    /// The SQLite database file.
    pub fn db_path(&self) -> PathBuf {
        self.at(&[crate::layout::VELOCT_DIR, crate::layout::DB_FILE])
    }

    pub fn content_dir(&self) -> PathBuf {
        self.at(&[crate::layout::CONTENT_DIR])
    }

    pub fn knowledge_dir(&self) -> PathBuf {
        self.at(&[crate::layout::KNOWLEDGE_DIR])
    }

    pub fn works_dir(&self) -> PathBuf {
        self.at(&[crate::layout::WORKS_DIR])
    }

    pub fn site_dir(&self) -> PathBuf {
        self.at(&[crate::layout::SITE_DIR])
    }

    pub fn dist_dir(&self) -> PathBuf {
        self.at(&[crate::layout::DIST_DIR])
    }

    /// Confirms a path is inside this project.
    pub fn contains(&self, path: impl AsRef<Path>) -> bool {
        crate::paths::normalise(path.as_ref()).starts_with(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_writes_do_not_touch_the_file() {
        // handoff §14.3 / §23.7: autosave must not create phantom changes.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        assert_eq!(write_text_if_changed(&file, "hello").unwrap(), WriteOutcome::Created);
        assert_eq!(write_text_if_changed(&file, "hello").unwrap(), WriteOutcome::Unchanged);
        assert_eq!(write_text_if_changed(&file, "hello world").unwrap(), WriteOutcome::Modified);
        assert!(!WriteOutcome::Unchanged.is_change());
        assert!(WriteOutcome::Created.is_change());
    }

    #[test]
    fn atomic_write_leaves_no_partial_files_behind() {
        // handoff §25.1 D3.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join("deep").join("a.md");
        write_atomic(&file, b"content").unwrap();
        assert_eq!(read_text(&file).unwrap(), "content");

        let leftovers: Vec<_> = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains("veloct-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files leaked: {leftovers:?}");
    }

    #[test]
    fn revisions_detect_external_modification() {
        // handoff §20.2.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        write_atomic(&file, b"original").unwrap();

        let baseline = FileRevision::of_text("original");
        assert!(baseline.matches_disk(&file).unwrap());

        write_atomic(&file, b"changed by an external editor").unwrap();
        assert!(!baseline.matches_disk(&file).unwrap());

        fs::remove_file(&file).unwrap();
        assert!(!baseline.matches_disk(&file).unwrap());
    }

    #[test]
    fn copy_dir_all_honours_skip_and_counts_files() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        write_atomic(&src.join("a.md"), b"a").unwrap();
        write_atomic(&src.join("sub").join("b.md"), b"b").unwrap();
        write_atomic(&src.join("skip-me.txt"), b"x").unwrap();

        let dst = dir.path().join("dst");
        let copied = copy_dir_all(&src, &dst, |path, _| {
            path.file_name().map(|n| n == "skip-me.txt").unwrap_or(false)
        })
        .unwrap();

        assert_eq!(copied, 2);
        assert!(dst.join("a.md").exists());
        assert!(dst.join("sub").join("b.md").exists());
        assert!(!dst.join("skip-me.txt").exists());
    }

    #[test]
    fn text_normalisation_makes_line_endings_irrelevant() {
        // A Release rebuilt on another OS must hash the same content identically.
        let lf = normalise_text(b"one\ntwo\n");
        let crlf = normalise_text(b"one\r\ntwo\r\n");
        let bom = normalise_text("\u{feff}one\ntwo\n".as_bytes());
        assert_eq!(lf, crlf);
        assert_eq!(lf, bom);
        assert_eq!(lf, "one\ntwo\n");
    }

    #[test]
    fn list_files_returns_sorted_posix_relative_paths() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(&dir.path().join("content").join("b.md"), b"b").unwrap();
        write_atomic(&dir.path().join("content").join("a.md"), b"a").unwrap();
        let files = list_files(dir.path()).unwrap();
        assert_eq!(files, vec!["content/a.md".to_string(), "content/b.md".to_string()]);
    }

    #[test]
    fn project_root_exposes_the_documented_layout() {
        // The layout constants are part of the on-disk contract in the data-model spec.
        let root = ProjectRoot::new(r"D:\Veloct-Blog\myproject");
        assert!(root.db_path().ends_with(r".veloct\veloct.db"));
        assert!(root.content_dir().ends_with("content"));
        assert!(root.knowledge_dir().ends_with("knowledge"));
        assert!(root.works_dir().ends_with("works"));
        assert!(root.dist_dir().ends_with("dist"));
        assert!(root.contains(root.content_dir().join("articles").join("a.md")));
        assert!(!root.contains(r"D:\Somewhere\else\a.md"));
    }

    #[test]
    fn optional_read_distinguishes_missing_from_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.md");
        assert!(read_text_optional(&missing).unwrap().is_none());
        write_atomic(&missing, b"now here").unwrap();
        assert_eq!(read_text_optional(&missing).unwrap().unwrap(), "now here");
    }

    #[test]
    fn dir_size_sums_file_lengths() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(&dir.path().join("a"), b"12345").unwrap();
        write_atomic(&dir.path().join("b"), b"123").unwrap();
        assert_eq!(dir_size(dir.path()), 8);
    }
}
