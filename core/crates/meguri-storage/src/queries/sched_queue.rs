//! U41 read-only inputs. All collection SQL stays here. Callers hold one
//! short snapshot for metadata, the four gather phases and grouped facts.
use crate::{
    StorageError,
    rows::{self, Record, Table},
};
use meguri_model::{card::Card, ids::DeckId};
use rusqlite::Connection;

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Intraday,
    Interday,
    Review,
    New,
}

pub(crate) fn candidates(
    conn: &Connection,
    decks: &[DeckId],
    phase: Phase,
    order: u8,
    today: u32,
    now: i64,
    boundary: i64,
) -> Result<Vec<Card>, StorageError> {
    candidates_overlay(conn, decks, phase, order, today, now, boundary, &[])
}

/// A read-only CTE substitutes pending card postimages before the same U41
/// predicates and ordering. It creates no table and writes no collection data.
#[allow(clippy::too_many_arguments)]
pub(crate) fn candidates_overlay(
    conn: &Connection,
    decks: &[DeckId],
    phase: Phase,
    order: u8,
    today: u32,
    now: i64,
    boundary: i64,
    overrides: &[Card],
) -> Result<Vec<Card>, StorageError> {
    let ids = serde_json::to_string(&decks.iter().map(|d| d.0).collect::<Vec<_>>()).expect("integer ids");
    let predicate = match phase {
        Phase::Intraday => "c.queue IN (1,4) AND c.due<=?4",
        Phase::Interday => "c.queue=3 AND c.due<=?2",
        Phase::Review => "c.queue=2 AND c.due<=?2",
        Phase::New => "c.queue=0",
    };
    let keys = match phase {
        Phase::Intraday | Phase::New => "c.did,c.id",
        _ => match order {
            1 => "c.due,d.key,fnvhash(c.id,c.mod)",
            2 => "d.key,c.due,fnvhash(c.id,c.mod)",
            3 => "c.ivl,fnvhash(c.id,c.mod)",
            4 => "c.ivl DESC,fnvhash(c.id,c.mod)",
            5 => "meguri_card_f32(c.data,'d') DESC,fnvhash(c.id,c.mod)",
            6 => "meguri_card_f32(c.data,'d'),fnvhash(c.id,c.mod)",
            7 => "meguri_retrievability(c.data,c.due,c.ivl,?2,?3),fnvhash(c.id,c.mod)",
            8 => "fnvhash(c.id,c.mod)",
            9 => "c.nid,c.ord,fnvhash(c.id,c.mod)",
            10 => "c.nid DESC,c.ord,fnvhash(c.id,c.mod)",
            11 => "meguri_retrievability(c.data,c.due,c.ivl,?2,?3) DESC,fnvhash(c.id,c.mod)",
            12 => "meguri_relative_retrievability(c.data,c.due,c.ivl,?2,?3),fnvhash(c.id,c.mod)",
            _ => "c.due,fnvhash(c.id,c.mod)",
        },
    };
    let columns = Table::Cards
        .columns()
        .iter()
        .map(|c| format!("c.\"{c}\""))
        .collect::<Vec<_>>()
        .join(",");
    let (prefix, source) = if overrides.is_empty() {
        (String::new(), "cards")
    } else {
        let projection = Table::Cards
            .columns()
            .iter()
            .enumerate()
            .map(|(i, column)| format!("json_extract(o.value,'$[{i}]') AS \"{column}\""))
            .collect::<Vec<_>>()
            .join(",");
        (
            format!(
                "WITH pending_cards AS (SELECT * FROM main.cards WHERE id NOT IN (SELECT json_extract(value,'$[0]') FROM json_each(?5)) UNION ALL SELECT {projection} FROM json_each(?5) o) "
            ),
            "pending_cards",
        )
    };
    let sql = format!(
        "{prefix}SELECT {columns} FROM {source} c JOIN json_each(?1) d ON c.did=d.value WHERE {predicate} AND ?2 IS NOT NULL AND ?3 IS NOT NULL AND ?4 IS NOT NULL AND ?5 IS NOT NULL ORDER BY {keys}"
    );
    let override_values = overrides
        .iter()
        .map(|card| {
            card.to_cells()
                .into_iter()
                .map(|cell| match cell {
                    rows::Cell::Integer(v) => serde_json::Value::from(v),
                    rows::Cell::Text(v) => serde_json::Value::String(String::from_utf8_lossy(&v).into_owned()),
                    _ => unreachable!("Card::to_cells writes integer and text cells"),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let overrides_json = serde_json::to_string(&override_values).expect("card row values");
    let mut statement = conn.prepare_cached(&sql)?;
    let found = statement.query_map((ids, i64::from(today), now, boundary, overrides_json), |row| {
        rows::row_image(row, Table::Cards)
    })?;
    found
        .map(|row| Card::from_cells(&row?).map_err(StorageError::from))
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub new: u32,
    pub review: u32,
    pub interday: u32,
    pub intraday: u32,
}
#[derive(Clone, Debug)]
pub struct DeckAggregate {
    pub deck: DeckId,
    pub cards: u64,
    pub today: Totals,
    pub tomorrow: Totals,
    pub next_change_at: Option<i64>,
    pub learning_remaining: u32,
    pub compatibility_next_due: Option<i64>,
    pub learning_remaining_today: u32,
    pub learning_later_today: u32,
    pub next_learning_due: Option<i64>,
    pub sibling_buried: u32,
    pub user_buried: u32,
}

/// Exactly one grouped card aggregation, including tomorrow's read-only
/// unbury projection. No deck loop issues SQL and no card images are loaded.
pub(crate) fn aggregate(
    conn: &Connection,
    today: u32,
    now: i64,
    boundary: i64,
    ahead: u32,
) -> Result<Vec<DeckAggregate>, StorageError> {
    let sums = |q: &str, day: &str, instant: &str| {
        format!(
            "sum({q}=0),sum({q}=2 AND due<={day}),sum({q}=3 AND due<={day}),sum(({q}=1 AND due<{instant}+?4) OR ({q}=4 AND due<={instant}+?4))"
        )
    };
    let current = sums("queue", "?1", "?2");
    let tomorrow = sums("projected_queue", "?5", "?3");
    let sql = format!("SELECT did,count(*),{current},{tomorrow},
        min(CASE WHEN queue=1 AND due-?4+1>?2 THEN due-?4+1 WHEN queue=4 AND due-?4>?2 THEN due-?4 END),
        sum(queue=1),min(CASE WHEN queue=1 THEN due END),
        sum(queue IN (1,4) AND due<=?3),sum(queue IN (1,4) AND due>?2 AND due<=?3),
        min(CASE WHEN queue IN (1,4) AND due>?2 AND due<=?3 THEN due END),
        sum(queue=-2),sum(queue=-3)
        FROM (SELECT *,CASE WHEN queue IN (-2,-3) THEN CASE type WHEN 0 THEN 0 WHEN 2 THEN 2 ELSE CASE WHEN due>1000000000 THEN 1 ELSE 3 END END ELSE queue END AS projected_queue FROM cards)
        GROUP BY did");
    let mut statement = conn.prepare_cached(&sql)?;
    Ok(statement
        .query_map(
            (
                i64::from(today),
                now,
                boundary,
                i64::from(ahead),
                i64::from(today.wrapping_add(1)),
            ),
            |r| {
                let totals = |i: usize| -> rusqlite::Result<Totals> {
                    Ok(Totals {
                        new: r.get(i)?,
                        review: r.get(i + 1)?,
                        interday: r.get(i + 2)?,
                        intraday: r.get(i + 3)?,
                    })
                };
                let cards: i64 = r.get(1)?;
                let cards = u64::try_from(cards).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, cards))?;
                Ok(DeckAggregate {
                    deck: DeckId(r.get(0)?),
                    cards,
                    today: totals(2)?,
                    tomorrow: totals(6)?,
                    next_change_at: r.get(10)?,
                    learning_remaining: r.get(11)?,
                    compatibility_next_due: r.get(12)?,
                    learning_remaining_today: r.get(13)?,
                    learning_later_today: r.get(14)?,
                    next_learning_due: r.get(15)?,
                    sibling_buried: r.get(16)?,
                    user_buried: r.get(17)?,
                })
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn empty(conn: &Connection, decks: &[DeckId]) -> Result<bool, StorageError> {
    let ids = serde_json::to_string(&decks.iter().map(|d| d.0).collect::<Vec<_>>()).expect("integer ids");
    Ok(!conn.query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE did IN (SELECT value FROM json_each(?1)) OR odid IN (SELECT value FROM json_each(?1)))",[ids],|r|r.get::<_,bool>(0))?)
}
