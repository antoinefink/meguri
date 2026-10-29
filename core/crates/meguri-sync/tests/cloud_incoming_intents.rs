//! Tests first, U113/U114: K12.3, K13.1-3; recovery V5/V2.4 step 8;
//! policy P24 / acceptance Q11 (refused versions and intent order); faults F5.
//! Starting commit: 9c983bea6396099912eae574dd30e1bebe103501.
//! Author log: one source consistency pass; no implementation or execution.
//! Source-only handoff. No runtime pass or P24/fullsync closure is claimed.
//!
//! Required future child observations (never a Rig-owned receive model):
//! - apply_intents: id, inbox_ids, keys {name, role, before_digest,
//!   after_digest, before_payload}, result_seq: Option<u64>.
//! - inbox_entries retains the existing flat reason projection and adds id,
//!   record_name, erev, retry_count. refused_entries reads real Store rows.
//! - incoming_observation: actual snapshot seq/file/generation, col_mod,
//!   undo_depth, change_events, current engine_instance; engine_state is the
//!   bound collection cursor projection of the actual saved serialization,
//!   last_fetch_engine_state is that projection from the actual fetch callback
//!   (neither includes unrelated root-zone progress). Incoming op results: runs
//!   {intent_id, seq, native_file, generation, entry_ids, changed_keys}, and
//!   actual completed writer wal_receipts {through, native_file, generation}
//!   and process-local native rollover results {seq}.
//!   Observing must neither flush nor supply/compute an expected result.
//! - deliver_collection_records forwards actual Service records to U114's
//!   Runtime receive boundary; drain_incoming grants bounded queued work.
//! - undo, add_note(mid, did, fields) -> {note, cards}, delete_note and
//!   bury_card forward native CoreHost services.
//! - read_cloud_review_hold(session) -> ReviewHold projects Collection's
//!   cloud_held_run(Some(session)), including sessionless Browse steps.
//! - pause_automatic_wal_flush_for_testing(bool) forwards Collection's existing
//!   hook. Explicit apply-worker wait_wal_covered still flushes the real WAL.
//! - binding_entity(binding, name) -> Option<EntityFacts> forwards the actual
//!   Store's entity_facts, including the old binding during publication.
//! - interrupt_next_incoming injects a one-shot real writer cancel/error/
//!   constraint/panic, allowing production rollback handling to run.
//! - arm_tracking_fault uses F5 tracking-commit errors; no apply error mode is
//!   attached to an F5 kill-only point.
//!
//! New noncrash Point latches gate actual child work: FetchBeforeInbox,
//! FetchAfterInbox, FetchAfterCursor, ApplyAfterIntent, ApplyAfterCommit,
//! ApplyAfterFlush, IncomingRollbackComplete, BeforeCollectionSave,
//! IntentSettlementComplete (the last
//! already required by the media replay tests). No new faultpoint names.
//! Structural-generation atomicity and other media resolutions remain separate
//! phases; this file does not replace the four immutable Q11 media replay tests.

#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::{
    ApplyFailure, BindingId, InboxReason,
    control_codec::Active,
    entity_codec::{self as codec, Payload},
    runtime::{KeyByteSource, KeySourceError},
};
use meguri_testkit::cloud::{
    Field, MutationSuccess, Record, Reply,
    child_driver::{ApplyInterruption, ApplyKeyRole, TrackingFault},
    coordinator::{Point, PublicationRig},
    devices::IdentityReads,
};
use std::{collections::BTreeSet, sync::Arc};
use support::*;

fn binding(pair: &Pair) -> BindingId {
    pair.receiver.facts().unwrap().current.unwrap().id
}

fn note(rig: &mut PublicationRig, id: i64) -> Payload {
    Payload::Note(notes(rig).into_iter().find(|n| n.id == id).unwrap())
}

fn name(pair: &Pair, kind: codec::Kind, id: i64) -> String {
    let key = pair.receiver.facts().unwrap().current.unwrap().facts.record_key;
    codec::entity_name(&key.0, kind, &codec::id_key(id)).unwrap()
}

fn saved(pair: &Pair, name: &str) -> Record {
    let zone = pair.sender.collection_zone().unwrap();
    pair.service.inspect(&scope()).unwrap().unwrap().zones[&zone].records[name].clone()
}

fn envelope(record: &Record) -> codec::Entity {
    let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
        panic!("envelope")
    };
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(bytes).unwrap() else {
        panic!("supported Service version")
    };
    entity
}

fn body(record: &Record) -> Payload {
    let entity = envelope(record);
    Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap()
}

struct NoReopenKeys;
impl KeyByteSource for NoReopenKeys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        Err(KeySourceError::Unavailable)
    }
}

fn held_note_send(pair: &mut Pair, id: i64, eligible: &[&str], kept: &[&str]) -> (Payload, Payload) {
    // Reopen the actually adopted files with auto enabled before any session
    // or edit. The reopen cannot supply a new binding or baseline.
    pair.receiver.close_clean().unwrap();
    pair.receiver = PublicationRig::open(
        &pair.receiver_dir,
        &scope(),
        IdentityReads::Available(scope().account),
        true,
        NOW,
        809,
        Arc::new(NoReopenKeys),
    )
    .unwrap();
    pair.receiver.hold(Point::BeforeCollectionSave).unwrap();
    pair.receiver.driver().edit_note_fields(id, eligible).unwrap();
    let sent = note(&mut pair.receiver, id);
    let session = pair.receiver.driver().review_begin(id).unwrap();
    let question = pair.receiver.driver().review_start(session).unwrap();
    pair.receiver.driver().review_show_answer(session, question.id).unwrap();
    let answer = pair.receiver.driver().review_answer_good(session, question.id).unwrap();
    pair.receiver.advance_clock_ms(61_001).unwrap();
    let first = pair.receiver.driver().edit_note_fields(id, kept).unwrap();
    let retained = note(&mut pair.receiver, id);
    pair.receiver.advance_clock_ms(61_001).unwrap();
    let newest = pair
        .receiver
        .driver()
        .edit_note_fields(id, &["held newest Q", "held newest A"])
        .unwrap();
    // These are separate sessionless edits in M8.1's run, never A3-merged.
    let hold = pair.receiver.driver().read_cloud_review_hold(session).unwrap();
    assert!(hold.op_ids.contains(&answer.op_id));
    assert!(hold.op_ids.contains(&first.seq));
    assert!(hold.op_ids.contains(&newest.seq));
    assert_ne!(retained.row_digest().unwrap(), sent.row_digest().unwrap());
    // P12.1 refuses Sync Now in a session. Grant the real automatic SDK lane;
    // its batch must send the held run's before-image and leave its edits free.
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeCollectionSave));
    let held_before_request = pair.receiver.driver().read_cloud_review_hold(session).unwrap();
    assert_eq!(held_before_request.op_ids, hold.op_ids);
    (sent, retained)
}

