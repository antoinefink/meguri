#[allow(dead_code)]
#[path = "support/meguri_bootstrap_script.rs"] mod bootstrap;
#[path = "support/meguri_round_script.rs"] mod round;
use bootstrap as b;
use round as r;
use meguri_engine::{OpSource, OpSpec, media::MediaJobKind, session::{AnswerOutcome, StartResult}};
use meguri_host::{CoreHost, MeguriBootstrapDirection, MeguriCollectionState, MeguriRoundStart};
use meguri_model::{clock::{FixedClock, UtcOffset}, ids::DeckId, time::{EpochMillis, EpochSecs}, usn::Usn};
use meguri_storage::{ReadRows, ReaderRole, Record, StampMode};
use meguri_sync::{ErrorKind, Method, provider::{MeguriReadiness, MeguriRoundRefusal, Selection}};
use serde_json::{Value, json};
use std::{fs, sync::Arc, time::Duration};

fn provider_bytes(config: &meguri_host::HostConfig) -> Vec<u8> { fs::read(config.manual_transfer_directory.join("provider.json")).unwrap() }
fn ready(root: &std::path::Path) -> (Arc<CoreHost>, meguri_host::HostConfig, meguri_sync::provider::MeguriConnection) {
    let (host, config, connection, _) = b::ready(root); (Arc::new(host), config, connection)
}
fn bound(host: &CoreHost) -> meguri_sync::provider::MeguriBinding {
    let binding = host.provider_authority().snapshot().unwrap().meguri_binding.unwrap();
    let baseline = host.media().unwrap().store().sync_baseline().unwrap();
    assert_eq!(baseline.owner.as_ref(), Some(&binding.media_owner()));
    assert!(binding.media_epoch().matches(&baseline));
    assert_eq!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready);
    binding
}
fn pause(host: &CoreHost) {
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true; host.save_provider_settings(settings).unwrap();
}
fn upload_steps() -> Vec<r::Step> { vec![r::begin(0), r::Step::new(Method::UploadChanges, r::envelope(json!([1, 1]))), r::sanity(true)] }
fn receive_steps(name: &str, bytes: &[u8], hash: &str) -> Vec<r::Step> {
    vec![r::begin(1), r::changes(json!([[name, 1, hash]])), r::changes(json!([])),
        r::Step::new(Method::DownloadFiles, r::download_zip(name, bytes)), r::sanity(true)]
}
fn bind_server(root: &std::path::Path, bytes: &[u8], facts: &meguri_storage::one_way::LocalTransferFacts)
    -> (CoreHost, meguri_host::HostConfig, meguri_sync::provider::MeguriConnection) {
    let (host, config) = b::seeded(root);
    let connection = b::select(&host);
    // Every observation describes the same generated, closed S served below.
    let meta = || b::Reply::Bytes(serde_json::to_vec(&json!({
        "mod": facts.stamps.modified.0, "scm": facts.stamps.schema_modified.0, "usn": facts.stamps.usn.0,
        "ts": b::NOW / 1000, "msg": "", "cont": true, "empty": facts.totals.cards == 0, "media_usn": 0,
    })).unwrap());
    let script = b::Script::new(vec![(Method::Meta, meta()),
        (Method::Download, b::Reply::Bytes(bytes.to_vec())),
        (Method::Meta, meta()), (Method::Meta, meta())]);
    let preview = b::prepare(&host, &connection, &script, MeguriBootstrapDirection::UseServerCollection);
    let collection = b::confirm(&host, &connection, &script, preview);
    let complete = b::finish_empty_media(&host, &connection, &collection);
    host.complete_meguri_bootstrap(&connection, b::USER, &complete.id).unwrap();
    bound(&host);
    (host, config, connection)
}
#[test]
fn meguri_round_relays_answered_card_revlog_and_media_to_second_bound_device() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    // Generate S using real authoring and FSRS policy; row stamps predate A's
    // answer so the receiver's pending-local tie rule cannot hide a regression.
    let config = b::config(&root.path().join("server"), Arc::new(FixedClock::new(b::NOW - 10_000, UtcOffset::UTC)));
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
    let server = CoreHost::open(config.clone()).unwrap();
    let collection = server.collection().unwrap();
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    let kind = meguri_engine::notetypes::add_stock(&collection, meguri_engine::notetypes::StockKind::Basic, Some("Generated routine".into())).unwrap().value.id;
    let added = meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
        notetype_id: kind, fields: vec!["Generated question".into(), "Generated answer".into()],
        tags: vec![], deck_id: DeckId(1), guid: None, update_add_defaults: false,
    }, OpSource::UserUndoable).unwrap().value;
    let card_id = added.cards[0];
    collection.run(OpSpec::named("synthetic server stamps", OpSource::Rollover).stamp(StampMode::Raw), |ctx| {
        ctx.scope().set_config("loadBalancerEnabled", &meguri_storage::ConfigValue::Bool(false))?;
        let mut col = ctx.scope().col()?; col.modified = EpochMillis(b::NOW - 1000);
        col.last_sync = col.modified; col.schema_modified = EpochMillis(b::NOW - 5000); col.usn = Usn(8);
        ctx.scope().put_col(&col)?; Ok(())
    }).unwrap();
    assert!(server.derived().wait_idle(Duration::from_secs(10)));
    let server_facts = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.begin_snapshot().unwrap();
        snapshot.transfer_facts().unwrap()
    };
    assert!(server_facts.totals.cards > 0, "generated S contains a real card");
    drop(collection); server.close().unwrap();
    let server_bytes = fs::read(config.collection_directory.join("collection.anki2")).unwrap();
    let (a, _, ac) = bind_server(&root.path().join("A"), &server_bytes, &server_facts);
    let (bhost, bc, connection_b) = bind_server(&root.path().join("B"), &server_bytes, &server_facts);
    let a_epoch = bound(&a).media_epoch().epoch();
    let b_epoch = bound(&bhost).media_epoch().epoch();
    assert!(a.derived().wait_idle(Duration::from_secs(10)));
    let StartResult::Started { session_id, head, .. } = a.review().start(DeckId(1), None).unwrap() else { panic!("generated card is reviewable") };
    let item = head.items.iter().find(|item| item.card_id == card_id).unwrap();
    assert!(matches!(a.review().answer(session_id, card_id, meguri_sched::answer::Rating::Easy,
        item.state_token, EpochMillis(b::NOW), 321).unwrap(), AnswerOutcome::Accepted { .. }));
    a.review().end(session_id).unwrap(); // The real end releases newest-answer hold-back.
    let media_bytes = b"generated nonempty daily review asset";
    a.media().unwrap().add("routine.txt", media_bytes).unwrap();
    let script_a = r::Script::new(r::normal(&a, json!({"done": true}), r::empty_changes(), 8));
    let collection_a = r::complete(&a, &r::started(a.start_meguri_collection(r::access(&ac, &script_a))));
    assert_eq!(collection_a.collection.as_ref().unwrap().state, MeguriCollectionState::Merged);
    script_a.assert_done();
    let sent_chunks = script_a.bodies(Method::ApplyChunk);
    assert_eq!(sent_chunks.len(), 1);
    let sent: Value = serde_json::from_slice(&sent_chunks[0]).unwrap();
    let chunk = sent["chunk"].clone();
    let decoded = meguri_sync::wire::chunk::decode_chunk(&serde_json::to_vec(&chunk).unwrap()).unwrap();
    assert_eq!(decoded.revlog.len(), 1);
    let sent_card = decoded.cards.iter().find(|card| card.id == card_id).unwrap();
    assert_eq!(decoded.revlog[0].card_id, card_id);
    assert_eq!(decoded.revlog[0].answer_millis, 321);
    assert_eq!(sent_card.reps, 1);
    // Protocol stamps are explicit: A's outgoing tuple and final local rows use
    // server USN 8; B's own next col.usn becomes 10 after its server-USN-9 round.
    assert_eq!(sent_card.usn, Usn(8)); assert_eq!(decoded.revlog[0].usn, Usn(8));
    let script_media_a = r::Script::new(upload_steps());
    let uploaded = r::complete(&a, &r::started(a.start_meguri_media(r::access(&ac, &script_media_a))));
    assert!(uploaded.media.as_ref().unwrap().complete); script_media_a.assert_done();
    let uploaded_zip = script_media_a.bodies(Method::UploadChanges);
    let entries = r::stored_entries(&uploaded_zip[0]);
    let map: Value = serde_json::from_slice(&entries["_meta"]).unwrap();
    assert_eq!(map, json!([["routine.txt", "0"]]));
    assert_eq!(entries["0"], media_bytes);
    let hash = a.media().unwrap().store().sync_entry("routine.txt").unwrap().unwrap().sha1.unwrap();
    let changes_a: Value = serde_json::from_slice(&script_a.bodies(Method::ApplyChanges)[0]).unwrap();
    let mut steps_b = r::normal(&bhost, chunk, changes_a["changes"].clone(), 9);
    steps_b[0].reply = r::meta(&bhost, b::NOW, 9); // A's acknowledged Finish is now the server's mod.
    let script_b = r::Script::new(steps_b);
    let collected = r::complete(&bhost, &r::started(bhost.start_meguri_collection(r::access(&connection_b, &script_b))));
    assert_eq!(collected.collection.as_ref().unwrap().state, MeguriCollectionState::Merged); script_b.assert_done();
    let media_b = r::Script::new(receive_steps("routine.txt", &entries["0"], &hash));
    let received = r::complete(&bhost, &r::started(bhost.start_meguri_media(r::access(&connection_b, &media_b))));
    assert!(received.media.as_ref().unwrap().complete); media_b.assert_done();
    let acoll = a.collection().unwrap(); let ar = acoll.reader(ReaderRole::Query).unwrap();
    let expected_card = ar.card(card_id).unwrap().unwrap(); let expected_review = ar.revlog_after(None, 10).unwrap()[0].clone();
    let bcoll = bhost.collection().unwrap(); let br = bcoll.reader(ReaderRole::Query).unwrap();
    assert_eq!(br.card(card_id).unwrap().unwrap().to_cells(), expected_card.to_cells());
    assert_eq!(br.revlog_entry(expected_review.id).unwrap().unwrap().to_cells(), expected_review.to_cells());
    assert_eq!(br.col().unwrap().usn, Usn(10));
    assert_eq!(br.card(card_id).unwrap().unwrap().due, expected_card.due);
    assert_eq!(br.card(card_id).unwrap().unwrap().mtime, EpochSecs(b::NOW / 1000));
    drop(br); drop(bcoll); drop(ar); drop(acoll);
    assert_eq!(fs::read(bhost.media().unwrap().store().media_dir().join("routine.txt")).unwrap(), media_bytes);
    assert_eq!(bound(&a).media_epoch().epoch(), a_epoch); assert_eq!(bound(&bhost).media_epoch().epoch(), b_epoch);
    bhost.close().unwrap();
    let reopened = CoreHost::open(bc).unwrap();
    let collection = reopened.collection().unwrap(); let reader = collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.card(card_id).unwrap().unwrap().to_cells(), expected_card.to_cells());
    assert_eq!(reader.revlog_entry(expected_review.id).unwrap().unwrap().to_cells(), expected_review.to_cells());
    drop(reader); drop(collection); bound(&reopened);
    assert_eq!(fs::read(reopened.media().unwrap().store().media_dir().join("routine.txt")).unwrap(), media_bytes);
    let unchanged = r::Script::new(vec![r::Step::new(Method::Meta, r::meta(&reopened, r::modified(&reopened), 10))]);
    let value = r::complete(&reopened, &r::started(reopened.start_meguri_collection(r::access(&connection_b, &unchanged))));
    assert_eq!(value.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges); unchanged.assert_done();
    let unchanged_media = r::Script::new(vec![r::begin(1)]);
    assert!(r::complete(&reopened, &r::started(reopened.start_meguri_media(r::access(&connection_b, &unchanged_media))))
        .media.as_ref().unwrap().complete); unchanged_media.assert_done();
    assert_eq!(bound(&reopened).media_epoch().epoch(), b_epoch);
    reopened.close().unwrap(); a.close().unwrap();
}

