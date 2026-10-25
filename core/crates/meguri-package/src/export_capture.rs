//! E7 source authority. Read the folder itself; never consult or mutate the journal.
use crate::{CollectionPackageWriteContext, Error, Result, cancelled, writer};
use sha1::{Digest as Sha1Digest, Sha1};
use std::{
    collections::BTreeSet,
    ffi::{CStr, CString, OsStr, OsString},
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
};

pub(crate) const MAP_LIMIT: u64 = 64 * 1024 * 1024;
pub(crate) const COLLECTION_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
pub(crate) const TOTAL_LIMIT: u64 = 16 * 1024 * 1024 * 1024;
const FIXED_RESERVE: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    length: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}
impl Stamp {
    fn of(m: &Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            length: m.len(),
            mtime: (m.mtime(), m.mtime_nsec()),
            ctime: (m.ctime(), m.ctime_nsec()),
        }
    }
}

pub(crate) struct MediaFile {
    original: OsString,
    // Usually no second name allocation, including all non-Apple exports.
    normalized: Option<String>,
    stamp: Stamp,
    pub(crate) sha1: [u8; 20],
}
impl MediaFile {
    pub(crate) fn name(&self) -> &str {
        self.normalized
            .as_deref()
            .unwrap_or_else(|| self.original.to_str().expect("validated UTF-8"))
    }
    pub(crate) fn length(&self) -> u32 {
        self.stamp.length as u32
    }
}

/// Owned, read-only source authority. It holds no engine or media-store lock.
/// Delivery must revalidate this *after* hashing the completed package.
pub struct ExportSourceValidation {
    directory: PathBuf,
    parent: File,
    root: Option<File>,
    pub(crate) files: Vec<MediaFile>,
    // Non-media entries are inventoried too, without retaining an unbounded index.
    excluded: InventoryHash,
    protected: Vec<PathBuf>,
}
#[derive(Default, PartialEq, Eq)]
struct InventoryHash {
    count: u64,
    sum: [u8; 32],
}
impl InventoryHash {
    fn add(&mut self, name: &OsStr, kind: u8, m: Stamp) -> Result<()> {
        use sha2::{Digest as Sha256Digest, Sha256};
        let mut hash = Sha256::new();
        hash.update(name.as_bytes());
        hash.update([0, kind]);
        hash.update(m.dev.to_le_bytes());
        hash.update(m.ino.to_le_bytes());
        let bytes: [u8; 32] = hash.finalize().into();
        // An order-independent multiset fingerprint; file authority below uses
        // exact names/stamps, not this summary of excluded directories/OS files.
        let mut carry = 0_u16;
        for (sum, byte) in self.sum.iter_mut().zip(bytes) {
            carry += u16::from(*sum) + u16::from(byte);
            *sum = carry as u8;
            carry >>= 8;
        }
        self.count = add(self.count, 1)?;
        Ok(())
    }
}
fn fresh() -> Error {
    Error::ExportSourceChanged
}
fn unsafe_entry(reason: &str) -> Error {
    Error::ExportSourceInvalid {
        detail: format!("collection.media: {reason}; correct the entry and start a fresh export"),
    }
}
fn entry_error(name: &OsStr, reason: &str) -> Error {
    let display: String = name
        .to_string_lossy()
        .chars()
        .take(80)
        .flat_map(|c| c.escape_default())
        .collect();
    unsafe_entry(&format!("entry '{display}': {reason}"))
}
pub(crate) fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(Error::ExportFormatLimit)
}
fn identity(m: &Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}

