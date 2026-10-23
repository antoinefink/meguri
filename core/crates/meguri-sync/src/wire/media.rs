//! Media sync bodies (specs/sync/wire-format.md W15 to W17): the result
//! envelope, the JSON of each `msync/` method, and the two zips. What media
//! sync does with them is U111's.

use std::collections::HashMap;
use std::io::{self, Cursor, Read, Seek, Write};
use std::path::Path;

use serde_json::value::RawValue;
use sha1::{Digest, Sha1};
use tempfile::TempPath;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::read::{self, At, DecodeError};
use super::write;
use crate::error::{ErrorKind, JobError};
use crate::identity::client_identity;
use crate::transport::Method;

#[path = "media_directory.rs"]
mod directory;
#[path = "media_io.rs"]
mod file_io;
pub use file_io::{
    MediaDigest, MediaSendReceipt, MediaUploadInput, MediaUploadRecord, PreparedMediaUpload, decode_media_file,
    prepare_media_upload, spool_media_reply,
};

/// The name of the zip entry that maps the others (W17).
pub const META_ENTRY: &str = "_meta";

/// The `begin` request: `{"v":<client identity>}` (W16, transport.md T3).
pub fn encode_begin() -> Vec<u8> {
    let mut out = String::from("{\"v\":");
    write::string(&mut out, client_identity());
    out.push('}');
    out.into_bytes()
}

/// The `mediaChanges` request: `{"lastUsn":…}`, the last media update
/// sequence number the client has applied.
pub fn encode_media_changes(last_usn: i32) -> Vec<u8> {
    let mut out = String::from("{\"lastUsn\":");
    write::int(&mut out, last_usn);
    out.push('}');
    out.into_bytes()
}

/// The `downloadFiles` request: `{"files":[…]}`, at most 25 names (R1577;
/// the caller batches).
pub fn encode_download_files(files: &[String]) -> Vec<u8> {
    let mut out = String::from("{\"files\":[");
    for (index, name) in files.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write::string(&mut out, name);
    }
    out.push_str("]}");
    out.into_bytes()
}

/// The `mediaSanity` request: `{"local":…}`, the client's media file count.
pub fn encode_media_sanity(local: u32) -> Vec<u8> {
    let mut out = String::from("{\"local\":");
    write::int(&mut out, local);
    out.push('}');
    out.into_bytes()
}

/// W15: `data` that reads as the method's result wins whatever `err` holds;
/// otherwise a string `err` is the server's refusal (`server_message`);
/// otherwise the body does not decode.
fn envelope<T>(
    body: &[u8],
    method: Method,
    result: impl for<'r> Fn(&'r RawValue, At<'_>) -> read::Result<T>,
) -> Result<T, JobError> {
    let at = At::Root;
    let refused = |err: DecodeError| JobError::bad_response(method, err.to_string());
    let root = read::object(read::body(body).map_err(refused)?, at).map_err(refused)?;
    let data = root.get("data", at).map_err(refused)?;
    let err = root.get("err", at).map_err(refused)?;
    let failure = match data {
        Some(raw) => match result(raw, at.member("data")) {
            Ok(value) => return Ok(value),
            Err(failure) => failure,
        },
        None => at.member("data").fail("is missing"),
    };
    if let Some(Ok(text)) = err.map(|raw| read::string(raw, at.member("err"))) {
        return Err(JobError::new(
            ErrorKind::ServerMessage { text },
            Some(method),
            "the server refused the call",
        ));
    }
    Err(refused(failure))
}

/// The `begin` answer's `data`: the server's media update sequence number.
/// `sk` is ignored (the official server puts the host key there).
pub fn decode_begin(body: &[u8]) -> Result<i32, JobError> {
    envelope(body, Method::MediaBegin, |raw, at| {
        let object = read::object(raw, at)?;
        read::strict::<i32>(object.required("usn", at)?, at.member("usn"))
    })
}

