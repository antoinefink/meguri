//! The production deck writer, opened as an archive, as SQLite and by the
//! production importer. All fixture text and media are generated here.
use super::apkg_import::{dump, entry, import};
use meguri_engine::{OpSource, OpSpec};
use meguri_model::{
    card::{CardQueue, CardType},
    ids::{CardId, DeckConfigId, DeckId, NotetypeId, RevlogId},
    proto::deck::DeckKind,
    revlog::{RevlogEntry, RevlogKind},
    usn::Usn,
};
use meguri_package::{
    CollectionPackageWriteContext, CollectionPackageWriteError as Error, DeckExportOptions, Finalisation,
    preflight_deck_export_from_live, write_deck_export_from_live,
};
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason, StructureReport, TypeChangeReport};
use meguri_storage::{ReadRows, ReaderRole, Table, atomic_file::Purpose, space::StatvfsSpace};
use meguri_testkit::normal_support::{Fixture, NOW};
use std::{
    fs,
    path::{Path, PathBuf},
};

const PRIVATE: &str = "unrelated-export-fixture-marker-7f3981";
fn seeded() -> Fixture {
    let f = Fixture::new(3);
    let cards = f.cards.clone();
    let ntid = f.notetype;
    f.collection
        .run(
            OpSpec::named("deck export fixture", OpSource::Maintenance),
            move |ctx| {
                let scope = ctx.scope();
                for (id, name) in [
                    (100, "Root"),
                    (101, "Root\u{1f}Child"),
                    (102, "Root\u{1f}Empty"),
                    (200, "Sibling"),
                    (300, PRIVATE),
                ] {
                    let mut deck = meguri_engine::decks::new_deck(name.into());
                    deck.id = DeckId(id);
                    if let DeckKind::Normal(n) = &mut deck.kind.kind {
                        n.config_id = 50;
                        n.extend_new = 9;
                    }
                    scope.put(&deck)?;
                }
                let mut preset = meguri_engine::deck_config::defaults();
                preset.id = DeckConfigId(50);
                preset.name = "Scoped preset".into();
                preset.config.set_fsrs_params_6(vec![0.4, 0.7]);
                scope.put(&preset)?;
                let mut nt = scope.notetype(ntid)?.unwrap();
                let before = LayoutShape::of(&nt)?;
                nt.config.css = ".card {background: url(_style.png)}".into();
                nt.templates[0].config.q_format = "<script src=_script.js></script>{{Front}}".into();
                let mut second = nt.templates[0].clone();
                second.ord = 1;
                second.name = "Other face".into();
                second.config.id = None;
                nt.templates.push(second);
                let after = LayoutShape::of(&nt)?;
                // Report both authored changes before writing either the layout
                // or the note's new type. The second template has no old origin.
                let mut report = StructureReport::default();
                report
                    .reasons
                    .extend([StructureReason::TemplatesChanged, StructureReason::NotesNotetypeChanged]);
                report.layouts.insert(
                    ntid.0,
                    LayoutReport {
                        before,
                        after,
                        field_map: vec![0, 1],
                        template_map: vec![0, -1],
                    },
                );
                report.type_changes.insert(
                    (ntid.0, 9000),
                    TypeChangeReport {
                        before,
                        after,
                        field_map: vec![0, 1],
                        template_map: vec![0, -1],
                    },
                );
                let structure = scope.start_structure_report(report)?;
                scope.put_notetype(&nt)?;
                let mut extra = nt.clone();
                extra.id = NotetypeId(9000);
                extra.name = PRIVATE.into();
                extra.config.css = PRIVATE.into();
                scope.put_notetype(&extra)?;
                for (index, id) in cards.iter().enumerate() {
                    let mut card = scope.card(*id)?.unwrap();
                    card.deck_id = DeckId([100, 101, 300][index]);
                    card.card_type = CardType::Review;
                    card.queue = CardQueue::Review;
                    card.due = 17;
                    card.interval = 12;
                    card.reps = 5;
                    card.flags = 6;
                    card.data = meguri_model::card_data::CardData::parse(
                        r#"{"pos":3,"s":1.2,"d":2.3,"dr":0.8,"extra":"kept"}"#,
                    );
                    scope.put(&card)?;
                    let mut note = scope.note(card.note_id)?.unwrap();
                    note.fields = match index {
                        0 => "target <img src=referenced.png>[sound:empty.mp3][latex]x^2[/latex]\u{1f}back".into(),
                        1 => "child <img src=é.png> <img src=REFERENCED.PNG>\u{1f}back".into(),
                        _ => PRIVATE.into(),
                    };
                    if index == 2 {
                        note.notetype_id = extra.id;
                    }
                    note.tags = "  b Marked leech a a leech::x ".into();
                    note.flags = 99;
                    note.data = "unused note data".into();
                    scope.put(&note)?;
                    scope.put(&RevlogEntry {
                        id: RevlogId(NOW - 100 + index as i64),
                        card_id: card.id,
                        usn: Usn(0),
                        button: 3,
                        interval: 12,
                        last_interval: 8,
                        ease_factor: 2500,
                        answer_millis: 1234,
                        kind: RevlogKind::Review,
                    })?;
                    if index == 0 {
                        let mut sibling = card.clone();
                        sibling.id = CardId(NOW + 500);
                        sibling.deck_id = DeckId(200);
                        sibling.ord = 1;
                        scope.put(&sibling)?;
                    }
                }
                scope.complete_structure_report(structure)?;
                scope.set_config("unrelatedExportFixture", &PRIVATE.into())?;
                scope.set_config("nextPos", &777_i64.into())?;
                scope.set_config("creationOffset", &(-60_i64).into())?;
                scope.add_grave(8787, meguri_model::grave::GraveKind::Note, Usn(0))?;
                Ok(())
            },
        )
        .unwrap();
    // Leave removed content in the live SQLite allocation history. The export
    // must author rows into a fresh file rather than copy and prune pages.
    let selected = f.cards[0];
    let deleted = f
        .collection
        .run(
            OpSpec::named("deleted export fixture", OpSource::Maintenance),
            move |ctx| {
                let scope = ctx.scope();
                let card = scope.card(selected)?.unwrap();
                let mut note = scope.note(card.note_id)?.unwrap();
                note.id = meguri_model::ids::NoteId(NOW + 900);
                note.guid = "deleted-export-fixture".into();
                note.fields = PRIVATE.repeat(4096);
                scope.put(&note)?;
                Ok(note.id)
            },
        )
        .unwrap()
        .value;
    f.collection
        .run(
            OpSpec::named("remove export fixture", OpSource::Maintenance),
            move |ctx| {
                ctx.scope().remove_note(deleted)?;
                Ok(())
            },
        )
        .unwrap();
    let media = f.collection.path().parent().unwrap().join("collection.media");
    fs::create_dir_all(&media).unwrap();
    f.host.media().unwrap().add("referenced.png", b"referenced").unwrap();
    for (name, bytes) in [
        ("referenced.png", b"referenced".as_slice()),
        ("empty.mp3", b"".as_slice()),
        ("é.png", b"unicode".as_slice()),
        ("_style.png", b"style".as_slice()),
        ("_script.js", b"script".as_slice()),
        (
            "latex-7046d961a8144b7b2c2da6066849a9f889ff2ac9.png",
            b"latex".as_slice(),
        ),
        ("unreferenced.png", PRIVATE.as_bytes()),
    ] {
        fs::write(media.join(name), bytes).unwrap();
    }
    // Unrelated unsafe entries do not belong to a deck export inventory.
    #[cfg(unix)]
    std::os::unix::fs::symlink("unreferenced.png", media.join("unrelated-link")).unwrap();
    f
}
fn write(f: &Fixture, options: DeckExportOptions, name: &str) -> PathBuf {
    let path = f.dir.path().join(name);
    let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
    let context = CollectionPackageWriteContext {
        collection_directory: f.collection.path().parent().unwrap(),
        purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate,
        clock: f.clock.as_ref(),
        random: f.random.as_ref(),
        space: &StatvfsSpace,
        is_cancelled: &|| false,
    };
    let plan = preflight_deck_export_from_live(&reader, &path, &context, 100, options).unwrap();
    assert_eq!(plan.note_count(), 2);
    assert_eq!(plan.card_count(), 3);
    assert_eq!(plan.suggested_name(), "Root.apkg");
    let written = write_deck_export_from_live(&reader, &path, &context, plan).unwrap();
    written.source_validation.validate(&|| false).unwrap();
    path
}
fn decoded(path: &Path, directory: &Path) -> PathBuf {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    assert_eq!(entry(&mut archive, "meta"), [8, 3]);
    let db = zstd::decode_all(entry(&mut archive, "collection.anki21b").as_slice()).unwrap();
    assert!(!db.windows(PRIVATE.len()).any(|w| w == PRIVATE.as_bytes()));
    let target = directory.join("decoded.anki2");
    fs::write(&target, db).unwrap();
    target
}
fn verify_media(path: &Path, enabled: bool) {
    use sha1::{Digest, Sha1};
    fn varint(input: &mut &[u8]) -> u64 {
        let mut out = 0;
        let mut shift = 0;
        loop {
            let byte = input[0];
            *input = &input[1..];
            out |= u64::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return out;
            }
            shift += 7;
            assert!(shift < 64);
        }
    }
    fn bytes<'a>(input: &mut &'a [u8]) -> &'a [u8] {
        let n = varint(input) as usize;
        let (value, tail) = input.split_at(n);
        *input = tail;
        value
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
    let map = zstd::decode_all(entry(&mut archive, "media").as_slice()).unwrap();
    let expected = [
        ("REFERENCED.PNG", b"referenced".as_slice()),
        ("_script.js", b"script".as_slice()),
        ("_style.png", b"style".as_slice()),
        ("empty.mp3", b"".as_slice()),
        (
            "latex-7046d961a8144b7b2c2da6066849a9f889ff2ac9.png",
            b"latex".as_slice(),
        ),
        ("referenced.png", b"referenced".as_slice()),
        ("é.png", b"unicode".as_slice()),
    ];
    let mut input = map.as_slice();
    for (index, (name, payload)) in expected.iter().take(if enabled { 7 } else { 0 }).enumerate() {
        assert_eq!(varint(&mut input), 10);
        let mut record = bytes(&mut input);
        assert_eq!(varint(&mut record), 10);
        assert_eq!(bytes(&mut record), name.as_bytes());
        assert_eq!(varint(&mut record), 16);
        assert_eq!(varint(&mut record), payload.len() as u64);
        assert_eq!(varint(&mut record), 26);
        assert_eq!(bytes(&mut record), Sha1::digest(payload).as_slice());
        assert!(record.is_empty());
        assert_eq!(
            zstd::decode_all(entry(&mut archive, &index.to_string()).as_slice()).unwrap(),
            *payload
        );
    }
    assert!(input.is_empty());
    assert_eq!(archive.len(), if enabled { 11 } else { 4 });
}
#[test]
fn deck_scope_options_archive_rows_and_fresh_import() {
    for scheduling in [false, true] {
        for presets in [false, true] {
            for media in [false, true] {
                let f = seeded();
                let before = dump(&f);
                let path = write(
                    &f,
                    DeckExportOptions {
                        include_scheduling: scheduling,
                        include_presets: presets,
                        include_media: media,
                    },
                    "scoped.apkg",
                );
                verify_media(&path, media);
                assert_eq!(dump(&f), before);
                let scratch = tempfile::tempdir().unwrap();
                let db = decoded(&path, scratch.path());
                let opened =
                    meguri_storage::deck_package::open_deck_candidate(&db, f.clock.as_ref(), f.random.as_ref())
                        .unwrap();
                let rows = opened.writer();
                assert_eq!(rows.row_count(Table::Notes).unwrap(), 2);
                assert_eq!(rows.row_count(Table::Cards).unwrap(), 3);
                assert_eq!(rows.row_count(Table::Revlog).unwrap(), if scheduling { 2 } else { 0 });
                assert_eq!(rows.row_count(Table::Notetypes).unwrap(), 1);
                assert_eq!(rows.notetype(f.notetype).unwrap().unwrap().templates.len(), 2);
                let sibling = rows.card(CardId(NOW + 500)).unwrap().unwrap();
                assert_eq!(sibling.ord, 1);
                assert_eq!(sibling.deck_id, DeckId(200));
                assert_eq!(sibling.note_id, rows.card(f.cards[0]).unwrap().unwrap().note_id);
                assert_eq!(rows.row_count(Table::Graves).unwrap(), 0);
                assert_eq!(rows.row_count(Table::Tags).unwrap(), 0);
                assert!(rows.deck(DeckId(101)).unwrap().is_some());
                assert!(rows.deck(DeckId(200)).unwrap().is_some());
                assert!(rows.deck(DeckId(102)).unwrap().is_none());
                assert!(rows.deck(DeckId(300)).unwrap().is_none());
                for key in ["curModel", "unrelatedExportFixture", "rollover", "localOffset"] {
                    assert!(rows.config_value(key).unwrap().is_none(), "{key}");
                }
                assert_eq!(rows.config_value("nextPos").unwrap().unwrap().as_i64(), Some(1));
                assert_eq!(
                    rows.config_value("creationOffset").unwrap().unwrap().as_i64(),
                    Some(-60)
                );
                for note in rows.notes_after(None, 10).unwrap() {
                    assert_eq!(note.flags, 0);
                    assert!(note.data.is_empty());
                    assert_eq!(
                        note.tags,
                        if scheduling {
                            " b Marked leech a a leech::x "
                        } else {
                            " b a a leech::x "
                        }
                    );
                }
                for card in rows.cards_after(None, 10).unwrap() {
                    assert_eq!(
                        card.card_type,
                        if scheduling { CardType::Review } else { CardType::New }
                    );
                    assert_eq!(card.reps, if scheduling { 5 } else { 0 });
                    assert_eq!(card.due, if scheduling { 17 } else { 3 });
                    assert_eq!(card.data.memory_state.is_some(), scheduling);
                    assert!(card.data.to_text().contains("extra"));
                }
                let deck = rows.deck(DeckId(100)).unwrap().unwrap();
                let DeckKind::Normal(normal) = &deck.kind.kind else {
                    panic!("normal")
                };
                assert_eq!(normal.config_id, if presets { 50 } else { 1 });
                assert_eq!(normal.extend_new, if scheduling { 9 } else { 0 });
                assert_eq!(rows.deck_config(DeckConfigId(50)).unwrap().is_some(), presets);
                if presets {
                    assert_eq!(
                        rows.deck_config(DeckConfigId(50))
                            .unwrap()
                            .unwrap()
                            .config
                            .fsrs_params_6()
                            .is_empty(),
                        !scheduling
                    );
                }
                opened.close().unwrap();
                let target = Fixture::new(0);
                let session = super::apkg_import::inspect(&target, &path).unwrap();
                assert_eq!(session.facts().media_files, if media { 7 } else { 0 });
                drop(session);
                let result = import(
                    &target,
                    &path,
                    meguri_package::apkg::ImportOptions {
                        with_scheduling: scheduling,
                        with_deck_configs: presets,
                        update_notetypes: meguri_package::apkg::UpdateCondition::Always,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(result.counts.notes_added, 2);
                assert_eq!(result.counts.cards_added, 3);
                let files = super::apkg_import::media_files(&target);
                assert!(!files.contains_key("unreferenced.png"));
                if media {
                    assert_eq!(files.get("referenced.png").unwrap(), b"referenced");
                    assert_eq!(files.get("_script.js").unwrap(), b"script");
                }
            }
        }
    }
}

#[test]
fn changed_media_cancellation_and_retained_alias_preserve_output() {
    for mode in 0..3 {
        let f = seeded();
        let path = f.dir.path().join("failure.apkg");
        let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let check = || cancel.load(std::sync::atomic::Ordering::SeqCst);
        let context = CollectionPackageWriteContext {
            collection_directory: f.collection.path().parent().unwrap(),
            purpose: Purpose::Export,
            finalisation: Finalisation::ExclusiveCreate,
            clock: f.clock.as_ref(),
            random: f.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &check,
        };
        let mut plan = preflight_deck_export_from_live(
            &reader,
            &path,
            &context,
            100,
            DeckExportOptions {
                include_scheduling: true,
                include_presets: true,
                include_media: true,
            },
        )
        .unwrap();
        let media = f
            .collection
            .path()
            .parent()
            .unwrap()
            .join("collection.media/referenced.png");
        match mode {
            0 => fs::write(&media, b"rewritten!").unwrap(),
            1 => cancel.store(true, std::sync::atomic::Ordering::SeqCst),
            _ => {
                let retained = f.dir.path().join("retained.apkg");
                fs::hard_link(&media, &retained).unwrap();
                assert!(plan.protect_retained_outputs(std::slice::from_ref(&retained), &check).is_err());
                assert_eq!(fs::read(retained).unwrap(), b"referenced");
                continue;
            }
        }
        let error = write_deck_export_from_live(&reader, &path, &context, plan)
            .err()
            .unwrap();
        assert!(matches!(error, Error::ExportSourceChanged | Error::Cancelled));
        assert!(!path.exists());
        assert!(
            !fs::read_dir(context.collection_directory).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".meguri-stage-"))
        );
    }
}

