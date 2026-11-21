//! T5, the text, and T7, reading Anki search text into a query. Items are
//! compared as T7 says: a `deck:`, `tag:` or `note:` term in normal mode
//! with no wildcard by the name it names in the name index, every other
//! item by its canonical text against the items of T3's values.

use std::sync::LazyLock;

use meguri_text::fold::compare_folded;
use meguri_text::normalize::nfc;
use regex::Regex;

use crate::ast::{NodeId, NodeKind, Query, TemplateRef, Term, TextMode};
use crate::builder::{BuiltItem, build_query, search_field_text, splits_a_term, tag_expression};
use crate::lexer::is_white_space;
use crate::name_index::NameIndex;
use crate::parser::parse;
use crate::pattern;
use crate::plan::names::deck_levels;

use super::{DueValue, FieldTest, FilterQuery, Kind, NotetypeValue, Period, StateValue, Token, Value, ValueToken};

/// T5's reading of the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextReading {
    /// The text stays text: these are its items (none for an empty text or
    /// White_Space only; literal words when it does not parse).
    Items(Vec<BuiltItem>),
    /// The text holds Anki syntax: T7 reads it when it is committed.
    Syntax,
}

/// T5: whether the text stays text, and its items.
pub fn classify_text(text: &str, names: &NameIndex) -> TextReading {
    let text = search_field_text(text);
    let query = match parse(&text) {
        Ok(query) => query,
        Err(_) => return TextReading::Items(literal_words(&text)),
    };
    let reader = Reader::new(names);
    let mut items = Vec::new();
    for id in run_items(&query) {
        match reader.text_item(&query, id) {
            Some(TextItem::Plain) => items.push(BuiltItem::Parsed {
                text: query.canonical_text_of(id),
                field_text: false,
            }),
            Some(TextItem::Literal(written)) => items.push(BuiltItem::Literal(written)),
            Some(TextItem::Nothing) => {}
            None => return TextReading::Syntax,
        }
    }
    TextReading::Items(items)
}

/// T5's last rule but one: each run of non-White_Space characters of a text
/// that does not parse is a literal item, in its order.
pub(crate) fn literal_words(text: &str) -> Vec<BuiltItem> {
    search_field_text(text)
        .split(is_white_space)
        .filter(|word| !word.is_empty())
        .map(|word| BuiltItem::Literal(word.to_owned()))
        .collect()
}

/// T7: the query `text` stands for. It never fails.
pub fn read_text(text: &str, names: &NameIndex) -> FilterQuery {
    let read = Reader::new(names).read(text, &[]);
    FilterQuery {
        tokens: read.tokens,
        text: read.text,
    }
}

/// The items of the top-level AND-run; a top-level OR (more than one
/// run) is one item, which T5 never keeps as text.
fn run_items(query: &Query) -> Vec<NodeId> {
    match &query.node(query.root()).kind {
        NodeKind::And(items) => items.clone(),
        _ => vec![query.root()],
    }
}

/// How an item stays text (T5).
enum TextItem {
    /// A plain-text term, or the negation of one, as read.
    Plain,
    /// A field term naming no field: the literal item of its written text.
    Literal(String),
    /// The whole-collection term of an empty text.
    Nothing,
}

/// What one item of the AND-run, or a whole top-level OR, becomes.
enum Class {
    Token(ValueToken),
    Text,
    Advanced,
}

/// One item a value comes from: a value, or `tag:leech`, which is Card
/// State: Leech alone and a tag or a state among the values of a group.
#[derive(Clone)]
enum Single {
    Value(Value),
    /// `tag:leech`; the tag's registered spelling, or the name as written.
    Leech(String),
}

pub(crate) struct Read {
    pub tokens: Vec<Token>,
    pub text: String,
}

/// T3's values that are compared by their item's canonical text.
static FIXED: LazyLock<Vec<(String, Value)>> = LazyLock::new(|| {
    let mut values = vec![
        Value::CurrentDeck,
        Value::Untagged,
        Value::State(StateValue::New),
        Value::State(StateValue::Learning),
        Value::State(StateValue::Review),
        Value::State(StateValue::Suspended),
        Value::State(StateValue::Buried),
        Value::NoFlag,
        Value::AnyFlag,
        Value::Due(DueValue::Today),
        Value::Due(DueValue::Overdue),
        Value::Due(DueValue::Next7Days),
    ];
    values.extend((1..=7).map(Value::Flag));
    for period in [Period::Today, Period::Last7Days, Period::Last30Days] {
        values.extend([Value::Added(period), Value::Edited(period), Value::Studied(period)]);
    }
    values
        .into_iter()
        .filter_map(|value| Some((item_key(&value.item()?)?, value)))
        .collect()
});

