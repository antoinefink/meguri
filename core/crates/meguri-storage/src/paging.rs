//! Typed reads on any connection of a collection (a reader, the writer, a
//! write scope), and pages by id range: each call is one short statement,
//! so no read transaction stays open between pages
//! (`storage/open-and-connections` C7).

use meguri_model::card::Card;
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::grave::{Grave, GraveKind};
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_model::revlog::RevlogEntry;
use meguri_model::tag::Tag;
use rusqlite::{Connection, params_from_iter};

use crate::connections::{Reader, Writer};
use crate::error::StorageError;
use crate::journal;
use crate::rows::config::{ConfigValue, stored_value};
use crate::rows::{self, Cell, Record, RowImage, RowKey, Table, col::COL_KEY, graves, notetypes};
use crate::snapshot::ReadSnapshot;
use crate::write_scope::WriteScope;

mod sealed {
    /// The connection a [`super::ReadRows`] implementor reads with.
    pub trait Connected {
        fn connection(&self) -> &rusqlite::Connection;
    }
}

impl sealed::Connected for Reader {
    fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl sealed::Connected for Writer {
    fn connection(&self) -> &Connection {
        &self.conn
    }
}

impl sealed::Connected for WriteScope<'_> {
    fn connection(&self) -> &Connection {
        self.conn()
    }
}

impl sealed::Connected for ReadSnapshot<'_> {
    fn connection(&self) -> &Connection {
        self.conn()
    }
}

/// A config value as stored: its row and its value read per
/// `config-table` C3.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredConfig {
    /// The row, `val`'s bytes whatever their storage class.
    pub entry: ConfigEntry,
    /// `val` was stored as a BLOB.
    pub is_blob: bool,
}

/// Typed reads (`model/row-records`): a row that cannot be read is
/// [`StorageError::Row`]; a missing row is `None`.
pub trait ReadRows: sealed::Connected {
    fn col(&self) -> Result<Col, StorageError> {
        rows::get::<Col>(self.connection(), &COL_KEY)?.ok_or_else(|| StorageError::Mode {
            detail: "the collection has no col row".to_owned(),
        })
    }

    fn note(&self, id: NoteId) -> Result<Option<Note>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    fn card(&self, id: CardId) -> Result<Option<Card>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    fn revlog_entry(&self, id: RevlogId) -> Result<Option<RevlogEntry>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    fn deck(&self, id: DeckId) -> Result<Option<Deck>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    fn deck_config(&self, id: DeckConfigId) -> Result<Option<DeckConfig>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    fn notetype(&self, id: NotetypeId) -> Result<Option<Notetype>, StorageError> {
        notetypes::get(self.connection(), id)
    }

    fn tag(&self, name: &str) -> Result<Option<Tag>, StorageError> {
        rows::get(self.connection(), &RowKey::text(name))
    }

    /// Every `tags` row, in the table's key order (`unicase`).
    fn tag_rows(&self) -> Result<Vec<Tag>, StorageError> {
        rows::select_where(self.connection(), Table::Tags, "1", &[])?
            .iter()
            .map(|cells| Tag::from_cells(cells).map_err(StorageError::from))
            .collect()
    }

