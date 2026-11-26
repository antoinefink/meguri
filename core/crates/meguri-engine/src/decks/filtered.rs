//! Filtered definitions, atomic gathering and returning members (F1/F2, D9).
use crate::EngineError;
use meguri_model::{
    card::{CardQueue, CardType},
    ids::DeckId,
};
use meguri_storage::{ReadRows, WriteScope};

pub fn return_home_in(scope: &mut WriteScope<'_>, id: DeckId) -> Result<(), EngineError> {
    let preview = matches!(super::require(scope, id)?.kind.kind, meguri_model::proto::deck::DeckKind::Filtered(ref f) if !f.reschedule);
    for mut card in scope.cards_of_deck(id)? {
        let zero_review = preview && card.card_type == CardType::Review && card.queue == CardQueue::Preview
            && card.original_due == 0 && scope.deck(card.original_deck_id)?.is_some_and(|d| !super::is_filtered(&d));
        if card.original_deck_id.0 != 0 {
            card.deck_id = card.original_deck_id;
            card.original_deck_id = DeckId(0);
            if zero_review {
                card.due = 0;
            } else if card.original_due != 0 {
                card.due = card.original_due;
            }
            card.original_due = 0;
            if card.queue.stored() >= 0 {
                card.queue = match card.card_type {
                    CardType::New => CardQueue::New,
                    CardType::Review => CardQueue::Review,
                    CardType::Learning | CardType::Relearning if card.due > 1_000_000_000 => CardQueue::Learning,
                    _ => CardQueue::InterdayLearning,
                };
            }
        }
        scope.put_card_forced(&card)?;
    }
    Ok(())
}

