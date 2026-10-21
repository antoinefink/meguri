//! Canonical C18 freshness additions. Source-authored; execution belongs to root.
mod common;
mod session_support;
use meguri_engine::{
    EngineError, OpSource, OpSpec, SessionId,
    rendering::{self, Platform, RenderMode},
    session::{AnswerOutcome, Head, HeadItem, PeekResult, Refusal, RenderStage, SessionEvent},
};
use meguri_model::{
    card_data::{CardData, MemoryState},
    clock::Clock,
    ids::{CardId, DeckConfigId, DeckId},
    notetype::Notetype,
    proto::{ProtoBlob, deck::DeckKind, deck_config::DeckConfigMessage},
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_sched::{answer::Rating, model::preset::PresetView};
use meguri_storage::{ReadRows, ReaderRole, RowKey, StampMode, Table, WriteScope};
use session_support::Harness;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};

fn edit(h: &Harness, f: impl FnOnce(&mut WriteScope<'_>) -> Result<(), EngineError> + Send + 'static) {
    h.collection
        .run(
            OpSpec::named("freshness fixed stamps", OpSource::SyncIncoming).stamp(StampMode::Raw),
            move |ctx| f(ctx.scope()),
        )
        .unwrap();
    h.settle();
}
fn preset(h: &Harness, f: impl FnOnce(&mut DeckConfigMessage) + Send + 'static) {
    edit(h, move |s| {
        let mut p = s.deck_config(DeckConfigId(1))?.unwrap();
        f(&mut p.config);
        s.put(&p)?;
        Ok(())
    });
}
fn kind(h: &Harness, card: CardId, f: impl FnOnce(&mut Notetype) + Send + 'static) {
    edit(h, move |s| {
        let card = s.card(card)?.unwrap();
        let note = s.note(card.note_id)?.unwrap();
        let mut nt = s.notetype(note.notetype_id)?.unwrap();
        f(&mut nt);
        s.put_notetype(&nt)?;
        Ok(())
    });
}
fn head(h: &Harness, id: SessionId) -> Head {
    let protected = common::steps::dump(&h.collection);
    let out = h.review.head(id, 3).unwrap();
    assert_eq!(common::steps::dump(&h.collection), protected, "projection is read-only");
    out
}
fn page(h: &Harness, id: SessionId, item: &HeadItem) -> Arc<rendering::RenderedCard> {
    let protected = common::steps::dump(&h.collection);
    let out = h
        .review
        .render_head(id, &[item.render_key], Platform::Mac)
        .unwrap()
        .remove(0);
    assert_eq!(
        common::steps::dump(&h.collection),
        protected,
        "render preserves every stored byte"
    );
    out
}
fn stale(h: &Harness, id: SessionId, prior: &HeadItem) -> Head {
    let protected = common::steps::dump(&h.collection);
    let AnswerOutcome::Stale { fresh_head, .. } = h
        .review
        .answer(
            id,
            prior.card_id,
            Rating::Good,
            prior.state_token,
            EpochMillis(h.clock.now_ms()),
            100,
        )
        .unwrap()
    else {
        panic!("stale prior token accepted")
    };
    assert_eq!(
        common::steps::dump(&h.collection),
        protected,
        "stale answer writes zero bytes"
    );
    fresh_head
}
fn display_template(h: &Harness) {
    kind(h, h.cards[0], |nt| {
        nt.templates[0].config.q_format = "{{Front}}|{{Tags}}|{{Type}}|{{Deck}}|{{Card}}|{{type:Back}}".into();
    });
}

#[test]
fn c5_fixed_stamps_parameters_steps_retention_ease_max_leech_cap_bury_and_stale_zero_writes() {
    let h = Harness::new(4);
    let (id, initial) = h.start();
    let mut prior = initial.items[0].clone();
    let original_card = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(prior.card_id.0))
        .unwrap();
    let stamp = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck_config(DeckConfigId(1))
        .unwrap()
        .unwrap()
        .mtime;
    for case in 0..14 {
        for version in 1..=2 {
            preset(&h, move |p| match case {
                0 => p.set_fsrs_params_6(vec![version as f32; 21]),
                1 => p.learn_steps = vec![version as f32, 11.0],
                2 => p.relearn_steps = vec![version as f32, 12.0],
                3 => p.desired_retention = 0.80 + version as f32 * 0.01,
                4 => p.initial_ease = 2.6 + version as f32 * 0.1,
                5 => p.maximum_review_interval = 100 + version,
                6 => p.leech_threshold = 10 + version,
                7 => p.cap_answer_time_to_secs = 100 + version,
                8 => p.bury_new = version == 1,
                9 => p.bury_reviews = version == 1,
                10 => p.bury_interday_learning = version == 1,
                11 => p.leech_action = if version == 1 { 0 } else { 1 },
                12 => p.learn_steps = vec![3.0, version as f32 + 30.0],
                _ => p.relearn_steps = vec![4.0, version as f32 + 30.0],
            });
            let current = head(&h, id).items[0].clone();
            assert_ne!(current.state_token, prior.state_token, "case {case}, edit {version}");
            assert_eq!(current.render_key, prior.render_key);
            assert_eq!(stale(&h, id, &prior).items[0].state_token, current.state_token);
            assert_eq!(
                h.collection
                    .reader(ReaderRole::Query)
                    .unwrap()
                    .deck_config(DeckConfigId(1))
                    .unwrap()
                    .unwrap()
                    .mtime,
                stamp
            );
            prior = current;
        }
    }
    for value in [Some(0.73), Some(0.74), None] {
        edit(&h, move |s| {
            let mut deck = s.deck(DeckId(1))?.unwrap();
            let DeckKind::Normal(n) = &mut deck.kind.kind else {
                panic!()
            };
            n.desired_retention = value;
            s.put(&deck)?;
            Ok(())
        });
        let current = head(&h, id).items[0].clone();
        assert_ne!(current.state_token, prior.state_token);
        assert_eq!(current.render_key, prior.render_key);
        stale(&h, id, &prior);
        prior = current;
    }
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Cards, &RowKey::Id(prior.card_id.0))
            .unwrap(),
        original_card
    );
    assert!(h.logs().is_empty());
}

