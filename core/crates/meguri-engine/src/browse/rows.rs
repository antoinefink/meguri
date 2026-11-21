//! Browse row records (`specs/browser/columns.md` C3 to C10): for each id
//! of a request, its state, tint, font and the requested cells, or its row
//! error. One read transaction on the query reader; nothing is written.
//! Cell values come from meguri-search's `browser::cells`; this module reads
//! the records and resolves names and templates. Sessions, paging and the
//! deferred Question and Answer cells are U85's (`browse::search`).

use meguri_model::card::Card;
use meguri_model::card_data::extractors::Fields;
use meguri_model::deck::Deck;
use meguri_model::ids::{CardId, NoteId};
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeTemplate};
use meguri_search::SearchMode;
use meguri_search::browser::cells::{self, Cell, Font, RowState, RowTime, Tint};
use meguri_search::browser::{deck_cell, qa};
use meguri_search::config::Column;
use meguri_storage::rows::{Cell as Stored, Record, RowError, RowKey, Table};
use meguri_storage::{ReadRows, ReaderRole, StorageError};
use meguri_template::card::CardRender;

use crate::{Collection, EngineError, rendering};

/// One row request (C3): the mode, the ids (card ids in cards mode, note ids
/// in notes mode), the columns in the order their cells are wanted, and the
/// now and today every row uses (C1).
#[derive(Clone, Copy, Debug)]
pub struct RowRequest<'a> {
    pub mode: SearchMode,
    pub ids: &'a [i64],
    pub columns: &'a [Column],
    pub time: RowTime,
}

/// A row record (C3).
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The card id in cards mode, the note id in notes mode.
    pub id: i64,
    pub note_id: NoteId,
    pub state: RowState,
    pub tint: Tint,
    /// C6: of the card's template, or of the first card's in notes mode.
    pub font: Font,
    /// C6: the Sort Field cell is laid out right to left.
    pub sort_field_rtl: bool,
    /// One per requested column, in request order.
    pub cells: Vec<Cell>,
}

/// C3.1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowFailure {
    /// The row's own record no longer exists.
    Deleted,
    /// A record the row needs is missing, or the note has no card.
    Damaged,
    /// A record exists but does not decode: the value that failed.
    Unreadable {
        table: &'static str,
        column: &'static str,
        row: String,
    },
}

pub type RowResult = Result<Row, RowFailure>;

/// C3: a row record or a row error per id, in request order. A row error
/// never stops the other rows; a storage failure that is not a decoding
/// failure fails the whole call.
pub fn rows(collection: &Collection, request: &RowRequest<'_>) -> Result<Vec<RowResult>, EngineError> {
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    rows_in(collection, &*snapshot, request, &mut || Ok(()))
}

/// [`rows`] on a snapshot the caller holds; `between` runs before each row
/// and stops the call with its error (`browser/result-sets` H7's
/// cancellation between rows).
pub(crate) fn rows_in<R: ReadRows + ?Sized>(
    collection: &Collection,
    rows: &R,
    request: &RowRequest<'_>,
    between: &mut dyn FnMut() -> Result<(), EngineError>,
) -> Result<Vec<RowResult>, EngineError> {
    let reading = Reading {
        collection,
        rows,
        request,
    };
    request
        .ids
        .iter()
        .map(|&id| {
            between()?;
            let row = match request.mode {
                SearchMode::Cards => reading.card_row(CardId(id)),
                SearchMode::Notes => reading.note_row(NoteId(id)),
            };
            match row {
                Ok(row) => Ok(Ok(row)),
                Err(Fail::Row(failure)) => Ok(Err(failure)),
                Err(Fail::Engine(error)) => Err(error),
            }
        })
        .collect()
}

enum Fail {
    Row(RowFailure),
    Engine(EngineError),
}

fn unreadable(error: &RowError) -> Fail {
    Fail::Row(RowFailure::Unreadable {
        table: error.table,
        column: error.column,
        row: error.key.clone(),
    })
}

