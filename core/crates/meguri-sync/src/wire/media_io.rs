//! File-backed media ZIP input and output using the shared wire codec.
//! Blocking operations retain their files until the sync worker returns.

use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};

use bytes::Bytes;
use futures_core::Stream;
use meguri_engine::CancelToken;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use tokio::sync::Notify;

use super::{
    COPY_CHUNK, EncodeChange, MAX_DOWNLOAD_ENTRY_BYTES, MediaFile, check_cancel, decode_zip, directory, download_error,
    encode_zip, local_error, upload_source_error,
};
use crate::error::{ErrorKind, JobError};
use crate::transport::{BoxFuture, Method, Payload};
use crate::{ResponseBody, ServerKind};

struct CancelFileWork(Arc<AtomicBool>);

impl Drop for CancelFileWork {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

fn worker_error(method: Method, kind: ErrorKind) -> JobError {
    let detail = if kind == ErrorKind::Cancelled {
        "media file work cancelled"
    } else {
        "media file work failed to return"
    };
    JobError::new(kind, Some(method), detail)
}

async fn file_work<T, F>(cancel: Arc<CancelToken>, method: Method, work: F) -> Result<T, JobError>
where
    T: Send + 'static,
    F: FnOnce(&dyn Fn() -> bool) -> Result<T, JobError> + Send + 'static,
{
    // Every file operation shares this pool, including reads whose caller has
    // gone away. A submitted closure releases its slot only when it returns.
    static CAPACITY: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let abandoned = CancelFileWork(Arc::new(AtomicBool::new(false)));
    let permit = tokio::select! {
        biased;
        () = cancel.cancelled() => return Err(worker_error(method, ErrorKind::Cancelled)),
        result = CAPACITY.acquire() => {
            result.map_err(|_| worker_error(method, ErrorKind::Internal))?
        }
    };
    check_cancel(&|| cancel.is_cancelled(), method)?;
    let dropped = Arc::clone(&abandoned.0);
    let worker_cancel = Arc::clone(&cancel);
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::transport::runtime::blocking_work(|| {
            let cancelled = || worker_cancel.is_cancelled() || dropped.load(Ordering::Acquire);
            check_cancel(&cancelled, method)?;
            work(&cancelled)
        })
    });
    // Joining preserves cleanup ordering on cancellation. Dropping this wait
    // signals the closure to stop; its captured resources still outlive its I/O.
    let result = task.await.map_err(|_| worker_error(method, ErrorKind::Internal))?;
    check_cancel(&|| cancel.is_cancelled(), method)?;
    result
}

fn download_member_limit(requested: &[String]) -> Result<usize, JobError> {
    match requested.len() {
        0..=25 => Ok(requested.len() + 1), // Include the required _meta member.
        _ => Err(download_error("download request exceeds the file count limit")),
    }
}

