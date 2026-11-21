//! The browser and search configuration keys (`specs/browser/config-keys.md`):
//! one typed accessor per key, read the way K2 to K7 say, and the values each
//! user action writes (K4.2, K4.3, K5.2, K7.2). Nothing here writes: the
//! engine's browser config ops (`meguri_engine::browser_config`) apply the
//! values computed here through `storage/config-table` C4.
//!
//! Reads never fail and never write (K2, R1138): a key that is missing,
//! unreadable (C3) or of another shape reads as its K1 default, and the
//! default is never stored.

pub mod column_sets;

use std::fmt;

use meguri_storage::{ConfigValue, ReadRows, StorageError};

use crate::plan::SearchMode;
use crate::plan::sort::FIELD_COLUMN_PREFIX;

/// The keys of K1, in K1's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    ActiveCols,
    ActiveNoteCols,
    SortType,
    SortBackwards,
    NoteSortType,
    BrowserNoteSortBackwards,
    BrowserTableShowNotesMode,
    SavedFilters,
    FlagLabels,
    CollapseSavedSearches,
    CollapseToday,
    CollapseFlags,
    CollapseCardState,
    CollapseDecks,
    CollapseNotetypes,
    CollapseTags,
    PreviewBothSides,
    SetDueBrowser,
    DefaultSearchText,
    IgnoreAccentsInSearch,
    RestorePositionBrowser,
    ResetCountsBrowser,
    NormalizeNoteText,
}

/// The JSON shape of a key and how it reads (K1, K2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// A JSON boolean; anything else reads as this default (K2.1).
    Boolean(bool),
    /// A JSON string; anything else reads as this default (K2.2).
    Text(&'static str),
    /// A column key of one mode's sort (K5).
    SortColumn,
    /// An array of column keys (K4).
    Columns,
    /// An object of names to texts (K7, K8).
    Names,
}

impl Key {
    pub const ALL: [Key; 23] = [
        Key::ActiveCols,
        Key::ActiveNoteCols,
        Key::SortType,
        Key::SortBackwards,
        Key::NoteSortType,
        Key::BrowserNoteSortBackwards,
        Key::BrowserTableShowNotesMode,
        Key::SavedFilters,
        Key::FlagLabels,
        Key::CollapseSavedSearches,
        Key::CollapseToday,
        Key::CollapseFlags,
        Key::CollapseCardState,
        Key::CollapseDecks,
        Key::CollapseNotetypes,
        Key::CollapseTags,
        Key::PreviewBothSides,
        Key::SetDueBrowser,
        Key::DefaultSearchText,
        Key::IgnoreAccentsInSearch,
        Key::RestorePositionBrowser,
        Key::ResetCountsBrowser,
        Key::NormalizeNoteText,
    ];

