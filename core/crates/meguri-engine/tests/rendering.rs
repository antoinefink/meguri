mod common;
use meguri_engine::{
    OpSpec, notes, notetypes,
    rendering::{self, Outcome, Platform, RenderMode},
};
use meguri_model::{
    changes::OpSource,
    ids::{CardId, DeckId},
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, StampMode};
fn fixture() -> (common::Harness, CardId) {
    let h = common::fresh();
    let id = notetypes::add(
        &h.collection,
        notetypes::NewNotetype {
            name: "Render".into(),
            kind: notetypes::NotetypeKind::Standard,
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![notetypes::NewTemplate {
                name: "Face".into(),
                front: "{{Front}}[sound:q]".into(),
                back: "{{FrontSide}}/{{Back}}[sound:a]".into(),
                browser_front: " ".into(),
                browser_back: "{{FrontSide}}B".into(),
            }],
            css: Some(".card{}".into()),
        },
    )
    .unwrap()
    .value
    .id;
    let note = notes::add(
        &h.collection,
        notes::AddNote {
            notetype_id: id,
            fields: vec!["猫".into(), "裏".into()],
            tags: vec!["MaRkEd".into()],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value;
    let card = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .cards_of_note(note.id)
        .unwrap()[0]
        .id;
    (h, card)
}
#[test]
fn stored_pipeline_display_browser_and_read_only() {
    let (h, id) = fixture();
    h.collection
        .run(OpSpec::named("render_config", OpSource::SyncIncoming), move |ctx| {
            let mut card = ctx.scope().card(id)?.unwrap();
            card.flags = 9;
            ctx.scope().put(&card)?;
            ctx.scope()
                .set_config("reviewExtra", &ConfigValue::String("<script>extra</script>".into()))?;
            ctx.scope()
                .set_config("hideAudioPlayButtons", &ConfigValue::Bool(true))?;
            Ok(())
        })
        .unwrap();
    let before = common::steps::dump(&h.collection);
    let seq = h.collection.last_seq();
    let rendered = rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Ios).unwrap();
    assert_eq!(rendered.comparable.question.as_deref(), Some("猫[anki:play:q:0]"));
    assert_eq!(
        rendered.comparable.answer.as_deref(),
        Some("猫[anki:play:q:0]/裏[anki:play:a:0]")
    );
    assert_eq!(rendered.question.as_deref(), Some("<style>.card{}</style>猫"));
    assert_eq!((rendered.flag, rendered.marked), (1, true));
    assert_eq!(rendered.review_extra, "<script>extra</script>");
    let browser = rendering::render_card(&h.collection, id, RenderMode::Browser, Platform::Mac).unwrap();
    assert_eq!(browser.question.as_deref(), Some(" "));
    assert_eq!(browser.answer.as_deref(), Some("B"));
    assert_eq!(browser.comparable.outcome, Outcome::Rendered);
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.last_seq(), seq);
}
#[test]
fn exact_source_witness_handles_same_mtime_raw_sync_and_undo() {
    let (h, id) = fixture();
    let render = || {
        rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Mac)
            .unwrap()
            .comparable
            .question
            .unwrap()
    };
    assert_eq!(render(), "猫[anki:play:q:0]");
    let original_mtime = {
        let rows = h.collection.reader(ReaderRole::Query).unwrap();
        let card = rows.card(id).unwrap().unwrap();
        let note = rows.note(card.note_id).unwrap().unwrap();
        rows.notetype(note.notetype_id).unwrap().unwrap().mtime
    };
    for text in ["{{Front}}new", "{{Front}}latest"] {
        h.collection
            .run(
                OpSpec::named("sync_template", OpSource::UserUndoable)
                    .undo_step("Edit Template", meguri_engine::StepKind::Other)
                    .stamp(StampMode::Raw),
                move |ctx| {
                    let card = ctx.scope().card(id)?.unwrap();
                    let note = ctx.scope().note(card.note_id)?.unwrap();
                    let mut kind = ctx.scope().notetype(note.notetype_id)?.unwrap();
                    assert_eq!(kind.mtime, original_mtime);
                    kind.templates[0].config.q_format = text.into();
                    ctx.scope().put_notetype(&kind)?;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(render(), text.replace("{{Front}}", "猫"));
    }
    h.collection.undo(meguri_engine::UndoScope::Global).unwrap();
    assert_eq!(render(), "猫new");
    h.collection.redo(meguri_engine::UndoScope::Global).unwrap();
    assert_eq!(render(), "猫latest");
}
#[test]
fn missing_card_is_not_a_template_error() {
    let h = common::fresh();
    assert!(matches!(
        rendering::render_card(&h.collection, CardId(-1), RenderMode::Review, Platform::Mac),
        Err(meguri_engine::EngineError::NotFound {
            table: meguri_storage::Table::Cards,
            ..
        })
    ));
}

#[test]
fn render_uses_one_snapshot_during_atomic_note_css_config_edits() {
    let (h, id) = fixture();
    fn edit(c: &meguri_engine::Collection, id: CardId, version: &str) {
        let version = version.to_owned();
        c.run(
            OpSpec::named("atomic_render_edit", OpSource::SyncIncoming),
            move |ctx| {
                let card = ctx.scope().card(id)?.unwrap();
                let mut note = ctx.scope().note(card.note_id)?.unwrap();
                let mut kind = ctx.scope().notetype(note.notetype_id)?.unwrap();
                note.fields = format!("{version}\u{1f}back");
                kind.config.css = version.clone();
                ctx.scope().put(&note)?;
                ctx.scope().put_notetype(&kind)?;
                ctx.scope().set_config("reviewExtra", &ConfigValue::String(version))?;
                Ok(())
            },
        )
        .unwrap();
    }
    edit(&h.collection, id, "0");
    let other = h.collection.clone();
    let worker = std::thread::spawn(move || {
        for n in 1..150 {
            edit(&other, id, &n.to_string());
        }
    });
    for _ in 0..150 {
        let card = rendering::render_card(&h.collection, id, RenderMode::Preview, Platform::Mac).unwrap();
        assert_eq!(card.css, card.review_extra);
        assert_eq!(
            card.comparable.question.unwrap(),
            format!("{}[anki:play:q:0]", card.css)
        );
    }
    worker.join().unwrap();
}
#[test]
fn replacement_rejects_old_handle_and_validates_cached_template_against_new_file() {
    let (h, id) = fixture();
    let initial = rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Mac).unwrap();
    assert_eq!(initial.comparable.question.as_deref(), Some("猫[anki:play:q:0]"));
    // A different collection deliberately reuses the exact ids.
    let (replacement, other_id) = fixture();
    assert_eq!(id, other_id);
    replacement
        .collection
        .run(
            OpSpec::named("replacement_template", OpSource::SyncIncoming),
            move |ctx| {
                let card = ctx.scope().card(id)?.unwrap();
                let note = ctx.scope().note(card.note_id)?.unwrap();
                let mut kind = ctx.scope().notetype(note.notetype_id)?.unwrap();
                kind.templates[0].config.q_format = "{{Front}}replacement".into();
                ctx.scope().put_notetype(&kind)?;
                Ok(())
            },
        )
        .unwrap();
    replacement.engine.close().unwrap();
    let path = replacement.path.clone();
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let current = exclusive
        .replace(move |live| {
            std::fs::copy(path, live)
                .map(|_| ())
                .map_err(|e| meguri_engine::EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    assert!(matches!(
        rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Mac),
        Err(meguri_engine::EngineError::StaleGeneration { .. })
    ));
    assert_eq!(
        rendering::render_card(&current, id, RenderMode::Review, Platform::Mac)
            .unwrap()
            .comparable
            .question
            .as_deref(),
        Some("猫replacement")
    );
}

#[test]
fn page_config_types_and_stored_tag_boundaries_are_exact() {
    let (h, id) = fixture();
    for (stored, marked) in [
        (" x\tmarked ", false),
        (" x\u{a0}marked ", false),
        (" x\u{3000}MaRkEd ", true),
    ] {
        let stored = stored.to_owned();
        h.collection
            .run(
                OpSpec::named("raw_render_tags", OpSource::SyncIncoming).stamp(StampMode::Raw),
                move |ctx| {
                    let card = ctx.scope().card(id)?.unwrap();
                    let mut note = ctx.scope().note(card.note_id)?.unwrap();
                    note.tags = stored;
                    ctx.scope().put(&note)?;
                    ctx.scope()
                        .set_config("hideAudioPlayButtons", &ConfigValue::Integer(1))?;
                    ctx.scope().set_config("reviewExtra", &ConfigValue::Bool(true))?;
                    Ok(())
                },
            )
            .unwrap();
        let rendered = rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Mac).unwrap();
        assert_eq!(rendered.marked, marked);
        assert!(!rendered.hide_play_buttons);
        assert!(rendered.review_extra.is_empty());
    }
}
