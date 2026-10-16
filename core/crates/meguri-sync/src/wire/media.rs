//! Media sync bodies (specs/sync/wire-format.md W15 to W17): the result
//! envelope, the JSON of each `msync/` method, and the two zips. What media
//! sync does with them is U111's.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use serde_json::value::RawValue;
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

/// One file of a `downloadFiles` zip, under its real name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    pub name: String,
    pub data: Vec<u8>,
}

/// The most bytes one entry of a `downloadFiles` zip may expand to: the
/// largest file a server keeps (W17.2) and Meguri sends (R1565).
pub const MAX_DOWNLOAD_ENTRY_BYTES: u64 = 100 * 1024 * 1024;

/// How many times its own size a `downloadFiles` zip may expand to, all its
/// entries together (errors.md E6: a decoder allocates only in proportion to
/// the bytes received). Stored entries, which servers send, stay under 1;
/// deflate alone reaches about 1032.
pub const MAX_DOWNLOAD_EXPANSION: u64 = 100;

/// The `downloadFiles` answer (W17.1): a zip whose `_meta` maps each entry
/// name to a real name. Entries may be stored or deflated. A file entry that
/// `_meta` does not map makes the whole zip a decode failure; a `_meta`
/// member with no entry is ignored. Files come in entry order.
///
/// Reading stops at the first byte past [`MAX_DOWNLOAD_ENTRY_BYTES`] in one
/// entry, or past [`MAX_DOWNLOAD_EXPANSION`] times the body's size in all of
/// them (`_meta` counting), and the zip is a decode failure: a small body
/// never expands into a large allocation.
pub fn decode_download_zip(body: &[u8]) -> Result<Vec<MediaFile>, JobError> {
    let fail = |detail: String| JobError::bad_response(Method::DownloadFiles, detail);
    let mut archive =
        ZipArchive::new(Cursor::new(body)).map_err(|err| fail(format!("the body is not a zip: {err}")))?;
    let mut budget = u64::try_from(body.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(MAX_DOWNLOAD_EXPANSION);
    let meta = match archive.index_for_name(META_ENTRY) {
        Some(index) => read_entry(&mut archive, index, &mut budget).map_err(fail)?,
        None => return Err(fail(format!("the zip has no {META_ENTRY} entry"))),
    };
    let names: HashMap<String, String> = entry_names(&meta)
        .map_err(|err| fail(format!("{META_ENTRY}: {err}")))?
        .into_iter()
        .collect();
    let mut files = Vec::new();
    for index in 0..archive.len() {
        let entry = match archive.name_for_index(index) {
            Some(Ok(name)) => name.into_owned(),
            Some(Err(err)) => return Err(fail(format!("zip entry {index}: {err}"))),
            None => return Err(fail(format!("zip entry {index} is missing"))),
        };
        if entry == META_ENTRY {
            continue;
        }
        let Some(name) = names.get(&entry) else {
            return Err(fail(format!("zip entry {entry:?} is not in {META_ENTRY}")));
        };
        let data = read_entry(&mut archive, index, &mut budget).map_err(fail)?;
        files.push(MediaFile {
            name: name.clone(),
            data,
        });
    }
    Ok(files)
}

/// The bytes of the entry at `index`, stored or deflated, taken from
/// `budget`: at most [`MAX_DOWNLOAD_ENTRY_BYTES`] and at most what `budget`
/// has left. One byte past either limit fails, and nothing past it is read.
fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, index: usize, budget: &mut u64) -> Result<Vec<u8>, String> {
    let entry = archive
        .by_index(index)
        .map_err(|err| format!("zip entry {index}: {err}"))?;
    if !matches!(
        entry.compression(),
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(format!("zip entry {index} is neither stored nor deflated"));
    }
    let limit = MAX_DOWNLOAD_ENTRY_BYTES.min(*budget);
    // The buffer grows with the bytes read, never from the declared size: an
    // entry may declare far more than it holds.
    let mut data = Vec::new();
    entry
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|err| format!("zip entry {index}: {err}"))?;
    let read = u64::try_from(data.len()).unwrap_or(u64::MAX);
    if read > MAX_DOWNLOAD_ENTRY_BYTES {
        return Err(format!(
            "zip entry {index} expands past {MAX_DOWNLOAD_ENTRY_BYTES} bytes"
        ));
    }
    if read > *budget {
        return Err(format!("the zip expands past {MAX_DOWNLOAD_EXPANSION} times its size"));
    }
    *budget -= read;
    Ok(data)
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