/// One change the server reports (W16.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaChange {
    pub name: String,
    pub usn: i32,
    /// 40 lower-case hex digits, or empty for a deletion.
    pub sha1: String,
}

/// The `mediaChanges` answer's `data`: an array of change tuples.
pub fn decode_media_changes(body: &[u8]) -> Result<Vec<MediaChange>, JobError> {
    envelope(body, Method::MediaChanges, |raw, at| {
        read::array(raw, at)?
            .into_iter()
            .enumerate()
            .map(|(index, raw)| {
                let at = at.index(index);
                let items = read::tuple(raw, 3, 3, at)?;
                Ok(MediaChange {
                    name: read::string(items[0], at.index(0))?,
                    usn: read::strict::<i32>(items[1], at.index(1))?,
                    sha1: read::string(items[2], at.index(2))?,
                })
            })
            .collect()
    })
}

/// The `uploadChanges` answer's `data`: how many of the zip's changes the
/// server processed, and its media update sequence number afterwards.
pub fn decode_upload_changes(body: &[u8]) -> Result<(u32, i32), JobError> {
    envelope(body, Method::UploadChanges, |raw, at| {
        let items = read::tuple(raw, 2, 2, at)?;
        Ok((
            read::strict::<u32>(items[0], at.index(0))?,
            read::strict::<i32>(items[1], at.index(1))?,
        ))
    })
}

/// The `mediaSanity` verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaSanity {
    /// `"OK"`: the counts match.
    Ok,
    /// `"mediaSanity"`: they do not.
    Mismatch,
}

pub fn decode_media_sanity(body: &[u8]) -> Result<MediaSanity, JobError> {
    envelope(body, Method::MediaSanity, |raw, at| {
        match read::string(raw, at)?.as_str() {
            "OK" => Ok(MediaSanity::Ok),
            "mediaSanity" => Ok(MediaSanity::Mismatch),
            _ => Err(at.fail("is not \"OK\" or \"mediaSanity\"")),
        }
    })
}

/// One file of a `downloadFiles` zip: its real name and its bytes, expanded
/// into the staging directory.
#[derive(Debug)]
pub struct MediaFile {
    /// The name `_meta` maps the entry to.
    pub name: String,
    pub file: StagedFile,
}

/// A file the decoder wrote into the caller's staging directory, under a
/// name of its own (never the entry's or the real name). Dropping the
/// handle removes the file; [`StagedFile::persist`] keeps it.
#[derive(Debug)]
pub struct StagedFile {
    path: TempPath,
    size: u64,
}

impl StagedFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The file's size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Moves the file to `to`, replacing a file there, and keeps it. When
    /// the move fails the file is removed. This is NOT the safe production
    /// media/trash publication primitive; use the store's publication rules.
    pub fn persist(self, to: &Path) -> io::Result<()> {
        self.path.persist(to).map_err(|err| err.error)
    }
}

/// The most bytes one entry of a `downloadFiles` zip may expand to: the
/// largest file a server keeps (W17.2) and Meguri sends (R1565).
pub const MAX_DOWNLOAD_ENTRY_BYTES: u64 = 100 * 1024 * 1024;

/// The most bytes the files of one `downloadFiles` zip may expand to
/// together, whatever the body's size: the 25 names a request asks for at
/// most (R1577), each at most [`MAX_DOWNLOAD_ENTRY_BYTES`]. The files go to
/// the staging directory, not to memory (R1852); this bounds what one
/// answer writes there.
pub const MAX_DOWNLOAD_BYTES: u64 = 25 * MAX_DOWNLOAD_ENTRY_BYTES;

/// How many times its own size a `downloadFiles` zip may expand to, all its
/// entries together (errors.md E6: a decoder allocates only in proportion to
/// the bytes received). Stored entries, which servers send, stay under 1;
/// deflate alone reaches about 1032.
pub const MAX_DOWNLOAD_EXPANSION: u64 = 100;

