//! Real writer/card/media/receipt tests. No protocol or storage stand-ins.
mod common;
use common::{fresh, notes::kind};
use meguri_engine::{Collection, EngineError, OpSource, OpSpec, UndoScope, notes, notetypes, media::MediaService};
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_storage::{ReadRows, ReaderRole, Table};

fn request(c: &Collection, id: NotetypeId) -> notes::AddScreenNote {
    let nt = c.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap();
    notes::AddScreenNote {
        expected_generation: c.generation(), notetype_id: id, kind: nt.config.kind,
        field_layout: nt.fields.iter().map(|f| notes::AddFieldIdentity { ord: f.ord, id: f.config.id, name: f.name.clone() }).collect(),
        notetype_base: notetypes::AggregateDigest::of(&nt),
        fields: vec!["A synthetic sentence. <img src=\"mining.png\">".into(), "A translation.".into()],
        tags: vec![], deck_id: DeckId(1),
    }
}
fn count(c: &Collection, table: Table) -> usize {
    c.reader(ReaderRole::Query).unwrap().ids_after(table, None, 100).unwrap().len()
}
fn settle(c: &Collection) {
    c.run(OpSpec::named("settle", OpSource::Rollover), |_| Ok(())).unwrap();
}
#[test]
fn media_cards_and_receipt_commit_once_and_conflicting_delivery_writes_nothing() {
    let h = fresh();
    let id = kind(&h, false, &["Sentence", "Translation"], &["{{Sentence}}"]);
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("mining.png", b"synthetic image").unwrap();
    let input = request(&h.collection, id);
    let key = "a".repeat(64);
    let digest = "b".repeat(64);
    let added = notes::mining::add(&h.collection, input.clone(), key.clone(), digest.clone(), Some(stage)).unwrap();
    assert!(added.value.written);
    assert_eq!(added.value.note.cards.len(), 1);
    assert_eq!(std::fs::read(media.store().media_dir().join("mining.png")).unwrap(), b"synthetic image");
    settle(&h.collection);
    common::steps::bury(&h.collection, None, added.value.note.cards[0]).unwrap();
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let before = common::steps::dump(&h.collection);
    let seq = h.collection.last_seq();
    let mut retry_stage = media.begin_staging().unwrap();
    retry_stage.add("retry-only.png", b"must not publish").unwrap();
    let retried = notes::mining::add(&h.collection, input.clone(), key.clone(), digest, Some(retry_stage)).unwrap();
    assert!(!retried.value.written);
    assert_eq!(retried.value.note.id, added.value.note.id);
    assert_eq!(retried.seq, seq);
    assert!(retried.changes.is_empty());
    assert_eq!(retried.col_mod, None);
    assert_eq!(retried.undo, None);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_eq!(common::steps::dump(&h.collection), before);
    assert!(!media.store().media_dir().join("retry-only.png").exists());
    assert_eq!(std::fs::read(media.store().media_dir().join("mining.png")).unwrap(), b"synthetic image");
    assert_eq!(count(&h.collection, Table::Notes), 1);
    assert_eq!(count(&h.collection, Table::Cards), 1);
    assert_eq!(h.collection.last_seq(), seq);
    let error = notes::mining::add(&h.collection, input, key, "c".repeat(64), None).unwrap_err();
    assert!(matches!(error, EngineError::InvalidArgument { .. }));
    assert_eq!(h.collection.last_seq(), seq);
    assert_eq!(count(&h.collection, Table::Notes), 1);
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    // Persistence, rather than a server-memory cache, protects a lost reply.
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    settle(&reopened);
    let seq = reopened.last_seq();
    let before = common::steps::dump(&reopened);
    let retried = notes::mining::add(&reopened, request(&reopened, id), "a".repeat(64), "b".repeat(64), None).unwrap();
    assert!(!retried.value.written);
    assert_eq!(retried.value.note.id, added.value.note.id);
    assert_eq!(reopened.last_seq(), seq);
    assert_eq!(common::steps::dump(&reopened), before);
}
#[test]
fn stale_destination_discards_staged_media_and_never_adds_a_partial_note() {
    let h = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let media = MediaService::open(&h.collection, h.clock.clone()).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("mining.png", b"must roll back").unwrap();
    let mut input = request(&h.collection, id);
    input.notetype_base.0[0] ^= 1;
    let result = notes::mining::add(&h.collection, input, "a".repeat(64), "b".repeat(64), Some(stage));
    assert!(matches!(result, Err(EngineError::Stale { .. })));
    assert_eq!(count(&h.collection, Table::Notes), 0);
    assert_eq!(count(&h.collection, Table::Cards), 0);
    assert!(!media.store().media_dir().join("mining.png").exists());
    assert!(!h.collection.reader(ReaderRole::Query).unwrap().config_keys().unwrap().iter().any(|key| key.starts_with("meguriMiningReceipt")));
}
#[test]
fn concurrent_deliveries_share_the_writer_receipt_and_generate_only_one_card() {
    let h = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let input = request(&h.collection, id);
    settle(&h.collection);
    let seq = h.collection.last_seq();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers: Vec<_> = (0..8).map(|_| {
        let collection = h.collection.clone();
        let input = input.clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            notes::mining::add(&collection, input, "a".repeat(64), "b".repeat(64), None).unwrap()
        })
    }).collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.value.written).count(), 1);
    assert!(results.iter().all(|r| r.value.note.id == results[0].value.note.id));
    assert!(results.iter().all(|r| r.seq.0 == seq.0 + 1));
    assert_eq!(h.collection.last_seq().0, seq.0 + 1);
    assert!(results.iter().filter(|r| !r.value.written).all(|r| r.changes.is_empty()));
    assert_eq!(count(&h.collection, Table::Notes), 1);
    assert_eq!(count(&h.collection, Table::Cards), 1);
}
