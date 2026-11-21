//! Browse's column sets (`specs/browser/workspace.md` V2.2 to V2.4, V4.3):
//! the Meguri key `meguriBrowserColumnSets` read per `storage/config-table`
//! C3, and the whole value each user change writes. Nothing here writes:
//! meguri-engine's `browser_config::column_sets` applies the value through C4.
//!
//! Anki's column and sort keys (`activeCols`, `activeNoteCols`, `sortType`,
//! `sortBackwards`, `noteSortType`, `browserNoteSortBackwards`) take no part:
//! when this key holds nothing, V2.3's defaults apply, whatever Anki stored.
//!
//! Shapes. The key is an object; member `std` is the standard set and a
//! member named by a note type id in decimal is that note type's own set.
//! A set is an object; its `cards` and `notes` are arrays of column keys,
//! and its `sort` is an object whose `cards` and `notes` are
//! `[<column key>, <descending>]`. A list reads as its known column keys in
//! order, each at its first occurrence: an entry that names no column (one
//! a newer Meguri knows) is skipped, so the layout survives on an older
//! device. A list with no known key, and any member of another shape, reads
//! as missing. A member Meguri does not know is kept as it is. A write to a
//! set stores each of its lists as it reads (decided 2026-11-21 by the
//! coordinator; the spec sentence is pending).

use meguri_model::ids::NotetypeId;
use meguri_storage::ConfigValue;

use super::{Column, ColumnKey, Refusal, SortDirection};
use crate::plan::SearchMode;
use crate::plan::sort::Sort;

/// The key's name in the `config` table.
pub const KEY: &str = "meguriBrowserColumnSets";

/// The standard set's member name.
const STANDARD: &str = "std";
/// A set's sort member (V4.3).
const SORT: &str = "sort";

/// The member that holds a mode's list, or a mode's sort inside `sort`.
fn mode_member(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Cards => "cards",
        SearchMode::Notes => "notes",
    }
}

const MODES: [SearchMode; 2] = [SearchMode::Cards, SearchMode::Notes];

/// Which set (V2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SetId {
    Standard,
    /// A note type's own set.
    Notetype(NotetypeId),
}

impl SetId {
    /// The set's member name in the key's object (V2.4).
    pub fn member(self) -> String {
        match self {
            SetId::Standard => STANDARD.to_owned(),
            SetId::Notetype(id) => id.0.to_string(),
        }
    }
}

/// V2.3: the default standard set's list of `mode`.
pub fn default_columns(mode: SearchMode) -> Vec<ColumnKey> {
    let columns: &[Column] = match mode {
        SearchMode::Cards => &[
            Column::SortField,
            Column::CardType,
            Column::Deck,
            Column::Due,
            Column::CardModified,
        ],
        SearchMode::Notes => &[
            Column::SortField,
            Column::Notetype,
            Column::CardType,
            Column::Tags,
            Column::Created,
        ],
    };
    columns.iter().copied().map(ColumnKey::Column).collect()
}

/// V4.3: the sort of a set that stores none, and of a stored sort that
/// cannot sort in its mode.
pub const DEFAULT_SORT: SetSort = SetSort {
    column: ColumnKey::Column(Column::SortField),
    direction: SortDirection::Ascending,
};

/// One mode's sort of a set (V4.3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SetSort {
    pub column: ColumnKey,
    pub direction: SortDirection,
}

impl SetSort {
    /// The stored form, `[<column key>, <descending>]`.
    fn value(&self) -> ConfigValue {
        ConfigValue::Array(vec![
            ConfigValue::from(self.column.key().as_str()),
            ConfigValue::Bool(self.direction.descending()),
        ])
    }
}

/// A mode's list as it reads: its known column keys in order, each at its
/// first occurrence, entries that name no column skipped; `None` when the
/// member is missing, is not an array or holds no known key.
fn stored_columns(value: Option<&ConfigValue>) -> Option<Vec<ColumnKey>> {
    let ConfigValue::Array(items) = value? else {
        return None;
    };
    let mut columns: Vec<ColumnKey> = Vec::with_capacity(items.len());
    for column in items.iter().filter_map(|item| ColumnKey::parse(item.as_str()?)) {
        if !columns.contains(&column) {
            columns.push(column);
        }
    }
    (!columns.is_empty()).then_some(columns)
}