use super::{names, ops, require, save_in, select_in};
use crate::{Collection, OpResult, OpSpec, OpSource, StepKind};
use meguri_model::{deck::Deck, proto::deck::{DeckKind, FilteredDeck, SearchTerm}};
use meguri_search::{Plan, SearchMode, SearchNotetypes, SearchContext};
use meguri_storage::{Cell, paging::FilteredGather};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Term {
    pub search: String,
    pub limit: u32,
    pub order: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    pub name: String,
    pub terms: Vec<Term>,
    pub reschedule: bool,
    pub again_seconds: u32,
    pub hard_seconds: u32,
    pub good_seconds: u32,
}
impl Default for Definition {
    fn default() -> Self {
        Self { name: String::new(), terms: vec![Term { search: String::new(), limit: 100, order: 1 }],
            reschedule: true, again_seconds: 60, hard_seconds: 600, good_seconds: 0 }
    }
}
pub(super) fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument { message: message.into() }
}
fn settings(deck: &Deck) -> Result<&FilteredDeck, EngineError> {
    match &deck.kind.kind {
        DeckKind::Filtered(f) => Ok(f),
        _ => Err(invalid("A filtered deck is required")),
    }
}
pub fn definition(rows: &impl ReadRows, id: DeckId) -> Result<Definition, EngineError> {
    let deck = require(rows, id)?;
    let f = settings(&deck)?;
    Ok(Definition { name: names::human_name(&deck.name),
        terms: f.search_terms.iter().map(|t| Term { search: t.search.clone(), limit: t.limit, order: t.order as u32 }).collect(),
        reschedule: f.reschedule, again_seconds: f.preview_again_secs,
        hard_seconds: f.preview_hard_secs, good_seconds: f.preview_good_secs })
}
fn validate(def: &Definition) -> Result<(), EngineError> {
    if def.name.trim().is_empty() || !(1..=2).contains(&def.terms.len()) {
        return Err(invalid("Provide a name and one or two search terms"));
    }
    for t in &def.terms {
        if t.order > 10 { return Err(invalid("Invalid filtered study limit or order")); }
        meguri_search::parse(&t.search).map_err(|e| invalid(e.to_string()))?;
    }
    Ok(())
}
pub(super) fn plan(rows: &impl ReadRows, text: &str, context: &SearchContext) -> Result<Plan, EngineError> {
    let query = meguri_search::parse(text).map_err(|e| invalid(e.to_string()))?;
    Plan::build(&query, SearchMode::Cards, context, &SearchNotetypes::read(rows)?)
        .map_err(|meguri_search::BuildError::Search(e)| invalid(e.to_string()))
}
fn cells(params: &[meguri_search::Param]) -> Vec<Cell> {
    params.iter().map(|p| match p {
        meguri_search::Param::Integer(i) => Cell::Integer(*i),
        meguri_search::Param::Real(f) => Cell::Real(*f),
        meguri_search::Param::Text(t) => Cell::text(t.clone()),
    }).collect()
}
pub(super) fn gather_in(scope: &mut WriteScope<'_>, id: DeckId, context: &SearchContext, salt: i64) -> Result<usize, EngineError> {
    let deck = require(scope, id)?;
    let f = settings(&deck)?.clone();
    // Validate all terms before releasing any member, including run-time
    // pattern checks in the second term. Failures still roll back the op.
    let plans = f.search_terms.iter().map(|t| {
        if !(0..=10).contains(&t.order) { return Err(invalid("Invalid filtered study limit or order")); }
        plan(scope, &t.search, context)
    }).collect::<Result<Vec<_>, _>>()?;
    return_home_in(scope, id)?;
    let filtered = scope.deck_rows()?.iter().filter(|d| super::is_filtered(d)).map(|d| d.id).collect::<Vec<_>>();
    let mut count = 0usize;
    for (term, plan) in f.search_terms.iter().zip(plans) {
        let ids = scope.filtered_card_ids(FilteredGather {
            search_sql: plan.sql(), search_params: &cells(plan.params()), filtered: &filtered,
            order: term.order as u32, limit: term.limit, today: context.today, now: context.now, salt,
        })?;
        for card_id in ids {
            let mut card = scope.card(card_id)?.ok_or_else(|| invalid("Search returned a missing card"))?;
            card.original_deck_id = card.deck_id;
            card.original_due = card.due;
            card.deck_id = id;
            if card.due > 0 {
                card.due = i32::try_from(count).ok().and_then(|p| (-100_000_i32).checked_add(p))
                    .ok_or_else(|| invalid("Too many filtered study cards"))?;
            }
            if !f.reschedule { card.queue = CardQueue::Review; }
            scope.put_card_forced(&card)?;
            count += 1;
        }
    }
    Ok(count)
}
pub(super) fn save_definition_in(scope: &mut WriteScope<'_>, id: Option<DeckId>, def: &Definition) -> Result<DeckId, EngineError> {
    validate(def)?;
    let rows = scope.deck_rows()?;
    let name = names::native_name(&def.name);
    if rows.iter().any(|d| Some(d.id) != id && names::same(&d.name, &name)) {
        return Err(invalid("A deck already has this name"));
    }
    let mut deck = if let Some(id) = id {
        let deck = require(scope, id)?;
        settings(&deck)?;
        deck
    } else {
        ops::add_in(scope, &def.name)?
    };
    if rows.iter().any(|d| names::descendant(&d.name, &deck.name)) {
        return Err(invalid("A filtered deck cannot have children"));
    }
    // Edit the live messages in place: legacy delays, message unknowns and
    // each retained term's unknown fields survive a public definition edit.
    if id.is_none() { deck.kind.kind = DeckKind::Filtered(FilteredDeck::default()); }
    let DeckKind::Filtered(ref mut f) = deck.kind.kind else { unreachable!() };
    f.reschedule = def.reschedule;
    f.preview_again_secs = def.again_seconds;
    f.preview_hard_secs = def.hard_seconds;
    f.preview_good_secs = def.good_seconds;
    f.search_terms.resize_with(def.terms.len(), SearchTerm::default);
    for (stored, input) in f.search_terms.iter_mut().zip(&def.terms) {
        stored.search = input.search.clone(); stored.limit = input.limit; stored.order = input.order as i32;
    }
    deck.name = name;
    Ok(save_in(scope, deck)?.id)
}
// Full-grammar searches can be expensive; use the existing catalog bulk
// operation with an undo step and a declared writer delay.
pub(super) fn study_spec(name: &'static str) -> OpSpec {
    OpSpec::new(crate::executor::catalog::BULK_OP, OpSource::UserUndoable)
        .undo_step(name, StepKind::Other).declare_long_write(std::time::Duration::from_secs(1))
}
pub fn save(collection: &Collection, id: Option<DeckId>, def: Definition, allow_empty: bool) -> Result<OpResult<DeckId>, EngineError> {
    save_controlled(collection, id, def, allow_empty, crate::CancelToken::new())
}
pub fn save_controlled(collection: &Collection, id: Option<DeckId>, def: Definition, allow_empty: bool, cancel: std::sync::Arc<crate::CancelToken>) -> Result<OpResult<DeckId>, EngineError> {
    collection.check_rollover()?;
    let collection_context = collection.clone();
    collection.run(study_spec(crate::undo::names::BUILD_DECK).cancel(cancel), move |ctx| {
        let salt = ctx.random().next_u64() as i64;
        let scope = ctx.scope();
        let context = crate::browse::search::search_context(&collection_context, scope)?;
        // Release with the OLD preview settings before editing them.
        if let Some(id) = id { settings(&require(scope, id)?)?; return_home_in(scope, id)?; }
        let id = save_definition_in(scope, id, &def)?;
        if gather_in(scope, id, &context, salt)? == 0 && !allow_empty {
            return Err(super::refused(super::Refusal::NoMatches));
        }
        select_in(scope, id)?;
        Ok(id)
    })
}
pub fn rebuild(collection: &Collection, id: DeckId) -> Result<OpResult<()>, EngineError> {
    rebuild_controlled(collection, id, crate::CancelToken::new())
}
pub fn rebuild_controlled(collection: &Collection, id: DeckId, cancel: std::sync::Arc<crate::CancelToken>) -> Result<OpResult<()>, EngineError> {
    collection.check_rollover()?;
    let c = collection.clone();
    collection.run(study_spec(crate::undo::names::BUILD_DECK).cancel(cancel), move |ctx| {
        let salt = ctx.random().next_u64() as i64;
        let scope = ctx.scope();
        let context = crate::browse::search::search_context(&c, scope)?;
        gather_in(scope, id, &context, salt)?;
        Ok(())
    })
}
pub fn empty(collection: &Collection, id: DeckId) -> Result<OpResult<()>, EngineError> {
    empty_controlled(collection, id, crate::CancelToken::new())
}
pub fn empty_controlled(collection: &Collection, id: DeckId, cancel: std::sync::Arc<crate::CancelToken>) -> Result<OpResult<()>, EngineError> {
    collection.run(study_spec(crate::undo::names::EMPTY).cancel(cancel), move |ctx| {
        settings(&require(ctx.scope(), id)?)?;
        return_home_in(ctx.scope(), id)
    })
}
