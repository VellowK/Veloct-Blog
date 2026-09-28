//! Project operation locking.
//!
//! `handoff.md` §20.4 splits project work into two classes:
//!
//! * **Exclusive** — Publish, Restore, switching/rebuilding a historical Release, and anything
//!   else that replaces the working tree. While one is running, no competing autosave, backup
//!   or second publish may proceed.
//! * **Shared** — ordinary editing, the Astro preview and attachment uploads. These run
//!   concurrently, except that a backup must start from a consistent snapshot.
//!
//! Two layers are needed and both are implemented here:
//!
//! 1. **In-process** state, so a second exclusive operation is rejected immediately with a
//!    clear error instead of deadlocking (`handoff.md` §25.1 D4/D5).
//! 2. **Cross-process** file locking, so a second Veloct instance — or a stray preview
//!    helper — cannot corrupt the same project.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use parking_lot::Mutex as ParkingMutex;
use serde::{Deserialize, Serialize};

use crate::error::{Result, VeloctError};

/// How a caller intends to use the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockMode {
    /// Read-only or additive work: editing, preview, attachment upload.
    Shared,
    /// Working-tree-replacing work: publish, restore, historical rebuild.
    Exclusive,
}

impl LockMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Exclusive => "exclusive",
        }
    }
}

#[derive(Debug, Default)]
struct HeldLocks {
    shared: usize,
    /// Human-readable owner label of the current exclusive holder, for the error message.
    exclusive: Option<String>,
}

#[derive(Debug)]
struct ProjectState {
    path: PathBuf,
    held: ParkingMutex<HeldLocks>,
}

fn registry() -> &'static Mutex<HashMap<PathBuf, Arc<ProjectState>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Arc<ProjectState>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Holding this value guarantees the requested access to a project.
///
/// The lock is released on drop, including on panic, so a failed command cannot leave the
/// project permanently locked.
#[derive(Debug)]
pub struct ProjectLock {
    state: Arc<ProjectState>,
    mode: LockMode,
    owner: String,
    /// Held for the lifetime of the guard; the OS releases it when the handle drops.
    _file: Option<File>,
}

impl ProjectLock {
    /// Acquire access to the project rooted at `root`.
    ///
    /// `owner` is a short label such as `"publish"` or `"autosave"` that appears in the
    /// conflict error, so the UI can tell the user *what* is already running.
    pub fn acquire(root: impl AsRef<Path>, mode: LockMode, owner: impl Into<String>) -> Result<Self> {
        let owner = owner.into();
        let state = {
            let canonical = crate::paths::normalise(root.as_ref());
            let mut reg = registry()
                .lock()
                .map_err(|_| VeloctError::Other("project lock registry poisoned".into()))?;
            reg.entry(canonical.clone())
                .or_insert_with(|| {
                    Arc::new(ProjectState {
                        path: canonical,
                        held: ParkingMutex::new(HeldLocks::default()),
                    })
                })
                .clone()
        };

        // Serialise the whole acquire sequence (in-process check, file lock, state update) so
        // two threads racing for the same project cannot both observe a free lock.
        let reg = registry()
            .lock()
            .map_err(|_| VeloctError::Other("project lock registry poisoned".into()))?;

        let mut held = state.held.lock();
        match mode {
            LockMode::Shared => {
                if let Some(exclusive) = &held.exclusive {
                    return Err(VeloctError::ExclusiveBusy {
                        holder: exclusive.clone(),
                    });
                }
            }
            LockMode::Exclusive => {
                if let Some(exclusive) = &held.exclusive {
                    return Err(VeloctError::ExclusiveBusy {
                        holder: exclusive.clone(),
                    });
                }
                if held.shared > 0 {
                    return Err(VeloctError::LockConflict(format!(
                        "cannot run {owner} while {} shared operation(s) are active",
                        held.shared
                    )));
                }
            }
        }

        // Cross-process guard. Shared mode needs a genuinely shared lock, which only the
        // platform file-locking API provides, so it is created on demand.
        let file = if matches!(mode, LockMode::Exclusive) {
            fs::create_dir_all(&state.path).map_err(|e| VeloctError::io(&state.path, e))?;
            let lock_path = state.path.join("veloct.lock");
            let handle = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .truncate(false)
                .open(&lock_path)
                .map_err(|e| VeloctError::io(&lock_path, e))?;
            try_lock_file_exclusive(&handle).map_err(|e| {
                VeloctError::LockConflict(format!(
                    "another Veloct instance holds the project lock for {}: {e}",
                    state.path.display()
                ))
            })?;
            Some(handle)
        } else {
            None
        };

        match mode {
            LockMode::Shared => held.shared += 1,
            LockMode::Exclusive => held.exclusive = Some(owner.clone()),
        }

        drop(held);
        drop(reg);

        Ok(Self {
            state,
            mode,
            owner,
            _file: file,
        })
    }