fn open_at(parent: &File, name: &OsStr, directory: bool) -> std::io::Result<File> {
    let name = CString::new(name.as_bytes()).map_err(|_| std::io::Error::other("invalid component"))?;
    let flags = libc::O_RDONLY
        | libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | libc::O_NONBLOCK
        | if directory { libc::O_DIRECTORY } else { 0 };
    // SAFETY: parent is live, name is NUL terminated, ownership transfers once.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[derive(Clone, Copy)]
struct EntryMetadata {
    stamp: Stamp,
    kind: libc::mode_t,
}
impl EntryMetadata {
    fn is_dir(self) -> bool {
        self.kind == libc::S_IFDIR
    }
    fn is_file(self) -> bool {
        self.kind == libc::S_IFREG
    }
    fn identity(self) -> (u64, u64) {
        (self.stamp.dev, self.stamp.ino)
    }
}
fn stat_at(parent: &File, name: &OsStr) -> std::io::Result<EntryMetadata> {
    let name = CString::new(name.as_bytes()).map_err(|_| std::io::Error::other("invalid component"))?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: parent is live, name is terminated, fstatat initializes stat on success.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let stat = unsafe { stat.assume_init() };
    let (mtime, ctime) = ((stat.st_mtime, stat.st_mtime_nsec), (stat.st_ctime, stat.st_ctime_nsec));
    #[cfg(target_vendor = "apple")]
    let dev = stat.st_dev as u64;
    #[cfg(not(target_vendor = "apple"))]
    let dev = stat.st_dev;
    Ok(EntryMetadata {
        stamp: Stamp {
            dev,
            ino: stat.st_ino,
            length: stat.st_size as u64,
            mtime,
            ctime,
        },
        kind: stat.st_mode & libc::S_IFMT,
    })
}
fn source_io(error: std::io::Error) -> Error {
    match error.kind() {
        std::io::ErrorKind::NotFound => fresh(),
        std::io::ErrorKind::PermissionDenied => unsafe_entry("an included item is unreadable"),
        _ if matches!(error.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR)) => {
            unsafe_entry("an item is a link or has changed type")
        }
        _ => error.into(), // Unknown I/O always refuses, without inventing a source diagnosis.
    }
}
fn open_media(root: &File, name: &OsStr) -> Result<File> {
    #[cfg(test)]
    {
        let action = BEFORE_OPEN.with(|action| action.borrow_mut().take());
        if let Some(action) = action {
            action();
        }
        if let Some(kind) = OPEN_ERROR.with(|error| error.get()) {
            return Err(source_io(std::io::Error::new(kind, "synthetic source open failure")));
        }
    }
    open_at(root, name, false).map_err(source_io)
}

