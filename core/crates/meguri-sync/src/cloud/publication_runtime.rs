use super::*;
use crate::cloud::{
    control_codec::{self, DecodeOutcome, Epoch, EpochReason, EpochState, KindCount},
    publication::*,
    row_payload,
    workflow::UpdateKind,
};
use meguri_engine::device_state::{DeviceState, PendingDecisionReason};
use std::collections::BTreeMap;

impl Runtime {
    fn check_absent_preview_file(&self, id: u64) -> Result<(), RuntimeError> {
        let absent = self
            .state()
            .publication
            .preview
            .as_ref()
            .is_some_and(|(preview, _)| preview.id == id && preview.native_file.is_none());
        if absent {
            match self.directory.join("collection.anki2").symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(meguri_storage::StorageError::from(error).into()),
                Ok(_) => return Err(PublicationError::AuthorityChanged.into()),
            }
        }
        Ok(())
    }
    /// V13 discovery never bootstraps a file, sidecar publication or binding.
    /// The Host checks NoCollection and retains its real generation authority.
    pub fn begin_connection_without_collection(&self) -> Result<PreviewWork, RuntimeError> {
        let path = self.directory.join("collection.anki2");
        let absent = || -> Result<(), RuntimeError> {
            match path.symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(meguri_storage::StorageError::from(error).into()),
                Ok(_) => Err(PublicationError::AuthorityChanged.into()),
            }
        };
        absent()?;
        let account = self.publication_account()?;
        let decision_before = DeviceState::for_collection(&path).publication_decision()?;
        absent()?;
        let mut s = self.state();
        if s.bound.is_some() || s.cloud.generation.is_some() {
            return Err(PublicationError::AuthorityChanged.into());
        }
        s.publication.next_preview = s.publication.next_preview.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.publication.next_preview;
        s.publication.preview = Some((
            Preview {
                id,
                local: Counts {
                    counts: BTreeMap::new(),
                    empty: false,
                },
                cloud: None,
                published_at_ms: None,
                decision: ConnectDecision::NoCollection,
                account,
                native_file: None,
                decision_before,
                active: None,
                active_epoch: None,
                unpublished_by_kind: BTreeMap::new(),
            },
            false,
        ));
        Ok(PreviewWork::ReadActive { id })
    }
    fn require_supported_publication(op: &PublishOp) -> Result<(), RuntimeError> {
        if op
            .binding_before
            .as_ref()
            .is_some_and(|b| b.state == BindingState::Isolated(IsolationReason::EncryptedDataReset))
        {
            return Err(PublicationError::ResetRecoveryUnavailable.into());
        }
        Ok(())
    }
    pub fn attach_key_bytes(&self, source: Arc<dyn KeyByteSource>) {
        *self.key_bytes.lock().unwrap_or_else(PoisonError::into_inner) = Some(source);
    }
    pub(super) fn publication_decision(&self) -> Result<DecisionSnapshot, RuntimeError> {
        DeviceState::for_collection(&self.directory.join("collection.anki2"))
            .publication_decision()?
            .ok_or_else(|| PublicationError::AuthorityChanged.into())
    }
    pub(super) fn publication_account(&self) -> Result<OpaqueBytes, RuntimeError> {
        match self.inputs.identity.read_identity(&self.inputs.scope) {
            IdentityStatus::Available(account) if !account.0.is_empty() => Ok(account),
            _ => Err(PublicationError::AccountUnavailable.into()),
        }
    }
    pub(super) fn publication_random_id(&self) -> Result<[u8; 16], RuntimeError> {
        let random = self
            .random
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        let mut id = [0; 16];
        id[..8].copy_from_slice(&random.next_u64().to_be_bytes());
        id[8..].copy_from_slice(&random.next_u64().to_be_bytes());
        Ok(id)
    }
    /// Read actual local rows. Counts do not prepare tracking, acknowledge records,
    /// merge collections, or change the sidecar decision.
    pub fn begin_connection_preview(
        &self,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
    ) -> Result<PreviewWork, RuntimeError> {
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let incarnation = self.state().incarnation;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let mut counts = BTreeMap::new();
        let mut nondefault_deck = false;
        for kind in row_payload::KINDS {
            let mut count = 0;
            if let Some(table) = row_payload::table(kind) {
                let mut after = None;
                loop {
                    let page = snapshot.row_page(table, after.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    for row in page {
                        after = Some(RowKey::from_stored(table, &row).map_err(meguri_storage::StorageError::from)?);
                        if kind == codec::Kind::Deck && row_payload::int(&row, 0)? != 1 {
                            nondefault_deck = true;
                        }
                        count += 1;
                    }
                }
            } else {
                count = u64::try_from(media.tracked_names()?.names.len()).map_err(|_| Error::InvalidData)?;
            }
            counts.insert(kind, count);
        }
        if self.inputs.files.identity(collection.path())? != file
            || self.publication_decision()? != decision
            || self.publication_account()? != account
            || !collection.is_current()
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let empty = [
            codec::Kind::Note,
            codec::Kind::Card,
            codec::Kind::Revlog,
            codec::Kind::MediaName,
        ]
        .iter()
        .all(|k| counts[k] == 0)
            && !nondefault_deck;
        let mut s = self.state();
        if s.incarnation != incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        s.publication.next_preview = s.publication.next_preview.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.publication.next_preview;
        let mut unpublished_by_kind = row_payload::KINDS
            .into_iter()
            .map(|k| (k, 0))
            .collect::<BTreeMap<_, _>>();
        with_tracking_access(|| -> Result<(), Error> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            if let Some(b) = store.publication_binding_for_account(&account)?
                && matches!(
                    b.facts.state,
                    BindingState::Isolated(IsolationReason::ReplacedLocally | IsolationReason::FileMismatch)
                )
            {
                for kind in row_payload::KINDS {
                    let mut after = None;
                    loop {
                        let page = store.entity_page(b.id, kind, after.as_deref())?;
                        if page.is_empty() {
                            break;
                        }
                        for entity in page {
                            after = Some(entity.key.clone());
                            if entity.mark.is_some() {
                                *unpublished_by_kind.get_mut(&kind).ok_or(Error::InvalidData)? += 1;
                            }
                        }
                    }
                }
            }
            Ok(())
        })?;
        s.publication.preview = Some((
            Preview {
                id,
                local: Counts { counts, empty },
                cloud: None,
                published_at_ms: None,
                decision: ConnectDecision::ConfirmUpload,
                account,
                native_file: Some(file),
                decision_before: Some(decision),
                active: None,
                active_epoch: None,
                unpublished_by_kind,
            },
            false,
        ));
        Ok(PreviewWork::ReadActive { id })
    }
    /// Deliver only an actual record-ID read of Active. Missing and unreadable are
    /// distinct; unreadable Active retains its conditional-save system fields.
    pub fn connection_active_read(&self, id: u64, read: RecordRead) -> Result<PreviewWork, RuntimeError> {
        let account = self.publication_account()?;
        self.check_absent_preview_file(id)?;
        let mut s = self.state();
        let (p, ready) = s
            .publication
            .preview
            .as_mut()
            .filter(|(p, _)| p.id == id && p.account == account)
            .ok_or(PublicationError::AuthorityChanged)?;
        if *ready || p.active.is_some() {
            return Err(PublicationError::AuthorityChanged.into());
        }
        match read {
            RecordRead::Missing | RecordRead::ZoneMissing => {
                *ready = true;
                Ok(PreviewWork::Ready(Box::new(p.clone())))
            }
            RecordRead::Found(record) => {
                if record.system_fields.0.is_empty() {
                    return Err(PublicationError::UnreadableControl.into());
                }
                let active = record
                    .encrypted
                    .as_deref()
                    .and_then(|e| control_codec::Active::decode(e).ok());
                p.active = Some(record);
                if let Some(active) = active {
                    let epoch = EpochId(active.epoch);
                    p.active_epoch = Some(epoch);
                    Ok(PreviewWork::ReadEpoch { id, epoch })
                } else {
                    p.decision = ConnectDecision::CloudUnreadable;
                    *ready = true;
                    Ok(PreviewWork::Ready(Box::new(p.clone())))
                }
            }
            RecordRead::Failed => Err(PublicationError::AccountUnavailable.into()),
        }
    }
    pub fn connection_epoch_read(&self, id: u64, read: RecordRead) -> Result<PreviewWork, RuntimeError> {
        let account = self.publication_account()?;
        self.check_absent_preview_file(id)?;
        let mut s = self.state();
        let (p, ready) = s
            .publication
            .preview
            .as_mut()
            .filter(|(p, _)| p.id == id && p.account == account)
            .ok_or(PublicationError::AuthorityChanged)?;
        if *ready {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let RecordRead::Found(record) = read else {
            return Err(PublicationError::EpochIncomplete.into());
        };
        let Some(DecodeOutcome::Supported(epoch)) = record.encrypted.as_deref().and_then(|b| Epoch::decode(b).ok())
        else {
            return Err(PublicationError::UnreadableControl.into());
        };
        if Some(EpochId(epoch.epoch_id)) != p.active_epoch || epoch.state != EpochState::Complete {
            return Err(PublicationError::EpochIncomplete.into());
        }
        let mut counts = row_payload::KINDS
            .into_iter()
            .map(|k| (k, 0))
            .collect::<BTreeMap<_, _>>();
        for count in epoch.counts {
            counts.insert(count.kind, count.live);
        }
        p.cloud = Some(Counts { counts, empty: false });
        p.published_at_ms = Some(epoch.published_at_ms);
        p.decision = if p.native_file.is_none() {
            ConnectDecision::NoCollection
        } else if p.local.empty {
            ConnectDecision::ConfirmAdoption
        } else {
            ConnectDecision::ChooseCollection
        };
        *ready = true;
        Ok(PreviewWork::Ready(Box::new(p.clone())))
    }
    /// Cancellation of an older native sheet cannot dismiss a newer preview.
    pub fn cancel_connection_preview_id(&self, id: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if !s.publication.preview.as_ref().is_some_and(|(p, _)| p.id == id) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        s.publication.preview = None;
        Ok(())
    }
    pub fn cancel_connection_preview(&self) {
        self.state().publication.preview = None;
    }
    pub fn publication_op(&self) -> Result<Option<PublishOp>, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.publication_op()?))
    }
    pub(super) fn check_preview_authority(&self, p: &Preview, collection: &Collection) -> Result<(), RuntimeError> {
        if self.publication_account()? != p.account
            || Some(self.publication_decision()?) != p.decision_before
            || Some(self.inputs.files.identity(collection.path())?) != p.native_file
            || !collection.is_current()
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        Ok(())
    }
    /// Explicit user confirmation. Both full-width keys are obtained before the
    /// first publication commit, outside engine, runtime-state and tracking locks.
    pub fn confirm_connection_publication(
        &self,
        p: &Preview,
        collection: &Collection,
    ) -> Result<[u8; 16], RuntimeError> {
        // Confirmed only records authorization. Bind settles old intents and
        // waits for incoming work before isolating the prior binding (V17.6).
        self.check_preview_authority(p, collection)?;
        let decision_before = p.decision_before.as_ref().ok_or(PublicationError::NotOffered)?;
        let native_file = p.native_file.ok_or(PublicationError::NotOffered)?;
        {
            let s = self.state();
            if s.publication.preview.as_ref() != Some(&(p.clone(), true)) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            if with_tracking_access(|| -> Result<bool, Error> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                Ok(store.publication_op()?.is_some() || store.adoption_op()?.is_some())
            })? {
                return Err(PublicationError::Busy.into());
            }
            // The 37-code reset recovery/Q11 phase has not landed. Refuse the
            // entire reset path before entropy, recovery rows or publication writes.
            let reset = with_tracking_access(|| -> Result<bool, Error> {
                Ok(s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .publication_binding_for_account(&p.account)?
                    .is_some_and(|b| b.facts.state == BindingState::Isolated(IsolationReason::EncryptedDataReset)))
            })?;
            if reset {
                return Err(PublicationError::ResetRecoveryUnavailable.into());
            }
        }
        let source = self
            .key_bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(PublicationError::KeyUnavailable)?;
        let record_key = source.draw_key().map_err(|_| PublicationError::KeyUnavailable)?;
        let media_key = source.draw_key().map_err(|_| PublicationError::KeyUnavailable)?;
        let id = self.publication_random_id()?;
        let epoch = EpochId(self.publication_random_id()?);
        let now = self.adapter_now()?;
        let crt = {
            let lease = collection.reader(ReaderRole::Cloud)?;
            lease.col()?.created.0
        };
        self.check_preview_authority(p, collection)?;
        let mut s = self.state();
        if s.publication.preview.as_ref() != Some(&(p.clone(), true)) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            let before = store.publication_binding_for_account(&p.account)?;
            // Recheck after platform key draws: isolation may have changed while
            // the provider was running outside these locks.
            if before
                .as_ref()
                .is_some_and(|b| b.facts.state == BindingState::Isolated(IsolationReason::EncryptedDataReset))
            {
                return Err(PublicationError::ResetRecoveryUnavailable.into());
            }
            let reason = match decision_before.pending_reason {
                Some(PendingDecisionReason::Restore) => EpochReason::Restore,
                Some(PendingDecisionReason::ColpkgImport) => EpochReason::CollectionPackageImport,
                Some(PendingDecisionReason::ManualImport) => EpochReason::ManualAnkiImport,
                None if p.active.is_some() => EpochReason::ReplaceCloud,
                None => EpochReason::Initial,
            };
            store.confirm_publication(&PublishOp {
                id,
                step: PublishStep::Confirmed,
                binding_before: before.map(|b| BindingBefore {
                    id: b.id,
                    state: b.facts.state,
                }),
                binding_after: None,
                account: p.account.clone(),
                confirmed_publication: decision_before.publication.identity,
                confirmed_file: native_file,
                fence_before: decision_before.clone(),
                epoch,
                record_key: RecordKey(record_key),
                media_key: RecordKey(media_key),
                active: p.active.clone(),
                active_epoch: p.active_epoch,
                epoch_system_fields: None,
                complete_system_fields: None,
                root_acked: false,
                switch_sent: false,
                switch_conflict: None,
                active_attempt: None,
                started_at_ms: now,
                retry_after_ms: None,
                crt,
                reason: reason as u8,
            })?;
            Ok(())
        })?;
        s.publication.preview = None;
        s.publication.waiting = false;
        Ok(id)
    }
    /// Event trigger, never a retry timer. One failed attempt remains waiting
    /// until a new policy/platform event explicitly reaches this entry point.
    pub fn publication_trigger(&self) {
        self.state().publication.waiting = false;
    }
    fn publication_epoch(op: &PublishOp, state: EpochState, counts: Vec<KindCount>) -> Result<Vec<u8>, RuntimeError> {
        Epoch {
            epoch_id: op.epoch.0,
            record_key: op.record_key.0,
            crt: op.crt,
            ver: 18,
            state,
            reason: EpochReason::try_from(u64::from(op.reason)).map_err(|_| Error::InvalidData)?,
            published_at_ms: op.started_at_ms,
            counts,
        }
        .encode()
        .map_err(|_| Error::InvalidData.into())
    }
    fn publication_exempt_locked(s: &State) -> bool {
        let Some(store) = &s.store else {
            return false;
        };
        let Ok(Some(op)) = store.publication_op() else {
            return false;
        };
        Self::require_supported_publication(&op).is_ok()
            && op.binding_after == s.bound.as_ref().map(|b| b.binding.id)
            && matches!(op.step, PublishStep::Zones | PublishStep::Sent | PublishStep::Complete)
    }
    pub(super) fn publication_allows_send_locked(s: &State) -> bool {
        Self::publication_exempt_locked(s)
    }
    pub(super) fn publication_holds_binding_locked(s: &State) -> bool {
        // Caller holds tracking access. No nested gate acquisition here.
        let Some(store) = &s.store else {
            return false;
        };
        if store.adoption_op().map_or(true, |op| op.is_some()) {
            return true;
        }
        let op = match store.publication_op() {
            Ok(Some(op)) => op,
            Ok(None) => return false,
            Err(_) => return true,
        };
        Self::require_supported_publication(&op).is_err()
            || (op.step == PublishStep::Confirmed && s.publication.restarted_confirmed)
            || matches!(op.step, PublishStep::Bound | PublishStep::Cancelling)
    }
    /// One actual local transition or one transport request at a time. Returning
    /// Scan/SendEntities grants existing Runtime/SDK work; it does not model it.
    pub fn next_publication_work(&self, collection: &Collection) -> Result<PublicationWork, RuntimeError> {
        if let Some(work) = self.next_adoption_work(Some(collection))? {
            return Ok(PublicationWork::Adoption(work));
        }
        let Some(op) = self.publication_op()? else {
            return Ok(PublicationWork::Idle);
        };
        Self::require_supported_publication(&op)?;
        if op.step == PublishStep::Cancelling {
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::Cancel,
            });
        }
        let account = match self.publication_account() {
            Ok(a) => a,
            Err(_) => return Ok(PublicationWork::Idle),
        };
        self.refresh_identity()?;
        let decision = self.publication_decision()?;
        let file = self.inputs.files.identity(collection.path())?;
        if op.step == PublishStep::Confirmed {
            let before_matches = {
                let s = self.state();
                with_tracking_access(|| -> Result<bool, Error> {
                    let store = s.store.as_ref().ok_or(Error::Database)?;
                    Ok(match &op.binding_before {
                        Some(before) => store.binding(before.id)?.is_some_and(|b| b.facts.state == before.state),
                        None => true,
                    })
                })?
            };
            if account != op.account
                || decision != op.fence_before
                || decision.publication.identity != op.confirmed_publication
                || file != op.confirmed_file
                || !before_matches
            {
                let mut s = self.state();
                with_tracking_access(|| {
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .discard_confirmed_publication(&op)
                })?;
                return Ok(PublicationWork::Idle);
            }
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::Bind,
            });
        }
        let desired = op.decision_written(&self.inputs.scope)?;
        if op.step == PublishStep::Bound && decision != op.fence_before && decision != desired {
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::Cancel,
            });
        }
        if account != op.account
            || file != op.confirmed_file
            || decision.publication.identity != op.confirmed_publication
            || !collection.is_current()
            || (op.step != PublishStep::Bound && decision != desired)
        {
            return Ok(PublicationWork::Idle);
        }
        if op.step == PublishStep::Bound && decision != desired {
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::WriteDecision,
            });
        }
        let now = self.adapter_now()?;
        let mut s = self.state();
        if s.adapter.paused
            || s.tracking_stopped
            || s.publication.waiting
            || s.publication.running.is_some()
            || op.switch_conflict.is_some()
            || op.retry_after_ms.is_some_and(|t| now < t)
            || !s
                .bound
                .as_ref()
                .is_some_and(|b| Some(b.binding.id) == op.binding_after && b.usable && b.verified)
        {
            return Ok(PublicationWork::Idle);
        }
        if with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.publication_op())?.as_ref() != Some(&op) {
            return Err(RuntimeError::StaleIncarnation);
        }
        if matches!(op.step, PublishStep::Zones | PublishStep::Sent | PublishStep::Complete)
            && (s.scan_required || !s.whole_complete)
        {
            return Ok(PublicationWork::Scan);
        }
        let zone = crate::cloud::adapter::collection_zone(op.epoch);
        let save = |zone: &str, name: &str, record_type: &str, encrypted: Vec<u8>, if_unchanged| OwnOperation::Save {
            zone: zone.to_owned(),
            name: name.to_owned(),
            record_type: record_type.to_owned(),
            encrypted,
            if_unchanged,
        };
        let (kind, operation) = match op.step {
            PublishStep::Bound if !s.publication.zones_created => (
                OwnKind::CreateZones,
                OwnOperation::CreateZones(vec![zone.clone(), "meguri-root".into(), "meguri-media".into()]),
            ),
            PublishStep::Bound if !op.root_acked => (
                OwnKind::SaveMediaRoot,
                save(
                    "meguri-media",
                    "root",
                    "MGMediaRoot",
                    control_codec::MediaRoot {
                        media_key: op.media_key.0,
                        created_at_ms: op.started_at_ms,
                    }
                    .encode()
                    .map_err(|_| Error::InvalidData)?,
                    None,
                ),
            ),
            PublishStep::Bound => (
                OwnKind::SavePublishingEpoch,
                save(
                    &zone,
                    "epoch",
                    "MGEpoch",
                    Self::publication_epoch(&op, EpochState::Publishing, Vec::new())?,
                    None,
                ),
            ),
            PublishStep::Zones => {
                if s.scan_required || !s.whole_complete {
                    return Ok(PublicationWork::Scan);
                }
                return Ok(PublicationWork::SendEntities);
            }
            PublishStep::Sent => {
                let mut counts = Vec::new();
                with_tracking_access(|| -> Result<(), RuntimeError> {
                    let store = s.store.as_ref().ok_or(Error::Database)?;
                    for kind in row_payload::KINDS {
                        let mut after = None;
                        let mut live = 0;
                        loop {
                            let page = store.entity_page(
                                op.binding_after.ok_or(Error::InvalidData)?,
                                kind,
                                after.as_deref(),
                            )?;
                            if page.is_empty() {
                                break;
                            }
                            for e in page {
                                after = Some(e.key);
                                if e.server_erev > 0 {
                                    live += 1;
                                }
                            }
                        }
                        counts.push(KindCount { kind, live });
                    }
                    Ok(())
                })?;
                (
                    OwnKind::SaveCompleteEpoch,
                    save(
                        &zone,
                        "epoch",
                        "MGEpoch",
                        Self::publication_epoch(&op, EpochState::Complete, counts)?,
                        op.epoch_system_fields.clone(),
                    ),
                )
            }
            PublishStep::Complete if !s.publication.own_epoch_read => (
                OwnKind::ReadOwnEpoch,
                OwnOperation::Read {
                    zone,
                    name: "epoch".into(),
                },
            ),
            PublishStep::Complete => {
                let bytes = control_codec::Active {
                    epoch: op.epoch.0,
                    activated_at_ms: now,
                    previous: op.active_epoch.map(|e| e.0),
                    replaced_through_ms: None,
                }
                .encode()
                .map_err(|_| Error::InvalidData)?;
                // Holding this request has not submitted a cloud mutation.
                s.publication.own_epoch_read = false;
                (
                    OwnKind::SaveActive,
                    save(
                        "meguri-root",
                        "active",
                        "MGActive",
                        bytes,
                        op.active.as_ref().map(|r| r.system_fields.clone()),
                    ),
                )
            }
            _ => return Ok(PublicationWork::Idle),
        };
        s.publication.next_attempt = s.publication.next_attempt.checked_add(1).ok_or(Error::InvalidData)?;
        let request = OwnRequest {
            op: op.id,
            attempt: s.publication.next_attempt,
            kind,
            operation,
            incarnation: s.incarnation,
            generation: collection.generation().0,
            instance: Self::instance_locked(&s)?,
            decision_before: op.fence_before.clone(),
            decision_current: desired,
        };
        s.publication.submitted = false;
        s.publication.running = Some(request.clone());
        Ok(PublicationWork::Own(Box::new(request)))
    }
    pub fn perform_publication_local(
        &self,
        id: [u8; 16],
        work: LocalWork,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        let Some(mut op) = self.publication_op()?.filter(|o| o.id == id) else {
            return Err(PublicationError::NotOffered.into());
        };
        Self::require_supported_publication(&op)?;
        if work == LocalWork::Cancel {
            return self.cancel_publication(id, collection);
        }
        let account = self.publication_account()?;
        let decision = self.publication_decision()?;
        let file = self.inputs.files.identity(collection.path())?;
        if account != op.account
            || file != op.confirmed_file
            || decision.publication.identity != op.confirmed_publication
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        match work {
            LocalWork::Bind if op.step == PublishStep::Confirmed && decision == op.fence_before => {
                self.settle_before_transition(collection, Duration::from_secs(30))?;
                let id = BindingId(self.publication_random_id()?);
                let now = self.adapter_now()?;
                {
                    let mut s = self.state();
                    with_tracking_access(|| {
                        s.store
                            .as_mut()
                            .ok_or(Error::Database)?
                            .bind_publication(&mut op, id, now)
                    })?;
                }
                self.bind(collection)?;
                self.refresh_identity()?;
            }
            LocalWork::WriteDecision if op.step == PublishStep::Bound => {
                let desired = op.decision_written(&self.inputs.scope)?;
                if decision != desired && !collection.resolve_cloud_publication_decision(op.id, &op.fence_before)? {
                    return Err(PublicationError::AuthorityChanged.into());
                }
                meguri_model::faultpoint!("cloud.publish.after_fence_clear").map_err(Error::from)?;
                self.bind(collection)?;
                self.refresh_identity()?;
            }
            _ => return Err(PublicationError::NotOffered.into()),
        }
        Ok(())
    }
    /// The executor invokes this at EACH synchronous transport enqueue, after
    /// any suspension. The callback must return acknowledged queue acceptance,
    /// never a deferred operation. Wait for replies after this method returns.
    /// No provider or transport callback runs under Runtime/tracking locks.
    pub fn submit_publication_request<T>(
        &self,
        request: &OwnRequest,
        collection: &Collection,
        submit: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        self.submit_publication_acknowledged(request, collection, submit, |_| true)
    }
    /// Native queues report synchronous admission separately from their later
    /// reply. False proves this operation was never added to the queue.
    pub fn submit_publication_queue(
        &self,
        request: &OwnRequest,
        collection: &Collection,
        submit: impl FnOnce() -> bool,
    ) -> Result<bool, RuntimeError> {
        self.submit_publication_acknowledged(request, collection, submit, |accepted| *accepted)
    }
    fn submit_publication_acknowledged<T>(
        &self,
        request: &OwnRequest,
        collection: &Collection,
        submit: impl FnOnce() -> T,
        accepted: impl FnOnce(&T) -> bool,
    ) -> Result<T, RuntimeError> {
        let _lease = collection.reader(ReaderRole::Cloud)?;
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let mut prior_active = None;
        {
            let mut s = self.state();
            if s.publication.running.as_ref() != Some(request)
                || s.incarnation != request.incarnation
                || Self::instance_locked(&s)? != request.instance
                || collection.generation().0 != request.generation
                || decision != request.decision_current
                || s.adapter.paused
                || s.tracking_stopped
                || (s.publication.submitted && request.kind != OwnKind::CreateZones)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                let mut op = store
                    .publication_op()?
                    .filter(|o| o.id == request.op)
                    .ok_or(RuntimeError::StaleIncarnation)?;
                Self::require_supported_publication(&op)?;
                if op.fence_before != request.decision_before
                    || op.decision_written(&self.inputs.scope)? != decision
                    || op.account != account
                    || op.confirmed_file != file
                    || !s.bound.as_ref().is_some_and(|b| {
                        Some(b.binding.id) == op.binding_after
                            && b.evidence.generation.0 == request.generation
                            && b.usable
                            && b.verified
                    })
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                if request.kind == OwnKind::SaveActive {
                    let OwnOperation::Save { encrypted, .. } = &request.operation else {
                        return Err(Error::InvalidData.into());
                    };
                    prior_active = Some((op.switch_sent, op.active_attempt.clone()));
                    op.switch_sent = true;
                    op.active_attempt = Some(OpaqueBytes(encrypted.clone()));
                    s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op)?;
                }
                Ok(())
            })?;
            s.publication.submitted = true;
        }
        // A submitted mutation whose reply is lost remains durably distinct
        // from a request rejected above, which never calls the transport.
        let result = submit();
        if !accepted(&result) {
            let mut s = self.state();
            // Never overwrite a successor's evidence if lifecycle invalidation
            // raced this acknowledgment. Such an operation remains recoverable.
            if s.publication.running.as_ref() != Some(request) {
                return Err(RuntimeError::StaleIncarnation);
            }
            // Restore only the evidence provisionally changed for this exact
            // enqueue. Earlier accepted attempts must remain uncertain.
            if let Some((switch_sent, active_attempt)) = prior_active {
                with_tracking_access(|| -> Result<(), RuntimeError> {
                    if let Some(store) = s.store.as_mut()
                        && let Some(mut op) = store.publication_op()?.filter(|op| op.id == request.op)
                    {
                        op.switch_sent = switch_sent;
                        op.active_attempt = active_attempt;
                        store.write_publication_op(&op)?;
                    }
                    Ok(())
                })?;
            }
            s.publication.submitted = false;
            s.publication.running = None;
            s.publication.waiting = true;
        }
        Ok(result)
    }
    pub fn publication_result(
        &self,
        request: &OwnRequest,
        result: OwnResult,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        if result == OwnResult::NotStarted {
            let mut s = self.state();
            if s.publication.running.as_ref() != Some(request) || s.publication.submitted {
                return Err(RuntimeError::StaleIncarnation);
            }
            s.publication.running = None;
            s.publication.waiting = true;
            return Ok(());
        }
        // Reject stale replies before consulting the host clock. Recheck after
        // provider reads, since they run outside Runtime/tracking locks.
        {
            let s = self.state();
            if s.publication.running.as_ref() != Some(request)
                || !s.publication.submitted
                || s.incarnation != request.incarnation
                || Self::instance_locked(&s)? != request.instance
                || collection.generation().0 != request.generation
                || !collection.is_current()
            {
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        // Provider/sidecar access occurs before acquiring either writer-shared lock.
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let now = self.adapter_now()?;
        let mut s = self.state();
        if s.publication.running.as_ref() != Some(request)
            || !s.publication.submitted
            || s.incarnation != request.incarnation
            || Self::instance_locked(&s)? != request.instance
            || collection.generation().0 != request.generation
            || !collection.is_current()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        s.publication.running = None;
        let mut op = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.publication_op())?
            .filter(|o| o.id == request.op)
            .ok_or(RuntimeError::StaleIncarnation)?;
        if account != op.account
            || file != op.confirmed_file
            || decision != op.decision_written(&self.inputs.scope)?
            || !s
                .bound
                .as_ref()
                .is_some_and(|b| Some(b.binding.id) == op.binding_after && b.usable && b.verified)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if let OwnResult::Failed { .. } | OwnResult::RetryAfter { .. } = result {
            op.retry_after_ms = match result {
                OwnResult::Failed { retry_after_ms } => retry_after_ms,
                OwnResult::RetryAfter { duration_ms } => {
                    Some(i64::try_from(duration_ms).map_or(i64::MAX, |duration| now.saturating_add(duration)))
                }
                _ => unreachable!(),
            };
            with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op))?;
            s.publication.waiting = true;
            return Ok(());
        }
        let mut accepted = false;
        match (request.kind, result) {
            (OwnKind::CreateZones, OwnResult::ZonesCreated) if op.step == PublishStep::Bound => {
                s.publication.zones_created = true;
                accepted = true;
            }
            (OwnKind::SaveMediaRoot, OwnResult::Saved(record) | OwnResult::Conflict(record))
                if op.step == PublishStep::Bound =>
            {
                if let Some(DecodeOutcome::Supported(root)) = record
                    .encrypted
                    .as_deref()
                    .and_then(|e| control_codec::MediaRoot::decode(e).ok())
                    && !record.system_fields.0.is_empty()
                {
                    op.media_key = RecordKey(root.media_key);
                    op.root_acked = true;
                    accepted = true;
                }
            }
            (
                OwnKind::SavePublishingEpoch | OwnKind::SaveCompleteEpoch,
                OwnResult::Saved(record) | OwnResult::Conflict(record),
            ) => {
                let expected_state = if request.kind == OwnKind::SavePublishingEpoch {
                    EpochState::Publishing
                } else {
                    EpochState::Complete
                };
                if let Some(DecodeOutcome::Supported(epoch)) =
                    record.encrypted.as_deref().and_then(|e| Epoch::decode(e).ok())
                    && epoch.epoch_id == op.epoch.0
                    && epoch.record_key == op.record_key.0
                    && epoch.state == expected_state
                    && epoch.crt == op.crt
                    && !record.system_fields.0.is_empty()
                {
                    if expected_state == EpochState::Publishing && op.step == PublishStep::Bound {
                        op.epoch_system_fields = Some(record.system_fields);
                        if op.root_acked {
                            op.step = PublishStep::Zones;
                        }
                        accepted = true;
                    } else if expected_state == EpochState::Complete && op.step == PublishStep::Sent {
                        op.complete_system_fields = Some(record.system_fields);
                        op.step = PublishStep::Complete;
                        accepted = true;
                    }
                }
            }
            (OwnKind::ReadOwnEpoch, OwnResult::Read(read)) if op.step == PublishStep::Complete => match read {
                RecordRead::Found(record) => {
                    if let Some(DecodeOutcome::Supported(epoch)) =
                        record.encrypted.as_deref().and_then(|e| Epoch::decode(e).ok())
                        && epoch.epoch_id == op.epoch.0
                        && epoch.record_key == op.record_key.0
                        && epoch.state == EpochState::Complete
                    {
                        s.publication.own_epoch_read = true;
                        accepted = true;
                    }
                }
                RecordRead::Missing | RecordRead::ZoneMissing => {
                    let id = op.binding_after.ok_or(Error::InvalidData)?;
                    with_tracking_access(|| {
                        s.store
                            .as_mut()
                            .ok_or(Error::Database)?
                            .isolate_runtime_binding(id, IsolationReason::ZoneMissing)
                    })?;
                    let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
                    b.binding.facts.state = BindingState::Isolated(IsolationReason::ZoneMissing);
                    b.usable = false;
                    b.verified = false;
                }
                _ => {}
            },
            (OwnKind::SaveActive, OwnResult::Saved(record) | OwnResult::Conflict(record))
                if op.step == PublishStep::Complete =>
            {
                if let Some(active) = record
                    .encrypted
                    .as_deref()
                    .and_then(|e| control_codec::Active::decode(e).ok())
                {
                    if active.epoch == op.epoch.0 && !record.system_fields.0.is_empty() {
                        let open = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.open_counter;
                        with_tracking_access(|| {
                            s.store
                                .as_mut()
                                .ok_or(Error::Database)?
                                .publication_done(&op, &record, open, now)
                        })?;
                        s.root_confirmed = true;
                        s.adapter.durable.active = Some(crate::cloud::workflow::Saved {
                            name: "active".into(),
                            envelope: record.encrypted.ok_or(Error::InvalidData)?,
                            system_fields: record.system_fields.0,
                        });
                        s.publication = ProcessState::default();
                        self.emit_pending_locked(&mut s, UpdateKind::Full, Default::default())?;
                        return Ok(());
                    }
                    // A valid competing Active is a durable choice, never a retry.
                    op.switch_conflict = Some(record.system_fields);
                    accepted = true;
                }
            }
            _ => {}
        }
        if accepted {
            op.retry_after_ms = None;
            with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.publication_ack_control(&op))?;
        } else {
            s.publication.waiting = true;
        }
        Ok(())
    }
    pub fn cancel_publication(&self, id: [u8; 16], collection: &Collection) -> Result<(), RuntimeError> {
        let Some(mut op) = self.publication_op()?.filter(|o| o.id == id) else {
            return Err(PublicationError::NotOffered.into());
        };
        {
            let mut s = self.state();
            if s.publication
                .running
                .as_ref()
                .is_some_and(|r| r.kind == OwnKind::SaveActive)
            {
                return Err(PublicationError::Busy.into());
            }
            if op.step == PublishStep::Confirmed {
                with_tracking_access(|| {
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .discard_confirmed_publication(&op)
                })?;
                return Ok(());
            }
            op.step = PublishStep::Cancelling;
            with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op))?;
            s.publication.running = None;
            Self::adapter_close_exchange_locked(&mut s);
            s.root_confirmed = false;
        }
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let desired = op.decision_written(&self.inputs.scope)?;
        if op.fence_before != desired && decision == desired {
            // A concurrent decision wins. Never restore a fence over it.
            if collection.resolve_cloud_publication_decision(op.id, &desired)? {
                meguri_model::faultpoint!("cloud.publish.after_cancel_fence").map_err(Error::from)?;
            }
        }
        let decision = self.publication_decision()?;
        let restored = match op.binding_before.as_ref().map(|b| b.state) {
            Some(BindingState::Active) if decision.publication.identity != op.confirmed_publication => {
                BindingState::Isolated(IsolationReason::ReplacedLocally)
            }
            Some(BindingState::Active) if file != op.confirmed_file => {
                BindingState::Isolated(IsolationReason::FileMismatch)
            }
            Some(state) => state,
            None => BindingState::Isolated(IsolationReason::Republished),
        };
        {
            let mut s = self.state();
            with_tracking_access(|| {
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .cancel_publication(&op, restored)
            })?;
            s.publication = ProcessState::default();
        }
        self.bind(collection)?;
        self.refresh_identity()?;
        Ok(())
    }
}