/// A mode's sort entry as stored, `[<text>, <bool>]`: `None` when it is
/// missing or of another shape. The text need not name a column that sorts.
fn stored_sort(value: Option<&ConfigValue>) -> Option<(&str, bool)> {
    match value? {
        ConfigValue::Array(items) => match items.as_slice() {
            [key, descending] => Some((key.as_str()?, descending.as_bool()?)),
            _ => None,
        },
        _ => None,
    }
}

/// An object value, or `None` for any other value.
fn object(value: Option<&ConfigValue>) -> Option<&ConfigValue> {
    value.filter(|value| matches!(value, ConfigValue::Object(_)))
}

/// A set as a list uses it: each mode's columns, in order, and its sort.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnSet {
    cards: Vec<ColumnKey>,
    notes: Vec<ColumnKey>,
    cards_sort: SetSort,
    notes_sort: SetSort,
}

impl ColumnSet {
    /// The set an object stores, each missing or misshapen member read as
    /// its default; `None` reads as a set that stores nothing.
    fn from_stored(set: Option<&ConfigValue>) -> ColumnSet {
        let member = |name: &str| set.and_then(|set| set.get(name));
        let sort = object(member(SORT));
        let sort_of = |mode: SearchMode| {
            stored_sort(sort.and_then(|sort| sort.get(mode_member(mode))))
                .and_then(|(key, descending)| {
                    let column = ColumnKey::parse(key).filter(|column| column.sortable(mode))?;
                    let direction = if descending {
                        SortDirection::Descending
                    } else {
                        SortDirection::Ascending
                    };
                    Some(SetSort { column, direction })
                })
                .unwrap_or(DEFAULT_SORT)
        };
        ColumnSet {
            cards: stored_columns(member(mode_member(SearchMode::Cards)))
                .unwrap_or_else(|| default_columns(SearchMode::Cards)),
            notes: stored_columns(member(mode_member(SearchMode::Notes)))
                .unwrap_or_else(|| default_columns(SearchMode::Notes)),
            cards_sort: sort_of(SearchMode::Cards),
            notes_sort: sort_of(SearchMode::Notes),
        }
    }

    /// The mode's columns, in order; never empty. The state column (V2.5)
    /// is not in the list.
    pub fn columns(&self, mode: SearchMode) -> &[ColumnKey] {
        match mode {
            SearchMode::Cards => &self.cards,
            SearchMode::Notes => &self.notes,
        }
    }

    /// The mode's sort, always one that sorts in the mode (V4.3: a stored
    /// column that the list no longer shows still sorts).
    pub fn sort(&self, mode: SearchMode) -> &SetSort {
        match mode {
            SearchMode::Cards => &self.cards_sort,
            SearchMode::Notes => &self.notes_sort,
        }
    }

    /// The sort request of the mode's sort (`browser/sorting` S1).
    pub fn sort_request(&self, mode: SearchMode) -> Sort {
        let sort = self.sort(mode);
        Sort::new(mode, &sort.column.key(), sort.direction).expect("a set's sort always sorts in its mode")
    }

    fn columns_mut(&mut self, mode: SearchMode) -> &mut Vec<ColumnKey> {
        match mode {
            SearchMode::Cards => &mut self.cards,
            SearchMode::Notes => &mut self.notes,
        }
    }

    fn sort_mut(&mut self, mode: SearchMode) -> &mut SetSort {
        match mode {
            SearchMode::Cards => &mut self.cards_sort,
            SearchMode::Notes => &mut self.notes_sort,
        }
    }

    fn columns_value(&self, mode: SearchMode) -> ConfigValue {
        ConfigValue::Array(
            self.columns(mode)
                .iter()
                .map(|column| ConfigValue::from(column.key().as_str()))
                .collect(),
        )
    }

    /// The whole set as stored, every known member written.
    fn value(&self) -> ConfigValue {
        let sort = ConfigValue::Object(
            MODES
                .iter()
                .map(|&mode| (mode_member(mode).to_owned(), self.sort(mode).value()))
                .collect(),
        );
        let mut members: Vec<(String, ConfigValue)> = MODES
            .iter()
            .map(|&mode| (mode_member(mode).to_owned(), self.columns_value(mode)))
            .collect();
        members.push((SORT.to_owned(), sort));
        ConfigValue::Object(members)
    }
}

