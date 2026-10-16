//! In-core observers (`engine/change-stream` S2): the `ChangeObserver` port
//! implementations U38 registers run on the writer thread after each commit
//! with a non-empty change set and before it is published, with the rows the
//! op changed. A failing or panicking observer is logged; the commit stands.

use std::sync::Arc;

use meguri_model::card::Card;
use meguri_model::changes::ChangeSet;
use meguri_model::ids::CardId;
use meguri_model::revlog::RevlogEntry;
use meguri_storage::{Record, RowError, RowImage};

use crate::changes::stream::Seq;
use crate::error::panic_message;
use crate::lifecycle::Generation;
use crate::ports::ChangeObserver;

/// One committed op as observers see it.
#[derive(Debug)]
pub struct ObservedCommit<'a> {
    pub seq: Seq,
    pub generation: Generation,
    pub changes: &'a ChangeSet,
    /// Every changed card, by id.
    pub cards: &'a [CardDelta],
    /// The stored rows of the review-log entries the op added.
    pub revlog_added: &'a [RevlogEntry],
    /// The before-images of the review-log entries the op removed.
    pub revlog_removed: &'a [RevlogEntry],
    /// Every changed config key, by key.
    pub config: &'a [ConfigDelta],
}

/// A changed card's row before and after the op (absent when added or
/// removed), read as a typed card on demand.
#[derive(Clone, Debug, PartialEq)]
pub struct CardDelta {
    pub id: CardId,
    pub before: Option<RowImage>,
    pub after: Option<RowImage>,
}

impl CardDelta {
    pub fn before_card(&self) -> Result<Option<Card>, RowError> {
        self.before.as_deref().map(Card::from_cells).transpose()
    }

    pub fn after_card(&self) -> Result<Option<Card>, RowError> {
        self.after.as_deref().map(Card::from_cells).transpose()
    }
}

/// A changed config key's stored value (`val` bytes) before and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDelta {
    pub key: String,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// Runs every observer in order; one that panics does not stop the others.
pub(crate) fn notify(observers: &[Arc<dyn ChangeObserver>], commit: &ObservedCommit<'_>) {
    for observer in observers {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer.observe(commit)));
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(observer = observer.name(), seq = commit.seq.0, %error, "a change observer failed");
            }
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                tracing::warn!(observer = observer.name(), seq = commit.seq.0, %message, "a change observer panicked");
            }
        }
    }
}
