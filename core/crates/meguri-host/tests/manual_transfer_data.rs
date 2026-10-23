//! Dedicated RF4 manual-transfer-data fixture/proof. Generated data only; ignored
//! in ordinary suites. The native test does all user-facing transfers/review.
#[path = "../../meguri-sync/tests/common/mod.rs"]
mod rig_common;

use meguri_engine::{EventKind, OpSource, OpSpec, Subscription};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::{FixedClock, UtcOffset},
    ids::{CardId, DeckId, NoteId},
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_storage::{
    ConfigValue, OpenOptions, ReadRows, ReaderRole,
    raw::{PlainDatabase, RawValue},
    space::StatvfsSpace,
};
use meguri_sync::{
    Endpoint, HostKey, Job, JobOptions, Method, Payload,
    oneway::TransferAccess,
    policy::{Direction, Phase, Scope},
    wire::{ServerMeta, decode_meta, encode_meta_request},
};
use serde_json::{Value, json};
use std::{fs, io::Write, net::ToSocketAddrs, path::Path, sync::Arc};

// Error payloads/row values may contain secrets or content. Never Debug-print
// them in nextest output (including on a negative test).
fn checked<T, E>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|_| panic!("manual transfer data operation failed"))
}
fn json_file(path: &Path) -> Value {
    checked(serde_json::from_slice(&checked(fs::read(path))))
}
fn write_json(path: &Path, value: Value) {
    checked(fs::write(path, checked(serde_json::to_vec(&value))));
}
fn account(index: usize) -> (String, String) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let example = checked(fs::read_to_string(root.join("docker/syncserver.env.example")));
    let prefix = format!("SYNC_USER{index}=");
    let entries: Vec<_> = example.lines().filter_map(|line| line.strip_prefix(&prefix)).collect();
    assert!(entries.len() == 1, "reserved synthetic account required");
    let (name, secret) = entries[0].split_once(':').expect("synthetic account pair required");
    let expected = match index {
        3 => ("meguri-c", "meguri-test-c"),
        4 => ("meguri-d", "meguri-test-d"),
        _ => panic!("unreserved account refused"),
    };
    assert!(
        (name, secret) == expected,
        "only reserved synthetic examples are permitted"
    );
    (name.into(), secret.into())
}
fn key(options: &JobOptions, index: usize) -> HostKey {
    let (name, password) = account(index);
    checked(meguri_sync::login_blocking(
        rig_common::threads(),
        options,
        &SeededRandom::new(200),
        &name,
        &password,
    ))
}
fn meta(options: &JobOptions, key: &HostKey) -> ServerMeta {
    let mut job = checked(Job::new(options, Some(key.clone()), &SeededRandom::new(201)));
    let bytes = rig_common::block_on(async {
        let response = checked(job.request(Method::Meta, Payload::Json(encode_meta_request())).await);
        checked(response.bytes().await)
    });
    checked(decode_meta(&bytes))
}
fn host_config(root: &Path, now: i64) -> HostConfig {
    let collection = root.join("collection");
    checked(fs::create_dir_all(&collection));
    HostConfig::new(
        collection,
        root.join("backups"),
        root.join("widget"),
        Arc::new(FixedClock::new(now, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(203)),
        Arc::new(StatvfsSpace),
    )
}
fn complete<T>(events: Subscription, task: ManualTransferTask<T>) -> T {
    loop {
        let event = checked(events.recv());
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == task.job.id) {
            break;
        }
    }
    checked(task.take_result().expect("completed task result"))
}
fn seed_one(root: &Path, options: &JobOptions, endpoint: &Endpoint, index: usize, now: i64) -> Value {
    assert!(matches!(index, 3 | 4), "account is not reserved for this fixture");
    let settings = host_config(root, now);
    checked(meguri_engine::bootstrap::create(
        &settings.collection_directory.join("collection.anki2"),
        settings.clock.as_ref(),
        settings.random.as_ref(),
    ));
    let host = checked(CoreHost::open(settings));
    let collection = checked(host.collection());
    checked(collection.run(
        OpSpec::named("RF4 generated account settings", OpSource::Maintenance),
        |operation| {
            let scope = operation.scope();
            for (name, value) in [
                ("fsrs", ConfigValue::Bool(true)),
                ("loadBalancerEnabled", ConfigValue::Bool(false)),
                ("rf4-account-index", ConfigValue::Integer(index as i128)),
            ] {
                scope.set_config(name, &value)?;
            }
            Ok(())
        },
    ));

    let mut identity = json!({});
    if index == 3 {
        let kind = checked(meguri_engine::notetypes::add_stock(
            &collection,
            meguri_engine::notetypes::StockKind::Basic,
            Some("RF4 generated review".into()),
        ))
        .value
        .id;
        let created = checked(meguri_engine::notes::add(
            &collection,
            meguri_engine::notes::AddNote {
                notetype_id: kind,
                fields: vec!["Review prompt".into(), "Review response".into()],
                tags: vec![],
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        ))
        .value;
        assert!(created.cards.len() == 1, "generated note must produce one card");
        let started = checked(host.review().start(DeckId(1), None));
        let meguri_engine::session::StartResult::Started { session_id, head, .. } = started else {
            panic!("generated card cannot start review")
        };
        assert!(head.items.len() == 1, "generated review must contain one item");
        checked(host.review().end(session_id));
        identity = json!({"noteID": created.id.0, "cardID": created.cards[0].0});
    }

    // Login and each host bind use separate transport runtime lifetimes.
    meguri_sync::transport::runtime::shut_down();
    let host_key = key(options, index);
    meguri_sync::transport::runtime::shut_down();
    let (username, _) = account(index);
    let access = || {
        checked(TransferAccess::new(
            endpoint.clone(),
            &username,
            host_key.clone(),
            options.clone(),
        ))
    };
    let subscription = host.engine().stream().subscribe();
    let pending = checked(host.prepare_manual_transfer(access(), Direction::Export, Scope::CollectionOnly));
    let confirmation = complete(subscription, pending).confirmation;
    let subscription = host.engine().stream().subscribe();
    let pending = checked(host.confirm_manual_transfer(access(), confirmation));
    let finished = complete(subscription, pending);
    assert!(
        finished.phase == Phase::Complete,
        "generated collection Export is incomplete"
    );

    let activated = checked(host.collection());
    let reader = checked(activated.reader(ReaderRole::Sync));
    let totals = checked(reader.transfer_facts()).totals;
    let expected = if index == 3 { 1 } else { 0 };
    assert!(totals.notes == expected, "generated note count differs");
    assert!(totals.cards == expected, "generated card count differs");
    assert!(totals.reviews == 0, "setup unexpectedly persisted a review");
    let marker = checked(reader.config_entry("rf4-account-index")).expect("generated account setting missing");
    assert!(
        marker.entry.value == index.to_string().as_bytes(),
        "generated account setting differs"
    );
    drop(reader);
    checked(host.close());
    meguri_sync::transport::runtime::shut_down();
    identity
}

fn downloaded(root: &Path, options: &JobOptions, host_key: &HostKey) {
    let mut job = checked(Job::new(options, Some(host_key.clone()), &SeededRandom::new(207)));
    rig_common::block_on(async {
        let mut body = checked(job.request(Method::Download, Payload::empty()).await);
        let mut file = checked(
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("peer.anki2")),
        );
        let mut size = 0usize;
        while let Some(chunk) = checked(body.chunk().await) {
            size += chunk.len();
            assert!(size <= 32 * 1024 * 1024, "tiny synthetic download size bound exceeded");
            checked(file.write_all(&chunk));
        }
        checked(file.sync_all());
    });
    assert!(
        checked(meguri_storage::one_way::verify_server_copy(&root.join("peer.anki2"))).is_some(),
        "peer must have current verified transfer facts; no upgrade transform is allowed"
    );
}

