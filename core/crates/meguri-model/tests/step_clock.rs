//! The steppable clock behind `test-hooks` (KTD18): it stands still until a
//! test hook steps or sets it.

use meguri_model::clock::{Clock, StepClock, UtcOffset};
use meguri_model::time::{EpochMillis, EpochSecs};

/// 2030-01-15T10:00:00Z in milliseconds.
const START_MS: i64 = 1_894_701_600_000;

#[test]
fn it_moves_only_when_stepped_or_set() {
    let paris_winter = UtcOffset::from_seconds(3600).unwrap();
    let clock = StepClock::new(START_MS, paris_winter, "Europe/Paris");
    let seam: &dyn Clock = &clock;
    assert_eq!(seam.now_ms(), START_MS);
    assert_eq!(seam.now_ms(), START_MS);
    assert_eq!(seam.utc_offset().seconds(), 3600);
    assert_eq!(seam.zone_id(), "Europe/Paris");

    clock.step_ms(1_500);
    assert_eq!(seam.now_ms(), START_MS + 1_500);
    assert_eq!(EpochSecs::from_clock(seam), EpochSecs(1_894_701_601));
    clock.step_ms(-86_400_000);
    assert_eq!(
        EpochMillis::from_clock(seam),
        EpochMillis(START_MS + 1_500 - 86_400_000)
    );

    clock.set_now_ms(0);
    clock.set_utc_offset(UtcOffset::from_seconds(-12_600).unwrap());
    clock.set_zone("America/St_Johns");
    assert_eq!((seam.now_ms(), seam.utc_offset().seconds()), (0, -12_600));
    assert_eq!(seam.zone_id(), "America/St_Johns");
}
