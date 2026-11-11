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
    child_driver::{ApplyInterruption, ApplyKeyRole, FetchedEnvelopeFault, IncomingProgress, TrackingFault},
    coordinator::{Point, PublicationRig},
    devices::IdentityReads,
};
use std::{collections::BTreeSet, io::Write, sync::Arc};
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
    let bytes = entity.payload.as_deref().unwrap_or_else(|| match &record.fields["pa"] {
        Field::Asset(asset) => asset.bytes(),
        _ => panic!("Service payload asset"),
    });
    assert_eq!(codec::sha256(bytes), entity.payload_sha256);
    Payload::decode(entity.fields.kind, bytes).unwrap()
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

fn changed_asset_note() -> (Pair, i64, String, Payload, Payload) {
    let mut pair = Pair::adopted();
    let id = notes(&mut pair.sender)[0].id;
    let key = name(&pair, codec::Kind::Note, id);
    let before = note(&mut pair.receiver, id);
    pair.sender.advance_clock_ms(7_000).unwrap();
    let field = "r".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(id, &[&field, "remote asset A"])
        .unwrap();
    let after = note(&mut pair.sender, id);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let record = saved(&pair, &key);
    assert!(envelope(&record).payload.is_none());
    assert!(envelope(&record).fields.generation.is_none());
    assert_eq!(body(&record), after);
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
fn fetched_envelope_spool_failure_ends_original_exchange_without_intake_or_writer_changes() {
    use meguri_sync::cloud::adapter::CollectionOutcome;

    for fault in [FetchedEnvelopeFault::Missing, FetchedEnvelopeFault::Short] {
        let (mut pair, _, _, _, _) = changed_asset_note();
        let b = binding(&pair);
        let initial = pair.receiver.driver().incoming_observation(b).unwrap();
        let (drains_before, missing_before) = pair.receiver.driver().fetch_lifecycle_for_testing().unwrap();
        let rows = pair.receiver.driver().collection_rows().unwrap();
        let entities = pair.receiver.facts().unwrap().entities;
        let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
        let intents = pair.receiver.driver().apply_intents(b).unwrap();
        assert_eq!(pair.receiver.driver().fetched_source_count_for_testing(), 0);
        pair.receiver.driver().fault_next_fetched_envelope_for_testing(fault);

        // Real SDKRig stages the actual Service page with the active CoreHost
        // exchange. Only its owned envelope spool is damaged before dispatch.
        let error = pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap_err();
        let sources = pair
            .receiver
            .driver()
            .fetched_envelope_fault_observation_for_testing()
            .unwrap();
        let exchange = sources.exchange.expect("original SDK callback exchange");
        assert!(
            sources.exchange_active_before_fault,
            "the staged callback owns a real active exchange"
        );
        let (active, completed) = pair.receiver.driver().fetched_exchange_for_testing(exchange).unwrap();
        assert!(!active, "the actual core exchange must terminate on hydration failure");
        let (drains_after, missing_after) = pair.receiver.driver().fetch_lifecycle_for_testing().unwrap();
        assert_eq!(
            drains_after, drains_before,
            "failed intake cannot certify a fetch drain"
        );
        assert_eq!(
            missing_after,
            missing_before + 1,
            "the original unsuccessful fetch was consumed without did-fetch"
        );
        let completed = completed.expect("actual completed original exchange");
        assert_eq!(completed.id, exchange);
        assert_eq!(completed.collection, CollectionOutcome::Failed);
        assert!(completed.result.selected_names.is_empty());
        assert!(completed.result.acked_names.is_empty());
        assert!(completed.result.outstanding_names.is_empty());
        assert_eq!(
            error.to_string(),
            "cloud device operation failed: FetchEnvelopeHydration(Unknown)"
        );
        assert_eq!(sources.staged_sources, 2, "one actual pa source and its envelope spool");
        assert_eq!(
            sources.sources_after_fault,
            match fault {
                FetchedEnvelopeFault::Missing => 1,
                FetchedEnvelopeFault::Short => 2,
            }
        );
        assert_eq!(
            sources.existing_sources(),
            0,
            "original terminal reply releases every source"
        );
        assert_eq!(pair.receiver.driver().fetched_source_count_for_testing(), 0);
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        assert!(pair.receiver.facts().unwrap().entities == entities);
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), inbox);
        assert_eq!(pair.receiver.driver().apply_intents(b).unwrap(), intents);
        let failed = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(failed.engine_instance, initial.engine_instance);
        assert_eq!(failed.engine_state, initial.engine_state);
        assert_eq!(failed.last_fetch_engine_state, initial.last_fetch_engine_state);
        assert_eq!(failed.seq, initial.seq);
        assert_eq!(failed.native_file, initial.native_file);
        assert_eq!(failed.generation, initial.generation);
        assert_eq!(failed.col_mod, initial.col_mod);
        assert_eq!(failed.undo_depth, initial.undo_depth);
        assert_eq!(failed.change_events, initial.change_events);
        assert_eq!(failed.runs, initial.runs);
        assert_eq!(failed.wal_receipts, initial.wal_receipts);
        assert_eq!(failed.rollovers, initial.rollovers);
        pair.close();
    }
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
fn power_loss_of_mixed_live_update_and_tombstone_replays_real_post_commit_intent() {
    for local_edit in [false, true] {
        let mut pair = Pair::adopted();
        let b = binding(&pair);
        let originals = notes(&mut pair.receiver);
        let live_id = originals[0].id;
        let deleted_id = originals[1].id;
        let live_key = name(&pair, codec::Kind::Note, live_id);
        let deleted_key = name(&pair, codec::Kind::Note, deleted_id);
        let before_rows = pair.receiver.driver().collection_rows().unwrap();
        let fields: Vec<_> = std::str::from_utf8(&originals[0].flds)
            .unwrap()
            .split('\u{1f}')
            .collect();

        // Checkpoint before applying; retain these database bytes for power loss.
        pair.receiver.close_clean().unwrap();
        let collection = pair.receiver_dir.join("collection.anki2");
        let durable_before = std::fs::read(&collection).unwrap();
        let wal = pair.receiver_dir.join("collection.anki2-wal");
        match std::fs::metadata(&wal) {
            Ok(metadata) => assert_eq!(metadata.len(), 0),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        }
        pair.receiver = online(&pair.receiver_dir, false);
        let old_ack = pair.receiver.facts().unwrap().entities[&live_key].clone();
        pair.sender.advance_clock_ms(7_000).unwrap();
        pair.sender
            .driver()
            .edit_note_fields(live_id, &["power-loss remote", fields[1]])
            .unwrap();
        pair.sender.driver().delete_note(deleted_id).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let remote_live = saved(&pair, &live_key);
        let remote_delete = saved(&pair, &deleted_key);
        let remote_payload = body(&remote_live);
        pair.receiver
            .driver()
            .pause_automatic_wal_flush_for_testing(true)
            .unwrap();
        pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
        pair.receiver
            .driver()
            .deliver_collection_records(&[remote_live, remote_delete])
            .unwrap();
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
        let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
        assert!(intent.originating_open.is_some());
        let committed = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(committed.runs.len(), 1);
        assert_eq!(intent.result_seq, Some(committed.runs[0].seq));
        assert_eq!(note(&mut pair.receiver, live_id), remote_payload);
        assert!(!notes(&mut pair.receiver).iter().any(|n| n.id == deleted_id));
        let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(inbox.len(), 2);
        assert!(pair.receiver.facts().unwrap().entities[&live_key] == old_ack);
        assert!(!committed.wal_receipts.iter().any(|r| {
            r.native_file == intent.native_file
                && r.generation == intent.generation
                && r.through >= intent.result_seq.unwrap()
        }));
        pair.receiver.kill().unwrap();
        // Restore database bytes in place; keep the committed tracking intent.
        std::fs::write(&collection, &durable_before).unwrap();
        for suffix in ["collection.anki2-wal", "collection.anki2-shm"] {
            let path = pair.receiver_dir.join(suffix);
            if path.exists() {
                std::fs::remove_file(path).unwrap();
            }
        }
        pair.reopen_before_identity();
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before_rows);
        assert_eq!(pair.receiver.driver().apply_intents(b).unwrap(), vec![intent.clone()]);
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), inbox);
        if local_edit {
            pair.receiver.advance_clock_ms(30_000).unwrap();
            pair.receiver
                .driver()
                .edit_note_fields(live_id, &[fields[0], "surviving local back"])
                .unwrap();
        }
        pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
        pair.verify_and_drive();
        assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
        let replayed = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
        assert_ne!(replayed.id, intent.id);
        assert_ne!(replayed.originating_open, intent.originating_open);
        assert_eq!(replayed.inbox_ids, intent.inbox_ids);
        // The server ACK is still old until the replayed real op is covered.
        let during = pair.receiver.facts().unwrap().entities[&live_key].clone();
        assert_eq!(during.server_erev, old_ack.server_erev);
        assert_eq!(during.acked_envelope, old_ack.acked_envelope);
        let Payload::Note(applied) = note(&mut pair.receiver, live_id) else {
            panic!("note")
        };
        let expected_back = if local_edit { "surviving local back" } else { fields[1] };
        assert_eq!(
            applied.flds,
            format!("power-loss remote\u{1f}{expected_back}").as_bytes()
        );
        assert!(!pair.receiver.driver().collection_rows().unwrap().iter().any(|p| {
            matches!(p, Payload::Note(n) if n.id == deleted_id) || matches!(p, Payload::Card(c) if c.nid == deleted_id)
        }));
        pair.receiver.release(Point::ApplyAfterCommit).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        settled(&mut pair, b);
        let ack = pair.receiver.facts().unwrap().entities[&live_key].clone();
        assert!(ack.acked_payload.as_ref() == Some(&remote_payload));
        assert_eq!(ack.mark.is_some(), local_edit);
        assert!(
            pair.receiver.facts().unwrap().entities[&deleted_key]
                .tombstone_at
                .is_some()
        );
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let Payload::Note(server) = body(&saved(&pair, &live_key)) else {
            panic!("server note")
        };
        assert_eq!(server.flds, applied.flds);
        assert!(envelope(&saved(&pair, &deleted_key)).fields.tombstone);
        pair.close();
    }
}

// Rust/core asset-read slice: integration base 332cfb29e29bf51aec54e92f6f8934dfa29da6e7;
// original batch base 1416a520542a8c5daa38a23bfe398da704c3d6c9.
// Review/fix: corrected P2 (ordinary equal-erev delivery stays stale); added
// named core retry cases before implementation. One source consistency pass;
// no build/test/formatter, commit, gate, or acceptance evidence in this packet.
// K13.1 / K20 / P24 / Q11: ordinary pa, before native retry scheduling.
#[test]
fn inbox_diagnostic_hashes_complete_near_inline_values_without_an_oversized_reply() {
    use meguri_sync::cloud::{InboxValue, OpaqueBytes};
    use meguri_testkit::cloud::{Caller, Operation, RequestedFields};

    const FRAME_BYTES: usize = 16 * 1024 * 1024;
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
    let zone = pair.sender.collection_zone().unwrap();
    let field = "r".repeat(codec::INLINE_ENVELOPE_BYTES - 4096);
    let mut expected = Vec::new();
    let mut ids = BTreeSet::new();
    let mut full_value_json_bytes = 0;
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
    for index in 0..8 {
        let added = pair
            .sender
            .driver()
            .add_note(template.mid, did, &[&format!("diagnostic {index}"), &field])
            .unwrap();
        assert!(
            pair.sender.sync_now_and_drain(&mut pair.service, STEPS).is_ok(),
            "sender sync failed index={index} phase={}",
            pair.sender.last_sync_phase()
        );
        let key = name(&pair, codec::Kind::Note, added.note.id);
        // Read one real saved version: the aggregate exceeds Service::inspect's
        // small-fixture cap too. Never collect the full record bodies together.
        let response = pair
            .service
            .request(
                &scope(),
                Caller::default(),
                Operation::Read {
                    zone: zone.clone(),
                    name: key.clone(),
                    fields: RequestedFields::All,
                },
            )
            .unwrap();
        let Ok(Reply::Record(remote)) = response.result else {
            panic!("actual saved sender record")
        };
        let entity = envelope(&remote);
        assert!(entity.payload.is_some());
        assert!(entity.fields.generation.is_none());
        assert!(!remote.fields.contains_key("pa"));
        let Field::EncryptedBytes(bytes) = &remote.fields["e"] else {
            panic!("actual inline envelope")
        };
        assert!(bytes.len() > codec::INLINE_ENVELOPE_BYTES - 8192);
        assert!(bytes.len() <= codec::INLINE_ENVELOPE_BYTES);
        let actual_value = InboxValue::Record {
            erev: entity.fields.erev,
            system_fields: OpaqueBytes(remote.system_fields.bytes().to_vec()),
            envelope: OpaqueBytes(bytes.clone()),
            staged_asset: None,
        };
        let encoded = serde_json::to_vec(&actual_value).unwrap();
        full_value_json_bytes += encoded.len();
        let digest = codec::sha256(&encoded);
        drop(encoded);
        pair.receiver
            .driver()
            .deliver_collection_records(std::slice::from_ref(&remote))
            .unwrap();
        let entries = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(entries.len(), expected.len() + 1);
        assert_eq!(&entries[..expected.len()], expected.as_slice());
        let entry = entries.last().unwrap();
        assert!(entry.id > 0);
        assert!(ids.insert(entry.id));
        assert_eq!(entry.record_name, key);
        assert_eq!(entry.erev, entity.fields.erev);
        assert_eq!(entry.value_sha256, digest);
        assert_eq!(entry.reason, InboxReason::Ready);
        assert_eq!(entry.retry_count, 0);
        expected.push(entry.clone());
    }
    // The sum uses each actual value's complete encoding, a strict lower bound
    // on the original reply including ids, names, reasons, retries and framing.
    assert!(full_value_json_bytes > FRAME_BYTES);
    let entries = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(entries, expected);
    let diagnostic_bytes = serde_json::to_vec(&entries).unwrap().len();
    assert!(diagnostic_bytes <= entries.len() * 1024);
    assert!(diagnostic_bytes < FRAME_BYTES);
    pair.close();
}

