#[path = "support/publication_incoming.rs"]
mod support;
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::entity_codec::{self as codec, Payload};
use meguri_testkit::{
    cloud::{
        Config, Field, LogOutcome, ReplyReference, RequestKind, Service, coordinator::PublicationRig,
        devices::PreboundDevice,
    },
    generate::recipe::{Recipe, generate_recipe},
};
use support::*;

#[test]
fn actual_blob_bytes_survive_adoption_reopen_and_reverse_direction() {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    pair.receiver.driver().put_media("reverse.bin", LOCAL).unwrap();
    let result = pair.receiver.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.receiver.last_sync_phase()));
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
    assert_eq!(pair.sender.driver().media_files().unwrap()["reverse.bin"], LOCAL);
    pair.close();
}

#[test]
fn adopted_device_reviews_and_uploads_real_rows_and_media_retained_on_both_reopens() {
    let Pair {
        mut service,
        mut sender,
        mut receiver,
        receiver_dir,
        root,
    } = published_review_pair(false);
    let sender_dir = root.path().join("sender");
    let before = sender.driver().collection_payloads().unwrap();
    let original_files = sender.driver().media_files().unwrap();
    assert_eq!(original_files["photo.jpg"], REMOTE);
    let prior_history = before
        .iter()
        .filter(|(_, row)| matches!(row, Payload::Revlog(_)))
        .collect::<Vec<_>>();
    assert!(!prior_history.is_empty());

    let preview = receiver.preview(&mut service, STEPS).unwrap();
    receiver.adopt_cloud(&preview).unwrap();
    receiver.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(receiver.driver().collection_payloads().unwrap(), before);
    assert_eq!(receiver.driver().media_files().unwrap(), original_files);
    assert_completed(&mut receiver);

    // Select a real adopted new card's note; Begin/Good/End cross CoreHost's
    // review service and application barrier, with the shared injected clock.
    let adopted = receiver.driver().collection_rows().unwrap();
    let selected = adopted
        .iter()
        .find_map(|row| match row {
            Payload::Card(card) if card.card_type == 0 => Some(card.clone()),
            _ => None,
        })
        .unwrap();
    assert!(
        adopted
            .iter()
            .any(|row| matches!(row, Payload::Note(note) if note.id == selected.nid))
    );
    let server_before_review = service.inspect(&scope()).unwrap().unwrap().zones;
    let session = receiver.driver().review_begin(selected.nid).unwrap();
    let question = receiver.driver().review_start(session).unwrap();
    assert_eq!(question.id, selected.id);
    let card_before = receiver.driver().read_card(question.id).unwrap();
    receiver.advance_clock_ms(1_000).unwrap();
    receiver.driver().review_show_answer(session, question.id).unwrap();
    receiver.advance_clock_ms(1_500).unwrap();
    let answer = receiver.driver().review_answer_good(session, question.id).unwrap();
    let card = receiver.driver().read_card(question.id).unwrap();
    let revlog = receiver.driver().read_revlog(answer.revlog_id).unwrap();
    assert_eq!(card.id, card_before.id);
    assert_eq!(card.nid, selected.nid);
    assert_eq!(card.reps, card_before.reps + 1);
    assert_ne!(card.card_type, card_before.card_type);
    assert_ne!(card.queue, card_before.queue);
    assert_ne!(card.due, card_before.due);
    assert_eq!(card.last_review_id, revlog.id);
    assert_eq!(revlog.id, answer.revlog_id);
    assert!(revlog.id >= NOW + 2_500); // retain the actual millisecond id below
    assert_eq!(revlog.cid, card.id);
    assert_eq!(revlog.ease, 3);
    assert_eq!(revlog.time, codec::Stored::Integer(2_500));
    let held_rows = receiver.driver().collection_payloads().unwrap();
    let card_name = held_rows
        .iter()
        .find(|(_, row)| **row == Payload::Card(card.clone()))
        .unwrap()
        .0
        .clone();
    let revlog_name = held_rows
        .iter()
        .find(|(_, row)| **row == Payload::Revlog(revlog.clone()))
        .unwrap()
        .0
        .clone();
    assert!(!before.contains_key(&revlog_name));
    let hold = receiver.driver().read_review_hold(session).unwrap();
    assert!(hold.op_ids.contains(&answer.op_id));
    assert!(hold.entity_names.contains(&card_name));
    assert!(hold.entity_names.contains(&revlog_name));
    receiver.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(service.inspect(&scope()).unwrap().unwrap().zones, server_before_review);

    receiver.driver().review_end_and_drain(session).unwrap();
    receiver.driver().put_media("review-added.bin", LOCAL).unwrap();
    let expected = receiver.driver().collection_payloads().unwrap();
    let expected_files = receiver.driver().media_files().unwrap();
    assert_eq!(expected[&card_name], Payload::Card(card.clone()));
    assert_eq!(expected[&revlog_name], Payload::Revlog(revlog.clone()));
    assert_eq!(expected.len(), before.len() + 1);
    for (name, row) in &before {
        assert!(expected.contains_key(name), "prior collection row was removed");
        // Deck study counters/config may legitimately change during review.
        // Notes, prior history, schema, presets and every other card stay exact.
        if name != &card_name && !matches!(row, Payload::Deck(_) | Payload::Config(_)) {
            assert_eq!(expected.get(name), Some(row));
        }
    }
    for (name, row) in prior_history {
        assert_eq!(expected.get(name), Some(row));
    }
    assert_eq!(expected_files.len(), original_files.len() + 1);
    for (name, bytes) in &original_files {
        assert_eq!(expected_files.get(name), Some(bytes));
    }
    assert_eq!(expected_files["review-added.bin"], LOCAL);
    let result = receiver.sync_now_and_drain(&mut service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", receiver.last_sync_phase()));
    assert_completed(&mut receiver);
    receiver.close_clean().unwrap();
    receiver = online(&receiver_dir, false); // no new key bytes available
    receiver.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(receiver.driver().read_card(card.id).unwrap(), card);
    assert_eq!(receiver.driver().read_revlog(revlog.id).unwrap(), revlog);
    assert_eq!(receiver.driver().collection_payloads().unwrap(), expected);
    assert_eq!(receiver.driver().media_files().unwrap(), expected_files);
    assert_completed(&mut receiver);

    let result = sender.sync_now_and_drain(&mut service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", sender.last_sync_phase()));
    assert_eq!(sender.driver().read_card(card.id).unwrap(), card);
    assert_eq!(sender.driver().read_revlog(revlog.id).unwrap(), revlog);
    assert_eq!(sender.driver().collection_payloads().unwrap(), expected);
    assert_eq!(sender.driver().media_files().unwrap(), expected_files);
    assert_completed(&mut sender);
    sender.close_clean().unwrap();
    sender = online(&sender_dir, false);
    sender.drive_ready(&mut service, STEPS).unwrap();
    // Whole Card/Revlog equality includes ids, reps, queue/type, due/interval,
    // last_review_id, ease and time; these reads never substitute tracking rows.
    assert_eq!(sender.driver().read_card(card.id).unwrap(), card);
    assert_eq!(sender.driver().read_revlog(revlog.id).unwrap(), revlog);
    assert_eq!(sender.driver().collection_payloads().unwrap(), expected);
    assert_eq!(sender.driver().media_files().unwrap(), expected_files);
    assert_completed(&mut sender);
    receiver.close_clean().unwrap();
    sender.close_clean().unwrap();
}

#[test]
fn durable_review_with_lost_capture_recovers_card_and_new_revlog_after_crash() {
    let mut pair = published_review_pair(false);
    let before = pair.sender.driver().collection_payloads().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), before);
    let binding = pair.receiver.facts().unwrap().current.unwrap();
    let selected = before
        .values()
        .find_map(|row| match row {
            Payload::Card(card) if card.card_type == 0 => Some(card.clone()),
            _ => None,
        })
        .unwrap();
    let session = pair.receiver.driver().review_begin(selected.nid).unwrap();
    let question = pair.receiver.driver().review_start(session).unwrap();
    assert_eq!(question.id, selected.id);
    let card_before = pair.receiver.driver().read_card(question.id).unwrap();
    pair.receiver.advance_clock_ms(1_000).unwrap();
    pair.receiver.driver().review_show_answer(session, question.id).unwrap();
    pair.receiver.advance_clock_ms(1_500).unwrap();
    let tracking_before = pair.receiver.facts().unwrap().entities;
    let runtime_before = pair.receiver.driver().stored_runtime_state().unwrap();
    let snapshot_before = pair.receiver.driver().incoming_observation(binding.id).unwrap();
    let request_before = pair.service.request_log().unwrap().last().unwrap().request_id;

    // Fail a real tracking write in the answer's observer transaction, not a
    // note-ID match. The collection commit and live review hold remain real.
    pair.receiver.driver().arm_capture_failure_for_testing(1).unwrap();
    let answer = pair.receiver.driver().review_answer_good(session, question.id).unwrap();
    let card = pair.receiver.driver().read_card(question.id).unwrap();
    let revlog = pair.receiver.driver().read_revlog(answer.revlog_id).unwrap();
    assert_eq!(card.id, selected.id);
    assert_eq!(card.nid, selected.nid);
    assert_eq!(card.reps, card_before.reps + 1);
    assert_ne!(card.card_type, card_before.card_type);
    assert_ne!(card.queue, card_before.queue);
    assert_ne!(card.due, card_before.due);
    assert_eq!(card.last_review_id, revlog.id);
    assert_eq!(revlog.cid, card.id);
    assert_eq!(revlog.id, answer.revlog_id);
    assert!(revlog.id >= NOW + 2_500);
    assert_eq!(revlog.ease, 3);
    assert_eq!(revlog.time, codec::Stored::Integer(2_500));
    let committed = pair.receiver.driver().incoming_observation(binding.id).unwrap();
    assert!(committed.seq > snapshot_before.seq);
    assert_eq!(committed.generation, snapshot_before.generation);
    assert_eq!(committed.native_file, snapshot_before.native_file);
    let capture = pair
        .receiver
        .driver()
        .capture_proof(committed.seq, committed.generation)
        .unwrap();
    assert_eq!(capture.observed_seq, committed.seq);
    assert_eq!(capture.generation, committed.generation);
    assert!(capture.attempted);
    assert!(!capture.persisted);
    assert_eq!(pair.receiver.facts().unwrap().entities, tracking_before);
    assert_eq!(
        pair.receiver.driver().stored_runtime_state().unwrap().change_counter,
        runtime_before.change_counter
    );
    let scan = pair.receiver.driver().scan_state().unwrap();
    assert!(scan.scan_required && scan.tracking_stopped && !scan.scan_running);
    let receipt = pair.receiver.driver().wal_proof(committed.seq).unwrap();
    assert!(receipt.through >= committed.seq);
    assert_eq!(receipt.generation, committed.generation);
    assert_eq!(receipt.native_file, committed.native_file);
    let card_name =
        codec::entity_name(&binding.facts.record_key.0, codec::Kind::Card, &codec::id_key(card.id)).unwrap();
    let revlog_name = codec::entity_name(
        &binding.facts.record_key.0,
        codec::Kind::Revlog,
        &codec::id_key(revlog.id),
    )
    .unwrap();
    assert!(!before.contains_key(&revlog_name));
    assert!(!tracking_before.contains_key(&revlog_name));
    let hold = pair.receiver.driver().read_review_hold(session).unwrap();
    assert!(hold.op_ids.contains(&answer.op_id));
    assert!(hold.entity_names.contains(&card_name));
    assert!(hold.entity_names.contains(&revlog_name));
    let cloud_hold = pair.receiver.driver().read_cloud_review_hold(session).unwrap();
    assert!(cloud_hold.op_ids.contains(&answer.op_id));
    assert!(cloud_hold.entity_names.contains(&card_name));
    assert!(cloud_hold.entity_names.contains(&revlog_name));
    let expected = pair.receiver.driver().collection_payloads().unwrap();
    let reviewed = [
        (card_name, Payload::Card(card.clone())),
        (revlog_name, Payload::Revlog(revlog.clone())),
    ];
    assert_eq!(expected.len(), before.len() + 1);
    for (name, payload) in &reviewed {
        assert_eq!(expected.get(name), Some(payload));
    }
    assert_eq!(
        pair.service.request_log().unwrap().last().unwrap().request_id,
        request_before
    );

    // No End, Sync Now, drive or reconcile after the failed capture. SIGKILL
    // releases the process's review session; reopen must recover the same files.
    let killed = pair.receiver.kill().unwrap();
    assert_eq!(killed.signal, Some(9));
    assert!(!killed.clean_close);
    pair.reopen_before_identity();
    assert_eq!(pair.receiver.facts().unwrap().native_file, receipt.native_file);
    assert!(pair.receiver.facts().unwrap().current.as_ref() == Some(&binding));
    assert_eq!(pair.receiver.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.receiver.driver().read_revlog(revlog.id).unwrap(), revlog);
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), expected);
    assert_eq!(pair.receiver.facts().unwrap().entities, tracking_before);
    let scan = pair.receiver.driver().scan_state().unwrap();
    assert!(scan.scan_required && !scan.scan_running && !scan.scan_completed_this_open);
    pair.receiver.verify_identity(scope().account).unwrap();
    assert!(pair.receiver.trace().iter().any(|event| matches!(
        event,
        meguri_sync::cloud::workflow::TraceEvent::IdentityVerified { account, .. }
            if account == &scope().account
    )));
    pair.receiver
        .driver()
        .reconcile_binding()
        .unwrap_or_else(|error| panic!("ordinary reconcile must recover the durable review's new Revlog: {error:?}"));
    let scan = pair.receiver.driver().scan_proof().unwrap();
    let pending = pair.receiver.facts().unwrap().entities;
    for (name, payload) in &reviewed {
        assert!(scan.recovered_names.contains(name));
        assert_eq!(&pending[name].payload, payload);
        assert!(pending[name].mark.is_some());
        assert!(pending[name].local_rev > pending[name].acked_rev);
    }
    assert!(pending[&reviewed[1].0].acked_payload.is_none());
    assert!(
        pair.receiver
            .driver()
            .read_cloud_review_hold(session)
            .unwrap()
            .op_ids
            .is_empty()
    );
    let result = pair.receiver.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.receiver.last_sync_phase()));
    assert_review_saved(&mut pair.receiver, &pair.service, request_before, &reviewed);
    assert_completed(&mut pair.receiver);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.receiver.driver().read_revlog(revlog.id).unwrap(), revlog);
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), expected);
    assert_review_saved(&mut pair.receiver, &pair.service, request_before, &reviewed);
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
    assert_eq!(pair.sender.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.sender.driver().read_revlog(revlog.id).unwrap(), revlog);
    assert_eq!(pair.sender.driver().collection_payloads().unwrap(), expected);
    assert_completed(&mut pair.sender);
    pair.close();
}

