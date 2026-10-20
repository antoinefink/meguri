//! X2.1/C5: automatic registration on the real writer and lazy readers,
//! including caller-supplied initializers, without persistent schema SQL.
mod common;

use meguri_engine::executor::catalog;
use meguri_engine::{Engine, EngineConfig};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_storage::raw::SqlValue;
use meguri_storage::{Initializers, ReaderRole};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

const QUERY: &str = "SELECT meguri_card_f32('{\"s\":5}', 's'), meguri_retrievability('{\"s\":5,\"d\":5,\"lrt\":0}',35,NULL,NULL,0), meguri_relative_retrievability('{}',30,10,30,0), meguri_original_position('{\"pos\":7}'), u82_custom()";

#[test]
fn engine_keeps_its_functions_and_custom_initializers_on_every_connection() {
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let clock = Arc::new(StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC"));
    let calls = Arc::new(AtomicUsize::new(0));
    let mut initializers = Initializers::new();
    let seen = calls.clone();
    initializers.register(move |conn| {
        seen.fetch_add(1, Ordering::SeqCst);
        conn.create_scalar_function(
            "u82_custom",
            0,
            rusqlite::functions::FunctionFlags::SQLITE_UTF8
                | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC
                | rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS,
            |_| Ok(43i64),
        )
    });
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let collection = engine
        .open_raw(&path, common::open_config(&clock).initializers(initializers))
        .unwrap();
    let expected = vec![
        SqlValue::Real(5.0),
        SqlValue::Real(1.0),
        SqlValue::Real(f64::from(-(10.0f32 + 0.001) / 10.0)),
        SqlValue::Integer(7),
        SqlValue::Integer(43),
    ];
    let writer = collection
        .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), |writer| {
            Ok(writer.execute_raw(QUERY, &[])?.rows)
        })
        .wait()
        .unwrap();
    assert_eq!(writer, vec![expected.clone()]);
    for role in [
        ReaderRole::Query,
        ReaderRole::Interactive,
        ReaderRole::Maintenance,
        ReaderRole::Sync,
    ] {
        let reader = collection.reader(role).unwrap();
        assert_eq!(reader.execute_raw(QUERY, &[]).unwrap().rows, vec![expected.clone()]);
        assert_eq!(reader.execute_raw("SELECT name FROM sqlite_master WHERE lower(sql) LIKE '%meguri_card_f32%' OR lower(sql) LIKE '%meguri_retrievability%' OR lower(sql) LIKE '%meguri_relative_retrievability%' OR lower(sql) LIKE '%meguri_original_position%'",&[]).unwrap().rows.len(),0);
    }
    let before = calls.load(Ordering::SeqCst);
    // Search worker 3 opens a connection not used by any earlier read.
    let reader = collection.search_reader(3).unwrap();
    assert_eq!(reader.execute_raw(QUERY, &[]).unwrap().rows, vec![expected]);
    assert!(calls.load(Ordering::SeqCst) > before);
    drop(reader);
    engine.close().unwrap();
}

#[test]
fn default_engine_open_registers_the_extractors_too() {
    let h = common::Harness::open();
    assert_eq!(
        common::query_int(&h.collection, "SELECT meguri_original_position('[7]')"),
        7
    );
}
