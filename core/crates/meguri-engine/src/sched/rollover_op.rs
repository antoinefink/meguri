//! One day query (`sched/day-clock` D5): the cached day when it is still
//! valid (D6), otherwise a fresh computation with its writes of `rollover`
//! and `localOffset`, then the unbury check (D7). It runs on a `Raw` write
//! scope: the day clock's `Rollover` ops, and the conformance runner's
//! sessions, which reach the collection through its writer.

use meguri_model::clock::UtcOffset;
use meguri_sched::timing::{self, DayInputs, SchedDay};
use meguri_storage::{ConfigValue, ReadRows, StorageError, WriteScope};

/// The queues of buried cards: by the scheduler and by the user (D7).
const BURIED: [i64; 2] = [-2, -3];

/// What a cached day was computed with (D6): any change drops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DayKey {
    crt: i64,
    creation_offset: Option<i32>,
    rollover_hour: u8,
    offset: UtcOffset,
}

/// The cached result of the last fresh computation (D6).
#[derive(Clone, Debug, Default)]
pub struct DayCache {
    cached: Option<(DayKey, SchedDay)>,
    // Automatic empty checks do not stamp synced config. Remember the input
    // marker as well as the day, so a real imported marker change is rechecked.
    automatic_unbury: Option<(u32, u32)>,
}

impl DayCache {
    /// Drops the cached result: the next day query computes afresh (an open,
    /// a raw write, a change of the timing keys).
    pub fn clear(&mut self) {
        self.cached = None;
        self.automatic_unbury = None;
    }

    /// A committed marker change invalidates the empty-check basis even when
    /// the replacement reads as the same effective day under D2.
    pub(super) fn forget_unbury_check(&mut self) {
        self.automatic_unbury = None;
    }

    /// The cached day, when it was computed under `offset` and `now` is
    /// before its boundary. The rest of its key (`crt`, the creation offset,
    /// the rollover hour) is the caller's to know unchanged.
    pub fn valid_at(&self, now: i64, offset: UtcOffset) -> Option<SchedDay> {
        let (key, day) = self.cached.as_ref()?;
        (key.offset == offset && now < day.next_day_at).then_some(*day)
    }
}

/// What a day query found and did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayQuery {
    pub day: SchedDay,
    /// It computed afresh (D5 step 2) rather than reuse the cached day.
    pub computed: bool,
    /// The cards the unbury gave a queue back (D7).
    pub unburied: usize,
}

/// A timing key's value as D2 reads it: the integer it holds, `None` when
/// it is missing or holds anything else.
fn stored(scope: &WriteScope<'_>, key: &str) -> Result<Option<i128>, StorageError> {
    Ok(match scope.config_value(key)? {
        Some(ConfigValue::Integer(value)) => Some(value),
        _ => None,
    })
}

/// D5 and D7 at `now` (epoch seconds) under `offset`. `scope` must be a
/// `Raw` scope: the writes stamp the config rows as local changes, leave
/// every card's `mod` and `usn`, and never move `col.mod`.
pub fn day_query(
    scope: &mut WriteScope<'_>,
    cache: &mut DayCache,
    now: i64,
    offset: UtcOffset,
) -> Result<DayQuery, StorageError> {
    query(scope, cache, now, offset, false)
}

/// Native lifecycle checks avoid synced bookkeeping when no card needs restoration.
pub(crate) fn automatic_day_query(
    scope: &mut WriteScope<'_>,
    cache: &mut DayCache,
    now: i64,
    offset: UtcOffset,
) -> Result<DayQuery, StorageError> {
    query(scope, cache, now, offset, true)
}

fn query(
    scope: &mut WriteScope<'_>,
    cache: &mut DayCache,
    now: i64,
    offset: UtcOffset,
    automatic: bool,
) -> Result<DayQuery, StorageError> {
    let crt = scope.col()?.created.0;
    let rollover = timing::rollover_hour(stored(scope, "rollover")?);
    let key = DayKey {
        crt,
        creation_offset: timing::creation_offset(stored(scope, "creationOffset")?),
        rollover_hour: rollover.unwrap_or(timing::DEFAULT_ROLLOVER_HOUR),
        offset,
    };
    let cached = match &cache.cached {
        Some((cached_key, day)) if *cached_key == key && now < day.next_day_at => Some(*day),
        _ => None,
    };
    let (day, computed) = match cached {
        Some(day) => (day, false),
        None => {
            cache.automatic_unbury = None;
            if rollover.is_none() && !automatic {
                scope.set_config_keeping_col_mod(
                    "rollover",
                    &ConfigValue::from(i64::from(timing::DEFAULT_ROLLOVER_HOUR)),
                )?;
            }
            let minutes = timing::minutes_west(offset);
            if timing::local_offset(stored(scope, "localOffset")?) != minutes {
                scope.set_config_keeping_col_mod("localOffset", &ConfigValue::from(i64::from(minutes)))?;
            }
            let day = timing::sched_day(&DayInputs {
                crt,
                creation_offset: key.creation_offset,
                offset,
                rollover_hour: key.rollover_hour,
                now,
            });
            cache.cached = Some((key, day));
            (day, true)
        }
    };
    let mut unburied = 0;
    let last_unburied = timing::last_unburied(stored(scope, "lastUnburied")?);
    if timing::needs_unbury(last_unburied, day.today)
        && (!automatic || cache.automatic_unbury != Some((day.today, last_unburied)))
    {
        for card in scope.cards_in_queues(&BURIED)? {
            if let Some(queue) = timing::unburied_queue(card.card_type, card.due)
                && scope.set_card_queue(card.id, queue)?.changed()
            {
                unburied += 1;
            }
        }
        if !automatic || unburied > 0 {
            scope.set_config_keeping_col_mod("lastUnburied", &ConfigValue::from(u64::from(day.today)))?;
        }
        if automatic {
            cache.automatic_unbury = Some((day.today, if unburied > 0 { day.today } else { last_unburied }));
        }
    }
    Ok(DayQuery {
        day,
        computed,
        unburied,
    })
}