#[test]
fn ordinary_asset_same_version_reread_recovers_through_intent_writer_wal_and_ack() {
    let (mut pair, id, key, before, after) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    let entity = envelope(&remote);
    let old_ack = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(entity.fields.origin, old_ack.origin);
    assert_ne!(entity.fields.writer, b.0, "ordinary incoming, not an own landed save");
    assert!(old_ack.in_flight.is_empty());
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("production send must save a real pa on Service")
    };
    assert_eq!(asset.bytes(), after.encode().unwrap());
    assert_eq!(asset.size(), entity.payload_size);
    assert_eq!(asset.sha256(), entity.payload_sha256);
    // The parent keeps both source files alive through their intake replies.
    // The saved Service record and its system fields never change on a reread.
    let mut downloaded = tempfile::NamedTempFile::new().unwrap();
    downloaded.write_all(asset.bytes()).unwrap();
    downloaded.as_file().sync_all().unwrap();
    let mut bad_bytes = asset.bytes().to_vec();
    bad_bytes[0] ^= 1;
    let mut invalid = tempfile::NamedTempFile::new().unwrap();
    invalid.write_all(&bad_bytes).unwrap();
    invalid.as_file().sync_all().unwrap();
    assert_ne!(codec::sha256(&bad_bytes), entity.payload_sha256);
    let rows = pair.receiver.driver().collection_rows().unwrap();
    let initial = pair.receiver.driver().incoming_observation(b).unwrap();
    let mut waiting = None;
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    // An ordinary equal-version delivery remains stale even with a valid file.
    let ordinary_wait = pair.receiver.driver().inbox_entries(b).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, Some(downloaded.path()))
        .unwrap();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), ordinary_wait);
    for path in [None, Some(invalid.path())] {
        let request = pair
            .receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .remove(0);
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), path)
            .unwrap();
        assert!(matches!(
            pair.receiver.driver().drain_incoming(STEPS).unwrap(),
            IncomingProgress::Completed(0)
        ));
        let entries = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].record_name, key);
        assert_eq!(entries[0].erev, entity.fields.erev);
        assert_eq!(entries[0].reason, InboxReason::Asset);
        if let Some(previous) = &waiting {
            let previous: &Vec<meguri_testkit::cloud::child_driver::IncomingInboxEntry> = previous;
            assert_eq!(entries[0].id, previous[0].id);
            assert_eq!(entries[0].erev, previous[0].erev);
            assert_eq!(entries[0].retry_count, previous[0].retry_count + 1);
        }
        waiting = Some(entries);
        assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
        assert!(pair.receiver.driver().refused_entries(b).unwrap().is_empty());
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
        let unchanged = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(unchanged.seq, initial.seq);
        assert_eq!(unchanged.col_mod, initial.col_mod);
        assert_eq!(unchanged.change_events, initial.change_events);
        assert_eq!(unchanged.runs, initial.runs);
    }
    let waiting = waiting.unwrap();
    pair.receiver
        .driver()
        .pause_automatic_wal_flush_for_testing(true)
        .unwrap();
    pair.receiver.hold(Point::ApplyAfterIntent).unwrap();
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    pair.receiver
        .driver()
        .asset_read_result(&request, Some(&remote), Some(downloaded.path()))
        .unwrap();
    // Explicit read intake copies the real bytes and settles this entry in place.
    let ready = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].reason, InboxReason::Ready);
    assert_eq!(ready[0].id, waiting[0].id);
    assert_eq!(ready[0].erev, entity.fields.erev);
    assert_eq!(ready[0].retry_count, 0);
    drop(downloaded); // subsequent application reads the file retained by Runtime
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterIntent));
    let intents = pair.receiver.driver().apply_intents(b).unwrap();
    assert_eq!(intents.len(), 1);
    let intent = &intents[0];
    assert_eq!(intent.inbox_ids, vec![waiting[0].id]);
    assert!(intent.result_seq.is_none());
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, key);
    assert_eq!(intent.keys[0].role, ApplyKeyRole::Entry);
    assert!(intent.keys[0].own_flight.is_none());
    assert_eq!(intent.keys[0].before_digest, Some(before.row_digest().unwrap()));
    assert_eq!(intent.keys[0].after_digest, Some(after.row_digest().unwrap()));
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
    pair.receiver.release(Point::ApplyAfterIntent).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
    assert_eq!(note(&mut pair.receiver, id), after);
    let committed = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(committed.runs.len(), initial.runs.len() + 1);
    let run = committed.runs.last().unwrap();
    assert_eq!(run.intent_id, intent.id);
    assert_eq!(run.entry_ids, intent.inbox_ids);
    assert_eq!(run.changed_keys, BTreeSet::from([key.clone()]));
    assert_eq!(
        committed.change_events,
        [initial.change_events.clone(), vec![run.seq]].concat()
    );
    let pending = pair.receiver.driver().apply_intents(b).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].result_seq, Some(run.seq));
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), ready);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    assert!(
        !committed
            .wal_receipts
            .iter()
            .any(|r| r.native_file == run.native_file && r.generation == run.generation && r.through >= run.seq)
    );
    pair.receiver.hold(Point::ApplyAfterFlush).unwrap();
    pair.receiver.release(Point::ApplyAfterCommit).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterFlush));
    let flushed = pair.receiver.driver().incoming_observation(b).unwrap();
    assert!(
        flushed
            .wal_receipts
            .iter()
            .any(|r| r.native_file == run.native_file && r.generation == run.generation && r.through >= run.seq)
    );
    assert_eq!(pair.receiver.driver().apply_intents(b).unwrap(), pending);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), ready);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.release(Point::ApplyAfterFlush).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    settled(&mut pair, b);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    let Field::EncryptedBytes(bytes) = &remote.fields["e"] else {
        panic!("envelope")
    };
    assert_eq!(ack.origin, entity.fields.origin);
    assert_eq!(ack.server_erev, entity.fields.erev);
    assert_eq!(ack.acked_system_fields, remote.system_fields.bytes());
    assert_eq!(ack.acked_envelope.0, *bytes);
    assert_eq!(ack.acked_payload_digest, entity.payload_sha256);
    assert_eq!(ack.acked_row_digest, after.row_digest().unwrap());
    assert!(ack.acked_payload.as_ref() == Some(&after));
    assert_eq!(ack.payload, after);
    assert_eq!(ack.acked_rev, ack.local_rev);
    assert!(ack.base_payload.is_none());
    assert!(ack.mark.is_none());
    assert!(ack.in_flight.is_empty());
    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.receiver
        .driver()
        .pause_automatic_wal_flush_for_testing(false)
        .unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(matches!(
        pair.receiver.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Completed(1)
    ));
    let applied = pair.receiver.driver().incoming_observation(b).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&remote))
        .unwrap();
    assert!(matches!(
        pair.receiver.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Completed(0)
    ));
    let duplicate = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(duplicate.runs, applied.runs);
    assert_eq!(duplicate.seq, applied.seq);
    assert_eq!(duplicate.change_events, applied.change_events);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    settled(&mut pair, b);
    assert_eq!(saved(&pair, &key), remote, "recovery issued no newer Service save");
    pair.receiver.close_clean().unwrap();
    pair.reopen_before_identity();
    assert_eq!(note(&mut pair.receiver, id), after);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    settled(&mut pair, b);
    pair.verify_and_drive();
    let reopened = pair.receiver.driver().incoming_observation(b).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&remote))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let duplicate = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(duplicate.runs, reopened.runs);
    assert_eq!(duplicate.seq, reopened.seq);
    assert_eq!(duplicate.change_events, reopened.change_events);
    assert_eq!(note(&mut pair.receiver, id), after);
    settled(&mut pair, b);
    pair.close();
}

fn asset_retry() -> meguri_sync::cloud::incoming::AssetReadKind {
    meguri_sync::cloud::incoming::AssetReadKind::Retry
}

// Admission review repairs over the supplied unlanded candidate (root gate
// 332cfb29); original integration base remains 1416a520542a8c5daa38a23bfe398da704c3d6c9.
// Source-only controls; root owns execution, review and acceptance.
#[test]
fn accepted_asset_accounting_fault_closes_admission_through_original_terminal_result() {
    use meguri_sync::cloud::incoming::AssetReadKind;
    for kind in [asset_retry(), AssetReadKind::SaveConflictFirst] {
        let (mut pair, id, key, _, _) = changed_asset_note();
        let b = binding(&pair);
        if kind == AssetReadKind::SaveConflictFirst {
            pair.receiver
                .driver()
                .edit_note_fields(id, &["local", "pending"])
                .unwrap();
            pair.receiver
                .driver()
                .deliver_real_asset_save_conflict(&mut pair.service, &scope(), &key)
                .unwrap();
        } else {
            let remote = saved(&pair, &key);
            pair.receiver
                .driver()
                .deliver_collection_asset_download(&remote, None)
                .unwrap();
        }
        let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
        let rows = pair.receiver.driver().collection_rows().unwrap();
        let ack = pair.receiver.facts().unwrap().entities[&key].clone();
        let pass = pair.receiver.driver().begin_asset_read_pass(kind).unwrap();
        let request = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
        pair.receiver
            .driver()
            .arm_tracking_fault("cloud.tracking.commit.asset_read", TrackingFault::Io)
            .unwrap();
        let error = pair.receiver.driver().admit_asset_read(&request, true).unwrap_err();
        assert!(error.to_string().contains("RuntimeTracking(Io)"), "{error}");
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&request).unwrap(),
            (true, true)
        );
        assert!(!pair.receiver.driver().cancel_asset_read(&request).unwrap());
        assert!(pair.receiver.driver().next_asset_read(&pass).is_err());
        for blocked in [asset_retry(), AssetReadKind::SaveConflictFirst] {
            assert!(pair.receiver.driver().begin_asset_read_pass(blocked).is_err());
        }
        // The failed accounting return did not cancel the accepted operation.
        // Its actual failed terminal callback still reaches preparation and owns
        // both runtime and transport state until that callback finishes.
        pair.receiver.driver().hold_asset_read_prepared().unwrap();
        assert!(pair.receiver.driver().asset_read_result(&request, None, None).is_err());
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&request).unwrap(),
            (true, true)
        );
        pair.receiver.driver().release_asset_read_prepared().unwrap();
        assert!(!pair.receiver.driver().asset_read_result(&request, None, None).unwrap());
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&request).unwrap(),
            (false, false)
        );
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
        assert!(pair.receiver.driver().begin_asset_read_pass(kind).is_err());
        pair.receiver.driver().recreate_asset_sdk().unwrap();
        assert!(pair.receiver.driver().begin_asset_read_pass(kind).is_err());
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
        assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
        // Actual reopening resets policy; no synthetic accounting recovery or
        // retry is supplied. Lost first-read grants become ordinary retries.
        pair.receiver.close_clean().unwrap();
        pair.reopen_before_identity();
        pair.verify_and_drive();
        let fresh = pair
            .receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .remove(0);
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, 1);
        pair.receiver.driver().asset_read_result(&fresh, None, None).unwrap();
        pair.close();
    }
}

#[test]
fn highest_unvisited_asset_replacement_waits_for_a_fresh_pass_despite_reused_rowid() {
    let (mut pair, id, key, _, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let other_id = notes(&mut pair.sender).into_iter().find(|n| n.id != id).unwrap().id;
    let other_key = name(&pair, codec::Kind::Note, other_id);
    let field = "h".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(other_id, &[&field, "unvisited"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let other = saved(&pair, &other_key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&other, None)
        .unwrap();
    let original = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(original.len(), 2);
    assert_eq!(original[1].record_name, other_key);
    assert!(original[1].id > original[0].id);
    let pass = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let visited = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
    assert_eq!(visited.record_name(), key);
    assert!(pair.receiver.driver().admit_asset_read(&visited, true).unwrap());
    pair.receiver.driver().asset_read_result(&visited, None, None).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(other_id, &[&field, "post-pass successor"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let successor = saved(&pair, &other_key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&successor, None)
        .unwrap();
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let replaced = waiting.iter().find(|e| e.record_name == other_key).unwrap();
    // Observe SQLite's real reuse after actual version replacement, rather than
    // pretending that INTEGER PRIMARY KEY provides an arrival counter.
    assert_eq!(replaced.id, original[1].id);
    assert!(replaced.erev > original[1].erev);
    assert_eq!(replaced.retry_count, 0);
    assert!(pair.receiver.driver().next_asset_read(&pass).unwrap().is_none());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    let fresh = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let first = pair.receiver.driver().next_asset_read(&fresh).unwrap().unwrap();
    assert_eq!(first.record_name(), key);
    assert!(pair.receiver.driver().cancel_asset_read(&first).unwrap());
    let next = pair.receiver.driver().next_asset_read(&fresh).unwrap().unwrap();
    assert_eq!(next.record_name(), other_key);
    assert!(!pair.receiver.driver().admit_asset_read(&next, false).unwrap());
    assert!(pair.receiver.driver().next_asset_read(&fresh).unwrap().is_none());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    pair.close();
}

#[test]
fn new_asset_pass_preserves_old_accepted_owner_until_stale_terminal_call() {
    for replace_sdk in [false, true] {
        let (mut pair, id, key, before, _) = changed_asset_note();
        let b = binding(&pair);
        let older = saved(&pair, &key);
        pair.receiver
            .driver()
            .deliver_collection_asset_download(&older, None)
            .unwrap();
        let old = pair
            .receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .remove(0);
        assert_eq!(pair.receiver.driver().asset_read_ownership(&old).unwrap(), (true, true));
        if replace_sdk {
            pair.receiver.driver().recreate_asset_sdk().unwrap();
        } else {
            let (before_sdk, after_sdk) = pair.receiver.driver().suspend_resume_asset_sdk().unwrap();
            assert_eq!(before_sdk, old.sdk_instance());
            assert_eq!(after_sdk, before_sdk);
        }
        assert_eq!(pair.receiver.driver().asset_read_ownership(&old).unwrap(), (true, true));
        let field = "s".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
        pair.sender
            .driver()
            .edit_note_fields(id, &[&field, "new authority successor"])
            .unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let newer = saved(&pair, &key);
        pair.receiver
            .driver()
            .deliver_collection_asset_download(&newer, None)
            .unwrap();
        let pass = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
        assert_eq!(pair.receiver.driver().asset_read_ownership(&old).unwrap(), (true, true));
        let successor = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
        assert_ne!(successor, old);
        assert_eq!(successor.record_name(), old.record_name());
        assert!(pair.receiver.driver().admit_asset_read(&successor, true).unwrap());
        let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&successor).unwrap(),
            (true, true)
        );
        assert!(
            pair.receiver
                .driver()
                .asset_read_result(&old, Some(&older), None)
                .is_err()
        );
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&old).unwrap(),
            (false, false)
        );
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&successor).unwrap(),
            (true, true)
        );
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
        assert_eq!(note(&mut pair.receiver, id), before);
        let Field::Asset(asset) = &newer.fields["pa"] else {
            panic!("pa")
        };
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(asset.bytes()).unwrap();
        file.as_file().sync_all().unwrap();
        assert!(
            pair.receiver
                .driver()
                .asset_read_result(&successor, Some(&newer), Some(file.path()))
                .unwrap()
        );
        assert_eq!(
            pair.receiver.driver().asset_read_ownership(&successor).unwrap(),
            (false, false)
        );
        assert!(pair.receiver.driver().next_asset_read(&pass).unwrap().is_none());
        drop(file);
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        assert_eq!(note(&mut pair.receiver, id), body(&newer));
        settled(&mut pair, b);
        pair.close();
    }
}

#[test]
fn asset_read_selection_is_unique_and_retry_budget_tracks_enqueue_success() {
    let (mut pair, id, key, _, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let late_id = notes(&mut pair.sender).into_iter().find(|n| n.id != id).unwrap().id;
    let late_key = name(&pair, codec::Kind::Note, late_id);
    let field = "q".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(late_id, &[&field, "second asset"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let late = saved(&pair, &late_key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&late, None)
        .unwrap();
    let initial = pair.receiver.driver().inbox_entries(b).unwrap();
    let rows = pair.receiver.driver().collection_rows().unwrap();
    let pass = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let first = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), initial);
    assert!(pair.receiver.driver().next_asset_read(&pass).is_err());
    // This acknowledges a real synchronous child enqueue, not a read result.
    assert!(pair.receiver.driver().admit_asset_read(&first, true).unwrap());
    assert!(!pair.receiver.driver().cancel_asset_read(&first).unwrap());
    assert!(pair.receiver.driver().next_asset_read(&pass).is_err());
    pair.receiver.driver().asset_read_result(&first, None, None).unwrap();
    let second = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
    assert_ne!(first.record_name(), second.record_name());
    assert!(!pair.receiver.driver().admit_asset_read(&second, false).unwrap());
    assert!(pair.receiver.driver().next_asset_read(&pass).unwrap().is_none());
    let after = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(
        after
            .iter()
            .find(|e| e.record_name == first.record_name())
            .unwrap()
            .retry_count,
        1
    );
    assert_eq!(
        after
            .iter()
            .find(|e| e.record_name == second.record_name())
            .unwrap()
            .retry_count,
        0
    );
    let fresh = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let cancelled = pair.receiver.driver().next_asset_read(&fresh).unwrap().unwrap();
    assert!(pair.receiver.driver().cancel_asset_read(&cancelled).unwrap());
    let remaining = pair.receiver.driver().next_asset_read(&fresh).unwrap().unwrap();
    assert_ne!(remaining.record_name(), cancelled.record_name());
    assert!(pair.receiver.driver().cancel_asset_read(&remaining).unwrap());
    assert!(pair.receiver.driver().next_asset_read(&fresh).unwrap().is_none());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), after);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.close();
}

