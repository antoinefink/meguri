//! U107 production JobRegistry executor regressions. Neutral N14/H18 contracts;
//! every normal round uses normal::start, the real host and SyncTransport.
mod common;
use meguri_engine::{ClearCause, DisabledReason, OpSource, OpSpec, UndoScope};
use meguri_model::{
    clock::Clock,
    grave::GraveKind,
    ids::*,
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_storage::{ReadRows, ReaderRole, RowKey, StampMode, Table};
use meguri_sync::{ErrorKind, normal};
use meguri_testkit::{
    normal_support::{Fixture, NOW, default_reply},
    sync_stub::{Reply, SyncStub},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
fn schema(f: &Fixture) -> i64 {
    f.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .col()
        .unwrap()
        .schema_modified
        .0
}
fn json_body(r: &meguri_testkit::sync_stub::Recorded) -> Value {
    serde_json::from_slice(r.body.as_ref().unwrap()).unwrap()
}
fn methods(stub: &SyncStub) -> Vec<String> {
    stub.requests()
        .iter()
        .map(|r| r.path().rsplit('/').next().unwrap().to_string())
        .collect()
}
fn baseline(f: &Fixture) {
    let stub = f.stub();
    f.start(&stub.url()).wait().unwrap();
}
fn card(f: &Fixture, id: CardId) -> meguri_model::card::Card {
    f.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(id)
        .unwrap()
        .unwrap()
}
fn incoming_card(row: &meguri_model::card::Card) -> String {
    let body = meguri_sync::wire::chunk::encode_apply_chunk(
        &meguri_sync::wire::chunk::OutgoingChunk {
            done: true,
            cards: std::slice::from_ref(row),
            notes: &[],
            revlog: &[],
        },
        Usn(5),
    );
    json_body_bytes(&body)["chunk"].to_string()
}
fn json_body_bytes(body: &[u8]) -> Value {
    serde_json::from_slice(body).unwrap()
}
#[test]
fn normal_request_order_and_closing_stamps_use_real_job() {
    let f = Fixture::new(2);
    let stub = f.stub();
    let out = f.start(&stub.url()).wait().unwrap();
    assert!(!out.merge.unwrap().one_way);
    assert_eq!(
        methods(&stub),
        [
            "meta",
            "start",
            "applyChanges",
            "chunk",
            "applyChunk",
            "sanityCheck2",
            "finish"
        ]
    );
    let col = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_eq!((col.modified.0, col.last_sync.0, col.usn.0), (NOW, NOW, 6));
    assert!(!f.collection.path().with_file_name("meguri-sync-stage.db").exists());
    for r in stub.requests() {
        assert_eq!(r.sync_member("s"), stub.requests()[0].sync_member("s"));
    }
}
#[test]
fn graves_251_are_250_plus_1_cards_then_notes_then_decks() {
    let f = Fixture::new(0);
    f.collection
        .run(OpSpec::named("graves fixture", OpSource::UserUndoable), |ctx| {
            for i in 0..251 {
                ctx.scope().add_grave(
                    i,
                    if i < 249 {
                        GraveKind::Card
                    } else if i == 249 {
                        GraveKind::Note
                    } else {
                        GraveKind::Deck
                    },
                    Usn::PENDING,
                )?;
            }
            Ok(())
        })
        .unwrap();
    let stub = f.stub();
    f.start(&stub.url()).wait().unwrap();
    let bodies = stub
        .requests()
        .into_iter()
        .filter(|r| r.path().ends_with("applyGraves"))
        .map(|r| json_body(&r)["chunk"].clone())
        .collect::<Vec<_>>();
    assert_eq!(bodies.len(), 2);
    assert_eq!(bodies[0]["cards"].as_array().unwrap().len(), 249);
    assert_eq!(bodies[0]["notes"], json!([249]));
    assert_eq!(bodies[1]["decks"], json!([250]));
}
#[test]
fn outgoing_602_objects_are_bounded_and_done_only_on_final() {
    let f = Fixture::new(301);
    let stub = f.stub();
    f.start(&stub.url()).wait().unwrap();
    let chunks = stub
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(|r| json_body(r)["chunk"].clone())
        .collect::<Vec<_>>();
    let sizes = chunks
        .iter()
        .map(|c| {
            ["revlog", "notes", "cards"]
                .iter()
                .map(|k| c[*k].as_array().map_or(0, Vec::len))
                .sum::<usize>()
        })
        .collect::<Vec<_>>();
    assert_eq!(sizes, vec![250, 250, 102]);
    assert_eq!(
        chunks.iter().map(|c| c["done"].as_bool().unwrap()).collect::<Vec<_>>(),
        vec![false, false, true]
    );
}
#[test]
fn exact_250_objects_require_final_empty_chunk() {
    let f = Fixture::new(125);
    let stub = f.stub();
    f.start(&stub.url()).wait().unwrap();
    let chunks = stub
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(|r| json_body(r)["chunk"].clone())
        .collect::<Vec<_>>();
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0]["done"], false);
    assert_eq!(chunks[1], json!({"done":true}));
}
#[test]
fn corrupt_lifecycle_sends_no_meta() {
    let f = Fixture::new(1);
    f.host.engine().mark_corrupt("synthetic check failure");
    let stub = f.stub();
    assert_eq!(
        f.start(&stub.url()).wait().unwrap_err().kind,
        ErrorKind::CollectionCorrupt
    );
    assert!(stub.requests().is_empty());
}
#[test]
fn every_stage_failure_aborts_and_preserves_rows_and_undo() {
    for fail in [
        "start",
        "applyGraves",
        "applyChanges",
        "chunk",
        "applyChunk",
        "sanityCheck2",
    ] {
        let f = Fixture::new(1);
        f.collection
            .run(OpSpec::named("pending grave", OpSource::UserUndoable), |c| {
                c.scope().add_grave(88, GraveKind::Card, Usn::PENDING)?;
                Ok(())
            })
            .unwrap();
        let before = f.images();
        let scm = schema(&f);
        let failed = fail.to_string();
        let (abort, aborted) = mpsc::sync_channel(1);
        let stub = SyncStub::serve(move |r| {
            let method = r.path().rsplit('/').next().unwrap();
            if method == "abort" {
                let _ = abort.send(());
            }
            if method == failed {
                Reply::status(400)
            } else {
                default_reply(r.path(), scm, NOW)
            }
        });
        let error = f.start(&stub.url()).wait().unwrap_err();
        assert!(matches!(error.kind, ErrorKind::DbCheckRequired { .. }));
        aborted.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(f.images(), before);
        assert!(!methods(&stub).contains(&"finish".into()));
    }
}
#[test]
fn malformed_replies_never_bypass_strict_wire_readers() {
    for (method, body) in [
        ("start", r#"{"cards":[],"cards":[],"notes":[],"decks":[]}"#),
        ("applyChanges", r#"{"models":[],"decks":[[]],"tags":[]}"#),
        ("chunk", r#"{"done":true,"cards":[[1]]}"#),
        ("sanityCheck2", r#"{"status":"unknown"}"#),
    ] {
        let f = Fixture::new(1);
        let before = f.images();
        let scm = schema(&f);
        let stub = SyncStub::serve(move |r| {
            if r.path().ends_with(method) {
                Reply::ok(body.as_bytes())
            } else {
                default_reply(r.path(), scm, NOW)
            }
        });
        assert_eq!(f.start(&stub.url()).wait().unwrap_err().kind, ErrorKind::BadResponse);
        assert_eq!(before, f.images());
        assert!(!methods(&stub).contains(&"finish".into()));
    }
}
#[test]
fn sanity_bad_keeps_counts_and_moves_only_scm() {
    let f = Fixture::new(1);
    baseline(&f);
    f.clock.step_ms(60000);
    let before = f.images();
    let scm = schema(&f);
    let (abort, aborted) = mpsc::sync_channel(1);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("abort") {
            let _ = abort.send(());
        }
        if r.path().ends_with("sanityCheck2") {
            Reply::ok(br#"{"status":"bad","c":[[0,0,0],1,1,0,0,8,1,1],"s":[[0,0,0],2,2,0,0,8,1,1]}"#)
        } else {
            // Keep server mod unequal to the baseline local mod. Advancing
            // this reply's clock by 60 s would make meta.mod equal NOW.
            default_reply(r.path(), scm, NOW)
        }
    });
    let error = f.start(&stub.url()).wait().unwrap_err();
    aborted.recv_timeout(Duration::from_secs(2)).unwrap();
    let client = meguri_sync::wire::sanity::SanityCounts {
        due: [0, 0, 0],
        cards: 1,
        notes: 1,
        revlog: 0,
        graves: 0,
        notetypes: 8,
        decks: 1,
        deck_configs: 1,
    };
    assert_eq!(
        error.kind,
        ErrorKind::SanityCheckFailed {
            client: Some(client),
            server: Some(meguri_sync::wire::sanity::SanityCounts {
                cards: 2,
                notes: 2,
                ..client
            }),
        }
    );
    let after = f.images();
    for ((table, a), (_, b)) in before.iter().zip(&after) {
        if *table != Table::Col {
            assert_eq!(a, b);
        } else {
            for i in 0..a.len() {
                if i != 3 {
                    assert_eq!(a[i], b[i]);
                }
            }
        }
    }
    let col = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert!(col.schema_modified > col.last_sync);
    let requests = methods(&stub);
    assert!(requests.contains(&"sanityCheck2".into()));
    assert!(requests.contains(&"abort".into()));
    assert!(!requests.contains(&"finish".into()));
}
#[test]
fn accepted_notetype_count_change_refused_before_finish() {
    let f = Fixture::new(1);
    let mut nt = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(f.notetype)
        .unwrap()
        .unwrap();
    nt.fields
        .push(meguri_engine::notetypes::new_field("extra", f.random.as_ref()));
    nt.fields[2].ord = 2;
    nt.mtime = EpochSecs(NOW / 1000 + 1);
    let body = format!(
        r#"{{"models":[{}],"decks":[[],[]],"tags":[]}}"#,
        meguri_model::schema11::notetype_to_json(&nt)
    );
    let scm = schema(&f);
    let before = f.images();
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("applyChanges") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    assert_eq!(
        f.start(&stub.url()).wait().unwrap_err().kind,
        ErrorKind::NotetypeSchemaChanged {
            notetype_id: f.notetype
        }
    );
    assert_eq!(f.images(), before);
    assert!(!methods(&stub).contains(&"finish".into()));
}
#[test]
fn incoming_note_notetype_and_field_count_validated_before_finish() {
    for missing in [false, true] {
        let f = Fixture::new(0);
        let mid = if missing { 123 } else { f.notetype.0 };
        let scm = schema(&f);
        let before = f.images();
        let body =
            json!({"done":true,"notes":[[1,"synthetic",mid,NOW/1000,5,"","only-one-field","","",0,""]]}).to_string();
        let stub = SyncStub::serve(move |r| {
            if r.path().ends_with("chunk") {
                Reply::ok(body.as_bytes())
            } else {
                default_reply(r.path(), scm, NOW)
            }
        });
        let kind = f.start(&stub.url()).wait().unwrap_err().kind;
        assert_eq!(
            kind,
            if missing {
                ErrorKind::MissingNotetype {
                    notetype_id: NotetypeId(mid),
                    note_id: NoteId(1),
                }
            } else {
                ErrorKind::NoteFieldsMismatch {
                    notetype_id: NotetypeId(mid),
                    note_id: NoteId(1),
                }
            }
        );
        assert_eq!(f.images(), before);
    }
}
#[test]
fn card_pending_ties_stay_local_nonpending_newer_is_replaced() {
    for pending in [false, true] {
        for delta in [-1, 0, 1] {
            let f = Fixture::new(1);
            baseline(&f);
            let id = f.cards[0];
            let mut remote = card(&f, id);
            remote.flags = 3;
            remote.mtime = EpochSecs(remote.mtime.0 + delta);
            if pending {
                f.collection
                    .run(
                        OpSpec::named("local pending flag", OpSource::UserUndoable),
                        move |ctx| {
                            let mut c = ctx.scope().card(id)?.unwrap();
                            c.flags = 2;
                            ctx.scope().put(&c)?;
                            Ok(())
                        },
                    )
                    .unwrap();
            }
            let body = incoming_card(&remote);
            let scm = schema(&f);
            let stub = SyncStub::serve(move |r| {
                if r.path().ends_with("/chunk") {
                    Reply::ok(body.as_bytes())
                } else {
                    default_reply(r.path(), scm, NOW)
                }
            });
            f.start(&stub.url()).wait().unwrap();
            assert_eq!(card(&f, id).flags, if pending && delta <= 0 { 2 } else { 3 });
        }
    }
}
#[test]
fn same_second_post_snapshot_flag_and_concurrent_note_edit_remain_pending() {
    let f = Fixture::new(1);
    baseline(&f);
    let id = f.cards[0];
    f.answer(id, None);
    let original = card(&f, id);
    let collection = f.collection.clone();
    let note = original.note_id;
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            collection
                .run(OpSpec::named("same second flag", OpSource::UserUndoable), move |ctx| {
                    let mut row = ctx.scope().card(id)?.unwrap();
                    row.flags = 4;
                    ctx.scope().put(&row)?;
                    let mut n = ctx.scope().note(note)?.unwrap();
                    n.fields = "edited during network\u{1f}back".into();
                    ctx.scope().put(&n)?;
                    Ok(())
                })
                .unwrap();
        }
        default_reply(r.path(), scm, NOW)
    });
    f.start(&stub.url()).wait().unwrap();
    let current = card(&f, id);
    assert_eq!(current.mtime, original.mtime);
    assert_eq!((current.flags, current.usn), (4, Usn::PENDING));
    assert_eq!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(note)
            .unwrap()
            .unwrap()
            .usn,
        Usn::PENDING
    );
    let sent = stub
        .requests()
        .into_iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(|r| json_body(&r))
        .collect::<Vec<_>>();
    assert!(
        sent.iter()
            .flat_map(|b| b["chunk"]["cards"].as_array().into_iter().flatten())
            .all(|c| c[16] != 4)
    );
    let next = f.stub();
    f.start(&next.url()).wait().unwrap();
    assert_eq!(card(&f, id).usn, Usn(5));
}
#[test]
fn own_id_grave_skip_stores_card_with_missing_note_without_recreating_note() {
    let f = Fixture::new(1);
    baseline(&f);
    let id = f.cards[0];
    let mut remote = card(&f, id);
    let note = remote.note_id;
    remote.id = CardId(id.0 + 9999);
    remote.mtime = EpochSecs(NOW / 1000 + 9);
    let body = incoming_card(&remote);
    let incoming_id = remote.id;
    let collection = f.collection.clone();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            meguri_engine::notes::delete(&collection, vec![note], OpSource::UserUndoable).unwrap();
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    f.start(&stub.url()).wait().unwrap();
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    assert!(rows.note(note).unwrap().is_none());
    assert!(rows.card(id).unwrap().is_none());
    assert_eq!(rows.card(incoming_id).unwrap().unwrap().note_id, note);
    assert_eq!(
        rows.row_image(Table::Graves, &RowKey::Pair(note.0, 1))
            .unwrap()
            .unwrap()[2]
            .as_i64(),
        Some(-1)
    );
}
#[test]
fn live_scm_mutation_prevents_every_merge_mark_and_finalize() {
    let f = Fixture::new(1);
    baseline(&f);
    let before = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let old = schema(&f);
    let collection = f.collection.clone();
    let mut remote = card(&f, f.cards[0]);
    remote.flags = 6;
    let body = incoming_card(&remote);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            collection
                .run(
                    OpSpec::named("confirmed schema during sync", OpSource::UserUndoable).stamp(StampMode::Raw),
                    move |ctx| {
                        let mut col = ctx.scope().col()?;
                        col.schema_modified = EpochMillis(NOW + 1);
                        ctx.scope().put_col(&col)?;
                        Ok(())
                    },
                )
                .unwrap();
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), old, NOW)
        }
    });
    assert!(f.start(&stub.url()).wait().unwrap().merge.unwrap().one_way);
    let after = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_eq!(
        (after.modified, after.last_sync, after.usn),
        (before.modified, before.last_sync, before.usn)
    );
    assert_ne!(card(&f, f.cards[0]).flags, 6);
}

