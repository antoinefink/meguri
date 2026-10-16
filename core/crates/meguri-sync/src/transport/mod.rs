//! The `SyncTransport` seam (KTD14): one call that posts a compressed body
//! to a URL with the `anki-sync` header and returns the status, the headers
//! and the body as it arrives. Protocol code ([`crate::job`]) never sees
//! reqwest, so a URLSession background transfer can stand in later. The
//! reqwest implementation is [`http::HttpTransport`].

pub(crate) mod framing;
pub mod http;
pub mod runtime;

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures_core::Stream;
use serde::Serialize;

use crate::error::SyncError;

/// A boxed future the seam returns.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A stream of body bytes.
pub type RawStream = Pin<Box<dyn Stream<Item = io::Result<Bytes>> + Send>>;

/// One protocol v11 method (specs/sync/transport.md T1). Names are
/// case-sensitive and spelled exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Method {
    /// Login (login-endpoints.md L5).
    HostKey,
    /// The status check; the only method that may follow a redirect (T7).
    Meta,
    Start,
    ApplyGraves,
    ApplyChanges,
    Chunk,
    ApplyChunk,
    SanityCheck2,
    Finish,
    Abort,
    Upload,
    Download,
    /// Media `begin`.
    MediaBegin,
    MediaChanges,
    UploadChanges,
    DownloadFiles,
    MediaSanity,
}

impl Method {
    /// The path after the base: `sync/<name>` or `msync/<name>` (L2).
    pub fn path(self) -> &'static str {
        match self {
            Method::HostKey => "sync/hostKey",
            Method::Meta => "sync/meta",
            Method::Start => "sync/start",
            Method::ApplyGraves => "sync/applyGraves",
            Method::ApplyChanges => "sync/applyChanges",
            Method::Chunk => "sync/chunk",
            Method::ApplyChunk => "sync/applyChunk",
            Method::SanityCheck2 => "sync/sanityCheck2",
            Method::Finish => "sync/finish",
            Method::Abort => "sync/abort",
            Method::Upload => "sync/upload",
            Method::Download => "sync/download",
            Method::MediaBegin => "msync/begin",
            Method::MediaChanges => "msync/mediaChanges",
            Method::UploadChanges => "msync/uploadChanges",
            Method::DownloadFiles => "msync/downloadFiles",
            Method::MediaSanity => "msync/mediaSanity",
        }
    }
}

/// What a request sends, before compression (T5).
pub enum Payload {
    /// The compact UTF-8 JSON text of the method's arguments.
    Json(Vec<u8>),
    /// Raw bytes (`upload`'s collection file, `uploadChanges`' zip),
    /// compressed while they are sent and never held twice. `total` is the
    /// raw size, for progress (T15).
    Stream { source: RawStream, total: Option<u64> },
}

impl Payload {
    /// The body of a method without arguments: `{}` (T5, R1507).
    pub fn empty() -> Payload {
        Payload::Json(b"{}".to_vec())
    }

    /// A method's arguments as compact JSON.
    pub fn json<T: Serialize + ?Sized>(arguments: &T) -> serde_json::Result<Payload> {
        serde_json::to_vec(arguments).map(Payload::Json)
    }
}

/// One POST as the transport sends it.
pub struct TransportRequest {
    /// The method URL (L2).
    pub url: String,
    /// The `anki-sync` header value, printable ASCII (T2).
    pub sync_header: String,
    /// The zstd-compressed body, produced as it is sent.
    pub body: RawStream,
    /// The job's stall timeout (T8).
    pub stall_timeout: Duration,
}

/// A response whose head arrived.
pub struct TransportResponse {
    pub status: u16,
    /// Header names in lower case, values as received.
    pub headers: Vec<(String, Vec<u8>)>,
    /// The body as received (not decompressed), still under the stall
    /// watchdog.
    pub body: Box<dyn RawBody>,
}

impl TransportResponse {
    /// Every value of a header, by lower-case name.
    pub fn header_values(&self, name: &str) -> Vec<&[u8]> {
        self.headers
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
            .collect()
    }
}

/// A response body as it arrives.
pub trait RawBody: Send {
    /// The next bytes received, `None` at the end of the body.
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>>;
}

/// Posts one request (KTD14). An implementation applies the stall watchdog
/// (T8) to the request and to the body it returns, follows no redirect and
/// never decompresses.
pub trait SyncTransport: Send + Sync {
    fn post(&self, request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>>;
}
