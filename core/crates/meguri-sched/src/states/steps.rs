//! Learning/relearning step choices, sched/steps L1–L6. No interval math
//! enters here: leaving steps is the next-state engine's decision.

pub fn seconds(minutes: f32) -> u32 {
    (minutes * 60.0) as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepDelay {
    pub seconds: u32,
    pub remaining: u32,
}

pub struct Steps<'a> {
    minutes: &'a [f32],
    remaining: u32,
}

impl<'a> Steps<'a> {
    pub fn new(minutes: &'a [f32], left: u32) -> Self {
        Self {
            minutes,
            remaining: left % 1000,
        }
    }
    pub fn count(&self) -> u32 {
        self.minutes.len() as u32
    }
    pub fn remaining(&self) -> u32 {
        self.remaining
    }
    pub fn index(&self) -> usize {
        self.minutes
            .len()
            .saturating_sub(self.remaining as usize)
            .min(self.minutes.len().saturating_sub(1))
    }
    pub fn current(&self) -> u32 {
        self.minutes.get(self.index()).copied().map(seconds).unwrap_or(0)
    }
    pub fn again(&self) -> Option<StepDelay> {
        self.minutes.first().map(|m| StepDelay {
            seconds: seconds(*m),
            remaining: self.count(),
        })
    }
    pub fn hard(&self) -> Option<StepDelay> {
        let first = seconds(*self.minutes.first()?);
        let delay = if self.index() > 0 {
            self.current()
        } else {
            let delay = if let Some(second) = self.minutes.get(1) {
                first.saturating_add(seconds(*second)) / 2
            } else {
                (first.saturating_mul(3) / 2).min(first.saturating_add(86400))
            };
            if delay > 86400 {
                (delay as f32 / 86400.0).round() as u32 * 86400
            } else {
                delay
            }
        };
        Some(StepDelay {
            seconds: delay,
            remaining: self.remaining,
        })
    }
    pub fn good(&self) -> Option<StepDelay> {
        let next = self.index() + 1;
        self.minutes.get(next).map(|m| StepDelay {
            seconds: seconds(*m),
            remaining: self.count() - next as u32,
        })
    }
}
