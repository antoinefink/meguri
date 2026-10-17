//! A scripted sync server for transport tests (specs/sync/transport.md T13):
//! per request it answers with a chosen status, headers and body, pauses,
//! trickles, stops reading the request body or stays silent; it serves plain
//! HTTP or TLS (a certificate from a test root, or a self-signed one); it can
//! act as an HTTP proxy (absolute-form requests and `CONNECT`, answering 407
//! without the right credentials); and it records every request with its
//! decoded `anki-sync` header and decompressed body.
//!
//! Blocking std sockets and threads, one request per connection (every reply
//! says `connection: close`): tokio is the sync crate's alone (KTD2). It
//! listens on 127.0.0.1 only and connects only where a test routes it.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde_json::Value;

/// How long a stub socket waits for the client before it gives up.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(60);

/// The longest request head the stub reads.
const HEAD_LIMIT: usize = 64 * 1024;

/// A request's head as the stub received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The request's place among the stub's requests, from 0.
    pub index: usize,
    pub method: String,
    /// As sent: a path, an absolute URL (to a proxy) or `host:port`
    /// (`CONNECT`).
    pub target: String,
    /// Names in lower case, in the order received.
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// The first value of a header.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Every value of a header.
    pub fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The target's path: `/sync/meta` for `/sync/meta` and for
    /// `http://h:1/sync/meta`.
    pub fn path(&self) -> &str {
        match self.target.split_once("://") {
            Some((_, rest)) => rest.find('/').map_or("/", |i| &rest[i..]),
            None => &self.target,
        }
    }
}

/// A request the stub received.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub head: Head,
    /// The `anki-sync` header read as JSON.
    pub sync_header: Option<Value>,
    /// The body as received, without chunk framing.
    pub raw_body: Vec<u8>,
    /// The body zstd-decompressed, when it decompresses.
    pub body: Option<Vec<u8>>,
    /// The body came with `transfer-encoding: chunked`.
    pub chunked: bool,
    /// The whole body arrived.
    pub complete: bool,
}

impl Recorded {
    /// A member of the `anki-sync` header.
    pub fn sync_member(&self, name: &str) -> Option<&Value> {
        self.sync_header.as_ref().and_then(|h| h.get(name))
    }
}

/// A piece of a reply body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Bytes(Vec<u8>),
    /// Sends nothing for a while.
    Pause(Duration),
    /// Closes the connection here.
    Close,
}

/// How the stub reads a request body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadBody {
    All,
    /// Reads `bytes` of the body, then stops reading for `pause`, then reads
    /// the rest.
    StallAfter {
        bytes: usize,
        pause: Duration,
    },
}

/// The stub's answer to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<Part>,
    /// `transfer-encoding: chunked`, one chunk per part; otherwise
    /// `content-length`.
    pub chunked: bool,
    pub read: ReadBody,
    /// Silence after the body was read, before the head.
    pub head_delay: Duration,
}

impl Reply {
    /// A status with no body.
    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            headers: Vec::new(),
            body: Vec::new(),
            chunked: true,
            read: ReadBody::All,
            head_delay: Duration::ZERO,
        }
    }

    /// 200 with `payload` zstd-compressed and its size declared, as the
    /// official server answers (T14).
    pub fn ok(payload: &[u8]) -> Reply {
        Reply::status(200)
            .with_header("anki-original-size", &payload.len().to_string())
            .with_body(vec![Part::Bytes(compress(payload))])
    }

    /// No answer at all: the stub closes the connection once the body was
    /// read, before any head.
    pub fn hang_up() -> Reply {
        Reply::status(0)
    }

    /// A redirect with an optional `Location`.
    pub fn redirect(status: u16, location: Option<&str>) -> Reply {
        let reply = Reply::status(status);
        match location {
            Some(location) => reply.with_header("location", location),
            None => reply,
        }
    }

    /// Sets a header, replacing any value it had.
    pub fn with_header(mut self, name: &str, value: &str) -> Reply {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    /// Adds a header value, keeping the ones it had.
    pub fn add_header(mut self, name: &str, value: &str) -> Reply {
        self.headers.push((name.to_ascii_lowercase(), value.to_owned()));
        self
    }

    pub fn without_header(mut self, name: &str) -> Reply {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self
    }

    pub fn with_body(mut self, parts: Vec<Part>) -> Reply {
        self.body = parts;
        self
    }

    /// Sends the body with `content-length` instead of chunks.
    pub fn content_length(mut self) -> Reply {
        self.chunked = false;
        self
    }

    pub fn read(mut self, read: ReadBody) -> Reply {
        self.read = read;
        self
    }

    pub fn head_delay(mut self, delay: Duration) -> Reply {
        self.head_delay = delay;
        self
    }

    /// The body's bytes, pauses aside.
    pub fn body_bytes(&self) -> Vec<u8> {
        self.body
            .iter()
            .flat_map(|part| match part {
                Part::Bytes(bytes) => bytes.as_slice(),
                _ => &[][..],
            })
            .copied()
            .collect()
    }
}

