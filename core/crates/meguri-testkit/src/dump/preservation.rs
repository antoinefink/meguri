//! normalized-dump.md N4, N6, N7. Callers hold their own connection or snapshot.
use std::collections::BTreeSet;

use meguri_storage::{
    StorageError,
    raw::{RawRows, SqlValue},
};
use serde_json::{Map, Value};

use crate::diff::{Difference, pointer, shown};
use crate::exec::{ExecError, db_dump_table_raw};
use crate::scenario::DUMP_TABLES;

/// Every stored column, with no masking. Use one snapshot for a concurrent collection.
pub fn raw_dump(mut query: impl FnMut(&str) -> Result<RawRows, StorageError>) -> Result<Value, ExecError> {
    let mut dump = Map::new();
    for table in DUMP_TABLES {
        let rows = query(&format!("SELECT * FROM {table}")).map_err(core)?;
        dump.insert(table.to_owned(), db_dump_table_raw(table, &rows)?);
    }
    Ok(dump.into())
}

/// N6.1: the twelve indicators of changes a future sync would carry.
pub const PENDING_QUERY: &str = "SELECT (SELECT count() FROM notes WHERE usn = -1), (SELECT count() FROM cards WHERE usn = -1), (SELECT count() FROM revlog WHERE usn = -1), (SELECT count() FROM decks WHERE usn = -1), (SELECT count() FROM deck_config WHERE usn = -1), (SELECT count() FROM notetypes WHERE usn = -1), (SELECT count() FROM templates WHERE usn = -1), (SELECT count() FROM tags WHERE usn = -1), (SELECT count() FROM config WHERE usn = -1), (SELECT count() FROM graves), (SELECT mod > ls FROM col), (SELECT scm > ls FROM col)";

pub fn pending(mut query: impl FnMut(&str) -> Result<RawRows, StorageError>) -> Result<[i64; 12], ExecError> {
    let rows = query(PENDING_QUERY).map_err(core)?;
    let [row] = rows.rows.as_slice() else {
        return Err(core("pending query must return one row"));
    };
    let mut counts = [0; 12];
    if row.len() != counts.len() {
        return Err(core("pending query must return twelve integers"));
    }
    for (out, value) in counts.iter_mut().zip(row) {
        let SqlValue::Integer(value) = value else {
            return Err(core("pending query must return twelve integers"));
        };
        *out = *value;
    }
    Ok(counts)
}

/// Exact changes at table, row or column depth; a BLOB digest is one column value.
pub fn differences(before: &Value, after: &Value) -> Vec<Difference> {
    fn walk(before: Option<&Value>, after: Option<&Value>, path: &mut Vec<String>, out: &mut Vec<Difference>) {
        if before == after {
            return;
        }
        if path.len() < 3 {
            if let (Some(Value::Object(b)), Some(Value::Object(a))) = (before, after) {
                let keys: BTreeSet<_> = b.keys().chain(a.keys()).collect();
                for key in keys {
                    path.push(key.clone());
                    walk(b.get(key), a.get(key), path, out);
                    path.pop();
                }
                return;
            }
        }
        out.push(Difference {
            pointer: pointer::render(path),
            expected: before.cloned(),
            actual: after.cloned(),
        });
    }
    let mut out = Vec::new();
    walk(Some(before), Some(after), &mut Vec::new(), &mut out);
    out.sort_by(|a, b| a.pointer.cmp(&b.pointer));
    out
}

/// Refuse every change outside the named O3 pointer patterns; report the first twenty.
pub fn assert_changes_allowed(before: &Value, after: &Value, allowed: &[&str]) -> Result<(), String> {
    let patterns: Vec<_> = allowed
        .iter()
        .filter(|p| p.starts_with('/'))
        .map(|p| pointer::parse(p))
        .collect();
    let unexpected: Vec<_> = differences(before, after)
        .into_iter()
        .filter(|d| {
            let path = pointer::parse(&d.pointer);
            !patterns
                .iter()
                .any(|p| p.len() == path.len() && p.iter().zip(&path).all(|(a, b)| a == "*" || a == b))
        })
        .collect();
    if unexpected.is_empty() {
        return Ok(());
    }
    let mut report = format!("{} unexpected preservation change(s)\n", unexpected.len());
    for d in unexpected.iter().take(20) {
        report.push_str(&format!(
            "{}: {} -> {}\n",
            d.pointer,
            shown(&d.expected),
            shown(&d.actual)
        ));
    }
    Err(report)
}

fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
