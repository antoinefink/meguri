//! The crate's errors: the categories the transport reports
//! (specs/sync/transport.md T11) as [`SyncError`], and the kinds every failed
//! job reports (specs/sync/errors.md) as [`JobError`]. No message here ever
//! holds the host key, the password or a request body (T11, E1).

use std::fmt;

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_storage::StorageError;

use crate::endpoint::{EndpointRule, ServerKind};
use crate::transport::Method;
use crate::wire::sanity::SanityCounts;

/// Why a sync job failed (transport.md T11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncError {
    /// A base or redirect target broke a rule of login-endpoints.md L1.
    InvalidEndpoint(EndpointRule),
    /// A caller value is out of range; nothing was sent.
    InvalidInput(InvalidInput),
    /// The job's proxy cannot be used (T10); nothing was sent.
    InvalidProxy(String),
    Network(NetworkError),
    /// The TLS handshake failed: untrusted, expired or mismatched
    /// certificate, no common protocol version.
    Tls(String),
    /// The stall watchdog fired (T8).
    Timeout,
    /// A 4xx or 5xx response, or a 407 from the proxy (T6.1, T10). `reason`
    /// is the start of the body, for logs only: classify by `code`.
    Status {
        code: u16,
        reason: String,
    },
    /// A redirect that was not followed (T7).
    Redirect(RedirectError),
    /// A 2xx response that breaks T6.2, or a login answer without a key (L5).
    Protocol(ProtocolError),
    /// The job's cancel token (T9).
    Cancelled,
    /// Builds with `test-hooks` only: the endpoint guard refused a URL or the
    /// proxy before anything connected (T12, R1876).
    #[cfg(feature = "test-hooks")]
    EndpointRefused(String),
}

impl SyncError {
    /// The T11 category name.
    pub fn category(&self) -> &'static str {
        match self {
            SyncError::InvalidEndpoint(_) => "invalid_endpoint",
            SyncError::InvalidInput(_) => "invalid_input",
            SyncError::InvalidProxy(_) => "invalid_proxy",
            SyncError::Network(_) => "network",
            SyncError::Tls(_) => "tls",
            SyncError::Timeout => "timeout",
            SyncError::Status { .. } => "status",
            SyncError::Redirect(_) => "redirect",
            SyncError::Protocol(_) => "protocol",
            SyncError::Cancelled => "cancelled",
            #[cfg(feature = "test-hooks")]
            SyncError::EndpointRefused(_) => "endpoint_refused",
        }
    }

    /// A 403 on any request: the server refused the credentials or the host
    /// key (login-endpoints.md L6, R1499). The app then clears the stored key.
    pub fn is_auth_failure(&self) -> bool {
        matches!(self, SyncError::Status { code: 403, .. })
    }
}

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.category())?;
        match self {
            SyncError::InvalidEndpoint(rule) => write!(f, ": {}", rule.as_str()),
            SyncError::InvalidInput(input) => write!(f, ": {}", input.as_str()),
            SyncError::InvalidProxy(detail) | SyncError::Tls(detail) => write!(f, ": {detail}"),
            SyncError::Network(network) => write!(f, ": {network}"),
            SyncError::Timeout | SyncError::Cancelled => Ok(()),
            SyncError::Status { code, reason } if reason.is_empty() => write!(f, ": HTTP {code}"),
            SyncError::Status { code, reason } => write!(f, ": HTTP {code}: {reason}"),
            SyncError::Redirect(redirect) => write!(f, ": {redirect}"),
            SyncError::Protocol(protocol) => write!(f, ": {}", protocol.as_str()),
            #[cfg(feature = "test-hooks")]
            SyncError::EndpointRefused(detail) => write!(f, ": {detail}"),
        }
    }
}

impl std::error::Error for SyncError {}

/// The caller value an `invalid_input` error names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidInput {
    /// A stall timeout of zero (T8).
    StallTimeout,
    /// A login user name that is empty once trimmed (L5).
    UserName,
    /// An empty login password (L5).
    Password,
}

impl InvalidInput {
    pub fn as_str(self) -> &'static str {
        match self {
            InvalidInput::StallTimeout => "stall_timeout",
            InvalidInput::UserName => "user_name",
            InvalidInput::Password => "password",
        }
    }
}

