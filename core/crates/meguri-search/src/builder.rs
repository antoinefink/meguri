//! Built searches (`specs/search/join-and-replace.md` J1, J3 to J5, and the
//! two field items of `specs/search/filter-tokens.md` T3.1): the search text
//! Meguri writes for a UI action from a structured item. Nothing here reads
//! a collection, config or clock. Items are walked with an explicit stack
//! and dropped without recursion, and a parsed-text item's query is copied
//! node by node, so no nesting depth exhausts the call stack (`errors.md`
//! E6).

use std::borrow::Cow;

use meguri_model::ids::NotetypeId;

use crate::ast::{
    CardState, Comparison, IdKind, NodeId, NodeKind, Property, Query, QueryBuilder, TemplateRef, Term, TextMode,
};
use crate::error::SearchError;
use crate::lexer::is_white_space;
use crate::parser::parse;

/// The keys `grammar.md` G6 reserves, lower case.
pub const RESERVED_KEYS: [&str; 23] = [
    "deck",
    "note",
    "tag",
    "card",
    "flag",
    "resched",
    "prop",
    "added",
    "edited",
    "introduced",
    "rated",
    "is",
    "did",
    "mid",
    "nid",
    "cid",
    "re",
    "nc",
    "sc",
    "w",
    "dupe",
    "has-cd",
    "preset",
];

/// How a group joins its items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Joiner {
    And,
    Or,
}

/// One built item of J3. Names are given as stored (a deck's full name with
/// `::` between levels); the builder escapes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuiltItem {
    WholeCollection,
    /// A deck by its full name (J3.1, J3.5).
    Deck(String),
    CurrentDeck,
    AnyDeck,
    /// A tag by name, normal mode, or J3.2's regex form.
    Tag(String),
    AnyTag,
    Untagged,
    /// A note type by name (J3.5).
    Notetype(String),
    AnyNotetype,
    NotetypeId(NotetypeId),
    /// A template by 0-based ordinal, 0 to 65534.
    Template(u32),
    /// The field holds anything: `f:_*`.
    FieldPresent(String),
    /// The field's text matches `text` exactly (its wildcards escaped).
    FieldText { field: String, text: String },
    /// T3.1: the field contains `text`: `f:*text*`.
    FieldContains { field: String, text: String },
    /// T3.1: the field is empty: `f:`.
    FieldEmpty(String),
    /// Plain text, its wildcards escaped.
    Literal(String),
    /// Flags 1 to 7; 0 is [`BuiltItem::NoFlag`].
    Flag(u8),
    NoFlag,
    AnyFlag,
    State(CardState),
    /// Added in the last `d` days; 0 is raised to 1.
    Added(u64),
    /// Edited in the last `d` days; 0 is raised to 1.
    Edited(u64),
    /// First reviewed in the last `d` days; 0 is raised to 1.
    Introduced(u64),
    /// Answered in the last `days` days (0 raised to 1), any button or one.
    Answered { days: u64, button: Option<u8> },
    /// Rescheduled in the last `d` days; 0 allowed.
    Rescheduled(u64),
    /// Due on day `i` (`prop:due=i`).
    DueOn(i64),
    /// Due within `i` days (`prop:due<=i`).
    DueWithin(i64),
    /// Note, card or deck ids, in the given order.
    Ids { kind: IdKind, ids: Vec<i64> },
    /// Notes of note type `notetype` whose first field is exactly `text`.
    Duplicates { notetype: NotetypeId, text: String },
    /// The items `grammar.md` reads from `text`. `field_text` is set when the
    /// text comes from a search field (J2 then applies first).
    Parsed { text: String, field_text: bool },
    /// AND or OR over one or more items.
    Group { joiner: Joiner, items: Vec<BuiltItem> },
    /// The item negated by J5.
    Negated(Box<BuiltItem>),
}

impl Drop for BuiltItem {
    /// Moves the children out first, so a deep item drops without recursion.
    fn drop(&mut self) {
        let mut pending = Vec::new();
        take_children(self, &mut pending);
        while let Some(mut item) = pending.pop() {
            take_children(&mut item, &mut pending);
        }
    }
}