#[test]
fn cancelled_enqueued_asset_read_keeps_original_file_and_completion_ownership() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let pass = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let request = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
    assert!(pair.receiver.driver().admit_asset_read(&request, true).unwrap());
    assert!(!pair.receiver.driver().cancel_asset_read(&request).unwrap());
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    pair.receiver.driver().hold_asset_read_prepared().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .is_err()
    );
    assert!(!pair.receiver.driver().cancel_asset_read(&request).unwrap());
    assert!(pair.receiver.driver().next_asset_read(&pass).is_err());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, 1);
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert!(file.path().exists());
    pair.receiver.driver().release_asset_read_prepared().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .unwrap()
    );
    assert!(!pair.receiver.driver().cancel_asset_read(&request).unwrap());
    assert!(pair.receiver.driver().next_asset_read(&pass).unwrap().is_none());
    assert_eq!(
        pair.receiver.driver().inbox_entries(b).unwrap()[0].reason,
        InboxReason::Ready
    );
    drop(file);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), body(&remote));
    settled(&mut pair, b);
    pair.close();
}

#[test]
fn first_asset_grant_survives_refused_enqueue_and_stale_pass_cannot_take_successor() {
    use meguri_sync::cloud::incoming::AssetReadKind;
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    pair.receiver
        .driver()
        .edit_note_fields(id, &["local", "pending"])
        .unwrap();
    let local = note(&mut pair.receiver, id);
    assert_ne!(local, before);
    pair.receiver
        .driver()
        .deliver_real_asset_save_conflict(&mut pair.service, &scope(), &key)
        .unwrap();
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let pass = pair
        .receiver
        .driver()
        .begin_asset_read_pass(AssetReadKind::SaveConflictFirst)
        .unwrap();
    let old = pair.receiver.driver().next_asset_read(&pass).unwrap().unwrap();
    assert!(!pair.receiver.driver().admit_asset_read(&old, false).unwrap());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert!(pair.receiver.driver().next_asset_read(&pass).unwrap().is_none());
    let cancelled_pass = pair
        .receiver
        .driver()
        .begin_asset_read_pass(AssetReadKind::SaveConflictFirst)
        .unwrap();
    let cancelled = pair
        .receiver
        .driver()
        .next_asset_read(&cancelled_pass)
        .unwrap()
        .unwrap();
    assert!(pair.receiver.driver().cancel_asset_read(&cancelled).unwrap());
    let granted_pass = pair
        .receiver
        .driver()
        .begin_asset_read_pass(AssetReadKind::SaveConflictFirst)
        .unwrap();
    let granted = pair.receiver.driver().next_asset_read(&granted_pass).unwrap().unwrap();
    assert!(pair.receiver.driver().admit_asset_read(&granted, true).unwrap());
    pair.receiver.driver().asset_read_result(&granted, None, None).unwrap();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    let stale_pass = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let stale = pair.receiver.driver().next_asset_read(&stale_pass).unwrap().unwrap();
    pair.receiver.driver().suspend_resume_asset_sdk().unwrap();
    assert!(pair.receiver.driver().next_asset_read(&stale_pass).is_err());
    let fresh_pass = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    let fresh = pair.receiver.driver().next_asset_read(&fresh_pass).unwrap().unwrap();
    assert!(!pair.receiver.driver().cancel_asset_read(&stale).unwrap());
    assert!(pair.receiver.driver().admit_asset_read(&stale, true).is_err());
    assert!(!pair.receiver.driver().asset_read_result(&stale, None, None).unwrap());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert!(pair.receiver.driver().admit_asset_read(&fresh, true).unwrap());
    assert!(!pair.receiver.driver().cancel_asset_read(&fresh).unwrap());
    pair.receiver.driver().asset_read_result(&fresh, None, None).unwrap();
    let done = pair
        .receiver
        .driver()
        .begin_asset_read_pass(AssetReadKind::SaveConflictFirst)
        .unwrap();
    assert!(pair.receiver.driver().next_asset_read(&done).unwrap().is_none());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, 1);
    assert_eq!(note(&mut pair.receiver, id), local);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.close();
}

#[test]
fn asset_retries_count_per_entry_including_a_late_entry_after_fifth_pass() {
    let (mut pair, id, key, _, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    let rows = pair.receiver.driver().collection_rows().unwrap();
    for count in 1..=5 {
        let requests = pair.receiver.driver().asset_read_requests(asset_retry()).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].record_name(), key);
        pair.receiver
            .driver()
            .asset_read_result(&requests[0], None, None)
            .unwrap();
        let entries = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(entries[0].retry_count, count);
        assert_eq!(entries[0].reason, InboxReason::Asset);
    }
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .is_empty()
    );
    let exhausted = pair.receiver.driver().begin_asset_read_pass(asset_retry()).unwrap();
    assert!(pair.receiver.driver().next_asset_read(&exhausted).unwrap().is_none());
    let late_id = notes(&mut pair.sender).into_iter().find(|n| n.id != id).unwrap().id;
    let late_key = name(&pair, codec::Kind::Note, late_id);
    let field = "l".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(late_id, &[&field, "late asset"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let late = saved(&pair, &late_key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&late, None)
        .unwrap();
    assert!(pair.receiver.driver().next_asset_read(&exhausted).unwrap().is_none());
    for count in 1..=5 {
        let requests = pair.receiver.driver().asset_read_requests(asset_retry()).unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].record_name(), late_key);
        pair.receiver
            .driver()
            .asset_read_result(&requests[0], Some(&late), None)
            .unwrap();
        let entries = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(entries.iter().find(|e| e.record_name == key).unwrap().retry_count, 5);
        assert_eq!(
            entries.iter().find(|e| e.record_name == late_key).unwrap().retry_count,
            count
        );
    }
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .is_empty()
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.close();
}

#[test]
fn asset_count_three_at_close_gets_five_reads_on_next_actual_open() {
    let (mut pair, _, key, _, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    for _ in 0..3 {
        let request = pair
            .receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .remove(0);
        pair.receiver.driver().asset_read_result(&request, None, None).unwrap();
    }
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, 3);
    pair.receiver.close_clean().unwrap();
    pair.reopen_before_identity();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, 0);
    pair.verify_and_drive();
    // The test-only driver does not run retry actions automatically.
    for count in 1..=5 {
        let request = pair
            .receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .remove(0);
        pair.receiver.driver().asset_read_result(&request, None, None).unwrap();
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, count);
    }
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .is_empty()
    );
    pair.close();
}

#[test]
fn asset_read_results_cannot_settle_a_successor_version_or_sdk() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let older = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&older, None)
        .unwrap();
    let old_request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let field = "v".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(id, &[&field, "successor"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let newer = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&newer, None)
        .unwrap();
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let Field::Asset(asset) = &older.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&old_request, Some(&older), Some(file.path()))
            .unwrap()
    );
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let selected = pair.receiver.driver().inbox_entries(b).unwrap();
    // Older results to the current request cannot clear its newer waiting version.
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&request, Some(&older), Some(file.path()))
            .unwrap()
    );
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), selected);
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let inbox_at_sdk_replacement = pair.receiver.driver().inbox_entries(b).unwrap();
    pair.receiver.driver().recreate_asset_sdk().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&newer), None)
            .is_err()
    );
    assert_eq!(
        pair.receiver.driver().inbox_entries(b).unwrap(),
        inbox_at_sdk_replacement
    );
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.close();
}

// Fixer packet: original base 1416a520542a8c5daa38a23bfe398da704c3d6c9;
// filtered seed 93850d35cda45b3ff758a995b36d970d80466ce1. Four runtime
// review repairs; source/callsite consistency only, execution belongs to root.
#[test]
fn outstanding_asset_read_is_refused_after_actual_same_sdk_suspend_resume() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let old = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let selected = pair.receiver.driver().inbox_entries(b).unwrap();
    let (before_sdk, after_sdk) = pair.receiver.driver().suspend_resume_asset_sdk().unwrap();
    assert_eq!(before_sdk, old.sdk_instance());
    assert_eq!(after_sdk, before_sdk);
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&old, Some(&remote), Some(file.path()))
            .is_err()
    );
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].id, selected[0].id);
    // No completion, caller state assertion, or SDK replacement grants this:
    // begin must prune the old selection using the new core's complete guard.
    let fresh = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    assert_eq!(fresh.sdk_instance(), old.sdk_instance());
    assert_ne!(fresh, old);
    assert_eq!(fresh.record_name(), old.record_name());
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&old, Some(&remote), Some(file.path()))
            .is_ok_and(|v| !v)
    );
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&fresh, Some(&remote), Some(file.path()))
            .unwrap()
    );
    drop(file);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), body(&remote));
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.close();
}

fn inbox_asset_files(pair: &Pair) -> Vec<std::path::PathBuf> {
    let staging = pair.receiver_dir.join("meguri-cloud-staging");
    let mut files = Vec::new();
    if staging.exists() {
        for directory in std::fs::read_dir(staging).unwrap() {
            let directory = directory.unwrap();
            if directory.file_name().to_string_lossy().ends_with("-inbox") {
                files.extend(
                    std::fs::read_dir(directory.path())
                        .unwrap()
                        .map(|file| file.unwrap().path()),
                );
            }
        }
    }
    files.sort();
    files
}

#[test]
fn preparation_storage_failure_retires_only_selection_and_restored_storage_recovers() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let observation = pair.receiver.driver().incoming_observation(b).unwrap();
    let files_before = inbox_asset_files(&pair);
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    let owner = b.0.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let staging = pair.receiver_dir.join("meguri-cloud-staging");
    std::fs::create_dir_all(&staging).unwrap();
    let destination = staging.join(format!("pa-{owner}-inbox"));
    let retained = staging.join("retained-inbox");
    let existed = destination.exists();
    if existed {
        std::fs::rename(&destination, &retained).unwrap();
    }
    // Block the destination used by the intake writer. The supplied Service
    // bytes are valid, so the returned error must describe the storage failure.
    std::fs::write(&destination, b"blocked destination").unwrap();
    let error = pair
        .receiver
        .driver()
        .asset_read_result(&request, Some(&remote), Some(file.path()))
        .unwrap_err();
    assert!(error.to_string().contains("RuntimeTracking(InvalidData)"));
    std::fs::remove_file(&destination).unwrap();
    if existed {
        std::fs::rename(&retained, &destination).unwrap();
    }
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert_eq!(inbox_asset_files(&pair), files_before);
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let unchanged = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(unchanged.seq, observation.seq);
    assert_eq!(unchanged.runs, observation.runs);
    assert_eq!(unchanged.change_events, observation.change_events);
    let fresh = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    assert_ne!(fresh, request);
    assert_eq!(fresh.sdk_instance(), request.sdk_instance());
    assert_eq!(fresh.record_name(), request.record_name());
    let selected = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(selected[0].id, waiting[0].id);
    assert_eq!(selected[0].retry_count, waiting[0].retry_count + 1);
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .unwrap()
    );
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), selected);
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&fresh, Some(&remote), Some(file.path()))
            .unwrap()
    );
    drop(file);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), body(&remote));
    let settled = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(settled.server_erev, envelope(&remote).fields.erev);
    assert_eq!(settled.acked_system_fields.as_slice(), remote.system_fields.bytes());
    assert_eq!(settled.acked_payload_digest, envelope(&remote).payload_sha256);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert_eq!(inbox_asset_files(&pair), files_before);
    pair.close();
}

#[test]
fn sent_handler_during_asset_preparation_retires_failed_selection_and_recovers() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let observation = pair.receiver.driver().incoming_observation(b).unwrap();
    let files_before = inbox_asset_files(&pair);
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    pair.receiver.driver().hold_asset_read_prepared().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .is_err()
    );
    let prepared_files = inbox_asset_files(&pair);
    assert_eq!(prepared_files.len(), files_before.len() + 1);
    let owned = prepared_files.iter().find(|path| !files_before.contains(path)).unwrap();
    assert_eq!(std::fs::read(owned).unwrap(), asset.bytes());
    // This enters the actual Runtime handler while its preparation worker is
    // outside both guards. No Sent result, ACK or success is supplied.
    pair.receiver
        .driver()
        .enter_asset_read_sent_handler(request.sdk_instance(), 0)
        .unwrap();
    pair.receiver.driver().release_asset_read_prepared().unwrap();
    let error = pair
        .receiver
        .driver()
        .asset_read_result(&request, Some(&remote), Some(file.path()))
        .unwrap_err();
    assert!(error.to_string().contains("RuntimeTracking(InvalidData)"));
    // The failed completion and its file cleanup finish with the handler still
    // active. Clearing it must expose this same waiting entry to fresh selection.
    assert_eq!(inbox_asset_files(&pair), files_before);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let failed = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(failed.seq, observation.seq);
    assert_eq!(failed.runs, observation.runs);
    assert_eq!(failed.wal_receipts, observation.wal_receipts);
    assert_eq!(failed.change_events, observation.change_events);
    assert_eq!(
        pair.receiver
            .driver()
            .return_asset_read_sent_handler(request.sdk_instance())
            .unwrap(),
        0
    );
    let fresh = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    assert_ne!(fresh, request);
    assert_eq!(fresh.sdk_instance(), request.sdk_instance());
    assert_eq!(fresh.record_name(), request.record_name());
    let selected = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(selected[0].id, waiting[0].id);
    assert_eq!(selected[0].retry_count, waiting[0].retry_count + 1);
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .unwrap()
    );
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), selected);
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&fresh, Some(&remote), Some(file.path()))
            .unwrap()
    );
    drop(file);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), body(&remote));
    let settled = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(settled.server_erev, envelope(&remote).fields.erev);
    assert_eq!(settled.acked_system_fields.as_slice(), remote.system_fields.bytes());
    assert_eq!(settled.acked_payload_digest, envelope(&remote).payload_sha256);
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert_eq!(inbox_asset_files(&pair), files_before);
    pair.close();
}

#[test]
fn published_asset_read_file_survives_until_wal_ack_and_finishes_without_orphan() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let observation = pair.receiver.driver().incoming_observation(b).unwrap();
    let files_before = inbox_asset_files(&pair);
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    pair.receiver.driver().hold_asset_read_finishing().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .is_err()
    );
    let prepared_files = inbox_asset_files(&pair);
    assert_eq!(prepared_files.len(), files_before.len() + 1);
    let owned = prepared_files.iter().find(|path| !files_before.contains(path)).unwrap();
    assert_eq!(std::fs::read(owned).unwrap(), asset.bytes());
    let ready = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].id, waiting[0].id);
    assert_eq!(ready[0].reason, InboxReason::Ready);
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    // Release file finalization before applying the entry and covering its WAL.
    // Keep the referenced file until acknowledgement removes the inbox entry.
    pair.receiver.driver().release_asset_read_finishing().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .unwrap()
    );
    assert_eq!(std::fs::read(owned).unwrap(), asset.bytes());
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), body(&remote));
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let applied = pair.receiver.driver().incoming_observation(b).unwrap();
    assert!(applied.seq > observation.seq);
    let run = applied.runs.last().unwrap();
    assert!(run.entry_ids.contains(&waiting[0].id));
    assert!(run.changed_keys.contains(&key));
    assert!(
        applied
            .wal_receipts
            .iter()
            .any(|receipt| receipt.native_file == run.native_file
                && receipt.generation == run.generation
                && receipt.through >= run.seq)
    );
    let settled = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(settled.server_erev, envelope(&remote).fields.erev);
    assert_eq!(settled.acked_system_fields.as_slice(), remote.system_fields.bytes());
    assert_eq!(settled.acked_payload_digest, envelope(&remote).payload_sha256);
    assert_eq!(inbox_asset_files(&pair), files_before);
    pair.close();
}