/// `data` in `pieces` parts of about equal size, each after a pause of
/// `every`.
pub fn trickle(data: &[u8], pieces: usize, every: Duration) -> Vec<Part> {
    let size = data.len().div_ceil(pieces.max(1)).max(1);
    data.chunks(size)
        .flat_map(|piece| [Part::Pause(every), Part::Bytes(piece.to_vec())])
        .collect()
}

/// One zstd frame of `data` (level 3).
pub fn compress(data: &[u8]) -> Vec<u8> {
    zstd::bulk::compress(data, 3).expect("zstd compresses any bytes")
}

/// `data` decompressed, frame after frame.
pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
    zstd::stream::decode_all(data)
}

/// What a proxy stub does (T10): which credentials it requires and where it
/// connects for a target `host:port`. Targets without a route must be a
/// socket address; the stub connects nowhere else.
#[derive(Debug, Clone, Default)]
pub struct ProxyRules {
    /// Basic credentials every request must carry in `proxy-authorization`;
    /// without them the proxy answers 407.
    pub credentials: Option<(String, String)>,
    /// `host:port` (lower case) to the address to connect to.
    pub routes: Vec<(String, SocketAddr)>,
}

type Script = dyn Fn(&Head) -> Reply + Send + Sync;

enum Handler {
    Script(Box<Script>),
    Proxy(ProxyRules),
}

struct Shared {
    stop: AtomicBool,
    count: AtomicUsize,
    log: Mutex<Vec<Recorded>>,
    handler: Handler,
    tls: Option<Arc<rustls::ServerConfig>>,
}

/// A running stub; dropping it stops it.
pub struct SyncStub {
    addr: SocketAddr,
    tls: bool,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl SyncStub {
    /// A plain-HTTP stub answering each request with `script`.
    pub fn serve(script: impl Fn(&Head) -> Reply + Send + Sync + 'static) -> SyncStub {
        SyncStub::start(Handler::Script(Box::new(script)), None)
    }

    /// A TLS stub presenting `cert`.
    pub fn serve_tls(cert: &ServerCert, script: impl Fn(&Head) -> Reply + Send + Sync + 'static) -> SyncStub {
        SyncStub::start(Handler::Script(Box::new(script)), Some(cert.server_config()))
    }

    /// An HTTP proxy stub.
    pub fn proxy(rules: ProxyRules) -> SyncStub {
        SyncStub::start(Handler::Proxy(rules), None)
    }

    fn start(handler: Handler, tls: Option<Arc<rustls::ServerConfig>>) -> SyncStub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the stub binds a loopback port");
        let addr = listener.local_addr().expect("a bound listener has an address");
        let is_tls = tls.is_some();
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            count: AtomicUsize::new(0),
            log: Mutex::new(Vec::new()),
            handler,
            tls,
        });
        let accepting = Arc::clone(&shared);
        let accept = thread::spawn(move || {
            for stream in listener.incoming() {
                if accepting.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let shared = Arc::clone(&accepting);
                thread::spawn(move || {
                    let _ = connection(&shared, stream);
                });
            }
        });
        SyncStub {
            addr,
            tls: is_tls,
            shared,
            accept: Some(accept),
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>/` or `https://...`.
    pub fn url(&self) -> String {
        self.url_for(&self.addr.ip().to_string())
    }

    /// The stub's URL under another host name (pinned to it by the test).
    pub fn url_for(&self, host: &str) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        format!("{scheme}://{host}:{}/", self.addr.port())
    }

    /// The requests received so far, in the order their heads arrived.
    pub fn requests(&self) -> Vec<Recorded> {
        let mut log = self.shared.log.lock().unwrap_or_else(PoisonError::into_inner).clone();
        log.sort_by_key(|r| r.head.index);
        log
    }
}