fn take_children(item: &mut BuiltItem, into: &mut Vec<BuiltItem>) {
    match item {
        BuiltItem::Group { items, .. } => into.append(items),
        BuiltItem::Negated(child) => into.push(std::mem::replace(&mut **child, BuiltItem::WholeCollection)),
        _ => {}
    }
}

/// Why J3 refuses an item (`invalid_item{reason}`). J3 names `empty_group`,
/// `reserved_field_name` and `unrepresentable_name`; the other reasons are
/// Meguri's names for J3's other refusals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InvalidItem {
    /// A group with no items.
    EmptyGroup,
    /// J3.3: a field named like a reserved key.
    ReservedFieldName,
    /// J3.5: a deck or note type name that E would change.
    UnrepresentableName,
    /// An empty deck, tag, note type or field name.
    EmptyName,
    /// An empty literal, or an empty "field contains" text.
    EmptyText,
    /// A template ordinal above 65534.
    OrdinalOutOfRange,
    /// A flag outside 1 to 7.
    FlagOutOfRange,
    /// Days above 4294967295.
    DaysOutOfRange,
    /// An answer button outside 1 to 4.
    ButtonOutOfRange,
    /// A due day outside signed 32-bit.
    DayOutOfRange,
    /// An empty id list.
    EmptyIdList,
    /// A negative id in an id list.
    NegativeId,
    /// J3.4: duplicates text holding White_Space other than U+0020, or
    /// U+001C to U+001F.
    UnrepresentableText,
}

impl InvalidItem {
    /// The reason's name in errors and across the FFI.
    pub fn reason(self) -> &'static str {
        match self {
            InvalidItem::EmptyGroup => "empty_group",
            InvalidItem::ReservedFieldName => "reserved_field_name",
            InvalidItem::UnrepresentableName => "unrepresentable_name",
            InvalidItem::EmptyName => "empty_name",
            InvalidItem::EmptyText => "empty_text",
            InvalidItem::OrdinalOutOfRange => "ordinal_out_of_range",
            InvalidItem::FlagOutOfRange => "flag_out_of_range",
            InvalidItem::DaysOutOfRange => "days_out_of_range",
            InvalidItem::ButtonOutOfRange => "button_out_of_range",
            InvalidItem::DayOutOfRange => "day_out_of_range",
            InvalidItem::EmptyIdList => "empty_id_list",
            InvalidItem::NegativeId => "negative_id",
            InvalidItem::UnrepresentableText => "unrepresentable_text",
        }
    }
}

/// Which text a parse error came from (J1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorSource {
    /// The search field's text that `join` and `replace` read.
    Existing,
    /// A parsed-text item, by its path from the top item: in a group, the
    /// item's index; under a negation, 0. The top item's path is empty.
    Item(Vec<usize>),
}

/// J1's errors. The first one met in item order is returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuilderError {
    /// A parse error; its range lies in the text `source` names.
    Parse { source: ErrorSource, error: SearchError },
    /// `invalid_item{reason}`.
    InvalidItem(InvalidItem),
}

impl From<InvalidItem> for BuilderError {
    fn from(reason: InvalidItem) -> BuilderError {
        BuilderError::InvalidItem(reason)
    }
}

/// J2: text from a search field has every character U+001C to U+001F
/// replaced by a space before anything reads it.
pub fn search_field_text(text: &str) -> Cow<'_, str> {
    if text.chars().any(is_separator) {
        Cow::Owned(text.chars().map(|c| if is_separator(c) { ' ' } else { c }).collect())
    } else {
        Cow::Borrowed(text)
    }
}

/// U+001C to U+001F.
fn is_separator(c: char) -> bool {
    ('\u{1c}'..='\u{1f}').contains(&c)
}

/// White_Space other than U+0020, or U+001C to U+001F: what a term cannot
/// hold as written (G1, J2).
fn splits_a_term(c: char) -> bool {
    (is_white_space(c) && c != ' ') || is_separator(c)
}

/// E(s): a `\` before every `\`, `*` and `_`, then every character that
/// would split the term replaced by `_`.
pub fn escape_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        if matches!(c, '\\' | '*' | '_') {
            out.push('\\');
            out.push(c);
        } else if splits_a_term(c) {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    out
}

/// J4: the search text of `item`, written as a whole search.
pub fn build(item: &BuiltItem) -> Result<String, BuilderError> {
    Ok(build_query(item)?.canonical_text())
}

