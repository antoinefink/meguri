//! U41 arithmetic examples are independent of the implementation. Corpus
//! identity/order proof lives in testkit/tests/queue_conformance.rs.
use meguri_sched::{
    queue::{limits::Remaining, mix::mix},
    sqlfns::hash,
};

#[test]
fn fnv_signed_and_unsigned_spec_vectors() {
    for (args, signed, unsigned) in [
        (vec![], -3750763034362895579i64, 14695981039346656037u64),
        (vec![0], -6284781860667377211, 12161962213042174405),
        (vec![1, 0], 4116863941369023524, 4116863941369023524),
        (vec![0, 0], -8637869204239850395, 9808874869469701221),
        (vec![-1, 5], -2510909584869376200, 15935834488840175416),
        (
            vec![1894701600000, 1894701600],
            5793145922124942247,
            5793145922124942247,
        ),
        (
            vec![1894701600000, 2654435761],
            -3475538189761224168,
            14971205883948327448,
        ),
        (vec![42, 7, 9], 3286408356281142785, 3286408356281142785),
    ] {
        assert_eq!(hash(&args), unsigned);
        assert_eq!(hash(&args) as i64, signed);
    }
}

#[test]
fn limits_use_wrapping_signed_arithmetic_before_clamping() {
    // L2: preset/override limits, new/review studied, ignore-review cap.
    for (n, r, ns, rs, ignore, expected) in [
        (20, 200, 0, 198, false, (2, 2)),
        (5, 200, 3, 10, false, (2, 187)),
        (5, 200, 3, 10, true, (2, 190)),
        (20, 200, 5, 199, false, (0, 0)),
        (5, 200, -3, 0, true, (8, 200)),
        (5, 2147483647, 0, -1, true, (5, 0)),
        (2147483648, 4294967295, 0, 0, true, (0, 0)),
    ] {
        let actual = Remaining::normal(n, r, ns, rs, ignore);
        assert_eq!((actual.new, actual.review), expected);
    }
}

#[test]
fn every_spreading_example_and_concatenation() {
    for (a, b, expected) in [
        (3, 3, "rnrnrn"),
        (3, 2, "rnrnr"),
        (3, 6, "nrnnrnnrn"),
        (3, 8, "nnrnnrnnrnn"),
        (7, 3, "rrnrrnrrnr"),
        (10, 1, "rrrrrnrrrrr"),
        (1, 10, "nnnnnrnnnnn"),
        (0, 4, "nnnn"),
        (4, 0, "rrrr"),
    ] {
        assert_eq!(
            mix(vec!['r'; a], vec!['n'; b], 0).into_iter().collect::<String>(),
            expected
        );
        assert_eq!(
            mix(vec!['r'; a], vec!['n'; b], 1),
            [vec!['r'; a], vec!['n'; b]].concat()
        );
        assert_eq!(
            mix(vec!['r'; a], vec!['n'; b], 2),
            [vec!['n'; b], vec!['r'; a]].concat()
        );
    }
}

#[test]
fn scalar_refuses_every_noninteger_storage_class_and_accepts_signed64() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    meguri_sched::sqlfns::register(&conn).unwrap();
    assert_eq!(
        conn.query_row("SELECT fnvhash()", [], |r| r.get::<_, i64>(0)).unwrap(),
        -3750763034362895579
    );
    for value in ["NULL", "1.0", "'1'", "x'0102'", "CAST(x'ff' AS TEXT)"] {
        assert!(
            conn.query_row(&format!("SELECT fnvhash(1,{value},2)"), [], |r| r.get::<_, i64>(0))
                .is_err()
        );
    }
    let args = [i64::MIN, i64::MAX, -1, 0];
    assert!(
        conn.query_row("SELECT fnvhash(?1,?2,?3,?4)", args, |r| r.get::<_, i64>(0))
            .is_ok(),
        "all signed64 inputs are accepted"
    );
    let flags = conn
        .query_row("SELECT flags FROM pragma_function_list WHERE name='fnvhash'", [], |r| {
            r.get::<_, i32>(0)
        })
        .unwrap();
    assert_ne!(
        flags & rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC.bits(),
        0
    );
    assert_ne!(flags & rusqlite::functions::FunctionFlags::SQLITE_INNOCUOUS.bits(), 0);
}

