//! Shared helpers: fixture copies in temporary directories, file digests and
//! directory listings, and raw SQL shortcuts. Fixtures are read only to copy
//! them; SQLite only ever opens the copies.

#![allow(dead_code)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{OpenError, OpenOptions, OpenedCollection, Reader, Writer, open_collection};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

/// 2030-01-15T10:00:00Z, the storage scenarios' clock.
pub const NOW_MS: i64 = 1_894_701_600_000;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures/storage")
        .join(name)
}

/// A temporary directory holding a copy of the fixture as
/// `collection.anki2`.
pub fn copy_fixture(name: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(fixture(name), &path).unwrap();
    (dir, path)
}

pub fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).unwrap();
    Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn listing(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

pub fn clock() -> FixedClock {
    FixedClock::new(NOW_MS, UtcOffset::UTC)
}

pub fn open(path: &Path) -> Result<OpenedCollection, OpenError> {
    let clock = clock();
    let random = SeededRandom::new(7);
    open_collection(path, &OpenOptions::new(&clock, &random))
}

pub fn exec(writer: &Writer, sql: &str) -> RawRows {
    writer.execute_raw(sql, &[]).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

pub fn query(reader: &Reader, sql: &str) -> RawRows {
    reader.execute_raw(sql, &[]).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

/// The only value of a one-row, one-column result.
pub fn single(rows: &RawRows) -> SqlValue {
    assert_eq!(rows.rows.len(), 1, "{rows:?}");
    assert_eq!(rows.rows[0].len(), 1, "{rows:?}");
    rows.rows[0][0].clone()
}

pub fn int(rows: &RawRows) -> i64 {
    match single(rows) {
        SqlValue::Integer(v) => v,
        other => panic!("not an integer: {other:?}"),
    }
}

pub fn text(rows: &RawRows) -> String {
    match single(rows) {
        SqlValue::Text(v) => v,
        other => panic!("not text: {other:?}"),
    }
}

/// `(type, name)` of every object in `sqlite_master`, names lower-cased:
/// SQLite names are case-insensitive, and releases at schema 17 stored the
/// table `FIELDS`.
pub fn object_set(writer: &Writer) -> BTreeSet<(String, String)> {
    writer
        .schema_objects()
        .unwrap()
        .into_iter()
        .map(|o| (o.kind, o.name.to_ascii_lowercase()))
        .collect()
}

/// The required tables and indexes of `storage/schema-18` S2 and S3 plus the
/// statistics tables of S5: the objects of a file Anki or Meguri created
/// (S6.2). Written out from the spec, not taken from the crate.
pub fn anki_made_objects() -> BTreeSet<(String, String)> {
    let tables = [
        "col",
        "notes",
        "cards",
        "revlog",
        "graves",
        "deck_config",
        "config",
        "tags",
        "fields",
        "templates",
        "notetypes",
        "decks",
        "sqlite_stat1",
        "sqlite_stat4",
    ];
    let indexes = [
        "ix_notes_usn",
        "ix_cards_usn",
        "ix_revlog_usn",
        "ix_cards_nid",
        "ix_cards_sched",
        "ix_revlog_cid",
        "ix_notes_csum",
        "idx_fields_name_ntid",
        "idx_templates_name_ntid",
        "idx_templates_usn",
        "idx_notetypes_name",
        "idx_notetypes_usn",
        "idx_decks_name",
        "idx_notes_mid",
        "idx_cards_odid",
        "idx_graves_pending",
    ];
    tables
        .iter()
        .map(|t| ("table".to_owned(), (*t).to_owned()))
        .chain(indexes.iter().map(|i| ("index".to_owned(), (*i).to_owned())))
        .collect()
}

/// A plain SQLite connection to a test's own file (never a fixture under
/// `conformance/`), for building files the gate must handle.
pub fn plain(path: &Path) -> meguri_storage::rusqlite::Connection {
    meguri_storage::rusqlite::Connection::open(path).unwrap()
}

/// `conformance/` of this checkout.
pub fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// The oracle's observations of a storage scenario (its golden), with the
/// deviations of `conformance/deviations.yaml` applied: what Meguri must
/// observe.
pub fn expected_steps(scenario: &str) -> Vec<serde_json::Value> {
    let dir = conformance_dir();
    let text = fs::read_to_string(dir.join(format!("goldens/{scenario}.json"))).unwrap();
    let golden = meguri_testkit::observe::parse_json(&text).unwrap();
    let mut observations = golden["observations"].clone();
    let suite = meguri_testkit::suite::Suite::open(&dir).unwrap();
    for deviation in suite
        .deviations
        .iter()
        .filter(|d| d.scenarios.iter().any(|s| s == scenario))
    {
        assert!(
            meguri_testkit::diff::transforms::apply(deviation, &mut observations),
            "deviation {} matches nothing",
            deviation.id
        );
    }
    observations["steps"].as_array().unwrap().clone()
}

/// A `col` row at schema 18 created and last changed at `now_ms`.
pub fn new_col(now_ms: i64) -> meguri_model::col::Col {
    use meguri_model::col::Col;
    use meguri_model::time::{EpochMillis, EpochSecs};
    Col {
        version: 18,
        created: EpochSecs(now_ms / 1000),
        modified: EpochMillis(now_ms),
        schema_modified: EpochMillis(now_ms),
        ..Col::default()
    }
}

/// The config rows a collection needs to open: `schedVer` 2 (the open
/// gate refuses the v1 scheduler).
pub fn new_config() -> Vec<meguri_model::config::ConfigEntry> {
    vec![meguri_model::config::ConfigEntry {
        key: "schedVer".to_owned(),
        value: b"2".to_vec(),
        ..meguri_model::config::ConfigEntry::default()
    }]
}

/// A new, empty collection made by the storage-only builder, closed.
pub fn empty_collection() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = clock();
    let random = SeededRandom::new(7);
    let builder = meguri_storage::CollectionBuilder::create(
        &path,
        &new_col(NOW_MS),
        &new_config(),
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    builder.finish().unwrap();
    (dir, path)
}

/// A note of note type `mid` with these field values.
pub fn note(mid: i64, fields: &[&str]) -> meguri_model::note::Note {
    meguri_model::note::Note {
        guid: format!("g{}", fields.join("")),
        notetype_id: meguri_model::ids::NotetypeId(mid),
        fields: fields.join("\u{1f}"),
        sort_field: fields.first().copied().unwrap_or_default().to_owned(),
        ..meguri_model::note::Note::default()
    }
}

/// A new card of a note.
pub fn card(note: meguri_model::ids::NoteId, ord: u16) -> meguri_model::card::Card {
    use meguri_model::card::{Card, CardQueue, CardType};
    use meguri_model::ids::{CardId, DeckId};
    Card {
        id: CardId(0),
        note_id: note,
        deck_id: DeckId(1),
        ord,
        mtime: meguri_model::time::EpochSecs(0),
        usn: meguri_model::usn::Usn(0),
        card_type: CardType::New,
        queue: CardQueue::New,
        due: 1,
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: meguri_model::card_data::CardData::default(),
    }
}

/// A review-log row of a card.
pub fn revlog(card: meguri_model::ids::CardId, time: u32) -> meguri_model::revlog::RevlogEntry {
    use meguri_model::revlog::{RevlogEntry, RevlogKind};
    RevlogEntry {
        id: meguri_model::ids::RevlogId(0),
        card_id: card,
        usn: meguri_model::usn::Usn(0),
        button: 3,
        interval: -600,
        last_interval: 0,
        ease_factor: 0,
        answer_millis: time,
        kind: RevlogKind::Learning,
    }
}

/// A normal deck named `name` (native form).
pub fn deck(name: &str) -> meguri_model::deck::Deck {
    use meguri_model::proto::ProtoBlob;
    use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck};
    meguri_model::deck::Deck {
        id: meguri_model::ids::DeckId(0),
        name: name.to_owned(),
        mtime: meguri_model::time::EpochSecs(0),
        usn: meguri_model::usn::Usn(0),
        common: ProtoBlob::new(DeckCommon::default()),
        kind: ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(NormalDeck::default()))),
    }
}

