//! Read-only access to a profile folder and per-run copies (R1938, R1939).
//!
//! A [`Vault`] opens exactly three names in its folder: the collection
//! database [`COLLECTION`], its write-ahead log [`COLLECTION_WAL`] when
//! present, and the media folder [`MEDIA_DIR`] with the regular files
//! directly in it. It never lists the folder itself, so preferences, add-on
//! data (API keys) and the sync key next to the collection are never opened.
//! Every path it opens is recorded in [`CopyReport::access`], relative to the
//! folder.
//!
//! It refuses to work when a CI environment marker is set ([`ci_marker`]):
//! the private collection never runs on a remote machine. With
//! [`VaultOptions::require_read_only`] (the default) it also refuses a
//! folder that is not on a read-only mount, so nothing it does can write to
//! the source.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// The collection database, the only file of the folder's root besides the
/// write-ahead log that the vault opens.
pub const COLLECTION: &str = "collection.anki2";
/// The collection's write-ahead log; copied with it when present, so the copy
/// holds every committed write.
pub const COLLECTION_WAL: &str = "collection.anki2-wal";
/// The media folder. Only regular files directly inside it are copied.
pub const MEDIA_DIR: &str = "collection.media";

/// Environment variables that mark a CI or other remote build machine. One
/// set to any non-empty value refuses the vault. `tools/mg.d/private.sh`
/// checks the same list before it starts a container (its self-test compares
/// the two lists).
// ci-markers:begin
pub const CI_MARKERS: &[&str] = &[
    "CI",
    "CONTINUOUS_INTEGRATION",
    "BUILDKITE",
    "BITBUCKET_BUILD_NUMBER",
    "CIRCLECI",
    "CI_XCODE_PROJECT",
    "CODEBUILD_BUILD_ID",
    "DRONE",
    "GITHUB_ACTIONS",
    "GITLAB_CI",
    "JENKINS_URL",
    "TEAMCITY_VERSION",
    "TF_BUILD",
    "TRAVIS",
];
// ci-markers:end

/// The first CI marker among `vars` that is set to a non-empty value.
pub fn ci_marker<'a, I>(vars: I) -> Option<String>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    vars.into_iter()
        .find(|(name, value)| !value.is_empty() && CI_MARKERS.contains(name))
        .map(|(name, _)| name.to_owned())
}

/// Whether `path` is on a mount that refuses writes (`statvfs` reports
/// `ST_RDONLY`).
pub fn is_read_only_mount(path: &Path) -> io::Result<bool> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: `statvfs` only writes the struct it is given; zeroed is a valid
    // initial value for it.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c_path` is a NUL-terminated string that outlives the call, and
    // `stats` is a valid, writable `statvfs`.
    let rc = unsafe { libc::statvfs(c_path.as_ptr(), &mut stats) };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(stats.f_flag & libc::ST_RDONLY != 0)
}

/// How a [`Vault`] checks its folder.
#[derive(Clone, Debug)]
pub struct VaultOptions {
    /// The environment the CI check reads: name and value pairs.
    pub env: Vec<(String, String)>,
    /// Refuse a folder that is not on a read-only mount. `tools/mg private`
    /// always mounts it so; only tests of the copy itself turn this off.
    pub require_read_only: bool,
}

impl VaultOptions {
    /// This process's environment, a read-only mount required.
    pub fn from_process() -> VaultOptions {
        VaultOptions {
            env: std::env::vars_os()
                .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().unwrap_or_default())))
                .collect(),
            require_read_only: true,
        }
    }
}