fn changed_note() -> (Pair, i64, String, Payload, Payload) {
    let mut pair = Pair::adopted();
    let id = notes(&mut pair.sender)[0].id;
    let key = name(&pair, codec::Kind::Note, id);
    let before = note(&mut pair.receiver, id);
    pair.sender.advance_clock_ms(7_000).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(id, &["remote Q", "remote A"])
        .unwrap();
    let after = note(&mut pair.sender, id);
    assert_ne!(before.row_digest().unwrap(), after.row_digest().unwrap());
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    (pair, id, key, before, after)
}

fn hold_receive(pair: &mut Pair, point: Point) {
    pair.receiver.hold(point).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(point));
}

fn settled(pair: &mut Pair, b: BindingId) {
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
}

#[test]
fn fetch_crash_cursor_never_outruns_durable_inbox_and_replay_precedes_send() {
    for point in [Point::FetchBeforeInbox, Point::FetchAfterInbox, Point::FetchAfterCursor] {
        let (mut pair, id, key, before, after) = changed_note();
        let b = binding(&pair);
        let initial = pair.receiver.driver().incoming_observation(b).unwrap();
        hold_receive(&mut pair, point);
        let held = pair.receiver.driver().incoming_observation(b).unwrap();
        let entries = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(note(&mut pair.receiver, id), before);
        if point == Point::FetchBeforeInbox {
            assert!(entries.is_empty());
        } else {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].record_name, key);
            assert_eq!(entries[0].erev, envelope(&saved(&pair, &key)).fields.erev);
        }
        if point == Point::FetchAfterCursor {
            assert_ne!(held.engine_state, initial.engine_state);
            assert_eq!(held.engine_state, held.last_fetch_engine_state);
        } else {
            assert_eq!(held.engine_state, initial.engine_state);
        }
        assert_eq!(pair.receiver.kill().unwrap().signal, Some(9));
        pair.reopen_before_identity();
        assert_eq!(note(&mut pair.receiver, id), before);
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), entries);
        assert_eq!(
            pair.receiver.driver().incoming_observation(b).unwrap().engine_state,
            held.engine_state
        );
        // A real local pending edit makes "before any send" non-vacuous.
        let other = notes(&mut pair.receiver).into_iter().find(|n| n.id != id).unwrap();
        pair.receiver
            .driver()
            .edit_note_fields(other.id, &["local pending", "back"])
            .unwrap();
        pair.receiver.hold(Point::BeforeCollectionSave).unwrap();
        pair.verify_and_drive();
        // Manual mode requires Sync Now even after fresh account verification.
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        assert!(pair.receiver.is_held(Point::BeforeCollectionSave));
        assert_eq!(note(&mut pair.receiver, id), after);
        settled(&mut pair, b);
        let replayed = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(replayed.runs.len(), 1, "one actual Incoming op after restart");
        pair.receiver.release(Point::BeforeCollectionSave).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        assert_eq!(note(&mut pair.receiver, id), after);
        assert_eq!(
            pair.receiver.driver().incoming_observation(b).unwrap().runs,
            replayed.runs
        );
        pair.close();
    }
}

#[test]
fn intent_precedes_commit_and_settle_requires_the_returned_seq_and_matching_real_wal() {
    let (mut pair, id, key, before, after) = changed_note();
    let b = binding(&pair);
    // Expectations are independently captured CoreHost rows, never intent data.
    let before_digest = before.row_digest().unwrap();
    let after_digest = after.row_digest().unwrap();
    let old_ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .pause_automatic_wal_flush_for_testing(true)
        .unwrap();
    hold_receive(&mut pair, Point::ApplyAfterIntent);
    let intents = pair.receiver.driver().apply_intents(b).unwrap();
    assert_eq!(intents.len(), 1);
    let intent = &intents[0];
    assert!(intent.result_seq.is_none());
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, key);
    assert_eq!(intent.keys[0].role, ApplyKeyRole::Entry);
    assert_eq!(intent.keys[0].before_digest, Some(before_digest));
    assert_eq!(intent.keys[0].after_digest, Some(after_digest));
    let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(intent.inbox_ids, inbox.iter().map(|e| e.id).collect::<Vec<_>>());
    assert_eq!(note(&mut pair.receiver, id), before);
    pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
    pair.receiver.release(Point::ApplyAfterIntent).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
    assert_eq!(note(&mut pair.receiver, id), after);
    let committed = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(committed.runs.len(), 1);
    let result = &committed.runs[0];
    assert_eq!(result.intent_id, intent.id);
    assert_eq!(result.entry_ids, intent.inbox_ids);
    assert_eq!(result.changed_keys, BTreeSet::from([key.clone()]));
    let pending = pair.receiver.driver().apply_intents(b).unwrap();
    assert_eq!(pending[0].result_seq, Some(result.seq));
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), inbox);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    assert!(
        !committed
            .wal_receipts
            .iter()
            .any(|r| r.native_file == result.native_file
                && r.generation == result.generation
                && r.through >= result.seq)
    );
    pair.receiver.hold(Point::ApplyAfterFlush).unwrap();
    pair.receiver.release(Point::ApplyAfterCommit).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterFlush));
    let flushed = pair.receiver.driver().incoming_observation(b).unwrap();
    assert!(
        flushed.wal_receipts.iter().any(|r| r.native_file == result.native_file
            && r.generation == result.generation
            && r.through >= result.seq)
    );
    pair.receiver
        .driver()
        .pause_automatic_wal_flush_for_testing(false)
        .unwrap();
    assert_eq!(pair.receiver.driver().apply_intents(b).unwrap(), pending);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), inbox);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.release(Point::ApplyAfterFlush).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    settled(&mut pair, b);
    let ack = &pair.receiver.facts().unwrap().entities[&key];
    assert_eq!(ack.acked_row_digest, after_digest);
    assert!(ack.acked_payload.as_ref() == Some(&after));
    assert!(ack.mark.is_none());
    pair.close();
}

