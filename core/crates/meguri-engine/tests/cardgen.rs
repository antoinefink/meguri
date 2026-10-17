//! Independent expectations from notes/deck-placement-and-position P4.
use meguri_engine::cardgen::position::random_position;
mod common;

macro_rules! scenario {
    ($name:ident, $id:literal) => {
        #[test]
        fn $name() {
            common::note_scenarios::play(concat!("cardgen/", $id));
        }
    };
}
scenario!(normal_conditionals, "normal/conditionals");
scenario!(normal_specials, "normal/specials-and-emptiness");
scenario!(normal_unusable_front, "normal/unusable-front");
scenario!(edits_only_add, "normal/edits-only-add");
scenario!(cloze_numbers, "cloze/numbers");
scenario!(cloze_above_500, "cloze/above-500");
scenario!(placement_new, "placement/new-note");
scenario!(placement_existing, "placement/existing-note");
scenario!(counter, "position/counter");
scenario!(random, "position/random");

#[test]
fn random_positions_match_recorded_table() {
    for (counter, expected) in [
        (0, 801),
        (1, 825),
        (2, 82),
        (3, 647),
        (998, 779),
        (999, 574),
        (1000, 998),
        (1001, 661),
        (5000, 4542),
        (123456789, 33597438),
        (u32::MAX, 2331729548),
    ] {
        assert_eq!(random_position(counter), expected, "counter {counter}");
    }
}