impl Drop for SyncStub {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wakes the accept loop.
        let _ = TcpStream::connect(self.addr);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

trait Conn: Read + Write + Send {}
impl<T: Read + Write + Send> Conn for T {}

fn connection(shared: &Shared, tcp: TcpStream) -> io::Result<()> {
    tcp.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    tcp.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    tcp.set_nodelay(true)?;
    match &shared.handler {
        Handler::Proxy(rules) => proxy(shared, rules, tcp),
        Handler::Script(script) => {
            let conn: Box<dyn Conn> = match &shared.tls {
                Some(config) => {
                    let server = rustls::ServerConnection::new(Arc::clone(config)).map_err(io::Error::other)?;
                    Box::new(rustls::StreamOwned::new(server, tcp))
                }
                None => Box::new(tcp),
            };
            serve(shared, script, conn)
        }
    }
}

fn serve(shared: &Shared, script: &Script, conn: Box<dyn Conn>) -> io::Result<()> {
    let mut reader = BufReader::new(conn);
    let Some(head) = read_head(&mut reader, shared)? else {
        return Ok(());
    };
    let reply = script(&head);
    let body = read_body(&mut reader, &head, reply.read);
    record(shared, head, &body);
    body.result?;
    thread::sleep(reply.head_delay);
    if reply.status == 0 {
        return Ok(());
    }
    write_reply(reader.get_mut(), &reply)
}

fn record(shared: &Shared, head: Head, body: &BodyRead) {
    let sync_header = head
        .header("anki-sync")
        .and_then(|value| serde_json::from_str(value).ok());
    let decoded = if body.bytes.is_empty() {
        None
    } else {
        decompress(&body.bytes).ok()
    };
    shared
        .log
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Recorded {
            head,
            sync_header,
            raw_body: body.bytes.clone(),
            body: decoded,
            chunked: body.chunked,
            complete: body.result.is_ok(),
        });
}

/// The head, or `None` when the client closed before sending one.
fn read_head(reader: &mut impl BufRead, shared: &Shared) -> io::Result<Option<Head>> {
    let mut lines = Vec::new();
    let mut total = 0;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return if lines.is_empty() {
                Ok(None)
            } else {
                Err(io::ErrorKind::UnexpectedEof.into())
            };
        }
        total += n;
        if total > HEAD_LIMIT {
            return Err(io::Error::other("request head too long"));
        }
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        if line.is_empty() {
            if lines.is_empty() {
                continue;
            }
            break;
        }
        lines.push(line);
    }
    let mut request_line = lines[0].split(' ');
    let method = request_line.next().unwrap_or_default().to_owned();
    let target = request_line.next().unwrap_or_default().to_owned();
    let headers = lines[1..]
        .iter()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Ok(Some(Head {
        index: shared.count.fetch_add(1, Ordering::SeqCst),
        method,
        target,
        headers,
    }))
}

struct BodyRead {
    bytes: Vec<u8>,
    chunked: bool,
    result: io::Result<()>,
}

fn read_body(reader: &mut impl BufRead, head: &Head, read: ReadBody) -> BodyRead {
    let chunked = head
        .header("transfer-encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"));
    let mut body = BodyRead {
        bytes: Vec::new(),
        chunked,
        result: Ok(()),
    };
    let mut stall = match read {
        ReadBody::All => None,
        ReadBody::StallAfter { bytes, pause } => Some((bytes, pause)),
    };
    body.result = if chunked {
        read_chunked(reader, &mut body.bytes, &mut stall)
    } else {
        let length = head
            .header("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        read_exact_into(reader, length, &mut body.bytes, &mut stall)
    };
    body
}

fn read_chunked(reader: &mut impl BufRead, out: &mut Vec<u8>, stall: &mut Option<(usize, Duration)>) -> io::Result<()> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let size = line.trim().split(';').next().unwrap_or_default();
        let size = usize::from_str_radix(size, 16).map_err(io::Error::other)?;
        if size == 0 {
            // Trailers, up to the empty line.
            loop {
                let mut trailer = String::new();
                if reader.read_line(&mut trailer)? == 0 || trailer.trim().is_empty() {
                    return Ok(());
                }
            }
        }
        read_exact_into(reader, size, out, stall)?;
        let mut crlf = [0u8; 2];
        reader.read_exact(&mut crlf)?;
    }
}