#[test]
fn durable_manual_bury_with_lost_capture_recovers_card_and_new_config_after_crash() {
    let mut pair = published_review_pair(true);
    let before = pair.sender.driver().collection_payloads().unwrap();
    assert!(
        !before
            .values()
            .any(|row| matches!(row, Payload::Config(config) if config.key == b"lastUnburied")),
        "the published fixture must lack lastUnburied"
    );
    let original_files = pair.sender.driver().media_files().unwrap();
    assert_eq!(original_files["photo.jpg"], REMOTE);
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), before);
    assert_eq!(pair.receiver.driver().media_files().unwrap(), original_files);
    assert_completed(&mut pair.receiver);
    let binding = pair.receiver.facts().unwrap().current.unwrap();
    let selected = before
        .values()
        .find_map(|row| match row {
            Payload::Card(card) if card.card_type == 0 => Some(card.clone()),
            _ => None,
        })
        .unwrap();
    let card_name = codec::entity_name(
        &binding.facts.record_key.0,
        codec::Kind::Card,
        &codec::id_key(selected.id),
    )
    .unwrap();
    let config_name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Config, b"lastUnburied").unwrap();
    let tracking_before = pair.receiver.facts().unwrap().entities;
    assert!(!before.contains_key(&config_name));
    assert!(!tracking_before.contains_key(&config_name));
    assert_eq!(tracking_before[&card_name].payload, Payload::Card(selected.clone()));
    let runtime_before = pair.receiver.driver().stored_runtime_state().unwrap();
    let snapshot_before = pair.receiver.driver().incoming_observation(binding.id).unwrap();
    let server_before = pair.service.inspect(&scope()).unwrap().unwrap().zones;
    let request_before = pair.service.request_log().unwrap().last().unwrap().request_id;
    // Inspect the actual adopted rows immediately before arming. There is no
    // active review, fixture edit, tracking seed or ACK seed at this boundary.
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), before);
    pair.receiver.driver().arm_capture_failure_for_testing(1).unwrap();
    pair.receiver.driver().bury_card(selected.id).unwrap();
    let card = pair.receiver.driver().read_card(selected.id).unwrap();
    assert_eq!(card.queue, -3); // real manual bury, not a sibling scheduling edit
    assert_eq!(card.id, selected.id);
    assert_eq!(card.nid, selected.nid);
    assert_eq!(card.card_type, selected.card_type);
    assert_eq!(card.reps, selected.reps);
    assert_eq!(card.last_review_id, selected.last_review_id);
    let expected = pair.receiver.driver().collection_payloads().unwrap();
    let Payload::Config(config) = &expected[&config_name] else {
        panic!("manual bury must create lastUnburied")
    };
    assert_eq!(config.key, b"lastUnburied");
    assert_eq!(
        config.val,
        meguri_testkit::generate::recipe::SPAN_DAYS.to_string().as_bytes()
    );
    assert_eq!(config.mtime_secs, NOW / 1_000);
    let buried = [
        (card_name, Payload::Card(card.clone())),
        (config_name, Payload::Config(config.clone())),
    ];
    assert_eq!(expected.len(), before.len() + 1);
    for (name, payload) in &buried {
        assert_eq!(expected.get(name), Some(payload));
    }
    for (name, payload) in &before {
        if name != &buried[0].0 {
            assert_eq!(expected.get(name), Some(payload));
        }
    }
    let committed = pair.receiver.driver().incoming_observation(binding.id).unwrap();
    assert!(committed.seq > snapshot_before.seq);
    assert_eq!(committed.generation, snapshot_before.generation);
    assert_eq!(committed.native_file, snapshot_before.native_file);
    let capture = pair
        .receiver
        .driver()
        .capture_proof(committed.seq, committed.generation)
        .unwrap();
    assert_eq!(capture.observed_seq, committed.seq);
    assert_eq!(capture.generation, committed.generation);
    assert!(capture.attempted && !capture.persisted);
    assert_eq!(pair.receiver.facts().unwrap().entities, tracking_before);
    assert_eq!(
        pair.receiver.driver().stored_runtime_state().unwrap().change_counter,
        runtime_before.change_counter
    );
    let scan = pair.receiver.driver().scan_state().unwrap();
    assert!(scan.scan_required && scan.tracking_stopped && !scan.scan_running);
    let receipt = pair.receiver.driver().wal_proof(committed.seq).unwrap();
    assert!(receipt.through >= committed.seq);
    assert_eq!(receipt.generation, committed.generation);
    assert_eq!(receipt.native_file, committed.native_file);
    assert_eq!(pair.receiver.driver().media_files().unwrap(), original_files);
    assert_eq!(pair.service.inspect(&scope()).unwrap().unwrap().zones, server_before);
    assert_eq!(
        pair.service.request_log().unwrap().last().unwrap().request_id,
        request_before
    );

    // Kill before any drive, Sync Now or reconcile can repair the failed
    // observer transaction. Reopen the same physical files without new keys.
    let killed = pair.receiver.kill().unwrap();
    assert_eq!(killed.signal, Some(9));
    assert!(!killed.clean_close);
    pair.reopen_before_identity();
    assert_eq!(pair.receiver.facts().unwrap().native_file, receipt.native_file);
    assert!(pair.receiver.facts().unwrap().current.as_ref() == Some(&binding));
    assert_eq!(pair.receiver.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), expected);
    assert_eq!(pair.receiver.driver().media_files().unwrap(), original_files);
    assert_eq!(pair.receiver.facts().unwrap().entities, tracking_before);
    let scan = pair.receiver.driver().scan_state().unwrap();
    assert!(scan.scan_required && !scan.scan_running && !scan.scan_completed_this_open);
    pair.receiver.verify_identity(scope().account).unwrap();
    assert!(pair.receiver.trace().iter().any(|event| matches!(
        event,
        meguri_sync::cloud::workflow::TraceEvent::IdentityVerified { account, .. }
            if account == &scope().account
    )));
    let reopened = pair.receiver.driver().incoming_observation(binding.id).unwrap();
    pair.receiver
        .driver()
        .reconcile_binding()
        .unwrap_or_else(|error| panic!("ordinary reconcile must recover manual bury's new Config: {error:?}"));
    let scan = pair.receiver.driver().scan_proof().unwrap();
    assert!(scan.snapshot_seq >= reopened.seq);
    assert_eq!(scan.snapshot_generation, reopened.generation);
    assert_eq!(scan.snapshot_native_file, reopened.native_file);
    assert_eq!(scan.receipt.generation, reopened.generation);
    assert_eq!(scan.receipt.native_file, reopened.native_file);
    assert!(scan.receipt.through >= scan.snapshot_seq);
    let pending = pair.receiver.facts().unwrap().entities;
    for (name, payload) in &buried {
        assert!(scan.recovered_names.contains(name));
        assert_eq!(&pending[name].payload, payload);
        assert!(pending[name].mark.is_some());
        assert!(pending[name].local_rev > pending[name].acked_rev);
    }
    assert!(pending[&buried[1].0].acked_payload.is_none());
    assert!(pending[&buried[1].0].base_payload.is_none());
    let result = pair.receiver.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.receiver.last_sync_phase()));
    assert_review_saved(&mut pair.receiver, &pair.service, request_before, &buried);
    assert_completed(&mut pair.receiver);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.receiver.driver().collection_payloads().unwrap(), expected);
    assert_eq!(pair.receiver.driver().media_files().unwrap(), original_files);
    assert_review_saved(&mut pair.receiver, &pair.service, request_before, &buried);
    assert_completed(&mut pair.receiver);
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
    assert_eq!(pair.sender.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.sender.driver().collection_payloads().unwrap(), expected);
    assert_eq!(pair.sender.driver().media_files().unwrap(), original_files);
    assert_completed(&mut pair.sender);
    let sender_dir = pair.root.path().join("sender");
    pair.sender.close_clean().unwrap();
    pair.sender = online(&sender_dir, false);
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.sender.driver().read_card(card.id).unwrap(), card);
    assert_eq!(pair.sender.driver().collection_payloads().unwrap(), expected);
    assert_eq!(pair.sender.driver().media_files().unwrap(), original_files);
    assert_completed(&mut pair.sender);
    pair.close();
}

