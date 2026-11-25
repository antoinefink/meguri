//! This crate's integration tests in one binary (`[[test]] server_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "accounts.rs"]
pub mod accounts;
#[path = "auth.rs"]
pub mod auth;
#[path = "backups.rs"]
pub mod backups;
#[path = "backup_round_trip.rs"]
pub mod backup_round_trip;
#[path = "browse.rs"]
pub mod browse;
#[path = "codecs.rs"]
pub mod codecs;
#[path = "deletion.rs"]
pub mod deletion;
#[path = "full.rs"]
pub mod full;
#[path = "http_contract.rs"]
pub mod http_contract;
#[path = "mcp_oauth.rs"]
pub mod mcp_oauth;
#[path = "media.rs"]
pub mod media;
#[path = "media_disk_full.rs"]
pub mod media_disk_full;
#[path = "media_recovery.rs"]
pub mod media_recovery;
#[path = "normal.rs"]
pub mod normal;
#[path = "provision.rs"]
pub mod provision;
#[path = "races.rs"]
pub mod races;
#[path = "reset_data.rs"]
pub mod reset_data;
#[path = "recovery.rs"]
pub mod recovery;
#[path = "session.rs"]
pub mod session;
#[path = "transport.rs"]
pub mod transport;
