//! Browse's filter tokens (`specs/search/filter-tokens.md` T1 to T7): the
//! query the search field holds (tokens, then the text), the kinds and
//! values of T3 with their built items, the run text of T4, the text rule
//! of T5, and T7's reading of Anki search text back into a query.
//!
//! The query is plain data: Swift edits it (T8 to T10), the core writes its
//! run text and reads text into it. Everything here reads only the name
//! index it is handed (D6.7), never the collection, config or clock.

mod read;

use meguri_model::deck::human_name;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::note::FIELD_SEPARATOR;

use crate::ast::{CardState, IdKind, TemplateRef, Term};
use crate::builder::{BuilderError, BuiltItem, InvalidItem, Joiner, build, build_query, escape_name, splits_a_term};
use crate::name_index::{DeckName, NameIndex, NotetypeNames};
use crate::parser::parse;

pub use read::{TextReading, classify_text, read_text};

/// What the search field holds (T1): tokens in the order they were made,
/// then the text the user typed after them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterQuery {
    pub tokens: Vec<Token>,
    pub text: String,
}

/// One token of the field (T1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Value(ValueToken),
    /// T6: a piece of Anki search text no kind can show, held as the
    /// canonical text of its item.
    Advanced(String),
}

/// One or more values of one kind, true when any value is (T2), or the
/// negation of that OR when `negated` (is not).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueToken {
    pub values: Vec<Value>,
    pub negated: bool,
}

/// What a token filters on (T3). Field kinds are one per field name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Deck,
    Tag,
    Notetype,
    CardType,
    CardState,
    Flag,
    Due,
    Added,
    Edited,
    Studied,
    Field(String),
    Duplicates,
    StudiedOn,
}

impl Kind {
    /// The same kind: field names compare by unicase equality, as field
    /// terms resolve them (`search/fields` F2 step 4).
    pub fn same(&self, other: &Kind) -> bool {
        match (self, other) {
            (Kind::Field(a), Kind::Field(b)) => meguri_text::fold::compare_folded(a, b).is_eq(),
            (a, b) => a == b,
        }
    }
}

/// One value of T3's table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Deck(DeckValue),
    CurrentDeck,
    /// A tag by its full name; it finds its descendants.
    Tag(String),
    Untagged,
    Notetype(NotetypeValue),
    /// A card type name: every template of that name in any note type.
    CardType(String),
    State(StateValue),
    /// Flags 1 to 7.
    Flag(u8),
    NoFlag,
    AnyFlag,
    Due(DueValue),
    Added(Period),
    Edited(Period),
    Studied(Period),
    Field {
        field: String,
        test: FieldTest,
    },
    /// Made only by Find Duplicates (`browser/workspace` V10).
    Duplicates(Vec<NoteId>),
    /// Made only by a heatmap day (`browser/workspace` V15).
    StudiedOn(Vec<CardId>),
}

/// A deck with its subdecks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckValue {
    pub id: DeckId,
    /// The stored name: levels joined by U+001F.
    pub native: String,
    /// The deck and every deck below it, for J3.5's deck-ids item.
    pub tree: Vec<DeckId>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotetypeValue {
    pub id: NotetypeId,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StateValue {
    New,
    Learning,
    Review,
    Suspended,
    Buried,
    /// `tag:leech`.
    Leech,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DueValue {
    /// Due on day 0.
    Today,
    /// Due, and not due on day 0.
    Overdue,
    /// Due on days 1 to 7.
    Next7Days,
}

/// Today and the days before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Period {
    Today,
    Last7Days,
    Last30Days,
}

impl Period {
    pub fn days(self) -> u64 {
        match self {
            Period::Today => 1,
            Period::Last7Days => 7,
            Period::Last30Days => 30,
        }
    }
}

/// A Field token's test (T3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldTest {
    Contains(String),
    Empty,
    HasContent,
}

