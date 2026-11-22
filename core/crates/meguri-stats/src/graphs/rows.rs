//! Reading rows for the graphs (`stats/graphs-common` G4): cards and
//! review-log rows by their typed values, with the graph engine's
//! tolerances, and the rows `model/row-records` R10 calls unreadable, listed
//! apart with their key and column. Values are never repaired.

use meguri_model::card::{CardQueue, CardType};
use meguri_model::card_data::{CardData, MemoryState};
use meguri_model::revlog::RevlogKind;
use meguri_storage::Cell;

/// One readable card, as the graphs read it (G4, G9.1; `stats/graphs-cards`
/// C1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StatsCard {
    pub id: i64,
    pub nid: i64,
    pub did: i64,
    pub odid: i64,
    pub card_type: CardType,
    pub queue: CardQueue,
    /// 0 when the stored value is not an INTEGER in the `i32` range (R4).
    pub due: i32,
    /// As `due`.
    pub odue: i32,
    pub ivl: u32,
    /// `s` and `d`, present only when both are (`model/card-data-json` C3).
    pub memory: Option<MemoryState>,
    pub decay: Option<f32>,
    /// The card data's `lrt` only (G9.1).
    pub lrt: Option<i64>,
}

/// One readable review-log row, as the graphs read it (G4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatsRow {
    pub id: i64,
    pub ease: u8,
    /// A REAL value truncated toward zero.
    pub ivl: i32,
    /// As `ivl`.
    pub last_ivl: i32,
    pub factor: u32,
    /// 0 when the stored value is not an INTEGER from 0 to 4,294,967,295.
    pub time: u32,
    /// An INTEGER whose low 8 bits are 0 to 5 is that kind; anything else
    /// is Learn.
    pub kind: RevlogKind,
}

/// A column whose stored value makes a row unreadable (R10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum BadColumn {
    CardNid,
    CardDid,
    CardOdid,
    CardType,
    CardQueue,
    CardIvl,
    RevlogCid,
    RevlogUsn,
    RevlogEase,
    RevlogIvl,
    RevlogLastIvl,
    RevlogFactor,
}

impl BadColumn {
    pub const ALL: [BadColumn; 12] = [
        BadColumn::CardNid,
        BadColumn::CardDid,
        BadColumn::CardOdid,
        BadColumn::CardType,
        BadColumn::CardQueue,
        BadColumn::CardIvl,
        BadColumn::RevlogCid,
        BadColumn::RevlogUsn,
        BadColumn::RevlogEase,
        BadColumn::RevlogIvl,
        BadColumn::RevlogLastIvl,
        BadColumn::RevlogFactor,
    ];

    /// The table's name (R10).
    pub fn table(self) -> &'static str {
        match self {
            BadColumn::CardNid
            | BadColumn::CardDid
            | BadColumn::CardOdid
            | BadColumn::CardType
            | BadColumn::CardQueue
            | BadColumn::CardIvl => "cards",
            _ => "revlog",
        }
    }

    /// The column's stored name (R10).
    pub fn column(self) -> &'static str {
        match self {
            BadColumn::CardNid => "nid",
            BadColumn::CardDid => "did",
            BadColumn::CardOdid => "odid",
            BadColumn::CardType => "type",
            BadColumn::CardQueue => "queue",
            BadColumn::CardIvl | BadColumn::RevlogIvl => "ivl",
            BadColumn::RevlogCid => "cid",
            BadColumn::RevlogUsn => "usn",
            BadColumn::RevlogEase => "ease",
            BadColumn::RevlogLastIvl => "lastIvl",
            BadColumn::RevlogFactor => "factor",
        }
    }

    pub(crate) fn code(self) -> u8 {
        BadColumn::ALL.iter().position(|c| *c == self).map_or(0, |at| at as u8)
    }

    pub(crate) fn from_code(code: u8) -> Option<BadColumn> {
        BadColumn::ALL.get(usize::from(code)).copied()
    }
}

