//! Enabled RED contracts for P24/Q11, K12.3 Saved, K12.4, A23 and P12.3.4.
//! Starting commit: 8f956816333d875e7cd0ebdd2e9a899781ddeb82.
//!
//! Future shared seam: cloud::adapter::SDKRig owns the real CoreHost child,
//! production Rust adapter/action queue, store and builder. Its driver() exposes
//! CoreHost note/review services, never direct SQL writes. All facts and traces
//! below must be read from those components or the existing cloud::Service.
//! PreboundDevice only arranges an already healthy canonical baseline; opening
//! the rig must reconcile every entity kind and an empty media feed, then confirm
//! this instance's root using an actual Service response.
//!
//! SDK scheduling is finite and deterministic: deduplicate additions, retain a
//! saved name throughout its handler, remove it ONLY at handler return, and drop
//! non-retry failures BEFORE the adapter callback (Q3.3). drive_ready executes
//! scheduled work at the current clock with no new policy or user input.
//! The bound limits scheduler steps regardless of send results.
//! observations() borrows the real trace; action inspection returns the original
//! complete guarded actions. enter_sent_handler_for_ordering enters the same
//! production sent-event handler with an empty result list, pausing its return;
//! it must not toggle a fixture-only queue or replace production action handling.
//!
//! Opaque PendingSnapshot inputs in the three queue tests isolate action ordering:
//! feed the real U113 pending-action producer and production adapter, not a test
//! queue. They assert no K19, asset-upload or blob-GC implementation. No helper
//! may insert the observations/assertion results that these tests inspect.
//! Rust-first component evidence only; Swift, signed A23 and full Q4/Q5 stay open.

use std::collections::{BTreeMap, BTreeSet};

use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::entity_codec as codec;
use meguri_testkit::{
    cloud::{
        Caller, Config, ErrorCode, Field, LogOutcome, Mutation, Operation, Record, Reply, ReplyReference, RequestKind,
        Service,
        adapter::{Observation, PendingChange, PendingSnapshot, SDKRig, UpdateKind},
        devices::PreboundDevice,
    },
    generate::recipe::{Recipe, generate_recipe},
};

const SEED: u64 = 0x2411_a023;
const NOW_MS: i64 = 1_735_689_600_000; // 2025-01-01T00:00:00Z
const STEPS: usize = 10_000;

fn fixture(notes: usize, automatic: bool) -> (tempfile::TempDir, Service<SeededRandom>, PreboundDevice, SDKRig) {
    let run = tempfile::tempdir().unwrap();
    let collection = run.path().join("device");
    let recipe = Recipe::parse(&format!(
        "recipe: p24-adapter\ndescription: Generic synthetic notes\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic deck\n    notetype: basic\n    new: {notes}\n    fields: [\"generic\", \"back\"]\n"
    ))
    .unwrap();
    let manifest = generate_recipe(&recipe, SEED, &collection).unwrap();
    assert_eq!(manifest.counts.notes, notes as u64);
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(SEED)).unwrap();
    let files = PreboundDevice::arrange(&collection, &mut service, SEED).unwrap();
    let mut rig = SDKRig::open(&files, automatic, NOW_MS, SEED).unwrap();
    rig.reconcile_and_confirm_root(&mut service, STEPS).unwrap();
    assert!(rig.tracking_pending_names().unwrap().is_empty());
    assert!(rig.sdk_pending().is_empty());
    let entity_names: Vec<String> = files.entity_names(codec::Kind::Note).into_iter().collect();
    for name in entity_names {
        let (entity, payload) = decode(&files, &record(&service, &files, &name));
        assert_eq!(entity.fields.erev, 1);
        assert_eq!(rig.entity_facts(&name).unwrap().acked_rev, 1);
        assert_eq!(rig.entity_facts(&name).unwrap().local_rev, 1);
        assert!(matches!(payload, codec::Payload::Note(_)));
    }
    (run, service, files, rig)
}

fn cursor(service: &Service<SeededRandom>) -> u64 {
    service.request_log().unwrap().last().map_or(0, |r| r.request_id)
}

