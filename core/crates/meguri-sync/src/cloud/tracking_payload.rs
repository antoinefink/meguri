//! Private tracking images, not wire records. Headers contain only metadata;
//! canonical payload blobs are scoped to a binding and shared by exact digest.
use super::{
    entity_codec as c,
    runtime::*,
    store::{decode, encode},
    workflow::*,
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Images {
    local: [u8; 32],
    base: Option<[u8; 32]>,
    acked: Option<[u8; 32]>,
    flights: Vec<[u8; 32]>,
    pub local_row_digest: [u8; 32],
}
#[derive(Serialize, Deserialize)]
struct Stored<T> {
    #[serde(flatten)]
    value: T,
    payload_images: Images,
}
#[derive(Deserialize)]
struct ImageMarker {
    #[serde(default)]
    payload_images: Option<Images>,
}
pub(super) struct Header {
    pub facts: EntityFacts,
    pub images: Images,
}
fn empty(kind: c::Kind) -> c::Payload {
    match kind {
        c::Kind::Note => c::Payload::Note(c::Note::default()),
        c::Kind::Card => c::Payload::Card(c::Card::default()),
        c::Kind::Revlog => c::Payload::Revlog(c::Revlog::default()),
        c::Kind::Deck => c::Payload::Deck(c::Deck::default()),
        c::Kind::Preset => c::Payload::Preset(c::Preset::default()),
        c::Kind::Notetype => c::Payload::Notetype(c::Notetype::default()),
        c::Kind::Tag => c::Payload::Tag(c::Tag::default()),
        c::Kind::Config => c::Payload::Config(c::Config::default()),
        c::Kind::MediaName => c::Payload::MediaName(c::MediaName::default()),
    }
}
fn flights(values: &[Flight], _kind: c::Kind, _key: &[u8]) -> Result<Vec<Flight>, Error> {
    values
        .iter()
        .map(|v| {
            Ok(Flight {
                local_rev: v.local_rev,
                payload_digest: v.payload_digest,
                row_digest: v.row_digest,
                payload: match &v.payload {
                    FlightPayload::Image { .. } => v.payload.clone(),
                    FlightPayload::Canonical(p) => FlightPayload::Image {
                        kind: p.kind(),
                        key: super::row_payload::payload_key(p)?,
                        digest: p.payload_digest().map_err(|_| Error::InvalidData)?,
                    },
                },
                erev: v.erev,
                image_seq: v.image_seq,
                held: v.held,
                tombstone: v.tombstone,
                tombstone_envelope_digest: v.tombstone_envelope_digest,
            })
        })
        .collect()
}
fn skeleton(e: &EntityFacts) -> Result<EntityFacts, Error> {
    Ok(EntityFacts {
        name: e.name.clone(),
        kind: e.kind,
        key: e.key.clone(),
        origin: e.origin,
        local_rev: e.local_rev,
        acked_rev: e.acked_rev,
        tombstone_at: e.tombstone_at,
        payload: empty(e.kind),
        base_payload: e.base_payload.as_ref().map(|_| empty(e.kind)),
        mark: e.mark.clone(),
        server_erev: e.server_erev,
        acked_system_fields: e.acked_system_fields.clone(),
        acked_row_digest: e.acked_row_digest,
        acked_payload_digest: e.acked_payload_digest,
        acked_payload: e.acked_payload.as_ref().map(|_| empty(e.kind)),
        acked_envelope: e.acked_envelope.clone(),
        in_flight: flights(&e.in_flight, e.kind, &e.key)?,
        observer_open: e.observer_open,
        observed_seq: e.observed_seq,
        observed_generation: e.observed_generation,
    })
}
fn note_header(n: NoteTracking) -> EntityFacts {
    EntityFacts {
        name: n.name,
        kind: c::Kind::Note,
        key: c::id_key(n.id).to_vec(),
        origin: n.origin,
        local_rev: n.local_rev,
        acked_rev: n.acked_rev,
        tombstone_at: n.tombstone_at,
        payload: empty(c::Kind::Note),
        base_payload: n.base.map(|_| empty(c::Kind::Note)),
        mark: n.mark,
        server_erev: n.acknowledged.erev,
        acked_system_fields: n.acknowledged.system_fields.0,
        acked_row_digest: n.acknowledged.row_digest,
        acked_payload_digest: n.acknowledged.payload_digest,
        acked_payload: (n.acknowledged.erev > 0).then(|| empty(c::Kind::Note)),
        acked_envelope: n.acked_envelope,
        in_flight: n.in_flight,
        observer_open: n.observer_open,
        observed_seq: n.observed_seq,
        observed_generation: n.observed_generation,
    }
}
fn as_note(e: &EntityFacts, row_digest: [u8; 32]) -> Result<NoteTracking, Error> {
    let id = i64::from_be_bytes(e.key.as_slice().try_into().map_err(|_| Error::InvalidData)?);
    Ok(NoteTracking {
        id,
        name: e.name.clone(),
        origin: e.origin,
        local_rev: e.local_rev,
        last_observed_digest: Some(row_digest),
        local_payload: OpaqueBytes(Vec::new()),
        base: e.base_payload.as_ref().map(|_| OpaqueBytes(Vec::new())),
        mark: e.mark.clone(),
        acknowledged: NoteAcknowledgement {
            erev: e.server_erev,
            system_fields: OpaqueBytes(e.acked_system_fields.clone()),
            payload: OpaqueBytes(Vec::new()),
            row_digest: e.acked_row_digest,
            payload_digest: e.acked_payload_digest,
        },
        observer_open: e.observer_open,
        observed_seq: e.observed_seq,
        observed_generation: e.observed_generation,
        acked_rev: e.acked_rev,
        in_flight: flights(&e.in_flight, c::Kind::Note, &e.key)?,
        acked_envelope: e.acked_envelope.clone(),
        tombstone_at: e.tombstone_at,
    })
}
fn owner_v5(cn: &Connection, b: BindingId) -> Result<bool, Error> {
    let markers: bool = cn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_payload_owners')",
        [],
        |r| r.get(0),
    )?;
    if markers {
        return Ok(cn.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_runtime_payload_owners WHERE binding=?1 AND version=5)",
            [b.0.as_slice()],
            |r| r.get(0),
        )?);
    }
    Ok(cn.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get::<_, i64>(0))? == 5)
}
pub(super) fn flight_payload(cn: &Connection, b: BindingId, f: &Flight) -> Result<c::Payload, Error> {
    match &f.payload {
        FlightPayload::Canonical(p) => Ok(p.clone()),
        FlightPayload::Image { kind, key, digest } => {
            let p = payload(cn, b, *kind, *digest)?;
            if super::row_payload::payload_key(&p)? != *key {
                return Err(Error::InvalidData);
            }
            Ok(p)
        }
    }
}
fn save_flight(tx: &Transaction<'_>, b: BindingId, f: &Flight) -> Result<[u8; 32], Error> {
    match &f.payload {
        FlightPayload::Canonical(p) => save_payload(tx, b, p),
        FlightPayload::Image { digest, .. } => {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM cloud_runtime_payloads WHERE binding=?1 AND digest=?2)",
                params![b.0.as_slice(), digest.as_slice()],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error::InvalidData);
            }
            Ok(*digest)
        }
    }
}
pub(super) fn validate_flights(
    cn: &Connection,
    b: BindingId,
    kind: c::Kind,
    key: &[u8],
    values: &[Flight],
) -> Result<(), Error> {
    if values.len() > 250 {
        return Err(Error::InvalidData);
    }
    for f in values {
        super::runtime_store::checked(f.image_seq)?;
        let p = flight_payload(cn, b, f)?;
        if p.kind() != kind
            || super::row_payload::payload_key(&p)? != key
            || ((!f.tombstone || kind == c::Kind::MediaName)
                && (p.row_digest().map_err(|_| Error::InvalidData)? != f.row_digest
                    || p.payload_digest().map_err(|_| Error::InvalidData)? != f.payload_digest))
            || (f.tombstone
                && kind != c::Kind::MediaName
                && (!matches!(kind, c::Kind::Note | c::Kind::Card | c::Kind::Config)
                    || f.row_digest != c::sha256(&[])
                    || f.payload_digest != c::sha256(&[])))
        {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}
pub(super) fn validate_config_flights(b: BindingId, e: &EntityFacts) -> Result<(), Error> {
    if e.kind != c::Kind::Config {
        return Ok(());
    }
    for f in e.in_flight.iter().filter(|f| f.tombstone) {
        if f.local_rev == 0
            || f.local_rev > e.local_rev
            || f.erev <= e.server_erev
            || f.held
            || f.tombstone_envelope_digest.is_none()
        {
            return Err(Error::InvalidData);
        }
        // A later local value retains the handed digest, as for Note/Card.
        // Current deletion fields cannot reconstruct that historical envelope.
        if f.local_rev < e.local_rev {
            continue;
        }
        let envelope = c::Entity::build(
            c::EntityFields {
                kind: e.kind,
                key: e.key.clone(),
                origin: e.origin,
                erev: f.erev,
                generation: None,
                tombstone: true,
                deleted_at_ms: e.tombstone_at.ok_or(Error::InvalidData)?,
                writer: b.0,
                base_sha256: (e.server_erev > 0).then_some(e.acked_payload_digest),
            },
            &[],
        )
        .map_err(|_| Error::InvalidData)?
        .encode()
        .map_err(|_| Error::InvalidData)?;
        if f.tombstone_envelope_digest != Some(c::sha256(&envelope)) {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}
/// Explicit diagnostics only: normal capture/open/preparation keep flight references.
pub(super) fn dump_note(cn: &Connection, b: BindingId, text: &str) -> Result<NoteTracking, Error> {
    let mut n = load_note(cn, b, text)?;
    for f in &mut n.in_flight {
        f.payload = flight_payload(cn, b, f)?.into();
    }
    Ok(n)
}
pub(super) fn dump_entity(cn: &Connection, b: BindingId, kind: c::Kind, text: &str) -> Result<EntityFacts, Error> {
    let mut e = load_entity(cn, b, kind, text)?;
    for f in &mut e.in_flight {
        f.payload = flight_payload(cn, b, f)?.into();
    }
    Ok(e)
}
pub(super) fn blob(cn: &Connection, b: BindingId, digest: [u8; 32]) -> Result<Vec<u8>, Error> {
    let bytes: Vec<u8> = cn.query_row(
        "SELECT bytes FROM cloud_runtime_payloads WHERE binding=?1 AND digest=?2",
        params![b.0.as_slice(), digest.as_slice()],
        |r| r.get(0),
    )?;
    if c::sha256(&bytes) != digest || bytes.len() as u64 > c::MAX_PAYLOAD_BYTES {
        return Err(Error::InvalidData);
    }
    Ok(bytes)
}
fn save_bytes(tx: &Transaction<'_>, b: BindingId, bytes: &[u8]) -> Result<[u8; 32], Error> {
    c::validate_payload_size(bytes.len() as u64).map_err(|_| Error::InvalidData)?;
    let digest = c::sha256(bytes);
    tx.execute("INSERT INTO cloud_runtime_payloads(binding,digest,bytes) VALUES(?1,?2,?3) ON CONFLICT(binding,digest) DO NOTHING",
        params![b.0.as_slice(), digest.as_slice(), bytes])?;
    Ok(digest)
}
fn save_payload(tx: &Transaction<'_>, b: BindingId, p: &c::Payload) -> Result<[u8; 32], Error> {
    save_bytes(tx, b, &p.encode().map_err(|_| Error::InvalidData)?)
}
pub(super) fn put_header(tx: &Transaction<'_>, b: BindingId, h: &Header) -> Result<(), Error> {
    let mut metadata = skeleton(&h.facts)?;
    for (flight, digest) in metadata.in_flight.iter_mut().zip(&h.images.flights) {
        flight.payload = FlightPayload::Image {
            kind: metadata.kind,
            key: metadata.key.clone(),
            digest: *digest,
        };
    }
    let e = &metadata;
    let old_text: Option<String> = if e.kind == c::Kind::Note {
        let id = i64::from_be_bytes(e.key.as_slice().try_into().map_err(|_| Error::InvalidData)?);
        tx.query_row(
            "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
            params![b.0.as_slice(), id],
            |r| r.get(0),
        )
        .optional()?
    } else {
        tx.query_row(
            "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
            params![b.0.as_slice(), e.kind as i64, &e.key],
            |r| r.get(0),
        )
        .optional()?
    };
    // Ignore legacy byte arrays while examining the representation marker;
    // do not deserialize a second full legacy record during conversion.
    let old_images = old_text
        .as_deref()
        .map(decode::<ImageMarker>)
        .transpose()?
        .and_then(|marker| marker.payload_images);
    if old_text.is_some() && old_images.is_none() && owner_v5(tx, b)? {
        return Err(Error::InvalidData);
    }
    if e.in_flight.len() != h.images.flights.len() {
        return Err(Error::InvalidData);
    }
    if e.kind == c::Kind::Note {
        let n = as_note(e, h.images.local_row_digest)?;
        let id = n.id;
        tx.execute("INSERT INTO cloud_runtime_notes(binding,id,content) VALUES(?1,?2,?3) ON CONFLICT(binding,id) DO UPDATE SET content=excluded.content",
            params![b.0.as_slice(), id, encode(&Stored { value: n, payload_images: h.images.clone() })?])?;
    } else {
        tx.execute("INSERT INTO cloud_runtime_entities(binding,kind,key,content) VALUES(?1,?2,?3,?4) ON CONFLICT(binding,kind,key) DO UPDATE SET content=excluded.content",
            params![b.0.as_slice(), e.kind as i64, &e.key, encode(&Stored { value: &e, payload_images: h.images.clone() })?])?;
    }
    if let Some(old) = old_images {
        collect(
            tx,
            b,
            &e.name,
            old.all().into_iter().filter(|d| !h.images.all().contains(d)),
        )?;
    }
    Ok(())
}
/// Collect only references retired by this write. Other headers retain their
/// current/base/ACK/flights. Apply intents own inline before/after images; an
/// unsettled own-flight intent additionally pins the removed header's blobs.
fn collect(
    tx: &Transaction<'_>,
    b: BindingId,
    name: &str,
    candidates: impl IntoIterator<Item = [u8; 32]>,
) -> Result<(), Error> {
    let has_intents: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_apply_intents')",
        [],
        |r| r.get(0),
    )?;
    if has_intents {
        let pinned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cloud_apply_intents i,json_each(i.content,'$.keys') k WHERE i.binding=?1 AND json_extract(k.value,'$.name')=?2 AND json_extract(k.value,'$.own_flight') IS NOT NULL)", params![b.0.as_slice(), name], |r| r.get(0))?;
        if pinned {
            for digest in candidates {
                tx.execute(
                    "INSERT OR IGNORE INTO cloud_runtime_payload_retired VALUES(?1,?2,?3)",
                    params![b.0.as_slice(), name, digest.as_slice()],
                )?;
            }
            return Ok(());
        }
    }
    let entities: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_entities')",
        [],
        |r| r.get(0),
    )?;
    let used = "(json_extract(content,'$.payload_images.local')=?2 OR json_extract(content,'$.payload_images.base')=?2 OR json_extract(content,'$.payload_images.acked')=?2 OR EXISTS(SELECT 1 FROM json_each(content,'$.payload_images.flights') WHERE value=?2))";
    let extra = if entities {
        format!(" AND NOT EXISTS(SELECT 1 FROM cloud_runtime_entities WHERE binding=?1 AND {used})")
    } else {
        String::new()
    };
    for digest in candidates {
        tx.execute(&format!("DELETE FROM cloud_runtime_payloads WHERE binding=?1 AND digest=?3 AND NOT EXISTS(SELECT 1 FROM cloud_runtime_notes WHERE binding=?1 AND {used}){extra}"), params![b.0.as_slice(), encode(&digest)?, digest.as_slice()])?;
        tx.execute(
            "DELETE FROM cloud_runtime_payload_retired WHERE binding=?1 AND name=?2 AND digest=?3",
            params![b.0.as_slice(), name, digest.as_slice()],
        )?;
    }
    Ok(())
}
pub(super) fn collect_retired(tx: &Transaction<'_>, b: BindingId) -> Result<(), Error> {
    let mut q = tx.prepare("SELECT name,digest FROM cloud_runtime_payload_retired WHERE binding=?1")?;
    let candidates = q
        .query_map([b.0.as_slice()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(q);
    for (name, digest) in candidates {
        collect(tx, b, &name, [digest.try_into().map_err(|_| Error::InvalidData)?])?;
    }
    Ok(())
}
pub(super) fn removed_images(
    tx: &Transaction<'_>,
    b: BindingId,
    kind: c::Kind,
    key: &[u8],
) -> Result<Option<Images>, Error> {
    let text: Option<String> = if kind == c::Kind::Note {
        let id = i64::from_be_bytes(key.try_into().map_err(|_| Error::InvalidData)?);
        tx.query_row(
            "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
            params![b.0.as_slice(), id],
            |r| r.get(0),
        )
        .optional()?
    } else {
        tx.query_row(
            "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
            params![b.0.as_slice(), kind as i64, key],
            |r| r.get(0),
        )
        .optional()?
    };
    text.map(|text| decode::<ImageMarker>(&text))
        .transpose()
        .map(|v| v.and_then(|v| v.payload_images))
}
pub(super) fn collect_removed(
    tx: &Transaction<'_>,
    b: BindingId,
    name: &str,
    images: Option<Images>,
) -> Result<(), Error> {
    if let Some(images) = images {
        collect(tx, b, name, images.all())?;
    }
    Ok(())
}
impl Images {
    fn all(&self) -> Vec<[u8; 32]> {
        std::iter::once(self.local)
            .chain(self.base)
            .chain(self.acked)
            .chain(self.flights.iter().copied())
            .collect()
    }
}
pub(super) fn put_entity(tx: &Transaction<'_>, b: BindingId, e: &EntityFacts) -> Result<(), Error> {
    let images = Images {
        local: save_payload(tx, b, &e.payload)?,
        base: e.base_payload.as_ref().map(|p| save_payload(tx, b, p)).transpose()?,
        acked: e.acked_payload.as_ref().map(|p| save_payload(tx, b, p)).transpose()?,
        flights: e
            .in_flight
            .iter()
            .map(|f| save_flight(tx, b, f))
            .collect::<Result<_, _>>()?,
        local_row_digest: e.payload.row_digest().map_err(|_| Error::InvalidData)?,
    };
    put_header(
        tx,
        b,
        &Header {
            facts: skeleton(e)?,
            images,
        },
    )
}
pub(super) fn put_note(tx: &Transaction<'_>, b: BindingId, n: &NoteTracking) -> Result<(), Error> {
    let images = Images {
        local: save_bytes(tx, b, &n.local_payload.0)?,
        base: n.base.as_ref().map(|p| save_bytes(tx, b, &p.0)).transpose()?,
        acked: (n.acknowledged.erev > 0)
            .then(|| save_bytes(tx, b, &n.acknowledged.payload.0))
            .transpose()?,
        flights: n
            .in_flight
            .iter()
            .map(|f| save_flight(tx, b, f))
            .collect::<Result<_, _>>()?,
        local_row_digest: n.last_observed_digest.ok_or(Error::InvalidData)?,
    };
    // Build metadata without cloning any canonical image or flight payload.
    let mut h = note_header(NoteTracking {
        id: n.id,
        name: n.name.clone(),
        origin: n.origin,
        local_rev: n.local_rev,
        last_observed_digest: n.last_observed_digest,
        local_payload: OpaqueBytes(Vec::new()),
        base: n.base.as_ref().map(|_| OpaqueBytes(Vec::new())),
        mark: n.mark.clone(),
        acknowledged: NoteAcknowledgement {
            erev: n.acknowledged.erev,
            system_fields: n.acknowledged.system_fields.clone(),
            payload: OpaqueBytes(Vec::new()),
            row_digest: n.acknowledged.row_digest,
            payload_digest: n.acknowledged.payload_digest,
        },
        observer_open: n.observer_open,
        observed_seq: n.observed_seq,
        observed_generation: n.observed_generation,
        acked_rev: n.acked_rev,
        in_flight: flights(&n.in_flight, c::Kind::Note, &c::id_key(n.id))?,
        acked_envelope: n.acked_envelope.clone(),
        tombstone_at: n.tombstone_at,
    });
    h.payload = empty(c::Kind::Note);
    put_header(tx, b, &Header { facts: h, images })
}
fn reference_flights(values: &mut [Flight], images: &[[u8; 32]], kind: c::Kind, key: &[u8]) -> Result<(), Error> {
    if values.len() != images.len() {
        return Err(Error::InvalidData);
    }
    for (f, digest) in values.iter_mut().zip(images) {
        if let FlightPayload::Image {
            kind: stored_kind,
            key: stored_key,
            digest: stored_digest,
        } = &f.payload
            && (*stored_kind != kind || stored_key != key || stored_digest != digest)
        {
            return Err(Error::InvalidData);
        }
        // Canonical here includes the original V5 seed's inline skeleton.
        f.payload = FlightPayload::Image {
            kind,
            key: key.to_vec(),
            digest: *digest,
        };
    }
    Ok(())
}
pub(super) fn header(kind: c::Kind, text: &str) -> Result<Header, Error> {
    if kind == c::Kind::Note {
        let mut v: Stored<NoteTracking> = decode(text)?;
        reference_flights(
            &mut v.value.in_flight,
            &v.payload_images.flights,
            kind,
            &c::id_key(v.value.id),
        )?;
        Ok(Header {
            facts: note_header(v.value),
            images: v.payload_images,
        })
    } else {
        let mut v: Stored<EntityFacts> = decode(text)?;
        reference_flights(&mut v.value.in_flight, &v.payload_images.flights, kind, &v.value.key)?;
        Ok(Header {
            facts: v.value,
            images: v.payload_images,
        })
    }
}
pub(super) fn payload(cn: &Connection, b: BindingId, kind: c::Kind, digest: [u8; 32]) -> Result<c::Payload, Error> {
    c::Payload::decode(kind, &blob(cn, b, digest)?).map_err(|_| Error::InvalidData)
}
pub(super) fn load_note(cn: &Connection, b: BindingId, text: &str) -> Result<NoteTracking, Error> {
    if decode::<ImageMarker>(text)?.payload_images.is_none() {
        if owner_v5(cn, b)? {
            return Err(Error::InvalidData);
        }
        return decode(text);
    }
    let v: Stored<NoteTracking> = decode(text)?;
    let mut n = v.value;
    n.local_payload = OpaqueBytes(blob(cn, b, v.payload_images.local)?);
    n.base = v
        .payload_images
        .base
        .map(|d| blob(cn, b, d).map(OpaqueBytes))
        .transpose()?;
    n.acknowledged.payload = OpaqueBytes(
        v.payload_images
            .acked
            .map(|d| blob(cn, b, d))
            .transpose()?
            .unwrap_or_default(),
    );
    reference_flights(
        &mut n.in_flight,
        &v.payload_images.flights,
        c::Kind::Note,
        &c::id_key(n.id),
    )?;
    Ok(n)
}
pub(super) fn load_entity(cn: &Connection, b: BindingId, kind: c::Kind, text: &str) -> Result<EntityFacts, Error> {
    if kind == c::Kind::Note {
        return super::workflow_store::note_facts(load_note(cn, b, text)?);
    }
    #[cfg(feature = "test-hooks")]
    super::workflow_store::body_read();
    if decode::<ImageMarker>(text)?.payload_images.is_none() {
        if owner_v5(cn, b)? {
            return Err(Error::InvalidData);
        }
        return decode(text);
    }
    let mut h = header(kind, text)?;
    h.facts.payload = payload(cn, b, kind, h.images.local)?;
    h.facts.base_payload = h.images.base.map(|d| payload(cn, b, kind, d)).transpose()?;
    h.facts.acked_payload = h.images.acked.map(|d| payload(cn, b, kind, d)).transpose()?;
    Ok(h.facts)
}
impl Header {
    pub(super) fn rekey_reference(
        &mut self,
        tx: &Transaction<'_>,
        b: BindingId,
        payload: c::Payload,
        counter: &mut u64,
        observed: (u64, u64, u64),
    ) -> Result<(), Error> {
        if payload.kind() != self.facts.kind
            || super::row_payload::payload_key(&payload)? != self.facts.key
            || self.facts.acked_tombstone()
        {
            return Err(Error::InvalidData);
        }
        let row_digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
        let digest = save_payload(tx, b, &payload)?;
        if self.facts.mark.is_none() {
            self.images.base = self.images.acked;
            self.facts.base_payload = self.images.base.map(|_| empty(self.facts.kind));
        }
        self.images.local = digest;
        self.images.local_row_digest = row_digest;
        self.facts.tombstone_at = None;
        self.rekey_mark(digest, counter, observed)
    }
    pub(super) fn rekey_config_removed(
        &mut self,
        at: i64,
        counter: &mut u64,
        observed: (u64, u64, u64),
    ) -> Result<(), Error> {
        if self.facts.kind != c::Kind::Config || self.facts.acked_tombstone() {
            return Err(Error::InvalidData);
        }
        if self.facts.tombstone_at.is_some() {
            return Ok(());
        }
        if self.facts.mark.is_none() {
            self.images.base = Some(self.images.local);
            self.facts.base_payload = Some(empty(self.facts.kind));
        }
        self.facts.tombstone_at = Some(at);
        self.rekey_mark(self.images.local, counter, observed)
    }
    fn rekey_mark(&mut self, digest: [u8; 32], counter: &mut u64, observed: (u64, u64, u64)) -> Result<(), Error> {
        self.facts.local_rev = super::runtime_store::next(self.facts.local_rev)?;
        *counter = super::runtime_store::next(*counter)?;
        self.facts.mark = Some(DirtyMark {
            source: DirtySource::Observer,
            change_seq: *counter,
            payload_digest: digest,
        });
        self.facts.observer_open = observed.0;
        self.facts.observed_seq = observed.1;
        self.facts.observed_generation = observed.2;
        Ok(())
    }
    pub(super) fn clear_rekey_base(&mut self) {
        self.facts.mark = None;
        self.facts.base_payload = None;
        self.images.base = None;
    }
    pub fn local_digest(&self) -> [u8; 32] {
        self.images.local
    }
    pub fn local_payload(&self, cn: &Connection, b: BindingId) -> Result<c::Payload, Error> {
        payload(cn, b, self.facts.kind, self.images.local)
    }
    pub fn flight_bytes(&self, cn: &Connection, b: BindingId, index: usize) -> Result<Vec<u8>, Error> {
        blob(cn, b, *self.images.flights.get(index).ok_or(Error::InvalidData)?)
    }
    pub fn append_flight(&mut self, tx: &Transaction<'_>, b: BindingId, f: &Flight) -> Result<(), Error> {
        self.images.flights.push(save_flight(tx, b, f)?);
        self.facts
            .in_flight
            .push(flights(std::slice::from_ref(f), self.facts.kind, &self.facts.key)?.remove(0));
        Ok(())
    }
    pub fn ack(&mut self, index: usize, saved: &Saved) -> Result<bool, Error> {
        let f = self.facts.in_flight.get(index).ok_or(Error::InvalidData)?;
        let (erev, local_rev, row_digest, payload_digest) = (f.erev, f.local_rev, f.row_digest, f.payload_digest);
        let digest = *self.images.flights.get(index).ok_or(Error::InvalidData)?;
        let e = &mut self.facts;
        e.server_erev = erev;
        e.acked_rev = local_rev;
        e.acked_row_digest = row_digest;
        e.acked_payload_digest = payload_digest;
        e.acked_system_fields = saved.system_fields.clone();
        e.acked_envelope = OpaqueBytes(saved.envelope.clone());
        e.acked_payload = Some(empty(e.kind));
        self.images.acked = Some(digest);
        if e.tombstone_at.is_none() && self.images.local_row_digest == row_digest {
            e.acked_rev = e.local_rev;
            e.mark = None;
            e.base_payload = None;
            self.images.base = None;
        } else {
            e.base_payload = Some(empty(e.kind));
            self.images.base = Some(digest);
        }
        let mut index = 0;
        self.images.flights.retain(|_| {
            let keep = e.in_flight[index].local_rev > local_rev;
            index += 1;
            keep
        });
        e.in_flight.retain(|f| f.local_rev > local_rev);
        Ok(e.mark.is_some())
    }
}

/// Only ordinary owners in this build scope may change. V5 is the shared
/// older-reader fence; per-binding markers record actual representation progress.
pub(super) fn migrate(tx: &Transaction<'_>, scope: &BuildScope) -> Result<(), Error> {
    tx.execute_batch("CREATE TABLE IF NOT EXISTS cloud_runtime_payloads(binding BLOB NOT NULL REFERENCES bindings(id),digest BLOB NOT NULL CHECK(length(digest)=32),bytes BLOB NOT NULL,PRIMARY KEY(binding,digest));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS cloud_runtime_payload_retired(binding BLOB NOT NULL REFERENCES bindings(id),name TEXT NOT NULL,digest BLOB NOT NULL CHECK(length(digest)=32),PRIMARY KEY(binding,name,digest));")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS cloud_runtime_payload_owners(binding BLOB PRIMARY KEY REFERENCES bindings(id),version INTEGER NOT NULL CHECK(version=5));")?;
    for (table, notes) in [("cloud_runtime_notes", true), ("cloud_runtime_entities", false)] {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            continue;
        }
        let mut after = i64::MIN;
        loop {
            let row: Option<(i64, Vec<u8>, String)> = tx
                .query_row(
                    &format!("SELECT r.rowid,r.binding,r.content FROM {table} r JOIN bindings b ON b.id=r.binding WHERE r.rowid>?1 AND b.container=?2 AND b.environment=?3 AND b.salvage=0 AND NOT EXISTS(SELECT 1 FROM cloud_runtime_payload_owners o WHERE o.binding=b.id) ORDER BY r.rowid LIMIT 1"),
                    params![after, scope.container.as_str(), scope.environment.code()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let Some((id, b, text)) = row else {
                break;
            };
            after = id;
            let b = super::store::binding_id(b)?;
            // Already externalized V5 seed rows need only the new owner marker.
            if decode::<ImageMarker>(&text)?.payload_images.is_some() {
                continue;
            }
            if notes {
                put_note(tx, b, &decode(&text)?)?;
            } else {
                put_entity(tx, b, &decode(&text)?)?;
            }
        }
    }
    tx.execute("INSERT INTO cloud_runtime_payload_owners SELECT id,5 FROM bindings WHERE container=?1 AND environment=?2 AND salvage=0 ON CONFLICT(binding) DO NOTHING", params![scope.container.as_str(), scope.environment.code()])?;
    meguri_model::faultpoint!("cloud.payload_migration.before_commit")?;
    tx.execute("UPDATE cloud_runtime_schema SET version=5", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> BuildScope {
        BuildScope {
            container: ContainerId::new("payload.scope-a").unwrap(),
            environment: Environment::Development,
        }
    }
    #[test]
    fn historical_config_tombstone_keeps_its_handed_digest_after_observe() {
        let (_, b, _, mut e, _, _) = legacy();
        let payload = c::Payload::Config(c::Config {
            key: b"synthetic-config".to_vec(),
            val: b"1".to_vec(),
            ..Default::default()
        });
        e.kind = c::Kind::Config;
        e.key = super::super::row_payload::payload_key(&payload).unwrap();
        e.payload = payload.clone();
        e.tombstone_at = Some(71);
        let bytes = c::Entity::build(
            c::EntityFields {
                kind: e.kind,
                key: e.key.clone(),
                origin: e.origin,
                erev: 1,
                generation: None,
                tombstone: true,
                deleted_at_ms: 71,
                writer: b.0,
                base_sha256: None,
            },
            &[],
        )
        .unwrap()
        .encode()
        .unwrap();
        e.in_flight = vec![Flight {
            local_rev: e.local_rev,
            payload_digest: c::sha256(&[]),
            row_digest: c::sha256(&[]),
            payload: payload.into(),
            erev: 1,
            image_seq: 5,
            held: false,
            tombstone: true,
            tombstone_envelope_digest: Some(c::sha256(&bytes)),
        }];
        assert!(validate_config_flights(b, &e).is_ok());
        let mut bad = e.clone();
        bad.in_flight[0].tombstone_envelope_digest = Some([0; 32]);
        assert!(validate_config_flights(b, &bad).is_err());
        let handed = e.in_flight.clone();
        let mut counter = 7;
        let newer = c::Payload::Config(c::Config {
            key: e.key.clone(),
            val: b"2".to_vec(),
            ..Default::default()
        });
        assert!(e.observe(newer.clone(), DirtySource::Observer, &mut counter).unwrap());
        assert!(e.tombstone_at.is_none());
        assert_eq!(e.payload, newer);
        assert!(e.in_flight == handed);
        assert!(validate_config_flights(b, &e).is_ok());
        for invalid in 0..4 {
            let mut bad = e.clone();
            match invalid {
                0 => bad.in_flight[0].tombstone_envelope_digest = None,
                1 => bad.in_flight[0].local_rev = bad.local_rev + 1,
                2 => bad.in_flight[0].held = true,
                _ => bad.in_flight[0].erev = bad.server_erev,
            }
            assert!(validate_config_flights(b, &bad).is_err());
        }
    }

    fn legacy() -> (Connection, BindingId, NoteTracking, EntityFacts, String, String) {
        let cn = Connection::open_in_memory().unwrap();
        cn.execute_batch("PRAGMA foreign_keys=ON;
            CREATE TABLE bindings(id BLOB PRIMARY KEY,container TEXT NOT NULL,environment INTEGER NOT NULL,salvage INTEGER NOT NULL);
            CREATE TABLE cloud_runtime_schema(version INTEGER); INSERT INTO cloud_runtime_schema VALUES(4);
            CREATE TABLE cloud_runtime_notes(binding BLOB REFERENCES bindings(id),id INTEGER,content TEXT CHECK(json_valid(content)),PRIMARY KEY(binding,id));
            CREATE TABLE cloud_runtime_entities(binding BLOB REFERENCES bindings(id),kind INTEGER,key BLOB,content TEXT CHECK(json_valid(content)),PRIMARY KEY(binding,kind,key));").unwrap();
        let b = BindingId([71; 16]);
        cn.execute(
            "INSERT INTO bindings VALUES(?1,?2,?3,0)",
            params![b.0.as_slice(), scope().container.as_str(), scope().environment.code()],
        )
        .unwrap();
        let old = c::Payload::Note(c::Note {
            id: 11,
            mid: 12,
            nt_struct_gen: 1,
            flds: b"front\x1fold".to_vec(),
            ..Default::default()
        });
        let mut current = old.clone();
        let c::Payload::Note(n) = &mut current else {
            unreachable!()
        };
        n.flds = [b"front\x1f".as_slice(), &vec![b'x'; 8192]].concat();
        let row = current.row_digest().unwrap();
        let digest = current.payload_digest().unwrap();
        let envelope = c::Entity::build(
            c::EntityFields {
                kind: c::Kind::Note,
                key: c::id_key(11).to_vec(),
                origin: [73; 16],
                erev: 1,
                generation: None,
                tombstone: false,
                deleted_at_ms: 0,
                writer: b.0,
                base_sha256: None,
            },
            &old.encode().unwrap(),
        )
        .unwrap()
        .encode()
        .unwrap();
        let note = NoteTracking {
            id: 11,
            name: "synthetic-note".into(),
            origin: [73; 16],
            local_rev: 2,
            last_observed_digest: Some(row),
            local_payload: OpaqueBytes(current.encode().unwrap()),
            base: Some(OpaqueBytes(old.encode().unwrap())),
            mark: Some(DirtyMark {
                source: DirtySource::Observer,
                change_seq: 3,
                payload_digest: digest,
            }),
            acknowledged: NoteAcknowledgement {
                erev: 1,
                system_fields: OpaqueBytes(vec![19]),
                payload: OpaqueBytes(old.encode().unwrap()),
                row_digest: old.row_digest().unwrap(),
                payload_digest: old.payload_digest().unwrap(),
            },
            observer_open: 4,
            observed_seq: 5,
            observed_generation: 6,
            acked_rev: 1,
            in_flight: vec![Flight {
                local_rev: 2,
                payload_digest: digest,
                row_digest: row,
                payload: current.into(),
                erev: 2,
                image_seq: 5,
                held: true,
                tombstone: false,
                tombstone_envelope_digest: None,
            }],
            acked_envelope: OpaqueBytes(envelope),
            tombstone_at: None,
        };
        let card = c::Payload::Card(c::Card {
            id: 21,
            nid: 11,
            did: 1,
            nt_struct_gen: 1,
            ..Default::default()
        });
        let entity = EntityFacts {
            name: "synthetic-card".into(),
            kind: c::Kind::Card,
            key: c::id_key(21).to_vec(),
            origin: [74; 16],
            local_rev: 1,
            acked_rev: 0,
            tombstone_at: None,
            payload: card.clone(),
            base_payload: None,
            mark: Some(DirtyMark {
                source: DirtySource::Observer,
                change_seq: 4,
                payload_digest: card.payload_digest().unwrap(),
            }),
            server_erev: 0,
            acked_system_fields: Vec::new(),
            acked_row_digest: [0; 32],
            acked_payload_digest: [0; 32],
            acked_payload: None,
            acked_envelope: OpaqueBytes(Vec::new()),
            in_flight: vec![Flight {
                local_rev: 1,
                payload_digest: card.payload_digest().unwrap(),
                row_digest: card.row_digest().unwrap(),
                payload: card.into(),
                erev: 1,
                image_seq: 5,
                held: false,
                tombstone: false,
                tombstone_envelope_digest: None,
            }],
            observer_open: 4,
            observed_seq: 5,
            observed_generation: 6,
        };
        note.validate().unwrap();
        entity.validate().unwrap();
        let note_text = encode(&note).unwrap();
        let entity_text = encode(&entity).unwrap();
        cn.execute(
            "INSERT INTO cloud_runtime_notes VALUES(?1,11,?2)",
            params![b.0.as_slice(), &note_text],
        )
        .unwrap();
        cn.execute(
            "INSERT INTO cloud_runtime_entities VALUES(?1,2,?2,?3)",
            params![b.0.as_slice(), &entity.key, &entity_text],
        )
        .unwrap();
        (cn, b, note, entity, note_text, entity_text)
    }
    #[test]
    fn v4_upgrade_preserves_current_base_ack_and_exact_note_card_flights_atomically() {
        let (mut cn, b, note, entity, old_note, old_entity) = legacy();
        let tx = cn.transaction().unwrap();
        migrate(&tx, &scope()).unwrap();
        // A caller failure before COMMIT must leave neither a marker nor half
        // converted rows. Repeat from the same durable legacy checkpoint.
        tx.rollback().unwrap();
        assert_eq!(
            cn.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(
            cn.query_row("SELECT content FROM cloud_runtime_notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            old_note
        );
        assert_eq!(
            cn.query_row("SELECT content FROM cloud_runtime_entities", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            old_entity
        );
        assert!(
            !cn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_payloads')",
                [],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
        );
        let tx = cn.transaction().unwrap();
        migrate(&tx, &scope()).unwrap();
        tx.commit().unwrap();
        // Previous runtime readers accepted only versions 1..=4. V5 is explicit
        // protection, checked by the unchanged private WAL-copy probe path.
        assert_eq!(
            cn.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            5
        );
        let text: String = cn
            .query_row("SELECT content FROM cloud_runtime_notes", [], |r| r.get(0))
            .unwrap();
        assert!(text.len() < 8192, "header must not carry images");
        assert!(dump_note(&cn, b, &text).unwrap() == note);
        let text: String = cn
            .query_row("SELECT content FROM cloud_runtime_entities", [], |r| r.get(0))
            .unwrap();
        assert!(dump_entity(&cn, b, c::Kind::Card, &text).unwrap() == entity);
        // Current==flight and base==ACK are shared without changing any image.
        assert_eq!(
            cn.query_row("SELECT count(*) FROM cloud_runtime_payloads", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            3
        );
        cn.execute_batch(
            "CREATE TABLE cloud_workflow_schema(version INTEGER); INSERT INTO cloud_workflow_schema VALUES(1);",
        )
        .unwrap();
        assert!(super::super::runtime_store::check_version(&cn).unwrap());
        cn.execute("UPDATE cloud_runtime_schema SET version=6", []).unwrap();
        assert!(matches!(
            super::super::runtime_store::check_version(&cn),
            Err(Error::RuntimeSchemaUnsupported)
        ));
    }
    #[test]
    fn failed_v4_image_migration_rolls_back_and_retries_without_losing_flights() {
        let (mut cn, b, note, _, old_note, _) = legacy();
        meguri_model::faultpoint::arm(
            "cloud.payload_migration.before_commit",
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::StorageFull),
        );
        let tx = cn.transaction().unwrap();
        let failed = migrate(&tx, &scope());
        meguri_model::faultpoint::disarm("cloud.payload_migration.before_commit");
        assert!(failed.is_err());
        drop(tx);
        assert_eq!(
            cn.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            4
        );
        assert_eq!(
            cn.query_row("SELECT content FROM cloud_runtime_notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            old_note
        );
        let tx = cn.transaction().unwrap();
        migrate(&tx, &scope()).unwrap();
        tx.commit().unwrap();
        let text: String = cn
            .query_row("SELECT content FROM cloud_runtime_notes", [], |r| r.get(0))
            .unwrap();
        assert!(dump_note(&cn, b, &text).unwrap() == note);
    }
    fn opened(directory: &std::path::Path, scope: BuildScope) -> TrackingStore {
        match TrackingStore::open(directory, scope).unwrap() {
            OpenOutcome::Missing(s) | OpenOutcome::Opened(s) => s,
            OpenOutcome::Damaged => panic!("tracking fixture open refused: damaged store"),
            OpenOutcome::NewerSchema { found } => panic!("tracking fixture open refused: schema {found}"),
        }
    }
    fn binding(store: &mut TrackingStore, seed: u64) -> BindingId {
        store
            .create_binding(
                BindingFacts {
                    account: OpaqueBytes(vec![1]),
                    epoch: EpochId([2; 16]),
                    record_key: RecordKey([3; 32]),
                    publication: serde_json::from_str("[17,29]").unwrap(),
                    state: BindingState::Active,
                },
                &meguri_model::clock::FixedClock::new(88, meguri_model::clock::UtcOffset::UTC),
                &meguri_model::rng::SeededRandom::new(seed),
            )
            .unwrap()
            .id
    }
    #[test]
    fn actual_two_scope_v4_open_leaves_foreign_rows_byte_identical_and_later_upgrades_their_owner() {
        two_scope_upgrade(false);
    }
    #[test]
    fn actual_two_scope_v4_open_ignores_malformed_foreign_runtime_and_refuses_it_for_its_owner() {
        two_scope_upgrade(true);
    }
    fn two_scope_upgrade(malformed: bool) {
        let (_cn, _, note, entity, old_note, old_entity) = legacy();
        let run = tempfile::tempdir().unwrap();
        let a_scope = scope();
        let b_scope = if malformed {
            BuildScope {
                environment: Environment::Production,
                ..scope()
            }
        } else {
            BuildScope {
                container: ContainerId::new("payload.scope-b").unwrap(),
                ..scope()
            }
        };
        let mut a = opened(run.path(), a_scope.clone());
        let a_id = binding(&mut a, 21);
        drop(a);
        let mut b = opened(run.path(), b_scope.clone());
        let b_id = binding(&mut b, 22);
        let tx = b.connection.transaction().unwrap();
        tx.execute_batch("CREATE TABLE cloud_runtime_schema(version INTEGER NOT NULL); INSERT INTO cloud_runtime_schema VALUES(4);
            CREATE TABLE cloud_runtime_bindings(binding BLOB PRIMARY KEY REFERENCES bindings(id),content TEXT NOT NULL CHECK(json_valid(content)));
            CREATE TABLE cloud_runtime_notes(binding BLOB REFERENCES bindings(id),id INTEGER,content TEXT NOT NULL CHECK(json_valid(content)),PRIMARY KEY(binding,id));
            CREATE TABLE cloud_workflow_schema(version INTEGER NOT NULL); INSERT INTO cloud_workflow_schema VALUES(1);
            CREATE TABLE cloud_runtime_entities(binding BLOB REFERENCES bindings(id),kind INTEGER,key BLOB,content TEXT NOT NULL CHECK(json_valid(content)),PRIMARY KEY(binding,kind,key));").unwrap();
        // V4 includes the real V3 adapter metadata and absence triggers; the
        // private open probe must validate the complete on-disk schema.
        super::super::adapter_store::install(&tx).unwrap();
        let foreign_note = if malformed { "{}" } else { old_note.as_str() };
        for (owner, text) in [(a_id, old_note.as_str()), (b_id, foreign_note)] {
            tx.execute(
                "INSERT INTO cloud_runtime_notes VALUES(?1,11,?2)",
                params![owner.0.as_slice(), text],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO cloud_runtime_entities VALUES(?1,2,?2,?3)",
                params![owner.0.as_slice(), &entity.key, &old_entity],
            )
            .unwrap();
        }
        if malformed {
            tx.execute(
                "INSERT INTO cloud_runtime_bindings VALUES(?1,?2)",
                params![b_id.0.as_slice(), "{\"open_counter\":\"broken\"}"],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        drop(b);
        let mut a = opened(run.path(), a_scope.clone());
        a.install_runtime().unwrap();
        assert!(a.tracked_note(a_id, 11).unwrap().unwrap() == note);
        assert_eq!(
            a.connection
                .query_row(
                    "SELECT content FROM cloud_runtime_notes WHERE binding=?1",
                    [b_id.0.as_slice()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            foreign_note
        );
        assert_eq!(
            a.connection
                .query_row(
                    "SELECT content FROM cloud_runtime_entities WHERE binding=?1",
                    [b_id.0.as_slice()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            old_entity
        );
        assert!(!owner_v5(&a.connection, b_id).unwrap());
        assert_eq!(
            a.connection
                .query_row(
                    "SELECT count(*) FROM cloud_runtime_payloads WHERE binding=?1",
                    [b_id.0.as_slice()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
        let a_header: String = a
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_notes WHERE binding=?1",
                [a_id.0.as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        drop(a);
        let mut b = opened(run.path(), b_scope);
        if malformed {
            assert!(b.install_runtime().is_err());
            assert!(!owner_v5(&b.connection, b_id).unwrap());
            assert_eq!(
                b.connection
                    .query_row(
                        "SELECT content FROM cloud_runtime_notes WHERE binding=?1",
                        [b_id.0.as_slice()],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                foreign_note
            );
            assert_eq!(
                b.connection
                    .query_row(
                        "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
                        [b_id.0.as_slice()],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                "{\"open_counter\":\"broken\"}"
            );
        } else {
            b.install_runtime().unwrap();
            assert!(owner_v5(&b.connection, b_id).unwrap());
            assert!(b.tracked_note(b_id, 11).unwrap().unwrap() == note);
            assert!(b.entity_dump(b_id, &entity.name).unwrap().unwrap() == entity);
        }
        assert_eq!(
            b.connection
                .query_row(
                    "SELECT content FROM cloud_runtime_notes WHERE binding=?1",
                    [a_id.0.as_slice()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            a_header
        );
        drop(b);
        let mut a = opened(run.path(), a_scope);
        a.install_runtime().unwrap();
        assert!(a.tracked_note(a_id, 11).unwrap().unwrap() == note);
    }
    #[test]
    fn never_sent_large_add_remove_collects_only_unshared_images_and_rolls_back_atomically() {
        let (mut cn, b, _, _, _, _) = legacy();
        let tx = cn.transaction().unwrap();
        migrate(&tx, &scope()).unwrap();
        let mut counter = 0;
        let large = c::Payload::Note(c::Note {
            id: 31,
            mid: 12,
            nt_struct_gen: 1,
            flds: vec![b'q'; 2 * 1024 * 1024],
            ..Default::default()
        });
        let e = super::super::workflow_store::new_pending_entity(
            &RecordKey([3; 32]),
            large,
            [9; 16],
            DirtySource::Observer,
            &mut counter,
            (1, 1, 1),
        )
        .unwrap();
        assert!(e.in_flight.is_empty() && e.server_erev == 0);
        put_entity(&tx, b, &e).unwrap();
        let digest = e.payload.payload_digest().unwrap();
        tx.commit().unwrap();
        let tx = cn.transaction().unwrap();
        super::super::workflow_store::remove_entity(&tx, b, &e).unwrap();
        assert!(blob(&tx, b, digest).is_err());
        tx.rollback().unwrap();
        assert!(blob(&cn, b, digest).is_ok());
        let tx = cn.transaction().unwrap();
        super::super::workflow_store::remove_entity(&tx, b, &e).unwrap();
        tx.commit().unwrap();
        assert!(blob(&cn, b, digest).is_err());
    }
    #[test]
    fn removal_keeps_shared_current_base_ack_flights_and_own_apply_intent_images() {
        let (mut cn, b, _, _, _, _) = legacy();
        let tx = cn.transaction().unwrap();
        migrate(&tx, &scope()).unwrap();
        super::super::incoming_store::install(&tx).unwrap();
        let mut counter = 0;
        let p = c::Payload::Note(c::Note {
            id: 41,
            mid: 12,
            nt_struct_gen: 1,
            flds: vec![b's'; 2 * 1024 * 1024],
            ..Default::default()
        });
        let e = super::super::workflow_store::new_pending_entity(
            &RecordKey([3; 32]),
            p,
            [9; 16],
            DirtySource::Observer,
            &mut counter,
            (1, 1, 1),
        )
        .unwrap();
        put_entity(&tx, b, &e).unwrap();
        let digest = e.payload.payload_digest().unwrap();
        // Identical images can be shared within one binding, even when another
        // header retains a historical image. This fixture models storage, not an ACK.
        let mut other = header(
            c::Kind::Note,
            &tx.query_row(
                "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=11",
                [b.0.as_slice()],
                |r| r.get::<_, String>(0),
            )
            .unwrap(),
        )
        .unwrap();
        let original = other.images.clone();
        for slot in 0..4 {
            other.images = original.clone();
            match slot {
                0 => other.images.local = digest,
                1 => other.images.base = Some(digest),
                2 => other.images.acked = Some(digest),
                _ => other.images.flights.push(digest),
            }
            if slot == 3 {
                other.facts.in_flight.push(Flight {
                    local_rev: e.local_rev,
                    payload_digest: digest,
                    row_digest: e.payload.row_digest().unwrap(),
                    payload: e.payload.clone().into(),
                    erev: 1,
                    image_seq: 1,
                    held: false,
                    tombstone: false,
                    tombstone_envelope_digest: None,
                });
            }
            put_header(&tx, b, &other).unwrap();
            super::super::workflow_store::remove_entity(&tx, b, &e).unwrap();
            assert!(blob(&tx, b, digest).is_ok());
            if slot == 3 {
                other.facts.in_flight.pop();
            }
            put_entity(&tx, b, &e).unwrap();
        }
        other.images = original;
        put_header(&tx, b, &other).unwrap();
        // A minimal SQL reference fixture pins an own-flight image until
        // that same transaction retires the intent; collection is targeted.
        tx.execute(
            "INSERT INTO cloud_apply_intents(binding,content) VALUES(?1,?2)",
            params![
                b.0.as_slice(),
                format!("{{\"keys\":[{{\"name\":\"{}\",\"own_flight\":1}}]}}", e.name)
            ],
        )
        .unwrap();
        super::super::workflow_store::remove_entity(&tx, b, &e).unwrap();
        assert!(blob(&tx, b, digest).is_ok());
        tx.execute("DELETE FROM cloud_apply_intents WHERE binding=?1", [b.0.as_slice()])
            .unwrap();
        collect_retired(&tx, b).unwrap();
        assert!(blob(&tx, b, digest).is_err());
        tx.rollback().unwrap();
    }
}