#[test]
fn c5_missing_preset_keeps_configured_id_and_presence_even_with_factory_fallback() {
    let h = Harness::new(3);
    let (id, _) = h.start();
    let set_home = |config_id| {
        edit(&h, move |s| {
            let mut d = s.deck(DeckId(1))?.unwrap();
            let DeckKind::Normal(n) = &mut d.kind.kind else {
                panic!()
            };
            n.config_id = config_id;
            s.put(&d)?;
            Ok(())
        })
    };
    set_home(700);
    let a = head(&h, id).items[0].clone();
    set_home(701);
    let b = head(&h, id).items[0].clone();
    assert_ne!(
        a.state_token, b.state_token,
        "two missing configured ids must remain distinct"
    );
    edit(&h, |s| {
        let fallback = PresetView::for_config(s, DeckConfigId(701))?;
        assert_eq!(fallback.source().id, DeckConfigId(701));
        assert!(s.deck_config(DeckConfigId(701))?.is_none());
        let mut p = fallback.source().clone();
        p.mtime = EpochSecs(0);
        s.put(&p)?;
        Ok(())
    });
    let c = head(&h, id).items[0].clone();
    assert_eq!(b.next_states, c.next_states);
    assert_ne!(b.state_token, c.state_token, "missing versus present at mtime zero");
    assert_eq!(a.render_key, c.render_key);
    stale(&h, id, &b);
}

#[test]
fn c5_later_unsupported_list_still_refuses_with_fixed_id_and_mtime() {
    let study = Harness::new(5);
    let card_ids = study.cards.clone();
    edit(&study, move |store| {
        let root = store.deck(DeckId(1))?.unwrap();
        let DeckKind::Normal(root_settings) = &root.kind.kind else {
            panic!("fixture root requires a normal deck")
        };
        assert_eq!(root_settings.config_id, 1);
        let original = store.deck_config(DeckConfigId(1))?.unwrap();
        for (home_id, config_id) in [(43004, 4), (43009, 9), (43011, 11)] {
            let mut configuration = original.clone();
            configuration.id = DeckConfigId(config_id);
            configuration.name = format!("u43-config-{config_id}");
            store.put(&configuration)?;
            let mut home = root.clone();
            home.id = DeckId(home_id);
            home.name = if config_id == 11 {
                format!("u43-home-{home_id}")
            } else {
                format!("{}\u{1f}u43-home-{home_id}", root.name)
            };
            let DeckKind::Normal(settings) = &mut home.kind.kind else {
                panic!("fixture home requires a normal deck")
            };
            settings.config_id = config_id;
            store.put(&home)?;
        }
        assert_eq!(store.card(card_ids[0])?.unwrap().deck_id, DeckId(1));
        for (card_id, home_id) in card_ids.into_iter().skip(1).zip([43004, 43004, 43009, 43011]) {
            let mut placed = store.card(card_id)?.unwrap();
            placed.deck_id = DeckId(home_id);
            store.put(&placed)?;
        }
        Ok(())
    });

    let (session, offered) = study.start();
    let offered_card = offered
        .items
        .first()
        .expect("supported session must offer a card")
        .clone();
    let notifications = study.engine.stream().subscribe();
    let original_times: Vec<_> = {
        let snapshot = study.collection.reader(ReaderRole::Query).unwrap();
        [1, 4, 9, 11]
            .into_iter()
            .map(|number| {
                let config_id = DeckConfigId(number);
                let configuration = snapshot.deck_config(config_id).unwrap().unwrap();
                (configuration.id, configuration.mtime)
            })
            .collect()
    };
    edit(&study, |store| {
        for number in [1, 4, 9, 11] {
            let mut configuration = store.deck_config(DeckConfigId(number))?.unwrap();
            let mut encoded = configuration.config.to_bytes().into_owned();
            encoded.extend_from_slice(&[0x3a, 0x01, 0x01]);
            configuration.config = ProtoBlob::decode(&encoded).unwrap();
            store.put(&configuration)?;
        }
        Ok(())
    });

    let required_presets = vec![DeckConfigId(1), DeckConfigId(4), DeckConfigId(9)];
    let mut unsupported_count = 0;
    while let Some(delivery) = notifications.try_recv() {
        if let meguri_engine::EventKind::Extension(extension) = &delivery.kind
            && let Some(SessionEvent::PresetUnsupported { session_id, preset_ids }) =
                extension.as_any().downcast_ref::<SessionEvent>()
        {
            assert_eq!(*session_id, session);
            assert_eq!(preset_ids, &required_presets);
            unsupported_count += 1;
        }
    }
    assert!(
        unsupported_count > 0,
        "preset commit must publish a typed refusal before any head request"
    );

    let committed_rows = common::steps::dump(&study.collection);
    let unsupported = EngineError::Unsupported {
        feature: "FSRS parameters".into(),
    };
    assert_eq!(
        study
            .review
            .answer(
                session,
                offered_card.card_id,
                Rating::Easy,
                offered_card.state_token,
                EpochMillis(study.clock.now_ms()),
                100,
            )
            .unwrap_err(),
        unsupported
    );
    assert_eq!(study.review.head(session, 3).unwrap_err(), unsupported);
    match study.review.peek_first_card(DeckId(1)).unwrap() {
        PeekResult::Refused(Refusal::PresetUnsupported { preset_ids }) => {
            assert_eq!(preset_ids, required_presets);
        }
        result => panic!("studied tree must refuse unsupported presets: {result:?}"),
    }
    {
        let snapshot = study.collection.reader(ReaderRole::Query).unwrap();
        for (config_id, original_time) in original_times {
            let configuration = snapshot.deck_config(config_id).unwrap().unwrap();
            assert_eq!(configuration.id, config_id);
            assert_eq!(configuration.mtime, original_time);
        }
    }
    assert_eq!(
        common::steps::dump(&study.collection),
        committed_rows,
        "refused answer and projections must preserve every collection row"
    );
}

