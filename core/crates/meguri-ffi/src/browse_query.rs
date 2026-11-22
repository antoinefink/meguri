//! Browse's query model and its reads over FFI (U264a): the filter tokens
//! query (`search/filter-tokens` T1 to T7) with its run text (T4), T7's
//! reading of Anki text and T8's commit; D5's incomplete text at the caret
//! (T8); the suggestions (T12); the filter counts (T12.4) as a job at
//! utility QoS; the column sets (`browser/workspace` V2.2 to V2.4); the
//! counts of a selection (V7, V8.1's "shown when") as a job; and Find
//! Duplicates (V10) as a job.
//!
//! The query and suggestion calls read only the open collection's name
//! index (`search/editing-services` D6.7), never the collection. Offsets
//! into the field's text cross in UTF-16 code units (as `browser/result-sets`
//! H15 does for error ranges); the core counts characters.

use std::ops::Range;
use std::sync::{Arc, Mutex};

use meguri_engine::browse::{self as browse, SelectionCounts};
use meguri_engine::browse::search::{BrowseError, SearchMode};
use meguri_engine::browser_config::column_sets;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_search::config::column_sets::{Change, SetId};
use meguri_search::config::{ColumnKey, SortDirection};
use meguri_search::filter_tokens::counts::CountItem;
use meguri_search::filter_tokens::suggest::{self, BuiltIn, Labels, Row, Section, SuggestRequest};
use meguri_search::filter_tokens::{
    self as tokens, DeckValue, DueValue, FieldTest, NotetypeValue, Period, StateValue, Token, Value, ValueToken,
};
use meguri_search::name_index::NameIndex;
use meguri_search::suggest as syntax;

use crate::browse_search::{BrowseMode, BrowseSelection, BrowseSort, BrowseSortDirection};
use crate::jobs::JobHandle;
use crate::mutations::BoolMutation;
use crate::{CoreError, callback, runtime};

// ---------------------------------------------------------------- offsets

/// The UTF-16 offset of character `chars` of `text`.
fn utf16_of(text: &str, chars: usize) -> u32 {
    text.chars().take(chars).map(|c| c.len_utf16() as u32).sum()
}

/// The character at UTF-16 offset `units` of `text`: the first character
/// that starts at or after it. `InvalidArgument` past the end.
fn chars_of(text: &str, units: u32) -> Result<usize, CoreError> {
    let mut at = 0u32;
    for (index, c) in text.chars().enumerate() {
        if at >= units {
            return Ok(index);
        }
        at += c.len_utf16() as u32;
    }
    if at >= units {
        Ok(text.chars().count())
    } else {
        Err(CoreError::InvalidArgument {
            message: format!("offset {units} is past the end of the text ({at} UTF-16 units)"),
        })
    }
}

/// The open collection's name index, as last built.
fn names() -> Result<Arc<NameIndex>, CoreError> {
    Ok(runtime::host()?.names().index())
}

// ------------------------------------------------------------ query model

/// A Card State value (T3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterState {
    New,
    Learning,
    Review,
    Suspended,
    Buried,
    /// `tag:leech`.
    Leech,
}

/// A Due value (T3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterDue {
    Today,
    Overdue,
    Next7Days,
}

/// The day values of Added, Edited and Studied (T3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterPeriod {
    Today,
    Last7Days,
    Last30Days,
}

/// A Field token's test (T3).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterFieldTest {
    Contains { text: String },
    Empty,
    HasContent,
}

/// One value of T3's table.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterValue {
    /// A deck with its subdecks: its stored name (levels joined by
    /// U+001F) and every deck below it (`filter_deck_value` makes one).
    Deck { id: i64, native: String, tree: Vec<i64> },
    CurrentDeck,
    /// A tag by its full name; it finds its descendants.
    Tag { name: String },
    Untagged,
    Notetype { id: i64, name: String },
    /// A card type name: every card type of that name in any note type.
    CardType { name: String },
    State { state: FilterState },
    /// Flags 1 to 7.
    Flag { flag: u8 },
    NoFlag,
    AnyFlag,
    Due { due: FilterDue },
    Added { period: FilterPeriod },
    Edited { period: FilterPeriod },
    Studied { period: FilterPeriod },
    Field { field: String, test: FilterFieldTest },
    /// Made only by Find Duplicates (`browser/workspace` V10).
    Duplicates { note_ids: Vec<i64> },
    /// Made only by a heatmap day (`browser/workspace` V15).
    StudiedOn { card_ids: Vec<i64> },
}

/// One token of the field (T1): a value token, or an advanced token holding
/// Anki search text no kind can show (T6).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterToken {
    Value { values: Vec<FilterValue>, negated: bool },
    Advanced { text: String },
}

/// What the search field holds (T1): tokens in their order, then the text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, uniffi::Record)]
pub struct FilterQuery {
    pub tokens: Vec<FilterToken>,
    pub text: String,
}

fn period(period: Period) -> FilterPeriod {
    match period {
        Period::Today => FilterPeriod::Today,
        Period::Last7Days => FilterPeriod::Last7Days,
        Period::Last30Days => FilterPeriod::Last30Days,
    }
}

fn engine_period(period: FilterPeriod) -> Period {
    match period {
        FilterPeriod::Today => Period::Today,
        FilterPeriod::Last7Days => Period::Last7Days,
        FilterPeriod::Last30Days => Period::Last30Days,
    }
}

