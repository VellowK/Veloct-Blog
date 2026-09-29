//! Where events go: the file, the rotation, and the in-memory sink the tests and the log page use.
//!
//! `handoff.md` §18 asks for a log FILE and the log page, and §18.1 asks for "reasonable size control /
//! rotation so it does not grow without bound". Both are here, and the rotation rule is deliberately
//! simple enough to reason about: a daily file, rotated to `.1`, `.2` … when it passes a size limit,
//! and the oldest beyond a count removed.
//!
//! The rotation policy is a value rather than constants so a test can drive it with tiny limits, which
//! is the difference between testing rotation and asserting that a constant exists.

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};

use veloct_core::{Clock, Result, VeloctError};

use crate::event::{Entry, LogEvent};

/// 8 MiB per file before it rotates.
pub const DEFAULT_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// How many rotated files to keep, per day.
pub const DEFAULT_MAX_FILES: usize = 5;

/// When to rotate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationPolicy {
    /// Rotate when the current file would exceed this.
    pub max_bytes: u64,
    /// How many rotated files to keep besides the current one.
    pub max_files: usize,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_files: DEFAULT_MAX_FILES,
        }
    }
}

impl RotationPolicy {
    /// A policy with no practical limit, for a caller that wants the day's file to grow.
    pub fn unbounded() -> Self {
        Self {
            max_bytes: u64::MAX,
            max_files: usize::MAX,
        }
    }

    /// Whether a file of this size plus the incoming line should rotate first.
    pub fn should_rotate(&self, current_bytes: u64, incoming_bytes: u64) -> bool {
        // Rotate BEFORE writing, so a record is never split across two files. A half-written line is
        // the one thing a log reader cannot recover from.
        current_bytes > 0 && current_bytes.saturating_add(incoming_bytes) > self.max_bytes
    }
}

/// A destination for events.
pub trait LogSink: Send + Sync {
    fn write(&self, event: &LogEvent) -> Result<()>;

    /// Read back the recent entries, for the log page.
    fn recent(&self, limit: usize) -> Result<Vec<Entry>>;

    /// Flush anything buffered, for the exit path.
    fn flush(&self) -> Result<()>;
}

/// The log directory inside a project.
pub fn default_log_path(project: &Path, timestamp: &str) -> PathBuf {
    // `.veloct/logs/veloct-YYYY-MM-DD.log`, the layout docs/spec/01-data-model.md section 1 fixes. The
    // date comes from the caller's clock so two runs cannot disagree about the file's name.
    let day = timestamp.get(..10).unwrap_or("undated");
    project
        .join(".veloct")
        .join("logs")
        .join(format!("veloct-{day}.log"))
}

/// Build a file name for a rotated file: `veloct-2026-01-01.1.log`.
fn rotated_name(base: &str, index: usize) -> String {
    match base.rsplit_once(".log") {
        Some((stem, _)) => format!("{stem}.{index}.log"),
        None => format!("{base}.{index}"),
    }
}

/// Writes to a file through any [`Write`], so the format and rotation are testable without a disk.
///
/// The abstraction is also what makes the "exit path flushes" behaviour testable: a test supplies a
/// writer it can inspect.
pub struct WriterSink<W: Write + Send> {
    writer: std::sync::Mutex<W>,
    policy: RotationPolicy,
    /// The bytes written so far, so rotation does not have to ask the filesystem every line.
    written: std::sync::atomic::AtomicU64,
}

