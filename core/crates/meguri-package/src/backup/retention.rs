//! Pure anchor walk and best-effort deletion after durable publication (B5).
use super::naming::{BackupKind, WallTime, parse_name};
use crate::export_capture::{open_at, stat_at, visit};
use meguri_engine::backup_limits::BackupLimits;
use std::{
    collections::BTreeSet,
    ffi::{CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

#[derive(Debug, Default, PartialEq, Eq)]
pub struct RetentionPlan {
    pub keep: Vec<String>,
    pub delete: Vec<String>,
}

/// Unknown names and recovery copies never enter the walk. Output is newest first.
pub fn plan(today: i64, names: &[String], limits: BackupLimits, protected: &BTreeSet<String>) -> RetentionPlan {
    let mut backups: Vec<_> = names
        .iter()
        .filter_map(|name| {
            let (BackupKind::Backup, wall) = parse_name(name)? else {
                return None;
            };
            Some((name, wall))
        })
        .collect();
    backups.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    backups.dedup_by(|a, b| a.0 == b.0);
    let mut remaining = [limits.daily, limits.weekly, limits.monthly];
    let mut anchor: Option<WallTime> = None;
    let mut out = RetentionPlan::default();
    for (name, wall) in backups {
        let retained = if wall.day_number() >= today.saturating_sub(1) {
            anchor = Some(wall);
            true
        } else if let Some(stage) = remaining.iter().position(|n| *n > 0) {
            let period = |wall: WallTime| match stage {
                0 => wall.day_number(),
                // 1970-01-01 was Thursday: Monday is day -3.
                1 => (wall.day_number() + 3).div_euclid(7),
                _ => i64::from(wall.year) * 12 + i64::from(wall.month),
            };
            if anchor.is_none_or(|anchor| period(wall) < period(anchor)) {
                remaining[stage] -= 1;
                anchor = Some(wall);
                true
            } else {
                false
            }
        } else {
            false
        };
        if retained || protected.contains(name) {
            out.keep.push(name.clone());
        } else {
            out.delete.push(name.clone());
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetentionFailure {
    pub name: Option<String>,
    pub detail: String,
}
#[derive(Debug, Default)]
pub struct RetentionReport {
    pub deleted_names: Vec<String>,
    pub failures: Vec<RetentionFailure>,
}
impl RetentionReport {
    fn failed(&mut self, name: Option<String>, error: impl std::fmt::Display) {
        tracing::warn!(name = ?name, error = %error, "backup retention skipped");
        self.failures.push(RetentionFailure {
            name,
            detail: error.to_string(),
        });
    }
}

/// Retention authority captured before publication, never reacquired afterwards.
pub(crate) struct PinnedDirectory {
    parent_path: PathBuf,
    parent: File,
    name: OsString,
    root: File,
}
fn identity(metadata: &fs::Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}
impl PinnedDirectory {
    pub(crate) fn open(directory: &Path) -> io::Result<Self> {
        let directory = if directory.is_absolute() {
            directory.to_owned()
        } else {
            std::env::current_dir()?.join(directory)
        };
        let name = directory
            .file_name()
            .ok_or_else(|| io::Error::other("backup root has no name"))?
            .to_owned();
        let parent_path = directory
            .parent()
            .ok_or_else(|| io::Error::other("backup root has no parent"))?
            .to_owned();
        // Ancestors may use platform aliases (e.g. macOS /var). Pin their
        // resolved parent identity; all entry access stays relative to root.
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&parent_path)?;
        let root = open_at(&parent, &name, true)?;
        let pinned = Self {
            parent_path,
            parent,
            name,
            root,
        };
        pinned.validate()?;
        Ok(pinned)
    }

    fn validate(&self) -> io::Result<()> {
        let entry = stat_at(&self.parent, &self.name)?;
        if identity(&fs::metadata(&self.parent_path)?) != identity(&self.parent.metadata()?)
            || !entry.is_dir()
            || entry.identity() != identity(&self.root.metadata()?)
        {
            return Err(io::Error::other("backup directory binding changed; retention skipped"));
        }
        Ok(())
    }

    fn unlink(&self, name: &str) -> io::Result<()> {
        let name = CString::new(name).map_err(|_| io::Error::other("invalid backup name"))?;
        // SAFETY: root is owned/live, name is terminated, flags forbid removing
        // directories. unlinkat never follows a leaf link or resolves ancestors.
        if unsafe { libc::unlinkat(self.root.as_raw_fd(), name.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

pub fn thin(directory: &Path, today: i64, limits: BackupLimits, protected: &BTreeSet<String>) -> RetentionReport {
    thin_pinned(PinnedDirectory::open(directory), today, limits, protected)
}

pub(crate) fn thin_pinned(
    directory: io::Result<PinnedDirectory>,
    today: i64,
    limits: BackupLimits,
    protected: &BTreeSet<String>,
) -> RetentionReport {
    let mut report = RetentionReport::default();
    let directory = match directory {
        Ok(directory) => directory,
        Err(error) => {
            report.failed(None, error);
            return report;
        }
    };
    if let Err(error) = directory.validate() {
        report.failed(None, error);
        return report;
    }
    let mut names = Vec::new();
    // Reuse the owned descriptor enumerator, tolerating per-entry stat failures.
    let scanned = visit(&directory.root, |entry| {
        let Some(name) = entry.to_str().map(str::to_owned) else {
            return Ok(());
        };
        if !matches!(parse_name(&name), Some((BackupKind::Backup, _))) {
            return Ok(());
        }
        match stat_at(&directory.root, &entry) {
            Ok(metadata) if metadata.is_file() => names.push(name),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => report.failed(Some(name), e),
        }
        Ok(())
    });
    if let Err(error) = scanned {
        report.failed(None, error);
    }
    for name in plan(today, &names, limits, protected).delete {
        // Recheck the namespace and type before every descriptor-relative unlink.
        // Even a subsequent ancestor/root swap cannot redirect this descriptor.
        if let Err(error) = directory.validate() {
            report.failed(None, error);
            break;
        }
        let result = match stat_at(&directory.root, OsStr::new(&name)) {
            Ok(metadata) if metadata.is_file() => directory.unlink(&name),
            Ok(_) => continue,
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => report.deleted_names.push(name),
            Err(e) if e.kind() == io::ErrorKind::NotFound => report.deleted_names.push(name),
            Err(e) => report.failed(Some(name), e),
        }
    }
    report
}
