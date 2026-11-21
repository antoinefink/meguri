//! Browse's suggestions (`specs/search/filter-tokens.md` T12.1 to T12.3):
//! the list under the search field, computed from the name index it is
//! handed (`search/editing-services` D6.7), never the collection. Counts
//! are not here: they arrive later from the engine's filter counts (T12.4).
//!
//! The list for a keystroke must come back within one frame with 10,000
//! tags (T12.3 step 5): names are matched on their folded texts, and a
//! value is built only for the rows that are shown.

use std::collections::{HashMap, HashSet};

use meguri_model::ids::NotetypeId;
use meguri_text::fold::folded;

use crate::builder::{BuiltItem, build};
use crate::lexer::is_white_space;
use crate::name_index::NameIndex;
use crate::suggest::{Candidate, Class, MOST, Matcher, field_names, key_at, template_names};
use crate::tail::InvalidOffset;

use super::{DeckValue, DueValue, FilterQuery, NotetypeValue, Period, StateValue, Token, Value, card_type_text};

/// A section of the list, in T12.3 step 2's order where it ranks rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Section {
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
    /// T12.1's first row: the kinds, each opening its value list.
    FilterBy,
}

impl Section {
    /// Filter By's kinds, in T12.1's order: Deck, Tag, Note Type, Card
    /// Type, Card State, Flag, Due, Added, Edited, Studied, Field.
    pub const KINDS: [Section; 11] = [
        Section::Decks,
        Section::Tags,
        Section::Notetypes,
        Section::CardTypes,
        Section::CardState,
        Section::Flag,
        Section::Due,
        Section::Added,
        Section::Edited,
        Section::Studied,
        Section::Fields,
    ];
}

/// A built-in saved search (T13), never stored in the collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    DueToday,
    AddedInLast7Days,
    Leeches,
}

impl BuiltIn {
    /// T13's order.
    pub const ALL: [BuiltIn; 3] = [BuiltIn::DueToday, BuiltIn::AddedInLast7Days, BuiltIn::Leeches];

    /// The query it gives: one value token.
    pub fn value(self) -> Value {
        match self {
            BuiltIn::DueToday => Value::Due(DueValue::Today),
            BuiltIn::AddedInLast7Days => Value::Added(Period::Last7Days),
            BuiltIn::Leeches => Value::State(StateValue::Leech),
        }
    }
}

/// The labels fixed values and built-ins match by (T12.2, T15). Swift
/// passes its localized strings; the default is T15's English.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
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

impl Default for Labels {
    fn default() -> Labels {
        let s = |text: &str| text.to_owned();
        Labels {
            current_deck: s("Current Deck"),
            untagged: s("Untagged"),
            new: s("New"),
            learning: s("Learning"),
            review: s("Review"),
            suspended: s("Suspended"),
            buried: s("Buried"),
            leech: s("Leech"),
            no_flag: s("No Flag"),
            any_flag: s("Any Flag"),
            today: s("Today"),
            overdue: s("Overdue"),
            next_7_days: s("Next 7 Days"),
            last_7_days: s("Last 7 Days"),
            last_30_days: s("Last 30 Days"),
            due_today: s("Due Today"),
            added_in_last_7_days: s("Added in Last 7 Days"),
            leeches: s("Leeches"),
        }
    }
}

impl Labels {
    pub fn built_in(&self, built_in: BuiltIn) -> &str {
        match built_in {
            BuiltIn::DueToday => &self.due_today,
            BuiltIn::AddedInLast7Days => &self.added_in_last_7_days,
            BuiltIn::Leeches => &self.leeches,
        }
    }

    fn period(&self, period: Period) -> &str {
        match period {
            Period::Today => &self.today,
            Period::Last7Days => &self.last_7_days,
            Period::Last30Days => &self.last_30_days,
        }
    }

