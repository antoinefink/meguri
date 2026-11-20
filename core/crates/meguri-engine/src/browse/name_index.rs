//! The name index of `specs/search/editing-services.md` D6.7 for the open
//! collection: built when the collection opens (an open step) and when a
//! replacement reopens it, then kept current from each committed change
//! set (a change observer). Suggestions read [`NameIndexProvider::index`],
//! never the collection: a read clones the index as last built and waits
//! for no write and no reader.
//!
//! A change set rebuilds only the parts it touches (`Sections::of`): the
//! deck and preset lists are reread whole (they are short), tags change one
//! row at a time, a note type is reread by id, and the two config keys are
//! read again. A whole-collection change rereads everything. The observer
//! reads only the committed writer's rows it is handed.

use std::sync::{Arc, Mutex, PoisonError};

use meguri_model::changes::{ChangeKind, ChangeSet};
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::NotetypeId;
use meguri_model::tag::Tag;
use meguri_search::config::Key;
use meguri_search::name_index::{DeckRow, NameIndex, NotetypeNames, PresetName, Sections};
use meguri_storage::rows::config::stored_value;
use meguri_storage::{Cell, ConfigValue, ReadRows, ReaderRole, Record, RowError, RowImage, RowKey, StorageError, Table};

use crate::changes::observers::{ObservedCommit, ObserverRows};
use crate::lifecycle::Generation;
use crate::ports::{ChangeObserver, GenerationListener, OpenStep};
use crate::{Collection, EngineError};

const NAME: &str = "name-index";

/// Holds the index of the open collection.
#[derive(Debug, Default)]
pub struct NameIndexProvider {
    /// The index as last built; held only to clone or replace the `Arc`.
    current: Mutex<Arc<NameIndex>>,
    /// Serializes builds and updates, so none is lost to another.
    building: Mutex<()>,
}

impl NameIndexProvider {
    pub fn new() -> NameIndexProvider {
        NameIndexProvider::default()
    }