fn schema_change_during_delayed_meta(structural: bool, count: usize, server_empty: bool) {
    let f = Fixture::new(count);
    baseline(&f);
    let cached_local =
        meguri_sync::status::LocalRecord::read(&*f.collection.reader(ReaderRole::Query).unwrap()).unwrap();
    f.clock.step_ms(1000);
    f.collection
        .run(OpSpec::named("pending before meta", OpSource::UserUndoable), |ctx| {
            ctx.scope().set_config("pendingMeta", &true.into())?;
            Ok(())
        })
        .unwrap();
    let old_schema = schema(&f);
    let server_mod = f.clock.now_ms() + 1000;
    let (entered, at_meta) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/meta") {
            entered.send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
            return Reply::ok(
                format!(
                    r#"{{"mod":{server_mod},"scm":{old_schema},"usn":5,"ts":{},"msg":"","cont":true,"empty":{server_empty},"media_usn":0}}"#,
                    server_mod / 1000
                )
                .as_bytes(),
            );
        }
        default_reply(r.path(), old_schema, NOW)
    });
    let collection = f.collection.clone();
    let (pending, observed) = mpsc::sync_channel(1);
    let _hook = normal::test_hooks::install(Arc::new(move |point| {
        if matches!(point, normal::test_hooks::Point::Snapshot(_)) {
            pending
                .send(collection.peek_undo(UndoScope::Global).unwrap().reason())
                .unwrap();
        }
    }));
    let task = f.start(&stub.url());
    at_meta.recv_timeout(Duration::from_secs(2)).unwrap();
    f.clock.step_ms(1000);
    let notetype = f.notetype;
    let note = f.cards.first().map(|id| card(&f, *id).note_id);
    f.collection
        .run(
            OpSpec::named("schema while meta waits", OpSource::UserUndoable),
            move |ctx| {
                meguri_engine::schema_gate::mark_schema_change(
                    ctx.scope(),
                    &[if structural {
                        meguri_engine::schema_gate::SchemaReason::FieldsChanged
                    } else {
                        meguri_engine::schema_gate::SchemaReason::ForceOneWay
                    }],
                )?;
                if structural {
                    let (scope, random) = ctx.scope_and_random();
                    let mut nt = scope.notetype(notetype)?.unwrap();
                    let mut field = meguri_engine::notetypes::new_field("Extra", random);
                    field.ord = nt.fields.len() as u32;
                    nt.fields.push(field);
                    scope.put_notetype(&nt)?;
                    let mut row = scope.note(note.unwrap())?.unwrap();
                    row.fields.push(meguri_model::note::FIELD_SEPARATOR);
                    scope.put(&row)?;
                }
                Ok(())
            },
        )
        .unwrap();
    let before = f.images();
    let col = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_ne!(col.schema_modified.0, old_schema);
    assert_eq!(col.modified.0, server_mod, "equal mod must not hide the changed schema");
    if structural {
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        let nt = rows.notetype(notetype).unwrap().unwrap();
        assert_eq!(nt.fields.len(), 3);
        assert_eq!(nt.usn, Usn::PENDING);
        assert_eq!(rows.note(note.unwrap()).unwrap().unwrap().field_values().count(), 3);
    }
    release.send(()).unwrap();
    let out = task.wait().unwrap();
    assert_eq!(
        out.check.result,
        meguri_sync::status::CheckResult::OneWay {
            upload: count > 0 || server_empty,
            download: !server_empty || count == 0,
        }
    );
    assert!(out.merge.is_none());
    assert_eq!(methods(&stub), ["meta"]);
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(2)).unwrap(),
        Some(DisabledReason::UploadPending)
    );
    assert_eq!(f.images(), before, "sync must not mutate any row or stamp");
    assert!(!f.collection.path().with_file_name("meguri-sync-stage.db").exists());
    let mut probe = common::job_with_key(&common::options(&stub.url()), "synthetic-normal-key");
    let remembered = common::block_on(
        f.memory
            .lock()
            .unwrap()
            .status(&mut probe, &cached_local, f.clock.as_ref()),
    )
    .unwrap();
    assert_eq!(remembered.status, meguri_sync::SyncStatus::FullSync);
    assert_eq!(methods(&stub), ["meta"], "the status must be remembered");
    f.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(schema(&f), old_schema);
    f.collection.undo(UndoScope::Global).unwrap();
    assert!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value("pendingMeta")
            .unwrap()
            .is_none()
    );
}