    /// A fixed value's label; a flag's is its shown name.
    fn fixed(&self, value: &Value, names: &NameIndex) -> String {
        match value {
            Value::CurrentDeck => self.current_deck.clone(),
            Value::Untagged => self.untagged.clone(),
            Value::State(state) => match state {
                StateValue::New => self.new.clone(),
                StateValue::Learning => self.learning.clone(),
                StateValue::Review => self.review.clone(),
                StateValue::Suspended => self.suspended.clone(),
                StateValue::Buried => self.buried.clone(),
                StateValue::Leech => self.leech.clone(),
            },
            Value::Flag(flag) => names.flag_names()[usize::from(flag.saturating_sub(1)).min(6)].clone(),
            Value::NoFlag => self.no_flag.clone(),
            Value::AnyFlag => self.any_flag.clone(),
            Value::Due(due) => match due {
                DueValue::Today => self.today.clone(),
                DueValue::Overdue => self.overdue.clone(),
                DueValue::Next7Days => self.next_7_days.clone(),
            },
            Value::Added(period) | Value::Edited(period) | Value::Studied(period) => self.period(*period).to_owned(),
            _ => String::new(),
        }
    }
}

/// One row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// "Contains “w”": keeps the word as text and commits (T12.1).
    Contains(String),
    BuiltIn(BuiltIn),
    /// A user saved search: accepting it replaces the query by T7's reading
    /// of its text (T9).
    SavedSearch { name: String, text: String },
    /// Filter By: opens the kind's value list.
    FilterBy(Section),
    /// A value of T3.
    Value(Value),
    /// A field name, with the note types that have it (T12.1); accepting it
    /// shows its three tests.
    Field { name: String, notetypes: Vec<NotetypeId> },
    /// A card type value, with the note types that have it.
    CardType { value: Value, notetypes: Vec<NotetypeId> },
    /// Ends a section cut short: opens that section's value list.
    ShowAll(Section),
    /// T13's Save Search…, last while the query is not empty.
    SaveSearch,
    /// D6's candidate while Anki syntax is typed at the caret (T12.2).
    Syntax(Candidate),
}

/// One row and the section it is shown under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionRow {
    pub section: Option<Section>,
    pub row: Row,
}

impl SuggestionRow {
    /// What Swift records when the user accepts the row and passes back in
    /// [`SuggestRequest::recent`] (newest first): an identity of the
    /// accepted thing, or D6's insert text for a syntax row.
    pub fn key(&self) -> Option<String> {
        row_key(&self.row)
    }
}

fn row_key(row: &Row) -> Option<String> {
    Some(match row {
        Row::BuiltIn(built_in) => format!("builtin:{built_in:?}"),
        Row::SavedSearch { name, .. } => format!("saved:{name}"),
        Row::Value(value) | Row::CardType { value, .. } => value_key(value)?,
        Row::Field { name, .. } => format!("field:{}", folded(name)),
        Row::Syntax(candidate) => candidate.insert.as_ref()?.text.clone(),
        Row::Contains(_) | Row::FilterBy(_) | Row::ShowAll(_) | Row::SaveSearch => return None,
    })
}

/// A value's identity: a deck or note type by id, a name with letter case
/// folded, a fixed value by itself.
fn value_key(value: &Value) -> Option<String> {
    Some(match value {
        Value::Deck(deck) => format!("deck:{}", deck.id.0),
        Value::CurrentDeck => "deck:current".to_owned(),
        Value::Tag(name) => format!("tag:{}", folded(name)),
        Value::Untagged => "untagged".to_owned(),
        Value::Notetype(notetype) => format!("notetype:{}", notetype.id.0),
        Value::CardType(name) => format!("cardtype:{}", folded(name)),
        Value::State(state) => format!("state:{state:?}"),
        Value::Flag(flag) => format!("flag:{flag}"),
        Value::NoFlag => "flag:none".to_owned(),
        Value::AnyFlag => "flag:any".to_owned(),
        Value::Due(due) => format!("due:{due:?}"),
        Value::Added(period) => format!("added:{period:?}"),
        Value::Edited(period) => format!("edited:{period:?}"),
        Value::Studied(period) => format!("studied:{period:?}"),
        Value::Field { .. } | Value::Duplicates(_) | Value::StudiedOn(_) => return None,
    })
}

