//! The `SyncTransport` seam (KTD14): one call that posts a compressed body
//! to a URL with the `anki-sync` header and returns the status, the headers
//! and the body as it arrives. Protocol code ([`crate::job`]) never sees
//! reqwest, so a URLSession background transfer can stand in later. The
//! reqwest implementation is [`http::HttpTransport`].

pub(crate) mod framing;
pub mod http;
pub mod runtime;

use std::{future::Future, io, pin::Pin, time::Duration};

use bytes::Bytes;

use crate::error::SyncError;

/// A boxed future the seam returns.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A stream of body bytes.
pub type RawStream = Pin<Box<dyn futures_core::Stream<Item = io::Result<Bytes>> + Send>>;

/// One protocol v11 method (specs/sync/transport.md T1): the collection
/// methods, then, from [`Method::MediaBegin`] on, the media methods.
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
    /// Media `begin`, the first media method.
    MediaBegin,
    MediaChanges,
    UploadChanges,
    DownloadFiles,
    MediaSanity,
}

/// T1's method names, case-sensitive, one per [`Method`] variant in its
/// order.
const NAMES: [&str; 17] = [
    "hostKey",
    "meta",
    "start",
    "applyGraves",
    "applyChanges",
    "chunk",
    "applyChunk",
    "sanityCheck2",
    "finish",
    "abort",
    "upload",
    "download",
    "begin",
    "mediaChanges",
    "uploadChanges",
    "downloadFiles",
    "mediaSanity",
];

impl Method {
    /// The method's name, spelled as the server expects it.
    pub fn name(self) -> &'static str {
        NAMES[self as usize]
    }

    /// Media methods sit under `msync/`, the others under `sync/` (T1, L2).
    pub fn is_media(self) -> bool {
        self as usize >= Method::MediaBegin as usize
    }

    /// The name an error reports (errors.md E1): media methods with their
    /// `msync/` prefix (`msync/begin`), the others bare (`meta`).
    pub fn qualified_name(self) -> &'static str {
        match self {
            Method::MediaBegin => "msync/begin",
            Method::MediaChanges => "msync/mediaChanges",
            Method::UploadChanges => "msync/uploadChanges",
            Method::DownloadFiles => "msync/downloadFiles",
            Method::MediaSanity => "msync/mediaSanity",
            other => other.name(),
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
    pub fn json<T: serde::Serialize + ?Sized>(arguments: &T) -> serde_json::Result<Payload> {
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
