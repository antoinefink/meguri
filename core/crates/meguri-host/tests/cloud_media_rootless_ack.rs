//! Rootless media ACK regressions require the Host composition root.
#[path = "support/media_rootless_ack.rs"]
mod rootless_ack;

use meguri_sync::cloud::{entity_codec as codec, media::ServerState, workflow::PendingChange};
use meguri_testkit::{
    cloud::{
        Asset, Caller, Config, ErrorCode, Field, Mutation, MutationSuccess, Operation, Record, RecordChange,
        RecordInput, Reply, RequestedFields, Service, SystemFields,
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::path::{Path, PathBuf};

// Exact synthetic inputs from meguri-sync/tests/support/publication_incoming.rs.
pub const NOW: i64 = 1_735_689_600_000;
pub const LOCAL: &[u8] = b"synthetic local created image";
const MEDIA: &str = "meguri-media";

pub fn generated(root: &Path, name: &str) -> PathBuf {
    let directory = root.join(name);
    let recipe = Recipe::parse("recipe: incoming-publication\ndescription: Public synthetic incoming tests\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Incoming\n    notetype: basic\n    new: 2\n    due: 0\n    tags: []\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, if name == "sender" { 801 } else { 802 }, &directory).unwrap();
    directory
}
