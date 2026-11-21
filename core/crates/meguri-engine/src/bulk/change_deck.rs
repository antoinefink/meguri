//! Change Deck (`specs/browser/bulk-actions.md` B7.2) as a bulk action
//! (`specs/browser/bulk-framework.md` K7), the one operation that moves
//! cards between decks.
//!
//! The compute phase, on a maintenance snapshot, resolves the target (an
//! existing deck, or a typed name: an existing deck of that name, else the
//! deck and its missing ancestors to create, step 0) with step 1's refusals
//! in order, lists the cards that move (step 2: not those already in the
//! target, nor ids that name no card), and plans step 4's memory states:
//! the set (the moved cards and the target deck's cards whose stability
//! reads as NULL, `search/card-extractors` X4) and, for every processed
//! card of it, the state the target preset's model gives
//! (`sched/memory-state` M4.1, M7.2).
//!
//! The commit takes no plan on trust that answering can change (K3): every
//! listed card and every member of the set carries an `Untouched`
//! precondition, presets a `TableUntouched` (answering never writes
//! them), and the target's own facts (its preset choice and desired
//! retention, which live in the deck row an answer's studied-today counters
//! rewrite; the `fsrs` key) and the set itself are read again from the live
//! rows before the first write: any difference is `Stale` and the action
//! plans again. Step 3 then rewrites each moved card once, from its live row
//! (its steps translated for the target preset, `sched/step-remap` R4, out
//! of its filtered deck, its memory keys replaced by what step 4 planned);
//! step 4 writes the set's other cards whose values change (M4.2).

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use meguri_model::card::CardType;
use meguri_model::card_data::{CardData, MemoryState};
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{CardId, DeckId};
use meguri_model::proto::deck::DeckKind;
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_sched::memory::items::{History, HistoryForm};
use meguri_sched::memory::resolve::{MemoryError, preset_cutoff};
use meguri_sched::model::card_sched::{CardSched, return_from_filtered};
use meguri_sched::model::deck_sched::DeckSched;
use meguri_sched::model::preset::PresetView;
use meguri_sched::model::sched_config::SchedConfig;
use meguri_sched::options::remap;
use meguri_sched::states::fsrs_glue::FsrsCache;
use meguri_storage::queries::revlog_fsrs::HistorySelection;
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

use super::{BulkCounts, BulkJob, BulkOutcome, CardTarget, check_ids, op_spec};
use crate::cancel::CancelToken;
use crate::collection::Collection;
use crate::decks::{self, Refusal as DeckRefusal, names};
use crate::error::EngineError;
use crate::executor::op::{OpContext, OpResult};
use crate::executor::plan_commit::{Plan, PlanContext, plan_and_commit_with};
use crate::executor::preconditions::Precondition;
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;
use crate::undo::names::CHANGE_DECK;

/// The engine op's name (`engine/op-model` O1, B7.2 step 7).
pub const OP_NAME: &str = "change_deck";

/// Cards read between two cancel checks in the compute phase.
const PAGE: usize = 1_000;

/// B7.2's target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckTarget {
    /// A deck by id.
    Deck(DeckId),
    /// A name typed in the chooser (human form, B7.1): the deck of that
    /// name, or a new one made in this operation (step 0).
    NewDeck(String),
}

/// The target as the compute phase resolved it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Resolved {
    /// A deck by id, or the deck a typed name names (its stored name, to
    /// find it again by that name in the commit).
    Existing { id: DeckId, name: Option<String> },
    /// Step 0's deck to create: the typed text and the stored name it gives
    /// (D1, D3's case adoption).
    Create { typed: String, name: String },
}

/// What step 4 computes with, read from the target deck and the config:
/// the commit compares them with the live rows.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TargetFacts {
    /// The target's `config_id` (1 for a deck step 0 creates).
    config_id: i64,
    /// Its desired retention in force (`sched/presets` V7).
    retention: f32,
    /// The collection's `fsrs` key.
    fsrs: bool,
}

/// The memory values a processed card gets (M4.1).
#[derive(Clone, Copy, Debug)]
struct MemoryValues {
    memory: Option<MemoryState>,
    retention: f32,
    decay: f32,
}