/// Reads `length` more body bytes, stopping once for a stall when the body
/// reaches its byte count.
fn read_exact_into(
    reader: &mut impl BufRead,
    length: usize,
    out: &mut Vec<u8>,
    stall: &mut Option<(usize, Duration)>,
) -> io::Result<()> {
    let mut left = length;
    let mut buf = [0u8; 8192];
    while left > 0 {
        if let Some((after, pause)) = *stall
            && out.len() >= after
        {
            *stall = None;
            thread::sleep(pause);
        }
        let mut want = left.min(buf.len());
        if let Some((after, _)) = *stall {
            want = want.min(after.saturating_sub(out.len()).max(1));
        }
        let n = reader.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        out.extend_from_slice(&buf[..n]);
        left -= n;
    }
    Ok(())
}

fn write_reply(out: &mut dyn Conn, reply: &Reply) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", reply.status, reason(reply.status));
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if reply.chunked {
        head.push_str("transfer-encoding: chunked\r\n");
    } else {
        head.push_str(&format!("content-length: {}\r\n", reply.body_bytes().len()));
    }
    head.push_str("connection: close\r\n\r\n");
    out.write_all(head.as_bytes())?;
    out.flush()?;
    for part in &reply.body {
        match part {
            Part::Bytes(bytes) if bytes.is_empty() => {}
            Part::Bytes(bytes) if reply.chunked => {
                out.write_all(format!("{:x}\r\n", bytes.len()).as_bytes())?;
                out.write_all(bytes)?;
                out.write_all(b"\r\n")?;
                out.flush()?;
            }
            Part::Bytes(bytes) => {
                out.write_all(bytes)?;
                out.flush()?;
            }
            Part::Pause(pause) => thread::sleep(*pause),
            Part::Close => return Ok(()),
        }
    }
    if reply.chunked {
        out.write_all(b"0\r\n\r\n")?;
    }
    out.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        301 => "Moved Permanently",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        403 => "Forbidden",
        407 => "Proxy Authentication Required",
        409 => "Conflict",
        500 => "Internal Server Error",
        _ => "Stub",
    }
}

fn proxy(shared: &Shared, rules: &ProxyRules, tcp: TcpStream) -> io::Result<()> {
    let mut reader = BufReader::new(tcp.try_clone()?);
    let mut client = tcp;
    let Some(head) = read_head(&mut reader, shared)? else {
        return Ok(());
    };
    let authorized = match &rules.credentials {
        None => true,
        Some((user, password)) => {
            let expected = format!("Basic {}", base64(format!("{user}:{password}").as_bytes()));
            head.header("proxy-authorization") == Some(expected.as_str())
        }
    };
    if head.method.eq_ignore_ascii_case("CONNECT") {
        let target = head.target.to_ascii_lowercase();
        record(
            shared,
            head,
            &BodyRead {
                bytes: Vec::new(),
                chunked: false,
                result: Ok(()),
            },
        );
        if !authorized {
            return client.write_all(PROXY_AUTH_REQUIRED);
        }
        let mut upstream = TcpStream::connect(route(rules, &target)?)?;
        client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
        let mut to_client = upstream.try_clone()?;
        let mut client_out = client.try_clone()?;
        let back = thread::spawn(move || {
            let _ = io::copy(&mut to_client, &mut client_out);
            let _ = client_out.shutdown(Shutdown::Write);
        });
        let _ = io::copy(&mut reader, &mut upstream);
        let _ = upstream.shutdown(Shutdown::Write);
        let _ = back.join();
        return Ok(());
    }

    // Absolute form: http://host:port/path.
    let body = read_body(&mut reader, &head, ReadBody::All);
    let target = head.target.clone();
    let method = head.method.clone();
    let forwarded: Vec<(String, String)> = head
        .headers
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "proxy-authorization" | "proxy-connection" | "connection" | "transfer-encoding" | "content-length"
            )
        })
        .cloned()
        .collect();
    let path = head.path().to_owned();
    record(shared, head, &body);
    body.result?;
    if !authorized {
        return client.write_all(PROXY_AUTH_REQUIRED);
    }
    let authority = target
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or_default().to_ascii_lowercase())
        .ok_or_else(|| io::Error::other("not an absolute-form target"))?;
    let mut upstream = TcpStream::connect(route(rules, &authority)?)?;
    upstream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    let mut request = format!("{method} {path} HTTP/1.1\r\n");
    for (name, value) in &forwarded {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str(&format!(
        "content-length: {}\r\nconnection: close\r\n\r\n",
        body.bytes.len()
    ));
    upstream.write_all(request.as_bytes())?;
    upstream.write_all(&body.bytes)?;
    io::copy(&mut upstream, &mut client)?;
    client.shutdown(Shutdown::Write)
}