/// The most bytes `_meta` may expand to, the one entry read into memory and
/// parsed (R1852, R1562): an object of at most 25 members (R1577), each a
/// real name of at most 255 bytes (W17.2) with every byte written as a
/// six-byte escape, comes to under 40 KiB.
pub const MAX_META_BYTES: u64 = 64 * 1024;

/// The bytes one read from an entry moves to its staged file.
const COPY_CHUNK: usize = 64 * 1024;

/// Small slice codec wrapper. Production HTTP replies use [`spool_media_reply`],
/// which additionally bounds raw ZIP inventory and validates request membership.
///
/// The `downloadFiles` answer (W17.1): a zip whose `_meta` maps each entry
/// name to a real name. Entries may be stored or deflated. A file entry that
/// `_meta` does not map makes the whole zip a decode failure; a `_meta`
/// member with no entry is ignored. Files come in entry order, each
/// expanded into a new file in `staging`, an existing directory: only
/// `_meta` is read into memory.
///
/// The zip is a decode failure when `_meta` expands past
/// [`MAX_META_BYTES`], one file is empty or past [`MAX_DOWNLOAD_ENTRY_BYTES`], its files
/// together past [`MAX_DOWNLOAD_BYTES`], or all its entries (`_meta`
/// counting) past [`MAX_DOWNLOAD_EXPANSION`] times the body's size. The sizes the zip
/// declares are checked before anything is staged; the bytes it expands to
/// are checked again as they are read, and reading stops one byte past a
/// limit. A failure leaves nothing in `staging`. A file that cannot be
/// written there is `disk_full` or `local_storage` (errors.md E7).
pub fn decode_download_zip(body: &[u8], staging: &Path) -> Result<Vec<MediaFile>, JobError> {
    decode_zip(Cursor::new(body), body.len() as u64, staging, None, &|| false)
}

