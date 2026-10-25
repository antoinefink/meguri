//! Small-fixture mechanism regressions for N6.5 and H6/H10.2.
use super::{commit, holdback, stage};
use crate::ErrorKind;
use crate::wire::chunk::{OutgoingChunk, decode_chunk, encode_apply_chunk};
use meguri_engine::{CancelToken, OpSource, OpSpec, ProgressSink, SyncHold};
use meguri_model::{
    card::Card,
    grave::GraveKind,
    ids::{CardId, NoteId, NotetypeId, RevlogId},
    note::Note,
    revlog::{RevlogEntry, RevlogKind},
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_storage::{
    ReadRows, ReaderRole, Record, RowKey, StampMode, Table,
    normal_stage::{Namespace, NormalStage},
};
use meguri_testkit::normal_support::{Fixture, NOW};

fn templates(f: &Fixture) -> (Card, Note) {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.card(f.cards[0]).unwrap().unwrap();
    let note = reader.note(card.note_id).unwrap().unwrap();
    (card, note)
}
fn capture(f: &Fixture) -> (NormalStage, SyncHold, EpochMillis) {
    let mut reader = f.collection.reader(ReaderRole::Sync).unwrap();
    let (snapshot, hold) = reader.capture_sync(&f.collection).unwrap();
    let schema = snapshot.col().unwrap().schema_modified;
    let disk = NormalStage::create(f.collection.path()).unwrap();
    holdback::build(&disk, &snapshot, &hold, f.clock.as_ref(), &CancelToken::new()).unwrap();
    (disk, hold, schema)
}
fn merge(f: &Fixture, disk: &NormalStage, hold: SyncHold, schema: EpochMillis) -> super::Result<commit::MergeOutcome> {
    commit::apply(
        &f.collection,
        disk,
        commit::MergeInput {
            hold,
            schema,
            usn: Usn(5),
            time: EpochMillis(NOW),
            meta: stage::Meta::default(),
        },
        CancelToken::new(),
        ProgressSink::new(),
    )
}
fn chunk(cards: &[Card], notes: &[Note], reviews: &[RevlogEntry]) -> crate::wire::chunk::IncomingChunk {
    let bytes = encode_apply_chunk(
        &OutgoingChunk {
            done: true,
            cards,
            notes,
            revlog: reviews,
        },
        Usn(5),
    );
    // The outgoing writer stamps a server USN. Preserve each supplied incoming
    // USN here to exercise strict-i32 reads, including pending and negative USNs.
    let mut body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for (i, row) in cards.iter().enumerate() {
        body["chunk"]["cards"][i][5] = row.usn.0.into();
    }
    for (i, row) in notes.iter().enumerate() {
        body["chunk"]["notes"][i][4] = row.usn.0.into();
    }
    for (i, row) in reviews.iter().enumerate() {
        body["chunk"]["revlog"][i][2] = row.usn.0.into();
    }
    decode_chunk(body["chunk"].to_string().as_bytes()).unwrap()
}
fn seed(f: &Fixture, cards: &[Card], notes: &[Note], reviews: &[RevlogEntry]) {
    let cards = cards.to_vec();
    let notes = notes.to_vec();
    let reviews = reviews.to_vec();
    f.collection
        .run(
            OpSpec::named("repeated row fixture", OpSource::Maintenance).stamp(StampMode::CallerSupplied),
            move |ctx| {
                for row in cards {
                    ctx.scope().put(&row)?;
                }
                for row in notes {
                    ctx.scope().put(&row)?;
                }
                for row in reviews {
                    ctx.scope().put(&row)?;
                }
                Ok(())
            },
        )
        .unwrap();
}
fn prepared(f: &Fixture, mut note: Note) -> Note {
    let nt = f
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(note.notetype_id)
        .unwrap()
        .unwrap();
    let fields = note
        .field_values()
        .map(meguri_engine::notes::sync_field)
        .collect::<Vec<_>>();
    meguri_engine::notes::prepare_sync_note(&mut note, &nt, &fields);
    note
}

#[test]
fn repeated_card_note_order_matrix_within_and_across_chunks() {
    // local scalar state; first and second incoming mod/usn; expected winner
    // (0 = local, 1 = first tuple, 2 = second tuple). Expectations are explicit.
    let cases = [
        (None, (30, 5), (10, 5), 2),
        (Some((40, 4)), (30, 5), (10, 5), 2),
        (Some((20, -1)), (10, 5), (20, 5), 0),
        (Some((20, -1)), (20, 5), (20, 5), 0),
        (Some((20, -1)), (30, 5), (10, 5), 2),
        (Some((20, -1)), (10, 5), (30, 5), 2),
        (Some((40, -1)), (30, 5), (10, 5), 0),
        (None, (10, 5), (30, 5), 2),
        (None, (20, 5), (20, 5), 2),
        (None, (30, -1), (10, 5), 1),
        (Some((40, 4)), (30, -1), (10, 5), 1),
        (Some((20, -1)), (30, -1), (10, 5), 1),
        (Some((20, -1)), (30, -1), (30, 5), 1),
        (None, (30, -2), (10, i32::MIN), 2),
        (Some((20, -1)), (10, -1), (30, 5), 2),
    ];
    for split in [false, true] {
        let f = Fixture::new(1);
        let (base_card, base_note) = templates(&f);
        let mut initial_cards = Vec::new();
        let mut initial_notes = Vec::new();
        let mut first_cards = Vec::new();
        let mut first_notes = Vec::new();
        let mut second_cards = Vec::new();
        let mut second_notes = Vec::new();
        let mut expected = Vec::new();
        for (index, &(local, first, second, winner)) in cases.iter().enumerate() {
            let id = 1000 + index as i64;
            let mut cards = Vec::new();
            let mut notes = Vec::new();
            for (version, (mtime, usn)) in [local.unwrap_or((0, 0)), first, second].into_iter().enumerate() {
                let mut card = base_card.clone();
                card.id = CardId(id);
                card.note_id = NoteId(id);
                card.mtime = EpochSecs(mtime);
                card.usn = Usn(usn);
                card.flags = version as u8;
                cards.push(card);
                let mut note = base_note.clone();
                note.id = NoteId(id);
                note.guid = format!("repeated-{id}-{version}");
                note.mtime = EpochSecs(mtime);
                note.usn = Usn(usn);
                note.fields = format!("version {version}\u{1}\u{1f}back");
                note.tags = " received_tag ".into();
                notes.push(note);
            }
            if local.is_some() {
                initial_cards.push(cards[0].clone());
                initial_notes.push(notes[0].clone());
            }
            first_cards.push(cards[1].clone());
            first_notes.push(notes[1].clone());
            second_cards.push(cards[2].clone());
            second_notes.push(notes[2].clone());
            expected.push((
                cards[winner].clone(),
                if winner == 0 {
                    notes[0].clone()
                } else {
                    prepared(&f, notes[winner].clone())
                },
            ));
        }
        seed(&f, &initial_cards, &initial_notes, &[]);
        let (disk, hold, schema) = capture(&f);
        let mut received = 0;
        if split {
            received += stage::chunk(&disk, chunk(&first_cards, &first_notes, &[]), f.clock.as_ref()).unwrap();
            received += stage::chunk(&disk, chunk(&second_cards, &second_notes, &[]), f.clock.as_ref()).unwrap();
        } else {
            let cards = first_cards
                .into_iter()
                .zip(second_cards)
                .flat_map(|(a, b)| [a, b])
                .collect::<Vec<_>>();
            let notes = first_notes
                .into_iter()
                .zip(second_notes)
                .flat_map(|(a, b)| [a, b])
                .collect::<Vec<_>>();
            received += stage::chunk(&disk, chunk(&cards, &notes, &[]), f.clock.as_ref()).unwrap();
        }
        assert_eq!(received, (cases.len() * 4) as u64);
        assert_eq!(disk.count(Table::Cards).unwrap(), cases.len() as u32 + 1);
        assert_eq!(disk.count(Table::Notes).unwrap(), cases.len() as u32 + 1);
        assert_eq!(disk.incoming_count().unwrap(), (cases.len() * 2) as u64);
        for (card, note) in &expected {
            assert_eq!(disk.card(card.id).unwrap().unwrap().to_cells(), card.to_cells());
            assert_eq!(disk.note(note.id).unwrap().unwrap().to_cells(), note.to_cells());
        }
        let events = f.host.engine().stream().subscribe();
        let outcome = merge(&f, &disk, hold, schema).unwrap();
        assert!(outcome.skipped.is_empty());
        let event = std::iter::from_fn(|| events.try_recv())
            .find(|e| Some(e.seq) == outcome.seq)
            .unwrap();
        let meguri_engine::EventKind::Changes(changes) = &event.kind else {
            panic!("merge changes")
        };
        let accepted = cases.iter().filter(|case| case.3 != 0).count();
        assert_eq!(changes.cards.len(), accepted);
        assert_eq!(changes.notes.len(), accepted);
        assert_eq!(
            changes
                .cards
                .iter()
                .map(|r| r.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            accepted
        );
        assert_eq!(
            changes
                .notes
                .iter()
                .map(|r| r.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            accepted
        );
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        for (card, note) in expected {
            assert_eq!(rows.card(card.id).unwrap().unwrap().to_cells(), card.to_cells());
            assert_eq!(rows.note(note.id).unwrap().unwrap().to_cells(), note.to_cells());
        }
        assert!(
            rows.tag("received_tag").unwrap().is_none(),
            "note preparation is row-only"
        );
    }
}

#[test]
fn repeated_identical_fresh_rows_are_unique_and_reviews_keep_first_complete_payload() {
    for split in [false, true] {
        let f = Fixture::new(1);
        let (mut card, mut note) = templates(&f);
        card.id = CardId(9003);
        card.note_id = NoteId(9003);
        card.usn = Usn(5);
        note.id = NoteId(9003);
        note.usn = Usn(5);
        let original = RevlogEntry {
            id: RevlogId(701),
            card_id: card.id,
            usn: Usn(4),
            button: 2,
            interval: 7,
            last_interval: 3,
            ease_factor: 2300,
            answer_millis: 1234,
            kind: RevlogKind::Review,
        };
        let removed = RevlogEntry {
            id: RevlogId(702),
            ..original.clone()
        };
        seed(&f, &[], &[], &[original.clone(), removed.clone()]);
        let (disk, hold, schema) = capture(&f);
        let removed_id = removed.id;
        f.collection
            .run(
                OpSpec::named("remove live review", OpSource::Maintenance).stamp(StampMode::CallerSupplied),
                move |ctx| {
                    ctx.scope().restore(Table::Revlog, &RowKey::Id(removed_id.0), None)?;
                    Ok(())
                },
            )
            .unwrap();
        let first = RevlogEntry {
            id: RevlogId(700),
            ..original.clone()
        };
        let later = RevlogEntry {
            card_id: CardId(999),
            usn: Usn(5),
            button: 4,
            interval: 99,
            last_interval: 88,
            ease_factor: 2500,
            answer_millis: 5678,
            kind: RevlogKind::Filtered,
            ..first.clone()
        };
        let local_collision = RevlogEntry {
            id: original.id,
            ..later.clone()
        };
        let first_removed = RevlogEntry {
            id: removed.id,
            ..later.clone()
        };
        let second_removed = RevlogEntry {
            answer_millis: 9999,
            ..first_removed.clone()
        };
        let mut total = 0;
        if split {
            total += stage::chunk(
                &disk,
                chunk(
                    std::slice::from_ref(&card),
                    std::slice::from_ref(&note),
                    &[first.clone(), local_collision.clone(), first_removed.clone()],
                ),
                f.clock.as_ref(),
            )
            .unwrap();
            total += stage::chunk(
                &disk,
                chunk(
                    std::slice::from_ref(&card),
                    std::slice::from_ref(&note),
                    &[later.clone(), local_collision, second_removed],
                ),
                f.clock.as_ref(),
            )
            .unwrap();
        } else {
            total += stage::chunk(
                &disk,
                chunk(
                    &[card.clone(), card.clone()],
                    &[note.clone(), note.clone()],
                    &[
                        first.clone(),
                        later.clone(),
                        local_collision.clone(),
                        local_collision,
                        first_removed.clone(),
                        second_removed,
                    ],
                ),
                f.clock.as_ref(),
            )
            .unwrap();
        }
        assert_eq!(total, 10);
        assert_eq!(disk.count(Table::Revlog).unwrap(), 3);
        assert_eq!(disk.count(Table::Cards).unwrap(), 2);
        assert_eq!(disk.count(Table::Notes).unwrap(), 2);
        assert_eq!(
            disk.get(Namespace::Incoming, Table::Revlog, &RowKey::Id(first.id.0))
                .unwrap()
                .unwrap(),
            first.to_cells()
        );
        merge(&f, &disk, hold, schema).unwrap();
        let expected_note = prepared(&f, note.clone());
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            rows.revlog_entry(original.id).unwrap().unwrap().to_cells(),
            original.to_cells()
        );
        assert_eq!(
            rows.revlog_entry(first.id).unwrap().unwrap().to_cells(),
            first.to_cells()
        );
        assert_eq!(
            rows.revlog_entry(removed.id).unwrap().unwrap().to_cells(),
            first_removed.to_cells()
        );
        assert_eq!(rows.card(card.id).unwrap().unwrap().to_cells(), card.to_cells());
        assert_eq!(
            rows.note(note.id).unwrap().unwrap().to_cells(),
            expected_note.to_cells()
        );
    }
}

#[test]
fn repeated_accepted_note_fault_is_not_overwritten_and_protected_note_stays_unchecked() {
    for split in [false, true] {
        for missing in [false, true] {
            for protected in [false, true] {
                let f = Fixture::new(1);
                let (_, mut note) = templates(&f);
                note.id = NoteId(70);
                note.mtime = EpochSecs(40);
                note.usn = Usn::PENDING;
                if protected {
                    seed(&f, &[], std::slice::from_ref(&note), &[]);
                }
                let (disk, _hold, _) = capture(&f);
                let mut bad = note.clone();
                bad.mtime = EpochSecs(30);
                bad.usn = Usn(5);
                bad.fields = "one field".into();
                if missing {
                    bad.notetype_id = NotetypeId(-900);
                }
                let mut good = note.clone();
                good.mtime = EpochSecs(10);
                good.usn = Usn(5);
                let result = if split {
                    stage::chunk(&disk, chunk(&[], &[bad.clone()], &[]), f.clock.as_ref())
                        .and_then(|_| stage::chunk(&disk, chunk(&[], &[good], &[]), f.clock.as_ref()))
                } else {
                    stage::chunk(&disk, chunk(&[], &[bad.clone(), good], &[]), f.clock.as_ref())
                };
                if protected {
                    result.unwrap();
                    assert_eq!(disk.note(note.id).unwrap().unwrap().to_cells(), note.to_cells());
                } else {
                    assert_eq!(
                        result.unwrap_err().kind,
                        if missing {
                            ErrorKind::MissingNotetype {
                                note_id: bad.id,
                                notetype_id: bad.notetype_id,
                            }
                        } else {
                            ErrorKind::NoteFieldsMismatch {
                                note_id: bad.id,
                                notetype_id: bad.notetype_id,
                            }
                        }
                    );
                    assert!(disk.note(note.id).unwrap().is_none());
                }
            }
        }
    }
}

#[test]
fn repeated_live_validation_uses_actual_held_or_post_snapshot_threshold_and_grave() {
    for split in [false, true] {
        for held in [false, true] {
            for (live_mod, grave, bad) in [
                (20, false, true),
                (40, false, true),
                (20, true, true),
                (20, false, false),
                (40, false, false),
            ] {
                let f = Fixture::new(1);
                let (mut local_card, mut local_note) = templates(&f);
                local_card.usn = Usn::PENDING;
                local_card.mtime = EpochSecs(40);
                local_note.usn = Usn::PENDING;
                local_note.mtime = EpochSecs(40);
                seed(&f, &[local_card.clone()], &[local_note.clone()], &[]);
                let active = held.then(|| f.host.review().begin_unqueued_session().unwrap());
                if let Some(active) = &active {
                    f.answer(local_card.id, Some(active.id()));
                }
                let collection = active
                    .as_ref()
                    .map_or_else(|| f.collection.clone(), |a| a.collection().unwrap());
                let touch = || {
                    let mut card = local_card.clone();
                    let mut note = local_note.clone();
                    card.mtime = EpochSecs(live_mod);
                    card.flags = 6;
                    note.mtime = EpochSecs(live_mod);
                    note.guid = "live scalar witness".into();
                    collection
                        .run(
                            OpSpec::named("live repeated row edit", OpSource::UserUndoable)
                                .stamp(StampMode::CallerSupplied),
                            move |ctx| {
                                ctx.scope().put(&card)?;
                                ctx.scope().put(&note)?;
                                if grave {
                                    ctx.scope().restore(Table::Cards, &RowKey::Id(card.id.0), None)?;
                                    ctx.scope().restore(Table::Notes, &RowKey::Id(note.id.0), None)?;
                                    ctx.scope().add_grave(card.id.0, GraveKind::Card, Usn::PENDING)?;
                                    ctx.scope().add_grave(note.id.0, GraveKind::Note, Usn::PENDING)?;
                                }
                                Ok(())
                            },
                        )
                        .unwrap();
                };
                if held {
                    touch();
                }
                let (disk, hold, schema) = capture(&f);
                if held {
                    assert!(hold.run.touches(Table::Notes, &RowKey::Id(local_note.id.0)));
                } else {
                    touch();
                }
                assert_eq!(disk.note(local_note.id).unwrap().unwrap().mtime, EpochSecs(40));
                let mut first_card = local_card.clone();
                first_card.mtime = EpochSecs(30);
                first_card.usn = Usn(5);
                first_card.flags = 2;
                let mut last_card = first_card.clone();
                last_card.mtime = EpochSecs(10);
                last_card.flags = 3;
                let mut first_note = local_note.clone();
                first_note.mtime = EpochSecs(30);
                first_note.usn = Usn(5);
                first_note.fields = if bad {
                    "one field".into()
                } else {
                    "first\u{1f}back".into()
                };
                let mut last_note = first_note.clone();
                last_note.mtime = EpochSecs(10);
                last_note.fields = "last\u{1f}back".into();
                if split {
                    assert_eq!(
                        stage::chunk(&disk, chunk(&[first_card], &[first_note], &[]), f.clock.as_ref()).unwrap(),
                        2
                    );
                    assert_eq!(
                        stage::chunk(
                            &disk,
                            chunk(&[last_card.clone()], &[last_note.clone()], &[]),
                            f.clock.as_ref()
                        )
                        .unwrap(),
                        2
                    );
                } else {
                    assert_eq!(
                        stage::chunk(
                            &disk,
                            chunk(&[first_card, last_card.clone()], &[first_note, last_note.clone()], &[]),
                            f.clock.as_ref()
                        )
                        .unwrap(),
                        4
                    );
                }
                assert_eq!(
                    disk.note(local_note.id).unwrap().unwrap().to_cells(),
                    local_note.to_cells()
                );
                let before = f.images();
                let events = f.host.engine().stream().subscribe();
                let result = merge(&f, &disk, hold, schema);
                if bad && live_mod == 20 && !grave {
                    assert_eq!(
                        result.unwrap_err().kind,
                        ErrorKind::NoteFieldsMismatch {
                            note_id: local_note.id,
                            notetype_id: local_note.notetype_id
                        }
                    );
                    assert_eq!(f.images(), before, "all changes and finalization roll back");
                    assert!(!std::iter::from_fn(|| events.try_recv()).any(|e| matches!(&e.kind, meguri_engine::EventKind::Changes(c) if !c.skipped_incoming_notes.is_empty() || !c.skipped_incoming_cards.is_empty())));
                } else {
                    let outcome = result.unwrap();
                    let expected_note = prepared(&f, last_note.clone());
                    let rows = f.collection.reader(ReaderRole::Query).unwrap();
                    if grave {
                        assert_eq!(
                            outcome.skipped,
                            [(Table::Cards, local_card.id.0), (Table::Notes, local_note.id.0)]
                        );
                        assert!(rows.card(local_card.id).unwrap().is_none());
                        assert!(rows.note(local_note.id).unwrap().is_none());
                    } else if live_mod == 20 {
                        assert_eq!(
                            rows.card(last_card.id).unwrap().unwrap().to_cells(),
                            last_card.to_cells()
                        );
                        assert_eq!(
                            rows.note(last_note.id).unwrap().unwrap().to_cells(),
                            expected_note.to_cells()
                        );
                    } else {
                        let mut expected = local_note.clone();
                        expected.guid = "live scalar witness".into();
                        assert_eq!(rows.note(expected.id).unwrap().unwrap().to_cells(), expected.to_cells());
                    }
                    if held {
                        assert!(
                            collection
                                .held_back_run(active.as_ref().map(|a| a.id()))
                                .unwrap()
                                .is_empty(),
                            "received targets clear the held run even when all tuples are rejected"
                        );
                        assert_eq!(
                            collection
                                .peek_undo(meguri_engine::UndoScope::Session(active.as_ref().unwrap().id()))
                                .unwrap()
                                .reason(),
                            Some(meguri_engine::DisabledReason::ClearedBySync)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn repeated_live_fault_uses_receive_ordinal_across_keys_and_validation_pages() {
    let f = Fixture::new(1);
    let (_, base) = templates(&f);
    let mut protected = Vec::new();
    for id in [9003, 70] {
        let mut row = base.clone();
        row.id = NoteId(id);
        row.mtime = EpochSecs(40);
        row.usn = Usn::PENDING;
        protected.push(row);
    }
    seed(&f, &[], &protected, &[]);
    let (disk, hold, schema) = capture(&f);
    let mut live = protected.clone();
    for row in &mut live {
        row.mtime = EpochSecs(20);
    }
    seed(&f, &[], &live, &[]);
    // More than PAGE scalar records precede a fault for one key.
    // All full payloads still reduce to one per key.
    let mut valid = protected[0].clone();
    valid.mtime = EpochSecs(10);
    valid.usn = Usn(5);
    for _ in 0..2 {
        assert_eq!(
            stage::chunk(&disk, chunk(&[], &vec![valid.clone(); 250], &[]), f.clock.as_ref()).unwrap(),
            250
        );
    }
    let mut first_bad = protected[0].clone();
    first_bad.mtime = EpochSecs(30);
    first_bad.usn = Usn(5);
    first_bad.notetype_id = NotetypeId(-900);
    let mut second_bad = protected[1].clone();
    second_bad.mtime = EpochSecs(30);
    second_bad.usn = Usn(5);
    second_bad.fields = "one field".into();
    stage::chunk(&disk, chunk(&[], &[first_bad.clone()], &[]), f.clock.as_ref()).unwrap();
    stage::chunk(&disk, chunk(&[], &[second_bad], &[]), f.clock.as_ref()).unwrap();
    stage::chunk(&disk, chunk(&[], &[valid], &[]), f.clock.as_ref()).unwrap();
    assert_eq!(disk.incoming_count().unwrap(), 2);
    let before = f.images();
    assert_eq!(
        merge(&f, &disk, hold, schema).unwrap_err().kind,
        ErrorKind::MissingNotetype {
            note_id: NoteId(9003),
            notetype_id: NotetypeId(-900)
        }
    );
    assert_eq!(f.images(), before);
}

#[test]
fn repeated_note_replay_declares_long_write_and_hands_off_durable_answer() {
    use meguri_engine::{
        EventKind, Qos, monotonic,
        session::{AnswerOutcome, JournalStage, StartResult},
    };
    use meguri_model::ids::DeckId;
    use meguri_testkit::normal_support::Rating;
    use std::{
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };

    let f = Fixture::new(2);
    let (session, head) = match f.host.review().start(DeckId(1), None).unwrap() {
        StartResult::Started { session_id, head, .. } => (session_id, head),
        other => panic!("{other:?}"),
    };
    let (_, mut note) = templates(&f);
    note.id = NoteId(9003);
    note.guid = "repeated budget witness".into();
    note.fields = "remote\u{1f}back".into();
    note.usn = Usn(5);
    let expected = prepared(&f, note.clone());
    let (disk, hold, schema) = capture(&f);
    let notes = vec![note; 250];
    for _ in 0..33 {
        assert_eq!(
            stage::chunk(&disk, chunk(&[], &notes, &[]), f.clock.as_ref()).unwrap(),
            250
        );
    }
    assert_eq!(disk.note_validation_count().unwrap(), 8250);
    assert_eq!(disk.incoming_count().unwrap(), 1);
    assert_eq!(disk.count(Table::Notes).unwrap(), 3);
    assert_eq!(disk.count(Table::Cards).unwrap(), 2);

    f.collection.record_durability_events();
    let events = f.host.engine().stream().subscribe();
    let (entered, merging) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    let released = Mutex::new(released);
    let _hook = super::test_hooks::install(Arc::new(move |point| {
        if let super::test_hooks::Point::Merged {
            table: Table::Notes,
            rows,
        } = point
        {
            assert_eq!(rows, 1);
            entered.send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }));
    let (intent_synced, durable_intent) = mpsc::channel();
    let (terminal_synced, durable_terminal) = mpsc::channel();
    let (answer_committed, applied_answer) = mpsc::sync_channel(1);
    let metadata = f.collection.clone();
    f.host
        .review()
        .journal_hook_for_testing(Arc::new(move |point| {
            if point == JournalStage::IntentSynced {
                intent_synced.send(monotonic::now()).unwrap();
            } else if point == JournalStage::PreparedSynced {
                let answer_committed = answer_committed.clone();
                metadata.after_next_commit(move |key| answer_committed.send(key).unwrap());
            } else if point == JournalStage::TerminalSynced {
                terminal_synced.send(()).unwrap();
            }
        }))
        .unwrap();
    let collection = f.collection.clone();
    let worker = f
        .host
        .engine()
        .threads()
        .spawn_owned("repeated note merge", Qos::UserInitiated, "jobs", move |_| {
            commit::apply(
                &collection,
                &disk,
                commit::MergeInput {
                    hold,
                    schema,
                    usn: Usn(5),
                    time: EpochMillis(NOW),
                    meta: stage::Meta::default(),
                },
                CancelToken::new(),
                ProgressSink::new(),
            )
        })
        .unwrap();
    merging.recv_timeout(Duration::from_secs(5)).unwrap();
    let delay = f.collection.write_delay().expect("one-key replay declares its work");
    assert_eq!(delay.op, "sync_incoming_merge");
    assert!(delay.declared > Duration::from_millis(200));

    let item = &head.items[0];
    let accepted_at = monotonic::now();
    assert!(matches!(
        f.host
            .review()
            .answer(
                session,
                item.card_id,
                Rating::Good,
                item.state_token,
                EpochMillis(NOW),
                3000
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    let synced_at = durable_intent.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(synced_at >= accepted_at);
    assert!(synced_at - accepted_at <= Duration::from_secs(1));
    assert!(f.collection.write_delay().is_some());
    {
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            rows.card(item.card_id).unwrap().unwrap().reps,
            0,
            "answer is still an overlay"
        );
        assert!(rows.note(expected.id).unwrap().is_none(), "merge is not yet visible");
        assert!(rows.ids_after(Table::Revlog, None, 250).unwrap().is_empty());
    }
    assert!(matches!(durable_terminal.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release.send(()).unwrap();
    let outcome = worker.join().unwrap().unwrap();
    let answer_deadline = monotonic::now() + Duration::from_secs(1);
    assert_eq!(outcome.write_budget, delay.declared);
    assert!(outcome.skipped.is_empty());
    f.host.review().application_barrier_for_testing().unwrap();
    let applied_key = applied_answer
        .recv_timeout(answer_deadline.saturating_sub(monotonic::now()))
        .unwrap();
    durable_terminal
        .recv_timeout(answer_deadline.saturating_sub(monotonic::now()))
        .unwrap();
    assert!(f.collection.write_delay().is_none());
    {
        let rows = f.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(rows.note(expected.id).unwrap().unwrap().to_cells(), expected.to_cells());
        assert_eq!(rows.ids_after(Table::Notes, None, 250).unwrap().len(), 3);
        assert_eq!(rows.ids_after(Table::Cards, None, 250).unwrap().len(), 2);
        assert_eq!(rows.card(item.card_id).unwrap().unwrap().reps, 1);
        assert_eq!(rows.ids_after(Table::Revlog, None, 250).unwrap().len(), 1);
    }
    let events = std::iter::from_fn(|| events.try_recv()).collect::<Vec<_>>();
    let delays = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match &e.kind {
            EventKind::WriteDelay {
                op: "sync_incoming_merge",
                declared,
            } => {
                assert_eq!(*declared, outcome.write_budget);
                Some(i)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let ends = events
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            matches!(
                &e.kind,
                EventKind::WriteDelayEnded {
                    op: "sync_incoming_merge"
                }
            )
            .then_some(i)
        })
        .collect::<Vec<_>>();
    assert_eq!(delays.len(), 1);
    assert_eq!(ends.len(), 1);
    let merged = events.iter().position(|e| Some(e.seq) == outcome.seq).unwrap();
    let EventKind::Changes(changes) = &events[merged].kind else {
        panic!("merge changes")
    };
    assert_eq!(changes.notes.len(), 1);
    assert_eq!(changes.notes[0].id, expected.id);
    assert!(changes.cards.is_empty());
    assert!(changes.skipped_incoming_notes.is_empty());
    assert!(changes.skipped_incoming_cards.is_empty());
    assert!(delays[0] < merged && merged < ends[0]);
    let answer = events
        .iter()
        .position(|e| matches!(&e.kind, EventKind::Changes(c) if c.cards.iter().any(|row| row.id == item.card_id)))
        .unwrap();
    assert!(ends[0] < answer, "overlay applies after the merge becomes visible");
    assert_eq!(
        events[answer].seq, applied_key,
        "answer event identifies the applied commit"
    );
    // WAL coverage writes Done synchronously before recording the sync attempt;
    // TerminalSynced alone does not mean that observation is available yet.
    let durability = loop {
        let durability = f.collection.durability_events();
        if durability
            .syncs
            .iter()
            .any(|sync| sync.succeeded && sync.through >= applied_key)
        {
            break durability;
        }
        assert!(
            monotonic::now() < answer_deadline,
            "real WAL sync did not cover the applied answer within one second after merge: {durability:?}"
        );
        std::thread::yield_now();
    };
    assert!(
        durability
            .syncs
            .iter()
            .any(|sync| sync.succeeded && sync.through >= applied_key && sync.completed <= answer_deadline),
        "real WAL sync covers the applied answer within one second after merge"
    );
}

#[test]
fn repeated_occurrences_still_count_towards_the_chunk_bound() {
    let f = Fixture::new(1);
    let (card, _) = templates(&f);
    let (disk, _hold, _) = capture(&f);
    let before = disk.count(Table::Cards).unwrap();
    assert_eq!(
        stage::chunk(&disk, chunk(&vec![card; 251], &[], &[]), f.clock.as_ref())
            .unwrap_err()
            .kind,
        ErrorKind::BadResponse
    );
    assert_eq!(disk.count(Table::Cards).unwrap(), before);
    assert_eq!(disk.incoming_count().unwrap(), 0);
}
