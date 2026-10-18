//! Ordered, bounded history reads and raw-type cutoff counts (U46).

use crate::{
    StorageError,
    rows::{Cell, Record, Table},
};
use meguri_model::{
    card::Card,
    ids::{CardId, DeckId},
    revlog::RevlogEntry,
};
use rusqlite::{Connection, OptionalExtension};

#[derive(Clone, Debug)]
pub enum HistorySelection {
    HomeDecks(Vec<DeckId>),
    Cards(Vec<CardId>),
}
impl HistorySelection {
    fn predicate(&self) -> &'static str {
        match self {
            Self::HomeDecks(_) => {
                "(CASE WHEN c.odid != 0 THEN c.odid ELSE c.did END) IN (SELECT value FROM json_each(?1))"
            }
            Self::Cards(_) => "c.id IN (SELECT value FROM json_each(?1))",
        }
    }
    fn json(&self) -> String {
        let ids: Vec<i64> = match self {
            Self::HomeDecks(ids) => ids.iter().map(|id| id.0).collect(),
            Self::Cards(ids) => ids.iter().map(|id| id.0).collect(),
        };
        // Only integers enter this bound parameter; no SQL is assembled from
        // caller text, and the full i64 id survives JSON without a float.
        serde_json::to_string(&ids).expect("integer id lists serialize")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncludedCount {
    pub included: u64,
    pub total: u64,
}
impl IncludedCount {
    pub fn warning(self) -> bool {
        self.total > 0 && u128::from(self.included) * 10 < u128::from(self.total)
    }
}

pub(crate) fn last_review_time(conn: &Connection, card: CardId) -> Result<Option<i64>, StorageError> {
    Ok(conn.prepare_cached("SELECT id FROM revlog WHERE cid=?1 AND ease BETWEEN 1 AND 4 AND NOT(type=3 AND factor=0) ORDER BY id DESC LIMIT 1")?
        .query_row([card.0],|row|row.get::<_,i64>(0)).optional()?.map(|id|id/1000))
}

pub(crate) fn count(
    conn: &Connection,
    selection: &HistorySelection,
    cutoff: i64,
    exclude_suspended: bool,
) -> Result<IncludedCount, StorageError> {
    let sql = format!(
        "SELECT count(*),coalesce(sum(EXISTS(SELECT 1 FROM revlog r WHERE r.cid=c.id AND r.type=0 AND r.id>?2)),0) FROM cards c WHERE {} AND c.type!=0 AND (?3=0 OR c.queue!=-1)",
        selection.predicate()
    );
    Ok(conn
        .prepare_cached(&sql)?
        .query_row((selection.json(), cutoff, exclude_suspended), |row| {
            Ok(IncludedCount {
                total: row.get::<_, i64>(0)? as u64,
                included: row.get::<_, i64>(1)? as u64,
            })
        })?)
}

/// One statement per preset, ordered by cid then id, keeping one card's
/// history at a time. A callback error stops the statement immediately.
pub(crate) fn visit<E: From<StorageError>>(
    conn: &Connection,
    selection: &HistorySelection,
    mut each: impl FnMut(Card, &[RevlogEntry]) -> Result<(), E>,
) -> Result<(), E> {
    let columns = |table: Table, alias: &str| {
        table
            .columns()
            .iter()
            .map(|column| format!("{alias}.\"{column}\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    let sql = format!(
        "SELECT {},{} FROM cards c LEFT JOIN revlog r ON r.cid=c.id WHERE {} ORDER BY c.id,r.id",
        columns(Table::Cards, "c"),
        columns(Table::Revlog, "r"),
        selection.predicate()
    );
    let mut statement = conn.prepare_cached(&sql).map_err(StorageError::from)?;
    let mut found = statement.query([selection.json()]).map_err(StorageError::from)?;
    let mut card: Option<Card> = None;
    let mut history = Vec::new();
    let width = Table::Cards.columns().len();
    while let Some(row) = found.next().map_err(StorageError::from)? {
        let id = row.get::<_, i64>(0).map_err(StorageError::from)?;
        if card.as_ref().is_none_or(|current| current.id.0 != id) {
            if let Some(previous) = card.take() {
                each(previous, &history)?;
                history.clear();
            }
            let cells = (0..width)
                .map(|index| row.get_ref(index).map(Cell::from_ref))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StorageError::from)?;
            card = Some(Card::from_cells(&cells).map_err(StorageError::from)?);
        }
        if !matches!(
            row.get_ref(width).map_err(StorageError::from)?,
            rusqlite::types::ValueRef::Null
        ) {
            let cells = (width..width + Table::Revlog.columns().len())
                .map(|index| row.get_ref(index).map(Cell::from_ref))
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(StorageError::from)?;
            history.push(RevlogEntry::from_cells(&cells).map_err(StorageError::from)?);
        }
    }
    if let Some(last) = card {
        each(last, &history)?;
    }
    Ok(())
}
