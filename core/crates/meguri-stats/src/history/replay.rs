//! One card's replay (`stats/history-reads` H2.2, H2.3): the days it spends
//! in each of the five states, and the day of its first review, on the
//! component's absolute day grid (H14.1).

use meguri_model::card::{CardQueue, CardType};
use meguri_model::revlog::RevlogKind;

use crate::graphs::rows::{BadColumn, CardRead, RowRead, StatsCard, StatsRow};

/// Seconds in a day of `stats/day-buckets` B3.
pub(crate) const DAY: i64 = 86_400;

/// The absolute day grid fixed by the phase of `N` (H14.1): day `k` ends
/// at the whole second `phase + k × 86,400`, so a day of B3 relative to an
/// `N` of that phase is `k − N ÷ 86,400`. A rollover moves nothing on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grid {
    phase: i64,
}

impl Grid {
    /// The grid an `N` of this phase reads.
    pub(crate) fn of(next_day_at: i64) -> Grid {
        Grid {
            phase: next_day_at.rem_euclid(DAY),
        }
    }

    pub(crate) fn from_phase(phase: i64) -> Grid {
        Grid {
            phase: phase.rem_euclid(DAY),
        }
    }

    pub(crate) fn phase(self) -> i64 {
        self.phase
    }

    /// The last whole second of `day` (H1's day end).
    pub(crate) fn end(self, day: i64) -> i64 {
        self.phase + day * DAY
    }

    /// A boundary of the grid after every row a collection holds (the year
    /// 2298): `sched/fsrs-items` I4's days counted back from it differ as
    /// the grid's days do, as they would from any `N` after the rows.
    pub(crate) fn reference(self) -> i64 {
        self.end(120_000)
    }

    /// The day that holds whole second `t`: the one whose end is the first
    /// boundary at or after it (H1's day end).
    fn day_of_second(self, t: i64) -> i64 {
        -(self.phase - t).div_euclid(DAY)
    }

    /// A review-log row's day: B3's `t = floor(id / 1,000)`.
    pub(crate) fn row_day(self, id: i64) -> i64 {
        self.day_of_second(id.div_euclid(1000))
    }

    /// A card's creation day: `stats/graphs-cards` C10's second, `id ÷
    /// 1,000` truncated toward zero.
    pub(crate) fn card_day(self, id: i64) -> i64 {
        self.day_of_second(id / 1000)
    }

    /// The day index relative to `next_day_at` (of this grid's phase), with
    /// every later day folded onto day 0 (H1), saturated to 32 bits.
    pub(crate) fn relative(self, day: i64, next_day_at: i64) -> i32 {
        let today = next_day_at.div_euclid(DAY);
        let relative = day.saturating_sub(today).min(0);
        i32::try_from(relative).unwrap_or(i32::MIN)
    }
}

/// The five states of H2, in the order of the part's counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum State {
    New = 0,
    Learning = 1,
    Relearning = 2,
    Young = 3,
    Mature = 4,
}

impl State {
    pub(crate) fn index(self) -> usize {
        self as usize
    }

    pub(crate) fn from_index(index: i64) -> Option<State> {
        Some(match index {
            0 => State::New,
            1 => State::Learning,
            2 => State::Relearning,
            3 => State::Young,
            4 => State::Mature,
            _ => return None,
        })
    }

    /// The H2.2 table: the state after `row`, from the state before it.
    fn after(self, row: &StatsRow) -> State {
        if row.kind == RevlogKind::Manual && row.factor == 0 {
            // A Reset marker.
            return State::New;
        }
        if is_cram(row) {
            return self;
        }
        if row.ivl >= 1 {
            return by_interval(row.ivl);
        }
        if row.ease >= 1 {
            return match self {
                State::New | State::Learning => State::Learning,
                _ => State::Relearning,
            };
        }
        self
    }

    /// `stats/graphs-cards` C4.1's type bucket of a card as it is now.
    fn current(card: &StatsCard) -> State {
        match card.card_type {
            CardType::New => State::New,
            CardType::Learning => State::Learning,
            CardType::Relearning => State::Relearning,
            CardType::Review => by_interval(i64::from(card.ivl)),
        }
    }
}

/// Young below 21 days, mature from 21 (C4.1, V1).
fn by_interval(ivl: impl Into<i64>) -> State {
    if ivl.into() < 21 { State::Young } else { State::Mature }
}

/// `sched/fsrs-items` I2: a filtered-deck review that did not reschedule.
fn is_cram(row: &StatsRow) -> bool {
    row.kind == RevlogKind::Filtered && row.factor == 0
}

/// I2's graded row: a button, and not a cram row (H2.3).
fn is_graded(row: &StatsRow) -> bool {
    row.ease >= 1 && !is_cram(row)
}

/// A card's deck key (H1): `did` and `odid` as stored now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Key {
    pub did: i64,
    pub odid: i64,
}

