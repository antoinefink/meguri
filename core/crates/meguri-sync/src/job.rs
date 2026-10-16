//! A sync job (specs/sync/transport.md T4, T6, T7, T9; login-endpoints.md
//! L4): one login, status check, normal, one-way or media sync. It holds what
//! the caller gave it (base, server kind, host key, stall timeout, proxy,
//! cancel token) and the session key it drew; it keeps nothing after it ends
//! (R1502). Every request goes through [`Job::request`].

use std::fmt;
#[cfg(feature = "test-hooks")]
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use meguri_engine::{CancelToken, ProgressSink};
use meguri_model::rng::RandomSource;

use crate::endpoint::{self, Endpoint, EndpointRule, ServerKind};
use crate::error::{InvalidInput, NetworkCause, NetworkError, RedirectError, RedirectRefusal, SyncError};
use crate::identity::client_identity;
use crate::transport::framing::{self, CompressedBody, Decompressor, Source};
use crate::transport::http::HttpTransport;
use crate::transport::runtime::with_cancel;
use crate::transport::{Method, Payload, RawBody, SyncTransport, TransportRequest, TransportResponse};

/// The host key a login returns (login-endpoints.md L7): opaque, compared only
/// for equality, sent only as the header's `k`, never logged.
#[derive(Clone, PartialEq, Eq)]
pub struct HostKey(String);

impl HostKey {
    pub fn new(key: impl Into<String>) -> HostKey {
        HostKey(key.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for HostKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("HostKey(..)")
    }
}

/// The proxy a job goes through (T10): an `http://<host>:<port>` URL the app
/// resolved from the system settings for the job's base, and optional basic
/// credentials.
#[derive(Clone, PartialEq, Eq)]
pub struct Proxy {
    url: String,
    credentials: Option<(String, String)>,
}

impl Proxy {
    pub fn new(url: impl Into<String>) -> Proxy {
        Proxy {
            url: url.into(),
            credentials: None,
        }
    }

    pub fn with_credentials(mut self, user: impl Into<String>, password: impl Into<String>) -> Proxy {
        self.credentials = Some((user.into(), password.into()));
        self
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub(crate) fn credentials(&self) -> Option<(&str, &str)> {
        self.credentials.as_ref().map(|(u, p)| (u.as_str(), p.as_str()))
    }
}

impl fmt::Debug for Proxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Proxy")
            .field("url", &self.url)
            .field("credentials", &self.credentials.as_ref().map(|(user, _)| user))
            .finish()
    }
}

/// What every job is given (L4): base, server kind, stall timeout, proxy and
/// cancel token. The host key goes to [`Job::new`].
#[derive(Clone, Debug)]
pub struct JobOptions {
    pub(crate) base: Endpoint,
    pub(crate) kind: ServerKind,
    pub(crate) stall_timeout: Duration,
    pub(crate) proxy: Option<Proxy>,
    pub(crate) cancel: Arc<CancelToken>,
    #[cfg(feature = "test-hooks")]
    pub(crate) pins: Vec<(String, SocketAddr)>,
    #[cfg(feature = "test-hooks")]
    pub(crate) roots: Vec<Vec<u8>>,
}

impl JobOptions {
    /// The stall timeout when the app sets none (R1504).
    pub const DEFAULT_STALL_TIMEOUT: Duration = Duration::from_secs(60);

    /// A job on `base` with the default stall timeout, no proxy and a fresh
    /// cancel token.
    pub fn new(base: Endpoint, kind: ServerKind) -> JobOptions {
        JobOptions {
            base,
            kind,
            stall_timeout: JobOptions::DEFAULT_STALL_TIMEOUT,
            proxy: None,
            cancel: CancelToken::new(),
            #[cfg(feature = "test-hooks")]
            pins: Vec::new(),
            #[cfg(feature = "test-hooks")]
            roots: Vec::new(),
        }
    }