/// The query J3 and J5 make of `item`. Its canonical text is [`build`]'s.
pub fn build_query(item: &BuiltItem) -> Result<Query, BuilderError> {
    let mut writer = Writer::default();
    let root = writer.item(item)?;
    Ok(writer.builder.finish(root, None))
}

/// One item being visited: its children's nodes as they are made.
struct Frame<'a> {
    item: &'a BuiltItem,
    visited: usize,
    children: Vec<NodeId>,
}

impl Frame<'_> {
    fn new(item: &BuiltItem) -> Frame<'_> {
        Frame {
            item,
            visited: 0,
            children: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Writer {
    builder: QueryBuilder,
}

impl Writer {
    /// The node of `top`, children before parents, in item order (J1).
    fn item(&mut self, top: &BuiltItem) -> Result<NodeId, BuilderError> {
        let mut stack = vec![Frame::new(top)];
        let mut path: Vec<usize> = Vec::new();
        loop {
            let Some(frame) = stack.last_mut() else {
                unreachable!("the item stack holds the top item until it is made")
            };
            let next = match frame.item {
                BuiltItem::Group { items, .. } => items.get(frame.visited),
                BuiltItem::Negated(child) => (frame.visited == 0).then_some(&**child),
                _ => None,
            };
            if let Some(child) = next {
                path.push(frame.visited);
                frame.visited += 1;
                stack.push(Frame::new(child));
                continue;
            }
            let Some(frame) = stack.pop() else {
                unreachable!("the frame was just read")
            };
            let node = self.made(frame, &path)?;
            match stack.last_mut() {
                Some(parent) => {
                    parent.children.push(node);
                    path.pop();
                }
                None => return Ok(node),
            }
        }
    }

    fn push(&mut self, kind: NodeKind) -> NodeId {
        self.builder.push(kind, 0..0)
    }

    fn term(&mut self, term: Term) -> NodeId {
        self.push(NodeKind::Term(term))
    }

    /// J5: a negation gives the item it negates; anything else is negated.
    fn negated(&mut self, item: NodeId) -> NodeId {
        match self.builder.kind(item) {
            NodeKind::Not(inner) => *inner,
            _ => self.push(NodeKind::Not(item)),
        }
    }

    /// An item whose children (if any) are made.
    fn made(&mut self, frame: Frame<'_>, path: &[usize]) -> Result<NodeId, BuilderError> {
        let Frame { item, children, .. } = frame;
        Ok(match item {
            BuiltItem::Group { joiner, .. } => match children.len() {
                0 => return Err(InvalidItem::EmptyGroup.into()),
                1 => children[0],
                _ => {
                    let joined = self.push(match joiner {
                        Joiner::And => NodeKind::And(children),
                        Joiner::Or => NodeKind::Or(children),
                    });
                    self.push(NodeKind::Group(joined))
                }
            },
            BuiltItem::Negated(_) => self.negated(children[0]),
            BuiltItem::Parsed { text, field_text } => self.parsed(text, *field_text, path)?,
            BuiltItem::Untagged => {
                let any = self.term(any_tag());
                self.push(NodeKind::Not(any))
            }
            BuiltItem::AnyFlag => {
                let none = self.term(Term::Flag(0));
                self.push(NodeKind::Not(none))
            }
            leaf => {
                let term = leaf_term(leaf)?;
                self.term(term)
            }
        })
    }

    /// A parsed-text item: its only item, or a group of all of them. The
    /// parsed nodes are copied in arena order, children before parents.
    fn parsed(&mut self, text: &str, field_text: bool, path: &[usize]) -> Result<NodeId, BuilderError> {
        let text = if field_text {
            search_field_text(text)
        } else {
            Cow::Borrowed(text)
        };
        let query = parse(&text).map_err(|error| BuilderError::Parse {
            source: ErrorSource::Item(path.to_vec()),
            error,
        })?;
        let base = self.builder.len();
        let moved = |id: &NodeId| NodeId::at(base + id.index());
        for node in query.nodes() {
            let kind = match &node.kind {
                NodeKind::And(items) => NodeKind::And(items.iter().map(moved).collect()),
                NodeKind::Or(runs) => NodeKind::Or(runs.iter().map(moved).collect()),
                NodeKind::Not(item) => NodeKind::Not(moved(item)),
                NodeKind::Group(inside) => NodeKind::Group(moved(inside)),
                NodeKind::Term(term) => NodeKind::Term(term.clone()),
                NodeKind::NegatedOperator(operator) => NodeKind::NegatedOperator(*operator),
            };
            self.push(kind);
        }
        let root = moved(&query.root());
        Ok(match self.builder.kind(root) {
            NodeKind::And(_) | NodeKind::Or(_) => self.push(NodeKind::Group(root)),
            _ => root,
        })
    }
}

const ANY: &str = "_*";

fn any_tag() -> Term {
    Term::Tag {
        mode: TextMode::Normal,
        text: ANY.to_owned(),
    }
}

/// The term of an item that makes one term (J3's table).
fn leaf_term(item: &BuiltItem) -> Result<Term, InvalidItem> {
    Ok(match item {
        BuiltItem::WholeCollection => Term::WholeCollection,
        BuiltItem::Deck(name) => Term::Deck(deck_text(name)?),
        BuiltItem::CurrentDeck => Term::Deck("current".to_owned()),
        BuiltItem::AnyDeck => Term::Deck(ANY.to_owned()),
        BuiltItem::Tag(name) => tag_term(name)?,
        BuiltItem::AnyTag => any_tag(),
        BuiltItem::Notetype(name) => Term::Notetype(notetype_text(name)?),
        BuiltItem::AnyNotetype => Term::Notetype(ANY.to_owned()),
        BuiltItem::NotetypeId(id) => Term::NotetypeId(*id),
        BuiltItem::Template(ordinal) => {
            let ordinal = u16::try_from(*ordinal)
                .ok()
                .filter(|k| *k <= 65534)
                .ok_or(InvalidItem::OrdinalOutOfRange)?;
            Term::Template(TemplateRef::Ordinal(ordinal))
        }
        BuiltItem::FieldPresent(field) => field_term(field, ANY.to_owned())?,
        BuiltItem::FieldText { field, text } => field_term(field, escape_name(text))?,
        BuiltItem::FieldContains { field, text } => {
            let name = field_name(field)?;
            if text.is_empty() {
                return Err(InvalidItem::EmptyText);
            }
            normal_field(name, format!("*{}*", escape_name(text)))
        }
        BuiltItem::FieldEmpty(field) => field_term(field, String::new())?,
        BuiltItem::Literal(text) => {
            if text.is_empty() {
                return Err(InvalidItem::EmptyText);
            }
            Term::Text(escape_name(text))
        }
        BuiltItem::Flag(flag) => {
            if !(1..=7).contains(flag) {
                return Err(InvalidItem::FlagOutOfRange);
            }
            Term::Flag(*flag)
        }
        BuiltItem::NoFlag => Term::Flag(0),
        BuiltItem::State(state) => Term::State(*state),
        BuiltItem::Added(d) => Term::Added(days(*d)?.max(1)),
        BuiltItem::Edited(d) => Term::Edited(days(*d)?.max(1)),
        BuiltItem::Introduced(d) => Term::Introduced(days(*d)?.max(1)),
        BuiltItem::Answered { days: d, button } => {
            let days = days(*d)?.max(1);
            if button.is_some_and(|b| !(1..=4).contains(&b)) {
                return Err(InvalidItem::ButtonOutOfRange);
            }
            Term::Rated { days, button: *button }
        }
        BuiltItem::Rescheduled(d) => Term::Rescheduled(days(*d)?),
        BuiltItem::DueOn(day) => due(*day, Comparison::Equal)?,
        BuiltItem::DueWithin(day) => due(*day, Comparison::LessOrEqual)?,
        BuiltItem::Ids { kind, ids } => {
            if ids.is_empty() {
                return Err(InvalidItem::EmptyIdList);
            }
            if ids.iter().any(|id| *id < 0) {
                return Err(InvalidItem::NegativeId);
            }
            let list: Vec<String> = ids.iter().map(i64::to_string).collect();
            Term::Ids {
                kind: *kind,
                list: list.join(","),
            }
        }
        BuiltItem::Duplicates { notetype, text } => {
            if text.chars().any(splits_a_term) {
                return Err(InvalidItem::UnrepresentableText);
            }
            Term::Duplicates {
                notetype: *notetype,
                text: text.clone(),
            }
        }
        BuiltItem::Parsed { .. }
        | BuiltItem::Group { .. }
        | BuiltItem::Negated(_)
        | BuiltItem::Untagged
        | BuiltItem::AnyFlag => unreachable!("Writer::made handles the items that are not one term"),
    })
}

fn days(d: u64) -> Result<u32, InvalidItem> {
    u32::try_from(d).map_err(|_| InvalidItem::DaysOutOfRange)
}

fn due(day: i64, comparison: Comparison) -> Result<Term, InvalidItem> {
    let day = i32::try_from(day).map_err(|_| InvalidItem::DayOutOfRange)?;
    Ok(Term::Property {
        property: Property::Due(day),
        comparison,
    })
}

/// J3.1: a value that is exactly a special lower-case word is written with
/// its first letter in upper case, which reads as the ordinary name.
fn ordinary(escaped: String, specials: &[&str]) -> String {
    if !specials.contains(&escaped.as_str()) {
        return escaped;
    }
    let mut chars = escaped.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => escaped,
    }
}

