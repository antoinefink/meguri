mod common;
use common::{
    notes::{add, input, kind},
    tagging::fresh,
};
use meguri_engine::{
    CancelToken, EngineError, OpSpec, Progress, ProgressListener, ProgressSink, UndoScope, cardgen::regenerate, notes,
};
use meguri_model::{
    changes::OpSource,
    ids::{CardId, NoteId},
    notetype::NotetypeTemplate,
};
use meguri_storage::{ReadRows, ReaderRole};
use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn extra_template(nt: &mut meguri_model::notetype::Notetype) {
    let mut template: NotetypeTemplate = nt.templates[0].clone();
    template.ord = nt.templates.len() as u32;
    template.name = "Added view".to_owned();
    template.config.q_format = "{{Back}}".to_owned();
    nt.templates.push(template);
}
struct Recorder {
    times: Mutex<Vec<Duration>>,
    cancel: Option<Arc<CancelToken>>,
}
thread_local! {
    static PLANNING_MS: Cell<u64> = const { Cell::new(0) };
}
fn planning_ticks() -> Duration {
    let now = PLANNING_MS.get() + 1;
    PLANNING_MS.set(now);
    Duration::from_millis(now)
}
impl ProgressListener for Recorder {
    fn on_progress(&self, progress: &Progress) {
        // Planning delivers synchronously: record the sink's latest sample,
        // without advancing the clock or measuring OS scheduling delays.
        self.times
            .lock()
            .unwrap()
            .push(Duration::from_millis(PLANNING_MS.get()));
        if let Some(cancel) = &self.cancel
            && progress.completed >= 500
        {
            cancel.cancel();
        }
    }
}

#[test]
fn twenty_thousand_notes_plan_cancel_commit_and_repeat() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    h.collection
        .run(OpSpec::named("position setup", OpSource::UserNonUndoable), |ctx| {
            ctx.scope()
                .set_config("nextPos", &meguri_storage::ConfigValue::from(1_i64))?;
            let mut preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
            preset.config.new_card_insert_order = 0;
            ctx.scope().put(&preset)?;
            Ok(())
        })
        .unwrap();
    let inputs = (0..20_000)
        .map(|i| input(nt, &[&format!("word {i}"), if i % 2 == 0 { "meaning" } else { "" }]))
        .collect();
    let added = notes::bulk_add(&h.collection, inputs, OpSource::UserUndoable)
        .unwrap()
        .value;
    let mut draft = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(nt)
        .unwrap()
        .unwrap();
    extra_template(&mut draft);
    let cancelled = CancelToken::new();
    let sink = ProgressSink::with_clock(planning_ticks);
    sink.set_listener(Arc::new(Recorder {
        times: Mutex::new(vec![]),
        cancel: Some(cancelled.clone()),
    }));
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    assert!(matches!(
        regenerate::plan(&*reader, &draft, &cancelled, &sink),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(reader.cards_after(None, 40_001).unwrap().len(), 20_000);
    let sink = ProgressSink::with_clock(planning_ticks);
    let recorder = Arc::new(Recorder {
        times: Mutex::new(vec![]),
        cancel: None,
    });
    sink.set_listener(recorder.clone());
    let planned = regenerate::plan(&*reader, &draft, &CancelToken::new(), &sink).unwrap();
    assert_eq!(planned.notes.iter().map(|n| n.cards.len()).sum::<usize>(), 10_000);
    let times = recorder.times.lock().unwrap();
    assert!(
        times.len() >= 3,
        "planning must emit several progress updates: {times:?}"
    );
    assert!(
        times
            .windows(2)
            .all(|p| p[1].saturating_sub(p[0]) >= Duration::from_millis(100))
    );
    drop(times);
    drop(reader);
    let created = h
        .collection
        .run(
            OpSpec::new(
                meguri_engine::executor::catalog::NOTETYPE_STRUCTURE_CHANGE,
                OpSource::UserUndoable,
            )
            .declare_long_write(Duration::from_secs(5)),
            move |ctx| {
                // Same transaction as U34's note type save.
                ctx.scope().put_notetype(&draft)?;
                regenerate::commit_in(ctx.scope(), &planned)
            },
        )
        .unwrap()
        .value;
    assert_eq!(created.len(), 10_000);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    for (i, note) in added.iter().enumerate() {
        let cards = reader.cards_of_note(note.id).unwrap();
        assert_eq!(
            cards.iter().map(|c| c.ord).collect::<Vec<_>>(),
            if i % 2 == 0 { vec![0, 1] } else { vec![0] }
        );
        assert!(cards.iter().all(|c| c.due == i as i32 + 1));
    }
    drop(reader);
    assert!(
        regenerate::regenerate(&h.collection, nt, CancelToken::new(), ProgressSink::new())
            .unwrap()
            .value
            .is_empty()
    );
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .cards_after(None, 40_001)
            .unwrap()
            .len(),
        20_000
    );
}

