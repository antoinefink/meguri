//! The time seam (R1875). The core never reads the system clock: the host
//! supplies a [`Clock`] (Swift through `HostConfig` in the app), and tests and
//! the scenario runner inject fixed instants, so Meguri and the oracle can be
//! given the same moments. `tools/mg core-lint` rejects `SystemTime::now` and
//! `Instant::now` outside a clock module or a bench (core/clippy.toml).

use std::fmt;

/// The source of the current instant and of the local UTC offset.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Milliseconds since the Unix epoch (1970-01-01T00:00:00Z).
    fn now_ms(&self) -> i64;

    /// The local offset from UTC in effect at `at_ms` (milliseconds since the
    /// Unix epoch). It can differ between instants, across a DST change.
    fn utc_offset_at(&self, at_ms: i64) -> UtcOffset;
}

/// Offset of local time from UTC in whole seconds, positive east of UTC:
/// +05:45 is 20 700 and -03:30 is -12 600.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UtcOffset {
    seconds: i32,
}

impl UtcOffset {
    pub const UTC: UtcOffset = UtcOffset { seconds: 0 };

    /// `None` unless the offset is less than a whole day either way.
    pub const fn from_seconds(seconds: i32) -> Option<UtcOffset> {
        if seconds > -86_400 && seconds < 86_400 {
            Some(UtcOffset { seconds })
        } else {
            None
        }
    }

    pub const fn seconds(self) -> i32 {
        self.seconds
    }
}

/// A clock stopped at one instant with one offset, for tests and scenario runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FixedClock {
    now_ms: i64,
    offset: UtcOffset,
}

impl FixedClock {
    pub const fn new(now_ms: i64, offset: UtcOffset) -> FixedClock {
        FixedClock { now_ms, offset }
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }

    fn utc_offset_at(&self, _at_ms: i64) -> UtcOffset {
        self.offset
    }
}
