//! Protobuf blob decoding never panics, on any input, and every record that
//! decodes re-encodes to bytes that decode to the same record
//! (specs/model/protobuf-records.md P2 to P4, P9).
//!
//! "The same" is `BlobMessage::equivalent`: floats bit for bit, except that
//! -0.0 in an implicit float comes back as 0.0 (P3), and unknown fields
//! compared in field-number order, the order P4 writes them in.

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_model::proto::BlobMessage;
use meguri_model::proto::deck::{DeckCommon, DeckKindMessage};
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};

fn check<M: BlobMessage + std::fmt::Debug>(data: &[u8]) {
    let Ok(record) = M::decode(data) else {
        return;
    };
    let encoded = record.encode();
    let again = M::decode(&encoded).expect("canonical bytes decode");
    assert!(again.equivalent(&record), "{record:?} -> {again:?}");
    assert_eq!(again.encode(), encoded, "canonical encoding is stable");
}

fuzz_target!(|data: &[u8]| {
    check::<NotetypeConfig>(data);
    check::<FieldConfig>(data);
    check::<TemplateConfig>(data);
    check::<DeckCommon>(data);
    check::<DeckKindMessage>(data);
    check::<DeckConfigMessage>(data);
});