fn decode_file<R: Read + Seek>(
    source: R,
    length: u64,
    staging: &Path,
    requested: &[String],
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<MediaFile>, JobError> {
    let entries = download_member_limit(requested)?;
    let source = directory::open(source, length, entries, cancelled)?;
    decode_zip(source, length, staging, Some(requested), cancelled)
}

fn write_response<W: Write>(output: &mut W, chunk: &[u8], cancelled: &dyn Fn() -> bool) -> Result<(), JobError> {
    for part in chunk.chunks(COPY_CHUNK) {
        check_cancel(cancelled, Method::DownloadFiles)?;
        output
            .write_all(part)
            .map_err(|err| local_error(Method::DownloadFiles, "writing downloaded ZIP", &err))?;
    }
    Ok(())
}

struct ReplyExtent {
    remaining: u64,
    received: u64,
}

impl ReplyExtent {
    fn admit(declared: u64, maximum: u64) -> Result<Self, JobError> {
        if maximum < declared {
            return Err(download_error("downloaded ZIP declaration exceeds admitted bytes"));
        }
        Ok(Self {
            remaining: maximum,
            received: 0,
        })
    }

    fn account(&mut self, bytes: &[u8]) -> Result<(), JobError> {
        let count = bytes.len() as u64;
        self.remaining = self
            .remaining
            .checked_sub(count)
            .ok_or_else(|| download_error("downloaded ZIP exceeds admitted bytes"))?;
        // Admission bounds the total, so a successful subtraction also proves
        // this sum fits u64. The declaration alone never proves receipt.
        self.received += count;
        Ok(())
    }
}

/// Stages a reply under the caller's byte ceiling, then validates the archive.
/// ZIP inspection starts only after transport EOF has verified framing and
/// declared length. Workers own temporary files for the duration of their I/O.
pub async fn spool_media_reply(
    mut response: ResponseBody,
    destination: PathBuf,
    names: Vec<String>,
    max_zip_bytes: u64,
    server: ServerKind,
    cancellation: Arc<CancelToken>,
) -> Result<Vec<MediaFile>, JobError> {
    let method = Method::DownloadFiles;
    download_member_limit(&names)?;
    check_cancel(&|| cancellation.is_cancelled(), method)?;
    let mut extent = ReplyExtent::admit(response.declared_size(), max_zip_bytes)?;
    let staging = destination.clone();
    let mut zip = file_work(Arc::clone(&cancellation), method, move |_| {
        tempfile::Builder::new()
            .prefix("media-download-")
            .tempfile_in(staging)
            .map_err(|err| local_error(method, "creating downloaded ZIP", &err))
    })
    .await?;
    loop {
        let chunk = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(worker_error(method, ErrorKind::Cancelled)),
            result = response.chunk() => {
                result.map_err(|err| JobError::from_transport(err, Some(method), server))?
            }
        };
        let Some(chunk) = chunk else { break };
        extent.account(&chunk)?;
        zip = file_work(Arc::clone(&cancellation), method, move |cancelled| {
            write_response(zip.as_file_mut(), &chunk, cancelled)?;
            Ok(zip)
        })
        .await?;
    }
    file_work(cancellation, method, move |cancelled| {
        zip.flush()
            .map_err(|err| local_error(method, "flushing downloaded ZIP", &err))?;
        let length = zip
            .as_file()
            .metadata()
            .map_err(|err| local_error(method, "measuring downloaded ZIP", &err))?
            .len();
        if length != extent.received {
            return Err(download_error("downloaded ZIP length differs from received bytes"));
        }
        // Expansion is relative to the complete received ZIP, including metadata.
        decode_file(zip.as_file_mut(), length, &destination, &names, cancelled)
    })
    .await
}

/// Decodes an open ZIP on the sync file worker. The input pathname remains
/// the caller's responsibility; returned staged handles own their output paths.
pub async fn decode_media_file(
    input: File,
    destination: PathBuf,
    names: Vec<String>,
    cancellation: Arc<CancelToken>,
) -> Result<Vec<MediaFile>, JobError> {
    download_member_limit(&names)?;
    file_work(cancellation, Method::DownloadFiles, move |cancelled| {
        let length = input
            .metadata()
            .map_err(|err| local_error(Method::DownloadFiles, "measuring input ZIP", &err))?
            .len();
        decode_file(input, length, &destination, &names, cancelled)
    })
    .await
}

/// File identity captured by the caller or observed while copying an addition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaDigest {
    pub length: u64,
    pub sha1: [u8; 20],
}

/// Open sources in batch order. Names and stamps are caller metadata; names
/// are never used to open a path by this component.
#[derive(Debug)]
pub enum MediaUploadInput {
    Put {
        name: String,
        file: File,
        stamp: u64,
        expected: MediaDigest,
    },
    Remove {
        name: String,
        stamp: u64,
    },
}

/// Facts at one batch position. `encoded` identifies bytes written into the
/// ZIP. Acceptance still requires the caller to check the current change stamp;
/// these facts cannot acknowledge a later change to the same name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaUploadRecord {
    pub name: String,
    pub stamp: u64,
    pub expected: Option<MediaDigest>,
    pub encoded: Option<MediaDigest>,
}

impl MediaUploadRecord {
    /// False requires registering the name again under Y10.3.
    pub fn matches_capture(&self) -> bool {
        self.expected.eq(&self.encoded)
    }
}

struct UploadFile {
    file: NamedTempFile,
    length: u64,
    sha256: [u8; 32],
}

