//! One short query snapshot for an ordered card-id list (C16).
use super::Question;
use crate::{Collection, EngineError, rendering};
use meguri_model::ids::CardId;
use meguri_search::browser::{deck_cell, qa};
use meguri_storage::{ReadRows, ReaderRole};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardExcerpt {
    Found {
        card_id: CardId,
        question: Question,
        deck: String,
    },
    Missing {
        card_id: CardId,
    },
    Damaged {
        card_id: CardId,
    },
}

pub fn card_excerpts(collection: &Collection, card_ids: &[CardId]) -> Result<Vec<CardExcerpt>, EngineError> {
    // Admission happens even for an empty list, after waiting for this reader.
    #[cfg(test)]
    tests::before_reader();
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    card_ids
        .iter()
        .map(|&id| {
            let row = excerpt(collection, &*snapshot, id)?;
            #[cfg(test)]
            tests::after_row();
            Ok(row)
        })
        .collect()
}

fn excerpt<R: ReadRows + ?Sized>(collection: &Collection, rows: &R, id: CardId) -> Result<CardExcerpt, EngineError> {
    let Some(card) = rows.card(id)? else {
        return Ok(CardExcerpt::Missing { card_id: id });
    };
    let damaged = CardExcerpt::Damaged { card_id: id };
    let Some(note) = rows.note(card.note_id)? else {
        return Ok(damaged);
    };
    let Some(kind) = rows.notetype(note.notetype_id)? else {
        return Ok(damaged);
    };
    let template = if kind.config.kind == 1 {
        kind.templates.first()
    } else {
        kind.templates.iter().find(|t| t.ord == u32::from(card.ord))
    };
    let Some(template) = template else {
        return Ok(damaged);
    };
    let Some(deck) = rows.deck(card.deck_id)? else {
        return Ok(damaged);
    };
    let home = if card.original_deck_id.0 != 0 {
        let Some(home) = rows.deck(card.original_deck_id)? else {
            return Ok(damaged);
        };
        Some(home.human_name())
    } else {
        None
    };
    let (rendered, _) = rendering::render_stored_in(
        collection,
        rows,
        &card,
        &note,
        &kind,
        template,
        rendering::RenderMode::Browser,
    )?;
    Ok(CardExcerpt::Found {
        card_id: id,
        question: qa::question(&rendered),
        deck: deck_cell::cards(&deck.human_name(), home.as_deref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, EngineConfig, LongOp, OpSpec, OpenConfig};
    use meguri_model::{
        changes::OpSource,
        clock::{StepClock, UtcOffset},
        ids::DeckId,
        rng::SeededRandom,
    };
    use std::{
        cell::RefCell,
        sync::{Arc, mpsc},
        thread,
    };
    thread_local! {
        static BEFORE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
        static AFTER: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
    }
    pub(super) fn before_reader() {
        BEFORE.with_borrow_mut(|f| {
            if let Some(f) = f {
                f();
            }
        });
    }
    pub(super) fn after_row() {
        AFTER.with_borrow_mut(|f| {
            if let Some(f) = f {
                f();
            }
        });
    }
    fn fixture() -> (tempfile::TempDir, Engine, Collection, CardId) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
        crate::bootstrap::create(&path, &*clock, &SeededRandom::new(7)).unwrap();
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let collection = engine
            .open_raw(&path, OpenConfig::new(clock, Arc::new(SeededRandom::new(7))))
            .unwrap();
        let kind = crate::notetypes::add(
            &collection,
            crate::notetypes::NewNotetype {
                name: "Excerpt admission".into(),
                kind: crate::notetypes::NotetypeKind::Standard,
                fields: vec!["Front".into(), "Back".into()],
                templates: vec![crate::notetypes::NewTemplate {
                    name: "Face".into(),
                    front: "{{Front}}".into(),
                    back: "{{Back}}".into(),
                    browser_front: String::new(),
                    browser_back: String::new(),
                }],
                css: None,
            },
        )
        .unwrap()
        .value
        .id;
        let note = crate::notes::add(
            &collection,
            crate::notes::AddNote {
                notetype_id: kind,
                fields: vec!["old".into(), "back".into()],
                tags: vec![],
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        let id = collection
            .reader(ReaderRole::Query)
            .unwrap()
            .cards_of_note(note.id)
            .unwrap()[0]
            .id;
        (dir, engine, collection, id)
    }
    #[test]
    fn request_waiting_for_reader_admission_refuses_replaced_handle_even_when_empty() {
        for empty in [false, true] {
            let (_dir, engine, old, id) = fixture();
            let (other_dir, other_engine, _other, _) = fixture();
            other_engine.close().unwrap();
            let (entered, ready) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let worker_collection = old.clone();
            // This gate is immediately before the pooled-reader request, as in
            // collection's existing lease-race test. Replacement wins admission.
            let worker = thread::spawn(move || {
                BEFORE.with_borrow_mut(|hook| {
                    *hook = Some(Box::new(move || {
                        entered.send(()).unwrap();
                        wait.recv().unwrap();
                    }))
                });
                card_excerpts(&worker_collection, if empty { &[] } else { std::slice::from_ref(&id) })
            });
            ready.recv().unwrap();
            let slot = engine.acquire_slot(LongOp::Restore).unwrap();
            let mut exclusive = slot.enter_exclusive(false).unwrap();
            let path = other_dir.path().join("collection.anki2");
            exclusive
                .replace(move |live| {
                    std::fs::copy(path, live)
                        .map(|_| ())
                        .map_err(|e| EngineError::Internal { message: e.to_string() })
                })
                .unwrap();
            drop(exclusive);
            drop(slot);
            release.send(()).unwrap();
            assert!(matches!(
                worker.join().unwrap(),
                Err(EngineError::StaleGeneration { .. })
            ));
        }
    }
    #[test]
    fn atomic_edit_between_two_results_cannot_change_the_second_result() {
        let (_dir, _engine, collection, id) = fixture();
        let baseline = card_excerpts(&collection, &[id]).unwrap()[0].clone();
        assert!(matches!(&baseline,CardExcerpt::Found {question:Question::Text(t),..} if t=="old"));
        let (entered, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let reader_collection = collection.clone();
        let worker = thread::spawn(move || {
            let mut first = true;
            AFTER.with_borrow_mut(|hook| {
                *hook = Some(Box::new(move || {
                    if first {
                        first = false;
                        entered.send(()).unwrap();
                        wait.recv().unwrap();
                    }
                }))
            });
            card_excerpts(&reader_collection, &[id, id])
        });
        ready.recv().unwrap();
        collection
            .run(
                OpSpec::named("excerpt_snapshot_edit", OpSource::SyncIncoming),
                move |ctx| {
                    let card = ctx.scope().card(id)?.unwrap();
                    let mut note = ctx.scope().note(card.note_id)?.unwrap();
                    note.fields = "new\u{1f}back".into();
                    ctx.scope().put(&note)?;
                    Ok(())
                },
            )
            .unwrap();
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap().unwrap(), vec![baseline.clone(), baseline]);
        assert!(matches!(&card_excerpts(&collection,&[id]).unwrap()[0],
            CardExcerpt::Found {question:Question::Text(t),..} if t=="new"));
    }
}