    /// The stall timeout (T8); zero is refused when the job starts.
    pub fn stall_timeout(mut self, timeout: Duration) -> JobOptions {
        self.stall_timeout = timeout;
        self
    }

    pub fn proxy(mut self, proxy: Proxy) -> JobOptions {
        self.proxy = Some(proxy);
        self
    }

    pub fn cancel(mut self, token: Arc<CancelToken>) -> JobOptions {
        self.cancel = token;
        self
    }

    pub fn cancel_token(&self) -> &Arc<CancelToken> {
        &self.cancel
    }

    /// Test hook (T12): resolves `name` to `address` (whose port is ignored:
    /// the URL's port is used). The address must pass the endpoint guard.
    #[cfg(feature = "test-hooks")]
    pub fn pin_host(mut self, name: &str, address: SocketAddr) -> JobOptions {
        self.pins.push((name.to_ascii_lowercase(), address));
        self
    }

    /// Test hook (T12): trusts this DER certificate as a root in addition to
    /// the platform's.
    #[cfg(feature = "test-hooks")]
    pub fn trust_root(mut self, der: Vec<u8>) -> JobOptions {
        self.roots.push(der);
        self
    }
}

/// One sync job. Its requests share one session key and one base, which a
/// followed redirect replaces for the rest of the job (T4, T7).
pub struct Job {
    transport: Box<dyn SyncTransport>,
    base: Endpoint,
    kind: ServerKind,
    host_key: Option<HostKey>,
    session_key: String,
    stall_timeout: Duration,
    cancel: Arc<CancelToken>,
    progress: Option<ProgressSink>,
    /// A redirect moved `base` (T7 allows one per job).
    followed: bool,
    /// The redirect target once `meta` answered there: until then the move
    /// is not the job's result, and a refused second redirect leaves none.
    new_endpoint: Option<Endpoint>,
}

impl Job {
    /// A job over the reqwest transport. `host_key` is `None` only for a
    /// login. Nothing connects until the first request.
    pub fn new(options: &JobOptions, host_key: Option<HostKey>, random: &dyn RandomSource) -> Result<Job, SyncError> {
        check_stall_timeout(options.stall_timeout)?;
        let transport = HttpTransport::new(options)?;
        Job::with_transport(options, host_key, random, Box::new(transport))
    }

    /// A job over another transport (the seam, KTD14).
    pub fn with_transport(
        options: &JobOptions,
        host_key: Option<HostKey>,
        random: &dyn RandomSource,
        transport: Box<dyn SyncTransport>,
    ) -> Result<Job, SyncError> {
        check_stall_timeout(options.stall_timeout)?;
        Ok(Job {
            transport,
            base: options.base.clone(),
            kind: options.kind,
            host_key,
            session_key: session_key(random),
            stall_timeout: options.stall_timeout,
            cancel: Arc::clone(&options.cancel),
            progress: None,
            followed: false,
            new_endpoint: None,
        })
    }

    /// Where the transfer counts of every later request go (T15).
    pub fn set_progress(&mut self, progress: ProgressSink) {
        self.progress = Some(progress);
    }

    /// The base requests go to: the given one, or the redirect target.
    pub fn base(&self) -> &Endpoint {
        &self.base
    }

    pub fn kind(&self) -> ServerKind {
        self.kind
    }

    /// The job's session key (T4): 8 characters of `0-9 A-Z a-z`.
    pub fn session_key(&self) -> &str {
        &self.session_key
    }

    /// The redirect target this job moved to, as normalised, once the
    /// repeated `meta` was answered there (T7): the job's result carries it as
    /// `newEndpoint` and the app stores it (AE70). `None` after a refused
    /// redirect, a second one included.
    pub fn new_endpoint(&self) -> Option<&Endpoint> {
        self.new_endpoint.as_ref()
    }