fn is_nfc(text: &str) -> bool {
    matches!(meguri_text::normalize::nfc(text), Cow::Borrowed(_))
}

/// J3.5 for a deck: every level, split at `::`, is non-empty, in NFC, holds
/// no White_Space other than U+0020 and no ASCII control character, and
/// neither starts nor ends with White_Space or `:`.
fn deck_name_is_exact(name: &str) -> bool {
    let edge = |c: char| is_white_space(c) || c == ':';
    name.split("::").all(|level| {
        !level.is_empty()
            && !level
                .chars()
                .any(|c| (is_white_space(c) && c != ' ') || c.is_ascii_control())
            && !level.starts_with(edge)
            && !level.ends_with(edge)
            && is_nfc(level)
    })
}

fn deck_text(name: &str) -> Result<String, InvalidItem> {
    if name.is_empty() {
        return Err(InvalidItem::EmptyName);
    }
    if !deck_name_is_exact(name) {
        return Err(InvalidItem::UnrepresentableName);
    }
    Ok(ordinary(escape_name(name), &["current", "filtered"]))
}

/// J3.5 for a note type: no character E would replace, and NFC.
fn notetype_text(name: &str) -> Result<String, InvalidItem> {
    if name.is_empty() {
        return Err(InvalidItem::EmptyName);
    }
    if name.chars().any(splits_a_term) || !is_nfc(name) {
        return Err(InvalidItem::UnrepresentableName);
    }
    Ok(escape_name(name))
}