fn rows_equal(a: &PlainDatabase, b: &PlainDatabase, table: &str, order: &str) -> bool {
    let query = format!("SELECT * FROM {table} ORDER BY {order}");
    checked(a.rows(&query, &[])) == checked(b.rows(&query, &[]))
}
fn same_rows(a: &PlainDatabase, b: &PlainDatabase, table: &str, order: &str) {
    assert!(rows_equal(a, b, table, order), "logical cells differ in {table}");
}
fn verify(root: &Path, options: &JobOptions) {
    let seed = json_file(&root.join("seed.json"));
    let note_id = NoteId(seed["noteID"].as_i64().expect("generated note identity missing"));
    let card_id = CardId(seed["cardID"].as_i64().expect("generated card identity missing"));
    let account_b = key(options, 4);
    let before = meta(options, &account_b);
    downloaded(root, options, &account_b);
    let after = meta(options, &account_b);
    assert!(after.usn.0 == before.usn.0 + 1, "peer download sequence change differs");
    assert!(
        after.schema_modified == before.schema_modified,
        "peer download changed schema time"
    );
    let lower = before.server_time.0 * 1000;
    let upper = (after.server_time.0 + 1) * 1000;
    assert!(
        (lower..upper).contains(&after.modified.0),
        "peer modification time is outside observed bounds"
    );
    meguri_sync::transport::runtime::shut_down();

    let peer_path = root.join("peer.anki2");
    let clock = FixedClock::new(after.server_time.0 * 1000, UtcOffset::UTC);
    let random = SeededRandom::new(208);
    let collection = checked(meguri_storage::open_collection(
        &peer_path,
        &OpenOptions::new(&clock, &random),
    ));
    let reader = checked(collection.reader_factory().open(ReaderRole::Sync));
    let facts = checked(reader.transfer_facts());
    for total in [facts.totals.notes, facts.totals.cards, facts.totals.reviews] {
        assert!(total == 1, "peer must contain one generated note, card and review");
    }
    assert!(facts.pending_reviews == 0, "peer has an outstanding review");
    let note = checked(reader.note(note_id)).expect("generated note absent from peer");
    assert!(
        note.fields == "Review prompt\u{1f}Review response",
        "generated field content changed"
    );
    let card = checked(reader.card(card_id)).expect("generated card absent from peer");
    let reviews = checked(reader.revlog_after(None, 2));
    assert!(reviews.len() == 1, "peer review count must be exactly one");
    let saved = &reviews[0];
    assert!(saved.card_id == card_id, "review belongs to another card");
    assert!(saved.button == 4, "saved review must use Easy");
    assert!(saved.usn.0 == 0, "saved review was not uploaded");
    let setup_time = seed["server_time_ms"].as_i64().expect("setup time missing");
    assert!(
        (setup_time..=before.server_time.0 * 1000 + 999).contains(&saved.id.0),
        "review time is outside the setup-to-observation interval"
    );
    assert!(
        card.note_id == note_id && card.deck_id == DeckId(1),
        "card identity or deck changed"
    );
    assert!(
        card.reps == 1 && card.lapses == 0 && card.interval > 0,
        "review counters or interval differ"
    );
    assert!(
        card.card_type == CardType::Review && card.queue == CardQueue::Review,
        "Easy did not persist a review card"
    );
    assert!(
        card.usn.0 == 0 && card.data.memory_state.is_some(),
        "uploaded scheduling state is missing"
    );
    let answer_time = saved.id.0;
    let answer_duration = saved.answer_millis;
    drop(reader);
    checked(collection.close());

    // Replay on a seed copy at the saved answer's instant and duration. The
    // original fixture fixes fuzz inputs and disables load balancing.
    let replay_settings = host_config(&root.join("expected-review"), answer_time);
    let replay_path = replay_settings.collection_directory.join("collection.anki2");
    checked(fs::copy(root.join("seed-a/collection/collection.anki2"), &replay_path));
    let replay_host = checked(CoreHost::open(replay_settings));
    let started = checked(replay_host.review().start(DeckId(1), None));
    let meguri_engine::session::StartResult::Started { session_id, head, .. } = started else {
        panic!("generated seed cannot start replay")
    };
    let item = &head.items[0];
    assert!(item.card_id == card_id, "replay selected another card");
    let outcome = checked(replay_host.review().answer(
        session_id,
        card_id,
        meguri_sched::answer::Rating::Easy,
        item.state_token,
        EpochMillis(answer_time),
        answer_duration,
    ));
    assert!(
        matches!(outcome, meguri_engine::session::AnswerOutcome::Accepted { .. }),
        "replay rejected the saved Easy answer"
    );
    checked(replay_host.review().end(session_id));
    checked(replay_host.close());

    let peer = checked(PlainDatabase::open(&peer_path));
    let replay = checked(PlainDatabase::open(&replay_path));
    // Export assigned zero only to the pending card and review USNs.
    for table in ["cards", "revlog"] {
        checked(replay.execute(&format!("UPDATE {table} SET usn = 0 WHERE usn = -1"), &[]));
    }
    for table in ["notes", "cards", "revlog"] {
        same_rows(&replay, &peer, table, "id");
    }
    checked(replay.close());

    // Check the activated Export before applying O7.4 to a comparison copy.
    let activated = root.join("native.anki2");
    let validated = checked(meguri_storage::one_way::verify_server_copy(&activated));
    assert!(validated.is_some(), "activated Export is not a verified server copy");
    let expected_path = root.join("expected-download.anki2");
    checked(fs::copy(&activated, &expected_path));
    let expected = checked(PlainDatabase::open(&expected_path));
    let actual_stamps = checked(expected.rows("SELECT mod, scm, ls, usn FROM col", &[]));
    let export_stamps = vec![vec![
        RawValue::Integer(before.modified.0),
        RawValue::Integer(before.schema_modified.0),
        RawValue::Integer(before.modified.0),
        RawValue::Integer(i64::from(before.usn.0)),
    ]];
    assert!(
        actual_stamps == export_stamps,
        "activated Export stamps differ from the pre-download server"
    );
    checked(expected.execute(
        "UPDATE col SET mod = ?1, usn = ?2 WHERE id = 1",
        &[
            RawValue::Integer(after.modified.0),
            RawValue::Integer(i64::from(after.usn.0)),
        ],
    ));
    for (table, order) in [
        ("col", "id"),
        ("config", "\"KEY\""),
        ("deck_config", "id"),
        ("decks", "id"),
        ("notetypes", "id"),
        ("fields", "ntid, ord"),
        ("templates", "ntid, ord"),
        ("tags", "tag"),
        ("graves", "oid, type"),
        ("notes", "id"),
        ("cards", "id"),
        ("revlog", "id"),
    ] {
        same_rows(&expected, &peer, table, order);
    }
    checked(expected.close());
    checked(peer.close());
    write_json(
        &root.join("peer-proof.json"),
        json!({
            "complete": true, "notes": 1, "cards": 1, "reviews": 1,
            "easy_reviews": 1, "identity_content": true, "scheduling": true,
            "all_logical_cells": true, "declared_download_transforms": true,
            "download_transforms": [
                "O7.4: observed download time becomes col.mod",
                "O7.4: the server advances col.usn by exactly one",
            ],
        }),
    );
}