#[test]
fn paused_asset_read_preparation_releases_guards_and_stale_authority_cleans_owned_file() {
    let (mut pair, id, key, before, _) = changed_asset_note();
    let b = binding(&pair);
    let remote = saved(&pair, &key);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    let observation = pair.receiver.driver().incoming_observation(b).unwrap();
    let files_before = inbox_asset_files(&pair);
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    pair.receiver.driver().hold_asset_read_prepared().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .is_err()
    );
    let prepared_files = inbox_asset_files(&pair);
    assert_eq!(prepared_files.len(), files_before.len() + 1);
    let owned = prepared_files.iter().find(|path| !files_before.contains(path)).unwrap();
    assert_eq!(std::fs::read(owned).unwrap(), asset.bytes());
    // These commands complete while the actual child preparation is latched.
    // status takes runtime state; inbox/intent/ACK diagnostics take tracking too.
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    let paused = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(paused.seq, observation.seq);
    assert_eq!(paused.native_file, observation.native_file);
    assert_eq!(paused.generation, observation.generation);
    assert_eq!(paused.col_mod, observation.col_mod);
    assert_eq!(paused.undo_depth, observation.undo_depth);
    assert_eq!(paused.runs, observation.runs);
    assert_eq!(paused.wal_receipts, observation.wal_receipts);
    assert_eq!(paused.change_events, observation.change_events);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].reason, InboxReason::Asset);
    assert_eq!(waiting[0].retry_count, 1);
    let (old_sdk, new_sdk) = pair.receiver.driver().suspend_resume_asset_sdk().unwrap();
    assert_eq!(old_sdk, new_sdk);
    let resumed = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0].id, waiting[0].id);
    assert_eq!(resumed[0].record_name, waiting[0].record_name);
    assert_eq!(resumed[0].erev, waiting[0].erev);
    assert_eq!(resumed[0].value_sha256, waiting[0].value_sha256);
    assert_eq!(resumed[0].reason, waiting[0].reason);
    assert_eq!(resumed[0].retry_count, 0);
    pair.receiver.driver().hold_asset_read_finishing().unwrap();
    pair.receiver.driver().release_asset_read_prepared().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .is_err()
    );
    // Pause finalization after authority validation has refused the result.
    // Both guards are released; the lease protects the file until removal.
    assert_eq!(inbox_asset_files(&pair), prepared_files);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), resumed);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    let finishing = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(finishing.seq, observation.seq);
    assert_eq!(finishing.change_events, observation.change_events);
    pair.receiver.driver().release_asset_read_finishing().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&remote), Some(file.path()))
            .is_err()
    );
    assert_eq!(inbox_asset_files(&pair), files_before);
    assert_eq!(note(&mut pair.receiver, id), before);
    let after = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(after.seq, observation.seq);
    assert_eq!(after.runs, observation.runs);
    assert_eq!(after.change_events, observation.change_events);
    assert!(pair.receiver.facts().unwrap().entities[&key] == ack);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), resumed);
    let fresh = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&fresh, Some(&remote), Some(file.path()))
            .unwrap()
    );
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&fresh, Some(&remote), Some(file.path()))
            .unwrap()
    );
    drop(file);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), body(&remote));
    let applied = pair.receiver.driver().incoming_observation(b).unwrap();
    assert!(applied.seq > observation.seq);
    let run = applied.runs.last().unwrap();
    assert!(run.entry_ids.contains(&waiting[0].id));
    assert!(run.changed_keys.contains(&key));
    assert_eq!(run.native_file, applied.native_file);
    assert_eq!(run.generation, applied.generation);
    assert!(
        applied
            .wal_receipts
            .iter()
            .any(|receipt| receipt.native_file == run.native_file
                && receipt.generation == run.generation
                && receipt.through >= run.seq)
    );
    let settled = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(settled.server_erev, envelope(&remote).fields.erev);
    assert_eq!(settled.acked_system_fields.as_slice(), remote.system_fields.bytes());
    assert_eq!(settled.acked_payload_digest, envelope(&remote).payload_sha256);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert_eq!(inbox_asset_files(&pair), files_before);
    pair.close();
}

#[test]
fn save_conflict_first_asset_read_is_granted_by_a_real_handed_result_and_is_separate() {
    use meguri_sync::cloud::incoming::AssetReadKind;
    let (mut pair, id, key, _, _) = changed_asset_note();
    let b = binding(&pair);
    // An ordinary fetch cannot obtain the extra save-conflict read.
    let remote = saved(&pair, &key);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&remote, None)
        .unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(AssetReadKind::SaveConflictFirst)
            .unwrap()
            .is_empty()
    );
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    let retry = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&retry, Some(&remote), Some(file.path()))
            .unwrap()
    );
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    // Advance the server revision before submitting the conflicting local save.
    let field = "c".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(id, &[&field, "conflict version"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let local = "l".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.receiver
        .driver()
        .edit_note_fields(id, &[&local, "pending"])
        .unwrap();
    assert!(pair.receiver.driver().asset_body_read_control(b, &key).unwrap() > 0);
    pair.receiver
        .driver()
        .deliver_real_asset_save_conflict(&mut pair.service, &scope(), &key)
        .unwrap();
    assert_eq!(pair.receiver.driver().asset_conflict_body_reads().unwrap(), 0);
    let requests = pair
        .receiver
        .driver()
        .asset_read_requests(AssetReadKind::SaveConflictFirst)
        .unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].record_name(), key);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, 0);
    pair.receiver
        .driver()
        .asset_read_result(&requests[0], None, None)
        .unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(AssetReadKind::SaveConflictFirst)
            .unwrap()
            .is_empty()
    );
    for count in 1..=5 {
        let request = pair
            .receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .remove(0);
        pair.receiver.driver().asset_read_result(&request, None, None).unwrap();
        assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap()[0].retry_count, count);
    }
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(asset_retry())
            .unwrap()
            .is_empty()
    );
    pair.close();
}

#[test]
fn save_conflict_first_same_version_read_applies_and_pending_merge_is_sent_after() {
    use meguri_sync::cloud::incoming::AssetReadKind;
    let (mut pair, id, key, _, _) = changed_asset_note();
    let b = binding(&pair);
    let server = saved(&pair, &key);
    pair.receiver.advance_clock_ms(20_000).unwrap();
    pair.receiver
        .driver()
        .edit_note_fields(id, &["new local", "pending merge"])
        .unwrap();
    let local = note(&mut pair.receiver, id);
    let ack = pair.receiver.facts().unwrap().entities[&key].clone();
    pair.receiver
        .driver()
        .deliver_real_asset_save_conflict(&mut pair.service, &scope(), &key)
        .unwrap();
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].reason, InboxReason::Asset);
    assert_eq!(waiting[0].retry_count, 0);
    let pending = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(pending.server_erev, ack.server_erev);
    assert_eq!(pending.acked_system_fields, ack.acked_system_fields);
    assert_eq!(pending.acked_payload_digest, ack.acked_payload_digest);
    assert!(pending.base_payload == ack.base_payload);
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(AssetReadKind::SaveConflictFirst)
        .unwrap()
        .remove(0);
    let Field::Asset(asset) = &server.fields["pa"] else {
        panic!("pa")
    };
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(asset.bytes()).unwrap();
    file.as_file().sync_all().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(&server), Some(file.path()))
            .unwrap()
    );
    let ready = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(ready[0].id, waiting[0].id);
    assert_eq!(ready[0].erev, waiting[0].erev);
    assert_eq!(ready[0].reason, InboxReason::Ready);
    assert_eq!(ready[0].retry_count, 0);
    drop(file);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(note(&mut pair.receiver, id), local);
    let merged = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(merged.server_erev, envelope(&server).fields.erev);
    assert!(merged.base_payload.as_ref() == Some(&body(&server)));
    assert!(merged.mark.is_some());
    assert!(
        pair.receiver
            .driver()
            .asset_read_requests(AssetReadKind::SaveConflictFirst)
            .unwrap()
            .is_empty()
    );
    // The duplicate reply cannot apply twice or recreate the one-shot grant.
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&request, Some(&server), None)
            .unwrap()
    );
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let sent = saved(&pair, &key);
    assert_eq!(envelope(&sent).fields.writer, b.0);
    assert_eq!(body(&sent), local);
    assert!(pair.receiver.facts().unwrap().entities[&key].mark.is_none());
    pair.close();
}

fn reopen_incoming_sdk(pair: &mut Pair) -> meguri_testkit::cloud::adapter::SDKRig {
    pair.receiver.close_clean().unwrap();
    let mut sdk = meguri_testkit::cloud::adapter::SDKRig::open_existing_collection(
        &pair.receiver_dir,
        &scope(),
        NOW,
        809,
        Arc::new(NoReopenKeys),
    )
    .unwrap();
    sdk.reconcile_register_and_confirm_root(&mut pair.service, STEPS)
        .unwrap();
    sdk
}

fn sdk_note(sdk: &mut meguri_testkit::cloud::adapter::SDKRig, id: i64) -> Payload {
    sdk.driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find(|row| matches!(row, Payload::Note(n) if n.id == id))
        .unwrap()
}

fn conditional_server_record(response: &meguri_testkit::cloud::Response, key: &str) -> Record {
    let Ok(Reply::Modified(items)) = &response.result else {
        panic!("actual conditional Service response")
    };
    let Err(error) = &items.iter().find(|item| item.name == key).unwrap().result else {
        panic!("the real conditional save must conflict")
    };
    assert_eq!(error.code, meguri_testkit::cloud::ErrorCode::ServerRecordChanged);
    error
        .server_record
        .as_deref()
        .expect("actual Service server record")
        .clone()
}

fn assert_same_sdk_ack(
    actual: &meguri_sync::cloud::workflow::EntityFacts,
    before: &meguri_sync::cloud::workflow::EntityFacts,
) {
    assert_eq!(actual.server_erev, before.server_erev);
    assert_eq!(actual.acked_rev, before.acked_rev);
    assert_eq!(actual.acked_system_fields, before.acked_system_fields);
    assert_eq!(actual.acked_row_digest, before.acked_row_digest);
    assert_eq!(actual.acked_payload_digest, before.acked_payload_digest);
    assert_eq!(actual.acked_payload, before.acked_payload);
    assert_eq!(actual.acked_envelope, before.acked_envelope);
    assert_eq!(actual.base_payload, before.base_payload);
}

#[test]
fn sdk_inline_save_conflict_keeps_ack_until_wal_then_resends_disjoint_field_merge() {
    use meguri_sync::cloud::{InboxSource, testing::IncomingStage};
    use meguri_testkit::cloud::Mutation;

    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let base = notes(&mut pair.receiver)[0].clone();
    let id = base.id;
    let key = name(&pair, codec::Kind::Note, id);
    let base_fields: Vec<_> = base.flds.split(|byte| *byte == 0x1f).collect();
    let mut sdk = reopen_incoming_sdk(&mut pair);
    sdk.advance_clock_ms(20_000).unwrap();
    sdk.driver()
        .edit_note_fields(id, &[std::str::from_utf8(base_fields[0]).unwrap(), "inline local A"])
        .unwrap();
    let local = sdk_note(&mut sdk, id);
    sdk.driver()
        .hold_incoming_stage(IncomingStage::BeforeCollectionSave)
        .unwrap();
    // The builder persists the original flight before this real child latch.
    // No transport has run, and resuming uses that same retained build command.
    assert!(sdk.request_collection_batch_response(&mut pair.service).is_err());
    assert!(sdk.driver().is_incoming_stage_held(IncomingStage::BeforeCollectionSave));
    let handed = sdk.entity_facts(&key).unwrap();
    assert!(handed.in_flight.iter().any(|flight| flight.payload == local));

    pair.sender.advance_clock_ms(7_000).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(id, &["inline remote Q", std::str::from_utf8(base_fields[1]).unwrap()])
        .unwrap();
    let remote = note(&mut pair.sender, id);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let server = saved(&pair, &key);
    assert!(envelope(&server).payload.is_some());
    assert!(!server.fields.contains_key("pa"));
    assert_eq!(body(&server), remote);
    assert_ne!(envelope(&server).fields.writer, b.0);
    assert_eq!(envelope(&server).fields.origin, handed.origin);

    sdk.driver()
        .release_incoming_stage(IncomingStage::BeforeCollectionSave)
        .unwrap();
    let batch = sdk.request_collection_batch_response(&mut pair.service).unwrap();
    let Mutation::Save { if_unchanged, .. } = batch
        .mutations
        .iter()
        .find(|mutation| matches!(mutation, Mutation::Save { record, .. } if record.name == key))
        .unwrap()
    else {
        panic!("original SDK save")
    };
    assert_eq!(if_unchanged.as_ref().unwrap().bytes(), handed.acked_system_fields);
    assert_ne!(if_unchanged.as_ref().unwrap(), &server.system_fields);
    assert_eq!(conditional_server_record(&batch.response, &key), server);
    sdk.deliver_sent_event(&batch.response).unwrap();
    // Baseline red: deliver_sent_event drops this server record as Other.
    let inbox = sdk.driver().inbox_entries(b).unwrap();
    assert_eq!(inbox.len(), 1, "normal SDK conflict must enter the durable inbox");
    assert_eq!(inbox[0].record_name, key);
    assert_eq!(inbox[0].source, InboxSource::SaveConflict);
    assert_eq!(inbox[0].reason, InboxReason::Ready);
    assert_eq!(inbox[0].erev, envelope(&server).fields.erev);
    assert_same_sdk_ack(&sdk.entity_facts(&key).unwrap(), &handed);
    assert_eq!(sdk_note(&mut sdk, id), local);
    sdk.return_sent_handler().unwrap();

    sdk.driver().pause_automatic_wal_flush_for_testing(true).unwrap();
    sdk.driver()
        .hold_incoming_stage(IncomingStage::ApplyAfterIntent)
        .unwrap();
    assert!(matches!(
        sdk.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Paused(IncomingStage::ApplyAfterIntent)
    ));
    let intent = sdk.driver().apply_intents(b).unwrap().remove(0);
    assert_eq!(intent.inbox_ids, vec![inbox[0].id]);
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, key);
    assert_eq!(intent.keys[0].role, ApplyKeyRole::Entry);
    assert_eq!(intent.keys[0].before_digest, Some(local.row_digest().unwrap()));
    assert!(intent.keys[0].own_flight.is_none());
    assert!(intent.result_seq.is_none());
    assert_eq!(sdk_note(&mut sdk, id), local);
    assert_same_sdk_ack(&sdk.entity_facts(&key).unwrap(), &handed);

    sdk.driver()
        .hold_incoming_stage(IncomingStage::ApplyAfterCommit)
        .unwrap();
    sdk.driver()
        .release_incoming_stage(IncomingStage::ApplyAfterIntent)
        .unwrap();
    assert!(matches!(
        sdk.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Paused(IncomingStage::ApplyAfterCommit)
    ));
    let merged = sdk_note(&mut sdk, id);
    let Payload::Note(merged_note) = &merged else {
        panic!("merged Note row")
    };
    assert_eq!(merged_note.flds, b"inline remote Q\x1finline local A");
    assert_ne!(merged, local);
    assert_ne!(merged, remote);
    assert_eq!(intent.keys[0].after_digest, Some(merged.row_digest().unwrap()));
    let committed = sdk.driver().incoming_observation(b).unwrap();
    let run = committed.runs.iter().find(|run| run.intent_id == intent.id).unwrap();
    assert_eq!(run.entry_ids, intent.inbox_ids);
    assert_eq!(run.changed_keys, BTreeSet::from([key.clone()]));
    assert_eq!(sdk.driver().apply_intents(b).unwrap()[0].result_seq, Some(run.seq));
    assert!(!committed.wal_receipts.iter().any(|receipt| {
        receipt.native_file == run.native_file && receipt.generation == run.generation && receipt.through >= run.seq
    }));
    assert_same_sdk_ack(&sdk.entity_facts(&key).unwrap(), &handed);
    assert_eq!(sdk.driver().inbox_entries(b).unwrap(), inbox);

    sdk.driver()
        .hold_incoming_stage(IncomingStage::ApplyAfterFlush)
        .unwrap();
    sdk.driver()
        .release_incoming_stage(IncomingStage::ApplyAfterCommit)
        .unwrap();
    assert!(matches!(
        sdk.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Paused(IncomingStage::ApplyAfterFlush)
    ));
    let flushed = sdk.driver().incoming_observation(b).unwrap();
    assert!(flushed.wal_receipts.iter().any(|receipt| {
        receipt.native_file == run.native_file && receipt.generation == run.generation && receipt.through >= run.seq
    }));
    assert_same_sdk_ack(&sdk.entity_facts(&key).unwrap(), &handed);
    sdk.driver()
        .release_incoming_stage(IncomingStage::ApplyAfterFlush)
        .unwrap();
    sdk.driver().drain_incoming(STEPS).unwrap();
    sdk.driver().pause_automatic_wal_flush_for_testing(false).unwrap();
    assert!(sdk.driver().inbox_entries(b).unwrap().is_empty());
    assert!(sdk.driver().apply_intents(b).unwrap().is_empty());
    let pending = sdk.entity_facts(&key).unwrap();
    assert_eq!(pending.acked_system_fields, server.system_fields.bytes());
    assert_eq!(pending.server_erev, envelope(&server).fields.erev);
    assert_eq!(pending.acked_payload_digest, remote.payload_digest().unwrap());
    assert_eq!(pending.acked_payload.as_ref(), Some(&remote));
    assert_eq!(pending.base_payload.as_ref(), Some(&remote));
    assert!(pending.mark.is_some());
    assert_eq!(sdk_note(&mut sdk, id), merged);

    // No fetch may repair the conflict before the pending merged resend.
    let resend = sdk.request_collection_batch_response(&mut pair.service).unwrap();
    let sent = saved(&pair, &key);
    assert_eq!(body(&sent), merged);
    assert_eq!(envelope(&sent).fields.writer, b.0);
    sdk.deliver_sent_event(&resend.response).unwrap();
    sdk.return_sent_handler().unwrap();
    assert!(sdk.entity_facts(&key).unwrap().mark.is_none());
    sdk.close_clean().unwrap();
    pair.sender.close_clean().unwrap();
}

