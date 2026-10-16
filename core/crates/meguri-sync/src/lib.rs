//! Sync protocol v11: the transport every sync path uses (login, the status
//! check, normal, one-way and media sync), endpoints and login
//! (specs/sync/transport.md, specs/sync/login-endpoints.md). Normal, one-way
//! and media sync and the sync policy build on [`Job`].
//!
//! The core keeps no endpoint, host key or redirect between jobs (R1502):
//! every job is given what it needs, and a followed redirect comes back as
//! [`Job::new_endpoint`].

// Built only with the build switch; nothing outside it names the service.
#[cfg(feature = "ankiweb")]
pub mod ankiweb;
pub mod endpoint;
#[cfg(feature = "test-hooks")]
pub mod endpoint_guard;
pub mod error;
pub mod identity;
pub mod job;
pub mod login;
pub mod transport;

pub use endpoint::{Endpoint, EndpointRule, SERVER_CHOICES, ServerKind, follow_redirect};
pub use error::{InvalidInput, NetworkCause, NetworkError, ProtocolError, RedirectError, RedirectRefusal, SyncError};
pub use identity::client_identity;
pub use job::{HostKey, Job, JobOptions, Proxy, ResponseBody};
pub use login::{login, login_blocking};
pub use transport::{Method, Payload, SyncTransport};
