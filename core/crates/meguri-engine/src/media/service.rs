use super::{MediaArrived, MediaJobKind, MediaJobLease, error, lock::MediaJobLock};
use crate::{Collection, EngineError, EventKind};
use meguri_model::{clock::Clock, ids::NoteId};
use meguri_storage::{
    ReadRows, ReaderRole, WriteScope,
    media::{MediaStore, NoteReference, RecoveryReport, StagedOp},
};
use std::sync::Arc;

/// One instance per open collection; clones share the journal and job lease.
#[derive(Clone, Debug)]
pub struct MediaService {
    collection: Collection,
    state: Arc<MediaState>,
}
#[derive(Debug)]
pub(crate) struct MediaState {
    store: MediaStore,
    jobs: MediaJobLock,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaNameSnapshot {
    pub generation: u64,
    pub root: std::path::PathBuf,
    pub names: Vec<meguri_storage::media::TrackedMediaName>,
}
impl MediaService {
    pub fn tracked_names(&self) -> Result<MediaNameSnapshot, EngineError> {
        // Keep the admitted lease until the journal and binding are captured.
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        Ok(MediaNameSnapshot {
            generation: self.collection.generation().0,
            root: self.state.store.media_dir().to_path_buf(),
            names: self.state.store.tracked_names().map_err(error)?,
        })
    }

    pub fn open(collection: &Collection, clock: Arc<dyn Clock>) -> Result<Self, EngineError> {
        let _reader = collection.operation_reader(ReaderRole::Query)?;
        let mut bound = collection
            .core
            .media
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = match bound.as_ref() {
            Some(state) => state.clone(),
            None => {
                let state = Arc::new(MediaState {
                    store: MediaStore::open(collection.path(), clock, collection.core.random.clone()).map_err(error)?,
                    jobs: MediaJobLock::default(),
                });
                *bound = Some(state.clone());
                state
            }
        };
        Ok(Self {
            collection: collection.clone(),
            state,
        })
    }
    pub fn store(&self) -> &MediaStore {
        &self.state.store
    }
    /// Recheck an admitted media operation's generation and lifecycle without
    /// reading the collection or retaining a reader across network/callbacks.
    pub fn check_generation(&self, expected: u64) -> Result<(), EngineError> {
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        if self.collection.generation().0 != expected {
            return Err(EngineError::StaleGeneration {
                planned: crate::Generation(expected),
                current: self.collection.generation(),
            });
        }
        Ok(())
    }
    pub fn try_job(&self, kind: MediaJobKind) -> Result<MediaJobLease, EngineError> {
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        self.state.jobs.try_acquire(kind)
    }
    /// Editor attachments outside a note transaction commit immediately.
    pub fn add(&self, name: &str, bytes: &[u8]) -> Result<String, EngineError> {
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        let stored = self.state.store.add(name, bytes).map_err(error)?;
        self.collection.core.exec.stream.publish(
            self.collection.generation(),
            None,
            EventKind::Extension(Arc::new(MediaArrived {
                names: vec![stored.clone()],
            })),
        );
        Ok(stored)
    }
    /// M6 fixed-name write for local ingestion. Replaced bytes go through the
    /// store's trash/publication journal; attachment collision naming is not used.
    /// The store returns a present case variant when it already has these bytes.
    /// Cloud incoming installs additionally need K9.3's case-collision refusal.
    pub fn write_exact(&self, name: &str, bytes: &[u8]) -> Result<String, EngineError> {
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        let stored = self.state.store.ensure_static_media(name, bytes).map_err(error)?;
        self.collection.core.exec.stream.publish(
            self.collection.generation(),
            None,
            EventKind::Extension(Arc::new(MediaArrived {
                names: vec![stored.clone()],
            })),
        );
        Ok(stored)
    }
    pub fn begin_staging(&self) -> Result<StagedOp, EngineError> {
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        self.state.store.begin_staging().map_err(error)
    }
    pub fn recover(&self) -> Result<RecoveryReport, EngineError> {
        let reader = self.collection.operation_reader(ReaderRole::Query)?;
        let mut snapshot = None;
        self.state
            .store
            .recover(|reference| {
                // Candidate capture excludes live stages. Pin the collection
                // only afterward, so every selected commit is visible.
                let snapshot = snapshot
                    .get_or_insert_with(|| reader.snapshot())
                    .as_ref()
                    .map_err(|e| meguri_storage::media::MediaError::Database(e.to_string()))?;
                snapshot
                    .note(NoteId(reference.note_id))
                    .map(|note| note.is_some_and(|note| reference.matches(&note)))
                    .map_err(|e| meguri_storage::media::MediaError::Database(e.to_string()))
            })
            .map_err(error)
    }
    /// U111 calls this only after publishing the download batch.
    pub fn media_arrived(&self, names: Vec<String>) -> Result<(), EngineError> {
        let _reader = self.collection.operation_reader(ReaderRole::Query)?;
        for name in &names {
            if self.state.store.lookup(name).map_err(error)?.is_none() {
                return Err(EngineError::InvalidArgument {
                    message: "media arrival names an unavailable file".into(),
                });
            }
        }
        if !names.is_empty() {
            self.collection.core.exec.stream.publish(
                self.collection.generation(),
                None,
                EventKind::Extension(Arc::new(MediaArrived { names })),
            );
        }
        Ok(())
    }
}

pub(crate) struct PendingMedia {
    pub stage: StagedOp,
    pub notes: Vec<NoteId>,
}
impl PendingMedia {
    /// Read the final stored post-images, after every step of the op.
    pub fn prepare(&mut self, scope: &WriteScope<'_>) -> Result<(), EngineError> {
        use meguri_storage::{Record, RowKey, Table};
        let names = self.stage.names().map_err(error)?;
        let mut referenced = std::collections::BTreeSet::new();
        let references =
            self.notes
                .iter()
                .map(|id| {
                    let note = scope.note(*id)?.ok_or(EngineError::NoteNotFound { id: *id })?;
                    if !names.is_empty() {
                        let journal = scope.journal().ok_or_else(|| EngineError::InvalidArgument {
                            message: "staged media requires a journaled note change".into(),
                        })?;
                        let entry = journal.entry(Table::Notes, &RowKey::Id(id.0)).ok_or_else(|| {
                            EngineError::InvalidArgument {
                                message: "staged media witness was not changed by this operation".into(),
                            }
                        })?;
                        if let Some(before) = journal.before(entry)?
                            && NoteReference::of(
                                &meguri_model::note::Note::from_cells(&before)
                                    .map_err(meguri_storage::StorageError::from)?,
                            ) == NoteReference::of(&note)
                        {
                            return Err(EngineError::InvalidArgument {
                                message: "staged media witness must differ from its pre-operation row".into(),
                            });
                        }
                        referenced.extend(meguri_text::media_refs::local_references(&note.fields));
                    }
                    Ok(NoteReference::of(&note))
                })
                .collect::<Result<Vec<_>, EngineError>>()?;
        if names.iter().any(|name| !referenced.contains(name)) {
            return Err(EngineError::InvalidArgument {
                message: "staged media must be referenced by its witness notes".into(),
            });
        }
        self.stage.set_references(&references).map_err(error)
    }
}