/// Why the vault refused or failed. Messages name only the vault's own file
/// names, never a media file's name.
#[derive(Debug)]
pub enum VaultError {
    /// A CI marker is set (its name).
    CiMarker(String),
    /// The folder is not on a read-only mount.
    NotReadOnly,
    /// The folder holds no collection database.
    NoCollection,
    /// One of the allowed names is not what it must be (a symlink, a
    /// directory where a file belongs, ...).
    NotAllowed(&'static str),
    /// The destination already exists.
    DestinationExists,
    /// A media file changed while it was copied (another file, size or
    /// modification time).
    SourceChanged(&'static str),
    /// The collection or its log changed during each of `attempts` copies.
    CollectionBusy { attempts: u32 },
    /// Reading or writing failed; `what` is a fixed description.
    Io { what: &'static str, source: io::Error },
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::CiMarker(name) => write!(
                f,
                "refused: the CI marker {name} is set; the private collection is used only on this Mac (R1939)"
            ),
            VaultError::NotReadOnly => write!(f, "refused: the source folder is not on a read-only mount (R1938)"),
            VaultError::NoCollection => write!(f, "the source folder has no {COLLECTION}"),
            VaultError::NotAllowed(what) => write!(f, "refused: {what}"),
            VaultError::DestinationExists => write!(f, "the destination already exists"),
            VaultError::SourceChanged(name) => write!(f, "{name} changed while it was copied"),
            VaultError::CollectionBusy { attempts } => write!(
                f,
                "{COLLECTION} or {COLLECTION_WAL} changed during each of {attempts} copies; quit Anki (or let it sit idle) and copy again"
            ),
            VaultError::Io { what, source } => write!(f, "{what}: {source}"),
        }
    }
}

impl std::error::Error for VaultError {}

fn io_error(what: &'static str) -> impl FnOnce(io::Error) -> VaultError {
    move |source| VaultError::Io { what, source }
}

/// One copied file: its size and SHA-256 (lower-case hex).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCopy {
    pub bytes: u64,
    pub sha256: String,
}

/// What a copy made. Only counts, sizes and hashes, except
/// [`CopyReport::access`], which names files and stays in `build/private/`.
#[derive(Clone, Debug)]
pub struct CopyReport {
    pub collection: FileCopy,
    pub wal: Option<FileCopy>,
    /// Regular files copied from the media folder (0 without one).
    pub media_files: u64,
    pub media_bytes: u64,
    /// Media folder entries left out: subfolders, symlinks, other kinds.
    pub media_skipped: u64,
    /// Every path the vault opened, relative to the folder, in order: the
    /// collection and the log (again for each attempt the source changed
    /// during), the media folder (listed) and each media file.
    pub access: Vec<String>,
}

/// How many times [`Vault::copy_to`] copies the collection and its log
/// before it gives up on a source that keeps changing.
pub const COLLECTION_ATTEMPTS: u32 = 5;

/// A point of [`Vault::copy_to_observed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyStep {
    /// The database file is copied (attempt `attempt`, from 1); its log is
    /// next.
    CollectionCopied { attempt: u32 },
}