    /// The key's name in the `config` table, case included.
    pub fn name(self) -> &'static str {
        match self {
            Key::ActiveCols => "activeCols",
            Key::ActiveNoteCols => "activeNoteCols",
            Key::SortType => "sortType",
            Key::SortBackwards => "sortBackwards",
            Key::NoteSortType => "noteSortType",
            Key::BrowserNoteSortBackwards => "browserNoteSortBackwards",
            Key::BrowserTableShowNotesMode => "browserTableShowNotesMode",
            Key::SavedFilters => "savedFilters",
            Key::FlagLabels => "flagLabels",
            Key::CollapseSavedSearches => "collapseSavedSearches",
            Key::CollapseToday => "collapseToday",
            Key::CollapseFlags => "collapseFlags",
            Key::CollapseCardState => "collapseCardState",
            Key::CollapseDecks => "collapseDecks",
            Key::CollapseNotetypes => "collapseNotetypes",
            Key::CollapseTags => "collapseTags",
            Key::PreviewBothSides => "previewBothSides",
            Key::SetDueBrowser => "setDueBrowser",
            Key::DefaultSearchText => "defaultSearchText",
            Key::IgnoreAccentsInSearch => "ignoreAccentsInSearch",
            Key::RestorePositionBrowser => "restorePositionBrowser",
            Key::ResetCountsBrowser => "resetCountsBrowser",
            Key::NormalizeNoteText => "normalize_note_text",
        }
    }

    /// The key named exactly `name`.
    pub fn from_name(name: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|key| key.name() == name)
    }

    pub fn shape(self) -> Shape {
        match self {
            Key::ActiveCols | Key::ActiveNoteCols => Shape::Columns,
            Key::SortType | Key::NoteSortType => Shape::SortColumn,
            Key::SavedFilters | Key::FlagLabels => Shape::Names,
            Key::SetDueBrowser => Shape::Text("0"),
            Key::DefaultSearchText => Shape::Text(""),
            Key::RestorePositionBrowser | Key::NormalizeNoteText => Shape::Boolean(true),
            Key::SortBackwards
            | Key::BrowserNoteSortBackwards
            | Key::BrowserTableShowNotesMode
            | Key::CollapseSavedSearches
            | Key::CollapseToday
            | Key::CollapseFlags
            | Key::CollapseCardState
            | Key::CollapseDecks
            | Key::CollapseNotetypes
            | Key::CollapseTags
            | Key::PreviewBothSides
            | Key::IgnoreAccentsInSearch
            | Key::ResetCountsBrowser => Shape::Boolean(false),
        }
    }

    /// K10.1: whether the key has a whole-value write. Every key but
    /// `normalize_note_text`, which Meguri reads and never writes.
    pub fn writable(self) -> bool {
        self != Key::NormalizeNoteText
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Shape {
    /// K10.1: whether a whole-value write may store `value` under a key of
    /// this shape.
    pub fn accepts(self, value: &ConfigValue) -> bool {
        match (self, value) {
            (Shape::Boolean(_), ConfigValue::Bool(_)) => true,
            (Shape::Text(_) | Shape::SortColumn, ConfigValue::String(_)) => true,
            (Shape::Columns, ConfigValue::Array(items)) => items.iter().all(|item| item.as_str().is_some()),
            (Shape::Names, ConfigValue::Object(members)) => members.iter().all(|(_, member)| member.as_str().is_some()),
            _ => false,
        }
    }
}

/// The 19 columns of K3, each named by its column key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Column {
    SortField,
    Question,
    Answer,
    CardType,
    Deck,
    Due,
    Ease,
    Interval,
    Reviews,
    Lapses,
    Created,
    CardModified,
    NoteModified,
    Notetype,
    Tags,
    Position,
    Stability,
    Difficulty,
    Retrievability,
}

/// A sort direction (K3, K5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn descending(self) -> bool {
        self == SortDirection::Descending
    }

    pub fn reversed(self) -> SortDirection {
        match self {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        }
    }
}

impl Column {
    pub const ALL: [Column; 19] = [
        Column::SortField,
        Column::Question,
        Column::Answer,
        Column::CardType,
        Column::Deck,
        Column::Due,
        Column::Ease,
        Column::Interval,
        Column::Reviews,
        Column::Lapses,
        Column::Created,
        Column::CardModified,
        Column::NoteModified,
        Column::Notetype,
        Column::Tags,
        Column::Position,
        Column::Stability,
        Column::Difficulty,
        Column::Retrievability,
    ];