/// Finished ZIP and the corresponding batch facts. Streams share ownership of
/// the ZIP so dropping this handle cannot remove a file still used by transport.
pub struct PreparedMediaUpload {
    file: Arc<UploadFile>,
    records: Vec<MediaUploadRecord>,
}

impl PreparedMediaUpload {
    pub fn byte_len(&self) -> u64 {
        self.file.length
    }

    pub fn digest(&self) -> [u8; 32] {
        self.file.sha256
    }

    pub fn records(&self) -> &[MediaUploadRecord] {
        self.records.as_slice()
    }

    /// Transport adds zstd framing. After the request terminates, drain the
    /// receipt before cleanup or acknowledgement. Verified source EOF alone
    /// establishes neither server acceptance nor journal completion.
    pub fn stream_payload(&self, cancellation: Arc<CancelToken>) -> (Payload, MediaSendReceipt) {
        let completion = Arc::new(UploadCompletion::default());
        let source = UploadStream {
            file: Arc::clone(&self.file),
            reader: Some(UploadReader {
                file: None,
                length: 0,
                hash: Sha256::new(),
            }),
            pending: None,
            cancel: cancellation,
            done: false,
            completion: Arc::clone(&completion),
        };
        let payload = Payload::Stream {
            source: Box::pin(source),
            total: Some(self.file.length),
        };
        (payload, MediaSendReceipt { completion })
    }
}

fn upload_batch(
    batch: Vec<MediaUploadInput>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(Vec<EncodeChange<File>>, Vec<MediaUploadRecord>), JobError> {
    if batch.len() > 25 {
        return Err(upload_source_error("upload batch exceeds the change count limit"));
    }
    let mut sources = Vec::with_capacity(batch.len());
    let mut records = Vec::with_capacity(batch.len());
    let mut length = 0u64;
    for change in batch {
        check_cancel(cancelled, Method::UploadChanges)?;
        // Test the fill target before each position: one eligible large file
        // can finish the batch, and deletions still occupy their original index.
        if length > 2_621_440 {
            return Err(upload_source_error("upload batch continues past the fill target"));
        }
        let (name, stamp, source) = match change {
            MediaUploadInput::Put {
                name,
                file,
                stamp,
                expected,
            } => (name, stamp, Some((file, expected))),
            MediaUploadInput::Remove { name, stamp } => (name, stamp, None),
        };
        if name.is_empty() || name.len() > 255 || name.contains(['/', '\0']) {
            return Err(upload_source_error("upload batch contains an invalid file name"));
        }
        let (source, expected) = if let Some((mut file, expected)) = source {
            let metadata = file
                .metadata()
                .map_err(|err| local_error(Method::UploadChanges, "checking upload file", &err))?;
            let eligible = 1..=MAX_DOWNLOAD_ENTRY_BYTES;
            if !metadata.is_file() || !eligible.contains(&metadata.len()) || !eligible.contains(&expected.length) {
                return Err(upload_source_error("upload file requires registration again"));
            }
            file.rewind()
                .map_err(|err| local_error(Method::UploadChanges, "seeking upload file", &err))?;
            length += expected.length;
            (
                EncodeChange::Add {
                    name: name.clone(),
                    source: file,
                },
                Some(expected),
            )
        } else {
            (EncodeChange::Delete { name: name.clone() }, None)
        };
        records.push(MediaUploadRecord {
            name,
            stamp,
            expected,
            encoded: None,
        });
        sources.push(source);
    }
    Ok((sources, records))
}

fn upload_file_digest(file: &mut NamedTempFile, cancelled: &dyn Fn() -> bool) -> Result<(u64, [u8; 32]), JobError> {
    let method = Method::UploadChanges;
    file.flush()
        .map_err(|err| local_error(method, "flushing upload ZIP", &err))?;
    let expected = file
        .as_file()
        .metadata()
        .map_err(|err| local_error(method, "measuring upload ZIP", &err))?
        .len();
    file.as_file_mut()
        .rewind()
        .map_err(|err| local_error(method, "seeking upload ZIP", &err))?;
    let mut length = 0u64;
    let mut hash = Sha256::new();
    let mut buffer = [0; COPY_CHUNK];
    loop {
        check_cancel(cancelled, method)?;
        let count = match file.as_file_mut().read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(local_error(method, "hashing upload ZIP", &err)),
        };
        length += count as u64;
        if length > expected {
            return Err(upload_source_error("upload ZIP grew while hashing"));
        }
        hash.update(&buffer[..count]);
    }
    if length != expected {
        return Err(upload_source_error("upload ZIP shortened while hashing"));
    }
    Ok((length, hash.finalize().into()))
}