#[test]
fn normal_refusal_before_finish_and_mid_chunk_retains_original_error_and_pending_rows() {
    let _lane = b::LANE.lock().unwrap();
    for mid_chunk in [false, true] {
        let root = tempfile::tempdir().unwrap(); let (host, _, connection) = ready(root.path());
        r::pending(&host);
        let mut steps = r::normal(&host, json!({"done": true}), r::empty_changes(), 9);
        let cut = if mid_chunk { 3 } else { 5 };
        if mid_chunk { steps[cut].reply = br#"{"done":false}"#.to_vec(); }
        let owner = host.clone(); steps[cut].after = Some(Box::new(move || pause(&owner)));
        steps.truncate(cut + 1); steps.push(r::Step::new(Method::Abort, b"null".to_vec()));
        let script = r::Script::new(steps);
        let task = r::started(host.start_meguri_collection(r::access(&connection, &script)));
        let result = r::complete(&host, &task); script.assert_done();
        let collection = result.collection.as_ref().unwrap();
        assert_eq!(collection.state, MeguriCollectionState::Failed);
        assert_eq!(collection.error.as_ref().unwrap().kind, ErrorKind::Interrupted);
        assert_eq!(collection.refusal, Some(MeguriRoundRefusal::Paused)); assert!(!collection.acceptance_unknown);
        assert!(script.bodies(Method::Finish).is_empty());
        assert!(host.collection().unwrap().reader(ReaderRole::Query).unwrap().config_value("routine-generated").unwrap().is_some());
        assert!(Arc::ptr_eq(&result, &host.meguri_round_result(&task).unwrap().unwrap()));
        host.close().unwrap();
    }
}
#[test]
fn normal_no_changes_and_media_no_reset_preserve_provider_bytes() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, config, connection) = ready(root.path()); let before = provider_bytes(&config); let binding = bound(&host);
    let normal = r::Script::new(vec![r::Step::new(Method::Meta, r::meta(&host, r::modified(&host), 9))]);
    let result = r::complete(&host, &r::started(host.start_meguri_collection(r::access(&connection, &normal))));
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges); normal.assert_done();
    let media = r::Script::new(vec![r::begin(0)]);
    let result = r::complete(&host, &r::started(host.start_meguri_media(r::access(&connection, &media))));
    assert!(result.media.as_ref().unwrap().complete); media.assert_done();
    assert_eq!(provider_bytes(&config), before); assert_eq!(bound(&host), binding); host.close().unwrap();
}
#[test]
fn bound_media_owner_or_epoch_mismatch_has_no_request_or_mutation() {
    let _lane = b::LANE.lock().unwrap();
    for foreign_owner in [false, true] {
        let root = tempfile::tempdir().unwrap(); let (host, config, connection) = ready(root.path());
        let media = host.media().unwrap();
        if foreign_owner { media.store().bind_sync_owner(&meguri_storage::media::MediaBaselineOwner::new("foreign")).unwrap(); }
        else { media.store().reset_sync().unwrap(); }
        let before = media.store().sync_baseline().unwrap(); let entries = media.store().entries().unwrap();
        let provider = provider_bytes(&config); let script = r::Script::new(vec![]);
        assert!(matches!(host.start_meguri_media(r::access(&connection, &script)),
            MeguriRoundStart::Refused(MeguriRoundRefusal::BindingChanged)));
        assert!(matches!(host.start_meguri_collection(r::access(&connection, &script)),
            MeguriRoundStart::Refused(MeguriRoundRefusal::BindingChanged)));
        assert_eq!(media.store().sync_baseline().unwrap(), before); assert_eq!(media.store().entries().unwrap(), entries);
        assert_eq!(provider_bytes(&config), provider); assert!(script.bodies(Method::MediaBegin).is_empty());
        host.close().unwrap();
    }
}
#[test]
fn y11_one_retry_and_second_reset_publish_only_the_original_sealed_epoch() {
    let _lane = b::LANE.lock().unwrap();
    for second_mismatch in [false, true] {
        let root = tempfile::tempdir().unwrap(); let (host, _, connection) = ready(root.path());
        let initial = bound(&host); host.media().unwrap().add("routine.txt", b"generated bytes").unwrap();
        let hash = host.media().unwrap().store().sync_entry("routine.txt").unwrap().unwrap().sha1.unwrap();
        let mut steps = upload_steps(); *steps.last_mut().unwrap() = r::sanity(false);
        steps.extend([r::begin(1), r::changes(json!([["routine.txt", 1, hash]])), r::changes(json!([])), r::sanity(!second_mismatch)]);
        let script = r::Script::new(steps); let task = r::started(host.start_meguri_media(r::access(&connection, &script)));
        let result = r::complete(&host, &task); script.assert_done();
        assert!(result.recovery.is_none()); let media = result.media.as_ref().unwrap();
        if second_mismatch {
            assert!(!media.complete); assert_eq!(media.failure.as_ref().unwrap().error.kind, ErrorKind::MediaResyncFailed);
        } else { assert!(media.complete); }
        let updated = bound(&host); assert_ne!(updated.media_epoch().epoch(), initial.media_epoch().epoch());
        assert_eq!(updated.account(), initial.account()); assert_eq!(updated.operation(), initial.operation());
        let baseline = host.media().unwrap().store().sync_baseline().unwrap();
        if second_mismatch { assert_eq!(baseline.usn, 0); }
        let next = r::Script::new(if second_mismatch { vec![r::begin(1), r::changes(json!([["routine.txt", 1, hash]])), r::changes(json!([])), r::sanity(true)] }
            else { vec![r::begin(1)] });
        let result = r::complete(&host, &r::started(host.start_meguri_media(r::access(&connection, &next))));
        assert!(result.media.as_ref().unwrap().complete); next.assert_done();
        assert_eq!(bound(&host), updated); host.close().unwrap();
    }
}
#[test]
fn routine_full_sync_requirement_holds_only_its_binding_without_directional_dispatch() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path()); let original = bound(&host); r::pending(&host);
    let mut meta: Value = serde_json::from_slice(&r::meta(&host, r::modified(&host)-1000, 9)).unwrap();
    meta["scm"] = json!(meta["scm"].as_i64().unwrap()+1);
    let script = r::Script::new(vec![r::Step::new(Method::Meta, serde_json::to_vec(&meta).unwrap())]);
    let task = r::started(host.start_meguri_collection(r::access(&connection, &script)));
    let result = r::complete(&host, &task); script.assert_done();
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::FullSyncRequired);
    assert!(result.recovery.is_none()); let doc = host.provider_authority().snapshot().unwrap();
    assert!(doc.meguri_binding.is_none()); assert!(!doc.settings.automatic); assert!(!doc.meguri_settings.automatic);
    assert!(!doc.settings.paused); assert_eq!(doc.meguri.as_ref(), Some(original.connection()));
    assert!(script.bodies(Method::Upload).is_empty()); assert!(script.bodies(Method::Download).is_empty());
    assert!(Arc::ptr_eq(&result, &host.meguri_round_result(&task).unwrap().unwrap()));
    assert!(matches!(host.start_meguri_collection(r::access(&connection, &r::Script::new(vec![]))), MeguriRoundStart::Refused(MeguriRoundRefusal::NotReady)));
    host.close().unwrap();
}
#[test]
fn finished_full_sync_round_settles_its_original_handoff_and_exact_failed_hold() {
    use meguri_model::faultpoint::{self, Fault};
    let _lane = b::LANE.lock().unwrap();
    for terminal_error in [false, true] {
        for failed_write in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (host, config, connection) = ready(&root.path().join("original"));
            let (foreign, foreign_config, _) = ready(&root.path().join("foreign"));
            let foreign_bytes = provider_bytes(&foreign_config);
            let original = bound(&host); r::pending(&host);
            host.media().unwrap().add("pending.txt", b"generated pending media").unwrap();
            let media_before = host.media().unwrap().store().sync_baseline().unwrap();
            let entries_before = host.media().unwrap().store().entries().unwrap();
            let steps = if terminal_error {
                let mut steps = r::normal(&host, json!({"done": true}), r::empty_changes(), 9);
                steps[5].reply = br#"{"status":"bad"}"#.to_vec();
                steps.truncate(6); steps.push(r::Step::new(Method::Abort, b"null".to_vec())); steps
            } else {
                let mut meta: Value = serde_json::from_slice(&r::meta(&host, r::modified(&host)-1000, 9)).unwrap();
                meta["scm"] = json!(meta["scm"].as_i64().unwrap()+1);
                vec![r::Step::new(Method::Meta, serde_json::to_vec(&meta).unwrap())]
            };
            let events = host.engine().stream().subscribe();
            let script = r::Script::new(steps);
            let task = r::started(host.start_meguri_collection(r::access(&connection, &script)));
            loop {
                let event = events.recv_timeout(Duration::from_secs(10)).expect("original JobFinished");
                if matches!(event.kind, meguri_engine::EventKind::JobFinished { id, .. } if id == task.handle().id) { break; }
            }
            task.wait(); script.assert_done(); // No result polling before this real completion.
            assert!(host.engine().jobs().running().is_empty());
            pause(&host);
            let handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
            let intended = host.provider_authority().snapshot().unwrap();
            let intended_bytes = provider_bytes(&config);
            assert!(intended.handoff.is_some()); assert_eq!(intended.meguri_binding.as_ref(), Some(&original));
            assert!(host.finish_provider_handoff(&handoff).is_err()); // Joined worker still has local finalization.
            assert!(matches!(foreign.meguri_round_result(&task), Err(MeguriRoundRefusal::BindingChanged)));
            if failed_write { faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other)); }
            let polled = host.meguri_round_result(&task);
            faultpoint::disarm("storage.sidecar.before_save");
            let polled = polled.unwrap().unwrap();
            let terminal = polled.collection.as_ref().unwrap();
            assert_eq!(terminal.state, MeguriCollectionState::FullSyncRequired);
            let expected_error = ErrorKind::SanityCheckFailed { client: None, server: None };
            assert_eq!(terminal.error.as_ref().map(|error| &error.kind), terminal_error.then_some(&expected_error));
            assert!(!terminal.acceptance_unknown); assert!(terminal.refusal.is_none());
            if failed_write {
                assert_eq!(polled.recovery, Some(meguri_host::MeguriRoundRecovery::Storage));
                assert_eq!(provider_bytes(&config), intended_bytes);
                assert!(host.finish_provider_handoff(&handoff).is_err());
                assert!(Arc::ptr_eq(&polled, &host.meguri_round_result(&task).unwrap().unwrap()));
            } else { assert!(polled.recovery.is_none()); }
            let settled = host.retry_meguri_round_finalization(&task).unwrap().unwrap();
            assert!(settled.recovery.is_none());
            let terminal = settled.collection.as_ref().unwrap();
            assert_eq!(terminal.state, MeguriCollectionState::FullSyncRequired);
            assert_eq!(terminal.error.as_ref().map(|error| &error.kind), terminal_error.then_some(&expected_error));
            assert_eq!(terminal.error.as_ref().and_then(|error| error.method), terminal_error.then_some(Method::SanityCheck2));
            assert_eq!(terminal.error.as_ref().map(|error| error.detail.as_ref()), terminal_error.then_some("merged counts refused"));
            assert_eq!(terminal.refusal, polled.collection.as_ref().unwrap().refusal);
            assert_eq!(terminal.acceptance_unknown, polled.collection.as_ref().unwrap().acceptance_unknown);
            let mut expected = intended.clone();
            expected.meguri_binding = None; expected.meguri_settings.automatic = false; expected.settings.automatic = false;
            assert_eq!(host.provider_authority().snapshot().unwrap(), expected);
            assert_eq!(host.media().unwrap().store().sync_baseline().unwrap(), media_before);
            assert_eq!(host.media().unwrap().store().entries().unwrap(), entries_before);
            assert!(host.collection().unwrap().reader(ReaderRole::Query).unwrap().config_value("routine-generated").unwrap().is_some());
            assert_eq!(provider_bytes(&foreign_config), foreign_bytes);
            host.finish_provider_handoff(&handoff).unwrap(); // Requires the actual round registry to be empty.
            let finished = host.provider_authority().snapshot().unwrap();
            assert_eq!(finished.selection, Selection::LocalOnly); assert!(finished.handoff.is_none());
            assert!(finished.meguri_binding.is_none()); assert!(!finished.meguri_settings.automatic);
            assert!(finished.meguri_settings.paused); assert_eq!(finished.icloud_settings, intended.icloud_settings);
            assert_eq!(script.bodies(Method::Meta).len(), 1); assert!(script.bodies(Method::Finish).is_empty());
            assert!(script.bodies(Method::Upload).is_empty()); assert!(script.bodies(Method::Download).is_empty());
            script.assert_done(); assert!(host.engine().jobs().running().is_empty());
            assert_eq!(provider_bytes(&foreign_config), foreign_bytes);
            foreign.close().unwrap(); host.close().unwrap();
        }
    }
}

