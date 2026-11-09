//! J2.3/J3.2 original-role observations. All identities and canonical images
//! remain private to this object; no path, query or caller's verdict is accepted.
use crate::{CoreError, runtime};
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_storage::{ReadRows, ReaderFactory, ReaderRole, RowKey, Table};
use meguri_sync::cloud::{
    entity_codec as c,
    row_payload::{self, View},
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, PoisonError},
};

fn invalid() -> CoreError {
    CoreError::Internal {
        message: "original role invalid".into(),
    }
}
fn checked<T, E>(r: Result<T, E>) -> Result<T, CoreError> {
    r.map_err(|_| invalid())
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CloudOriginalRoleFacts {
    pub note_digest: Vec<u8>,
    pub baseline_digest: Vec<u8>,
    pub source_main_sha256: Vec<u8>,
    pub source_wal_sha256: Option<Vec<u8>>,
    pub field_count: u32,
    pub template_count: u32,
    pub card_count: u32,
    pub deck_count: u32,
    pub q_ordinal: u32,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudOriginalOwnedFacts {
    pub generation: u64,
    pub seq: u64,
    pub note_digest: Vec<u8>,
    pub q_changed: bool,
}

#[derive(Clone)]
struct Baseline {
    note: c::Note,
    layout: c::Notetype,
    cards: Vec<c::Card>,
    decks: Vec<c::Deck>,
    q: usize,
    sort: usize,
    marker: String,
}
#[derive(uniffi::Object)]
pub struct CloudOriginalRole {
    baseline: Baseline,
    facts: CloudOriginalRoleFacts,
    edit: Mutex<EditState>,
    owner: Mutex<Option<(std::sync::Weak<meguri_host::CoreHost>, meguri_engine::Generation)>>,
}
#[derive(Default)]
struct EditState {
    attempted: bool,
    // Actual executor result, including native change keys, stays in Rust.
    committed: Option<meguri_engine::OpResult<(meguri_engine::notes::Updated, i64)>>,
    facts: Option<CloudOriginalEditFacts>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudOriginalEditFacts {
    pub generation: u64,
    pub seq: u64,
    pub note_digest: Vec<u8>,
    pub local_revision: Option<u64>,
    pub source: crate::changes::OpSource,
    pub undo_name: Option<String>,
    pub updated_notes: u32,
}
#[derive(uniffi::Object)]
pub struct CloudOriginalEditResult {
    pub(crate) cell: Arc<crate::cloud_policy::policy_jobs::Cell<CloudOriginalEditFacts>>,
}
#[derive(Clone, uniffi::Record)]
pub struct CloudOriginalEditJob {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<CloudOriginalEditResult>,
}
impl std::fmt::Debug for CloudOriginalRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CloudOriginalRole")
    }
}

pub fn inspect(main: Vec<u8>, wal: Option<Vec<u8>>) -> Result<Arc<CloudOriginalRole>, CoreError> {
    let reader = checked(ReaderFactory::original_role_bytes(&main, wal.as_deref()))?;
    let snapshot = checked(reader.begin_snapshot())?;
    let integrity = checked(snapshot.smoke_integrity())?;
    if !integrity.integrity_ok
        || !integrity.schema_ok
        || integrity.missing_card_notes != 0
        || integrity.missing_card_decks != 0
        || integrity.missing_note_types != 0
        || integrity.missing_field_types != 0
        || integrity.wrong_field_counts != 0
    {
        return Err(invalid());
    }
    let first = checked(snapshot.notes_after(None, 1))?;
    let id = first.first().ok_or_else(invalid)?.id;
    let baseline = capture(&snapshot, id)?;
    let facts = baseline.facts(&main, wal.as_deref())?;
    Ok(Arc::new(CloudOriginalRole {
        baseline,
        facts,
        edit: Mutex::new(EditState::default()),
        owner: Mutex::new(None),
    }))
}

fn capture(rows: &impl ReadRows, id: NoteId) -> Result<Baseline, CoreError> {
    let view = row_payload::ReadView(rows);
    let raw = checked(view.row(Table::Notes, &RowKey::Id(id.0)))?.ok_or_else(invalid)?;
    let mid = checked(row_payload::int(&raw, 2))?;
    let typed = checked(rows.notetype(NotetypeId(mid)))?.ok_or_else(invalid)?;
    let layout_row = checked(view.row(Table::Notetypes, &RowKey::Id(mid)))?.ok_or_else(invalid)?;
    let layout = checked(row_payload::notetype(&view, &layout_row, 1))?;
    let sort = typed.config.sort_field_idx as usize;
    if typed.config.kind != 0
        || layout.name != b"QA"
        || layout.fields.len() != 2
        || layout.templates.len() != 1
        || sort >= layout.fields.len()
    {
        return Err(invalid());
    }
    let mut names = BTreeSet::new();
    for (i, field) in layout.fields.iter().enumerate() {
        if field.ord != i as i64 || !names.insert(field.name.clone()) || std::str::from_utf8(&field.name).is_err() {
            return Err(invalid());
        }
    }
    if !names.contains(b"A".as_slice()) {
        return Err(invalid());
    }
    for (i, template) in layout.templates.iter().enumerate() {
        if template.ord != i as i64 {
            return Err(invalid());
        }
    }
    let q = layout.fields.iter().position(|f| f.name == b"Q").ok_or_else(invalid)?;
    let project = |kind, raw: &[_]| {
        checked(row_payload::project(&view, kind, raw, |id| {
            if id != mid {
                return Err(meguri_sync::cloud::Error::InvalidData);
            }
            Ok(layout.clone())
        }))
    };
    let c::Payload::Note(note) = project(c::Kind::Note, &raw)? else {
        return Err(invalid());
    };
    if note.id != id.0
        || note.guid.is_empty()
        || note.flds.split(|b| *b == 31).count() != layout.fields.len()
        || std::str::from_utf8(&note.flds).is_err()
    {
        return Err(invalid());
    }
    // A duplicate native identity is ambiguous even when its primary key differs.
    let mut after = None;
    let mut matches = 0;
    let mut scanned = 0usize;
    loop {
        let page = checked(rows.notes_after(after, 250))?;
        if page.is_empty() {
            break;
        }
        for n in page {
            if after.is_some_and(|a| n.id <= a) {
                return Err(invalid());
            }
            after = Some(n.id);
            scanned += 1;
            if scanned > 1_000_000 {
                return Err(invalid());
            }
            if n.guid.as_bytes() == note.guid {
                matches += 1;
            }
        }
    }
    if matches != 1 {
        return Err(invalid());
    }
    let mut cards = Vec::new();
    let mut ords = BTreeSet::new();
    for card in checked(rows.cards_of_note(id))? {
        let raw = checked(view.row(Table::Cards, &RowKey::Id(card.id.0)))?.ok_or_else(invalid)?;
        let c::Payload::Card(card) = project(c::Kind::Card, &raw)? else {
            return Err(invalid());
        };
        if card.nid != id.0
            || card.ord < 0
            || card.ord as usize >= layout.templates.len()
            || !ords.insert(card.ord)
            || card.odid != 0
        {
            return Err(invalid());
        }
        cards.push(card);
    }
    cards.sort_by_key(|c| c.id);
    if cards.is_empty() || cards.len() != layout.templates.len() {
        return Err(invalid());
    }
    let deck_rows = checked(rows.deck_rows())?;
    if deck_rows.len() != 2 {
        return Err(invalid());
    }
    let mut decks = Vec::new();
    for row in deck_rows {
        let typed = checked(rows.deck(row.id))?.ok_or_else(invalid)?;
        if !matches!(typed.kind.kind, meguri_model::proto::deck::DeckKind::Normal(_)) {
            return Err(invalid());
        }
        let raw = checked(view.row(Table::Decks, &RowKey::Id(row.id.0)))?.ok_or_else(invalid)?;
        let c::Payload::Deck(deck) = project(c::Kind::Deck, &raw)? else {
            return Err(invalid());
        };
        decks.push(deck);
    }
    decks.sort_by_key(|d| d.id);
    let parent = decks
        .iter()
        .find(|d| d.id == 1 && d.parent_id == 0)
        .ok_or_else(invalid)?;
    let name = std::str::from_utf8(&parent.name).map_err(|_| invalid())?;
    let marker = name
        .strip_suffix("-D1")
        .filter(|m| !m.is_empty())
        .ok_or_else(invalid)?
        .to_owned();
    let child_name = format!("{marker}-D1\u{1f}{marker}-D2");
    let children: Vec<_> = decks.iter().filter(|d| d.id != parent.id).collect();
    if children.len() != 1
        || children[0].parent_id != parent.id
        || children[0].name != child_name.as_bytes()
        || !cards.iter().all(|c| decks.iter().any(|d| d.id == c.did))
    {
        return Err(invalid());
    }
    Ok(Baseline {
        note,
        layout,
        cards,
        decks,
        q,
        sort,
        marker,
    })
}
impl Baseline {
    fn facts(&self, main: &[u8], wal: Option<&[u8]>) -> Result<CloudOriginalRoleFacts, CoreError> {
        let mut hash = Sha256::new();
        let payloads = [
            c::Payload::Note(self.note.clone()),
            c::Payload::Notetype(self.layout.clone()),
        ]
        .into_iter()
        .chain(self.cards.iter().cloned().map(c::Payload::Card))
        .chain(self.decks.iter().cloned().map(c::Payload::Deck));
        for payload in payloads {
            hash.update([payload.kind() as u8]);
            hash.update(checked(payload.payload_digest())?);
        }
        Ok(CloudOriginalRoleFacts {
            note_digest: checked(c::Payload::Note(self.note.clone()).payload_digest())?.to_vec(),
            baseline_digest: hash.finalize().to_vec(),
            source_main_sha256: Sha256::digest(main).to_vec(),
            source_wal_sha256: wal.map(|w| Sha256::digest(w).to_vec()),
            field_count: self.layout.fields.len() as u32,
            template_count: self.layout.templates.len() as u32,
            card_count: self.cards.len() as u32,
            deck_count: self.decks.len() as u32,
            q_ordinal: self.q as u32,
        })
    }
    fn compare(&self, current: &Self) -> Result<bool, CoreError> {
        if self.layout != current.layout
            || self.cards != current.cards
            || self.decks != current.decks
            || self.q != current.q
            || self.sort != current.sort
            || self.marker != current.marker
        {
            return Err(invalid());
        }
        if self.note == current.note {
            return Ok(false);
        }
        let mut expected = self.note.clone();
        let fields = std::str::from_utf8(&expected.flds).map_err(|_| invalid())?;
        let mut fields: Vec<String> = fields.split('\u{1f}').map(str::to_owned).collect();
        let edit = format!("{}-sync-now-n1-Q", self.marker);
        if fields[self.q] == edit {
            return Err(invalid());
        }
        fields[self.q] = edit;
        expected.flds = fields.join("\u{1f}").into_bytes();
        expected.csum = i64::from(meguri_text::checksum::note_checksum(&fields[0]));
        let sort = meguri_text::checksum::sort_text_of(&fields, self.sort).into_owned();
        // Q4.2 fields contain their marked text, so their derived sort value is TEXT.
        expected.sfld = c::Stored::Text(sort.into_bytes());
        if current.note.modified < expected.modified {
            return Err(invalid());
        }
        expected.modified = current.note.modified;
        if expected != current.note {
            return Err(invalid());
        }
        Ok(true)
    }
}
impl CloudOriginalRole {
    pub(crate) fn facts_inner(&self) -> CloudOriginalRoleFacts {
        self.facts.clone()
    }
    pub(crate) fn validate_owned_inner(&self) -> Result<CloudOriginalOwnedFacts, CoreError> {
        // Always read the actual retained Host, never another writer or a
        // caller's current-row ID. Adoption preserving these native identities
        // is supported; rekeyed/ambiguous adoption fails closed.
        let host = checked(runtime::host())?;
        let collection = checked(host.collection())?;
        let reader = checked(collection.reader(ReaderRole::Query))?;
        let snapshot = checked(reader.snapshot())?;
        let current = capture(&*snapshot, NoteId(self.baseline.note.id))?;
        let changed = self.baseline.compare(&current)?;
        let mut owner = self.owner.lock().unwrap_or_else(PoisonError::into_inner);
        if owner.is_none() {
            *owner = Some((Arc::downgrade(&host), snapshot.generation()));
        }
        Ok(CloudOriginalOwnedFacts {
            generation: snapshot.generation().0,
            seq: snapshot.seq().0,
            note_digest: checked(c::Payload::Note(current.note).payload_digest())?.to_vec(),
            q_changed: changed,
        })
    }
}

impl CloudOriginalRole {
    pub(crate) fn start_fixed_edit_inner(self: Arc<Self>) -> Result<CloudOriginalEditJob, CoreError> {
        use crate::cloud_policy::policy_jobs::{Cell, Completion};
        let host = runtime::host()?;
        let collection = host.cloud_connection_collection()?;
        let generation = collection.generation();
        {
            let owner = self.owner.lock().unwrap_or_else(PoisonError::into_inner);
            let (bound, bound_generation) = owner.as_ref().ok_or_else(invalid)?;
            if *bound_generation != generation || !bound.upgrade().is_some_and(|h| Arc::ptr_eq(&h, &host)) {
                return Err(invalid());
            }
        }
        // A fresh handle cannot recover an interrupted command from matching
        // content. J5 restoration remains a separate, missing inspector.
        {
            let mut state = self.edit.lock().unwrap_or_else(PoisonError::into_inner);
            if state.attempted {
                return Err(invalid());
            }
            state.attempted = true;
        }
        let cell = Arc::new(Cell(Mutex::new(None)));
        let completion = Completion(cell.clone());
        let retained = host.clone();
        let handle = host.start_cloud_connection_job(generation, "CloudOriginalRole.fixed_edit", move |job| {
            let value = (|| {
                job.cancel.check()?;
                let baseline = self.baseline.clone();
                let owner = retained.clone();
                // Same ordinary Update Note admission and updater as
                // notes::update: one UserUndoable op, no raw write or replay.
                let name = meguri_engine::undo::names::UPDATE_NOTE;
                let spec = meguri_engine::OpSpec::named(name, meguri_engine::OpSource::UserUndoable)
                    .undo_step(name, meguri_engine::StepKind::Other)
                    .cancel(job.cancel.clone())
                    .progress(job.progress.clone());
                let result = collection.run(spec, move |ctx| {
                    let actual = runtime::host().map_err(meguri_engine::EngineError::from)?;
                    if !Arc::ptr_eq(&actual, &owner) {
                        return Err(meguri_engine::EngineError::from(invalid()));
                    }
                    let now = ctx.now().0;
                    let scope = ctx.scope();
                    // Read and validate inside the real write boundary: a
                    // concurrent edit cannot be overwritten by prepared fields.
                    let current = capture(scope, NoteId(baseline.note.id)).map_err(meguri_engine::EngineError::from)?;
                    if baseline.compare(&current).map_err(meguri_engine::EngineError::from)? {
                        return Err(meguri_engine::EngineError::from(invalid()));
                    }
                    let note = scope
                        .note(NoteId(baseline.note.id))?
                        .ok_or_else(|| meguri_engine::EngineError::from(invalid()))?;
                    let mut fields: Vec<String> = note.field_values().map(str::to_owned).collect();
                    let edit = format!("{}-sync-now-n1-Q", baseline.marker);
                    if fields[baseline.q] == edit {
                        return Err(meguri_engine::EngineError::from(invalid()));
                    }
                    fields[baseline.q] = edit;
                    let updated = meguri_engine::notes::update_in(
                        scope,
                        &meguri_engine::notes::UpdateNote {
                            id: note.id,
                            fields,
                            tags: meguri_engine::tags::split_text(&note.tags).map(str::to_owned).collect(),
                        },
                    )?;
                    let after = capture(scope, note.id).map_err(meguri_engine::EngineError::from)?;
                    if !updated.written || !baseline.compare(&after).map_err(meguri_engine::EngineError::from)? {
                        return Err(meguri_engine::EngineError::from(invalid()));
                    }
                    Ok((updated, now))
                })?;
                // Keep the real committed provenance even if flush/readback
                // fails. Such a failure returns no successful edit facts.
                let seq = result.seq;
                let now = result.value.1;
                let summary = crate::mutations::MutationSummary::of(&result);
                self.edit.lock().unwrap_or_else(PoisonError::into_inner).committed = Some(result);
                // Use the existing generation-bound WAL path off the writer.
                // The lifecycle control lane can be draining this very job;
                // acquiring that lane here would deadlock close/suspend.
                let durability_wait = std::time::Duration::from_secs(5);
                collection.flush_wal(durability_wait)?;
                let actual = runtime::host()?;
                if !Arc::ptr_eq(&actual, &retained) || retained.cloud_connection_generation()? != generation {
                    return Err(invalid());
                }
                let reader = collection.reader(ReaderRole::Query)?;
                let snapshot = reader.snapshot()?;
                let current = capture(&*snapshot, NoteId(self.baseline.note.id))?;
                let note = snapshot.note(NoteId(self.baseline.note.id))?.ok_or_else(invalid)?;
                if !self.baseline.compare(&current)?
                    || snapshot.generation() != generation
                    || snapshot.seq() < seq
                    || note.mtime.0 != now.div_euclid(1000)
                    || note.usn.0 != -1
                    || snapshot.col()?.modified.0 != now
                    || summary.changes.source != crate::changes::OpSource::UserUndoable
                    || summary.changes.notes.len() != 1
                    || summary.changes.notes[0].id != self.baseline.note.id
                    || summary.changes.notes[0].kind != crate::changes::ChangeKind::Updated
                    || !summary.changes.cards.is_empty()
                {
                    return Err(invalid());
                }
                let identity = snapshot.wal_identity();
                match collection.wait_wal_covered(identity, seq, durability_wait)? {
                    meguri_engine::WalCoverage::Covered(receipt)
                        if receipt.identity() == identity && receipt.through() >= seq => {}
                    _ => return Err(invalid()),
                }
                let digest = checked(c::Payload::Note(current.note).payload_digest())?;
                let local_revision = if let Some(core) = retained.cloud_runtime() {
                    let status = checked(core.status())?;
                    let binding = status.binding.ok_or_else(invalid)?;
                    let name = checked(c::entity_name(
                        &binding.facts.record_key.0,
                        c::Kind::Note,
                        &c::id_key(self.baseline.note.id),
                    ))?;
                    let entity = checked(core.entity_facts(&name))?.ok_or_else(invalid)?;
                    if entity.local_rev == 0
                        || entity.tombstone_at.is_some()
                        || entity.observed_generation != generation.0
                        || entity.observed_seq != seq.0
                        || checked(entity.payload.payload_digest())? != digest
                    {
                        return Err(invalid());
                    }
                    Some(entity.local_rev)
                } else {
                    None
                };
                let facts = CloudOriginalEditFacts {
                    generation: summary.generation,
                    seq: summary.seq,
                    note_digest: digest.to_vec(),
                    local_revision,
                    source: summary.changes.source,
                    undo_name: summary.undo_name,
                    updated_notes: summary.changes.notes.len() as u32,
                };
                self.edit.lock().unwrap_or_else(PoisonError::into_inner).facts = Some(facts.clone());
                Ok(facts)
            })();
            completion.finish(value)
        })?;
        Ok(CloudOriginalEditJob {
            handle: crate::jobs::wrap(handle),
            result: Arc::new(CloudOriginalEditResult { cell }),
        })
    }
}
