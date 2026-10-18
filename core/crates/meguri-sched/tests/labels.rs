use meguri_sched::states::labels::{TimeUnit, format_seconds};

#[test]
fn timespan_strings_follow_the_recorded_units_and_round_after_selecting_the_unit() {
    for (seconds, text) in [
        (0, "(end)"),
        (45, "45s"),
        (59, "59s"),
        (60, "1m"),
        (90, "2m"),
        (150, "3m"),
        (225, "4m"),
        (600, "10m"),
        (3570, "60m"),
        (5400, "1.5h"),
        (7170, "2h"),
        (8100, "2.3h"),
        (86399, "24h"),
        (86400, "1d"),
        (30 * 86400, "30d"),
        (31 * 86400, "1mo"),
        (70 * 86400, "2.3mo"),
        (364 * 86400, "12mo"),
        (365 * 86400, "1y"),
        (400 * 86400, "1.1y"),
        (u32::MAX, "136.2y"),
    ] {
        assert_eq!(format_seconds(seconds, 0).text(), text, "{seconds}");
    }
    assert_eq!(format_seconds(45, 0).unit, TimeUnit::Seconds);
    assert_eq!(format_seconds(31 * 86400, 0).amount, "1");
}

#[test]
fn less_than_compares_the_unrounded_seconds_and_never_prefixes_end() {
    for (seconds, limit, text) in [
        (45, 1200, "<45s"),
        (600, 1200, "<10m"),
        (1199, 1200, "<20m"),
        (1200, 1200, "20m"),
        (600, 600, "10m"),
        (300, 1200, "<5m"),
        (2 * 86400, 2 * 86400, "2d"),
        (0, 1200, "(end)"),
    ] {
        let label = format_seconds(seconds, limit);
        assert_eq!(label.text(), text);
        assert_eq!(label.less_than, text.starts_with('<'));
    }
}
