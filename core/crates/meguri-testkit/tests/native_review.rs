//! F11.2 record refusals and trusted Rust delivery/persistence on an owned
//! unit fixture. Unit-authored preparation metadata exercises the protocol;
//! pinned-decoder producer compatibility and native journeys remain required.
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    config::ConfigEntry,
    ids::DeckId,
    rng::SeededRandom,
    usn::Usn,
};
use meguri_storage::{CollectionBuilder, OpenOptions, ReadRows, StampMode, open_collection};
use meguri_testkit::generate::{
    self,
    recipe::{self, Recipe, generate_recipe},
    rng::{Rng, Stream},
};
use meguri_testkit::native_review::{
    self, Action, Answer, BoundFile, Choice, Code, Identity, Input, Rating, Seeds, Wall,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
fn id(bytes: &[u8]) -> Identity {
    Identity {
        sha256: Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect(),
        bytes: bytes.len() as u64,
    }
}
fn record() -> Input {
    let run = "build/mg/ui-test/record-test";
    Input {
        schema_version: 1,
        purpose: "f11.2-trusted-rust-delivery-persistence".into(),
        run: run.into(),
        platform: "mac".into(),
        preparation: BoundFile {
            path: format!("{run}/source-preparation/preparation.json"),
            identity: id(&[0x11, 0x00]),
        },
        parent_source: "build/fixtures/parent".into(),
        parent_files: BTreeMap::new(),
        prepared_source: format!("{run}/source-preparation/fixture"),
        prepared_files: BTreeMap::new(),
        package: BoundFile {
            path: format!("{run}/expected-replay/input.colpkg"),
            identity: id(&[0x22, 0x00]),
        },
        generator_seed: 9,
        wall: Wall {
            now: "2030-01-15T10:00:00+01:00".into(),
            tz: "Europe/Paris".into(),
            now_ms: 1_894_698_000_000,
        },
        seeds: Seeds {
            source_open: 101,
            package_write: 102,
            inspection_and_initial_activation: 103,
            reopen: 104,
        },
        chosen: Choice {
            deck_id: 1,
            deck_name: "Default".into(),
            card_id: 1_880_000_000_000,
            resume_card_id: None,
        },
        answer: Answer {
            rating: Rating::Easy,
            elapsed_ms: 1000,
            count: 1,
            preset_cap_answer_time_secs: 60,
        },
        no_old_database: true,
        actions: vec![
            Action::InspectAndActivateInitial,
            Action::SelectDeck,
            Action::StartSession,
            Action::RevealAnswer,
            Action::AnswerEasy,
            Action::EndSession,
            Action::Close,
            Action::Reopen,
            Action::ExportAllMedia,
        ],
        code: Code {
            revision: "0".repeat(40),
            build_profile: "dev".into(),
            executable: id(&[0x33, 0x00]),
            replay_source: id(&[0x44, 0x00]),
        },
    }
}
#[test]
fn native_review_record_rejects_unknown_missing_overflow_wrong_run_and_actions() {
    let input = record();
    let bytes = serde_json::to_vec(&input).unwrap();
    assert_eq!(Input::parse(&bytes, &input.run, "mac").unwrap(), input);
    assert!(Input::parse(&bytes, "build/mg/ui-test/another", "mac").is_err());
    assert!(Input::parse(&bytes, &input.run, "ipad").is_err());
    let original = serde_json::to_value(&input).unwrap();
    for pointer in [
        "/seeds/reopen",
        "/actions",
        "/no_old_database",
        "/preparation",
        "/chosen/resume_card_id",
    ] {
        let mut changed = original.clone();
        let (prefix, field) = pointer.rsplit_once('/').unwrap();
        changed
            .pointer_mut(prefix)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            Input::parse(&serde_json::to_vec(&changed).unwrap(), &input.run, "mac").is_err(),
            "{pointer}"
        );
    }
    for changed in [
        {
            let mut v = original.clone();
            v["seeds"]["extra_open"] = 1.into();
            v
        },
        {
            let mut v = original.clone();
            v["answer"]["elapsed_ms"] = u64::MAX.into();
            v
        },
        {
            let mut v = original.clone();
            v["answer"]["count"] = 2.into();
            v
        },
        {
            let mut v = original.clone();
            v["answer"]["preset_cap_answer_time_secs"] = u32::MAX.into();
            v
        },
        {
            let mut v = original.clone();
            v["actions"][4] = "force_head".into();
            v
        },
        {
            let mut v = original.clone();
            v["no_old_database"] = false.into();
            v
        },
        {
            let mut v = original.clone();
            v["wall"]["now_ms"] = 1.into();
            v
        },
    ] {
        assert!(Input::parse(&serde_json::to_vec(&changed).unwrap(), &input.run, "mac").is_err());
    }
}
fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        assert!(!entry.file_type().unwrap().is_symlink());
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}
fn fixture_files(dir: &Path) -> BTreeMap<String, Identity> {
    let mut files = BTreeMap::new();
    for name in ["collection.anki2", "manifest.json"] {
        files.insert(name.into(), id(&fs::read(dir.join(name)).unwrap()));
    }
    for entry in fs::read_dir(dir.join("collection.media")).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        files.insert(
            format!("collection.media/{}", entry.file_name().to_str().unwrap()),
            id(&fs::read(entry.path()).unwrap()),
        );
    }
    files
}
// Same owned fixture pattern as TestMedia.silentWAV in WebTestHarness.swift:
// 8 kHz, mono, 16-bit PCM, here one quarter second of zero-valued samples.
fn silent_wav() -> Vec<u8> {
    let data_bytes = 4_000u32;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data_bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(8_000u32.to_le_bytes());
    wav.extend(16_000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(data_bytes.to_le_bytes());
    wav.resize(wav.len() + data_bytes as usize, 0);
    wav
}
fn prepare_unit_fixture(root: &Path, run: &str) -> serde_json::Value {
    let parent = root.join("build/fixtures/parent");
    fs::create_dir_all(parent.parent().unwrap()).unwrap();
    let recipe = Recipe::named("local-review").unwrap();
    let seed = record().generator_seed;
    let manifest = generate_recipe(&recipe, seed, &parent).unwrap();
    assert_eq!(manifest.counts.cards, 4);
    let parent_files = fixture_files(&parent);
    let parent_manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(parent.join("manifest.json")).unwrap()).unwrap();
    let preparation = root.join(run).join("source-preparation");
    let prepared = preparation.join("fixture");
    fs::create_dir_all(prepared.join("collection.media")).unwrap();
    let read_copy = tempfile::tempdir().unwrap();
    copy_tree(&parent, &read_copy.path().join("parent"));
    let read_path = read_copy.path().join("parent/collection.anki2");
    let clock = FixedClock::new(record().wall.now_ms, UtcOffset::from_seconds(3600).unwrap()).in_zone("Europe/Paris");
    let random = SeededRandom::new(seed);
    let opened = open_collection(&read_path, &OpenOptions::new(&clock, &random)).unwrap();
    let reader = opened.writer();
    let col = reader.col().unwrap();
    let mut card = reader.cards_after(None, 1).unwrap().remove(0);
    let original_card = card.clone();
    let mut note = reader.note(card.note_id).unwrap().unwrap();
    let original_note = note.clone();
    let selected_parent_deck = reader.deck(card.deck_id).unwrap().unwrap();
    let deck = reader.deck(DeckId(1)).unwrap().unwrap();
    let preset = reader.deck_config_rows().unwrap().remove(0);
    let notetype = reader.notetype(note.notetype_id).unwrap().unwrap();
    let history = reader
        .revlog_after(None, 30)
        .unwrap()
        .into_iter()
        .filter(|row| row.card_id == card.id)
        .collect::<Vec<_>>();
    assert!(!history.is_empty() && history.len() < 29);
    let tags = reader.tag_rows().unwrap();
    let mut config = reader
        .config_keys()
        .unwrap()
        .into_iter()
        .map(|key| reader.config_entry(&key).unwrap().unwrap().entry)
        .collect::<Vec<_>>();
    assert!(!config.iter().any(|row| row.key == "localOffset"));
    let offset = ConfigEntry {
        key: "localOffset".into(),
        value: b"-60".to_vec(),
        usn: Usn(-1),
        mtime: col.created,
    };
    config.push(offset.clone());
    card.deck_id = DeckId(1);
    note.fields
        .push_str(" <img src=\"meguri-review.png\"> [sound:meguri-review.wav]");
    let changes = serde_json::json!({
        "scope":"Unit fixture built with typed rows; decoder preparation pending.",
        "card_before":format!("{original_card:?}"), "card_after":format!("{card:?}"),
        "note_before":format!("{original_note:?}"), "note_after":format!("{note:?}"),
        "localOffset_added":format!("{offset:?}")});
    let builder = CollectionBuilder::create(
        &prepared.join("collection.anki2"),
        &col,
        &config,
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    {
        let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
        scope.put(&deck).unwrap();
        scope.put(&preset).unwrap();
        scope.put_notetype(&notetype).unwrap();
        scope.put(&note).unwrap();
        scope.put(&card).unwrap();
        for row in &history {
            scope.put(row).unwrap();
        }
        for tag in &tags {
            scope.put(tag).unwrap();
        }
        scope.commit().unwrap();
    }
    builder.finish().unwrap();
    opened.close().unwrap();
    let mut rng = Rng::stream(seed, Stream::Media, 0);
    fs::write(
        prepared.join("collection.media/meguri-review.png"),
        generate::media::png(1, 1, &mut rng),
    )
    .unwrap();
    fs::write(prepared.join("collection.media/meguri-review.wav"), silent_wav()).unwrap();
    fs::write(
        prepared.join("collection.media/_meguri-unused.txt"),
        &[0x00, 0x71, 0x00, 0x92],
    )
    .unwrap();
    fs::write(
        prepared.join("manifest.json"),
        serde_json::to_vec(&serde_json::json!({
        "kind":"meguri-f11.2-unit-subset", "parent_manifest_identity":parent_files["manifest.json"],
        "row_changes":changes}))
        .unwrap(),
    )
    .unwrap();
    let prepared_files = fixture_files(&prepared);
    let media_bytes = prepared_files
        .iter()
        .filter(|(name, _)| name.starts_with("collection.media/"))
        .map(|(_, identity)| identity.bytes)
        .sum::<u64>();
    let recipe_value =
        meguri_testkit::scenario::parse_yaml(&fs::read_to_string(recipe::dir().join("local-review.yaml")).unwrap())
            .unwrap();
    let p = serde_json::json!({"schema_version":1, "operation":"prepare-native-review", "stage":"source-only-before-app",
        "run":run, "platform":"mac", "source":"build/fixtures/parent",
        "prepared_source":format!("{run}/source-preparation/fixture"), "clock":record().wall,
        "generator_request":{"recipe":recipe_value,"seed":seed,"generator_version":generate::GENERATOR_VERSION},
        "parent_files":parent_files, "prepared_files":prepared_files,
        "source_facts":{"kind":"meguri-f11.2-source", "parent_manifest":parent_manifest,
            "parent_manifest_identity":parent_files["manifest.json"], "note_id":note.id.0, "card_id":card.id.0,
            "deck_id":1,"deck_name":"Default", "row_counts":{"notes":1,"cards":1,"decks":1,"revlog":history.len()},
            "media_refs":2,"media_files":3,"media_bytes":media_bytes},
        "preset_cap_answer_time_secs":preset.config.cap_answer_time_to_secs,
        "media":prepared_files.iter().filter(|(name, _)| name.starts_with("collection.media/")).collect::<BTreeMap<_, _>>(),
        "row_changes":changes, "selected_parent_deck":format!("{selected_parent_deck:?}"),
        "preserved_card":format!("{original_card:?}"),"preserved_history":format!("{history:?}"),
        "media_pattern":"Generated PNG; silent WAV at 8 kHz, mono PCM16.",
        "derivation":"Unit metadata for replay checks; no decoder producer evidence.",
        "remaining":["Pinned decoder source run", "Archive and client comparison", "Mac, iPhone and iPad app runs"]});
    fs::write(preparation.join("preparation.json"), serde_json::to_vec(&p).unwrap()).unwrap();
    p
}
fn snapshot_rows(output: &Path, state: &str) -> (meguri_model::col::Col, Vec<meguri_model::revlog::RevlogEntry>) {
    // Never open or change a sealed artifact to inspect its rows.
    let copy = tempfile::tempdir().unwrap();
    let path = copy.path().join("collection.anki2");
    fs::copy(output.join(state).join("collection.anki2"), &path).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let clock = FixedClock::new(record().wall.now_ms, UtcOffset::from_seconds(3600).unwrap()).in_zone("Europe/Paris");
    let random = SeededRandom::new(0);
    let opened = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let col = opened.writer().col().unwrap();
    let history = opened.writer().revlog_after(None, 30).unwrap();
    assert!(history.len() < 30);
    opened.close().unwrap();
    (col, history)
}
fn assert_tamper_refused(dir: &Path, p: &Path, sig: &Identity) {
    use std::os::unix::fs::PermissionsExt;
    let buf = fs::read(p).unwrap();
    let mode = fs::metadata(p).unwrap().permissions();
    fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
    let mut bad = buf.clone();
    bad.push(b' ');
    fs::write(p, bad).unwrap();
    assert!(
        Input::load_frozen(dir, "actual-replay", "mac", sig).is_err(),
        "{}",
        p.display()
    );
    fs::write(p, buf).unwrap();
    fs::set_permissions(p, mode).unwrap();
    Input::load_frozen(dir, "actual-replay", "mac", sig).unwrap();
}
#[test]
fn native_review_actual_replay_one_accepted_answer_reopen_media_and_unchanged_parent() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let r = "build/mg/ui-test/actual-replay";
    let meta = prepare_unit_fixture(&dir, r);
    let old = fs::read(dir.join("build/fixtures/parent/collection.anki2")).unwrap();
    let out = native_review::derive(
        &dir,
        "actual-replay",
        meta["platform"].as_str().unwrap(),
        record().seeds,
        &"0".repeat(40),
        "dev",
    )
    .unwrap();
    let sig = id(&fs::read(out.join("freeze.json")).unwrap());
    let v = Input::load_frozen(&dir, "actual-replay", meta["platform"].as_str().unwrap(), &sig).unwrap();
    assert_eq!(v.seeds, record().seeds);
    assert_eq!(v.chosen.card_id, meta["source_facts"]["card_id"].as_i64().unwrap());
    assert_eq!(v.answer, record().answer);
    assert_eq!(v.actions, record().actions);
    assert_eq!(v.parent_files, fixture_files(&dir.join(&v.parent_source)));
    assert_eq!(v.prepared_files, fixture_files(&dir.join(&v.prepared_source)));
    assert_eq!(serde_json::to_value(&v.parent_files).unwrap(), meta["parent_files"]);
    assert_eq!(serde_json::to_value(&v.prepared_files).unwrap(), meta["prepared_files"]);
    assert_eq!(
        fs::read(dir.join("build/fixtures/parent/collection.anki2")).unwrap(),
        old
    );
    let log: serde_json::Value = serde_json::from_slice(&fs::read(out.join("replay.json")).unwrap()).unwrap();
    assert_eq!(log["accepted_answers"], 1);
    assert_eq!(
        log["import_media"],
        serde_json::json!({"incoming":3,"added":3,"reused":0,"trashed":0})
    );
    assert_eq!(log["reopened"], true);
    assert!(log["backup"].is_null());
    assert_eq!(log["journal_initialized"], true);
    let (_, h0) = snapshot_rows(&out, "source");
    let (c1, h1) = snapshot_rows(&out, "import");
    let (c2, h2) = snapshot_rows(&out, "review");
    assert_eq!(h1, h0);
    assert_eq!(
        h0.len() as u64,
        meta["source_facts"]["row_counts"]["revlog"].as_u64().unwrap()
    );
    assert_eq!(h2.len(), h0.len() + 1);
    assert!(h0.iter().all(|row| h2.contains(row)));
    let add = h2.iter().filter(|row| !h0.contains(row)).collect::<Vec<_>>();
    assert_eq!(add.len(), 1);
    assert_eq!(
        (add[0].card_id.0, add[0].id.0, add[0].button, add[0].answer_millis),
        (v.chosen.card_id, v.wall.now_ms, 4, 1000)
    );
    assert_eq!(c1.modified.0, v.wall.now_ms);
    assert_eq!(c1.schema_modified.0, v.wall.now_ms);
    assert_eq!(c2.modified.0, v.wall.now_ms);
    assert_eq!(c2.schema_modified.0, c1.schema_modified.0);
    for s in ["source", "import", "review"] {
        let fs0 = fixture_files(&out.join(s));
        assert_eq!(fs0.len(), 5);
        for (n, sum) in v
            .prepared_files
            .iter()
            .filter(|(n, _)| n.starts_with("collection.media/"))
        {
            assert_eq!(&fs0[n], sum, "{s}/{n}");
        }
    }
    let f: serde_json::Value = serde_json::from_slice(&fs::read(out.join("freeze.json")).unwrap()).unwrap();
    let fs0 = f["files"].as_object().unwrap();
    assert_eq!(fs0.len(), 18);
    let mut ks = vec![
        "input.json".to_owned(),
        "input.colpkg".to_owned(),
        "replay.json".to_owned(),
    ];
    for s in ["source", "import", "review"] {
        ks.extend(v.prepared_files.keys().map(|n| format!("{s}/{n}")));
    }
    ks.sort();
    assert_eq!(fs0.keys().cloned().collect::<Vec<_>>(), ks);
    use std::os::unix::fs::PermissionsExt;
    for (n, want) in fs0 {
        let p = out.join(n);
        assert_eq!(serde_json::to_value(id(&fs::read(&p).unwrap())).unwrap(), *want);
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o400);
        assert_tamper_refused(&dir, &p, &sig);
    }
    assert_tamper_refused(&dir, &out.join("freeze.json"), &sig);
    assert!(Input::load_frozen(&dir, "actual-replay", "mac", &id(&[0x55, 0x00])).is_err());
    assert!(Input::load_frozen(&dir, "wrong-run", "mac", &sig).is_err());
    assert!(Input::load_frozen(&dir, "actual-replay", "ipad", &sig).is_err());
    for (src, fs0) in [
        (&v.parent_source, &v.parent_files),
        (&v.prepared_source, &v.prepared_files),
    ] {
        for n in fs0.keys() {
            assert_tamper_refused(&dir, &dir.join(src).join(n), &sig);
        }
    }
    assert_tamper_refused(&dir, &dir.join(&v.preparation.path), &sig);
    v.recheck(&dir, r, "mac").unwrap();
    assert!(native_review::derive(&dir, "actual-replay", "mac", record().seeds, &"0".repeat(40), "dev").is_err());
    fs::set_permissions(out.join("input.json"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(out.join("input.json"), b"{}").unwrap();
    assert!(Input::load_frozen(&dir, "actual-replay", meta["platform"].as_str().unwrap(), &sig).is_err());
    assert!(
        native_review::derive(
            &dir,
            "actual-replay",
            meta["platform"].as_str().unwrap(),
            record().seeds,
            &"0".repeat(40),
            "dev"
        )
        .is_err()
    );
}