/// A `network` error (T11): no response head arrived, or the connection
/// broke while the body was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkError {
    pub cause: NetworkCause,
    /// Whether the response's status line and headers had arrived.
    pub head_arrived: bool,
    /// The underlying failure, for logs.
    pub detail: String,
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cause = match self.cause {
            NetworkCause::ConnectTimeout => "connect_timeout",
            NetworkCause::NameLookup => "name_lookup",
            NetworkCause::Unreachable => "unreachable",
            NetworkCause::Connect => "connect",
            NetworkCause::Connection => "connection",
            NetworkCause::RequestBody => "request_body",
        };
        let head = if self.head_arrived {
            ", after the response head"
        } else {
            ""
        };
        write!(f, "{cause}{head}: {}", self.detail)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkCause {
    /// Name lookup, TCP, TLS and a proxy tunnel together took over 30 s (T8).
    ConnectTimeout,
    /// The server's name could not be resolved.
    NameLookup,
    /// The network or the host is unreachable, or the network is down.
    Unreachable,
    /// No connection for another reason: refused, or a failure the causes
    /// above do not name.
    Connect,
    /// The connection was reset or closed.
    Connection,
    /// The caller's stream of request body bytes failed.
    RequestBody,
}

/// A redirect that was not followed (T7): the status and the `Location` text
/// as received, and which check refused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectError {
    pub code: u16,
    pub location: Option<String>,
    pub refusal: RedirectRefusal,
}

impl fmt::Display for RedirectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP {} ({})", self.code, self.refusal.as_str())?;
        match &self.location {
            Some(location) => write!(f, " to {location:?}"),
            None => Ok(()),
        }
    }
}

impl RedirectError {
    /// Informational only: this destination never authorizes another request.
    fn diagnostic(&self) -> RedirectDiagnostic {
        let destination = self.location.as_deref().and_then(|location| {
            let mut url = url::Url::parse(location).ok()?;
            if !matches!(url.scheme(), "http" | "https") {
                return None;
            }
            url.host_str()?;
            url.set_username("").ok()?;
            url.set_password(None).ok()?;
            url.set_query(None);
            url.set_fragment(None);
            Some(url.into())
        });
        RedirectDiagnostic {
            destination,
            reason: self.refusal,
        }
    }

    /// The text a job error's `detail` holds (errors.md E1): the status, the
    /// refusal and, when `Location` is an absolute URL with a host, that URL
    /// without user name, password, query or fragment. A `Location` that is
    /// not such a URL is left out, since it may hold credentials anywhere.
    fn job_detail(&self) -> String {
        let mut text = format!("redirect: HTTP {} ({})", self.code, self.refusal.as_str());
        if let Some(destination) = self.diagnostic().destination {
            use fmt::Write as _;
            let _ = write!(text, " to {destination:?}");
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectDiagnostic {
    pub destination: Option<String>,
    pub reason: RedirectRefusal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectRefusal {
    /// Only `meta` follows a redirect (R1511).
    NotMeta,
    /// Only a 308 is followed.
    NotPermanent,
    MissingLocation,
    /// The target breaks an endpoint rule (L1); a relative one is
    /// `unparseable`.
    Target(EndpointRule),
    /// The base was `https` and the target is not.
    LostHttps,
    /// The target's host breaks the host rule of the job's server kind.
    Host,
    /// A job follows at most one redirect.
    Repeated,
    /// The request's body was a stream, which cannot be sent twice.
    NotResendable,
}

impl RedirectRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            RedirectRefusal::NotMeta => "not_meta",
            RedirectRefusal::NotPermanent => "not_308",
            RedirectRefusal::MissingLocation => "missing_location",
            RedirectRefusal::Target(rule) => rule.as_str(),
            RedirectRefusal::LostHttps => "https_lost",
            RedirectRefusal::Host => "host",
            RedirectRefusal::Repeated => "repeated",
            RedirectRefusal::NotResendable => "not_resendable",
        }
    }
}

/// A `protocol` error (T6.2, L5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    MissingOriginalSize,
    BadOriginalSize,
    SizeExceeded,
    SizeShort,
    BadCompression,
    /// A login answer that is not an object with a non-empty string `key`.
    BadLoginAnswer,
}

impl ProtocolError {
    pub fn as_str(self) -> &'static str {
        match self {
            ProtocolError::MissingOriginalSize => "missing_original_size",
            ProtocolError::BadOriginalSize => "bad_original_size",
            ProtocolError::SizeExceeded => "size_exceeded",
            ProtocolError::SizeShort => "size_short",
            ProtocolError::BadCompression => "bad_compression",
            ProtocolError::BadLoginAnswer => "bad_login_answer",
        }
    }
}

