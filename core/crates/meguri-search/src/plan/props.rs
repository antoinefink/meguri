//! `prop:` and `has-cd:` (`specs/search/properties.md`), with `prop:rated`
//! and `prop:resched` from `history.md` H8.

use crate::ast::{Comparison, Float, Property};
use crate::context::{ContextReads, ContextValue};
use crate::error::{ErrorKind, SearchFailure};

use super::history;
use super::sql::{Param, Params, TermEnv, TermSql};

/// X1, P4: the card's home due when it has one.
const DUE: &str = "(CASE WHEN c.odue != 0 THEN c.odue ELSE c.due END)";

fn column(sql: String) -> TermSql {
    TermSql {
        sql,
        slow: false,
        reads: ContextReads::NONE,
    }
}

fn per_row(sql: String, reads: ContextReads) -> TermSql {
    TermSql { sql, slow: true, reads }
}

/// P6: L(y), the shortest decimal of the 32-bit operand, written into the
/// statement for SQLite to read. An operand that is not finite is P14's
/// `search_failed`; the plan fills in the term's text.
fn literal(y: f32) -> Result<String, ErrorKind> {
    if y.is_finite() {
        Ok(format!("({y})"))
    } else {
        Err(ErrorKind::SearchFailed(SearchFailure::NumberOutOfRange { term: String::new() }))
    }
}

/// P3: x × 1000 in 32-bit floats, to an unsigned 32-bit integer by
/// truncation (NaN and negatives 0, overflow the largest value).
fn ease(x: f32) -> u32 {
    (x * 1000.0) as u32
}

pub(super) fn property(
    property: &Property,
    comparison: Comparison,
    env: &TermEnv<'_>,
    params: &mut Params,
) -> Result<TermSql, ErrorKind> {
    // P1: SQLite's comparison.
    let op = comparison.symbol();
    let ctx = env.ctx;
    Ok(match property {
        // P2.
        Property::Interval(n) => column(format!("c.ivl {op} {n}")),
        Property::Reps(n) => column(format!("c.reps {op} {n}")),
        Property::Lapses(n) => column(format!("c.lapses {op} {n}")),
        // P3.
        Property::Ease(Float(x)) => column(format!("c.factor {op} {}", ease(*x))),
        // P4: day numbers wrap in 32 bits; learning dues are whole days from
        // the next boundary, truncated toward zero, by SQLite's arithmetic
        // (a 64-bit overflow becomes a real there, as P4 asks).
        Property::Due(n) => {
            let target = n.wrapping_add(ctx.today as i32);
            let next_day_at = ctx.next_day_at;
            TermSql {
                sql: format!(
                    "(c.queue IN (2, 3) AND {DUE} {op} ({target})) \
                     OR (c.queue IN (1, 4) AND (({DUE} - ({next_day_at})) / 86400) {op} ({n}))"
                ),
                slow: false,
                reads: ContextReads::NONE
                    .with(ContextValue::Today)
                    .with(ContextValue::NextDayAt),
            }
        }
        // P5: new by type, compared on the home due.
        Property::Position(n) => column(format!("c.type = 0 AND {DUE} {op} {n}")),
        // P7.
        Property::Stability(Float(x)) => per_row(
            format!("meguri_card_f32(c.data, 's') {op} {}", literal(*x)?),
            ContextReads::NONE,
        ),
        Property::Difficulty(Float(x)) => {
            let mapped = *x * 9.0 + 1.0;
            per_row(
                format!("meguri_card_f32(c.data, 'd') {op} {}", literal(mapped)?),
                ContextReads::NONE,
            )
        }
        // P8: false, not unknown, for a new card.
        Property::Retrievability(Float(x)) => {
            let value = literal(*x)?;
            let (today, now) = (ctx.today, ctx.now);
            per_row(
                format!("c.type != 0 AND meguri_retrievability(c.data, {DUE}, c.ivl, {today}, ({now})) {op} {value}"),
                ContextReads::NONE.with(ContextValue::Today).with(ContextValue::Now),
            )
        }
        // P9.
        Property::CustomNumber { key, value } => {
            let value = literal(value.0)?;
            let key = params.bind(Param::Text(key.clone()));
            per_row(
                format!("CAST(meguri_custom_data(c.data, {key}) AS REAL) {op} {value}"),
                ContextReads::NONE,
            )
        }
        Property::CustomString { key, value } => {
            let key = params.bind(Param::Text(key.clone()));
            let value = params.bind(Param::Text(value.clone()));
            per_row(
                format!("meguri_custom_data(c.data, {key}) {op} {value}"),
                ContextReads::NONE,
            )
        }
        // H8.
        Property::Rated { days, button } => history::rated_on_day(*days, *button, comparison, ctx),
        Property::Rescheduled(days) => history::rescheduled_on_day(*days, comparison, ctx),
    })
}

/// P9: `has-cd:`, never unknown.
pub(super) fn has_custom_data(key: &str, params: &mut Params) -> TermSql {
    let key = params.bind(Param::Text(key.to_owned()));
    per_row(
        format!("meguri_custom_data(c.data, {key}) IS NOT NULL"),
        ContextReads::NONE,
    )
}

#[cfg(test)]
mod tests {
    use super::{ease, literal};

    #[test]
    fn ease_operands_truncate_in_32_bits() {
        assert_eq!(ease(2.5), 2500);
        assert_eq!(ease(1.3), 1300);
        assert_eq!(ease(1.2999), 1299);
        assert_eq!(ease(f32::INFINITY), u32::MAX);
        assert_eq!(ease(f32::NAN), 0);
        assert_eq!(ease(-1.0), 0);
    }

    #[test]
    fn literals_are_the_shortest_decimal_of_the_float() {
        for (y, text) in [
            (21.0f32, "(21)"),
            (2.5, "(2.5)"),
            (0.1, "(0.1)"),
            (0.000_000_1, "(0.0000001)"),
            (1e30, "(1000000000000000000000000000000)"),
            (-3.0, "(-3)"),
            (-0.0, "(-0)"),
            (0.3 * 9.0 + 1.0, "(3.7)"),
        ] {
            assert_eq!(literal(y).unwrap(), text, "{y}");
        }
        assert!(literal(f32::INFINITY).is_err());
        assert!(literal(f32::NAN).is_err());
    }
}