    /// The column key stored in `activeCols`, `activeNoteCols`, `sortType`
    /// and `noteSortType`.
    pub fn key(self) -> &'static str {
        match self {
            Column::SortField => "noteFld",
            Column::Question => "question",
            Column::Answer => "answer",
            Column::CardType => "template",
            Column::Deck => "deck",
            Column::Due => "cardDue",
            Column::Ease => "cardEase",
            Column::Interval => "cardIvl",
            Column::Reviews => "cardReps",
            Column::Lapses => "cardLapses",
            Column::Created => "noteCrt",
            Column::CardModified => "cardMod",
            Column::NoteModified => "noteMod",
            Column::Notetype => "note",
            Column::Tags => "noteTags",
            Column::Position => "originalPosition",
            Column::Stability => "stability",
            Column::Difficulty => "difficulty",
            Column::Retrievability => "retrievability",
        }
    }

    /// The column of a column key, compared exactly (K3); `None` for an
    /// unknown column key.
    pub fn from_key(key: &str) -> Option<Column> {
        Column::ALL.into_iter().find(|column| column.key() == key)
    }

    /// Whether the column sorts in `mode` (K3, R1140).
    pub fn sortable(self, mode: SearchMode) -> bool {
        match self {
            Column::Question | Column::Answer => false,
            // The memory-state columns sort cards only.
            Column::Stability | Column::Difficulty | Column::Retrievability => mode == SearchMode::Cards,
            _ => true,
        }
    }

    /// The direction the column sorts in when the user first sorts by it
    /// (K3, R1139); `None` for a column that never sorts.
    pub fn first_direction(self) -> Option<SortDirection> {
        match self {
            Column::Question | Column::Answer => None,
            Column::SortField | Column::Deck | Column::Notetype | Column::Tags => Some(SortDirection::Ascending),
            _ => Some(SortDirection::Descending),
        }
    }

    /// The column's English label in `mode` (`browser/columns` C2,
    /// R1118-R1130).
    pub fn label(self, mode: SearchMode) -> &'static str {
        let notes = mode == SearchMode::Notes;
        match self {
            Column::SortField => "Sort Field",
            Column::Question => "Question",
            Column::Answer => "Answer",
            Column::CardType if notes => "Cards",
            Column::CardType => "Card Type",
            Column::Deck => "Deck",
            Column::Due => "Due",
            Column::Ease if notes => "Avg. Ease",
            Column::Ease => "Ease",
            Column::Interval if notes => "Avg. Interval",
            Column::Interval => "Interval",
            Column::Reviews => "Reviews",
            Column::Lapses => "Lapses",
            Column::Created => "Created",
            Column::CardModified => "Card Modified",
            Column::NoteModified => "Note Modified",
            Column::Notetype => "Note Type",
            Column::Tags => "Tags",
            Column::Position => "Position",
            Column::Stability => "Stability",
            Column::Difficulty => "Difficulty",
            Column::Retrievability => "Retrievability",
        }
    }

    /// C2, R1131: text columns start-aligned, values centred.
    pub fn alignment(self) -> Alignment {
        match self {
            Column::SortField
            | Column::Question
            | Column::Answer
            | Column::CardType
            | Column::Deck
            | Column::Notetype
            | Column::Tags => Alignment::Start,
            _ => Alignment::Center,
        }
    }

    /// C2: where a value too long for its cell is shortened.
    pub fn elision(self) -> Elision {
        match self {
            Column::Deck => Elision::Middle,
            _ => Elision::End,
        }
    }

    /// C2, C6, R1150: the column's cells use the row's font.
    pub fn uses_row_font(self) -> bool {
        matches!(self, Column::SortField | Column::Question | Column::Answer)
    }
}

/// A column of a Browse list (`browser/workspace` V2.3): one of the 19
/// columns of K3, or a field column `fld:<field name>` (V3).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKey {
    Column(Column),
    /// The exact stored field name.
    Field(String),
}

impl ColumnKey {
    /// The column `key` names; `None` for a key that names none.
    pub fn parse(key: &str) -> Option<ColumnKey> {
        if let Some(name) = key.strip_prefix(FIELD_COLUMN_PREFIX) {
            return Some(ColumnKey::Field(name.to_owned()));
        }
        Column::from_key(key).map(ColumnKey::Column)
    }

    /// The column key as stored.
    pub fn key(&self) -> String {
        match self {
            ColumnKey::Column(column) => column.key().to_owned(),
            ColumnKey::Field(name) => format!("{FIELD_COLUMN_PREFIX}{name}"),
        }
    }

    /// `browser/sorting` S1: a field column sorts in both modes.
    pub fn sortable(&self, mode: SearchMode) -> bool {
        match self {
            ColumnKey::Column(column) => column.sortable(mode),
            ColumnKey::Field(_) => true,
        }
    }

    /// The direction a first click on the header sorts in (S1, V4.1); `None`
    /// for a column that never sorts.
    pub fn first_direction(&self) -> Option<SortDirection> {
        match self {
            ColumnKey::Column(column) => column.first_direction(),
            ColumnKey::Field(_) => Some(SortDirection::Ascending),
        }
    }
}