#[test]
fn delayed_meta_structural_schema_change_requires_one_way_before_start() {
    schema_change_during_delayed_meta(true, 1, false);
}

#[test]
fn delayed_meta_force_one_way_preserves_direction_capabilities_even_with_equal_mod() {
    for count in [0, 1] {
        for server_empty in [false, true] {
            schema_change_during_delayed_meta(false, count, server_empty);
        }
    }
}

#[test]
fn holdback_latest_answer_and_following_bury_restore_deck_counts_in_v() {
    let f = Fixture::new(3);
    baseline(&f);
    let active = f.host.review().begin_unqueued_session().unwrap();
    let scoped = active.collection().unwrap();
    f.answer(f.cards[0], Some(active.id()));
    f.clock.step_ms(1000);
    f.answer(f.cards[1], Some(active.id()));
    active
        .apply(meguri_engine::sched::ops::Request::Bury(
            meguri_engine::sched::ops::Targets::Cards(vec![f.cards[2]]),
        ))
        .unwrap();
    let stub = f.stub();
    f.start(&stub.url()).wait().unwrap();
    let chunks = stub
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(json_body)
        .collect::<Vec<_>>();
    let reviews = chunks
        .iter()
        .flat_map(|b| b["chunk"]["revlog"].as_array().into_iter().flatten())
        .collect::<Vec<_>>();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0][1], f.cards[0].0);
    assert!(
        scoped
            .peek_undo(UndoScope::Session(active.id()))
            .unwrap()
            .name()
            .is_some()
    );
    assert_eq!(card(&f, f.cards[1]).usn, Usn::PENDING);
}
#[test]
fn finish_drop_marks_pending_uploaded_and_replay_is_safe() {
    let f = Fixture::new(2);
    baseline(&f);
    f.answer(f.cards[0], None);
    let before = f.images();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("finish") {
            Reply::hang_up()
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    assert_eq!(f.start(&stub.url()).wait().unwrap_err().kind, ErrorKind::Interrupted);
    assert_eq!(before, f.images());
    assert_eq!(
        f.collection.peek_undo(UndoScope::Global).unwrap().reason(),
        Some(DisabledReason::AlreadyUploaded)
    );
    let next = f.stub();
    f.start(&next.url()).wait().unwrap();
    assert_eq!(card(&f, f.cards[0]).usn, Usn(5));
}
#[test]
fn captured_run_survives_history_clear_and_pending_before_image_is_not_marked() {
    let f = Fixture::new(1);
    baseline(&f);
    let id = f.cards[0];
    f.collection
        .run(
            OpSpec::named("pending same second", OpSource::UserUndoable),
            move |ctx| {
                let mut c = ctx.scope().card(id)?.unwrap();
                c.flags = 1;
                ctx.scope().put(&c)?;
                Ok(())
            },
        )
        .unwrap();
    let active = f.host.review().begin_unqueued_session().unwrap();
    f.answer(id, Some(active.id()));
    let collection = f.collection.clone();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            collection.clear_undo(ClearCause::Rollover).unwrap();
        }
        default_reply(r.path(), scm, NOW)
    });
    f.start(&stub.url()).wait().unwrap();
    assert_eq!(card(&f, id).usn, Usn::PENDING);
    let sent = stub
        .requests()
        .into_iter()
        .find(|r| r.path().ends_with("applyChunk"))
        .unwrap();
    let b = json_body(&sent);
    assert_eq!(b["chunk"]["cards"][0][16], 1);
    assert!(b["chunk"].get("revlog").is_none());
}