static UPLOAD_DRAIN_WAIT: std::sync::Mutex<Option<(std::sync::mpsc::SyncSender<()>, std::sync::mpsc::Receiver<()>)>> = std::sync::Mutex::new(None);
fn hold_upload_drain() {
    if let Some((entered, resume)) = UPLOAD_DRAIN_WAIT.lock().unwrap().take() {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).expect("release held upload drain");
    }
}
struct EmptyUploadAnswer;
impl meguri_sync::transport::RawBody for EmptyUploadAnswer {
    fn next(&mut self) -> meguri_sync::transport::BoxFuture<'_, Result<Option<bytes::Bytes>, meguri_sync::SyncError>> {
        Box::pin(async { Ok(None) })
    }
}
struct HeldUploadAnswer {
    begin: r::Transport,
    held: std::sync::mpsc::SyncSender<meguri_sync::transport::TransportRequest>,
    cancel_first: Option<Arc<CoreHost>>,
}
impl meguri_sync::SyncTransport for HeldUploadAnswer {
    fn post(&self, request: meguri_sync::transport::TransportRequest)
        -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
        Box::pin(async move {
            if !request.url.ends_with("/uploadChanges") { return self.begin.post(request).await; }
            // A real outgoing body remains live after the real 403 answer.
            // Its storage receipt cannot drain until the test drops this owner.
            self.held.send(request).unwrap();
            if let Some(host) = &self.cancel_first {
                let running = host.engine().jobs().running();
                running.iter().find(|job| job.name == meguri_engine::JobKind::MediaSync.name()).unwrap().cancel.cancel();
            }
            Ok(meguri_sync::transport::TransportResponse { status: 403, headers: vec![], body: Box::new(EmptyUploadAnswer) })
        })
    }
}
#[test]
fn genuine_upload_answer_error_precedes_cancel_during_actual_held_drain() {
    use meguri_model::faultpoint::{self, Fault};
    let _lane = b::LANE.lock().unwrap();
    for cancel_first in [false, true] {
        let root = tempfile::tempdir().unwrap(); let (host, config, connection) = ready(root.path());
        let provider = provider_bytes(&config);
        let service = host.media().unwrap(); service.add("pending.txt", b"generated held-upload bytes").unwrap();
        let baseline = service.store().sync_baseline().unwrap();
        let entry = service.store().sync_entry("pending.txt").unwrap().unwrap();
        let begin = r::Script::new(vec![r::begin(0)]);
        let (hold, held) = std::sync::mpsc::sync_channel(1);
        let (entered, reached) = std::sync::mpsc::sync_channel(1);
        let (release, resume) = std::sync::mpsc::sync_channel(1);
        *UPLOAD_DRAIN_WAIT.lock().unwrap() = Some((entered, resume));
        faultpoint::arm("media.sync.upload.drain_pending", Fault::Callback(hold_upload_drain));
        let access = meguri_host::MeguriAccess::new(connection.clone(), b::USER, meguri_sync::HostKey::new("synthetic-key")).unwrap()
            .with_transport_for_testing(Box::new(HeldUploadAnswer { begin: r::Transport(begin.clone()), held: hold,
                cancel_first: cancel_first.then(|| host.clone()) }));
        let task = r::started(host.start_meguri_media(access));
        let body = held.recv_timeout(Duration::from_secs(10));
        // This callback runs only AFTER receipt.drain() actually polls Pending,
        // which is after the typed answer and cancellation-order sample.
        let pending = reached.recv_timeout(Duration::from_secs(10));
        let running = host.engine().jobs().running().iter().any(|job| job.id == task.handle().id);
        let unpolled = host.meguri_round_result(&task).unwrap();
        let held_lease = service.try_job(MediaJobKind::Sync).is_err();
        task.cancel(false);
        let received_body = body.is_ok(); drop(body);
        let released = release.send(());
        task.wait();
        faultpoint::disarm("media.sync.upload.drain_pending");
        *UPLOAD_DRAIN_WAIT.lock().unwrap() = None;
        pending.unwrap(); released.unwrap(); assert!(received_body); assert!(running); assert!(unpolled.is_none()); assert!(held_lease);
        let result = host.meguri_round_result(&task).unwrap().unwrap();
        assert!(result.recovery.is_none()); let media = result.media.as_ref().unwrap();
        assert!(!media.complete); let failure = media.failure.as_ref().unwrap();
        assert_eq!(failure.error.kind, if cancel_first { ErrorKind::Cancelled } else { ErrorKind::AuthFailed });
        assert_eq!(failure.error.method, if cancel_first { None } else { Some(Method::UploadChanges) });
        assert_eq!(media.effects, failure.snapshot.totals);
        assert_eq!(media.effects.uploaded_files, 0); assert_eq!(media.effects.uploaded_deletions, 0); assert_eq!(media.effects.bytes_uploaded, 0);
        assert_eq!(service.store().sync_baseline().unwrap(), baseline);
        let pending_entry = service.store().sync_entry("pending.txt").unwrap().unwrap();
        assert!(pending_entry.dirty); assert_eq!(pending_entry.sha1, entry.sha1); assert_eq!(pending_entry.change_counter, entry.change_counter);
        assert_eq!(provider_bytes(&config), provider);
        assert!(host.engine().jobs().running().is_empty());
        assert_eq!(fs::read_dir(service.store().staging_dir()).unwrap().count(), 0);
        drop(service.try_job(MediaJobKind::Sync).unwrap());
        assert!(Arc::ptr_eq(&result, &host.retry_meguri_round_finalization(&task).unwrap().unwrap()));
        begin.assert_done(); host.close().unwrap();
    }
}
#[test]
fn media_pause_drains_and_does_not_install_received_files_or_report_complete() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path()); let bytes = b"generated incoming bytes";
    // SHA-1 of synthetic bytes is obtained from an independent generated store.
    let data_root = tempfile::tempdir().unwrap();
    let store = meguri_storage::media::MediaStore::open(&data_root.path().join("collection.anki2"),
        Arc::new(FixedClock::new(b::NOW, UtcOffset::UTC)), Arc::new(meguri_model::rng::SeededRandom::new(99))).unwrap();
    store.add("incoming.txt", bytes).unwrap(); let hash = store.sync_entry("incoming.txt").unwrap().unwrap().sha1.unwrap();
    let mut steps = receive_steps("incoming.txt", bytes, &hash); let owner = host.clone();
    steps[3].after = Some(Box::new(move || pause(&owner))); steps.truncate(4);
    let script = r::Script::new(steps);
    let result = r::complete(&host, &r::started(host.start_meguri_media(r::access(&connection, &script)))); script.assert_done();
    let media = result.media.as_ref().unwrap(); assert!(!media.complete); assert!(media.failure.is_some());
    assert_eq!(media.refusal, Some(MeguriRoundRefusal::Paused)); assert!(media.effects.downloaded_files > 0);
    assert!(!host.media().unwrap().store().media_dir().join("incoming.txt").exists());
    drop(host.media().unwrap().try_job(MediaJobKind::Sync).unwrap()); host.close().unwrap();
}