/// What the list is asked for.
#[derive(Clone, Copy, Debug)]
pub struct SuggestRequest<'a> {
    /// The field's query.
    pub query: &'a FilterQuery,
    /// The caret, in characters of the query's text.
    pub caret: usize,
    /// A kind's value list (from Filter By, "Add Another…", "Show All" or
    /// the Filters sheet), narrowed by the word at the caret when there is
    /// one; `None` for the field's own list.
    pub scope: Option<Section>,
    /// Keys ([`SuggestionRow::key`]) of rows the user accepted, newest
    /// first (device state, never in the collection).
    pub recent: &'a [String],
}

/// T12: the rows for `request`, in display order.
pub fn suggest(names: &NameIndex, labels: &Labels, request: &SuggestRequest<'_>) -> Result<Vec<SuggestionRow>, InvalidOffset> {
    let text = &request.query.text;
    let chars: Vec<char> = text.chars().collect();
    if request.caret > chars.len() {
        return Err(InvalidOffset);
    }
    let start = chars[..request.caret]
        .iter()
        .rposition(|c| is_white_space(*c))
        .map_or(0, |at| at + 1);
    let word: String = chars[start..request.caret].iter().collect();
    let lists = Lists::new(names, labels, request);
    if let Some(section) = request.scope {
        return Ok(lists.scoped(section, &word));
    }
    let mut rows = if word.is_empty() {
        lists.opening()
    } else if key_at(text, request.caret).is_some() {
        // T12.2: Anki syntax at the caret completes as D6 says.
        crate::suggest::suggest(text, request.caret, MOST, request.recent, names)?
            .into_iter()
            .map(|candidate| SuggestionRow {
                section: None,
                row: Row::Syntax(candidate),
            })
            .collect()
    } else {
        lists.word(&word)
    };
    if !request.query.is_empty() {
        rows.push(SuggestionRow {
            section: None,
            row: Row::SaveSearch,
        });
    }
    Ok(rows)
}

/// Where a candidate comes from; a value is built only when it is shown.
#[derive(Clone, Debug)]
enum Source {
    BuiltIn(BuiltIn),
    Saved(usize),
    Fixed(Value),
    Deck(usize),
    Tag(usize),
    Notetype(usize),
    CardType(String, Vec<NotetypeId>),
    Field(String, Vec<NotetypeId>),
}

struct Pending {
    class: Class,
    section: Section,
    recent: usize,
    order: usize,
    source: Source,
}

/// The values the query's **is** tokens hold, which are not suggested again
/// (T12.1).
#[derive(Default)]
struct Taken {
    decks: HashSet<i64>,
    tags: HashSet<String>,
    notetypes: HashSet<i64>,
    card_types: HashSet<String>,
    fixed: Vec<Value>,
}

impl Taken {
    fn of(query: &FilterQuery) -> Taken {
        let mut taken = Taken::default();
        for token in &query.tokens {
            let Token::Value(token) = token else { continue };
            if token.negated {
                continue;
            }
            for value in &token.values {
                match value {
                    Value::Deck(deck) => {
                        taken.decks.insert(deck.id.0);
                    }
                    Value::Tag(name) => {
                        taken.tags.insert(folded(name));
                    }
                    Value::Notetype(notetype) => {
                        taken.notetypes.insert(notetype.id.0);
                    }
                    Value::CardType(name) => {
                        taken.card_types.insert(folded(name));
                    }
                    other => taken.fixed.push(other.clone()),
                }
            }
        }
        taken
    }
}

struct Lists<'a> {
    names: &'a NameIndex,
    labels: &'a Labels,
    taken: Taken,
    recent: HashMap<&'a str, usize>,
}

