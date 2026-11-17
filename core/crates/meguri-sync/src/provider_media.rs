//! Sealed accounting of every file promised by the live media inventory.
use crate::policy::{MediaStage, Operation, OperationId, Scope};
use meguri_storage::media::{MediaInventory, MediaStore, MediaSyncState, UnsupportedMedia};
use std::{collections::{BTreeMap, BTreeSet}, fmt};

pub const MAX_ENTRY_BYTES: u64 = crate::wire::media::MAX_DOWNLOAD_ENTRY_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromisedState {
    Transferred,
    Pending,
    Excluded(UnsupportedMedia),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromisedFile {
    pub name: String,
    pub lossy_name: bool,
    pub state: PromisedState,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MediaAccounting {
    operation: OperationId,
    operation_revision: u64,
    generation: u64,
    files: Vec<PromisedFile>,
    remote_pending: u64,
    verified: bool,
}

impl fmt::Debug for MediaAccounting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MediaAccounting")
            .field("files", &self.files.len())
            .field("transferred", &self.transferred())
            .field("pending", &self.files.iter().filter(|file| file.state == PromisedState::Pending).count())
            .field("excluded", &self.files.iter().filter(|file| matches!(file.state, PromisedState::Excluded(_))).count())
            .field("remote_pending", &self.remote_pending)
            .field("verified", &self.verified)
            .finish()
    }
}

impl MediaAccounting {
    pub fn operation(&self) -> &OperationId {
        &self.operation
    }

    pub fn operation_revision(&self) -> u64 {
        self.operation_revision
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn files(&self) -> &[PromisedFile] {
        &self.files
    }

    pub fn remote_pending(&self) -> u64 {
        self.remote_pending
    }

    pub fn verified(&self) -> bool {
        self.verified
    }

    /// Completion requires verified authority and no pending or excluded work.
    pub fn is_complete(&self) -> bool {
        self.verified
            && self.remote_pending == 0
            && self.files.iter().all(|file| file.state == PromisedState::Transferred)
    }

    pub fn transferred(&self) -> u64 {
        self.files.iter().filter(|file| file.state == PromisedState::Transferred).count() as u64
    }

    pub fn affected(&self) -> impl Iterator<Item = &PromisedFile> {
        self.files.iter().filter(|file| file.state != PromisedState::Transferred)
    }
}

/// Recomputes accounting from the operation, a fresh inventory and the journal.
/// Call only while holding the media-job lease, after the operation's media job
/// has stopped. This takes no lease and opens no collection reader. Journal
/// reads may run existing removal-intent recovery, including journal updates
/// and trash receipt cleanup; this is not a no-write API.
pub fn account_media(
    store: &MediaStore,
    inventory: &MediaInventory,
    op: &Operation,
) -> meguri_storage::media::Result<MediaAccounting> {
    let entries: BTreeMap<_, _> = store.entries()?.into_iter()
        .map(|entry| (entry.name.clone(), entry)).collect();
    let files: Vec<_> = inventory.files().iter().map(|file| {
        let state = match file.unsupported {
            Some(reason) => PromisedState::Excluded(reason),
            None => {
                if entries.get(&file.name).is_some_and(|entry| {
                    entry.sync_state == MediaSyncState::Live
                        && !entry.dirty
                        && !entry.pending_download
                        && file.matches_journal(entry)
                }) {
                    PromisedState::Transferred
                } else {
                    PromisedState::Pending
                }
            }
        };
        PromisedFile { name: file.name.clone(), lossy_name: file.lossy_name, state }
    }).collect();
    let names: BTreeSet<_> = inventory.files().iter().map(|file| file.name.as_str()).collect();
    let remote_pending = store.awaiting_names()?.iter()
        .filter(|name| !names.contains(name.as_str())).count() as u64;
    let oversized: BTreeSet<_> = files.iter()
        .filter(|file| file.state == PromisedState::Excluded(UnsupportedMedia::OverLimit))
        .map(|file| file.name.as_str()).collect();
    let empty: BTreeSet<_> = files.iter()
        .filter(|file| file.state == PromisedState::Excluded(UnsupportedMedia::Empty))
        .map(|file| file.name.as_str()).collect();
    let verified = op.scope == Scope::CollectionAndMedia
        && op.collection_complete
        && op.actual_generation.is_some()
        && op.media.stage == MediaStage::Complete
        && op.media.exclusions_verified
        && op.media.counters.oversized.iter().all(|name| oversized.contains(name.as_str()))
        && op.media.counters.empty.iter().all(|name| empty.contains(name.as_str()))
        && inventory.max_entry_bytes() == MAX_ENTRY_BYTES
        && inventory.is_from(store);
    Ok(MediaAccounting {
        operation: op.id.clone(),
        operation_revision: op.revision,
        generation: op.actual_generation.unwrap_or(0),
        files,
        remote_pending,
        verified,
    })
}
