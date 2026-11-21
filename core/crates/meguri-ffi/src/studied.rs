//! Today's numbers over FFI (`stats/studied-today` T12): one short read,
//! called on the host's query lane.

use crate::{callback, error::CoreError, runtime};

/// T12.1's four numbers and the generation the read saw.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct StudiedTodayNumbers {
    /// Answers in the studied-today window (`stats/counting-rules` N6).
    pub answers: u64,
    /// The different cards they answer.
    pub cards: u64,
    /// The sum of their answer times, in milliseconds.
    pub time_ms: i64,
    /// Answers in the seven scheduler days that end with today.
    pub recent_answers: u64,
    pub recent_time_ms: i64,
    pub generation: u64,
}

impl From<meguri_engine::studied::StudiedTodayNumbers> for StudiedTodayNumbers {
    fn from(read: meguri_engine::studied::StudiedTodayNumbers) -> Self {
        StudiedTodayNumbers {
            answers: read.answers,
            cards: read.cards,
            time_ms: read.time_ms,
            recent_answers: read.recent_answers,
            recent_time_ms: read.recent_time_ms,
            generation: read.generation,
        }
    }
}

/// Today's and the last seven days' answers for the whole collection.
/// Reads only, on the query reader.
#[uniffi::export]
pub fn studied_today_numbers() -> Result<StudiedTodayNumbers, CoreError> {
    callback::enter("studied_today_numbers");
    let host = runtime::host()?;
    let collection = host.collection()?;
    Ok(meguri_engine::studied::studied_today_numbers(&collection)?.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_with_no_collection_open_is_unavailable() {
        assert!(matches!(studied_today_numbers(), Err(CoreError::Unavailable { .. })));
    }
}
