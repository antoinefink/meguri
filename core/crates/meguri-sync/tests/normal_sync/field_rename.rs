//! Field rename propagation through normal sync after collection baselines.

use super::*;
use meguri_engine::notetypes::{self, AggregateDigest, FieldIdentity, FieldRename, RenameFields};
use meguri_engine::{CancelToken, ProgressSink};
use meguri_model::notetype::Notetype;
use meguri_model::schema11::{notetype_from_json, notetype_to_json};

fn read_notetype(f: &Fixture) -> Notetype {
    f.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(f.notetype)
        .unwrap()
        .unwrap()
}

fn semantic_json(nt: &Notetype) -> Value {
    serde_json::from_str(&notetype_to_json(nt)).unwrap()
}

fn normal_round(f: &Fixture, stub: &SyncStub) {
    let out = f.start(&stub.url()).wait().unwrap();
    assert!(matches!(out.check.result, meguri_sync::CheckResult::Normal { .. }));
    assert!(!out.merge.expect("normal round must actually merge").one_way);
    let requests = methods(stub);
    for required in ["meta", "start", "applyChanges", "chunk", "sanityCheck2", "finish"] {
        assert!(requests.iter().any(|method| method == required), "missing {required}");
    }
    assert!(requests.iter().all(|method| method != "upload" && method != "download"));
}