#[cfg(test)]
thread_local! {
    static OPEN_ERROR: std::cell::Cell<Option<std::io::ErrorKind>> = const { std::cell::Cell::new(None) };
    static BEFORE_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

struct ProtectedInventory {
    identities: BTreeSet<(u64, u64)>,
    parent_stamp: Stamp,
    stages: Vec<(OsString, Stamp)>,
    paths: Vec<(PathBuf, Option<(u64, u64)>)>,
}
impl ProtectedInventory {
    fn validate(&self, authority: &ExportSourceValidation) -> Result<()> {
        let parent = &authority.parent;
        if Stamp::of(&parent.metadata()?) != self.parent_stamp {
            return Err(fresh());
        }
        for (name, stamp) in &self.stages {
            if stat_at(parent, name).map_err(source_io)?.stamp != *stamp {
                return Err(fresh());
            }
        }
        for (path, expected) in &self.paths {
            if authority.protected_path(path)?.map(EntryMetadata::identity) != *expected {
                return Err(fresh());
            }
        }
        Ok(())
    }
}

// readdir needs a separate open description, not dup (which shares its offset).
fn visit(root: &File, mut f: impl FnMut(OsString) -> Result<()>) -> Result<()> {
    let scan = open_at(root, OsStr::new("."), true)?;
    let fd = scan.into_raw_fd();
    // SAFETY: fd is a fresh directory descriptor; DIR owns it on success.
    let dir = unsafe { libc::fdopendir(fd) };
    if dir.is_null() {
        let error = std::io::Error::last_os_error();
        unsafe {
            libc::close(fd);
        }
        return Err(error.into());
    }
    struct Directory(*mut libc::DIR);
    impl Drop for Directory {
        fn drop(&mut self) {
            unsafe {
                libc::closedir(self.0);
            }
        }
    }
    let guard = Directory(dir);
    loop {
        // SAFETY: this thread exclusively owns DIR; errno is thread local.
        let entry = unsafe {
            #[cfg(target_vendor = "apple")]
            let errno = libc::__error();
            #[cfg(not(target_vendor = "apple"))]
            let errno = libc::__errno_location();
            *errno = 0;
            let entry = libc::readdir(guard.0);
            if entry.is_null() && *errno != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            entry
        };
        if entry.is_null() {
            break;
        }
        let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes != b"." && bytes != b".." {
            f(OsStr::from_bytes(bytes).to_owned())?;
        }
    }
    Ok(())
}
// M8 alone: Unicode simple folding of these words adds only long s to ASCII.
fn ignored(name: &str) -> bool {
    fn matches(name: &str, thumbs: bool) -> bool {
        let mut chars = name
            .chars()
            .map(|c| if c == 'ſ' { 's' } else { c.to_ascii_lowercase() });
        let mut word = ['\0'; 9];
        for slot in &mut word {
            let Some(c) = chars.next() else {
                return false;
            };
            *slot = c;
        }
        chars.next().is_none()
            && if thumbs {
                word[..6] == ['t', 'h', 'u', 'm', 'b', 's'] && word[7..] == ['d', 'b']
            } else {
                word[1..] == ['d', 's', '_', 's', 't', 'o', 'r', 'e']
            }
    }
    matches(name.strip_prefix(':').unwrap_or(name), true) || matches(name, false)
}
fn archive_name(original: &OsStr) -> Result<Option<String>> {
    let text = original
        .to_str()
        .ok_or_else(|| entry_error(original, "file name is not UTF-8"))?;
    let nfc = meguri_text::normalize::nfc(text);
    if !cfg!(target_vendor = "apple") && nfc.as_ref() != text {
        return Err(entry_error(original, "file name is not NFC"));
    }
    if nfc.is_empty() || nfc.len() > 120 || meguri_text::media_name::safe_media_name(nfc.as_ref()) != nfc.as_ref() {
        return Err(entry_error(
            original,
            "unsafe file name (use one NFC component of at most 120 UTF-8 bytes)",
        ));
    }
    Ok((nfc.as_ref() != text).then(|| nfc.into_owned()))
}

impl ExportSourceValidation {
    pub(crate) fn inventory(
        directory: &Path,
        protected: Vec<PathBuf>,
        check: &(dyn Fn() -> bool + Sync),
    ) -> Result<Self> {
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(directory)
            .map_err(source_io)?;
        let root = match open_at(&parent, OsStr::new("collection.media"), true) {
            Ok(root) => Some(root),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(source_io(error)),
        };
        let mut proof = Self {
            directory: directory.to_owned(),
            parent,
            root,
            files: Vec::new(),
            excluded: InventoryHash::default(),
            protected,
        };
        let (files, excluded) = proof.scan(check)?;
        proof.files = files;
        proof.excluded = excluded;
        proof.validate(check)?;
        Ok(proof)
    }
    pub(crate) fn protect(&mut self, paths: &[PathBuf], check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
        self.protected.extend_from_slice(paths);
        self.validate(check)
    }
    fn bound_root(&self) -> Result<()> {
        let bound_parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&self.directory)
            .map_err(source_io)?;
        if identity(&bound_parent.metadata()?) != identity(&self.parent.metadata()?) {
            return Err(fresh());
        }
        let bound = open_at(&self.parent, OsStr::new("collection.media"), true);
        match (&self.root, bound) {
            (None, Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            (Some(root), Ok(bound)) if identity(&root.metadata()?) == identity(&bound.metadata()?) => Ok(()),
            (_, Err(error)) => Err(source_io(error)),
            _ => Err(fresh()),
        }
    }
    // Resolve protected path parents component by component; never follow a
    // symlink merely to discover an alias. Missing outputs have no identity yet.
    fn protected_path(&self, path: &Path) -> Result<Option<EntryMetadata>> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()?.join(path)
        };
        // Source-local identities share the already held collection authority.
        // External output paths are normalized by their admitting caller.
        let (mut parent, relative) = match absolute.strip_prefix(&self.directory) {
            Ok(relative) => (self.parent.try_clone()?, relative),
            Err(_) => (File::open("/")?, absolute.as_path()),
        };
        let mut components = relative.components().peekable();
        while let Some(component) = components.next() {
            match component {
                Component::RootDir | Component::CurDir => continue,
                Component::Normal(name) => {
                    let result = if components.peek().is_none() {
                        return match stat_at(&parent, name) {
                            Ok(meta) if meta.kind == libc::S_IFLNK => {
                                Err(unsafe_entry("protected path is a symbolic link"))
                            }
                            Ok(meta) => Ok(Some(meta)),
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                            Err(e) => Err(e.into()),
                        };
                    } else {
                        open_at(&parent, name, true)
                    };
                    match result {
                        Ok(next) => parent = next,
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                        Err(e) => return Err(e.into()),
                    }
                }
                _ => return Err(unsafe_entry("protected path has no stable authority")),
            }
        }
        Err(unsafe_entry("protected path is not a file"))
    }
    fn protected_identities(&self, check: &(dyn Fn() -> bool + Sync)) -> Result<ProtectedInventory> {
        const MAX_IDENTITIES: usize = 400_000;
        let mut identities = BTreeSet::new();
        let mut stages = Vec::new();
        let mut paths = Vec::new();
        let mut entries = 0_usize;
        let mut insert = |meta: EntryMetadata| -> Result<()> {
            if entries >= MAX_IDENTITIES {
                return Err(Error::ExportFormatLimit);
            }
            entries += 1;
            identities.insert(meta.identity());
            Ok(())
        };
        for path in &self.protected {
            cancelled(check)?;
            let meta = self.protected_path(path)?;
            paths.push((path.clone(), meta.map(EntryMetadata::identity)));
            if let Some(meta) = meta {
                insert(meta)?;
            }
        }
        let before = Stamp::of(&self.parent.metadata()?);
        visit(&self.parent, |name| {
            cancelled(check)?;
            let Some(text) = name.to_str() else {
                return Ok(());
            };
            let media_staging = text == "media.staging";
            let stage = media_staging || text.strip_prefix(".meguri-stage-").is_some_and(|s| !s.is_empty());
            let temporary = ["replace", "stage", "export", "backup", "media", "sidecar"]
                .iter()
                .any(|purpose| {
                    text.strip_prefix(&format!(".meguri-tmp-{purpose}-"))
                        .is_some_and(|s| !s.is_empty())
                })
                || (text.starts_with("meguri-sync-stage-") && text.ends_with(".tmp"))
                || (["meguri-export-", "meguri-backup-"].iter().any(|p| text.starts_with(p))
                    && text.ends_with(".partial"));
            if !stage && !temporary {
                return Ok(());
            }
            let meta = stat_at(&self.parent, &name)?;
            if meta.is_file() && !media_staging {
                return insert(meta);
            }
            if !stage || !meta.is_dir() {
                return Err(unsafe_entry("internal staging has an unsafe type"));
            }
            if stages.len() >= MAX_IDENTITIES {
                return Err(Error::ExportFormatLimit);
            }
            if media_staging {
                insert(meta)?;
            }
            let directory = open_at(&self.parent, &name, true).map_err(source_io)?;
            if Stamp::of(&directory.metadata()?) != meta.stamp {
                return Err(fresh());
            }
            visit(&directory, |child| {
                cancelled(check)?;
                let child = stat_at(&directory, &child)?;
                if child.is_file() {
                    insert(child)?;
                } else if media_staging || !child.is_dir() {
                    return Err(unsafe_entry("internal staging contains a nonregular entry"));
                }
                Ok(()) // No recursion, including into directories in a recognized stage.
            })?;
            if Stamp::of(&directory.metadata()?) != meta.stamp || stat_at(&self.parent, &name)?.stamp != meta.stamp {
                return Err(fresh());
            }
            stages.push((name, meta.stamp));
            Ok(())
        })?;
        let inventory = ProtectedInventory {
            identities,
            parent_stamp: before,
            stages,
            paths,
        };
        inventory.validate(self)?;
        Ok(inventory)
    }
    fn enumerate(
        &self,
        check: &(dyn Fn() -> bool + Sync),
        mut accept: impl FnMut(MediaFile) -> Result<()>,
    ) -> Result<InventoryHash> {
        self.bound_root()?;
        let protected = self.protected_identities(check)?;
        let mut excluded = InventoryHash::default();
        if let Some(root) = &self.root {
            let before = Stamp::of(&root.metadata()?);
            visit(root, |original| {
                cancelled(check)?;
                let meta = stat_at(root, &original).map_err(source_io)?;
                if meta.is_dir() {
                    excluded.add(&original, 1, meta.stamp)?;
                    return Ok(());
                }
                if !meta.is_file() {
                    return Err(entry_error(&original, "not a regular file or directory"));
                }
                let text = original
                    .to_str()
                    .ok_or_else(|| entry_error(&original, "file name is not UTF-8"))?;
                if ignored(text) {
                    excluded.add(&original, 2, meta.stamp)?;
                    return Ok(());
                }
                let normalized = archive_name(&original)?;
                if meta.stamp.length > u64::from(u32::MAX) {
                    return Err(Error::ExportFormatLimit);
                }
                if protected.identities.contains(&meta.identity()) {
                    return Err(entry_error(
                        &original,
                        "file aliases the live database, auxiliary state, internal staging or export output",
                    ));
                }
                let file = open_media(root, &original)?;
                if !file.metadata()?.is_file()
                    || Stamp::of(&file.metadata()?) != meta.stamp
                    || stat_at(root, &original).map_err(source_io)?.stamp != meta.stamp
                {
                    return Err(fresh());
                }
                let entry = MediaFile {
                    original,
                    normalized,
                    stamp: meta.stamp,
                    sha1: [0; 20],
                };
                accept(entry)
            })?;
            if Stamp::of(&root.metadata()?) != before {
                return Err(fresh());
            }
        }
        protected.validate(self)?;
        self.bound_root()?;
        Ok(excluded)
    }
    fn scan(&self, check: &(dyn Fn() -> bool + Sync)) -> Result<(Vec<MediaFile>, InventoryHash)> {
        let mut files = Vec::new();
        let mut map_size = 0;
        let mut bytes = 0;
        let excluded = self.enumerate(check, |entry| {
            map_size = add(
                map_size,
                crate::media_map::encoded_record_length(entry.name(), entry.length()),
            )?;
            bytes = add(bytes, u64::from(entry.length()))?;
            if files.len() >= 399_996 || map_size > MAP_LIMIT || bytes > TOTAL_LIMIT {
                return Err(Error::ExportFormatLimit);
            }
            files.push(entry);
            Ok(())
        })?;
        files.sort_unstable_by(|a, b| a.name().as_bytes().cmp(b.name().as_bytes()));
        if files.windows(2).any(|pair| pair[0].name() == pair[1].name()) {
            return Err(unsafe_entry("file names collide after NFC normalization"));
        }
        self.bound_root()?;
        Ok((files, excluded))
    }
    /// Call at the actual pre-Ready boundary, after all package I/O.
    pub fn validate(&self, check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
        cancelled(check)?;
        // One bit per known media name detects aliases/duplicates without
        // allocating a second manifest beside the source and ZIP indexes.
        let mut seen = vec![0_u8; self.files.len().div_ceil(8)];
        let mut count = 0;
        let excluded = self.enumerate(check, |entry| {
            let index = self
                .files
                .binary_search_by(|expected| expected.name().as_bytes().cmp(entry.name().as_bytes()))
                .map_err(|_| fresh())?;
            let expected = &self.files[index];
            let mask = 1 << (index % 8);
            if seen[index / 8] & mask != 0 || entry.original != expected.original || entry.stamp != expected.stamp {
                return Err(fresh());
            }
            seen[index / 8] |= mask;
            count += 1;
            Ok(())
        })?;
        if excluded != self.excluded || count != self.files.len() {
            return Err(fresh());
        }
        cancelled(check)
    }
    pub(crate) fn capture(&mut self, staging: &Path, check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
        self.validate(check)?;
        for (index, entry) in self.files.iter_mut().enumerate() {
            cancelled(check)?;
            let mut input = open_media(self.root.as_ref().expect("inventoried root"), &entry.original)?;
            if !input.metadata()?.is_file() || Stamp::of(&input.metadata()?) != entry.stamp {
                return Err(fresh());
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staging.join(index.to_string()))?;
            let mut digest = Sha1::new();
            let mut length = 0;
            let mut buffer = [0; 64 * 1024];
            loop {
                cancelled(check)?;
                let wanted = (entry.stamp.length.saturating_sub(length) + 1).min(buffer.len() as u64) as usize;
                let n = input.read(&mut buffer[..wanted]).map_err(source_io)?;
                if n == 0 {
                    break;
                }
                length = add(length, n as u64)?;
                if length > entry.stamp.length {
                    return Err(fresh());
                }
                output.write_all(&buffer[..n])?;
                digest.update(&buffer[..n]);
                meguri_model::faultpoint!("package.export.capture_chunk")?;
            }
            if length != entry.stamp.length || Stamp::of(&input.metadata()?) != entry.stamp {
                return Err(fresh());
            }
            entry.sha1 = digest.finalize().into();
            // Captures remain private; reopening writable is never part of the writer.
            let mut permissions = output.metadata()?.permissions();
            permissions.set_readonly(true);
            output.set_permissions(permissions)?;
        }
        self.validate(check)
    }
}

