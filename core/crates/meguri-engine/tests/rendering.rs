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
fn stored_field_positions_render_front_back_and_audio_for_sparse_and_dense_types() {
    use meguri_engine::rendering::AvTag;
    use meguri_storage::raw::SqlValue;

    for cloze in [false, true] {
        for ords in [[0, 1, 2], [2, 4, 6]] {
            let h = common::fresh();
            let front = if cloze { "{{cloze:Front}}" } else { "{{Front}}" };
            let question = format!("<div class=mg-front>{front}</div>");
            let answer = "{{FrontSide}}<hr><div class=mg-back>{{Back}}</div>";
            let nt = notetypes::add(
                &h.collection,
                notetypes::NewNotetype {
                    name: "Stored field positions".into(),
                    kind: if cloze { notetypes::NotetypeKind::Cloze } else { notetypes::NotetypeKind::Standard },
                    fields: vec!["Front".into(), "Back".into(), "Sibling".into()],
                    templates: vec![notetypes::NewTemplate {
                        name: "Face".into(),
                        front: question.clone(),
                        back: answer.into(),
                        ..Default::default()
                    }],
                    css: Some(String::new()),
                },
            )
            .unwrap()
            .value
            .id;
            let note = notes::add(
                &h.collection,
                notes::AddNote {
                    notetype_id: nt,
                    fields: vec![
                        if cloze { "{{c1::Synthetic many-field prompt}}" } else { "Synthetic many-field prompt" }.into(),
                        "Synthetic many-field answer [sound:answer.mp3]".into(),
                        "third [sound:sibling.mp3]".into(),
                    ],
                    tags: vec![],
                    deck_id: DeckId(1),
                    guid: None,
                    update_add_defaults: false,
                },
                OpSource::UserUndoable,
            )
            .unwrap()
            .value;
            // Change the persisted rows, not just an in-memory render input.
            // Descending updates avoid primary-key collisions with dense rows.
            for position in (0..ords.len()).rev() {
                common::tagging::raw(
                    &h.collection,
                    "UPDATE fields SET ord = ? WHERE ntid = ? AND ord = ?",
                    &[SqlValue::Integer(ords[position]), SqlValue::Integer(nt.0), SqlValue::Integer(position as i64)],
                );
            }
            let card = {
                let rows = h.collection.reader(ReaderRole::Query).unwrap();
                let stored = rows.notetype(nt).unwrap().unwrap();
                assert_eq!(stored.fields.iter().map(|f| i64::from(f.ord)).collect::<Vec<_>>(), ords);
                rows.cards_of_note(note.id).unwrap()[0].id
            };
            let before = common::steps::dump(&h.collection);
            let seq = h.collection.last_seq();
            for mode in [RenderMode::Preview, RenderMode::Review, RenderMode::Browser] {
                let rendered = rendering::render_card(&h.collection, card, mode, Platform::Mac).unwrap();
                assert_eq!(rendered.comparable.outcome, Outcome::Rendered);
                let q = rendered.question.unwrap();
                let a = rendered.answer.unwrap();
                if cloze {
                    assert!(q.contains("<span class=\"cloze\" data-cloze=\"Synthetic many-field prompt\""), "{q}");
                } else {
                    assert!(q.contains("<div class=mg-front>Synthetic many-field prompt</div>"), "{q}");
                }
                if mode != RenderMode::Browser {
                    assert!(a.contains("Synthetic many-field prompt"), "{a}");
                }
                assert!(a.contains("<div class=mg-back>Synthetic many-field answer "), "{a}");
                assert!(!q.contains("third") && !a.contains("third"));
                assert!(rendered.comparable.question_av.is_empty());
                if mode == RenderMode::Browser {
                    assert!(a.contains("[sound:answer.mp3]"), "{a}");
                    assert!(rendered.comparable.answer_av.is_empty());
                } else {
                    assert_eq!(rendered.comparable.answer_av, vec![AvTag::Sound { name: "answer.mp3".into() }]);
                }
                assert_eq!(rendered.media_refs, ["answer.mp3"]);
            }
            // Template overrides use the same field resolution, without AV extraction.
            let preview = rendering::render_template(&h.collection, card, &question, answer).unwrap();
            assert_eq!(preview.outcome, Outcome::Rendered);
            assert!(preview.answer.as_deref().unwrap().contains("Synthetic many-field answer [sound:answer.mp3]"));
            assert!(!preview.question.as_deref().unwrap().contains("third"));
            assert_eq!(common::steps::dump(&h.collection), before);
            assert_eq!(h.collection.last_seq(), seq);
        }
    }
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

fn configure_type_answer(h: &common::Harness, id: CardId, font: &str) {
    let font = font.to_owned();
    h.collection
        .run(
            OpSpec::named("type_answer_fixture", OpSource::SyncIncoming),
            move |ctx| {
                let card = ctx.scope().card(id)?.unwrap();
                let mut note = ctx.scope().note(card.note_id)?.unwrap();
                let mut kind = ctx.scope().notetype(note.notetype_id)?.unwrap();
                note.fields = "cat<img src=\"my file#1.jpg\">\u{1f}<b>Qáv</b>[sound:typed file.ogg]".into();
                kind.fields[1].config.font_name = font;
                kind.fields[1].config.font_size = 28;
                kind.config.css = "[[type:Back]]<hr id=answer><img src=\"css logo.png\">".into();
                kind.templates[0].config.q_format = "{{Front}}|{{type:Back}}|{{type:Front}}".into();
                kind.templates[0].config.a_format = "{{FrontSide}}<hr id=answer>{{type:Front}}".into();
                ctx.scope().put(&note)?;
                ctx.scope().put_notetype(&kind)?;
                Ok(())
            },
        )
        .unwrap();
}
#[test]
fn type_answer_stored_descriptor_whole_css_repeated_markers_and_read_only_reveal() {
    use meguri_template::type_answer::{self, ANSWER_SLOT, Mode};
    let (h, id) = fixture();
    configure_type_answer(&h, id, "Qav Serif");
    let before = common::steps::dump(&h.collection);
    let seq = h.collection.last_seq();
    for _ in 0..3 {
        let card = rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Ios).unwrap();
        let d = card.type_in.unwrap();
        assert_eq!(d.expected, "<b>Qáv</b>[sound:typed file.ogg]");
        assert_eq!(
            (d.font.as_str(), d.size, d.mode, d.case_insensitive),
            ("Qav Serif", 28, Mode::Plain, false)
        );
        let question = card.question.unwrap();
        assert_eq!(
            question
                .matches("id=typeans autocapitalize=\"off\" autocorrect=\"off\" spellcheck=\"false\"")
                .count(),
            3
        );
        assert!(question.contains("my%20file%231.jpg"));
        assert!(question.contains("css%20logo.png"));
        assert_eq!(
            card.comparable.question.as_deref(),
            Some("cat<img src=\"my file#1.jpg\">|[[type:Back]]|[[type:Front]]")
        );
        assert!(card.comparable.question_av.is_empty());
        let answer = card.answer.unwrap();
        assert_eq!(answer.matches(ANSWER_SLOT).count(), 4);
        assert_eq!(answer.matches("<hr id=answer>").count(), 4);
        let observation = rendering::review_html(&h.collection, id, "Qáv").unwrap();
        assert_eq!(observation.question.matches("id=typeans onkeypress").count(), 3);
        assert!(!observation.question.contains("autocorrect="));
        // Full reveal equals A5's independently rendered observation form.
        assert_eq!(
            answer.replace(ANSWER_SLOT, &type_answer::compare_typed_answer(&d, "Qáv")),
            observation.answer
        );
    }
    let browser = rendering::render_card(&h.collection, id, RenderMode::Browser, Platform::Mac).unwrap();
    assert!(browser.type_in.is_none());
    assert_eq!(browser.question.as_deref(), Some(" "));
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.last_seq(), seq);
}
#[test]
fn invalid_imported_font_refuses_without_partial_html_or_churn_and_raw_render_stays_available() {
    let (h, id) = fixture();
    configure_type_answer(&h, id, r"Qav\q");
    let before = common::steps::dump(&h.collection);
    let seq = h.collection.last_seq();
    assert_eq!(
        rendering::review_html(&h.collection, id, "Qáv"),
        Err(meguri_engine::EngineError::InvalidTypeAnswerReplacement)
    );
    assert_eq!(
        rendering::render_card(&h.collection, id, RenderMode::Review, Platform::Mac),
        Err(meguri_engine::EngineError::InvalidTypeAnswerReplacement)
    );
    let raw = rendering::render_card_comparable(&h.collection, id, RenderMode::Review).unwrap();
    assert_eq!(raw.comparable.outcome, Outcome::Rendered);
    assert!(raw.comparable.question.unwrap().contains("[[type:Back]]"));
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.last_seq(), seq);
}
#[test]
fn io_cards_have_exact_active_mask_audio_and_encoded_image_on_each_card() {
    use meguri_engine::rendering::AvTag;
    let h = common::fresh();
    let template =
        "{{cloze:Occlusion}}{{Image}}<div style=\"display:none\">{{cloze:A01}}{{cloze:A02}}{{cloze:A03}}</div>";
    let kind = notetypes::add(
        &h.collection,
        notetypes::NewNotetype {
            name: "IO render".into(),
            kind: notetypes::NotetypeKind::Cloze,
            fields: ["Image", "Occlusion", "A01", "A02", "A03"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            templates: vec![notetypes::NewTemplate {
                name: "Masks".into(),
                front: template.into(),
                back: template.into(),
                browser_front: String::new(),
                browser_back: String::new(),
            }],
            css: Some(String::new()),
        },
    )
    .unwrap()
    .value
    .id;
    let note = notes::add(&h.collection, notes::AddNote {
        notetype_id: kind, fields: vec![
            "<img src=\"mask image#1.png\">".into(),
            "{{c1::image-occlusion:rect:left=.1:width=.2}}{{c2::image-occlusion:ellipse:rx=.3}}{{c3::image-occlusion:polygon:points=1,2 3,4}}{{c0::image-occlusion:text:text=label}}".into(),
            "{{c1::[sound:one file.mp3]}}".into(), "{{c2::[sound:two.mp3]}}".into(), "{{c3::[sound:three.mp3]}}".into(),
        ], tags: vec![], deck_id: DeckId(1), guid: None, update_add_defaults: false,
    }, OpSource::UserUndoable).unwrap().value;
    let cards = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .cards_of_note(note.id)
        .unwrap();
    assert_eq!(cards.len(), 3);
    let before = common::steps::dump(&h.collection);
    let seq = h.collection.last_seq();
    for card in cards {
        let rendered = rendering::render_card(&h.collection, card.id, RenderMode::Review, Platform::Mac).unwrap();
        assert!(rendered.comparable.question_av.is_empty());
        let name = ["one file.mp3", "two.mp3", "three.mp3"][usize::from(card.ord)];
        assert_eq!(rendered.comparable.answer_av, vec![AvTag::Sound { name: name.into() }]);
        let q = rendered.question.unwrap();
        let a = rendered.answer.unwrap();
        assert!(q.contains("mask%20image%231.png"));
        assert_eq!(q.matches("<div class=\"cloze-inactive\"").count(), 2);
        assert_eq!(a.matches("<div class=\"cloze-highlight\"").count(), 1);
        assert!(a.contains("<div class=\"cloze\" data-ordinal=\"0\" data-shape=\"text\" data-text=\"label\" ></div>"));
        assert!(!q.contains("replay-button"));
        assert!(a.contains("<div style=\"display:none\"><span class=\"cloze\""));
        assert!(a.contains("pycmd('play:a:0')"));
    }
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.last_seq(), seq);
}