impl From<Value> for FilterValue {
    fn from(value: Value) -> FilterValue {
        match value {
            Value::Deck(deck) => FilterValue::Deck {
                id: deck.id.0,
                native: deck.native,
                tree: deck.tree.into_iter().map(|id| id.0).collect(),
            },
            Value::CurrentDeck => FilterValue::CurrentDeck,
            Value::Tag(name) => FilterValue::Tag { name },
            Value::Untagged => FilterValue::Untagged,
            Value::Notetype(notetype) => FilterValue::Notetype {
                id: notetype.id.0,
                name: notetype.name,
            },
            Value::CardType(name) => FilterValue::CardType { name },
            Value::State(state) => FilterValue::State {
                state: match state {
                    StateValue::New => FilterState::New,
                    StateValue::Learning => FilterState::Learning,
                    StateValue::Review => FilterState::Review,
                    StateValue::Suspended => FilterState::Suspended,
                    StateValue::Buried => FilterState::Buried,
                    StateValue::Leech => FilterState::Leech,
                },
            },
            Value::Flag(flag) => FilterValue::Flag { flag },
            Value::NoFlag => FilterValue::NoFlag,
            Value::AnyFlag => FilterValue::AnyFlag,
            Value::Due(due) => FilterValue::Due {
                due: match due {
                    DueValue::Today => FilterDue::Today,
                    DueValue::Overdue => FilterDue::Overdue,
                    DueValue::Next7Days => FilterDue::Next7Days,
                },
            },
            Value::Added(p) => FilterValue::Added { period: period(p) },
            Value::Edited(p) => FilterValue::Edited { period: period(p) },
            Value::Studied(p) => FilterValue::Studied { period: period(p) },
            Value::Field { field, test } => FilterValue::Field {
                field,
                test: match test {
                    FieldTest::Contains(text) => FilterFieldTest::Contains { text },
                    FieldTest::Empty => FilterFieldTest::Empty,
                    FieldTest::HasContent => FilterFieldTest::HasContent,
                },
            },
            Value::Duplicates(ids) => FilterValue::Duplicates {
                note_ids: ids.into_iter().map(|id| id.0).collect(),
            },
            Value::StudiedOn(ids) => FilterValue::StudiedOn {
                card_ids: ids.into_iter().map(|id| id.0).collect(),
            },
        }
    }
}

impl From<FilterValue> for Value {
    fn from(value: FilterValue) -> Value {
        match value {
            FilterValue::Deck { id, native, tree } => Value::Deck(DeckValue {
                id: DeckId(id),
                native,
                tree: tree.into_iter().map(DeckId).collect(),
            }),
            FilterValue::CurrentDeck => Value::CurrentDeck,
            FilterValue::Tag { name } => Value::Tag(name),
            FilterValue::Untagged => Value::Untagged,
            FilterValue::Notetype { id, name } => Value::Notetype(NotetypeValue {
                id: NotetypeId(id),
                name,
            }),
            FilterValue::CardType { name } => Value::CardType(name),
            FilterValue::State { state } => Value::State(match state {
                FilterState::New => StateValue::New,
                FilterState::Learning => StateValue::Learning,
                FilterState::Review => StateValue::Review,
                FilterState::Suspended => StateValue::Suspended,
                FilterState::Buried => StateValue::Buried,
                FilterState::Leech => StateValue::Leech,
            }),
            FilterValue::Flag { flag } => Value::Flag(flag),
            FilterValue::NoFlag => Value::NoFlag,
            FilterValue::AnyFlag => Value::AnyFlag,
            FilterValue::Due { due } => Value::Due(match due {
                FilterDue::Today => DueValue::Today,
                FilterDue::Overdue => DueValue::Overdue,
                FilterDue::Next7Days => DueValue::Next7Days,
            }),
            FilterValue::Added { period } => Value::Added(engine_period(period)),
            FilterValue::Edited { period } => Value::Edited(engine_period(period)),
            FilterValue::Studied { period } => Value::Studied(engine_period(period)),
            FilterValue::Field { field, test } => Value::Field {
                field,
                test: match test {
                    FilterFieldTest::Contains { text } => FieldTest::Contains(text),
                    FilterFieldTest::Empty => FieldTest::Empty,
                    FilterFieldTest::HasContent => FieldTest::HasContent,
                },
            },
            FilterValue::Duplicates { note_ids } => Value::Duplicates(note_ids.into_iter().map(NoteId).collect()),
            FilterValue::StudiedOn { card_ids } => Value::StudiedOn(card_ids.into_iter().map(CardId).collect()),
        }
    }
}

impl From<tokens::FilterQuery> for FilterQuery {
    fn from(query: tokens::FilterQuery) -> FilterQuery {
        FilterQuery {
            tokens: query
                .tokens
                .into_iter()
                .map(|token| match token {
                    Token::Value(token) => FilterToken::Value {
                        values: token.values.into_iter().map(Into::into).collect(),
                        negated: token.negated,
                    },
                    Token::Advanced(text) => FilterToken::Advanced { text },
                })
                .collect(),
            text: query.text,
        }
    }
}

impl From<FilterQuery> for tokens::FilterQuery {
    fn from(query: FilterQuery) -> tokens::FilterQuery {
        tokens::FilterQuery::new(
            query
                .tokens
                .into_iter()
                .map(|token| match token {
                    FilterToken::Value { values, negated } => {
                        Token::Value(ValueToken::new(values.into_iter().map(Into::into).collect(), negated))
                    }
                    FilterToken::Advanced { text } => Token::Advanced(text),
                })
                .collect(),
            query.text,
        )
    }
}

/// T4: the Anki search text `query` stands for, which `start_search` runs
/// and Save Search stores. `InvalidArgument` for a token T3, T7 and T10
/// never make (no values, values of several kinds, a value not offered, an
/// advanced token whose text does not parse).
#[uniffi::export]
pub fn filter_run_text(query: FilterQuery) -> Result<String, CoreError> {
    callback::enter("filter_run_text");
    let names = names()?;
    tokens::FilterQuery::from(query)
        .run_text(&names)
        .map_err(|error| CoreError::InvalidArgument {
            message: format!("the query has no run text: {error:?}"),
        })
}

/// T7: the query Anki search text reads as (typed and committed, pasted,
/// stored, or handed to Browse). It never fails.
#[uniffi::export]
pub fn filter_read_text(text: String) -> Result<FilterQuery, CoreError> {
    callback::enter("filter_read_text");
    Ok(tokens::read_text(&text, &*names()?).into())
}

