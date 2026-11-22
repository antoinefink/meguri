//! The request context (`stats/graphs-common` G1), its token (G8.3) and the
//! graph preferences (G7).

use meguri_storage::{ReadRows, StorageError};

use super::scope::{History, Scope};

/// The values one refresh is computed for (G1). The result carries them, so
/// the presentation layer needs no clock of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    /// `N`: the next day boundary, epoch seconds (`sched/day-clock` D8).
    pub next_day_at: i64,
    /// The scheduler's day number (`days_elapsed`).
    pub today: u32,
    /// `R`: the rollover hour, 0 to 23.
    pub rollover_hour: u8,
    /// Now, epoch milliseconds.
    pub now_ms: i64,
    /// The zone's UTC offset at now, seconds east.
    pub utc_offset_secs: i32,
    /// The device zone's IANA id, read with now.
    pub zone: String,
    /// The snapshot's collection generation and seq (`engine/op-model` O6).
    pub generation: u64,
    pub seq: u64,
}

/// The opaque token of a refresh's first Data result (G8.3): a second
/// request passing it computes with these values on a snapshot of the same
/// generation and seq, or gets Context changed. Nothing is held open
/// between the two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextToken {
    pub(crate) refresh: u64,
    pub(crate) scope: Scope,
    pub(crate) history: History,
    pub(crate) context: Context,
}

impl ContextToken {
    pub fn refresh(&self) -> u64 {
        self.refresh
    }
}

/// The three synced keys that change what graphs show, and `fsrs`, as read
/// in the request's snapshot (G7). Requests never write them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    /// `firstDayOfWeek` (`editor/config-keys` K5): 0 Sunday, 1 Monday, 5
    /// Friday, 6 Saturday; any other value reads Sunday.
    pub first_day_of_week: u8,
    /// `cardCountsSeparateInactive`: a JSON boolean, else `true`.
    pub card_counts_separate_inactive: bool,
    /// `futureDueShowBacklog`: a JSON boolean, else `true`.
    pub future_due_show_backlog: bool,
    /// `fsrs`: a JSON boolean, else `false`.
    pub fsrs: bool,
}

impl Preferences {
    pub(crate) fn read(rows: &impl ReadRows) -> Result<Preferences, StorageError> {
        let flag = |key: &str, missing: bool| -> Result<bool, StorageError> {
            Ok(rows.config_value(key)?.and_then(|value| value.as_bool()).unwrap_or(missing))
        };
        let first_day_of_week = match rows.config_value("firstDayOfWeek")?.and_then(|value| value.as_i64()) {
            Some(day @ (0 | 1 | 5 | 6)) => day as u8,
            _ => 0,
        };
        Ok(Preferences {
            first_day_of_week,
            card_counts_separate_inactive: flag("cardCountsSeparateInactive", true)?,
            future_due_show_backlog: flag("futureDueShowBacklog", true)?,
            fsrs: flag("fsrs", false)?,
        })
    }
}