impl DeckValue {
    /// The deck `id` of `names`, with every deck its name has below it.
    pub fn of(names: &NameIndex, id: DeckId) -> Option<DeckValue> {
        names
            .decks()
            .iter()
            .find(|deck| deck.id == id)
            .and_then(|deck| deck_value(names, deck))
    }

    /// The name as shown: levels joined by `::`.
    pub fn name(&self) -> String {
        human_name(&self.native)
    }
}

/// The tree is what the deck term finds by name (`names-and-ids` N3.3):
/// every deck whose stored name is the deck's, or starts with it and a
/// level separator, letter case folded. `None` when that expression does
/// not compile (a name past the size limit): the tree is never cut short
/// (J3.5), so the deck is not offered.
pub(crate) fn deck_value(names: &NameIndex, deck: &DeckName) -> Option<DeckValue> {
    let expression = format!("(?i)^{}($|{FIELD_SEPARATOR})", regex::escape(&deck.native));
    let below = regex::Regex::new(&expression).ok()?;
    let tree = names
        .decks()
        .iter()
        .filter(|other| below.is_match(&other.native))
        .map(|other| other.id)
        .collect();
    Some(DeckValue {
        id: deck.id,
        native: deck.native.clone(),
        tree,
    })
}

impl NotetypeValue {
    pub fn of(notetype: &NotetypeNames) -> NotetypeValue {
        NotetypeValue {
            id: notetype.id,
            name: notetype.name.clone(),
        }
    }
}

impl Value {
    pub fn kind(&self) -> Kind {
        match self {
            Value::Deck(_) | Value::CurrentDeck => Kind::Deck,
            Value::Tag(_) | Value::Untagged => Kind::Tag,
            Value::Notetype(_) => Kind::Notetype,
            Value::CardType(_) => Kind::CardType,
            Value::State(_) => Kind::CardState,
            Value::Flag(_) | Value::NoFlag | Value::AnyFlag => Kind::Flag,
            Value::Due(_) => Kind::Due,
            Value::Added(_) => Kind::Added,
            Value::Edited(_) => Kind::Edited,
            Value::Studied(_) => Kind::Studied,
            Value::Field { field, .. } => Kind::Field(field.clone()),
            Value::Duplicates(_) => Kind::Duplicates,
            Value::StudiedOn(_) => Kind::StudiedOn,
        }
    }

    /// The value's built item (T3), or `None` when T3 does not offer the
    /// value because J3 refuses its item (a negative id, a field name J3.3
    /// refuses, an empty name; see [`card_type_text`] for card types).
    pub fn item(&self) -> Option<BuiltItem> {
        let item = self.candidate()?;
        build_query(&item).ok().map(|_| item)
    }

    /// Whether T3 offers the value.
    pub fn is_offered(&self) -> bool {
        self.item().is_some()
    }

