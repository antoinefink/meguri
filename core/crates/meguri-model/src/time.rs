//! The two time units of collection columns (`specs/model/row-records.md`):
//! most `mod` and `mtime_secs` columns are epoch seconds; `col.mod`, `col.scm`,
//! `col.ls` and revlog ids are epoch milliseconds (R1456).

use std::fmt;

use crate::clock::Clock;

/// Epoch milliseconds: milliseconds since 1970-01-01T00:00:00Z.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EpochMillis(pub i64);

/// Epoch seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EpochSecs(pub i64);

impl EpochMillis {
    /// The instant the injected clock reports.
    pub fn from_clock(clock: &dyn Clock) -> EpochMillis {
        EpochMillis(clock.now_ms())
    }

    /// The whole second holding this instant: rounded toward negative
    /// infinity, so -1 ms is in second -1.
    pub fn whole_secs(self) -> EpochSecs {
        EpochSecs(self.0.div_euclid(1000))
    }
}

impl EpochSecs {
    /// The whole second of the injected clock's instant.
    pub fn from_clock(clock: &dyn Clock) -> EpochSecs {
        EpochMillis::from_clock(clock).whole_secs()
    }
}

impl fmt::Display for EpochMillis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for EpochSecs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
