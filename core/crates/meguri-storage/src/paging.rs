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
use meguri_model::usn::Usn;
use rusqlite::{Connection, params_from_iter};

use crate::connections::{Reader, Writer};
use crate::day_writes::CardQueueRow;
use crate::error::StorageError;
use crate::journal;
use crate::rows::config::{ConfigValue, stored_value};
use crate::rows::{self, Cell, Record, RowImage, RowKey, Table, col::COL_KEY, graves, notetypes};
use crate::snapshot::ReadSnapshot;
use crate::write_scope::WriteScope;

pub(crate) mod sealed {
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
    /// Count raw rows without reading their variable-sized payload columns.
    fn row_count(&self, table: Table) -> Result<u64, StorageError> {
        let count: i64 = self.connection().query_row(
            &format!("SELECT count(*) FROM {}", table.name()), [], |row| row.get(0))?;
        u64::try_from(count).map_err(|_| StorageError::Io { detail: "invalid collection row count".into() })
    }
    fn nondefault_deck_exists(&self) -> Result<bool, StorageError> {
        Ok(self.connection().query_row("SELECT EXISTS(SELECT 1 FROM decks WHERE id<>1)", [], |row| row.get(0))?)
    }

    /// At most 250 stored images and 8 MiB of copied cells. One larger
    /// selected row is returned alone so keyset traversal always progresses.
    fn row_page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, StorageError> {
        crate::normal_stage::raw_page(self.connection(), table, after)
    }