#[test]
fn restarted_publication_settles_old_ordinary_intent_before_new_binding_scan() {
    let (mut pair, id, key, before, after) = changed_note();
    let old_binding = binding(&pair);
    let old_zone = pair.receiver.collection_zone().unwrap();
    let expected_rows = pair.sender.driver().collection_rows().unwrap();
    let incoming = saved(&pair, &key);
    let incoming_entity = envelope(&incoming);
    // Preview/confirm use the public Runtime workflow, without binding seeding.
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), before);
    hold_receive(&mut pair, Point::ApplyAfterIntent);
    let pending = pair.receiver.driver().apply_intents(old_binding).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].result_seq.is_none());
    assert_eq!(pending[0].keys.len(), 1);
    assert_eq!(pending[0].keys[0].role, ApplyKeyRole::Entry);
    assert_eq!(pending[0].keys[0].before_digest, Some(before.row_digest().unwrap()));
    assert_eq!(pending[0].keys[0].after_digest, Some(after.row_digest().unwrap()));
    let entries = pair.receiver.driver().inbox_entries(old_binding).unwrap();
    assert!(!entries.is_empty());
    assert_eq!(
        pending[0].inbox_ids,
        entries.iter().map(|entry| entry.id).collect::<Vec<_>>()
    );
    pair.receiver.confirm_publication(&preview).unwrap();
    assert_eq!(pair.receiver.facts().unwrap().recovery_ops.len(), 1);
    assert_eq!(pair.receiver.kill().unwrap().signal, Some(9));
    pair.reopen_before_identity();
    assert_eq!(note(&mut pair.receiver, id), before);
    assert_eq!(pair.receiver.driver().apply_intents(old_binding).unwrap(), pending);
    assert_eq!(pair.receiver.driver().inbox_entries(old_binding).unwrap(), entries);

    pair.receiver.hold(Point::BeforeScan).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::BeforeScan));
    let scanning = pair.receiver.facts().unwrap();
    let replacement = scanning.current.as_ref().unwrap();
    assert_ne!(replacement.id, old_binding);
    assert_eq!(scanning.recovery_ops.len(), 1);
    assert!(scanning.bindings.iter().any(|b| b.id == old_binding));
    settled(&mut pair, old_binding);
    let old_entity = pair
        .receiver
        .driver()
        .binding_entity(old_binding, &key)
        .unwrap()
        .unwrap();
    assert_eq!(old_entity.server_erev, incoming_entity.fields.erev);
    assert_eq!(old_entity.acked_system_fields, incoming.system_fields.bytes());
    assert_eq!(old_entity.acked_row_digest, after.row_digest().unwrap());
    assert_eq!(old_entity.acked_payload_digest, incoming_entity.payload_sha256);
    assert!(old_entity.acked_payload.as_ref() == Some(&after));
    assert!(old_entity.mark.is_none());
    assert_eq!(note(&mut pair.receiver, id), after);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected_rows);
    let replay = pair.receiver.driver().incoming_observation(old_binding).unwrap();
    assert_eq!(replay.runs.len(), 1);
    // A new apply attempt has a new intent but the same input entries.
    assert_eq!(replay.runs[0].entry_ids, pending[0].inbox_ids);
    assert_eq!(replay.runs[0].changed_keys, BTreeSet::from([key]));
    assert!(replay.wal_receipts.iter().any(|receipt| {
        receipt.native_file == replay.runs[0].native_file
            && receipt.generation == replay.runs[0].generation
            && receipt.through >= replay.runs[0].seq
    }));
    let backup = pair.receiver.driver().backup_now().unwrap();
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        expected_rows
    );
    pair.receiver.release(Point::BeforeScan).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let completed = pair.receiver.facts().unwrap();
    assert!(completed.recovery_ops.is_empty());
    assert!(completed.bindings.iter().all(|b| b.id != old_binding));
    assert_eq!(completed.current.as_ref().unwrap().id, replacement.id);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected_rows);
    let zone = pair.receiver.collection_zone().unwrap();
    assert_ne!(zone, old_zone);
    let server = pair.service.inspect(&scope()).unwrap().unwrap();
    let Field::EncryptedBytes(active) = &server.zones["meguri-root"].records["active"].fields["e"] else {
        panic!("actual published Active")
    };
    assert_eq!(Active::decode(active).unwrap().epoch, replacement.facts.epoch.0);
    let published: Vec<_> = server.zones[&zone]
        .records
        .values()
        .filter(|record| record.record_type == "MGEntity")
        .map(body)
        .collect();
    assert_eq!(published.len(), expected_rows.len());
    assert!(expected_rows.iter().all(|row| published.contains(row)));
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected_rows);
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        expected_rows
    );
    pair.close();
}

