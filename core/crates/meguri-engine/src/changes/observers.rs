//! In-core observers (`engine/change-stream` S2): the `ChangeObserver` port
//! implementations U38 registers run on the writer thread after each commit
//! with a non-empty change set and before it is published, with the rows the
//! op changed. A failing or panicking observer is logged; the commit stands.

use std::sync::Arc;

use meguri_model::card::Card;
use meguri_model::changes::ChangeSet;
use meguri_model::col::Col;
use meguri_model::ids::{CardId, RevlogId};
use meguri_model::revlog::RevlogEntry;
use meguri_storage::derived::CollectionMark;
use meguri_storage::journal::{Entry, Journal};
use meguri_storage::{Record, RowImage, RowKey, StorageError, Table};

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
    /// Immutable same-commit access. Cloning captures ownership, never images.
    pub payload: CommitPayload,
    /// Collection metadata decoded before the transaction commits.
    pub collection: Option<CollectionMark>,
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
    pub fn before_card(&self) -> Result<Option<Card>, StorageError> {
        Ok(self.before.as_deref().map(Card::from_cells).transpose()?)
    }

    pub fn after_card(&self) -> Result<Option<Card>, StorageError> {
        Ok(self.after.as_deref().map(Card::from_cells).transpose()?)
    }
}

/// A changed config key's stored value (`val` bytes) before and after.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDelta {
    pub key: String,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// Owned immutable capability for one commit. It contains no collection reader
/// or decoded cache. Keys can be enumerated and handles cloned without I/O;
/// each independent keyed read decodes at most that row on each side.
#[derive(Clone, Debug)]
pub struct CommitPayload {
    seq: Seq,
    generation: Generation,
    journal: Option<Arc<Journal>>,
    // Whole-collection external invalidations have no row journal.
    external_collection: Option<Arc<Col>>,
}

impl CommitPayload {
    pub(crate) fn new(seq: Seq, generation: Generation, journal: Option<Arc<Journal>>) -> Self {
        Self {
            seq,
            generation,
            journal,
            external_collection: None,
        }
    }

    pub(crate) fn external(seq: Seq, generation: Generation, col: Col) -> Self {
        Self {
            seq,
            generation,
            journal: None,
            external_collection: Some(Arc::new(col)),
        }
    }

    pub fn seq(&self) -> Seq {
        self.seq
    }
    pub fn generation(&self) -> Generation {
        self.generation
    }

    fn entries(&self, table: Table) -> impl Iterator<Item = &Entry> {
        self.journal
            .iter()
            .flat_map(|j| j.entries())
            .filter(move |e| e.table() == table && (e.existed_before() || e.exists_after()))
    }