#[test]
fn sdk_inline_same_content_from_another_writer_is_save_conflict_not_own_ack() {
    use meguri_sync::cloud::{InboxSource, testing::IncomingStage};

    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let id = notes(&mut pair.receiver)[0].id;
    let key = name(&pair, codec::Kind::Note, id);
    let mut sdk = reopen_incoming_sdk(&mut pair);
    // Both actual writers use the same legal modified stamp and field values.
    sdk.advance_clock_ms(7_000).unwrap();
    pair.sender.advance_clock_ms(7_000).unwrap();
    sdk.driver()
        .edit_note_fields(id, &["equal inline Q", "equal inline A"])
        .unwrap();
    let local = sdk_note(&mut sdk, id);
    sdk.driver()
        .hold_incoming_stage(IncomingStage::BeforeCollectionSave)
        .unwrap();
    assert!(sdk.request_collection_batch_response(&mut pair.service).is_err());
    assert!(sdk.driver().is_incoming_stage_held(IncomingStage::BeforeCollectionSave));
    let handed = sdk.entity_facts(&key).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(id, &["equal inline Q", "equal inline A"])
        .unwrap();
    assert_eq!(
        note(&mut pair.sender, id),
        local,
        "independently edited real rows must match"
    );
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    sdk.driver()
        .release_incoming_stage(IncomingStage::BeforeCollectionSave)
        .unwrap();
    let batch = sdk.request_collection_batch_response(&mut pair.service).unwrap();
    let server = conditional_server_record(&batch.response, &key);
    let remote = envelope(&server);
    assert!(remote.payload.is_some());
    assert_eq!(body(&server), local);
    assert_eq!(remote.fields.origin, handed.origin);
    assert_ne!(remote.fields.writer, b.0);
    assert!(
        handed
            .in_flight
            .iter()
            .any(|flight| flight.payload_digest == remote.payload_sha256)
    );

    sdk.deliver_sent_event(&batch.response).unwrap();
    let inbox = sdk.driver().inbox_entries(b).unwrap();
    assert_eq!(
        inbox.len(),
        1,
        "equal content from another binding still needs normal inbox apply"
    );
    assert_eq!(inbox[0].source, InboxSource::SaveConflict);
    assert_eq!(inbox[0].reason, InboxReason::Ready);
    assert_same_sdk_ack(&sdk.entity_facts(&key).unwrap(), &handed);
    sdk.return_sent_handler().unwrap();
    sdk.driver()
        .hold_incoming_stage(IncomingStage::ApplyAfterIntent)
        .unwrap();
    assert!(matches!(
        sdk.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Paused(IncomingStage::ApplyAfterIntent)
    ));
    let intent = sdk.driver().apply_intents(b).unwrap().remove(0);
    assert_eq!(intent.inbox_ids, vec![inbox[0].id]);
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, key);
    assert!(
        intent.keys[0].own_flight.is_none(),
        "writer identity is required for own ACK"
    );
    assert_same_sdk_ack(&sdk.entity_facts(&key).unwrap(), &handed);
    sdk.driver()
        .release_incoming_stage(IncomingStage::ApplyAfterIntent)
        .unwrap();
    sdk.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(sdk_note(&mut sdk, id), local);
    let ack = sdk.entity_facts(&key).unwrap();
    assert_eq!(ack.acked_system_fields, server.system_fields.bytes());
    assert!(ack.mark.is_none());
    sdk.close_clean().unwrap();
    pair.sender.close_clean().unwrap();
}

#[test]
fn sdk_own_landed_inline_conflict_acks_historical_flight_and_keeps_newer_local_edit() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let id = notes(&mut pair.receiver)[0].id;
    let key = name(&pair, codec::Kind::Note, id);
    let mut sdk = reopen_incoming_sdk(&mut pair);
    sdk.advance_clock_ms(7_000).unwrap();
    sdk.driver().edit_note_fields(id, &["lost inline Q", "back"]).unwrap();
    let lost_row = sdk_note(&mut sdk, id);
    let old_ack = sdk.entity_facts(&key).unwrap();
    let lost = sdk.lose_collection_batch_reply(&mut pair.service).unwrap();
    let Ok(Reply::Modified(items)) = &lost.response.result else {
        panic!("actual lost save result")
    };
    let Ok(MutationSuccess::Saved(landed)) = &items.iter().find(|item| item.name == key).unwrap().result else {
        panic!("original save really landed")
    };
    let historical = envelope(landed);
    assert!(historical.payload.is_some());
    assert_eq!(historical.fields.writer, b.0);
    assert_eq!(historical.fields.origin, old_ack.origin);
    assert_eq!(body(landed), lost_row);
    let outstanding = sdk.entity_facts(&key).unwrap();
    assert!(outstanding.sent, "actual engine handoff persists sent");
    assert_same_sdk_ack(&outstanding, &old_ack);
    let historical_flight = outstanding
        .in_flight
        .iter()
        .find(|flight| flight.payload_digest == historical.payload_sha256)
        .unwrap()
        .clone();
    assert_eq!(historical_flight.erev, historical.fields.erev);
    assert!(historical_flight.payload == lost_row);
    assert!(sdk.send_changes_calls().last().unwrap().returned);

    // A legal host edit after the lost result, with a separate undo run.
    sdk.advance_clock_ms(61_001).unwrap();
    sdk.driver()
        .edit_note_fields(id, &["newer inline Q", "newer inline A"])
        .unwrap();
    let current = sdk_note(&mut sdk, id);
    assert_ne!(current, lost_row);
    assert!(sdk.entity_facts(&key).unwrap().sent);
    let retry = sdk.request_collection_batch_response(&mut pair.service).unwrap();
    let returned = conditional_server_record(&retry.response, &key);
    assert_eq!(&returned, landed);
    assert_eq!(body(&returned), lost_row);
    let handed = sdk.entity_facts(&key).unwrap();
    assert!(handed.local_rev > historical_flight.local_rev);
    assert!(handed.in_flight.contains(&historical_flight));
    assert!(
        handed
            .in_flight
            .iter()
            .any(|flight| { flight.local_rev > historical_flight.local_rev && flight.payload == current })
    );
    assert_same_sdk_ack(&handed, &old_ack);
    let rows = sdk.driver().collection_rows().unwrap();
    let before = sdk.driver().incoming_observation(b).unwrap();

    sdk.deliver_sent_event(&retry.response).unwrap();
    // Baseline red: the ordinary SDK path currently drops the historical ACK.
    let ack = sdk.entity_facts(&key).unwrap();
    assert_eq!(
        ack.acked_system_fields,
        landed.system_fields.bytes(),
        "ACK the actual older landed flight"
    );
    assert!(ack.sent, "retiring the historical flight must retain sent");
    assert_eq!(ack.server_erev, historical.fields.erev);
    assert_eq!(ack.acked_payload_digest, historical.payload_sha256);
    assert_eq!(ack.acked_row_digest, lost_row.row_digest().unwrap());
    assert_eq!(ack.acked_payload.as_ref(), Some(&lost_row));
    assert_eq!(ack.base_payload.as_ref(), Some(&lost_row));
    assert_eq!(ack.local_rev, handed.local_rev);
    assert!(ack.acked_rev < ack.local_rev);
    assert!(ack.mark.is_some());
    let newer_flights: Vec<_> = handed
        .in_flight
        .iter()
        .filter(|flight| flight.local_rev > historical_flight.local_rev)
        .cloned()
        .collect();
    assert!(ack.in_flight == newer_flights);
    sdk.return_sent_handler().unwrap();
    sdk.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(sdk.driver().collection_rows().unwrap(), rows);
    assert_eq!(sdk_note(&mut sdk, id), current);
    assert!(sdk.driver().inbox_entries(b).unwrap().is_empty());
    assert!(sdk.driver().apply_intents(b).unwrap().is_empty());
    let after = sdk.driver().incoming_observation(b).unwrap();
    assert_eq!(
        after.seq, before.seq,
        "historical conditional ACK submits no collection write"
    );
    assert_eq!(after.native_file, before.native_file);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.col_mod, before.col_mod);
    assert_eq!(after.undo_depth, before.undo_depth);
    assert_eq!(after.change_events, before.change_events);
    assert_eq!(after.runs, before.runs);
    assert!(sdk.core_tracking_sendables().unwrap().contains_key(&key));

    let resend = sdk.request_collection_batch_response(&mut pair.service).unwrap();
    let saved_current = saved(&pair, &key);
    assert_eq!(body(&saved_current), current);
    sdk.deliver_sent_event(&resend.response).unwrap();
    sdk.return_sent_handler().unwrap();
    let settled = sdk.entity_facts(&key).unwrap();
    assert!(settled.sent && settled.mark.is_none() && settled.in_flight.is_empty());
    sdk.advance_clock_ms(61_001).unwrap();
    sdk.driver()
        .edit_note_fields(id, &["post ACK Q", "post ACK A"])
        .unwrap();
    let edited = sdk.entity_facts(&key).unwrap();
    assert!(edited.sent && edited.mark.is_some() && edited.in_flight.is_empty());
    sdk.close_clean().unwrap();
    let mut reopened = meguri_testkit::cloud::adapter::SDKRig::open_existing_collection(
        &pair.receiver_dir,
        &scope(),
        NOW + 130_000,
        809,
        Arc::new(NoReopenKeys),
    )
    .unwrap();
    let durable = reopened.entity_facts(&key).unwrap();
    assert!(durable.sent && durable.in_flight.is_empty());
    assert!(durable == edited);
    reopened.close_clean().unwrap();
    pair.sender.close_clean().unwrap();
}