pub(crate) fn map_length(files: &[MediaFile]) -> Result<u64> {
    files.iter().try_fold(0, |n, f| {
        add(n, crate::media_map::encoded_record_length(f.name(), f.length()))
    })
}
/// Conservative expansion, including a frame header/checksum even for empty input.
fn frame_bound(size: u64) -> Result<u64> {
    add(add(size, size / 128)?, 1024)
}
fn limits(database: u64, count: u64, map: u64, media: u64) -> Result<()> {
    if database > COLLECTION_LIMIT || count > 399_996 || map > MAP_LIMIT {
        return Err(Error::ExportFormatLimit);
    }
    if add(add(add(database, map)?, media)?, FIXED_RESERVE)? > TOTAL_LIMIT {
        return Err(Error::ExportFormatLimit);
    }
    Ok(())
}
pub(crate) fn budgets(database: u64, files: &[MediaFile]) -> Result<(u64, u64)> {
    let map = map_length(files)?;
    if database > COLLECTION_LIMIT || files.len() > 399_996 || map > MAP_LIMIT {
        return Err(Error::ExportFormatLimit);
    }
    let mut media_bytes = 0;
    let mut capture = add(database.checked_mul(2).ok_or(Error::ExportFormatLimit)?, FIXED_RESERVE)?;
    let mut archive = add(frame_bound(database)?, FIXED_RESERVE)?;
    for file in files {
        if file.stamp.length > u64::from(u32::MAX) {
            return Err(Error::ExportFormatLimit);
        }
        media_bytes = add(media_bytes, file.stamp.length)?;
        capture = add(capture, u64::from(file.length()))?;
        archive = add(archive, frame_bound(u64::from(file.length()))?)?;
    }
    limits(database, files.len() as u64, map, media_bytes)?;
    archive = add(archive, frame_bound(map)?)?;
    archive = add(
        archive,
        (files.len() as u64 + 4)
            .checked_mul(256)
            .ok_or(Error::ExportFormatLimit)?,
    )?;
    Ok((capture, archive))
}
pub(crate) fn preflight(
    database: u64,
    files: &[MediaFile],
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
) -> Result<()> {
    let (capture, archive) = budgets(database, files)?;
    let output = writer::existing_directory(destination.parent().unwrap_or(Path::new(".")))?;
    let same = fs::metadata(context.collection_directory)?.dev() == fs::metadata(&output)?.dev();
    capacity(context, &output, capture, archive, same)
}
fn capacity(
    context: &CollectionPackageWriteContext<'_>,
    output: &Path,
    capture: u64,
    archive: u64,
    same: bool,
) -> Result<()> {
    let (capture, archive) = if same {
        let sum = add(capture, archive)?;
        (sum, sum)
    } else {
        (capture, archive)
    };
    // Query every written filesystem before any mutation (even if one is full).
    let source_available = context.space.available_bytes(context.collection_directory);
    let output_available = context.space.available_bytes(output);
    for (needed, available) in [(capture, source_available?), (archive, output_available?)] {
        if available < needed {
            return Err(Error::InsufficientSpace { needed, available });
        }
    }
    Ok(())
}

