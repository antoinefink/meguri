//! Provider metadata publication under the caller's current one-way sync slot.

mod common;

use std::cell::Cell;
use std::time::Duration;

use common::{Harness, fresh, open_config};
use meguri_engine::{EngineError, Generation, LongOp, SuspendOutcome};

#[test]
fn own_current_one_way_sync_slot_publishes_once_with_the_open_path() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let calls = Cell::new(0);

    let value = h.engine.with_provider_metadata_publication_for_slot(
        &slot,
        h.engine.generation(),
        |path| {
            calls.set(calls.get() + 1);
            assert_eq!(path, Some(h.path.as_path()));
            Ok(73)
        },
    ).unwrap();

    assert_eq!(calls.get(), 1);
    assert_eq!(value, 73);
}

#[test]
fn old_lane_refuses_a_held_slot_and_publishes_after_release() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let generation = h.engine.generation();
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication(generation, |_| {
        calls.set(calls.get() + 1);
        Ok(29)
    });
    assert!(matches!(result, Err(EngineError::CollectionUnavailable { .. })));
    assert_eq!(calls.get(), 0);

    drop(slot);
    let value = h.engine.with_provider_metadata_publication(generation, |path| {
        calls.set(calls.get() + 1);
        assert_eq!(path, Some(h.path.as_path()));
        Ok(29)
    }).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(value, 29);
}

#[test]
fn foreign_one_way_sync_guard_is_refused_before_publication() {
    let h = Harness::open();
    let _own = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let other = fresh();
    let slot = other.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication_for_slot(
        &slot,
        h.engine.generation(),
        |_| {
            calls.set(calls.get() + 1);
            Ok(())
        },
    );

    assert!(matches!(result, Err(EngineError::CollectionUnavailable { .. })));
    assert_eq!(calls.get(), 0);
}

#[test]
fn restore_guard_is_refused_before_publication() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication_for_slot(
        &slot,
        h.engine.generation(),
        |_| {
            calls.set(calls.get() + 1);
            Ok(())
        },
    );

    assert!(matches!(result, Err(EngineError::CollectionUnavailable { .. })));
    assert_eq!(calls.get(), 0);
}

#[test]
fn stale_generation_is_refused_before_publication() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let current = h.engine.generation();
    let planned = Generation(current.0 + 1);
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication_for_slot(&slot, planned, |_| {
        calls.set(calls.get() + 1);
        Ok(())
    });

    assert_eq!(result, Err(EngineError::StaleGeneration { planned, current }));
    assert_eq!(calls.get(), 0);
}

#[test]
fn retired_guard_is_refused_after_close_and_reopen() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, open_config(&h.clock)).unwrap();
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication_for_slot(
        &slot,
        reopened.generation(),
        |_| {
            calls.set(calls.get() + 1);
            Ok(())
        },
    );

    assert!(matches!(result, Err(EngineError::CollectionUnavailable { .. })));
    assert_eq!(calls.get(), 0);
}

#[test]
fn closed_collection_is_refused_before_publication() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    h.engine.close().unwrap();
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication_for_slot(
        &slot,
        h.engine.generation(),
        |_| {
            calls.set(calls.get() + 1);
            Ok(())
        },
    );

    assert!(matches!(result, Err(EngineError::CollectionUnavailable { .. })));
    assert_eq!(calls.get(), 0);
}

#[test]
fn suspended_engine_is_refused_before_publication() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    assert_eq!(
        h.engine.suspend(Duration::from_secs(2)).unwrap(),
        SuspendOutcome::Suspended,
    );
    let calls = Cell::new(0);

    let result = h.engine.with_provider_metadata_publication_for_slot(
        &slot,
        h.engine.generation(),
        |_| {
            calls.set(calls.get() + 1);
            Ok(())
        },
    );

    assert!(matches!(result, Err(EngineError::CollectionUnavailable { .. })));
    assert_eq!(calls.get(), 0);
    h.engine.resume().unwrap();
}