#[test]
fn asset_intake_preserves_pending_version_when_revision_or_origin_is_invalid() {
    use meguri_testkit::cloud::{Mutation, Operation, RecordInput};

    let (mut pair, id, key, before, _) = changed_note();
    let b = binding(&pair);
    let older = saved(&pair, &key);
    let old_ack = pair.receiver.facts().unwrap().entities[&key].clone();
    let field = "n".repeat(codec::INLINE_ENVELOPE_BYTES + 1);
    pair.sender
        .driver()
        .edit_note_fields(id, &[&field, "newer asset A"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let newer = saved(&pair, &key);
    let newest = envelope(&newer);
    assert!(newest.payload.is_none());
    assert!(newest.fields.erev > envelope(&older).fields.erev);
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&newer, None)
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let waiting = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].reason, InboxReason::Asset);
    assert_eq!(waiting[0].erev, newest.fields.erev);
    let initial = pair.receiver.driver().incoming_observation(b).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_asset_download(&older, None)
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    // A real conditional Service save with another origin and a higher erev.
    // Its owned bytes pass asset intake; id-keyed origin safety must still win.
    let mut fields = newest.fields.clone();
    fields.origin[0] ^= 1;
    fields.erev += 1;
    let wrong = codec::Entity::build(fields.clone(), &body(&newer).encode().unwrap()).unwrap();
    let mut record = RecordInput {
        name: newer.name.clone(),
        record_type: newer.record_type.clone(),
        fields: newer.fields.clone(),
    };
    record
        .fields
        .insert("e".into(), Field::EncryptedBytes(wrong.encode().unwrap()));
    let response = pair
        .service
        .request(
            &scope(),
            Default::default(),
            Operation::Modify {
                zone: pair.sender.collection_zone().unwrap(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record,
                    if_unchanged: Some(newer.system_fields.clone()),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("real modify")
    };
    let Ok(MutationSuccess::Saved(wrong)) = &items[0].result else {
        panic!("real save")
    };
    let Field::Asset(asset) = &wrong.fields["pa"] else {
        panic!("real pa")
    };
    let mut downloaded = tempfile::NamedTempFile::new().unwrap();
    downloaded.write_all(asset.bytes()).unwrap();
    downloaded.as_file().sync_all().unwrap();
    let request = pair
        .receiver
        .driver()
        .asset_read_requests(asset_retry())
        .unwrap()
        .remove(0);
    let selected = pair.receiver.driver().inbox_entries(b).unwrap();
    let files_before = inbox_asset_files(&pair);
    pair.receiver.driver().hold_asset_read_finishing().unwrap();
    assert!(
        pair.receiver
            .driver()
            .asset_read_result(&request, Some(wrong), Some(downloaded.path()))
            .is_err()
    );
    // The rejected intake has prepared valid bytes. Inspect its temporary file
    // while finalization waits with runtime and tracking access released.
    assert_eq!(inbox_asset_files(&pair).len(), files_before.len() + 1);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), selected);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert_eq!(pair.receiver.driver().refused_entries(b).unwrap().len(), 1);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    assert_eq!(note(&mut pair.receiver, id), before);
    let finishing = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(finishing.seq, initial.seq);
    assert_eq!(finishing.change_events, initial.change_events);
    pair.receiver.driver().release_asset_read_finishing().unwrap();
    assert!(
        !pair
            .receiver
            .driver()
            .asset_read_result(&request, Some(wrong), Some(downloaded.path()))
            .unwrap()
    );
    assert_eq!(inbox_asset_files(&pair), files_before);
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), selected);
    // Verify the origin rejection recorded by this explicit read before any
    // subsequent fetch can contribute another refusal entry.
    assert_eq!(inbox_asset_files(&pair).len(), 0);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), selected);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let refused = pair.receiver.driver().refused_entries(b).unwrap();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].record_name, key);
    assert_eq!(refused[0].origin, Some(fields.origin));
    assert_eq!(refused[0].erev, fields.erev);
    assert_eq!(refused[0].reason, "origin_mismatch");
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    let unchanged = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(unchanged.seq, initial.seq);
    assert_eq!(unchanged.runs, initial.runs);
    assert_eq!(unchanged.change_events, initial.change_events);
    assert_eq!(pair.receiver.kill().unwrap().signal, Some(9));
    pair.reopen_before_identity();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), waiting);
    assert_eq!(pair.receiver.driver().refused_entries(b).unwrap(), refused);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert_eq!(note(&mut pair.receiver, id), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old_ack);
    pair.close();
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
        .deliver_collection_records(std::slice::from_ref(&first))
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
        .deliver_collection_records(std::slice::from_ref(repaired))
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
            .request(&scope(), request.caller, request.operation.clone())
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
            // The original builder persisted its flight before this latch, but
            // still owns its reply. Consume it without driving SDK transport.
            pair.receiver.release(Point::BeforeCollectionSave).unwrap();
            let original = pair.receiver.driver().complete_pending_build().unwrap();
            let original_save = original.saves.iter().find(|save| save.name == key).unwrap();
            let meguri_testkit::cloud::Operation::Modify { changes, .. } = &request.operation else {
                panic!("original collection save request")
            };
            let requested_envelope = changes
                .iter()
                .find_map(|change| match change {
                    meguri_testkit::cloud::Mutation::Save { record, .. } if record.name == key => {
                        Some(&record.fields["e"])
                    }
                    _ => None,
                })
                .unwrap();
            assert!(matches!(requested_envelope, Field::EncryptedBytes(bytes) if *bytes == original_save.envelope));
            assert!(matches!(&landed.fields["e"], Field::EncryptedBytes(bytes) if *bytes == original_save.envelope));
            let state = pair.receiver.driver().incoming_observation(b).unwrap();
            let batch = pair
                .receiver
                .driver()
                .build_for_instance(state.engine_instance.unwrap(), BTreeSet::from([key.clone()]))
                .unwrap();
            assert!(batch.saves.iter().any(|s| s.name == key));
            let save = batch.saves.iter().find(|save| save.name == key).unwrap();
            let codec::DecodeOutcome::Supported(newer) = codec::Entity::decode(&save.envelope).unwrap() else {
                panic!("supported newer save")
            };
            assert_eq!(
                Payload::decode(newer.fields.kind, newer.payload.as_deref().unwrap()).unwrap(),
                current
            );
            assert_eq!(newer.payload_sha256, current.payload_digest().unwrap());
            let persisted = pair.receiver.facts().unwrap().entities[&key].clone();
            assert!(persisted.in_flight.iter().any(|flight| {
                flight.local_rev > landed_revision
                    && flight.erev == newer.fields.erev
                    && flight.payload_digest == newer.payload_sha256
                    && flight.payload == current
            }));
        }
        let flights = pair.receiver.facts().unwrap().entities[&key].in_flight.clone();
        pair.receiver.kill().unwrap(); // real lost reply
        pair.reopen_before_identity();
        let before_apply = pair.receiver.driver().incoming_observation(b).unwrap();
        let current_rows = pair.receiver.driver().collection_rows().unwrap();
        pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
        pair.verify_and_drive();
        // Manual mode needs an explicit fetch of the save whose reply was lost.
        pair.receiver
            .sync_now_and_drain(&mut pair.service, STEPS)
            .unwrap_or_else(|error| {
                panic!(
                    "own_landed_save action={action} phase={}: {error:?}",
                    pair.receiver.last_sync_phase()
                )
            });
        assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
        assert_eq!(note(&mut pair.receiver, id), current);
        let state = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(state.col_mod, before_apply.col_mod);
        assert_eq!(state.undo_depth, before_apply.undo_depth);
        assert_eq!(state.change_events, before_apply.change_events);
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), current_rows);
        let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
        let successful_names: BTreeSet<_> = items
            .iter()
            .filter_map(|item| match &item.result {
                Ok(MutationSuccess::Saved(record)) => Some(record.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            intent
                .keys
                .iter()
                .map(|intent_key| intent_key.name.as_str())
                .collect::<BTreeSet<_>>(),
            successful_names,
            "own_landed_save action={action}: intent keys match actual successful saves"
        );
        assert_eq!(
            intent.keys.len(),
            successful_names.len(),
            "own_landed_save action={action}: no duplicate intent keys"
        );
        for intent_key in &intent.keys {
            assert_eq!(intent_key.role, ApplyKeyRole::Entry, "own_landed_save action={action}");
            assert!(intent_key.before_digest.is_some(), "own_landed_save action={action}");
            assert_eq!(
                intent_key.before_digest, intent_key.after_digest,
                "own_landed_save action={action}"
            );
            assert!(intent_key.own_flight.is_some(), "own_landed_save action={action}");
        }
        let target_key = intent
            .keys
            .iter()
            .find(|intent_key| intent_key.name == key)
            .unwrap_or_else(|| panic!("own_landed_save action={action}: missing target Note key"));
        assert_eq!(
            target_key.before_digest,
            Some(current.row_digest().unwrap()),
            "own_landed_save action={action}"
        );
        assert_eq!(
            target_key.after_digest,
            Some(current.row_digest().unwrap()),
            "own_landed_save action={action}"
        );
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
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
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
    // Manual mode needs an explicit fetch of the remote built on the lost save.
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
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

#[test]
fn detached_review_rows_and_incoming_held_names_wait_for_real_session_end() {
    let (mut pair, id, key, _, _) = changed_note();
    let session = pair.receiver.driver().review_begin(id).unwrap();
    let card = pair.receiver.driver().review_start(session).unwrap();
    pair.receiver.driver().review_show_answer(session, card.id).unwrap();
    let answer = pair.receiver.driver().review_answer_good(session, card.id).unwrap();
    pair.receiver
        .driver()
        .edit_note_fields(id, &["held local", "back"])
        .unwrap();
    let held = pair.receiver.driver().read_cloud_review_hold(session).unwrap();
    assert!(held.op_ids.contains(&answer.op_id));
    let remote = saved(&pair, &key);
    pair.receiver.driver().deliver_collection_records(&[remote]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let b = binding(&pair);
    settled(&mut pair, b);
    assert!(
        pair.receiver
            .driver()
            .read_cloud_review_hold(session)
            .unwrap()
            .op_ids
            .is_empty()
    );
    // A later local full update cannot release the incoming-held name. The
    // detached answer still supplies absent Revlog and the old Card history.
    pair.receiver.advance_clock_ms(61_001).unwrap();
    pair.receiver
        .driver()
        .edit_note_fields(id, &["later local", "back"])
        .unwrap();
    let card_name = name(&pair, codec::Kind::Card, card.id);
    let revlog_name = name(&pair, codec::Kind::Revlog, answer.revlog_id);
    let requested = BTreeSet::from([key.clone(), card_name.clone(), revlog_name.clone()]);
    let instance = pair
        .receiver
        .driver()
        .incoming_observation(b)
        .unwrap()
        .engine_instance
        .unwrap();
    let batch = pair
        .receiver
        .driver()
        .build_for_instance(instance.clone(), requested.clone())
        .unwrap();
    assert!(batch.saves.is_empty());
    pair.receiver.driver().review_end_and_drain(session).unwrap();
    let released = pair.receiver.driver().build_for_instance(instance, requested).unwrap();
    let names: BTreeSet<_> = released.saves.iter().map(|save| save.name.clone()).collect();
    assert!(names.contains(&key));
    assert!(names.contains(&card_name));
    assert!(names.contains(&revlog_name));
    pair.close();
}

#[test]
fn actual_delete_undo_before_handoff_restores_clean_note_and_cards() {
    let mut pair = Pair::adopted();
    let id = notes(&mut pair.sender)[0].id;
    let before = pair.sender.driver().collection_rows().unwrap();
    let note_name = name(&pair, codec::Kind::Note, id);
    let original = saved(&pair, &note_name);
    pair.sender.driver().delete_note(id).unwrap();
    let deleted = pair.sender.facts().unwrap().entities[&note_name].clone();
    assert!(deleted.tombstone_at.is_some());
    let sender_binding = pair.sender.facts().unwrap().current.unwrap().id;
    let committed = pair.sender.driver().incoming_observation(sender_binding).unwrap();
    assert_eq!(deleted.tombstone_at, Some(committed.col_mod));
    for row in before.iter().filter(|p| matches!(p, Payload::Card(c) if c.nid == id)) {
        let Payload::Card(card) = row else { unreachable!() };
        let key = name(&pair, codec::Kind::Card, card.id);
        let tracked = pair.sender.facts().unwrap().entities[&key].clone();
        assert_eq!(tracked.tombstone_at, Some(committed.col_mod));
        assert!(tracked.payload == *row);
        assert!(tracked.base_payload.as_ref() == Some(row));
    }
    assert!(deleted.base_payload.as_ref() == before.iter().find(|p| matches!(p, Payload::Note(n) if n.id == id)));
    pair.sender.driver().undo().unwrap();
    let restored = pair.sender.driver().collection_rows().unwrap();
    for payload in before
        .iter()
        .filter(|p| matches!(p, Payload::Note(n) if n.id == id) || matches!(p, Payload::Card(c) if c.nid == id))
    {
        assert!(restored.contains(payload));
    }
    let entity = pair.sender.facts().unwrap().entities[&note_name].clone();
    assert!(entity.tombstone_at.is_none());
    assert!(entity.mark.is_none());
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(saved(&pair, &note_name), original);
    pair.close();
}

#[test]
fn pending_delete_reopen_keeps_tracking_and_counts_live_rows() {
    let mut pair = Pair::adopted();
    let id = notes(&mut pair.receiver)[0].id;
    let key = name(&pair, codec::Kind::Note, id);
    pair.receiver.driver().delete_note(id).unwrap();
    let absent = pair.receiver.driver().collection_rows().unwrap();
    let deleted = pair
        .receiver
        .facts()
        .unwrap()
        .entities
        .into_iter()
        .filter(|(_, e)| e.tombstone_at.is_some())
        .collect::<Vec<_>>();
    assert!(deleted.iter().any(|(name, _)| name == &key));
    assert!(deleted.iter().any(|(_, e)| e.kind == codec::Kind::Card));
    for (_, e) in &deleted {
        assert!(e.mark.is_some() && e.in_flight.is_empty());
        assert!(e.base_payload.is_some());
    }
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    pair.verify_and_drive();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    let reopened = pair.receiver.facts().unwrap();
    for (name, before) in deleted {
        assert!(reopened.entities[&name] == before);
    }
    assert_scan_live_counts(&pair.receiver, &absent);
    pair.close();
}

fn assert_scan_live_counts(rig: &PublicationRig, rows: &[Payload]) {
    use meguri_sync::cloud::workflow::{Observation, TraceEvent};
    assert!(
        rig.trace()
            .iter()
            .any(|event| matches!(event, TraceEvent::ScanCommitted { .. }))
    );
    for kind in [codec::Kind::Note, codec::Kind::Card] {
        let expected = rows.iter().filter(|p| p.kind() == kind).count();
        assert!(rig.observations().iter().any(|event| matches!(event,
            Observation::ReconcileKindCompleted { kind: actual, row_count }
                if *actual == kind && *row_count == expected)));
    }
}

#[test]
fn held_actual_delete_waits_then_saves_empty_conditional_tombstones_and_reopens() {
    let mut pair = Pair::adopted();
    pair.sender.close_clean().unwrap();
    pair.sender = PublicationRig::open(
        &pair.root.path().join("sender"),
        &scope(),
        IdentityReads::Available(scope().account),
        true,
        NOW,
        808,
        Arc::new(NoReopenKeys),
    )
    .unwrap();
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    let id = notes(&mut pair.sender)[0].id;
    // P8.1/M2: drain initial deck selection before the held-review baseline.
    let setup_session = pair.sender.driver().review_begin(id).unwrap();
    pair.sender.driver().review_end_and_drain(setup_session).unwrap();
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    let note_name = name(&pair, codec::Kind::Note, id);
    let cards: Vec<_> = pair
        .sender
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .filter_map(|p| match p {
            Payload::Card(c) if c.nid == id => Some(c),
            _ => None,
        })
        .collect();
    let record_key = pair.sender.facts().unwrap().current.unwrap().facts.record_key;
    let config_name = codec::entity_name(&record_key.0, codec::Kind::Config, b"curDeck").unwrap();
    let Payload::Config(current) = body(&saved(&pair, &config_name)) else {
        panic!("Service selected-deck Config required")
    };
    assert_eq!(current.key, b"curDeck");
    assert_eq!(current.val, cards.first().unwrap().did.to_string().as_bytes());
    let baseline = saved(&pair, &note_name);
    let base_entity = envelope(&baseline);
    let after_baseline = pair.service.request_log().unwrap().last().unwrap().request_id;
    let session = pair.sender.driver().review_begin(id).unwrap();
    let question = pair.sender.driver().review_start(session).unwrap();
    pair.sender.driver().review_show_answer(session, question.id).unwrap();
    pair.sender.driver().review_answer_good(session, question.id).unwrap();
    pair.sender.driver().delete_note(id).unwrap();
    let held = pair.sender.driver().read_cloud_review_hold(session).unwrap();
    assert!(held.entity_names.contains(&note_name));
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(
        !pair
            .service
            .request_log()
            .unwrap()
            .iter()
            .any(|r| { r.request_id > after_baseline && r.kind == meguri_testkit::cloud::RequestKind::Modify })
    );
    let facts = pair.sender.facts().unwrap();
    let pending = &facts.entities[&note_name];
    assert!(pending.tombstone_at.is_some() && pending.mark.is_some());
    assert!(pending.payload == body(&baseline));
    assert_eq!(
        pending.acked_envelope.0,
        match &baseline.fields["e"] {
            Field::EncryptedBytes(bytes) => bytes.clone(),
            _ => panic!("baseline envelope"),
        }
    );
    assert!(pending.in_flight.is_empty());
    let sender_binding = facts.current.unwrap().id;
    let instance = pair
        .sender
        .driver()
        .incoming_observation(sender_binding)
        .unwrap()
        .engine_instance
        .unwrap();
    let mut requested = BTreeSet::from([note_name.clone()]);
    for card in &cards {
        requested.insert(name(&pair, codec::Kind::Card, card.id));
    }
    let batch = pair.sender.driver().build_for_instance(instance, requested).unwrap();
    assert!(batch.saves.is_empty());
    assert_eq!(saved(&pair, &note_name), baseline);
    for card in &cards {
        assert!(
            !envelope(&saved(&pair, &name(&pair, codec::Kind::Card, card.id)))
                .fields
                .tombstone
        );
    }
    pair.sender.driver().review_end_and_drain(session).unwrap();
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    let deleted = envelope(&saved(&pair, &note_name));
    assert!(deleted.fields.tombstone);
    assert_eq!(deleted.payload.as_deref(), Some([].as_slice()));
    assert_eq!(deleted.fields.erev, base_entity.fields.erev + 1);
    assert_eq!(deleted.fields.base_sha256, Some(base_entity.payload_sha256));
    for card in cards {
        let key = name(&pair, codec::Kind::Card, card.id);
        let entity = envelope(&saved(&pair, &key));
        assert!(entity.fields.tombstone);
        assert_eq!(entity.payload.as_deref(), Some([].as_slice()));
    }
    assert!(pair.sender.driver().undo().is_err());
    pair.sender.close_clean().unwrap();
    pair.sender = online(&pair.root.path().join("sender"), false);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let facts = pair.sender.facts().unwrap().entities[&note_name].clone();
    assert!(facts.tombstone_at.is_some());
    assert!(facts.mark.is_none());
    assert_eq!(facts.acked_payload_digest, codec::sha256(&[]));
    pair.close();
}

#[test]
fn incoming_note_tombstone_side_effect_cards_settle_against_actual_server_tombstones() {
    let mut pair = Pair::adopted();
    let id = notes(&mut pair.sender)[0].id;
    let note_name = name(&pair, codec::Kind::Note, id);
    let cards: Vec<_> = pair
        .receiver
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .filter_map(|p| match p {
            Payload::Card(c) if c.nid == id => Some(c),
            _ => None,
        })
        .collect();
    pair.sender.driver().delete_note(id).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let tombstone = saved(&pair, &note_name);
    pair.receiver.driver().deliver_collection_records(&[tombstone]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    for card in &cards {
        let key = name(&pair, codec::Kind::Card, card.id);
        let facts = pair.receiver.facts().unwrap().entities[&key].clone();
        assert!(facts.base_payload == Some(Payload::Card(card.clone())));
        assert!(facts.tombstone_at.is_some());
        assert!(facts.mark.is_some());
    }
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    for card in cards {
        let key = name(&pair, codec::Kind::Card, card.id);
        assert!(envelope(&saved(&pair, &key)).fields.tombstone);
        let facts = pair.receiver.facts().unwrap().entities[&key].clone();
        assert!(facts.mark.is_none());
        assert_eq!(facts.acked_payload_digest, codec::sha256(&[]));
    }
    pair.close();
}

fn mixed_live_card_then_note_tombstone(new_card: bool) {
    mixed_live_card_then_note_tombstone_with_asset(new_card, false);
}

fn mixed_live_card_then_note_tombstone_with_asset(new_card: bool, large: bool) {
    use meguri_testkit::cloud::{Mutation, Operation};

    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let existing = notes(&mut pair.sender)[0].clone();
    let (id, card_id) = if new_card {
        let old_card = pair
            .sender
            .driver()
            .collection_rows()
            .unwrap()
            .into_iter()
            .find_map(|p| match p {
                Payload::Card(c) if c.nid == existing.id => Some(c),
                _ => None,
            })
            .unwrap();
        pair.sender.advance_clock_ms(7_000).unwrap();
        let added = pair
            .sender
            .driver()
            .add_note(existing.mid, old_card.did, &["new remote", "back"])
            .unwrap();
        if large {
            pair.sender
                .driver()
                .write_card_data(
                    added.cards[0].id,
                    format!(r#"{{"fixture":"{}"}}"#, "c".repeat(codec::INLINE_ENVELOPE_BYTES + 1)),
                )
                .unwrap();
        }
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let note_key = name(&pair, codec::Kind::Note, added.note.id);
        // Download the real Note alone; its Card has never had tracking here.
        let live_note = saved(&pair, &note_key);
        pair.receiver.driver().deliver_collection_records(&[live_note]).unwrap();
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        settled(&mut pair, b);
        (added.note.id, added.cards[0].id)
    } else {
        let card = pair
            .sender
            .driver()
            .collection_rows()
            .unwrap()
            .into_iter()
            .find_map(|p| match p {
                Payload::Card(c) if c.nid == existing.id => Some(c),
                _ => None,
            })
            .unwrap();
        pair.sender.driver().bury_card(card.id).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        (existing.id, card.id)
    };
    let card_key = name(&pair, codec::Kind::Card, card_id);
    let note_key = name(&pair, codec::Kind::Note, id);
    assert_eq!(
        pair.receiver.facts().unwrap().entities.contains_key(&card_key),
        !new_card
    );
    let live_card = saved(&pair, &card_key);
    let live = envelope(&live_card);
    assert!(!live.fields.tombstone);
    let server_payload = body(&live_card);
    if large {
        assert!(new_card);
        assert!(live.payload.is_none());
        assert!(server_payload.encode().unwrap().len() > codec::INLINE_ENVELOPE_BYTES);
        let Field::Asset(asset) = &live_card.fields["pa"] else {
            panic!("real Service pa")
        };
        assert_eq!(asset.bytes(), server_payload.encode().unwrap());
    }

    pair.sender.driver().delete_note(id).unwrap();
    pair.sender.hold(Point::BeforeCollectionSave).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.sender.is_held(Point::BeforeCollectionSave));
    let request = pair.sender.held_request().unwrap();
    let Operation::Modify { zone, atomic, changes } = request.operation else {
        panic!("actual sender deletion request")
    };
    // Commit only the production-built Note tombstone. The actual Service Card
    // remains LIVE, so the receiver must itself save its conditional tombstone.
    let changes: Vec<_> = changes
        .into_iter()
        .filter(|change| matches!(change, Mutation::Save { record, .. } if record.name == note_key))
        .collect();
    assert_eq!(changes.len(), 1);
    let response = pair
        .service
        .request(&scope(), request.caller, Operation::Modify { zone, atomic, changes })
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual Service reply")
    };
    let Ok(MutationSuccess::Saved(tombstone)) = &items[0].result else {
        panic!("actual Service Note tombstone")
    };
    assert!(envelope(tombstone).fields.tombstone);
    pair.receiver.hold(Point::ApplyAfterIntent).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(&[live_card.clone(), tombstone.clone()])
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterIntent));
    let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
    assert_eq!(intent.inbox_ids.len(), 2);
    let card = intent.keys.iter().find(|key| key.name == card_key).unwrap();
    assert_eq!(card.role, ApplyKeyRole::Entry);
    assert_eq!(card.after_digest, None);
    assert!(card.before_payload.as_ref() == Some(&server_payload));
    pair.receiver.release(Point::ApplyAfterIntent).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    settled(&mut pair, b);
    assert!(
        !pair
            .receiver
            .driver()
            .collection_rows()
            .unwrap()
            .iter()
            .any(|p| { matches!(p, Payload::Note(n) if n.id == id) || matches!(p, Payload::Card(c) if c.nid == id) })
    );
    let pending = pair.receiver.facts().unwrap().entities[&card_key].clone();
    assert!(pending.tombstone_at.is_some());
    assert!(pending.mark.is_some());
    assert!(pending.base_payload.as_ref() == Some(&server_payload));
    assert!(pending.acked_payload.as_ref() == Some(&server_payload));
    assert_eq!(pending.server_erev, live.fields.erev);
    assert_eq!(pending.acked_system_fields, live_card.system_fields.bytes());
    assert_eq!(saved(&pair, &card_key), live_card);

    pair.receiver.hold(Point::BeforeCollectionSave).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeCollectionSave));
    let request = pair.receiver.held_request().unwrap();
    let Operation::Modify { changes, .. } = request.operation else {
        panic!("actual receiver tombstone request")
    };
    let (record, conditional) = changes
        .iter()
        .find_map(|change| match change {
            Mutation::Save { record, if_unchanged } if record.name == card_key => Some((record, if_unchanged)),
            _ => None,
        })
        .unwrap();
    assert_eq!(conditional.as_ref(), Some(&live_card.system_fields));
    let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
        panic!("envelope")
    };
    let codec::DecodeOutcome::Supported(sent) = codec::Entity::decode(bytes).unwrap() else {
        panic!("supported")
    };
    assert!(sent.fields.tombstone);
    assert_eq!(sent.fields.erev, live.fields.erev + 1);
    assert_eq!(sent.fields.base_sha256, Some(live.payload_sha256));
    pair.receiver.release(Point::BeforeCollectionSave).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(envelope(&saved(&pair, &card_key)).fields.tombstone);
    let acked = pair.receiver.facts().unwrap().entities[&card_key].clone();
    assert!(acked.tombstone_at.is_some());
    assert!(acked.mark.is_none());
    assert_eq!(acked.acked_payload_digest, codec::sha256(&[]));
    // Discard the sender's original held batch; it was never submitted whole.
    pair.sender.kill().unwrap();
    pair.sender = online(&pair.root.path().join("sender"), false);
    pair.close();
}