/// T8's commit: text that holds Anki syntax becomes tokens after the
/// query's own; text that stays text stays as typed.
#[uniffi::export]
pub fn filter_commit(query: FilterQuery) -> Result<FilterQuery, CoreError> {
    callback::enter("filter_commit");
    Ok(tokens::FilterQuery::from(query).commit(&*names()?).into())
}

/// T8: whether the text is incomplete at the caret (`search/editing-services`
/// D5: a key with no value, an unclosed quote or bracket, a dangling `-`,
/// `and` or `or`), so the live search keeps the last results. `caret` is
/// a UTF-16 offset into `text`.
#[uniffi::export]
pub fn filter_text_incomplete(text: String, caret: u32) -> Result<bool, CoreError> {
    callback::enter("filter_text_incomplete");
    let caret = chars_of(&text, caret)?;
    let tail = meguri_search::tail::classify_tail(&text, caret).map_err(|_| CoreError::InvalidArgument {
        message: "the caret is outside the text".into(),
    })?;
    Ok(matches!(tail, meguri_search::tail::Tail::Incomplete { .. }))
}

/// T3's Deck value of deck `id` (with every deck below it, the tree its
/// deck-ids item names), as a sidebar row makes it; none when no deck has
/// that id or the deck is not offered.
#[uniffi::export]
pub fn filter_deck_value(deck_id: i64) -> Result<Option<FilterValue>, CoreError> {
    callback::enter("filter_deck_value");
    let names = names()?;
    Ok(DeckValue::of(&names, DeckId(deck_id))
        .map(Value::Deck)
        .filter(Value::is_offered)
        .map(Into::into))
}

// ------------------------------------------------------------ suggestions

/// A section of the suggestions (T12.1, T12.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterSection {
    SavedSearches,
    CardState,
    Flag,
    Due,
    Added,
    Edited,
    Studied,
    Decks,
    Tags,
    Notetypes,
    CardTypes,
    Fields,
    FilterBy,
}

impl From<Section> for FilterSection {
    fn from(section: Section) -> FilterSection {
        match section {
            Section::SavedSearches => FilterSection::SavedSearches,
            Section::CardState => FilterSection::CardState,
            Section::Flag => FilterSection::Flag,
            Section::Due => FilterSection::Due,
            Section::Added => FilterSection::Added,
            Section::Edited => FilterSection::Edited,
            Section::Studied => FilterSection::Studied,
            Section::Decks => FilterSection::Decks,
            Section::Tags => FilterSection::Tags,
            Section::Notetypes => FilterSection::Notetypes,
            Section::CardTypes => FilterSection::CardTypes,
            Section::Fields => FilterSection::Fields,
            Section::FilterBy => FilterSection::FilterBy,
        }
    }
}

impl From<FilterSection> for Section {
    fn from(section: FilterSection) -> Section {
        match section {
            FilterSection::SavedSearches => Section::SavedSearches,
            FilterSection::CardState => Section::CardState,
            FilterSection::Flag => Section::Flag,
            FilterSection::Due => Section::Due,
            FilterSection::Added => Section::Added,
            FilterSection::Edited => Section::Edited,
            FilterSection::Studied => Section::Studied,
            FilterSection::Decks => Section::Decks,
            FilterSection::Tags => Section::Tags,
            FilterSection::Notetypes => Section::Notetypes,
            FilterSection::CardTypes => Section::CardTypes,
            FilterSection::Fields => Section::Fields,
            FilterSection::FilterBy => Section::FilterBy,
        }
    }
}

/// A built-in saved search (T13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterBuiltIn {
    DueToday,
    AddedInLast7Days,
    Leeches,
}

/// The labels fixed values and built-ins match by (T12.2, T15): Swift's
/// localized strings.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FilterLabels {
    pub current_deck: String,
    pub untagged: String,
    pub new: String,
    pub learning: String,
    pub review: String,
    pub suspended: String,
    pub buried: String,
    pub leech: String,
    pub no_flag: String,
    pub any_flag: String,
    pub today: String,
    pub overdue: String,
    pub next_7_days: String,
    pub last_7_days: String,
    pub last_30_days: String,
    pub due_today: String,
    pub added_in_last_7_days: String,
    pub leeches: String,
}

impl From<FilterLabels> for Labels {
    fn from(labels: FilterLabels) -> Labels {
        Labels {
            current_deck: labels.current_deck,
            untagged: labels.untagged,
            new: labels.new,
            learning: labels.learning,
            review: labels.review,
            suspended: labels.suspended,
            buried: labels.buried,
            leech: labels.leech,
            no_flag: labels.no_flag,
            any_flag: labels.any_flag,
            today: labels.today,
            overdue: labels.overdue,
            next_7_days: labels.next_7_days,
            last_7_days: labels.last_7_days,
            last_30_days: labels.last_30_days,
            due_today: labels.due_today,
            added_in_last_7_days: labels.added_in_last_7_days,
            leeches: labels.leeches,
        }
    }
}

/// D6.1's kinds of a syntax candidate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SyntaxKind {
    AsTyped,
    Keyword,
    Deck,
    Tag,
    Notetype,
    Template,
    Field,
    Flag,
    State,
    Property,
    Operator,
    ValueHint,
    Days,
    Button,
    Preset,
    SavedSearch,
}

/// A property's unit (D6.3.1 step 3); Swift writes the words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SyntaxUnit {
    Days,
    DaysFromToday,
    Count,
    EaseFactor,
    Position,
    DaysAgo,
    ZeroToOne,
    Number,
    Text,
}

/// D6.1's `detail`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum SyntaxDetail {
    Filtered,
    FlagName { name: String },
    Unit { unit: SyntaxUnit },
    Days { days: u32 },
    Button { button: u8 },
}