/// Writes and finishes a Stored ZIP before making it sendable. The shared
/// encoder hashes the bytes it copies. Captured lengths retain the batch fill
/// rule and 1..100 MiB eligibility; the Y10.2 body limit remains unchanged.
pub async fn prepare_media_upload(
    selection: Vec<MediaUploadInput>,
    directory: PathBuf,
    cancellation: Arc<CancelToken>,
) -> Result<PreparedMediaUpload, JobError> {
    file_work(cancellation, Method::UploadChanges, move |cancelled| {
        let (mut sources, mut records) = upload_batch(selection, cancelled)?;
        let mut file = tempfile::Builder::new()
            .prefix("media-upload-")
            .tempfile_in(directory)
            .map_err(|err| local_error(Method::UploadChanges, "creating upload ZIP", &err))?;
        let (_, observed) = encode_zip(file.as_file_mut(), &mut sources, true, cancelled)?;
        for (record, observed) in records.iter_mut().zip(observed) {
            record.encoded = observed;
        }
        let (length, sha256) = upload_file_digest(&mut file, cancelled)?;
        Ok(PreparedMediaUpload {
            file: Arc::new(UploadFile { file, length, sha256 }),
            records,
        })
    })
    .await
}

/// Source outcome and file-worker drain, independent of the transport error.
pub struct MediaSendReceipt {
    completion: Arc<UploadCompletion>,
}

impl MediaSendReceipt {
    /// Call after request termination. An abandoned request must drop its
    /// payload first; an unconsumed live payload can keep this wait pending.
    pub async fn drain(&self) -> Result<(), JobError> {
        loop {
            let notified = self.completion.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(result) = self.completion.result() {
                return result;
            }
            notified.await;
        }
    }
}

#[derive(Default)]
struct UploadCompletion {
    status: Mutex<UploadStatus>,
    changed: Notify,
}

#[derive(Default)]
struct UploadStatus {
    active: usize,
    result: Option<Result<(), JobError>>,
}

impl UploadCompletion {
    fn result(&self) -> Option<Result<(), JobError>> {
        let state = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        if state.active != 0 {
            return None;
        }
        state.result.clone()
    }

    fn finish(&self, result: Result<(), JobError>) {
        let mut state = self.status.lock().unwrap_or_else(PoisonError::into_inner);
        if state.result.is_none() {
            state.result = Some(result);
        }
        drop(state);
        self.changed.notify_waiters();
    }

    fn start(self: &Arc<Self>, file: Arc<UploadFile>) -> UploadReadGuard {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).active += 1;
        UploadReadGuard {
            file: Some(file),
            completion: Arc::clone(self),
        }
    }
}

struct UploadReadGuard {
    file: Option<Arc<UploadFile>>,
    completion: Arc<UploadCompletion>,
}

impl Drop for UploadReadGuard {
    fn drop(&mut self) {
        // Cleanup precedes the drain notification, even for an abandoned read.
        drop(self.file.take());
        self.completion
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .active -= 1;
        self.completion.changed.notify_waiters();
    }
}

struct UploadReader {
    file: Option<File>,
    length: u64,
    hash: Sha256,
}

type UploadReadResult = Result<(UploadReader, Option<Bytes>), JobError>;

struct UploadStream {
    file: Arc<UploadFile>,
    reader: Option<UploadReader>,
    pending: Option<BoxFuture<'static, UploadReadResult>>,
    cancel: Arc<CancelToken>,
    done: bool,
    completion: Arc<UploadCompletion>,
}