#[test]
fn commit_replans_changed_cards_and_uses_live_note_fields() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let a = add(&h, input(nt, &["a", "yes"]));
    let b = add(&h, input(nt, &["b", "yes"]));
    let no_card = add(&h, input(nt, &["c", "yes"]));
    common::tagging::raw(
        &h.collection,
        "DELETE FROM cards WHERE nid=?",
        &[meguri_storage::raw::SqlValue::Integer(no_card.id.0)],
    );
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let mut draft = reader.notetype(nt).unwrap().unwrap();
    extra_template(&mut draft);
    let plan = regenerate::plan(&*reader, &draft, &CancelToken::new(), &ProgressSink::new()).unwrap();
    assert_eq!(plan.notes.len(), 2);
    drop(reader);
    let late = add(&h, input(nt, &["added after planning", "yes"]));
    let aid = a.cards[0];
    h.collection
        .run(OpSpec::named("move sibling", OpSource::UserUndoable), move |ctx| {
            let mut card = ctx.scope().card(aid)?.unwrap();
            card.due = 42;
            ctx.scope().put(&card)?;
            notes::update_in(
                ctx.scope(),
                &notes::UpdateNote {
                    id: b.id,
                    fields: vec!["b".to_owned(), "".to_owned()],
                    tags: vec![],
                },
            )?;
            Ok(())
        })
        .unwrap();
    let added = h
        .collection
        .run(OpSpec::named("regenerate", OpSource::UserUndoable), move |ctx| {
            regenerate::commit_in(ctx.scope(), &plan)
        })
        .unwrap()
        .value;
    assert_eq!(added.len(), 2);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.card(added[0]).unwrap().unwrap();
    assert_eq!((card.note_id, card.ord, card.due), (a.id, 1, 42));
    assert_eq!(reader.cards_of_note(late.id).unwrap().len(), 2);
    assert!(reader.cards_of_note(no_card.id).unwrap().is_empty());
}

#[test]
fn standard_regeneration_skips_equal_card_count_with_orphan_ordinal() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    let added = add(&h, input(nt, &["front", "back"]));
    common::tagging::raw(
        &h.collection,
        "UPDATE cards SET ord=5 WHERE id=?",
        &[meguri_storage::raw::SqlValue::Integer(added.cards[1].0)],
    );
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let nt = reader.notetype(nt).unwrap().unwrap();
    let plan = regenerate::plan(&*reader, &nt, &CancelToken::new(), &ProgressSink::new()).unwrap();
    assert!(plan.notes.is_empty());
    assert_eq!(
        reader
            .cards_of_note(added.id)
            .unwrap()
            .iter()
            .map(|card| card.ord)
            .collect::<Vec<_>>(),
        vec![0, 5]
    );
    drop(reader);
    let created = h
        .collection
        .run(OpSpec::named("regenerate", OpSource::UserUndoable), move |ctx| {
            regenerate::commit_in(ctx.scope(), &plan)
        })
        .unwrap()
        .value;
    assert!(created.is_empty());
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        reader
            .cards_of_note(added.id)
            .unwrap()
            .iter()
            .map(|card| (card.id, card.ord))
            .collect::<Vec<_>>(),
        vec![(added.cards[0], 0), (added.cards[1], 5)]
    );
}

