//! The day clock of an open collection (`sched/day-clock` D6, D8 to D10,
//! contract C6): `checkRollover()`, `today` and `nextDayAt` on the
//! collection handle.
//!
//! Every day query runs on the writer, in an op of source `Rollover` written
//! in the `Raw` mode (D10): its writes stamp the config rows and leave
//! `col.mod`, cards' `mod` and `usn`, and the undo history alone. When the
//! day differs from the one last reported, that op is the rollover op (D9):
//! it also clears the history with `ClearedByRollover`. The events follow
//! the op's commit, from a commit hook on the writer, so a check that nobody
//! waits for (one parked while the collection was unavailable, one a change
//! set started) still reports.
//!
//! A check that finds nothing to do skips the writer: the cached day is
//! valid, the offset and zone are the ones last seen, and no change set has
//! touched the timing keys or `col` since the last check. The day clock
//! runs no timer (R1854): the app arms one at `nextDayAt` (U51).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use meguri_model::changes::{ChangeSet, OpSource};
use meguri_model::clock::Clock;
use meguri_storage::StampMode;

use crate::changes::stream::{ChangeStream, EventKind};
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::ExecShared;
use crate::executor::op::{CommitNotice, OpContext, OpSpec};
use crate::executor::writer;
use crate::undo::reasons::ClearCause;

use super::rollover_op::{DayCache, automatic_day_query};

/// The op every day query runs in (D10).
const OP_NAME: &str = "rollover";

/// What a check found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DayReport {
    /// The scheduler day number.
    pub today: u32,
    /// The instant the next day starts, epoch seconds. In legacy timing it
    /// equals `now` for the whole first second of a day (D4): a timer armed
    /// at it must wait for a later instant (D8).
    pub next_day_at: i64,
    /// The day before, when this check saw the day change and published
    /// `RolledOver`.
    pub rolled_over_from: Option<u32>,
}

/// The values the day clock last reported (D8).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Reported {
    day: u32,
    next_day_at: i64,
    checked_at: i64,
    offset: meguri_model::clock::UtcOffset,
    zone: String,
}

#[derive(Default)]
struct State {
    cache: DayCache,
    /// Counts what may change the next day query's result or writes: change
    /// sets touching the timing keys or `col`, raw writes, opens and
    /// generation changes.
    changes: u64,
    /// `changes` as the last committed day query saw it.
    checked: u64,
    /// `None` until the first check after an open or a generation change,
    /// which records and reports nothing (D8).
    reported: Option<Reported>,
    /// A check refused while the collection was unavailable waits parked
    /// (D9); one is enough.
    parked: bool,
}

/// One open collection's day clock.
pub struct DayClock {
    clock: Arc<dyn Clock>,
    stream: ChangeStream,
    state: Mutex<State>,
}

impl std::fmt::Debug for DayClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.lock();
        f.debug_struct("DayClock")
            .field("reported", &state.reported)
            .field("dirty", &(state.changes != state.checked))
            .finish_non_exhaustive()
    }
}

/// The instant and offset a day query runs at (D1).
struct Now {
    secs: i64,
    offset: meguri_model::clock::UtcOffset,
    zone: String,
}

