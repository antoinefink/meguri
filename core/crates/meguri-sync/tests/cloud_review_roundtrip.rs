//! Enabled RED integration contract: actual reviewCard + Revlog save, K6.1,
//! K11.3, K12.1-4, P8/P9 and Q2's Rust-first U113/U114-core lane.
//! Starting commit: 8f956816333d875e7cd0ebdd2e9a899781ddeb82.
//!
//! Uses the SAME future cloud::adapter::SDKRig/CoreHost process driver as
//! cloud_adapter_regressions, not a local ReviewService or fabricated result.
//! PreboundDevice arranges a stable, previously healthy canonical baseline,
//! including prior history; this does not claim publication/adoption support.
//! read_collection_payloads projects the actual closed collection's rows with
//! the production codec (Card.last_review_id from exact millisecond revlog.id).
//! It must not read expected/acked payloads out of tracking instead of the rows.
//!
//! Additional shared APIs required here: accepted_root_read (actual accepted
//! response reference plus persisted core root facts), scan observations for all
//! nine kinds and empty media, actual CoreHost review begin/start/show/Good/end
//! and drain, shared fake monotonic/wall-clock advancement, read_card/read_revlog,
//! reopen of the same files, and generic durable entity facts. No implementation
//! of those helpers or production behavior belongs in this test target.
//! No Swift fake, signed SDK, full Q4/Q5, oracle or acceptance claim.

use std::collections::{BTreeMap, BTreeSet};

use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::entity_codec as codec;
use meguri_testkit::{
    cloud::{
        Config, Field, LogOutcome, Mutation, Record, ReplyReference, RequestKind, Service,
        adapter::{Observation, SDKRig, UpdateKind},
        devices::PreboundDevice,
    },
    generate::recipe::{Recipe, generate_recipe},
};

const SEED: u64 = 0x2411_ca4d;
const NOW_MS: i64 = 1_735_689_600_000;
const STEPS: usize = 10_000;

fn decode(files: &PreboundDevice, record: &Record) -> (codec::Entity, codec::Payload) {
    assert_eq!(record.record_type, "MGEntity");
    let Field::EncryptedBytes(e) = &record.fields["e"] else {
        panic!("actual envelope required")
    };
    let received = codec::receive(
        &files.binding.facts.record_key.0,
        &record.name,
        e,
        codec::AssetInput::Absent,
    )
    .unwrap();
    let payload = received.payload.unwrap();
    assert_eq!(received.entity.fields.kind, payload.kind());
    assert_eq!(received.entity.payload_sha256, payload.payload_digest().unwrap());
    (received.entity, payload)
}

fn server_records(service: &Service<SeededRandom>, files: &PreboundDevice) -> BTreeMap<String, Record> {
    service.inspect(&files.scope).unwrap().unwrap().zones[&files.collection_zone]
        .records
        .iter()
        .filter(|(_, r)| r.record_type == "MGEntity")
        .map(|(name, r)| (name.clone(), r.clone()))
        .collect()
}

fn assert_root_response(service: &Service<SeededRandom>, files: &PreboundDevice, rig: &SDKRig) {
    let proof = rig.accepted_root_read().unwrap();
    assert_eq!(proof.instance, rig.instance());
    assert_eq!(proof.persisted_epoch, files.binding.facts.epoch);
    let logs = service.request_log().unwrap();
    let log = logs.iter().find(|r| r.request_id == proof.request_id).unwrap();
    assert_eq!(log.kind, RequestKind::Read);
    assert_eq!(log.scope, files.scope);
    assert_eq!(log.zone.as_deref(), Some("meguri-root"));
    assert_eq!(log.caller.instance.as_deref(), Some(rig.instance().as_str()));
    assert_eq!(log.outcome, LogOutcome::Success);
    let outcomes = service.outcomes().unwrap();
    let outcome = outcomes.iter().find(|r| r.request_id == proof.request_id).unwrap();
    let Ok(ReplyReference::Record(reference)) = &outcome.result else {
        panic!("actual root read required")
    };
    let record = service.resolve_record(&files.scope, reference).unwrap();
    assert_eq!(record.name, "active");
    assert_eq!(record.record_type, "MGActive");
    assert_eq!(proof.accepted_record, record);
    assert!(matches!(record.fields.get("e"), Some(Field::EncryptedBytes(_))));
}

fn assert_no_collection_send(service: &mut Service<SeededRandom>, after: u64, rig: &mut SDKRig) {
    let batch = rig.request_collection_batch(service).unwrap();
    assert!(batch.mutations.is_empty());
    assert!(batch.request_ids.is_empty());
    assert!(
        !service
            .request_log()
            .unwrap()
            .iter()
            .any(|r| r.request_id > after && r.kind == RequestKind::Modify)
    );
}