impl From<Column> for ColumnKey {
    fn from(column: Column) -> ColumnKey {
        ColumnKey::Column(column)
    }
}

/// A column's alignment (`browser/columns` C2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Alignment {
    Start,
    Center,
}

/// Where a cell's value is shortened (`browser/columns` C2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Elision {
    /// Deck names: `Parent::…::Child`.
    Middle,
    End,
}

/// Why an action writes nothing (K4.2, K4.3, K5.2, K7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    /// Hiding the only displayed column (K4.2, R1133).
    LastColumn,
    /// A reorder that is not every displayed column exactly once (K4.3).
    InvalidOrder,
    /// Sorting by a column that does not sort in the mode (K5.2, R1140).
    UnsortableColumn,
    /// Saving a search under an empty name (K7.2).
    EmptyName,
}

impl Refusal {
    /// The refusal's kind as the spec names it.
    pub fn kind(self) -> &'static str {
        match self {
            Refusal::LastColumn => "last_column",
            Refusal::InvalidOrder => "invalid_order",
            Refusal::UnsortableColumn => "unsortable_column",
            Refusal::EmptyName => "empty_name",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind())
    }
}

impl std::error::Error for Refusal {}

/// The active columns of one mode (K4): the base list a write starts from,
/// and the columns it displays.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveColumns {
    mode: SearchMode,
    base: Vec<ConfigValue>,
}

/// The column an item of a column list names, when it is a column key.
fn known(item: &ConfigValue) -> Option<Column> {
    item.as_str().and_then(Column::from_key)
}

/// The key that holds a mode's active columns.
pub fn columns_key(mode: SearchMode) -> Key {
    match mode {
        SearchMode::Cards => Key::ActiveCols,
        SearchMode::Notes => Key::ActiveNoteCols,
    }
}

/// K1: a mode's default columns.
pub fn default_columns(mode: SearchMode) -> [Column; 4] {
    match mode {
        SearchMode::Cards => [Column::SortField, Column::CardType, Column::Due, Column::Deck],
        SearchMode::Notes => [Column::SortField, Column::Notetype, Column::CardType, Column::Tags],
    }
}

impl ActiveColumns {
    /// K4.1 rules 1 and 4, from the key's value read per C3.
    pub fn from_stored(mode: SearchMode, stored: Option<&ConfigValue>) -> ActiveColumns {
        let defaults = || default_columns(mode).map(|column| ConfigValue::from(column.key()));
        let mut base = match stored {
            Some(ConfigValue::Array(items)) => items.clone(),
            _ => defaults().to_vec(),
        };
        if !base.iter().any(|item| known(item).is_some()) {
            base.extend(defaults());
        }
        ActiveColumns { mode, base }
    }

    pub fn mode(&self) -> SearchMode {
        self.mode
    }

    /// The base list (K4.1).
    pub fn base(&self) -> &[ConfigValue] {
        &self.base
    }

    /// K4.1 rule 3: the known columns of the base list, each once.
    pub fn displayed(&self) -> Vec<Column> {
        let mut displayed = Vec::new();
        for column in self.base.iter().filter_map(known) {
            if !displayed.contains(&column) {
                displayed.push(column);
            }
        }
        displayed
    }

    /// K4.2: the base list with every repeated occurrence of a known column
    /// after its first removed; unknown entries all kept, in place.
    fn without_repeats(&self) -> Vec<ConfigValue> {
        let mut seen = Vec::new();
        let mut list = Vec::with_capacity(self.base.len());
        for item in &self.base {
            if let Some(column) = known(item) {
                if seen.contains(&column) {
                    continue;
                }
                seen.push(column);
            }
            list.push(item.clone());
        }
        list
    }

    /// K4.2 Show column: the value to write, `None` when the column is
    /// displayed already.
    pub fn show(&self, column: Column) -> Option<ConfigValue> {
        if self.displayed().contains(&column) {
            return None;
        }
        let mut list = self.without_repeats();
        list.push(ConfigValue::from(column.key()));
        Some(ConfigValue::Array(list))
    }

