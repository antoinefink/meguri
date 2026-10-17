//! Whether a swap changed the live collection file (`engine/lifecycle` L8).
//! A swap can fail after it changed the file (a directory sync after the
//! rename); the file is then another collection, and the generation must
//! move even though the swap returned an error.

use std::fs::{File, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The live file before a swap. Taken with every connection closed.
pub(crate) struct LiveFile {
    path: PathBuf,
    before: Stamp,
    /// Keeps the old file alive, so a file put at the path cannot get its
    /// inode number.
    _held: Option<File>,
}

impl LiveFile {
    pub(crate) fn before_swap(path: &Path) -> LiveFile {
        LiveFile {
            path: path.to_owned(),
            _held: File::open(path).ok(),
            before: Stamp::of(path),
        }
    }

    /// Whether the path now holds something else. Closes the old file: do it
    /// before SQLite opens the path again, because closing any descriptor of
    /// a file drops the process's POSIX locks on it, SQLite's included.
    pub(crate) fn changed(mut self) -> bool {
        self.release_and_check()
    }

    /// Retain the original stamp across a failed reopen, but never retain a
    /// descriptor whose later close could release the reopened SQLite locks.
    pub(crate) fn release_and_check(&mut self) -> bool {
        self._held.take();
        Stamp::of(&self.path) != self.before
    }
}

/// What the path holds, without reading it: which file, its size and its
/// last change, and how much WAL it has. The change time moves on every
/// write and cannot be set back.
#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    file: Option<FileStamp>,
    /// An absent WAL counts as an empty one: the swap removes the live WAL,
    /// which the writer's close left empty or deleted.
    wal_len: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl FileStamp {
    fn of(metadata: &Metadata) -> FileStamp {
        FileStamp {
            dev: metadata.dev(),
            ino: metadata.ino(),
            len: metadata.len(),
            mtime: (metadata.mtime(), metadata.mtime_nsec()),
            ctime: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

impl Stamp {
    fn of(path: &Path) -> Stamp {
        let mut wal = path.as_os_str().to_owned();
        wal.push("-wal");
        Stamp {
            file: std::fs::metadata(path).ok().map(|m| FileStamp::of(&m)),
            wal_len: std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0),
        }
    }
}