fn assert_review_saved(
    rig: &mut PublicationRig,
    service: &Service<SeededRandom>,
    after: u64,
    reviewed: &[(String, Payload)],
) {
    let facts = rig.facts().unwrap();
    let binding = facts.current.unwrap();
    let zone = rig.collection_zone().unwrap();
    let outcomes = service.outcomes().unwrap();
    for (name, payload) in reviewed {
        let ack = &facts.entities[name];
        assert_eq!(ack.local_rev, ack.acked_rev);
        assert!(ack.mark.is_none() && ack.base_payload.is_none() && ack.in_flight.is_empty());
        assert_eq!(ack.acked_payload.as_ref(), Some(payload));
        let mut saved_count = 0;
        for request in service.request_log().unwrap().iter().filter(|request| {
            request.request_id > after
                && request.kind == RequestKind::Modify
                && request.scope == scope()
                && request.zone.as_deref() == Some(zone.as_str())
        }) {
            let outcome = outcomes
                .iter()
                .find(|outcome| outcome.request_id == request.request_id)
                .unwrap();
            let Ok(ReplyReference::Modified(items)) = &outcome.result else {
                continue;
            };
            for item in items.iter().filter(|item| &item.name == name) {
                let reference = item.result.as_ref().unwrap().as_ref().unwrap();
                assert!(
                    request
                        .records
                        .iter()
                        .any(|record| &record.name == name && record.outcome == LogOutcome::Success)
                );
                let saved = service.resolve_record(&scope(), reference).unwrap();
                let Field::EncryptedBytes(envelope) = &saved.fields["e"] else {
                    panic!("entity envelope")
                };
                let received =
                    codec::receive(&binding.facts.record_key.0, name, envelope, codec::AssetInput::Absent).unwrap();
                assert_eq!(received.payload.as_ref(), Some(payload));
                assert_eq!(ack.acked_system_fields, saved.system_fields.bytes());
                assert_eq!(ack.acked_envelope.0, *envelope);
                assert_eq!(ack.acked_row_digest, payload.row_digest().unwrap());
                assert_eq!(ack.acked_payload_digest, payload.payload_digest().unwrap());
                saved_count += 1;
            }
        }
        assert_eq!(saved_count, 1, "Saved outcome count mismatch");
    }
    assert!(rig.sdk_pending().is_empty());
    assert!(rig.sdk_database_pending().is_empty());
}

