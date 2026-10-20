//! C16: list semantics, typed cells and an entirely read-only query.
mod common;
use meguri_engine::{
    OpSpec,
    browse::{CardExcerpt, Question, card_excerpts},
    notes, notetypes,
};
use meguri_model::{
    changes::OpSource,
    ids::{CardId, DeckId},
};
use meguri_storage::{ReadRows, ReaderRole, StampMode};
use meguri_template::{ErrorKind, av_tags::AvSide};

fn fixture(cloze: bool, front: &str, browser: &str, back: &str, fields: &[&str]) -> (common::Harness, CardId) {
    let h = common::fresh();
    let kind = notetypes::add(
        &h.collection,
        notetypes::NewNotetype {
            name: "Excerpt".into(),
            kind: if cloze {
                notetypes::NotetypeKind::Cloze
            } else {
                notetypes::NotetypeKind::Standard
            },
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![notetypes::NewTemplate {
                name: "Face".into(),
                front: front.into(),
                back: "{{Back}}".into(),
                browser_front: browser.into(),
                browser_back: back.into(),
            }],
            css: Some(".card{color:red}".into()),
        },
    )
    .unwrap()
    .value
    .id;
    let note = notes::add(
        &h.collection,
        notes::AddNote {
            notetype_id: kind,
            fields: fields.iter().map(|v| (*v).to_owned()).collect(),
            tags: vec![],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value;
    let id = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .cards_of_note(note.id)
        .unwrap()[0]
        .id;
    (h, id)
}
fn text(id: CardId, value: &str) -> CardExcerpt {
    CardExcerpt::Found {
        card_id: id,
        question: Question::Text(value.into()),
        deck: "Default".into(),
    }
}
fn raw(h: &common::Harness, sql: &str) {
    common::tagging::raw(&h.collection, sql, &[]);
}
#[test]
fn browser_formats_media_and_whitespace_use_cell_text() {
    for (browser, front, expected) in [
        ("{{Back}} ({{Front}})", "Baum", "tree (Baum)"),
        ("", "<b>猫</b><br>[sound:neko.mp3]", "猫 🔉neko.mp3🔉"),
        ("", "<div>犬</div><img src=\"inu.jpg\">", "犬 inu.jpg"),
        (
            "",
            "ねこ [anki:tts lang=ja_JP]猫[/anki:tts] &amp; 犬",
            "ねこ 💬猫💬 & 犬",
        ),
        ("", "[anki:tts]hi[/anki:tts][anki:tts][/anki:tts]", "💬hi💬💬💬"),
        ("  ", "Baum", ""),
        ("{{Back}}", "Baum", "tree"),
        ("{{FrontSide}}", "Baum", ""),
    ] {
        let (h, id) = fixture(false, "{{Front}}", browser, "{{FrontSide}}{{Back}}", &[front, "tree"]);
        assert_eq!(card_excerpts(&h.collection, &[id]).unwrap(), vec![text(id, expected)]);
    }
    let (h, id) = fixture(false, "{{Front}}?", "{{Back}}", "", &["Baum", ""]);
    assert_eq!(card_excerpts(&h.collection, &[id]).unwrap(), vec![text(id, "")]);
    let (h, id) = fixture(false, "{{Front}}", "", "", &["Baum", "tree"]);
    raw(&h, "UPDATE notes SET flds = char(31)");
    assert_eq!(card_excerpts(&h.collection, &[id]).unwrap(), vec![text(id, "")]);
}
#[test]
fn cloze_numbers_and_missing_cloze_are_typed_without_a_notice() {
    let (h, id) = fixture(
        true,
        "{{cloze:Front}}",
        "",
        "",
        &["{{c1::犬}} と {{c2::猫::ねこ}}<br>[sound:neko.mp3]", ""],
    );
    let cards = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .cards_after(None, 10)
        .unwrap();
    let second = cards.iter().find(|c| c.ord == 1).unwrap().id;
    assert_eq!(
        card_excerpts(&h.collection, &[second, id]).unwrap(),
        vec![
            text(second, "犬 と [ねこ] 🔉neko.mp3🔉"),
            text(id, "[...] と 猫 🔉neko.mp3🔉")
        ]
    );
    raw(&h, "UPDATE cards SET ord = 8");
    let result = card_excerpts(&h.collection, &[id]).unwrap();
    assert_eq!(
        result,
        vec![CardExcerpt::Found {
            card_id: id,
            question: Question::MissingCloze {
                kind: meguri_template::card::BlankKind::MissingCloze,
                text: "犬 と 猫 🔉neko.mp3🔉".into()
            },
            deck: "Default".into()
        }]
    );
}
#[test]
fn full_browser_outcome_includes_front_and_back_template_errors() {
    for (front, back, side) in [
        ("{{Absent}}", "{{Back}}", AvSide::Question),
        ("{{Front}}", "{{Absent}}", AvSide::Answer),
    ] {
        let (h, id) = fixture(false, "{{Front}}", front, back, &["ok", "back"]);
        assert_eq!(
            card_excerpts(&h.collection, &[id]).unwrap(),
            vec![CardExcerpt::Found {
                card_id: id,
                question: Question::TemplateError {
                    kind: ErrorKind::UnknownField,
                    side
                },
                deck: "Default".into()
            }]
        );
    }
}
#[test]
fn empty_duplicates_signed_extremes_missing_and_deleted_keep_their_places() {
    let (h, id) = fixture(false, "{{Front}}", "", "", &["ok", "back"]);
    assert_eq!(card_excerpts(&h.collection, &[]).unwrap(), vec![]);
    let ids = [CardId(i64::MAX), id, CardId(i64::MIN), id];
    assert_eq!(
        card_excerpts(&h.collection, &ids).unwrap(),
        vec![
            CardExcerpt::Missing { card_id: ids[0] },
            text(id, "ok"),
            CardExcerpt::Missing { card_id: ids[2] },
            text(id, "ok")
        ]
    );
    raw(&h, "DELETE FROM cards");
    assert_eq!(
        card_excerpts(&h.collection, &[id]).unwrap(),
        vec![CardExcerpt::Missing { card_id: id }]
    );
}
#[test]
fn missing_required_records_are_damaged_but_decode_errors_fail_the_whole_list() {
    for sql in [
        "UPDATE cards SET did=9999",
        "UPDATE cards SET odid=9999",
        "UPDATE cards SET ord=8",
        "UPDATE notes SET mid=9999",
        "DELETE FROM notes",
    ] {
        let (h, id) = fixture(false, "{{Front}}", "", "", &["ok", "back"]);
        raw(&h, sql);
        assert_eq!(
            card_excerpts(&h.collection, &[CardId(-1), id]).unwrap(),
            vec![
                CardExcerpt::Missing { card_id: CardId(-1) },
                CardExcerpt::Damaged { card_id: id }
            ]
        );
    }
    let (h, id) = fixture(false, "{{Front}}", "", "", &["ok", "back"]);
    raw(&h, "UPDATE cards SET queue=99");
    assert!(card_excerpts(&h.collection, &[CardId(-1), id]).is_err());
}
#[test]
fn nonfiltered_current_deck_still_includes_home_even_with_zero_original_due() {
    let (h, id) = fixture(false, "{{Front}}/{{Deck}}/{{Subdeck}}", "", "", &["ok", "back"]);
    let home = meguri_engine::decks::add(&h.collection, "Japanese::Mining".into())
        .unwrap()
        .value
        .id;
    raw(&h, &format!("UPDATE cards SET odid={}, odue=0", home.0));
    assert_eq!(
        card_excerpts(&h.collection, &[id]).unwrap(),
        vec![CardExcerpt::Found {
            card_id: id,
            question: Question::Text("ok/Japanese::Mining/Mining".into()),
            deck: "Default (Japanese::Mining)".into()
        }]
    );
}
#[test]
fn reads_leave_every_table_history_events_and_files_unchanged_and_release_reader() {
    let (h, id) = fixture(false, "{{Front}}", "", "", &["[latex]x[/latex] [sound:a.mp3]", "back"]);
    // Raw rollover barrier settles automatic open work before the witnesses.
    h.collection
        .run(
            OpSpec::named("excerpt_barrier", OpSource::Rollover).stamp(StampMode::Raw),
            |_| Ok(()),
        )
        .unwrap();
    let before = common::steps::dump(&h.collection);
    let undo = h.collection.peek_undo(meguri_engine::UndoScope::Global).unwrap();
    let seq = h.collection.last_seq();
    let files = || {
        let mut names = std::fs::read_dir(h.dir.path())
            .unwrap()
            .map(|p| p.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    let before_files = files();
    assert_eq!(
        card_excerpts(&h.collection, &[id, id]).unwrap(),
        vec![text(id, "[latex]x[/latex] 🔉a.mp3🔉"); 2]
    );
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.peek_undo(meguri_engine::UndoScope::Global).unwrap(), undo);
    assert_eq!(h.collection.last_seq(), seq);
    assert_eq!(files(), before_files);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.card(id).unwrap().is_some());
    drop(reader);
    h.engine.close().unwrap();
}
#[test]
fn one_list_sees_one_snapshot_during_atomic_edits() {
    let (h, id) = fixture(false, "{{Front}}", "", "", &["0", "back"]);
    let c = h.collection.clone();
    let worker = std::thread::spawn(move || {
        for n in 1..40 {
            c.run(OpSpec::named("excerpt_edit", OpSource::SyncIncoming), move |ctx| {
                let card = ctx.scope().card(id)?.unwrap();
                let mut note = ctx.scope().note(card.note_id)?.unwrap();
                note.fields = format!("{n}\u{1f}back");
                ctx.scope().put(&note)?;
                Ok(())
            })
            .unwrap();
        }
    });
    for _ in 0..40 {
        let rows = card_excerpts(&h.collection, &vec![id; 50]).unwrap();
        assert_eq!(rows.len(), 50);
        assert!(rows.iter().all(|r| r == &rows[0]));
    }
    worker.join().unwrap();
}
#[test]
fn replacement_rejects_whole_stale_calls_including_empty() {
    let (h, id) = fixture(false, "{{Front}}", "", "", &["ok", "back"]);
    let replacement = common::fresh();
    replacement.engine.close().unwrap();
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    let path = replacement.path.clone();
    exclusive
        .replace(move |live| {
            std::fs::copy(path, live)
                .map(|_| ())
                .map_err(|e| meguri_engine::EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    for ids in [vec![], vec![id, CardId(-1)]] {
        assert!(matches!(
            card_excerpts(&h.collection, &ids),
            Err(meguri_engine::EngineError::StaleGeneration { .. })
        ));
    }
}

#[test]
fn storage_and_notetype_decode_faults_abort_instead_of_returning_partial_records() {
    for sql in ["DROP TABLE notes", "UPDATE notetypes SET config=x'ff'"] {
        let (h, id) = fixture(false, "{{Front}}", "", "", &["ok", "back"]);
        raw(&h, sql);
        assert!(card_excerpts(&h.collection, &[CardId(-1), id]).is_err());
    }
}
