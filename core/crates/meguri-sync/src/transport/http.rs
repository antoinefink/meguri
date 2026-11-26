//! The reqwest transport (KTD14; specs/sync/transport.md T1, T8, T10, T12):
//! HTTP/1.1 only, no redirects, no retries, no automatic decompression, no
//! proxy but the job's, rustls with the platform verifier, a 30 s connect
//! limit, and the stall watchdog over both directions.

use std::error::Error as StdError;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_core::Stream;
use reqwest::header::{CONTENT_TYPE, HeaderValue};
use tokio::time::{Instant, sleep_until};

use crate::error::{NetworkCause, NetworkError, SyncError};
use crate::job::JobOptions;
use crate::transport::{BoxFuture, RawBody, RawStream, SyncTransport, TransportRequest, TransportResponse};

/// Name lookup, TCP, TLS and a proxy tunnel together (T8).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) struct EditorImageResponse {
    pub bytes: Vec<u8>,
    pub content_type: String,
    pub encoding: String,
}

/// One explicitly selected HTTP(S) editor image. Keep this client separate
/// from sync: GET, redirects and content decoding have different contracts.
pub(crate) async fn get_editor_image(written_url: &str) -> Result<EditorImageResponse, SyncError> {
    use crate::remote_media::failure;
    let mut url = url::Url::parse(written_url).map_err(|e| failure(&e.to_string()))?;
    let client = reqwest::Client::builder()
        .http1_only().no_proxy().retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT).read_timeout(CONNECT_TIMEOUT)
        .tls_version_min(reqwest::tls::Version::TLS_1_2)
        .dns_resolver(Arc::new(SystemResolver))
        .build().map_err(|e| failure(&chain(&e)))?;
    for redirects in 0..=10 {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(failure("Only HTTP and HTTPS image URLs are supported."));
        }
        #[cfg(feature = "test-hooks")]
        guard(url.as_str())?;
        let mut response = client.get(url.clone())
            .header("User-Agent", "Mozilla/5.0 (compatible; Meguri)")
            .header("Accept-Encoding", "gzip, deflate")
            .send().await.map_err(|e| failure(&chain(&e)))?;
        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308)
            && let Some(location) = response.headers().get("location")
        {
            if redirects == 10 { return Err(failure("The image URL redirected too many times.")); }
            url = url.join(location.to_str().map_err(|e| failure(&e.to_string()))?)
                .map_err(|e| failure(&e.to_string()))?;
            continue;
        }
        if status != 200 { return Err(SyncError::Status { code: status, reason: String::new() }); }
        let header = |name| response.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
        let content_type = header("content-type");
        let encoding = header("content-encoding").trim().to_ascii_lowercase();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| failure(&chain(&e)))? {
            bytes.extend_from_slice(&chunk);
        }
        return Ok(EditorImageResponse { bytes, content_type, encoding });
    }
    unreachable!("the redirect bound returns in the loop")
}

/// One job's HTTP client: its proxy and (test builds) its pins and roots are
/// fixed when the job starts.
#[derive(Debug)]
pub struct HttpTransport {
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new(options: &JobOptions) -> Result<HttpTransport, SyncError> {
        let mut builder = reqwest::Client::builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(CONNECT_TIMEOUT)
            .tls_version_min(reqwest::tls::Version::TLS_1_2)
            // The system's name lookup, its failures marked so `classify` can
            // tell them from other connect failures (errors.md E5).
            .dns_resolver(Arc::new(SystemResolver))
            // Neither environment variables nor system settings: only the
            // proxy the job carries (T10).
            .no_proxy();
        if let Some(proxy) = &options.proxy {
            builder = builder.proxy(reqwest_proxy(proxy)?);
        }
        #[cfg(feature = "test-hooks")]
        {
            builder = test_hooks(builder, options)?;
        }
        let client = builder
            .build()
            .map_err(|err| SyncError::Tls(format!("the HTTP client did not start: {}", chain(&err))))?;
        Ok(HttpTransport { client })
    }

