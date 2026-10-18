//! New-collection B1-B8: defaults, atomic publication, and creation timing.
use meguri_engine::{Engine, EngineConfig, OpenConfig, UndoScope, bootstrap};
use meguri_model::{
    clock::{Clock, FixedClock, UtcOffset},
    faultpoint::{self, Fault},
    ids::{DeckConfigId, DeckId},
    rng::SeededRandom,
};
use meguri_storage::{ConfigValue, OpenOptions, ReadRows, open_collection};
use meguri_testkit::exec::clock::ScenarioClock;
use std::sync::Arc;

#[test]
fn defaults_open_without_undo_or_bootstrap_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(5));
    bootstrap::create(&path, clock.as_ref(), random.as_ref()).unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let collection = engine.open_raw(&path, OpenConfig::new(clock, random)).unwrap();
    let r = collection.reader(meguri_storage::ReaderRole::Query).unwrap();
    let col = r.col().unwrap();
    assert_eq!(
        (col.created.0, col.modified.0, col.schema_modified.0),
        (1_894_680_000, 0, 1_894_701_600_000)
    );
    assert_eq!(r.config_keys().unwrap().len(), 16); // B4; automatic open uses the effective default.
    assert_eq!(
        r.config_value("activeDecks").unwrap(),
        Some(ConfigValue::Array(vec![ConfigValue::from(1_i64)]))
    );
    assert_eq!(r.config_value("nextPos").unwrap(), Some(ConfigValue::from(1_i64)));
    let default = r.deck(DeckId(1)).unwrap().unwrap();
    assert_eq!(default.name, "Default");
    assert_eq!(default.common.to_bytes().as_ref(), [8, 1, 16, 1]);
    assert_eq!(default.kind.to_bytes().as_ref(), [10, 2, 8, 1]);
    assert_eq!((default.mtime.0, default.usn.0), (0, 0));
    let preset = r.deck_config(DeckConfigId(1)).unwrap().unwrap();
    assert!(preset.config.fsrs_params_6().is_empty());
    assert_eq!(preset.config.learn_steps, [1.0, 10.0]);
    assert!(collection.peek_undo(UndoScope::Global).unwrap().name().is_none());
    assert!(r.cards_after(None, 1).unwrap().is_empty());
    assert_eq!(
        r.ids_after(meguri_storage::Table::Notetypes, None, 10).unwrap(),
        (1_894_701_600_000..1_894_701_600_006).collect::<Vec<_>>()
    );
}

#[test]
fn failure_never_publishes_partial_file_and_existing_file_stays() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = FixedClock::new(1_894_701_600_000, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    for point in ["storage.bootstrap.in_transaction", "storage.bootstrap.before_publish"] {
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::StorageFull));
        let result = bootstrap::create(&path, &clock, &random);
        faultpoint::disarm(point);
        assert!(result.is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    bootstrap::create(&path, &clock, &random).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bootstrap::create(&path, &clock, &random).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn creation_before_rollover_and_across_dst_uses_the_offset_at_crt() {
    for (start, zone, crt, offset) in [
        ("2030-01-15T03:30:00", "Europe/Paris", 1_894_590_000, -60),
        // D11: 03:30 after the spring transition anchors at yesterday's
        // 04:00 under the current offset; the offset at that instant was +01:00.
        ("2030-03-31T03:30:00", "Europe/Paris", 1_901_066_400, -60),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let clock = ScenarioClock::new(zone, start).unwrap();
        let random = SeededRandom::new(1);
        bootstrap::create(&path, &clock, &random).unwrap();
        let opened = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
        assert_eq!(opened.writer().col().unwrap().created.0, crt, "{start}");
        assert_eq!(
            opened.writer().config_value("creationOffset").unwrap(),
            Some(ConfigValue::from(i64::from(offset)))
        );
        assert_eq!(opened.writer().col().unwrap().schema_modified.0, clock.now_ms());
    }
}

#[test]
fn bootstrap_uses_one_creation_instant_even_if_the_host_clock_moves() {
    use std::sync::atomic::{AtomicI64, Ordering};
    #[derive(Debug)]
    struct MovingClock(AtomicI64);
    impl Clock for MovingClock {
        fn now_ms(&self) -> i64 {
            self.0.fetch_add(2000, Ordering::SeqCst)
        }
        fn utc_offset_at(&self, _: i64) -> UtcOffset {
            UtcOffset::UTC
        }
        fn zone_id(&self) -> String {
            "UTC".into()
        }
    }
    let clock = MovingClock(AtomicI64::new(1_894_701_600_000));
    let random = SeededRandom::new(1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    bootstrap::create(&path, &clock, &random).unwrap();
    let opened = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    assert_eq!(
        opened
            .writer()
            .ids_after(meguri_storage::Table::Notetypes, None, 10)
            .unwrap(),
        (1_894_701_600_000..1_894_701_600_006).collect::<Vec<_>>()
    );
    assert_eq!(
        opened.writer().config_entry("curModel").unwrap().unwrap().entry.mtime.0,
        1_894_701_600
    );
}
