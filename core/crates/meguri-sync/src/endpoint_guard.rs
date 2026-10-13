//! Sync endpoint guard (R1876): no test, harness or scenario may sync with
//! AnkiWeb. Every sync endpoint a test uses passes [`check_endpoint`] before
//! anything connects to it. This module is the one copy of the rule: the
//! `test-hooks` transport applies it, and meguri-testkit re-exports it for
//! harnesses (`exec::sync_guard`). It exists only with feature `test-hooks`,
//! so a build without it (AppStore) compiles in no AnkiWeb name.
//!
//! The guard reads the endpoint text only: it never resolves a name or opens a
//! socket, so a refused endpoint gets no network call. It is an allowlist. A
//! host passes when it is
//! - loopback: `127.0.0.0/8`, `[::1]` or `localhost`;
//! - a private range: `10/8`, `172.16/12`, `192.168/16` or `[fc00::/7]`;
//! - a Docker compose name: one DNS label that starts with a letter, such as
//!   a service (`syncserver`, `toxiproxy`) or a `compose run` container name;
//! - a name under the reserved `test` top-level domain (RFC 6761), such as the
//!   stand-in AnkiWeb suffix that tests pin to a local stub. Like a compose
//!   name, it resolves only through local configuration.
//!
//! An AnkiWeb host is refused by name first, so its refusal says so. Anything
//! the guard cannot classify with certainty is refused: a WHATWG URL parser
//! (the one sync clients use) reads `http://2130706433/` and `http://127.1/` as
//! IPv4 addresses and strips tabs inside a URL, so the guard accepts only the
//! strict dotted quad, names of ASCII letters, digits, `-` and `_`, and
//! authorities without user info.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

/// AnkiWeb's registrable domain: the host itself and every name under it are
/// refused. The only AnkiWeb host name in tests and tooling (U19's scan);
/// tests build their AnkiWeb cases from it.
pub const ANKIWEB_HOST: &str = "ankiweb.net";

/// Why an endpoint passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allowed {
    Loopback,
    PrivateRange,
    ComposeName,
    /// A name under the reserved `test` TLD, such as `sync20.stub.test`.
    ReservedTestName,
}

/// Why an endpoint was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The host is AnkiWeb or a name under it.
    AnkiWeb,
    /// The host is not loopback, a private address, a compose name or a name
    /// under `.test`.
    NotLocal,
    /// The endpoint is not an `http`/`https` URL or `host[:port]` with a host
    /// the guard can read without ambiguity.
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub endpoint: String,
    pub reason: Reason,
    /// What in the endpoint broke the rule, for people.
    pub detail: String,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = match self.reason {
            Reason::AnkiWeb => "AnkiWeb is never contacted by tests; use the local sync server",
            Reason::NotLocal => "only loopback, private addresses, compose names and .test names are allowed",
            Reason::Malformed => "the endpoint cannot be read without ambiguity",
        };
        write!(
            f,
            "sync endpoint {:?} refused (R1876): {rule}: {}",
            self.endpoint, self.detail
        )
    }
}

impl std::error::Error for Refused {}

/// Checks a sync endpoint: an `http://` or `https://` URL, or a bare
/// `host[:port]` such as `syncserver:8080`.
pub fn check_endpoint(endpoint: &str) -> Result<Allowed, Refused> {
    let refuse = |reason: Reason, detail: String| {
        Err(Refused {
            endpoint: endpoint.to_owned(),
            reason,
            detail,
        })
    };

    let (scheme, rest) = match endpoint.find("://") {
        Some(i) => (Some(&endpoint[..i]), &endpoint[i + 3..]),
        None => (None, endpoint),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let (user_info, host_port) = match authority.rfind('@') {
        Some(i) => (Some(&authority[..i]), &authority[i + 1..]),
        None => (None, authority),
    };
    let (host, port, bracketed) = match split_host_port(host_port) {
        Some(parts) => parts,
        None => return refuse(Reason::Malformed, format!("authority {authority:?}")),
    };

    if !bracketed && is_ankiweb(host) {
        return refuse(Reason::AnkiWeb, format!("host {host}"));
    }
    if let Some(scheme) = scheme
        && !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    {
        return refuse(Reason::Malformed, format!("scheme {scheme:?} is not http or https"));
    }
    if user_info.is_some() {
        return refuse(Reason::Malformed, "user info in the authority".to_owned());
    }
    if let Some(port) = port
        && !valid_port(port)
    {
        return refuse(Reason::Malformed, format!("port {port:?}"));
    }
    if host.is_empty() {
        return refuse(Reason::Malformed, "no host".to_owned());
    }

    if bracketed {
        let Ok(ip) = host.parse::<Ipv6Addr>() else {
            return refuse(Reason::Malformed, format!("IPv6 literal [{host}]"));
        };
        return if ip == Ipv6Addr::LOCALHOST {
            Ok(Allowed::Loopback)
        } else if ip.segments()[0] & 0xfe00 == 0xfc00 {
            Ok(Allowed::PrivateRange)
        } else {
            refuse(Reason::NotLocal, format!("host [{ip}]"))
        };
    }

    if !host
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return refuse(
            Reason::Malformed,
            format!("host {host:?} has characters outside ASCII letters, digits, '.', '-' and '_'"),
        );
    }
    let host = host.to_ascii_lowercase();
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return if ip.is_loopback() {
            Ok(Allowed::Loopback)
        } else if ip.is_private() {
            Ok(Allowed::PrivateRange)
        } else {
            refuse(Reason::NotLocal, format!("host {ip}"))
        };
    }
    if host == "localhost" {
        return Ok(Allowed::Loopback);
    }
    if !host.contains('.') && host.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Ok(Allowed::ComposeName);
    }
    if is_reserved_test_name(&host) {
        return Ok(Allowed::ReservedTestName);
    }
    refuse(Reason::NotLocal, format!("host {host}"))
}

/// `<label>(.<label>)*.test` with no empty label. A trailing dot is refused,
/// as it is for every other name.
fn is_reserved_test_name(host: &str) -> bool {
    host.strip_suffix(".test")
        .is_some_and(|head| !head.is_empty() && head.split('.').all(|label| !label.is_empty()))
}

/// `host`, `host:port`, `[v6]` or `[v6]:port` → (host, port, bracketed).
/// `None` when the brackets or colons do not split cleanly.
fn split_host_port(host_port: &str) -> Option<(&str, Option<&str>, bool)> {
    if let Some(inner) = host_port.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        return match after {
            "" => Some((host, None, true)),
            _ => Some((host, Some(after.strip_prefix(':')?), true)),
        };
    }
    match host_port.split_once(':') {
        None => Some((host_port, None, false)),
        Some((_, port)) if port.contains(':') => None,
        Some((host, port)) => Some((host, Some(port), false)),
    }
}

fn is_ankiweb(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    host == ANKIWEB_HOST || host.strip_suffix(ANKIWEB_HOST).is_some_and(|head| head.ends_with('.'))
}

fn valid_port(port: &str) -> bool {
    !port.is_empty()
        && port.bytes().all(|b| b.is_ascii_digit())
        && port.parse::<u32>().is_ok_and(|p| (1..=65_535).contains(&p))
}