/// What the compute phase hands the commit: ids and four memory values per
/// processed card, never row images (K7).
#[derive(Debug)]
struct Planned {
    target: Resolved,
    facts: TargetFacts,
    /// Step 3's cards, in list order.
    moved: Vec<CardId>,
    /// Step 4's set, ascending, when step 4 runs (FSRS on, a card moved).
    set: Option<Vec<CardId>>,
    /// The processed cards' values: every processed moved card, and the
    /// set's other cards whose values differ from the stored ones (M4.2).
    memory: HashMap<CardId, MemoryValues>,
    /// For each card of the set, how many review-log rows the plan read and
    /// the highest id among them (0 for none): a row added or removed since,
    /// which need not touch the card row, makes the commit stale.
    histories: Vec<(CardId, (u64, i64))>,
    considered: u64,
}

fn stale(table: Table, key: impl Into<String>) -> EngineError {
    EngineError::Stale {
        table,
        key: key.into(),
    }
}

/// The deck's preset as `decks/deck-presets` P3 resolves it: its
/// `config_id`, else the default preset, else the built-in defaults; a deck
/// that does not exist or is filtered gives the built-in defaults
/// (`sched/step-remap` R4 step 1).
fn preset_of(rows: &impl ReadRows, deck: Option<&Deck>) -> Result<DeckConfig, EngineError> {
    let Some(DeckKind::Normal(normal)) = deck.map(|deck| &deck.kind.kind) else {
        return Ok(crate::deck_config::defaults());
    };
    Ok(match rows.deck_config(meguri_model::ids::DeckConfigId(normal.config_id))? {
        Some(config) => config,
        None => rows
            .deck_config(meguri_model::ids::DeckConfigId(1))?
            .unwrap_or_else(crate::deck_config::defaults),
    })
}

/// The target's facts: `deck` is the target deck's row (for a deck step 0
/// creates, D4's new deck).
fn facts_of(rows: &impl ReadRows, deck: &Deck) -> Result<TargetFacts, EngineError> {
    let preset = PresetView::new(&preset_of(rows, Some(deck))?);
    let (config_id, normal) = match &deck.kind.kind {
        DeckKind::Normal(normal) => (normal.config_id, Some(normal)),
        DeckKind::Filtered(_) => (0, None),
    };
    Ok(TargetFacts {
        config_id,
        retention: DeckSched::new(normal, &deck.common).retention(preset.fields().desired_retention),
        fsrs: SchedConfig::new(rows).fsrs()?,
    })
}

/// Step 0 and step 1's deck refusals, in order, on `decks`.
fn resolve_target(decks: &[Deck], target: &DeckTarget) -> Result<Resolved, EngineError> {
    let (existing, typed_name) = match target {
        DeckTarget::Deck(id) => (
            decks.iter().find(|deck| deck.id == *id).ok_or_else(|| EngineError::NotFound {
                table: Table::Decks,
                key: id.0.to_string(),
            })?,
            None,
        ),
        DeckTarget::NewDeck(typed) => {
            let name = names::native_name(typed);
            match names::find(decks, &name) {
                Some(deck) => (deck, Some(name)),
                // D3's refusals (`filtered_parent`, `too_deep`), then case
                // adoption.
                None => {
                    return Ok(Resolved::Create {
                        typed: typed.clone(),
                        name: names::prepare(decks, name)?,
                    });
                }
            }
        }
    };
    if decks::is_filtered(existing) {
        return Err(EngineError::DeckRefused(DeckRefusal::FilteredDeck));
    }
    Ok(Resolved::Existing {
        id: existing.id,
        name: typed_name,
    })
}