    /// Raw fields of one existing parent, in stored `ord` order, at most 250
    /// rows. Continue after the last returned ordinal; gaps are preserved.
    /// Copy at most 8 MiB, except that a larger first row is returned alone.
    /// Continue until an empty page. A missing parent returns an empty page.
    fn notetype_fields_page(&self, notetype: NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError> {
        notetype_parts_page(self.connection(), Table::Fields, notetype, after)
    }

    /// Raw templates, with the same paging contract as
    /// [`ReadRows::notetype_fields_page`].
    fn notetype_templates_page(&self, notetype: NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError> {
        notetype_parts_page(self.connection(), Table::Templates, notetype, after)
    }

    /// Bounded keyset enumeration for raw identity, including invalid stored
    /// ordinal classes. The continuation is the actual last stored cell.
    fn notetype_parts_raw_page(&self, table: Table, notetype: NotetypeId, after: Option<&Cell>) -> Result<Vec<RowImage>, StorageError> {
        if !matches!(table, Table::Fields | Table::Templates) {
            return Err(StorageError::Io { detail: "invalid notetype part table".into() });
        }
        notetype_parts_raw_page(self.connection(), table, notetype, after)
    }

    /// Unfiltered maximum stored revlog id for this card on this connection's
    /// view, or 0 when no row exists. Keeps signed millisecond ids as is.
    /// This reads live rows (or the reader's snapshot); a future held-image
    /// history must compute its own image-aware maximum.
    fn max_revlog_id(&self, card: CardId) -> Result<i64, StorageError> {
        Ok(self
            .connection()
            .query_row("SELECT coalesce(max(id),0) FROM revlog WHERE cid=?1", [card.0], |row| {
                row.get::<_, i64>(0)
            })?)
    }

    /// How many review-log rows the card has and the highest id among them
    /// (0 for none): what a plan that read its history compares again in
    /// its commit (Change Deck's step 4, `browser/bulk-actions` B7.2).
    fn revlog_extent(&self, card: CardId) -> Result<(u64, i64), StorageError> {
        let (count, max) = self.connection().query_row(
            "SELECT count(*), coalesce(max(id),0) FROM revlog WHERE cid=?1",
            [card.0],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        )?;
        Ok((u64::try_from(count).unwrap_or(0), max))
    }

    /// Maximum below an exclusive id bound, for a held-image history that
    /// omits newly created review rows without scanning the whole collection.
    fn max_revlog_id_before(&self, card: CardId, before: i64) -> Result<i64, StorageError> {
        Ok(self.connection().query_row(
            "SELECT coalesce(max(id),0) FROM revlog WHERE cid=?1 AND id<?2",
            (card.0, before),
            |row| row.get::<_, i64>(0),
        )?)
    }

    /// Scalar normal-merge base; validation replay never loads a note body.
    fn sync_row_state(
        &self,
        table: Table,
        id: i64,
    ) -> Result<Option<(Usn, meguri_model::time::EpochSecs)>, StorageError> {
        use rusqlite::OptionalExtension;
        if !matches!(table, Table::Cards | Table::Notes) {
            return Err(StorageError::Mode {
                detail: "sync scalar table".into(),
            });
        }
        Ok(self
            .connection()
            .query_row(
                &format!("SELECT usn,mod FROM {} WHERE id=?1", table.name()),
                [id],
                |row| Ok((Usn(row.get(0)?), meguri_model::time::EpochSecs(row.get(1)?))),
            )
            .optional()?)
    }

    fn answer_due_range(&self, today: u32) -> Result<Vec<crate::queries::due_range::DueRow>, StorageError> {
        crate::queries::due_range::read(self.connection(), today)
    }
    fn answer_siblings(
        &self,
        note: NoteId,
        except: CardId,
    ) -> Result<Vec<crate::queries::siblings::Sibling>, StorageError> {
        crate::queries::siblings::read(self.connection(), note, except)
    }
    fn sched_queue_candidates(
        &self,
        decks: &[DeckId],
        phase: crate::queries::sched_queue::Phase,
        order: u8,
        today: u32,
        now: i64,
        boundary: i64,
    ) -> Result<Vec<Card>, StorageError> {
        crate::queries::sched_queue::candidates(self.connection(), decks, phase, order, today, now, boundary)
    }
    #[allow(clippy::too_many_arguments)]
    fn sched_queue_candidates_overlay(
        &self,
        decks: &[DeckId],
        phase: crate::queries::sched_queue::Phase,
        order: u8,
        today: u32,
        now: i64,
        boundary: i64,
        overrides: &[Card],
    ) -> Result<Vec<Card>, StorageError> {
        crate::queries::sched_queue::candidates_overlay(
            self.connection(),
            decks,
            phase,
            order,
            today,
            now,
            boundary,
            overrides,
        )
    }
    fn sched_deck_aggregates(
        &self,
        today: u32,
        now: i64,
        boundary: i64,
        ahead: u32,
    ) -> Result<Vec<crate::queries::sched_queue::DeckAggregate>, StorageError> {
        crate::queries::sched_queue::aggregate(self.connection(), today, now, boundary, ahead)
    }
    fn sched_subtree_empty(&self, decks: &[DeckId]) -> Result<bool, StorageError> {
        crate::queries::sched_queue::empty(self.connection(), decks)
    }
    /// The activity heatmap's counts of days 0 to 370 before `next_day_at`
    /// (`stats/activity-heatmap` AH5): one statement, bounded by `id`.
    fn activity_day_counts(
        &self,
        scope: crate::queries::activity::ActivityScope<'_>,
        next_day_at: i64,
    ) -> Result<Vec<u32>, StorageError> {
        crate::queries::activity::day_counts(self.connection(), scope, next_day_at)
    }
    /// `stats/studied-today` T12.1: the number of N6's rows after review-log
    /// id `after` (`stats/day-buckets` B2) and the sum of their `time`.
    fn studied_since(&self, after: i64) -> Result<crate::queries::studied::Studied, StorageError> {
        crate::queries::studied::studied_since(self.connection(), after)
    }

    /// Execute the read-only one-column card-id statement compiled by the
    /// search crate. SQL remains inside storage; callers bind typed cells.
    fn search_card_ids(&self, sql: &str, params: &[Cell]) -> Result<Vec<CardId>, StorageError> {
        let mut statement = self.connection().prepare_cached(sql)?;
        if !statement.readonly() || statement.column_count() != 1 {
            return Err(StorageError::Mode {
                detail: "card search must be a read-only id query".into(),
            });
        }
        Ok(statement
            .query_map(params_from_iter(params), |row| row.get::<_, i64>(0).map(CardId))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Runs one read-only statement compiled by the search crate whose one
    /// or two columns are integers (a result's ids, with their note ids),
    /// each row handed to `visit` as SQLite returns it (result-sets H3).
    /// `cancel` is polled while SQLite works: once it returns true the
    /// statement stops with SQLite's interrupt (code 9). `visit` returns
    /// false to stop reading early.
    fn visit_id_rows(
        &self,
        sql: &str,
        params: &[Cell],
        cancel: &dyn Fn() -> bool,
        visit: &mut dyn FnMut(&[i64]) -> bool,
    ) -> Result<(), StorageError> {
        crate::search_rows::visit(self.connection(), sql, params, cancel, visit)
    }
    /// M3's indexed fallback; Reset does not truncate this lookup.
    fn fsrs_last_review_time(&self, card: CardId) -> Result<Option<i64>, StorageError> {
        crate::queries::revlog_fsrs::last_review_time(self.connection(), card)
    }

    /// Home deck ids of cards currently in the selected decks, including
    /// filtered cards. Returns one id per home, without loading card rows.
    fn card_home_decks_in(&self, decks: &[DeckId]) -> Result<Vec<DeckId>, StorageError> {
        crate::queries::revlog_fsrs::home_decks_in(self.connection(), decks)
    }

    fn fsrs_selection_count(
        &self,
        selection: &crate::queries::revlog_fsrs::HistorySelection,
    ) -> Result<u64, StorageError> {
        crate::queries::revlog_fsrs::selection_count(self.connection(), selection)
    }

    fn fsrs_included_count(
        &self,
        selection: &crate::queries::revlog_fsrs::HistorySelection,
        cutoff: i64,
        exclude_suspended: bool,
    ) -> Result<crate::queries::revlog_fsrs::IncludedCount, StorageError> {
        crate::queries::revlog_fsrs::count(self.connection(), selection, cutoff, exclude_suspended)
    }

    fn visit_fsrs_histories<E: From<StorageError>>(
        &self,
        selection: &crate::queries::revlog_fsrs::HistorySelection,
        each: impl FnMut(Card, &[RevlogEntry]) -> Result<(), E>,
    ) -> Result<(), E> {
        crate::queries::revlog_fsrs::visit(self.connection(), selection, each)
    }
    fn col(&self) -> Result<Col, StorageError> {
        rows::get::<Col>(self.connection(), &COL_KEY)?.ok_or_else(|| StorageError::Mode {
            detail: "the collection has no col row".to_owned(),
        })
    }

    fn note(&self, id: NoteId) -> Result<Option<Note>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    /// Indexed candidates for the editor's duplicate check (field-check K4).
    fn notes_with_checksum(&self, notetype: NotetypeId, checksum: u32) -> Result<Vec<Note>, StorageError> {
        rows::select_where(
            self.connection(),
            Table::Notes,
            "csum = ?1 AND mid = ?2",
            &[Cell::Integer(i64::from(checksum)), Cell::Integer(notetype.0)],
        )?
        .iter()
        .map(|cells| Note::from_cells(cells).map_err(StorageError::from))
        .collect()
    }

    /// How many notes one type has, for regeneration progress.
    fn notetype_note_count(&self, notetype: NotetypeId) -> Result<u64, StorageError> {
        Ok(self
            .connection()
            .query_row("SELECT count(*) FROM notes WHERE mid = ?1", [notetype.0], |row| {
                row.get::<_, i64>(0)
            })? as u64)
    }

    /// The notes among `ids` that exist, in id order, from one statement
    /// (bulk actions read their listed notes a page at a time).
    fn notes_with_ids(&self, ids: &[NoteId]) -> Result<Vec<Note>, StorageError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = json_ids(ids);
        let mut statement = self.connection().prepare_cached(
            "SELECT id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data FROM notes WHERE id IN (SELECT value FROM json_each(?1)) ORDER BY id")?;
        let found = statement.query_map([list], |row| rows::row_image(row, Table::Notes))?;
        found
            .map(|cells| Note::from_cells(&cells?).map_err(StorageError::from))
            .collect()
    }

    /// The cards of the notes among `notes`, by note id then card id, from
    /// one statement (a bulk rewrite's page of notes).
    fn cards_of_notes_with_ids(&self, notes: &[NoteId]) -> Result<Vec<Card>, StorageError> {
        if notes.is_empty() {
            return Ok(Vec::new());
        }
        let mut statement = self.connection().prepare_cached(&format!(
            "SELECT {} FROM cards WHERE nid IN (SELECT value FROM json_each(?1)) ORDER BY nid, id",
            Table::Cards
                .columns()
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ))?;
        let found = statement.query_map([json_ids(notes)], |row| rows::row_image(row, Table::Cards))?;
        found
            .map(|cells| Card::from_cells(&cells?).map_err(StorageError::from))
            .collect()
    }

    /// The number of notes of each `mid` that has notes, by `mid`
    /// (`notetypes/editor-actions` A2's note counts). A `mid` may name no
    /// note type; the caller keeps only those that do.
    fn note_counts_by_notetype(&self) -> Result<Vec<(NotetypeId, u64)>, StorageError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT mid, count(*) FROM notes GROUP BY mid ORDER BY mid")?;
        let rows = statement.query_map([], |row| Ok((NotetypeId(row.get(0)?), row.get::<_, i64>(1)?)))?;
        rows.map(|row| {
            let (id, count) = row?;
            let count = u64::try_from(count).map_err(|_| StorageError::Io {
                detail: "invalid note count".into(),
            })?;
            Ok((id, count))
        })
        .collect()
    }

    /// Every note type's id and stored name, by name under the `unicase`
    /// collation of the `notetypes.name` column, ties by ascending id
    /// (`notetypes/editor-actions` A2's ordered list).
    fn notetype_names_by_name(&self) -> Result<Vec<(NotetypeId, String)>, StorageError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT id, name FROM notetypes ORDER BY name COLLATE unicase, id")?;
        let rows = statement.query_map([], |row| Ok((NotetypeId(row.get(0)?), row.get::<_, String>(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Notes of one type, in id order, for cancellable regeneration.
    fn notetype_notes_after(
        &self,
        notetype: NotetypeId,
        after: Option<NoteId>,
        limit: usize,
    ) -> Result<Vec<Note>, StorageError> {
        // The first page has no id bound, so a note whose id is i64::MIN is
        // read like any other.
        let images: Vec<RowImage> = match after {
            None => self
                .connection()
                .prepare_cached(
                    "SELECT id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data FROM notes \
                     WHERE mid = ?1 ORDER BY id LIMIT ?2",
                )?
                .query_map((notetype.0, limit_value(limit)), |row| rows::row_image(row, Table::Notes))?
                .collect::<rusqlite::Result<_>>()?,
            Some(after) => self
                .connection()
                .prepare_cached(
                    "SELECT id, guid, mid, mod, usn, tags, flds, sfld, csum, flags, data FROM notes \
                     WHERE mid = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
                )?
                .query_map((notetype.0, after.0, limit_value(limit)), |row| rows::row_image(row, Table::Notes))?
                .collect::<rusqlite::Result<_>>()?,
        };
        images
            .iter()
            .map(|cells| Note::from_cells(cells).map_err(StorageError::from))
            .collect()
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

    /// Deck rows in the hierarchy's unicase order (decks/decks D3).
    fn deck_rows(&self) -> Result<Vec<Deck>, StorageError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT id, name, mtime_secs, usn, common, kind FROM decks ORDER BY name")?;
        let rows = statement.query_map([], |row| rows::row_image(row, Table::Decks))?;
        rows.map(|row| Deck::from_cells(&row?).map_err(StorageError::from))
            .collect()
    }

    fn deck_config_rows(&self) -> Result<Vec<DeckConfig>, StorageError> {
        rows::select_where(self.connection(), Table::DeckConfig, "1", &[])?
            .iter()
            .map(|row| DeckConfig::from_cells(row).map_err(StorageError::from))
            .collect()
    }

    /// Includes a home deck's cards currently held in a filtered deck.
    fn cards_of_deck(&self, id: DeckId) -> Result<Vec<Card>, StorageError> {
        rows::select_where(
            self.connection(),
            Table::Cards,
            "did = ?1 OR odid = ?1",
            &[Cell::Integer(id.0)],
        )?
        .iter()
        .map(|row| Card::from_cells(row).map_err(StorageError::from))
        .collect()
    }

    /// The ids of the deck's cards (`did` or `odid`, so its cards held in a
    /// filtered deck too) whose stability reads as NULL the way
    /// `search/card-extractors` X4 reads `s`, ascending: Change Deck's
    /// memory-state set (`browser/bulk-actions` B7.2 step 4). It needs the
    /// search crate's `meguri_card_f32`, which every engine connection
    /// registers (C5).
    fn deck_cards_without_stability(&self, id: DeckId) -> Result<Vec<CardId>, StorageError> {
        Ok(self
            .connection()
            .prepare_cached(
                // `odid != 0` lets SQLite use the partial `idx_cards_odid`.
                "SELECT id FROM cards WHERE (did = ?1 OR (odid = ?1 AND odid != 0)) \
                 AND meguri_card_f32(data, 's') IS NULL ORDER BY id",
            )?
            .query_map([id.0], |row| row.get::<_, i64>(0).map(CardId))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// How many cards the listed notes that exist hold (Delete Notes'
    /// second count, `browser/bulk-actions` B11); a card whose note row is
    /// missing is not counted. Repeated ids count once.
    fn card_count_of_notes(&self, notes: &[NoteId]) -> Result<u64, StorageError> {
        let ids = serde_json::to_string(&notes.iter().map(|id| id.0).collect::<Vec<_>>())
            .expect("integer ids serialize");
        let count = self.connection().query_row(
            "SELECT count(*) FROM cards WHERE nid IN (SELECT id FROM notes WHERE id IN (SELECT value FROM json_each(?1)))",
            [ids],
            |row| row.get::<_, i64>(0),
        )?;
        Ok(u64::try_from(count).unwrap_or(0))
    }

    fn deck_has_cards(&self, id: DeckId) -> Result<bool, StorageError> {
        Ok(self
            .connection()
            .query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE did = ?1)", [id.0], |r| {
                r.get(0)
            })?)
    }

    fn deck_config(&self, id: DeckConfigId) -> Result<Option<DeckConfig>, StorageError> {
        rows::get(self.connection(), &RowKey::Id(id.0))
    }

    fn notetype(&self, id: NotetypeId) -> Result<Option<Notetype>, StorageError> {
        notetypes::get(self.connection(), id)
    }

    /// Editor defaults (K4.1): select the persisted identity in SQL unicase
    /// name order, independently of id order and without loading templates.
    fn first_notetype_id_by_name(&self) -> Result<Option<NotetypeId>, StorageError> {
        use rusqlite::OptionalExtension;
        Ok(self
            .connection()
            .query_row("SELECT id FROM notetypes ORDER BY name COLLATE unicase LIMIT 1", [], |row| {
                row.get::<_, i64>(0).map(NotetypeId)
            })
            .optional()?)
    }

    fn tag(&self, name: &str) -> Result<Option<Tag>, StorageError> {
        rows::get(self.connection(), &RowKey::text(name))
    }

    /// Stream only stored tag names in SQLite unicase order. `check` runs
    /// before preparing and before each step; Break never steps another row.
    /// Both callbacks stay synchronous and the statement closes on return.
    fn visit_tag_names<E: From<StorageError>>(
        &self,
        mut check: impl FnMut() -> Result<(), E>,
        mut each: impl FnMut(String) -> Result<std::ops::ControlFlow<()>, E>,
    ) -> Result<(), E> {
        check()?;
        let mut statement = self.connection()
            .prepare("SELECT tag FROM tags ORDER BY tag COLLATE unicase")
            .map_err(StorageError::from)?;
        let mut rows = statement.query([]).map_err(StorageError::from)?;
        loop {
            check()?;
            let Some(row) = rows.next().map_err(StorageError::from)? else { break };
            let name = row.get::<_, String>(0).map_err(StorageError::from)?;
            if each(name)?.is_break() {
                break;
            }
        }
        Ok(())
    }

    /// Every `tags` row, in the table's key order (`unicase`).
    fn tag_rows(&self) -> Result<Vec<Tag>, StorageError> {
        rows::select_where(self.connection(), Table::Tags, "1", &[])?
            .iter()
            .map(|cells| Tag::from_cells(cells).map_err(StorageError::from))
            .collect()
    }

    /// Up to `limit` `tags` rows whose `tag` is `from` or comes after it in
    /// the table's key order (`unicase`), in that order. The rows whose
    /// names fold to a text beginning with `from`'s folding come first, one
    /// after another (`tags/tags` T1's tree spelling).
    fn tag_rows_from(&self, from: &str, limit: usize) -> Result<Vec<Tag>, StorageError> {
        let mut statement = self
            .connection()
            .prepare_cached("SELECT tag, usn, collapsed, config FROM tags WHERE tag >= ?1 ORDER BY tag LIMIT ?2")?;
        let images = statement.query_map((from, limit_value(limit)), |row| rows::row_image(row, Table::Tags))?;
        let mut out = Vec::new();
        for image in images {
            out.push(Tag::from_cells(&image?)?);
        }
        Ok(out)
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

    /// Read a page of up to 1,000 stored (card id, ordinal) pairs.
    /// Filter by note type and continue after the last returned card id
    /// until an empty page; `None` includes the entire signed id domain.
    /// Only note membership and card id/ordinal columns are read. Invalid
    /// selected ordinals fail rather than being omitted or normalized.
    /// A write scope reads through its existing writer transaction.
    fn card_ordinals_of_notetype_page(
        &self,
        notetype: NotetypeId,
        after: Option<CardId>,
    ) -> Result<Vec<(CardId, u16)>, StorageError> {
        // Cards in id order, each joined to its note: a page reads from the
        // cursor on (an `id > ?` range), so a whole pass reads each card
        // once rather than from the start on every page. A pass reads the
        // whole cards table once even for a small type (about 30 ms at
        // 150k), never more.
        let read = |row: &rusqlite::Row<'_>| Ok((CardId(row.get(0)?), row.get::<_, u16>(1)?));
        let rows = match after {
            None => self
                .connection()
                .prepare_cached(
                    "SELECT cards.id, cards.ord FROM cards CROSS JOIN notes ON cards.nid=notes.id \
                     WHERE notes.mid=?1 ORDER BY cards.id LIMIT 1000",
                )?
                .query_map((notetype.0,), read)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
            Some(after) => self
                .connection()
                .prepare_cached(
                    "SELECT cards.id, cards.ord FROM cards CROSS JOIN notes ON cards.nid=notes.id \
                     WHERE notes.mid=?1 AND cards.id>?2 ORDER BY cards.id LIMIT 1000",
                )?
                .query_map((notetype.0, after.0), read)?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        };
        Ok(rows)
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

    /// The cards whose stored `queue` is one of `queues`, by id: their id,
    /// `type`, `queue` and `due` (the day clock's unbury, `sched/day-clock`
    /// D7).
    fn cards_in_queues(&self, queues: &[i64]) -> Result<Vec<CardQueueRow>, StorageError> {
        crate::day_writes::cards_in_queues(self.connection(), queues)
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

/// A JSON array of the ids, for `json_each`.
fn json_ids(ids: &[NoteId]) -> String {
    let mut list = String::with_capacity(ids.len() * 14 + 2);
    list.push('[');
    for (at, id) in ids.iter().enumerate() {
        if at > 0 {
            list.push(',');
        }
        list.push_str(&id.0.to_string());
    }
    list.push(']');
    list
}

fn limit_value(limit: usize) -> i64 {
    i64::try_from(limit).unwrap_or(i64::MAX)
}

fn notetype_parts_page(
    conn: &Connection,
    table: Table,
    notetype: NotetypeId,
    after: Option<i64>,
) -> Result<Vec<RowImage>, StorageError> {
    notetype_parts_raw_page(conn, table, notetype, after.map(Cell::Integer).as_ref())
}

fn notetype_parts_raw_page(
    conn: &Connection,
    table: Table,
    notetype: NotetypeId,
    after: Option<&Cell>,
) -> Result<Vec<RowImage>, StorageError> {
    let columns = table
        .columns()
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(",");
    let continuation = if after.is_some() { " AND ord>?2" } else { "" };
    let mut statement = conn.prepare_cached(&format!(
        "SELECT {columns} FROM {} WHERE ntid=?1{continuation} \
         AND EXISTS(SELECT 1 FROM notetypes WHERE id=?1) ORDER BY ord LIMIT {}",
        table.name(),
        crate::normal_stage::PAGE,
    ))?;
    let mut params = vec![Cell::Integer(notetype.0)];
    if let Some(ord) = after {
        params.push(ord.clone());
    }
    let mut cursor = statement.query(params_from_iter(params))?;
    let mut out = Vec::new();
    let mut bytes = 0usize;
    while let Some(row) = cursor.next()? {
        let size = crate::normal_stage::borrowed_row_size(row, table)?;
        if !out.is_empty() && bytes.saturating_add(size) > 8 * 1024 * 1024 {
            break;
        }
        out.push(rows::row_image(row, table)?);
        bytes = bytes.saturating_add(size);
    }
    Ok(out)
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

impl sealed::Connected for crate::snapshot::SyncSnapshot {
    fn connection(&self) -> &Connection {
        &self.reader.conn
    }
}
impl ReadRows for crate::snapshot::SyncSnapshot {}

impl sealed::Connected for crate::snapshot::CloudSnapshot {
    fn connection(&self) -> &Connection {
        &self.reader.conn
    }
}
impl ReadRows for crate::snapshot::CloudSnapshot {}