#[test]
fn complete_u29_field_rename_upload_reaches_fresh_peer_after_real_baseline() {
    let f = Fixture::new(1);

    // Fixture metadata only, before baseline. Counts/order/kind/sort stay put;
    // the tested post-baseline edit below has no raw write or plain-save path.
    let mut seed = semantic_json(&read_notetype(&f));
    seed["css"] = json!(".card { color: #456; }");
    seed["latexPre"] = json!("rename fixture preamble");
    seed["latexPost"] = json!("rename fixture postamble");
    seed["renameFixture"] = json!({"revision": 29, "keep": [true, "parent"]});
    seed["flds"][0]["sticky"] = json!(true);
    seed["flds"][0]["description"] = json!("first field identity");
    seed["flds"][0]["tag"] = json!(7);
    seed["flds"][0]["preventDeletion"] = json!(true);
    seed["flds"][0]["renameFixture"] = json!({"keep": "first"});
    seed["flds"][1]["rtl"] = json!(true);
    seed["flds"][1]["font"] = json!("Fixture Serif");
    seed["flds"][1]["size"] = json!(23);
    seed["flds"][1]["plainText"] = json!(true);
    seed["flds"][1]["excludeFromSearch"] = json!(true);
    seed["flds"][1]["renameFixture"] = json!(["second", 107]);
    seed["tmpls"][0]["qfmt"] = json!("{{Front}}|{{text:Back}}");
    seed["tmpls"][0]["afmt"] = json!("{{FrontSide}}|{{#Front}}{{hint:Back}}{{/Front}}");
    seed["tmpls"][0]["bqfmt"] = json!("{{text:hint:Front}}|{{^Back}}{{Front}}{{/Back}}");
    seed["tmpls"][0]["bafmt"] = json!("{{Back}}|{{# Front }}{{text:Front}}{{/ Front }}<!--{{Back}}-->");
    seed["tmpls"][0]["bfont"] = json!("Fixture Sans");
    seed["tmpls"][0]["bsize"] = json!(19);
    seed["tmpls"][0]["renameFixture"] = json!({"keep": ["template", false]});
    seed["req"] = json!([[0, "any", [0, 1]]]);
    let prepared = notetype_from_json(&seed.to_string()).unwrap();
    f.collection
        .run(
            OpSpec::named("field rename sync fixture", OpSource::Maintenance),
            move |ctx| {
                ctx.scope().put_notetype(&prepared)?;
                Ok(())
            },
        )
        .unwrap();

    let initial = f.stub();
    normal_round(&f, &initial);
    let original_scm = schema(&f);
    let captured = read_notetype(&f);
    let base = AggregateDigest::of(&captured);
    assert_eq!(captured.usn, Usn(5));
    seed["usn"] = json!(5);
    assert_eq!(
        semantic_json(&captured),
        seed,
        "baseline must retain the prepared metadata"
    );
    assert_eq!(captured.fields.len(), 2);
    assert_eq!(captured.templates.len(), 1);
    assert!(captured.fields.iter().all(|field| field.config.id.is_some()));
    assert_ne!(captured.fields[0].config.id, captured.fields[1].config.id);
    assert!(captured.templates[0].config.id.is_some());
    let card_before = card(&f, f.cards[0]);
    let preserved_rows = |f: &Fixture| {
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        [
            rows.row_image(Table::Notes, &RowKey::Id(card_before.note_id.0))
                .unwrap(),
            rows.row_image(Table::Cards, &RowKey::Id(card_before.id.0)).unwrap(),
        ]
    };
    let before_rows = preserved_rows(&f);

    f.clock.step_ms(2000);
    let renamed_at = EpochSecs(f.clock.now_ms() / 1000);
    let saved = notetypes::rename_fields(
        &f.collection,
        RenameFields {
            notetype: captured.id,
            base,
            fields: vec![
                FieldRename {
                    field: FieldIdentity::of(&captured.fields[0]),
                    name: "Back".into(),
                },
                FieldRename {
                    field: FieldIdentity::of(&captured.fields[1]),
                    name: "Front".into(),
                },
            ],
        },
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap();
    assert!(saved.value.written);
    let renamed = read_notetype(&f);
    assert_eq!(renamed.usn, Usn::PENDING);
    assert_eq!(renamed.mtime, renamed_at);
    assert_eq!(schema(&f), original_scm);
    assert_eq!(preserved_rows(&f), before_rows);

    // Independent expected edits on the captured whole object: every other
    // member, including IDs, ordinals, names/options and unknown JSON, stays.
    let mut expected = semantic_json(&captured);
    expected["flds"][0]["name"] = json!("Back");
    expected["flds"][1]["name"] = json!("Front");
    expected["tmpls"][0]["qfmt"] = json!("{{Back}}|{{text:Front}}");
    expected["tmpls"][0]["afmt"] = json!("{{FrontSide}}|{{#Back}}{{hint:Front}}{{/Back}}");
    expected["tmpls"][0]["bqfmt"] = json!("{{text:hint:Back}}|{{^Front}}{{Back}}{{/Front}}");
    expected["tmpls"][0]["bafmt"] = json!("{{Front}}|{{# Back }}{{text:Back}}{{/ Back }}<!--{{Back}}-->");
    expected["req"] = json!([[0, "any", [0, 1]]]);
    expected["mod"] = json!(renamed_at.0);
    expected["usn"] = json!(-1);
    assert_eq!(semantic_json(&renamed), expected);
    assert_eq!(renamed.templates[0].mtime, captured.templates[0].mtime);
    assert_eq!(renamed.templates[0].usn, captured.templates[0].usn);

    let upload = f.stub();
    normal_round(&f, &upload);
    let requests = upload.requests();
    let changes = requests.iter().find(|r| r.path().ends_with("applyChanges")).unwrap();
    let changes = json_body(changes);
    let models = changes["changes"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 1, "baseline must leave only the renamed parent pending");
    let uploaded_model = models[0].clone();
    // W6.2/N9.2: outgoing parent USN is the server's, not pending -1.
    expected["usn"] = json!(5);
    assert_eq!(uploaded_model, expected);
    let uploaded = read_notetype(&f);
    assert_eq!(uploaded.usn, Usn(5));
    assert_eq!(uploaded.mtime, renamed_at);
    assert_eq!(semantic_json(&uploaded), expected);
    assert_eq!(schema(&f), original_scm);

    let peer = Fixture::new(0);
    assert_eq!(peer.notetype, f.notetype);
    let peer_initial = peer.stub();
    normal_round(&peer, &peer_initial);
    let peer_scm = schema(&peer);
    assert_eq!(peer_scm, original_scm);
    assert_eq!(read_notetype(&peer).fields[0].name, "Front");
    // Feed the captured complete upload, without reconstructing an incoming
    // note type or patching stamps, through the peer's actual applyChanges.
    let body = json!({"models": [uploaded_model], "decks": [[], []], "tags": []}).to_string();
    let remote = SyncStub::serve(move |r| {
        if r.path().ends_with("applyChanges") {
            Reply::ok(body.as_bytes())
        } else {
            default_reply(r.path(), peer_scm, NOW)
        }
    });
    normal_round(&peer, &remote);
    let received = read_notetype(&peer);
    assert_eq!(semantic_json(&received), expected);
    assert_eq!(received.usn, Usn(5));
    assert_eq!(received.mtime, renamed_at);
    // N5.2/S1: schema-11 has no template stamps. Incoming templates take
    // zero; the local rename preserves the captured template stamps.
    assert_eq!(received.templates[0].usn, Usn(0));
    assert_eq!(received.templates[0].mtime, EpochSecs(0));
    assert_eq!(schema(&peer), peer_scm);
    assert_eq!(schema(&f), schema(&peer));
}
