//! Media sync bodies (specs/sync/wire-format.md W15 to W17): the result
//! envelope, the JSON of each `msync/` method, and the two zips. What media
//! sync does with them is U111's.

use std::collections::HashMap;
use std::io::{self, Cursor, Read, Write};
use std::path::Path;

use serde_json::value::RawValue;
use tempfile::TempPath;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::read::{self, At, DecodeError};
use super::write;
use crate::error::{ErrorKind, JobError};
use crate::identity::client_identity;
use crate::transport::Method;

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
    /// the move fails the file is removed.
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

/// The `downloadFiles` answer (W17.1): a zip whose `_meta` maps each entry
/// name to a real name. Entries may be stored or deflated. A file entry that
/// `_meta` does not map makes the whole zip a decode failure; a `_meta`
/// member with no entry is ignored. Files come in entry order, each
/// expanded into a new file in `staging`, an existing directory: only
/// `_meta` is read into memory.
///
/// The zip is a decode failure when `_meta` expands past
/// [`MAX_META_BYTES`], one file past [`MAX_DOWNLOAD_ENTRY_BYTES`], its files
/// together past [`MAX_DOWNLOAD_BYTES`], or all its entries (`_meta`
/// counting) past [`MAX_DOWNLOAD_EXPANSION`] times the body's size. The sizes the zip
/// declares are checked before anything is staged; the bytes it expands to
/// are checked again as they are read, and reading stops one byte past a
/// limit. A failure leaves nothing in `staging`. A file that cannot be
/// written there is `disk_full` or `local_storage` (errors.md E7).
pub fn decode_download_zip(body: &[u8], staging: &Path) -> Result<Vec<MediaFile>, JobError> {
    let fail = |detail: String| JobError::bad_response(Method::DownloadFiles, detail);
    let mut archive =
        ZipArchive::new(Cursor::new(body)).map_err(|err| fail(format!("the body is not a zip: {err}")))?;
    let mut limits = Limits {
        all: u64::try_from(body.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(MAX_DOWNLOAD_EXPANSION),
        files: MAX_DOWNLOAD_BYTES,
    };
    let entries = declared_entries(&archive, limits).map_err(fail)?;
    let meta = match archive.index_for_name(META_ENTRY) {
        Some(index) => read_meta(&mut archive, index, &mut limits).map_err(fail)?,
        None => return Err(fail(format!("the zip has no {META_ENTRY} entry"))),
    };
    let names: HashMap<String, String> = entry_names(&meta)
        .map_err(|err| fail(format!("{META_ENTRY}: {err}")))?
        .into_iter()
        .collect();
    let mut mapped = Vec::with_capacity(entries.len());
    for (index, entry) in entries {
        let Some(name) = names.get(&entry) else {
            return Err(fail(format!("zip entry {entry:?} is not in {META_ENTRY}")));
        };
        mapped.push((index, name.clone()));
    }
    let mut buffer = vec![0u8; COPY_CHUNK];
    let mut files = Vec::with_capacity(mapped.len());
    for (index, name) in mapped {
        let file = stage_entry(&mut archive, index, &mut limits, staging, &mut buffer)?;
        files.push(MediaFile { name, file });
    }
    Ok(files)
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
fn declared_entries(archive: &ZipArchive<Cursor<&[u8]>>, limits: Limits) -> Result<Vec<(usize, String)>, String> {
    let mut all = 0u64;
    let mut files = 0u64;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_data(index)
            .map_err(|err| format!("zip entry {index}: {err}"))?;
        let name = entry
            .name()
            .map_err(|err| format!("zip entry {index}: {err}"))?
            .into_owned();
        if !matches!(
            entry.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(format!("zip entry {index} is neither stored nor deflated"));
        }
        let size = entry.size();
        if size > MAX_DOWNLOAD_ENTRY_BYTES {
            return Err(format!(
                "zip entry {index} declares more than {MAX_DOWNLOAD_ENTRY_BYTES} bytes"
            ));
        }
        if name == META_ENTRY && size > MAX_META_BYTES {
            return Err(format!("{META_ENTRY} declares more than {MAX_META_BYTES} bytes"));
        }
        all = all.saturating_add(size);
        if name != META_ENTRY {
            files = files.saturating_add(size);
            entries.push((index, name));
        }
    }
    if files > limits.files {
        return Err(format!("the zip's files declare more than {MAX_DOWNLOAD_BYTES} bytes"));
    }
    if all > limits.all {
        return Err(format!(
            "the zip declares more than {MAX_DOWNLOAD_EXPANSION} times its size"
        ));
    }
    Ok(entries)
}

/// The bytes of `_meta`, taken from `limits.all`: at most
/// [`MAX_META_BYTES`] and at most what is left. One byte past either limit
/// fails, and nothing past it is read, so nothing larger is ever parsed.
fn read_meta(archive: &mut ZipArchive<Cursor<&[u8]>>, index: usize, limits: &mut Limits) -> Result<Vec<u8>, String> {
    let entry = archive
        .by_index(index)
        .map_err(|err| format!("zip entry {index}: {err}"))?;
    let limit = MAX_META_BYTES.min(limits.all);
    // The buffer grows with the bytes read, never from the declared size.
    let mut data = Vec::new();
    entry
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|err| format!("zip entry {index}: {err}"))?;
    let read = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if read > MAX_META_BYTES {
        return Err(format!("{META_ENTRY} expands past {MAX_META_BYTES} bytes"));
    }
    if read > limits.all {
        return Err(format!("the zip expands past {MAX_DOWNLOAD_EXPANSION} times its size"));
    }
    limits.all -= read;
    Ok(data)
}