impl<W: Write + Send> WriterSink<W> {
    pub fn new(writer: W, policy: RotationPolicy) -> Self {
        Self {
            writer: std::sync::Mutex::new(writer),
            policy,
            written: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// The bytes written through this sink.
    pub fn written(&self) -> u64 {
        self.written.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether a line of this size would rotate, so a caller can act before writing.
    pub fn would_rotate(&self, incoming: u64) -> bool {
        self.policy.should_rotate(self.written(), incoming)
    }
}

impl<W: Write + Send> LogSink for WriterSink<W> {
    fn write(&self, event: &LogEvent) -> Result<()> {
        let line = event.line(&veloct_core::clock::format_rfc3339(
            veloct_core::SystemClock.now(),
        ));
        let mut buffer = line.into_bytes();
        buffer.push(b'\n');
        let len = buffer.len() as u64;

        let mut writer = self
            .writer
            .lock()
            .map_err(|_| VeloctError::Other("the log writer is poisoned".into()))?;
        writer
            .write_all(&buffer)
            .map_err(|error| VeloctError::Other(format!("the log could not be written: {error}")))?;
        self.written
            .fetch_add(len, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn recent(&self, _limit: usize) -> Result<Vec<Entry>> {
        // A bare writer cannot read back. That is honest: the file sink below does, and a caller that
        // needs the log page uses that one.
        Err(VeloctError::Unsupported(
            "this sink cannot read its own output".into(),
        ))
    }

    fn flush(&self) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| VeloctError::Other("the log writer is poisoned".into()))?;
        writer
            .flush()
            .map_err(|error| VeloctError::Other(format!("the log could not be flushed: {error}")))
    }
}

/// The real sink: one file per day, rotated by size, with the oldest removed.
///
/// The clock is INJECTED rather than read directly. That is not decoration: the day determines the file
/// name, so a test that wants to prove the day rolls over needs to be able to move the date, and a
/// production sink wants the real clock. Hardcoding `SystemClock` made the rollover untestable and the
/// `now` parameter a value that was used once and then ignored.
pub struct FileSink {
    directory: PathBuf,
    policy: RotationPolicy,
    clock: Box<dyn Clock>,
    /// The day the current file belongs to, so a date change starts a new file rather than appending
    /// to yesterday's.
    current_day: std::sync::Mutex<String>,
    /// The current file's name, so rotation knows which one to rotate.
    current_file: std::sync::Mutex<PathBuf>,
    written: std::sync::atomic::AtomicU64,
}

impl FileSink {
    /// Open the sink for a project's log directory, using the system clock.
    pub fn open(project: &Path, policy: RotationPolicy, now: &str) -> Result<Self> {
        Self::with_clock(project, policy, now, Box::new(veloct_core::SystemClock))
    }

    /// Open the sink with a specific clock, which is what makes the day rollover testable.
    pub fn with_clock(
        project: &Path,
        policy: RotationPolicy,
        now: &str,
        clock: Box<dyn Clock>,
    ) -> Result<Self> {
        let directory = project.join(".veloct").join("logs");
        std::fs::create_dir_all(&directory).map_err(|error| VeloctError::io(&directory, error))?;
        let path = default_log_path(project, now);
        let written = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);

        Ok(Self {
            directory,
            policy,
            clock,
            current_day: std::sync::Mutex::new(now.get(..10).unwrap_or("undated").to_string()),
            current_file: std::sync::Mutex::new(path),
            written: std::sync::atomic::AtomicU64::new(written),
        })
    }

    /// The current time, from the injected clock.
    fn now(&self) -> String {
        veloct_core::clock::format_rfc3339(self.clock.now())
    }

    /// The file events are currently going to.
    pub fn current_path(&self) -> PathBuf {
        self.current_file
            .lock()
            .expect("the current-file lock is not poisoned")
            .clone()
    }

    /// The log directory.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Every log file, newest first, for the log page's file picker.
    pub fn files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&self.directory)
            .map_err(|error| VeloctError::io(&self.directory, error))?
            .flatten()
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("log") {
                files.push(path);
            }
        }
        files.sort();
        files.reverse();
        Ok(files)
    }

    /// Rotate the current file if the policy says so.
    ///
    /// Rotation happens BEFORE the write, so a record is never split across two files. It also means a
    /// single event larger than the limit still gets written whole rather than dropped.
    fn rotate_if_needed(&self, day: &str, incoming: u64) -> Result<()> {
        let current_size = self.written.load(std::sync::atomic::Ordering::SeqCst);
        if !self.policy.should_rotate(current_size, incoming) {
            return Ok(());
        }

        let current = self.current_path();
        if !current.exists() {
            self.written.store(0, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        }

        // Shift `.1` to `.2`, and so on, so the newest rotated file is always `.1`.
        let base = current
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("veloct-{day}.log"));

        let mut index = self.policy.max_files;
        while index > 1 {
            let from = self.directory.join(rotated_name(&base, index - 1));
            let to = self.directory.join(rotated_name(&base, index));
            if from.exists() {
                if index >= self.policy.max_files {
                    // Beyond the kept count: remove rather than shift into a slot nobody will read.
                    let _ = std::fs::remove_file(&from);
                } else {
                    let _ = std::fs::rename(&from, &to);
                }
            }
            index -= 1;
        }

        let first = self.directory.join(rotated_name(&base, 1));
        let _ = std::fs::remove_file(&first);
        std::fs::rename(&current, &first).map_err(|error| VeloctError::io(&current, error))?;

        self.written.store(0, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

impl LogSink for FileSink {
    fn write(&self, event: &LogEvent) -> Result<()> {
        let now = self.now();
        let day = now.get(..10).unwrap_or("undated").to_string();

        // A date change starts a new file: appending today's events to yesterday's file would make the
        // file name a lie.
        {
            let mut current_day = self
                .current_day
                .lock()
                .map_err(|_| VeloctError::Other("the log day lock is poisoned".into()))?;
            if *current_day != day {
                *current_day = day.clone();
                let mut file = self
                    .current_file
                    .lock()
                    .map_err(|_| VeloctError::Other("the log file lock is poisoned".into()))?;
                *file = self.directory.join(format!("veloct-{day}.log"));
                let size = std::fs::metadata(&*file).map(|meta| meta.len()).unwrap_or(0);
                self.written.store(size, std::sync::atomic::Ordering::SeqCst);
            }
        }

        let line = event.line(&now);
        let mut bytes = line.into_bytes();
        bytes.push(b'\n');

        self.rotate_if_needed(&day, bytes.len() as u64)?;

        let path = self.current_path();
        veloct_core::workdir::ensure_parent(&path)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| VeloctError::io(&path, error))?;
        file.write_all(&bytes)
            .map_err(|error| VeloctError::io(&path, error))?;
        file.flush().map_err(|error| VeloctError::io(&path, error))?;

        self.written
            .fetch_add(bytes.len() as u64, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<Entry>> {
        let files = self.files()?;
        let mut entries: Vec<Entry> = Vec::new();

        for path in files {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                // A log file that cannot be read is skipped rather than failing the page: the point of
                // the page is to show what IS there.
                Err(_) => continue,
            };
            let mut file_entries: Vec<Entry> =
                text.lines().filter_map(Entry::parse).collect();
            // Newest first within the file, then the caller reverses the whole thing so the page is
            // chronological while a `limit` keeps the most RECENT entries rather than the oldest.
            file_entries.reverse();
            entries.extend(file_entries);
            if entries.len() >= limit {
                break;
            }
        }

        entries.truncate(limit);
        entries.reverse();
        Ok(entries)
    }

    fn flush(&self) -> Result<()> {
        // Every write opens, appends and flushes, so there is nothing buffered. Reported as success
        // rather than pretending to do something.
        Ok(())
    }
}

/// The log file names `FileSink` produces, for a test or the page.
pub fn log_file_name(day: &str) -> String {
    format!("veloct-{day}.log")
}

/// Removes log files older than a cutoff day.
///
/// Called at startup rather than after every write: a project can accumulate a year of daily files, and
/// checking on each event would stat the directory thousands of times a day for no benefit.
pub fn prune_old_logs(directory: &Path, keep_days: usize) -> Result<usize> {
    if !directory.is_dir() {
        return Ok(0);
    }

    let mut files: Vec<PathBuf> = std::fs::read_dir(directory)
        .map_err(|error| VeloctError::io(directory, error))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("log"))
        .collect();
    files.sort();
    // Newest last, so keeping the tail keeps the newest.
    let remove_count = files.len().saturating_sub(keep_days);
    let mut removed = 0usize;
    for path in files.into_iter().take(remove_count) {
        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// A sink that keeps entries in memory.
///
/// Used by the tests, and by the log page when a project's directory is read-only: a log that cannot be
/// written must still be SHOWABLE, or a failed write would hide its own cause.
pub struct MemorySink {
    entries: std::sync::Mutex<VecDeque<Entry>>,
    capacity: usize,
}

impl MemorySink {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: std::sync::Mutex::new(VecDeque::with_capacity(capacity.min(1024))),
            capacity: capacity.max(1),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|entries| entries.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Everything held, oldest first.
    pub fn snapshot(&self) -> MemorySnapshot {
        let entries = self
            .entries
            .lock()
            .map(|entries| entries.iter().cloned().collect())
            .unwrap_or_default();
        MemorySnapshot { entries }
    }
}

impl LogSink for MemorySink {
    fn write(&self, event: &LogEvent) -> Result<()> {
        let line = event.line(&veloct_core::clock::format_rfc3339(
            veloct_core::SystemClock.now(),
        ));
        let Some(entry) = Entry::parse(&line) else {
            return Err(VeloctError::Other(
                "the event could not be rendered as a log line".into(),
            ));
        };
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| VeloctError::Other("the log buffer is poisoned".into()))?;
        // Bounded, so a long session cannot grow without limit in memory either.
        while entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
        Ok(())
    }

    fn recent(&self, limit: usize) -> Result<Vec<Entry>> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| VeloctError::Other("the log buffer is poisoned".into()))?;
        let skip = entries.len().saturating_sub(limit);
        Ok(entries.iter().skip(skip).cloned().collect())
    }

    fn flush(&self) -> Result<()> {
        Ok(())
    }
}

