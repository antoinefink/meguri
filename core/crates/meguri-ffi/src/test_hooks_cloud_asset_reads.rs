use super::{CloudAssetReadFixtureObservation, CloudAssetReadRemoteFixture};
use crate::{CoreError, runtime, sync::cloud_error};
use meguri_model::ids::NoteId;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_sync::cloud::{adapter::collection_zone, entity_codec as codec, row_payload};

pub(super) fn remote(ordinal: u32) -> Result<CloudAssetReadRemoteFixture, CoreError> {
    if !(1..=2).contains(&ordinal) {
        return Err(cloud_error("unsupported synthetic asset ordinal"));
    }
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("cloud fixture unavailable"))?;
    let binding = core
        .status()
        .map_err(cloud_error)?
        .binding
        .ok_or_else(|| cloud_error("fixture unbound"))?;
    let snapshot = core.adapter_snapshot().map_err(cloud_error)?;
    let mut note = snapshot
        .entities
        .into_iter()
        .find_map(|entity| match entity.payload {
            codec::Payload::Note(note) => Some(note),
            _ => None,
        })
        .ok_or_else(|| cloud_error("fixture requires a captured note"))?;
    note.id = note
        .id
        .checked_add(1_000_000 + i64::from(ordinal))
        .ok_or_else(|| cloud_error("fixture id overflow"))?;
    note.guid = format!("synthetic-asset-executor-{ordinal}").into_bytes();
    note.flds = format!("Synthetic remote {ordinal}\u{1f}{}", "r".repeat(800_000)).into_bytes();
    let note_id = note.id;
    let fields = note.flds.clone();
    let payload = codec::Payload::Note(note);
    let bytes = payload.encode().map_err(cloud_error)?;
    let key = row_payload::payload_key(&payload).map_err(cloud_error)?;
    let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Note, &key).map_err(cloud_error)?;
    let entity = codec::Entity::build(
        codec::EntityFields {
            kind: codec::Kind::Note,
            key,
            origin: [83; 16],
            erev: 1,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer: [84; 16],
            base_sha256: None,
        },
        &bytes,
    )
    .map_err(cloud_error)?;
    if entity.payload.is_some() {
        return Err(cloud_error("fixture must require pa"));
    }
    Ok(CloudAssetReadRemoteFixture {
        record: crate::sync::CloudRecord {
            name,
            envelope: entity.encode().map_err(cloud_error)?,
            system_fields: Some(vec![83, u8::try_from(ordinal).map_err(cloud_error)?]),
            record_type: Some("MGEntity".into()),
            zone: Some(collection_zone(binding.facts.epoch)),
            ..Default::default()
        },
        payload: bytes,
        note_id,
        fields,
    })
}

pub(super) fn observe(name: &str, note_id: i64) -> Result<CloudAssetReadFixtureObservation, CoreError> {
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("cloud fixture unavailable"))?;
    let binding = core
        .status()
        .map_err(cloud_error)?
        .binding
        .ok_or_else(|| cloud_error("fixture unbound"))?;
    let retry_count = core
        .inbox_entries(binding.id)
        .map_err(cloud_error)?
        .into_iter()
        .find(|entry| entry.entry.record_name == name)
        .map(|entry| entry.entry.retry_count);
    let collection = host.collection()?;
    let row_fields = collection
        .reader(ReaderRole::Query)?
        .note(NoteId(note_id))?
        .map(|note| note.fields.into_bytes());
    Ok(CloudAssetReadFixtureObservation {
        retry_count,
        owned_count: u64::try_from(core.asset_read_owned_count_for_testing()).map_err(cloud_error)?,
        row_fields,
        wal_receipts: u64::try_from(core.incoming_wal_receipts().len()).map_err(cloud_error)?,
        changed_by_incoming: core
            .incoming_runs()
            .iter()
            .any(|run| run.changed_keys.iter().any(|key| key == name)),
    })
}