/// A user change to one mode of a set (V2.3, V4.1, V4.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Checking a column in the header menu: it is added at the end.
    Show(ColumnKey),
    /// Unchecking a column; refused `last_column` for the only one (R1133).
    Hide(ColumnKey),
    /// Dragging a header: the list in this order, each column once; refused
    /// `invalid_order` otherwise (R1135).
    Reorder(Vec<ColumnKey>),
    /// A header click (V4.1): the column in its first direction, or the
    /// current sort reversed; refused `unsortable_column`.
    SortBy(ColumnKey),
    /// The touch Sort By menu: this column and direction; refused
    /// `unsortable_column`.
    SetSort(ColumnKey, SortDirection),
}

impl Change {
    /// The refusal that does not depend on what is stored: a sort by a
    /// column that does not sort in `mode` (S1).
    pub fn check(&self, mode: SearchMode) -> Result<(), Refusal> {
        match self {
            Change::SortBy(column) | Change::SetSort(column, _) if !column.sortable(mode) => {
                Err(Refusal::UnsortableColumn)
            }
            _ => Ok(()),
        }
    }

    /// The set after the change; `None` when the change leaves it as it is.
    fn apply(&self, set: &ColumnSet, mode: SearchMode) -> Result<Option<ColumnSet>, Refusal> {
        self.check(mode)?;
        let mut next = set.clone();
        let columns = set.columns(mode);
        match self {
            Change::Show(column) => {
                if columns.contains(column) {
                    return Ok(None);
                }
                next.columns_mut(mode).push(column.clone());
            }
            Change::Hide(column) => {
                if !columns.contains(column) {
                    return Ok(None);
                }
                if columns.len() == 1 {
                    return Err(Refusal::LastColumn);
                }
                next.columns_mut(mode).retain(|c| c != column);
            }
            Change::Reorder(order) => {
                let permutation = order.len() == columns.len()
                    && columns
                        .iter()
                        .all(|column| order.iter().filter(|c| *c == column).count() == 1);
                if !permutation {
                    return Err(Refusal::InvalidOrder);
                }
                next.columns_mut(mode).clone_from(order);
            }
            Change::SortBy(column) => {
                let current = set.sort(mode);
                let direction = if current.column == *column {
                    current.direction.reversed()
                } else {
                    column.first_direction().ok_or(Refusal::UnsortableColumn)?
                };
                *next.sort_mut(mode) = SetSort {
                    column: column.clone(),
                    direction,
                };
            }
            Change::SetSort(column, direction) => {
                *next.sort_mut(mode) = SetSort {
                    column: column.clone(),
                    direction: *direction,
                };
            }
        }
        Ok((next != *set).then_some(next))
    }
}

/// A stored set's object with the member `change` concerns set from `next`,
/// every other member kept.
fn with_change(set: &ConfigValue, next: &ColumnSet, mode: SearchMode, change: &Change) -> ConfigValue {
    match change {
        Change::SortBy(_) | Change::SetSort(..) => {
            let empty = ConfigValue::Object(Vec::new());
            let sort = object(set.get(SORT))
                .unwrap_or(&empty)
                .with_member(mode_member(mode), next.sort(mode).value());
            set.with_member(SORT, sort)
        }
        Change::Show(_) | Change::Hide(_) | Change::Reorder(_) => {
            set.with_member(mode_member(mode), next.columns_value(mode))
        }
    }
}

/// A stored set as a write stores it: each list that is stored as it
/// reads, each other known member of another shape replaced by what it
/// reads as (a list's unknown and repeated keys are dropped then); missing
/// and unknown members as they are (V2.4).
fn repaired(stored: &ConfigValue) -> ConfigValue {
    let read = ColumnSet::from_stored(Some(stored));
    let mut value = stored.clone();
    for mode in MODES {
        let name = mode_member(mode);
        let as_read = read.columns_value(mode);
        if let Some(member) = stored.get(name)
            && *member != as_read
        {
            value = value.with_member(name, as_read);
        }
    }
    match stored.get(SORT) {
        None => {}
        Some(sort) if object(Some(sort)).is_none() => {
            let fixed = ConfigValue::Object(
                MODES
                    .iter()
                    .map(|&mode| (mode_member(mode).to_owned(), read.sort(mode).value()))
                    .collect(),
            );
            value = value.with_member(SORT, fixed);
        }
        Some(sort) => {
            let mut fixed = sort.clone();
            for mode in MODES {
                let name = mode_member(mode);
                if let Some(entry) = sort.get(name)
                    && stored_sort(Some(entry)).is_none()
                {
                    fixed = fixed.with_member(name, DEFAULT_SORT.value());
                }
            }
            value = value.with_member(SORT, fixed);
        }
    }
    value
}

