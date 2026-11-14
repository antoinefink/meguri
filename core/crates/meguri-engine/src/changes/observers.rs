//! In-core observers (`engine/change-stream` S2): the `ChangeObserver` port
//! implementations U38 registers run on the writer thread after each commit
//! with a non-empty change set and before it is published, with the rows the
//! op changed. A failing or panicking observer is logged; the commit stands.

use std::sync::Arc;

use meguri_model::card::Card;
use meguri_model::changes::ChangeSet;
use meguri_model::col::Col;
use meguri_model::ids::{CardId, NotetypeId, RevlogId};
use meguri_model::revlog::RevlogEntry;
use meguri_storage::derived::CollectionMark;
use meguri_storage::journal::{After, Entry, Journal, NoteDigest};
use meguri_storage::{ReadRows, Record, RowImage, RowKey, StorageError, Table, Writer};

use crate::changes::stream::Seq;
use crate::error::panic_message;
use crate::lifecycle::Generation;
use crate::ports::ChangeObserver;

/// Read-only access to the committed writer's rows, only during a callback.
/// Copy any needed row images before returning. This capability is never
/// part of CommitPayload and must not reenter the writer or call Swift.
///
/// ```compile_fail
/// use meguri_engine::ObserverRows;
/// fn retain<'a>(rows: &'a ObserverRows<'a>) -> &'static ObserverRows<'static> {
///     rows
/// }
/// ```
pub struct ObserverRows<'a> {
    writer: &'a Writer,
    native_owner: Option<(&'a crate::executor::ExecShared, i64, (u64, u64))>,
}

impl ObserverRows<'_> {
    /// Cover the original writer's commit before granting native layout
    /// authority. A foreign commit cannot enter the pinned image while its
    /// observer commits dependent tracking state. This never submits an op.
    pub fn with_native_layout_authority<T, E>(&self, capture: impl FnOnce() -> Result<T, E>) -> Result<T, E>
    where E: From<crate::EngineError> + From<StorageError> {
        let (shared, version, file) = self.native_owner
            .ok_or_else(|| crate::EngineError::unavailable("native commit ownership unavailable"))?;
        if self.writer.external_data_version()? != version || self.writer.validate_native_identity()? != file {
            return Err(crate::EngineError::unavailable("native commit image changed").into());
        }
        shared.coverage.sync(shared, self.writer)?;
        self.writer.with_committed_image(version, file, capture)
    }

    pub fn validate_native_layout_image(&self) -> Result<(), crate::EngineError> {
        let (_, version, file) = self.native_owner
            .ok_or_else(|| crate::EngineError::unavailable("native commit ownership unavailable"))?;
        if self.writer.external_data_version()? != version || self.writer.validate_native_identity()? != file {
            return Err(crate::EngineError::unavailable("native commit image changed"));
        }
        Ok(())
    }

    /// Exact raw millisecond ID from the borrowed committed writer view.
    /// Returns zero only when this card has no history.
    pub fn max_revlog_id(&self, card: CardId) -> Result<i64, StorageError> {
        self.writer.max_revlog_id(card)
    }

    /// Raw notetype parts from this same committed image, including parts
    /// newly added or removed by the op (never a tracking-layout enumeration).
    pub fn notetype_fields_page(&self, id: meguri_model::ids::NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError> {
        self.writer.notetype_fields_page(id, after)
    }
    pub fn notetype_templates_page(&self, id: meguri_model::ids::NotetypeId, after: Option<i64>) -> Result<Vec<RowImage>, StorageError> {
        self.writer.notetype_templates_page(id, after)
    }

    /// Raw ordinal keys, including malformed classes, on this committed view.
    pub fn notetype_parts_raw_page(&self, table: Table, id: meguri_model::ids::NotetypeId, after: Option<&meguri_storage::Cell>) -> Result<Vec<RowImage>, StorageError> {
        self.writer.notetype_parts_raw_page(table, id, after)
    }

    /// Reads a real stored row from this commit, before any later op runs.
    pub fn row_image(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
        self.writer.row_image(table, key)
    }
}

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

/// A changed row's stored after-image, or the journal's compact note metadata.
#[derive(Clone, PartialEq)]
pub enum RowAfter {
    Absent,
    Row(RowImage),
    NoteDigest(NoteDigest),
}