// --- The kinds a failed job reports (errors.md) -----------------------------

/// What the app does with a failure (E2, R165).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorClass {
    /// A quiet banner or status glyph with Retry: the same sync can succeed
    /// later without the user changing anything.
    Transient,
    /// An alert that names the next step.
    Actionable,
    /// Not shown as an error: the user cancelled, the system took the time
    /// away, or the sync waits for something to end.
    Silent,
}

impl ErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorClass::Transient => "transient",
            ErrorClass::Actionable => "actionable",
            ErrorClass::Silent => "silent",
        }
    }
}

/// When the sync policy may try a failed sync again (E2, U113).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Retry {
    /// Later, with backoff and a quiet status.
    Auto,
    /// At the next trigger that removes the cause (the end of the review
    /// session, the app's next foreground), not on a timer.
    AfterTrigger,
    /// Retrying without the user changing something fails the same way.
    No,
}

impl Retry {
    pub fn as_str(self) -> &'static str {
        match self {
            Retry::Auto => "auto",
            Retry::AfterTrigger => "after_trigger",
            Retry::No => "no",
        }
    }
}

/// Where a `db_check_required` comes from (E3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DbCheckOrigin {
    /// The server answered 400.
    Server,
    /// Local data that cannot be sent (E7).
    Local,
}

impl DbCheckOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            DbCheckOrigin::Server => "server",
            DbCheckOrigin::Local => "local",
        }
    }
}

/// One kind of E3 with its parameters. The ids ([`ErrorKind::id`]), classes,
/// retry values and parameter names are an interface (E1): Swift maps each
/// id to its own String Catalog entry and U113 reads the class and the retry,
/// so renaming one is a breaking change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    /// HTTP 403 on any request (R1499).
    AuthFailed,
    /// HTTP 409: another device is syncing.
    Conflict,
    /// HTTP 501.
    ClientTooOld,
    /// HTTP 429, or any 5xx but 501.
    ServerError { code: u16 },
    /// HTTP 400, or local data that cannot be sent (E7).
    DbCheckRequired { origin: DbCheckOrigin },
    /// HTTP 407, from the proxy or the server.
    ProxyAuth,
    /// Any 4xx that E4 does not name.
    UnexpectedStatus { code: u16 },
    /// No route to the server: its name did not resolve, or the network or
    /// host is unreachable.
    Offline,
    /// Any other network failure.
    Network {
        /// Whether the answer's status line had arrived.
        head_received: bool,
    },
    /// The stall watchdog fired, or HTTP 408.
    Timeout,
    /// The TLS handshake failed.
    Tls {
        /// The job's server is the user's own: the message adds the HTTPS and
        /// TLS 1.2 hint (R1561).
        self_hosted: bool,
    },
    /// A redirect that is not followed (transport.md T7).
    RedirectRefused { code: u16 },
    /// A 2xx answer that breaks the transport's checks or does not decode
    /// (E6).
    BadResponse,
    /// The job's base is not a valid endpoint (login-endpoints.md L1).
    InvalidEndpoint { rule: EndpointRule },
    /// A caller value out of range.
    InvalidInput { field: InvalidInput },
    /// The job's proxy cannot be used.
    ProxyUnusable,
    /// Builds with `test-hooks` only: the endpoint guard refused a URL.
    #[cfg(feature = "test-hooks")]
    EndpointRefused,
    /// The server's own text, exactly as sent, possibly empty: `meta`
    /// answered `cont` false (R1514), a one-way upload answered something
    /// other than `OK`, or a media answer holds only `err`.
    ServerMessage { text: String },
    /// The server's clock and the device's differ by more than 300 s
    /// (meta-status.md M2, R1515).
    ClockIncorrect {
        /// Server minus device.
        offset_secs: i64,
    },
    /// A one-way upload over the AnkiWeb limits (R1563, U109).
    #[cfg(feature = "ankiweb")]
    UploadTooLarge { size_bytes: u64, limit_bytes: u64 },
    /// `sanityCheck2` answered `bad` (R1533): the answer's counts when present.
    SanityCheckFailed {
        client: Option<SanityCounts>,
        server: Option<SanityCounts>,
    },
    /// An incoming note type that would replace a local one has a different
    /// number of fields or templates (R1525).
    NotetypeSchemaChanged { notetype_id: NotetypeId },
    /// An incoming note uses a note type the collection does not have
    /// (R1528).
    MissingNotetype { notetype_id: NotetypeId, note_id: NoteId },
    /// A one-way download failed its integrity check (R1543).
    DownloadInvalid,
    /// The media sanity check still disagrees after the automatic re-sync
    /// (R1581).
    MediaResyncFailed,
    /// Not enough free space (R2015).
    DiskFull {
        needed_bytes: Option<u64>,
        available_bytes: Option<u64>,
    },
    /// Any other local file or database failure.
    LocalStorage,
    /// A one-way sync is needed while a review session is open (R458); the
    /// job made no change.
    SessionActive,
    /// Another exclusive job holds the collection.
    ExclusiveBusy,
    /// The job's cancel token fired.
    Cancelled,
    /// The system ended the job (R1554).
    Interrupted,
    /// A broken invariant inside Meguri (a bug).
    Internal,
}