#[test]
fn restart_decides_all_before_all_after_and_mixed_keys_and_preserves_intervening_edit() {
    for (point, edit_key) in [
        (Point::ApplyAfterIntent, false),
        (Point::ApplyAfterCommit, false),
        (Point::ApplyAfterFlush, false),
        (Point::ApplyAfterCommit, true),
    ] {
        let (mut pair, id, _, before, after) = changed_note();
        let b = binding(&pair);
        // Two independently changed keys make the mixed case a real mix.
        let other = notes(&mut pair.sender).into_iter().find(|n| n.id != id).unwrap();
        pair.sender
            .driver()
            .edit_note_fields(other.id, &["second remote", "remote back"])
            .unwrap();
        let other_after = note(&mut pair.sender, other.id);
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        hold_receive(&mut pair, point);
        let intents = pair.receiver.driver().apply_intents(b).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].keys.len(), 2);
        assert_eq!(
            note(&mut pair.receiver, id),
            if point == Point::ApplyAfterIntent {
                before
            } else {
                after
            }
        );
        pair.receiver.kill().unwrap();
        pair.reopen_before_identity();
        if edit_key {
            pair.receiver.advance_clock_ms(30_000).unwrap();
            pair.receiver
                .driver()
                .edit_note_fields(id, &["remote Q", "intervening local A"])
                .unwrap();
        }
        let intervening = note(&mut pair.receiver, id);
        pair.verify_and_drive();
        settled(&mut pair, b);
        assert_eq!(note(&mut pair.receiver, other.id), other_after);
        if edit_key {
            assert_eq!(note(&mut pair.receiver, id), intervening);
        }
        let replay = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(
            replay.runs.len(),
            usize::from(point == Point::ApplyAfterIntent || edit_key)
        );
        let rows = pair.receiver.driver().collection_rows().unwrap();
        pair.receiver.kill().unwrap();
        pair.reopen_before_identity();
        pair.verify_and_drive();
        settled(&mut pair, b);
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        assert!(pair.receiver.driver().incoming_observation(b).unwrap().runs.is_empty());
        pair.close();
    }
}