/// An unreadable card, with what a Deck scope needs to place it: its `did`
/// and `odid` when they read as integers (G2.1, G4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadCard {
    pub id: i64,
    pub did: Option<i64>,
    pub odid: Option<i64>,
    pub column: BadColumn,
}

/// An unreadable review-log row, with its `cid` when it reads as an
/// integer: the card whose scope it belongs to (G4, G9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadRow {
    pub id: i64,
    pub cid: Option<i64>,
    pub column: BadColumn,
}

/// A card as read: readable, or unreadable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardRead {
    Card(StatsCard),
    Bad(BadCard),
}

impl CardRead {
    pub fn id(&self) -> i64 {
        match self {
            CardRead::Card(card) => card.id,
            CardRead::Bad(bad) => bad.id,
        }
    }

    /// The deck values a Deck scope compares (G2.1).
    pub(crate) fn decks(&self) -> (Option<i64>, Option<i64>) {
        match self {
            CardRead::Card(card) => (Some(card.did), Some(card.odid)),
            CardRead::Bad(bad) => (bad.did, bad.odid),
        }
    }
}

/// A review-log row as read, with its card id as stored (none when `cid`
/// does not read as an integer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowRead {
    Row { row: StatsRow, cid: i64 },
    Bad(BadRow),
}

impl RowRead {
    pub fn id(&self) -> i64 {
        match self {
            RowRead::Row { row, .. } => row.id,
            RowRead::Bad(bad) => bad.id,
        }
    }

    pub(crate) fn cid(&self) -> Option<i64> {
        match self {
            RowRead::Row { cid, .. } => Some(*cid),
            RowRead::Bad(bad) => bad.cid,
        }
    }

    pub(crate) fn with_cid(self, card: i64) -> RowRead {
        match self {
            RowRead::Row { row, .. } => RowRead::Row { row, cid: card },
            RowRead::Bad(bad) => RowRead::Bad(BadRow { cid: Some(card), ..bad }),
        }
    }
}

/// Where the columns a card read takes sit in a row image.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CardColumns {
    id: usize,
    nid: usize,
    did: usize,
    odid: usize,
    card_type: usize,
    queue: usize,
    due: usize,
    odue: usize,
    ivl: usize,
    data: usize,
}

/// The order of `ReadRows::visit_stats_cards`.
pub(crate) const VISITED_CARD: CardColumns = CardColumns {
    id: 0,
    nid: 1,
    did: 2,
    odid: 3,
    card_type: 4,
    queue: 5,
    due: 6,
    odue: 7,
    ivl: 8,
    data: 9,
};

/// The card columns of `ReadRows::visit_stats_histories`, which reads no
/// `data` (`stats/history-reads` H2 needs no memory state).
pub(crate) const HISTORY_CARD: CardColumns = CardColumns {
    id: 0,
    nid: 1,
    did: 2,
    odid: 3,
    card_type: 4,
    queue: 5,
    due: 6,
    odue: 7,
    ivl: 8,
    data: 9,
};

/// The `cards` table's own order (a journal image).
pub(crate) const TABLE_CARD: CardColumns = CardColumns {
    id: 0,
    nid: 1,
    did: 2,
    odid: 15,
    card_type: 6,
    queue: 7,
    due: 8,
    odue: 14,
    ivl: 9,
    data: 17,
};

fn integer(cell: Option<&Cell>) -> Option<i64> {
    match cell {
        Some(Cell::Integer(value)) => Some(*value),
        _ => None,
    }
}

/// R4 and R5: an INTEGER, or a REAL truncated toward zero, in the range of
/// `T`; anything else is unreadable.
fn converted<T: TryFrom<i64>>(cell: Option<&Cell>) -> Option<T> {
    match cell {
        Some(Cell::Integer(value)) => T::try_from(*value).ok(),
        Some(Cell::Real(value)) if value.is_finite() => {
            let truncated = value.trunc();
            if (-9.223_372_036_854_776e18..9.223_372_036_854_776e18).contains(&truncated) {
                T::try_from(truncated as i64).ok()
            } else {
                None
            }
        }
        _ => None,
    }
}

