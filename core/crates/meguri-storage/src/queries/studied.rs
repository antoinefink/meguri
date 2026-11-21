//! Today's answers for the Today page (`stats/studied-today` T12): the
//! number and time of `stats/counting-rules` N6's rows after a first id,
//! in one statement bounded by the review log's primary key.

use rusqlite::Connection;

use crate::StorageError;

/// N6's rows: every row whose `id` is greater than `?1` (B2: a row stamped
/// exactly at the start is not in) and whose `type` is neither 4 nor 5. Any
/// `ease`, deleted cards' rows and every deck count. SQLite's integer `sum`
/// fails on overflow, so the total never wraps (T12.1).
pub const STUDIED_SQL: &str =
    "SELECT count(*), count(DISTINCT cid), coalesce(sum(time), 0) FROM revlog WHERE id > ?1 AND type NOT IN (4, 5)";

/// The rows after review-log id `after`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Studied {
    /// Their number.
    pub answers: u64,
    /// The number of different cards they answer.
    pub cards: u64,
    /// The sum of their `time`.
    pub time_ms: i64,
}

pub(crate) fn studied_since(conn: &Connection, after: i64) -> Result<Studied, StorageError> {
    let mut statement = conn.prepare_cached(STUDIED_SQL)?;
    let (answers, cards, time_ms): (i64, i64, i64) =
        statement.query_row([after], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    Ok(Studied {
        answers: u64::try_from(answers).unwrap_or(0),
        cards: u64::try_from(cards).unwrap_or(0),
        time_ms,
    })
}