#[test]
fn complete_u29_field_option_upload_reaches_fresh_peer_after_real_baseline() {
    let f = Fixture::new(1);
    baseline(&f);
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    let original_scm = rows.col().unwrap().schema_modified;
    let nt = rows.notetype(f.notetype).unwrap().unwrap();
    drop(rows);
    let mut draft = meguri_engine::notetypes::Draft::of(nt);
    draft.fields[0].config.sticky = true;
    meguri_engine::notetypes::save(&f.collection, draft, Default::default()).unwrap();
    let stub = f.stub();
    assert!(matches!(
        f.start(&stub.url()).wait().unwrap().check.result,
        meguri_sync::CheckResult::Normal { .. }
    ));
    let changes = stub
        .requests()
        .into_iter()
        .find(|r| r.path().ends_with("applyChanges"))
        .map(|r| json_body(&r)["changes"].clone())
        .unwrap();
    let model = changes["models"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == f.notetype.0)
        .unwrap()
        .clone();
    assert_eq!(model["flds"][0]["sticky"], true);
    assert_eq!(model["flds"].as_array().unwrap().len(), 2);
    assert_eq!(model["tmpls"].as_array().unwrap().len(), 1);
    let peer = Fixture::new(0);
    baseline(&peer);
    let scm = schema(&peer);
    let body = json!({"models":[model],"decks":[[],[]],"tags":[]}).to_string();
    let remote = SyncStub::serve(move |r| {
        if r.path().ends_with("applyChanges") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    peer.start(&remote.url()).wait().unwrap();
    assert!(
        peer.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .notetype(f.notetype)
            .unwrap()
            .unwrap()
            .fields[0]
            .config
            .sticky
    );
    assert_eq!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .col()
            .unwrap()
            .schema_modified,
        original_scm
    );
    assert!(methods(&stub).iter().all(|m| m != "upload" && m != "download"));
    assert!(methods(&remote).iter().all(|m| m != "upload" && m != "download"));
}
#[test]
fn all_object_mtimes_tie_to_server_and_names_gain_suffixes() {
    for delta in [-1, 0, 1] {
        let f = Fixture::new(0);
        baseline(&f);
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        let mut nt = rows.notetype(f.notetype).unwrap().unwrap();
        let mut deck = rows.deck(DeckId(1)).unwrap().unwrap();
        let mut preset = rows.deck_config(DeckConfigId(1)).unwrap().unwrap();
        drop(rows);
        nt.name = "Remote type".into();
        nt.mtime = EpochSecs(nt.mtime.0 + delta);
        deck.name = "Remote deck".into();
        deck.mtime = EpochSecs(deck.mtime.0 + delta);
        preset.name = "Remote preset".into();
        preset.mtime = EpochSecs(preset.mtime.0 + delta);
        let body = format!(
            r#"{{"models":[{}],"decks":[[{}],[{}]],"tags":[]}}"#,
            meguri_model::schema11::notetype_to_json(&nt),
            meguri_model::schema11::deck_to_json(&deck),
            meguri_model::schema11::deck_config_to_json(&preset)
        );
        let scm = schema(&f);
        let stub = SyncStub::serve(move |r| {
            if r.path().ends_with("applyChanges") {
                Reply::ok(body.as_bytes())
            } else {
                default_reply(r.path(), scm, NOW)
            }
        });
        f.start(&stub.url()).wait().unwrap();
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            rows.notetype(f.notetype).unwrap().unwrap().name == "Remote type",
            delta >= 0
        );
        assert_eq!(rows.deck(DeckId(1)).unwrap().unwrap().name == "Remote deck", delta >= 0);
        assert_eq!(
            rows.deck_config(DeckConfigId(1)).unwrap().unwrap().name == "Remote preset",
            delta >= 0
        );
    }
    let f = Fixture::new(0);
    let mut deck = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    deck.id = DeckId(111);
    deck.usn = Usn(5);
    deck.name = "default".into();
    let scm = schema(&f);
    let body = format!(
        r#"{{"models":[],"decks":[[{}],[]],"tags":[]}}"#,
        meguri_model::schema11::deck_to_json(&deck)
    );
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("applyChanges") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    f.start(&stub.url()).wait().unwrap();
    assert_eq!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck(DeckId(111))
            .unwrap()
            .unwrap()
            .name,
        "default+"
    );
}
#[test]
fn notes_use_pending_tie_rule_and_strip_controls_without_nfc() {
    for delta in [-1, 0, 1] {
        let f = Fixture::new(1);
        let local = card(&f, f.cards[0]);
        let id = local.note_id;
        let scm = schema(&f);
        let mid = f.notetype.0;
        let body=json!({"done":true,"notes":[[id.0,"remote",mid,NOW/1000+delta,5," remote\u{3000}tag ","e\u{301}\r\n\u{1f}back","","",0,""]]}).to_string();
        let stub = SyncStub::serve(move |r| {
            if r.path().ends_with("/chunk") {
                Reply::ok(body.as_bytes())
            } else {
                default_reply(r.path(), scm, NOW)
            }
        });
        f.start(&stub.url()).wait().unwrap();
        let note = f
            .collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(id)
            .unwrap()
            .unwrap();
        if delta <= 0 {
            assert_eq!(note.field_values().next(), Some("front 0"));
        } else {
            assert_eq!(note.field_values().next(), Some("e\u{301}\n"));
            assert_eq!(note.tags, " remote tag ");
            assert_eq!((note.flags, note.data), (0, String::new()));
        }
    }
}
#[test]
fn review_log_taken_id_is_never_overwritten() {
    let f = Fixture::new(1);
    f.answer(f.cards[0], None);
    let row = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .revlog_after(None, 1)
        .unwrap()[0]
        .clone();
    let mut remote = row.clone();
    remote.button = 1;
    remote.answer_millis = 99;
    let encoded = meguri_sync::wire::chunk::encode_apply_chunk(
        &meguri_sync::wire::chunk::OutgoingChunk {
            done: true,
            revlog: &[remote],
            cards: &[],
            notes: &[],
        },
        Usn(5),
    );
    let body = json_body_bytes(&encoded)["chunk"].to_string();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    f.start(&stub.url()).wait().unwrap();
    let stored = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .revlog_entry(row.id)
        .unwrap()
        .unwrap();
    assert_eq!((stored.button, stored.answer_millis), (row.button, row.answer_millis));
}
#[test]
fn post_snapshot_curdeck_survives_whole_config_replacement() {
    let f = Fixture::new(0);
    let collection = f.collection.clone();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/meta") {
            // The server is newer, so it sends the whole config table.
            return default_reply(r.path(), scm, NOW + 61_000);
        }
        if r.path().ends_with("applyChanges") {
            return Reply::ok(
                br#"{"models":[],"decks":[[],[]],"tags":[],"conf":{"curDeck":1,"schedVer":2,"remote":true}}"#,
            );
        }
        if r.path().ends_with("/chunk") {
            let before = collection
                .run(OpSpec::named("select after snapshot", OpSource::UserUndoable), |ctx| {
                    let before = ctx.scope().col()?.modified;
                    assert_eq!(before.0, ctx.now().0);
                    ctx.scope().set_config("curDeck", &77_i64.into())?;
                    Ok(before)
                })
                .unwrap()
                .value;
            assert_eq!(
                collection.reader(ReaderRole::Query).unwrap().col().unwrap().modified,
                before
            );
        }
        default_reply(r.path(), scm, NOW + 1000)
    });
    f.start(&stub.url()).wait().unwrap();
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(rows.config_value("curDeck").unwrap().unwrap().as_i64(), Some(77));
    assert_eq!(rows.config_entry("curDeck").unwrap().unwrap().entry.usn, Usn::PENDING);
    assert!(rows.col().unwrap().modified > rows.col().unwrap().last_sync);
    assert_eq!(rows.col().unwrap().modified.0, rows.col().unwrap().last_sync.0 + 1);
    assert_eq!(
        meguri_sync::status::LocalRecord::read(&*rows).unwrap().offline_status(),
        meguri_sync::status::OfflineStatus::PendingChanges
    );
    // Received config rows carry positive server USNs, not pending -1.
    assert_eq!(rows.config_entry("remote").unwrap().unwrap().entry.usn, Usn(5));
    let expected_config: serde_json::Map<String, Value> = rows
        .config_keys()
        .unwrap()
        .into_iter()
        .map(|key| {
            let row = rows.config_entry(&key).unwrap().unwrap();
            (key, serde_json::from_slice(&row.entry.value).unwrap())
        })
        .collect();
    drop(rows);

    let next = SyncStub::serve(move |r| {
        if r.path().ends_with("/meta") {
            // Server mod equals the preceding finish time; the pending
            // config-only edit must make the client newer this round.
            default_reply(r.path(), scm, NOW + 61_000)
        } else {
            default_reply(r.path(), scm, NOW + 2000)
        }
    });
    let out = f.start(&next.url()).wait().unwrap();
    assert!(matches!(
        out.check.result,
        meguri_sync::status::CheckResult::Normal { local_is_newer: true }
    ));
    assert!(!out.merge.unwrap().one_way);
    let requests = next.requests();
    let sent = json_body(requests.iter().find(|r| r.path().ends_with("applyChanges")).unwrap());
    assert_eq!(sent["changes"]["conf"], Value::Object(expected_config));
    assert_eq!(sent["changes"]["conf"]["curDeck"], 77);
    assert_eq!(sent["changes"]["conf"]["remote"], true);
    assert_eq!(sent["changes"]["conf"]["schedVer"], 2);
    assert_eq!(sent["changes"]["models"], json!([]));
    assert_eq!(sent["changes"]["decks"], json!([[], []]));
    assert_eq!(sent["changes"]["tags"], json!([]));
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(rows.config_entry("curDeck").unwrap().unwrap().entry.usn, Usn(0));
    assert_eq!(rows.col().unwrap().modified, rows.col().unwrap().last_sync);
    assert_eq!(
        meguri_sync::status::LocalRecord::read(&*rows).unwrap().offline_status(),
        meguri_sync::status::OfflineStatus::NothingPending
    );
    drop(rows);
    let clean = SyncStub::serve(move |r| default_reply(r.path(), scm, NOW + 62_000));
    let out = f.start(&clean.url()).wait().unwrap();
    assert!(matches!(out.check.result, meguri_sync::status::CheckResult::NoChanges));
    assert!(out.merge.is_none());
    assert_eq!(methods(&clean), ["meta"]);
}