#[test]
fn c6_c13_twice_fixed_stamp_fields_and_duplicate_ordered_tag_tokens_refresh_actual_html() {
    let h = Harness::new(5);
    display_template(&h);
    let (id, initial) = h.start();
    let mut prior = initial.items[0].clone();
    let mut previous = page(&h, id, &prior);
    for (front, tags) in [
        ("edited once", " alpha alpha beta "),
        ("edited twice", " beta alpha alpha "),
    ] {
        let card = prior.card_id;
        edit(&h, move |s| {
            let c = s.card(card)?.unwrap();
            let mut note = s.note(c.note_id)?.unwrap();
            note.fields = format!("{front}\u{1f}answer");
            note.tags = tags.into();
            s.put(&note)?;
            Ok(())
        });
        let current = head(&h, id);
        assert_eq!(current.items[0].state_token, prior.state_token);
        assert_ne!(current.items[0].render_key, prior.render_key);
        assert_eq!(
            &current.items[1..],
            &initial.items[1..],
            "unrelated next two heads are stable"
        );
        let count = h.review.render_count();
        let rendered = page(&h, id, &current.items[0]);
        assert_eq!(h.review.render_count(), count + 1);
        assert!(rendered.question.as_deref().unwrap().contains(front));
        assert!(rendered.question.as_deref().unwrap().contains(tags.trim()));
        assert!(!Arc::ptr_eq(&previous, &rendered));
        assert!(Arc::ptr_eq(&rendered, &page(&h, id, &current.items[0])));
        assert_eq!(h.review.render_count(), count + 1);
        prior = current.items[0].clone();
        previous = rendered;
    }
}

#[test]
fn c6_c13_twice_fixed_stamp_selected_template_css_type_name_template_name_field_and_type_in_metadata() {
    let h = Harness::new(4);
    display_template(&h);
    let (id, initial) = h.start();
    let mut prior = initial.items[0].clone();
    for case in 0..10 {
        for version in 1..=2 {
            let previous_head = head(&h, id);
            let previous = page(&h, id, &prior);
            kind(&h, prior.card_id, move |nt| match case {
                0 => {
                    nt.templates[0].config.q_format = format!(
                        "{{{{Front}}}}|{{{{Tags}}}}|{{{{Type}}}}|{{{{Deck}}}}|{{{{Card}}}}|{{{{type:Back}}}} q{version}"
                    )
                }
                1 => nt.templates[0].config.a_format = format!("{{{{FrontSide}}}}/{{{{Back}}}} a{version}"),
                2 => nt.config.css = format!(".card {{ padding: {version}px; }}"),
                3 => nt.name = format!("Renamed type {version}"),
                4 => nt.templates[0].name = format!("Renamed template {version}"),
                5 => nt.fields[1].config.font_name = format!("Fresh Font {version}"),
                6 => nt.fields[1].config.font_size = 25 + version,
                7 => nt.fields[0].name = format!("RenamedFront{version}"),
                8 => {
                    nt.fields[0].ord = 1;
                    nt.fields[1].ord = 0;
                    nt.fields.swap(0, 1);
                }
                _ => nt.config.kind = if version == 1 { 1 } else { 0 },
            });
            let current = head(&h, id);
            assert_eq!(current.items[0].state_token, prior.state_token);
            assert_ne!(
                current.items[0].render_key, prior.render_key,
                "case {case}, edit {version}"
            );
            // Every note in this harness shares its type, so all dependents refresh.
            for (now, old) in current.items.iter().zip(&previous_head.items) {
                assert_eq!(now.state_token, old.state_token);
                assert_ne!(
                    now.render_key, old.render_key,
                    "shared type refreshes every dependent head"
                );
            }
            let rendered = page(&h, id, &current.items[0]);
            let direct =
                rendering::render_card(&h.collection, prior.card_id, RenderMode::Review, Platform::Mac).unwrap();
            assert_eq!(*rendered, direct, "cached package follows the actual U62 pipeline");
            if case == 5 {
                assert_eq!(rendered.type_in.as_ref().unwrap().font, format!("Fresh Font {version}"));
            }
            if case == 6 {
                assert_eq!(rendered.type_in.as_ref().unwrap().size, 25 + version);
            }
            if case <= 4 {
                assert_ne!(*rendered, *previous, "consumed edit changes the real package");
            }
            prior = current.items[0].clone();
        }
    }
}