#[test]
fn selection_uses_a_pinned_view_while_writer_commits() {
    let f = seeded();
    let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    let before = snapshot.select_deck_export(100, true, true, NOW, &|| false).unwrap();
    let card = f.cards[0];
    f.collection
        .run(
            OpSpec::named("move after snapshot", OpSource::Maintenance),
            move |ctx| {
                let mut card = ctx.scope().card(card)?.unwrap();
                card.deck_id = DeckId(300);
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    let after = snapshot.select_deck_export(100, true, true, NOW, &|| false).unwrap();
    assert_eq!(before.notes, after.notes);
    assert_eq!(before.cards, after.cards);
    drop(snapshot);
    let fresh = reader
        .begin_snapshot()
        .unwrap()
        .select_deck_export(100, true, true, NOW, &|| false)
        .unwrap();
    assert_eq!(fresh.notes.len(), 1);
}
#[test]
fn missing_deck_refuses_before_creating_anything() {
    let f = seeded();
    let path = f.dir.path().join("missing.apkg");
    let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
    let context = CollectionPackageWriteContext {
        collection_directory: f.collection.path().parent().unwrap(),
        purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate,
        clock: f.clock.as_ref(),
        random: f.random.as_ref(),
        space: &StatvfsSpace,
        is_cancelled: &|| false,
    };
    let before = super::apkg_import::listing(&f);
    let result = preflight_deck_export_from_live(
        &reader,
        &path,
        &context,
        987654,
        DeckExportOptions {
            include_scheduling: true,
            include_presets: true,
            include_media: true,
        },
    );
    assert!(matches!(result, Err(Error::DeckNotFound { id: 987654 })));
    assert!(!path.exists());
    assert_eq!(before, super::apkg_import::listing(&f));
}

#[test]
fn missing_preset_refuses_only_when_presets_are_requested_and_empty_deck_is_valid() {
    let f = seeded();
    f.collection
        .run(OpSpec::named("missing export preset", OpSource::Maintenance), |ctx| {
            ctx.scope().remove_deck_config(DeckConfigId(50))?;
            Ok(())
        })
        .unwrap();
    let path = f.dir.path().join("missing-preset.apkg");
    let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
    let context = CollectionPackageWriteContext {
        collection_directory: f.collection.path().parent().unwrap(),
        purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate,
        clock: f.clock.as_ref(),
        random: f.random.as_ref(),
        space: &StatvfsSpace,
        is_cancelled: &|| false,
    };
    let options = DeckExportOptions {
        include_scheduling: true,
        include_presets: true,
        include_media: false,
    };
    assert!(
        matches!(preflight_deck_export_from_live(&reader,&path,&context,100,options),Err(Error::ExportNeedsCheckDatabase))
    );
    assert!(!path.exists());
    assert!(
        preflight_deck_export_from_live(
            &reader,
            &path,
            &context,
            100,
            DeckExportOptions {
                include_presets: false,
                ..options
            }
        )
        .is_ok()
    );
    let empty = f.dir.path().join("empty.apkg");
    let plan = preflight_deck_export_from_live(&reader, &empty, &context, 102, options).unwrap();
    assert_eq!(plan.note_count(), 0);
    write_deck_export_from_live(&reader, &empty, &context, plan).unwrap();
    let target = Fixture::new(0);
    let session = super::apkg_import::inspect(&target, &empty).unwrap();
    assert_eq!(session.facts().notes, 0);
    assert_eq!(session.facts().cards, 0);
}

#[test]
fn subtree_selection_compares_complete_folded_names_and_literal_wildcards() {
    for (parent, child) in [("Straße", "STRASSE"), ("STRASSE", "Straße"), ("100%_", "100%_")] {
        let f = Fixture::new(6);
        let cards = f.cards.clone();
        let names = [parent.to_owned(), format!("{child}\u{1f}Child"), format!("{child}\u{1f}Child\u{1f}Grandchild"),
            format!("{child}!\u{1f}Adjacent"), format!("{child}2\u{1f}Prefix"), "100XX\u{1f}Wildcard".into()];
        f.collection.run(OpSpec::named("folded export scope", OpSource::Maintenance), move |ctx| {
            for (index, (name, card)) in names.iter().zip(&cards).enumerate() {
                let mut deck = meguri_engine::decks::new_deck(name.clone());
                deck.id = DeckId(100 + index as i64);
                ctx.scope().put(&deck)?;
                let mut card = ctx.scope().card(*card)?.unwrap();
                card.deck_id = deck.id;
                ctx.scope().put(&card)?;
            }
            Ok(())
        }).unwrap();
        let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
        let snapshot = reader.begin_snapshot().unwrap();
        let selected = snapshot.select_deck_export(100, true, false, NOW, &|| false).unwrap();
        let mut expected_cards: Vec<_> = f.cards[..3].iter().map(|id| id.0).collect();
        expected_cards.sort();
        assert_eq!(selected.cards, expected_cards);
        assert_eq!(selected.notes.len(), 3);
    }
}

#[test]
fn preflight_and_write_keep_original_membership_and_payloads_with_or_without_media() {
    for media in [false, true] {
        let f = seeded();
        let path = f.dir.path().join("pinned.apkg");
        let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
        let context = CollectionPackageWriteContext {
            collection_directory: f.collection.path().parent().unwrap(), purpose: Purpose::Export,
            finalisation: Finalisation::ExclusiveCreate, clock: f.clock.as_ref(), random: f.random.as_ref(),
            space: &StatvfsSpace, is_cancelled: &|| false,
        };
        let original = reader.note(reader.card(f.cards[1]).unwrap().unwrap().note_id).unwrap().unwrap();
        let options = DeckExportOptions { include_scheduling: true, include_presets: false, include_media: media };
        let plan = preflight_deck_export_from_live(&reader, &path, &context, 100, options).unwrap();
        assert_eq!(plan.note_count(), 2);
        assert_eq!(plan.card_count(), 3);
        let moved_out = f.cards[1];
        let moved_in = f.cards[2];
        f.clock.step_ms(1000);
        // Swap notes without changing the counts. A counts-only assertion
        // would miss a writer that selects again, especially without media.
        f.collection.run(OpSpec::named("move after export preflight", OpSource::Maintenance), move |ctx| {
            for (id, deck) in [(moved_out, 300), (moved_in, 100)] {
                let mut card = ctx.scope().card(id)?.unwrap();
                card.deck_id = DeckId(deck);
                ctx.scope().put(&card)?;
            }
            let mut note = ctx.scope().note(original.id)?.unwrap();
            note.fields = "changed after preflight\u{1f}later back".into();
            ctx.scope().put(&note)?;
            Ok(())
        }).unwrap();
        f.clock.step_ms(1000);
        let output = write_deck_export_from_live(&reader, &path, &context, plan).unwrap();
        assert_eq!(output.deck_counts, Some(meguri_package::DeckExportCounts {
            notes: 2, cards: 3, media: if media { 7 } else { 0 },
        }));
        verify_media(&path, media);
        let scratch = tempfile::tempdir().unwrap();
        let db = decoded(&path, scratch.path());
        let opened = meguri_storage::deck_package::open_deck_candidate(&db, f.clock.as_ref(), f.random.as_ref()).unwrap();
        let rows = opened.writer();
        assert_eq!(rows.row_count(Table::Notes).unwrap(), 2);
        assert_eq!(rows.row_count(Table::Cards).unwrap(), 3);
        assert_eq!(rows.note(original.id).unwrap().unwrap().fields, original.fields);
        assert_eq!(rows.card(moved_out).unwrap().unwrap().deck_id, DeckId(101));
        assert!(rows.card(moved_in).unwrap().is_none());
        assert_eq!(output.written.snapshot_modified, rows.col().unwrap().modified);
        opened.close().unwrap();
        // The consumed plan released its transaction; a new view sees the move.
        let fresh = reader.begin_snapshot().unwrap();
        assert_eq!(fresh.card(moved_out).unwrap().unwrap().deck_id, DeckId(300));
        assert_ne!(output.written.snapshot_modified, fresh.col().unwrap().modified);
    }
}

#[test]
fn preflight_plan_refuses_another_reader_directory_or_destination() {
    for mode in 0..3 {
        let f = seeded();
        let path = f.dir.path().join("bound.apkg");
        let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
        let mut context = CollectionPackageWriteContext {
            collection_directory: f.collection.path().parent().unwrap(), purpose: Purpose::Export,
            finalisation: Finalisation::ExclusiveCreate, clock: f.clock.as_ref(), random: f.random.as_ref(),
            space: &StatvfsSpace, is_cancelled: &|| false,
        };
        let options = DeckExportOptions { include_scheduling: true, include_presets: false, include_media: false };
        let plan = preflight_deck_export_from_live(&reader, &path, &context, 100, options).unwrap();
        let other = f.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(reader.native_identity(), other.native_identity());
        let different_destination = f.dir.path().join("different.apkg");
        let (caller, destination) = match mode {
            0 => (&*other, &path),
            1 => {
                context.collection_directory = f.dir.path();
                (&*reader, &path)
            }
            _ => (&*reader, &different_destination),
        };
        assert!(matches!(write_deck_export_from_live(caller, destination, &context, plan), Err(Error::ExportSourceChanged)));
        assert!(!path.exists());
        assert!(!different_destination.exists());
        assert!(reader.begin_snapshot().is_ok(), "refusal releases the plan's transaction");
    }
}

#[test]
fn future_selected_card_refuses_with_typed_check_database_guidance() {
    let f = seeded();
    let original = f.cards[0];
    f.collection.run(OpSpec::named("future export id", OpSource::Maintenance), move |ctx| {
        let mut card = ctx.scope().card(original)?.unwrap();
        card.id = CardId(NOW + 86_400_001);
        ctx.scope().put(&card)?;
        Ok(())
    }).unwrap();
    let reader = f.collection.reader(ReaderRole::Maintenance).unwrap();
    let path = f.dir.path().join("invalid-ids.apkg");
    let context = CollectionPackageWriteContext {
        collection_directory: f.collection.path().parent().unwrap(), purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate, clock: f.clock.as_ref(), random: f.random.as_ref(),
        space: &StatvfsSpace, is_cancelled: &|| false,
    };
    let result = preflight_deck_export_from_live(&reader, &path, &context, 100,
        DeckExportOptions { include_scheduling: false, include_presets: false, include_media: false });
    assert!(matches!(result, Err(Error::ExportInvalidIds)));
    assert!(!path.exists());
}