/// G4 and R4: a card from the cells at `at`. An unreadable one keeps its
/// first bad column, in the table's column order.
pub(crate) fn read_card(cells: &[Cell], at: &CardColumns) -> CardRead {
    let id = integer(cells.get(at.id)).unwrap_or(0);
    let did = integer(cells.get(at.did));
    let odid = integer(cells.get(at.odid));
    let bad = |column| CardRead::Bad(BadCard { id, did, odid, column });
    let Some(nid) = integer(cells.get(at.nid)) else {
        return bad(BadColumn::CardNid);
    };
    let Some(did_value) = did else {
        return bad(BadColumn::CardDid);
    };
    let Some(card_type) = integer(cells.get(at.card_type)).and_then(CardType::from_stored) else {
        return bad(BadColumn::CardType);
    };
    let Some(queue) = integer(cells.get(at.queue)).and_then(CardQueue::from_stored) else {
        return bad(BadColumn::CardQueue);
    };
    // R4: the converted `ivl` is checked; −1, like any value outside u32,
    // makes the row unreadable.
    let Some(ivl) = converted::<u32>(cells.get(at.ivl)) else {
        return bad(BadColumn::CardIvl);
    };
    let Some(odid_value) = odid else {
        return bad(BadColumn::CardOdid);
    };
    let small = |cell: Option<&Cell>| integer(cell).and_then(|v| i32::try_from(v).ok()).unwrap_or(0);
    let data = match cells.get(at.data) {
        Some(Cell::Text(bytes)) => CardData::parse(&String::from_utf8_lossy(bytes)),
        _ => CardData::default(),
    };
    CardRead::Card(StatsCard {
        id,
        nid,
        did: did_value,
        odid: odid_value,
        card_type,
        queue,
        due: small(cells.get(at.due)),
        odue: small(cells.get(at.odue)),
        ivl,
        memory: data.memory_state,
        decay: data.decay,
        lrt: data.last_review_secs,
    })
}

/// G4 and R5: a review-log row from its cells in the table's order (`id`,
/// `cid`, `usn`, `ease`, `ivl`, `lastIvl`, `factor`, `time`, `type`).
pub(crate) fn read_row(cells: &[Cell]) -> RowRead {
    let id = integer(cells.first()).unwrap_or(0);
    let cid = integer(cells.get(1));
    let bad = |column| RowRead::Bad(BadRow { id, cid, column });
    let Some(cid_value) = cid else {
        return bad(BadColumn::RevlogCid);
    };
    // `model/row-records` R1 (changed 2026-11-17): any INTEGER `usn` reads,
    // its low 32 bits kept; only another storage class is unreadable.
    if integer(cells.get(2)).is_none() {
        return bad(BadColumn::RevlogUsn);
    }
    let Some(ease) = integer(cells.get(3)).and_then(|ease| u8::try_from(ease).ok()) else {
        return bad(BadColumn::RevlogEase);
    };
    let Some(ivl) = converted::<i32>(cells.get(4)) else {
        return bad(BadColumn::RevlogIvl);
    };
    let Some(last_ivl) = converted::<i32>(cells.get(5)) else {
        return bad(BadColumn::RevlogLastIvl);
    };
    let Some(factor) = integer(cells.get(6)).and_then(|factor| u32::try_from(factor).ok()) else {
        return bad(BadColumn::RevlogFactor);
    };
    let time = integer(cells.get(7)).and_then(|time| u32::try_from(time).ok()).unwrap_or(0);
    let kind = integer(cells.get(8)).map_or(RevlogKind::Learning, RevlogKind::from_stored);
    RowRead::Row {
        row: StatsRow {
            id,
            ease,
            ivl,
            last_ivl,
            factor,
            time,
            kind,
        },
        cid: cid_value,
    }
}