/// A profile folder opened for reading.
#[derive(Debug)]
pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// Checks the environment and the folder: no CI marker, a read-only
    /// mount (unless the options turn that off), a regular
    /// [`COLLECTION`] file.
    pub fn open(root: &Path, options: &VaultOptions) -> Result<Vault, VaultError> {
        if let Some(marker) = ci_marker(options.env.iter().map(|(n, v)| (n.as_str(), v.as_str()))) {
            return Err(VaultError::CiMarker(marker));
        }
        let meta = fs::metadata(root).map_err(io_error("reading the source folder"))?;
        if !meta.is_dir() {
            return Err(VaultError::NotAllowed("the source is not a folder"));
        }
        if options.require_read_only && !is_read_only_mount(root).map_err(io_error("checking the source mount"))? {
            return Err(VaultError::NotReadOnly);
        }
        match fs::symlink_metadata(root.join(COLLECTION)) {
            Ok(meta) if meta.file_type().is_file() => {}
            Ok(_) => return Err(VaultError::NotAllowed("collection.anki2 is not a regular file")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(VaultError::NoCollection),
            Err(e) => {
                return Err(VaultError::Io {
                    what: "reading collection.anki2",
                    source: e,
                });
            }
        }
        Ok(Vault {
            root: root.to_path_buf(),
        })
    }

    /// Copies the allowed files into `dest`, which must not exist yet: the
    /// collection and its log at the top, the media files under
    /// [`MEDIA_DIR`]. Each file is hashed as it is copied.
    ///
    /// The collection and its log are one state only together: a checkpoint
    /// moves committed pages from the log into the database file and then
    /// empties the log. So the two are copied as a pair: the files' stamps
    /// (device, inode, size, modification time to the nanosecond; the log's
    /// absence counts) are taken before the database file is copied and again
    /// after the log is, and any difference starts the pair over, up to
    /// [`COLLECTION_ATTEMPTS`] times ([`VaultError::CollectionBusy`]). The
    /// read-only mount does not stop Anki on the host from writing. A media
    /// file must keep its stamp while it is copied ([`VaultError::SourceChanged`]).
    /// On an error, `dest` is removed.
    pub fn copy_to(&self, dest: &Path) -> Result<CopyReport, VaultError> {
        self.copy_to_observed(dest, &mut |_| {})
    }

    /// [`Vault::copy_to`], telling `observer` each [`CopyStep`] as it is
    /// reached, so a test can change the source at that moment.
    pub fn copy_to_observed(&self, dest: &Path, observer: &mut dyn FnMut(CopyStep)) -> Result<CopyReport, VaultError> {
        match fs::symlink_metadata(dest) {
            Ok(_) => return Err(VaultError::DestinationExists),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(VaultError::Io {
                    what: "checking the destination",
                    source: e,
                });
            }
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(io_error("creating the destination's parent"))?;
        }
        fs::create_dir(dest).map_err(io_error("creating the destination"))?;
        let copied = self.copy_into(dest, observer);
        if copied.is_err() {
            // A part copy of the private collection is of no use to anyone.
            let _ = fs::remove_dir_all(dest);
        }
        copied
    }

    fn copy_into(&self, dest: &Path, observer: &mut dyn FnMut(CopyStep)) -> Result<CopyReport, VaultError> {
        let mut access = Vec::new();
        let (collection, wal) = self.copy_collection(dest, &mut access, observer)?;

        let (mut media_files, mut media_bytes, mut media_skipped) = (0, 0, 0);
        let media = self.root.join(MEDIA_DIR);
        match fs::symlink_metadata(&media) {
            Ok(meta) if meta.file_type().is_dir() => {
                access.push(format!("{MEDIA_DIR}/"));
                let media_dest = dest.join(MEDIA_DIR);
                fs::create_dir(&media_dest).map_err(io_error("creating the media copy"))?;
                let mut entries = fs::read_dir(&media)
                    .map_err(io_error("listing collection.media"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(io_error("listing collection.media"))?;
                entries.sort_by_key(|entry| entry.file_name());
                for entry in entries {
                    let kind = entry.file_type().map_err(io_error("reading a media entry"))?;
                    if !kind.is_file() {
                        media_skipped += 1;
                        continue;
                    }
                    let name = entry.file_name();
                    access.push(format!("{MEDIA_DIR}/{}", name.to_string_lossy()));
                    let copied = copy_file(&entry.path(), &media_dest.join(&name), "a media file")?;
                    media_files += 1;
                    media_bytes += copied.bytes;
                }
            }
            Ok(_) => return Err(VaultError::NotAllowed("collection.media is not a folder")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(VaultError::Io {
                    what: "reading collection.media",
                    source: e,
                });
            }
        }

        Ok(CopyReport {
            collection,
            wal,
            media_files,
            media_bytes,
            media_skipped,
            access,
        })
    }

    /// The collection and its log, copied as one pair (see
    /// [`Vault::copy_to`]).
    fn copy_collection(
        &self,
        dest: &Path,
        access: &mut Vec<String>,
        observer: &mut dyn FnMut(CopyStep),
    ) -> Result<(FileCopy, Option<FileCopy>), VaultError> {
        let (db, wal) = (self.root.join(COLLECTION), self.root.join(COLLECTION_WAL));
        let (db_copy, wal_copy) = (dest.join(COLLECTION), dest.join(COLLECTION_WAL));
        for attempt in 1..=COLLECTION_ATTEMPTS {
            let before = (pair_stamp(&db, COLLECTION)?, pair_stamp(&wal, COLLECTION_WAL)?);
            if before.0.is_none() {
                return Err(VaultError::NoCollection);
            }
            access.push(COLLECTION.to_owned());
            let collection = copy_bytes(&db, &db_copy)?;
            observer(CopyStep::CollectionCopied { attempt });
            let log = match before.1 {
                Some(_) => {
                    access.push(COLLECTION_WAL.to_owned());
                    // A log that went away since the stamp is a change, seen below.
                    copy_bytes_if_present(&wal, &wal_copy)?
                }
                None => None,
            };
            let after = (pair_stamp(&db, COLLECTION)?, pair_stamp(&wal, COLLECTION_WAL)?);
            let sizes_match = before.0.map(|stamp| stamp.len) == Some(collection.bytes)
                && before.1.map(|stamp| stamp.len) == log.as_ref().map(|copy| copy.bytes);
            if before == after && sizes_match {
                return Ok((collection, log));
            }
            for path in [&db_copy, &wal_copy] {
                match fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => {
                        return Err(VaultError::Io {
                            what: "removing a stale copy",
                            source: e,
                        });
                    }
                }
            }
        }
        Err(VaultError::CollectionBusy {
            attempts: COLLECTION_ATTEMPTS,
        })
    }
}

