//! Tracking-only reference and deletion admission. No network or timer authority.
use super::*;
use crate::cloud::{store::decode, tracking_payload};
use rusqlite::{Connection, OptionalExtension, Transaction};
use std::collections::BTreeSet;

pub(crate) const RETENTION_MS: i64 = 30 * 86_400_000;
pub(crate) const FRESH_MS: i64 = 3_600_000;

fn rows<T: serde::de::DeserializeOwned>(
    c: &Connection,
    b: BindingId,
    table: &str,
    column: &str,
) -> Result<Vec<T>, Error> {
    let mut q = c.prepare(&format!("SELECT {column} FROM {table} WHERE binding=?1"))?;
    q.query_map([b.0.as_slice()], |r| r.get::<_, String>(0))?
        .map(|v| decode(&v?))
        .collect()
}

fn local_references(c: &Connection, b: BindingId, ledger: &Ledger) -> Result<BTreeSet<[u8; 32]>, Error> {
    let mut refs = BTreeSet::new();
    for name in ledger.names.values().filter(|n| n.tombstone_at.is_none()) {
        refs.extend(name.local.as_ref().map(|v| v.sha256));
    }
    let mut q = c.prepare("SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2")?;
    let texts = q.query_map(rusqlite::params![b.0.as_slice(), c::Kind::MediaName as i64], |r| {
        r.get::<_, String>(0)
    })?;
    for text in texts {
        let e = tracking_payload::load_entity(c, b, c::Kind::MediaName, &text?)?;
        let case = std::str::from_utf8(&e.key).map_err(|_| Error::InvalidData)?;
        if e.tombstone_at.is_none()
            && ledger.names.get(case).is_none_or(|n| n.tombstone_at.is_none())
            && let c::Payload::MediaName(name) = &e.payload
        {
            refs.insert(name.sha256);
        }
        if e.server_erev > 0 && !e.acked_media_tombstone() {
            let Some(c::Payload::MediaName(name)) = &e.acked_payload else {
                return Err(Error::InvalidData);
            };
            refs.insert(name.sha256);
        }
    }
    Ok(refs)
}

pub(crate) fn update_blob_reference_age(
    tx: &Transaction<'_>,
    b: BindingId,
    ledger: &mut Ledger,
    now: i64,
) -> Result<(), Error> {
    let refs = local_references(tx, b, ledger)?;
    for blob in ledger.blobs.values_mut() {
        if refs.contains(&blob.metadata.sha256) {
            blob.unreferenced_since_ms = None;
            blob.deletion_due_ms = None;
        } else if blob.unreferenced_since_ms.is_none_or(|at| now < at) {
            // Restart the full wait when the observation predates its stored start.
            blob.unreferenced_since_ms = Some(now);
            blob.deletion_due_ms = None;
        }
    }
    Ok(())
}

// Missing decoded content withholds deletion authority for every blob.
fn envelope_reference(key: &[u8; 32], name: &str, bytes: &[u8], refs: &mut BTreeSet<[u8; 32]>) -> bool {
    let Ok(received) = c::receive(key, name, bytes, c::AssetInput::Absent) else {
        return false;
    };
    if !received.entity.fields.tombstone && received.payload.is_none() {
        return false;
    }
    if received.entity.fields.kind == c::Kind::MediaName {
        let Some(c::Payload::MediaName(value)) = received.payload else {
            return false;
        };
        if !received.entity.fields.tombstone {
            refs.insert(value.sha256);
        }
    }
    true
}

