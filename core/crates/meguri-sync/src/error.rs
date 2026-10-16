//! The crate's error: the categories the transport reports
//! (specs/sync/transport.md T11). Turning them into the kinds the user sees is
//! U106's (`errors.md`). No message here ever holds the host key, the password
//! or a request body (T11).

use std::fmt;

use crate::endpoint::EndpointRule;

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
    /// No connection: name lookup failed, refused, unreachable.
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
