//! U113/U114 source, stamping and catalog contracts: op-model O1/O1.1,
//! O4/O5/O9, ids-usn-mtime S3/S4, undo M11. Enabled tests for future APIs.

mod common;

use std::sync::{Arc, mpsc};
use std::time::Duration;

use common::{Harness, steps};
use meguri_engine::executor::{
    catalog,
    source::{QueueKind, default_stamp, queue_of},
};
use meguri_engine::{CancelToken, EngineError, OpSpec, SourceEffect, UndoClass, UndoScope, source_effect};
use meguri_model::changes::OpSource;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, ReaderRole, Record, StampMode};

#[test]
fn cloud_sources_are_background_with_distinct_stamping_and_targeted_undo() {
    assert_eq!(default_stamp(OpSource::CloudIncoming), StampMode::CallerSupplied);
    assert_eq!(default_stamp(OpSource::CloudRekey), StampMode::Local);
    for source in [OpSource::CloudIncoming, OpSource::CloudRekey] {
        assert_eq!(queue_of(source), QueueKind::Background);
        assert_eq!(source_effect(source), SourceEffect::CloudTargetedDrop);
    }
    assert_ne!(OpSource::CloudIncoming, OpSource::SyncIncoming);
}

#[test]
fn cloud_incoming_apply_is_a_non_destructive_cancellable_declared_long_write() {
    let def = catalog::lookup("cloud_incoming_apply").expect("O9 catalog entry");
    assert_eq!(def.undo_class(), Some(UndoClass::NonDestructive));
    assert!(def.may_declare_long_write());
    assert!(def.cancellable_write());
    assert!(!def.is_maintenance_task());
    assert!(!def.undo_class().unwrap().requires_confirmation());
    assert!(!def.undo_class().unwrap().requires_backup());
    for name in ["cloud_barrier", "cloud_capture"] {
        assert!(catalog::lookup(name).is_none(), "O1.1 uses a plain op");
    }
}

#[test]
fn incoming_preserves_payload_mtime_pending_usn_and_collection_sync_stamps_then_is_idempotent() {
    let h = Harness::open();
    let c = &h.collection;
    let (_, cid) = steps::setup_note(c, "a");
    c.run(OpSpec::named("settle", OpSource::Rollover), |_| Ok(())).unwrap();
    let (mut expected, mut expected_col) = {
        let reader = c.reader(ReaderRole::Query).unwrap();
        (reader.card(cid).unwrap().unwrap(), reader.col().unwrap())
    };
    expected.due = 51;
    expected.mtime = EpochSecs(456);
    expected.usn = Usn::PENDING;
    h.clock.step_ms(2_000);
    expected_col.modified = EpochMillis(common::NOW_MS + 2_000);
    let incoming = expected.clone();
    let applied = c
        .run(OpSpec::named("cloud_card", OpSource::CloudIncoming), move |ctx| {
            assert_eq!(ctx.scope().mode(), StampMode::CallerSupplied);
            ctx.scope().put(&incoming)?;
            Ok(())
        })
        .unwrap();
    assert!(!applied.changes.is_empty());
    assert!(applied.undo.is_none());
    {
        let reader = c.reader(ReaderRole::Query).unwrap();
        assert_eq!(reader.card(cid).unwrap().unwrap().to_cells(), expected.to_cells());
        // Includes col.usn, col.ls and col.scm, with their storage classes.
        assert_eq!(reader.col().unwrap().to_cells(), expected_col.to_cells());
    }
    let before = steps::dump(c);
    let events = h.engine.stream().subscribe();
    h.clock.step_ms(2_000);
    let unchanged = c
        .run(OpSpec::named("cloud_card", OpSource::CloudIncoming), move |ctx| {
            ctx.scope().put(&expected)?;
            Ok(())
        })
        .unwrap();
    assert!(unchanged.seq > applied.seq);
    assert!(unchanged.changes.is_empty());
    assert!(unchanged.cloud_detached_run.is_none());
    assert_eq!(steps::dump(c), before);
    assert!(events.try_recv().is_none());
}