/// The compute phase (steps 0 to 4 on the snapshot).
fn plan(
    context: &PlanContext<'_>,
    cards: &[CardId],
    target: &DeckTarget,
    next_day_at: i64,
) -> Result<Plan<Planned>, EngineError> {
    let reader = &**context.snapshot();
    let decks = reader.deck_rows()?;
    let resolved = resolve_target(&decks, target)?;
    let (target_id, target_deck) = match &resolved {
        Resolved::Existing { id, .. } => (
            Some(*id),
            decks.iter().find(|deck| deck.id == *id).cloned().ok_or_else(|| stale(Table::Decks, id.0.to_string()))?,
        ),
        Resolved::Create { name, .. } => (None, decks::new_deck(name.clone())),
    };
    let facts = facts_of(reader, &target_deck)?;
    // Step 2: listed cards that exist and are not in the target already.
    let progress = context.progress();
    let total = cards.len() as u64;
    let mut moved = Vec::new();
    for (page, chunk) in cards.chunks(PAGE).enumerate() {
        context.check_cancel()?;
        if let Some(progress) = progress {
            progress.report((page * PAGE) as u64, Some(total));
        }
        for id in chunk {
            if let Some(card) = reader.card(*id)?
                && Some(card.deck_id) != target_id
            {
                moved.push(card.id);
            }
        }
    }
    if let Some(progress) = progress {
        progress.report(total, Some(total));
    }
    // Step 4.
    let mut set = None;
    let mut memory = HashMap::new();
    let mut histories = Vec::new();
    if facts.fsrs && !moved.is_empty() {
        let moving: HashSet<CardId> = moved.iter().copied().collect();
        let mut members: BTreeSet<CardId> = moving.iter().copied().collect();
        if let Some(id) = target_id {
            members.extend(reader.deck_cards_without_stability(id)?);
        }
        let members: Vec<CardId> = members.into_iter().collect();
        let preset = PresetView::new(&preset_of(reader, Some(&target_deck))?);
        let mut cache = FsrsCache::default();
        // R704: with parameters the model cannot take, the set's cards keep
        // their rows (the moved ones still lose their keys in step 3).
        if let Ok(model) = cache.model(&preset) {
            // M4.3: the cutoff text is read before any card, and a refused
            // one fails the whole op.
            let cutoff = preset_cutoff(&preset).map_err(crate::sched::memory::error)?;
            let decay = preset.decay();
            let historical = preset.fields().historical_retention;
            let mut visited = 0usize;
            reader.visit_fsrs_histories(&HistorySelection::Cards(members.clone()), |card, history| {
                visited += 1;
                if visited.is_multiple_of(PAGE) {
                    context.check_cancel()?;
                }
                let last = history.iter().map(|row| row.id.0).max().unwrap_or(0);
                histories.push((card.id, (history.len() as u64, last)));
                if card.card_type == CardType::New || history.is_empty() {
                    return Ok::<_, EngineError>(());
                }
                let state = History::select(history, cutoff, HistoryForm::Memory)
                    .map(|history| history.memory(model, next_day_at, historical))
                    .transpose()
                    .map_err(MemoryError::from)
                    .map_err(crate::sched::memory::error)?;
                let values = MemoryValues {
                    memory: state,
                    retention: facts.retention,
                    decay,
                };
                if moving.contains(&card.id) || !card.data.same_memory_values(&values.data()) {
                    memory.insert(card.id, values);
                }
                Ok(())
            })?;
        }
        set = Some(members);
    }
    // K3: answering writes cards and the review log, so each card the plan
    // read is guarded by its own row; presets are never written by it.
    let since = context.seq();
    let guarded: BTreeSet<CardId> = cards.iter().chain(set.iter().flatten()).copied().collect();
    let mut preconditions: Vec<Precondition> = guarded
        .into_iter()
        .map(|id| Precondition::Untouched {
            table: Table::Cards,
            key: RowKey::Id(id.0),
            since,
        })
        .collect();
    preconditions.push(Precondition::TableUntouched {
        table: Table::DeckConfig,
        since,
    });
    Ok(Plan {
        value: Planned {
            target: resolved,
            facts,
            moved,
            set,
            memory,
            histories,
            considered: total,
        },
        preconditions,
    })
}

impl MemoryValues {
    /// The four keys as data, for M4.2's comparison.
    fn data(&self) -> CardData {
        let mut data = CardData::default();
        data.memory_state = self.memory;
        data.desired_retention = Some(self.retention);
        data.decay = Some(self.decay);
        data
    }
}