fn decode_zip<R: Read + Seek>(
    source: R,
    length: u64,
    staging: &Path,
    requested: Option<&[String]>,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<MediaFile>, JobError> {
    check_cancel(cancelled, Method::DownloadFiles)?;
    let mut archive = ZipArchive::new(source).map_err(decode_zip_error)?;
    let mut limits = Limits {
        all: length.saturating_mul(MAX_DOWNLOAD_EXPANSION),
        files: MAX_DOWNLOAD_BYTES,
    };
    let entries = declared_entries(&archive, limits, cancelled)?;
    let meta = match archive.index_for_name(META_ENTRY) {
        Some(index) => read_meta(&mut archive, index, &mut limits, cancelled)?,
        None => return Err(download_error("zip has no _meta entry")),
    };
    let names: HashMap<String, String> = entry_names(&meta)
        .map_err(|_| download_error("_meta JSON mapping is invalid"))?
        .into_iter()
        .collect();
    let mut mapped = Vec::with_capacity(entries.len());
    for (index, entry) in entries {
        check_cancel(cancelled, Method::DownloadFiles)?;
        let name = names
            .get(&entry)
            .ok_or_else(|| download_error("zip entry is not mapped by _meta"))?;
        if let Some(requested) = requested {
            if !requested.contains(name) {
                return Err(download_error("_meta returned an unrequested real name"));
            }
            if mapped.iter().any(|(_, prior)| prior == name) {
                return Err(download_error("_meta returned a repeated real name"));
            }
        }
        mapped.push((index, name.clone()));
    }
    if requested.is_some_and(|names| !names.is_empty()) && mapped.is_empty() {
        return Err(download_error("zip returned no files for a nonempty request"));
    }
    let mut buffer = vec![0u8; COPY_CHUNK];
    let mut files = Vec::with_capacity(mapped.len());
    for (index, name) in mapped {
        check_cancel(cancelled, Method::DownloadFiles)?;
        let file = stage_entry(&mut archive, index, &mut limits, staging, &mut buffer, cancelled)?;
        files.push(MediaFile { name, file });
    }
    check_cancel(cancelled, Method::DownloadFiles)?;
    Ok(files)
}

fn download_error(stage: &str) -> JobError {
    JobError::bad_response(Method::DownloadFiles, stage)
}

fn check_cancel(cancelled: &dyn Fn() -> bool, method: Method) -> Result<(), JobError> {
    if cancelled() {
        Err(JobError::new(ErrorKind::Cancelled, Some(method), "media ZIP cancelled"))
    } else {
        Ok(())
    }
}

fn decode_io_error(err: io::Error) -> JobError {
    match err.kind() {
        io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData | io::ErrorKind::InvalidInput => {
            download_error("zip entry read or CRC failed")
        }
        _ => local_error(Method::DownloadFiles, "reading ZIP", &err),
    }
}

fn decode_zip_error(err: zip::result::ZipError) -> JobError {
    match err {
        zip::result::ZipError::Io(err) => decode_io_error(err),
        _ => download_error("zip structure or entry is invalid"),
    }
}

/// What a download zip may still expand to.
#[derive(Debug, Clone, Copy)]
struct Limits {
    /// All entries, `_meta` counting ([`MAX_DOWNLOAD_EXPANSION`]).
    all: u64,
    /// The files ([`MAX_DOWNLOAD_BYTES`]).
    files: u64,
}

/// The index and name of each file entry (every entry but `_meta`), after
/// checking what every entry declares against the limits: an entry that is
/// neither stored nor deflated, or sizes past a limit, fail before anything
/// expands.
fn declared_entries<R: Read + Seek>(
    archive: &ZipArchive<R>,
    limits: Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<(usize, String)>, JobError> {
    let mut all = 0u64;
    let mut files = 0u64;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        check_cancel(cancelled, Method::DownloadFiles)?;
        let entry = archive.by_index_data(index).map_err(decode_zip_error)?;
        let name = entry
            .name()
            .map_err(|_| download_error("zip entry name is invalid"))?
            .into_owned();
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(download_error("zip entry is neither stored nor deflated"));
        }
        let size = entry.size();
        if name != META_ENTRY && size == 0 {
            return Err(download_error("zip file entry declared size is zero"));
        }
        if size > MAX_DOWNLOAD_ENTRY_BYTES {
            return Err(download_error("zip entry declared size exceeds file bound"));
        }
        if name == META_ENTRY && size > MAX_META_BYTES {
            return Err(download_error("_meta declared size exceeds bound"));
        }
        all = all.saturating_add(size);
        if name != META_ENTRY {
            files = files.saturating_add(size);
            entries.push((index, name));
        }
    }
    if files > limits.files {
        return Err(download_error("zip files declared size exceeds total bound"));
    }
    if all > limits.all {
        return Err(download_error("zip declared size exceeds expansion bound"));
    }
    Ok(entries)
}

/// The bytes of `_meta`, taken from `limits.all`: at most
/// [`MAX_META_BYTES`] and at most what is left. One byte past either limit
/// fails, and nothing past it is read, so nothing larger is ever parsed.
fn read_meta<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limits: &mut Limits,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<u8>, JobError> {
    let entry = archive.by_index(index).map_err(decode_zip_error)?;
    let limit = MAX_META_BYTES.min(limits.all);
    let mut reader = entry.take(limit.saturating_add(1));
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        check_cancel(cancelled, Method::DownloadFiles)?;
        let n = match reader.read(&mut buffer) {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => result.map_err(decode_io_error)?,
        };
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..n]);
    }
    if data.len() as u64 > limit {
        return Err(download_error("_meta actual size exceeds metadata or expansion bound"));
    }
    limits.all -= data.len() as u64;
    Ok(data)
}