impl DayClock {
    pub(crate) fn new(clock: Arc<dyn Clock>, stream: ChangeStream) -> DayClock {
        DayClock {
            clock,
            stream,
            state: Mutex::new(State::default()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> Now {
        let now_ms = self.clock.now_ms();
        Now {
            secs: now_ms.div_euclid(1000),
            offset: self.clock.utc_offset_at(now_ms),
            zone: self.clock.zone_id(),
        }
    }

    /// Forgets the cached day and what was reported: an open or a
    /// generation change (D8), after which the first check records and
    /// reports nothing.
    pub(crate) fn reset(&self) {
        let mut state = self.lock();
        let changes = state.changes + 1;
        *state = State {
            changes,
            checked: state.checked,
            ..State::default()
        };
    }

    /// Drops the cached day (D6): something changed the collection without
    /// a change set (a raw write, another process).
    pub(crate) fn drop_cached(&self) {
        let mut state = self.lock();
        state.cache.clear();
        state.changes += 1;
    }

    /// The cached report when a check would change nothing.
    fn unchanged(&self, now: &Now) -> Option<DayReport> {
        let state = self.lock();
        if state.changes != state.checked || state.parked {
            return None;
        }
        let reported = state.reported.as_ref()?;
        let day = state.cache.valid_at(now.secs, now.offset)?;
        (reported.zone == now.zone && reported.day == day.today && reported.next_day_at == day.next_day_at).then_some(
            DayReport {
                today: day.today,
                next_day_at: day.next_day_at,
                rolled_over_from: None,
            },
        )
    }

    /// Queue watches consult the maintained clock without scheduling an op.
    pub(super) fn queue_day_is_current(&self, today: u32, boundary: i64) -> bool {
        let now = self.now();
        self.queue_report_is_current(
            DayReport {
                today,
                next_day_at: boundary,
                rolled_over_from: None,
            },
            now.secs,
            now.offset,
            &now.zone,
        )
    }

    /// D8 report validation after a queue prelude; D4 may report a boundary equal to now.
    pub(super) fn queue_report_is_current(
        &self,
        day: DayReport,
        now: i64,
        offset: meguri_model::clock::UtcOffset,
        zone: &str,
    ) -> bool {
        let state = self.lock();
        state.changes == state.checked
            && !state.parked
            && state.reported.as_ref().is_some_and(|reported| {
                reported.day == day.today
                    && reported.next_day_at == day.next_day_at
                    && reported.zone == zone
                    && (state
                        .cache
                        .valid_at(now, offset)
                        .is_some_and(|cached| cached.today == day.today && cached.next_day_at == day.next_day_at)
                        || (reported.checked_at == now && reported.next_day_at == now && reported.offset == offset))
            })
    }

    /// The writer's side of a check, inside a `Rollover` op (D8 steps 1 to
    /// 5; D9, D10).
    pub(crate) fn run(self: &Arc<DayClock>, ctx: &mut OpContext<'_, '_>) -> Result<DayReport, EngineError> {
        let now = self.now();
        let (mut cache, seen, before) = {
            let mut state = self.lock();
            state.parked = false;
            (state.cache.clone(), state.changes, state.reported.clone())
        };
        let query = automatic_day_query(ctx.scope(), &mut cache, now.secs, now.offset)?;
        let rolled_over_from = before
            .as_ref()
            .filter(|reported| reported.day != query.day.today)
            .map(|reported| reported.day);
        if rolled_over_from.is_some() {
            // D9: the day changed. The session rebuilds its queues on the
            // event (U43); the per-deck counters reset when read (U42).
            ctx.clear_undo_on_commit(ClearCause::Rollover);
        }
        let reported = Reported {
            day: query.day.today,
            next_day_at: query.day.next_day_at,
            // Queue reads may use this fresh D4 report during this second;
            // it does not extend the reusable day cache past its boundary.
            checked_at: now.secs,
            offset: now.offset,
            zone: now.zone,
        };
        let this = Arc::clone(self);
        ctx.on_commit(move |notice| this.committed(cache, seen, reported, notice));
        Ok(DayReport {
            today: query.day.today,
            next_day_at: query.day.next_day_at,
            rolled_over_from,
        })
    }

    /// After the op committed: keeps its result and publishes what changed
    /// since the last report (D8 steps 2 to 5).
    fn committed(&self, cache: DayCache, seen: u64, now: Reported, notice: CommitNotice) {
        let before = {
            let mut state = self.lock();
            // A change noted meanwhile keeps the clock dirty and its cache
            // dropped.
            if state.changes == seen {
                state.cache = cache;
            }
            state.checked = state.checked.max(seen);
            state.reported.replace(now.clone())
        };
        let Some(before) = before else {
            return;
        };
        let cause = notice.published.then_some(notice.seq);
        let publish = |kind| {
            self.stream.publish(notice.generation, cause, kind);
        };
        if before.zone != now.zone {
            publish(EventKind::TimeZoneChanged { zone_id: now.zone });
        }
        if before.day != now.day {
            publish(EventKind::RolledOver {
                old_day: before.day,
                new_day: now.day,
            });
        }
        if before.next_day_at != now.next_day_at {
            publish(EventKind::NextDayAtChanged {
                next_day_at: now.next_day_at,
            });
        }
    }

    /// Notes a committed change set, on the writer. True when the day clock
    /// must compute afresh and report at once: `rollover`, `creationOffset`
    /// or `col` changed (D6, D8). A change of `lastUnburied` only makes the
    /// next check run its unbury check on the writer (D7).
    pub(crate) fn note_commit(&self, changes: &ChangeSet) -> bool {
        if changes.source == OpSource::Rollover {
            // The day clock's own writes, which its cache reflects.
            return false;
        }
        let touches = |name: &str| changes.config_keys.iter().any(|k| k.key == name);
        let timing = touches("rollover") || touches("creationOffset");
        if !(timing || changes.collection_row || touches("lastUnburied")) {
            return false;
        }
        let mut state = self.lock();
        state.changes += 1;
        if timing {
            state.cache.clear();
        } else if touches("lastUnburied") {
            state.cache.forget_unbury_check();
        }
        timing || changes.collection_row
    }

    /// Submits a check nobody waits for: it waits parked while the
    /// collection is unavailable and runs on whatever generation is open
    /// then; its events come from its commit hook.
    pub(crate) fn schedule_check(self: &Arc<DayClock>, shared: &ExecShared) {
        let this = Arc::clone(self);
        let spec = op_spec().park_when_unavailable().run_on_any_generation();
        drop(shared.submit_op(spec, shared.generation(), None, move |ctx| this.run(ctx)));
    }

    /// D9: a check refused while the collection is unavailable runs once it
    /// is `Ready` again.
    fn park(self: &Arc<DayClock>, shared: &ExecShared) {
        {
            let mut state = self.lock();
            if state.parked {
                return;
            }
            state.parked = true;
        }
        self.schedule_check(shared);
    }
}

/// The op every check runs in (D10).
pub(crate) fn op_spec() -> OpSpec {
    OpSpec::named(OP_NAME, OpSource::Rollover).stamp(StampMode::Raw)
}

impl Collection {
    /// `checkRollover()` (C6, D8): runs the day query with its writes and
    /// unbury, and publishes what changed. Idempotent and cheap: a second
    /// call in the same state writes and publishes nothing, and usually
    /// skips the writer. Never call it from inside an op: it waits for the
    /// writer (`InvalidArgument` there). While the collection is unavailable
    /// it fails with `CollectionUnavailable`, and the check runs by itself
    /// once the collection is `Ready` again (D9).
    pub fn check_rollover(&self) -> Result<DayReport, EngineError> {
        if writer::on_writer_thread() {
            return Err(EngineError::InvalidArgument {
                message: "the day query never runs on the writer thread: run check_rollover before the op and pass \
                          the day in (sched/day-clock D8)"
                    .to_owned(),
            });
        }
        self.check_current()?;
        let exec = &self.core.exec;
        let day_clock = &exec.day_clock;
        if let Some(report) = day_clock.unchanged(&day_clock.now()) {
            return Ok(report);
        }
        let this = Arc::clone(day_clock);
        match self.submit(op_spec(), move |ctx| this.run(ctx)).wait() {
            Ok(result) => Ok(result.value),
            Err(error @ EngineError::CollectionUnavailable { .. }) => {
                day_clock.park(exec);
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    /// The scheduler day number now (C6): the value of a day query run at
    /// the time of the read (D8).
    pub fn today(&self) -> Result<u32, EngineError> {
        Ok(self.check_rollover()?.today)
    }

    /// The instant the next day starts, epoch seconds (C6, D8).
    pub fn next_day_at(&self) -> Result<i64, EngineError> {
        Ok(self.check_rollover()?.next_day_at)
    }

    /// Drops the cached day (D6) after a change that brought no change set:
    /// a raw write, or another process changing the file (U32).
    pub fn drop_cached_day(&self) -> Result<(), EngineError> {
        self.check_current()?;
        self.core.exec.day_clock.drop_cached();
        Ok(())
    }

    /// Restarts the day clock and runs its first check (D8): the open's
    /// last step, before anything reads counts. After a generation change
    /// the writer's replace task does the same itself, before the readers
    /// open (L8).
    pub(crate) fn restart_day_clock(&self) -> Result<DayReport, EngineError> {
        self.core.exec.day_clock.reset();
        self.check_rollover()
    }
}