/// A deck preset named `name`.
pub fn preset(name: &str) -> meguri_model::deck_config::DeckConfig {
    use meguri_model::proto::ProtoBlob;
    use meguri_model::proto::deck_config::DeckConfigMessage;
    meguri_model::deck_config::DeckConfig {
        id: meguri_model::ids::DeckConfigId(0),
        name: name.to_owned(),
        mtime: meguri_model::time::EpochSecs(0),
        usn: meguri_model::usn::Usn(0),
        config: ProtoBlob::new(DeckConfigMessage::default()),
    }
}

/// A standard note type with these fields and templates.
pub fn notetype(name: &str, fields: &[&str], templates: &[&str]) -> meguri_model::notetype::Notetype {
    use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
    use meguri_model::proto::ProtoBlob;
    use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
    Notetype {
        id: meguri_model::ids::NotetypeId(0),
        name: name.to_owned(),
        mtime: meguri_model::time::EpochSecs(0),
        usn: meguri_model::usn::Usn(0),
        config: ProtoBlob::new(NotetypeConfig::default()),
        fields: fields
            .iter()
            .enumerate()
            .map(|(i, f)| NotetypeField {
                ord: i as u32,
                name: (*f).to_owned(),
                config: ProtoBlob::new(FieldConfig::default()),
                prior_ord: None,
            })
            .collect(),
        templates: templates
            .iter()
            .enumerate()
            .map(|(i, t)| NotetypeTemplate {
                ord: i as u32,
                name: (*t).to_owned(),
                mtime: meguri_model::time::EpochSecs(0),
                usn: meguri_model::usn::Usn(0),
                config: ProtoBlob::new(TemplateConfig::default()),
                prior_ord: None,
            })
            .collect(),
    }
}

/// A tag row.
pub fn tag(name: &str) -> meguri_model::tag::Tag {
    meguri_model::tag::Tag {
        name: name.to_owned(),
        ..meguri_model::tag::Tag::default()
    }
}