/// D6.6: replace `start..end` (UTF-16 offsets of the text) by `text`, and
/// put the caret at `caret_after`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SyntaxInsertion {
    pub start: u32,
    pub end: u32,
    pub text: String,
    pub caret_after: u32,
}

/// One D6 candidate while Anki syntax is typed at the caret (T12.2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct SyntaxCandidate {
    pub kind: SyntaxKind,
    pub label: String,
    pub detail: Option<SyntaxDetail>,
    pub insert: Option<SyntaxInsertion>,
    /// Accepting it commits the search (D6.6).
    pub completes: bool,
}

/// One row of the suggestions (T12.1).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum FilterSuggestionRow {
    /// "Contains “w”": keeps the word as text and commits.
    Contains { word: String },
    BuiltIn { search: FilterBuiltIn },
    /// A user saved search: accepting it replaces the query by T7's
    /// reading of its text.
    SavedSearch { name: String, text: String },
    /// Filter By: opens the kind's value list.
    FilterBy { section: FilterSection },
    Value { value: FilterValue },
    /// A field name with the note types that have it; accepting it shows
    /// its three tests.
    Field { name: String, notetypes: Vec<i64> },
    /// A card type value with the note types that have it.
    CardType { value: FilterValue, notetypes: Vec<i64> },
    /// Ends a section cut short: opens that section's value list.
    ShowAll { section: FilterSection },
    /// Save Search…, last while the query is not empty.
    SaveSearch,
    Syntax { candidate: SyntaxCandidate },
}

/// One row, the section it is shown under, and the key to pass back in
/// `recent` once the user accepts it (none for rows not ranked by use).
#[derive(Clone, Debug, PartialEq, Eq, Hash, uniffi::Record)]
pub struct FilterSuggestion {
    pub section: Option<FilterSection>,
    pub row: FilterSuggestionRow,
    pub key: Option<String>,
}

fn syntax_candidate(candidate: syntax::Candidate, text: &str) -> SyntaxCandidate {
    use syntax::{CandidateKind as K, Detail, Unit};
    SyntaxCandidate {
        kind: match candidate.kind {
            K::AsTyped => SyntaxKind::AsTyped,
            K::Keyword => SyntaxKind::Keyword,
            K::Deck => SyntaxKind::Deck,
            K::Tag => SyntaxKind::Tag,
            K::Notetype => SyntaxKind::Notetype,
            K::Template => SyntaxKind::Template,
            K::Field => SyntaxKind::Field,
            K::Flag => SyntaxKind::Flag,
            K::State => SyntaxKind::State,
            K::Property => SyntaxKind::Property,
            K::Operator => SyntaxKind::Operator,
            K::ValueHint => SyntaxKind::ValueHint,
            K::Days => SyntaxKind::Days,
            K::Button => SyntaxKind::Button,
            K::Preset => SyntaxKind::Preset,
            K::SavedSearch => SyntaxKind::SavedSearch,
        },
        label: candidate.label,
        detail: candidate.detail.map(|detail| match detail {
            Detail::Filtered => SyntaxDetail::Filtered,
            Detail::FlagName(name) => SyntaxDetail::FlagName { name },
            Detail::Unit(unit) => SyntaxDetail::Unit {
                unit: match unit {
                    Unit::Days => SyntaxUnit::Days,
                    Unit::DaysFromToday => SyntaxUnit::DaysFromToday,
                    Unit::Count => SyntaxUnit::Count,
                    Unit::EaseFactor => SyntaxUnit::EaseFactor,
                    Unit::Position => SyntaxUnit::Position,
                    Unit::DaysAgo => SyntaxUnit::DaysAgo,
                    Unit::ZeroToOne => SyntaxUnit::ZeroToOne,
                    Unit::Number => SyntaxUnit::Number,
                    Unit::Text => SyntaxUnit::Text,
                },
            },
            Detail::Days(days) => SyntaxDetail::Days { days },
            Detail::Button(button) => SyntaxDetail::Button { button },
        }),
        insert: candidate.insert.map(|insert| {
            // The insertion's offsets count characters of the text before
            // the insertion; the caret after it, of the text after it.
            let Range { start, end } = insert.range;
            let after: String = text
                .chars()
                .take(start)
                .chain(insert.text.chars())
                .chain(text.chars().skip(end))
                .collect();
            SyntaxInsertion {
                start: utf16_of(text, start),
                end: utf16_of(text, end),
                caret_after: utf16_of(&after, insert.cursor_after),
                text: insert.text,
            }
        }),
        completes: candidate.completes,
    }
}

fn suggestion_row(row: Row, text: &str) -> FilterSuggestionRow {
    let ids = |notetypes: Vec<NotetypeId>| notetypes.into_iter().map(|id| id.0).collect();
    match row {
        Row::Contains(word) => FilterSuggestionRow::Contains { word },
        Row::BuiltIn(built_in) => FilterSuggestionRow::BuiltIn {
            search: match built_in {
                BuiltIn::DueToday => FilterBuiltIn::DueToday,
                BuiltIn::AddedInLast7Days => FilterBuiltIn::AddedInLast7Days,
                BuiltIn::Leeches => FilterBuiltIn::Leeches,
            },
        },
        Row::SavedSearch { name, text } => FilterSuggestionRow::SavedSearch { name, text },
        Row::FilterBy(section) => FilterSuggestionRow::FilterBy {
            section: section.into(),
        },
        Row::Value(value) => FilterSuggestionRow::Value { value: value.into() },
        Row::Field { name, notetypes } => FilterSuggestionRow::Field {
            name,
            notetypes: ids(notetypes),
        },
        Row::CardType { value, notetypes } => FilterSuggestionRow::CardType {
            value: value.into(),
            notetypes: ids(notetypes),
        },
        Row::ShowAll(section) => FilterSuggestionRow::ShowAll {
            section: section.into(),
        },
        Row::SaveSearch => FilterSuggestionRow::SaveSearch,
        Row::Syntax(candidate) => FilterSuggestionRow::Syntax {
            candidate: syntax_candidate(candidate, text),
        },
    }
}