impl UploadStream {
    fn read_next(&mut self) {
        let mut reader = self.reader.take().expect("upload reader available between reads");
        let owner = self.completion.start(Arc::clone(&self.file));
        let cancel = Arc::clone(&self.cancel);
        self.pending = Some(Box::pin(file_work(cancel, Method::UploadChanges, move |cancelled| {
            let upload = owner.file.as_ref().expect("read worker owns the upload file");
            check_cancel(cancelled, Method::UploadChanges)?;
            if reader.file.is_none() {
                let mut file = upload
                    .file
                    .reopen()
                    .map_err(|err| local_error(Method::UploadChanges, "opening upload ZIP stream", &err))?;
                file.rewind()
                    .map_err(|err| local_error(Method::UploadChanges, "seeking upload ZIP stream", &err))?;
                reader.file = Some(file);
            }
            let chunk = read_upload_chunk(
                reader.file.as_mut().expect("upload stream file is open"),
                &mut reader.length,
                &mut reader.hash,
                upload.length,
                upload.sha256,
                cancelled,
            )?;
            Ok((reader, chunk))
        })));
    }
}

impl Stream for UploadStream {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        if self.pending.is_none() {
            self.read_next();
        }
        let pending = self.pending.as_mut().expect("upload file read is pending");
        let result = match std::future::Future::poll(pending.as_mut(), cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        self.pending = None;
        match result {
            Ok((reader, Some(chunk))) => {
                self.reader = Some(reader);
                Poll::Ready(Some(Ok(chunk)))
            }
            Ok((reader, None)) => {
                self.reader = Some(reader);
                self.done = true;
                self.completion.finish(Ok(()));
                Poll::Ready(None)
            }
            Err(err) => {
                self.done = true;
                self.completion.finish(Err(err.clone()));
                let kind = if err.kind == ErrorKind::Cancelled {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::Other
                };
                Poll::Ready(Some(Err(io::Error::new(kind, err))))
            }
        }
    }
}

impl Drop for UploadStream {
    fn drop(&mut self) {
        let kind = if self.cancel.is_cancelled() {
            ErrorKind::Cancelled
        } else {
            ErrorKind::Interrupted
        };
        let result = JobError::new(
            kind,
            Some(Method::UploadChanges),
            "upload stream dropped before verified EOF",
        );
        self.completion.finish(Err(result));
    }
}

fn read_upload_chunk<R: Read>(
    source: &mut R,
    length: &mut u64,
    hash: &mut Sha256,
    expected_length: u64,
    expected_sha256: [u8; 32],
    cancelled: &dyn Fn() -> bool,
) -> Result<Option<Bytes>, JobError> {
    let method = Method::UploadChanges;
    let mut buffer = vec![0; COPY_CHUNK];
    let count = loop {
        check_cancel(cancelled, method)?;
        match source.read(&mut buffer) {
            Ok(count) => break count,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(local_error(method, "reading upload ZIP stream", &err)),
        }
    };
    check_cancel(cancelled, method)?;
    if count == 0 {
        let sha256: [u8; 32] = hash.clone().finalize().into();
        if *length != expected_length || sha256 != expected_sha256 {
            return Err(upload_source_error("upload ZIP stream length or digest changed"));
        }
        return Ok(None);
    }
    *length += count as u64;
    if *length > expected_length {
        return Err(upload_source_error("upload ZIP stream exceeds measured length"));
    }
    hash.update(&buffer[..count]);
    buffer.truncate(count);
    Ok(Some(Bytes::from(buffer)))
}

#[cfg(test)]
mod tests {
    use super::super::META_ENTRY;
    use super::*;
    use std::cell::Cell;
    use zip::{ZipWriter, write::SimpleFileOptions};

    struct Full {
        written: usize,
    }
    impl Write for Full {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.written >= COPY_CHUNK {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            self.written += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn receive_disk_full_and_mid_chunk_cancel_are_typed() {
        let bytes = vec![0; 2 * COPY_CHUNK];
        let err = write_response(&mut Full { written: 0 }, &bytes, &|| false).unwrap_err();
        assert!(matches!(err.kind, ErrorKind::DiskFull { .. }));
        let calls = Cell::new(0);
        let check = || {
            calls.set(calls.get() + 1);
            calls.get() >= 2
        };
        assert_eq!(
            write_response(&mut io::sink(), &bytes, &check).unwrap_err().kind,
            ErrorKind::Cancelled
        );
    }