#[test]
fn erev_order_ignores_duplicates_and_older_accepted_versions_cannot_clear_newer_refusal() {
    let (mut pair, id, key, before, first_row) = changed_note();
    let b = binding(&pair);
    let first = saved(&pair, &key);
    pair.sender
        .driver()
        .edit_note_fields(id, &["newest Q", "newest A"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let newest = saved(&pair, &key);
    let newest_row = note(&mut pair.sender, id);
    assert!(envelope(&newest).fields.erev > envelope(&first).fields.erev);
    // Transport delivery order is adversarial; all versions are real Service
    // saves. This entry is intent-owned when the later version arrives.
    pair.receiver.hold(Point::ApplyAfterIntent).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(&[first.clone()])
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterIntent));
    pair.receiver
        .driver()
        .deliver_collection_records(&[newest.clone(), first.clone(), newest.clone()])
        .unwrap();
    let entries = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.erev).collect::<Vec<_>>(),
        vec![envelope(&first).fields.erev, envelope(&newest).fields.erev]
    );
    assert_eq!(note(&mut pair.receiver, id), before);
    pair.receiver.release(Point::ApplyAfterIntent).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), newest_row);
    let runs = pair.receiver.driver().incoming_observation(b).unwrap().runs;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].entry_ids, vec![entries[0].id]);
    assert_eq!(runs[1].entry_ids, vec![entries[1].id]);
    assert_ne!(first_row, newest_row);
    settled(&mut pair, b);
    pair.close();

    let (mut pair, id, key, before, _) = changed_note();
    let b = binding(&pair);
    let older = saved(&pair, &key);
    // A real Service save with a K20-invalid, same-origin higher-erev payload.
    // Codec accepts bytes; receive must refuse the malformed Note payload.
    let mut fields = envelope(&older).fields;
    fields.erev += 1;
    let invalid = codec::Entity::build(fields.clone(), b"invalid note payload").unwrap();
    let response = pair
        .service
        .request(
            &scope(),
            Default::default(),
            meguri_testkit::cloud::Operation::Modify {
                zone: pair.sender.collection_zone().unwrap(),
                atomic: false,
                changes: vec![meguri_testkit::cloud::Mutation::Save {
                    record: meguri_testkit::cloud::RecordInput::encrypted(&key, "MGEntity", invalid.encode().unwrap()),
                    if_unchanged: Some(older.system_fields.clone()),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("real modify")
    };
    let Ok(MutationSuccess::Saved(refused)) = &items[0].result else {
        panic!("real save")
    };
    pair.receiver
        .driver()
        .deliver_collection_records(&[refused.clone(), older.clone(), older])
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), before);
    let refusals = pair.receiver.driver().refused_entries(b).unwrap();
    assert_eq!(refusals.len(), 1);
    assert_eq!(refusals[0].record_name, key);
    assert_eq!(refusals[0].erev, fields.erev);
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
    // Repair through Service, preserving origin and increasing erev again.
    fields.erev += 1;
    let repaired_row = note(&mut pair.sender, id);
    let repaired = codec::Entity::build(fields, &repaired_row.encode().unwrap()).unwrap();
    let response = pair
        .service
        .request(
            &scope(),
            Default::default(),
            meguri_testkit::cloud::Operation::Modify {
                zone: pair.sender.collection_zone().unwrap(),
                atomic: false,
                changes: vec![meguri_testkit::cloud::Mutation::Save {
                    record: meguri_testkit::cloud::RecordInput::encrypted(&key, "MGEntity", repaired.encode().unwrap()),
                    if_unchanged: Some(refused.system_fields.clone()),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("real modify")
    };
    let Ok(MutationSuccess::Saved(repaired)) = &items[0].result else {
        panic!("real repair")
    };
    pair.receiver
        .driver()
        .deliver_collection_records(&[repaired.clone()])
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.driver().refused_entries(b).unwrap().is_empty());
    assert_eq!(note(&mut pair.receiver, id), repaired_row);
    settled(&mut pair, b);
    pair.close();
}

#[test]
fn own_landed_save_is_noop_even_after_undo_or_edit_and_retains_a_newer_flight() {
    for action in ["same", "undo", "edit"] {
        let mut pair = Pair::adopted();
        let b = binding(&pair);
        let id = notes(&mut pair.receiver)[0].id;
        let key = name(&pair, codec::Kind::Note, id);
        let (landed_row, undo_row) = if action == "undo" {
            let (sent, kept) = held_note_send(&mut pair, id, &["lost Q", "back"], &["kept Q", "back"]);
            (sent, Some(kept))
        } else {
            pair.receiver
                .driver()
                .edit_note_fields(id, &["lost Q", "back"])
                .unwrap();
            let sent = note(&mut pair.receiver, id);
            hold_receive(&mut pair, Point::BeforeCollectionSave);
            (sent, None)
        };
        let request = pair.receiver.held_request().unwrap();
        let submitted = pair.receiver.facts().unwrap().entities[&key].clone();
        let response = pair
            .service
            .request(&scope(), request.caller, request.operation)
            .unwrap();
        let Ok(Reply::Modified(items)) = &response.result else {
            panic!("actual Service reply")
        };
        let Ok(MutationSuccess::Saved(landed)) = &items.iter().find(|i| i.name == key).unwrap().result else {
            panic!("save landed; reply will never reach child")
        };
        let entity = envelope(landed);
        assert_eq!(entity.fields.writer, b.0);
        assert_eq!(entity.fields.origin, submitted.origin);
        assert_eq!(entity.payload_sha256, landed_row.payload_digest().unwrap());
        assert_eq!(body(landed), landed_row);
        assert!(
            submitted
                .in_flight
                .iter()
                .any(|f| f.payload_digest == entity.payload_sha256)
        );
        let landed_revision = submitted
            .in_flight
            .iter()
            .find(|f| f.payload_digest == entity.payload_sha256)
            .unwrap()
            .local_rev;
        match action {
            "undo" => {
                pair.receiver.driver().undo().unwrap();
            }
            "edit" => {
                pair.receiver
                    .driver()
                    .edit_note_fields(id, &["newer Q", "back"])
                    .unwrap();
            }
            _ => {}
        }
        let current = note(&mut pair.receiver, id);
        if action == "undo" {
            assert_eq!(current, undo_row.unwrap());
            assert_ne!(current.row_digest().unwrap(), landed_row.row_digest().unwrap());
        }
        // Exercise the actual batch builder to persist a newer unsaved flight;
        // no made-up sent/acked rows and no fabricated Service response.
        if action == "edit" {
            let state = pair.receiver.driver().incoming_observation(b).unwrap();
            let batch = pair
                .receiver
                .driver()
                .build_for_instance(state.engine_instance.unwrap(), BTreeSet::from([key.clone()]))
                .unwrap();
            assert!(batch.saves.iter().any(|s| s.name == key));
        }
        let flights = pair.receiver.facts().unwrap().entities[&key].in_flight.clone();
        pair.receiver.kill().unwrap(); // real lost reply
        pair.reopen_before_identity();
        let before_apply = pair.receiver.driver().incoming_observation(b).unwrap();
        let current_rows = pair.receiver.driver().collection_rows().unwrap();
        pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
        pair.verify_and_drive();
        // Manual mode needs an explicit fetch of the save whose reply was lost.
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
        assert_eq!(note(&mut pair.receiver, id), current);
        let state = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(state.col_mod, before_apply.col_mod);
        assert_eq!(state.undo_depth, before_apply.undo_depth);
        assert_eq!(state.change_events, before_apply.change_events);
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), current_rows);
        let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
        assert_eq!(intent.keys.len(), 1);
        assert_eq!(intent.keys[0].role, ApplyKeyRole::Entry);
        assert_eq!(intent.keys[0].before_digest, Some(current.row_digest().unwrap()));
        assert_eq!(intent.keys[0].after_digest, Some(current.row_digest().unwrap()));
        assert_eq!(state.runs.len(), 1);
        assert!(state.runs[0].changed_keys.is_empty());
        assert_eq!(intent.result_seq, Some(state.runs[0].seq));
        assert!(pair.receiver.facts().unwrap().entities[&key].in_flight == flights);
        // A no-op still has a result seq, but no mtime, undo or event changes.
        let before_settle = state;
        pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
        pair.receiver.release(Point::ApplyAfterCommit).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
        let settled_state = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(settled_state.col_mod, before_settle.col_mod);
        assert_eq!(settled_state.undo_depth, before_settle.undo_depth);
        assert_eq!(settled_state.change_events, before_settle.change_events);
        assert!(
            settled_state
                .wal_receipts
                .iter()
                .any(|r| r.through >= before_settle.runs[0].seq
                    && r.native_file == before_settle.runs[0].native_file
                    && r.generation == before_settle.runs[0].generation)
        );
        settled(&mut pair, b);
        let ack = pair.receiver.facts().unwrap().entities[&key].clone();
        assert_eq!(ack.server_erev, entity.fields.erev);
        assert_eq!(ack.acked_system_fields, landed.system_fields.bytes());
        assert_eq!(ack.acked_payload_digest, entity.payload_sha256);
        assert!(ack.acked_payload.as_ref() == Some(&landed_row));
        let remaining: Vec<_> = flights.into_iter().filter(|f| f.local_rev > landed_revision).collect();
        assert!(ack.in_flight == remaining);
        if action == "same" {
            assert!(ack.mark.is_none());
        } else {
            if action == "edit" {
                assert!(!ack.in_flight.is_empty());
            }
            assert!(ack.mark.is_some());
            assert!(ack.base_payload.as_ref() == Some(&landed_row));
        }
        pair.close();
    }
}