impl ErrorKind {
    /// The kind's E3 id.
    pub fn id(&self) -> &'static str {
        match self {
            ErrorKind::AuthFailed => "auth_failed",
            ErrorKind::Conflict => "conflict",
            ErrorKind::ClientTooOld => "client_too_old",
            ErrorKind::ServerError { .. } => "server_error",
            ErrorKind::DbCheckRequired { .. } => "db_check_required",
            ErrorKind::ProxyAuth => "proxy_auth",
            ErrorKind::UnexpectedStatus { .. } => "unexpected_status",
            ErrorKind::Offline => "offline",
            ErrorKind::Network { .. } => "network",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Tls { .. } => "tls",
            ErrorKind::RedirectRefused { .. } => "redirect_refused",
            ErrorKind::BadResponse => "bad_response",
            ErrorKind::InvalidEndpoint { .. } => "invalid_endpoint",
            ErrorKind::InvalidInput { .. } => "invalid_input",
            ErrorKind::ProxyUnusable => "proxy_unusable",
            #[cfg(feature = "test-hooks")]
            ErrorKind::EndpointRefused => "endpoint_refused",
            ErrorKind::ServerMessage { .. } => "server_message",
            ErrorKind::ClockIncorrect { .. } => "clock_incorrect",
            #[cfg(feature = "ankiweb")]
            ErrorKind::UploadTooLarge { .. } => "upload_too_large",
            ErrorKind::SanityCheckFailed { .. } => "sanity_check_failed",
            ErrorKind::NotetypeSchemaChanged { .. } => "notetype_schema_changed",
            ErrorKind::MissingNotetype { .. } => "missing_notetype",
            ErrorKind::DownloadInvalid => "download_invalid",
            ErrorKind::MediaResyncFailed => "media_resync_failed",
            ErrorKind::DiskFull { .. } => "disk_full",
            ErrorKind::LocalStorage => "local_storage",
            ErrorKind::SessionActive => "session_active",
            ErrorKind::ExclusiveBusy => "exclusive_busy",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Interrupted => "interrupted",
            ErrorKind::Internal => "internal",
        }
    }

    /// E3's class.
    pub fn class(&self) -> ErrorClass {
        match self {
            ErrorKind::Conflict
            | ErrorKind::ServerError { .. }
            | ErrorKind::Offline
            | ErrorKind::Network { .. }
            | ErrorKind::Timeout
            | ErrorKind::BadResponse
            | ErrorKind::ExclusiveBusy => ErrorClass::Transient,
            ErrorKind::SessionActive | ErrorKind::Cancelled | ErrorKind::Interrupted => ErrorClass::Silent,
            _ => ErrorClass::Actionable,
        }
    }

    /// E3's retry.
    pub fn retry(&self) -> Retry {
        match self {
            ErrorKind::Conflict
            | ErrorKind::ServerError { .. }
            | ErrorKind::Offline
            | ErrorKind::Network { .. }
            | ErrorKind::Timeout
            | ErrorKind::BadResponse
            | ErrorKind::ExclusiveBusy => Retry::Auto,
            ErrorKind::SessionActive | ErrorKind::Interrupted => Retry::AfterTrigger,
            _ => Retry::No,
        }
    }

    /// E4: the kind of a 4xx or 5xx answer, by its code alone.
    pub fn for_status(code: u16) -> ErrorKind {
        match code {
            400 => ErrorKind::DbCheckRequired {
                origin: DbCheckOrigin::Server,
            },
            403 => ErrorKind::AuthFailed,
            407 => ErrorKind::ProxyAuth,
            408 => ErrorKind::Timeout,
            409 => ErrorKind::Conflict,
            501 => ErrorKind::ClientTooOld,
            429 | 500..=599 => ErrorKind::ServerError { code },
            _ => ErrorKind::UnexpectedStatus { code },
        }
    }
}