    /// The index as last built (empty before the first open).
    pub fn index(&self) -> Arc<NameIndex> {
        Arc::clone(&self.current.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn install(&self, index: NameIndex) {
        *self.current.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(index);
    }

    /// Builds the whole index from a snapshot of `collection`. A change
    /// observed meanwhile waits for it and is applied after it (applying a
    /// change the snapshot already holds again changes nothing).
    pub fn rebuild(&self, collection: &Collection) -> Result<(), EngineError> {
        let _building = self.building.lock().unwrap_or_else(PoisonError::into_inner);
        let reader = collection.reader(ReaderRole::Query)?;
        let snapshot = reader.snapshot()?;
        let mut index = NameIndex::default();
        fill(&mut index, &SnapshotRows(&*snapshot), Sections::ALL, None)?;
        drop(snapshot);
        self.install(index);
        Ok(())
    }

    /// Applies one committed change set.
    fn apply(&self, changes: &ChangeSet, rows: &ObserverRows<'_>) -> Result<(), EngineError> {
        let sections = Sections::of(changes);
        if !sections.any() {
            return Ok(());
        }
        let _building = self.building.lock().unwrap_or_else(PoisonError::into_inner);
        let mut index = (*self.index()).clone();
        let source = CommitRows(rows);
        let result = fill(&mut index, &source, sections, Some(changes));
        if result.is_err() {
            // The changed rows could not be read one by one: read everything
            // the commit holds, or keep the index as it was.
            let mut whole = NameIndex::default();
            fill(&mut whole, &source, Sections::ALL, None)?;
            index = whole;
        }
        self.install(index);
        Ok(())
    }
}

impl OpenStep for NameIndexProvider {
    fn name(&self) -> &str {
        NAME
    }
    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        self.rebuild(collection)
    }
}

impl ChangeObserver for NameIndexProvider {
    fn name(&self) -> &str {
        NAME
    }
    /// Every commit reaches [`ChangeObserver::observe_with_rows`], which
    /// reads the rows; nothing can be updated without them.
    fn observe(&self, _commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        Ok(())
    }
    fn observe_with_rows(&self, commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Result<(), EngineError> {
        self.apply(commit.changes, rows)
    }
}

impl GenerationListener for NameIndexProvider {
    fn name(&self) -> &str {
        NAME
    }
    fn collection_reopened(&self, collection: &Collection) -> Result<(), EngineError> {
        self.rebuild(collection)
    }
    fn collection_replaced(&self, _old: Generation, _new: Generation) {}
}

/// The reads the index needs, from a reader snapshot or a commit's rows.
trait Source {
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, StorageError>;
    fn image(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError>;
    fn parts(&self, table: Table, notetype: NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError>;
}

struct SnapshotRows<'a, R: ReadRows + ?Sized>(&'a R);

impl<R: ReadRows + ?Sized> Source for SnapshotRows<'_, R> {
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, StorageError> {
        self.0.row_page(table, after)
    }
    fn image(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
        self.0.row_image(table, key)
    }
    fn parts(&self, table: Table, notetype: NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError> {
        match table {
            Table::Fields => self.0.notetype_fields_page(notetype, after),
            _ => self.0.notetype_templates_page(notetype, after),
        }
    }
}

struct CommitRows<'a, 'b>(&'a ObserverRows<'b>);

impl Source for CommitRows<'_, '_> {
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, StorageError> {
        self.0.row_page(table, after)
    }
    fn image(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
        self.0.row_image(table, key)
    }
    fn parts(&self, table: Table, notetype: NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError> {
        match table {
            Table::Fields => self.0.notetype_fields_page(notetype, after),
            _ => self.0.notetype_templates_page(notetype, after),
        }
    }
}

/// Every stored row of `table`, page by page.
fn all_rows(source: &dyn Source, table: Table) -> Result<Vec<RowImage>, StorageError> {
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let page = source.page(table, after.as_ref())?;
        let Some(last) = page.last() else { break };
        after = Some(RowKey::from_stored(table, last)?);
        rows.extend(page);
    }
    Ok(rows)
}

/// Rebuilds `sections` of `index`. With `changes`, tags and note types are
/// updated by the rows the change set lists; without, they are reread.
fn fill(
    index: &mut NameIndex,
    source: &dyn Source,
    sections: Sections,
    changes: Option<&ChangeSet>,
) -> Result<(), StorageError> {
    if sections.decks {
        let decks = all_rows(source, Table::Decks)?
            .iter()
            .map(|cells| {
                let deck = Deck::from_cells(cells)?;
                Ok(DeckRow {
                    filtered: crate::decks::is_filtered(&deck),
                    id: deck.id,
                    native: deck.name,
                })
            })
            .collect::<Result<_, StorageError>>()?;
        index.set_decks(decks);
    }
    if sections.presets {
        let presets = all_rows(source, Table::DeckConfig)?
            .iter()
            .map(|cells| {
                let preset = DeckConfig::from_cells(cells)?;
                Ok(PresetName {
                    id: preset.id,
                    name: preset.name,
                })
            })
            .collect::<Result<_, StorageError>>()?;
        index.set_presets(presets);
    }
    if sections.tags {
        match changes {
            Some(changes) if !changes.whole_collection => {
                for change in &changes.tags {
                    match change.kind {
                        ChangeKind::Removed => index.remove_tag(&change.name),
                        ChangeKind::Added | ChangeKind::Updated => index.put_tag(&change.name),
                    }
                }
            }
            _ => {
                let tags = all_rows(source, Table::Tags)?
                    .iter()
                    .map(|cells| Ok(Tag::from_cells(cells)?.name))
                    .collect::<Result<_, StorageError>>()?;
                index.set_tags(tags);
            }
        }
    }
    if sections.notetypes {
        match changes {
            Some(changes) if !changes.whole_collection => {
                for change in &changes.notetypes {
                    match notetype(source, change.id)? {
                        Some(names) => index.put_notetype(names),
                        None => index.remove_notetype(change.id),
                    }
                }
            }
            _ => {
                let mut notetypes = Vec::new();
                for cells in all_rows(source, Table::Notetypes)? {
                    let id = NotetypeId(integer(Table::Notetypes, &cells, 0)?);
                    if let Some(names) = notetype(source, id)? {
                        notetypes.push(names);
                    }
                }
                index.set_notetypes(notetypes);
            }
        }
    }
    if sections.saved_searches {
        index.set_saved_searches(config(source, Key::SavedFilters)?.as_ref());
    }
    if sections.flag_names {
        let labels = config(source, Key::FlagLabels)?;
        index.set_flag_names(std::array::from_fn(|k| {
            crate::flags::shown_name(labels.as_ref(), (k + 1) as u8)
        }));
    }
    Ok(())
}

/// A config key's value read per C3: `None` when missing or not JSON.
fn config(source: &dyn Source, key: Key) -> Result<Option<ConfigValue>, StorageError> {
    Ok(source
        .image(Table::Config, &RowKey::text(key.name()))?
        .and_then(|cells| cells.get(3).and_then(stored_value)))
}

/// A note type's name and its template and field names by ordinal; `None`
/// when it no longer exists.
fn notetype(source: &dyn Source, id: NotetypeId) -> Result<Option<NotetypeNames>, StorageError> {
    let Some(head) = source.image(Table::Notetypes, &RowKey::Id(id.0))? else {
        return Ok(None);
    };
    Ok(Some(NotetypeNames {
        id,
        name: text(Table::Notetypes, &head, 1)?,
        templates: part_names(source, Table::Templates, id)?,
        fields: part_names(source, Table::Fields, id)?,
    }))
}

/// The names of a note type's fields or templates, in `ord` order.
fn part_names(source: &dyn Source, table: Table, id: NotetypeId) -> Result<Vec<String>, StorageError> {
    let mut names = Vec::new();
    let mut after = None;
    loop {
        let page = source.parts(table, id, after)?;
        if page.is_empty() {
            return Ok(names);
        }
        for cells in &page {
            after = Some(integer(table, cells, 1)?);
            names.push(text(table, cells, 2)?);
        }
    }
}

fn bad(table: Table, column: usize, reason: &str) -> StorageError {
    RowError {
        table: table.name(),
        column: table.columns()[column],
        key: String::new(),
        reason: reason.into(),
    }
    .into()
}

fn integer(table: Table, cells: &[Cell], column: usize) -> Result<i64, StorageError> {
    match cells.get(column) {
        Some(Cell::Integer(value)) => Ok(*value),
        _ => Err(bad(table, column, "expected an integer")),
    }
}

fn text(table: Table, cells: &[Cell], column: usize) -> Result<String, StorageError> {
    match cells.get(column) {
        Some(Cell::Text(bytes)) => {
            String::from_utf8(bytes.clone()).map_err(|_| bad(table, column, "invalid UTF-8 text"))
        }
        _ => Err(bad(table, column, "expected text")),
    }
}
