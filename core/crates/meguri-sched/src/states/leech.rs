//! sched/fsrs-transitions T12. Stored zero is normalized by PresetView.
pub fn is_leech(lapses: u32, threshold: u32) -> bool {
    if threshold == 0 || lapses < threshold {
        return false;
    }
    let repeat = (threshold as f32 / 2.0).ceil() as u32;
    (lapses - threshold).is_multiple_of(repeat.max(1))
}