impl Runtime {
    /// Actual database-zone announcements (not an inventory guessed from Active).
    pub fn publication_zones_seen(&self, instance: &str, zones: &[String]) -> Result<(), RuntimeError> {
        let now = self.adapter_now()?;
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_publication_zones(b, zones, now)
        })?;
        Ok(())
    }
    /// Called from a real collection drain; no timer makes a zone due.
    pub fn publication_collection_drained(&self, instance: &str) -> Result<(), RuntimeError> {
        let now = self.adapter_now()?;
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if !s.root_confirmed || Self::publication_allows_send_locked(&s) || Self::gate_locked(&s).is_err() {
            return Ok(());
        }
        if s.publication
            .last_cleanup_ms
            .is_some_and(|last| now.checked_sub(last).is_none_or(|age| age < 86_400_000))
        {
            return Ok(());
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .mark_publication_zones_due(b, now)
        })?;
        s.publication.last_cleanup_ms = Some(now);
        Ok(())
    }
    pub fn begin_zone_deletion_read(
        &self,
        instance: &str,
        collection: &Collection,
    ) -> Result<ZoneDeletionGuard, RuntimeError> {
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let account = self.publication_account()?;
        let s = self.state();
        Self::caller_locked(&s, instance)?;
        Self::gate_locked(&s)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !s.root_confirmed
            || Self::publication_allows_send_locked(&s)
            || file != b.file
            || account != b.binding.facts.account
        {
            return Err(PublicationError::NotOffered.into());
        }
        Ok(ZoneDeletionGuard {
            instance: instance.into(),
            incarnation: s.incarnation,
            file,
            decision,
            account,
        })
    }
    pub(super) fn validate_zone_deletion_guard(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        if self.inputs.files.identity(collection.path())? != guard.file
            || self.publication_decision()? != guard.decision
            || self.publication_account()? != guard.account
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation || !s.root_confirmed || Self::publication_allows_send_locked(&s) {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(())
    }
    /// A fresh record-ID Active read must still name this binding, immediately
    /// before the platform submits any deletion. Failure/unreadable deletes none.
    pub fn zone_deletion_check(
        &self,
        guard: &ZoneDeletionGuard,
        read: RecordRead,
        collection: &Collection,
    ) -> Result<Vec<String>, RuntimeError> {
        self.validate_zone_deletion_guard(guard, collection)?;
        let RecordRead::Found(record) = read else {
            return Ok(Vec::new());
        };
        let Some(active) = record
            .encrypted
            .as_deref()
            .and_then(|e| control_codec::Active::decode(e).ok())
        else {
            return Ok(Vec::new());
        };
        let s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if active.epoch != b.binding.facts.epoch.0 {
            return Ok(Vec::new());
        }
        let zones = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.retained_zones(b.binding.id))?;
        let active_zone = crate::cloud::adapter::collection_zone(EpochId(active.epoch));
        Ok(zones
            .into_iter()
            .filter_map(|(name, state)| (state.deletion_due && name != active_zone).then_some(name))
            .collect())
    }
    /// `deleted` means a successful deletion or an actual zone-not-found result.
    /// Other results keep the durable due mark for the next drained fetch.
    pub fn zone_deletion_result(
        &self,
        guard: &ZoneDeletionGuard,
        zone: &str,
        deleted: bool,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        self.validate_zone_deletion_guard(guard, collection)?;
        if !deleted {
            return Ok(());
        }
        let mut s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .remove_publication_zone(b, zone)
        })?;
        Ok(())
    }
}

