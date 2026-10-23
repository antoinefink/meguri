#![allow(dead_code)]
use bytes::Bytes;
use meguri_engine::CancelToken;
use meguri_model::rng::SeededRandom;
use meguri_sync::{
    Endpoint, HostKey, Job, JobOptions, NetworkCause, NetworkError, ServerKind, SyncError,
    transport::{BoxFuture, RawBody, RawStream, SyncTransport, TransportRequest, TransportResponse},
    wire::media::MediaDigest,
};
use sha1::{Digest, Sha1};
use std::{
    fs::File,
    io::{Read, Seek, Write},
    path::Path,
    sync::Arc,
};

pub fn content(bytes: &[u8]) -> MediaDigest {
    MediaDigest {
        length: bytes.len() as u64,
        sha1: Sha1::digest(bytes).into(),
    }
}
pub fn source(dir: &Path, name: &str, bytes: &[u8]) -> File {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    File::open(path).unwrap()
}
pub fn zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for &(name, bytes, deflated) in entries {
        let method = if deflated {
            zip::CompressionMethod::Deflated
        } else {
            zip::CompressionMethod::Stored
        };
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
pub fn disk_zip(dir: &Path, bytes: &[u8]) -> File {
    source(dir, "input.zip", bytes)
}
pub async fn next(stream: &mut RawStream) -> Option<std::io::Result<Bytes>> {
    std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
}
pub fn count(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

/// The measured receive tests keep only on-disk compressed bytes. Each body
/// poll allocates one bounded incoming chunk inside the measurement interval.
pub struct FileTransport {
    pub compressed: File,
    pub declared: u64,
    pub fail_after: Option<usize>,
    pub cancel_after: Option<(usize, Arc<CancelToken>)>,
    pub drain_upload: bool,
    pub fail_upload_after: Option<usize>,
}
impl FileTransport {
    pub fn new(zip: &mut File, dir: &Path) -> Self {
        let path = dir.join("response.zst");
        let mut output = File::create(&path).unwrap();
        zip.rewind().unwrap();
        zstd::stream::copy_encode(&mut *zip, &mut output, 1).unwrap();
        Self {
            compressed: File::open(path).unwrap(),
            declared: zip.metadata().unwrap().len(),
            fail_after: None,
            cancel_after: None,
            drain_upload: false,
            fail_upload_after: None,
        }
    }
    pub fn job(self) -> Job {
        let options = JobOptions::new(Endpoint::parse("http://media.test/").unwrap(), ServerKind::SelfHosted);
        Job::with_transport(
            &options,
            Some(HostKey::new("test")),
            &SeededRandom::new(110),
            Box::new(self),
        )
        .unwrap()
    }
}
impl SyncTransport for FileTransport {
    fn post(&self, mut request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        Box::pin(async move {
            if self.drain_upload {
                let mut count = 0;
                while let Some(piece) = next(&mut request.body).await {
                    piece.map_err(|_| network_failure())?;
                    count += 1;
                    if self.fail_upload_after.is_some_and(|n| count >= n) {
                        return Err(network_failure());
                    }
                }
            }
            let mut file = self.compressed.try_clone().unwrap();
            file.rewind().unwrap();
            Ok(TransportResponse {
                status: 200,
                headers: vec![("anki-original-size".into(), self.declared.to_string().into_bytes())],
                body: Box::new(FileBody {
                    file,
                    chunks: 0,
                    fail_after: self.fail_after,
                    cancel_after: self.cancel_after.clone(),
                }),
            })
        })
    }
}
fn network_failure() -> SyncError {
    SyncError::Network(NetworkError {
        cause: NetworkCause::Connect,
        head_arrived: true,
        detail: "scripted media body failure".into(),
    })
}
struct FileBody {
    file: File,
    chunks: usize,
    fail_after: Option<usize>,
    cancel_after: Option<(usize, Arc<CancelToken>)>,
}
impl RawBody for FileBody {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>> {
        Box::pin(async move {
            if self.fail_after.is_some_and(|n| self.chunks >= n) {
                return Err(network_failure());
            }
            if let Some((n, token)) = &self.cancel_after
                && self.chunks >= *n
            {
                token.cancel();
            }
            let mut buffer = vec![0; 64 * 1024];
            let n = self.file.read(&mut buffer).unwrap();
            self.chunks += 1;
            buffer.truncate(n);
            Ok(if n == 0 { None } else { Some(Bytes::from(buffer)) })
        })
    }
}

/// Bounded deterministic noise, generated directly into files/ZIP entries.
pub fn noise(output: &mut impl Write, length: usize) -> MediaDigest {
    let mut state = 0x4b81_7c99_1920_51adu64;
    let mut hash = Sha1::new();
    let mut buffer = [0; 64 * 1024];
    let mut left = length;
    while left != 0 {
        let n = left.min(buffer.len());
        for byte in &mut buffer[..n] {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state as u8;
        }
        output.write_all(&buffer[..n]).unwrap();
        hash.update(&buffer[..n]);
        left -= n;
    }
    MediaDigest {
        length: length as u64,
        sha1: hash.finalize().into(),
    }
}