impl From<StorageError> for Fail {
    fn from(error: StorageError) -> Fail {
        match error {
            StorageError::Row(error) => unreadable(&error),
            other => Fail::Engine(other.into()),
        }
    }
}

impl From<EngineError> for Fail {
    fn from(error: EngineError) -> Fail {
        match error {
            EngineError::Storage(storage) => storage.into(),
            other => Fail::Engine(other),
        }
    }
}

fn damaged<T>(record: Option<T>) -> Result<T, Fail> {
    record.ok_or(Fail::Row(RowFailure::Damaged))
}

/// The records one row reads, its own card (the first card in notes mode)
/// and the card data `browser/columns` C1 reads.
struct Records<'r> {
    note: Note,
    kind: Notetype,
    card: Card,
    data: Fields,
    deck: Deck,
    home: Option<Deck>,
    /// Notes mode: every card of the note, by id.
    cards: &'r [Card],
}

struct Reading<'a, R: ReadRows + ?Sized> {
    collection: &'a Collection,
    rows: &'a R,
    request: &'a RowRequest<'a>,
}

impl<R: ReadRows + ?Sized> Reading<'_, R> {
    /// The card and its X3 card data from one stored row.
    fn card(&self, id: CardId) -> Result<Option<(Card, Fields)>, Fail> {
        let Some(image) = self.rows.row_image(Table::Cards, &RowKey::Id(id.0))? else {
            return Ok(None);
        };
        let card = Card::from_cells(&image).map_err(|error| unreadable(&error))?;
        let data = Table::Cards
            .columns()
            .iter()
            .position(|column| *column == "data")
            .and_then(|index| match &image[index] {
                Stored::Text(bytes) => std::str::from_utf8(bytes).ok().map(Fields::parse),
                _ => None,
            })
            .unwrap_or_default();
        Ok(Some((card, data)))
    }

    /// C3.1's reading order after the row's own record and the note: the
    /// note type, the deck, the home deck and the template.
    fn records<'r>(&self, note: Note, card: Card, data: Fields, cards: &'r [Card]) -> Result<Records<'r>, Fail> {
        let kind = damaged(self.rows.notetype(note.notetype_id)?)?;
        let deck = damaged(self.rows.deck(card.deck_id)?)?;
        let home = if cells::is_filtered(&card) {
            Some(damaged(self.rows.deck(card.original_deck_id)?)?)
        } else {
            None
        };
        damaged(template(&kind, &card))?;
        Ok(Records {
            note,
            kind,
            card,
            data,
            deck,
            home,
            cards,
        })
    }

    fn card_row(&self, id: CardId) -> Result<Row, Fail> {
        let (card, data) = self.card(id)?.ok_or(Fail::Row(RowFailure::Deleted))?;
        let note = damaged(self.rows.note(card.note_id)?)?;
        let records = self.records(note, card, data, &[])?;
        let state = cells::card_state(&records.card, &records.note.tags);
        let tint = cells::card_tint(&records.card, &state);
        self.row(id.0, &records, state, tint)
    }

    fn note_row(&self, id: NoteId) -> Result<Row, Fail> {
        let note = self.rows.note(id)?.ok_or(Fail::Row(RowFailure::Deleted))?;
        let mut cards = self.rows.cards_of_note(id)?;
        cards.sort_by_key(|card| card.id);
        let first = damaged(cards.first())?.clone();
        let data = if self.request.columns.iter().any(|column| reads_card_data(*column)) {
            self.card(first.id)?.map(|(_, data)| data).unwrap_or_default()
        } else {
            Fields::default()
        };
        let records = self.records(note, first, data, &cards)?;
        let state = cells::note_state(&records.note.tags);
        let tint = cells::note_tint(&state);
        self.row(id.0, &records, state, tint)
    }

    fn row(&self, id: i64, records: &Records<'_>, state: RowState, tint: Tint) -> Result<Row, Fail> {
        let template = template(&records.kind, &records.card).expect("checked by records");
        let rendered = if self
            .request
            .columns
            .iter()
            .any(|column| matches!(column, Column::Question | Column::Answer))
        {
            Some(self.render(records, template)?)
        } else {
            None
        };
        let cells = self
            .request
            .columns
            .iter()
            .map(|column| self.cell(*column, records, rendered.as_ref()))
            .collect();
        Ok(Row {
            id,
            note_id: records.note.id,
            state,
            tint,
            font: Font::of(&template.config.browser_font_name, template.config.browser_font_size),
            // C6: the field at index `sortf`, as the Sort Field cell reads it.
            sort_field_rtl: records
                .kind
                .fields
                .get(records.kind.config.sort_field_idx as usize)
                .is_some_and(|field| field.config.rtl),
            cells,
        })
    }

    /// C10 step 1: the browser render (K5); it never writes (step 7).
    fn render(&self, records: &Records<'_>, template: &NotetypeTemplate) -> Result<CardRender, Fail> {
        let (rendered, _) = rendering::render_stored_in(
            self.collection,
            self.rows,
            &records.card,
            &records.note,
            &records.kind,
            template,
            rendering::RenderMode::Browser,
        )?;
        Ok(rendered)
    }

    /// C7 in cards mode, C8 in notes mode.
    fn cell(&self, column: Column, records: &Records<'_>, rendered: Option<&CardRender>) -> Cell {
        let notes = self.request.mode == SearchMode::Notes;
        let time = self.request.time;
        let (card, note, data) = (&records.card, &records.note, &records.data);
        match column {
            Column::SortField => Cell::Text(cells::sort_field(note, records.kind.config.sort_field_idx)),
            Column::Question => Cell::Question(qa::question(rendered.expect("rendered for Question"))),
            Column::Answer => Cell::Answer(qa::answer(rendered.expect("rendered for Answer"))),
            Column::CardType if notes => Cell::Count(records.cards.len() as u64),
            Column::CardType => {
                let template = template(&records.kind, card).expect("checked by records");
                Cell::Text(cells::card_type(&template.name, records.kind.config.kind == 1, card.ord))
            }
            Column::Deck if notes && cells::notes::distinct_decks(records.cards) > 1 => {
                Cell::DeckCount(cells::notes::distinct_decks(records.cards))
            }
            Column::Deck => Cell::Text(deck_cell::cards(
                &records.deck.human_name(),
                records.home.as_ref().map(Deck::human_name).as_deref(),
            )),
            Column::Due if notes => cells::notes::due(records.cards, time),
            Column::Due => cells::due(card, time),
            Column::Ease if notes => cells::notes::ease(records.cards),
            Column::Ease => cells::ease(card),
            Column::Interval if notes => cells::notes::interval(records.cards),
            Column::Interval => cells::interval(card),
            Column::Reviews if notes => cells::notes::reviews(records.cards),
            Column::Reviews => Cell::Count(u64::from(card.reps)),
            Column::Lapses if notes => cells::notes::lapses(records.cards),
            Column::Lapses => Cell::Count(u64::from(card.lapses)),
            Column::Created => cells::created(note.id.0),
            Column::CardModified if notes => cells::notes::card_modified(records.cards),
            Column::CardModified => Cell::DateTime(card.mtime.0),
            Column::NoteModified => Cell::DateTime(note.mtime.0),
            Column::Notetype => Cell::Text(records.kind.name.clone()),
            Column::Tags => Cell::Text(cells::tags(&note.tags)),
            Column::Position => cells::position(card, data),
            Column::Stability => cells::stability(data),
            Column::Difficulty => cells::difficulty(data),
            Column::Retrievability => cells::retrievability(card, data, time),
        }
    }
}

/// C1: a standard note type's template at the card's ordinal; a cloze note
/// type's first template.
fn template<'k>(kind: &'k Notetype, card: &Card) -> Option<&'k NotetypeTemplate> {
    if kind.config.kind == 1 {
        kind.templates.first()
    } else {
        kind.templates.iter().find(|template| template.ord == u32::from(card.ord))
    }
}

fn reads_card_data(column: Column) -> bool {
    matches!(
        column,
        Column::Position | Column::Stability | Column::Difficulty | Column::Retrievability
    )
}