impl Runtime {
    /// Actual durable media-root authority, independent of the in-progress op.
    pub fn publication_media_state(&self) -> Result<Option<(RecordKey, bool, i64)>, RuntimeError> {
        let s = self.state();
        let Some(bound) = &s.bound else {
            return Ok(None);
        };
        with_tracking_access(|| {
            Ok(s.store
                .as_ref()
                .ok_or(Error::Database)?
                .publication_media(bound.binding.id)?)
        })
    }
}

impl meguri_engine::ports::CloudPublicationAuthority for Runtime {
    fn current_account(&self) -> meguri_engine::device_state::AccountBinding {
        use meguri_engine::device_state::AccountBinding;
        // Read the platform directly: neither local ownership nor a recovery
        // row is evidence of the current account. No Runtime/tracking guard.
        #[cfg(feature = "test-hooks")]
        if let Some(hook) = self.take_current_account_hook_for_test() {
            hook();
        }
        match self.inputs.identity.read_identity(&self.inputs.scope) {
            IdentityStatus::Available(account) => match String::from_utf8(account.0) {
                Ok(account) if !account.is_empty() => AccountBinding::Connected {
                    account,
                    configuration: self.inputs.scope.container.as_str().to_owned(),
                    environment: match self.inputs.scope.environment {
                        Environment::Development => "development",
                        Environment::Production => "production",
                    }
                    .into(),
                },
                _ => AccountBinding::Unknown,
            },
            _ => AccountBinding::Unknown,
        }
    }