#[test]
fn c6_c13_shared_note_refreshes_both_dependents_and_preserves_unrelated_page() {
    let h = Harness::new(5);
    let first = h.cards[0];
    let second = h.cards[1];
    edit(&h, move |s| {
        let a = s.card(first)?.unwrap();
        let mut b = s.card(second)?.unwrap();
        b.note_id = a.note_id;
        s.put(&b)?;
        Ok(())
    });
    let (id, initial) = h.start();
    let unaffected = page(&h, id, &initial.items[2]);
    edit(&h, move |s| {
        let c = s.card(first)?.unwrap();
        let mut n = s.note(c.note_id)?.unwrap();
        n.fields = "shared new text\u{1f}answer".into();
        s.put(&n)?;
        Ok(())
    });
    let current = head(&h, id);
    for i in 0..2 {
        assert_eq!(current.items[i].state_token, initial.items[i].state_token);
        assert_ne!(current.items[i].render_key, initial.items[i].render_key);
        assert!(
            page(&h, id, &current.items[i])
                .question
                .as_ref()
                .unwrap()
                .contains("shared new text")
        );
    }
    assert_eq!(current.items[2], initial.items[2]);
    assert!(Arc::ptr_eq(&unaffected, &page(&h, id, &current.items[2])));
}

#[test]
fn c6_c13_fixed_stamp_home_and_ancestor_renames_refresh_full_human_deck_name() {
    let h = Harness::new(4);
    display_template(&h);
    edit(&h, |s| {
        meguri_engine::decks::rename_in(s, DeckId(1), "Root")?;
        Ok(())
    });
    let child = meguri_engine::decks::add(&h.collection, "Root::Child".into())
        .unwrap()
        .value
        .id;
    let cards = h.cards.clone();
    edit(&h, move |s| {
        for id in cards {
            let mut c = s.card(id)?.unwrap();
            c.deck_id = child;
            s.put(&c)?;
        }
        Ok(())
    });
    let (id, initial) = h.start();
    let mut prior = initial.items[0].clone();
    for (deck, name, expected) in [
        (child, "Root::First", "Root::First"),
        (child, "Root::Second", "Root::Second"),
        (DeckId(1), "Ancestor", "Ancestor::Second"),
        (DeckId(1), "Latest", "Latest::Second"),
    ] {
        edit(&h, move |s| {
            meguri_engine::decks::rename_in(s, deck, name)?;
            Ok(())
        });
        let current = head(&h, id);
        assert_eq!(current.items[0].state_token, prior.state_token);
        assert_ne!(current.items[0].render_key, prior.render_key);
        assert!(
            page(&h, id, &current.items[0])
                .question
                .as_ref()
                .unwrap()
                .contains(expected)
        );
        prior = current.items[0].clone();
    }
}