#[test]
fn live_tracked_card_and_note_tombstone_settle_then_save_real_conditional_card_tombstone() {
    mixed_live_card_then_note_tombstone(false);
}

#[test]
fn live_new_card_and_note_tombstone_settle_then_save_real_conditional_card_tombstone() {
    mixed_live_card_then_note_tombstone(true);
}

#[test]
fn live_new_pa_card_and_note_tombstone_settle_then_save_real_conditional_card_tombstone() {
    mixed_live_card_then_note_tombstone_with_asset(true, true);
}

#[test]
fn original_handed_entity_tombstone_save_unknown_item_drops_note_and_cards_durably() {
    use meguri_testkit::cloud::{ErrorCode, LogOutcome, Mutation, Operation, RequestKind, TaggedMissing};
    use std::collections::BTreeMap;

    let mut pair = Pair::adopted();
    let b = binding(&pair);
    assert_eq!(
        pair.service.config(&scope()).unwrap().tagged_missing,
        TaggedMissing::UnknownItem
    );
    let original = pair.receiver.driver().collection_rows().unwrap();
    let originals = notes(&mut pair.receiver);
    let id = originals[0].id;
    let unrelated = name(&pair, codec::Kind::Note, originals[1].id);
    let unrelated_record = saved(&pair, &unrelated);
    let acked = pair.receiver.facts().unwrap().entities;
    let mut deleted = BTreeSet::from([name(&pair, codec::Kind::Note, id)]);
    for row in &original {
        if let Payload::Card(card) = row
            && card.nid == id
        {
            deleted.insert(name(&pair, codec::Kind::Card, card.id));
        }
    }
    assert!(deleted.len() > 1, "the acknowledged Note must have Cards");
    let baselines: BTreeMap<_, _> = deleted.iter().map(|key| (key.clone(), saved(&pair, key))).collect();
    for key in &deleted {
        let entity = &acked[key];
        assert!(entity.server_erev > 0 && entity.acked_rev > 0);
        assert!(entity.mark.is_none() && entity.in_flight.is_empty() && entity.tombstone_at.is_none());
        assert!(!envelope(&baselines[key]).fields.tombstone);
        assert_eq!(entity.acked_system_fields, baselines[key].system_fields.bytes());
    }

    pair.receiver.driver().delete_note(id).unwrap();
    hold_receive(&mut pair, Point::BeforeCollectionSave);
    let request = pair.receiver.held_request().unwrap();
    let Operation::Modify { zone, atomic, changes } = &request.operation else {
        panic!("original production-built tombstone Save request")
    };
    assert_eq!(zone, &pair.receiver.collection_zone().unwrap());
    assert!(!atomic);
    let observation = pair.receiver.driver().incoming_observation(b).unwrap();
    let instance = observation.engine_instance.clone().unwrap();
    assert_eq!(request.caller.instance.as_deref(), Some(instance.as_str()));
    assert!(request.caller.device.is_none() && request.caller.own_operation.is_none());
    let handed = pair.receiver.facts().unwrap().entities;
    assert!(handed[&unrelated] == acked[&unrelated]);
    let mut sent_names = BTreeSet::new();
    for change in changes {
        let Mutation::Save { record, if_unchanged } = change else {
            panic!("MGEntity deletion is a conditional Save, never a Delete")
        };
        assert!(sent_names.insert(record.name.clone()));
        assert!(deleted.contains(&record.name));
        assert_eq!(record.record_type, "MGEntity");
        assert_eq!(if_unchanged.as_ref(), Some(&baselines[&record.name].system_fields));
        let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
            panic!("actual handed envelope")
        };
        let codec::DecodeOutcome::Supported(sent) = codec::Entity::decode(bytes).unwrap() else {
            panic!("supported actual handed envelope")
        };
        let entity = &handed[&record.name];
        let before = envelope(&baselines[&record.name]);
        assert!(matches!(sent.fields.kind, codec::Kind::Note | codec::Kind::Card));
        assert_eq!(sent.fields.kind, entity.kind);
        assert_eq!(sent.fields.origin, entity.origin);
        assert_eq!(sent.fields.writer, b.0);
        assert!(sent.fields.tombstone);
        assert_eq!(sent.fields.erev, before.fields.erev + 1);
        assert_eq!(sent.fields.base_sha256, Some(before.payload_sha256));
        assert_eq!(sent.payload.as_deref(), Some([].as_slice()));
        assert_eq!(sent.payload_sha256, codec::sha256(&[]));
        assert!(!record.fields.contains_key("pa"));
        assert!(entity.sent && entity.tombstone_at.is_some() && entity.mark.is_some());
        assert_eq!(entity.acked_rev, acked[&record.name].acked_rev);
        assert_eq!(entity.acked_envelope.0, acked[&record.name].acked_envelope.0);
        assert_eq!(entity.in_flight.len(), 1);
        let flight = &entity.in_flight[0];
        assert!(flight.tombstone);
        assert_eq!(flight.local_rev, entity.local_rev);
        assert_eq!(flight.erev, sent.fields.erev);
        assert_eq!(flight.payload_digest, codec::sha256(&[]));
        assert_eq!(flight.row_digest, codec::sha256(&[]));
        assert_eq!(flight.tombstone_envelope_digest, Some(codec::sha256(bytes)));
        // This reads the actual Store, rather than a Rig-owned send model.
        assert!(pair.receiver.driver().binding_entity(b, &record.name).unwrap().as_ref() == Some(entity));
    }
    assert_eq!(sent_names, deleted);
    let mut retained = handed.clone();
    retained.retain(|key, _| !deleted.contains(key));
    let absent = pair.receiver.driver().collection_rows().unwrap();
    assert!(!absent.iter().any(|row| {
        matches!(row, Payload::Note(n) if n.id == id) || matches!(row, Payload::Card(c) if c.nid == id)
    }));
    settled(&mut pair, b);

    // Another Service client removes exactly these records while the original
    // conditional Saves remain held. No reply or tracking state is injected.
    let removed = pair
        .service
        .request(
            &scope(),
            Default::default(),
            Operation::Modify {
                zone: zone.clone(),
                atomic: false,
                changes: deleted
                    .iter()
                    .map(|key| Mutation::Delete { name: key.clone() })
                    .collect(),
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = &removed.result else {
        panic!("actual independent Service removal")
    };
    assert_eq!(
        items.iter().map(|item| item.name.clone()).collect::<BTreeSet<_>>(),
        deleted
    );
    assert!(
        items
            .iter()
            .all(|item| matches!(item.result, Ok(MutationSuccess::Deleted)))
    );
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    assert!(deleted.iter().all(|key| !state.zones[zone].records.contains_key(key)));
    assert_eq!(saved(&pair, &unrelated), unrelated_record);
    let still_held = pair.receiver.held_request().unwrap();
    assert_eq!(still_held.operation, request.operation);
    assert_eq!(still_held.caller, request.caller);
    assert!(pair.receiver.facts().unwrap().entities == handed);

    // Block any later fetched deletion before intake: only the original Save
    // result may account for the following tracking removal.
    pair.receiver.hold(Point::FetchBeforeInbox).unwrap();
    pair.receiver.release(Point::BeforeCollectionSave).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let log = pair.service.request_log().unwrap();
    let sends: Vec<_> = log
        .iter()
        .filter(|entry| {
            entry.request_id > removed.request_id && entry.kind == RequestKind::Modify && entry.caller == request.caller
        })
        .collect();
    assert_eq!(sends.len(), 1, "submit the original request exactly once");
    let receipt = sends[0];
    assert_eq!(receipt.scope, scope());
    assert_eq!(receipt.zone.as_ref(), Some(zone));
    assert_eq!(receipt.records.len(), deleted.len());
    assert!(receipt.records.iter().all(|record| {
        deleted.contains(&record.name) && record.outcome == LogOutcome::Failure(ErrorCode::UnknownItem)
    }));
    let delivered = pair.receiver.sent_request(receipt.request_id).unwrap();
    assert_eq!(delivered.instance, instance);
    assert!(delivered.backend_batch_id > 0);
    assert_eq!(&delivered.mutations, changes);
    assert_eq!(delivered.response.request_id, receipt.request_id);
    let Ok(Reply::Modified(items)) = &delivered.response.result else {
        panic!("actual original Save response retained by normal transport")
    };
    assert_eq!(items.len(), deleted.len());
    assert_eq!(
        items.iter().map(|item| item.name.clone()).collect::<BTreeSet<_>>(),
        deleted
    );
    assert!(
        items
            .iter()
            .all(|item| matches!(&item.result, Err(error) if error.code == ErrorCode::UnknownItem))
    );
    for key in &deleted {
        assert!(pair.receiver.driver().binding_entity(b, key).unwrap().is_none());
        assert!(!pair.receiver.facts().unwrap().entities.contains_key(key));
    }
    assert!(pair.receiver.facts().unwrap().entities == retained);
    settled(&mut pair, b);
    assert!(pair.receiver.sdk_pending().is_empty());
    assert!(pair.receiver.sdk_database_pending().is_empty());
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    let after = pair.receiver.driver().incoming_observation(b).unwrap();
    // The final empty SDK batch still captures through cloud_snapshot_barrier.
    // It reserves exactly one seq without changing rows or emitting Changes.
    assert_eq!(after.seq, observation.seq + 1);
    assert_eq!(after.native_file, observation.native_file);
    assert_eq!(after.generation, observation.generation);
    let wal = pair.receiver.driver().wal_proof(after.seq).unwrap();
    assert!(wal.through >= after.seq);
    assert_eq!(wal.native_file, after.native_file);
    assert_eq!(wal.generation, after.generation);
    assert_eq!(after.col_mod, observation.col_mod);
    assert_eq!(after.undo_depth, observation.undo_depth);
    assert_eq!(after.change_events, observation.change_events);
    assert_eq!(after.runs.len(), observation.runs.len());

    pair.receiver.release(Point::FetchBeforeInbox).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    pair.receiver.close_clean().unwrap();
    pair.reopen_before_identity();
    // Read persisted tracking and physical rows before identity or any fetch.
    assert_eq!(binding(&pair), b);
    for key in &deleted {
        assert!(pair.receiver.driver().binding_entity(b, key).unwrap().is_none());
    }
    assert!(pair.receiver.facts().unwrap().entities == retained);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    settled(&mut pair, b);
    pair.verify_and_drive();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    for key in &deleted {
        assert!(pair.receiver.driver().binding_entity(b, key).unwrap().is_none());
    }
    settled(&mut pair, b);
    assert!(pair.receiver.sdk_pending().is_empty());
    assert!(pair.receiver.sdk_database_pending().is_empty());
    let actual = pair.receiver.facts().unwrap().entities;
    let state = pair.receiver.driver().stored_runtime_state().unwrap();
    let coverage = state.coverage.unwrap();
    let live = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(coverage.open, state.open_counter);
    assert_eq!(
        coverage.open,
        retained.values().map(|e| e.observer_open).max().unwrap() + 1
    );
    assert_eq!(coverage.cursor.generation, live.generation);
    assert_eq!(coverage.cursor.native_file, live.native_file);
    assert!(
        coverage.cursor.seq > 0
            && coverage.cursor.covered_through >= coverage.cursor.seq
            && live.seq >= coverage.cursor.seq
    );
    let mut scanned = retained.clone();
    for entity in scanned.values_mut() {
        assert_ne!(entity.kind, codec::Kind::MediaName);
        entity.observer_open = coverage.open;
        entity.observed_seq = coverage.cursor.seq;
        entity.observed_generation = coverage.cursor.generation;
    }
    // Reopen verification ran the real scan. Compare its certified stamps and
    // every retained payload, revision, ACK, flight and other tracking field.
    assert!(actual == scanned, "only certified scan stamps may change after reopen");
    assert_eq!(saved(&pair, &unrelated), unrelated_record);
    assert!(!pair.service.request_log().unwrap().iter().any(|entry| {
        entry.request_id > receipt.request_id
            && entry.caller.instance.is_some()
            && entry.kind == RequestKind::Modify
            && entry.records.iter().any(|record| deleted.contains(&record.name))
    }));
    pair.close();
}

fn lost_note_tombstone() -> (Pair, i64, String, Record) {
    let mut pair = Pair::adopted();
    let id = notes(&mut pair.receiver)[0].id;
    let key = name(&pair, codec::Kind::Note, id);
    pair.receiver.driver().delete_note(id).unwrap();
    hold_receive(&mut pair, Point::BeforeCollectionSave);
    let request = pair.receiver.held_request().unwrap();
    let response = pair
        .service
        .request(&scope(), request.caller, request.operation)
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual Service reply")
    };
    let Ok(MutationSuccess::Saved(landed)) = &items.iter().find(|i| i.name == key).unwrap().result else {
        panic!("actual landed tombstone")
    };
    assert!(envelope(landed).fields.tombstone);
    (pair, id, key, landed.clone())
}

