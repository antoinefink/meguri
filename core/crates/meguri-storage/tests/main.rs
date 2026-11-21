//! This crate's integration tests in one binary (`[[test]] storage_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "activity.rs"]
pub mod activity;
#[path = "atomic_file.rs"]
pub mod atomic_file;
#[path = "card_note_id_projection.rs"]
pub mod card_note_id_projection;
#[path = "card_ordinals.rs"]
pub mod card_ordinals;
#[path = "cloud_rekey.rs"]
pub mod cloud_rekey;
#[cfg(feature = "test-hooks")]
#[path = "collation_order.rs"]
pub mod collation_order;
#[path = "collection_package.rs"]
pub mod collection_package;
#[cfg(feature = "test-hooks")]
#[path = "config_bytes.rs"]
pub mod config_bytes;
#[cfg(feature = "test-hooks")]
#[path = "connections.rs"]
pub mod connections;
#[cfg(feature = "test-hooks")]
#[path = "day_writes.rs"]
pub mod day_writes;
#[cfg(feature = "test-hooks")]
#[path = "derived_store.rs"]
pub mod derived_store;
#[cfg(feature = "test-hooks")]
#[path = "graves.rs"]
pub mod graves;
#[cfg(feature = "test-hooks")]
#[path = "ids.rs"]
pub mod ids;
#[path = "journal_images.rs"]
pub mod journal_images;
#[path = "live_snapshot.rs"]
pub mod live_snapshot;
#[path = "media_add.rs"]
pub mod media_add;
#[path = "media_bound_reset.rs"]
pub mod media_bound_reset;
#[path = "media_capacity.rs"]
pub mod media_capacity;
#[path = "media_cloud_guard.rs"]
pub mod media_cloud_guard;
#[path = "media_collection_import.rs"]
pub mod media_collection_import;
#[path = "media_inventory.rs"]
pub mod media_inventory;
#[path = "media_journal.rs"]
pub mod media_journal;
#[path = "media_scan.rs"]
pub mod media_scan;
#[path = "media_staging.rs"]
pub mod media_staging;
#[path = "media_sync_storage.rs"]
pub mod media_sync_storage;
#[path = "media_trash.rs"]
pub mod media_trash;
#[path = "newcloud_read_rows.rs"]
pub mod newcloud_read_rows;
#[cfg(feature = "test-hooks")]
#[path = "noop_writes.rs"]
pub mod noop_writes;
#[path = "notetype_gapped_children.rs"]
pub mod notetype_gapped_children;
#[path = "one_way.rs"]
pub mod one_way;
#[cfg(feature = "test-hooks")]
#[path = "open_budget.rs"]
pub mod open_budget;
#[cfg(feature = "test-hooks")]
#[path = "open_refuse.rs"]
pub mod open_refuse;
#[path = "package_inspection.rs"]
pub mod package_inspection;
#[cfg(feature = "test-hooks")]
#[path = "savepoints.rs"]
pub mod savepoints;
#[cfg(feature = "test-hooks")]
#[path = "schema_objects.rs"]
pub mod schema_objects;
#[path = "search_rows.rs"]
pub mod search_rows;
#[path = "sidecar.rs"]
pub mod sidecar;
#[path = "smoke_integrity.rs"]
pub mod smoke_integrity;
#[cfg(feature = "test-hooks")]
#[path = "snapshots.rs"]
pub mod snapshots;
#[path = "verified_replace.rs"]
pub mod verified_replace;