/// The write phase.
fn commit(context: &mut OpContext<'_, '_>, planned: Planned) -> Result<BulkCounts, EngineError> {
    let scope = context.scope();
    let live = scope.deck_rows()?;
    // Everything the plan decided from rows answering can change, checked
    // before the first write.
    if SchedConfig::new(&*scope).fsrs()? != planned.facts.fsrs {
        return Err(stale(Table::Config, "fsrs"));
    }
    let existing = match &planned.target {
        Resolved::Existing { id, name } => {
            let deck = live
                .iter()
                .find(|deck| deck.id == *id)
                .ok_or_else(|| stale(Table::Decks, id.0.to_string()))?;
            // A typed name still names the deck it named.
            let renamed = name
                .as_ref()
                .is_some_and(|name| names::find(&live, name).map(|found| found.id) != Some(*id));
            if renamed || decks::is_filtered(deck) || facts_of(&*scope, deck)? != planned.facts {
                return Err(stale(Table::Decks, id.0.to_string()));
            }
            Some(*id)
        }
        Resolved::Create { name, .. } => {
            if names::find(&live, name).is_some() {
                return Err(stale(Table::Decks, name.clone()));
            }
            None
        }
    };
    if let (Some(set), Some(id)) = (&planned.set, existing) {
        let mut members: BTreeSet<CardId> = planned.moved.iter().copied().collect();
        members.extend(scope.deck_cards_without_stability(id)?);
        if !members.into_iter().eq(set.iter().copied()) {
            return Err(stale(Table::Cards, format!("memory-state set of deck {}", id.0)));
        }
    }
    // K3: answering writes the review log, so no table precondition covers
    // it; each history step 4 computed from is compared with its live rows.
    for (id, extent) in &planned.histories {
        if scope.revlog_extent(*id)? != *extent {
            return Err(stale(Table::Revlog, format!("card {}", id.0)));
        }
    }
    // Step 0: the deck and its missing ancestors (D5 step 3).
    let target = match (&planned.target, existing) {
        (_, Some(id)) => decks::require(&*scope, id)?,
        (Resolved::Create { typed, name }, None) => {
            let deck = decks::add_in(scope, typed)?;
            if !names::same(&deck.name, name) {
                return Err(stale(Table::Decks, name.clone()));
            }
            deck
        }
        (Resolved::Existing { .. }, None) => {
            return Err(EngineError::internal("an existing target without its id"));
        }
    };
    let target_config = preset_of(&*scope, Some(&target))?.config.for_use();
    let mut home_presets: HashMap<DeckId, DeckConfigMessage> = HashMap::new();
    let mut affected = 0u64;
    // Step 3, one write per card, in list order.
    for id in &planned.moved {
        let drifted = || stale(Table::Cards, id.0.to_string());
        let mut card = scope.card(*id)?.ok_or_else(drifted)?;
        if card.deck_id == target.id {
            return Err(drifted());
        }
        // 3.1: the remaining steps, from the home deck's preset.
        let home = CardSched::new(&card).home_deck();
        let old = match home_presets.entry(home) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let deck = live.iter().find(|deck| deck.id == home);
                entry.insert(preset_of(&*scope, deck)?.config.for_use())
            }
        };
        if let Some(left) = remap::preset_change(card.card_type, card.remaining_steps, old, &target_config) {
            card.remaining_steps = left;
        }
        // 3.2: out of its filtered deck.
        return_from_filtered(&mut card);
        // 3.3, and step 4's values for a processed card.
        card.data.memory_state = None;
        card.data.desired_retention = None;
        card.data.decay = None;
        if let Some(values) = planned.memory.get(id) {
            card.data.memory_state = values.memory;
            card.data.desired_retention = Some(values.retention);
            card.data.decay = Some(values.decay);
        }
        // 3.4, 3.5: the scope stamps `mod` and `usn` and encodes `data` (C4).
        card.deck_id = target.id;
        scope.put(&card)?;
        affected += 1;
    }
    // Step 4: the set's other cards, each written only when it changes.
    let moved: HashSet<CardId> = planned.moved.iter().copied().collect();
    let mut others: Vec<(&CardId, &MemoryValues)> =
        planned.memory.iter().filter(|(id, _)| !moved.contains(id)).collect();
    others.sort_unstable_by_key(|(id, _)| **id);
    for (id, values) in others {
        scope.update_card_memory(*id, values.memory, values.retention, values.decay)?;
    }
    Ok(BulkCounts {
        affected,
        considered: planned.considered,
    })
}

/// B7.2 on `target`, blocking: the compute phase on a maintenance snapshot,
/// then one write transaction and one "Change Deck" step (none when nothing
/// moved and no deck was created).
pub fn run(
    collection: &Collection,
    target: &CardTarget,
    deck: &DeckTarget,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<BulkCounts>, EngineError> {
    cancel.check()?;
    let cards = target.resolve(collection)?;
    // Step 1's first refusal.
    check_ids(&cards, "card")?;
    // The day for step 4's memory states, before the op (`sched/day-clock`
    // D8).
    let next_day_at = collection.next_day_at()?;
    let deck = deck.clone();
    let (cancel_plan, progress_plan) = (Arc::clone(&cancel), progress.clone());
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        op_spec(OP_NAME, CHANGE_DECK, 0, &cancel, &progress),
        move |context| plan(context, &cards, &deck, next_day_at),
        move |_, planned| op_spec(OP_NAME, CHANGE_DECK, planned.moved.len(), &cancel_plan, &progress_plan),
        commit,
    )
}

/// [`run`] as a job (`browser/bulk-framework` K2).
pub fn start(engine: &Engine, collection: Collection, target: CardTarget, deck: DeckTarget) -> Result<BulkJob, EngineError> {
    super::start(engine, move |context| {
        run(
            &collection,
            &target,
            &deck,
            Arc::clone(&context.cancel),
            context.progress.clone(),
        )
        .map(BulkOutcome::of)
    })
}
