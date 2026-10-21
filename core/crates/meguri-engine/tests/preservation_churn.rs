//! U36: real engine operations over copies of the preservation fixture.
//! U40 must keep these invariants when composing the scheduler's lifecycle ports.
mod common;

use common::{Harness, NOW_MS, open_config};
use meguri_engine::{
    Collection, Engine, EngineConfig, OpSource, Ports, deck_config, decks, flags, media::MediaService, notes,
    sched::answer, tags, undo::UndoScope,
};
use meguri_model::{
    card::{CardQueue, CardType},
    clock::Clock,
    ids::{CardId, DeckConfigId, DeckId, NoteId},
    proto::deck::DeckKind,
};
use meguri_sched::{
    answer::Rating,
    states::{engine::State, fsrs_glue::FsrsCache},
};
use meguri_storage::{ReadRows, ReaderRole, raw::RawRows};
use meguri_testkit::dump::preservation::{assert_changes_allowed, pending, raw_dump};
use serde_json::Value;

fn fixture() -> Harness {
    Harness::open_fixture("../preservation/kitchen-sink.anki2", Ports::default())
}
fn dump(c: &Collection) -> Value {
    let reader = c.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    raw_dump(|sql| snapshot.execute_raw(sql, &[])).unwrap()
}
fn pending_counts(c: &Collection) -> [i64; 12] {
    let reader = c.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    pending(|sql| snapshot.execute_raw(sql, &[])).unwrap()
}
fn schema(c: &Collection) -> RawRows {
    c.reader(ReaderRole::Query).unwrap().execute_raw("SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT IN ('sqlite_stat1', 'sqlite_stat4') ORDER BY type, name", &[]).unwrap()
}
fn relaunch(h: &mut Harness) {
    h.engine.close().unwrap();
    h.engine = Engine::new(EngineConfig::default()).unwrap();
    h.collection = h.engine.open_raw(&h.path, open_config(&h.clock)).unwrap();
}