/// Expands the file entry at `index` into a new file in `staging`, taken
/// from both limits: at most [`MAX_DOWNLOAD_ENTRY_BYTES`] and at most what
/// either limit has left. One byte past a limit fails, and nothing past it is
/// read or written. On any failure the file is removed.
fn stage_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    index: usize,
    limits: &mut Limits,
    staging: &Path,
    buffer: &mut [u8],
) -> Result<StagedFile, JobError> {
    let fail = |detail: String| JobError::bad_response(Method::DownloadFiles, detail);
    let entry = archive
        .by_index(index)
        .map_err(|err| fail(format!("zip entry {index}: {err}")))?;
    let mut file = tempfile::Builder::new()
        .prefix("download-")
        .tempfile_in(staging)
        .map_err(|err| staging_error(&err))?;
    let limit = MAX_DOWNLOAD_ENTRY_BYTES.min(limits.all).min(limits.files);
    let mut reader = entry.take(limit.saturating_add(1));
    let mut size = 0u64;
    loop {
        let read = match reader.read(buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(fail(format!("zip entry {index}: {err}"))),
        };
        size = size.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
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
        file.write_all(&buffer[..read]).map_err(|err| staging_error(&err))?;
    }
    file.flush().map_err(|err| staging_error(&err))?;
    limits.all -= size;
    limits.files -= size;
    Ok(StagedFile {
        path: file.into_temp_path(),
        size,
    })
}

/// E7: a staged file that cannot be created or written. Lack of space is
/// `disk_full`; anything else is `local_storage`.
fn staging_error(err: &io::Error) -> JobError {
    let kind = match err.kind() {
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => ErrorKind::DiskFull {
            needed_bytes: None,
            available_bytes: None,
        },
        _ => ErrorKind::LocalStorage,
    };
    JobError::new(
        kind,
        Some(Method::DownloadFiles),
        format!("staging a downloaded file failed: {err}"),
    )
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
    let fail = |err: String| {
        JobError::new(
            ErrorKind::Internal,
            Some(Method::UploadChanges),
            format!("writing the media zip failed: {err}"),
        )
    };
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut meta = String::from("[");
    for (index, change) in batch.iter().enumerate() {
        if index > 0 {
            meta.push(',');
        }
        meta.push('[');
        match change {
            UploadChange::Add { name, data } => {
                let entry = index.to_string();
                zip.start_file(entry.as_str(), options)
                    .map_err(|err| fail(err.to_string()))?;
                zip.write_all(data).map_err(|err| fail(err.to_string()))?;
                write::string(&mut meta, name);
                meta.push(',');
                write::string(&mut meta, &entry);
            }
            UploadChange::Delete { name } => {
                write::string(&mut meta, name);
                meta.push_str(",null");
            }
        }
        meta.push(']');
    }
    meta.push(']');
    zip.start_file(META_ENTRY, options)
        .map_err(|err| fail(err.to_string()))?;
    zip.write_all(meta.as_bytes()).map_err(|err| fail(err.to_string()))?;
    zip.finish()
        .map(Cursor::into_inner)
        .map_err(|err| fail(err.to_string()))
}