    async fn send(&self, request: TransportRequest) -> Result<TransportResponse, SyncError> {
        #[cfg(feature = "test-hooks")]
        guard(&request.url)?;
        let watchdog = Watchdog::new(request.stall_timeout);
        let body_failure = Arc::new(Mutex::new(None));
        let body = WatchedBody {
            inner: request.body,
            watchdog: watchdog.clone(),
            failure: Arc::clone(&body_failure),
        };
        let header = HeaderValue::from_str(&request.sync_header).map_err(|_| {
            // framing::sync_header writes printable ASCII only.
            SyncError::Network(NetworkError {
                cause: NetworkCause::RequestBody,
                head_arrived: false,
                detail: "the anki-sync header is not printable ASCII".to_owned(),
            })
        })?;
        let send = self
            .client
            .post(&request.url)
            .header("anki-sync", header)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"))
            .body(reqwest::Body::wrap_stream(body))
            .send();
        let response = tokio::select! {
            biased;
            sent = send => sent.map_err(|err| classify(&err, false, &body_failure))?,
            () = watchdog.fired() => return Err(SyncError::Timeout),
        };
        watchdog.touch();
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
            .collect();
        Ok(TransportResponse {
            status,
            headers,
            body: Box::new(HttpBody {
                response,
                watchdog,
                body_failure,
            }),
        })
    }
}

impl SyncTransport for HttpTransport {
    fn post(&self, request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        Box::pin(self.send(request))
    }
}

/// The job's proxy as reqwest's (T10): `http://` only; an `https` target
/// goes through `CONNECT`, an `http` one in absolute form.
fn reqwest_proxy(proxy: &crate::job::Proxy) -> Result<reqwest::Proxy, SyncError> {
    // The detail names the rule only: the URL the app passed could carry
    // credentials.
    let invalid = |why: &str| SyncError::InvalidProxy(why.to_owned());
    let url = url::Url::parse(proxy.url()).map_err(|_| invalid("not a URL"))?;
    if url.scheme() != "http" {
        return Err(invalid("only http:// proxies are supported"));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid("no host"));
    }
    #[cfg(feature = "test-hooks")]
    guard(proxy.url())?;
    let mut out = reqwest::Proxy::all(url.as_str()).map_err(|_| invalid("the HTTP client refused it"))?;
    if let Some((user, password)) = proxy.credentials() {
        out = out.basic_auth(user, password);
    }
    Ok(out)
}

/// The test hooks of T12: pinned names (each address passes the endpoint
/// guard) and extra trusted roots.
#[cfg(feature = "test-hooks")]
fn test_hooks(mut builder: reqwest::ClientBuilder, options: &JobOptions) -> Result<reqwest::ClientBuilder, SyncError> {
    for (name, address) in &options.pins {
        guard(&format!("http://{address}/"))?;
        builder = builder.resolve(name, *address);
    }
    if !options.roots.is_empty() {
        let roots = options
            .roots
            .iter()
            .map(|der| reqwest::Certificate::from_der(der))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| SyncError::Tls(format!("a test root does not parse: {err}")))?;
        builder = builder.tls_certs_merge(roots);
    }
    Ok(builder)
}

/// The endpoint guard (R1876): test builds sync only with local servers.
#[cfg(feature = "test-hooks")]
fn guard(url: &str) -> Result<(), SyncError> {
    crate::endpoint_guard::check_endpoint(url)
        .map(|_| ())
        .map_err(|refused| SyncError::EndpointRefused(refused.to_string()))
}

/// The stall watchdog (T8): fires when no progress happened for the whole
/// timeout. Progress is a request body chunk handed to the connection, the
/// response head, or response body bytes read. Its clock is the process's
/// monotonic time, never the collection clock.
#[derive(Clone)]
struct Watchdog(Arc<WatchdogState>);