#[test]
fn a_thousand_real_engine_ops_and_twenty_relaunches_preserve_schema_and_foreign_rows() {
    let mut h = fixture();
    let original_schema = schema(&h.collection);
    let original = dump(&h.collection);
    for _ in 0..10 {
        relaunch(&mut h);
    }
    let mid = common::notes::kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let options = tags::RunOptions::default();
    // Ten successful public engine calls per iteration. Answering is U43's later leg.
    for i in 0..100 {
        h.clock.set_now_ms(NOW_MS + (i + 1) * 60_000);
        let added = notes::add(
            &h.collection,
            common::notes::input(mid, &[&format!("own-{i}"), "before"]),
            OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        assert_eq!(added.cards.len(), 1);
        notes::update(
            &h.collection,
            notes::UpdateNote {
                id: added.id,
                fields: vec![format!("own-{i}"), "after".into()],
                tags: vec![],
            },
            OpSource::UserUndoable,
        )
        .unwrap();
        tags::add_to_notes(&h.collection, &[added.id], "meguri-churn", &options).unwrap();
        flags::set_flag(&h.collection, &added.cards, 3, &options).unwrap();
        decks::add(&h.collection, format!("Churn {i}")).unwrap();
        let preset = deck_config::add(&h.collection, format!("Churn {i}")).unwrap().value;
        deck_config::rename(&h.collection, preset, format!("Churn renamed {i}")).unwrap();
        assert_eq!(
            notes::delete(&h.collection, vec![added.id], OpSource::UserUndoable)
                .unwrap()
                .unwrap()
                .value,
            1
        );
        h.collection.undo(UndoScope::Global).unwrap();
        h.collection.redo(UndoScope::Global).unwrap();
    }
    for _ in 0..10 {
        relaunch(&mut h);
    }
    assert_eq!(schema(&h.collection), original_schema);
    let after = dump(&h.collection);
    // User operations may add their own rows. Existing fixture records keep all bytes.
    for table in [
        "notes",
        "cards",
        "notetypes",
        "fields",
        "templates",
        "decks",
        "deck_config",
        "tags",
    ] {
        for (key, row) in original[table].as_object().unwrap() {
            assert_eq!(&after[table][key], row, "{table}/{key}");
        }
    }
}

#[test]
fn a_hundred_idle_relaunches_across_days_and_static_font_ensures_create_no_churn() {
    let mut h = fixture();
    let before = dump(&h.collection);
    let original_schema = schema(&h.collection);
    let name = "_meguri-preservation-font.woff2";
    let font = b"Meguri synthetic preservation font";
    let (counter, entries, modified) = {
        let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
        assert_eq!(media.store().ensure_static_media(name, font).unwrap(), name);
        let changes = media.store().pending().unwrap();
        assert_eq!(changes.len(), 1);
        assert!(media.store().acknowledge(name, changes[0].change_counter).unwrap());
        (
            media.store().change_counter().unwrap(),
            media.store().entries().unwrap(),
            std::fs::metadata(media.store().media_dir().join(name))
                .unwrap()
                .modified()
                .unwrap(),
        )
    };
    assert_eq!(pending_counts(&h.collection), [0; 12]);
    for i in 0..100 {
        h.clock.set_now_ms(NOW_MS + (i / 13) * 86_400_000 + i * 60_000);
        h.collection.check_rollover().unwrap();
        assert_eq!(pending_counts(&h.collection), [0; 12]);
        assert_changes_allowed(&before, &dump(&h.collection), &[]).unwrap();
        assert_eq!(schema(&h.collection), original_schema);
        relaunch(&mut h);
        let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
        assert_eq!(media.store().ensure_static_media(name, font).unwrap(), name);
        assert!(media.store().pending().unwrap().is_empty());
        assert_eq!(media.store().change_counter().unwrap(), counter);
        assert_eq!(media.store().entries().unwrap(), entries);
        assert_eq!(
            std::fs::metadata(media.store().media_dir().join(name))
                .unwrap()
                .modified()
                .unwrap(),
            modified
        );
        assert_eq!(pending_counts(&h.collection), [0; 12]);
        assert_changes_allowed(&before, &dump(&h.collection), &[]).unwrap();
        assert_eq!(schema(&h.collection), original_schema);
    }
    assert!(h.clock.now_ms() > NOW_MS + 7 * 86_400_000);
}

#[test]
fn editing_one_fixture_note_changes_only_its_fields_and_stamps_and_no_op_edit_changes_nothing() {
    let h = fixture();
    let before = dump(&h.collection);
    let mut ids: Vec<i64> = before["notes"]
        .as_object()
        .unwrap()
        .keys()
        .map(|id| id.parse().unwrap())
        .collect();
    ids.sort();
    let id = NoteId(ids[1]); // N9: Vocab's second note.
    let note = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(id)
        .unwrap()
        .unwrap();
    let mut fields: Vec<_> = note.field_values().map(str::to_owned).collect();
    fields[1] = "to drink; to swallow".into();
    let edit = notes::UpdateNote {
        id,
        fields,
        tags: note.tags.split_whitespace().map(str::to_owned).collect(),
    };
    h.clock.set_now_ms(NOW_MS + 60_000);
    assert!(
        notes::update(&h.collection, edit.clone(), OpSource::UserUndoable)
            .unwrap()
            .value
            .written
    );
    let after = dump(&h.collection);
    let allowed = [
        "/col/1/mod".into(),
        format!("/notes/{}/flds", id.0),
        format!("/notes/{}/mod", id.0),
        format!("/notes/{}/usn", id.0),
    ];
    assert_changes_allowed(&before, &after, &allowed.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
    let counts = pending_counts(&h.collection);
    assert_eq!(counts[0], 1);
    assert_eq!(counts[10], 1);
    assert!(
        !notes::update(&h.collection, edit, OpSource::UserUndoable)
            .unwrap()
            .value
            .written
    );
    assert_changes_allowed(&after, &dump(&h.collection), &[]).unwrap();
}

#[test]
fn two_hundred_supported_fixture_answers_over_twenty_days_preserve_foreign_data() {
    // N10/N11: an own-test supported leg, NOT the ten-card Steady reference
    // case. Use the pre-generated public fixture unchanged, on Harness's copy.
    let source = common::fixture("../preservation/kitchen-sink.anki2");
    let source_bytes = std::fs::read(&source).unwrap();
    let h = fixture();
    h.collection.check_rollover().unwrap();
    let original = dump(&h.collection);
    let original_schema = schema(&h.collection);
    let ids = [CardId(1893488400014), CardId(1893488400015), CardId(1893488400016)];
    let math = DeckId(1893488400002);
    let (initial_reps, original_deck, preset_bytes, config_bytes) = {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        let deck = r.deck(math).unwrap().unwrap();
        let DeckKind::Normal(normal) = &deck.kind.kind else {
            panic!("Math must stay normal")
        };
        assert_eq!(normal.config_id, 1893488400001);
        let legacy = r.deck_config(DeckConfigId(normal.config_id)).unwrap().unwrap();
        assert_eq!(legacy.config.fsrs_params_5().len(), 19);
        assert!(legacy.config.fsrs_params_5().iter().all(|p| p.is_finite()));
        assert!(legacy.config.fsrs_params_6().is_empty());
        assert!(!legacy.config.has_later_fsrs_parameters());
        assert!(serde_json::from_slice::<Value>(legacy.config.other()).unwrap()["addonLegacy"].is_object());
        assert_eq!(
            legacy.config.leech_action, 1,
            "the fixture permits real tag-only leeches"
        );
        assert_eq!(legacy.config.leech_threshold, 8);
        assert_eq!(
            deck.common
                .unknown_fields()
                .iter()
                .map(|f| f.bytes())
                .collect::<Vec<_>>(),
            vec![&[0x48, 0x01][..]],
        );
        assert_eq!(
            deck.kind.unknown_fields().iter().map(|f| f.bytes()).collect::<Vec<_>>(),
            vec![&[0x18, 0x05][..]],
        );
        let reps = ids.map(|id| {
            let card = r.card(id).unwrap().unwrap();
            assert_eq!(card.deck_id, math);
            assert_eq!(card.original_deck_id, DeckId(0));
            assert_ne!(card.queue, CardQueue::Suspended);
            assert_eq!(
                card.card_type,
                if id == ids[2] { CardType::New } else { CardType::Review }
            );
            card.reps
        });
        assert_eq!(
            r.card(CardId(1893488400017)).unwrap().unwrap().queue,
            CardQueue::Suspended
        );
        (
            reps,
            deck,
            r.execute_raw("SELECT * FROM deck_config ORDER BY id", &[]).unwrap(),
            r.execute_raw("SELECT * FROM config ORDER BY key", &[]).unwrap(),
        )
    };
    let mut models = FsrsCache::default();
    let mut answer_counts = [0u32; 3];
    let mut ratings = [0usize; 4];
    let mut days = std::collections::BTreeSet::new();
    let mut logs = std::collections::BTreeSet::new();
    let mut leech_notes = std::collections::BTreeSet::new();
    let mut leech_answers = 0;
    let mut tag_writes = 0;
    let mut previous = original.clone();
    for i in 0..200usize {
        // Ten Grade Now answers per day. Ninety minutes between a card's
        // appearances clears Legacy's ten-minute relearning step, including fuzz.
        h.clock
            .set_now_ms(NOW_MS + (i / 10) as i64 * 86_400_000 + (i % 10) as i64 * 1_800_000);
        h.collection.check_rollover().unwrap();
        let before = dump(&h.collection); // after mandatory maintenance
        assert_changes_allowed(&previous, &before, &[]).unwrap();
        let slot = i % ids.len();
        let id = ids[slot];
        let (card, deck, note) = {
            let r = h.collection.reader(ReaderRole::Query).unwrap();
            let card = r.card(id).unwrap().unwrap();
            let note = r.note(card.note_id).unwrap().unwrap();
            (card, r.deck(math).unwrap().unwrap(), note)
        };
        assert!(matches!(
            card.queue,
            CardQueue::New | CardQueue::Learning | CardQueue::Review | CardQueue::InterdayLearning
        ));
        assert_eq!(card.original_deck_id, DeckId(0));
        if card.queue == CardQueue::Learning {
            assert!(i64::from(card.due) <= h.clock.now_ms() / 1000);
        }
        let prediction = answer::predict(&h.collection, id, &mut models, None).unwrap();
        assert_eq!(prediction.answered_at.0, h.clock.now_ms());
        assert_eq!(prediction.today, h.collection.today().unwrap());
        assert_eq!(prediction.next_day_at, h.collection.next_day_at().unwrap());
        assert!(prediction.next_day_at > h.clock.now_ms() / 1000);
        assert_eq!(prediction.today, 14 + (i / 10) as u32);
        days.insert(prediction.today);
        // Repeated real lapses/graduations on c15 reach the actual leech/tag
        // path. Other cards exercise Hard/Good/Easy; no row is force-unsuspended.
        let rating = if slot == 0 {
            if card.card_type == CardType::Review {
                Rating::Again
            } else {
                Rating::Easy
            }
        } else {
            [Rating::Hard, Rating::Good, Rating::Easy][(i / 3) % 3]
        };
        let elapsed_ms = 1_000 + (i % 10) as u64 * 100;
        let request = prediction.answer(rating, elapsed_ms);
        let leeched = match request.chosen.state {
            State::Review(r) | State::Relearning { review: r, .. } => r.leeched,
            _ => false,
        };
        let today = request.today;
        let preview = request.chosen.revlog;
        let result = answer::answer(&h.collection, request).unwrap();
        assert_eq!(result.value.leech, leeched);
        assert!(!result.value.suspended);
        assert!(result.value.buried.is_empty(), "Formula notes have no siblings");
        let log_id = result.value.revlog_id;
        assert!(
            logs.insert(log_id.0),
            "each production answer inserts its own revlog row"
        );
        assert!(before["revlog"].get(&log_id.0.to_string()).is_none());
        answer_counts[slot] += 1;
        ratings[rating as usize - 1] += 1;
        let after = dump(&h.collection);
        let mut allowed = vec!["/col/1/mod".to_owned(), format!("/revlog/{}", log_id.0)];
        for column in [
            "mod", "usn", "type", "queue", "due", "ivl", "factor", "reps", "lapses", "left", "data",
        ] {
            allowed.push(format!("/cards/{}/{column}", id.0));
        }
        for column in ["common", "mtime_secs", "usn"] {
            allowed.push(format!("/decks/{}/{column}", math.0));
        }
        let already_tagged = note
            .tags
            .split_whitespace()
            .any(|tag| tag.eq_ignore_ascii_case("leech"));
        if leeched {
            leech_answers += 1;
            leech_notes.insert(note.id.0.to_string());
            if !already_tagged {
                tag_writes += 1;
                for column in ["tags", "mod", "usn"] {
                    allowed.push(format!("/notes/{}/{column}", note.id.0));
                }
            }
        }
        assert_changes_allowed(&before, &after, &allowed.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        let written = r.card(id).unwrap().unwrap();
        assert_eq!(written.reps, initial_reps[slot] + answer_counts[slot]);
        assert_eq!(written.mtime.0, h.clock.now_ms() / 1000);
        assert_eq!(written.usn.0, -1);
        assert_eq!(written.data.last_review_secs, Some(h.clock.now_ms() / 1000));
        assert!(written.data.memory_state.is_some());
        assert_eq!(written.data.custom_data(), card.data.custom_data());
        assert_eq!(
            written.data.unknown_keys().collect::<Vec<_>>(),
            card.data.unknown_keys().collect::<Vec<_>>()
        );
        let log = r.revlog_entry(log_id).unwrap().unwrap();
        assert_eq!(log.card_id, id);
        assert_eq!(log.button, rating as u8);
        assert_eq!(
            (log.interval, log.last_interval, log.ease_factor),
            (preview.interval, preview.last_interval, preview.factor)
        );
        assert_eq!(log.kind as u8, preview.kind);
        assert!(
            log.ease_factor > 0,
            "FSRS difficulty is recorded, never SM-2's zero factor"
        );
        assert_eq!(log.answer_millis, elapsed_ms as u32);
        assert_eq!(log.usn.0, -1);
        // common is the only rewritten blob. Its exact expected encoding
        // changes counters alone, retaining all opaque and unused fields.
        let mut expected_common = deck.common.clone();
        if expected_common.last_day_studied != today {
            expected_common.last_day_studied = today;
            expected_common.new_studied = 0;
            expected_common.review_studied = 0;
            expected_common.milliseconds_studied = 0;
        }
        match card.queue {
            CardQueue::New => expected_common.new_studied += 1,
            CardQueue::Review | CardQueue::InterdayLearning => expected_common.review_studied += 1,
            _ => {}
        }
        expected_common.milliseconds_studied += elapsed_ms as i32;
        let written_deck = r.deck(math).unwrap().unwrap();
        assert_eq!(written_deck.common.to_bytes(), expected_common.to_bytes());
        assert_eq!(
            written_deck.common.unknown_fields(),
            original_deck.common.unknown_fields()
        );
        assert_eq!(written_deck.common.other(), original_deck.common.other());
        assert_eq!(written_deck.kind.original(), original_deck.kind.original());
        assert_eq!(written_deck.kind.unknown_fields(), original_deck.kind.unknown_fields());
        assert_eq!(written_deck.mtime.0, h.clock.now_ms() / 1000);
        assert_eq!(written_deck.usn.0, -1);
        if leeched {
            let tagged = r.note(note.id).unwrap().unwrap();
            assert!(tagged.tags.split_whitespace().any(|tag| tag == "leech"));
            if !already_tagged {
                assert_eq!(tagged.mtime.0, h.clock.now_ms() / 1000);
                assert_eq!(tagged.usn.0, -1);
            }
        }
        assert_eq!(r.col().unwrap().modified.0, h.clock.now_ms());
        previous = after;
    }
    assert_eq!(answer_counts, [67, 67, 66]);
    assert_eq!(answer_counts.iter().sum::<u32>(), 200, "200 real production answers");
    assert_eq!(logs.len(), 200);
    assert_eq!(days.len(), 20, "twenty distinct scheduler days");
    assert!(ratings.iter().all(|count| *count > 0));
    assert!(leech_answers > 1, "exercise first and repeated leeches");
    assert_eq!(tag_writes, 1, "the first actual leech adds the tag once");
    assert_eq!(
        previous["revlog"].as_object().unwrap().len(),
        original["revlog"].as_object().unwrap().len() + 200
    );
    for (key, row) in original["revlog"].as_object().unwrap() {
        assert_eq!(&previous["revlog"][key], row, "imported revlog/{key}");
    }
    for table in [
        "deck_config",
        "config",
        "notetypes",
        "fields",
        "templates",
        "tags",
        "graves",
    ] {
        assert_eq!(previous[table], original[table], "protected {table}");
    }
    for (key, row) in original["cards"].as_object().unwrap() {
        if !ids.iter().any(|id| id.0.to_string() == *key) {
            assert_eq!(
                &previous["cards"][key], row,
                "unselected card/{key}, including suspended c18 and filtered cards"
            );
        }
    }
    for (key, row) in original["notes"].as_object().unwrap() {
        if !leech_notes.contains(key) {
            assert_eq!(&previous["notes"][key], row, "non-leech note/{key}");
        }
    }
    for (key, row) in original["decks"].as_object().unwrap() {
        if *key != math.0.to_string() {
            assert_eq!(
                &previous["decks"][key], row,
                "unstudied deck/{key}, including Languages"
            );
        }
    }
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        r.execute_raw("SELECT * FROM deck_config ORDER BY id", &[]).unwrap(),
        preset_bytes
    );
    assert_eq!(
        r.execute_raw("SELECT * FROM config ORDER BY key", &[]).unwrap(),
        config_bytes
    );
    assert_eq!(schema(&h.collection), original_schema);
    assert_eq!(
        std::fs::read(source).unwrap(),
        source_bytes,
        "the original pre-generated fixture is untouched"
    );
}

#[test]
fn unchanged_steady_fixture_refuses_prediction_without_writes_or_repair() {
    let source = common::fixture("../preservation/kitchen-sink.anki2");
    let source_bytes = std::fs::read(&source).unwrap();
    let h = fixture();
    h.collection.check_rollover().unwrap();
    let before = dump(&h.collection);
    let original_schema = schema(&h.collection);
    {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        let steady = r.deck_config(DeckConfigId(1893488400000)).unwrap().unwrap();
        assert!(steady.config.has_later_fsrs_parameters());
        assert!(serde_json::from_slice::<Value>(steady.config.other()).unwrap()["fsrsParams7"].is_array());
        assert!(
            steady
                .config
                .unknown_fields()
                .iter()
                .any(|f| f.number() == 7 && f.bytes() == [0x3a, 4, 0, 0, 0x80, 0x40])
        );
    }
    let mut models = FsrsCache::default();
    // c2 is new, c13 review: both keep their original Steady home preset.
    // This does not activate or relabel preservation/review/answers as passing.
    for id in [CardId(1893488400001), CardId(1893488400012)] {
        assert!(matches!(
            answer::predict(&h.collection, id, &mut models, None),
            Err(meguri_engine::EngineError::Unsupported { feature }) if feature == "unsupported FSRS parameters"
        ));
        assert_changes_allowed(&before, &dump(&h.collection), &[]).unwrap();
    }
    assert_eq!(schema(&h.collection), original_schema);
    assert_eq!(
        std::fs::read(source).unwrap(),
        source_bytes,
        "no recipe or fixture repair"
    );
}
