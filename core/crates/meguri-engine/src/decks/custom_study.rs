//! Deck-scoped Custom Study (filtered-study F5). Every effect, including
//! remembered choices and selection, belongs to the same undo journal.
use super::{filtered::{self, Definition, Term, invalid}, names, require, save_in, select_in};
use crate::{Collection, EngineError, OpResult};
use meguri_model::{ids::DeckId, proto::deck::DeckKind};
use meguri_search::builder::{BuiltItem, build};
use meguri_storage::{ReadRows, WriteScope, ConfigValue};
use meguri_sched::model::sched_config::SchedConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { IncreaseNew, IncreaseReview, Forgotten, Ahead, PreviewNew, ByState }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State { New, Due, Review, All }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub mode: Mode,
    pub amount: i32,
    pub state: State,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Defaults {
    pub available_new: u32,
    pub available_review: u32,
    pub extend_new: i32,
    pub extend_review: i32,
    pub include_tags: Vec<String>,
    pub exclude_tags: Vec<String>,
}
fn normal(rows: &impl ReadRows, id: DeckId) -> Result<meguri_model::deck::Deck, EngineError> {
    let deck = require(rows, id)?;
    if !matches!(deck.kind.kind, DeckKind::Normal(_)) { return Err(invalid("Custom Study requires a normal deck")); }
    Ok(deck)
}
fn literal(item: BuiltItem) -> Result<String, EngineError> {
    build(&item).map_err(|_| invalid("This name cannot be represented by a literal search"))
}
fn tags(rows: &impl ReadRows, id: DeckId, key: &str) -> Result<Vec<String>, EngineError> {
    Ok(match rows.config_value(&format!("_deck_{}_{key}", id.0))? {
        Some(ConfigValue::Array(values)) => values.into_iter().filter_map(|v| v.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    })
}
pub fn defaults(collection: &Collection, id: DeckId) -> Result<Defaults, EngineError> {
    let reader = collection.reader(meguri_storage::ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let rows = &*snapshot;
    let context = crate::browse::search::search_context(collection, rows)?;
    let deck = normal(rows, id)?;
    let DeckKind::Normal(ref n) = deck.kind.kind else { unreachable!() };
    let scope = rows.deck_rows()?.iter().filter(|d| !super::is_filtered(d) &&
        (d.id == id || names::descendant(&d.name, &deck.name))).map(|d| d.id).collect::<Vec<_>>();
    let (available_new, available_review) = rows.study_availability(&scope, context.today)?;
    Ok(Defaults { available_new, available_review,
        extend_new: i32::try_from(n.extend_new).unwrap_or(i32::MAX),
        extend_review: i32::try_from(n.extend_review).unwrap_or(i32::MAX),
        include_tags: tags(rows, id, "customStudyIncludeTags")?,
        exclude_tags: tags(rows, id, "customStudyExcludeTags")? })
}
fn extend_in(scope: &mut WriteScope<'_>, id: DeckId, request: &Request, today: u32) -> Result<(), EngineError> {
    if !(-99_999..=99_999).contains(&request.amount) {
        return Err(invalid("Daily limit adjustments must be between -99999 and 99999"));
    }
    let source = normal(scope, id)?;
    let parents = SchedConfig::new(scope).apply_all_parent_limits()?;
    for mut deck in scope.deck_rows()?.into_iter().filter(|d| d.id == id ||
        (parents && !super::is_filtered(d) && names::descendant(&source.name, &d.name))) {
        if deck.common.last_day_studied != today {
            deck.common.last_day_studied = today;
            deck.common.new_studied = 0; deck.common.review_studied = 0;
            deck.common.learning_studied = 0; deck.common.milliseconds_studied = 0;
        }
        let counter = if request.mode == Mode::IncreaseNew { &mut deck.common.new_studied } else { &mut deck.common.review_studied };
        *counter = counter.checked_sub(request.amount).ok_or_else(|| invalid("The daily limit adjustment is too large"))?;
        if deck.id == id && request.amount > 0 {
            let DeckKind::Normal(ref mut n) = deck.kind.kind else { unreachable!() };
            if request.mode == Mode::IncreaseNew { n.extend_new = request.amount as u32; }
            else { n.extend_review = request.amount as u32; }
        }
        save_in(scope, deck)?;
    }
    Ok(())
}
fn definition_for(deck_scope: String, request: &Request) -> Result<Definition, EngineError> {
    if request.amount < 1 { return Err(invalid("Provide a positive day count or card limit")); }
    let (search, limit, order, reschedule) = match request.mode {
        Mode::Forgotten => {
            if request.amount > 30 { return Err(invalid("Forgotten card days must be between 1 and 30")); }
            (format!("rated:{}:1", request.amount), 99_999, 1, false)
        },
        Mode::Ahead => (format!("prop:due<={}", request.amount), 99_999, 6, true),
        Mode::PreviewNew => (format!("is:new added:{}", request.amount), 99_999, 5, false),
        Mode::ByState => {
            let (s, order, reschedule) = match request.state {
                State::New => ("is:new", 5, true), State::Due => ("is:due", 6, true),
                State::Review => ("-is:new", 1, true), State::All => ("", 1, false),
            };
            (s.to_owned(), request.amount as u32, order, reschedule)
        },
        _ => return Err(invalid("A search mode is required")),
    };
    let mut operands = vec![deck_scope];
    if !search.is_empty() { operands.push(format!("({search})")); }
    if request.mode == Mode::ByState {
        if !request.include_tags.is_empty() {
            let included = request.include_tags.iter().map(|t| literal(BuiltItem::Tag(t.clone()))).collect::<Result<Vec<_>, _>>()?;
            operands.push(format!("({})", included.join(" OR ")));
        }
        for t in &request.exclude_tags { operands.push(format!("-({})", literal(BuiltItem::Tag(t.clone()))?)); }
    }
    Ok(Definition { name: "Custom Study Session".into(),
        terms: vec![Term { search: operands.join(" "), limit, order }], reschedule, ..Definition::default() })
}
pub fn custom(collection: &Collection, id: DeckId, request: Request) -> Result<OpResult<DeckId>, EngineError> {
    custom_controlled(collection, id, request, crate::CancelToken::new())
}
pub fn custom_controlled(collection: &Collection, id: DeckId, request: Request, cancel: std::sync::Arc<crate::CancelToken>) -> Result<OpResult<DeckId>, EngineError> {
    collection.check_rollover()?;
    let c = collection.clone();
    collection.run(filtered::study_spec(crate::undo::names::CUSTOM_STUDY).cancel(cancel), move |ctx| {
        let salt = ctx.random().next_u64() as i64;
        let scope = ctx.scope();
        normal(scope, id)?;
        let context = crate::browse::search::search_context(&c, scope)?;
        if matches!(request.mode, Mode::IncreaseNew | Mode::IncreaseReview) {
            extend_in(scope, id, &request, context.today)?;
            return Ok(id);
        }
        let def = definition_for(super::search_scope::deck(scope, id)?, &request)?;
        let rows = scope.deck_rows()?;
        let previous = names::find(&rows, &names::native_name(&def.name));
        if previous.is_some_and(|d| !super::is_filtered(d)) {
            return Err(invalid("A normal deck named Custom Study Session already exists"));
        }
        if let Some(deck) = previous { filtered::return_home_in(scope, deck.id)?; }
        let selected = filtered::save_definition_in(scope, previous.map(|d| d.id), &def)?;
        if filtered::gather_in(scope, selected, &context, salt)? == 0 { return Err(super::refused(super::Refusal::NoMatches)); }
        if request.mode == Mode::ByState {
            for (key, values) in [("customStudyIncludeTags", &request.include_tags), ("customStudyExcludeTags", &request.exclude_tags)] {
                scope.set_config(&format!("_deck_{}_{key}", id.0),
                    &ConfigValue::Array(values.iter().cloned().map(ConfigValue::String).collect()))?;
            }
        }
        select_in(scope, selected)?;
        Ok(selected)
    })
}