pub(crate) fn eligible(
    cn: &Connection,
    b: BindingId,
    ledger: &Ledger,
    now: i64,
    allowed: bool,
) -> Result<BTreeSet<String>, Error> {
    let mut result = BTreeSet::new();
    if !allowed || ledger.zone_missing || ledger.cleanup_drain_ms.is_none() {
        return Ok(result);
    }
    if cn.query_row(
        "SELECT EXISTS(SELECT 1 FROM cloud_publication_zones WHERE binding=?1)",
        [b.0.as_slice()],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(result);
    }
    let facts: BindingFacts = decode(&cn.query_row(
        "SELECT facts FROM bindings WHERE id=?1",
        [b.0.as_slice()],
        |r| r.get::<_, String>(0),
    )?)?;
    if facts.state != BindingState::Active {
        return Ok(result);
    }
    let mut refs = local_references(cn, b, ledger)?;
    for intent in rows::<incoming::ApplyIntent>(cn, b, "cloud_apply_intents", "content")? {
        if intent.media.is_some()
            || intent
                .keys
                .iter()
                .any(|k| k.kind == c::Kind::MediaName && k.role == incoming::ApplyKeyRole::Entry)
        {
            return Ok(result);
        }
    }
    for entry in rows::<InboxEntry>(cn, b, "inbox", "entry")? {
        if let InboxValue::Record { envelope, .. } = entry.value
            && !envelope_reference(&facts.record_key.0, &entry.record_name, &envelope.0, &mut refs)
        {
            return Ok(result);
        }
    }
    for entry in rows::<incoming::RefusedEntry>(cn, b, "cloud_refused", "content")? {
        if !envelope_reference(&facts.record_key.0, &entry.record_name, &entry.envelope.0, &mut refs) {
            return Ok(result);
        }
    }
    // Copies can belong to an isolated binding, a different build or recovery.
    // Read their authority without selecting or modifying those owners.
    let mut copies = cn.prepare(
        "SELECT c.content,b.facts,s.metadata FROM conflicts c
        LEFT JOIN bindings b ON b.id=c.binding LEFT JOIN salvage_bindings s ON s.id=c.binding",
    )?;
    let entries = copies.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for entry in entries {
        let (content, ordinary, salvaged) = entry?;
        let Ok(copy) = decode::<ConflictCopy>(&content) else {
            return Ok(result);
        };
        if copy.resolved_at_ms.is_some() || copy.kind != ConflictKind::MediaName {
            continue;
        }
        let key = match (ordinary, salvaged) {
            (Some(facts), None) => decode::<BindingFacts>(&facts).ok().map(|b| b.record_key),
            (None, Some(metadata)) => decode::<SalvageBinding>(&metadata).ok().and_then(|s| s.record_key),
            _ => None,
        };
        let Some(key) = key else { return Ok(result) };
        let Ok(name) = c::entity_name(&key.0, c::Kind::MediaName, &copy.key.0) else {
            return Ok(result);
        };
        if !envelope_reference(&key.0, &name, &copy.losing_envelope.0, &mut refs) {
            return Ok(result);
        }
    }
    for (name, blob) in &ledger.blobs {
        if blob.server != ServerState::Deleted
            && !refs.contains(&blob.metadata.sha256)
            && !matches!(blob.upload, Transfer::Queued | Transfer::Running | Transfer::Staged)
            && !matches!(blob.download, Transfer::Queued | Transfer::Running | Transfer::Staged)
            && blob.staged.is_none()
            && blob
                .unreferenced_since_ms
                .and_then(|at| now.checked_sub(at))
                .is_some_and(|age| age >= RETENTION_MS)
        {
            result.insert(name.clone());
        }
    }
    Ok(result)
}