#[test]
fn same_instant_post_snapshot_config_removal_keeps_final_mod_pending() {
    for mode in [StampMode::Local, StampMode::KeepMtime] {
        let f = Fixture::new(0);
        f.collection
            .run(OpSpec::named("seed removable config", OpSource::LocalApi), |ctx| {
                ctx.scope().set_config("remove-after-snapshot", &1_i64.into())?;
                Ok(())
            })
            .unwrap();
        let collection = f.collection.clone();
        let scm = schema(&f);
        let stub = SyncStub::serve(move |r| {
            if r.path().ends_with("/meta") {
                return default_reply(r.path(), scm, NOW + 61_000);
            }
            if r.path().ends_with("applyChanges") {
                return Reply::ok(
                    br#"{"models":[],"decks":[[],[]],"tags":[],"conf":{"schedVer":2,"remove-after-snapshot":1}}"#,
                );
            }
            if r.path().ends_with("/chunk") {
                collection
                    .run(
                        OpSpec::named("remove after snapshot", OpSource::LocalApi).stamp(mode),
                        |ctx| {
                            assert_eq!(ctx.scope().col()?.modified.0, ctx.now().0);
                            assert!(ctx.scope().remove_config("remove-after-snapshot")?.changed());
                            Ok(())
                        },
                    )
                    .unwrap();
                assert_eq!(
                    collection.reader(ReaderRole::Query).unwrap().col().unwrap().modified.0,
                    NOW
                );
            }
            default_reply(r.path(), scm, NOW + 1000)
        });
        f.start(&stub.url()).wait().unwrap();
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        assert!(rows.config_entry("remove-after-snapshot").unwrap().is_none());
        assert_eq!(rows.col().unwrap().modified.0, rows.col().unwrap().last_sync.0 + 1);
        assert_eq!(
            meguri_sync::status::LocalRecord::read(&*rows).unwrap().offline_status(),
            meguri_sync::status::OfflineStatus::PendingChanges
        );
    }
}