    /// Sends one request and returns its checked, decompressing body (T6).
    /// A 308 on `meta` is followed once when T7 allows it; every other 3xx,
    /// 4xx or 5xx is an error. Runs under the job's cancel token (T9).
    pub async fn request(&mut self, method: Method, payload: Payload) -> Result<ResponseBody, SyncError> {
        let cancel = Arc::clone(&self.cancel);
        with_cancel(&cancel, self.send(method, payload)).await
    }

    async fn send(&mut self, method: Method, payload: Payload) -> Result<ResponseBody, SyncError> {
        let (first, total, resend) = match payload {
            Payload::Json(json) => {
                let json = Bytes::from(json);
                (Source::Bytes(json.clone()), Some(json.len() as u64), Some(json))
            }
            Payload::Stream { source, total } => (Source::Stream(source), total, None),
        };
        let mut source = first;
        loop {
            let body = CompressedBody::new(source, total, self.progress.clone()).map_err(|err| {
                SyncError::Network(NetworkError {
                    cause: NetworkCause::RequestBody,
                    head_arrived: false,
                    detail: err.to_string(),
                })
            })?;
            // The login's `k` is empty whatever key the job holds (T2).
            let key = match (&self.host_key, method) {
                (Some(key), m) if m != Method::HostKey => key.as_str(),
                _ => "",
            };
            let request = TransportRequest {
                url: self.base.method_url(method),
                sync_header: framing::sync_header(key, client_identity(), &self.session_key),
                body: Box::pin(body),
                stall_timeout: self.stall_timeout,
            };
            let response = self.transport.post(request).await?;
            match response.status {
                200..=299 => {
                    let body = ResponseBody::new(response, Arc::clone(&self.cancel), self.progress.clone())?;
                    if self.followed && self.new_endpoint.is_none() {
                        // The repeated `meta` was answered at the target (T7).
                        self.new_endpoint = Some(self.base.clone());
                    }
                    return Ok(body);
                }
                code @ 300..=399 => {
                    let location = response.header_values("location").first().map(|v| v.to_vec());
                    // The redirect's body is never read.
                    drop(response);
                    source = Source::Bytes(self.follow(method, code, location.as_deref(), resend.as_ref())?);
                }
                _ => return Err(status_error(response).await),
            }
        }
    }

    /// T7's checks for a 3xx. On success the job moves to the target, and
    /// the request's body is what to send there.
    fn follow(
        &mut self,
        method: Method,
        code: u16,
        location: Option<&[u8]>,
        resend: Option<&Bytes>,
    ) -> Result<Bytes, SyncError> {
        let text = location.map(|raw| String::from_utf8_lossy(raw).into_owned());
        let refuse = |refusal| {
            SyncError::Redirect(RedirectError {
                code,
                location: text.clone(),
                refusal,
            })
        };
        if method != Method::Meta {
            return Err(refuse(RedirectRefusal::NotMeta));
        }
        if code != 308 {
            return Err(refuse(RedirectRefusal::NotPermanent));
        }
        if self.followed {
            return Err(refuse(RedirectRefusal::Repeated));
        }
        let Some(raw) = location else {
            return Err(refuse(RedirectRefusal::MissingLocation));
        };
        let Ok(raw) = std::str::from_utf8(raw) else {
            return Err(refuse(RedirectRefusal::Target(EndpointRule::Unparseable)));
        };
        let Some(body) = resend else {
            return Err(refuse(RedirectRefusal::NotResendable));
        };
        self.base = endpoint::follow_redirect(&self.base, self.kind, raw).map_err(refuse)?;
        self.followed = true;
        Ok(body.clone())
    }
}

fn check_stall_timeout(timeout: Duration) -> Result<(), SyncError> {
    if timeout.is_zero() {
        Err(SyncError::InvalidInput(InvalidInput::StallTimeout))
    } else {
        Ok(())
    }
}