#[test]
fn rekey_stamps_the_moved_row_and_its_references_locally_without_an_old_id_grave() {
    let h = Harness::open();
    let c = &h.collection;
    let (old, cid) = steps::setup_note(c, "object");
    c.run(OpSpec::named("settle", OpSource::Rollover), |_| Ok(())).unwrap();
    let new = meguri_model::ids::NoteId(old.0 + 100);
    let (mut expected_note, mut expected_card, mut expected_col) = {
        let reader = c.reader(ReaderRole::Query).unwrap();
        (
            reader.note(old).unwrap().unwrap(),
            reader.card(cid).unwrap().unwrap(),
            reader.col().unwrap(),
        )
    };
    expected_note.id = new;
    expected_note.mtime = EpochSecs(common::NOW_MS / 1_000 + 2);
    expected_note.usn = Usn::PENDING;
    expected_card.note_id = new;
    expected_card.mtime = expected_note.mtime;
    expected_card.usn = Usn::PENDING;
    expected_col.modified = EpochMillis(common::NOW_MS + 2_000);
    let graves_before = steps::dump(c)["graves"].clone();
    h.clock.step_ms(2_000);
    let applied = c
        .run(OpSpec::named("cloud_rekey", OpSource::CloudRekey), move |ctx| {
            assert_eq!(ctx.scope().mode(), StampMode::Local);
            let mut note = ctx.scope().note(old)?.unwrap();
            ctx.scope().remove_note(old)?;
            note.id = new;
            ctx.scope().put(&note)?;
            let mut card = ctx.scope().card(cid)?.unwrap();
            card.note_id = new;
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    assert!(!applied.changes.is_empty());
    assert!(applied.undo.is_none());
    {
        let reader = c.reader(ReaderRole::Query).unwrap();
        assert!(reader.note(old).unwrap().is_none());
        assert_eq!(reader.note(new).unwrap().unwrap().to_cells(), expected_note.to_cells());
        assert_eq!(reader.card(cid).unwrap().unwrap().to_cells(), expected_card.to_cells());
        assert_eq!(reader.col().unwrap().to_cells(), expected_col.to_cells());
    }
    assert_eq!(steps::dump(c)["graves"], graves_before);
    let before = steps::dump(c);
    let events = h.engine.stream().subscribe();
    h.clock.step_ms(2_000);
    let unchanged = c
        .run(
            OpSpec::named("cloud_rekey_no_change", OpSource::CloudRekey),
            move |ctx| {
                ctx.scope().put(&expected_note)?;
                ctx.scope().put(&expected_card)?;
                Ok(())
            },
        )
        .unwrap();
    assert!(unchanged.seq > applied.seq);
    assert!(unchanged.changes.is_empty());
    assert_eq!(steps::dump(c), before);
    assert!(events.try_recv().is_none());
}

#[test]
fn an_unchanged_cloud_write_keeps_both_undo_and_redo() {
    for source in [OpSource::CloudIncoming, OpSource::CloudRekey] {
        let h = Harness::open();
        let c = &h.collection;
        let (n1, _) = steps::setup_note(c, "one");
        let (n2, _) = steps::setup_note(c, "two");
        let first = steps::edit(c, None, n1, "edited one").unwrap();
        steps::edit(c, None, n2, "edited two").unwrap();
        c.undo(UndoScope::Global).unwrap();
        let before = steps::dump(c);
        let noop = c
            .run(OpSpec::named("cloud_unchanged", source), move |ctx| {
                let note = ctx.scope().note(n1)?.unwrap();
                ctx.scope().put(&note)?;
                Ok(())
            })
            .unwrap();
        assert!(noop.changes.is_empty());
        assert_eq!(steps::dump(c), before);
        c.redo(UndoScope::Global).unwrap();
        assert_eq!(common::note_fields(c, n2), "edited two\u{1f}back");
        c.undo(UndoScope::Global).unwrap();
        assert_eq!(c.undo(UndoScope::Global).unwrap().value.id.0, first.seq.0);
        assert_eq!(common::note_fields(c, n1), "one\u{1f}back");
    }
}

#[test]
fn cancellation_between_cloud_apply_batches_rolls_back_but_a_plain_write_completes() {
    for long in [true, false] {
        let h = Harness::open();
        let c = &h.collection;
        let (nid, _) = steps::setup_note(c, "a");
        let before = steps::dump(c);
        let cancel = CancelToken::new();
        let mut spec = if long {
            OpSpec::new(
                catalog::lookup("cloud_incoming_apply").expect("O9 entry"),
                OpSource::CloudIncoming,
            )
            .declare_long_write(Duration::from_millis(400))
        } else {
            OpSpec::named("cloud_plain", OpSource::CloudIncoming)
        };
        spec = spec.cancel(Arc::clone(&cancel));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let pending = c.submit(spec, move |ctx| {
            steps::set_first_field(ctx, nid, "first batch".to_owned())?;
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            ctx.check_cancel()?;
            steps::set_first_field(ctx, nid, "second batch".to_owned())
        });
        started_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        cancel.cancel();
        release_tx.send(()).unwrap();
        let result = pending.wait();
        if long {
            assert!(matches!(result, Err(EngineError::Cancelled)));
            assert_eq!(steps::dump(c), before, "the first batch rolls back too");
        } else {
            let committed = result.unwrap();
            assert!(committed.past_cancel_point);
            assert_eq!(common::note_fields(c, nid), "second batch\u{1f}back");
        }
    }
}