#[test]
fn post_snapshot_raw_day_bookkeeping_does_not_keep_final_mod_pending() {
    let f = Fixture::new(0);
    let collection = f.collection.clone();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/meta") {
            return default_reply(r.path(), scm, NOW + 61_000);
        }
        if r.path().ends_with("/chunk") {
            collection
                .run(
                    OpSpec::named("day bookkeeping after snapshot", OpSource::Rollover).stamp(StampMode::Raw),
                    |ctx| {
                        assert!(ctx.scope().set_config("lastUnburied", &(-1_i64).into())?.changed());
                        Ok(())
                    },
                )
                .unwrap();
        }
        default_reply(r.path(), scm, NOW + 1000)
    });
    f.start(&stub.url()).wait().unwrap();
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(rows.col().unwrap().modified, rows.col().unwrap().last_sync);
    assert_eq!(
        meguri_sync::status::LocalRecord::read(&*rows).unwrap().offline_status(),
        meguri_sync::status::OfflineStatus::NothingPending
    );
}

#[test]
fn received_config_server_usns_leave_clean_status() {
    let f = Fixture::new(0);
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/meta") {
            return default_reply(r.path(), scm, NOW + 61_000);
        }
        if r.path().ends_with("applyChanges") {
            return Reply::ok(br#"{"models":[],"decks":[[],[]],"tags":[],"conf":{"schedVer":2,"remote":true}}"#);
        }
        default_reply(r.path(), scm, NOW + 1000)
    });
    f.start(&stub.url()).wait().unwrap();
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(rows.config_entry("remote").unwrap().unwrap().entry.usn, Usn(5));
    assert_eq!(rows.col().unwrap().modified, rows.col().unwrap().last_sync);
    assert_eq!(
        meguri_sync::status::LocalRecord::read(&*rows).unwrap().offline_status(),
        meguri_sync::status::OfflineStatus::NothingPending
    );
}

fn check_sync_config_undo(conf: Value) {
    let f = Fixture::new(1);
    f.collection
        .run(OpSpec::named("config A", OpSource::UserUndoable), |ctx| {
            ctx.scope().set_config("x", &"A".into())?;
            Ok(())
        })
        .unwrap();
    baseline(&f);
    f.clock.step_ms(1000);
    let active = f.host.review().begin_unqueued_session().unwrap();
    let scoped = active.collection().unwrap();
    f.answer(f.cards[0], Some(active.id()));
    scoped
        .run(OpSpec::named("config B after answer", OpSource::UserUndoable), |ctx| {
            ctx.scope().set_config("x", &"B".into())?;
            Ok(())
        })
        .unwrap();
    let held = scoped.held_back_run(Some(active.id())).unwrap();
    assert_eq!(held.steps.len(), 2);
    assert!(held.touches(Table::Cards, &RowKey::Id(f.cards[0].0)));
    assert!(held.touches(Table::Config, &RowKey::Text("x".into())));
    assert_eq!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value("x")
            .unwrap()
            .unwrap()
            .as_str(),
        Some("B")
    );
    let scm = schema(&f);
    let server_mod = f.clock.now_ms() + 1000;
    let body = json!({"models":[],"decks":[[],[]],"tags":[],"conf":conf}).to_string();
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/meta") {
            return Reply::ok(
                format!(
                    r#"{{"mod":{server_mod},"scm":{scm},"usn":5,"ts":{},"msg":"","cont":true,"empty":false,"media_usn":0}}"#,
                    server_mod / 1000
                )
                .as_bytes(),
            );
        }
        if r.path().ends_with("/applyChanges") {
            return Reply::ok(body.as_bytes());
        }
        default_reply(r.path(), scm, server_mod)
    });
    let out = f.start(&stub.url()).wait().unwrap();
    assert!(matches!(
        out.check.result,
        meguri_sync::status::CheckResult::Normal { local_is_newer: false }
    ));
    assert!(!out.merge.unwrap().one_way);
    assert_eq!(methods(&stub).last().unwrap(), "finish");
    assert!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value("x")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        scoped.peek_undo(UndoScope::Session(active.id())).unwrap().reason(),
        Some(DisabledReason::ClearedBySync)
    );
    assert!(scoped.held_back_run(Some(active.id())).unwrap().is_empty());
    assert_eq!(
        scoped.undo(UndoScope::Session(active.id())).unwrap_err(),
        meguri_engine::EngineError::UndoUnavailable {
            reason: DisabledReason::ClearedBySync
        }
    );
    assert!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value("x")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        card(&f, f.cards[0]).reps,
        1,
        "the answer stays committed after its undo run is cleared"
    );
}

#[test]
fn whole_config_removal_clears_held_answer_and_config_undo() {
    check_sync_config_undo(json!({"remote":true}));
}

#[test]
fn empty_config_replacement_clears_held_answer_and_config_undo() {
    check_sync_config_undo(json!({}));
}