#[test]
fn c5_c6_held_stamp_unknown_runtime_unused_and_unselected_bytes_add_no_invalidation() {
    let h = Harness::new(4);
    preset(&h, |p| p.set_fsrs_params_6(vec![1.0; 21]));
    // Add an unselected template before the session; it has no card at ord 0.
    let first = h.cards[0];
    edit(&h, move |s| {
        meguri_engine::mark_schema_change(s, &[meguri_engine::schema_gate::SchemaReason::TemplatesChanged], true)?;
        let c = s.card(first)?.unwrap();
        let n = s.note(c.note_id)?.unwrap();
        let mut nt = s.notetype(n.notetype_id)?.unwrap();
        let mut t = nt.templates[0].clone();
        t.ord = 1;
        t.name = "Other".into();
        nt.templates.push(t);
        s.put_notetype(&nt)?;
        Ok(())
    });
    let (id, initial) = h.start();
    let cached = page(&h, id, &initial.items[0]);
    for version in 1..=2 {
        let first = h.cards[0];
        edit(&h, move |s| {
            let mut p = s.deck_config(DeckConfigId(1))?.unwrap();
            p.name = format!("unused preset name {version}");
            p.usn = Usn(version);
            p.config.param_search = format!("unused optimizer metadata {version}");
            p.config.minimum_lapse_interval = 20 + version as u32;
            p.config.easy_multiplier = 2.0 + version as f32;
            let mut legacy: serde_json::Value =
                serde_json::from_str(&meguri_model::schema11::deck_config::deck_config_to_json(&p)).unwrap();
            legacy["fsrsWeights"] = serde_json::json!(vec![version as f32; 17]);
            legacy["fsrsParams5"] = serde_json::json!(vec![version as f32; 19]);
            legacy["futureFreshness"] = serde_json::json!({"kept": version});
            p = meguri_model::schema11::deck_config::deck_config_from_json(&legacy.to_string()).unwrap();
            let mut bytes = p.config.to_bytes().into_owned();
            bytes.extend_from_slice(&[0xa0, 0x1f, version as u8]);
            p.config = ProtoBlob::decode(&bytes).unwrap();
            s.put(&p)?;
            let mut unused = p.clone();
            unused.id = DeckConfigId(900);
            unused.config.learn_steps = vec![99.0 + version as f32];
            s.put(&unused)?;
            let c = s.card(first)?.unwrap();
            let n = s.note(c.note_id)?.unwrap();
            let mut nt = s.notetype(n.notetype_id)?.unwrap();
            nt.config.latex_pre = format!("unused {version}");
            let mut bytes = nt.config.to_bytes().into_owned();
            bytes.extend_from_slice(&[0xa0, 0x1f, version as u8]);
            nt.config = ProtoBlob::decode(&bytes).unwrap();
            nt.fields[0].config.sticky = version == 1;
            nt.templates[0].config.q_format_browser = format!("browser {version}");
            nt.templates[0].config.a_format_browser = format!("browser answer {version}");
            nt.templates[1].config.q_format = format!("unselected {version}");
            s.put_notetype(&nt)?;
            let mut d = s.deck(DeckId(1))?.unwrap();
            d.common.new_studied = 123 + version;
            s.put(&d)?;
            s.set_config(
                "futureRuntimePreference",
                &meguri_storage::ConfigValue::String(format!("runtime {version}")),
            )?;
            Ok(())
        });
        let now = head(&h, id);
        for (a, b) in now.items.iter().zip(&initial.items) {
            assert_eq!(a.state_token, b.state_token);
            assert_eq!(a.render_key, b.render_key);
        }
        assert!(Arc::ptr_eq(&cached, &page(&h, id, &now.items[0])));
    }
}

#[test]
fn c6_tag_separator_spelling_is_not_a_dependency_but_token_order_and_duplicates_are() {
    let h = Harness::new(3);
    display_template(&h);
    let first = h.cards[0];
    let tags = |text: &'static str| {
        edit(&h, move |s| {
            let c = s.card(first)?.unwrap();
            let mut n = s.note(c.note_id)?.unwrap();
            n.tags = text.into();
            s.put(&n)?;
            Ok(())
        })
    };
    tags(" alpha alpha beta ");
    let (id, initial) = h.start();
    let a = page(&h, id, &initial.items[0]);
    tags("\u{3000}alpha  alpha\u{3000}beta\u{3000}");
    assert_eq!(head(&h, id).items[0].render_key, initial.items[0].render_key);
    assert!(Arc::ptr_eq(&a, &page(&h, id, &initial.items[0])));
    tags("alpha beta alpha");
    let b = head(&h, id).items[0].clone();
    assert_ne!(b.render_key, initial.items[0].render_key);
    tags("alpha beta");
    assert_ne!(head(&h, id).items[0].render_key, b.render_key);
}

#[test]
fn c5_c6_flag_usn_memory_only_and_full_row_digest_remain_separate() {
    let h = Harness::new(5);
    let first = h.cards[0];
    edit(&h, move |s| {
        let mut c = s.card(first)?.unwrap();
        c.data = CardData::parse(r#"{ "extension":[1, 2] }"#);
        s.put(&c)?;
        Ok(())
    });
    let (id, initial) = h.start();
    let first = initial.items[0].card_id;
    let digest = || {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        meguri_storage::journal::digest(&r.row_image(Table::Cards, &RowKey::Id(first.0)).unwrap().unwrap())
    };
    let before = digest();
    edit(&h, move |s| {
        let mut c = s.card(first)?.unwrap();
        c.usn = Usn(777);
        s.put(&c)?;
        Ok(())
    });
    let usn = head(&h, id);
    assert_eq!(usn.items, initial.items);
    assert_ne!(before, digest(), "full-row precondition still covers usn");
    edit(&h, move |s| {
        let mut c = s.card(first)?.unwrap();
        c.flags = 9;
        s.put(&c)?;
        Ok(())
    });
    let flag = head(&h, id);
    assert_eq!(flag.items[0].state_token, initial.items[0].state_token);
    assert_ne!(flag.items[0].render_key, initial.items[0].render_key);
    let cached = page(&h, id, &flag.items[0]);
    let renders = h.review.render_count();
    let stream = h.engine.stream().subscribe();
    edit(&h, move |s| {
        let mut c = s.card(first)?.unwrap();
        c.data.memory_state = Some(MemoryState {
            stability: 3.0,
            difficulty: 5.0,
        });
        c.data.last_review_secs = Some(1_894_701_000);
        s.put(&c)?;
        Ok(())
    });
    let mut memory_only = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind
            && let Some(SessionEvent::CurrentCardChanged {
                change: meguri_engine::session::CurrentChange::MemoryOnly,
                ..
            }) = e.as_any().downcast_ref()
        {
            memory_only = true;
        }
    }
    assert!(memory_only);
    let memory = head(&h, id);
    assert_ne!(memory.items[0].state_token, flag.items[0].state_token);
    assert_eq!(memory.items[0].render_key, flag.items[0].render_key);
    assert_eq!(&memory.items[1..], &initial.items[1..]);
    assert_eq!(h.review.render_count(), renders, "memory refresh never renders HTML");
    assert!(Arc::ptr_eq(&cached, &page(&h, id, &memory.items[0])));
    let c = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .card(first)
        .unwrap()
        .unwrap();
    assert_eq!(
        c.data.memory_state,
        Some(MemoryState {
            stability: 3.0,
            difficulty: 5.0
        })
    );
    assert_eq!(c.data.unknown_keys().count(), 1);
}

