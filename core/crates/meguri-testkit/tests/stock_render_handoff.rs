//! Meguri's side of `notetypes/records/stock-render-handoff` (records.md N12,
//! notetype-ops.md T10.2): its segment is empty, so Meguri only makes its new
//! collection and hands it over at step 0. The oracle then adds one note of
//! each stock kind to deck 1, which Anki refuses when the collection has no
//! deck 1, and renders their cards. Only T1 runs the oracle's part
//! (`tools/mg conformance --live notetypes/records`); this test checks the
//! collection Meguri hands over holds the default deck and preset rows that
//! T10.2 gives byte for byte.

use std::path::{Path, PathBuf};

use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_testkit::exec::live::handoff_dir;
use meguri_testkit::exec::{MeguriExecutor, SegmentExecutor, SegmentRequest};
use meguri_testkit::scenario::{self, Side};

const ID: &str = "notetypes/records/stock-render-handoff";

/// T10.2's preset `config`: `model/protobuf-records` P6.6's example.
const PRESET_CONFIG: &str = "0a080000803f00002041120400002041221c0000803f0000803f0000803f0000803f0000803f0000803f0000803f481450c8015d00002040656666a63f6d9a99993f7d0000803f8001949d02880101900101980104a80101b00108c0013cad026666663fc5026666663fe00201";

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn bytes(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn meguri_hands_over_the_default_deck_and_preset_of_t10_2() {
    let loaded = scenario::load(&conformance().join(format!("scenarios/{ID}.yaml"))).unwrap();
    let segments = loaded.scenario.segments();
    let first = &segments[0];
    assert_eq!(first.side, Side::Meguri);
    assert_eq!(first.steps, 0..0, "Meguri's segment is empty (records.md N12)");
    assert_eq!(first.handoff, Some(0));

    let root = tempfile::tempdir().unwrap();
    let rel = handoff_dir(ID, 0);
    let out = MeguriExecutor::new()
        .segment(
            &loaded,
            &SegmentRequest {
                resume: None,
                steps: first.steps.clone(),
                hand_off_to: Some(rel.clone()),
                root: root.path().to_path_buf(),
            },
        )
        .unwrap();
    assert!(out.steps.is_empty());
    assert_eq!(out.handoff.as_ref().map(|h| h.dir.as_str()), Some(rel.as_str()));

    let db = PlainDatabase::open(&root.path().join(&rel).join("collection.anki2")).unwrap();
    let decks = db
        .rows(
            "SELECT id, name, mtime_secs, usn, common, kind FROM decks ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(
        decks,
        [vec![
            RawValue::Integer(1),
            RawValue::text("Default"),
            RawValue::Integer(0),
            RawValue::Integer(0),
            RawValue::Blob(bytes("08011001")),
            RawValue::Blob(bytes("0a020801")),
        ]]
    );
    let presets = db
        .rows(
            "SELECT id, name, mtime_secs, usn, config FROM deck_config ORDER BY id",
            &[],
        )
        .unwrap();
    assert_eq!(
        presets,
        [vec![
            RawValue::Integer(1),
            RawValue::text("Default"),
            RawValue::Integer(0),
            RawValue::Integer(0),
            RawValue::Blob(bytes(PRESET_CONFIG)),
        ]]
    );
    db.close().unwrap();
}
