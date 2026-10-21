use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::*,
};
use meguri_sched::{
    load_balancer::{EasyDay, Entry, LoadBalancer, easy_factors, weight},
    states::interval_limits::FuzzedDayChooser,
};

fn cache() -> LoadBalancer {
    LoadBalancer::new(
        0,
        86400,
        &FixedClock::new(0, UtcOffset::UTC),
        [(DeckConfigId(1), vec![1.0; 7])],
        [(
            DeckConfigId(1),
            0,
            Entry {
                card: CardId(1),
                note: NoteId(1),
            },
        )],
    )
    .unwrap()
}
#[test]
fn exact_offer_boundaries_and_missing_presets() {
    let c = cache();
    assert!(c.chooser(DeckConfigId(1), None, 1).choose(90.99, 90, 36500).is_some());
    assert_eq!(c.chooser(DeckConfigId(1), None, 1).choose(91.0, 90, 36500), None);
    assert_eq!(c.chooser(DeckConfigId(1), None, 1).choose(90.0, 91, 36500), None);
    assert_eq!(c.chooser(DeckConfigId(2), None, 1).choose(30.0, 1, 36500), None);
}
#[test]
fn append_keeps_duplicates_and_undo_removes_one_per_day() {
    let mut c = cache();
    let e = Entry {
        card: CardId(1),
        note: NoteId(1),
    };
    c.append(DeckConfigId(1), 0, e);
    c.append(DeckConfigId(1), 5, e);
    assert_eq!(c.entries(DeckConfigId(1), 0).len(), 2);
    c.remove_answer(CardId(1));
    assert_eq!(c.entries(DeckConfigId(1), 0), &[e]);
    assert!(c.entries(DeckConfigId(1), 5).is_empty());
}
#[test]
fn easy_day_factors_and_weight_order() {
    let f = easy_factors(
        &[4, 4, 4, 4, 4, 3, 1],
        &[
            EasyDay::Normal,
            EasyDay::Normal,
            EasyDay::Normal,
            EasyDay::Normal,
            EasyDay::Normal,
            EasyDay::Reduced,
            EasyDay::Reduced,
        ],
    );
    assert_eq!(f, vec![1.0, 1.0, 1.0, 1.0, 1.0, 0.0001, 1.0]);
    assert_eq!(weight(0, 30, 0.000001, 0.0001), 1.0);
    let q = 1.0f32 / 30.0;
    assert_eq!(weight(1, 30, 0.2, 0.0001), ((q * (q * q)) * 0.2) * 0.0001);
    assert!(
        LoadBalancer::new(
            0,
            86400,
            &FixedClock::new(0, UtcOffset::UTC),
            [(DeckConfigId(1), vec![1.0; 6])],
            []
        )
        .is_err()
    );
}
#[test]
fn failed_draws_and_single_reduced_candidate() {
    use meguri_sched::load_balancer::draw;
    for weights in [
        vec![],
        vec![0.0, 0.0],
        vec![-1.0, 2.0],
        vec![f32::NAN],
        vec![f32::INFINITY],
    ] {
        assert_eq!(draw(&weights, 1), None);
    }
    assert_eq!(easy_factors(&[0], &[EasyDay::Reduced]), vec![1.0]);
}
#[test]
fn sibling_days_are_unique_across_presets_and_multiply_in_day_order() {
    let c = LoadBalancer::new(
        0,
        86400,
        &FixedClock::new(0, UtcOffset::UTC),
        [(DeckConfigId(1), vec![])],
        [
            (
                DeckConfigId(1),
                27,
                Entry {
                    card: CardId(1),
                    note: NoteId(9),
                },
            ),
            (
                DeckConfigId(2),
                27,
                Entry {
                    card: CardId(2),
                    note: NoteId(9),
                },
            ),
            (
                DeckConfigId(2),
                33,
                Entry {
                    card: CardId(3),
                    note: NoteId(9),
                },
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        c.sibling_factors(Some(NoteId(9)), 25, 31),
        vec![0.4, 0.2, 0.000001, 0.2, 0.4f32 * 0.8, 0.6f32 * 0.6, 0.8f32 * 0.4]
    );
    assert_eq!(c.sibling_factors(None, 25, 31), vec![1.0; 7]);
}

fn from_rows(rows: &[(DeckConfigId, u32, Entry)]) -> LoadBalancer {
    LoadBalancer::new(
        0,
        86400,
        &FixedClock::new(0, UtcOffset::UTC),
        [(DeckConfigId(1), vec![]), (DeckConfigId(3), vec![])],
        rows.iter().copied(),
    )
    .unwrap()
}

fn assert_matches_rows(actual: &LoadBalancer, rows: &[(DeckConfigId, u32, Entry)]) {
    let expected = from_rows(rows);
    assert_eq!(actual, &expected);
    for preset in (1..=5).map(DeckConfigId) {
        for day in 0..99 {
            assert_eq!(actual.entries(preset, day), expected.entries(preset, day));
        }
        for note in [None, Some(NoteId(1)), Some(NoteId(2)), Some(NoteId(9))] {
            assert_eq!(
                actual.sibling_factors(note, 0, 98),
                expected.sibling_factors(note, 0, 98)
            );
            for seed in [0, 1, 17, u64::MAX] {
                for interval in [2.0, 30.0, 90.0] {
                    assert_eq!(
                        actual.chooser(preset, note, seed).choose(interval, 1, 36500),
                        expected.chooser(preset, note, seed).choose(interval, 1, 36500)
                    );
                }
            }
        }
    }
}

#[test]
fn cloned_snapshots_and_two_mutating_copies_remain_independent() {
    let preset = DeckConfigId(1);
    let rows = (24..=36)
        .map(|day| {
            (
                preset,
                day,
                Entry {
                    card: CardId(i64::from(day)),
                    note: NoteId(2),
                },
            )
        })
        .chain([
            (
                preset,
                27,
                Entry {
                    card: CardId(70),
                    note: NoteId(1),
                },
            ),
            (
                DeckConfigId(2),
                33,
                Entry {
                    card: CardId(71),
                    note: NoteId(1),
                },
            ),
        ])
        .collect::<Vec<_>>();
    let original = from_rows(&rows);
    let mut left = original.clone();
    let mut right = original.clone();
    let before_append = left.clone();
    let added = (
        preset,
        30,
        Entry {
            card: CardId(80),
            note: NoteId(1),
        },
    );
    left.append(added.0, added.1, added.2);
    let after_append = left.clone();
    let mut appended_rows = rows.clone();
    appended_rows.push(added);
    left.remove_answer(CardId(80));
    left.remove_answer(CardId(27));
    let removed_rows = rows
        .iter()
        .copied()
        .filter(|row| row.2.card != CardId(27))
        .collect::<Vec<_>>();
    let after_remove = left.clone();
    left.append(added.0, added.1, added.2);
    let mut left_rows = removed_rows.clone();
    left_rows.push(added);

    right.append(
        preset,
        30,
        Entry {
            card: CardId(81),
            note: NoteId(9),
        },
    );
    right.remove_answer(CardId(70));
    let mut right_rows = rows
        .iter()
        .copied()
        .filter(|row| row.2.card != CardId(70))
        .collect::<Vec<_>>();
    right_rows.push((
        preset,
        30,
        Entry {
            card: CardId(81),
            note: NoteId(9),
        },
    ));

    assert_matches_rows(&original, &rows);
    assert_matches_rows(&before_append, &rows);
    assert_matches_rows(&after_append, &appended_rows);
    assert_matches_rows(&after_remove, &removed_rows);
    assert_matches_rows(&left, &left_rows);
    assert_matches_rows(&right, &right_rows);
}

#[test]
fn removal_counts_the_actual_first_entry_and_keeps_other_note_occurrences() {
    let p = DeckConfigId(1);
    let q = DeckConfigId(2); // Counted rows need no Easy Days schedule.
    let a = Entry {
        card: CardId(1),
        note: NoteId(1),
    };
    let b = Entry {
        card: CardId(1),
        note: NoteId(2),
    };
    let c = Entry {
        card: CardId(2),
        note: NoteId(2),
    };
    let sentinel = Entry {
        card: CardId(9),
        note: NoteId(9),
    };
    let mut actual = from_rows(&[
        (p, 27, a),
        (p, 27, b),
        (p, 27, b),
        (p, 27, c),
        (q, 27, a),
        (p, 33, a),
        (q, 33, c),
        (q, 40, b),
        (p, 0, sentinel),
        (q, 0, sentinel),
    ]);
    actual.remove_answer(CardId(1));
    let mut remaining = vec![
        (p, 27, b),
        (p, 27, b),
        (p, 27, c),
        (q, 33, c),
        (p, 0, sentinel),
        (q, 0, sentinel),
    ];
    assert_matches_rows(&actual, &remaining);
    assert_eq!(actual.sibling_factors(Some(NoteId(1)), 25, 31), vec![1.0; 7]);
    assert_eq!(
        actual.sibling_factors(Some(NoteId(2)), 25, 31),
        vec![0.4, 0.2, 0.000001, 0.2, 0.4f32 * 0.8, 0.6f32 * 0.6, 0.8f32 * 0.4]
    );
    for _ in 0..2 {
        actual.remove_answer(CardId(1));
        remaining.remove(0);
        assert_matches_rows(&actual, &remaining);
    }
    actual.remove_answer(CardId(1)); // An absent card changes nothing.
    assert_matches_rows(&actual, &remaining);
    actual.remove_answer(CardId(2));
    assert_matches_rows(&actual, &[(p, 0, sentinel), (q, 0, sentinel)]);
    assert_eq!(actual.sibling_factors(Some(NoteId(2)), 25, 40), vec![1.0; 16]);
}

#[test]
fn only_accepted_rows_establish_presets_and_append_respects_the_window() {
    let entry = Entry {
        card: CardId(1),
        note: NoteId(1),
    };
    let accepted = [(DeckConfigId(1), 0, entry), (DeckConfigId(2), 98, entry)];
    let mut rows = accepted.to_vec();
    rows.extend([(DeckConfigId(4), 99, entry), (DeckConfigId(4), u32::MAX, entry)]);
    let mut actual = from_rows(&rows);
    for preset in [DeckConfigId(3), DeckConfigId(4), DeckConfigId(5)] {
        actual.append(preset, 0, entry);
    }
    for day in [99, u32::MAX] {
        actual.append(DeckConfigId(1), day, entry);
        assert!(actual.entries(DeckConfigId(1), day).is_empty());
    }
    assert_matches_rows(&actual, &accepted);
    actual.append(DeckConfigId(2), 0, entry);
    assert_matches_rows(&actual, &[accepted[0], accepted[1], (DeckConfigId(2), 0, entry)]);
    assert_eq!(
        actual
            .chooser(DeckConfigId(2), Some(entry.note), 17)
            .choose(30.0, 1, 36500),
        None
    );
    actual.remove_answer(entry.card);
    actual.append(DeckConfigId(1), 0, entry);
    actual.append(DeckConfigId(2), 98, entry);
    assert_matches_rows(&actual, &accepted);
}