/// A new session key (T4): 8 characters of `0-9 A-Z a-z` from the job's
/// random source.
fn session_key(random: &dyn RandomSource) -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    (0..8)
        .map(|_| char::from(ALPHABET[(random.next_u64() % 62) as usize]))
        .collect()
}

/// A failed status (T6.1): its code and the start of its body as the reason;
/// an empty reason when reading the body fails.
async fn status_error(mut response: TransportResponse) -> SyncError {
    let code = response.status;
    let mut text = Vec::new();
    loop {
        match response.body.next().await {
            Ok(Some(chunk)) => {
                let room = framing::REASON_LIMIT - text.len();
                text.extend_from_slice(&chunk[..chunk.len().min(room)]);
                if text.len() >= framing::REASON_LIMIT {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) => {
                text.clear();
                break;
            }
        }
    }
    SyncError::Status {
        code,
        reason: framing::reason_text(&text),
    }
}

/// A successful response's body, decompressed as it arrives and held to its
/// declared size (T6.2). It holds at most one raw chunk and one decoded piece:
/// the caller pulls the body at its own pace.
pub struct ResponseBody {
    /// `None` once the body ended or failed: a failure closes the connection
    /// at once (T6.2, T9).
    body: Option<Box<dyn RawBody>>,
    decoder: Decompressor,
    /// Received bytes the decoder has not consumed yet.
    pending: Bytes,
    ended: bool,
    failed: Option<SyncError>,
    cancel: Arc<CancelToken>,
    progress: Option<ProgressSink>,
}

impl ResponseBody {
    /// Checks `anki-original-size` before any body byte is read.
    fn new(
        response: TransportResponse,
        cancel: Arc<CancelToken>,
        progress: Option<ProgressSink>,
    ) -> Result<ResponseBody, SyncError> {
        let declared =
            framing::original_size(&response.header_values(framing::ORIGINAL_SIZE)).map_err(SyncError::Protocol)?;
        Ok(ResponseBody {
            body: Some(response.body),
            decoder: Decompressor::new(declared).map_err(SyncError::Protocol)?,
            pending: Bytes::new(),
            ended: false,
            failed: None,
            cancel,
            progress,
        })
    }

    /// The decompressed size the server declared.
    pub fn declared_size(&self) -> u64 {
        self.decoder.declared()
    }

    /// The next decompressed bytes, `None` once the whole declared size
    /// arrived. An error stays: later calls return it again.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, SyncError> {
        if let Some(failed) = &self.failed {
            return Err(failed.clone());
        }
        let cancel = Arc::clone(&self.cancel);
        let result = with_cancel(&cancel, self.next_chunk()).await;
        if let Err(err) = &result {
            self.body = None;
            self.pending = Bytes::new();
            self.failed = Some(err.clone());
        }
        result
    }

    async fn next_chunk(&mut self) -> Result<Option<Bytes>, SyncError> {
        loop {
            if let Some(piece) = self.decoder.step(&mut self.pending).map_err(SyncError::Protocol)? {
                if let Some(progress) = &self.progress {
                    progress.report(self.decoder.produced(), Some(self.decoder.declared()));
                }
                return Ok(Some(piece));
            }
            if self.ended {
                return Ok(None);
            }
            let Some(body) = self.body.as_mut() else {
                return Ok(None);
            };
            match body.next().await? {
                Some(raw) => self.pending = raw,
                None => {
                    self.ended = true;
                    self.body = None;
                    self.decoder.finish().map_err(SyncError::Protocol)?;
                }
            }
        }
    }

    /// The whole decompressed body.
    pub async fn bytes(mut self) -> Result<Vec<u8>, SyncError> {
        let mut out = Vec::new();
        while let Some(piece) = self.chunk().await? {
            out.extend_from_slice(&piece);
        }
        Ok(out)
    }
}

impl fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseBody")
            .field("declared", &self.decoder.declared())
            .field("produced", &self.decoder.produced())
            .finish_non_exhaustive()
    }
}