    #[test]
    fn cancellation_during_production_decode_drops_earlier_staged_file() {
        let directory = tempfile::tempdir().unwrap();
        let mut source = tempfile::NamedTempFile::new().unwrap();
        let mut zip = ZipWriter::new(source.as_file_mut());
        for entry in ["0", "1", META_ENTRY] {
            zip.start_file(entry, SimpleFileOptions::default()).unwrap();
            zip.write_all(if entry == META_ENTRY {
                br#"{"0":"a","1":"b"}"#.as_slice()
            } else {
                b"bytes"
            })
            .unwrap();
        }
        zip.finish().unwrap();
        let length = source.as_file().metadata().unwrap().len();
        let check = || std::fs::read_dir(directory.path()).unwrap().count() >= 2;
        let result = decode_file(
            source.as_file_mut(),
            length,
            directory.path(),
            &["a".into(), "b".into()],
            &check,
        );
        assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        let calls = Cell::new(0);
        let check_directory = || {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        };
        let result = decode_file(
            source.as_file_mut(),
            length,
            directory.path(),
            &["a".into(), "b".into()],
            &check_directory,
        );
        assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn send_read_error_preserves_local_category_for_the_drain_receipt() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::from(io::ErrorKind::StorageFull))
            }
        }
        let err = read_upload_chunk(&mut Broken, &mut 0, &mut Sha256::new(), 1, [0; 32], &|| false).unwrap_err();
        assert!(matches!(err.kind, ErrorKind::DiskFull { .. }));
        assert_eq!(err.method, Some(Method::UploadChanges));
    }
}

#[cfg(test)]
mod drain_tests {
    use super::*;
    use meguri_engine::ThreadRegistry;
    use std::sync::mpsc;

    #[test]
    fn cancelled_file_work_does_not_enter_the_blocking_closure() {
        let cancellation = CancelToken::new();
        cancellation.cancel();
        let entered = Arc::new(AtomicBool::new(false));
        let worker_entered = Arc::clone(&entered);
        let result: Result<(), JobError> =
            crate::transport::runtime::block_on(&ThreadRegistry::new(), &CancelToken::default(), async {
                Ok(file_work(cancellation, Method::DownloadFiles, move |_| {
                    worker_entered.store(true, Ordering::Release);
                    Ok(())
                })
                .await)
            })
            .unwrap();
        assert_eq!(result.unwrap_err().kind, ErrorKind::Cancelled);
        assert!(!entered.load(Ordering::Acquire));
    }

    #[test]
    fn dropped_send_waits_for_owned_file_worker_before_reporting_drain() {
        let directory = tempfile::tempdir().unwrap();
        let file = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        let path = file.path().to_owned();
        let artifact = Arc::new(UploadFile {
            file,
            length: 0,
            sha256: Sha256::digest([]).into(),
        });
        let completion = Arc::new(UploadCompletion::default());
        let receipt = MediaSendReceipt {
            completion: completion.clone(),
        };
        let lease = completion.start(artifact); // Last artifact owner is now worker-owned.
        let (started, entered) = mpsc::channel();
        let (release, finish) = mpsc::channel();
        crate::transport::runtime::block_on(&ThreadRegistry::new(), &CancelToken::default(), async {
            let task = file_work(CancelToken::new(), Method::UploadChanges, move |_| {
                started.send(()).unwrap();
                finish.recv().unwrap();
                drop(lease);
                Ok(())
            });
            tokio::pin!(task);
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(task.as_mut(), cx).is_pending());
                Poll::Ready(())
            })
            .await;
            entered.recv().unwrap();
            assert!(path.exists());
            completion.finish(Err(JobError::new(
                ErrorKind::Interrupted,
                Some(Method::UploadChanges),
                "source abandoned",
            )));
            let drain = receipt.drain();
            tokio::pin!(drain);
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(drain.as_mut(), cx).is_pending());
                Poll::Ready(())
            })
            .await;
            release.send(()).unwrap();
            task.await.unwrap();
            assert_eq!(drain.await.unwrap_err().kind, ErrorKind::Interrupted);
            assert!(!path.exists());
            Ok(())
        })
        .unwrap();
    }
}