fn published_review_pair(omit_last_unburied: bool) -> Pair {
    let root = tempfile::tempdir().unwrap();
    let sender_dir = root.path().join("sender");
    let recipe = Recipe::parse(
        r#"recipe: published-review-media
description: Public synthetic review with retained history and media
now: "2025-01-01T00:00:00Z"
decks:
  - name: Generic::Review
    notetype: basic
    new: 1
    due: 1
    tags: [generic]
    fields: ['generic {n}<img src="photo.jpg">', 'back {n}']
"#,
    )
    .unwrap();
    let manifest = generate_recipe(&recipe, 801, &sender_dir).unwrap();
    assert_eq!(manifest.counts.notes, 2);
    assert!(manifest.counts.revlog > 0);
    PreboundDevice::select_review_deck(&sender_dir, NOW, 801).unwrap();
    if omit_last_unburied {
        // The generator seeds this to preserve generated buried cards. This
        // recipe has none; omit only that seed before any binding is published.
        let host = meguri_testkit::backup_support::open_restore_host(
            &sender_dir.join("collection.anki2"),
            &root.path().join("fixture-backups"),
            std::sync::Arc::new(meguri_model::clock::FixedClock::new(
                NOW,
                meguri_model::clock::UtcOffset::UTC,
            )),
            std::sync::Arc::new(meguri_storage::space::StatvfsSpace),
        )
        .unwrap();
        host.collection()
            .unwrap()
            .run(
                meguri_engine::OpSpec::named("omit generator unbury seed", meguri_model::changes::OpSource::LocalApi),
                |ctx| {
                    assert!(ctx.scope().remove_config("lastUnburied")?.changed());
                    Ok(())
                },
            )
            .unwrap();
        host.close().unwrap();
    }
    let receiver_dir = generated(root.path(), "receiver");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut sender = online(&sender_dir, true);
    sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    publish(&mut sender, &mut service);
    let receiver = online(&receiver_dir, true);
    Pair {
        service,
        sender,
        receiver,
        receiver_dir,
        root,
    }
}

fn assert_completed(rig: &mut PublicationRig) {
    let facts = rig.facts().unwrap();
    assert!(facts.recovery_ops.is_empty());
    let binding = facts.current.unwrap().id;
    assert!(rig.driver().apply_intents(binding).unwrap().is_empty());
    assert!(rig.sdk_database_pending().is_empty());
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
