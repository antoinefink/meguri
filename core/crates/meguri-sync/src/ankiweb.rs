//! AnkiWeb (feature `ankiweb`, KTD21; specs/sync/login-endpoints.md L3,
//! transport.md T7 and T12). This module is the only place the AnkiWeb names
//! exist: without the feature nothing naming AnkiWeb is compiled in. The
//! AnkiWeb upload limits (U109) join it.
//!
//! With `test-hooks` as well, tests replace both names with one stand-in
//! under the reserved `.test` domain ([`use_stand_in`]), so no test holds an
//! AnkiWeb host (U19) and the endpoint guard keeps every test build local
//! (R1876).

use crate::endpoint::Endpoint;

/// The host suffix of T7's redirect rule: this host or any name under it.
pub const HOST_SUFFIX: &str = "ankiweb.net";

/// The AnkiWeb default endpoint (L3).
pub const DEFAULT_ENDPOINT: &str = "https://sync.ankiweb.net/";

/// The base an AnkiWeb job starts from when the app holds no redirected
/// endpoint (L3, L4).
pub fn default_endpoint() -> Endpoint {
    #[cfg(feature = "test-hooks")]
    if let Some((_, endpoint)) = stand_in() {
        return endpoint;
    }
    Endpoint::parse(DEFAULT_ENDPOINT).expect("the AnkiWeb default endpoint is a valid base")
}

/// Whether a host name (already in ASCII lower case) may receive an AnkiWeb
/// job's `meta` redirect: the suffix itself or a name ending with `.` and the
/// suffix (T7).
pub(crate) fn is_service_host(host: &str) -> bool {
    #[cfg(feature = "test-hooks")]
    if let Some((suffix, _)) = stand_in() {
        return under(host, &suffix);
    }
    under(host, HOST_SUFFIX)
}

fn under(host: &str, suffix: &str) -> bool {
    host == suffix || host.strip_suffix(suffix).is_some_and(|head| head.ends_with('.'))
}

#[cfg(feature = "test-hooks")]
static STAND_IN: std::sync::RwLock<Option<(String, Endpoint)>> = std::sync::RwLock::new(None);

#[cfg(feature = "test-hooks")]
fn stand_in() -> Option<(String, Endpoint)> {
    STAND_IN
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Test hook (T12): replaces the host suffix and the default endpoint for the
/// rest of the process, for example `aw.test` and `https://sync.aw.test/`.
/// Tests pin those names to a local stub (`JobOptions::pin_host`).
#[cfg(feature = "test-hooks")]
pub fn use_stand_in(host_suffix: &str, default_endpoint: &str) -> Result<(), crate::endpoint::EndpointRule> {
    let endpoint = Endpoint::parse(default_endpoint)?;
    let suffix = host_suffix.to_ascii_lowercase();
    *STAND_IN.write().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((suffix, endpoint));
    Ok(())
}