/// What a source file must keep while it is copied: which file it is
/// (device and inode), its size and its modification time, to the
/// nanosecond.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
}

impl Stamp {
    fn of(meta: &fs::Metadata) -> Stamp {
        use std::os::unix::fs::MetadataExt;
        Stamp {
            dev: meta.dev(),
            ino: meta.ino(),
            len: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
        }
    }
}

/// The stamp of the collection or its log, `None` when it does not exist.
/// Anything there but a regular file is refused.
fn pair_stamp(path: &Path, name: &'static str) -> Result<Option<Stamp>, VaultError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(Some(Stamp::of(&meta))),
        Ok(_) => Err(VaultError::NotAllowed(if name == COLLECTION {
            "collection.anki2 is not a regular file"
        } else {
            "collection.anki2-wal is not a regular file"
        })),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(VaultError::Io {
            what: if name == COLLECTION {
                "reading collection.anki2"
            } else {
                "reading collection.anki2-wal"
            },
            source: e,
        }),
    }
}

/// [`copy_bytes`], or `None` when `source` no longer exists.
fn copy_bytes_if_present(source: &Path, dest: &Path) -> Result<Option<FileCopy>, VaultError> {
    match File::open(source) {
        Ok(input) => copy_from(input, dest).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(VaultError::Io {
            what: "opening a source file",
            source: e,
        }),
    }
}

/// Copies `source` to the new file `dest`, hashing it; no stamp check.
fn copy_bytes(source: &Path, dest: &Path) -> Result<FileCopy, VaultError> {
    copy_from(File::open(source).map_err(io_error("opening a source file"))?, dest)
}

fn copy_from(mut input: File, dest: &Path) -> Result<FileCopy, VaultError> {
    let mut output = File::create_new(dest).map_err(io_error("creating a copy"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(io_error("reading a source file"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        output.write_all(&buffer[..n]).map_err(io_error("writing a copy"))?;
        bytes += n as u64;
    }
    output.sync_all().map_err(io_error("writing a copy"))?;
    Ok(FileCopy {
        bytes,
        sha256: crate::generate::hex(&hasher.finalize()),
    })
}

/// Copies one media file, which must keep its stamp while it is copied.
fn copy_file(source: &Path, dest: &Path, name: &'static str) -> Result<FileCopy, VaultError> {
    let input = File::open(source).map_err(io_error("opening a source file"))?;
    let before = Stamp::of(&input.metadata().map_err(io_error("reading a source file"))?);
    let copied = copy_from(input, dest)?;
    let after = Stamp::of(&fs::metadata(source).map_err(io_error("reading a source file"))?);
    if before != after || copied.bytes != after.len {
        return Err(VaultError::SourceChanged(name));
    }
    Ok(copied)
}