const PROXY_AUTH_REQUIRED: &[u8] = b"HTTP/1.1 407 Proxy Authentication Required\r\n\
proxy-authenticate: Basic realm=\"stub\"\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

fn route(rules: &ProxyRules, target: &str) -> io::Result<SocketAddr> {
    if let Some((_, address)) = rules.routes.iter().find(|(name, _)| name == target) {
        return Ok(*address);
    }
    target
        .parse()
        .map_err(|_| io::Error::other(format!("the proxy stub has no route to {target}")))
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for group in data.chunks(3) {
        let b = [group[0], *group.get(1).unwrap_or(&0), *group.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= group.len() {
                out.push(char::from(ALPHABET[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A certificate authority that only test builds trust (T12): its leaves
/// let the stub serve `.test` names over TLS.
pub struct TestRoot {
    der: Vec<u8>,
    issuer: Issuer<'static, KeyPair>,
}

impl TestRoot {
    pub fn generate() -> TestRoot {
        let mut params = CertificateParams::new(Vec::<String>::new()).expect("empty names are valid");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        params
            .distinguished_name
            .push(DnType::CommonName, "Meguri sync stub test root");
        let key = KeyPair::generate().expect("a key pair");
        let cert = params.self_signed(&key).expect("a self-signed root");
        TestRoot {
            der: cert.der().to_vec(),
            issuer: Issuer::new(params, key),
        }
    }

    /// The root's certificate, DER: what a test job trusts.
    pub fn der(&self) -> Vec<u8> {
        self.der.clone()
    }

    /// A server certificate for `names` signed by this root.
    pub fn server_cert(&self, names: &[&str]) -> ServerCert {
        let params = leaf_params(names);
        let key = KeyPair::generate().expect("a key pair");
        let cert = params.signed_by(&key, &self.issuer).expect("a signed leaf");
        ServerCert {
            chain: vec![cert.der().clone()],
            key: key.serialize_der(),
        }
    }
}

/// A server certificate and its key.
pub struct ServerCert {
    chain: Vec<CertificateDer<'static>>,
    key: Vec<u8>,
}

impl ServerCert {
    /// A certificate for `names` that signs itself: no root a client trusts
    /// covers it.
    pub fn self_signed(names: &[&str]) -> ServerCert {
        let params = leaf_params(names);
        let key = KeyPair::generate().expect("a key pair");
        let cert = params.self_signed(&key).expect("a self-signed leaf");
        ServerCert {
            chain: vec![cert.der().clone()],
            key: key.serialize_der(),
        }
    }

    fn server_config(&self) -> Arc<rustls::ServerConfig> {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()));
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("the provider supports TLS 1.2 and 1.3")
            .with_no_client_auth()
            .with_single_cert(self.chain.clone(), key)
            .expect("the stub's certificate and key match");
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Arc::new(config)
    }
}

fn leaf_params(names: &[&str]) -> CertificateParams {
    let mut params =
        CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>()).expect("valid names");
    params
        .distinguished_name
        .push(DnType::CommonName, names.first().copied().unwrap_or("stub"));
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected, "{input:?}");
        }
    }

    #[test]
    fn a_head_names_its_path_in_every_target_form() {
        let head = |target: &str| Head {
            index: 0,
            method: "POST".to_owned(),
            target: target.to_owned(),
            headers: Vec::new(),
        };
        assert_eq!(head("/sync/meta").path(), "/sync/meta");
        assert_eq!(head("http://h:1/anki/sync/meta").path(), "/anki/sync/meta");
        assert_eq!(head("http://h:1").path(), "/");
    }
}