    fn candidate(&self) -> Option<BuiltItem> {
        Some(match self {
            Value::Deck(deck) => deck_item(deck),
            Value::CurrentDeck => BuiltItem::CurrentDeck,
            Value::Tag(name) => BuiltItem::Tag(name.clone()),
            Value::Untagged => BuiltItem::Untagged,
            Value::Notetype(notetype) => {
                let by_name = BuiltItem::Notetype(notetype.name.clone());
                match refusal(&by_name) {
                    Some(InvalidItem::UnrepresentableName) => BuiltItem::NotetypeId(notetype.id),
                    _ => by_name,
                }
            }
            Value::CardType(name) => BuiltItem::Parsed {
                text: card_type_text(name)?,
                field_text: false,
            },
            Value::State(state) => match state {
                StateValue::New => BuiltItem::State(CardState::New),
                StateValue::Learning => BuiltItem::State(CardState::Learn),
                StateValue::Review => BuiltItem::State(CardState::Review),
                StateValue::Suspended => BuiltItem::State(CardState::Suspended),
                StateValue::Buried => BuiltItem::State(CardState::Buried),
                StateValue::Leech => BuiltItem::Tag("leech".to_owned()),
            },
            Value::Flag(flag) => BuiltItem::Flag(*flag),
            Value::NoFlag => BuiltItem::NoFlag,
            Value::AnyFlag => BuiltItem::AnyFlag,
            Value::Due(due) => match due {
                DueValue::Today => BuiltItem::DueOn(0),
                DueValue::Overdue => BuiltItem::Group {
                    joiner: Joiner::And,
                    items: vec![
                        BuiltItem::State(CardState::Due),
                        BuiltItem::Negated(Box::new(BuiltItem::DueOn(0))),
                    ],
                },
                DueValue::Next7Days => BuiltItem::Parsed {
                    text: "prop:due>=1 prop:due<=7".to_owned(),
                    field_text: false,
                },
            },
            Value::Added(period) => BuiltItem::Added(period.days()),
            Value::Edited(period) => BuiltItem::Edited(period.days()),
            Value::Studied(period) => BuiltItem::Answered {
                days: period.days(),
                button: None,
            },
            Value::Field { field, test } => match test {
                FieldTest::Contains(text) => BuiltItem::FieldContains {
                    field: field.clone(),
                    text: text.clone(),
                },
                FieldTest::Empty => BuiltItem::FieldEmpty(field.clone()),
                FieldTest::HasContent => BuiltItem::FieldPresent(field.clone()),
            },
            Value::Duplicates(ids) => BuiltItem::Ids {
                kind: IdKind::Notes,
                ids: ids.iter().map(|id| id.0).collect(),
            },
            Value::StudiedOn(ids) => BuiltItem::Ids {
                kind: IdKind::Cards,
                ids: ids.iter().map(|id| id.0).collect(),
            },
        })
    }
}

fn refusal(item: &BuiltItem) -> Option<InvalidItem> {
    match build_query(item) {
        Err(BuilderError::InvalidItem { reason, .. }) => Some(reason),
        _ => None,
    }
}

/// T3's Deck: the deck by name, or J3.5's deck-ids item (the deck and its
/// subdecks either way) when J3 refuses the name or a stored level holds
/// `::`. Every id of the list is kept: a negative one refuses the item.
fn deck_item(deck: &DeckValue) -> BuiltItem {
    let by_ids = || BuiltItem::Ids {
        kind: IdKind::Decks,
        ids: deck.tree.iter().map(|id| id.0).collect(),
    };
    if deck.native.split(FIELD_SEPARATOR).any(|level| level.contains("::")) {
        return by_ids();
    }
    let by_name = BuiltItem::Deck(deck.name());
    match refusal(&by_name) {
        Some(InvalidItem::UnrepresentableName) => by_ids(),
        _ => by_name,
    }
}

/// T3's Card Type: `card:` + E(s) as F4 and F5 write it, or `None` when it
/// is not offered: an empty name; a name of ASCII digits (T3), and any
/// other name whose text reads back as an ordinal (`+1`, `grammar.md` G8);
/// a name E would change or that is not in NFC, which would find
/// look-alikes or nothing (as J3.5 for note types).
pub fn card_type_text(name: &str) -> Option<String> {
    let digits = name.bytes().all(|b| b.is_ascii_digit());
    if name.is_empty() || digits || name.chars().any(splits_a_term) || !is_nfc(name) {
        return None;
    }
    let text = crate::canonical::term_text(&Term::Template(TemplateRef::Name(escape_name(name))));
    let query = parse(&text).ok()?;
    match &query.node(query.root()).kind {
        crate::ast::NodeKind::Term(Term::Template(TemplateRef::Name(_))) => Some(text),
        _ => None,
    }
}

fn is_nfc(text: &str) -> bool {
    matches!(meguri_text::normalize::nfc(text), std::borrow::Cow::Borrowed(_))
}

impl ValueToken {
    pub fn new(values: Vec<Value>, negated: bool) -> ValueToken {
        ValueToken { values, negated }
    }

    /// The kind of its values; `None` when it has none or several kinds.
    pub fn kind(&self) -> Option<Kind> {
        let kind = self.values.first()?.kind();
        self.values.iter().all(|value| value.kind().same(&kind)).then_some(kind)
    }
}

