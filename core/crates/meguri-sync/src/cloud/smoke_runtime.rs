//! Fixed first-smoke observations. No tracking writes or batch construction.
use super::*;
use crate::cloud::{
    held_view::HeldView,
    row_payload::{self, View},
    workflow::*,
};
use meguri_engine::undo::holdback::HeldBackRun;

impl Runtime {
    pub(super) fn smoke_project_image(
        &self,
        guard: &ScanGuard,
        collection: &Collection,
        rows: &(impl ReadRows + ?Sized),
        runs: &[HeldBackRun],
        seq: Seq,
        e: &EntityFacts,
    ) -> Result<SmokeReviewImage, RuntimeError> {
        if !matches!(e.kind, codec::Kind::Card | codec::Kind::Revlog) {
            return Err(Error::InvalidData.into());
        }
        let view = HeldView::new(rows, runs, seq);
        let row = view.row(
            row_payload::table(e.kind).ok_or(Error::InvalidData)?,
            &super::super::workflow_store::row_key(e.kind, &e.key)?,
        )?;
        let digest = row.map(|row| {
            self.project_send_row(guard, collection, &view, e, false, &row)?
                .payload_digest().map_err(|_| Error::InvalidData)
        }).transpose()?;
        let (image_seq, held) = view.image();
        Ok(SmokeReviewImage { name: e.name.clone(), envelope_digest: None, digest, image_seq, held })
    }

    pub(super) fn smoke_built_batch_locked(
        &self,
        s: &State,
        instance: &str,
        id: u64,
        context: (u64, u64, Option<u64>),
        saves: &[SaveInput],
        mut images: Vec<SmokeReviewImage>,
    ) -> Result<SmokeBatchProof, RuntimeError> {
        let (generation, seq, session) = context;
        Self::caller_locked(s, instance)?;
        let binding = &s.bound.as_ref().ok_or(Error::InvalidData)?.binding;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        for image in &mut images {
            let input = saves.iter().find(|v| v.name == image.name).ok_or(Error::InvalidData)?;
            let handed = s.actions.request_flights.get(&(instance.to_owned(), id))
                .and_then(|v| v.get(&image.name)).ok_or(Error::InvalidData)?;
            let header = store.entity_header(binding.id, &image.name)?.ok_or(Error::InvalidData)?;
            let (index, flight) = header.facts.in_flight.iter().enumerate().find(|(_, f)| {
                f.local_rev == handed.local_rev && f.erev == handed.erev
                    && f.payload_digest == handed.payload_digest && f.tombstone == handed.tombstone
            }).ok_or(Error::InvalidData)?;
            let bytes = header.flight_bytes(&store.connection, binding.id, index)?;
            let received = codec::receive(&binding.facts.record_key.0, &input.name,
                &input.envelope, codec::AssetInput::Bytes(&bytes)).map_err(|_| Error::InvalidData)?;
            image.envelope_digest = Some(handed.envelope_digest);
            if image.digest != Some(flight.payload_digest)
                || image.image_seq != flight.image_seq || image.held != flight.held
                || codec::sha256(&input.envelope) != handed.envelope_digest
                || received.entity.payload_sha256 != flight.payload_digest
                || received.entity.fields.writer != binding.id.0
                || received.entity.fields.origin != handed.origin
                || received.entity.fields.erev != handed.erev
                || received.entity.fields.tombstone || received.entity.fields.generation.is_some()
            {
                return Err(Error::InvalidData.into());
            }
        }
        Ok(SmokeBatchProof { generation, seq, session, saves: saves.len() as u64, images })
    }

    pub fn smoke_batch_for_testing(&self, instance: &str, id: u64) -> Result<SmokeBatchProof, RuntimeError> {
        let s = self.state();
        Self::caller_locked(&s, instance)?;
        s.actions.smoke_batches.get(&(instance.to_owned(), id)).cloned().flatten().ok_or(Error::InvalidData.into())
    }

    pub fn smoke_review_for_testing(
        &self, instance: &str, collection: &Collection, card: i64, review: i64,
    ) -> Result<SmokeBatchProof, RuntimeError> {
        let session = self.active_review().ok_or(Error::InvalidData)?;
        let lease = collection.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        let (guard, mut runs, entities) = {
            let s = self.state();
            Self::caller_locked(&s, instance)?;
            let b = s.bound.as_ref().ok_or(Error::InvalidData)?;
            let guard = ScanGuard { invalidation: s.invalidation, binding: b.binding.id,
                open: b.open_counter, generation: b.evidence.generation, file: b.file, incarnation: s.incarnation };
            let entities = with_tracking_access(|| {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                [(codec::Kind::Card, card), (codec::Kind::Revlog, review)].into_iter().map(|(kind, id)| {
                    let name = codec::entity_name(&b.binding.facts.record_key.0, kind, &codec::id_key(id))
                        .map_err(|_| Error::InvalidData)?;
                    Ok(store.entity_header(b.binding.id, &name)?.ok_or(Error::InvalidData)?.facts)
                }).collect::<Result<Vec<_>, Error>>()
            })?;
            (guard, s.cloud.detached.clone(), entities)
        };
        runs.push(collection.cloud_held_run(Some(meguri_engine::SessionId(session)))?);
        let images = entities.iter().map(|e| self.smoke_project_image(
            &guard, collection, &*snapshot, &runs, snapshot.seq(), e,
        )).collect::<Result<Vec<_>, _>>()?;
        self.with_scan(&guard, collection, |_| Ok(()))?;
        if snapshot.generation() != collection.generation() || snapshot.seq() != collection.last_seq()
            || self.active_review() != Some(session) {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(SmokeBatchProof { generation: snapshot.generation().0, seq: snapshot.seq().0,
            session: Some(session), saves: 0, images })
    }
}
