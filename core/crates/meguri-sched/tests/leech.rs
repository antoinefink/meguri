use meguri_sched::states::leech::is_leech;

#[test]
fn leech_flags_follow_threshold_and_half_threshold_repeats() {
    for lapses in 0..=24 {
        assert_eq!(is_leech(lapses, 8), [8, 12, 16, 20, 24].contains(&lapses));
        assert_eq!(
            is_leech(lapses, 3),
            [3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23].contains(&lapses)
        );
        assert_eq!(is_leech(lapses, 1), lapses >= 1);
        assert!(!is_leech(lapses, 0));
    }
}
