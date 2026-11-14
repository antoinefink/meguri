//! V12.2/V13.4 local information. No preview, decision resolver or provider.
use super::{entity_codec::Kind, publication::Counts, row_payload, runtime::RuntimeError};
use meguri_engine::{
    Collection,
    device_state::{AccountBinding, DeviceState, PendingDecisionReason, PublicationDecision},
    media::MediaService,
};
use meguri_storage::{ReadRows, ReaderRole};
use std::{collections::BTreeMap, os::unix::fs::MetadataExt};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalPublicationCondition {
    Pending,
    Clear,
    LocalOnly,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalPendingDecisionReason {
    Restore,
    ColpkgImport,
    ManualImport,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolatedLocalContent {
    /// Tracking describes the collection replaced by a restore/import.
    ReplacedLocally,
    /// Tracking describes the file present before an external file replacement.
    FileMismatch,
    /// No assertion about presence in the current collection.
    Unverified,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalBackupKnowledge {
    /// No backup retention/availability proof was read.
    Unknown,
    /// Meguri did not back up the external file replacement (V13.4).
    NotTakenForExternalReplacement,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsolatedLocalInformation {
    pub pending_by_kind: BTreeMap<Kind, u64>,
    pub content: IsolatedLocalContent,
    pub backup: LocalBackupKnowledge,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTrackingInformation {
    /// Aggregates for ordinary isolated bindings within the current build scope.
    /// Entries have no defined order and expose neither account nor binding IDs.
    pub isolated: Vec<IsolatedLocalInformation>,
    /// Count of unresolved copies retained for ordinary bindings in that scope.
    /// Salvage ownership is excluded; this count makes no claim about other devices.
    pub unresolved_conflict_copies: u64,
}
/// Informational values only. Cannot be supplied to any confirmation method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalPublicationInformation {
    /// Engine generation only; not a publication or confirmation identity.
    pub generation: u64,
    pub collection_present: bool,
    /// None when no collection or no recorded decision exists.
    pub condition: Option<LocalPublicationCondition>,
    /// None includes the legacy pending decision with no recorded reason.
    pub pending_reason: Option<LocalPendingDecisionReason>,
    /// None for no collection, never a fabricated empty collection.
    pub local: Option<Counts>,
    pub last_review_ms: Option<i64>,
    pub last_change_ms: Option<i64>,
    /// None when no runtime/tracking store is available, never fabricated zeros.
    pub tracking: Option<LocalTrackingInformation>,
}
impl LocalPublicationInformation {
    pub fn absent(generation: u64) -> Self {
        Self {
            generation,
            collection_present: false,
            condition: None,
            pending_reason: None,
            local: None,
            last_review_ms: None,
            last_change_ms: None,
            tracking: None,
        }
    }
}

/// The exact nine-kind scan formerly embedded in begin_connection_preview.
/// The caller owns the collection snapshot and captured media-name view.
pub(crate) fn count_local_rows(rows: &impl ReadRows, media_names: usize) -> Result<Counts, RuntimeError> {
    let mut counts = BTreeMap::new();
    let nondefault_deck = rows.nondefault_deck_exists()?;
    for kind in row_payload::KINDS {
        let count = if let Some(table) = row_payload::table(kind) {
            rows.row_count(table)?
        } else {
            u64::try_from(media_names).map_err(|_| super::Error::InvalidData)?
        };
        counts.insert(kind, count);
    }
    let empty = [Kind::Note, Kind::Card, Kind::Revlog, Kind::MediaName]
        .iter()
        .all(|k| counts[k] == 0)
        && !nondefault_deck;
    Ok(Counts { counts, empty })
}

/// Capture off the writer. Reader ownership pins generation through validation;
/// optimistic sequence/media/sidecar checks refuse concurrent changes once,
/// without retrying or preparing a remote preview. No account/file provider.
pub fn read_local_publication_information(
    collection: &Collection,
    media: &MediaService,
    runtime: Option<&super::runtime::Runtime>,
) -> Result<LocalPublicationInformation, RuntimeError> {
    use super::publication::PublicationError::AuthorityChanged;
    let device = DeviceState::for_collection(collection.path());
    let lease = collection.reader(ReaderRole::Cloud)?;
    let before = device.publication_decision()?;
    let tracking_before = runtime.map(|r| r.local_publication_tracking()).transpose()?;
    let media_before = media.tracked_names()?;
    let feed_before = media.store().media_feed_status().map_err(|_| super::Error::Database)?;
    let snapshot = lease.snapshot()?;
    let local = count_local_rows(&*snapshot, media_before.names.len())?;
    // Reuse the existing model/storage semantics: max stored revlog id and
    // col.mod, both in epoch milliseconds, from this same pinned read view.
    let dates = snapshot.transfer_facts()?;
    let metadata = std::fs::symlink_metadata(collection.path()).map_err(meguri_storage::StorageError::from)?;
    if !metadata.file_type().is_file()
        || (metadata.dev(), metadata.ino()) != snapshot.wal_identity().native_file()
        || collection.generation().0 != media_before.generation
        || media.tracked_names()? != media_before
        || media.store().media_feed_status().map_err(|_| super::Error::Database)? != feed_before
        || runtime.map(|r| r.local_publication_tracking()).transpose()? != tracking_before
        || device.publication_decision()? != before
        || !collection.is_current()
        || collection.last_seq() != snapshot.seq()
    {
        return Err(AuthorityChanged.into());
    }
    let condition = before
        .as_ref()
        .map(|d| match (&d.publication.decision, &d.publication.binding) {
            (PublicationDecision::Pending, _) => LocalPublicationCondition::Pending,
            (PublicationDecision::Clear, AccountBinding::LocalOnly) => LocalPublicationCondition::LocalOnly,
            (PublicationDecision::Clear, _) => LocalPublicationCondition::Clear,
        });
    let pending_reason = before.as_ref().and_then(|d| d.pending_reason).map(|r| match r {
        PendingDecisionReason::Restore => LocalPendingDecisionReason::Restore,
        PendingDecisionReason::ColpkgImport => LocalPendingDecisionReason::ColpkgImport,
        PendingDecisionReason::ManualImport => LocalPendingDecisionReason::ManualImport,
    });
    Ok(LocalPublicationInformation {
        generation: collection.generation().0,
        collection_present: true,
        condition,
        pending_reason,
        local: Some(local),
        last_review_ms: dates.last_review.map(|v| v.0),
        last_change_ms: Some(dates.stamps.modified.0),
        tracking: tracking_before.and_then(|(_, facts)| facts),
    })
}