    /// Every changed card key, in journal order, without reading any images.
    pub fn card_ids(&self) -> impl Iterator<Item = CardId> + '_ {
        self.entries(Table::Cards).map(|e| match e.key() {
            RowKey::Id(id) => CardId(*id),
            _ => unreachable!("cards have integer keys"),
        })
    }

    /// `None` means this key was not changed; absent sides are explicit in the delta.
    pub fn card(&self, id: CardId) -> Result<Option<CardDelta>, StorageError> {
        let Some(journal) = &self.journal else { return Ok(None) };
        let Some(entry) = journal.entry(Table::Cards, &RowKey::Id(id.0)) else {
            return Ok(None);
        };
        if !entry.existed_before() && !entry.exists_after() {
            return Ok(None);
        }
        Ok(Some(CardDelta {
            id,
            before: journal.before(entry)?,
            after: journal.after(entry)?,
        }))
    }

    pub fn cards(&self) -> impl Iterator<Item = Result<CardDelta, StorageError>> + '_ {
        self.card_ids().map(|id| self.card(id)?.ok_or_else(|| missing("card")))
    }

    pub fn config_keys(&self) -> impl Iterator<Item = &[u8]> {
        self.entries(Table::Config).map(|e| match e.key() {
            RowKey::Text(key) => key.as_slice(),
            _ => unreachable!("config has text keys"),
        })
    }

    pub fn config(&self, key: &str) -> Result<Option<ConfigDelta>, StorageError> {
        let Some(journal) = &self.journal else { return Ok(None) };
        let Some(entry) = journal.entry(Table::Config, &RowKey::text(key)) else {
            return Ok(None);
        };
        if !entry.existed_before() && !entry.exists_after() {
            return Ok(None);
        }
        fn value(image: Option<RowImage>) -> Result<Option<Vec<u8>>, StorageError> {
            let Some(image) = image else { return Ok(None) };
            if image.len() != Table::Config.columns().len() {
                return Err(missing("config columns"));
            }
            Ok(Some(meguri_model::config::ConfigEntry::from_cells(&image)?.value))
        }
        Ok(Some(ConfigDelta {
            key: key.to_owned(),
            before: value(journal.before(entry)?)?,
            after: value(journal.after(entry)?)?,
        }))
    }

    pub fn revlog_added_ids(&self) -> impl Iterator<Item = RevlogId> + '_ {
        self.revlog_ids(true)
    }
    pub fn revlog_removed_ids(&self) -> impl Iterator<Item = RevlogId> + '_ {
        self.revlog_ids(false)
    }
    fn revlog_ids(&self, added: bool) -> impl Iterator<Item = RevlogId> + '_ {
        self.entries(Table::Revlog)
            .filter(move |e| {
                if added {
                    !e.existed_before() && e.exists_after()
                } else {
                    e.existed_before() && !e.exists_after()
                }
            })
            .map(|e| match e.key() {
                RowKey::Id(id) => RevlogId(*id),
                _ => unreachable!("revlog has integer keys"),
            })
    }
    pub fn revlog_added(&self, id: RevlogId) -> Result<Option<RevlogEntry>, StorageError> {
        self.revlog(id, true)
    }
    pub fn revlog_removed(&self, id: RevlogId) -> Result<Option<RevlogEntry>, StorageError> {
        self.revlog(id, false)
    }
    fn revlog(&self, id: RevlogId, added: bool) -> Result<Option<RevlogEntry>, StorageError> {
        let Some(journal) = &self.journal else { return Ok(None) };
        let Some(entry) = journal.entry(Table::Revlog, &RowKey::Id(id.0)) else {
            return Ok(None);
        };
        if entry.existed_before() == entry.exists_after() || added != entry.exists_after() {
            return Ok(None);
        }
        let image = if added {
            journal.after(entry)?
        } else {
            journal.before(entry)?
        };
        let image = image.ok_or_else(|| missing("revlog"))?;
        Ok(Some(RevlogEntry::from_cells(&image)?))
    }

    pub fn collection(&self) -> Result<Option<Col>, StorageError> {
        if let Some(col) = &self.external_collection {
            return Ok(Some((**col).clone()));
        }
        let Some(journal) = &self.journal else { return Ok(None) };
        let Some(entry) = journal.entry(Table::Col, &RowKey::Id(1)) else {
            return Ok(None);
        };
        Ok(journal.after(entry)?.as_deref().map(Col::from_cells).transpose()?)
    }

    /// Raw-cell flag proof, retaining at most the requested pair of images.
    pub(crate) fn flag_only(&self, id: CardId) -> Result<bool, StorageError> {
        Ok(self
            .card(id)?
            .is_some_and(|r| meguri_sched::queue::rebuild_rules::flag_only(r.before.as_deref(), r.after.as_deref())))
    }

    #[cfg(feature = "test-hooks")]
    pub fn journal_owner(&self) -> Option<std::sync::Weak<Journal>> {
        self.journal.as_ref().map(Arc::downgrade)
    }

    #[cfg(feature = "test-hooks")]
    pub fn image_reads(&self) -> usize {
        self.journal.as_ref().map_or(0, |j| j.image_reads())
    }
    #[cfg(feature = "test-hooks")]
    pub fn held_image_bytes(&self) -> usize {
        self.journal.as_ref().map_or(0, |j| j.held_bytes())
    }
    #[cfg(feature = "test-hooks")]
    pub fn spilled_image_bytes(&self) -> u64 {
        self.journal.as_ref().map_or(0, |j| j.spilled_bytes())
    }
    #[cfg(feature = "test-hooks")]
    pub fn fail_image_reads(&self, fail: bool) {
        if let Some(journal) = &self.journal {
            journal.fail_image_reads(fail);
        }
    }
}

fn missing(what: &str) -> StorageError {
    StorageError::Io {
        detail: format!("invalid journal {what} image"),
    }
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