#[test]
fn failed_epoch_publication_retains_terminal_and_retries_no_network() {
    use meguri_model::faultpoint::{self, Fault};
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, config, connection) = ready(root.path()); let before = provider_bytes(&config);
    host.media().unwrap().add("routine.txt", b"generated bytes").unwrap();
    let hash = host.media().unwrap().store().sync_entry("routine.txt").unwrap().unwrap().sha1.unwrap();
    let mut steps = upload_steps(); *steps.last_mut().unwrap() = r::sanity(false);
    steps.extend([r::begin(1), r::changes(json!([["routine.txt", 1, hash]])), r::changes(json!([])), r::sanity(false)]);
    let script = r::Script::new(steps); let task = r::started(host.start_meguri_media(r::access(&connection, &script)));
    task.wait(); script.assert_done();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
    let result = host.meguri_round_result(&task);
    faultpoint::disarm("storage.sidecar.before_save");
    let result = result.unwrap().unwrap();
    assert!(result.recovery.is_some()); assert!(!result.media.as_ref().unwrap().complete);
    assert_eq!(result.media.as_ref().unwrap().failure.as_ref().unwrap().error.kind, ErrorKind::MediaResyncFailed);
    assert_eq!(provider_bytes(&config), before);
    assert!(host.media().unwrap().try_job(MediaJobKind::Sync).is_err());
    assert!(Arc::ptr_eq(&result, &host.meguri_round_result(&task).unwrap().unwrap()));
    let calls = script.bodies(Method::MediaBegin).len();
    let retried = host.retry_meguri_round_finalization(&task).unwrap().unwrap();
    assert!(retried.recovery.is_none()); assert!(!retried.media.as_ref().unwrap().complete);
    assert_eq!(retried.media.as_ref().unwrap().failure.as_ref().unwrap().error.kind, ErrorKind::MediaResyncFailed);
    assert_eq!(script.bodies(Method::MediaBegin).len(), calls);
    bound(&host); drop(host.media().unwrap().try_job(MediaJobKind::Sync).unwrap()); host.close().unwrap();
}
#[test]
fn foreign_same_owner_reset_after_join_cannot_advance_provider_epoch() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, config, connection) = ready(root.path()); let before = provider_bytes(&config);
    host.media().unwrap().add("routine.txt", b"generated bytes").unwrap();
    let hash = host.media().unwrap().store().sync_entry("routine.txt").unwrap().unwrap().sha1.unwrap();
    let mut steps = upload_steps(); *steps.last_mut().unwrap() = r::sanity(false);
    steps.extend([r::begin(1), r::changes(json!([["routine.txt", 1, hash]])), r::changes(json!([])), r::sanity(true)]);
    let script = r::Script::new(steps); let task = r::started(host.start_meguri_media(r::access(&connection, &script)));
    task.wait(); script.assert_done();
    host.media().unwrap().store().reset_sync().unwrap(); // Violates the old job's captured epoch, despite the same owner.
    let result = host.meguri_round_result(&task).unwrap().unwrap();
    assert_eq!(result.recovery, Some(meguri_host::MeguriRoundRecovery::BindingChanged));
    assert!(!result.media.as_ref().unwrap().complete); assert_eq!(provider_bytes(&config), before);
    assert!(host.provider_authority().network_blocked()); host.close().unwrap();
}
#[test]
fn handoff_cancels_only_actual_registered_round_and_waits_for_result_owner() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path());
    let (entered, reached) = std::sync::mpsc::sync_channel(1); let (release, resume) = std::sync::mpsc::sync_channel::<()>(1);
    let script = r::Script::new(vec![r::begin(0).after(move || { entered.send(()).unwrap(); let _ = resume.recv(); })]);
    let task = r::started(host.start_meguri_media(r::access(&connection, &script)));
    let ready = reached.recv_timeout(Duration::from_secs(10));
    let handoff = host.begin_provider_handoff(Selection::LocalOnly);
    let early = handoff.as_ref().map(|handoff| host.finish_provider_handoff(handoff));
    // Release and join original workers before any assertion can panic.
    drop(release); task.wait();
    ready.unwrap(); assert!(early.unwrap().is_err()); let handoff = handoff.unwrap();
    let result = host.meguri_round_result(&task).unwrap().unwrap();
    assert!(!result.media.as_ref().unwrap().complete);
    assert!(task.handle().cancel.is_cancelled());
    host.finish_provider_handoff(&handoff).unwrap(); host.close().unwrap();
}
#[test]
fn fresh_promised_inventory_reports_external_empty_and_oversized_files() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path()); let service = host.media().unwrap();
    fs::write(service.store().media_dir().join("external-empty.bin"), b"").unwrap();
    let large = fs::File::create(service.store().media_dir().join("external-large.bin")).unwrap();
    large.set_len(104_857_601).unwrap(); drop(large);
    let script = r::Script::new(vec![r::begin(0)]);
    let result = r::complete(&host, &r::started(host.start_meguri_media(r::access(&connection, &script)))); script.assert_done();
    let media = result.media.as_ref().unwrap(); assert!(!media.complete); assert!(media.failure.is_none());
    let accounting = media.accounting.as_ref().unwrap(); assert_eq!(accounting.excluded, 2);
    assert!(accounting.affected.iter().any(|a| a.name == "external-empty.bin" && a.kind == meguri_sync::media::AffectedMediaKind::Empty));
    assert!(accounting.affected.iter().any(|a| a.name == "external-large.bin" && a.kind == meguri_sync::media::AffectedMediaKind::OverLimit));
    assert!(service.store().media_dir().join("external-empty.bin").exists());
    assert_eq!(fs::metadata(service.store().media_dir().join("external-large.bin")).unwrap().len(), 104_857_601);
    host.close().unwrap();
}