impl Token {
    pub fn kind(&self) -> Option<Kind> {
        match self {
            Token::Value(token) => token.kind(),
            Token::Advanced(_) => None,
        }
    }
}

/// Why a query has no run text. A query the UI makes has one: these name
/// a token that T3, T10 or T7 never make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunTextError {
    /// A value token with no values, or values of several kinds.
    InvalidToken { token: usize },
    /// A value T3 does not offer (it has no built item).
    NotOffered { token: usize, value: usize },
    /// An advanced token whose text does not parse (its path's first step
    /// is the token's index).
    Build(BuilderError),
}

impl FilterQuery {
    pub fn new(tokens: Vec<Token>, text: impl Into<String>) -> FilterQuery {
        FilterQuery {
            tokens,
            text: text.into(),
        }
    }

    /// No token and no text (T2: the whole collection).
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty() && self.text.is_empty()
    }

    /// T4: the Anki search text the query stands for. Text that holds Anki
    /// syntax runs as T7 reads it (T5, T8's live search); text that does
    /// not parse runs as literal words. A query that gives no item runs
    /// the empty text (T4 step 4).
    pub fn run_text(&self, names: &NameIndex) -> Result<String, RunTextError> {
        match classify_text(&self.text, names) {
            TextReading::Syntax => {
                let committed = self.commit(names);
                let reading = classify_text(&committed.text, names);
                committed.run_text_of(reading)
            }
            reading => self.run_text_of(reading),
        }
    }

    /// T4 with T5's reading of the text; text that still holds syntax
    /// (never, after a commit) runs as literal words.
    fn run_text_of(&self, reading: TextReading) -> Result<String, RunTextError> {
        let mut items = Vec::with_capacity(self.tokens.len() + 1);
        for (k, token) in self.tokens.iter().enumerate() {
            items.push(token_item(k, token)?);
        }
        match reading {
            TextReading::Items(text_items) => items.extend(text_items),
            TextReading::Syntax => items.extend(read::literal_words(&self.text)),
        }
        if items.is_empty() {
            return Ok(String::new());
        }
        build(&BuiltItem::Group {
            joiner: Joiner::And,
            items,
        })
        .map_err(RunTextError::Build)
    }

    /// T8's commit: text that holds Anki syntax is read by T7, its tokens
    /// put after the query's own (T7 step 4 counting those too) and its
    /// text items left as the text. Text that stays text (T5), or does not
    /// parse, stays as typed.
    pub fn commit(&self, names: &NameIndex) -> FilterQuery {
        match classify_text(&self.text, names) {
            TextReading::Syntax => {
                let read = read::Reader::new(names).read(&self.text, &self.tokens);
                let mut tokens = self.tokens.clone();
                tokens.extend(read.tokens);
                FilterQuery {
                    tokens,
                    text: read.text,
                }
            }
            TextReading::Items(_) => self.clone(),
        }
    }
}

/// T4 steps 1 and 2: a value token's item, the group OR of its values'
/// items when it has several, negated by J5 when it is "is not"; an
/// advanced token's parsed text.
fn token_item(k: usize, token: &Token) -> Result<BuiltItem, RunTextError> {
    match token {
        Token::Advanced(text) => Ok(BuiltItem::Parsed {
            text: text.clone(),
            field_text: false,
        }),
        Token::Value(value_token) => {
            if value_token.kind().is_none() {
                return Err(RunTextError::InvalidToken { token: k });
            }
            let mut items = Vec::with_capacity(value_token.values.len());
            for (v, value) in value_token.values.iter().enumerate() {
                items.push(value.item().ok_or(RunTextError::NotOffered { token: k, value: v })?);
            }
            let item = if items.len() == 1 {
                items.swap_remove(0)
            } else {
                BuiltItem::Group {
                    joiner: Joiner::Or,
                    items,
                }
            };
            Ok(if value_token.negated {
                BuiltItem::Negated(Box::new(item))
            } else {
                item
            })
        }
    }
}