    /// K4.2 Hide column: the value to write, `None` when the column is not
    /// displayed; refused when it is the only one.
    pub fn hide(&self, column: Column) -> Result<Option<ConfigValue>, Refusal> {
        let displayed = self.displayed();
        if !displayed.contains(&column) {
            return Ok(None);
        }
        if displayed.len() == 1 {
            return Err(Refusal::LastColumn);
        }
        let mut list = self.without_repeats();
        list.retain(|item| known(item) != Some(column));
        Ok(Some(ConfigValue::Array(list)))
    }

    /// K4.3: the value to write for the displayed columns in `order`.
    pub fn reorder(&self, order: &[Column]) -> Result<ConfigValue, Refusal> {
        let displayed = self.displayed();
        let once = order.len() == displayed.len()
            && displayed
                .iter()
                .all(|column| order.iter().filter(|c| *c == column).count() == 1);
        if !once {
            return Err(Refusal::InvalidOrder);
        }
        let mut next = order.iter();
        let list = self
            .without_repeats()
            .into_iter()
            .map(|item| match known(&item) {
                Some(_) => next.next().map_or(item, |column| ConfigValue::from(column.key())),
                None => item,
            })
            .collect();
        Ok(ConfigValue::Array(list))
    }
}

/// One mode's sort (K5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sort {
    pub column: Column,
    pub direction: SortDirection,
}

/// The column key and the backwards key of a mode's sort (K5).
pub fn sort_keys(mode: SearchMode) -> (Key, Key) {
    match mode {
        SearchMode::Cards => (Key::SortType, Key::SortBackwards),
        SearchMode::Notes => (Key::NoteSortType, Key::BrowserNoteSortBackwards),
    }
}

impl Sort {
    /// K5.1, from the two keys' values read per C3.
    pub fn from_stored(mode: SearchMode, column: Option<&ConfigValue>, backwards: Option<&ConfigValue>) -> Sort {
        let column = column
            .and_then(ConfigValue::as_str)
            .and_then(Column::from_key)
            .filter(|column| column.sortable(mode))
            .unwrap_or(Column::Created);
        let direction = match backwards.and_then(ConfigValue::as_bool) {
            Some(true) => SortDirection::Descending,
            _ => SortDirection::Ascending,
        };
        Sort { column, direction }
    }

    /// K5.2 Sort by column (a header click) from this sort.
    pub fn clicked(self, mode: SearchMode, column: Column) -> Result<Sort, Refusal> {
        let first = sortable_first(mode, column)?;
        let direction = if column == self.column {
            self.direction.reversed()
        } else {
            first
        };
        Ok(Sort { column, direction })
    }

    /// K5.2 Set sort (the iPhone Sort menu).
    pub fn chosen(mode: SearchMode, column: Column, direction: SortDirection) -> Result<Sort, Refusal> {
        sortable_first(mode, column)?;
        Ok(Sort { column, direction })
    }
}

/// The first direction of a column that sorts in `mode` (K3); refused for
/// any other column (K5.2).
fn sortable_first(mode: SearchMode, column: Column) -> Result<SortDirection, Refusal> {
    column
        .first_direction()
        .filter(|_| column.sortable(mode))
        .ok_or(Refusal::UnsortableColumn)
}

/// A sidebar section whose collapsed state is a key (K6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    SavedSearches,
    Today,
    Flags,
    CardState,
    Decks,
    Notetypes,
    Tags,
}

impl Section {
    pub const ALL: [Section; 7] = [
        Section::SavedSearches,
        Section::Today,
        Section::Flags,
        Section::CardState,
        Section::Decks,
        Section::Notetypes,
        Section::Tags,
    ];

    pub fn key(self) -> Key {
        match self {
            Section::SavedSearches => Key::CollapseSavedSearches,
            Section::Today => Key::CollapseToday,
            Section::Flags => Key::CollapseFlags,
            Section::CardState => Key::CollapseCardState,
            Section::Decks => Key::CollapseDecks,
            Section::Notetypes => Key::CollapseNotetypes,
            Section::Tags => Key::CollapseTags,
        }
    }
}