#[test]
fn admitted_merge_rechecks_after_enqueue_at_the_writers_actual_mutation_boundary() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path()); r::pending(&host);
    let collection = host.collection().unwrap(); let original_col = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let (pending_sent, pending_received) = std::sync::mpsc::sync_channel(1);
    let (entered, waiting) = std::sync::mpsc::sync_channel(1); let (release, resume) = std::sync::mpsc::sync_channel::<()>(1);
    let mut steps = r::normal(&host, json!({"done": true}), r::empty_changes(), 9);
    let original = collection.clone();
    steps.last_mut().unwrap().after = Some(Box::new(move || {
        let pending = original.submit(OpSpec::named("hold writer for admitted merge", OpSource::Rollover), move |_| {
            let _ = entered.send(()); let _ = resume.recv(); Ok(())
        });
        pending_sent.send(pending).unwrap();
    }));
    steps.push(r::Step::new(Method::Abort, b"null".to_vec()));
    let script = r::Script::new(steps); let task = r::started(host.start_meguri_collection(r::access(&connection, &script)));
    let pending = pending_received.recv_timeout(Duration::from_secs(10));
    let entered = waiting.recv_timeout(Duration::from_secs(10));
    let queued = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| collection.wait_queued_op_for_testing("sync_incoming_merge")));
    let paused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pause(&host)));
    drop(release);
    if let Ok(pending) = pending { pending.wait().unwrap(); }
    task.wait(); script.assert_done(); entered.unwrap(); queued.unwrap(); paused.unwrap();
    let result = host.meguri_round_result(&task).unwrap().unwrap(); let terminal = result.collection.as_ref().unwrap();
    assert_eq!(terminal.state, MeguriCollectionState::Failed); assert!(terminal.acceptance_unknown);
    assert_eq!(terminal.refusal, Some(MeguriRoundRefusal::Paused));
    assert_eq!(collection.reader(ReaderRole::Query).unwrap().col().unwrap(), original_col);
    assert!(collection.reader(ReaderRole::Query).unwrap().config_value("routine-generated").unwrap().is_some());
    assert_eq!(collection.peek_undo(meguri_engine::UndoScope::Global).unwrap().reason(), Some(meguri_engine::DisabledReason::AlreadyUploaded));
    host.close().unwrap();
}
#[test]
fn finish_lost_after_pause_keeps_real_acceptance_unknown_and_uploaded_undo_guard() {
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path()); r::pending(&host);
    let mut steps = r::normal(&host, json!({"done": true}), r::empty_changes(), 9);
    let owner = host.clone(); let finish = steps.last_mut().unwrap(); finish.lost = true;
    finish.after = Some(Box::new(move || pause(&owner))); steps.push(r::Step::new(Method::Abort, b"null".to_vec()));
    let script = r::Script::new(steps); let task = r::started(host.start_meguri_collection(r::access(&connection, &script)));
    let result = r::complete(&host, &task); script.assert_done(); let terminal = result.collection.as_ref().unwrap();
    assert_eq!(terminal.state, MeguriCollectionState::Failed); assert!(terminal.acceptance_unknown);
    assert_eq!(terminal.error.as_ref().unwrap().kind, ErrorKind::Interrupted); assert_eq!(script.bodies(Method::Finish).len(), 1);
    assert_eq!(host.collection().unwrap().peek_undo(meguri_engine::UndoScope::Global).unwrap().reason(), Some(meguri_engine::DisabledReason::AlreadyUploaded));
    host.close().unwrap();
}