/// T12: the suggestions for `query` with the caret at UTF-16 offset `caret`
/// of its text, in display order. `scope` asks for one kind's value list
/// ("Add Another…", "Show All", the Filters sheet); `recent` holds the keys
/// of rows the user accepted, newest first (device state). Reads only the
/// name index; within one frame (T12.3 step 5). Counts come from
/// `start_filter_counts`.
#[uniffi::export]
pub fn filter_suggestions(
    query: FilterQuery,
    caret: u32,
    scope: Option<FilterSection>,
    recent: Vec<String>,
    labels: FilterLabels,
) -> Result<Vec<FilterSuggestion>, CoreError> {
    callback::enter("filter_suggestions");
    let names = names()?;
    let query = tokens::FilterQuery::from(query);
    let caret = chars_of(&query.text, caret)?;
    let labels = Labels::from(labels);
    let request = SuggestRequest {
        query: &query,
        caret,
        scope: scope.map(Into::into),
        recent: &recent,
    };
    let rows = suggest::suggest(&names, &labels, &request).map_err(|_| CoreError::InvalidArgument {
        message: "the caret is outside the text".into(),
    })?;
    Ok(rows
        .into_iter()
        .map(|row| FilterSuggestion {
            key: row.key(),
            section: row.section.map(Into::into),
            row: suggestion_row(row.row, &query.text),
        })
        .collect())
}

// ---------------------------------------------------------- filter counts

/// One thing a sidebar or suggestion row counts (T12.3 step 4).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FilterCountItem {
    /// A value alone.
    Value { value: FilterValue },
    /// A user saved search by its stored text (T4 of T7's reading of it).
    Saved { text: String },
    /// A whole query, such as the deck page's Suspended Cards row.
    Query { query: FilterQuery },
}

impl From<FilterCountItem> for CountItem {
    fn from(item: FilterCountItem) -> CountItem {
        match item {
            FilterCountItem::Value { value } => CountItem::Value(value.into()),
            FilterCountItem::Saved { text } => CountItem::Saved(text),
            FilterCountItem::Query { query } => CountItem::Query(query.into()),
        }
    }
}

type CountsResult = Arc<Mutex<Option<Result<Vec<Option<u64>>, CoreError>>>>;

#[derive(Debug, uniffi::Object)]
pub struct FilterCountsJob {
    result: CountsResult,
}

#[uniffi::export]
impl FilterCountsJob {
    /// One entry per item, in order: its rows of the mode, or none when it
    /// cannot be counted. `None` until the job ends.
    pub fn result(&self) -> Result<Option<Vec<Option<u64>>>, CoreError> {
        callback::enter("FilterCountsJob::result");
        lock(&self.result).clone().transpose()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The newest of a kind of count job: calls take a ticket when they enter,
/// in call order, and the job of the newest ticket cancels every older one,
/// whichever reaches the engine first.
#[derive(Debug, Default)]
pub struct Newest {
    issued: std::sync::atomic::AtomicU64,
    /// The newest call that reached `start` (whether its job started or
    /// not), and the token of the job it started.
    running: Mutex<(u64, Option<Arc<meguri_engine::CancelToken>>)>,
}

impl Newest {
    const fn new() -> Newest {
        Newest {
            issued: std::sync::atomic::AtomicU64::new(0),
            running: Mutex::new((0, None)),
        }
    }

    /// A call's place in call order; take it before preparing the call.
    fn ticket(&self) -> u64 {
        self.issued.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
    }

    /// Starts `start` for `ticket` after cancelling an older job; a call
    /// older than one that already reached here is refused `Cancelled`, so
    /// it never cancels a newer job, even when that one failed to start.
    fn start(
        &self,
        ticket: u64,
        start: impl FnOnce() -> Result<meguri_engine::JobHandle, CoreError>,
    ) -> Result<meguri_engine::JobHandle, CoreError> {
        let mut running = lock(&self.running);
        if running.0 > ticket {
            return Err(CoreError::Cancelled);
        }
        running.0 = ticket;
        if let Some(older) = running.1.take() {
            older.cancel();
        }
        let handle = start()?;
        running.1 = Some(Arc::clone(&handle.cancel));
        Ok(handle)
    }
}

/// The newest filter counts job.
static NEWEST_COUNTS: Newest = Newest::new();

#[derive(Clone, Debug, uniffi::Record)]
pub struct FilterCountsJobStart {
    pub handle: JobHandle,
    pub result: Arc<FilterCountsJob>,
}

/// T12.4: for each item, the rows of `mode` it finds in the whole
/// collection, as a job of the engine's pool at utility QoS, so searches go
/// first. Cached by change seq; a newer call cancels this one (its outcome
/// is then `Cancelled`), as does its token. A collection replaced before it
/// runs fails it with `Unavailable`.
#[uniffi::export]
pub fn start_filter_counts(mode: BrowseMode, items: Vec<FilterCountItem>) -> Result<FilterCountsJobStart, CoreError> {
    callback::enter("start_filter_counts");
    let ticket = NEWEST_COUNTS.ticket();
    let host = runtime::host()?;
    let engine = runtime::engine()?;
    let mode = SearchMode::from(mode);
    let items: Vec<CountItem> = items.into_iter().map(Into::into).collect();
    let result: CountsResult = Arc::default();
    let out = Arc::clone(&result);
    let handle = NEWEST_COUNTS.start(ticket, || {
        Ok(engine.start_utility_job(meguri_engine::JobKind::BulkOp, move |job| {
            // The host it was started for: its name index and its counts.
            if !Arc::ptr_eq(&host, &runtime::host()?) {
                return Err(meguri_engine::EngineError::CollectionUnavailable {
                    reason: "the collection was replaced".into(),
                });
            }
            let names = host.names().index();
            let counts = match host
                .filter_counts()
                .filter_counts(engine, &names, mode, &items, Some(&*job.cancel))
            {
                Ok(counts) => Ok(counts),
                Err(BrowseError::Engine(error)) => return Err(error),
                Err(refusal) => Err(CoreError::from(refusal)),
            };
            *lock(&out) = Some(counts);
            Ok(())
        })?)
    })?;
    Ok(FilterCountsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(FilterCountsJob { result }),
    })
}

// ------------------------------------------------------------ column sets

/// Which set a change writes (V2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum ColumnSetId {
    Standard,
    /// A note type's own set.
    Notetype { id: i64 },
}