#[test]
fn c6_k9_effective_defaults_platform_mode_isolation_and_error_cards_stay_displayable() {
    let h = Harness::new(3);
    let (id, initial) = h.start();
    let a = page(&h, id, &initial.items[0]);
    edit(&h, |s| {
        s.set_config(
            "hideAudioPlayButtons",
            &meguri_storage::ConfigValue::String("true".into()),
        )?;
        s.set_config("reviewExtra", &meguri_storage::ConfigValue::Bool(true))?;
        Ok(())
    });
    assert_eq!(head(&h, id).items[0].render_key, initial.items[0].render_key);
    for value in [true, false] {
        let prior = head(&h, id).items[0].clone();
        edit(&h, move |s| {
            s.set_config("hideAudioPlayButtons", &meguri_storage::ConfigValue::Bool(value))?;
            s.set_config(
                "reviewExtra",
                &meguri_storage::ConfigValue::String(format!("<b>extra {value}</b>")),
            )?;
            Ok(())
        });
        let now = head(&h, id).items[0].clone();
        assert_ne!(now.render_key, prior.render_key);
        assert_eq!(now.state_token, prior.state_token);
        let rendered = page(&h, id, &now);
        assert_eq!(rendered.hide_play_buttons, value);
        assert_eq!(rendered.review_extra, format!("<b>extra {value}</b>"));
        assert_ne!(*a, *rendered);
        assert!(
            h.review.render_head(id, &[now.render_key], Platform::Ios).is_err(),
            "native session rejects another platform's namespace"
        );
        let browser = rendering::render_card(&h.collection, now.card_id, RenderMode::Browser, Platform::Mac).unwrap();
        assert_eq!(browser.mode, RenderMode::Browser);
        assert!(
            Arc::ptr_eq(&rendered, &page(&h, id, &now)),
            "browser renders cannot populate review cache"
        );
    }
    kind(&h, h.cards[0], |nt| {
        nt.templates[0].config.q_format = "{{MissingField}}".into()
    });
    let error = head(&h, id).items[0].clone();
    let package = page(&h, id, &error);
    assert!(matches!(package.comparable.outcome, rendering::Outcome::Error { .. }));
    kind(&h, h.cards[0], |nt| nt.templates[0].config.q_format.clear());
    let blank = head(&h, id).items[0].clone();
    assert!(matches!(
        page(&h, id, &blank).comparable.outcome,
        rendering::Outcome::Blank(_)
    ));
}

fn insertion_race(stage: RenderStage) {
    let h = Harness::new(4);
    let (id, initial) = h.start();
    let first = initial.items[0].clone();
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let once = Arc::new(AtomicBool::new(false));
    let (signal, wait, armed) = (reached.clone(), release.clone(), once.clone());
    h.review.render_hook_for_testing(Some(Arc::new(move |at| {
        if at == stage && !armed.swap(true, Ordering::SeqCst) {
            signal.wait();
            wait.wait();
        }
    })));
    let service = h.review.clone();
    let old = h
        .engine
        .threads()
        .spawn("old-session-render", meguri_engine::Qos::Default, move |_| {
            service.render_head(id, &[first.render_key], Platform::Mac)
        })
        .unwrap();
    reached.wait();
    kind(&h, h.cards[0], |nt| {
        nt.templates[0].config.q_format = "newer {{Front}}".into()
    });
    let fresh = head(&h, id).items[0].clone();
    let newer = page(&h, id, &fresh);
    assert!(newer.question.as_ref().unwrap().contains("newer front"));
    release.wait();
    assert!(old.join().unwrap().is_err(), "older capture must use stale path");
    h.review.render_hook_for_testing(None);
    assert!(
        Arc::ptr_eq(&newer, &page(&h, id, &fresh)),
        "old result cannot replace new payload"
    );
}
#[test]
fn c6_race_older_snapshot_after_capture_cannot_serve_or_cache_under_newer_key() {
    insertion_race(RenderStage::Captured);
}
#[test]
fn c6_race_older_render_at_publication_cannot_replace_newer_payload() {
    insertion_race(RenderStage::Publishing);
}

