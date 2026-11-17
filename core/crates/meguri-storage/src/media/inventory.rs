//! Read-only folder inventory for the provider's promised-media accounting.
use super::{
    MediaEntry, MediaError, MediaStore, Result,
    case_fold::{ignored, normalized},
    folder::regular,
};
use meguri_text::normalize::nfc;
use std::{
    fmt, fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    sync::atomic::Ordering,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnsupportedMedia {
    InvalidName,
    NotRegularFile,
    Empty,
    OverLimit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InventoryFile {
    pub name: String,
    pub lossy_name: bool,
    pub size: u64,
    pub unsupported: Option<UnsupportedMedia>,
    mtime_secs: i64,
    mtime_nanos: i64,
}

impl InventoryFile {
    /// True only for a supported file whose exact name and observed file
    /// facts equal the journal entry. Modification times have no ordering.
    pub fn matches_journal(&self, entry: &MediaEntry) -> bool {
        self.unsupported.is_none()
            && entry.name == self.name
            && entry.live
            && entry.size == self.size
            && entry.mtime_secs == self.mtime_secs
            && i64::from(entry.mtime_nanos) == self.mtime_nanos
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct MediaInventory {
    files: Vec<InventoryFile>,
    ignored: u64,
    max_entry_bytes: u64,
    media_dir: (u64, u64),
}

impl fmt::Debug for MediaInventory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaInventory")
            .field("files", &self.files.len())
            .field("excluded", &self.files.iter().filter(|file| file.unsupported.is_some()).count())
            .field("ignored", &self.ignored)
            .finish()
    }
}

impl MediaInventory {
    pub fn files(&self) -> &[InventoryFile] {
        &self.files
    }

    pub fn ignored(&self) -> u64 {
        self.ignored
    }

    pub fn max_entry_bytes(&self) -> u64 {
        self.max_entry_bytes
    }

    /// A stat failure or a different directory identity invalidates the listing.
    pub fn is_from(&self, store: &MediaStore) -> bool {
        fs::metadata(store.media_dir()).is_ok_and(|m| (m.dev(), m.ino()) == self.media_dir)
    }
}

impl MediaStore {
    /// Lists the live media folder without recovering, hashing or changing any
    /// files or journal state. No journal guard spans a callback or listing.
    pub fn promised_inventory(
        &self,
        max_entry_bytes: u64,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<MediaInventory> {
        if self.inner.importing.load(Ordering::Acquire) {
            return Err(MediaError::InvalidManifest(
                "collection media replacement owns the store".into(),
            ));
        }
        drop(self.import_db()?);
        if self.inner.importing.load(Ordering::Acquire) {
            return Err(MediaError::InvalidManifest(
                "collection media replacement owns the store".into(),
            ));
        }
        if !keep_going() {
            return Err(MediaError::Cancelled);
        }
        let directory = fs::metadata(self.media_dir())?;
        let media_dir = (directory.dev(), directory.ino());
        let mut ignored_count = 0;
        // Disk spellings exist only during classification and sorting. They are
        // never retained in the sealed inventory or included in diagnostics.
        let mut listed = Vec::new();
        for entry in fs::read_dir(self.media_dir())? {
            if !keep_going() {
                return Err(MediaError::Cancelled);
            }
            let entry = entry?;
            let disk = entry.file_name();
            let disk_bytes = disk.as_bytes().to_vec();
            let Some(disk_name) = disk.to_str() else {
                listed.push((InventoryFile {
                    name: disk.to_string_lossy().into_owned(),
                    lossy_name: true,
                    size: 0,
                    unsupported: Some(UnsupportedMedia::InvalidName),
                    mtime_secs: 0,
                    mtime_nanos: 0,
                }, disk_bytes, false));
                continue;
            };
            let name = nfc(disk_name).into_owned();
            if ignored(&name) {
                ignored_count += 1;
                continue;
            }
            let canonical = disk_name == name;
            let metadata = regular(&entry.path())?;
            let (size, mtime_secs, mtime_nanos) = metadata.as_ref()
                .map(|m| (m.len(), m.mtime(), m.mtime_nsec()))
                .unwrap_or((0, 0, 0));
            let unsupported = if !normalized(&name) {
                Some(UnsupportedMedia::InvalidName)
            } else if metadata.is_none() {
                Some(UnsupportedMedia::NotRegularFile)
            } else if size == 0 {
                Some(UnsupportedMedia::Empty)
            } else if size > max_entry_bytes {
                Some(UnsupportedMedia::OverLimit)
            } else {
                None
            };
            listed.push((InventoryFile {
                name,
                lossy_name: false,
                size,
                unsupported,
                mtime_secs,
                mtime_nanos,
            }, disk_bytes, canonical));
        }
        listed.sort_by(|a, b| a.0.name.cmp(&b.0.name)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.1.cmp(&b.1)));
        let mut start = 0;
        while start < listed.len() {
            let mut end = start + 1;
            while end < listed.len() && listed[end].0.name == listed[start].0.name {
                end += 1;
            }
            if end - start > 1 {
                for (file, _, canonical) in &mut listed[start..end] {
                    if !*canonical {
                        file.unsupported = Some(UnsupportedMedia::InvalidName);
                    }
                }
            }
            start = end;
        }
        Ok(MediaInventory {
            files: listed.into_iter().map(|(file, _, _)| file).collect(),
            ignored: ignored_count,
            max_entry_bytes,
            media_dir,
        })
    }
}