impl From<ColumnSetId> for SetId {
    fn from(set: ColumnSetId) -> SetId {
        match set {
            ColumnSetId::Standard => SetId::Standard,
            ColumnSetId::Notetype { id } => SetId::Notetype(NotetypeId(id)),
        }
    }
}

impl From<SetId> for ColumnSetId {
    fn from(set: SetId) -> ColumnSetId {
        match set {
            SetId::Standard => ColumnSetId::Standard,
            SetId::Notetype(id) => ColumnSetId::Notetype { id: id.0 },
        }
    }
}

/// The set a list shows (V2.2), with each mode's columns and sort.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ColumnSetShown {
    /// The set changes write to: the note type's when the query names one
    /// that exists, even before it has an own set.
    pub set: ColumnSetId,
    /// Whether that note type has an own set; false for the standard set.
    pub own: bool,
    /// Column keys in order (V2.3), never empty; the state column is not
    /// listed.
    pub cards: Vec<String>,
    pub notes: Vec<String>,
    pub cards_sort: BrowseSort,
    pub notes_sort: BrowseSort,
}

/// A user change to a set (V2.3, V4).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ColumnSetChange {
    /// Checking a column: it is added at the end.
    Show { key: String },
    /// Unchecking a column; refused for the only one.
    Hide { key: String },
    /// Dragging a header: the list in this order, each column once.
    Reorder { keys: Vec<String> },
    /// A header click: the column in its first direction, or the current
    /// sort reversed.
    SortBy { key: String },
    /// The touch Sort By menu: this column and direction.
    SetSort { sort: BrowseSort },
}

fn column(key: &str) -> Result<ColumnKey, CoreError> {
    ColumnKey::parse(key).ok_or_else(|| CoreError::InvalidArgument {
        message: format!("{key:?} is not a column key"),
    })
}

fn direction(direction: BrowseSortDirection) -> SortDirection {
    match direction {
        BrowseSortDirection::Ascending => SortDirection::Ascending,
        BrowseSortDirection::Descending => SortDirection::Descending,
    }
}

impl TryFrom<ColumnSetChange> for Change {
    type Error = CoreError;
    fn try_from(change: ColumnSetChange) -> Result<Change, CoreError> {
        Ok(match change {
            ColumnSetChange::Show { key } => Change::Show(column(&key)?),
            ColumnSetChange::Hide { key } => Change::Hide(column(&key)?),
            ColumnSetChange::Reorder { keys } => {
                Change::Reorder(keys.iter().map(|key| column(key)).collect::<Result<_, _>>()?)
            }
            ColumnSetChange::SortBy { key } => Change::SortBy(column(&key)?),
            ColumnSetChange::SetSort { sort } => Change::SetSort(column(&sort.key)?, direction(sort.direction)),
        })
    }
}

fn shown_sort(sort: &meguri_search::config::column_sets::SetSort) -> BrowseSort {
    BrowseSort {
        key: sort.column.key(),
        direction: match sort.direction {
            SortDirection::Ascending => BrowseSortDirection::Ascending,
            SortDirection::Descending => BrowseSortDirection::Descending,
        },
    }
}

/// V2.2: the set a list uses when the query's Note Type token names
/// `notetype` (one value, set to is), or the standard set for none. Read
/// from `meguriBrowserColumnSets` on the query reader; it writes nothing.
#[uniffi::export]
pub fn browser_column_set(notetype: Option<i64>) -> Result<ColumnSetShown, CoreError> {
    callback::enter("browser_column_set");
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(meguri_storage::ReaderRole::Query)?;
    // The key and the note type are read in one snapshot.
    let snapshot = reader.snapshot()?;
    let shown = column_sets::shown(&*snapshot, notetype.map(NotetypeId))?;
    let keys = |mode: SearchMode| shown.columns.columns(mode).iter().map(ColumnKey::key).collect();
    Ok(ColumnSetShown {
        set: shown.set.into(),
        own: shown.own,
        cards: keys(SearchMode::Cards),
        notes: keys(SearchMode::Notes),
        cards_sort: shown_sort(shown.columns.sort(SearchMode::Cards)),
        notes_sort: shown_sort(shown.columns.sort(SearchMode::Notes)),
    })
}

/// A user change to `set` in `mode`, as one op that records no undo step
/// and keeps the undo steps (V2.4). `written` is false when the stored
/// value stays the same. Refusals (the last column, a reorder that is not
/// every column once, an unsortable column) are `InvalidArgument`; the set
/// of a note type that does not exist is `NotFound`.
#[uniffi::export]
pub fn change_browser_column_set(
    set: ColumnSetId,
    mode: BrowseMode,
    change: ColumnSetChange,
) -> Result<BoolMutation, CoreError> {
    callback::enter("change_browser_column_set");
    let change = Change::try_from(change)?;
    let collection = runtime::host()?.collection()?;
    Ok(column_sets::change(&collection, set.into(), mode.into(), change)?.into())
}

/// Use Standard Columns (V2.2): the note type's own set becomes a copy of
/// the standard set; nothing is written when it has none.
#[uniffi::export]
pub fn use_standard_browser_columns(notetype: i64) -> Result<BoolMutation, CoreError> {
    callback::enter("use_standard_browser_columns");
    let collection = runtime::host()?.collection()?;
    Ok(column_sets::use_standard(&collection, NotetypeId(notetype))?.into())
}