/// K6.4: whether a default search text is set, so that Browse opens with
/// it rather than with `deck:current`.
pub fn default_search_is_set(text: &str) -> bool {
    text.chars().any(|c| !trimmed_from_default_search(c))
}

/// The characters K6.4 removes from both ends of a default search text.
fn trimmed_from_default_search(c: char) -> bool {
    matches!(
        c,
        '\u{9}'..='\u{d}'
            | '\u{1c}'..='\u{20}'
            | '\u{85}'
            | '\u{a0}'
            | '\u{1680}'
            | '\u{2000}'..='\u{200a}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
    )
}

/// The saved searches (K7): the `savedFilters` value read per C3.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedSearches {
    stored: Option<ConfigValue>,
}

impl SavedSearches {
    pub fn from_stored(stored: Option<&ConfigValue>) -> SavedSearches {
        SavedSearches {
            stored: stored.cloned(),
        }
    }

    /// K7.1: each member whose value is a string, as (name, search text),
    /// by name in code point order, each name once.
    pub fn list(&self) -> Vec<(&str, &str)> {
        self.stored
            .as_ref()
            .map(ConfigValue::members)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, value)| Some((name, value.as_str()?)))
            .collect()
    }

    /// K7.1: the search text saved under exactly `name`.
    pub fn search(&self, name: &str) -> Option<&str> {
        self.stored.as_ref()?.get(name)?.as_str()
    }

    /// The object with member `name` set to `search`, every other member
    /// kept; a stored value that is not an object counts as an empty one.
    fn with_search(&self, name: &str, search: &str) -> ConfigValue {
        let empty = ConfigValue::Object(Vec::new());
        self.stored
            .as_ref()
            .unwrap_or(&empty)
            .with_member(name, ConfigValue::from(search))
    }

    /// K7.2 Save Search: the value to write, `None` when the name already
    /// holds this search; refused for an empty name.
    pub fn save(&self, name: &str, search: &str) -> Result<Option<ConfigValue>, Refusal> {
        if name.is_empty() {
            return Err(Refusal::EmptyName);
        }
        Ok(self.update(name, search))
    }

    /// K7.2 Update Saved Search: the value to write, `None` when the name
    /// already holds this search.
    pub fn update(&self, name: &str, search: &str) -> Option<ConfigValue> {
        if self.search(name) == Some(search) {
            return None;
        }
        Some(self.with_search(name, search))
    }

    /// K7.2 Rename Saved Search: the value to write, `None` when nothing
    /// changes.
    pub fn rename(&self, old: &str, new_text: &str) -> Option<ConfigValue> {
        let new: String = new_text.chars().filter(|&c| c != '"').collect();
        if new.is_empty() || new == old {
            return None;
        }
        let search = self.search(old)?;
        match self.with_search(&new, search) {
            ConfigValue::Object(mut members) => {
                members.retain(|(name, _)| name != old);
                Some(ConfigValue::Object(members))
            }
            _ => None,
        }
    }

    /// K7.2 Delete Saved Searches: the value to write, `None` when no listed
    /// name is a member.
    pub fn delete(&self, names: &[&str]) -> Option<ConfigValue> {
        let Some(ConfigValue::Object(members)) = &self.stored else {
            return None;
        };
        let kept: Vec<(String, ConfigValue)> = members
            .iter()
            .filter(|(name, _)| !names.contains(&name.as_str()))
            .cloned()
            .collect();
        (kept.len() < members.len()).then_some(ConfigValue::Object(kept))
    }
}

/// The 23 keys as one collection holds them, read per C3, with a typed
/// accessor for each (K1).
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserConfig {
    stored: Vec<Option<ConfigValue>>,
}

impl BrowserConfig {
    /// Reads every key: one config read each, nothing written (K2).
    pub fn read(reader: &(impl ReadRows + ?Sized)) -> Result<BrowserConfig, StorageError> {
        let mut stored = Vec::with_capacity(Key::ALL.len());
        for key in Key::ALL {
            stored.push(reader.config_value(key.name())?);
        }
        Ok(BrowserConfig { stored })
    }