#[test]
fn c13_r330_complete_metadata_refresh_emits_diff_even_when_both_keys_equal() {
    let h = Harness::new(4);
    let (id, initial) = h.start();
    let stream = h.engine.stream().subscribe();
    preset(&h, |p| {
        p.disable_autoplay = true;
        p.show_timer = true;
        p.seconds_to_show_question = 9.0;
    });
    let mut emitted = None;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind
            && let Some(SessionEvent::QueueHeadChanged { diff, .. }) = e.as_any().downcast_ref()
        {
            emitted = Some(diff.head.clone());
        }
    }
    let refreshed = emitted.expect("observer refreshes complete R330 values before explicit head read");
    assert_eq!(refreshed.items[0].state_token, initial.items[0].state_token);
    assert_eq!(refreshed.items[0].render_key, initial.items[0].render_key);
    assert!(!refreshed.items[0].home_preset.autoplay);
    assert!(refreshed.items[0].home_preset.show_timer);
    assert_eq!(refreshed.items[0].home_preset.question_seconds, 9.0);
    assert_eq!(head(&h, id).items[0], refreshed.items[0]);
}

#[test]
fn c5_required_preset_mtime_changes_before_acceptance_but_does_not_veto_c12_accepted_intent() {
    use meguri_engine::executor::catalog;
    use std::{sync::mpsc, time::Duration};
    let h = Harness::new(4);
    let (id, initial) = h.start();
    edit(&h, |s| {
        let mut p = s.deck_config(DeckConfigId(1))?.unwrap();
        p.mtime.0 += 1;
        s.put(&p)?;
        Ok(())
    });
    let current = head(&h, id);
    assert_ne!(current.items[0].state_token, initial.items[0].state_token);
    assert_eq!(current.items[0].next_states, initial.items[0].next_states);
    stale(&h, id, &initial.items[0]);
    let (started, ready) = mpsc::sync_channel(1);
    let (release, wait) = mpsc::channel();
    let writer = h.collection.submit(
        OpSpec::new(catalog::BULK_OP, OpSource::SyncIncoming)
            .stamp(StampMode::Raw)
            .declare_long_write(Duration::from_secs(3)),
        move |ctx| {
            started.send(()).unwrap();
            wait.recv().unwrap();
            let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            p.mtime.0 += 1;
            p.name = "Changed after acceptance".into();
            p.config.param_search = "unrelated optimizer metadata".into();
            let mut bytes = p.config.to_bytes().into_owned();
            bytes.extend_from_slice(&[0xa0, 0x1f, 0x01]);
            p.config = ProtoBlob::decode(&bytes).unwrap();
            ctx.scope().put(&p)?;
            Ok(())
        },
    );
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let first = &current.items[0];
    let renders = h.review.render_count();
    let accepted = h
        .review
        .answer(
            id,
            first.card_id,
            Rating::Easy,
            first.state_token,
            EpochMillis(h.clock.now_ms()),
            100,
        )
        .unwrap();
    release.send(()).unwrap();
    writer.wait().unwrap();
    assert!(matches!(accepted, AnswerOutcome::Accepted { .. }));
    h.settle();
    assert_eq!(
        h.logs().len(),
        1,
        "C12 retains accepted semantics across unrelated stamp/opaque edits"
    );
    assert_eq!(h.review.render_count(), renders, "answer and commit never render");
}

#[test]
fn c6_retains_last_eight_distinct_payloads_and_unchanged_peek_start_identity() {
    let h = Harness::new(3);
    let protected = common::steps::dump(&h.collection);
    let PeekResult::Card(peek) = h.review.peek_first_card(DeckId(1)).unwrap() else {
        panic!()
    };
    assert_eq!(common::steps::dump(&h.collection), protected);
    let (id, initial) = h.start();
    assert_eq!(peek, initial.items[0]);
    let mut pages = Vec::new();
    for v in 0..10 {
        kind(&h, h.cards[0], move |nt| {
            nt.templates[0].config.q_format = format!("{{{{Front}}}} version {v}")
        });
        let item = head(&h, id).items[0].clone();
        pages.push((item, page(&h, id, &head(&h, id).items[0])));
    }
    let renders = h.review.render_count();
    for (v, prior) in pages.iter().enumerate().take(10).skip(2) {
        kind(&h, h.cards[0], move |nt| {
            nt.templates[0].config.q_format = format!("{{{{Front}}}} version {v}")
        });
        let item = head(&h, id).items[0].clone();
        assert_eq!(item.render_key, prior.0.render_key);
        assert!(Arc::ptr_eq(&prior.1, &page(&h, id, &item)));
    }
    assert_eq!(h.review.render_count(), renders);
}