/// T3's fixed values of a section, in its order.
fn fixed_values(section: Section) -> Vec<Value> {
    let periods = [Period::Today, Period::Last7Days, Period::Last30Days];
    match section {
        Section::CardState => [
            StateValue::New,
            StateValue::Learning,
            StateValue::Review,
            StateValue::Suspended,
            StateValue::Buried,
            StateValue::Leech,
        ]
        .into_iter()
        .map(Value::State)
        .collect(),
        Section::Flag => (1..=7)
            .map(Value::Flag)
            .chain([Value::NoFlag, Value::AnyFlag])
            .collect(),
        Section::Due => [DueValue::Today, DueValue::Overdue, DueValue::Next7Days]
            .into_iter()
            .map(Value::Due)
            .collect(),
        Section::Added => periods.into_iter().map(Value::Added).collect(),
        Section::Edited => periods.into_iter().map(Value::Edited).collect(),
        Section::Studied => periods.into_iter().map(Value::Studied).collect(),
        Section::Decks => vec![Value::CurrentDeck],
        Section::Tags => vec![Value::Untagged],
        _ => Vec::new(),
    }
}

impl<'a> Lists<'a> {
    fn new(names: &'a NameIndex, labels: &'a Labels, request: &SuggestRequest<'a>) -> Lists<'a> {
        let mut recent = HashMap::new();
        for (k, key) in request.recent.iter().enumerate() {
            recent.entry(key.as_str()).or_insert(k);
        }
        Lists {
            names,
            labels,
            taken: Taken::of(request.query),
            recent,
        }
    }

    /// T12.1's first row: Saved Searches (the built-ins, then the user's),
    /// then Filter By; at most 50 rows (R1104).
    fn opening(&self) -> Vec<SuggestionRow> {
        let mut rows: Vec<SuggestionRow> = self.saved_rows();
        // Every kind stays: the saved searches give way, ending with Show
        // All when they are cut, 50 rows in all.
        let room = MOST - Section::KINDS.len();
        if rows.len() > room {
            rows.truncate(room - 1);
            rows.push(SuggestionRow {
                section: Some(Section::SavedSearches),
                row: Row::ShowAll(Section::SavedSearches),
            });
        }
        rows.extend(Section::KINDS.iter().map(|kind| SuggestionRow {
            section: Some(Section::FilterBy),
            row: Row::FilterBy(*kind),
        }));
        rows
    }

    fn saved_rows(&self) -> Vec<SuggestionRow> {
        let built_in = BuiltIn::ALL.iter().map(|b| Row::BuiltIn(*b));
        let user = self.names.saved_searches().iter().map(|saved| Row::SavedSearch {
            name: saved.name.clone(),
            text: saved.text.clone(),
        });
        built_in
            .chain(user)
            .map(|row| SuggestionRow {
                section: Some(Section::SavedSearches),
                row,
            })
            .collect()
    }

    /// A candidate's place among the recently accepted rows (newest
    /// first); the key is made only when there are recent rows.
    fn rank(&self, key: impl FnOnce() -> Option<String>) -> usize {
        if self.recent.is_empty() {
            return usize::MAX;
        }
        key().and_then(|key| self.recent.get(key.as_str()).copied())
            .unwrap_or(usize::MAX)
    }

    fn fixed_taken(&self, value: &Value) -> bool {
        self.taken.fixed.contains(value)
    }

    /// Every candidate of `sections` matching `word` (every one when it is
    /// empty, all in the prefix class), each with its order in its section.
    /// Names are matched before anything else is made of them. Current
    /// Deck and Untagged are in a kind's value list only (`scoped`): the
    /// word's list matches deck and tag names (T12.1).
    fn candidates(&self, sections: &[Section], word: &str, scoped: bool) -> Vec<Pending> {
        let matcher = Matcher::new(word);
        let empty = word.is_empty();
        let class_of = |class: Option<Class>| if empty { Some(Class::Prefix) } else { class };
        let mut out = Vec::new();
        let mut push = |class: Class, section: Section, order: usize, source: Source, recent: usize| {
            out.push(Pending {
                class,
                section,
                recent: if empty { usize::MAX } else { recent },
                order,
                source,
            });
        };
        let fixed = |section: Section, push: &mut dyn FnMut(Class, Section, usize, Source, usize)| {
            for (k, value) in fixed_values(section).into_iter().enumerate() {
                if self.fixed_taken(&value) {
                    continue;
                }
                let label = self.labels.fixed(&value, self.names);
                if let Some(class) = class_of(matcher.substring(&label)) {
                    let recent = self.rank(|| value_key(&value));
                    push(class, section, k, Source::Fixed(value), recent);
                }
            }
        };
        for &section in sections {
            match section {
                Section::SavedSearches => {
                    for (k, built_in) in BuiltIn::ALL.iter().enumerate() {
                        if let Some(class) = class_of(matcher.substring(self.labels.built_in(*built_in))) {
                            let recent = self.rank(|| row_key(&Row::BuiltIn(*built_in)));
                            push(class, section, k, Source::BuiltIn(*built_in), recent);
                        }
                    }
                    for (k, saved) in self.names.saved_searches().iter().enumerate() {
                        if let Some(class) = class_of(matcher.substring(&saved.name)) {
                            let recent = self.rank(|| Some(format!("saved:{}", saved.name)));
                            push(class, section, 3 + k, Source::Saved(k), recent);
                        }
                    }
                }
                Section::Decks => {
                    if scoped {
                        fixed(section, &mut push);
                    }
                    for (k, deck) in self.names.decks().iter().enumerate() {
                        let Some(class) = class_of(matcher.components(deck.native.split('\u{1f}'))) else {
                            continue;
                        };
                        if self.taken.decks.contains(&deck.id.0) {
                            continue;
                        }
                        let recent = self.rank(|| Some(format!("deck:{}", deck.id.0)));
                        push(class, section, 1 + k, Source::Deck(k), recent);
                    }
                }
                Section::Tags => {
                    if scoped {
                        fixed(section, &mut push);
                    }
                    for (k, tag) in self.names.tags().iter().enumerate() {
                        let Some(class) = class_of(matcher.components(tag.split("::"))) else {
                            continue;
                        };
                        if !self.taken.tags.is_empty() && self.taken.tags.contains(&folded(tag)) {
                            continue;
                        }
                        let recent = self.rank(|| Some(format!("tag:{}", folded(tag))));
                        push(class, section, 1 + k, Source::Tag(k), recent);
                    }
                }
                Section::Notetypes => {
                    for (k, notetype) in self.names.notetypes().iter().enumerate() {
                        if self.taken.notetypes.contains(&notetype.id.0) {
                            continue;
                        }
                        if let Some(class) = class_of(matcher.substring(&notetype.name)) {
                            let recent = self.rank(|| Some(format!("notetype:{}", notetype.id.0)));
                            push(class, section, k, Source::Notetype(k), recent);
                        }
                    }
                }
                Section::CardTypes => {
                    let all: Vec<_> = self.names.notetypes().iter().collect();
                    for (k, (name, ids)) in template_names(&all).into_iter().enumerate() {
                        let Some(class) = class_of(matcher.substring(&name)) else { continue };
                        let key = folded(&name);
                        if self.taken.card_types.contains(&key) {
                            continue;
                        }
                        let recent = self.rank(|| Some(format!("cardtype:{key}")));
                        push(class, section, k, Source::CardType(name, ids), recent);
                    }
                }
                Section::Fields => {
                    let all: Vec<_> = self.names.notetypes().iter().collect();
                    for (k, (name, ids)) in field_names(&all).into_iter().enumerate() {
                        if let Some(class) = class_of(matcher.substring(&name)) {
                            let recent = self.rank(|| Some(format!("field:{}", folded(&name))));
                            push(class, section, k, Source::Field(name, ids), recent);
                        }
                    }
                }
                Section::FilterBy => {}
                other => fixed(other, &mut push),
            }
        }
        out
    }

    /// The row a candidate shows, or `None` when T3 does not offer its
    /// value.
    fn row(&self, source: &Source) -> Option<Row> {
        Some(match source {
            Source::BuiltIn(built_in) => Row::BuiltIn(*built_in),
            Source::Saved(k) => {
                let saved = &self.names.saved_searches()[*k];
                Row::SavedSearch {
                    name: saved.name.clone(),
                    text: saved.text.clone(),
                }
            }
            Source::Fixed(value) => Row::Value(value.clone()),
            Source::Deck(k) => {
                let value = Value::Deck(DeckValue::of(self.names, self.names.decks()[*k].id)?);
                value.is_offered().then_some(Row::Value(value))?
            }
            Source::Tag(k) => {
                let name = &self.names.tags()[*k];
                build(&BuiltItem::Tag(name.clone())).ok()?;
                Row::Value(Value::Tag(name.clone()))
            }
            Source::Notetype(k) => {
                let value = Value::Notetype(NotetypeValue::of(&self.names.notetypes()[*k]));
                value.is_offered().then_some(Row::Value(value))?
            }
            Source::CardType(name, ids) => {
                card_type_text(name)?;
                Row::CardType {
                    value: Value::CardType(name.clone()),
                    notetypes: ids.clone(),
                }
            }
            Source::Field(name, ids) => {
                build(&BuiltItem::FieldPresent(name.clone())).ok()?;
                Row::Field {
                    name: name.clone(),
                    notetypes: ids.clone(),
                }
            }
        })
    }

    /// T12.1's second row with T12.3's order and limits.
    fn word(&self, word: &str) -> Vec<SuggestionRow> {
        const SECTIONS: [Section; 12] = [
            Section::SavedSearches,
            Section::CardState,
            Section::Flag,
            Section::Due,
            Section::Added,
            Section::Edited,
            Section::Studied,
            Section::Decks,
            Section::Tags,
            Section::Notetypes,
            Section::CardTypes,
            Section::Fields,
        ];
        let mut candidates = self.candidates(&SECTIONS, word, false);
        // Step 2: the prefix class first, then by section, recently
        // accepted first, then each section's own order.
        candidates.sort_by_key(|c| (c.class, c.section, c.recent, c.order));
        let mut by_section: Vec<(Section, Vec<(usize, &Pending)>)> = Vec::new();
        for (at, candidate) in candidates.iter().enumerate() {
            match by_section.iter_mut().find(|(section, _)| *section == candidate.section) {
                Some((_, list)) => list.push((at, candidate)),
                None => by_section.push((candidate.section, vec![(at, candidate)])),
            }
        }
        // Each section's first shown rows, and whether more follow.
        let mut shown: Vec<(usize, Section, Vec<Row>, bool)> = Vec::new();
        for (section, list) in &by_section {
            let mut rows = Vec::new();
            let mut first = None;
            let mut more = false;
            for (at, candidate) in list {
                let Some(row) = self.row(&candidate.source) else { continue };
                if rows.len() == 5 {
                    more = true;
                    break;
                }
                first.get_or_insert(*at);
                rows.push(row);
            }
            if let Some(first) = first {
                shown.push((first, *section, rows, more));
            }
        }
        // Step 3: sections in the order of their first row.
        shown.sort_by_key(|(first, ..)| *first);
        let mut out = vec![SuggestionRow {
            section: None,
            row: Row::Contains(word.to_owned()),
        }];
        let mut total = 1;
        for (_, section, rows, more) in shown {
            if total == MOST {
                break;
            }
            let room = MOST - total;
            let cut = more || rows.len() > room;
            for row in rows.into_iter().take(room) {
                total += 1;
                out.push(SuggestionRow {
                    section: Some(section),
                    row,
                });
            }
            if cut {
                out.push(SuggestionRow {
                    section: Some(section),
                    row: Row::ShowAll(section),
                });
            }
        }
        out
    }

    /// T12.1's third row: a kind's values, fixed values in T3's order then
    /// names in D6.5 step 3's order; narrowed by `word` when there is one,
    /// then the prefix class first and recently accepted first in a class.
    fn scoped(&self, section: Section, word: &str) -> Vec<SuggestionRow> {
        if section == Section::FilterBy {
            return Section::KINDS
                .iter()
                .map(|kind| SuggestionRow {
                    section: Some(Section::FilterBy),
                    row: Row::FilterBy(*kind),
                })
                .collect();
        }
        let mut candidates = self.candidates(&[section], word, true);
        candidates.sort_by_key(|c| (c.class, c.recent, c.order));
        candidates
            .iter()
            .filter_map(|candidate| self.row(&candidate.source))
            .map(|row| SuggestionRow {
                section: Some(section),
                row,
            })
            .collect()
    }
}
