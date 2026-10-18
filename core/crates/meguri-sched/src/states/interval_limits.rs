//! Day-boundary conversion and shared interval selection (L7, T14, Z8).
use super::fuzz;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delay {
    Seconds(u32),
    Days(u32),
}
impl Delay {
    pub fn label_seconds(self) -> u32 {
        match self {
            Self::Seconds(s) => s,
            Self::Days(d) => d.saturating_mul(86400),
        }
    }
    pub fn revlog_interval(self) -> i32 {
        match self {
            Self::Seconds(s) => -(s.min(i32::MAX as u32) as i32),
            Self::Days(d) => d as i32,
        }
    }
}

pub fn step_delay(seconds: u32, until_next_day: u64) -> Delay {
    if u64::from(seconds) >= until_next_day {
        Delay::Days(((u64::from(seconds) - until_next_day) / 86400 + 1) as u32)
    } else {
        Delay::Seconds(seconds)
    }
}

/// U44 implements this seam. A chosen day must obey the supplied bounds;
/// declining leaves the pinned plain-fuzz result in force.
pub trait FuzzedDayChooser {
    fn choose(&mut self, interval: f32, minimum: u32, maximum: u32) -> Option<u32>;
}

pub(crate) struct Picker<'a> {
    pub factor: f32,
    pub chooser: Option<&'a mut dyn FuzzedDayChooser>,
}
impl Picker<'_> {
    pub fn pick(&mut self, interval: f32, minimum: u32, maximum: u32) -> u32 {
        if let Some(day) = self
            .chooser
            .as_mut()
            .and_then(|chooser| chooser.choose(interval, minimum, maximum))
        {
            return day;
        }
        fuzz::fuzzed_interval(interval, minimum, maximum, self.factor)
    }
}

pub(crate) fn review_days(intervals: [f32; 3], current: u32, maximum: u32, picker: &mut Picker<'_>) -> [u32; 3] {
    let mut values = [0; 3];
    let mut next_minimum = 1;
    for (index, interval) in intervals.into_iter().enumerate() {
        let minimum = fuzz::no_regression_minimum(interval, current, maximum)
            .max(next_minimum)
            .clamp(1, maximum.max(1));
        values[index] = picker.pick(interval, minimum, maximum.max(1));
        next_minimum = values[index].saturating_add(1);
    }
    values
}
