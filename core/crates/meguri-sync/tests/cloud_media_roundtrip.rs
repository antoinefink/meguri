#[path = "support/publication_incoming.rs"]
mod support;
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::entity_codec::{self as codec, Payload};
use meguri_testkit::{
    cloud::{Config, Service, coordinator::PublicationRig, devices::PreboundDevice},
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
    let receiver_dir = generated(root.path(), "receiver");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut sender = online(&sender_dir, true);
    sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    publish(&mut sender, &mut service);
    let before = sender.driver().collection_payloads().unwrap();
    let original_files = sender.driver().media_files().unwrap();
    assert_eq!(original_files["photo.jpg"], REMOTE);
    let prior_history = before
        .iter()
        .filter(|(_, row)| matches!(row, Payload::Revlog(_)))
        .collect::<Vec<_>>();
    assert!(!prior_history.is_empty());

    let mut receiver = online(&receiver_dir, true);
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
