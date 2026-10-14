//! Protobuf blob decoding never panics, on any input: every input gives a
//! record or a P9 error. Every record that decodes re-encodes (P3, P4) to
//! bytes that decode to an equal record, and encoding that record again gives
//! the same bytes (specs/model/protobuf-records.md P9).
//!
//! "Equal" is `BlobMessage::equivalent`: P4 equality (known values bit for
//! bit, unknown fields per field number in read order), except that -0.0 in
//! an implicit float comes back as 0.0 (P3).

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
