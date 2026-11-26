//! This crate's integration tests in one binary (`[[test]] host_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

/// `meguri_testkit::cloud::child_driver` starts its child process as this
/// binary with `--exact cloud_child_entry`. Here the entries the files define
/// are `<file>::cloud_child_entry`, so the child runs this one, the same call.
/// Outside such a child it returns at once, as theirs do.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[path = "cloud_cleanup_listing_q11.rs"]
pub mod cloud_cleanup_listing_q11;
#[path = "cloud_media_rootless_ack.rs"]
pub mod cloud_media_rootless_ack;
#[path = "cloud_publication_backup_provenance.rs"]
pub mod cloud_publication_backup_provenance;
#[path = "cloud_sdk_registration.rs"]
pub mod cloud_sdk_registration;
#[path = "collection_export_safety.rs"]
pub mod collection_export_safety;
#[path = "lifecycle.rs"]
pub mod lifecycle;
#[path = "manual_transfer_data.rs"]
pub mod manual_transfer_data;
#[path = "manual_transfers_rig.rs"]
pub mod manual_transfers_rig;
#[path = "own_handoff_attribution.rs"]
pub mod own_handoff_attribution;
#[path = "ports.rs"]
pub mod ports;
#[path = "provider_admission.rs"]
pub mod provider_admission;
#[cfg(feature = "test-hooks")]
#[path = "provider_bootstrap.rs"]
pub mod provider_bootstrap;
#[path = "provider_fixture_transport.rs"]
pub mod provider_fixture_transport;
#[cfg(feature = "test-hooks")]
#[path = "provider_import.rs"]
pub mod provider_import;
#[cfg(feature = "test-hooks")]
#[path = "provider_normal.rs"]
pub mod provider_normal;
// Keep the removal suite's `super::{b, r, s}` on the structure fixtures,
// including their shared serialization lane.
#[cfg(feature = "test-hooks")]
use provider_structure::{bootstrap as b, round as r, structure as s};
#[cfg(feature = "test-hooks")]
#[path = "provider_notetype_removal.rs"]
pub mod provider_notetype_removal;
#[cfg(feature = "test-hooks")]
#[path = "provider_setup.rs"]
pub mod provider_setup;
#[cfg(feature = "test-hooks")]
#[path = "provider_structure.rs"]
pub mod provider_structure;
#[path = "review_close_suspend.rs"]
pub mod review_close_suspend;
#[path = "review_media_fixture.rs"]
pub mod review_media_fixture;