struct WatchdogState {
    start: Instant,
    timeout: Duration,
    /// Nanoseconds after `start` of the last progress.
    last: AtomicU64,
}

impl Watchdog {
    fn new(timeout: Duration) -> Watchdog {
        Watchdog(Arc::new(WatchdogState {
            start: Instant::now(),
            timeout,
            last: AtomicU64::new(0),
        }))
    }

    fn touch(&self) {
        let since = u64::try_from(self.0.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.0.last.fetch_max(since, Ordering::Relaxed);
    }

    fn deadline(&self) -> Instant {
        self.0.start + Duration::from_nanos(self.0.last.load(Ordering::Relaxed)) + self.0.timeout
    }

    /// Completes once the timeout passed since the last progress.
    async fn fired(&self) {
        loop {
            let deadline = self.deadline();
            if Instant::now() >= deadline {
                return;
            }
            sleep_until(deadline).await;
        }
    }
}

/// The request body as hyper pulls it: each chunk handed over is progress.
struct WatchedBody {
    inner: RawStream,
    watchdog: Watchdog,
    /// The caller's source failed: reported as such, not as the network.
    failure: Arc<Mutex<Option<String>>>,
}

impl Stream for WatchedBody {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>> {
        let polled = self.inner.as_mut().poll_next(cx);
        match &polled {
            Poll::Ready(Some(Ok(_))) => self.watchdog.touch(),
            Poll::Ready(Some(Err(err))) => {
                *self.failure.lock().unwrap_or_else(PoisonError::into_inner) = Some(err.to_string());
            }
            _ => {}
        }
        polled
    }
}

/// A response body under the watchdog.
struct HttpBody {
    response: reqwest::Response,
    watchdog: Watchdog,
    body_failure: Arc<Mutex<Option<String>>>,
}

impl RawBody for HttpBody {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>> {
        Box::pin(async move {
            tokio::select! {
                biased;
                chunk = self.response.chunk() => match chunk {
                    Ok(Some(bytes)) => {
                        self.watchdog.touch();
                        Ok(Some(bytes))
                    }
                    Ok(None) => Ok(None),
                    Err(err) => Err(classify(&err, true, &self.body_failure)),
                },
                () = self.watchdog.fired() => Err(SyncError::Timeout),
            }
        })
    }
}

/// A reqwest failure in T11's categories: a 407 answer to `CONNECT` is a
/// `status` 407 (T10); a TLS handshake failure is `tls`; a connect past
/// 30 s is `network` (`connect_timeout`); everything else is `network`.
fn classify(err: &reqwest::Error, head_arrived: bool, body_failure: &Mutex<Option<String>>) -> SyncError {
    if let Some(detail) = body_failure.lock().unwrap_or_else(PoisonError::into_inner).take() {
        return SyncError::Network(NetworkError {
            cause: NetworkCause::RequestBody,
            head_arrived,
            detail,
        });
    }
    let mut tls = None;
    let mut proxy_auth = false;
    each_cause(err, &mut |cause| {
        if let Some(rustls_error) = cause.downcast_ref::<rustls::Error>() {
            tls.get_or_insert_with(|| rustls_error.to_string());
        }
        // hyper-util's tunnel error (not exported) for a 407 to CONNECT.
        if format!("{cause:?}") == "ProxyAuthRequired" {
            proxy_auth = true;
        }
    });
    if proxy_auth {
        return SyncError::Status {
            code: 407,
            reason: String::new(),
        };
    }
    if let Some(detail) = tls {
        return SyncError::Tls(detail);
    }
    let cause = match (err.is_connect(), err.is_timeout()) {
        (true, true) => NetworkCause::ConnectTimeout,
        (true, false) => connect_cause(err),
        _ => NetworkCause::Connection,
    };
    SyncError::Network(NetworkError {
        cause,
        head_arrived,
        detail: chain(err),
    })
}

/// Why a connection could not be made (errors.md E5): the name lookup
/// failed, the network or host is unreachable (both `offline`), or another
/// cause, a refused connection among them (`network`).
fn connect_cause(err: &(dyn StdError + 'static)) -> NetworkCause {
    let mut cause = NetworkCause::Connect;
    each_cause(err, &mut |current| {
        if current.downcast_ref::<NameLookupFailed>().is_some() {
            cause = NetworkCause::NameLookup;
        } else if let Some(io) = current.downcast_ref::<io::Error>()
            && unreachable(io.kind())
            && cause == NetworkCause::Connect
        {
            cause = NetworkCause::Unreachable;
        }
    });
    cause
}

/// The socket errors that mean the device has no route to the server.
fn unreachable(kind: io::ErrorKind) -> bool {
    matches!(
        kind,
        io::ErrorKind::NetworkUnreachable | io::ErrorKind::HostUnreachable | io::ErrorKind::NetworkDown
    )
}

/// Name lookups through the system resolver (`getaddrinfo` on a blocking
/// thread, as reqwest's own resolver does), with failures wrapped in
/// [`NameLookupFailed`].
struct SystemResolver;

impl reqwest::dns::Resolve for SystemResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let found = tokio::task::spawn_blocking(move || {
                super::runtime::blocking_work(|| {
                    std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 0)).map(Iterator::collect::<Vec<_>>)
                })
            })
            .await
            .unwrap_or_else(|join| Err(io::Error::other(join.to_string())));
            match found {
                Ok(addresses) => Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs),
                Err(err) => Err(Box::new(NameLookupFailed(err)) as Box<dyn StdError + Send + Sync>),
            }
        })
    }
}