#[test]
fn native_peer_comparison_detects_content_and_schedule_mismatch() {
    let directory = checked(tempfile::tempdir());
    let settings = host_config(&directory.path().join("reference"), 1_790_000_000_000);
    let original_file = settings.collection_directory.join("collection.anki2");
    checked(meguri_engine::bootstrap::create(
        &original_file,
        settings.clock.as_ref(),
        settings.random.as_ref(),
    ));
    let host = checked(CoreHost::open(settings));
    let collection = checked(host.collection());
    let notetype = checked(meguri_engine::notetypes::add_stock(
        &collection,
        meguri_engine::notetypes::StockKind::Basic,
        Some("RF4 comparison fixture".into()),
    ))
    .value
    .id;
    let input = meguri_engine::notes::AddNote {
        notetype_id: notetype,
        fields: vec!["Review prompt".into(), "Review response".into()],
        tags: vec![],
        deck_id: DeckId(1),
        guid: None,
        update_add_defaults: false,
    };
    checked(meguri_engine::notes::add(&collection, input, OpSource::UserUndoable));
    checked(host.close());

    let comparison_file = directory.path().join("comparison.anki2");
    checked(fs::copy(original_file.as_path(), comparison_file.as_path()));
    let original = checked(PlainDatabase::open(original_file.as_path()));
    let comparison = checked(PlainDatabase::open(comparison_file.as_path()));
    for table in ["notes", "cards"] {
        assert!(
            rows_equal(&original, &comparison, table, "id"),
            "initial comparison differs"
        );
    }
    checked(comparison.execute("UPDATE notes SET flds = 'Changed prompt'", &[]));
    assert!(
        !rows_equal(&original, &comparison, "notes", "id"),
        "content mutation escaped comparison"
    );
    checked(comparison.execute("UPDATE cards SET reps = reps + 1", &[]));
    assert!(
        !rows_equal(&original, &comparison, "cards", "id"),
        "review-count mutation escaped comparison"
    );
    checked(original.close());
    checked(comparison.close());
}

