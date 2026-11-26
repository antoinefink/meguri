//! Before-acceptance identity for U1 lifecycle operations, independent of C12.
use crate::{EngineError, OpSpec, Seq, undo::names};
use meguri_model::{card::Card, changes::{ChangeKind, DeckChange}, ids::DeckId};
use meguri_sched::session::token::StateToken;
use meguri_storage::{Journal, Record, Table};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

/// Cloned into each reader snapshot under the commit lock. Only lifecycle
/// commits and deck deletions replace the shared map; ordinary answers and
/// edits keep it intact. The executor survives file replacement, which resets
/// this state before any reader of the new generation opens.
#[derive(Clone, Default)]
pub(crate) struct FilteredLifecycle(Arc<BTreeMap<DeckId, Seq>>);

impl FilteredLifecycle {
    pub(crate) fn revision(&self, deck: DeckId) -> Option<Seq> {
        self.0.get(&deck).copied()
    }

    pub(crate) fn record(&mut self, seq: Seq, decks: BTreeSet<DeckId>, changes: &[DeckChange]) {
        let removed = || changes.iter().filter(|d| d.kind == ChangeKind::Removed).map(|d| d.id);
        if !decks.is_empty() || removed().any(|id| self.0.contains_key(&id)) {
            let revisions = Arc::make_mut(&mut self.0);
            for deck in decks {
                revisions.insert(deck, seq);
            }
            for deck in removed() {
                revisions.remove(&deck);
            }
        }
    }

    pub(crate) fn token(&self, token: StateToken, deck: DeckId) -> StateToken {
        let mut hash = Sha256::new();
        hash.update(b"meguri.session.filtered-lifecycle.v1\0");
        hash.update(token);
        hash.update(self.0.get(&deck).copied().unwrap_or_default().0.to_le_bytes());
        hash.finalize().into()
    }
}

/// The existing writer operation identifies the lifecycle even when its final
/// journal images equal its initial images. Collect both sides before commit:
/// Empty has only the old membership, while a new build has only the new one.
pub(crate) fn affected(spec: &OpSpec, journal: Option<&Journal>) -> Result<BTreeSet<DeckId>, EngineError> {
    if !matches!(spec.undo.step, Some((names::BUILD_DECK | names::EMPTY | names::CUSTOM_STUDY, _))) {
        return Ok(BTreeSet::new());
    }
    let mut decks = BTreeSet::new();
    if let Some(journal) = journal {
        for entry in journal.entries().filter(|e| e.table() == Table::Cards) {
            for image in [journal.before(entry)?, journal.after(entry)?].into_iter().flatten() {
                let card = Card::from_cells(&image).map_err(meguri_storage::StorageError::from)?;
                if card.original_deck_id.0 != 0 {
                    decks.insert(card.deck_id);
                }
            }
        }
    }
    Ok(decks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_deck_retires_only_its_revision_and_preserves_snapshot_tokens() {
        let mut live = FilteredLifecycle::default();
        let deleted = DeckId(10);
        let other = DeckId(11);
        live.record(Seq(1), [deleted, other].into_iter().collect(), &[]);
        let snapshot = live.clone();
        let token = [0; 32];
        live.record(Seq(2), BTreeSet::new(), &[DeckChange { id: deleted, kind: ChangeKind::Removed }]);
        assert!(!live.0.contains_key(&deleted));
        assert_eq!(live.token(token, deleted), FilteredLifecycle::default().token(token, deleted));
        assert_ne!(snapshot.token(token, deleted), live.token(token, deleted));
        assert_eq!(snapshot.token(token, other), live.token(token, other));
    }
}