/// A name that did not resolve.
#[derive(Debug)]
struct NameLookupFailed(io::Error);

impl std::fmt::Display for NameLookupFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "name lookup failed: {}", self.0)
    }
}

impl StdError for NameLookupFailed {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

/// Visits every error in `err`'s chain, the errors an `io::Error` wraps
/// included: its `source()` skips them, and the TLS handshake's error sits
/// inside two of them (tokio-rustls's, then the connector's).
fn each_cause(err: &(dyn StdError + 'static), visit: &mut dyn FnMut(&(dyn StdError + 'static))) {
    let mut next = Some(err);
    while let Some(current) = next {
        visit(current);
        if let Some(inner) = current.downcast_ref::<io::Error>().and_then(io::Error::get_ref) {
            // The wrapper's source() is the inner error's: this covers it.
            each_cause(inner, visit);
            return;
        }
        next = current.source();
    }
}

/// An error and its sources, for logs. URLs hold no secret (the host key
/// never goes in one, T11).
fn chain(err: &(dyn StdError + 'static)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(current) = source {
        let text = current.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = current.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_failures_are_told_apart_by_their_cause() {
        // errors.md E5: no route (offline) against every other cause.
        for (kind, cause) in [
            (io::ErrorKind::NetworkUnreachable, NetworkCause::Unreachable),
            (io::ErrorKind::HostUnreachable, NetworkCause::Unreachable),
            (io::ErrorKind::NetworkDown, NetworkCause::Unreachable),
            (io::ErrorKind::ConnectionRefused, NetworkCause::Connect),
            (io::ErrorKind::Other, NetworkCause::Connect),
        ] {
            assert_eq!(connect_cause(&io::Error::from(kind)), cause, "{kind:?}");
        }
        let lookup = NameLookupFailed(io::Error::other("no such name"));
        assert_eq!(connect_cause(&lookup), NetworkCause::NameLookup);
        // Inside the wrappers a connector puts around it.
        let wrapped = io::Error::other(NameLookupFailed(io::Error::from(io::ErrorKind::HostUnreachable)));
        assert_eq!(connect_cause(&wrapped), NetworkCause::NameLookup);
    }
}