/// The canonical text of a built item as it stands inside a longer search
/// (a group keeps its brackets), the form [`Query::canonical_text_of`]
/// gives a parsed item.
fn item_key(item: &BuiltItem) -> Option<String> {
    let query = build_query(item).ok()?;
    Some(query.canonical_text_of(query.root()))
}

/// `(?i)^leech$`: `tag:leech` with letter case folded (`names-and-ids` N2.2).
static LEECH: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)^leech$").expect("a fixed expression"));

pub(crate) struct Reader<'n> {
    names: &'n NameIndex,
}

impl<'n> Reader<'n> {
    pub(crate) fn new(names: &'n NameIndex) -> Reader<'n> {
        Reader { names }
    }

    /// T7 steps 1 to 6, with `existing` the tokens already in the query
    /// (T8: step 4 counts them).
    pub(crate) fn read(&self, text: &str, existing: &[Token]) -> Read {
        let empty = Read {
            tokens: Vec::new(),
            text: String::new(),
        };
        let field_text = search_field_text(text);
        let query = match parse(&field_text) {
            Ok(query) => query,
            // Step 1: the text alone, read as literal words by T5.
            Err(_) => {
                return Read {
                    tokens: Vec::new(),
                    text: text.to_owned(),
                };
            }
        };
        let root = query.root();
        if let NodeKind::Term(term) = &query.node(root).kind {
            if *term == Term::WholeCollection || matches!(term, Term::Deck(value) if nfc(value) == "*") {
                return empty;
            }
        }
        let mut kinds: Vec<Kind> = existing.iter().filter_map(Token::kind).collect();
        let mut out = Read {
            tokens: Vec::new(),
            text: String::new(),
        };
        // Step 2: a top-level OR is one group OR, else one advanced token.
        if let NodeKind::Or(runs) = &query.node(root).kind {
            let token = self
                .group_or(&query, runs)
                .map(|values| ValueToken::new(values, false));
            out.tokens
                .push(self.placed(token, &mut kinds, || advanced_text(&query, &[root], query.canonical_text())));
            return out;
        }
        let mut words = Vec::new();
        let items = run_items(&query);
        let mut k = 0;
        while k < items.len() {
            // Step 3's pairs: Overdue and Next 7 Days at the top level.
            if let Some(due) = items.get(k + 1).and_then(|next| pair(&query, items[k], *next)) {
                let token = ValueToken::new(vec![Value::Due(due)], false);
                let (a, b) = (items[k], items[k + 1]);
                out.tokens.push(self.placed(Some(token), &mut kinds, || {
                    format!("{} {}", query.canonical_text_of(a), query.canonical_text_of(b))
                }));
                k += 2;
                continue;
            }
            let id = items[k];
            match self.classify(&query, id) {
                Class::Token(token) => out
                    .tokens
                    .push(self.placed(Some(token), &mut kinds, || whole_text(&query, id))),
                Class::Text => {
                    if let Some(written) = query.source_text(&query.node(id).span) {
                        words.push(written);
                    }
                }
                Class::Advanced => out.tokens.push(Token::Advanced(whole_text(&query, id))),
            }
            k += 1;
        }
        out.text = words.join(" ");
        out
    }

    /// Step 4: a value token whose kind already has a token is an advanced
    /// token of its item's text instead.
    fn placed(&self, token: Option<ValueToken>, kinds: &mut Vec<Kind>, text: impl FnOnce() -> String) -> Token {
        let Some(token) = token else {
            return Token::Advanced(text());
        };
        match token.kind() {
            Some(kind) if !kinds.iter().any(|seen| seen.same(&kind)) => {
                kinds.push(kind);
                Token::Value(token)
            }
            _ => Token::Advanced(text()),
        }
    }

    /// Step 3's rows, first fit, for one item of the AND-run.
    fn classify(&self, query: &Query, id: NodeId) -> Class {
        if let Some(single) = self.single(query, id) {
            return Class::Token(alone(single));
        }
        match &query.node(id).kind {
            NodeKind::Not(child) => {
                if let Some(single) = self.single(query, *child) {
                    return negated(alone(single));
                }
                if let NodeKind::Group(inside) = query.node(*child).kind {
                    if let NodeKind::Or(runs) = &query.node(inside).kind {
                        return match self.group_or(query, runs) {
                            Some(values) => negated(ValueToken::new(values, false)),
                            None => Class::Advanced,
                        };
                    }
                }
            }
            NodeKind::Group(inside) => {
                if let NodeKind::Or(runs) = &query.node(*inside).kind {
                    if let Some(values) = self.group_or(query, runs) {
                        return Class::Token(ValueToken::new(values, false));
                    }
                }
            }
            _ => {}
        }
        match self.text_item(query, id) {
            Some(TextItem::Plain | TextItem::Literal(_)) => Class::Text,
            _ => Class::Advanced,
        }
    }

    /// Rows 3, 5 and 6: a group OR whose runs are all value items of one
    /// kind, `tag:leech` a tag among tags and a state among states. Each
    /// value once.
    fn group_or(&self, query: &Query, runs: &[NodeId]) -> Option<Vec<Value>> {
        let mut singles = Vec::with_capacity(runs.len());
        for run in runs {
            let single = match &query.node(*run).kind {
                NodeKind::And(items) if items.len() == 2 => Single::Value(Value::Due(pair(query, items[0], items[1])?)),
                _ => self.single(query, *run)?,
            };
            singles.push(single);
        }
        let kind = singles
            .iter()
            .find_map(|single| match single {
                Single::Value(value) => Some(value.kind()),
                Single::Leech(_) => None,
            })
            .unwrap_or(Kind::Tag);
        let mut values: Vec<Value> = Vec::with_capacity(singles.len());
        for single in singles {
            let value = match (single, &kind) {
                (Single::Value(value), _) => value,
                (Single::Leech(name), Kind::Tag) => Value::Tag(name),
                (Single::Leech(_), Kind::CardState) => Value::State(StateValue::Leech),
                (Single::Leech(_), _) => return None,
            };
            if !value.kind().same(&kind) || values.contains(&value) {
                return None;
            }
            values.push(value);
        }
        Some(values)
    }

    /// Row 1: the value whose item this item is, or `tag:leech`.
    fn single(&self, query: &Query, id: NodeId) -> Option<Single> {
        if let NodeKind::Term(term) = &query.node(id).kind {
            match term {
                Term::Deck(value) => {
                    if let Some(deck) = self.deck(value) {
                        return Some(Single::Value(deck));
                    }
                }
                Term::Tag { mode, text } => {
                    if let Some(single) = self.tag(*mode, text) {
                        return Some(single);
                    }
                }
                Term::Notetype(value) => {
                    if let Some(notetype) = self.notetype(value) {
                        return Some(Single::Value(notetype));
                    }
                }
                _ => {}
            }
        }
        let key = query.canonical_text_of(id);
        if let Some((_, value)) = FIXED.iter().find(|(fixed, _)| *fixed == key) {
            return Some(Single::Value(value.clone()));
        }
        match &query.node(id).kind {
            NodeKind::Term(Term::Template(TemplateRef::Name(name))) if !pattern::has_wildcards(name) => {
                let typed = pattern::exact_text(&nfc(name)).into_owned();
                if item_key(&Value::CardType(typed.clone()).item()?)? != key {
                    return None;
                }
                // A card type some note type has, in its stored spelling
                // (N4.2 compares by unicase equality); one no note type has
                // stays advanced (T7.1: renamed or deleted).
                self.template_named(&typed)
                    .map(|stored| Single::Value(Value::CardType(stored)))
            }
            NodeKind::Term(Term::Field {
                name,
                mode: TextMode::Normal,
                text,
            }) => {
                let field = self.field_named(name)?;
                let typed = pattern::exact_text(name).into_owned();
                let test = field_test(text)?;
                // Compared with the item written for the name as typed; the
                // value keeps the stored spelling (F2 step 4 folds case).
                let typed_value = Value::Field {
                    field: typed,
                    test: test.clone(),
                };
                if item_key(&typed_value.item()?)? != key {
                    return None;
                }
                Some(Single::Value(Value::Field { field, test }))
            }
            _ => None,
        }
    }

    /// A deck term naming exactly one deck (N3.3's equality: the value's
    /// levels, letter case folded, against each stored name).
    fn deck(&self, value: &str) -> Option<Value> {
        let value = nfc(value);
        if matches!(value.as_ref(), "*" | "current" | "filtered") || pattern::has_wildcards(&value) {
            return None;
        }
        let exact = Regex::new(&format!("(?i)^{}$", deck_levels(&value))).ok()?;
        let mut found = self.names.decks().iter().filter(|deck| exact.is_match(&deck.native));
        let deck = found.next()?;
        if found.next().is_some() {
            return None;
        }
        Some(Value::Deck(super::deck_value(self.names, deck)?))
    }

    /// A tag term naming exactly one registered tag (N2.2's equality), or
    /// `tag:leech`; in regex mode, the J3.2 form of exactly one tag.
    fn tag(&self, mode: TextMode, text: &str) -> Option<Single> {
        match mode {
            TextMode::Normal => {
                let value = nfc(text);
                if matches!(value.as_ref(), "none" | "*") || value.contains(' ') || pattern::has_wildcards(&value) {
                    return None;
                }
                let exact = pattern::exact_text(&value).into_owned();
                let equal = Regex::new(&format!("(?i)^{}$", regex::escape(&exact))).ok()?;
                let mut found = self.names.tags().iter().filter(|tag| equal.is_match(tag));
                let only = found.next();
                let unique = only.filter(|_| found.next().is_none());
                if LEECH.is_match(&exact) {
                    return Some(Single::Leech(unique.cloned().unwrap_or(exact)));
                }
                unique.map(|tag| Single::Value(Value::Tag(tag.clone())))
            }
            TextMode::Regex => {
                let mut found = self
                    .names
                    .tags()
                    .iter()
                    .filter(|tag| tag.chars().any(splits_a_term) && tag_expression(tag) == text);
                let tag = found.next()?;
                found.next().is_none().then(|| Single::Value(Value::Tag(tag.clone())))
            }
            TextMode::NoCombining => None,
        }
    }

    /// A note type term naming exactly one note type (N4.1: unicase
    /// equality of the exact text).
    fn notetype(&self, value: &str) -> Option<Value> {
        let value = nfc(value);
        if pattern::has_wildcards(&value) {
            return None;
        }
        let exact = pattern::exact_text(&value);
        let mut found = self
            .names
            .notetypes()
            .iter()
            .filter(|notetype| compare_folded(&notetype.name, &exact).is_eq());
        let notetype = found.next()?;
        found
            .next()
            .is_none()
            .then(|| Value::Notetype(NotetypeValue::of(notetype)))
    }

    /// The stored spelling of a template named `name` (unicase equality),
    /// the first in the index's note type order.
    fn template_named(&self, name: &str) -> Option<String> {
        self.names
            .notetypes()
            .iter()
            .flat_map(|notetype| notetype.templates.iter())
            .find(|template| compare_folded(template, name).is_eq())
            .cloned()
    }

    /// The stored spelling of the field a field term's name names, when it
    /// has no wildcard and names a field of some note type (F2 steps 1, 2
    /// and 4).
    fn field_named(&self, name: &str) -> Option<String> {
        let name = nfc(name);
        if matches!(name.as_ref(), "*" | "_*" | "*_") || pattern::has_wildcards(&name) {
            return None;
        }
        let exact = pattern::exact_text(&name);
        self.names
            .notetypes()
            .iter()
            .flat_map(|notetype| notetype.fields.iter())
            .find(|field| compare_folded(field, &exact).is_eq())
            .cloned()
    }

    /// T5 (and step 3's text row): a plain-text term, the negation of one,
    /// or a field term naming no field of any note type.
    fn text_item(&self, query: &Query, id: NodeId) -> Option<TextItem> {
        match &query.node(id).kind {
            NodeKind::Term(Term::WholeCollection) => Some(TextItem::Nothing),
            NodeKind::Term(Term::Text(_)) => Some(TextItem::Plain),
            NodeKind::Not(child) => {
                matches!(query.node(*child).kind, NodeKind::Term(Term::Text(_))).then_some(TextItem::Plain)
            }
            NodeKind::Term(Term::Field { name, mode, text }) => {
                let nfc_name = nfc(name);
                if matches!(nfc_name.as_ref(), "*" | "_*" | "*_") || pattern::has_wildcards(&nfc_name) {
                    return None;
                }
                if self.field_named(name).is_some() {
                    return None;
                }
                let value = match mode {
                    TextMode::Normal => pattern::exact_text(text).into_owned(),
                    TextMode::Regex => format!("re:{text}"),
                    TextMode::NoCombining => format!("nc:{text}"),
                };
                Some(TextItem::Literal(format!("{}:{value}", pattern::exact_text(name))))
            }
            _ => None,
        }
    }
}

/// A token of one value.
fn alone(single: Single) -> ValueToken {
    let value = match single {
        Single::Value(value) => value,
        Single::Leech(_) => Value::State(StateValue::Leech),
    };
    ValueToken::new(vec![value], false)
}

/// Rows 2 and 4, in T10's normal form: "is not" of No Flag is Any Flag and
/// the reverse; Is Not is never set on a token holding No Flag, Any Flag
/// or Untagged otherwise, so such a negation stays advanced.
fn negated(mut token: ValueToken) -> Class {
    match token.values.as_slice() {
        [Value::NoFlag] => Class::Token(ValueToken::new(vec![Value::AnyFlag], false)),
        [Value::AnyFlag] => Class::Token(ValueToken::new(vec![Value::NoFlag], false)),
        values
            if values
                .iter()
                .any(|v| matches!(v, Value::NoFlag | Value::AnyFlag | Value::Untagged)) =>
        {
            Class::Advanced
        }
        _ => {
            token.negated = true;
            Class::Token(token)
        }
    }
}

/// Step 3's adjacent pairs: `is:due` then `-prop:due=0` (Overdue), and
/// `prop:due>=1` then `prop:due<=7` (Next 7 Days).
fn pair(query: &Query, a: NodeId, b: NodeId) -> Option<DueValue> {
    match (query.canonical_text_of(a).as_str(), query.canonical_text_of(b).as_str()) {
        ("is:due", "-prop:due=0") => Some(DueValue::Overdue),
        ("prop:due>=1", "prop:due<=7") => Some(DueValue::Next7Days),
        _ => None,
    }
}

/// An advanced token's text: its item's canonical text as a whole search
/// (F2.2: a group loses its brackets), or the item as written when that
/// canonical text would not read back as the same item (`canonical-form`
/// F7's shapes: a negated operator word, `preset:`, `prop:cds:`).
fn whole_text(query: &Query, id: NodeId) -> String {
    let canonical = match query.node(id).kind {
        NodeKind::Group(inside) => query.canonical_text_of(inside),
        _ => query.canonical_text_of(id),
    };
    advanced_text(query, &[id], canonical)
}

/// `canonical`, the text of the items `ids`, when it reads back as itself
/// and the items hold no negated operator word; else their written text.
fn advanced_text(query: &Query, ids: &[NodeId], canonical: String) -> String {
    let operator = ids.iter().any(|id| {
        subtree(query, *id)
            .into_iter()
            .any(|node| matches!(query.node(node).kind, NodeKind::NegatedOperator(_)))
    });
    let stable = parse(&canonical).is_ok_and(|again| again.canonical_text() == canonical);
    if stable && !operator {
        return canonical;
    }
    let (Some(first), Some(last)) = (ids.first(), ids.last()) else {
        return canonical;
    };
    let span = query.node(*first).span.start..query.node(*last).span.end;
    query.source_text(&span).unwrap_or(canonical)
}

/// The nodes below and at `id`, with an explicit stack (E6).
fn subtree(query: &Query, id: NodeId) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![id];
    while let Some(id) = stack.pop() {
        out.push(id);
        match &query.node(id).kind {
            NodeKind::And(children) | NodeKind::Or(children) => stack.extend(children.iter().copied()),
            NodeKind::Not(child) | NodeKind::Group(child) => stack.push(*child),
            NodeKind::Term(_) | NodeKind::NegatedOperator(_) => {}
        }
    }
    out
}

/// The test a Field token's item has: `f:` empty, `f:_*` content, and
/// `f:*t*` contains t (checked against the item written for t).
fn field_test(text: &str) -> Option<FieldTest> {
    match text {
        "" => Some(FieldTest::Empty),
        "_*" => Some(FieldTest::HasContent),
        _ => {
            let inner = text.strip_prefix('*')?.strip_suffix('*')?;
            let t = pattern::exact_text(inner).into_owned();
            (!t.is_empty()).then_some(FieldTest::Contains(t))
        }
    }
}