static LATE_INSTALL_HOST: std::sync::Mutex<Option<std::sync::Weak<CoreHost>>> = std::sync::Mutex::new(None);
fn pause_after_download_hash() {
    let host = LATE_INSTALL_HOST.lock().unwrap().take().and_then(|host| host.upgrade()).unwrap();
    pause(&host);
}
#[test]
fn pause_after_download_hash_is_rechecked_inside_storage_before_install() {
    use meguri_model::faultpoint::{self, Fault};
    let _lane = b::LANE.lock().unwrap(); let root = tempfile::tempdir().unwrap();
    let (host, _, connection) = ready(root.path());
    let source_root = tempfile::tempdir().unwrap();
    let store = meguri_storage::media::MediaStore::open(&source_root.path().join("collection.anki2"),
        Arc::new(FixedClock::new(b::NOW, UtcOffset::UTC)), Arc::new(meguri_model::rng::SeededRandom::new(53))).unwrap();
    let bytes = b"generated late-install asset"; store.add("late.txt", bytes).unwrap();
    let hash = store.sync_entry("late.txt").unwrap().unwrap().sha1.unwrap();
    let mut steps = receive_steps("late.txt", bytes, &hash); steps.truncate(4);
    let script = r::Script::new(steps);
    *LATE_INSTALL_HOST.lock().unwrap() = Some(Arc::downgrade(&host));
    faultpoint::arm("media.download.after_hash", Fault::Callback(pause_after_download_hash));
    let task = r::started(host.start_meguri_media(r::access(&connection, &script))); task.wait();
    faultpoint::disarm("media.download.after_hash");
    let result = host.meguri_round_result(&task).unwrap().unwrap(); script.assert_done();
    assert!(!result.media.as_ref().unwrap().complete);
    assert_eq!(result.media.as_ref().unwrap().refusal, Some(MeguriRoundRefusal::Paused));
    assert!(!host.media().unwrap().store().media_dir().join("late.txt").exists());
    assert_eq!(result.media.as_ref().unwrap().effects.downloaded_files, 1);
    drop(host.media().unwrap().try_job(MediaJobKind::Sync).unwrap()); host.close().unwrap();
}
