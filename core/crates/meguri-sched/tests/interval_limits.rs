use meguri_sched::states::interval_limits::{Delay, step_delay};

#[test]
fn step_delay_switches_to_whole_days_at_the_exact_next_boundary() {
    for (delay, until, expected) in [
        (43500, 14700, Delay::Days(1)),
        (86400, 14700, Delay::Days(1)),
        (600, 14700, Delay::Seconds(600)),
        (600, 300, Delay::Days(1)),
        (259200, 64800, Delay::Days(3)),
        (169200, 64800, Delay::Days(2)),
        (299, 300, Delay::Seconds(299)),
        (300, 300, Delay::Days(1)),
    ] {
        assert_eq!(step_delay(delay, until), expected);
    }
    assert_eq!(Delay::Days(u32::MAX).label_seconds(), u32::MAX);
    assert_eq!(Delay::Seconds(45).label_seconds(), 45);
    assert_eq!(Delay::Seconds(u32::MAX).revlog_interval(), -i32::MAX);
    assert_eq!(Delay::Days(u32::MAX).revlog_interval(), -1);
}