    /// The keys' values as `stored` gives them, read per C3 (`None`:
    /// missing or unreadable).
    pub fn from_stored(mut stored: impl FnMut(Key) -> Option<ConfigValue>) -> BrowserConfig {
        BrowserConfig {
            stored: Key::ALL.into_iter().map(&mut stored).collect(),
        }
    }

    /// A key's value read per C3, whatever its shape.
    pub fn stored(&self, key: Key) -> Option<&ConfigValue> {
        self.stored.get(key as usize).and_then(Option::as_ref)
    }

    /// K2.1: a boolean key's value; `None` for a key of another shape.
    pub fn boolean(&self, key: Key) -> Option<bool> {
        match key.shape() {
            Shape::Boolean(default) => Some(self.stored(key).and_then(ConfigValue::as_bool).unwrap_or(default)),
            _ => None,
        }
    }

    /// K2.2: a text key's value; `None` for a key of another shape.
    pub fn text(&self, key: Key) -> Option<String> {
        match key.shape() {
            Shape::Text(default) => Some(
                self.stored(key)
                    .and_then(ConfigValue::as_str)
                    .unwrap_or(default)
                    .to_owned(),
            ),
            _ => None,
        }
    }

    fn flag(&self, key: Key) -> bool {
        self.boolean(key).unwrap_or(false)
    }

    fn string(&self, key: Key) -> String {
        self.text(key).unwrap_or_default()
    }

    /// `browserTableShowNotesMode` (K6.1): Browse shows notes.
    pub fn notes_mode(&self) -> bool {
        self.flag(Key::BrowserTableShowNotesMode)
    }

    /// `activeCols` or `activeNoteCols` (K4).
    pub fn columns(&self, mode: SearchMode) -> ActiveColumns {
        ActiveColumns::from_stored(mode, self.stored(columns_key(mode)))
    }

    /// `sortType` and `sortBackwards`, or `noteSortType` and
    /// `browserNoteSortBackwards` (K5).
    pub fn sort(&self, mode: SearchMode) -> Sort {
        let (column, backwards) = sort_keys(mode);
        Sort::from_stored(mode, self.stored(column), self.stored(backwards))
    }

    /// `savedFilters` (K7).
    pub fn saved_searches(&self) -> SavedSearches {
        SavedSearches::from_stored(self.stored(Key::SavedFilters))
    }

    /// `flagLabels` as stored (K8): the shown names are
    /// `tags/flags-and-marked` F4's (`meguri_engine::flags`).
    pub fn flag_labels(&self) -> Option<&ConfigValue> {
        self.stored(Key::FlagLabels)
    }

    /// The seven `collapse*` keys (K6.2).
    pub fn section_collapsed(&self, section: Section) -> bool {
        self.flag(section.key())
    }

    /// `previewBothSides` (K6.3).
    pub fn preview_both_sides(&self) -> bool {
        self.flag(Key::PreviewBothSides)
    }

    /// `setDueBrowser` (K6.6).
    pub fn set_due_browser(&self) -> String {
        self.string(Key::SetDueBrowser)
    }

    /// `defaultSearchText` as stored, untrimmed (K6.4).
    pub fn default_search_text(&self) -> String {
        self.string(Key::DefaultSearchText)
    }

    /// K6.4: the search Browse opens with when the text is set; `None`
    /// opens with `deck:current` and an empty field.
    pub fn default_search(&self) -> Option<String> {
        Some(self.default_search_text()).filter(|text| default_search_is_set(text))
    }

    /// `ignoreAccentsInSearch` (K6.5).
    pub fn ignore_accents_in_search(&self) -> bool {
        self.flag(Key::IgnoreAccentsInSearch)
    }

    /// `restorePositionBrowser` (K6.6).
    pub fn restore_position_browser(&self) -> bool {
        self.flag(Key::RestorePositionBrowser)
    }

    /// `resetCountsBrowser` (K6.6).
    pub fn reset_counts_browser(&self) -> bool {
        self.flag(Key::ResetCountsBrowser)
    }

    /// `normalize_note_text` (K6.5).
    pub fn normalize_note_text(&self) -> bool {
        self.flag(Key::NormalizeNoteText)
    }
}
