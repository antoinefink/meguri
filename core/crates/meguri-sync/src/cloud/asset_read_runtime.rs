//! Explicit ordinary-entity asset reads; policy owns every retry pass.
use super::*;
use crate::cloud::{
    adapter::collection_zone,
    incoming::*,
    workflow::{FetchedRecord, Saved},
};
use std::collections::BTreeMap;

pub(crate) struct PendingRead {
    request: AssetReadRequest,
    sdk: String,
    guard: ScanGuard,
    entry: StoredInboxEntry,
    pass: u64,
    command: Option<crate::cloud::policy::Command>,
    phase: ReadPhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadPhase {
    Selected,
    Enqueueing,
    Started,
}

pub(crate) struct ReadPass {
    pass: AssetReadPass,
    guard: ScanGuard,
    command: Option<crate::cloud::policy::Command>,
    after: i64,
    members: BTreeMap<i64, [u8; 32]>,
}

/// Retained only by an actual issued pass. Its full original guard survives
/// finish so terminal activity never resolves a fresh SDK or file owner.
pub struct AssetReadActivityGuard {
    sdk: String,
    guard: ScanGuard,
}

impl AssetReadActivityGuard {
    pub fn sdk_instance(&self) -> &str {
        &self.sdk
    }
}

impl Runtime {
    pub fn asset_read_activity_guard(&self, pass: &AssetReadPass) -> Result<AssetReadActivityGuard, RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, &pass.sdk)?;
        let owner = s
            .incoming
            .asset_passes
            .get(&pass.id)
            .filter(|p| p.pass == *pass)
            .ok_or(RuntimeError::StaleIncarnation)?;
        Ok(AssetReadActivityGuard {
            sdk: pass.sdk.clone(),
            guard: owner.guard.clone(),
        })
    }

    pub fn validate_asset_read_activity(
        &self,
        collection: &Collection,
        owner: &AssetReadActivityGuard,
    ) -> Result<(), RuntimeError> {
        self.with_scan(&owner.guard, collection, |s| Self::sdk_locked(s, &owner.sdk))
    }

    /// Start one event-triggered pass outside SDK handlers. No attempts or first
    /// grants are consumed. A later pass includes entries arriving after this one.
    pub fn begin_asset_read_pass(
        &self,
        sdk: &str,
        collection: &Collection,
        kind: AssetReadKind,
    ) -> Result<AssetReadPass, RuntimeError> {
        self.begin_asset_read_pass_owned(sdk, collection, kind, None)
    }

    pub(crate) fn begin_asset_read_pass_owned(
        &self,
        sdk: &str,
        collection: &Collection,
        kind: AssetReadKind,
        command: Option<crate::cloud::policy::Command>,
    ) -> Result<AssetReadPass, RuntimeError> {
        let guard = {
            let s = self.state();
            Self::sdk_locked(&s, sdk)?;
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            ScanGuard {
                invalidation: s.invalidation,
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file: b.file,
                incarnation: s.incarnation,
            }
        };
        self.with_scan(&guard, collection, |s| {
            Self::sdk_locked(s, sdk)?;
            if s.actions.handler.is_some() || s.tracking_stopped {
                return Err(Error::InvalidData.into());
            }
            // Only unstarted selections can be abandoned on authority change.
            // Enqueueing/accepted operations still own their original result.
            s.incoming
                .asset_reads
                .retain(|_, read| read.phase != ReadPhase::Selected || (read.sdk == sdk && read.guard == guard));
            s.incoming
                .asset_passes
                .retain(|_, pass| pass.pass.sdk == sdk && pass.guard == guard);
            Self::asset_read_accounting_clear(s, &guard)?;
            let members = s
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .asset_read_pass_members(guard.binding)?;
            s.incoming.asset_pass_next = s.incoming.asset_pass_next.checked_add(1).ok_or(Error::InvalidData)?;
            let pass = AssetReadPass {
                id: s.incoming.asset_pass_next,
                sdk: sdk.to_owned(),
                kind,
            };
            s.incoming.asset_passes.insert(
                pass.id,
                ReadPass {
                    pass: pass.clone(),
                    guard: guard.clone(),
                    command,
                    after: 0,
                    members,
                },
            );
            Ok(pass)
        })
    }

    /// Reserve at most one request. Until it is refused, cancelled or completed,
    /// asking the same pass for another request is an error, never an empty drain.
    pub fn next_asset_read(
        &self,
        collection: &Collection,
        pass: &AssetReadPass,
    ) -> Result<Option<AssetReadRequest>, RuntimeError> {
        let guard = self
            .state()
            .incoming
            .asset_passes
            .get(&pass.id)
            .filter(|p| p.pass == *pass)
            .map(|p| p.guard.clone())
            .ok_or(RuntimeError::StaleIncarnation)?;
        self.with_scan(&guard, collection, |s| {
            Self::sdk_locked(s, &pass.sdk)?;
            if s.actions.handler.is_some() || s.tracking_stopped {
                return Err(Error::InvalidData.into());
            }
            Self::asset_read_accounting_clear(s, &guard)?;
            let p = s
                .incoming
                .asset_passes
                .get(&pass.id)
                .filter(|p| p.pass == *pass)
                .ok_or(RuntimeError::StaleIncarnation)?;
            if s.incoming.asset_reads.values().any(|r| r.pass == pass.id) {
                return Err(Error::InvalidData.into());
            }
            let excluded = s
                .incoming
                .asset_reads
                .values()
                .filter(|r| r.sdk == pass.sdk && r.guard == guard)
                .map(|r| r.entry.id)
                .collect();
            let entry = s.store.as_ref().ok_or(Error::Database)?.select_asset_read(
                guard.binding,
                pass.kind,
                p.after,
                &p.members,
                &excluded,
            )?;
            let Some(entry) = entry else {
                let through = p.members.last_key_value().map_or(0, |(id, _)| *id);
                s.incoming
                    .asset_passes
                    .get_mut(&pass.id)
                    .ok_or(Error::InvalidData)?
                    .after = through;
                return Ok(None);
            };
            let command = p.command.clone();
            s.incoming
                .asset_passes
                .get_mut(&pass.id)
                .ok_or(Error::InvalidData)?
                .after = entry.id;
            let zone = collection_zone(
                s.bound
                    .as_ref()
                    .ok_or(RuntimeError::StaleIncarnation)?
                    .binding
                    .facts
                    .epoch,
            );
            s.incoming.asset_read_next = s.incoming.asset_read_next.checked_add(1).ok_or(Error::InvalidData)?;
            let request = AssetReadRequest {
                id: s.incoming.asset_read_next,
                sdk: pass.sdk.clone(),
                name: entry.entry.record_name.clone(),
                zone,
                kind: pass.kind,
            };
            s.incoming.asset_reads.insert(
                request.id,
                PendingRead {
                    request: request.clone(),
                    sdk: pass.sdk.clone(),
                    guard: guard.clone(),
                    entry,
                    pass: pass.id,
                    command,
                    phase: ReadPhase::Selected,
                },
            );
            Ok(Some(request))
        })
    }

    fn asset_read_accounting_clear(s: &State, guard: &ScanGuard) -> Result<(), RuntimeError> {
        if s.incoming
            .asset_read_accounting
            .values()
            .any(|owner| *owner == (guard.binding.0, guard.open))
        {
            return Err(Error::InvalidData.into());
        }
        Ok(())
    }

    #[cfg(feature = "test-hooks")]
    pub fn asset_read_owned_for_testing(&self, request: &AssetReadRequest) -> bool {
        self.state()
            .incoming
            .asset_reads
            .get(&request.id)
            .is_some_and(|read| read.request == *request && read.phase != ReadPhase::Selected)
    }

    #[cfg(feature = "test-hooks")]
    pub fn asset_read_owned_count_for_testing(&self) -> usize {
        self.state()
            .incoming
            .asset_reads
            .values()
            .filter(|read| read.phase != ReadPhase::Selected)
            .count()
    }

    pub(crate) fn asset_pass_command(&self, pass: &AssetReadPass, command: &crate::cloud::policy::Command) -> bool {
        self.state()
            .incoming
            .asset_passes
            .get(&pass.id)
            .is_some_and(|p| p.pass == *pass && p.command.as_ref() == Some(command))
    }
    pub(crate) fn asset_request_command(
        &self,
        request: &AssetReadRequest,
        command: &crate::cloud::policy::Command,
    ) -> bool {
        self.state()
            .incoming
            .asset_reads
            .get(&request.id)
            .is_some_and(|r| r.request == *request && r.command.as_ref() == Some(command))
    }

    /// False leaves an already enqueued read owned until its original result.
    /// Exact identity means an old cancellation cannot remove a newer selection.
    pub fn cancel_asset_read(&self, request: &AssetReadRequest) -> bool {
        let mut s = self.state();
        if s.incoming
            .asset_reads
            .get(&request.id)
            .is_some_and(|r| r.request == *request && r.phase == ReadPhase::Selected)
        {
            s.incoming.asset_reads.remove(&request.id);
            true
        } else {
            false
        }
    }

    pub(crate) fn retire_asset_read(&self, request: &AssetReadRequest) {
        let mut s = self.state();
        if s.incoming
            .asset_reads
            .get(&request.id)
            .is_some_and(|r| r.request == *request)
        {
            s.incoming.asset_reads.remove(&request.id);
        }
    }

    /// Stop selecting. An accepted enqueue still belongs to its original result.
    pub fn cancel_asset_read_pass(&self, pass: &AssetReadPass) {
        let mut s = self.state();
        if s.incoming.asset_passes.get(&pass.id).is_some_and(|p| p.pass == *pass) {
            s.incoming.asset_passes.remove(&pass.id);
            s.incoming
                .asset_reads
                .retain(|_, r| r.pass != pass.id || r.phase != ReadPhase::Selected);
        }
    }

    /// Synchronous acceptance of an already prepared read; never await or reenter
    /// core from enqueue. Wait for the original reply only after this returns.
    /// No runtime/tracking/Driver lock crosses the callback. False consumes no
    /// attempt or grant. A true enqueue remains owned even if accounting fails;
    /// that error is surfaced, never represented as an unstarted cancellation.
    pub fn submit_asset_read_queue(
        &self,
        collection: &Collection,
        request: &AssetReadRequest,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<bool, RuntimeError> {
        let completion = (|| {
            let _lease = collection.reader(ReaderRole::Cloud)?;
            let guard = self
                .state()
                .incoming
                .asset_reads
                .get(&request.id)
                .filter(|r| r.request == *request)
                .map(|r| r.guard.clone())
                .ok_or(RuntimeError::StaleIncarnation)?;
            self.with_scan(&guard, collection, |s| {
                Self::sdk_locked(s, &request.sdk)?;
                if s.actions.handler.is_some() || s.tracking_stopped {
                    return Err(Error::InvalidData.into());
                }
                Self::asset_read_accounting_clear(s, &guard)?;
                let read = s
                    .incoming
                    .asset_reads
                    .get(&request.id)
                    .filter(|r| r.request == *request)
                    .ok_or(RuntimeError::StaleIncarnation)?;
                if read.phase != ReadPhase::Selected
                    || read.guard != guard
                    || !s
                        .store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .asset_read_current(guard.binding, &read.entry)?
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                s.incoming
                    .asset_reads
                    .get_mut(&request.id)
                    .ok_or(Error::InvalidData)?
                    .phase = ReadPhase::Enqueueing;
                Ok(())
            })?;
            let accepted = enqueue();
            {
                let mut s = self.state();
                let Some(read) = s
                    .incoming
                    .asset_reads
                    .get_mut(&request.id)
                    .filter(|r| r.request == *request && r.guard == guard && r.phase == ReadPhase::Enqueueing)
                else {
                    return Err(RuntimeError::StaleIncarnation);
                };
                if !accepted {
                    s.incoming.asset_reads.remove(&request.id);
                    return Ok(false);
                }
                read.phase = ReadPhase::Started;
                // Record the admission obligation before checking storage again.
                // Retain it on error until accounting commits or the collection
                // opens again, including after result handling returns.
                s.incoming
                    .asset_read_accounting
                    .insert(request.id, (guard.binding.0, guard.open));
            }
            // Charge only an actual admitted read, and never mutate a rebound or
            // successor row. The reader lease spans admission, not the reply wait.
            self.with_scan(&guard, collection, |s| {
                Self::sdk_locked(s, &request.sdk)?;
                let read = s
                    .incoming
                    .asset_reads
                    .get(&request.id)
                    .filter(|r| r.request == *request && r.guard == guard && r.phase == ReadPhase::Started)
                    .ok_or(RuntimeError::StaleIncarnation)?;
                let admitted = s.store.as_mut().ok_or(Error::Database)?.admit_asset_read(
                    guard.binding,
                    &read.entry,
                    request.kind,
                )?;
                s.incoming
                    .asset_reads
                    .get_mut(&request.id)
                    .ok_or(Error::InvalidData)?
                    .entry = admitted;
                s.incoming.asset_read_accounting.remove(&request.id);
                Ok(true)
            })
        })();
        if completion.is_err() {
            // Terminal pre-enqueue errors retire only this still-unstarted
            // selection. An actual accepted enqueue must keep its result owner.
            self.cancel_asset_read(request);
        }
        completion
    }

    /// None is a failed record-ID read. Transport retains any supplied file
    /// through this return; intake creates and syncs its own durable copy.
    /// This only settles inbox work. Existing drain_incoming owns writer/WAL/ACK.
    pub fn asset_read_result(
        &self,
        sdk: &str,
        collection: &Collection,
        request: &AssetReadRequest,
        result: Option<&FetchedRecord>,
    ) -> Result<bool, RuntimeError> {
        let selection = {
            let s = self.state();
            s.incoming
                .asset_reads
                .get(&request.id)
                .filter(|read| read.request == *request && read.sdk == sdk && read.phase == ReadPhase::Started)
                .map(|read| (read.guard.clone(), read.entry.clone()))
        };
        let completion = (|| {
            let guard = {
                let s = self.state();
                Self::sdk_locked(&s, sdk)?;
                let Some((guard, _)) = selection.as_ref() else {
                    return Ok(false);
                };
                guard.clone()
            };
            let snapshot = self.with_scan(&guard, collection, |s| {
                Self::sdk_locked(s, sdk)?;
                if s.actions.handler.is_some() || s.tracking_stopped {
                    return Err(Error::InvalidData.into());
                }
                let Some(read) = s.incoming.asset_reads.get(&request.id) else {
                    return Ok(None);
                };
                if read.request != *request
                    || read.sdk != sdk
                    || read.guard != guard
                    || read.phase != ReadPhase::Started
                {
                    return Ok(None);
                }
                let expected = read.entry.clone();
                if !s
                    .store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .asset_read_current(guard.binding, &expected)?
                {
                    s.incoming.asset_reads.remove(&request.id);
                    return Ok(None);
                }
                let key = s
                    .bound
                    .as_ref()
                    .ok_or(RuntimeError::StaleIncarnation)?
                    .binding
                    .facts
                    .record_key
                    .0;
                Ok(Some((expected, key)))
            })?;
            let Some((expected, key)) = snapshot else {
                return Ok(false);
            };
            // Release runtime and tracking access for decoding and file creation.
            // Retire the request after validating and recording the intake result.
            let prepare = || -> Result<_, RuntimeError> {
                let Some(result) = result else { return Ok(None) };
                if result.record.name != request.name {
                    return Ok(None);
                }
                let InboxValue::Record {
                    envelope,
                    erev,
                    system_fields,
                    ..
                } = &expected.entry.value
                else {
                    return Err(Error::InvalidData.into());
                };
                let Ok(codec::DecodeOutcome::Supported(before)) = codec::Entity::decode(&envelope.0) else {
                    return Err(Error::InvalidData.into());
                };
                let Ok(codec::DecodeOutcome::Supported(after)) = codec::Entity::decode(&result.record.envelope) else {
                    return Ok(None);
                };
                if after.fields.erev < *erev
                    || after.fields.generation.is_some()
                    || after.fields.kind == codec::Kind::MediaName
                {
                    return Ok(None);
                }
                let same = after.fields.erev == *erev;
                if same
                    && (after.fields.origin != before.fields.origin
                        || result.record.envelope != envelope.0
                        || result.record.system_fields != system_fields.0)
                {
                    return Ok(None);
                }
                // Higher supported versions use canonical intake and its persisted
                // origin refusal, exactly as ordinary fetched records do.
                let at = self.adapter_now()?;
                let intake = super::super::payload_asset::intake(
                    &self.directory,
                    guard.binding,
                    &key,
                    std::slice::from_ref(result),
                    at,
                )?;
                Ok(Some((intake, same)))
            };
            let prepared = prepare()?;
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::AssetReadPrepared);
            let intake_outcome = self.with_scan(&guard, collection, |s| {
                Self::sdk_locked(s, sdk)?;
                if s.actions.handler.is_some() || s.tracking_stopped {
                    return Err(Error::InvalidData.into());
                }
                let Some(read) = s.incoming.asset_reads.get(&request.id) else {
                    return Ok((false, None));
                };
                if read.request != *request || read.sdk != sdk || read.guard != guard || read.entry != expected {
                    return Ok((false, None));
                }
                if !s
                    .store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .asset_read_current(guard.binding, &expected)?
                {
                    s.incoming.asset_reads.remove(&request.id);
                    return Ok((false, None));
                }
                let Some((intake, same)) = prepared.as_ref() else {
                    s.incoming.asset_reads.remove(&request.id);
                    return Ok((false, None));
                };
                let live = s.store.as_mut().ok_or(Error::Database)?.settle_asset_read(
                    guard.binding,
                    &expected,
                    &intake.entries,
                    &intake.refused,
                    *same,
                )?;
                let settled = !live
                    .iter()
                    .any(|e| e.id == expected.id && e.entry.reason == InboxReason::Asset);
                // Publish only lease metadata while durable references and sweeps
                // are serialized. All owned file finishing stays outside guards.
                intake.publish(&live);
                s.incoming.asset_reads.remove(&request.id);
                Ok((settled, Some(live)))
            });
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::AssetReadFinishing);
            // Finalize prepared files after leaving both guards, including on
            // errors. Keep their lease active throughout retention or removal.
            let finished = prepared.map(|(intake, _)| intake.finish()).transpose();
            let (settled, live) = intake_outcome?;
            finished?;
            // Clean only the selected old file outside both guards.
            if let Some(live) = live {
                super::super::payload_asset::discard_replaced(&self.directory, guard.binding, &expected, &live)?;
            }
            Ok(settled)
        })();
        {
            // Remove the selection identified by this call even if authority
            // validation failed. File finalization precedes this guarded section;
            // preserve any outstanding admission accounting.
            let mut s = self.state();
            if let Some((guard, expected)) = selection
                && s.incoming.asset_reads.get(&request.id).is_some_and(|read| {
                    read.request == *request && read.sdk == sdk && read.guard == guard && read.entry == expected
                })
            {
                s.incoming.asset_reads.remove(&request.id);
            }
        }
        completion
    }

    /// Compatibility entry for callers delivering a single actual conflict.
    pub fn sent_asset_save_conflict(
        &self,
        sdk: &str,
        collection: &Collection,
        request_id: u64,
        server: &Saved,
    ) -> Result<(), RuntimeError> {
        self.sent_results_with_conflicts(sdk, collection, request_id, &[], std::slice::from_ref(server), &[], 1)
            .map(|_| ())
    }

    /// Check the original handler's current handed authority before using an older
    /// matching retained flight. Requires tracking access.
    pub(super) fn decode_save_conflict_locked(
        s: &State,
        sdk: &str,
        request_id: u64,
        server: &Saved,
    ) -> Result<Option<codec::Received>, RuntimeError> {
        let b = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        let handed = s
            .actions
            .request_flights
            .get(&(sdk.to_owned(), request_id))
            .and_then(|names| names.get(&server.name))
            .ok_or(Error::InvalidData)?;
        if server.system_fields.is_empty() {
            return Err(Error::InvalidData.into());
        }
        let header = store.entity_header(b.id, &server.name)?.ok_or(Error::InvalidData)?;
        let local = &header.facts;
        if local.origin != handed.origin
            || !local.in_flight.iter().any(|f| {
                f.local_rev == handed.local_rev
                    && f.erev == handed.erev
                    && f.payload_digest == handed.payload_digest
                    && f.tombstone == handed.tombstone
                    && (!f.tombstone
                        || local.kind == codec::Kind::MediaName
                        || f.tombstone_envelope_digest == Some(handed.envelope_digest))
            })
        {
            return Err(Error::InvalidData.into());
        }
        if let Ok(codec::DecodeOutcome::Supported(entity)) = codec::Entity::decode(&server.envelope) {
            // Structural generations and media retain their separate workers.
            // Identity collisions enter the shared guarded re-key path.
            // K20 refusals still reach ordinary classification below.
            let (_, refused) = classify(&b.facts.record_key.0, std::slice::from_ref(server), 0);
            if refused.is_empty()
                && (entity.fields.generation.is_some()
                    || (entity.fields.kind == codec::Kind::MediaName
                        || (entity.fields.kind == codec::Kind::Notetype
                            && !(entity.fields.origin != local.origin && local.server_erev == 0)))
                    || (entity.fields.tombstone
                        && !matches!(entity.fields.kind, codec::Kind::Note | codec::Kind::Card)))
            {
                return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
            }
            if entity.fields.writer == b.id.0
                && entity.fields.origin == local.origin
                && entity.fields.kind == local.kind
                && entity.fields.key == local.key
                && entity.fields.generation.is_none()
                && entity.fields.erev > local.server_erev
            {
                let flight = if entity.fields.tombstone {
                    local.own_row_tombstone_flight(b.id.0, &entity, &server.envelope)
                } else {
                    local.in_flight.iter().find(|f| {
                        !f.tombstone && f.erev == entity.fields.erev && f.payload_digest == entity.payload_sha256
                    })
                };
                if let Some(flight) = flight {
                    let index = local
                        .in_flight
                        .iter()
                        .position(|f| std::ptr::eq(f, flight))
                        .ok_or(Error::InvalidData)?;
                    let bytes = if flight.tombstone {
                        Vec::new()
                    } else {
                        header.flight_bytes(&store.connection, b.id, index)?
                    };
                    // The real persisted bytes must explain this exact envelope,
                    // including inline/asset choice. Empty tombstones additionally
                    // require the retained handed envelope hash above.
                    let rebuilt = codec::Entity::build(entity.fields.clone(), &bytes)
                        .and_then(|e| e.encode())
                        .map_err(|_| Error::InvalidData)?;
                    if rebuilt != server.envelope {
                        return Err(Error::InvalidData.into());
                    }
                    let received = codec::receive(
                        &b.facts.record_key.0,
                        &server.name,
                        &server.envelope,
                        codec::AssetInput::Bytes(&bytes),
                    )
                    .map_err(|_| Error::InvalidData)?;
                    if !flight.tombstone {
                        let payload = received.payload.as_ref().ok_or(Error::InvalidData)?;
                        if payload.kind() != local.kind
                            || super::super::row_payload::payload_key(payload).map_err(|_| Error::InvalidData)?
                                != local.key
                            || payload.row_digest().map_err(|_| Error::InvalidData)? != flight.row_digest
                        {
                            return Err(Error::InvalidData.into());
                        }
                    }
                    return Ok(Some(received));
                }
            }
        }
        Ok(None)
    }
}