fn tag_term(name: &str) -> Result<Term, InvalidItem> {
    if name.is_empty() {
        return Err(InvalidItem::EmptyName);
    }
    if name.chars().any(splits_a_term) {
        return Ok(Term::Tag {
            mode: TextMode::Regex,
            text: tag_expression(name),
        });
    }
    Ok(Term::Tag {
        mode: TextMode::Normal,
        text: ordinary(escape_name(name), &["none"]),
    })
}

/// J3.2: `^`, the tag with its regex metacharacters escaped and every
/// White_Space character and U+001C to U+001F written as `\x{hex}`, then
/// `(::|$)`, so it matches the tag and its descendants.
fn tag_expression(name: &str) -> String {
    let mut out = String::from("^");
    for c in name.chars() {
        if is_white_space(c) || is_separator(c) {
            out.push_str(&format!("\\x{{{:x}}}", u32::from(c)));
        } else {
            if matches!(
                c,
                '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '#' | '&' | '-' | '~'
            ) {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out.push_str("(::|$)");
    out
}

/// A field item's name: refused when empty or reserved (J3.3), else E(f).
fn field_name(field: &str) -> Result<String, InvalidItem> {
    if field.is_empty() {
        return Err(InvalidItem::EmptyName);
    }
    if RESERVED_KEYS.contains(&field.to_ascii_lowercase().as_str()) {
        return Err(InvalidItem::ReservedFieldName);
    }
    Ok(escape_name(field))
}

fn field_term(field: &str, text: String) -> Result<Term, InvalidItem> {
    Ok(normal_field(field_name(field)?, text))
}

fn normal_field(name: String, text: String) -> Term {
    Term::Field {
        name,
        mode: TextMode::Normal,
        text,
    }
}