#[test]
#[ignore = "RF4 controller child; reserved synthetic accounts only"]
fn rig_manual_transfer_data() {
    let directory = std::env::var("MEGURI_MANUAL_TRANSFER_DATA_RUN").expect("missing controller directory");
    let root = Path::new(&directory);
    let configuration = json_file(&root.join("rig-input.json"));
    let port = configuration["port"]
        .as_u64()
        .filter(|value| (1..=65535).contains(value))
        .expect("missing rig TCP port") as u16;
    let address = ("host.docker.internal", port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addresses| addresses.next())
        .unwrap_or_else(|| ([127, 0, 0, 1], port).into());
    let (transport, _route) = rig_common::rig_route(address);
    let endpoint = rig_common::base(&format!("http://sync-rig.test:{port}/"));
    let action = std::env::var("MEGURI_MANUAL_TRANSFER_DATA_MODE").expect("missing controller action");
    match action.as_str() {
        "seed" => {
            let credential = key(&transport, 3);
            let instant = meta(&transport, &credential).server_time.0 * 1000;
            meguri_sync::transport::runtime::shut_down();
            let identity = seed_one(&root.join("seed-a"), &transport, &endpoint, 3, instant);
            seed_one(&root.join("seed-b"), &transport, &endpoint, 4, instant);
            let proof = json!({
                "notes": 1, "cards": 1, "reviews": 0, "deckID": 1,
                "noteID": identity["noteID"], "cardID": identity["cardID"],
                "server_time_ms": instant, "distinct_accounts": account(3).0 != account(4).0,
                "empty_nondefault_b": true,
            });
            write_json(&root.join("seed.json"), proof);
        }
        "verify" => verify(root, &transport),
        _ => panic!("fixture accepts only setup or peer-verification actions"),
    }
}
