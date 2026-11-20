//! The activity heatmap's day counts (`stats/activity-heatmap` AH2 to AH5):
//! one statement per scope, bounded by the review log's primary key and
//! aggregated per day inside SQLite.

use meguri_model::ids::DeckId;
use rusqlite::Connection;

use crate::StorageError;

/// Days the query covers, `k` = 0 (today) to 370: 52 full weeks and the
/// current one (AH5).
pub const ACTIVITY_DAYS: u32 = 371;

const DAY_SECS: i64 = 86_400;

/// Which rows count (AH4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityScope<'a> {
    /// Every counted row, deleted cards' rows included.
    Collection,
    /// Rows of existing cards whose home deck (`odid` when not 0, else
    /// `did`) is one of these decks: a deck and its descendants.
    HomeDecks(&'a [DeckId]),
}

/// Collection scope. `?1` is `N` (epoch seconds), `?2` the window's first id.
/// The `id` range is a rowid range: the statement never reads rows before it.
pub const COLLECTION_SQL: &str = "SELECT max(0, (?1 - id / 1000) / 86400), count(*) FROM revlog \
     WHERE id >= ?2 AND ease > 0 GROUP BY 1";

/// Deck scope for `decks` deck ids, bound as `?3` onwards. Each half of
/// the card set has its own index: `ix_cards_sched` (`did` first) for cards
/// in their home deck, the partial `idx_cards_odid` for cards in a filtered
/// deck. Their rows come through `ix_revlog_cid` with the rowid bound. The
/// ids are a plain list of parameters, not `json_each`, so SQLite knows how
/// few they are and keeps to the indexes even with ANALYZE statistics.
pub fn deck_sql(decks: usize) -> String {
    let list = (0..decks.max(1)).map(|i| format!("?{}", i + 3)).collect::<Vec<_>>().join(", ");
    format!(
        "SELECT max(0, (?1 - r.id / 1000) / 86400), count(*) FROM revlog r \
         WHERE r.id >= ?2 AND r.ease > 0 AND r.cid IN (\
           SELECT c.id FROM cards c WHERE c.odid = 0 AND c.did IN ({list}) \
           UNION ALL \
           SELECT c.id FROM cards c WHERE c.odid != 0 AND c.odid IN ({list})) \
         GROUP BY 1"
    )
}

/// The first review-log id of the window for `next_day_at` (AH5): the rows
/// with `id >= ` it are those with `k <= 370`, and every later row.
pub fn window_start(next_day_at: i64) -> i64 {
    next_day_at
        .saturating_sub(i64::from(ACTIVITY_DAYS) * DAY_SECS)
        .saturating_add(1)
        .saturating_mul(1000)
}

/// The counts of days `k` = 0 to 370 before `next_day_at`, zeros included
/// (AH3, AH6).
pub(crate) fn day_counts(
    conn: &Connection,
    scope: ActivityScope<'_>,
    next_day_at: i64,
) -> Result<Vec<u32>, StorageError> {
    let start = window_start(next_day_at);
    let mut counts = vec![0u32; ACTIVITY_DAYS as usize];
    let mut add = |k: i64, n: i64| {
        // The bound keeps k at or under 370; a row stamped in the far future
        // has k 0 (max), so nothing falls outside the vector.
        if let Some(slot) = usize::try_from(k).ok().and_then(|k| counts.get_mut(k)) {
            *slot = slot.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
        }
    };
    match scope {
        ActivityScope::Collection => {
            let mut statement = conn.prepare_cached(COLLECTION_SQL)?;
            let mut rows = statement.query((next_day_at, start))?;
            while let Some(row) = rows.next()? {
                add(row.get(0)?, row.get(1)?);
            }
        }
        ActivityScope::HomeDecks(decks) => {
            if decks.is_empty() {
                return Ok(counts);
            }
            // Only integers are bound; the text holds placeholders only.
            let mut values = vec![next_day_at, start];
            values.extend(decks.iter().map(|d| d.0));
            let mut statement = conn.prepare_cached(&deck_sql(decks.len()))?;
            let mut rows = statement.query(rusqlite::params_from_iter(values))?;
            while let Some(row) = rows.next()? {
                add(row.get(0)?, row.get(1)?);
            }
        }
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_starts_one_second_after_371_days_before_the_boundary() {
        let n = 1_791_856_800;
        assert_eq!(window_start(n), (n - 371 * 86_400 + 1) * 1000);
        // k of the window's first row is 370; one millisecond earlier is outside.
        let k = |id: i64| (n - id / 1000) / 86_400;
        assert_eq!(k(window_start(n)), 370);
        assert_eq!(k(window_start(n) - 1), 371);
        assert_eq!(window_start(i64::MIN), i64::MIN);
    }
}