fn record(service: &Service<SeededRandom>, files: &PreboundDevice, name: &str) -> Record {
    service.inspect(&files.scope).unwrap().unwrap().zones[&files.collection_zone].records[name].clone()
}

fn decode(files: &PreboundDevice, record: &Record) -> (codec::Entity, codec::Payload) {
    assert_eq!(record.record_type, "MGEntity");
    let Field::EncryptedBytes(e) = &record.fields["e"] else {
        panic!("missing envelope")
    };
    let received = codec::receive(
        &files.binding.facts.record_key.0,
        &record.name,
        e,
        codec::AssetInput::Absent,
    )
    .unwrap();
    let payload = received.payload.unwrap();
    assert_eq!(received.entity.payload_sha256, payload.payload_digest().unwrap());
    (received.entity, payload)
}

fn saved(
    service: &Service<SeededRandom>,
    files: &PreboundDevice,
    response: &meguri_testkit::cloud::Response,
    name: &str,
) -> Record {
    let Ok(Reply::Modified(items)) = &response.result else {
        panic!("actual Service save required")
    };
    let item = items.iter().find(|i| i.name == name).unwrap();
    let Ok(meguri_testkit::cloud::MutationSuccess::Saved(record)) = &item.result else {
        panic!("actual Saved response required")
    };
    let outcomes = service.outcomes().unwrap();
    let outcome = outcomes.iter().find(|o| o.request_id == response.request_id).unwrap();
    let Ok(ReplyReference::Modified(items)) = &outcome.result else {
        panic!("missing Service receipt")
    };
    let reference = items
        .iter()
        .find(|i| i.name == name)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(&service.resolve_record(&files.scope, reference).unwrap(), record);
    record.clone()
}

fn conditional_note(
    mutations: &[Mutation],
    baseline: &Record,
    expected: &codec::Note,
    files: &PreboundDevice,
    erev: u64,
) {
    assert_eq!(mutations.len(), 1);
    let Mutation::Save {
        record: input,
        if_unchanged,
    } = &mutations[0]
    else {
        panic!("expected Save")
    };
    assert_eq!(input.name, baseline.name);
    assert_eq!(if_unchanged.as_ref(), Some(&baseline.system_fields));
    let built = Record {
        name: input.name.clone(),
        record_type: input.record_type.clone(),
        fields: input.fields.clone(),
        system_fields: baseline.system_fields.clone(),
    };
    let (entity, payload) = decode(files, &built);
    assert_eq!(entity.fields.erev, erev);
    assert_eq!(payload, codec::Payload::Note(expected.clone()));
}

fn assert_no_modify_since(service: &Service<SeededRandom>, after: u64) {
    assert!(
        !service
            .request_log()
            .unwrap()
            .iter()
            .any(|r| r.request_id > after && r.kind == RequestKind::Modify)
    );
}