#[cfg(test)]
impl MediaFile {
    pub(crate) fn expected(name: &str, length: u32, sha1: [u8; 20]) -> Self {
        Self {
            original: name.into(),
            normalized: None,
            stamp: Stamp {
                dev: 0,
                ino: 0,
                length: u64::from(length),
                mtime: (0, 0),
                ctime: (0, 0),
            },
            sha1,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    struct OpenFailure;
    impl Drop for OpenFailure {
        fn drop(&mut self) {
            OPEN_ERROR.with(|error| error.set(None));
        }
    }
    #[test]
    fn excluded_unreadable_entries_need_no_read_open_but_included_errors_refuse() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        let child = media.join("child");
        fs::create_dir(&child).unwrap();
        fs::write(media.join(".DS_Store"), b"metadata").unwrap();
        fs::set_permissions(&child, fs::Permissions::from_mode(0o0)).unwrap();
        fs::set_permissions(media.join(".DS_Store"), fs::Permissions::from_mode(0o0)).unwrap();
        // mode 000 alone proves nothing under root. Deny every included-file
        // read via this thread's seam; successful inventory proves no such open.
        OPEN_ERROR.with(|error| error.set(Some(std::io::ErrorKind::PermissionDenied)));
        let _reset = OpenFailure;
        let proof = ExportSourceValidation::inventory(directory.path(), vec![], &|| false).unwrap();
        assert!(proof.files.is_empty());
        fs::write(media.join("included"), b"must be read").unwrap();
        assert!(matches!(
            ExportSourceValidation::inventory(directory.path(), vec![], &|| false),
            Err(Error::ExportSourceInvalid { .. })
        ));
        OPEN_ERROR.with(|error| error.set(Some(std::io::ErrorKind::Other)));
        assert!(matches!(
            ExportSourceValidation::inventory(directory.path(), vec![], &|| false),
            Err(Error::Io { .. })
        ));
        fs::set_permissions(child, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(media.join(".DS_Store"), fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[test]
    fn included_open_rejects_replacement_and_staging_rebinding() {
        use std::os::unix::fs::symlink;
        for replace_stage in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let media = directory.path().join("collection.media");
            fs::create_dir(&media).unwrap();
            let included = media.join("included");
            fs::write(&included, b"original").unwrap();
            let stage = directory.path().join(".meguri-stage-existing");
            fs::create_dir(&stage).unwrap();
            fs::write(stage.join("snapshot.anki2"), b"snapshot").unwrap();
            BEFORE_OPEN.with(|action| {
                *action.borrow_mut() = Some(Box::new(move || {
                    if replace_stage {
                        fs::rename(&stage, stage.with_extension("old")).unwrap();
                        fs::create_dir(&stage).unwrap();
                        fs::write(stage.join("snapshot.anki2"), b"replacement").unwrap();
                    } else {
                        fs::remove_file(&included).unwrap();
                        symlink(stage.join("snapshot.anki2"), &included).unwrap();
                    }
                }))
            });
            assert!(ExportSourceValidation::inventory(directory.path(), vec![], &|| false).is_err());
        }
    }
    #[test]
    fn pending_namespace_absence_is_allowed_and_rebinding_during_inventory_refuses() {
        use std::os::unix::fs::symlink;
        let absent = tempfile::tempdir().unwrap();
        let proof = ExportSourceValidation::inventory(absent.path(), vec![], &|| false).unwrap();
        proof.validate(&|| false).unwrap();
        assert!(!absent.path().join("media.staging").exists());
        for change in 0..3 {
            let directory = tempfile::tempdir().unwrap();
            let media = directory.path().join("collection.media");
            fs::create_dir(&media).unwrap();
            fs::write(media.join("included"), b"live media bytes").unwrap();
            let pending = directory.path().join("media.staging");
            let retained = directory.path().join("retained-pending");
            if change != 2 {
                fs::create_dir(&pending).unwrap();
                fs::write(pending.join("pending-file"), b"pending operation bytes").unwrap();
            }
            let pending_at_open = pending.clone();
            let retained_at_open = retained.clone();
            BEFORE_OPEN.with(|action| {
                *action.borrow_mut() = Some(Box::new(move || {
                    if change != 2 {
                        fs::rename(&pending_at_open, &retained_at_open).unwrap();
                    }
                    if change == 1 {
                        symlink(&retained_at_open, &pending_at_open).unwrap();
                    } else {
                        fs::create_dir(&pending_at_open).unwrap();
                    }
                }));
            });
            assert!(ExportSourceValidation::inventory(directory.path(), vec![], &|| false).is_err());
            assert_eq!(fs::read(media.join("included")).unwrap(), b"live media bytes");
            if change != 2 {
                assert_eq!(
                    fs::read(retained.join("pending-file")).unwrap(),
                    b"pending operation bytes"
                );
            }
            assert!(!directory.path().join("out.colpkg").exists());
        }
    }
    #[test]
    fn arithmetic_limits_without_large_allocations_or_io() {
        assert!(add(u64::MAX, 1).is_err());
        assert!(frame_bound(u64::MAX).is_err());
        assert!(limits(COLLECTION_LIMIT + 1, 0, 0, 0).is_err());
        assert!(limits(0, 399_997, 0, 0).is_err());
        assert!(limits(0, 0, MAP_LIMIT + 1, 0).is_err());
        assert!(limits(COLLECTION_LIMIT, 1, 128, TOTAL_LIMIT - COLLECTION_LIMIT).is_err());
        assert!(limits(0, 1, 128, u64::MAX).is_err());
        limits(0, 399_996, MAP_LIMIT, TOTAL_LIMIT - MAP_LIMIT - FIXED_RESERVE).unwrap();
        let file = MediaFile::expected("a", 8 * 1024 * 1024, [0; 20]);
        let (capture, archive) = budgets(1024, &[file]).unwrap();
        assert!(capture + archive > 16 * 1024 * 1024);
    }
    #[test]
    fn capacity_queries_both_volumes_before_reporting_failure_and_sums_shared_costs() {
        use meguri_model::{
            clock::{FixedClock, UtcOffset},
            rng::SeededRandom,
        };
        use meguri_storage::{StorageError, atomic_file::Purpose, space::SpaceProvider};
        struct Space(std::sync::Mutex<Vec<PathBuf>>);
        impl SpaceProvider for Space {
            fn available_bytes(&self, path: &Path) -> std::result::Result<u64, StorageError> {
                self.0.lock().unwrap().push(path.to_owned());
                Ok(if path == Path::new("capture") { 8 } else { 11 })
            }
        }
        let space = Space(std::sync::Mutex::new(Vec::new()));
        let context = CollectionPackageWriteContext {
            collection_directory: Path::new("capture"),
            purpose: Purpose::Export,
            finalisation: crate::Finalisation::ExclusiveCreate,
            clock: &FixedClock::new(0, UtcOffset::UTC),
            random: &SeededRandom::new(1),
            space: &space,
            is_cancelled: &|| false,
        };
        capacity(&context, Path::new("output"), 8, 11, false).unwrap();
        assert_eq!(
            *space.0.lock().unwrap(),
            vec![PathBuf::from("capture"), PathBuf::from("output")]
        );
        space.0.lock().unwrap().clear();
        assert!(matches!(
            capacity(&context, Path::new("output"), 8, 11, true),
            Err(Error::InsufficientSpace {
                needed: 19,
                available: 8
            })
        ));
        assert_eq!(space.0.lock().unwrap().len(), 2);
    }
    #[test]
    fn os_metadata_exclusions_are_only_m8() {
        for name in [
            "Thumbs.db",
            "THUMBS.DB",
            "thumbs_db",
            "thumbſ-db",
            ".DS_Store",
            "xds_store",
            "ſDS_STORE",
            ":Thumbs.db",
            ":thumbſ_db",
            ":ThUmBſ💾dB",
            "thumbs💾db",
            ".Dſ_ſtore",
        ] {
            assert!(ignored(name));
        }
        for name in [
            "_static",
            "empty",
            "thumbs.db.png",
            "thumbs.dbx",
            "..ds_store",
            "ds_store",
            ":.DS_Store",
            ":xds_store",
            ": .DS_Store",
        ] {
            assert!(!ignored(name));
        }
    }
}