#[test]
fn rebuild_table_uses_payloads_generation_and_explicit_ownership() {
    use meguri_model::{changes::*, ids::*};
    use meguri_sched::queue::rebuild_rules::*;
    use meguri_storage::Cell;
    let built = Stamp { seq: 1, generation: 1 };
    let commit = Stamp { seq: 2, generation: 1 };
    let context = Context {
        built,
        commit,
        owned: None,
        day_changed: false,
    };
    let base = ChangeSet::empty(OpSource::UserUndoable);
    let classify_no_rows = |c: &ChangeSet| classify(c, &[], context);
    assert_eq!(classify_no_rows(&base), Action::None);
    for source in [OpSource::Maintenance, OpSource::UserUndoable] {
        let mut c = ChangeSet::empty(source);
        c.whole_collection = true;
        assert_eq!(
            classify(
                &c,
                &[],
                Context {
                    owned: Some(commit),
                    ..context
                }
            ),
            Action::Rebuild
        );
    }
    assert_eq!(
        classify(
            &base,
            &[],
            Context {
                commit: Stamp {
                    generation: 2,
                    ..commit
                },
                owned: Some(commit),
                ..context
            }
        ),
        Action::Rebuild
    );
    assert_eq!(
        classify(
            &base,
            &[],
            Context {
                day_changed: true,
                ..context
            }
        ),
        Action::Rebuild
    );
    assert_eq!(
        classify_no_rows(&ChangeSet::empty(OpSource::SyncIncoming)),
        Action::Rebuild
    );
    let mut cards = base.clone();
    cards.cards.push(CardChange {
        id: CardId(1),
        note_id: NoteId(2),
        kind: ChangeKind::Updated,
        memory_state_only: false,
    });
    assert_eq!(
        classify_no_rows(&cards),
        Action::Rebuild,
        "source does not prove own answer"
    );
    assert_eq!(
        classify(
            &cards,
            &[],
            Context {
                owned: Some(commit),
                ..context
            }
        ),
        Action::None
    );
    let before = vec![Cell::Integer(0); 18];
    let mut after = before.clone();
    after[4] = Cell::Integer(42);
    after[5] = Cell::Integer(-1);
    after[16] = Cell::Integer(3);
    fn images<'a>(before: &'a [Cell], after: &'a [Cell]) -> [CardImages<'a>; 1] {
        [CardImages {
            id: CardId(1),
            before: Some(before),
            after: Some(after),
        }]
    }
    assert_eq!(classify(&cards, &images(&before, &after), context), Action::None);
    for source in [OpSource::CloudIncoming, OpSource::CloudRekey] {
        let empty = ChangeSet::empty(source);
        let mut note_only = empty.clone();
        note_only.notes.push(NoteChange {
            id: NoteId(2),
            kind: ChangeKind::Updated,
        });
        let mut flag_only = cards.clone();
        flag_only.source = source;
        for changes in [&empty, &note_only, &flag_only] {
            for owned in [None, Some(commit)] {
                let context = Context { owned, ..context };
                assert_eq!(classify(changes, &images(&before, &after), context), Action::Rebuild);
                assert_eq!(
                    classify_with(changes, context, |_| -> Result<bool, ()> {
                        panic!("Cloud source must rebuild before reading payloads")
                    }),
                    Ok(Action::Rebuild)
                );
            }
        }
    }
    for column in [0, 1, 2, 3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 17] {
        let mut row = after.clone();
        row[column] = Cell::Integer(99);
        assert_eq!(
            classify(&cards, &images(&before, &row), context),
            Action::Rebuild,
            "column {column}"
        );
    }
    cards.cards[0].memory_state_only = true;
    let mut memory = after.clone();
    memory[17] = Cell::text("{\"s\":5,\"d\":6}");
    assert_eq!(classify(&cards, &images(&before, &memory), context), Action::Rebuild);
    for kind in [ChangeKind::Added, ChangeKind::Removed] {
        cards.cards[0].kind = kind;
        assert_eq!(classify(&cards, &images(&before, &after), context), Action::Rebuild);
    }
    let mut c = base.clone();
    c.decks.push(DeckChange {
        id: DeckId(1),
        kind: ChangeKind::Updated,
    });
    assert_eq!(classify_no_rows(&c), Action::Rebuild);
    let mut c = base.clone();
    c.deck_configs.push(DeckConfigChange {
        id: DeckConfigId(1),
        kind: ChangeKind::Updated,
    });
    assert_eq!(classify_no_rows(&c), Action::Rebuild);
    for key in [
        "curDeck",
        "collapseTime",
        "applyAllParentLimits",
        "newCardsIgnoreReviewLimit",
        "fsrs",
        "fsrsShortTermWithStepsEnabled",
        "loadBalancerEnabled",
        "rollover",
        "creationOffset",
        "localOffset",
    ] {
        let mut c = base.clone();
        c.config_keys.push(ConfigKeyChange {
            key: key.into(),
            kind: ChangeKind::Updated,
        });
        assert_eq!(classify_no_rows(&c), Action::Rebuild, "{key}");
    }
    let mut c = base.clone();
    c.notes.push(NoteChange {
        id: NoteId(1),
        kind: ChangeKind::Updated,
    });
    assert_eq!(classify_no_rows(&c), Action::Patch);
    let mut c = base.clone();
    c.notetypes.push(NotetypeChange {
        id: NotetypeId(1),
        kind: ChangeKind::Updated,
        schema_changed: false,
    });
    assert_eq!(classify_no_rows(&c), Action::Patch);
    let mut c = base.clone();
    c.tags.push(TagChange {
        name: "registry".into(),
        kind: ChangeKind::Updated,
    });
    assert_eq!(classify_no_rows(&c), Action::None);
    c.notes.push(NoteChange {
        id: NoteId(1),
        kind: ChangeKind::Updated,
    });
    assert_eq!(classify_no_rows(&c), Action::Patch, "used tag rename");
    let mut c = base;
    c.collection_row = true;
    c.revlog_added.push(RevlogId(1));
    c.media_added.push("m".into());
    c.config_keys.push(ConfigKeyChange {
        key: "unrelated".into(),
        kind: ChangeKind::Updated,
    });
    assert_eq!(classify_no_rows(&c), Action::None);
}