/// A snapshot of a [`MemorySink`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySnapshot {
    pub entries: Vec<Entry>,
}

impl MemorySnapshot {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries at one level.
    pub fn at_level(&self, level: crate::event::Level) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.level == level)
            .collect()
    }

    /// The entries in one category.
    pub fn in_category(&self, category: crate::event::Category) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.category == category)
            .collect()
    }

    /// Everything matching a search term.
    pub fn matching(&self, needle: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.matches(needle))
            .collect()
    }

    /// The whole snapshot rendered as one string, for a test that asserts a secret is absent.
    pub fn render(&self) -> String {
        self.entries
            .iter()
            .map(|entry| {
                let fields: Vec<String> = entry
                    .fields
                    .iter()
                    .map(|(key, value)| format!("{key}={value}"))
                    .collect();
                format!(
                    "{} {} {}: {} {}",
                    entry.timestamp,
                    entry.level.bracket(),
                    entry.category.as_str(),
                    entry.message,
                    fields.join(" ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Category, Level, LogEvent};
    use std::sync::Arc;
    use tempfile::TempDir;

    const ANTHROPIC_KEY: &str = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";

    #[test]
    fn the_default_log_path_is_the_documented_layout() {
        // docs/spec/01-data-model.md section 1: .veloct/logs/veloct-YYYY-MM-DD.log
        let path = default_log_path(Path::new("D:/proj"), "2026-01-01T12:00:00Z");
        let text = path.to_string_lossy().replace('\\', "/");
        assert!(text.ends_with(".veloct/logs/veloct-2026-01-01.log"), "{text}");
    }

    #[test]
    fn an_undated_timestamp_still_produces_a_file_name() {
        let path = default_log_path(Path::new("D:/proj"), "");
        assert!(path.to_string_lossy().contains("undated"));
    }

    #[test]
    fn the_rotation_policy_rotates_before_writing_so_a_record_is_never_split() {
        let policy = RotationPolicy {
            max_bytes: 100,
            max_files: 3,
        };
        // An empty file never rotates: the first record must land somewhere.
        assert!(!policy.should_rotate(0, 500));
        // Fits.
        assert!(!policy.should_rotate(50, 40));
        // Exactly at the limit fits.
        assert!(!policy.should_rotate(50, 50));
        // One byte over rotates.
        assert!(policy.should_rotate(50, 51));
        // A single record larger than the limit still gets written whole.
        assert!(policy.should_rotate(10, 500));
    }

    #[test]
    fn the_default_rotation_is_bounded() {
        let policy = RotationPolicy::default();
        assert_eq!(policy.max_bytes, DEFAULT_MAX_BYTES);
        assert_eq!(policy.max_files, DEFAULT_MAX_FILES);
        assert!(policy.max_bytes > 0);
        assert!(policy.max_files > 0);
    }

    #[test]
    fn a_writer_sink_writes_one_line_per_event() {
        let buffer: Arc<std::sync::Mutex<Vec<u8>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = WriterSink::new(SharedBuffer(buffer.clone()), RotationPolicy::unbounded());

        sink.write(&LogEvent::info(Category::App, "started")).unwrap();
        sink.write(&LogEvent::error(Category::Build, "failed")).unwrap();
        sink.flush().unwrap();

        let text = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains("[info] app: started"), "{text}");
        assert!(text.contains("[error] build: failed"), "{text}");
        assert!(sink.written() > 0);
    }

    /// A `Write` over a shared buffer, so a test can read what the sink wrote.
    struct SharedBuffer(Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_writer_sink_that_cannot_read_back_says_so() {
        // Honest rather than returning an empty list, which would look like an empty log.
        let buffer: Arc<std::sync::Mutex<Vec<u8>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = WriterSink::new(SharedBuffer(buffer), RotationPolicy::unbounded());
        let error = sink.recent(10).unwrap_err();
        assert!(error.to_string().contains("cannot read"), "{error}");
    }

    #[test]
    fn a_memory_sink_holds_and_returns_entries_oldest_first() {
        let sink = MemorySink::new(10);
        sink.write(&LogEvent::info(Category::App, "one")).unwrap();
        sink.write(&LogEvent::warning(Category::Save, "two")).unwrap();
        sink.write(&LogEvent::error(Category::Build, "three")).unwrap();

        let recent = sink.recent(10).unwrap();
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].message, "one");
        assert_eq!(recent[2].message, "three");
        assert_eq!(recent[2].level, Level::Error);
    }

    #[test]
    fn a_memory_sink_is_bounded_so_a_long_session_cannot_grow_without_limit() {
        let sink = MemorySink::new(3);
        for index in 0..10 {
            sink.write(&LogEvent::info(Category::App, format!("event {index}")))
                .unwrap();
        }
        assert_eq!(sink.len(), 3, "the capacity is respected");
        let recent = sink.recent(10).unwrap();
        // The OLDEST are dropped, so the newest survive.
        assert_eq!(recent[0].message, "event 7");
        assert_eq!(recent[2].message, "event 9");
    }

    #[test]
    fn a_recent_limit_returns_the_newest_entries() {
        let sink = MemorySink::new(100);
        for index in 0..20 {
            sink.write(&LogEvent::info(Category::App, format!("event {index}")))
                .unwrap();
        }
        let recent = sink.recent(5).unwrap();
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0].message, "event 15");
        assert_eq!(recent[4].message, "event 19");
    }

    #[test]
    fn a_snapshot_filters_by_level_category_and_search() {
        let sink = MemorySink::new(100);
        sink.write(&LogEvent::info(Category::Save, "saved a.md")).unwrap();
        sink.write(
            &LogEvent::error(Category::Deploy, "cloudflare failed")
                .with_field("target", "cf"),
        )
        .unwrap();
        sink.write(&LogEvent::warning(Category::Deploy, "edgeone skipped"))
            .unwrap();

        let snapshot = sink.snapshot();
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot.at_level(Level::Error).len(), 1);
        assert_eq!(snapshot.in_category(Category::Deploy).len(), 2);
        assert_eq!(snapshot.matching("cloudflare").len(), 1);
        assert_eq!(snapshot.matching("skipped").len(), 1);
        assert_eq!(snapshot.matching("").len(), 3);
    }

    #[test]
    fn a_secret_never_reaches_a_sink() {
        // The end-to-end form of handoff.md section 18.1: the type redacts, so the sink cannot see it.
        let sink = MemorySink::new(10);
        sink.write(
            &LogEvent::info(Category::Ai, format!("calling with {ANTHROPIC_KEY}"))
                .with_field("api_key", ANTHROPIC_KEY)
                .with_field("note", format!("the key {ANTHROPIC_KEY} was rejected")),
        )
        .unwrap();

        let rendered = sink.snapshot().render();
        assert!(
            !rendered.contains(ANTHROPIC_KEY),
            "the log leaked the key: {rendered}"
        );
        assert!(rendered.contains("REDACTED"), "{rendered}");
    }

    #[test]
    fn a_file_sink_writes_a_real_file_and_reads_it_back() {
        let dir = TempDir::new().unwrap();
        let sink = FileSink::open(dir.path(), RotationPolicy::unbounded(), "2026-01-01T12:00:00Z")
            .unwrap();

        sink.write(&LogEvent::info(Category::App, "opened")).unwrap();
        sink.write(&LogEvent::error(Category::Build, "failed").with_field("target", "site"))
            .unwrap();
        sink.flush().unwrap();

        let path = sink.current_path();
        assert!(path.is_file(), "the file was created at {}", path.display());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[info] app: opened"), "{text}");
        assert!(text.contains("target=site"), "{text}");

        let recent = sink.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "opened");
        assert_eq!(recent[1].level, Level::Error);
    }

    #[test]
    fn a_file_sink_rotates_when_the_file_passes_the_limit_and_keeps_a_bounded_count() {
        // handoff.md section 18.1: rotation so the log cannot grow without bound.
        let dir = TempDir::new().unwrap();
        // A tiny limit, so the test drives rotation rather than asserting a constant exists.
        let policy = RotationPolicy {
            max_bytes: 200,
            max_files: 2,
        };
        let sink = FileSink::open(dir.path(), policy, "2026-01-01T12:00:00Z").unwrap();

        for index in 0..60 {
            sink.write(&LogEvent::info(Category::App, format!("event number {index}")))
                .unwrap();
        }

        let mut files: Vec<String> = std::fs::read_dir(sink.directory())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        files.sort();
        assert!(
            files.len() <= policy.max_files + 1,
            "at most the current file plus the kept count: {files:?}"
        );
        assert!(
            files.iter().any(|name| name.contains(".1.log")),
            "a rotated file exists: {files:?}"
        );
        // The newest events are still readable through the sink.
        let recent = sink.recent(5).unwrap();
        assert_eq!(recent.last().unwrap().message, "event number 59");
    }

    #[test]
    fn a_file_sink_starts_a_new_file_when_the_day_changes() {
        // Appending today's events to yesterday's file would make the file name a lie.
        //
        // The clock is injected, so the rollover is exercised for real rather than by poking the
        // sink's private fields, which would have tested nothing about the production path.
        let dir = TempDir::new().unwrap();
        let clock = std::sync::Arc::new(MovableClock::at("2026-01-01T23:59:59Z"));
        let sink = FileSink::with_clock(
            dir.path(),
            RotationPolicy::unbounded(),
            "2026-01-01T23:59:59Z",
            Box::new(SharedClock(clock.clone())),
        )
        .unwrap();

        sink.write(&LogEvent::info(Category::App, "before midnight"))
            .unwrap();
        assert!(sink
            .current_path()
            .to_string_lossy()
            .ends_with("veloct-2026-01-01.log"));

        clock.move_to("2026-01-02T00:00:01Z");
        sink.write(&LogEvent::info(Category::App, "after midnight"))
            .unwrap();
        assert!(sink
            .current_path()
            .to_string_lossy()
            .ends_with("veloct-2026-01-02.log"));

        let files = sink.files().unwrap();
        assert_eq!(files.len(), 2, "{files:?}");
        // Both events are readable, from their own days.
        let recent = sink.recent(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].message, "before midnight");
        assert_eq!(recent[1].message, "after midnight");
    }

    /// A clock a test can move.
    ///
    /// `Clock` requires `Debug`, so both test clocks derive it: a trait bound is not an obstacle to
    /// work around, it is a statement that a clock can appear in a diagnostic.
    #[derive(Debug)]
    struct MovableClock(std::sync::Mutex<time::OffsetDateTime>);

    impl MovableClock {
        fn at(timestamp: &str) -> Self {
            Self(std::sync::Mutex::new(
                time::OffsetDateTime::parse(
                    timestamp,
                    &time::format_description::well_known::Rfc3339,
                )
                .unwrap(),
            ))
        }

        fn move_to(&self, timestamp: &str) {
            *self.0.lock().unwrap() = time::OffsetDateTime::parse(
                timestamp,
                &time::format_description::well_known::Rfc3339,
            )
            .unwrap();
        }
    }

    /// Forwards to a shared movable clock.
    #[derive(Debug)]
    struct SharedClock(std::sync::Arc<MovableClock>);

    impl Clock for SharedClock {
        fn now(&self) -> time::OffsetDateTime {
            *self.0 .0.lock().unwrap()
        }
    }

    #[test]
    fn pruning_keeps_the_newest_log_files() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        for day in 1..=10 {
            std::fs::write(dir.path().join(log_file_name(&format!("2026-01-{day:02}"))), "x").unwrap();
        }
        // A non-log file must not be touched.
        std::fs::write(dir.path().join("notes.txt"), "keep me").unwrap();

        let removed = prune_old_logs(&dir.path().join("."), 3).unwrap();
        assert_eq!(removed, 7, "ten files, three kept");
        assert!(dir.path().join("notes.txt").is_file(), "a non-log file survives");

        let remaining = prune_old_logs(&dir.path().join("."), 10).unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn pruning_a_directory_that_does_not_exist_is_not_an_error() {
        let dir = TempDir::new().unwrap();
        assert_eq!(prune_old_logs(&dir.path().join("absent"), 3).unwrap(), 0);
    }

    #[test]
    fn a_rotated_name_inserts_the_index_before_the_extension() {
        assert_eq!(rotated_name("veloct-2026-01-01.log", 1), "veloct-2026-01-01.1.log");
        assert_eq!(rotated_name("veloct-2026-01-01.log", 4), "veloct-2026-01-01.4.log");
        assert_eq!(rotated_name("noextension", 2), "noextension.2");
    }

    #[test]
    fn a_file_sink_lists_its_files_newest_first() {
        let dir = TempDir::new().unwrap();
        // The sink's file belongs to 2026-01-01, so a hand-written 2026-01-02 file must sort first.
        let sink = FileSink::with_clock(
            dir.path(),
            RotationPolicy::unbounded(),
            "2026-01-01T12:00:00Z",
            Box::new(SharedClock(std::sync::Arc::new(MovableClock::at(
                "2026-01-01T12:00:00Z",
            )))),
        )
        .unwrap();
        sink.write(&LogEvent::info(Category::App, "x")).unwrap();
        let logs = dir.path().join(".veloct").join("logs");
        std::fs::write(logs.join(log_file_name("2026-01-02")), "y").unwrap();

        let files = sink.files().unwrap();
        assert_eq!(files.len(), 2, "{files:?}");
        assert!(files[0].to_string_lossy().contains("2026-01-02"), "{files:?}");
    }

    #[test]
    fn a_file_sink_reports_the_directory_and_the_written_count() {
        let dir = TempDir::new().unwrap();
        let sink = FileSink::open(dir.path(), RotationPolicy::unbounded(), "2026-01-01T12:00:00Z")
            .unwrap();
        assert!(sink.directory().ends_with("logs"));
        sink.write(&LogEvent::info(Category::App, "x")).unwrap();
        assert!(sink.written.load(std::sync::atomic::Ordering::SeqCst) > 0);
    }

    #[test]
    fn a_writer_sink_reports_whether_a_line_would_rotate() {
        let buffer: Arc<std::sync::Mutex<Vec<u8>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = WriterSink::new(SharedBuffer(buffer), RotationPolicy { max_bytes: 10, max_files: 1 });
        // Nothing written yet, so the first record never rotates.
        assert!(!sink.would_rotate(100));
        sink.write(&LogEvent::info(Category::App, "aa")).unwrap();
        assert!(sink.would_rotate(100), "the buffer is past the limit");
    }
}