/// K12.3 Saved, K12.4 Between full updates, A23; real revs/digests, not counters alone.
#[test]
fn saved_rev3_in_flight_edit4_rehanded_after_handler_return_without_trigger() {
    let (_run, mut service, files, mut rig) = fixture(1, true);
    let name = files.entity_name.clone();
    let baseline = record(&service, &files, &name);
    rig.driver()
        .edit_note_fields(files.note.id, &["revision two", "back"])
        .unwrap();
    let edit3 = rig
        .driver()
        .edit_note_fields(files.note.id, &["revision three", "back"])
        .unwrap();
    assert_eq!(rig.entity_facts(&name).unwrap().local_rev, 3);
    // This scheduler barrier holds the actual successful response, not the save.
    let batch3 = rig.drive_until_response(&mut service, STEPS).unwrap();
    conditional_note(&batch3.mutations, &baseline, &edit3.note, &files, 2);
    let saved3 = saved(&service, &files, &batch3.response, &name);
    assert_eq!(record(&service, &files, &name), saved3);
    assert_eq!(decode(&files, &saved3).1, codec::Payload::Note(edit3.note.clone()));
    let edit4 = rig
        .driver()
        .edit_note_fields(files.note.id, &["revision four", "back"])
        .unwrap();
    let before_handler = rig.entity_facts(&name).unwrap();
    assert_eq!(before_handler.local_rev, 4);
    assert_eq!(before_handler.acked_rev, 1);
    assert!(before_handler.mark.is_some());
    assert!(before_handler.in_flight.iter().any(|entry| entry.local_rev == 3
        && entry.payload_digest == codec::Payload::Note(edit3.note.clone()).payload_digest().unwrap()));
    let trace_start = rig.observations().len();
    rig.deliver_sent_event(&batch3.response).unwrap();
    let during = rig.entity_facts(&name).unwrap();
    let payload3 = codec::Payload::Note(edit3.note.clone());
    assert_eq!(during.local_rev, 4);
    assert!(during.mark.is_some());
    assert!(during.acked_rev < during.local_rev);
    assert_eq!(during.server_erev, 2);
    assert_eq!(during.acked_system_fields, saved3.system_fields.bytes());
    assert_eq!(during.acked_row_digest, payload3.row_digest().unwrap());
    assert_eq!(during.acked_payload_digest, payload3.payload_digest().unwrap());
    assert_eq!(during.base_payload, Some(payload3));
    assert_eq!(during.payload, codec::Payload::Note(edit4.note.clone()));
    // Saved is still pending in the deduplicating SDK while the handler runs.
    assert_eq!(rig.sdk_pending().get(&name), Some(&PendingChange::Save));
    assert!(!rig.observations()[trace_start..].iter().any(|o| matches!(o,
        Observation::PendingAdded { name: n, .. } if n == &name)));
    rig.return_sent_handler().unwrap();
    // No LocalChange, SyncNow, foreground, timer, fetch or settings input here.
    let batch4 = rig.drive_until_response(&mut service, STEPS).unwrap();
    conditional_note(&batch4.mutations, &saved3, &edit4.note, &files, 3);
    let observations = &rig.observations()[trace_start..];
    let returned = observations
        .iter()
        .position(
            |o| matches!(o, Observation::HandlerReturned { request_id } if *request_id == batch3.response.request_id),
        )
        .unwrap();
    let removed = observations
        .iter()
        .position(|o| matches!(o, Observation::SavedPendingRemoved { name: n, .. } if n == &name))
        .unwrap();
    let rehanded = observations
        .iter()
        .position(|o| matches!(o, Observation::PendingAdded { name: n, .. } if n == &name))
        .unwrap();
    assert!(removed >= returned && rehanded > removed);
    let saved4 = saved(&service, &files, &batch4.response, &name);
    assert_ne!(saved4.system_fields, saved3.system_fields);
    assert_eq!(decode(&files, &saved4).1, codec::Payload::Note(edit4.note.clone()));
    rig.deliver_sent_event(&batch4.response).unwrap();
    rig.return_sent_handler().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    rig.close_clean().unwrap();
    let durable = files.read_entity_durable(&name).unwrap();
    assert_eq!(durable.local_rev, 4);
    assert_eq!(durable.acked_rev, 4);
    assert_eq!(durable.payload, codec::Payload::Note(edit4.note));
    assert_eq!(durable.acked_payload_digest, decode(&files, &saved4).0.payload_sha256);
    assert_eq!(durable.acked_row_digest, durable.payload.row_digest().unwrap());
    assert_eq!(durable.acked_system_fields, saved4.system_fields.bytes());
    assert_eq!(durable.server_erev, 3);
    assert!(durable.mark.is_none());
    assert!(durable.in_flight.is_empty());
}