// --------------------------------------------------------------- opening

/// What Browse opens with (`search/filter-tokens` T14): the default search
/// of a first open and the mode the Cards / Notes control shows.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseOpening {
    /// `defaultSearchText` as stored, untrimmed, when it is set
    /// (`browser/config-keys` K6.4); none opens the whole collection.
    pub default_search: Option<String>,
    /// `browserTableShowNotesMode` (K6.1): Browse shows notes.
    pub notes_mode: bool,
}

/// T14 and K6.1: the two keys a first open of Browse reads, in one
/// snapshot of the query reader. It writes nothing.
#[uniffi::export]
pub fn browse_opening() -> Result<BrowseOpening, CoreError> {
    callback::enter("browse_opening");
    let collection = runtime::host()?.collection()?;
    let reader = collection.reader(meguri_storage::ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let config = meguri_search::config::BrowserConfig::read(&*snapshot)?;
    Ok(BrowseOpening {
        default_search: config.default_search(),
        notes_mode: config.notes_mode(),
    })
}

// ------------------------------------------------------ selection counts

/// What the inspector's summary line and the commands' "shown when" read
/// (V7, V8.1), for the selection's target cards and notes (B1).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BrowseSelectionCounts {
    pub cards: u64,
    pub notes: u64,
    pub suspended: u64,
    pub buried: u64,
    /// Distinct home decks (a filtered card's original deck).
    pub decks: u64,
    /// That deck when there is exactly one.
    pub deck: Option<i64>,
    /// The target notes' note types, ascending.
    pub notetypes: Vec<i64>,
}

impl From<SelectionCounts> for BrowseSelectionCounts {
    fn from(counts: SelectionCounts) -> BrowseSelectionCounts {
        BrowseSelectionCounts {
            cards: counts.cards,
            notes: counts.notes,
            suspended: counts.suspended,
            buried: counts.buried,
            decks: counts.decks,
            deck: counts.deck.map(|id| id.0),
            notetypes: counts.notetypes.into_iter().map(|id| id.0).collect(),
        }
    }
}

#[derive(Debug, uniffi::Object)]
pub struct BrowseSelectionCountsJob {
    result: Arc<Mutex<Option<Result<BrowseSelectionCounts, CoreError>>>>,
}

#[uniffi::export]
impl BrowseSelectionCountsJob {
    /// `None` until the job ends; then the counts, or the selection's
    /// refusal (`UnknownHandle`, `StaleRevision`, `OutOfRange`).
    pub fn result(&self) -> Result<Option<BrowseSelectionCounts>, CoreError> {
        callback::enter("BrowseSelectionCountsJob::result");
        lock(&self.result).clone().transpose()
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct BrowseSelectionCountsJobStart {
    pub handle: JobHandle,
    pub result: Arc<BrowseSelectionCountsJob>,
}

/// V7 and V8.1: the counts of `selection`'s target cards and notes, as a
/// job at utility QoS reading one snapshot of a search worker's reader (it scales with
/// the selection, and must not hold the query reader row pages use). A
/// newer call for a selection of the same session cancels this one. A
/// refusal of the selection itself ends the job successfully and `result`
/// returns it.
#[uniffi::export]
pub fn start_browse_selection_counts(
    selection: Arc<BrowseSelection>,
) -> Result<BrowseSelectionCountsJobStart, CoreError> {
    callback::enter("start_browse_selection_counts");
    let slot = Arc::clone(selection.counts_slot());
    let ticket = slot.ticket();
    let engine = runtime::engine()?;
    let result = Arc::new(Mutex::new(None));
    let out = Arc::clone(&result);
    let handle = slot.start(ticket, || {
        Ok(engine.start_utility_job(meguri_engine::JobKind::BulkOp, move |job| {
            let counts = match browse::selection_counts(selection.engine_selection(), &job.cancel) {
                Ok(counts) => Ok(counts.into()),
                Err(BrowseError::Engine(error)) => return Err(error),
                Err(refusal) => Err(CoreError::from(refusal)),
            };
            *lock(&out) = Some(counts);
            Ok(())
        })?)
    })?;
    Ok(BrowseSelectionCountsJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(BrowseSelectionCountsJob { result }),
    })
}

// --------------------------------------------------------- find duplicates

/// What Find Duplicates found (V10).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FindDuplicatesResult {
    /// Each group: two or more note ids with equal keys, ascending; the
    /// groups by their first note. Empty: "No duplicates found."
    pub groups: Vec<Vec<i64>>,
    /// Every grouped note, ascending: the Duplicates token's note ids
    /// (`search/filter-tokens` T3).
    pub note_ids: Vec<i64>,
    /// The target notes read: those whose note type has the field.
    pub considered: u64,
}

impl From<browse::Duplicates> for FindDuplicatesResult {
    fn from(found: browse::Duplicates) -> FindDuplicatesResult {
        FindDuplicatesResult {
            note_ids: found.note_ids().into_iter().map(|id| id.0).collect(),
            groups: found
                .groups
                .into_iter()
                .map(|group| group.into_iter().map(|id| id.0).collect())
                .collect(),
            considered: found.considered,
        }
    }
}

#[derive(Debug, uniffi::Object)]
pub struct FindDuplicatesJob {
    result: Arc<Mutex<Option<Result<FindDuplicatesResult, CoreError>>>>,
}