    fn with_resolution(
        &self,
        collection: &Collection,
        operation: [u8; 16],
        expected: &DecisionSnapshot,
        apply: &mut dyn FnMut(&DecisionSnapshot) -> Result<bool, EngineError>,
    ) -> Result<bool, EngineError> {
        // These providers may reenter platform code. No Runtime, tracking,
        // lifecycle-binding or sidecar mutex is held while they run.
        let account = self.inputs.identity.read_identity(&self.inputs.scope);
        let file = self.inputs.files.identity(collection.path()).map_err(engine_error)?;
        let evidence = collection.open_evidence()?;
        let current = self.publication_decision().map_err(engine_error)?;
        if &current != expected
            || !collection.is_current()
            || evidence.generation != collection.generation()
            || evidence.native_file != file.pair()
            || file.created_nanos >= 1_000_000_000
        {
            return Err(engine_error("stale decision"));
        }
        let s = self.state();
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or_else(|| engine_error("missing store"))?;
            let op = store
                .publication_op()
                .map_err(engine_error)?
                .filter(|o| o.id == operation)
                .ok_or_else(|| engine_error("missing recovery authority"))?;
            let after_id = op
                .binding_after
                .ok_or_else(|| engine_error("missing publication binding"))?;
            let after = store
                .binding(after_id)
                .map_err(engine_error)?
                .ok_or_else(|| engine_error("missing publication binding"))?;
            if op.confirmed_publication != current.publication.identity
                || after.facts.publication != op.confirmed_publication
                || after.facts.account != op.account
                || after.facts.epoch != op.epoch
            {
                return Err(engine_error("stale recovery authority"));
            }
            if op.step == PublishStep::Bound {
                Self::require_supported_publication(&op).map_err(engine_error)?;
                if account != IdentityStatus::Available(op.account.clone())
                    || op.confirmed_file != file
                    || !s.bound.as_ref().is_some_and(|b| {
                        Some(b.binding.id) == op.binding_after
                            && b.evidence.generation == collection.generation()
                            && b.file == file
                    })
                {
                    return Err(engine_error("stale publication authority"));
                }
            }
            let desired = op.decision_written(&self.inputs.scope).map_err(engine_error)?;
            let replacement = match op.step {
                PublishStep::Bound if expected == &op.fence_before => desired,
                // Cancel grants no new publication authority. Current native
                // evidence and the exact scoped row/CAS suffice even after an
                // account event or a file change since confirmation (V15.3).
                PublishStep::Cancelling if expected == &desired => op.fence_before,
                _ => return Err(engine_error("decision authority refused")),
            };
            // Serialize the actual durable row through the sidecar CAS. No
            // writer work or Runtime method is invoked by the engine callback.
            apply(&replacement)
        })
    }
}

