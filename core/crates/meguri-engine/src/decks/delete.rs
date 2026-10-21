//! D8: summary, backup, subtree deletion, and graves in one undo step.
use super::{filtered, is_filtered, names, ops};
use crate::{CancelToken, Collection, EngineError, OpResult, Precondition, ProgressSink};
use meguri_model::{grave::GraveKind, ids::DeckId, usn::Usn};
use meguri_storage::{ReadRows, ReaderRole, Table, WriteScope};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeletionSummary {
    pub decks: usize,
    pub cards: usize,
}

fn selected(rows: &impl ReadRows, ids: &[DeckId]) -> Result<Vec<meguri_model::deck::Deck>, EngineError> {
    let all = rows.deck_rows()?;
    let roots: Vec<_> = all.iter().filter(|d| ids.contains(&d.id)).collect();
    Ok(all
        .iter()
        .filter(|d| {
            roots
                .iter()
                .any(|root| d.id == root.id || names::descendant(&d.name, &root.name))
        })
        .cloned()
        .collect())
}

pub fn deletion_summary(rows: &impl ReadRows, ids: &[DeckId]) -> Result<DeletionSummary, EngineError> {
    let decks = selected(rows, ids)?;
    let mut cards = BTreeSet::new();
    for deck in decks.iter().filter(|d| !is_filtered(d)) {
        cards.extend(rows.cards_of_deck(deck.id)?.into_iter().map(|c| c.id));
    }
    Ok(DeletionSummary {
        decks: decks.len(),
        cards: cards.len(),
    })
}

/// No backup here: the caller owns the pre-op hook (conformance K5).
pub fn delete_in(scope: &mut WriteScope<'_>, ids: &[DeckId]) -> Result<DeletionSummary, EngineError> {
    let mut result = DeletionSummary::default();
    for &id in ids {
        let Some(deck) = scope.deck(id)? else { continue };
        let mut subtree = vec![deck.clone()];
        subtree.extend(
            scope
                .deck_rows()?
                .into_iter()
                .filter(|d| names::descendant(&d.name, &deck.name)),
        );
        for deck in subtree {
            if is_filtered(&deck) {
                filtered::return_home_in(scope, deck.id)?;
            } else {
                let cards = scope.cards_of_deck(deck.id)?.iter().map(|c| c.id).collect::<Vec<_>>();
                result.cards += crate::cards::remove_in(scope, &cards, true)?;
            }
            let prefix = format!("_deck_{}_", deck.id.0);
            for key in scope
                .config_keys()?
                .into_iter()
                .filter(|k| k.starts_with(&prefix) && k.len() > prefix.len())
            {
                scope.remove_config(&key)?;
            }
            if deck.id == DeckId(1) {
                let mut deck = deck;
                deck.name = names::unique(&scope.deck_rows()?, deck.id, "Default".into());
                // D8: name reset and stamp, preserving every other field.
                scope.put_deck_forced(&deck)?;
            } else {
                scope.remove_deck(deck.id)?;
                scope.add_grave(deck.id.0, GraveKind::Deck, Usn::PENDING)?;
            }
            result.decks += 1;
        }
    }
    Ok(result)
}

/// Back up off the writer, then commit only if decks/cards still match
/// that snapshot. A concurrent change replans and backs up the new state.
pub fn delete(
    collection: &Collection,
    ids: Vec<DeckId>,
    backup: Arc<dyn crate::ports::BackupService>,
) -> Result<OpResult<DeletionSummary>, EngineError> {
    delete_controlled(collection, ids, backup, CancelToken::new(), ProgressSink::new())
}

pub fn delete_controlled(
    collection: &Collection,
    ids: Vec<DeckId>,
    backup: Arc<dyn crate::ports::BackupService>,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<DeletionSummary>, EngineError> {
    for attempt in 0..=crate::executor::plan_commit::REPLANS {
        cancel.check()?;
        progress.set_phase(crate::ProgressPhase::Planning);
        let (summary, seq) = {
            let lease = collection.reader(ReaderRole::Maintenance)?;
            let snapshot = lease.snapshot()?;
            (deletion_summary(&*snapshot, &ids)?, snapshot.seq())
        };
        cancel.check()?;
        if summary.cards > 0 {
            backup.before_deck_delete()?;
        }
        cancel.check()?;
        let preconditions = [Table::Decks, Table::Cards]
            .into_iter()
            .map(|table| Precondition::TableUntouched { table, since: seq });
        let apply = ids.clone();
        // Bulk deletion can exceed the short write budget. The existing
        // bulk catalog entry lets the executor publish that delay (O9).
        let spec = if summary.cards + summary.decks > 2_000 {
            crate::OpSpec::new(crate::executor::catalog::BULK_OP, crate::OpSource::UserUndoable)
                .undo_step(crate::undo::names::DELETE_DECK, crate::StepKind::Other)
                .declare_long_write(
                    std::time::Duration::from_micros(100)
                        .saturating_mul(u32::try_from(summary.cards + summary.decks).unwrap_or(u32::MAX)),
                )
        } else {
            ops::spec("delete_decks", crate::undo::names::DELETE_DECK)
        };
        let result = collection.run(
            spec.cancel(cancel.clone())
                .progress(progress.clone())
                .preconditions(preconditions),
            move |ctx| delete_in(ctx.scope(), &apply),
        );
        match result {
            Err(EngineError::Stale { .. }) if attempt < crate::executor::plan_commit::REPLANS => continue,
            other => return other,
        }
    }
    unreachable!("all attempts return or replan")
}
