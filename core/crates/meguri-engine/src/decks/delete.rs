//! D8: summary, backup, subtree deletion, and graves in one undo step.
//! The counts the delete question showed are a precondition (D8 step 6,
//! changed 2026-11-21): counted again on the planning snapshot, before the
//! backup, and inside the write transaction before its first write; a
//! larger live count writes nothing and returns `LossesChanged`.
use super::{Refusal, filtered, is_filtered, names, ops, refused};
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

/// D8 step 6's input: what the delete question stated, or `Any` for a
/// caller whose request already accepts every loss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmedLosses {
    /// The `decks` and `cards` of the last summary read before the delete
    /// (the question's, or the zero-card read's when it asked nothing).
    Counts(DeletionSummary),
    /// AnkiConnect `deleteDecks` with `cardsToo` true (A5.6).
    Any,
}

/// D8 step 6's comparison: each live count against its own confirmed count.
fn check(confirmed: ConfirmedLosses, live: DeletionSummary) -> Result<(), EngineError> {
    match confirmed {
        ConfirmedLosses::Counts(shown) if live.cards > shown.cards || live.decks > shown.decks => {
            Err(refused(Refusal::LossesChanged {
                decks: live.decks as u64,
                cards: live.cards as u64,
            }))
        }
        _ => Ok(()),
    }
}

#[cfg(feature = "test-hooks")]
thread_local! {
    static WITHOUT_TABLE_PRECONDITIONS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` with this thread's deck deletes submitted without their table
/// preconditions, so the counts in the write transaction must refuse alone
/// (D16).
#[cfg(feature = "test-hooks")]
pub fn without_table_preconditions_for_testing<R>(f: impl FnOnce() -> R) -> R {
    WITHOUT_TABLE_PRECONDITIONS.with(|flag| flag.set(true));
    let result = f();
    WITHOUT_TABLE_PRECONDITIONS.with(|flag| flag.set(false));
    result
}

fn table_preconditions() -> bool {
    #[cfg(feature = "test-hooks")]
    if WITHOUT_TABLE_PRECONDITIONS.with(|flag| flag.get()) {
        return false;
    }
    true
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
/// that snapshot. A concurrent change replans with the same protected backup.
pub fn delete(
    collection: &Collection,
    ids: Vec<DeckId>,
    confirmed: ConfirmedLosses,
    backup: Arc<dyn crate::ports::BackupService>,
) -> Result<OpResult<DeletionSummary>, EngineError> {
    delete_controlled(collection, ids, confirmed, backup, CancelToken::new(), ProgressSink::new())
}

pub fn delete_controlled(
    collection: &Collection,
    ids: Vec<DeckId>,
    confirmed: ConfirmedLosses,
    backup: Arc<dyn crate::ports::BackupService>,
    cancel: Arc<CancelToken>,
    progress: ProgressSink,
) -> Result<OpResult<DeletionSummary>, EngineError> {
    let mut safety = None;
    for attempt in 0..=crate::executor::plan_commit::REPLANS {
        cancel.check()?;
        progress.set_phase(crate::ProgressPhase::Planning);
        let (summary, seq) = {
            let lease = collection.reader(ReaderRole::Maintenance)?;
            let snapshot = lease.snapshot()?;
            let summary = deletion_summary(&*snapshot, &ids)?;
            // Counted on this snapshot: the decks this attempt plans to process.
            progress.report(0, Some(summary.decks as u64));
            (summary, snapshot.seq())
        };
        cancel.check()?;
        // The first count: an out-of-date confirmation costs no backup.
        check(confirmed, summary)?;
        if summary.cards > 0 && safety.is_none() {
            safety = Some(backup.before_safety_operation(collection, &cancel)?);
        }
        cancel.check()?;
        let backed_up = safety.is_some();
        let tables: &[Table] = if table_preconditions() { &[Table::Decks, Table::Cards] } else { &[] };
        let preconditions = tables
            .iter()
            .copied()
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
            move |ctx| {
                // The second count, on this transaction's rows after the
                // executor's preconditions and before the first write.
                let live = deletion_summary(ctx.scope(), &apply)?;
                if live.cards > 0 && !backed_up {
                    // Cards the plan did not see and no backup yet (D8 step 6,
                    // "Backup at commit"): plan again so step 4 backs up first.
                    return Err(EngineError::Stale {
                        table: Table::Cards,
                        key: "deck deletion".into(),
                    });
                }
                check(confirmed, live)?;
                delete_in(ctx.scope(), &apply)
            },
        );
        match result {
            Err(EngineError::Stale { .. }) if attempt < crate::executor::plan_commit::REPLANS => continue,
            other => return other,
        }
    }
    unreachable!("all attempts return or replan")
}