/// What the component keeps for one card (H14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CardRecord {
    pub id: i64,
    pub key: Key,
    pub suspended: bool,
    /// The state the H2.2 table left the card in after its newest row.
    pub table_state: Option<State>,
    pub newest_row: Option<i64>,
    /// The day of its first review (H2.3).
    pub first_review: Option<i64>,
}

/// One card's part of the series.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Replayed {
    pub record: CardRecord,
    /// From each day on, the card is in that state; days ascend and two
    /// neighbours differ in state. The first is the day it enters (H2.2).
    pub segments: Vec<(i64, State)>,
}

/// A card or a row of it that `model/row-records` R10 calls unreadable
/// (`stats/graphs-common` G4): the part fails for a scope holding the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BadCard {
    pub card: i64,
    pub did: Option<i64>,
    pub odid: Option<i64>,
    /// An unreadable card row, else its unreadable review-log row with the
    /// smallest id.
    pub row: Option<i64>,
    pub column: BadColumn,
}

/// What a card adds to the component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Contribution {
    Card(Replayed),
    Bad(BadCard),
}

/// A card as read, with its review-log rows of `id` ≥ 0 that read its id
/// as their `cid`, in any order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CardInput {
    pub card: CardRead,
    pub rows: Vec<RowRead>,
}

/// What of a card, as it is now, the series reads besides its rows: its
/// key, whether it is suspended and its type bucket, or why it cannot be
/// read. A card change that keeps it changes nothing in the component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Card { key: Key, suspended: bool, current: State },
    Bad { did: Option<i64>, odid: Option<i64>, column: BadColumn },
}

pub(crate) fn shape(card: &CardRead) -> Shape {
    match card {
        CardRead::Card(card) => Shape::Card {
            key: Key {
                did: card.did,
                odid: card.odid,
            },
            suspended: card.queue == CardQueue::Suspended,
            current: State::current(card),
        },
        CardRead::Bad(bad) => Shape::Bad {
            did: bad.did,
            odid: bad.odid,
            column: bad.column,
        },
    }
}

/// A readable card with its readable rows sorted by `id`, or why it is
/// unreadable (G4).
pub(crate) fn readable(input: &CardInput) -> Result<(StatsCard, Vec<StatsRow>), BadCard> {
    let card = match input.card {
        CardRead::Card(card) => card,
        CardRead::Bad(bad) => {
            return Err(BadCard {
                card: bad.id,
                did: bad.did,
                odid: bad.odid,
                row: None,
                column: bad.column,
            });
        }
    };
    let mut rows: Vec<StatsRow> = Vec::with_capacity(input.rows.len());
    let mut first_bad: Option<(i64, BadColumn)> = None;
    for row in &input.rows {
        match row {
            RowRead::Row { row, .. } => rows.push(*row),
            RowRead::Bad(bad) => {
                if first_bad.is_none_or(|(id, _)| bad.id < id) {
                    first_bad = Some((bad.id, bad.column));
                }
            }
        }
    }
    if let Some((row, column)) = first_bad {
        return Err(BadCard {
            card: card.id,
            did: Some(card.did),
            odid: Some(card.odid),
            row: Some(row),
            column,
        });
    }
    rows.sort_by_key(|row| row.id);
    Ok((card, rows))
}

/// H2.2 and H2.3 for a readable card and its rows sorted by `id`.
pub(crate) fn replay_rows(grid: Grid, card: &StatsCard, rows: &[StatsRow]) -> Replayed {
    let current = State::current(card);
    let created = grid.card_day(card.id);
    let key = Key {
        did: card.did,
        odid: card.odid,
    };
    let mut record = CardRecord {
        id: card.id,
        key,
        suspended: card.queue == CardQueue::Suspended,
        table_state: None,
        newest_row: None,
        first_review: None,
    };
    let Some(newest) = rows.last() else {
        // No row: in its current state from its creation day.
        return Replayed {
            record,
            segments: vec![(created, current)],
        };
    };
    let newest_day = grid.row_day(newest.id);
    let first_day = grid.row_day(rows[0].id);
    let mut segments: Vec<(i64, State)> = Vec::new();
    let mut push = |day: i64, state: State| {
        if segments.last().is_none_or(|(_, last)| *last != state) {
            segments.push((day, state));
        }
    };
    if created < first_day {
        push(created, State::New);
    }
    let mut state = State::New;
    let mut at = 0;
    while at < rows.len() {
        let day = grid.row_day(rows[at].id);
        // Two rows on one day: the later decides.
        while at < rows.len() && grid.row_day(rows[at].id) == day {
            state = state.after(&rows[at]);
            at += 1;
        }
        // From the newest row's day on, the card is in its current state.
        push(day, if day == newest_day { current } else { state });
    }
    record.table_state = Some(state);
    record.newest_row = Some(newest.id);
    record.first_review = rows.iter().find(|row| is_graded(row)).map(|row| grid.row_day(row.id));
    Replayed { record, segments }
}