impl std::fmt::Debug for RowAfter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Absent => "RowAfter::Absent",
            Self::Row(_) => "RowAfter::Row",
            Self::NoteDigest(_) => "RowAfter::NoteDigest",
        })
    }
}

/// One changed row in stored column order, without model coercion.
/// `before: None` and `after: RowAfter::Absent` mean the row did not exist
/// on the corresponding side. Notes retain the journal's digest after-image.
#[derive(Clone, PartialEq)]
pub struct RowDelta {
    pub table: Table,
    /// The journal's original key, including for a case-equivalent tag lookup.
    pub key: RowKey,
    pub before: Option<RowImage>,
    pub after: RowAfter,
}

impl std::fmt::Debug for RowDelta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RowDelta")
    }
}

fn row_delta(journal: &Journal, entry: &Entry) -> Result<RowDelta, StorageError> {
    Ok(RowDelta {
        table: entry.table(),
        key: entry.key().clone(),
        before: journal.before(entry)?,
        after: match entry.after() {
            After::Absent => RowAfter::Absent,
            After::Note(digest) => RowAfter::NoteDigest(*digest),
            After::Row(_) | After::Spilled { .. } => journal.after(entry)?.map_or(RowAfter::Absent, RowAfter::Row),
        },
    })
}

/// Journal presence facts, read without decoding either stored image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RowPresence {
    pub before: bool,
    pub after: bool,
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

    /// Explicit native identity maps; absent for external or unreported writes.
    pub fn identity_layout(&self, id: NotetypeId) -> Option<&meguri_storage::journal::IdentityLayout> {
        self.journal.as_ref()?.identity_layout(id.0)
    }

    /// Changed keys in journal order, without reading images. External
    /// invalidations without a journal have no keys or row deltas.
    pub fn row_keys(&self, table: Table) -> impl Iterator<Item = &RowKey> {
        self.entries(table).map(Entry::key)
    }

    /// `None` means no effective journal entry for this key. Presence comes
    /// from the journal itself, including entries whose images were spilled.
    pub fn row_presence(&self, table: Table, key: &RowKey) -> Option<RowPresence> {
        let entry = self.journal.as_ref()?.entry(table, key)?;
        let presence = RowPresence { before: entry.existed_before(), after: entry.exists_after() };
        (presence.before || presence.after).then_some(presence)
    }

    /// Reads only this key's stored sides. `None` means no journalled change;
    /// keys use the table's collation. Note after-images expose the journal's
    /// digest and changed columns. Image read failures return a storage error.
    pub fn row(&self, table: Table, key: &RowKey) -> Result<Option<RowDelta>, StorageError> {
        let Some(journal) = &self.journal else { return Ok(None) };
        let Some(entry) = journal.entry(table, key) else {
            return Ok(None);
        };
        if !entry.existed_before() && !entry.exists_after() {
            return Ok(None);
        }
        row_delta(journal, entry).map(Some)
    }

    /// Reads each visited entry directly, retaining only its row delta.
    /// Errors are yielded at the affected entry; unvisited images are not read.
    pub fn rows(&self, table: Table) -> impl Iterator<Item = Result<RowDelta, StorageError>> + '_ {
        self.journal.iter().flat_map(move |journal| {
            journal
                .entries()
                .filter(move |e| e.table() == table && (e.existed_before() || e.exists_after()))
                .map(move |entry| row_delta(journal, entry))
        })
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
pub(crate) fn notify(
    observers: &[Arc<dyn ChangeObserver>], commit: &ObservedCommit<'_>, writer: &Writer,
    native_owner: Option<(&crate::executor::ExecShared, i64, (u64, u64))>,
) {
    let rows = ObserverRows { writer, native_owner };
    for observer in observers {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            observer.observe_with_rows(commit, &rows)
        }));
        match outcome {
            Ok(Ok(())) => continue,
            Ok(Err(error)) => {
                tracing::warn!(observer = observer.name(), seq = commit.seq.0, %error, "a change observer failed");
            }
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                tracing::warn!(observer = observer.name(), seq = commit.seq.0, %message, "a change observer panicked");
            }
        }
        // Exactly once for this observer's failed callback. A failing hook
        // cannot recursively signal failure or interrupt later observers.
        if let Err(payload) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer.observe_failed(commit.seq)))
        {
            let message = panic_message(payload.as_ref());
            tracing::warn!(observer = observer.name(), seq = commit.seq.0, %message, "a change observer failure hook panicked");
        }
    }
}
