//! The two time units of collection columns (`specs/model/row-records.md`):
//! most `mod` and `mtime_secs` columns are epoch seconds; `col.mod`, `col.scm`,
//! `col.ls` and revlog ids are epoch milliseconds (R1456).

use std::fmt;

use crate::clock::Clock;

/// Seconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimestampSecs(pub i64);

/// Milliseconds since the Unix epoch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimestampMillis(pub i64);

impl TimestampSecs {
    /// The clock's current second (rounded down).
    pub fn now(clock: &dyn Clock) -> TimestampSecs {
        TimestampMillis::now(clock).as_secs()
    }
}

impl TimestampMillis {
    pub fn now(clock: &dyn Clock) -> TimestampMillis {
        TimestampMillis(clock.now_ms())
    }

    /// The second this instant falls in (rounded down, also before 1970).
    pub fn as_secs(self) -> TimestampSecs {
        TimestampSecs(self.0.div_euclid(1000))
    }
}

impl fmt::Display for TimestampSecs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for TimestampMillis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