    /// Up to `limit` notes' `tags` columns with ids above `after` (all when
    /// `None`), by id: the tag scans of `tags/tags` T5 and T11, which read
    /// no field. A `tags` value that is not UTF-8 text makes the row
    /// unreadable, as it does for [`ReadRows::note`].
    fn note_tags_after(&self, after: Option<NoteId>, limit: usize) -> Result<Vec<(NoteId, String)>, StorageError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT id, tags FROM notes WHERE id > ?1 ORDER BY id LIMIT ?2")?;
        let found = statement.query_map((after.map_or(i64::MIN, |id| id.0), limit_value(limit)), |row| {
            Ok((row.get::<_, i64>(0)?, Cell::from_ref(row.get_ref(1)?)))
        })?;
        let mut out = Vec::new();
        for item in found {
            let (id, tags) = item?;
            let tags = match tags {
                Cell::Text(bytes) => String::from_utf8(bytes).map_err(|_| "text is not UTF-8".to_owned()),
                other => Err(format!("not text: {other:?}")),
            };
            match tags {
                Ok(tags) => out.push((NoteId(id), tags)),
                Err(reason) => {
                    return Err(StorageError::from(rows::RowError {
                        table: Table::Notes.name(),
                        column: "tags",
                        key: RowKey::Id(id).to_string(),
                        reason,
                    }));
                }
            }
        }
        Ok(out)
    }

    fn grave(&self, oid: i64, kind: GraveKind) -> Result<Option<Grave>, StorageError> {
        rows::get(self.connection(), &graves::key(oid, kind))
    }

    /// The stored row with `key`, every column by storage class, as the
    /// journal records rows; `None` when there is none.
    fn row_image(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
        rows::select_image(self.connection(), table, key)
    }

    /// The SHA-256 of the stored row with `key` ([`journal::digest`]), the
    /// content precondition of `engine/op-model` O7; `None` when there is no
    /// such row.
    fn row_digest(&self, table: Table, key: &RowKey) -> Result<Option<[u8; 32]>, StorageError> {
        Ok(self.row_image(table, key)?.map(|image| journal::digest(&image)))
    }

    /// The cards of a note, by id.
    fn cards_of_note(&self, note: NoteId) -> Result<Vec<Card>, StorageError> {
        rows::select_where(self.connection(), Table::Cards, "nid = ?1", &[Cell::Integer(note.0)])?
            .iter()
            .map(|cells| Card::from_cells(cells).map_err(StorageError::from))
            .collect()
    }

    /// A config row as stored.
    fn config_entry(&self, key: &str) -> Result<Option<StoredConfig>, StorageError> {
        match rows::select_image(self.connection(), Table::Config, &RowKey::text(key))? {
            Some(cells) => Ok(Some(StoredConfig {
                is_blob: matches!(cells[3], Cell::Blob(_)),
                entry: ConfigEntry::from_cells(&cells)?,
            })),
            None => Ok(None),
        }
    }

    /// A config value read per C3: `None` when the key is missing, its
    /// value is not a BLOB or not valid JSON. Never fails on the value.
    fn config_value(&self, key: &str) -> Result<Option<ConfigValue>, StorageError> {
        Ok(
            rows::select_image(self.connection(), Table::Config, &RowKey::text(key))?
                .and_then(|cells| stored_value(&cells[3])),
        )
    }

    /// Every config key, in byte order.
    fn config_keys(&self) -> Result<Vec<String>, StorageError> {
        Ok(rows::select_where(self.connection(), Table::Config, "1", &[])?
            .iter()
            .filter_map(|cells| match &cells[0] {
                Cell::Text(bytes) => String::from_utf8(bytes.clone()).ok(),
                _ => None,
            })
            .collect())
    }

    /// Up to `limit` notes with ids above `after` (all when `None`), by id.
    fn notes_after(&self, after: Option<NoteId>, limit: usize) -> Result<Vec<Note>, StorageError> {
        page(self.connection(), after.map(|id| id.0), limit)
    }

    /// Up to `limit` cards with ids above `after`, by id.
    fn cards_after(&self, after: Option<CardId>, limit: usize) -> Result<Vec<Card>, StorageError> {
        page(self.connection(), after.map(|id| id.0), limit)
    }

    /// Up to `limit` review-log rows with ids above `after`, by id.
    fn revlog_after(&self, after: Option<RevlogId>, limit: usize) -> Result<Vec<RevlogEntry>, StorageError> {
        page(self.connection(), after.map(|id| id.0), limit)
    }

    /// Up to `limit` ids of `table` (one with an `id` column) above `after`.
    fn ids_after(&self, table: Table, after: Option<i64>, limit: usize) -> Result<Vec<i64>, StorageError> {
        if !table.allocates_ids() {
            return Err(StorageError::Mode {
                detail: format!("{table} has no id column to page by"),
            });
        }
        let sql = format!("SELECT id FROM {} WHERE id > ?1 ORDER BY id LIMIT ?2", table.name());
        let mut statement = self.connection().prepare_cached(&sql)?;
        let ids = statement.query_map((after.unwrap_or(i64::MIN), limit_value(limit)), |row| row.get(0))?;
        Ok(ids.collect::<rusqlite::Result<Vec<i64>>>()?)
    }
}

impl ReadRows for Reader {}
impl ReadRows for Writer {}
impl ReadRows for WriteScope<'_> {}
impl ReadRows for ReadSnapshot<'_> {}

fn limit_value(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

/// One page of a table keyed by `id`: rows with ids above `after`.
fn page<R: Record>(conn: &Connection, after: Option<i64>, limit: usize) -> Result<Vec<R>, StorageError> {
    let columns = R::TABLE
        .columns()
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {columns} FROM {} WHERE id > ?1 ORDER BY id LIMIT ?2",
        R::TABLE.name()
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let width = R::TABLE.columns().len();
    let params = [
        Cell::Integer(after.unwrap_or(i64::MIN)),
        Cell::Integer(limit_value(limit)),
    ];
    let images = statement.query_map(params_from_iter(params.iter()), |row| {
        (0..width)
            .map(|i| row.get_ref(i).map(Cell::from_ref))
            .collect::<rusqlite::Result<Vec<Cell>>>()
    })?;
    let mut out = Vec::new();
    for image in images {
        out.push(R::from_cells(&image?)?);
    }
    Ok(out)
}
