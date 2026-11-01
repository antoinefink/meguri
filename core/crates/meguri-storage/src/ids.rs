//! Ids of new rows (`specs/storage/ids-usn-mtime.md` I1 to I3, R772, R1469,
//! R1470).
//!
//! A new row takes the current instant in milliseconds when no row of its
//! table has that id, else the table's largest id plus one (I2), never more
//! than one day ahead of now (I3). Rows that arrive with an id keep it (I4):
//! they never come here.

use rusqlite::{Connection, OptionalExtension};

use crate::error::StorageError;
use crate::rows::Table;

/// One day in milliseconds: the furthest ahead of now an id may lie (I3).
pub const MAX_AHEAD_MS: i64 = 86_400_000;

/// The id a new row of `table` takes at `now_ms`, given the rows the table
/// holds now. Each new row is allocated on its own, after the rows created
/// before it in the same operation are written (I2).
///
/// # Errors
///
/// [`StorageError::IdsExhausted`] when every id from `now_ms + 1` to
/// `now_ms + MAX_AHEAD_MS` is in use and I2's value lies beyond the cap.
pub fn allocate(conn: &Connection, table: Table, now_ms: i64) -> Result<i64, StorageError> {
    debug_assert!(table.allocates_ids(), "{table} has no allocated ids");
    let name = table.name();
    if !taken(conn, name, now_ms)? {
        return Ok(now_ms);
    }
    let largest: i64 = conn
        .prepare_cached(&format!("SELECT max(id) FROM {name}"))?
        .query_row([], |row| row.get(0))?;
    let cap = now_ms.saturating_add(MAX_AHEAD_MS);
    if let Some(next) = largest.checked_add(1)
        && next <= cap
    {
        return Ok(next);
    }
    // I3: the smallest id above now that no row uses. Now itself is taken;
    // if now + 1 is taken too, the answer follows the first taken id above
    // now whose successor is free.
    let first_above = now_ms.saturating_add(1);
    let free = if taken(conn, name, first_above)? {
        conn.prepare_cached(&format!(
            "SELECT a.id + 1 FROM {name} AS a WHERE a.id > ?1 AND a.id < ?2 \
             AND NOT EXISTS (SELECT 1 FROM {name} AS b WHERE b.id = a.id + 1) ORDER BY a.id LIMIT 1"
        ))?
        .query_row((now_ms, cap), |row| row.get::<_, i64>(0))
        .optional()?
    } else {
        Some(first_above)
    };
    free.filter(|id| *id <= cap)
        .ok_or(StorageError::IdsExhausted { table: name })
}

/// I5 physical half of allocation. Scan ordered integer headers only; return
/// at the first gap, never a clock candidate or the collection maximum.
pub(crate) fn rekey_candidate(
    conn: &Connection,
    table: Table,
    after: i64,
    now_ms: i64,
    mut cancelled: impl FnMut() -> bool,
) -> Result<i64, StorageError> {
    let name = table.name();
    if !table.allocates_ids() {
        return Err(StorageError::Mode {
            detail: "rekey requires an allocated table".into(),
        });
    }
    let exhausted = || StorageError::IdsExhausted { table: name };
    let cap = now_ms.checked_add(MAX_AHEAD_MS).ok_or_else(exhausted)?;
    let mut candidate = after.checked_add(1).filter(|id| *id <= cap).ok_or_else(exhausted)?;
    // A page bounds each SQLite cursor's lifetime and memory. The binding
    // owner checks cancellation between successive candidate requests.
    loop {
        if cancelled() {
            return Err(StorageError::Mode {
                detail: "CloudKit rekey cancelled".into(),
            });
        }
        let mut q = conn.prepare(&format!(
            "SELECT id FROM {name} WHERE id>=?1 AND id<=?2 ORDER BY id LIMIT 250"
        ))?;
        let ids = q
            .query_map((candidate, cap), |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if ids.is_empty() {
            return Ok(candidate);
        }
        for id in ids {
            if id > candidate {
                return Ok(candidate);
            }
            candidate = candidate.checked_add(1).filter(|id| *id <= cap).ok_or_else(exhausted)?;
        }
    }
}

fn taken(conn: &Connection, table: &str, id: i64) -> Result<bool, StorageError> {
    Ok(conn
        .prepare_cached(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE id = ?1)"))?
        .query_row([id], |row| row.get::<_, i64>(0))?
        != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_894_701_600_000;
    const DAY: i64 = MAX_AHEAD_MS;

    fn table(ids: &[i64]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE notes (id integer PRIMARY KEY)")
            .unwrap();
        for id in ids {
            conn.execute("INSERT INTO notes (id) VALUES (?1)", [id]).unwrap();
        }
        conn
    }

    /// The tables of I2 and I3, row by row.
    #[test]
    fn allocation_follows_the_spec_tables() {
        let cases: [(&[i64], i64, i64); 9] = [
            (&[], T, T),
            (&[T], T, T + 1),
            (&[T, T + 1], T, T + 2),
            (&[T + 3 * DAY], T + DAY, T + DAY),
            (&[T, T + 86_399_000], T, T + 86_399_001),
            // I3's table.
            (&[T + DAY, T + 3 * DAY], T + DAY, T + DAY + 1),
            (&[T, T + DAY], T, T + 1),
            // Exactly now + 1 day is allowed.
            (&[T, T + DAY - 1], T, T + DAY),
            // Gaps above now: the smallest free id, not the first gap after
            // the largest.
            (&[T, T + 1, T + 3, T + 2 * DAY], T, T + 2),
        ];
        for (ids, now, expected) in cases {
            assert_eq!(
                allocate(&table(ids), Table::Notes, now).unwrap(),
                expected,
                "{ids:?} at {now}"
            );
        }
    }

    #[test]
    fn a_full_day_ahead_fails() {
        let conn = table(&[T, T + 2 * DAY]);
        conn.execute_batch(&format!(
            "WITH RECURSIVE n(i) AS (SELECT {} UNION ALL SELECT i + 1 FROM n WHERE i < {}) \
             INSERT INTO notes (id) SELECT i FROM n",
            T + 1,
            T + DAY
        ))
        .unwrap();
        assert_eq!(
            allocate(&conn, Table::Notes, T),
            Err(StorageError::IdsExhausted { table: "notes" })
        );
    }

    #[test]
    fn the_largest_possible_id_falls_back_to_the_cap_rule() {
        assert_eq!(allocate(&table(&[T, i64::MAX]), Table::Notes, T).unwrap(), T + 1);
    }
}
