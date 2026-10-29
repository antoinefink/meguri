//! Production V14 collection adoption. The platform supplies record replies;
//! Runtime owns the slot, unpublished stage, validation and durable transitions.
use super::*;
use crate::cloud::{
    adoption::*,
    control_codec::{DecodeOutcome, Epoch, EpochState, MediaRoot},
    publication::*,
    workflow::{EntityFacts, Saved},
};
use meguri_engine::{
    Engine, LongOp, SlotGuard,
    device_state::{AccountBinding, Publication, PublicationDecision, PublicationId},
    replace::{CloudAdoptInput, ReplaceReason, ReplacementStage},
};
use meguri_model::{
    col::Col,
    time::{EpochMillis, EpochSecs},
};
use meguri_storage::{
    atomic_file::{AtomicFile, Purpose},
    space::SpaceProvider,
};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
};

pub(super) struct AdoptionOwner {
    slot: SlotGuard,
    directory: PathBuf,
    stage: Mutex<Option<ReplacementStage>>,
    next: Mutex<Option<AdoptionTask>>,
}
impl AdoptionOwner {
    fn set_next(&self, task: AdoptionTask) {
        *self.next.lock().unwrap_or_else(PoisonError::into_inner) = Some(task);
    }
    fn claim(&self, task: &AdoptionTask) -> Result<(), RuntimeError> {
        let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
        if next.as_ref() != Some(task) {
            return Err(RuntimeError::StaleIncarnation);
        }
        *next = None;
        Ok(())
    }
}
fn refused() -> RuntimeError {
    Error::InvalidData.into()
}
fn storage_io(e: std::io::Error) -> RuntimeError {
    meguri_storage::StorageError::from(e).into()
}
fn publication_id(id: [u8; 16]) -> PublicationId {
    PublicationId::from_bytes(id)
}
fn epoch(op: &AdoptOp) -> Result<Epoch, RuntimeError> {
    let record = op.epoch_record.as_ref().ok_or_else(refused)?;
    let DecodeOutcome::Supported(epoch) =
        Epoch::decode(record.encrypted.as_deref().ok_or_else(refused)?).map_err(|_| refused())?
    else {
        return Err(refused());
    };
    if epoch.epoch_id != op.epoch.0 || epoch.state != EpochState::Complete || epoch.ver != 18 {
        return Err(refused());
    }
    Ok(epoch)
}
fn root(op: &AdoptOp) -> Result<MediaRoot, RuntimeError> {
    let record = op.media_root.as_ref().ok_or_else(refused)?;
    let DecodeOutcome::Supported(root) =
        MediaRoot::decode(record.encrypted.as_deref().ok_or_else(refused)?).map_err(|_| refused())?
    else {
        return Err(refused());
    };
    Ok(root)
}
fn saved(path: &Path) -> Result<Saved, RuntimeError> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(storage_io)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(storage_io)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(refused());
    }
    serde_json::from_slice(&bytes).map_err(|_| refused())
}
fn received(op: &AdoptOp, record: &Saved) -> Result<codec::Received, RuntimeError> {
    let received = codec::receive(
        &epoch(op)?.record_key,
        &record.name,
        &record.envelope,
        codec::AssetInput::Absent,
    )
    .map_err(|_| refused())?;
    // K19 and remote asset/tombstone application require the next phase. Never
    // expose a partially installed collection when these are encountered.
    if received.entity.fields.generation.is_some()
        || received.entity.fields.tombstone
        || received.payload.is_none()
        || received.entity.fields.erev == 0
        || record.system_fields.is_empty()
    {
        return Err(refused());
    }
    if let Some(payload) = &received.payload {
        if payload.kind() != codec::Kind::MediaName
            && crate::cloud::row_payload::payload_key(payload)? != received.entity.fields.key
        {
            return Err(refused());
        }
    }
    Ok(received)
}
fn spool(directory: &Path) -> Result<Vec<PathBuf>, RuntimeError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory.join("incoming")).map_err(storage_io)? {
        let entry = entry.map_err(storage_io)?;
        if !entry.file_type().map_err(storage_io)?.is_file() {
            return Err(refused());
        }
        paths.push(entry.path());
    }
    paths.sort();
    Ok(paths)
}
fn check_space(space: &dyn SpaceProvider, directory: &Path) -> Result<(), RuntimeError> {
    fn size(path: &Path) -> Result<u64, RuntimeError> {
        let mut bytes = 0_u64;
        for entry in fs::read_dir(path).map_err(storage_io)? {
            let entry = entry.map_err(storage_io)?;
            let meta = entry.metadata().map_err(storage_io)?;
            let n = if meta.is_dir() {
                size(&entry.path())?
            } else if meta.is_file() {
                meta.len()
            } else {
                return Err(refused());
            };
            bytes = bytes.checked_add(n).ok_or_else(refused)?;
        }
        Ok(bytes)
    }
    let required = size(directory)?
        .checked_mul(2)
        .and_then(|n| n.checked_add(64 * 1024 * 1024))
        .ok_or_else(refused)?;
    meguri_storage::space::require_space(space, directory, required)?;
    Ok(())
}
fn require_empty_media(media: &meguri_engine::media::MediaService) -> Result<(), RuntimeError> {
    let store = media.store();
    let feed = store.media_feed_snapshot().map_err(|_| refused())?;
    let mut entries = fs::read_dir(store.media_dir()).map_err(storage_io)?;
    if !feed.status.complete
        || !feed.items.is_empty()
        || !store.tracked_names().map_err(|_| refused())?.is_empty()
        || store.pending_any().map_err(|_| refused())?
        || entries.next().transpose().map_err(storage_io)?.is_some()
    {
        return Err(PublicationError::MediaApplyUnavailable.into());
    }
    Ok(())
}
impl Runtime {
    /// Fixed catalog boundary for the child crash harness only.
    #[cfg(feature = "test-hooks")]
    pub fn arm_adoption_activation_crash_for_testing(&self) {
        meguri_model::faultpoint::arm("cloud.adopt.after_activation", meguri_model::faultpoint::Fault::Abort);
    }
    fn adoption_clock(&self) -> Result<Arc<dyn meguri_model::clock::Clock>, RuntimeError> {
        self.adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData.into())
    }
    fn adoption_owner(&self) -> Result<Arc<AdoptionOwner>, RuntimeError> {
        self.adoption
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(RuntimeError::StaleIncarnation)
    }
    pub fn adoption_op(&self) -> Result<Option<AdoptOp>, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.adoption_op()?))
    }
    fn write_adoption(&self, op: &AdoptOp) -> Result<(), RuntimeError> {
        let mut s = self.state();
        with_tracking_access(|| Ok(s.store.as_mut().ok_or(Error::Database)?.update_adoption(op)?))
    }
    fn check_adoption_before(&self, op: &AdoptOp, collection: &Collection) -> Result<(), RuntimeError> {
        meguri_engine::device_state::DeviceState::for_collection(collection.path()).settled_publication()?;
        if self.publication_account()? != op.account
            || self.publication_decision()? != op.before
            || self.inputs.files.identity(collection.path())? != op.file_before
            || !collection.is_current()
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        Ok(())
    }
    /// V14.2 steps 2–3. Network starts only when the host later drives work.
    pub fn confirm_cloud_adoption(
        &self,
        preview: &Preview,
        collection: &Collection,
        engine: &Engine,
    ) -> Result<[u8; 16], RuntimeError> {
        self.check_preview_authority(preview, collection)?;
        meguri_engine::device_state::DeviceState::for_collection(collection.path()).settled_publication()?;
        if !matches!(
            preview.decision,
            ConnectDecision::ConfirmAdoption | ConnectDecision::ChooseCollection
        ) {
            return Err(PublicationError::NotOffered.into());
        }
        {
            let s = self.state();
            if s.publication.preview.as_ref() != Some(&(preview.clone(), true)) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                if store.publication_op()?.is_some() || store.adoption_op()?.is_some() {
                    return Err(PublicationError::Busy.into());
                }
                if store
                    .publication_binding_for_account(&preview.account)?
                    .is_some_and(|b| b.facts.state == BindingState::Isolated(IsolationReason::EncryptedDataReset))
                {
                    return Err(PublicationError::ResetRecoveryUnavailable.into());
                }
                Ok(())
            })?;
        }
        let slot = engine.acquire_slot(LongOp::CloudAdopt)?;
        slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        let stage = ReplacementStage::new(&self.directory)?;
        fs::create_dir(stage.directory().join("incoming")).map_err(storage_io)?;
        let id = self.publication_random_id()?;
        let candidate = publication_id(self.publication_random_id()?);
        if candidate == preview.decision_before.publication.identity || candidate == publication_id(id) {
            return Err(refused());
        }
        let binding_after = BindingId(self.publication_random_id()?);
        let binding = AccountBinding::Connected {
            account: String::from_utf8(preview.account.0.clone()).map_err(|_| refused())?,
            configuration: self.inputs.scope.container.as_str().into(),
            environment: match self.inputs.scope.environment {
                Environment::Development => "development",
                Environment::Production => "production",
            }
            .into(),
        };
        let mut op = AdoptOp {
            id,
            step: AdoptStep::Staging,
            binding_before: None,
            binding_after,
            account: preview.account.clone(),
            before: preview.decision_before.clone(),
            file_before: preview.native_file,
            candidate: Publication {
                identity: candidate,
                binding,
                decision: PublicationDecision::Clear,
            },
            active: preview.active.clone().ok_or(PublicationError::NotOffered)?,
            epoch: preview.active_epoch.ok_or(PublicationError::NotOffered)?,
            epoch_record: None,
            media_root: None,
            staging_name: stage
                .directory()
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(refused)?
                .into(),
            started_at_ms: self.adapter_now()?,
        };
        self.check_preview_authority(preview, collection)?;
        let owner = Arc::new(AdoptionOwner {
            slot,
            directory: stage.directory().to_owned(),
            stage: Mutex::new(Some(stage)),
            next: Mutex::new(Some(AdoptionTask::ReadActive)),
        });
        let mut held = self.adoption.lock().unwrap_or_else(PoisonError::into_inner);
        if held.is_some() {
            return Err(PublicationError::Busy.into());
        }
        let mut s = self.state();
        if s.publication.preview.as_ref() != Some(&(preview.clone(), true)) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            op.binding_before = store
                .publication_binding_for_account(&op.account)?
                .map(|b| BindingBefore {
                    id: b.id,
                    state: b.facts.state,
                });
            store.record_adoption(&op)?;
            Ok(())
        })?;
        *held = Some(owner);
        s.publication.preview = None;
        Self::adapter_close_exchange_locked(&mut s);
        Ok(id)
    }
    /// A distinct durable adoption row drives the existing publication lane.
    pub fn next_adoption_work(&self, collection: &Collection) -> Result<Option<AdoptionWork>, RuntimeError> {
        let Some(op) = self.adoption_op()? else {
            return Ok(None);
        };
        if matches!(
            self.inputs.identity.read_identity(&self.inputs.scope),
            IdentityStatus::ReadFailed
                | IdentityStatus::TemporarilyUnavailable
                | IdentityStatus::Restricted
                | IdentityStatus::Configuration
        ) {
            return Ok(None);
        }
        if self.check_adoption_before(&op, collection).is_err() {
            self.cancel_cloud_adoption(op.id, collection)?;
            return Ok(None);
        }
        let owner = self.adoption_owner()?;
        let next = owner.next.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if next == Some(AdoptionTask::Activate)
            && self
                .review
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .upgrade()
                .and_then(|r| r.active_session())
                .is_some()
        {
            return Ok(None);
        }
        Ok(next.map(|task| AdoptionWork { op: op.id, task }))
    }
    /// Delivery owns no reader lease, Runtime/store guard or lifecycle lock.
    pub fn cloud_adoption_reply(
        &self,
        work: &AdoptionWork,
        reply: AdoptionReply,
        collection: &Collection,
        space: &dyn SpaceProvider,
    ) -> Result<(), RuntimeError> {
        let mut op = self
            .adoption_op()?
            .filter(|p| p.id == work.op)
            .ok_or(RuntimeError::StaleIncarnation)?;
        let owner = self.adoption_owner()?;
        owner.claim(&work.task)?;
        if matches!(
            self.inputs.identity.read_identity(&self.inputs.scope),
            IdentityStatus::ReadFailed
                | IdentityStatus::TemporarilyUnavailable
                | IdentityStatus::Restricted
                | IdentityStatus::Configuration
        ) {
            owner.set_next(work.task.clone());
            return Err(PublicationError::AccountUnavailable.into());
        }
        if self.check_adoption_before(&op, collection).is_err() {
            return self.abort_adoption(&op, collection);
        }
        if matches!(&reply, AdoptionReply::Read(RecordRead::Failed)) {
            owner.set_next(work.task.clone());
            return Err(PublicationError::AccountUnavailable.into());
        }
        let outcome = (|| {
            let next = match (&work.task, reply) {
                (
                    AdoptionTask::ReadActive | AdoptionTask::RecheckActive,
                    AdoptionReply::Read(RecordRead::Found(record)),
                ) => {
                    if record != op.active {
                        return Err(PublicationError::AuthorityChanged.into());
                    }
                    if work.task == AdoptionTask::ReadActive {
                        AdoptionTask::ReadEpoch
                    } else {
                        AdoptionTask::Activate
                    }
                }
                (AdoptionTask::ReadEpoch, AdoptionReply::Read(RecordRead::Found(record))) => {
                    if record.system_fields.0.is_empty() {
                        return Err(refused());
                    }
                    op.epoch_record = Some(record);
                    epoch(&op)?;
                    self.write_adoption(&op)?;
                    AdoptionTask::ReadMediaRoot
                }
                (AdoptionTask::ReadMediaRoot, AdoptionReply::Read(RecordRead::Found(record))) => {
                    if record.system_fields.0.is_empty() {
                        return Err(refused());
                    }
                    op.media_root = Some(record);
                    root(&op)?;
                    self.write_adoption(&op)?;
                    let source = epoch(&op)?;
                    let clock = self.adoption_clock()?;
                    let now = EpochMillis(self.adapter_now()?);
                    let col = Col {
                        version: i64::from(source.ver),
                        created: EpochSecs(source.crt),
                        modified: now,
                        schema_modified: now,
                        ..Col::default()
                    };
                    meguri_storage::bootstrap::create::<RuntimeError>(
                        &owner.directory.join("collection.anki2"),
                        &col,
                        &[],
                        clock.as_ref(),
                        |_| Ok(()),
                    )?;
                    check_space(space, &owner.directory)?;
                    AdoptionTask::Fetch { cursor: None }
                }
                (AdoptionTask::Fetch { .. }, AdoptionReply::Page { records, deleted, next }) => {
                    // A from-start snapshot may page while the server changes;
                    // deletion/control changes invalidate the candidate as a whole.
                    if !deleted.is_empty() {
                        return Err(refused());
                    }
                    let mut payloads = Vec::new();
                    for item in records {
                        if item.record_type == "MGEpoch" && item.name == "epoch" {
                            if Some(&item.record) != op.epoch_record.as_ref() {
                                return Err(refused());
                            }
                            continue;
                        }
                        if item.record_type != "MGEntity" {
                            return Err(refused());
                        }
                        let record = Saved {
                            name: item.name,
                            envelope: item.record.encrypted.ok_or_else(refused)?,
                            system_fields: item.record.system_fields.0,
                        };
                        let incoming = received(&op, &record)?;
                        if let Some(payload) = incoming.payload
                            && payload.kind() != codec::Kind::MediaName
                        {
                            payloads.push(payload);
                        }
                        // receive checked the HMAC name before it becomes a path.
                        let path = owner.directory.join("incoming").join(&record.name);
                        let bytes = serde_json::to_vec(&record).map_err(|_| refused())?;
                        let mut file = AtomicFile::new(&path, Purpose::Stage)?;
                        file.file_mut().write_all(&bytes).map_err(storage_io)?;
                        file.publish(|_| Ok(()))?;
                    }
                    let clock = self.adoption_clock()?;
                    {
                        let stage = owner.stage.lock().unwrap_or_else(PoisonError::into_inner);
                        meguri_storage::verified_replace::write_cloud_stage::<RuntimeError>(
                            stage.as_ref().ok_or_else(refused)?,
                            clock.as_ref(),
                            |scope| {
                                for payload in &payloads {
                                    crate::cloud::row_payload::stage_payload(scope, payload)?;
                                }
                                Ok(())
                            },
                        )?;
                    }
                    check_space(space, &owner.directory)?;
                    if let Some(cursor) = next {
                        AdoptionTask::Fetch { cursor: Some(cursor) }
                    } else {
                        op.step = AdoptStep::Staged;
                        self.write_adoption(&op)?;
                        AdoptionTask::Validate
                    }
                }
                _ => return Err(refused()),
            };
            owner.set_next(next);
            Ok(())
        })();
        if let Err(error) = outcome {
            self.abort_adoption(&op, collection)?;
            return Err(error);
        }
        Ok(())
    }
    fn abort_adoption(&self, op: &AdoptOp, collection: &Collection) -> Result<(), RuntimeError> {
        let file = self.inputs.files.identity(collection.path())?;
        {
            let mut s = self.state();
            with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.end_adoption(op, None, file))?;
        }
        self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.bind(collection)?;
        self.refresh_identity()?;
        Ok(())
    }
    pub fn cancel_cloud_adoption(&self, id: [u8; 16], collection: &Collection) -> Result<(), RuntimeError> {
        let op = self
            .adoption_op()?
            .filter(|p| p.id == id)
            .ok_or(PublicationError::NotOffered)?;
        let owner = self.adoption_owner()?;
        if owner.next.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            return Err(PublicationError::Busy.into());
        }
        self.abort_adoption(&op, collection)
    }
    pub fn perform_cloud_adoption_local(
        &self,
        work: &AdoptionWork,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        space: &dyn SpaceProvider,
        backup: &dyn meguri_engine::ports::BackupService,
    ) -> Result<(), RuntimeError> {
        let mut op = self
            .adoption_op()?
            .filter(|p| p.id == work.op)
            .ok_or(RuntimeError::StaleIncarnation)?;
        let owner = self.adoption_owner()?;
        owner.claim(&work.task)?;
        if self.check_adoption_before(&op, collection).is_err() {
            return self.abort_adoption(&op, collection);
        }
        let result = match work.task {
            AdoptionTask::Validate => self.validate_adoption(&owner, &mut op, collection, media, space),
            AdoptionTask::Activate => self.activate_adoption(&owner, &op, collection, media, backup),
            _ => Err(refused()),
        };
        if matches!(&result, Err(RuntimeError::Engine(EngineError::SessionActive))) {
            owner.set_next(AdoptionTask::Activate);
            return Ok(());
        }
        if let Err(error) = result {
            // Never roll tracking back after a proven/uncertain R6 activation.
            if self.publication_decision().is_ok_and(|d| d == op.before) && collection.is_current() {
                self.abort_adoption(&op, collection)?;
            }
            return Err(error);
        }
        Ok(())
    }
    fn validate_adoption(
        &self,
        owner: &AdoptionOwner,
        op: &mut AdoptOp,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        space: &dyn SpaceProvider,
    ) -> Result<(), RuntimeError> {
        let epoch = epoch(op)?;
        let paths = spool(&owner.directory)?;
        let mut keys = BTreeSet::new();
        let mut has_media = false;
        for path in &paths {
            let record = saved(path)?;
            let received = received(op, &record)?;
            let payload = received.payload.ok_or_else(refused)?;
            if payload.kind() == codec::Kind::MediaName {
                has_media = true;
                continue;
            }
            if !keys.insert((received.entity.fields.kind, received.entity.fields.key)) {
                return Err(refused());
            }
        }
        // Missing-notetype graphs fail here, even when media is also pending.
        for path in &paths {
            let record = saved(path)?;
            let payload = received(op, &record)?.payload.ok_or_else(refused)?;
            for reference in crate::cloud::publication_store::references(&payload)? {
                if !keys.contains(&reference) {
                    return Err(refused());
                }
            }
            match &payload {
                codec::Payload::Note(note) => {
                    let nt = load_notetype(owner, op, note.mid)?;
                    if note.nt_struct_gen != nt.struct_gen {
                        return Err(refused());
                    }
                    // The model's U+001F split includes trailing empty fields.
                    // SQL integrity and projection equality cannot enforce K21.2.
                    let fields = std::str::from_utf8(&note.flds).map_err(|_| refused())?;
                    if nt.fields.is_empty()
                        || fields.split(meguri_model::note::FIELD_SEPARATOR).count() != nt.fields.len()
                    {
                        return Err(refused());
                    }
                }
                codec::Payload::Card(card) => {
                    let name = codec::entity_name(&epoch.record_key, codec::Kind::Note, &codec::id_key(card.nid))
                        .map_err(|_| refused())?;
                    let note = received(op, &saved(&owner.directory.join("incoming").join(name))?)?
                        .payload
                        .ok_or_else(refused)?;
                    let codec::Payload::Note(note) = note else {
                        return Err(refused());
                    };
                    let nt = load_notetype(owner, op, note.mid)?;
                    if card.nt_struct_gen != nt.struct_gen {
                        return Err(refused());
                    }
                }
                _ => {}
            }
        }
        if has_media {
            return Err(PublicationError::MediaApplyUnavailable.into());
        }
        require_empty_media(media)?;
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        let random = self
            .random
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        check_space(space, &owner.directory)?;
        // Gate, integrity, checkpoint and immutable integrity all precede ready.
        {
            let stage = owner.stage.lock().unwrap_or_else(PoisonError::into_inner);
            meguri_storage::verified_replace::prepare(
                stage.as_ref().ok_or_else(refused)?,
                collection.path(),
                &meguri_storage::OpenOptions::new(clock.as_ref(), random.as_ref()),
                clock.as_ref(),
                false,
                |_| Ok(()),
            )
            .map_err(|_| refused())?;
        }
        {
            let reader = meguri_storage::connections::ReaderFactory::recovery(
                &owner.directory.join("collection.anki2"),
                &meguri_storage::conn_init::Initializers::new(),
            )
            .open(ReaderRole::Cloud)?;
            let view = crate::cloud::row_payload::ReadView(&reader);
            for path in &paths {
                let payload = received(op, &saved(path)?)?.payload.ok_or_else(refused)?;
                let table = crate::cloud::row_payload::table(payload.kind()).ok_or_else(refused)?;
                let key = crate::cloud::row_payload::payload_key(&payload)?;
                let row_key = if (payload.kind() as u8) <= 6 {
                    let bytes: [u8; 8] = key.as_slice().try_into().map_err(|_| refused())?;
                    RowKey::Id(i64::from_be_bytes(bytes))
                } else {
                    RowKey::Text(key)
                };
                let row = reader.row_image(table, &row_key)?.ok_or_else(refused)?;
                let projected = crate::cloud::row_payload::project(&view, payload.kind(), &row, |id| {
                    load_notetype(owner, op, id).map_err(|_| Error::InvalidData)
                })?;
                if projected != payload {
                    return Err(refused());
                }
            }
        }
        self.check_adoption_before(op, collection)?;
        let media_key = RecordKey(root(op)?.media_key);
        let acknowledged_op = op.clone();
        let entities = paths.into_iter().map(|path| {
            let record = saved(&path).map_err(|_| Error::InvalidData)?;
            let received = received(&acknowledged_op, &record).map_err(|_| Error::InvalidData)?;
            let payload = received.payload.ok_or(Error::InvalidData)?;
            let row_digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
            let payload_digest = payload.payload_digest().map_err(|_| Error::InvalidData)?;
            Ok(EntityFacts {
                name: record.name,
                kind: payload.kind(),
                key: received.entity.fields.key,
                origin: received.entity.fields.origin,
                local_rev: 1,
                acked_rev: 1,
                payload: payload.clone(),
                base_payload: None,
                mark: None,
                server_erev: received.entity.fields.erev,
                acked_system_fields: record.system_fields,
                acked_row_digest: row_digest,
                acked_payload_digest: payload_digest,
                acked_payload: Some(payload),
                acked_envelope: OpaqueBytes(record.envelope),
                in_flight: Vec::new(),
                observer_open: 0,
                observed_seq: 0,
                observed_generation: 0,
            })
        });
        {
            let mut s = self.state();
            with_tracking_access(|| {
                s.store.as_mut().ok_or(Error::Database)?.ready_adoption(
                    op,
                    RecordKey(epoch.record_key),
                    media_key,
                    epoch.crt,
                    entities,
                )
            })?;
            if let Some(bound) = &mut s.bound {
                bound.usable = false;
            }
        }
        owner.set_next(AdoptionTask::RecheckActive);
        Ok(())
    }
    fn activate_adoption(
        &self,
        owner: &AdoptionOwner,
        op: &AdoptOp,
        _collection: &Collection,
        media: &meguri_engine::media::MediaService,
        backup: &dyn meguri_engine::ports::BackupService,
    ) -> Result<(), RuntimeError> {
        owner.slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        let mut exclusive = owner.slot.enter_exclusive(true)?;
        let working = exclusive.collection().clone();
        owner.slot.check_verified_review(ReplaceReason::CloudAdopt)?;
        self.check_adoption_before(op, exclusive.collection())?;
        require_empty_media(media)?;
        let stage = owner
            .stage
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .ok_or_else(refused)?;
        let mut pin = None;
        let adopted = exclusive
            .replace_verified_cloud_adopt_with_backup(
                stage,
                CloudAdoptInput {
                    operation: publication_id(op.id),
                    candidate: op.candidate.clone(),
                },
                || {
                    // The package provider performs its P10 budget against this
                    // drained Working collection and retains the actual publication.
                    let guarded = backup.before_safety_operation(&working, &meguri_engine::CancelToken::new())?;
                    let receipt = guarded
                        .receipt
                        .clone()
                        .ok_or_else(|| engine_error("adoption safety backup unavailable"))?;
                    pin = Some(guarded);
                    self.check_adoption_before(op, &working).map_err(engine_error)?;
                    Ok(receipt)
                },
            )
            .map_err(|e| match e {
                meguri_engine::replace::ReplaceError::Lifecycle(error) => RuntimeError::Engine(error),
                error => RuntimeError::Engine(engine_error(error)),
            })?;
        let decision = self.publication_decision()?;
        let evidence = meguri_engine::device_state::DeviceState::for_collection(adopted.path())
            .replacement_evidence_for(op.before.publication.identity)?
            .ok_or_else(refused)?;
        if decision.publication != op.candidate
            || evidence.candidate != op.candidate
            || evidence.reason != ReplaceReason::CloudAdopt
            || evidence.safety_backup.is_none()
        {
            return Err(refused());
        }
        let file = self.inputs.files.identity(adopted.path())?;
        let activation = adopted.cloud_adopt_activation()?.ok_or_else(refused)?;
        meguri_model::faultpoint!("cloud.adopt.after_activation").map_err(Error::from)?;
        {
            let mut s = self.state();
            if !with_tracking_access(|| {
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .end_adoption(op, Some(&activation), file)
            })? {
                return Err(refused());
            }
        }
        self.adoption.lock().unwrap_or_else(PoisonError::into_inner).take();
        self.bind(&adopted)?;
        self.refresh_identity()?;
        drop(pin);
        // Root remains unconfirmed. The normal actual reconcile precedes the
        // first no-cursor SDK instance; its root fetch supplies the root proof.
        Ok(())
    }
}
fn load_notetype(owner: &AdoptionOwner, op: &AdoptOp, id: i64) -> Result<codec::Notetype, RuntimeError> {
    let name =
        codec::entity_name(&epoch(op)?.record_key, codec::Kind::Notetype, &codec::id_key(id)).map_err(|_| refused())?;
    let payload = received(op, &saved(&owner.directory.join("incoming").join(name))?)?
        .payload
        .ok_or_else(refused)?;
    match payload {
        codec::Payload::Notetype(nt) => Ok(nt),
        _ => Err(refused()),
    }
}