pub(crate) fn ledger(cn: &Connection, b: BindingId) -> Result<Ledger, Error> {
    let text: Option<String> = cn
        .query_row(
            "SELECT content FROM cloud_media_state WHERE binding=?1",
            [b.0.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    text.map(|v| decode(&v)).transpose().map(Option::unwrap_or_default)
}

impl TrackingStore {
    pub(crate) fn media_deletions(&self, b: BindingId, now: i64, allowed: bool) -> Result<BTreeSet<String>, Error> {
        let ledger = self.media_ledger(b)?;
        let eligible = eligible(&self.connection, b, &ledger, now, allowed)?;
        Ok(eligible
            .into_iter()
            .filter(|name| {
                ledger.blobs[name]
                    .deletion_due_ms
                    .and_then(|at| now.checked_sub(at))
                    .is_some_and(|age| (0..FRESH_MS).contains(&age))
            })
            .collect())
    }
    pub(crate) fn clear_invalid_media_deletions(&mut self, b: BindingId, now: i64, allowed: bool) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut ledger = ledger(&tx, b)?;
        let eligible = eligible(&tx, b, &ledger, now, allowed)?;
        let mut changed = false;
        for (name, blob) in &mut ledger.blobs {
            if let Some(at) = blob.deletion_due_ms
                && (!eligible.contains(name) || now.checked_sub(at).is_none_or(|age| !(0..FRESH_MS).contains(&age)))
            {
                blob.deletion_due_ms = None;
                changed = true;
            }
        }
        if changed {
            super::store::put_ledger(&tx, b, &ledger)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn media_deleted(&mut self, b: BindingId, name: &str) -> Result<(), Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut ledger = ledger(&tx, b)?;
        let blob = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
        blob.server = ServerState::Deleted;
        blob.upload = Transfer::None;
        blob.deletion_due_ms = None;
        blob.recheck_at = None;
        blob.rechecked = false;
        super::store::put_ledger(&tx, b, &ledger)?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        rng::SeededRandom,
    };

    fn scope(container: &str) -> BuildScope {
        BuildScope {
            container: ContainerId::new(container).unwrap(),
            environment: Environment::Development,
        }
    }
    fn open(path: &std::path::Path, scope: BuildScope) -> TrackingStore {
        match TrackingStore::open(path, scope).unwrap() {
            OpenOutcome::Missing(s) | OpenOutcome::Opened(s) => s,
            _ => panic!("supported storage fixture"),
        }
    }
    fn binding(store: &mut TrackingStore, state: BindingState, seed: u64) -> Binding {
        store
            .create_binding(
                BindingFacts {
                    account: OpaqueBytes(vec![seed as u8]),
                    epoch: EpochId([seed as u8; 16]),
                    record_key: RecordKey([seed as u8; 32]),
                    publication: serde_json::from_str("[1,2]").unwrap(),
                    state,
                },
                &FixedClock::new(0, UtcOffset::UTC),
                &SeededRandom::new(seed),
            )
            .unwrap()
    }
    fn copy(owner: &Binding, metadata: &blob::Blob) -> ConflictCopy {
        let payload = c::Payload::MediaName(metadata.media("copy.bin"));
        let envelope = c::Entity::build(
            c::EntityFields {
                kind: c::Kind::MediaName,
                key: b"copy.bin".to_vec(),
                origin: [4; 16],
                erev: 1,
                generation: None,
                tombstone: false,
                deleted_at_ms: 0,
                writer: owner.id.0,
                base_sha256: None,
            },
            &payload.encode().unwrap(),
        )
        .unwrap()
        .encode()
        .unwrap();
        ConflictCopy {
            id: CopyId([5; 16]),
            kind: ConflictKind::MediaName,
            key: OpaqueBytes(b"copy.bin".to_vec()),
            reason: ConflictReason::MediaContent,
            losing_envelope: OpaqueBytes(envelope),
            losing_payload_digest: PayloadDigest(payload.payload_digest().unwrap()),
            winning_payload_digest: PayloadDigest([6; 32]),
            field_names: None,
            template_names: None,
            note_deck: None,
            staged_path: None,
            created_at_ms: 0,
            resolved_at_ms: None,
        }
    }
    // Typed storage inputs exercise K9.5; resolved_at_ms is not UI resolution proof.
    #[test]
    fn every_copy_owner_blocks_each_eligibility_read() {
        for owner_kind in 0..3 {
            let root = tempfile::tempdir().unwrap();
            let own_scope = scope("test.cleanup.current");
            let foreign_scope = scope("test.cleanup.other");
            let mut store = open(root.path(), foreign_scope.clone());
            let old = binding(&mut store, BindingState::Isolated(IsolationReason::SignedOut), 71);
            let metadata = blob::Blob {
                sha256: [7; 32],
                sha1: [8; 20],
                size: 3,
            };
            let preserved = copy(&old, &metadata);
            store.store_conflict(old.id, &preserved).unwrap();
            if owner_kind == 2 {
                // Use the actual storage recovery path to create a salvage owner.
                store.connection.execute_batch("DROP TABLE inbox").unwrap();
                drop(store);
                store = match TrackingStore::recover(
                    root.path(),
                    own_scope.clone(),
                    &FixedClock::new(1, UtcOffset::UTC),
                    &SeededRandom::new(72),
                )
                .unwrap()
                {
                    OpenOutcome::Opened(s) | OpenOutcome::Missing(s) => s,
                    _ => panic!("recovered storage fixture"),
                };
                let salvaged = store.salvage_bindings().unwrap();
                assert_eq!(salvaged.len(), 1);
                assert_eq!(salvaged[0].record_key, Some(old.facts.record_key.clone()));
                assert!(store.binding(salvaged[0].id).unwrap().is_none());
            } else {
                drop(store);
                store = open(root.path(), if owner_kind == 0 { foreign_scope } else { own_scope });
            }
            store.install_runtime().unwrap();
            let current = binding(&mut store, BindingState::Active, 73);
            let name = metadata.name(&current.facts.record_key.0);
            let mut state = Ledger {
                cleanup_drain_ms: Some(RETENTION_MS),
                ..Default::default()
            };
            state.blobs.insert(
                name.clone(),
                BlobState {
                    metadata,
                    server: ServerState::Present,
                    unreferenced_since_ms: Some(0),
                    deletion_due_ms: Some(RETENTION_MS),
                    ..Default::default()
                },
            );
            store.put_media(current.id, &state).unwrap();
            // There are no live names, ACKs, transfers, intents or retained zones.
            assert!(
                eligible(&store.connection, current.id, &state, RETENTION_MS, true)
                    .unwrap()
                    .is_empty()
            );
            assert!(
                store
                    .media_deletions(current.id, RETENTION_MS, true)
                    .unwrap()
                    .is_empty()
            );
            store
                .clear_invalid_media_deletions(current.id, RETENTION_MS, true)
                .unwrap();
            assert_eq!(
                store.media_ledger(current.id).unwrap().blobs[&name].deletion_due_ms,
                None
            );
            assert!(
                eligible(
                    &store.connection,
                    current.id,
                    &store.media_ledger(current.id).unwrap(),
                    RETENTION_MS,
                    true
                )
                .unwrap()
                .is_empty()
            );
            assert_eq!(store.conflict_copies().unwrap()[0].copy, preserved);
            // Set resolved_at_ms on the stored copy before reading eligibility again.
            let mut resolved = preserved.clone();
            resolved.resolved_at_ms = Some(RETENTION_MS);
            store
                .connection
                .execute(
                    "UPDATE conflicts SET content=?1",
                    [crate::cloud::store::encode(&resolved).unwrap()],
                )
                .unwrap();
            let ledger = store.media_ledger(current.id).unwrap();
            assert_eq!(ledger.blobs[&name].unreferenced_since_ms, Some(0));
            assert_eq!(
                eligible(&store.connection, current.id, &ledger, RETENTION_MS, true).unwrap(),
                BTreeSet::from([name])
            );
            assert_eq!(store.conflict_copies().unwrap()[0].copy, resolved);
        }
    }

    #[test]
    fn unreadable_copy_authority_blocks_instead_of_using_current_owner() {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path(), scope("test.cleanup.current"));
        store.install_runtime().unwrap();
        let current = binding(&mut store, BindingState::Active, 74);
        let metadata = blob::Blob {
            sha256: [7; 32],
            sha1: [8; 20],
            size: 3,
        };
        let name = metadata.name(&current.facts.record_key.0);
        let mut state = Ledger {
            cleanup_drain_ms: Some(RETENTION_MS),
            ..Default::default()
        };
        state.blobs.insert(
            name.clone(),
            BlobState {
                metadata: metadata.clone(),
                unreferenced_since_ms: Some(0),
                ..Default::default()
            },
        );
        assert_eq!(
            eligible(&store.connection, current.id, &state, RETENTION_MS, true).unwrap(),
            BTreeSet::from([name])
        );
        let preserved = copy(&current, &metadata);
        store
            .connection
            .execute(
                "INSERT INTO conflicts VALUES(?1,?2,?3)",
                rusqlite::params![
                    preserved.id.0.as_slice(),
                    [99u8; 16].as_slice(),
                    crate::cloud::store::encode(&preserved).unwrap()
                ],
            )
            .unwrap();
        assert!(
            eligible(&store.connection, current.id, &state, RETENTION_MS, true)
                .unwrap()
                .is_empty()
        );
    }
}