impl Runtime {
    #[cfg(feature = "test-hooks")]
    pub fn publication_control_for_testing(&self) -> Result<super::testing::PublicationControl, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let current = s.bound.as_ref().map(|b| b.binding.clone());
            let zones = match &current {
                Some(binding) => store.retained_zones(binding.id)?,
                None => BTreeMap::new(),
            };
            Ok(super::testing::PublicationControl {
                current,
                recovery_ops: store
                    .publication_op()?
                    .into_iter()
                    .map(Into::into)
                    .chain(store.adoption_op()?.into_iter().map(Into::into))
                    .collect(),
                zones,
            })
        })
    }
    pub fn publication_facts(&self, collection: &Collection) -> Result<PublicationFacts, RuntimeError> {
        // Provider reads precede locks; a snapshot is observational, never authority
        // for a later write (the mutating APIs revalidate independently).
        let decision = self.publication_decision()?;
        let native_file = self.inputs.files.identity(collection.path())?;
        let s = self.state();
        with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let current = s.bound.as_ref().map(|b| b.binding.clone());
            let mut entities = BTreeMap::new();
            let mut zones = BTreeMap::new();
            let mut media_key = None;
            let mut root_acked = false;
            if let Some(binding) = &current {
                for kind in row_payload::KINDS {
                    let mut after = None;
                    loop {
                        let page = store.entity_page(binding.id, kind, after.as_deref())?;
                        if page.is_empty() {
                            break;
                        }
                        for entity in page {
                            after = Some(entity.key.clone());
                            let entity = store.entity_dump(binding.id, &entity.name)?.ok_or(Error::InvalidData)?;
                            entities.insert(entity.name.clone(), entity);
                        }
                    }
                }
                zones = store.retained_zones(binding.id)?;
                if let Some((key, acked, _)) = store.publication_media(binding.id)? {
                    media_key = Some(key);
                    root_acked = acked;
                }
            }
            Ok(PublicationFacts {
                current,
                bindings: store.publication_bindings()?,
                recovery_ops: store
                    .publication_op()?
                    .into_iter()
                    .map(Into::into)
                    .chain(store.adoption_op()?.into_iter().map(Into::into))
                    .collect(),
                entities,
                zones,
                decision,
                native_file,
                media_key,
                root_acked,
            })
        })
    }
    /// Submit/enqueue while the deletion guard is current, after any platform hold.
    pub fn submit_zone_deletion<T>(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        submit: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        let _lease = collection.reader(ReaderRole::Cloud)?;
        self.validate_zone_deletion_guard(guard, collection)?;
        Ok(submit())
    }
    pub(super) fn publication_automatic_locked(s: &State) -> bool {
        // The saved user preference is unchanged; only zones temporarily forces
        // automatic scheduling (P6/V15.2). No transport fixture grants permission.
        with_tracking_access(|| {
            s.store
                .as_ref()
                .and_then(|store| store.publication_op().ok().flatten())
                .is_some_and(|op| {
                    op.step == PublishStep::Zones && op.binding_after == s.bound.as_ref().map(|b| b.binding.id)
                })
        })
    }
}