/// Expands the file entry at `index` into a new file in `staging`, taken
/// from both limits: at most [`MAX_DOWNLOAD_ENTRY_BYTES`] and at most what
/// either limit has left. One byte past a limit fails, and nothing past it is
/// read or written. On any failure the file is removed.
fn stage_entry<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    limits: &mut Limits,
    staging: &Path,
    buffer: &mut [u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<StagedFile, JobError> {
    let entry = archive.by_index(index).map_err(decode_zip_error)?;
    let mut file = tempfile::Builder::new()
        .prefix("download-")
        .tempfile_in(staging)
        .map_err(|err| staging_error(&err))?;
    let size = copy_entry(entry, index, limits, &mut file, buffer, cancelled)?;
    file.flush().map_err(|err| staging_error(&err))?;
    Ok(StagedFile {
        path: file.into_temp_path(),
        size,
    })
}

fn copy_entry<R: Read, W: Write>(
    entry: R,
    index: usize,
    limits: &mut Limits,
    output: &mut W,
    buffer: &mut [u8],
    cancelled: &dyn Fn() -> bool,
) -> Result<u64, JobError> {
    let fail = |detail: String| JobError::bad_response(Method::DownloadFiles, detail);
    let limit = MAX_DOWNLOAD_ENTRY_BYTES.min(limits.all).min(limits.files);
    let mut reader = entry.take(limit.saturating_add(1));
    let mut size = 0u64;
    loop {
        check_cancel(cancelled, Method::DownloadFiles)?;
        let read = match reader.read(buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(decode_io_error(err)),
        };
        size = size.saturating_add(read as u64);
        if size > MAX_DOWNLOAD_ENTRY_BYTES {
            return Err(fail(format!(
                "zip entry {index} expands past {MAX_DOWNLOAD_ENTRY_BYTES} bytes"
            )));
        }
        if size > limits.files {
            return Err(fail(format!("the zip's files expand past {MAX_DOWNLOAD_BYTES} bytes")));
        }
        if size > limits.all {
            return Err(fail(format!(
                "the zip expands past {MAX_DOWNLOAD_EXPANSION} times its size"
            )));
        }
        output.write_all(&buffer[..read]).map_err(|err| staging_error(&err))?;
    }
    if size == 0 {
        return Err(download_error("zip file entry actual size is zero"));
    }
    limits.all -= size;
    limits.files -= size;
    Ok(size)
}

/// E7: a staged file that cannot be created or written. Lack of space is
/// `disk_full`; anything else is `local_storage`.
fn staging_error(err: &io::Error) -> JobError {
    local_error(Method::DownloadFiles, "staging download", err)
}

fn local_error(method: Method, stage: &str, err: &io::Error) -> JobError {
    let kind = match err.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => ErrorKind::DiskFull {
            needed_bytes: None,
            available_bytes: None,
        },
        _ => ErrorKind::LocalStorage,
    };
    // An I/O error can contain a caller's path or custom source text.
    JobError::new(kind, Some(method), format!("{stage}: {:?}", err.kind()))
}

/// `_meta` of a downloaded zip: an object mapping entry names to real names
/// (a repeated entry name keeps its last value).
fn entry_names(meta: &[u8]) -> read::Result<Vec<(String, String)>> {
    let at = At::Root;
    read::object(read::body(meta)?, at)?
        .last_wins()
        .into_iter()
        .map(|(entry, raw)| {
            let name = read::string(raw, at.member(&entry))?;
            Ok((entry, name))
        })
        .collect()
}

/// One change of an `uploadChanges` batch, in batch order (W17.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadChange<'a> {
    /// A file with content.
    Add {
        name: &'a str,
        data: &'a [u8],
    },
    Delete {
        name: &'a str,
    },
}