#[test]
fn commit_uses_live_card_count_for_the_standard_skip() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let becomes_complete = add(&h, input(nt, &["front", "back"]));
    let becomes_incomplete = add(&h, input(nt, &["other", "back"]));
    h.collection
        .run(OpSpec::named("orphan setup", OpSource::UserNonUndoable), move |ctx| {
            ctx.scope().add_card(&common::card(becomes_incomplete.id, 5))?;
            Ok(())
        })
        .unwrap();
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let mut draft = reader.notetype(nt).unwrap().unwrap();
    extra_template(&mut draft);
    let plan = regenerate::plan(&*reader, &draft, &CancelToken::new(), &ProgressSink::new()).unwrap();
    let entry = plan
        .notes
        .iter()
        .find(|entry| entry.note == becomes_complete.id)
        .unwrap();
    assert_eq!(entry.cards.iter().map(|card| card.ord).collect::<Vec<_>>(), vec![1]);
    drop(reader);
    // Complete the first note by count only, and make the skipped note
    // incomplete. Commit must re-evaluate G9.2 for both live populations.
    h.collection
        .run(
            OpSpec::named("change siblings", OpSource::UserNonUndoable),
            move |ctx| {
                ctx.scope().add_card(&common::card(becomes_complete.id, 5))?;
                let orphan = ctx
                    .scope()
                    .cards_of_note(becomes_incomplete.id)?
                    .into_iter()
                    .find(|c| c.ord == 5)
                    .unwrap();
                meguri_engine::cards::remove_in(ctx.scope(), &[orphan.id], false)?;
                Ok(())
            },
        )
        .unwrap();
    let created = h
        .collection
        .run(OpSpec::named("regenerate", OpSource::UserUndoable), move |ctx| {
            regenerate::commit_in(ctx.scope(), &plan)
        })
        .unwrap()
        .value;
    assert_eq!(created.len(), 1);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.card(created[0]).unwrap().unwrap();
    assert_eq!((card.note_id, card.ord), (becomes_incomplete.id, 1));
    assert_eq!(
        reader
            .cards_of_note(becomes_complete.id)
            .unwrap()
            .iter()
            .map(|card| card.ord)
            .collect::<Vec<_>>(),
        vec![0, 5]
    );
}

#[test]
fn cloze_regeneration_does_not_skip_equal_card_and_template_counts() {
    let h = fresh();
    let nt = meguri_engine::notetypes::add(
        &h.collection,
        meguri_engine::notetypes::NewNotetype {
            name: "Regeneration cloze".to_owned(),
            kind: meguri_engine::notetypes::NotetypeKind::Cloze,
            fields: vec!["Text".to_owned()],
            templates: vec![meguri_engine::notetypes::NewTemplate {
                name: "Gap".to_owned(),
                front: "{{cloze:Text}}".to_owned(),
                back: "{{cloze:Text}}".to_owned(),
                ..Default::default()
            }],
            css: Some(String::new()),
        },
    )
    .unwrap()
    .value
    .id;
    let added = add(&h, input(nt, &["{{c1::one}}"]));
    common::tagging::raw(
        &h.collection,
        "UPDATE notes SET flds=? WHERE id=?",
        &[
            meguri_storage::raw::SqlValue::Text("{{c1::one}} {{c2::two}}".to_owned()),
            meguri_storage::raw::SqlValue::Integer(added.id.0),
        ],
    );
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let nt = reader.notetype(nt).unwrap().unwrap();
    let plan = regenerate::plan(&*reader, &nt, &CancelToken::new(), &ProgressSink::new()).unwrap();
    assert_eq!(plan.notes.len(), 1);
    assert_eq!(
        plan.notes[0].cards.iter().map(|card| card.ord).collect::<Vec<_>>(),
        vec![1]
    );
    drop(reader);
    let created = h
        .collection
        .run(OpSpec::named("regenerate", OpSource::UserUndoable), move |ctx| {
            regenerate::commit_in(ctx.scope(), &plan)
        })
        .unwrap()
        .value;
    assert_eq!(created.len(), 1);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.card(created[0]).unwrap().unwrap();
    assert_eq!((card.note_id, card.ord), (added.id, 1));
}

#[test]
fn cards_remove_can_remove_orphans_without_logging() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    let a = add(&h, input(nt, &["a"]));
    h.collection
        .run(OpSpec::named("empty cards", OpSource::UserUndoable), move |ctx| {
            assert_eq!(
                meguri_engine::cards::remove_in(ctx.scope(), &[a.cards[0], a.cards[0], CardId(-9)], true)?,
                1
            );
            Ok(())
        })
        .unwrap();
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(a.id)
            .unwrap()
            .is_none()
    );
    assert!(!h.path.with_file_name("deleted.txt").exists());
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(NoteId(a.id.0))
            .unwrap()
            .is_some()
    );
}
