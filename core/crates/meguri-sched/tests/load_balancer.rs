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
