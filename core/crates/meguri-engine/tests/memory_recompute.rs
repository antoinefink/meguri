mod common;
use common::Harness;
use meguri_engine::{
    CancelToken, Collection, EngineError, OpSpec,
    progress::{Progress, ProgressListener, ProgressPhase, ProgressSink},
    sched::memory::{self, Target},
    tags::RunOptions,
};
use meguri_model::{
    card::{CardQueue, CardType},
    card_data::CardData,
    changes::OpSource,
    ids::{CardId, DeckConfigId, RevlogId},
    proto::ProtoBlob,
};
use meguri_storage::{ReadRows, ReaderRole, StampMode, queries::revlog_fsrs::HistorySelection};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

fn setup(count: usize) -> Harness {
    let h = common::fresh();
    h.collection
        .run(
            OpSpec::named("memory_test_setup", OpSource::Maintenance).stamp(StampMode::Raw),
            move |context| {
                for index in 0..count {
                    let mut card = common::card(meguri_model::ids::NoteId(1), 0);
                    card.id = CardId(100 + index as i64);
                    card.card_type = CardType::Review;
                    card.queue = CardQueue::Review;
                    card.interval = 20;
                    card.ease_factor = 2500;
                    card.data = CardData::parse(r#"{"s":1,"d":5,"dr":0.7,"decay":0.5,"lrt":123,"custom":[1, 2]}"#);
                    context.scope().put(&card)?;
                    let mut row = common::revlog(card.id);
                    row.id = RevlogId(1000 + index as i64);
                    row.interval = 100;
                    context.scope().put(&row)?;
                }
                Ok(())
            },
        )
        .unwrap();
    h
}
fn dump(collection: &Collection) -> common::steps::Dump {
    common::steps::dump(collection)
}
fn edit_preset(
    h: &Harness,
    edit: impl FnOnce(&mut meguri_model::proto::deck_config::DeckConfigMessage) + Send + 'static,
) {
    h.collection
        .run(OpSpec::named("test_preset", OpSource::Maintenance), move |context| {
            let mut preset = context.scope().deck_config(DeckConfigId(1))?.unwrap();
            let mut fields = (*preset.config).clone();
            edit(&mut fields);
            preset.config = ProtoBlob::new(fields);
            context.scope().put(&preset)?;
            Ok(())
        })
        .unwrap();
}

struct CancelAfterBatch(Arc<CancelToken>);
impl ProgressListener for CancelAfterBatch {
    fn on_progress(&self, progress: &Progress) {
        if matches!(progress.phase, ProgressPhase::Named("Recomputing memory")) && progress.completed >= 256 {
            self.0.cancel();
        }
    }
}
fn ticks() -> Duration {
    static NOW: AtomicU64 = AtomicU64::new(0);
    Duration::from_secs(NOW.fetch_add(1, Ordering::Relaxed))
}

#[test]
fn cancellation_during_the_write_batches_rolls_back_all_rows_and_undo() {
    let h = setup(600);
    let before = dump(&h.collection);
    let undo = h.collection.peek_undo(meguri_engine::UndoScope::Global).unwrap();
    let cancel = CancelToken::new();
    let progress = ProgressSink::with_clock(ticks);
    progress.set_listener(Arc::new(CancelAfterBatch(cancel.clone())));
    let error = memory::recompute(
        &h.collection,
        Target::All,
        OpSource::UserUndoable,
        "Deck Options",
        &RunOptions {
            cancel: Some(cancel.clone()),
            progress: Some(progress),
        },
    )
    .unwrap_err();
    assert_eq!(error, EngineError::Cancelled);
    assert!(cancel.is_cancelled());
    assert_eq!(dump(&h.collection), before);
    assert_eq!(h.collection.peek_undo(meguri_engine::UndoScope::Global).unwrap(), undo);
}

#[test]
fn invalid_cutoff_is_typed_atomic_and_unsupported_presets_keep_their_bytes() {
    let h = setup(3);
    edit_preset(&h, |fields| fields.ignore_revlogs_before_date = "bad date".into());
    let before = dump(&h.collection);
    let error = memory::recompute(
        &h.collection,
        Target::All,
        OpSource::Maintenance,
        "Memory",
        &RunOptions::default(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        EngineError::InvalidIgnoreBeforeDate {
            preset: DeckConfigId(1)
        }
    );
    assert_eq!(dump(&h.collection), before);
    edit_preset(&h, |fields| fields.set_fsrs_params_6(vec![1.0, 2.0]));
    let before = dump(&h.collection);
    let report = memory::recompute(
        &h.collection,
        Target::All,
        OpSource::Maintenance,
        "Memory",
        &RunOptions::default(),
    )
    .unwrap()
    .value;
    assert_eq!(report.skipped_presets, [DeckConfigId(1)]);
    assert_eq!(report.processed, 0);
    assert_eq!(report.changed, 0);
    assert_eq!(dump(&h.collection)["cards"], before["cards"]);
    assert_eq!(dump(&h.collection)["revlog"], before["revlog"]);
}

#[test]
fn ordered_bulk_reader_and_raw_type_count_keep_the_two_interpretations_distinct() {
    let h = setup(3);
    h.collection
        .run(OpSpec::named("crafted_rows", OpSource::Maintenance), |context| {
            let mut card = context.scope().card(CardId(102))?.unwrap();
            card.queue = CardQueue::Suspended;
            context.scope().put(&card)?;
            let mut row = common::revlog(CardId(100));
            row.id = RevlogId(1);
            row.kind = meguri_model::revlog::RevlogKind::Learning;
            row.button = 0;
            context.scope().put(&row)?;
            let mut row = common::revlog(CardId(102));
            row.id = RevlogId(2);
            row.kind = meguri_model::revlog::RevlogKind::Learning;
            context.scope().put(&row)?;
            Ok(())
        })
        .unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let selection = HistorySelection::HomeDecks(vec![meguri_model::ids::DeckId(1)]);
    let mut groups = Vec::new();
    reader
        .visit_fsrs_histories(&selection, |card, rows| -> Result<(), meguri_storage::StorageError> {
            groups.push((card.id, rows.iter().map(|row| row.id).collect::<Vec<_>>()));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        groups,
        [
            (CardId(100), vec![RevlogId(1), RevlogId(1000)]),
            (CardId(101), vec![RevlogId(1001)]),
            (CardId(102), vec![RevlogId(2), RevlogId(1002)])
        ]
    );
    let count = reader.fsrs_included_count(&selection, 0, true).unwrap();
    assert_eq!((count.included, count.total), (1, 2));
    let count = reader.fsrs_included_count(&selection, 1, false).unwrap();
    assert_eq!((count.included, count.total), (1, 3));
    assert_eq!(reader.fsrs_last_review_time(CardId(100)).unwrap(), Some(1));
}