/// P24 held variant, P8/P9. Conservative gate until real ReviewService end/drain.
#[test]
fn held_edit_waits_for_session_ended_full_update() {
    let (_run, mut service, files, mut rig) = fixture(1, true);
    let before = cursor(&service);
    let session = rig.driver().review_begin(files.note.id).unwrap();
    let card = rig.driver().review_start(session).unwrap();
    rig.driver().review_show_answer(session, card.id).unwrap();
    rig.driver().review_answer_good(session, card.id).unwrap();
    let edit = rig
        .driver()
        .edit_note_fields(files.note.id, &["held edit", "back"])
        .unwrap();
    let held = rig.driver().read_review_hold(session).unwrap();
    assert!(!held.op_ids.is_empty());
    assert!(held.entity_names.contains(&files.entity_name));
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.sdk_pending().is_empty());
    let probe = rig.request_collection_batch(&mut service).unwrap();
    assert!(probe.mutations.is_empty());
    assert!(probe.request_ids.is_empty());
    assert_no_modify_since(&service, before);
    assert!(rig.entity_facts(&files.entity_name).unwrap().mark.is_some());
    rig.driver().review_end_and_drain(session).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let payload = decode(&files, &record(&service, &files, &files.entity_name)).1;
    assert_eq!(payload, codec::Payload::Note(edit.note));
    let observations = rig.observations();
    let ended = observations
        .iter()
        .position(|o| matches!(o, Observation::SessionEnded { session: s } if *s == session))
        .unwrap();
    let full = observations
        .iter()
        .enumerate()
        .skip(ended + 1)
        .find(|(_, o)| {
            matches!(
                o,
                Observation::CorePendingEmitted {
                    kind: UpdateKind::Full,
                    ..
                }
            )
        })
        .unwrap()
        .0;
    let handed = observations
        .iter()
        .enumerate()
        .skip(full + 1)
        .find(|(_, o)| matches!(o, Observation::PendingAdded { name, .. } if name == &files.entity_name))
        .unwrap()
        .0;
    assert!(ended < full && full < handed);
    assert!(rig.tracking_pending_names().unwrap().is_empty());
    rig.close_clean().unwrap();
}

/// P24(ii), P4 full skip-comparison invariant. Opaque structure inputs only.
#[test]
fn deferred_add_then_full_remove_keeps_name_absent_and_comparison_matches() {
    let (_run, mut service, _files, mut rig) = fixture(1, false);
    let x = "opaque-structural-member".to_owned();
    let obsolete = "opaque-obsolete-member".to_owned();
    rig.core_pending_transition(UpdateKind::Full, PendingSnapshot::saves([obsolete.clone()]))
        .unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let applied_before = rig.applied_update_ids().len();
    rig.enter_sent_handler_for_ordering().unwrap();
    let add = rig
        .core_pending_transition(UpdateKind::Changes, PendingSnapshot::saves([x.clone()]))
        .unwrap();
    let remove = rig
        .core_pending_transition(UpdateKind::Full, PendingSnapshot::empty())
        .unwrap();
    assert!(add.adds.contains_key(&x));
    assert!(add.removes.contains(&obsolete));
    assert!(remove.removes.contains(&x));
    // Neither half of the deferred whole UpdatePending may run inside a handler.
    assert_eq!(rig.applied_update_ids().len(), applied_before);
    assert!(rig.sdk_pending().contains_key(&obsolete));
    rig.return_sent_handler().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(&rig.applied_update_ids()[applied_before..], &[add.id, remove.id]);
    assert!(!rig.sdk_pending().contains_key(&obsolete));
    assert!(!rig.sdk_pending().contains_key(&x));
    let next = rig
        .core_pending_transition(UpdateKind::Full, PendingSnapshot::empty())
        .unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(rig.sdk_pending(), rig.core_tracking_sendables().unwrap());
    assert!(!rig.sdk_pending().contains_key(&x));
    assert!(rig.applied_update_ids().contains(&next.id));
    // This is the production optimization observation, not a fixture flag.
    assert!(rig.observations().iter().any(|o| matches!(o,
        Observation::FullComparisonSkipped { action_id } if *action_id == next.id)));
    rig.close_clean().unwrap();
}

/// P24(iii); SDK save-add after delete-add would discard the delete.
#[test]
fn deferred_blob_save_add_then_delete_add_keeps_delete() {
    let (_run, mut service, _files, mut rig) = fixture(1, false);
    let blob = "opaque-blob".to_owned();
    let applied_before = rig.applied_update_ids().len();
    rig.enter_sent_handler_for_ordering().unwrap();
    let save = rig
        .core_pending_transition(UpdateKind::Changes, PendingSnapshot::saves([blob.clone()]))
        .unwrap();
    let delete = rig
        .core_pending_transition(UpdateKind::Changes, PendingSnapshot::deletions([blob.clone()]))
        .unwrap();
    assert_eq!(save.adds.get(&blob), Some(&PendingChange::Save));
    assert_eq!(delete.adds.get(&blob), Some(&PendingChange::Delete));
    assert_eq!(rig.applied_update_ids().len(), applied_before);
    rig.return_sent_handler().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(&rig.applied_update_ids()[applied_before..], &[save.id, delete.id]);
    assert_eq!(rig.sdk_pending().get(&blob), Some(&PendingChange::Delete));
    rig.close_clean().unwrap();
}