#[test]
fn another_writer_with_the_same_payload_uses_ordinary_apply_not_own_save_ack() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let baseline = notes(&mut pair.receiver)[0].clone();
    let id = baseline.id;
    let key = name(&pair, codec::Kind::Note, id);
    let original_fields: Vec<_> = baseline
        .flds
        .split(|byte| *byte == 0x1f)
        .map(|field| std::str::from_utf8(field).unwrap())
        .collect();
    // Keep L's parts equal to the actual acked baseline. K15.1 must then take
    // S whole despite this held edit's later stamp, making the apply nonempty.
    let (pending, retained) = held_note_send(&mut pair, id, &["equal Q", "equal A"], &original_fields);
    // Building the original real request persists its flight; do not send it.
    let _not_landed = pair.receiver.held_request().unwrap();
    let flight = pair.receiver.facts().unwrap().entities[&key].in_flight.clone();
    let origin = pair.receiver.facts().unwrap().entities[&key].origin;
    assert!(
        flight
            .iter()
            .any(|f| f.payload_digest == pending.payload_digest().unwrap())
    );
    pair.receiver.driver().undo().unwrap();
    let undone = note(&mut pair.receiver, id);
    assert_eq!(undone, retained);
    assert_ne!(undone.row_digest().unwrap(), pending.row_digest().unwrap());
    pair.receiver.kill().unwrap();
    pair.sender
        .driver()
        .edit_note_fields(id, &["equal Q", "equal A"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let remote = saved(&pair, &key);
    let entity = envelope(&remote);
    assert_ne!(entity.fields.writer, b.0);
    assert_eq!(entity.fields.origin, origin);
    assert_eq!(entity.payload_sha256, pending.payload_digest().unwrap());
    assert_eq!(body(&remote), pending);
    pair.reopen_before_identity();
    pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
    assert_ne!(note(&mut pair.receiver, id), undone);
    assert_eq!(note(&mut pair.receiver, id), note(&mut pair.sender, id));
    let state = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(state.runs.len(), 1);
    assert_eq!(state.runs[0].changed_keys, BTreeSet::from([key]));
    pair.receiver.release(Point::ApplyAfterCommit).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    settled(&mut pair, b);
    pair.close();
}

#[test]
fn remote_built_on_a_lost_save_uses_its_flight_as_base_and_keeps_local_undo() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let baseline = notes(&mut pair.receiver)[0].clone();
    let id = baseline.id;
    let key = name(&pair, codec::Kind::Note, id);
    let original_q = baseline.flds.split(|b| *b == 0x1f).next().unwrap();
    let (lost, retained) = held_note_send(
        &mut pair,
        id,
        &["lost Q", "back"],
        &[std::str::from_utf8(original_q).unwrap(), "back"],
    );
    let request = pair.receiver.held_request().unwrap();
    let submitted = pair.receiver.facts().unwrap().entities[&key].clone();
    let response = pair
        .service
        .request(&scope(), request.caller, request.operation)
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual save")
    };
    let Ok(MutationSuccess::Saved(landed)) = &items.iter().find(|i| i.name == key).unwrap().result else {
        panic!("save landed; reply will never reach child")
    };
    let entity = envelope(landed);
    assert_eq!(body(landed), lost);
    assert_eq!(entity.fields.writer, b.0);
    assert_eq!(entity.fields.origin, submitted.origin);
    assert_eq!(entity.payload_sha256, lost.payload_digest().unwrap());
    assert!(
        submitted
            .in_flight
            .iter()
            .any(|f| f.payload_digest == entity.payload_sha256)
    );
    pair.receiver.driver().undo().unwrap();
    assert_eq!(note(&mut pair.receiver, id), retained);
    assert_ne!(retained.row_digest().unwrap(), lost.row_digest().unwrap());
    pair.receiver.kill().unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(note(&mut pair.sender, id), lost);
    pair.sender
        .driver()
        .edit_note_fields(id, &["lost Q", "other A"])
        .unwrap();
    let remote_row = note(&mut pair.sender, id);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let remote = saved(&pair, &key);
    assert_eq!(
        envelope(&remote).fields.base_sha256,
        Some(lost.payload_digest().unwrap())
    );
    pair.reopen_before_identity();
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    let merged = notes(&mut pair.receiver).into_iter().find(|n| n.id == id).unwrap();
    assert_eq!(merged.flds.split(|b| *b == 0x1f).next().unwrap(), original_q);
    assert!(merged.flds.ends_with(b"\x1fother A"));
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    assert!(ack.mark.is_some());
    assert!(ack.base_payload.as_ref() == Some(&remote_row));
    assert!(ack.in_flight.is_empty());
    settled(&mut pair, b);
    pair.close();
}

#[test]
fn actual_child_before_parent_waits_without_a_partial_graph() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let template = notes(&mut pair.sender)[0].clone();
    let rows = pair.sender.driver().collection_rows().unwrap();
    let did = rows
        .iter()
        .find_map(|p| match p {
            Payload::Card(c) if c.nid == template.id => Some(c.did),
            _ => None,
        })
        .unwrap();
    let added = pair
        .sender
        .driver()
        .add_note(template.mid, did, &["new parent", "new back"])
        .unwrap();
    assert_eq!(added.cards.len(), 1);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let card_key = name(&pair, codec::Kind::Card, added.cards[0].id);
    let note_key = name(&pair, codec::Kind::Note, added.note.id);
    let child = saved(&pair, &card_key);
    let parent = saved(&pair, &note_key);
    let before = pair.receiver.driver().collection_rows().unwrap();
    pair.receiver.driver().deliver_collection_records(&[child]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);
    let entries = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].record_name, card_key);
    assert_eq!(entries[0].reason, InboxReason::Reference);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.receiver.driver().deliver_collection_records(&[parent]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(notes(&mut pair.receiver).contains(&added.note));
    assert_eq!(
        pair.receiver.driver().read_card(added.cards[0].id).unwrap(),
        added.cards[0]
    );
    settled(&mut pair, b);
    pair.close();
}

