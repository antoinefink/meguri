//! The history terms (`specs/search/history.md`): `added:`, `edited:`,
//! `introduced:`, `rated:`, `resched:`, `prop:rated` and `prop:resched`.
//! Days count back from next-day-at in 86,400-second steps (H2); the review
//! log is read through subqueries on `revlog`, whose `id` is its key, so each
//! window is a range of the table (H10). None is slow.

use crate::ast::Comparison;
use crate::context::{ContextReads, ContextValue, SearchContext};

use super::sql::TermSql;

const DAY: i128 = 86_400;

fn reading_next_day_at(sql: String) -> TermSql {
    TermSql {
        sql,
        slow: false,
        reads: ContextReads::NONE.with(ContextValue::NextDayAt),
    }
}

/// An instant beyond the 64-bit range becomes the nearest end. Every
/// comparison then gives what the true instant would, except for a stored
/// value equal to that end, which only a next-day-at near the ends of the
/// range can meet.
fn clamp(value: i128) -> i64 {
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// H2: next-day-at + 86,400 × `days`, in seconds and in milliseconds.
fn boundary_secs(ctx: &SearchContext, days: i128) -> i128 {
    i128::from(ctx.next_day_at) + DAY * days
}

fn boundary_ms(ctx: &SearchContext, days: i128) -> i64 {
    clamp(boundary_secs(ctx, days) * 1000)
}

/// H1: which entries a term counts.
#[derive(Clone, Copy)]
enum Entries {
    /// `ease` > 0, or one button.
    Answers(Option<u8>),
    /// `ease` 0.
    Manual,
}

impl Entries {
    fn sql(self) -> String {
        match self {
            Entries::Answers(None) => "ease > 0".to_owned(),
            Entries::Answers(Some(button)) => format!("ease = {button}"),
            Entries::Manual => "ease = 0".to_owned(),
        }
    }
}

/// The cards with an entry of `entries` whose id satisfies `window`.
fn with_entry(entries: Entries, window: &str) -> TermSql {
    reading_next_day_at(format!(
        "c.id IN (SELECT cid FROM revlog WHERE {window} AND {})",
        entries.sql()
    ))
}

/// H3.
pub(super) fn added(days: u32, ctx: &SearchContext) -> TermSql {
    reading_next_day_at(format!("c.id > {}", boundary_ms(ctx, -i128::from(days))))
}

/// H4.
pub(super) fn edited(days: u32, ctx: &SearchContext) -> TermSql {
    reading_next_day_at(format!("n.mod > {}", clamp(boundary_secs(ctx, -i128::from(days)))))
}

/// H5: an answer at or after the cutoff, later than now included.
pub(super) fn rated(days: u32, button: Option<u8>, ctx: &SearchContext) -> TermSql {
    let cutoff = boundary_ms(ctx, -i128::from(days));
    with_entry(Entries::Answers(button), &format!("id >= {cutoff}"))
}

/// H6.
pub(super) fn rescheduled(days: u32, ctx: &SearchContext) -> TermSql {
    let cutoff = boundary_ms(ctx, -i128::from(days));
    with_entry(Entries::Manual, &format!("id >= {cutoff}"))
}

/// H7: the first entry with `ease` other than 0 lies after the cutoff: one
/// such entry after it and none at or before it.
pub(super) fn introduced(days: u32, ctx: &SearchContext) -> TermSql {
    let cutoff = boundary_ms(ctx, -i128::from(days));
    reading_next_day_at(format!(
        "c.id IN (SELECT r.cid FROM revlog r WHERE r.id > {cutoff} AND r.ease != 0 \
         AND NOT EXISTS (SELECT 1 FROM revlog e WHERE e.cid = r.cid AND e.ease != 0 AND e.id <= {cutoff}))"
    ))
}

/// H8's table: the entry ids of day `day` (0 today, -1 yesterday) that the
/// comparison selects.
fn day_window(day: i32, comparison: Comparison, ctx: &SearchContext) -> String {
    let start = boundary_ms(ctx, i128::from(day) - 1);
    let end = boundary_ms(ctx, i128::from(day));
    match comparison {
        Comparison::Equal => format!("id >= {start} AND id < {end}"),
        Comparison::NotEqual => format!("(id < {start} OR id >= {end})"),
        Comparison::Less => format!("id < {start}"),
        Comparison::LessOrEqual => format!("id < {end}"),
        Comparison::Greater => format!("id >= {end}"),
        Comparison::GreaterOrEqual => format!("id >= {start}"),
    }
}

/// H8: `prop:rated`.
pub(super) fn rated_on_day(day: i32, button: Option<u8>, comparison: Comparison, ctx: &SearchContext) -> TermSql {
    with_entry(Entries::Answers(button), &day_window(day, comparison, ctx))
}

/// H8: `prop:resched`.
pub(super) fn rescheduled_on_day(day: i32, comparison: Comparison, ctx: &SearchContext) -> TermSql {
    with_entry(Entries::Manual, &day_window(day, comparison, ctx))
}