/// A failed job's error value (E1): the kind with its parameters, the method
/// whose request or answer failed, and a detail for logs. Every failure
/// inside a sync job becomes one; none crosses the FFI as a panic (R1562).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobError {
    pub kind: ErrorKind,
    /// `None` when the failure was not tied to one request.
    pub method: Option<Method>,
    /// English text for logs and diagnostics only, never the message shown to
    /// the user. Never a host key, a password, a body (but the server's own
    /// message) or a URL with credentials.
    pub detail: String,
    /// Sanitized redirect facts, separate from unrestricted log text.
    pub redirect: Option<RedirectDiagnostic>,
}

impl JobError {
    pub fn new(kind: ErrorKind, method: Option<Method>, detail: impl Into<String>) -> JobError {
        JobError {
            kind,
            method,
            detail: detail.into(),
            redirect: None,
        }
    }

    pub fn class(&self) -> ErrorClass {
        self.kind.class()
    }

    pub fn retry(&self) -> Retry {
        self.kind.retry()
    }

    /// The method as E1 names it: `meta`, `applyChanges`, `msync/begin`.
    pub fn method_name(&self) -> Option<&'static str> {
        self.method.map(Method::qualified_name)
    }

    /// E4 and E5: a transport failure of a job on a server of kind `server`.
    pub fn from_transport(error: SyncError, method: Option<Method>, server: ServerKind) -> JobError {
        let redirect = match &error {
            SyncError::Redirect(error) => Some(error.diagnostic()),
            _ => None,
        };
        // A status's reason is the start of the answer's body, and a
        // redirect's `Location` may carry credentials: E1 keeps both out of
        // the detail.
        let detail = match &error {
            SyncError::Status { code, .. } => format!("status: HTTP {code}"),
            SyncError::Redirect(redirect) => redirect.job_detail(),
            other => other.to_string(),
        };
        let kind = match error {
            SyncError::InvalidEndpoint(rule) => ErrorKind::InvalidEndpoint { rule },
            SyncError::InvalidInput(field) => ErrorKind::InvalidInput { field },
            SyncError::InvalidProxy(_) => ErrorKind::ProxyUnusable,
            SyncError::Network(NetworkError {
                cause: NetworkCause::NameLookup | NetworkCause::Unreachable,
                ..
            }) => ErrorKind::Offline,
            SyncError::Network(network) => ErrorKind::Network {
                head_received: network.head_arrived,
            },
            SyncError::Tls(_) => ErrorKind::Tls {
                self_hosted: server == ServerKind::SelfHosted,
            },
            SyncError::Timeout => ErrorKind::Timeout,
            SyncError::Status { code, .. } => ErrorKind::for_status(code),
            SyncError::Redirect(redirect) => ErrorKind::RedirectRefused { code: redirect.code },
            SyncError::Protocol(_) => ErrorKind::BadResponse,
            SyncError::Cancelled => ErrorKind::Cancelled,
            #[cfg(feature = "test-hooks")]
            SyncError::EndpointRefused(_) => ErrorKind::EndpointRefused,
        };
        let mut error = JobError::new(kind, method, detail);
        error.redirect = redirect;
        error
    }

    /// E6: an answer of `method` that does not decode; `detail` names the
    /// member or element, never the body.
    pub fn bad_response(method: Method, detail: impl Into<String>) -> JobError {
        JobError::new(ErrorKind::BadResponse, Some(method), detail)
    }

    /// E7: a local storage failure. A full disk is `disk_full`; a row that
    /// cannot be read is `db_check_required` (origin local); anything else is
    /// `local_storage`.
    pub fn from_storage(error: &StorageError) -> JobError {
        /// SQLite's primary result code for a full disk or database.
        const SQLITE_FULL: i32 = 13;
        let kind = match error {
            StorageError::Sqlite { code: SQLITE_FULL, .. } => ErrorKind::DiskFull {
                needed_bytes: None,
                available_bytes: None,
            },
            StorageError::Row(_) => ErrorKind::DbCheckRequired {
                origin: DbCheckOrigin::Local,
            },
            _ => ErrorKind::LocalStorage,
        };
        JobError::new(kind, None, error.to_string())
    }
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind.id())?;
        if let Some(method) = self.method_name() {
            write!(f, " ({method})")?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for JobError {}
