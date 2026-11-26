//! Before-acceptance identity for U1 lifecycle operations, independent of C12.
use crate::{EngineError, OpSpec, Seq, undo::names};
use meguri_model::{card::Card, ids::DeckId};
use meguri_sched::session::token::StateToken;
use meguri_storage::{Journal, Record, Table};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, sync::Arc};

/// Cloned into each reader snapshot under the commit lock. Only lifecycle
/// commits replace the shared map; ordinary answers and edits keep it intact.
#[derive(Clone, Default)]
pub(crate) struct FilteredLifecycle(Arc<BTreeMap<DeckId, Seq>>);

impl FilteredLifecycle {
    pub(crate) fn record(&mut self, seq: Seq, decks: BTreeSet<DeckId>) {
        if !decks.is_empty() {
            let revisions = Arc::make_mut(&mut self.0);
            for deck in decks {
                revisions.insert(deck, seq);
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
