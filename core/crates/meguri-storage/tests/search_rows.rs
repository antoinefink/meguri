//! `ReadRows::visit_id_rows`, the statement runner of Browse searches
//! (`browser/result-sets` H3): rows in statement order, one or two integer
//! columns, an early stop, read-only statements only, and a cancel test
//! that interrupts a running statement promptly and is gone afterwards.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{empty_collection, open};
use meguri_storage::{Cell, ReadRows, ReaderRole, StorageError};

const COUNT: &str = "WITH RECURSIVE r(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM r WHERE x < ?1) SELECT x, -x FROM r";

fn visit(reader: &impl ReadRows, sql: &str, params: &[Cell], cancel: &dyn Fn() -> bool) -> Result<Vec<Vec<i64>>, StorageError> {
    let mut rows = Vec::new();
    reader.visit_id_rows(sql, params, cancel, &mut |row| {
        rows.push(row.to_vec());
        true
    })?;
    Ok(rows)
}

#[test]
fn rows_come_in_statement_order_with_one_or_two_columns_and_can_stop_early() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let reader = collection.open_reader(ReaderRole::Search).unwrap();
    let rows = visit(&reader, COUNT, &[Cell::Integer(3)], &|| false).unwrap();
    assert_eq!(rows, vec![vec![1, -1], vec![2, -2], vec![3, -3]]);
    let rows = visit(&reader, "SELECT 7 UNION ALL SELECT 5", &[], &|| false).unwrap();
    assert_eq!(rows, vec![vec![7], vec![5]]);
    let mut seen = 0;
    reader
        .visit_id_rows(COUNT, &[Cell::Integer(1000)], &|| false, &mut |_| {
            seen += 1;
            seen < 4
        })
        .unwrap();
    assert_eq!(seen, 4);
    for refused in ["DELETE FROM cards", "SELECT 1, 2, 3"] {
        assert!(
            matches!(visit(&reader, refused, &[], &|| false), Err(StorageError::Mode { .. })),
            "{refused}"
        );
    }
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn a_cancel_interrupts_a_running_statement_and_leaves_the_next_one_alone() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let reader = collection.open_reader(ReaderRole::Search).unwrap();
    // Set before the call: nothing runs.
    let err = visit(&reader, COUNT, &[Cell::Integer(1)], &|| true).unwrap_err();
    assert!(matches!(err, StorageError::Sqlite { code: 9, .. }), "{err:?}");
    // Set from another thread while the statement counts to 10^12, which
    // would take hours: the call returning at all is the interrupt.
    let stop = Arc::new(AtomicBool::new(false));
    let setter = {
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            stop.store(true, Ordering::SeqCst);
        })
    };
    let long = "WITH RECURSIVE r(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM r WHERE x < 1000000000000) SELECT max(x) FROM r";
    let err = visit(&reader, long, &[], &|| stop.load(Ordering::SeqCst)).unwrap_err();
    setter.join().unwrap();
    assert!(matches!(err, StorageError::Sqlite { code: 9, .. }), "{err:?}");
    // The handler went with the statement: the next one runs whole.
    assert_eq!(visit(&reader, COUNT, &[Cell::Integer(2)], &|| false).unwrap().len(), 2);
    assert_eq!(reader.row_count(meguri_storage::Table::Cards).unwrap(), 0);
    drop(reader);
    collection.close().unwrap();
}