/// P24(iv), Q3.3 failure dropping, P12.3.4's eight real sendChanges calls.
#[test]
fn sync_now_auto_off_400_after_first_250_limit_exceeded_finishes_within_eight_calls() {
    let (_run, mut service, files, mut rig) = fixture(400, false);
    let names = files
        .entity_names(codec::Kind::Note)
        .into_iter()
        .collect::<BTreeSet<_>>();
    assert_eq!(names.len(), 400);
    let mut expected = BTreeMap::new();
    for (i, note) in files.notes().iter().enumerate() {
        let edit = rig
            .driver()
            .edit_note_fields(note.id, &[&format!("selected {i}"), "back"])
            .unwrap();
        let name = codec::entity_name(
            &files.binding.facts.record_key.0,
            codec::Kind::Note,
            &codec::id_key(note.id),
        )
        .unwrap();
        assert_eq!(rig.entity_facts(&name).unwrap().local_rev, 2);
        expected.insert(name, codec::Payload::Note(edit.note));
    }
    let before = cursor(&service);
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(rig.tracking_pending_names().unwrap(), names);
    assert_no_modify_since(&service, before);
    assert!(rig.send_changes_calls().is_empty());
    // Existing Service configuration injects a real limitExceeded outcome for
    // the first 250-record request; restore the healthy 250 ceiling afterward.
    let mut constrained = Config::default();
    constrained.limits.records_per_request = 125;
    service
        .request(&files.scope, Caller::default(), Operation::Configure(constrained))
        .unwrap()
        .result
        .unwrap();
    let exchange = rig.sync_now().unwrap();
    let first = rig.drive_until_response(&mut service, STEPS).unwrap();
    assert_eq!(first.mutations.len(), 250);
    assert_eq!(
        first.response.result.as_ref().unwrap_err().code,
        ErrorCode::LimitExceeded
    );
    assert_eq!(rig.send_changes_calls().len(), 1);
    service
        .request(&files.scope, Caller::default(), Operation::Configure(Config::default()))
        .unwrap()
        .result
        .unwrap();
    let first_names = first
        .mutations
        .iter()
        .map(|m| match m {
            Mutation::Save { record, if_unchanged } => {
                assert!(if_unchanged.is_some());
                assert!(names.contains(&record.name));
                record.name.clone()
            }
            _ => panic!("expected selected Note Save"),
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(first_names.len(), 250);
    assert!(
        rig.send_changes_calls()[0]
            .request_ids
            .contains(&first.response.request_id)
    );
    rig.sdk_drop_nonretry_failures(&first.response).unwrap();
    assert!(first_names.iter().all(|n| !rig.sdk_pending().contains_key(n)));
    rig.deliver_sent_event(&first.response).unwrap();
    rig.return_sent_handler().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let calls = rig.send_changes_calls();
    assert!((2..=8).contains(&calls.len()));
    assert!(calls.iter().all(|c| c.exchange == Some(exchange) && !c.automatic));
    assert!(calls.iter().all(|c| c.returned));
    assert_eq!(calls.iter().map(|c| c.id).collect::<BTreeSet<_>>().len(), calls.len());
    let logs = service.request_log().unwrap();
    let outcomes = service.outcomes().unwrap();
    let requests = logs
        .iter()
        .filter(|r| r.request_id > before && r.kind == RequestKind::Modify)
        .collect::<Vec<_>>();
    assert!(requests.len() >= 5); // failed 250 + at least four <=125-record requests
    assert_eq!(requests[0].request_id, first.response.request_id);
    assert_eq!(requests[0].outcome, LogOutcome::Failure(ErrorCode::LimitExceeded));
    let mut accepted = BTreeSet::new();
    for request in requests.iter().skip(1) {
        assert!(!request.records.is_empty() && request.records.len() <= 125);
        assert_eq!(request.caller.instance.as_deref(), Some(rig.instance().as_str()));
        assert_eq!(request.scope, files.scope);
        assert_eq!(request.zone.as_deref(), Some(files.collection_zone.as_str()));
        assert!(calls.iter().any(|c| c.request_ids.contains(&request.request_id)));
        let outcome = outcomes.iter().find(|o| o.request_id == request.request_id).unwrap();
        let Ok(ReplyReference::Modified(items)) = &outcome.result else {
            panic!("expected actual acceptance")
        };
        assert_eq!(items.len(), request.records.len());
        for item in items {
            assert!(accepted.insert(item.name.clone()), "duplicate accepted name");
            let reference = item.result.as_ref().unwrap().as_ref().unwrap();
            let saved = service.resolve_record(&files.scope, reference).unwrap();
            let (entity, payload) = decode(&files, &saved);
            assert_eq!(entity.fields.kind, codec::Kind::Note);
            assert_eq!(entity.fields.erev, 2);
            assert_eq!(&payload, expected.get(&item.name).unwrap());
            // Read just this current version: materializing the whole retained
            // Service database repeatedly could exceed its inspection cap.
            let current = service
                .request(
                    &files.scope,
                    Caller::default(),
                    Operation::Read {
                        zone: files.collection_zone.clone(),
                        name: item.name.clone(),
                        fields: meguri_testkit::cloud::RequestedFields::All,
                    },
                )
                .unwrap();
            let Ok(Reply::Record(current)) = current.result else {
                panic!("missing accepted record")
            };
            assert_eq!(current, saved);
        }
    }
    assert_eq!(accepted, names);
    assert!(rig.tracking_pending_names().unwrap().is_empty());
    assert!(rig.sdk_pending().is_empty());
    let result = rig.exchange_result(exchange).unwrap();
    assert_eq!(result.selected_names, names);
    assert_eq!(result.acked_names, names);
    assert!(result.outstanding_names.is_empty());
    rig.close_clean().unwrap();
    for name in &names {
        let durable = files.read_entity_durable(name).unwrap();
        assert_eq!(durable.local_rev, durable.acked_rev);
        assert!(durable.mark.is_none());
        assert_eq!(&durable.payload, expected.get(name).unwrap());
    }
}

/// U113 P24 obligation: actual action-list emission order, removals included.
#[test]
fn core_pending_actions_preserve_emission_order() {
    let (_run, mut service, _files, mut rig) = fixture(1, false);
    let x = "opaque-x".to_owned();
    let y = "opaque-y".to_owned();
    let applied_before = rig.applied_update_ids().len();
    let emitted_before = rig.emitted_update_ids().len();
    rig.enter_sent_handler_for_ordering().unwrap();
    let first = rig
        .core_pending_transition(UpdateKind::Changes, PendingSnapshot::saves([x.clone(), y.clone()]))
        .unwrap();
    let second = rig
        .core_pending_transition(UpdateKind::Changes, PendingSnapshot::saves([y.clone()]))
        .unwrap();
    let third = rig
        .core_pending_transition(UpdateKind::Full, PendingSnapshot::deletions([y.clone()]))
        .unwrap();
    assert!(second.removes.contains(&x));
    assert_eq!(third.adds.get(&y), Some(&PendingChange::Delete));
    let ids = vec![first.id, second.id, third.id];
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(&rig.emitted_update_ids()[emitted_before..], ids.as_slice());
    assert_eq!(rig.applied_update_ids().len(), applied_before);
    rig.return_sent_handler().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(&rig.applied_update_ids()[applied_before..], ids.as_slice());
    assert_eq!(&rig.applied_updates()[applied_before..], &[first, second, third]);
    assert_eq!(rig.sdk_pending(), BTreeMap::from([(y, PendingChange::Delete)]));
    assert_eq!(rig.sdk_pending(), rig.core_tracking_sendables().unwrap());
    rig.close_clean().unwrap();
}