#[test]
fn lazy_rebuild_proofs_do_not_read_images_before_compact_exits_and_propagate_errors() {
    use meguri_model::{
        changes::{CardChange, ChangeKind, ChangeSet, ConfigKeyChange, OpSource},
        ids::{CardId, NoteId},
    };
    use meguri_sched::queue::rebuild_rules::{Action, Context, Stamp, classify_with};
    let stamp = Stamp { seq: 2, generation: 1 };
    let base = Context {
        built: Stamp { seq: 1, ..stamp },
        commit: stamp,
        owned: None,
        day_changed: false,
    };
    let mut changes = ChangeSet::empty(OpSource::UserUndoable);
    changes.cards.push(CardChange {
        id: CardId(4),
        note_id: NoteId(7),
        kind: ChangeKind::Updated,
        memory_state_only: false,
    });
    for exit in 0..5 {
        let mut changes = changes.clone();
        let mut context = base;
        match exit {
            0 => context.day_changed = true,
            1 => changes.whole_collection = true,
            2 => changes.source = OpSource::SyncIncoming,
            3 => context.built.generation += 1,
            _ => changes.config_keys.push(ConfigKeyChange {
                key: "curDeck".into(),
                kind: ChangeKind::Updated,
            }),
        }
        assert_eq!(
            classify_with(&changes, context, |_| -> Result<bool, ()> {
                panic!("decoder called before rebuild classification")
            }),
            Ok(Action::Rebuild)
        );
    }
    assert_eq!(
        classify_with(
            &changes,
            Context {
                owned: Some(stamp),
                ..base
            },
            |_| -> Result<bool, ()> { panic!("owned image read") }
        ),
        Ok(Action::None)
    );
    assert_eq!(classify_with(&changes, base, |_| Err("I/O")), Err("I/O"));
}
