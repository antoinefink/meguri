//! Sync endpoints (specs/sync/login-endpoints.md L1-L4): normalising a
//! server URL into a base, method URLs, the server kinds a build offers and
//! the redirect host rule (transport.md T7). The core keeps no endpoint
//! between jobs (L4, R1502): every job is given its base.

use std::fmt;

use url::{Host, Url};

use crate::error::RedirectRefusal;
use crate::transport::Method;

/// A sync base: an `http` or `https` URL with a host, no credentials, no
/// query or fragment, and a path ending with `/` (L1).
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Endpoint(Url);

impl Endpoint {
    /// Turns server URL text into a base, or names the rule it breaks
    /// (login-endpoints.md L1). The app validates its settings field with this
    /// and passes only its result to jobs.
    pub fn parse(text: &str) -> Result<Endpoint, EndpointRule> {
        let text = text.trim();
        if text.is_empty() {
            return Err(EndpointRule::Empty);
        }
        let mut url = Url::parse(text).map_err(|_| EndpointRule::Unparseable)?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(EndpointRule::UnsupportedScheme);
        }
        if url.host_str().is_none_or(str::is_empty) {
            return Err(EndpointRule::MissingHost);
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(EndpointRule::Credentials);
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(EndpointRule::QueryOrFragment);
        }
        if !url.path().ends_with('/') {
            let path = format!("{}/", url.path());
            url.set_path(&path);
        }
        Ok(Endpoint(url))
    }

    /// The parser's serialisation: lower-case scheme and host, no default
    /// port, the path percent-encoded, a final `/`.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn is_https(&self) -> bool {
        self.0.scheme() == "https"
    }

    /// The base followed by `sync/<method>` or `msync/<method>` (L2): plain
    /// concatenation, since the base ends with `/`.
    pub fn method_url(&self, method: Method) -> String {
        let area = if method.is_media() { "msync" } else { "sync" };
        format!("{}{area}/{}", self.0.as_str(), method.name())
    }

    fn host(&self) -> Option<Host<&str>> {
        self.0.host()
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Endpoint({:?})", self.as_str())
    }
}

/// The L1 rule a server URL breaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointRule {
    Empty,
    Unparseable,
    UnsupportedScheme,
    MissingHost,
    Credentials,
    QueryOrFragment,
}

impl EndpointRule {
    pub fn as_str(self) -> &'static str {
        match self {
            EndpointRule::Empty => "empty",
            EndpointRule::Unparseable => "unparseable",
            EndpointRule::UnsupportedScheme => "unsupported_scheme",
            EndpointRule::MissingHost => "missing_host",
            EndpointRule::Credentials => "credentials",
            EndpointRule::QueryOrFragment => "query_or_fragment",
        }
    }
}

// The kinds of server a build offers (L3, R1496, R2031, R2032). The hosted
// service's kind exists only with feature `ankiweb`: without it the variant,
// its name in debug output and its endpoint are not compiled in (KTD21). Doc
// comments outside that feature never name the service either: they reach
// the library's metadata.
/// A kind of sync server the build offers. The words the UI shows for each
/// are the app's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerKind {
    #[cfg(feature = "ankiweb")]
    AnkiWeb,
    /// The user's own server: the URL they entered, after L1.
    SelfHosted,
}

/// The server kinds this build offers, in the order the UI lists them (L3).
pub const SERVER_CHOICES: &[ServerKind] = &[
    #[cfg(feature = "ankiweb")]
    ServerKind::AnkiWeb,
    ServerKind::SelfHosted,
];

// Without the build switch only the self-hosted server is offered (R2032).
// Checked whenever the crate compiles without it, as the scan build of
// tools/mg core-lint does.
#[cfg(not(feature = "ankiweb"))]
const _: () = assert!(SERVER_CHOICES.len() == 1 && matches!(SERVER_CHOICES[0], ServerKind::SelfHosted));

/// The base a 308 on `meta` moves a job to, when T7's checks allow it: the
/// `Location` text read as an endpoint (L1), `https` kept when the current
/// base has it, and the host rule of the job's server kind (R1510).
pub fn follow_redirect(current: &Endpoint, kind: ServerKind, location: &str) -> Result<Endpoint, RedirectRefusal> {
    let target = Endpoint::parse(location).map_err(RedirectRefusal::Target)?;
    if current.is_https() && !target.is_https() {
        return Err(RedirectRefusal::LostHttps);
    }
    let host_allowed = match kind {
        // A name equal to the service's suffix or under it; never an address.
        #[cfg(feature = "ankiweb")]
        ServerKind::AnkiWeb => {
            matches!(target.host(), Some(Host::Domain(name)) if crate::ankiweb::is_service_host(name))
        }
        // Names compare in ASCII lower case (the parser lowered them) and
        // addresses as addresses; the port may change.
        ServerKind::SelfHosted => target.host() == current.host(),
    };
    if host_allowed {
        Ok(target)
    } else {
        Err(RedirectRefusal::Host)
    }
}