#[test]
fn lost_tombstone_reply_refuses_undo_and_fetches_exact_ack_after_reopen() {
    let (mut pair, id, key, landed) = lost_note_tombstone();
    let b = binding(&pair);
    let handed = pair.receiver.facts().unwrap().entities[&key].clone();
    let flight = handed.in_flight.iter().find(|f| f.tombstone).unwrap().clone();
    let Field::EncryptedBytes(bytes) = &landed.fields["e"] else {
        panic!("envelope")
    };
    assert_eq!(flight.erev, envelope(&landed).fields.erev);
    assert_eq!(flight.tombstone_envelope_digest, Some(codec::sha256(bytes)));
    assert!(pair.receiver.driver().undo().is_err());
    let absent = pair.receiver.driver().collection_rows().unwrap();
    assert!(
        !absent
            .iter()
            .any(|p| { matches!(p, Payload::Note(n) if n.id == id) || matches!(p, Payload::Card(c) if c.nid == id) })
    );
    assert!(pair.receiver.facts().unwrap().entities[&key] == handed);
    // The Service committed the original request; its reply never reaches the child.
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    assert!(pair.receiver.facts().unwrap().entities[&key] == handed);
    pair.verify_and_drive();
    assert_scan_live_counts(&pair.receiver, &absent);
    // Fetch the actual landed record while its original reply remains lost.
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&landed))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    let retained = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(retained.local_rev, handed.local_rev);
    assert!(retained.payload == handed.payload);
    assert_eq!(retained.tombstone_at, handed.tombstone_at);
    assert_eq!(retained.server_erev, flight.erev);
    assert_eq!(retained.acked_rev, flight.local_rev);
    assert_eq!(retained.acked_envelope.0, *bytes);
    assert_eq!(retained.acked_system_fields, landed.system_fields.bytes());
    assert_eq!(retained.acked_payload_digest, codec::sha256(&[]));
    assert_eq!(retained.acked_row_digest, codec::sha256(&[]));
    assert!(retained.mark.is_none() && retained.base_payload.is_none());
    assert!(retained.in_flight.is_empty());
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
    assert!(!inbox.iter().any(|e| e.record_name == key));
    let instance = pair
        .receiver
        .driver()
        .incoming_observation(b)
        .unwrap()
        .engine_instance
        .unwrap();
    assert!(
        pair.receiver
            .driver()
            .build_for_instance(instance, BTreeSet::from([key.clone()]))
            .unwrap()
            .saves
            .is_empty()
    );
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    // Force another real opening to scan the ACKed deletion facts as well.
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    pair.verify_and_drive();
    assert_scan_live_counts(&pair.receiver, &absent);
    assert!(pair.receiver.facts().unwrap().entities[&key] == retained);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), absent);
    pair.close();
}

#[test]
fn empty_tombstone_digest_cannot_ack_another_revision_or_another_handed_envelope() {
    for change_revision in [true, false] {
        let (mut pair, _, key, landed) = lost_note_tombstone();
        let b = binding(&pair);
        let handed = pair.receiver.facts().unwrap().entities[&key].clone();
        let mut fields = envelope(&landed).fields;
        if change_revision {
            fields.erev += 1;
        } else {
            fields.deleted_at_ms += 1;
        }
        let other = codec::Entity::build(fields, &[]).unwrap().encode().unwrap();
        let response = pair
            .service
            .request(
                &scope(),
                Default::default(),
                meguri_testkit::cloud::Operation::Modify {
                    zone: pair.receiver.collection_zone().unwrap(),
                    atomic: false,
                    changes: vec![meguri_testkit::cloud::Mutation::Save {
                        record: meguri_testkit::cloud::RecordInput::encrypted(&key, "MGEntity", other),
                        if_unchanged: Some(landed.system_fields.clone()),
                    }],
                },
            )
            .unwrap();
        let Ok(Reply::Modified(items)) = response.result else {
            panic!("actual Service reply")
        };
        let Ok(MutationSuccess::Saved(other)) = &items[0].result else {
            panic!("actual Service save")
        };
        assert_eq!(envelope(other).payload_sha256, codec::sha256(&[]));
        pair.receiver.kill().unwrap();
        pair.reopen_before_identity();
        pair.verify_and_drive();
        // Deliver only this actual Service version, independently of delta coalescing.
        pair.receiver
            .driver()
            .deliver_collection_records(std::slice::from_ref(other))
            .unwrap();
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        let retained = pair.receiver.facts().unwrap().entities[&key].clone();
        assert_eq!(retained.server_erev, handed.server_erev);
        assert_eq!(retained.acked_rev, handed.acked_rev);
        assert!(retained.in_flight == handed.in_flight);
        assert!(
            pair.receiver
                .driver()
                .inbox_entries(b)
                .unwrap()
                .iter()
                .any(|e| e.record_name == key && e.reason == InboxReason::Structure)
        );
        pair.close();
    }
}

#[test]
fn local_changes_during_apply() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let original = pair.receiver.driver().collection_rows().unwrap();
    let originals = notes(&mut pair.receiver);
    let deleted_id = originals[0].id;
    let live_id = originals[1].id;
    let deleted_key = name(&pair, codec::Kind::Note, deleted_id);
    let live_key = name(&pair, codec::Kind::Note, live_id);
    let old_deleted = pair.receiver.facts().unwrap().entities[&deleted_key].clone();
    pair.receiver.driver().delete_note(deleted_id).unwrap();
    pair.sender.advance_clock_ms(7_000).unwrap();
    pair.sender.driver().delete_note(deleted_id).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(live_id, &["remote live B", "remote back B"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let remote_delete = saved(&pair, &deleted_key);
    let remote_live = saved(&pair, &live_key);
    let remote_payload = body(&remote_live);
    let remote_entity = envelope(&remote_live);
    pair.receiver
        .driver()
        .pause_automatic_wal_flush_for_testing(true)
        .unwrap();
    pair.receiver.hold(Point::ApplyAfterFlush).unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(&[remote_live.clone(), remote_delete])
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ApplyAfterFlush));
    let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
    let committed = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(committed.runs.len(), 1);
    assert_eq!(intent.result_seq, Some(committed.runs[0].seq));
    assert_eq!(committed.runs[0].changed_keys, BTreeSet::from([live_key.clone()]));
    assert_eq!(note(&mut pair.receiver, live_id), remote_payload);
    assert!(!notes(&mut pair.receiver).iter().any(|n| n.id == deleted_id));

    pair.receiver.driver().undo().unwrap();
    pair.receiver
        .driver()
        .edit_note_fields(live_id, &["remote live B", "new local back B"])
        .unwrap();
    let retained_rows = pair.receiver.driver().collection_rows().unwrap();
    for row in original.iter().filter(|p| {
        matches!(p, Payload::Note(n) if n.id == deleted_id) || matches!(p, Payload::Card(c) if c.nid == deleted_id)
    }) {
        assert!(retained_rows.contains(row));
    }
    let newer = pair.receiver.facts().unwrap();
    let newer_delete = &newer.entities[&deleted_key];
    let newer_live = &newer.entities[&live_key];
    assert!(newer_delete.observed_seq > intent.result_seq.unwrap());
    assert!(newer_live.observed_seq > newer_delete.observed_seq);
    assert_eq!(newer_live.observer_open, intent.originating_open.unwrap());
    assert_eq!(newer_live.observed_generation, intent.generation);
    assert!(newer_live.payload == note(&mut pair.receiver, live_id));
    assert!(
        !committed
            .wal_receipts
            .iter()
            .any(|r| r.native_file == intent.native_file
                && r.generation == intent.generation
                && r.through >= newer_live.observed_seq)
    );
    pair.receiver.release(Point::ApplyAfterFlush).unwrap();
    let stale_receipt = pair.receiver.driver().drain_incoming(STEPS).unwrap_err();
    assert_eq!(
        stale_receipt.to_string(),
        "cloud device operation failed: RuntimeEngine(Busy)"
    );
    assert_eq!(pair.receiver.driver().apply_intents(b).unwrap(), vec![intent.clone()]);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), retained_rows);

    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), retained_rows);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let settled = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(settled.runs, committed.runs);
    assert!(settled.wal_receipts.iter().any(|r| r.native_file == intent.native_file
        && r.generation == intent.generation
        && r.through >= newer_live.observed_seq));
    let facts = pair.receiver.facts().unwrap();
    let deleted = &facts.entities[&deleted_key];
    assert!(deleted.payload == newer_delete.payload);
    assert!(deleted.tombstone_at.is_none());
    assert!(deleted.mark.is_some());
    assert_eq!(deleted.local_rev, newer_delete.local_rev);
    assert_eq!(deleted.server_erev, old_deleted.server_erev);
    assert_eq!(deleted.acked_rev, old_deleted.acked_rev);
    assert_eq!(deleted.acked_envelope, old_deleted.acked_envelope);
    let live = &facts.entities[&live_key];
    assert!(live.payload == newer_live.payload);
    assert_eq!(live.local_rev, newer_live.local_rev);
    assert!(live.mark.is_some());
    assert!(live.base_payload.as_ref() == Some(&remote_payload));
    assert!(live.acked_payload.as_ref() == Some(&remote_payload));
    assert_eq!(live.server_erev, remote_entity.fields.erev);
    assert_eq!(live.acked_system_fields, remote_live.system_fields.bytes());
    let Field::EncryptedBytes(remote_envelope) = &remote_live.fields["e"] else {
        panic!("actual live envelope required")
    };
    assert!(live.acked_envelope.0 == *remote_envelope);
    let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].record_name, deleted_key);
    assert_eq!(inbox[0].reason, InboxReason::Structure);
    pair.receiver
        .driver()
        .pause_automatic_wal_flush_for_testing(false)
        .unwrap();
    pair.close();
}

#[test]
fn held_delete_undo_keeps_rows_and_wal() {
    for undo_after_flush in [false, true] {
        let mut pair = Pair::adopted();
        let b = binding(&pair);
        let id = notes(&mut pair.receiver)[0].id;
        let key = name(&pair, codec::Kind::Note, id);
        let original = pair.receiver.driver().collection_rows().unwrap();
        let old_ack = pair.receiver.facts().unwrap().entities[&key].clone();
        pair.receiver.driver().delete_note(id).unwrap();
        pair.sender.driver().delete_note(id).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let tombstone = saved(&pair, &key);
        pair.receiver
            .driver()
            .pause_automatic_wal_flush_for_testing(true)
            .unwrap();
        pair.receiver.driver().deliver_collection_records(&[tombstone]).unwrap();
        pair.receiver.hold(Point::ApplyAfterCommit).unwrap();
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        assert!(pair.receiver.is_held(Point::ApplyAfterCommit));
        let intent = pair.receiver.driver().apply_intents(b).unwrap().remove(0);
        let committed = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(committed.runs.len(), 1);
        assert!(committed.runs[0].changed_keys.is_empty());
        assert_eq!(intent.result_seq, Some(committed.runs[0].seq));
        assert!(
            !committed
                .wal_receipts
                .iter()
                .any(|r| r.native_file == intent.native_file
                    && r.generation == intent.generation
                    && r.through >= intent.result_seq.unwrap())
        );
        if !undo_after_flush {
            pair.receiver.driver().undo().unwrap();
        }
        pair.receiver.hold(Point::ApplyAfterFlush).unwrap();
        pair.receiver.release(Point::ApplyAfterCommit).unwrap();
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        assert!(pair.receiver.is_held(Point::ApplyAfterFlush));
        let flushed = pair.receiver.driver().incoming_observation(b).unwrap();
        assert!(flushed.wal_receipts.iter().any(|r| r.native_file == intent.native_file
            && r.generation == intent.generation
            && r.through >= intent.result_seq.unwrap()));
        if undo_after_flush {
            pair.receiver.driver().undo().unwrap();
        }
        let restored = pair.receiver.driver().collection_rows().unwrap();
        for row in original
            .iter()
            .filter(|p| matches!(p, Payload::Note(n) if n.id == id) || matches!(p, Payload::Card(c) if c.nid == id))
        {
            assert!(restored.contains(row));
        }
        let newer = pair.receiver.facts().unwrap().entities[&key].clone();
        assert!(newer.observed_seq > intent.result_seq.unwrap());
        pair.receiver.release(Point::ApplyAfterFlush).unwrap();
        if undo_after_flush {
            // The old receipt cannot cover Undo: retain the intent on Busy.
            assert!(pair.receiver.driver().drain_incoming(STEPS).is_err());
            assert_eq!(pair.receiver.driver().apply_intents(b).unwrap(), vec![intent.clone()]);
            assert_eq!(pair.receiver.driver().collection_rows().unwrap(), restored);
        }
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), restored);
        assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
        let retained = pair.receiver.facts().unwrap().entities[&key].clone();
        assert!(retained.payload == note(&mut pair.receiver, id));
        assert!(retained.tombstone_at.is_none());
        assert!(retained.mark.is_some());
        assert_eq!(retained.local_rev, newer.local_rev);
        assert_eq!(retained.server_erev, old_ack.server_erev);
        assert_eq!(retained.acked_rev, old_ack.acked_rev);
        assert_eq!(retained.acked_envelope, old_ack.acked_envelope);
        assert!(
            pair.receiver
                .driver()
                .inbox_entries(b)
                .unwrap()
                .iter()
                .any(|e| e.record_name == key && e.reason == InboxReason::Structure)
        );
        let settled = pair.receiver.driver().incoming_observation(b).unwrap();
        assert_eq!(settled.runs, committed.runs);
        assert!(settled.wal_receipts.iter().any(|r| r.native_file == intent.native_file
            && r.generation == intent.generation
            && r.through >= newer.observed_seq));
        pair.receiver
            .driver()
            .pause_automatic_wal_flush_for_testing(false)
            .unwrap();
        pair.close();
    }
}