#[uniffi::export]
impl FindDuplicatesJob {
    /// `None` until the job ends; then what it found, or the selection's
    /// refusal (`UnknownHandle`, `StaleRevision`, `OutOfRange`).
    pub fn result(&self) -> Result<Option<FindDuplicatesResult>, CoreError> {
        callback::enter("FindDuplicatesJob::result");
        lock(&self.result).clone().transpose()
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FindDuplicatesJobStart {
    pub handle: JobHandle,
    pub result: Arc<FindDuplicatesJob>,
}

/// V10's Find: the notes whose values of the field named exactly `field`
/// have equal keys, grouped, among every note of the collection (All Notes,
/// `found` none) or the notes of `found`'s rows (All Found: a selection of
/// every row of the current results). A job reading one snapshot of a
/// search worker's reader, with progress (notes read, of a known total)
/// and Cancel; a replacement, a suspend or a close cancels it. It never
/// writes. It holds one of the search readers until it ends, so a caller
/// whose sheet goes away cancels it. A refusal of the selection itself ends
/// the job successfully and `result` returns it.
#[uniffi::export]
pub fn start_find_duplicates(
    field: String,
    found: Option<Arc<BrowseSelection>>,
) -> Result<FindDuplicatesJobStart, CoreError> {
    callback::enter("start_find_duplicates");
    let engine = runtime::engine()?;
    let result = Arc::new(Mutex::new(None));
    let out = Arc::clone(&result);
    let handle = engine.start_job(meguri_engine::JobKind::BulkOp, move |job| {
        let target = match &found {
            Some(selection) => browse::DuplicatesTarget::Found(selection.engine_selection()),
            None => browse::DuplicatesTarget::AllNotes(engine),
        };
        let found = match browse::find_duplicates(target, &field, &job.cancel, &job.progress) {
            Ok(found) => Ok(found.into()),
            Err(BrowseError::Engine(error)) => return Err(error),
            Err(refusal) => Err(CoreError::from(refusal)),
        };
        *lock(&out) = Some(found);
        Ok(())
    })?;
    Ok(FindDuplicatesJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(FindDuplicatesJob { result }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_duplicates_cross_with_their_note_ids_ascending() {
        let found = browse::Duplicates {
            groups: vec![vec![NoteId(2), NoteId(9)], vec![NoteId(4), NoteId(5), NoteId(7)]],
            considered: 12,
        };
        assert_eq!(
            FindDuplicatesResult::from(found),
            FindDuplicatesResult {
                groups: vec![vec![2, 9], vec![4, 5, 7]],
                note_ids: vec![2, 4, 5, 7, 9],
                considered: 12,
            }
        );
    }

    #[test]
    fn an_older_call_never_starts_after_a_newer_one_reached_the_slot() {
        let newest = Newest::default();
        let (first, second) = (newest.ticket(), newest.ticket());
        let refused = || Err(CoreError::Unavailable { reason: "suspended".into() });
        // The newer call reaches the slot first and fails to start.
        assert!(matches!(newest.start(second, refused), Err(CoreError::Unavailable { .. })));
        // The older one is refused without starting, though no job runs.
        let mut started = false;
        let result = newest.start(first, || {
            started = true;
            refused()
        });
        assert_eq!(result.map(|_| ()), Err(CoreError::Cancelled));
        assert!(!started);
    }

    #[test]
    fn offsets_cross_in_utf16_units() {
        let text = "a\u{1D11E}b";
        assert_eq!(utf16_of(text, 2), 3);
        assert_eq!(chars_of(text, 3).unwrap(), 2);
        assert_eq!(chars_of(text, 4).unwrap(), 3);
        // Inside the surrogate pair: the next character.
        assert_eq!(chars_of(text, 2).unwrap(), 2);
        assert!(chars_of(text, 5).is_err());
    }

    #[test]
    fn every_value_comes_back_as_it_went() {
        let values = vec![
            FilterValue::Deck {
                id: -3,
                native: "A\u{1f}B".into(),
                tree: vec![-3, 9],
            },
            FilterValue::CurrentDeck,
            FilterValue::Tag { name: "x::y".into() },
            FilterValue::Untagged,
            FilterValue::Notetype {
                id: 4,
                name: "Basic".into(),
            },
            FilterValue::CardType { name: "Card 1".into() },
            FilterValue::State {
                state: FilterState::Leech,
            },
            FilterValue::Flag { flag: 7 },
            FilterValue::NoFlag,
            FilterValue::AnyFlag,
            FilterValue::Due { due: FilterDue::Overdue },
            FilterValue::Added {
                period: FilterPeriod::Today,
            },
            FilterValue::Edited {
                period: FilterPeriod::Last7Days,
            },
            FilterValue::Studied {
                period: FilterPeriod::Last30Days,
            },
            FilterValue::Field {
                field: "Back".into(),
                test: FilterFieldTest::Contains { text: "k".into() },
            },
            FilterValue::Field {
                field: "Extra".into(),
                test: FilterFieldTest::Empty,
            },
            FilterValue::Duplicates { note_ids: vec![1, 2] },
            FilterValue::StudiedOn { card_ids: vec![i64::MAX] },
        ];
        let query = FilterQuery {
            tokens: vec![
                FilterToken::Value {
                    values: values.clone(),
                    negated: true,
                },
                FilterToken::Advanced {
                    text: "prop:ivl>3".into(),
                },
            ],
            text: "two words".into(),
        };
        assert_eq!(FilterQuery::from(tokens::FilterQuery::from(query.clone())), query);
    }

    #[test]
    fn a_syntax_insertion_crosses_in_utf16_units() {
        let candidate = syntax::Candidate {
            kind: syntax::CandidateKind::Deck,
            label: "\u{1D11E}x".into(),
            detail: None,
            insert: Some(syntax::Insertion {
                range: 6..8,
                text: "\"deck:\u{1D11E}x\"".into(),
                cursor_after: 15,
            }),
            completes: true,
        };
        // In `é𝄞 c d:xy` the range covers `:x`, after six characters that
        // take seven UTF-16 units; the caret after the insertion (nine
        // characters, ten units) counts in the new text.
        let converted = syntax_candidate(candidate, "\u{e9}\u{1D11E} c d:xy");
        let insert = converted.insert.unwrap();
        assert_eq!((insert.start, insert.end), (7, 9));
        assert_eq!(insert.caret_after, 17);
    }
}
