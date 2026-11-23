//! The strip's Studied Today for a page scope (`stats/history-reads` H12):
//! `stats/studied-today` T12.1's `answers`, `cards` and `time_ms` over the
//! scope's rows, read from the snapshot by the review log's id range with
//! the stored `type` and `time` (`stats/counting-rules` N6), not from G9's
//! columns, so All Decks equals T12.1 on every row.

use meguri_model::ids::DeckId;
use meguri_storage::{ReadRows, StorageError};

use crate::graphs::scope::{self, Scope};

/// Part `studied_today`. Member order: `answers`, `cards`, `time_ms`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StudiedTodayPart {
    /// N6's rows in the studied-today window (`stats/day-buckets` B2: `id`
    /// greater than `S`), `type` neither 4 nor 5, of the scope.
    pub answers: u64,
    /// The different cards those rows answer.
    pub cards: u64,
    /// The sum of their stored `time`; a sum past 64 bits is an error.
    pub time_ms: i64,
}

/// H12 for `scope` (Collection or Deck; G2.1's decks, deleted cards' rows
/// in the Collection scope only) on `rows`, with B2's `S` from `N`.
pub(crate) fn read(rows: &impl ReadRows, scope: &Scope, next_day_at: i64) -> Result<StudiedTodayPart, StorageError> {
    let after = meguri_engine::studied::window_start(next_day_at);
    let studied = match scope {
        Scope::Deck(deck) => {
            // The scope resolved on this snapshot, so the deck is there.
            let decks: Vec<DeckId> = scope::deck_set(&rows.deck_rows()?, *deck)
                .map(|set| set.into_iter().map(DeckId).collect())
                .unwrap_or_default();
            rows.studied_since_in(after, &decks)?
        }
        // A Search scope never reaches here (H1: `unsupported_scope`).
        Scope::Collection | Scope::Search(_) => rows.studied_since(after)?,
    };
    Ok(StudiedTodayPart {
        answers: studied.answers,
        cards: studied.cards,
        time_ms: studied.time_ms,
    })
}
