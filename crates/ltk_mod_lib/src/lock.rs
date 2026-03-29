use std::fs::{self, File};

use camino::{Utf8Path, Utf8PathBuf};
use fs2::FileExt;

use crate::error::{LibraryError, LibraryResult};

/// Advisory file lock for the mod storage directory.
///
/// Acquired before any write to `library.json`, released on drop.
/// Non-blocking: if the lock is held by another process, fails immediately
/// with a clear error.
#[derive(Debug)]
pub struct StorageLock {
    _file: File,
    path: Utf8PathBuf,
}

impl StorageLock {
    /// Acquire an exclusive lock on the storage directory.
    ///
    /// Creates `library.lock` in the storage root. Returns an error if
    /// another process holds the lock.
    pub fn acquire(storage_dir: &Utf8Path) -> LibraryResult<Self> {
        fs::create_dir_all(storage_dir.as_std_path())?;
        let path = storage_dir.join("library.lock");
        let file = File::create(path.as_std_path())?;

        file.try_lock_exclusive()
            .map_err(|_| LibraryError::StorageLocked)?;

        Ok(Self { _file: file, path })
    }
}

impl Drop for StorageLock {
    fn drop(&mut self) {
        if let Err(e) = self._file.unlock() {
            tracing::warn!("Failed to release storage lock at {}: {}", self.path, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_lock_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        let _lock = StorageLock::acquire(&dir_path).unwrap();
        assert!(dir_path.join("library.lock").exists());
    }

    #[test]
    fn acquire_lock_creates_directory() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(dir.path().join("nested").join("dir")).unwrap();

        let _lock = StorageLock::acquire(&dir_path).unwrap();
        assert!(dir_path.join("library.lock").exists());
    }

    #[test]
    fn lock_released_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        {
            let _lock = StorageLock::acquire(&dir_path).unwrap();
        }

        // Should be able to acquire again after drop
        let _lock2 = StorageLock::acquire(&dir_path).unwrap();
    }

    #[test]
    fn double_lock_same_process_fails() {
        let dir = tempfile::tempdir().unwrap();
        let dir_path = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();

        let _lock1 = StorageLock::acquire(&dir_path).unwrap();
        let result = StorageLock::acquire(&dir_path);
        assert!(matches!(result.unwrap_err(), LibraryError::StorageLocked));
    }
}