#[test]
fn rejected_newer_local_deck_still_drops_whole_held_answer_run() {
    let f = Fixture::new(1);
    baseline(&f);
    let baseline_deck = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    let active = f.host.review().begin_unqueued_session().unwrap();
    f.answer(f.cards[0], Some(active.id()));
    let scoped = active.collection().unwrap();
    let held = scoped.held_back_run(Some(active.id())).unwrap();
    assert!(!held.is_empty());
    assert!(held.touches(Table::Decks, &RowKey::Id(1)));
    let before = held
        .before(Table::Decks, &RowKey::Id(1))
        .unwrap()
        .unwrap()
        .before
        .unwrap();
    assert_eq!(before[3].as_i64(), Some(i64::from(baseline_deck.usn.0)));
    let mut remote = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(DeckId(1))
        .unwrap()
        .unwrap();
    remote.name = "remote old name".into();
    remote.mtime = EpochSecs(remote.mtime.0 - 1);
    // The live answer is pending, but a server object carries a server USN.
    // V has the older pre-answer deck, so this object is accepted there and
    // rejected against the newer live deck at commit.
    remote.usn = Usn(5);
    assert!(baseline_deck.mtime < remote.mtime);
    assert!(remote.mtime < EpochSecs(NOW / 1000));
    let body = format!(
        r#"{{"models":[],"decks":[[{}],[]],"tags":[]}}"#,
        meguri_model::schema11::deck_to_json(&remote)
    );
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("applyChanges") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    f.start(&stub.url()).wait().unwrap();
    assert_eq!(methods(&stub).last().unwrap(), "finish");
    let sent = stub
        .requests()
        .into_iter()
        .find(|r| r.path().ends_with("applyChanges"))
        .unwrap();
    assert!(json_body(&sent)["changes"]["decks"][0].as_array().unwrap().is_empty());
    assert_eq!(
        active
            .collection()
            .unwrap()
            .peek_undo(UndoScope::Session(active.id()))
            .unwrap()
            .reason(),
        Some(DisabledReason::ClearedBySync)
    );
    assert_ne!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck(DeckId(1))
            .unwrap()
            .unwrap()
            .name,
        "remote old name"
    );
    assert!(scoped.held_back_run(Some(active.id())).unwrap().is_empty());
    assert_eq!(card(&f, f.cards[0]).reps, 1);
    assert_eq!(card(&f, f.cards[0]).usn, Usn::PENDING);
    assert_eq!(
        f.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck(DeckId(1))
            .unwrap()
            .unwrap()
            .usn,
        Usn::PENDING
    );
}
#[test]
fn post_snapshot_answer_undo_then_held_answer_undo() {
    let f = Fixture::new(3);
    baseline(&f);
    let active = f.host.review().begin_unqueued_session().unwrap();
    f.answer(f.cards[0], Some(active.id()));
    let collection = active.collection().unwrap();
    let answer = f.cards[1];
    let session = active.id();
    let scm = schema(&f);
    let mut remote = card(&f, f.cards[2]);
    remote.flags = 4;
    let body = incoming_card(&remote);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            let predicted =
                meguri_engine::sched::answer::predict(&collection, answer, &mut Default::default(), None).unwrap();
            let mut request = predicted.answer(meguri_testkit::normal_support::Rating::Good, 3000);
            request.session = Some(session);
            meguri_engine::sched::answer::answer(&collection, request).unwrap();
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    // Incoming card 3 only; its note does not target the deck touched by the
    // answers, so both session steps remain independently undoable.
    f.start(&stub.url()).wait().unwrap();
    let collection = active.collection().unwrap();
    collection.undo(UndoScope::Session(active.id())).unwrap();
    assert_eq!(card(&f, f.cards[1]).reps, 0);
    collection.undo(UndoScope::Session(active.id())).unwrap();
    assert_eq!(card(&f, f.cards[0]).reps, 0);
}
#[test]
fn corruption_during_applychunk_aborts_before_finish() {
    let f = Fixture::new(126);
    let before = f.images();
    let engine = f.host.engine().clone();
    let scm = schema(&f);
    let (abort, aborted) = mpsc::sync_channel(1);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("applyChunk") {
            engine.mark_corrupt("synthetic background quick-check");
        }
        if r.path().ends_with("abort") {
            let _ = abort.send(());
        }
        default_reply(r.path(), scm, NOW)
    });
    assert_eq!(
        f.start(&stub.url()).wait().unwrap_err().kind,
        ErrorKind::CollectionCorrupt
    );
    aborted.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(f.images(), before);
    assert!(!methods(&stub).contains(&"finish".into()));
}
#[test]
fn cancellation_during_chunk_is_unchanged_and_next_job_succeeds() {
    let f = Fixture::new(1);
    let before = f.images();
    let scm = schema(&f);
    let (entered, at_chunk) = mpsc::sync_channel(1);
    let (abort, aborted) = mpsc::sync_channel(1);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            let _ = entered.send(());
            return default_reply(r.path(), scm, NOW).head_delay(Duration::from_secs(3));
        }
        if r.path().ends_with("abort") {
            let _ = abort.send(());
        }
        default_reply(r.path(), scm, NOW)
    });
    let task = f.start(&stub.url());
    at_chunk.recv_timeout(Duration::from_secs(2)).unwrap();
    task.handle.cancel.cancel();
    assert_eq!(task.wait().unwrap_err().kind, ErrorKind::Cancelled);
    aborted.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(f.images(), before);
    let next = f.stub();
    f.start(&next.url()).wait().unwrap();
    assert!(!f.collection.path().with_file_name("meguri-sync-stage.db").exists());
}
#[test]
fn three_second_network_delay_does_not_delay_three_fsynced_answers() {
    let f = Fixture::new(4);
    baseline(&f);
    let scm = schema(&f);
    let (entered, at_chunk) = mpsc::sync_channel(1);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            let _ = entered.send(());
            return default_reply(r.path(), scm, NOW).head_delay(Duration::from_secs(3));
        }
        default_reply(r.path(), scm, NOW)
    });
    let task = f.start(&stub.url());
    at_chunk.recv_timeout(Duration::from_secs(2)).unwrap();
    for id in &f.cards[..3] {
        let accepted = meguri_storage::counting_vfs::clock::now();
        f.answer(*id, None);
        loop {
            let now = meguri_storage::counting_vfs::clock::now();
            assert!(
                now - accepted <= Duration::from_secs(1),
                "answer durability exceeded one second"
            );
            if f.vfs.records().iter().any(|r| {
                r.filename.ends_with("-wal")
                    && r.result == 0
                    && r.started >= accepted
                    && r.completed <= accepted + Duration::from_secs(1)
            }) {
                break;
            }
            std::thread::yield_now();
        }
    }
    task.wait().unwrap();
    let col = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert!(col.modified > col.last_sync);
    let uploads = stub
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(json_body)
        .collect::<Vec<_>>();
    assert!(uploads.iter().all(|b| b["chunk"].get("revlog").is_none()));
    let next = f.stub();
    f.start(&next.url()).wait().unwrap();
    let reviews = next
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(json_body)
        .map(|v| v["chunk"]["revlog"].as_array().map_or(0, Vec::len))
        .sum::<usize>();
    assert_eq!(reviews, 3);
}
#[test]
fn second_normal_is_refused_while_media_job_remains_independent() {
    let f = Fixture::new(1);
    let scm = schema(&f);
    let (entered, at_chunk) = mpsc::sync_channel(1);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            let _ = entered.send(());
            return default_reply(r.path(), scm, NOW).head_delay(Duration::from_secs(3));
        }
        default_reply(r.path(), scm, NOW)
    });
    let task = f.start(&stub.url());
    at_chunk.recv_timeout(Duration::from_secs(2)).unwrap();
    let second = normal::start(
        f.host.engine(),
        f.collection.clone(),
        meguri_sync::JobOptions::new(
            meguri_sync::Endpoint::parse(&stub.url()).unwrap(),
            meguri_sync::ServerKind::SelfHosted,
        ),
        meguri_sync::HostKey::new("synthetic"),
        f.clock.clone(),
        f.random.clone(),
        f.memory.clone(),
        None,
    );
    assert!(matches!(second, Err(meguri_engine::EngineError::Busy { .. })));
    let media = f
        .host
        .engine()
        .start_job(meguri_engine::JobKind::MediaSync, |_| Ok(()))
        .unwrap();
    assert_ne!(media.id, task.handle.id);
    task.handle.cancel.cancel();
    assert_eq!(task.wait().unwrap_err().kind, ErrorKind::Cancelled);
}
#[test]
fn snapshot_barrier_races_1000_real_ops_without_mixing_sequences() {
    let f = Fixture::new(1);
    baseline(&f);
    let note = card(&f, f.cards[0]).note_id;
    let collection = f.collection.clone();
    let records = Arc::new(Mutex::new(Vec::new()));
    let recorded = records.clone();
    let captured = Arc::new(Mutex::new(None));
    let seen = captured.clone();
    let _hook = normal::test_hooks::install(Arc::new(move |p| {
        if let normal::test_hooks::Point::Snapshot(seq) = p {
            *seen.lock().unwrap() = Some(seq);
        }
    }));
    let producer = f
        .host
        .engine()
        .threads()
        .spawn_owned(
            "normal barrier race",
            meguri_engine::Qos::UserInitiated,
            "jobs",
            move |_| {
                for i in 0..1000 {
                    let result = collection
                        .run(OpSpec::named("race edit", OpSource::UserUndoable), move |ctx| {
                            let mut row = ctx.scope().note(note)?.unwrap();
                            row.fields = format!("{i}\u{1f}back");
                            ctx.scope().put(&row)?;
                            Ok(())
                        })
                        .unwrap();
                    recorded.lock().unwrap().push((result.seq, i));
                }
            },
        )
        .unwrap();
    let stub = f.stub();
    f.start(&stub.url()).wait().unwrap();
    producer.join().unwrap();
    let seq = captured.lock().unwrap().unwrap();
    let expected = records
        .lock()
        .unwrap()
        .iter()
        .filter(|(s, _)| *s <= seq)
        .max_by_key(|(s, _)| *s)
        .map(|(_, i)| i.to_string());
    let sent = stub
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("applyChunk"))
        .map(json_body)
        .flat_map(|v| v["chunk"]["notes"].as_array().cloned().unwrap_or_default())
        .find(|v| v[0] == note.0);
    match expected {
        Some(value) => assert_eq!(sent.unwrap()[6], format!("{value}\u{1f}back")),
        None => assert!(sent.is_none()),
    }
}
#[test]
fn pending_undo_refused_during_round_and_restored_after_pre_finish_failure() {
    let f = Fixture::new(2);
    baseline(&f);
    f.answer(f.cards[0], None);
    let collection = f.collection.clone();
    let scm = schema(&f);
    let (reason, observed) = mpsc::sync_channel(1);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            let refusal = collection.undo(UndoScope::Global).unwrap_err();
            reason.send(refusal).unwrap();
            return Reply::status(400);
        }
        default_reply(r.path(), scm, NOW)
    });
    assert!(matches!(
        f.start(&stub.url()).wait().unwrap_err().kind,
        ErrorKind::DbCheckRequired { .. }
    ));
    assert!(matches!(
        observed.recv_timeout(Duration::from_secs(2)).unwrap(),
        meguri_engine::EngineError::UndoUnavailable {
            reason: DisabledReason::UploadPending
        }
    ));
    f.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(card(&f, f.cards[0]).reps, 0);
}
#[test]
fn held_deletion_own_id_skips_incoming_note_and_card_but_not_other_missing_note_card() {
    let f = Fixture::new(1);
    baseline(&f);
    let id = f.cards[0];
    let mut remote = card(&f, id);
    let note = remote.note_id;
    let active = f.host.review().begin_unqueued_session().unwrap();
    f.answer(id, Some(active.id()));
    meguri_engine::notes::delete(&active.collection().unwrap(), vec![note], OpSource::UserUndoable).unwrap();
    remote.mtime = EpochSecs(NOW / 1000 + 3);
    let own = json_body_bytes(incoming_card(&remote).as_bytes())["cards"][0].clone();
    remote.id = CardId(id.0 + 9000);
    let other = remote.id;
    let otherrow = json_body_bytes(incoming_card(&remote).as_bytes())["cards"][0].clone();
    let mid = f.notetype.0;
    let body=json!({"done":true,"cards":[own,otherrow],"notes":[[note.0,"remote",mid,NOW/1000+3,5,"","new\u{1f}back","","",0,""]]}).to_string();
    let scm = schema(&f);
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    let outcome = f.start(&stub.url()).wait().unwrap();
    let skipped = outcome.merge.unwrap().skipped;
    assert!(skipped.contains(&(Table::Cards, id.0)));
    assert!(skipped.contains(&(Table::Notes, note.0)));
    let rows = f.collection.reader(ReaderRole::Query).unwrap();
    assert!(rows.note(note).unwrap().is_none());
    assert!(rows.card(id).unwrap().is_none());
    assert!(rows.card(other).unwrap().is_some());
}
#[test]
fn real_current_card_changed_event_refuses_old_state_token() {
    use meguri_engine::session::{AnswerOutcome, SessionEvent, StartResult};
    let f = Fixture::new(2);
    baseline(&f);
    let StartResult::Started { session_id, head, .. } = f.host.review().start(DeckId(1), None).unwrap() else {
        panic!("session refused")
    };
    let shown = head.items[0].clone();
    let mut remote = card(&f, shown.card_id);
    remote.flags = 5;
    remote.mtime = EpochSecs(remote.mtime.0 + 1);
    let body = incoming_card(&remote);
    let scm = schema(&f);
    let events = f.host.engine().stream().subscribe();
    let stub = SyncStub::serve(move |r| {
        if r.path().ends_with("/chunk") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), scm, NOW)
        }
    });
    f.start(&stub.url()).wait().unwrap();
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(2);
    let mut changed = false;
    while !changed {
        assert!(meguri_engine::monotonic::now() < deadline, "CurrentCardChanged absent");
        if let Some(event) = events.try_recv() {
            if let meguri_engine::EventKind::Extension(e) = &event.kind {
                changed = matches!(e.as_any().downcast_ref::<SessionEvent>(),Some(SessionEvent::CurrentCardChanged{card_id,..}) if *card_id==shown.card_id);
            }
        } else {
            std::thread::yield_now();
        }
    }
    assert!(matches!(
        f.host
            .review()
            .answer(
                session_id,
                shown.card_id,
                meguri_testkit::normal_support::Rating::Good,
                shown.state_token,
                EpochMillis(NOW),
                3000
            )
            .unwrap(),
        AnswerOutcome::Stale { .. }
    ));
}
#[path = "normal_sync/field_rename.rs"]
mod field_rename;
#[path = "normal_sync/large.rs"]
mod large;

#[path = "normal_sync/diagnostics.rs"]
mod diagnostics;