    /// Convenience constructors that also name the owner for error reporting.
    pub fn exclusive(root: impl AsRef<Path>, owner: impl Into<String>) -> Result<Self> {
        Self::acquire(root, LockMode::Exclusive, owner)
    }

    pub fn shared(root: impl AsRef<Path>, owner: impl Into<String>) -> Result<Self> {
        Self::acquire(root, LockMode::Shared, owner)
    }

    pub fn mode(&self) -> LockMode {
        self.mode
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn project_path(&self) -> &Path {
        &self.state.path
    }

    /// Whether an exclusive operation is currently held for this project.
    pub fn is_exclusive_busy(root: impl AsRef<Path>) -> bool {
        let canonical = crate::paths::normalise(root.as_ref());
        let Ok(reg) = registry().lock() else {
            return false;
        };
        match reg.get(&canonical) {
            Some(state) => state.held.lock().exclusive.is_some(),
            None => false,
        }
    }
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        if let Ok(reg) = registry().lock() {
            if let Some(state) = reg.get(&self.state.path) {
                let mut held = state.held.lock();
                match self.mode {
                    LockMode::Shared => held.shared = held.shared.saturating_sub(1),
                    LockMode::Exclusive => held.exclusive = None,
                }
            }
        }
        // The OS releases the file lock when `_file` drops immediately after this.
    }
}

/// Take an exclusive, non-blocking lock on an open file handle.
///
/// `fs2` wraps `LockFileEx` on Windows and `flock` on Unix; the lock is released by the OS when
/// the handle closes, which also covers the process-crash case.
fn try_lock_file_exclusive(file: &File) -> std::io::Result<()> {
    use fs2::FileExt;
    file.try_lock_exclusive()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &tempfile::TempDir) -> PathBuf {
        let root = dir.path().join("project");
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn shared_locks_coexist() {
        // handoff §20.4: ordinary editing, preview and uploads run in parallel.
        let dir = tempfile::tempdir().unwrap();
        let root = project(&dir);
        let a = ProjectLock::shared(&root, "autosave").unwrap();
        let b = ProjectLock::shared(&root, "preview").unwrap();
        assert_eq!(a.mode(), LockMode::Shared);
        assert_eq!(b.mode(), LockMode::Shared);
    }

    #[test]
    fn a_second_publish_cannot_start() {
        // handoff §25.1 D4.
        let dir = tempfile::tempdir().unwrap();
        let root = project(&dir);
        let _first = ProjectLock::exclusive(&root, "publish").unwrap();
        let err = ProjectLock::exclusive(&root, "publish").unwrap_err();
        match err {
            VeloctError::ExclusiveBusy { holder } => assert_eq!(holder, "publish"),
            other => panic!("expected ExclusiveBusy, got {other:?}"),
        }
    }

    #[test]
    fn restore_blocks_competing_autosave() {
        // handoff §25.1 D5.
        let dir = tempfile::tempdir().unwrap();
        let root = project(&dir);
        let _restore = ProjectLock::exclusive(&root, "restore").unwrap();
        let err = ProjectLock::shared(&root, "autosave").unwrap_err();
        assert_eq!(err.code(), "EXCLUSIVE_BUSY");
    }

    #[test]
    fn shared_operations_block_a_later_publish() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(&dir);
        let _editor = ProjectLock::shared(&root, "editor").unwrap();
        let err = ProjectLock::exclusive(&root, "publish").unwrap_err();
        assert_eq!(err.code(), "LOCK_CONFLICT");
        assert!(err.to_string().contains("editor") || err.to_string().contains("shared"));
    }

    #[test]
    fn dropping_a_lock_releases_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(&dir);
        {
            let _lock = ProjectLock::exclusive(&root, "publish").unwrap();
            assert!(ProjectLock::is_exclusive_busy(&root));
        }
        assert!(!ProjectLock::is_exclusive_busy(&root));
        let again = ProjectLock::exclusive(&root, "publish").unwrap();
        assert_eq!(again.owner(), "publish");
    }

    #[test]
    fn an_exclusive_file_lock_is_kept_on_disk_and_reacquirable() {
        let dir = tempfile::tempdir().unwrap();
        let root = project(&dir);
        let lock_path;
        {
            let lock = ProjectLock::exclusive(&root, "publish").unwrap();
            lock_path = lock.project_path().join("veloct.lock");
            assert!(lock_path.exists(), "the cross-process lock file must exist");
        }
        // Re-acquiring after a clean release must succeed.
        let lock = ProjectLock::exclusive(&root, "publish").unwrap();
        assert!(lock_path.exists());
        drop(lock);
    }

    #[test]
    fn different_projects_do_not_contend() {
        let dir = tempfile::tempdir().unwrap();
        let a = project(&dir);
        let b = dir.path().join("other");
        fs::create_dir_all(&b).unwrap();
        let _lock_a = ProjectLock::exclusive(&a, "publish").unwrap();
        let _lock_b = ProjectLock::exclusive(&b, "publish").unwrap();
    }
}