#[test]
fn ordinary_ops_take_at_most_500_actual_entries_without_losing_the_tail() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let template = notes(&mut pair.sender)[0].clone();
    let did = pair
        .sender
        .driver()
        .collection_rows()
        .unwrap()
        .iter()
        .find_map(|p| match p {
            Payload::Card(c) if c.nid == template.id => Some(c.did),
            _ => None,
        })
        .unwrap();
    let mut names = Vec::new();
    for i in 0..251 {
        let added = pair
            .sender
            .driver()
            .add_note(template.mid, did, &[&format!("budget {i}"), "back"])
            .unwrap();
        assert_eq!(added.cards.len(), 1);
        names.push(name(&pair, codec::Kind::Note, added.note.id));
        names.push(name(&pair, codec::Kind::Card, added.cards[0].id));
    }
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let records: Vec<_> = names.iter().map(|n| saved(&pair, n)).collect();
    assert_eq!(records.len(), 502);
    pair.receiver.hold(Point::ApplyAfterIntent).unwrap();
    pair.receiver.driver().deliver_collection_records(&records).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterIntent));
    let entries = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(entries.len(), 502);
    let ids: BTreeSet<_> = entries.iter().map(|e| e.id).collect();
    let intents = pair.receiver.driver().apply_intents(b).unwrap();
    assert_eq!(intents.len(), 1);
    assert!(!intents[0].inbox_ids.is_empty());
    assert!(intents[0].inbox_ids.len() <= 500);
    pair.receiver.release(Point::ApplyAfterIntent).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let runs = pair.receiver.driver().incoming_observation(b).unwrap().runs;
    assert!(runs.len() >= 2);
    assert!(runs.iter().all(|r| !r.entry_ids.is_empty() && r.entry_ids.len() <= 500));
    let applied: Vec<_> = runs.iter().flat_map(|r| r.entry_ids.iter().copied()).collect();
    assert_eq!(applied.len(), 502);
    assert_eq!(applied.into_iter().collect::<BTreeSet<_>>(), ids);
    // Native note creation also changes Config. First prove that the 502
    // delivered Note/Card records account for every non-Config row exactly.
    use std::collections::BTreeMap;
    let record_key = pair.receiver.facts().unwrap().current.unwrap().facts.record_key;
    let keyed_rows = |rows: Vec<Payload>| {
        rows.into_iter()
            .map(|row| {
                let key = meguri_sync::cloud::row_payload::payload_key(&row).unwrap();
                let name = codec::entity_name(&record_key.0, row.kind(), &key).unwrap();
                (name, row)
            })
            .collect::<BTreeMap<_, _>>()
    };
    let (sender_config, sender_other): (BTreeMap<_, _>, BTreeMap<_, _>) =
        keyed_rows(pair.sender.driver().collection_rows().unwrap())
            .into_iter()
            .partition(|(_, row)| matches!(row, Payload::Config(_)));
    let (receiver_config, receiver_other): (BTreeMap<_, _>, BTreeMap<_, _>) =
        keyed_rows(pair.receiver.driver().collection_rows().unwrap())
            .into_iter()
            .partition(|(_, row)| matches!(row, Payload::Config(_)));
    assert!(
        sender_other == receiver_other,
        "non-Config row mismatches: {:?}",
        sender_other
            .keys()
            .chain(receiver_other.keys())
            .filter(|key| sender_other.get(*key) != receiver_other.get(*key))
            .collect::<BTreeSet<_>>()
    );
    let changed_config: BTreeSet<_> = sender_config
        .keys()
        .chain(receiver_config.keys())
        .filter(|key| sender_config.get(*key) != receiver_config.get(*key))
        .cloned()
        .collect();
    assert!(
        !changed_config.is_empty(),
        "expected an actual ancillary Config difference"
    );
    // Transfer the actual saved Config images from the existing sender sync,
    // through the same receive/apply boundary, before comparing all rows.
    let config_records: Vec<_> = changed_config
        .iter()
        .map(|key| {
            let record = saved(&pair, key);
            assert!(
                sender_config.get(key) == Some(&body(&record)),
                "Service Config differs from sender at key {key}"
            );
            record
        })
        .collect();
    pair.receiver
        .driver()
        .deliver_collection_records(&config_records)
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(
        pair.receiver.driver().collection_rows().unwrap(),
        pair.sender.driver().collection_rows().unwrap()
    );
    settled(&mut pair, b);
    pair.close();
}

#[test]
fn day_rollover_commits_before_ordinary_incoming_and_startup_replay() {
    for replay in [false, true] {
        let (mut pair, id, _, _, after) = changed_note();
        let b = binding(&pair);
        let card = pair
            .receiver
            .driver()
            .collection_rows()
            .unwrap()
            .into_iter()
            .find_map(|p| match p {
                Payload::Card(c) if c.nid == id => Some(c),
                _ => None,
            })
            .unwrap();
        pair.receiver.driver().bury_card(card.id).unwrap();
        let buried_queue = pair.receiver.driver().read_card(card.id).unwrap().queue;
        assert!(buried_queue < -1);
        if replay {
            hold_receive(&mut pair, Point::ApplyAfterIntent);
            pair.receiver.kill().unwrap();
            pair.reopen_before_identity();
        }
        pair.receiver.advance_clock_ms(86_400_000).unwrap();
        assert_eq!(pair.receiver.driver().read_card(card.id).unwrap().queue, buried_queue);
        if replay {
            pair.verify_and_drive();
        } else {
            pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        }
        assert!(pair.receiver.driver().read_card(card.id).unwrap().queue >= 0);
        assert_eq!(note(&mut pair.receiver, id), after);
        let state = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(state.rollovers.len(), 1);
        assert_eq!(state.runs.len(), 1);
        assert!(state.rollovers[0].seq < state.runs[0].seq);
        settled(&mut pair, b);
        pair.close();
    }
}