#[test]
fn c5_resolved_u46_history_changes_without_card_edits_and_keeps_render_identity() {
    let h = Harness::new(1);
    let first = h.cards[0];
    edit(&h, move |s| {
        let mut c = s.card(first)?.unwrap();
        c.card_type = meguri_model::card::CardType::Review;
        c.queue = meguri_model::card::CardQueue::Review;
        c.due = 0;
        c.interval = 3;
        c.ease_factor = 2500;
        s.put(&c)?;
        Ok(())
    });
    let (id, initial) = h.start();
    let original = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(first.0))
        .unwrap();
    let cached = page(&h, id, &initial.items[0]);
    let mut prior = initial.items[0].clone();
    for version in 1..=2 {
        edit(&h, move |s| {
            let mut row = common::revlog(first);
            row.id = meguri_model::ids::RevlogId(session_support::NOW - (3 - version) * 86_400_000);
            row.button = if version == 1 { 3 } else { 4 };
            row.interval = 3;
            row.last_interval = 1;
            s.put(&row)?;
            Ok(())
        });
        let current = head(&h, id).items[0].clone();
        assert_ne!(current.state_token, prior.state_token);
        assert_eq!(current.render_key, prior.render_key);
        assert!(Arc::ptr_eq(&cached, &page(&h, id, &current)));
        assert_eq!(
            h.collection
                .reader(ReaderRole::Query)
                .unwrap()
                .row_image(Table::Cards, &RowKey::Id(first.0))
                .unwrap(),
            original
        );
        stale(&h, id, &prior);
        prior = current;
    }
}

#[test]
fn c5_next_boundary_only_change_within_same_day_invalidates_before_acceptance() {
    let h = Harness::new(3);
    let (id, initial) = h.start();
    let first = initial.items[0].clone();
    let stored = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(first.card_id.0))
        .unwrap();
    edit(&h, |s| {
        s.set_config("rollover", &meguri_storage::ConfigValue::from(5_i64))?;
        Ok(())
    });
    let current = head(&h, id).items[0].clone();
    assert_ne!(current.state_token, first.state_token);
    assert_eq!(current.render_key, first.render_key);
    assert_eq!(
        current.next_states.current, first.next_states.current,
        "unchanged creation day"
    );
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Cards, &RowKey::Id(first.card_id.0))
            .unwrap(),
        stored
    );
    stale(&h, id, &first);
}

#[test]
fn c13_counts_only_rebuild_notifies_with_unchanged_complete_items_without_head_poll() {
    let h = Harness::new(5);
    let (_, before) = h.start();
    h.settle();
    let stream = h.engine.stream().subscribe();
    let notetype_id = {
        let rows = h.collection.reader(ReaderRole::Query).unwrap();
        let card = rows.card(h.cards[0]).unwrap().unwrap();
        rows.note(card.note_id).unwrap().unwrap().notetype_id
    };
    meguri_engine::notes::add(
        &h.collection,
        meguri_engine::notes::AddNote {
            notetype_id,
            fields: vec!["later new card".into(), "response".into()],
            tags: Vec::new(),
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    h.settle();
    let mut changed = Vec::new();
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(payload) = &event.kind
            && let Some(SessionEvent::QueueHeadChanged { diff, .. }) = payload.as_any().downcast_ref()
        {
            changed.push(diff.clone());
        }
    }
    let update = changed.last().expect("new count must arrive on the session channel");
    assert_eq!(update.head.items, before.items);
    assert_eq!(update.head.counts.new, before.counts.new + 1);
    assert_eq!(update.head.counts.learning, before.counts.learning);
    assert_eq!(update.head.counts.review, before.counts.review);
    assert_eq!(update.head.valid_until, before.valid_until);
    assert!(update.removed.is_empty());
    assert!(update.marks.iter().all(|mark| *mark == Default::default()));
}

#[test]
fn c17_empty_rollover_publishes_valid_until_only_diff_without_head_poll() {
    let h = Harness::new(0);
    let (id, before) = h.start();
    h.settle();
    let stream = h.engine.stream().subscribe();
    h.clock.set_now_ms(h.clock.now_ms() + 86_400_000);
    h.collection.check_rollover().unwrap();
    h.settle();
    h.review.application_barrier_for_testing().unwrap();
    let mut updates = Vec::new();
    let mut completions = Vec::new();
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(payload) = &event.kind {
            match payload.as_any().downcast_ref::<SessionEvent>() {
                Some(SessionEvent::QueueHeadChanged { session_id, diff }) => {
                    assert_eq!(*session_id, id);
                    updates.push(diff.head.clone());
                }
                Some(SessionEvent::Finished { session_id, state }) => {
                    assert_eq!(*session_id, id);
                    completions.push(state.clone());
                }
                _ => {}
            }
        }
    }
    let updated = updates.last().expect("rollover must publish the new validity boundary");
    assert_eq!(updated.items, before.items);
    assert_eq!(updated.counts, before.counts);
    assert!(updated.valid_until > before.valid_until);
    let actual = meguri_engine::sched::queue::QueueProvider::default()
        .deck_study_state(&h.collection, Some(DeckId(1)))
        .unwrap();
    assert!(!completions.is_empty());
    assert!(completions.iter().all(|state| *state == actual));
}
