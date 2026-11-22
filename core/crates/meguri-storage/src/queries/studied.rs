//! Today's answers for the Today page (`stats/studied-today` T12): the
//! number and time of `stats/counting-rules` N6's rows after a first id,
//! in one statement bounded by the review log's primary key; and the same
//! for the cards of a Statistics page deck scope (`stats/history-reads`
//! H12).

use meguri_model::ids::DeckId;
use rusqlite::Connection;

use crate::StorageError;

/// N6's rows: every row whose `id` is greater than `?1` (B2: a row stamped
/// exactly at the start is not in) and whose `type` is neither 4 nor 5. Any
/// `ease`, deleted cards' rows and every deck count. SQLite's integer `sum`
/// fails on overflow, so the total never wraps (T12.1).
pub const STUDIED_SQL: &str =
    "SELECT count(*), count(DISTINCT cid), coalesce(sum(time), 0) FROM revlog WHERE id > ?1 AND type NOT IN (4, 5)";

/// [`STUDIED_SQL`] over the rows of the cards whose `did` is one of
/// `decks` deck ids, or whose non-zero `odid` is (`stats/graphs-common`
/// G2.1), bound as `?2` onwards. Each half of the card set has its own
/// index (`ix_cards_sched`, the partial `idx_cards_odid`); the rows come
/// through `ix_revlog_cid` with the rowid bound.
pub fn studied_in_decks_sql(decks: usize) -> String {
    let list = (0..decks.max(1)).map(|i| format!("?{}", i + 2)).collect::<Vec<_>>().join(", ");
    format!(
        "SELECT count(*), count(DISTINCT cid), coalesce(sum(time), 0) FROM revlog \
         WHERE id > ?1 AND type NOT IN (4, 5) AND cid IN (\
           SELECT id FROM cards WHERE did IN ({list}) \
           UNION ALL \
           SELECT id FROM cards WHERE odid != 0 AND odid IN ({list}))"
    )
}

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
    Ok(studied(answers, cards, time_ms))
}

/// The rows after `after` of the cards in `decks` (G2.1); none for no deck.
pub(crate) fn studied_since_in(conn: &Connection, after: i64, decks: &[DeckId]) -> Result<Studied, StorageError> {
    if decks.is_empty() {
        return Ok(Studied::default());
    }
    // Only integers are bound; the text holds placeholders only.
    let mut values = vec![after];
    values.extend(decks.iter().map(|deck| deck.0));
    let mut statement = conn.prepare_cached(&studied_in_decks_sql(decks.len()))?;
    let (answers, cards, time_ms): (i64, i64, i64) = statement.query_row(rusqlite::params_from_iter(values), |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(studied(answers, cards, time_ms))
}

fn studied(answers: i64, cards: i64, time_ms: i64) -> Studied {
    Studied {
        answers: u64::try_from(answers).unwrap_or(0),
        cards: u64::try_from(cards).unwrap_or(0),
        time_ms,
    }
}
