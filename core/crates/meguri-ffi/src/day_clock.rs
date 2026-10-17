//! The day clock over FFI (U40, contract C6, `specs/sched/day-clock.md` D8):
//! `check_rollover` on the open collection, and the events the day clock
//! publishes. Swift calls it on foreground, from its timer at `next_day_at`
//! and on a system time, zone or clock change (U51); the core runs no timer
//! (R1854).

use meguri_engine::{DayReport, EventKind};

use crate::callback;
use crate::error::CoreError;
use crate::events::CoreEventKind;
use crate::runtime;

/// What a check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DayClockReport {
    /// The scheduler day number.
    pub today: u32,
    /// The instant the next day starts, epoch seconds. It can equal the
    /// current second (legacy timing, D4): a timer armed at it must wait for
    /// a later instant, at least the next whole second (D8).
    pub next_day_at: i64,
    /// The day before, when this check saw the day change (a `RolledOver`
    /// event follows).
    pub rolled_over_from: Option<u32>,
}

impl From<DayReport> for DayClockReport {
    fn from(report: DayReport) -> DayClockReport {
        DayClockReport {
            today: report.today,
            next_day_at: report.next_day_at,
            rolled_over_from: report.rolled_over_from,
        }
    }
}

/// `checkRollover()`: runs the day query on the open collection (its
/// writes, its unbury, the rollover op when the day changed) and returns
/// today and `next_day_at`. Cheap when nothing changed. `Unavailable` with
/// no collection open, or while an exclusive phase runs: the check then
/// runs by itself once the collection is ready again (D9).
#[uniffi::export]
pub fn check_rollover() -> Result<DayClockReport, CoreError> {
    callback::enter("check_rollover");
    let collection = runtime::engine()?.collection()?;
    Ok(collection.check_rollover()?.into())
}

/// The `CoreEventKind` of a day clock event; `None` for other kinds.
pub(crate) fn core_event_kind(kind: &EventKind) -> Option<CoreEventKind> {
    Some(match kind {
        EventKind::RolledOver { old_day, new_day } => CoreEventKind::RolledOver {
            old_day: *old_day,
            new_day: *new_day,
        },
        EventKind::TimeZoneChanged { zone_id } => CoreEventKind::TimeZoneChanged {
            zone_id: zone_id.clone(),
        },
        EventKind::NextDayAtChanged { next_day_at } => CoreEventKind::NextDayAtChanged {
            next_day_at: *next_day_at,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_clock_events_cross_with_their_values() {
        assert_eq!(
            core_event_kind(&EventKind::RolledOver { old_day: 5, new_day: 6 }),
            Some(CoreEventKind::RolledOver { old_day: 5, new_day: 6 })
        );
        assert_eq!(
            core_event_kind(&EventKind::TimeZoneChanged {
                zone_id: "Africa/Ceuta".to_owned()
            }),
            Some(CoreEventKind::TimeZoneChanged {
                zone_id: "Africa/Ceuta".to_owned()
            })
        );
        assert_eq!(
            core_event_kind(&EventKind::NextDayAtChanged {
                next_day_at: 1_895_194_800
            }),
            Some(CoreEventKind::NextDayAtChanged {
                next_day_at: 1_895_194_800
            })
        );
        assert_eq!(core_event_kind(&EventKind::Available), None);
    }

    #[test]
    fn a_check_with_no_collection_open_is_unavailable() {
        assert!(matches!(check_rollover(), Err(CoreError::Unavailable { .. })));
    }
}
