//! Sync protocol v11: the transport every sync path uses (login, the status
//! check, normal, one-way and media sync), endpoints and login
//! (specs/sync/transport.md, specs/sync/login-endpoints.md), the bodies of
//! every method (`wire`, specs/sync/wire-format.md), the `meta` status check
//! and the sync status (`status`, specs/sync/meta-status.md) and the error
//! kinds a failed job reports ([`JobError`], specs/sync/errors.md). Normal,
//! one-way and media sync and the sync policy build on [`Job`].
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
pub mod media;
pub mod oneway;
pub mod policy;
pub mod status;
pub mod transport;
pub mod wire;

pub use endpoint::{Endpoint, EndpointRule, SERVER_CHOICES, ServerKind, follow_redirect};
pub use error::{
    DbCheckOrigin, ErrorClass, ErrorKind, InvalidInput, JobError, NetworkCause, NetworkError, ProtocolError,
    RedirectDiagnostic, RedirectError, RedirectRefusal, Retry, SyncError,
};
pub use identity::client_identity;
pub use job::{HostKey, Job, JobOptions, Proxy, ResponseBody};
pub use login::{decode_login_answer, login, login_blocking};
pub use status::{
    CheckOutcome, CheckResult, LocalRecord, OfflineStatus, ProbeOutcome, StatusMemory, StatusOutcome, SyncStatus,
};
pub use transport::{Method, Payload, SyncTransport};

pub mod normal;
