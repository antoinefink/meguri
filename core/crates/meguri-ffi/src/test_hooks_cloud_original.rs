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

#[track_caller]
fn invalid() -> CoreError {
    CoreError::Internal {
        message: format!(
            "original role invalid: validation line {}",
            std::panic::Location::caller().line()
        ),
    }
}
#[track_caller]
fn checked<T, E>(r: Result<T, E>) -> Result<T, CoreError> {
    let line = std::panic::Location::caller().line();
    r.map_err(|_| CoreError::Internal {
        message: format!("original role invalid: validation line {line}"),
    })
}
fn invalid_category(category: &'static str) -> CoreError {
    CoreError::Internal {
        message: format!("original role invalid: {category}"),
    }
}
// Only locally classified capture/compare errors enter here, never raw errors.
fn at_stage<T>(stage: &'static str, result: Result<T, CoreError>) -> Result<T, CoreError> {
    result.map_err(|error| match error {
        CoreError::Internal { message } => CoreError::Internal {
            message: format!("original role invalid: {stage}: {message}"),
        },
        other => other,
    })
}
fn invalid_mask(category: &'static str, fields: &[(&'static str, bool)]) -> CoreError {
    let mask: Vec<_> = fields
        .iter()
        .map(|(field, mismatch)| format!("{field}={mismatch}"))
        .collect();
    CoreError::Internal {
        message: format!("original role invalid: {category}: {}", mask.join(",")),
    }
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

#[derive(Clone, uniffi::Record)]
pub struct CloudOriginalObservation {
    pub generation: u64,
    pub seq: u64,
    pub q_changed: bool,
    pub entity: super::CloudSmokeEntity,
    pub observed_seq: u64,
    pub applied_seq: Option<u64>,
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
    committed: Option<meguri_engine::OpResult<(meguri_engine::notes::Updated, c::Note)>>,
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
    pub original_witness: Option<Vec<u8>>,
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
        return Err(invalid_category("capture.structure.layout"));
    }
    let mut names = BTreeSet::new();
    for (i, field) in layout.fields.iter().enumerate() {
        if field.ord != i as i64 || !names.insert(field.name.clone()) || std::str::from_utf8(&field.name).is_err() {
            return Err(invalid_category("capture.structure.fields"));
        }
    }
    if !names.contains(b"A".as_slice()) {
        return Err(invalid_category("capture.structure.answer_missing"));
    }
    for (i, template) in layout.templates.iter().enumerate() {
        if template.ord != i as i64 {
            return Err(invalid_category("capture.ordinal.template"));
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
        return Err(invalid_category("capture.note.payload_kind"));
    };
    if note.id != id.0
        || note.guid.is_empty()
        || note.flds.split(|b| *b == 31).count() != layout.fields.len()
        || std::str::from_utf8(&note.flds).is_err()
    {
        return Err(invalid_category("capture.note.shape"));
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
                return Err(invalid_category("capture.note.scan_order"));
            }
            after = Some(n.id);
            scanned += 1;
            if scanned > 1_000_000 {
                return Err(invalid_category("capture.note.scan_budget"));
            }
            if n.guid.as_bytes() == note.guid {
                matches += 1;
            }
        }
    }
    if matches != 1 {
        return Err(invalid_category("capture.note.guid_ambiguous"));
    }
    let mut cards = Vec::new();
    let mut ords = BTreeSet::new();
    for card in checked(rows.cards_of_note(id))? {
        let raw = checked(view.row(Table::Cards, &RowKey::Id(card.id.0)))?.ok_or_else(invalid)?;
        let c::Payload::Card(card) = project(c::Kind::Card, &raw)? else {
            return Err(invalid_category("capture.card.payload_kind"));
        };
        if card.nid != id.0
            || card.ord < 0
            || card.ord as usize >= layout.templates.len()
            || !ords.insert(card.ord)
            || card.odid != 0
        {
            return Err(invalid_category("capture.card.shape"));
        }
        cards.push(card);
    }
    cards.sort_by_key(|c| c.id);
    if cards.is_empty() || cards.len() != layout.templates.len() {
        return Err(invalid_category("capture.card.count"));
    }
    let deck_rows = checked(rows.deck_rows())?;
    if deck_rows.len() != 2 {
        return Err(invalid_category("capture.deck.count"));
    }
    let mut decks = Vec::new();
    for row in deck_rows {
        let typed = checked(rows.deck(row.id))?.ok_or_else(invalid)?;
        if !matches!(typed.kind.kind, meguri_model::proto::deck::DeckKind::Normal(_)) {
            return Err(invalid_category("capture.deck.kind"));
        }
        let raw = checked(view.row(Table::Decks, &RowKey::Id(row.id.0)))?.ok_or_else(invalid)?;
        let c::Payload::Deck(deck) = project(c::Kind::Deck, &raw)? else {
            return Err(invalid_category("capture.deck.payload_kind"));
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
        return Err(invalid_category("capture.deck.graph"));
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
            return Err(invalid_mask(
                "compare",
                &[
                    ("structure", self.layout != current.layout),
                    ("card", self.cards != current.cards),
                    ("deck", self.decks != current.decks),
                    ("q_ordinal", self.q != current.q),
                    ("sort_ordinal", self.sort != current.sort),
                    ("marker", self.marker != current.marker),
                ],
            ));
        }
        if self.note == current.note {
            return Ok(false);
        }
        let mut expected = self.note.clone();
        let fields = std::str::from_utf8(&expected.flds).map_err(|_| invalid_category("compare.note.flds_utf8"))?;
        let mut fields: Vec<String> = fields.split('\u{1f}').map(str::to_owned).collect();
        let edit = format!("{}-sync-now-n1-Q", self.marker);
        if fields[self.q] == edit {
            return Err(invalid_category("compare.note.already_edited"));
        }
        fields[self.q] = edit;
        expected.flds = fields.join("\u{1f}").into_bytes();
        expected.csum = i64::from(meguri_text::checksum::note_checksum(&fields[0]));
        let sort = meguri_text::checksum::sort_text_of(&fields, self.sort).into_owned();
        // Q4.2 fields contain their marked text, so their derived sort value is TEXT.
        expected.sfld = c::Stored::Text(sort.into_bytes());
        if current.note.modified < expected.modified {
            return Err(invalid_category("compare.note.modified_regressed"));
        }
        expected.modified = current.note.modified;
        if expected != current.note {
            return Err(invalid_mask(
                "compare.note",
                &[
                    ("id", expected.id != current.note.id),
                    ("guid", expected.guid != current.note.guid),
                    ("mid", expected.mid != current.note.mid),
                    ("modified", expected.modified != current.note.modified),
                    ("tags", expected.tags != current.note.tags),
                    ("flds", expected.flds != current.note.flds),
                    ("sfld", expected.sfld != current.note.sfld),
                    ("csum", expected.csum != current.note.csum),
                    ("flags", expected.flags != current.note.flags),
                    ("data", expected.data != current.note.data),
                    ("nt_struct_gen", expected.nt_struct_gen != current.note.nt_struct_gen),
                ],
            ));
        }
        Ok(true)
    }
}
impl CloudOriginalRole {
    pub(crate) fn observe_inner(&self) -> Result<CloudOriginalObservation, CoreError> {
        let host = checked(runtime::host())?;
        let collection = checked(host.cloud_connection_collection())?;
        let generation = collection.generation();
        let bound_generation = {
            let owner = self.owner.lock().unwrap_or_else(PoisonError::into_inner);
            let (bound, bound_generation) = owner.as_ref().ok_or_else(invalid)?;
            if !bound.upgrade().is_some_and(|h| Arc::ptr_eq(&h, &host)) {
                return Err(invalid_category("observation.owner_or_generation"));
            }
            *bound_generation
        };
        let core = host.cloud_runtime().ok_or_else(invalid)?;
        if bound_generation != generation {
            // Existing-collection adoption replaces the collection inside the
            // retained Host. Only its actual completed activation bridges this
            // generation; an ordinary successor opening cannot rebind a role.
            use meguri_sync::cloud::selected_observation::{Proof, Provenance};
            let adopted = checked(core.selected_control_for_testing(&collection))?;
            if generation <= bound_generation
                || adopted.status != Proof::Proven
                || adopted.provenance != Some(Provenance::RetainedAdoption)
                || adopted.activation != Proof::Proven
                || adopted.owned_backup != Proof::Proven
            {
                return Err(invalid_category("observation.adoption_authority"));
            }
        }
        let reader = checked(collection.reader(ReaderRole::Query))?;
        let snapshot = checked(reader.snapshot())?;
        let current = capture(&*snapshot, NoteId(self.baseline.note.id))?;
        let q_changed = self.baseline.compare(&current)?;
        let digest = checked(c::Payload::Note(current.note).payload_digest())?;
        let (e, applied_seq) = core
            .original_note_observation_for_testing(
                &collection,
                self.baseline.note.id,
                snapshot.wal_identity(),
                snapshot.seq(),
                digest,
            )
            .map_err(|error| {
                // This module is test-hooks-only. Preserve Internal while retaining
                // closed variant tags instead of any Runtime error text or payload.
                let (kind, variant) = error.refusal_tags_for_testing();
                CoreError::Internal {
                    message: format!("original role invalid: observation.runtime: errorKind={kind} variant={variant}"),
                }
            })?;
        let actual = checked(runtime::host())?;
        if !Arc::ptr_eq(&actual, &host)
            || snapshot.generation() != generation
            || collection.last_seq() != snapshot.seq()
            || checked(host.cloud_connection_generation())? != generation
        {
            return Err(invalid_category("observation.owner_changed"));
        }
        Ok(CloudOriginalObservation {
            generation: generation.0,
            seq: snapshot.seq().0,
            q_changed,
            observed_seq: e.observed_seq,
            applied_seq,
            entity: super::CloudSmokeEntity {
                label: "n1".into(),
                record_name: e.name,
                revision: e.local_rev,
                acked_revision: e.acked_rev,
                digest: digest.to_vec(),
                acked_envelope_digest: c::sha256(&e.acked_envelope.0).to_vec(),
                acknowledged: e.local_rev > 0
                    && e.acked_rev == e.local_rev
                    && e.acked_payload_digest == digest
                    && e.mark.is_none()
                    && e.in_flight.is_empty()
                    && e.server_erev > 0
                    && !e.acked_system_fields.is_empty()
                    && !e.acked_envelope.0.is_empty(),
                media_bytes: None,
                media_sha256: None,
                blob_present: false,
                media_change_counter: None,
            },
        })
    }
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
            let (bound, bound_generation) = owner
                .as_ref()
                .ok_or_else(|| invalid_category("binding.owner_missing"))?;
            if *bound_generation != generation || !bound.upgrade().is_some_and(|h| Arc::ptr_eq(&h, &host)) {
                return Err(invalid_category("binding.owner_or_generation"));
            }
        }
        // A fresh handle cannot recover an interrupted command from matching
        // content. J5 restoration remains a separate, missing inspector.
        {
            let mut state = self.edit.lock().unwrap_or_else(PoisonError::into_inner);
            if state.attempted {
                return Err(invalid_category("pre_write.already_attempted"));
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
                        return Err(meguri_engine::EngineError::from(invalid_category("pre_write.owner")));
                    }
                    let scope = ctx.scope();
                    // Read and validate inside the real write boundary: a
                    // concurrent edit cannot be overwritten by prepared fields.
                    let current = at_stage("pre_write.capture", capture(scope, NoteId(baseline.note.id)))
                        .map_err(meguri_engine::EngineError::from)?;
                    if at_stage("pre_write.compare", baseline.compare(&current))
                        .map_err(meguri_engine::EngineError::from)?
                    {
                        return Err(meguri_engine::EngineError::from(invalid_category(
                            "pre_write.compare.changed",
                        )));
                    }
                    let note = scope
                        .note(NoteId(baseline.note.id))?
                        .ok_or_else(|| meguri_engine::EngineError::from(invalid_category("pre_write.note_missing")))?;
                    let mut fields: Vec<String> = note.field_values().map(str::to_owned).collect();
                    let edit = format!("{}-sync-now-n1-Q", baseline.marker);
                    if fields[baseline.q] == edit {
                        return Err(meguri_engine::EngineError::from(invalid_category(
                            "pre_write.already_edited",
                        )));
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
                    let after = at_stage("post_updater.capture", capture(scope, note.id))
                        .map_err(meguri_engine::EngineError::from)?;
                    if !updated.written
                        || !at_stage("post_updater.compare", baseline.compare(&after))
                            .map_err(meguri_engine::EngineError::from)?
                    {
                        return Err(meguri_engine::EngineError::from(invalid_category(
                            if !updated.written {
                                "post_updater.not_written"
                            } else {
                                "post_updater.compare.unchanged"
                            },
                        )));
                    }
                    // Retain the actual canonical after-image, including the
                    // updater's independently sampled note stamp.
                    Ok((updated, after.note))
                })?;
                // Keep the real committed provenance even if flush/readback
                // fails. Such a failure returns no successful edit facts.
                let seq = result.seq;
                let committed_note = result.value.1.clone();
                let col_mod = result.col_mod;
                let summary = crate::mutations::MutationSummary::of(&result);
                self.edit.lock().unwrap_or_else(PoisonError::into_inner).committed = Some(result);
                let col_mod =
                    col_mod.ok_or_else(|| invalid_category("postcommit.metadata.collection_stamp_missing"))?;
                // Use the existing generation-bound WAL path off the writer.
                // The lifecycle control lane can be draining this very job;
                // acquiring that lane here would deadlock close/suspend.
                let durability_wait = std::time::Duration::from_secs(5);
                collection.flush_wal(durability_wait)?;
                let actual = runtime::host()?;
                if !Arc::ptr_eq(&actual, &retained) || retained.cloud_connection_generation()? != generation {
                    return Err(invalid_category("postcommit.binding.owner_or_generation"));
                }
                let reader = collection.reader(ReaderRole::Query)?;
                let snapshot = reader.snapshot()?;
                let current = at_stage("postcommit.capture", capture(&*snapshot, NoteId(self.baseline.note.id)))?;
                let note = snapshot
                    .note(NoteId(self.baseline.note.id))?
                    .ok_or_else(|| invalid_category("postcommit.note_missing"))?;
                if !at_stage("postcommit.compare", self.baseline.compare(&current))? {
                    return Err(invalid_category("postcommit.compare.unchanged"));
                }
                if snapshot.generation() != generation {
                    return Err(invalid_category("postcommit.metadata.generation"));
                }
                if snapshot.seq() < seq {
                    return Err(invalid_category("postcommit.metadata.seq"));
                }
                if current.note != committed_note {
                    return Err(invalid_category("postcommit.metadata.note_afterimage"));
                }
                if note.usn.0 != -1 {
                    return Err(invalid_category("postcommit.metadata.usn"));
                }
                // Later unrelated commits may legitimately advance col.mod.
                // The original stamp remains owned by the retained OpResult;
                // an exact-seq snapshot must still match it exactly.
                if snapshot.seq() == seq && snapshot.col()?.modified != col_mod {
                    return Err(invalid_category("postcommit.metadata.collection_modified"));
                }
                if summary.changes.source != crate::changes::OpSource::UserUndoable {
                    return Err(invalid_category("postcommit.metadata.source"));
                }
                if summary.changes.notes.len() != 1 {
                    return Err(invalid_category("postcommit.metadata.note_count"));
                }
                if summary.changes.notes[0].id != self.baseline.note.id {
                    return Err(invalid_category("postcommit.metadata.note_identity"));
                }
                if summary.changes.notes[0].kind != crate::changes::ChangeKind::Updated {
                    return Err(invalid_category("postcommit.metadata.note_kind"));
                }
                if !summary.changes.cards.is_empty() {
                    return Err(invalid_category("postcommit.metadata.cards"));
                }
                let identity = snapshot.wal_identity();
                match collection.wait_wal_covered(identity, seq, durability_wait)? {
                    meguri_engine::WalCoverage::Covered(receipt)
                        if receipt.identity() == identity && receipt.through() >= seq => {}
                    meguri_engine::WalCoverage::Covered(receipt) => {
                        return Err(invalid_category(if receipt.identity() != identity {
                            "wal.coverage.identity"
                        } else {
                            "wal.coverage.through"
                        }));
                    }
                    _ => return Err(invalid_category("wal.coverage.not_covered")),
                }
                let digest = checked(c::Payload::Note(committed_note).payload_digest())?;
                let local_revision = if let Some(core) = retained.cloud_runtime() {
                    let status = checked(core.status())?;
                    match status.binding {
                        Some(binding) => {
                            let name = checked(c::entity_name(
                                &binding.facts.record_key.0,
                                c::Kind::Note,
                                &c::id_key(self.baseline.note.id),
                            ))?;
                            let entity = checked(core.entity_facts(&name))?
                                .ok_or_else(|| invalid_category("binding.entity_missing"))?;
                            if entity.local_rev == 0 {
                                return Err(invalid_category("revision.zero"));
                            }
                            if entity.tombstone_at.is_some() {
                                return Err(invalid_category("revision.tombstone"));
                            }
                            if entity.observed_generation != generation.0 {
                                return Err(invalid_category("revision.generation"));
                            }
                            if entity.observed_seq != seq.0 {
                                return Err(invalid_category("revision.seq"));
                            }
                            if checked(entity.payload.payload_digest())? != digest {
                                return Err(invalid_category("revision.payload_mismatch"));
                            }
                            Some(entity.local_rev)
                        }
                        None => None,
                    }
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
                    original_witness: if let Some(core) = retained.cloud_runtime() {
                        if local_revision.is_some() {
                            Some(
                                checked(
                                    core.original_edit_witness_for_testing(
                                        &collection,
                                        self.baseline.note.id,
                                        self.facts
                                            .baseline_digest
                                            .as_slice()
                                            .try_into()
                                            .map_err(|_| invalid())?,
                                        (identity, snapshot.seq()),
                                        seq,
                                        digest,
                                    ),
                                )?
                                .to_vec(),
                            )
                        } else {
                            None
                        }
                    } else {
                        None
                    },
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

// This carries historical fields explicitly; never construct CloudOriginalEditFacts
// with the new generation or claim that the new process executed Update Note.
#[derive(Clone, uniffi::Record)]
pub struct CloudHistoricalOriginalEdit {
    pub generation: u64,
    pub original_generation: u64,
    pub original_seq: u64,
    pub local_revision: u64,
    pub note_digest: Vec<u8>,
}
impl CloudOriginalRole {
    pub(crate) fn read_original_edit_inner(&self, witness: Vec<u8>) -> Result<CloudHistoricalOriginalEdit, CoreError> {
        let witness = witness.as_slice().try_into().map_err(|_| invalid())?;
        let host = runtime::host()?;
        let collection = host.cloud_connection_collection()?;
        let generation = collection.generation();
        // Re-establish this newly prepared baseline's current owned mapping.
        let owned = self.validate_owned_inner()?;
        if owned.generation != generation.0 || !owned.q_changed {
            return Err(invalid());
        }
        let core = host.cloud_runtime().ok_or_else(invalid)?;
        let historical = checked(
            core.read_original_edit_for_testing(
                &collection,
                self.baseline.note.id,
                self.facts
                    .baseline_digest
                    .as_slice()
                    .try_into()
                    .map_err(|_| invalid())?,
                witness,
            ),
        )?;
        // Current durability is independently checked; this receipt is NOT the
        // original operation's post-flush receipt.
        let wait = std::time::Duration::from_secs(5);
        collection.flush_wal(wait)?;
        let reader = collection.reader(ReaderRole::Query)?;
        let snapshot = reader.snapshot()?;
        let current = capture(&*snapshot, NoteId(self.baseline.note.id))?;
        let note = snapshot.note(NoteId(self.baseline.note.id))?.ok_or_else(invalid)?;
        let digest = checked(c::Payload::Note(current.note.clone()).payload_digest())?;
        let integrity = snapshot.smoke_integrity()?;
        if !self.baseline.compare(&current)?
            || note.usn.0 != -1
            || digest != historical.digest
            || !integrity.integrity_ok
            || !integrity.schema_ok
            || integrity.missing_card_notes != 0
            || integrity.missing_card_decks != 0
            || integrity.missing_note_types != 0
            || integrity.missing_field_types != 0
            || integrity.wrong_field_counts != 0
        {
            return Err(invalid());
        }
        match collection.wait_wal_covered(snapshot.wal_identity(), snapshot.seq(), wait)? {
            meguri_engine::WalCoverage::Covered(r)
                if r.identity() == snapshot.wal_identity() && r.through() >= snapshot.seq() => {}
            _ => return Err(invalid()),
        }
        let reread = checked(
            core.read_original_edit_for_testing(
                &collection,
                self.baseline.note.id,
                self.facts
                    .baseline_digest
                    .as_slice()
                    .try_into()
                    .map_err(|_| invalid())?,
                witness,
            ),
        )?;
        if reread.digest != digest
            || reread.original_seq != historical.original_seq
            || reread.original_generation != historical.original_generation
            || reread.local_revision != historical.local_revision
            || !Arc::ptr_eq(&runtime::host()?, &host)
            || !collection.is_current()
            || collection.last_seq() != snapshot.seq()
            || snapshot.generation() != generation
        {
            return Err(invalid());
        }
        Ok(CloudHistoricalOriginalEdit {
            generation: generation.0,
            original_generation: historical.original_generation,
            original_seq: historical.original_seq,
            local_revision: historical.local_revision,
            note_digest: digest.to_vec(),
        })
    }
}