/// The key as one collection holds it, read per C3 (V2.4).
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnSets {
    stored: Option<ConfigValue>,
}

impl ColumnSets {
    /// From the key's value read per C3 (`None`: missing or unreadable).
    pub fn from_stored(stored: Option<&ConfigValue>) -> ColumnSets {
        ColumnSets {
            stored: stored.cloned(),
        }
    }

    /// The stored object of `set`; `None` when it is missing or of another
    /// shape.
    fn stored_set(&self, set: SetId) -> Option<&ConfigValue> {
        object(object(self.stored.as_ref())?.get(&set.member()))
    }

    /// The standard set, V2.3's defaults where it stores nothing.
    pub fn standard(&self) -> ColumnSet {
        ColumnSet::from_stored(self.stored_set(SetId::Standard))
    }

    /// Whether the note type has an own set (V2.2). The caller checks that
    /// the note type exists: a set whose note type was removed is ignored
    /// (V2.4).
    pub fn has_own(&self, notetype: NotetypeId) -> bool {
        self.stored_set(SetId::Notetype(notetype)).is_some()
    }

    /// The set a list of `set` shows (V2.2): a note type without an own set
    /// uses the standard set, live.
    pub fn set(&self, set: SetId) -> ColumnSet {
        match set {
            SetId::Notetype(id) if !self.has_own(id) => self.standard(),
            _ => ColumnSet::from_stored(self.stored_set(set)),
        }
    }

    /// The whole value to write for `change` to `set` in `mode`; `None`
    /// when it changes nothing (R1138). Only `set`'s member changes; every
    /// other member of the key is kept. A note type's first change writes
    /// its own set as a copy of the standard set with the change applied
    /// (V2.2).
    pub fn change(&self, set: SetId, mode: SearchMode, change: &Change) -> Result<Option<ConfigValue>, Refusal> {
        let Some(next) = change.apply(&self.set(set), mode)? else {
            return Ok(None);
        };
        let value = match (self.stored_set(set), set) {
            (Some(stored), _) => with_change(&repaired(stored), &next, mode, change),
            (None, SetId::Standard) => with_change(&ConfigValue::Object(Vec::new()), &next, mode, change),
            (None, SetId::Notetype(_)) => next.value(),
        };
        Ok(Some(self.with_set(set, value)))
    }

    /// Use Standard Columns (V2.2): the note type's own set becomes a copy
    /// of the standard set (its columns and sorts; its unknown members
    /// kept). `None` when the note type has no own set.
    pub fn use_standard(&self, notetype: NotetypeId) -> Option<ConfigValue> {
        let stored = self.stored_set(SetId::Notetype(notetype))?;
        // The own set's unknown members are kept, in the set and in its
        // `sort` (V2.4, C6); the standard set's are not copied.
        let standard = self.standard();
        let empty = ConfigValue::Object(Vec::new());
        let mut sort = object(stored.get(SORT)).unwrap_or(&empty).clone();
        let mut copy = stored.clone();
        for mode in MODES {
            sort = sort.with_member(mode_member(mode), standard.sort(mode).value());
            copy = copy.with_member(mode_member(mode), standard.columns_value(mode));
        }
        Some(self.with_set(SetId::Notetype(notetype), copy.with_member(SORT, sort)))
    }

    /// The key's value with `set`'s member replaced; a stored value that is
    /// not an object counts as an empty one.
    fn with_set(&self, set: SetId, value: ConfigValue) -> ConfigValue {
        let empty = ConfigValue::Object(Vec::new());
        object(self.stored.as_ref())
            .unwrap_or(&empty)
            .with_member(&set.member(), value)
    }
}