/// P8/P9 release, real conditional Card Save and new Revlog Save; no Note-only intermediate.
#[test]
fn core_review_good_end_drain_conditionally_saves_card_and_exact_revlog_and_survives_reopen() {
    let run = tempfile::tempdir().unwrap();
    let collection = run.path().join("device");
    let recipe = Recipe::parse(
        r#"recipe: cloud-review-roundtrip
description: Generic review and prior history
now: "2025-01-01T00:00:00Z"
decks:
  - name: Generic deck
    notetype: basic
    new: 1
    due: 1
    tags: [generic]
    fields: ["generic {n}", "back {n}"]
"#,
    )
    .unwrap();
    let manifest = generate_recipe(&recipe, SEED, &collection).unwrap();
    assert_eq!(manifest.counts.notes, 2);
    assert!(manifest.counts.revlog > 0);
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(SEED)).unwrap();
    let files = PreboundDevice::arrange(&collection, &mut service, SEED).unwrap();
    let before_payloads = files.read_collection_payloads().unwrap();
    let before_records = server_records(&service, &files);
    assert_eq!(before_records.len(), before_payloads.len());
    for (name, expected) in &before_payloads {
        assert_eq!(&decode(&files, &before_records[name]).1, expected);
    }
    let prior_history = before_payloads
        .iter()
        .filter(|(_, p)| p.kind() == codec::Kind::Revlog)
        .map(|(name, p)| (name.clone(), p.clone()))
        .collect::<BTreeMap<_, _>>();
    assert!(!prior_history.is_empty());
    let mut rig = SDKRig::open(&files, true, NOW_MS, SEED).unwrap();
    rig.reconcile_and_confirm_root(&mut service, STEPS).unwrap();
    assert_root_response(&service, &files, &rig);
    // Even zero-row kinds must actually finish reconciliation. These are core
    // scan events, never a helper-supplied "all kinds supported" flag.
    for code in 1..=9 {
        let kind = codec::Kind::try_from(code).unwrap();
        let count = before_payloads.values().filter(|p| p.kind() == kind).count();
        assert!(rig.observations().iter().any(|o| matches!(o,
            Observation::ReconcileKindCompleted { kind: k, row_count } if *k == kind && *row_count == count)));
    }
    assert!(
        rig.observations()
            .iter()
            .any(|o| matches!(o, Observation::MediaFeedDrained { entry_count: 0, .. }))
    );
    assert!(rig.tracking_pending_names().unwrap().is_empty());
    let after_baseline = service.request_log().unwrap().last().unwrap().request_id;

    // Calls below must cross the real CoreHost process boundary and invoke its
    // ReviewService. The timer is the shared injected clock, not a row edit.
    let session = rig.driver().review_begin(files.note.id).unwrap();
    let question = rig.driver().review_start(session).unwrap();
    let card_before = rig.driver().read_card(question.id).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_no_collection_send(&mut service, after_baseline, &mut rig);
    rig.advance_clock_ms(1_000).unwrap();
    rig.driver().review_show_answer(session, question.id).unwrap();
    rig.advance_clock_ms(1_500).unwrap();
    let answer = rig.driver().review_answer_good(session, question.id).unwrap();
    let card_after = rig.driver().read_card(question.id).unwrap();
    let revlog = rig.driver().read_revlog(answer.revlog_id).unwrap();
    assert_eq!(card_after.id, card_before.id);
    assert_eq!(card_after.reps, card_before.reps + 1);
    assert_ne!(card_after.due, card_before.due);
    assert_eq!(card_after.last_review_id, revlog.id);
    assert_eq!(revlog.id, answer.revlog_id); // exact id: never divide by 1000
    assert!(revlog.id >= NOW_MS);
    assert_eq!(revlog.cid, card_after.id);
    assert_eq!(revlog.ease, 3); // Good
    assert_eq!(revlog.time, codec::Stored::Integer(2_500));
    let card_name = codec::entity_name(
        &files.binding.facts.record_key.0,
        codec::Kind::Card,
        &codec::id_key(card_after.id),
    )
    .unwrap();
    let revlog_name = codec::entity_name(
        &files.binding.facts.record_key.0,
        codec::Kind::Revlog,
        &codec::id_key(revlog.id),
    )
    .unwrap();
    assert!(!before_records.contains_key(&revlog_name));
    let hold = rig.driver().read_review_hold(session).unwrap();
    assert!(hold.op_ids.contains(&answer.op_id));
    assert!(hold.entity_names.contains(&card_name));
    assert!(hold.entity_names.contains(&revlog_name));
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_no_collection_send(&mut service, after_baseline, &mut rig);
    assert_eq!(server_records(&service, &files), before_records);
    assert!(!rig.core_tracking_sendables().unwrap().contains_key(&card_name));
    assert!(!rig.core_tracking_sendables().unwrap().contains_key(&revlog_name));

    rig.driver().review_end_and_drain(session).unwrap();
    // Read the collection rows before Saved and compare their payloads below.
    // The acknowledgement code must not supply the expected values.
    let expected = rig.driver().collection_payloads().unwrap();
    assert_eq!(expected[&card_name], codec::Payload::Card(card_after.clone()));
    assert_eq!(expected[&revlog_name], codec::Payload::Revlog(revlog.clone()));
    let changed = expected
        .iter()
        .filter(|(name, p)| before_payloads.get(*name) != Some(*p))
        .map(|(name, _)| name.clone())
        .collect::<BTreeSet<_>>();
    assert!(changed.contains(&card_name) && changed.contains(&revlog_name));
    rig.drive_ready(&mut service, STEPS).unwrap();
    let observations = rig.observations();
    let ended = observations
        .iter()
        .position(|o| {
            matches!(o,
        Observation::SessionEnded { session: s } if *s == session)
        })
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
    for name in [&card_name, &revlog_name] {
        let handed = observations
            .iter()
            .position(|o| {
                matches!(o,
            Observation::BatchBuilt { names, .. } if names.contains(name))
            })
            .unwrap();
        assert!(full < handed);
    }

    let logs = service.request_log().unwrap();
    let outcomes = service.outcomes().unwrap();
    let mut accepted = BTreeSet::new();
    for request in logs
        .iter()
        .filter(|r| r.request_id > after_baseline && r.kind == RequestKind::Modify)
    {
        assert_eq!(request.scope, files.scope);
        assert_eq!(request.zone.as_deref(), Some(files.collection_zone.as_str()));
        assert_eq!(request.caller.instance.as_deref(), Some(rig.instance().as_str()));
        assert_eq!(request.outcome, LogOutcome::Success);
        let outcome = outcomes.iter().find(|o| o.request_id == request.request_id).unwrap();
        let Ok(ReplyReference::Modified(items)) = &outcome.result else {
            panic!("actual Saved outcome required")
        };
        // This transport observation is the actual builder output handed to
        // Service::request, not an independently constructed expected batch.
        let sent = rig.sent_request(request.request_id).unwrap();
        for item in items {
            assert!(accepted.insert(item.name.clone()), "duplicate saved entity");
            let reference = item.result.as_ref().unwrap().as_ref().unwrap();
            let saved = service.resolve_record(&files.scope, reference).unwrap();
            let (envelope, payload) = decode(&files, &saved);
            assert_eq!(&payload, expected.get(&item.name).unwrap());
            let Mutation::Save { record, if_unchanged } = sent
                .mutations
                .iter()
                .find(|m| {
                    matches!(m,
                Mutation::Save { record, .. } if record.name == item.name)
                })
                .unwrap()
            else {
                unreachable!()
            };
            assert_eq!(record.fields, saved.fields);
            match before_records.get(&item.name) {
                Some(previous) => {
                    assert_eq!(if_unchanged.as_ref(), Some(&previous.system_fields));
                    let old = decode(&files, previous).0;
                    assert_eq!(envelope.fields.erev, old.fields.erev + 1);
                    assert_eq!(envelope.fields.base_sha256, Some(old.payload_sha256));
                }
                None => {
                    assert!(if_unchanged.is_none());
                    assert_eq!(envelope.fields.erev, 1);
                    assert_eq!(envelope.fields.base_sha256, None);
                }
            }
            let facts = rig.entity_facts(&item.name).unwrap();
            assert_eq!(facts.acked_system_fields, saved.system_fields.bytes());
            assert_eq!(facts.acked_row_digest, payload.row_digest().unwrap());
            assert_eq!(facts.acked_payload_digest, payload.payload_digest().unwrap());
            assert_eq!(facts.acked_rev, facts.local_rev);
            assert!(facts.mark.is_none());
        }
    }
    assert_eq!(accepted, changed);
    let final_records = server_records(&service, &files);
    assert_eq!(final_records.len(), expected.len());
    for (name, payload) in &expected {
        assert_eq!(&decode(&files, &final_records[name]).1, payload);
    }
    for (name, payload) in &prior_history {
        assert_eq!(expected.get(name), Some(payload));
        assert_eq!(&decode(&files, &final_records[name]).1, payload);
    }
    assert!(rig.tracking_pending_names().unwrap().is_empty());
    assert!(rig.sdk_pending().is_empty());
    rig.close_clean().unwrap();
    // Canonical payloads are compared byte-exact. Only excluded native metadata
    // (usn/collection bookkeeping times, K11.3) may vary; Revlog.time is retained.
    assert_eq!(files.read_collection_payloads().unwrap(), expected);
    let mut reopened = SDKRig::open(&files, true, NOW_MS + 2_500, SEED).unwrap();
    reopened.reconcile_and_confirm_root(&mut service, STEPS).unwrap();
    assert_root_response(&service, &files, &reopened);
    let durable_card = reopened.driver().read_card(card_after.id).unwrap();
    assert_eq!(durable_card.reps, card_after.reps);
    assert_eq!(durable_card.queue, card_after.queue);
    assert_eq!(durable_card.due, card_after.due);
    assert_eq!(durable_card.last_review_id, revlog.id);
    assert_eq!(durable_card, card_after);
    let durable_revlog = reopened.driver().read_revlog(revlog.id).unwrap();
    assert_eq!(durable_revlog, revlog); // includes cid/ease/ivl/last_ivl/factor/time/type
    assert_eq!(reopened.driver().collection_payloads().unwrap(), expected);
    assert!(reopened.tracking_pending_names().unwrap().is_empty());
    assert_eq!(server_records(&service, &files), final_records);
    reopened.close_clean().unwrap();
}

// Infrastructure entry for the shared CoreHost subprocess protocol.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
