//! The day clock's writes (U40; `specs/sched/day-clock.md` D5, D7;
//! `storage/ids-usn-mtime` S3): config keys stamped as a local change
//! without moving `col.mod`, and the unbury's queue writes, which keep each
//! card's `mod` and `usn`. Both run in a `Raw` scope, which never moves
//! `col.mod`; which queue a card gets is the day clock's (meguri-sched).

use meguri_model::ids::CardId;
use meguri_model::usn::Usn;

use crate::error::StorageError;
use crate::rows::config::{ConfigValue, same_value};
use crate::rows::{self, Cell, RowKey, Table};
use crate::stamp::StampMode;
use crate::write_scope::{WriteScope, Written};

/// The column of `cards.queue`.
const QUEUE: usize = 7;

/// A card's `queue` and what decides where it goes, as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardQueueRow {
    pub id: CardId,
    pub card_type: i64,
    pub queue: i64,
    pub due: i64,
}

/// The cards whose stored `queue` is one of `queues`, by id. A row whose
/// `type` or `due` is not an integer cannot be read as a card (row-records
/// R4) and is left out.
pub(crate) fn cards_in_queues(conn: &rusqlite::Connection, queues: &[i64]) -> Result<Vec<CardQueueRow>, StorageError> {
    if queues.is_empty() {
        return Ok(Vec::new());
    }
    let marks = vec!["?"; queues.len()].join(", ");
    let params: Vec<Cell> = queues.iter().map(|q| Cell::Integer(*q)).collect();
    Ok(
        rows::select_where(conn, Table::Cards, &format!("queue IN ({marks})"), &params)?
            .iter()
            .filter_map(|cells| {
                Some(CardQueueRow {
                    id: CardId(cells[0].as_i64()?),
                    card_type: cells[6].as_i64()?,
                    queue: cells[QUEUE].as_i64()?,
                    due: cells[8].as_i64()?,
                })
            })
            .collect(),
    )
}

impl WriteScope<'_> {
    fn require_raw(&self, what: &str) -> Result<(), StorageError> {
        match self.mode() {
            StampMode::Raw => Ok(()),
            mode => Err(StorageError::Mode {
                detail: format!("{what} is a day clock write: a Raw scope only, not {mode:?}"),
            }),
        }
    }

    /// Sets a config key as the day clock does (S3: `rollover`,
    /// `localOffset`, `lastUnburied`): nothing when the key already holds the
    /// same value (config-table C4); otherwise the value in compact form,
    /// `usn` -1 and `mtime_secs` now, with `col.mod` left alone. Only in a
    /// `Raw` scope.
    pub fn set_config_keeping_col_mod(&mut self, key: &str, value: &ConfigValue) -> Result<Written, StorageError> {
        self.require_raw("set_config_keeping_col_mod")?;
        let stored = rows::select_image(self.conn(), Table::Config, &RowKey::text(key))?;
        if stored.as_ref().is_some_and(|row| same_value(&row[3], value)) {
            return Ok(Written::Unchanged);
        }
        let now = self.now().whole_secs().0;
        let cells = vec![
            Cell::text(key),
            Cell::Integer(i64::from(Usn::PENDING.0)),
            Cell::Integer(now),
            Cell::Blob(value.compact().into_bytes()),
        ];
        self.put_cells(Table::Config, cells, false)
    }

    /// Sets the stored `queue` of card `id` and nothing else: its `mod` and
    /// `usn` stay (R599). A missing card or one already in `queue` is no
    /// change. Only in a `Raw` scope.
    pub fn set_card_queue(&mut self, id: CardId, queue: i64) -> Result<Written, StorageError> {
        self.require_raw("set_card_queue")?;
        let Some(mut cells) = rows::select_image(self.conn(), Table::Cards, &RowKey::Id(id.0))? else {
            return Ok(Written::Unchanged);
        };
        cells[QUEUE] = Cell::Integer(queue);
        self.put_cells(Table::Cards, cells, false)
    }
}
