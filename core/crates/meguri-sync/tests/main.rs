//! This crate's integration tests in one binary (`[[test]] sync_it`).
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

#[path = "cloud_adapter.rs"]
pub mod cloud_adapter;
#[path = "cloud_adapter_regressions.rs"]
pub mod cloud_adapter_regressions;
#[path = "cloud_adoption_capacity.rs"]
pub mod cloud_adoption_capacity;
#[path = "cloud_adoption_prior_intents.rs"]
pub mod cloud_adoption_prior_intents;
#[path = "cloud_bound_adoption.rs"]
pub mod cloud_bound_adoption;
#[path = "cloud_collection_adoption.rs"]
pub mod cloud_collection_adoption;
#[path = "cloud_control_codec.rs"]
pub mod cloud_control_codec;
#[path = "cloud_entity_codec.rs"]
pub mod cloud_entity_codec;
#[path = "cloud_generation_runtime.rs"]
pub mod cloud_generation_runtime;
#[path = "cloud_incoming_intents.rs"]
pub mod cloud_incoming_intents;
#[path = "cloud_incoming_replies.rs"]
pub mod cloud_incoming_replies;
#[path = "cloud_incoming_review.rs"]
pub mod cloud_incoming_review;
#[path = "cloud_media_cleanup.rs"]
pub mod cloud_media_cleanup;
#[path = "cloud_media_codec.rs"]
pub mod cloud_media_codec;
#[path = "cloud_media_name_conflicts.rs"]
pub mod cloud_media_name_conflicts;
#[path = "cloud_media_r1.rs"]
pub mod cloud_media_r1;
#[path = "cloud_media_roundtrip.rs"]
pub mod cloud_media_roundtrip;
#[path = "cloud_payload_key.rs"]
pub mod cloud_payload_key;
#[path = "cloud_policy.rs"]
pub mod cloud_policy;
#[path = "cloud_publication.rs"]
pub mod cloud_publication;
#[path = "cloud_publication_adoption.rs"]
pub mod cloud_publication_adoption;
#[path = "cloud_publication_cleanup.rs"]
pub mod cloud_publication_cleanup;
#[path = "cloud_publication_driver.rs"]
pub mod cloud_publication_driver;
#[path = "cloud_publication_intent_replay.rs"]
pub mod cloud_publication_intent_replay;
#[path = "cloud_publication_media_write.rs"]
pub mod cloud_publication_media_write;
#[path = "cloud_publication_submission.rs"]
pub mod cloud_publication_submission;
#[path = "cloud_reset_adoption.rs"]
pub mod cloud_reset_adoption;
#[path = "cloud_review_roundtrip.rs"]
pub mod cloud_review_roundtrip;
#[path = "cloud_selected_control.rs"]
pub mod cloud_selected_control;
#[path = "cloud_setup.rs"]
pub mod cloud_setup;
#[cfg(feature = "test-hooks")]
#[path = "endpoint_guard.rs"]
pub mod endpoint_guard;
#[cfg(all(feature = "test-hooks", feature = "ankiweb"))]
#[path = "errors.rs"]
pub mod errors;
#[path = "media_files.rs"]
pub mod media_files;
#[path = "media_reconcile.rs"]
pub mod media_reconcile;
#[cfg(feature = "test-hooks")]
#[path = "normal_sync.rs"]
pub mod normal_sync;
#[path = "owned_round_admission.rs"]
pub mod owned_round_admission;
#[path = "provider_admission.rs"]
pub mod provider_admission;
#[path = "provider_media_accounting.rs"]
pub mod provider_media_accounting;
#[cfg(feature = "test-hooks")]
#[path = "status.rs"]
pub mod status;
#[cfg(all(feature = "test-hooks", feature = "ankiweb"))]
#[path = "transport.rs"]
pub mod transport;
#[cfg(feature = "test-hooks")]
#[path = "wire.rs"]
pub mod wire;