#[test]
fn clean_side_effect_card_keeps_its_actual_before_image_as_pending_tombstone_base() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let id = notes(&mut pair.receiver)[0].id;
    let note_key = name(&pair, codec::Kind::Note, id);
    let rows = pair.receiver.driver().collection_rows().unwrap();
    let cards: Vec<_> = rows
        .iter()
        .filter_map(|p| match p {
            Payload::Card(c) if c.nid == id => Some(c.clone()),
            _ => None,
        })
        .collect();
    assert!(!cards.is_empty());
    for card in &cards {
        assert!(
            pair.receiver.facts().unwrap().entities[&name(&pair, codec::Kind::Card, card.id)]
                .mark
                .is_none()
        );
    }
    pair.sender.driver().delete_note(id).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    // Deliver only the actual note tombstone: the native apply must delete
    // its cards, so they are side effects rather than inbox entry keys.
    let tombstone = saved(&pair, &note_key);
    pair.receiver.hold(Point::ApplyAfterIntent).unwrap();
    pair.receiver.driver().deliver_collection_records(&[tombstone]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterIntent));
    let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
    assert_eq!(intent.keys.len(), cards.len() + 1);
    assert_eq!(
        intent.keys.iter().find(|k| k.name == note_key).unwrap().role,
        ApplyKeyRole::Entry
    );
    for card in &cards {
        let key = name(&pair, codec::Kind::Card, card.id);
        let effect = intent.keys.iter().find(|k| k.name == key).unwrap();
        let before = Payload::Card(card.clone());
        assert_eq!(effect.role, ApplyKeyRole::SideEffect);
        assert_eq!(effect.before_digest, Some(before.row_digest().unwrap()));
        assert_eq!(effect.after_digest, None);
        assert!(effect.before_payload.as_ref() == Some(&before));
        assert_eq!(pair.receiver.driver().read_card(card.id).unwrap(), *card);
    }
    pair.receiver.release(Point::ApplyAfterIntent).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    settled(&mut pair, b);
    let after = pair.receiver.driver().collection_rows().unwrap();
    assert!(
        !after
            .iter()
            .any(|p| matches!(p, Payload::Note(n) if n.id == id) || matches!(p, Payload::Card(c) if c.nid == id))
    );
    for card in cards {
        let entity = pair.receiver.facts().unwrap().entities[&name(&pair, codec::Kind::Card, card.id)].clone();
        assert!(entity.mark.is_some());
        assert!(entity.base_payload == Some(Payload::Card(card)));
    }
    pair.close();
}

#[test]
fn incoming_op_interruptions_preserve_rows_and_bound_retries() {
    for interruption in [
        ApplyInterruption::Cancel,
        ApplyInterruption::Error,
        ApplyInterruption::Constraint,
        ApplyInterruption::Panic,
    ] {
        let (mut pair, id, _, before, after) = changed_note();
        let b = binding(&pair);
        pair.receiver.hold(Point::IncomingRollbackComplete).unwrap();
        pair.receiver.driver().interrupt_next_incoming(interruption).unwrap();
        let _ = pair.receiver.sync_now_and_drain(&mut pair.service, STEPS);
        assert!(pair.receiver.is_held(Point::IncomingRollbackComplete));
        assert_eq!(note(&mut pair.receiver, id), before);
        assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
        let entries = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(entries.len(), 1);
        pair.receiver.release(Point::IncomingRollbackComplete).unwrap();
        if interruption == ApplyInterruption::Cancel {
            assert_eq!(entries[0].reason, InboxReason::Ready);
            assert_eq!(entries[0].retry_count, 0);
        } else {
            assert!(matches!(
                entries[0].reason,
                InboxReason::ApplyFailed {
                    failure: ApplyFailure::Apply,
                    ..
                }
            ));
            assert_eq!(entries[0].retry_count, 1);
            pair.receiver.advance_clock_ms(3_600_000).unwrap();
            pair.receiver.driver().drain_incoming(STEPS).unwrap();
            assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), entries);
            assert_eq!(note(&mut pair.receiver, id), before);
        }
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        assert_eq!(note(&mut pair.receiver, id), after);
        settled(&mut pair, b);
        pair.close();
    }
}

#[test]
fn failed_inbox_commit_keeps_cursor_and_collection_behind_the_batch() {
    for fault in [
        TrackingFault::Io,
        TrackingFault::NoSpace,
        TrackingFault::Constraint,
        TrackingFault::Panic,
    ] {
        let (mut pair, id, _, before, after) = changed_note();
        let b = binding(&pair);
        let initial = pair.receiver.driver().incoming_observation(b).unwrap();
        hold_receive(&mut pair, Point::FetchBeforeInbox);
        pair.receiver
            .driver()
            .arm_tracking_fault("cloud.tracking.commit.inbox", fault)
            .unwrap();
        pair.receiver.release(Point::FetchBeforeInbox).unwrap();
        // The bounded driver may report the expected stop as an error.
        let _ = pair.receiver.drive_ready(&mut pair.service, STEPS);
        assert_eq!(note(&mut pair.receiver, id), before);
        assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
        assert_eq!(
            pair.receiver.driver().incoming_observation(b).unwrap().engine_state,
            initial.engine_state
        );
        pair.receiver.advance_clock_ms(3_600_000).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        assert_eq!(note(&mut pair.receiver, id), before);
        assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
        assert_eq!(
            pair.receiver.driver().incoming_observation(b).unwrap().engine_state,
            initial.engine_state
        );
        pair.receiver.kill().unwrap();
        pair.reopen_before_identity();
        assert_eq!(note(&mut pair.receiver, id), before);
        pair.verify_and_drive();
        // With no durable inbox entry, manual mode needs an explicit fetch.
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        assert_eq!(note(&mut pair.receiver, id), after);
        settled(&mut pair, b);
        pair.close();
    }
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
