//! Memory-only recompute: plan on one snapshot, then stamp compact updates
//! in one cancellable transaction. No scheduling column or revlog is written.

use crate::{
    Collection, EngineError, OpResult, OpSpec,
    executor::{
        catalog,
        op::OpContext,
        plan_commit::{Plan, plan_and_commit_with},
        preconditions::Precondition,
    },
    progress::{ProgressPhase, ProgressSink},
    tags::RunOptions,
    undo::entry::StepKind,
};
use meguri_model::{
    card::CardType,
    card_data::MemoryState,
    changes::OpSource,
    ids::{CardId, DeckConfigId, DeckId},
    proto::deck::DeckKind,
};
use meguri_sched::{
    memory::{
        items::{History, HistoryForm},
        resolve::{MemoryError, preset_cutoff},
    },
    model::{card_sched::CardSched, deck_sched::DeckSched, preset::PresetView},
    states::fsrs_glue::FsrsCache,
};
use meguri_storage::{ReadRows, ReaderRole, Table, Written, queries::revlog_fsrs::HistorySelection};
use std::{collections::BTreeMap, time::Duration};

#[derive(Clone, Debug)]
pub enum Target {
    All,
    Presets(Vec<DeckConfigId>),
    Decks(Vec<DeckId>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecomputeReport {
    pub processed: usize,
    pub changed: usize,
    pub skipped_presets: Vec<DeckConfigId>,
}

#[derive(Debug)]
struct Update {
    id: CardId,
    memory: Option<MemoryState>,
    retention: f32,
    decay: f32,
}

/// This holds four memory fields per changed card, never card or revlog
/// images. A caller combining writes plans inside its own transaction and
/// applies this before committing (U49/U47/U50).
#[derive(Debug)]
pub struct MemoryPlan {
    updates: Vec<Update>,
    report: RecomputeReport,
}

pub(crate) fn error(error: MemoryError) -> EngineError {
    match error {
        MemoryError::InvalidDate { preset } => EngineError::InvalidIgnoreBeforeDate { preset },
        MemoryError::UnsupportedParams => EngineError::Unsupported {
            feature: "FSRS parameters".into(),
        },
        MemoryError::Fsrs(error) => EngineError::InvalidArgument {
            message: error.to_string(),
        },
    }
}

pub fn plan(
    rows: &impl ReadRows,
    target: &Target,
    next_day_at: i64,
    check: &dyn Fn() -> Result<(), EngineError>,
    progress: Option<&ProgressSink>,
) -> Result<MemoryPlan, EngineError> {
    let mut groups: BTreeMap<DeckConfigId, Vec<_>> = BTreeMap::new();
    for deck in rows.deck_rows()? {
        let DeckKind::Normal(normal) = &deck.kind.kind else {
            continue;
        };
        let preset = DeckConfigId(normal.config_id);
        let selected = match target {
            Target::All => true,
            Target::Presets(ids) => ids.contains(&preset),
            Target::Decks(ids) => ids.contains(&deck.id),
        };
        if selected {
            groups.entry(preset).or_default().push(deck);
        }
    }
    let mut cache = FsrsCache::default();
    let mut planned = MemoryPlan {
        updates: Vec::new(),
        report: RecomputeReport::default(),
    };
    for (preset_id, decks) in groups {
        check()?;
        let preset = PresetView::for_config(rows, preset_id)?;
        let Ok(model) = cache.model(&preset) else {
            planned.report.skipped_presets.push(preset_id);
            continue;
        };
        let retentions: BTreeMap<_, _> = decks
            .iter()
            .map(|deck| {
                let normal = match &deck.kind.kind {
                    DeckKind::Normal(normal) => Some(normal),
                    _ => None,
                };
                (
                    deck.id,
                    DeckSched::new(normal, &deck.common).retention(preset.fields().desired_retention),
                )
            })
            .collect();
        let selection = HistorySelection::HomeDecks(decks.iter().map(|deck| deck.id).collect());
        let mut cutoff = None;
        rows.visit_fsrs_histories(&selection, |card, history| -> Result<(), EngineError> {
            check()?;
            if card.card_type == CardType::New || history.is_empty() {
                return Ok(());
            }
            let cutoff = match cutoff {
                Some(cutoff) => cutoff,
                None => {
                    let parsed = preset_cutoff(&preset).map_err(error)?;
                    cutoff = Some(parsed);
                    parsed
                }
            };
            let memory = History::select(history, cutoff, HistoryForm::Memory)
                .map(|history| history.memory(model, next_day_at, preset.fields().historical_retention))
                .transpose()
                .map_err(MemoryError::from)
                .map_err(error)?;
            let retention = retentions[&CardSched::new(&card).home_deck()];
            let mut desired = card.data.clone();
            desired.memory_state = memory;
            desired.desired_retention = Some(retention);
            desired.decay = Some(preset.decay());
            if !card.data.same_memory_values(&desired) {
                planned.updates.push(Update {
                    id: card.id,
                    memory,
                    retention,
                    decay: preset.decay(),
                });
            }
            planned.report.processed += 1;
            if let Some(progress) = progress {
                progress.report(planned.report.processed as u64, None);
            }
            Ok(())
        })?;
    }
    Ok(planned)
}

pub fn apply_in(context: &mut OpContext<'_, '_>, planned: MemoryPlan) -> Result<RecomputeReport, EngineError> {
    let MemoryPlan { updates, mut report } = planned;
    let total = updates.len() as u64;
    if let Some(progress) = context.progress() {
        progress.set_phase(ProgressPhase::Named("Recomputing memory"));
    }
    for (index, update) in updates.into_iter().enumerate() {
        if index % 256 == 0 {
            context.check_cancel()?;
            if let Some(progress) = context.progress() {
                progress.report(index as u64, Some(total));
            }
        }
        let Some(mut card) = context.scope().card(update.id)? else {
            continue;
        };
        card.data.memory_state = update.memory;
        card.data.desired_retention = Some(update.retention);
        card.data.decay = Some(update.decay);
        if context.scope().put_card_data(update.id, &card.data)? == Written::Updated {
            report.changed += 1;
        }
    }
    if let Some(progress) = context.progress() {
        progress.report(total, Some(total));
    }
    context.check_cancel()?;
    Ok(report)
}

/// User calls pass their undo name; optimizer/maintenance retain their source.
pub fn recompute(
    collection: &Collection,
    target: Target,
    source: OpSource,
    undo_name: &'static str,
    options: &RunOptions,
) -> Result<OpResult<RecomputeReport>, EngineError> {
    if !matches!(
        source,
        OpSource::UserUndoable | OpSource::Optimizer | OpSource::Maintenance
    ) {
        return Err(EngineError::InvalidArgument {
            message: "invalid memory recompute source".into(),
        });
    }
    let next_day_at = collection.next_day_at()?;
    let mut spec = OpSpec::new(catalog::MEMORY_RECOMPUTE, source).undo_step(undo_name, StepKind::Other);
    if let Some(cancel) = &options.cancel {
        spec = spec.cancel(cancel.clone());
    }
    if let Some(progress) = &options.progress {
        spec = spec.progress(progress.clone());
    }
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        spec,
        |context| {
            let value = plan(
                &**context.snapshot(),
                &target,
                next_day_at,
                &|| context.check_cancel(),
                context.progress(),
            )?;
            let preconditions = [Table::Cards, Table::Revlog, Table::Decks, Table::DeckConfig]
                .map(|table| Precondition::TableUntouched {
                    table,
                    since: context.seq(),
                })
                .to_vec();
            Ok(Plan { value, preconditions })
        },
        |spec, planned| {
            spec.clone().declare_long_write(
                Duration::from_micros(50).saturating_mul(u32::try_from(planned.updates.len()).unwrap_or(u32::MAX)),
            )
        },
        apply_in,
    )
}