/// The `uploadChanges` zip (W17.2): additions as stored entries named by
/// their position in the batch (deletions counting), then `_meta`: one
/// `[<real name>, "<entry name>"]` or `[<real name>, null]` per change. Which
/// changes form a batch (at most 25, the 2.5 MiB fill) is U111's.
pub fn encode_upload_zip(batch: &[UploadChange<'_>]) -> Result<Vec<u8>, JobError> {
    let mut sources: Vec<_> = batch
        .iter()
        .map(|change| match change {
            UploadChange::Add { name, data } => EncodeChange::Add {
                name: (*name).into(),
                source: Cursor::new(*data),
            },
            UploadChange::Delete { name } => EncodeChange::Delete { name: (*name).into() },
        })
        .collect();
    let (output, _) = encode_zip(Cursor::new(Vec::new()), &mut sources, false, &|| false)?;
    Ok(output.into_inner())
}

pub(super) enum EncodeChange<R> {
    Add { name: String, source: R },
    Delete { name: String },
}

fn encode_zip<W: Write + Seek, R: Read>(
    output: W,
    batch: &mut [EncodeChange<R>],
    eligible: bool,
    cancelled: &dyn Fn() -> bool,
) -> Result<(W, Vec<Option<MediaDigest>>), JobError> {
    let method = Method::UploadChanges;
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = ZipWriter::new(output);
    let mut meta = String::from("[");
    let mut observed = Vec::with_capacity(batch.len());
    let mut buffer = vec![0; COPY_CHUNK];
    for (index, change) in batch.iter_mut().enumerate() {
        check_cancel(cancelled, method)?;
        if index > 0 {
            meta.push(',');
        }
        meta.push('[');
        match change {
            EncodeChange::Add { name, source } => {
                let entry = index.to_string();
                zip.start_file(entry.as_str(), options).map_err(encode_zip_error)?;
                let mut hash = Sha1::new();
                let mut length = 0u64;
                loop {
                    check_cancel(cancelled, method)?;
                    let cap = if eligible {
                        (MAX_DOWNLOAD_ENTRY_BYTES.saturating_sub(length) + 1).min(COPY_CHUNK as u64) as usize
                    } else {
                        COPY_CHUNK
                    };
                    let n = match source.read(&mut buffer[..cap]) {
                        Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                        result => result.map_err(|err| local_error(method, "reading upload source", &err))?,
                    };
                    if n == 0 {
                        break;
                    }
                    length += n as u64;
                    if eligible && length > MAX_DOWNLOAD_ENTRY_BYTES {
                        return Err(upload_source_error(
                            "upload source exceeds eligible size; register name again",
                        ));
                    }
                    zip.write_all(&buffer[..n])
                        .map_err(|err| local_error(method, "writing upload ZIP", &err))?;
                    hash.update(&buffer[..n]);
                }
                if eligible && length == 0 {
                    return Err(upload_source_error("upload source is empty; register name again"));
                }
                observed.push(Some(MediaDigest {
                    length,
                    sha1: hash.finalize().into(),
                }));
                write::string(&mut meta, name);
                meta.push(',');
                write::string(&mut meta, &entry);
            }
            EncodeChange::Delete { name } => {
                observed.push(None);
                write::string(&mut meta, name);
                meta.push_str(",null");
            }
        }
        meta.push(']');
        if meta.len() + 1 > MAX_META_BYTES as usize {
            return Err(upload_source_error("upload _meta exceeds bound"));
        }
    }
    meta.push(']');
    check_cancel(cancelled, method)?;
    zip.start_file(META_ENTRY, options).map_err(encode_zip_error)?;
    zip.write_all(meta.as_bytes())
        .map_err(|err| local_error(method, "writing upload _meta", &err))?;
    let output = zip.finish().map_err(encode_zip_error)?;
    check_cancel(cancelled, method)?;
    Ok((output, observed))
}

fn upload_source_error(stage: &str) -> JobError {
    JobError::new(ErrorKind::LocalStorage, Some(Method::UploadChanges), stage)
}

fn encode_zip_error(err: zip::result::ZipError) -> JobError {
    match err {
        zip::result::ZipError::Io(err) => local_error(Method::UploadChanges, "writing upload ZIP", &err),
        _ => JobError::new(
            ErrorKind::Internal,
            Some(Method::UploadChanges),
            "writing upload ZIP structure failed",
        ),
    }
}

#[cfg(test)]
mod file_copy_tests {
    use super::*;
    use std::{
        cell::Cell,
        fs::File,
        io::{Seek, SeekFrom},
        path::Path,
    };

    struct FullWriter<W> {
        inner: W,
        left: usize,
    }
    impl<W: Write> Write for FullWriter<W> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.left == 0 {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            let n = self.left.min(bytes.len());
            let n = self.inner.write(&bytes[..n])?;
            self.left -= n;
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }
    impl<W: Seek> Seek for FullWriter<W> {
        fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
            self.inner.seek(from)
        }
    }
    fn entries() -> Vec<EncodeChange<Cursor<Vec<u8>>>> {
        vec![EncodeChange::Add {
            name: "a".into(),
            source: Cursor::new(vec![3; COPY_CHUNK * 2]),
        }]
    }
    fn count(path: &Path) -> usize {
        std::fs::read_dir(path).unwrap().count()
    }

    #[test]
    fn partial_decode_encode_writes_keep_disk_full_typed() {
        let directory = tempfile::tempdir().unwrap();
        for stage in 0..2 {
            let mut output = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
            let mut writer = FullWriter {
                inner: output.as_file_mut(),
                left: 100,
            };
            let result = if stage == 0 {
                let mut limits = Limits {
                    all: MAX_DOWNLOAD_BYTES,
                    files: MAX_DOWNLOAD_BYTES,
                };
                copy_entry(
                    Cursor::new(vec![1; 1024]),
                    0,
                    &mut limits,
                    &mut writer,
                    &mut [0; 64],
                    &|| false,
                )
                .map(|_| ())
            } else {
                encode_zip(writer, &mut entries(), true, &|| false).map(|_| ())
            };
            let err = result.unwrap_err();
            assert!(matches!(err.kind, ErrorKind::DiskFull { .. }));
            assert_eq!(
                err.method,
                Some(if stage == 0 {
                    Method::DownloadFiles
                } else {
                    Method::UploadChanges
                })
            );
            drop(output);
            assert_eq!(count(directory.path()), 0);
        }
    }

    #[test]
    fn cancelled_mid_copy_does_not_produce_a_sendable_zip() {
        let directory = tempfile::tempdir().unwrap();
        let mut output = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        let checks = Cell::new(0);
        let cancelled = || {
            checks.set(checks.get() + 1);
            checks.get() >= 3
        };
        let result = encode_zip(output.as_file_mut(), &mut entries(), true, &cancelled);
        assert!(matches!(result, Err(ref e) if e.kind == ErrorKind::Cancelled));
        drop(result);
        drop(output);
        assert_eq!(count(directory.path()), 0);
    }

    #[test]
    fn mutation_between_file_reads_hashes_only_bytes_in_the_zip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("source");
        let before = vec![1; COPY_CHUNK * 2];
        std::fs::write(&path, &before).unwrap();
        let mut batch = vec![EncodeChange::Add {
            name: "a".into(),
            source: File::open(&path).unwrap(),
        }];
        let reads = Cell::new(0);
        let check = || {
            reads.set(reads.get() + 1);
            if reads.get() == 3 {
                let mut writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
                writer.seek(SeekFrom::Start(COPY_CHUNK as u64)).unwrap();
                writer.write_all(&vec![2; COPY_CHUNK]).unwrap();
            }
            false
        };
        let mut output = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        let (_, observed) = encode_zip(output.as_file_mut(), &mut batch, true, &check).unwrap();
        let mut expected = before;
        expected[COPY_CHUNK..].fill(2);
        let expected_hash: [u8; 20] = Sha1::digest(&expected).into();
        assert_eq!(observed[0].as_ref().unwrap().sha1, expected_hash);
        let mut archive = ZipArchive::new(output.as_file_mut()).unwrap();
        let mut entry = archive.by_name("0").unwrap();
        let mut actual = Vec::new();
        entry.read_to_end(&mut actual).unwrap(); // Test fixture only, 128 KiB.
        assert_eq!(actual, expected);
    }
}
