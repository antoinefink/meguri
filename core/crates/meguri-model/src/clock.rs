//! The time seam (R1875). The core never reads the system clock or the
//! system time zone: the host supplies a [`Clock`] (Swift through
//! `HostConfig` in the app), and tests and the scenario runner inject fixed
//! instants, so Meguri and the oracle can be given the same moments.
//! `tools/mg core-lint` rejects `SystemTime::now` and `Instant::now` outside a
//! clock module or a bench (core/clippy.toml).

use std::fmt;

/// The source of the current instant, the local UTC offset and the local time
/// zone's IANA id.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Milliseconds since the Unix epoch (1970-01-01T00:00:00Z).
    fn now_ms(&self) -> i64;

    /// The local offset from UTC in effect at `at_ms` (milliseconds since the
    /// Unix epoch). It can differ between instants, across a DST change.
    fn utc_offset_at(&self, at_ms: i64) -> UtcOffset;

    /// The IANA id of the local time zone, such as `Europe/Paris`, as the host
    /// reports it. The core never looks it up itself.
    fn zone_id(&self) -> String;

    /// The local offset from UTC now.
    fn utc_offset(&self) -> UtcOffset {
        self.utc_offset_at(self.now_ms())
    }
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
    zone: &'static str,
}

impl FixedClock {
    /// A clock whose zone id is `UTC`; [`FixedClock::in_zone`] names another.
    pub const fn new(now_ms: i64, offset: UtcOffset) -> FixedClock {
        FixedClock {
            now_ms,
            offset,
            zone: "UTC",
        }
    }

    pub const fn in_zone(self, zone: &'static str) -> FixedClock {
        FixedClock { zone, ..self }
    }
}

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }

    fn utc_offset_at(&self, _at_ms: i64) -> UtcOffset {
        self.offset
    }

    fn zone_id(&self) -> String {
        self.zone.to_owned()
    }
}

#[cfg(feature = "test-hooks")]
pub use step::StepClock;

#[cfg(feature = "test-hooks")]
mod step {
    use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};
    use std::sync::{Mutex, PoisonError};

    use super::{Clock, UtcOffset};

    /// A fixed clock that test hooks move (KTD18): it stands still until
    /// stepped or set. Only in builds with the `test-hooks` feature.
    #[derive(Debug)]
    pub struct StepClock {
        now_ms: AtomicI64,
        offset_secs: AtomicI32,
        zone: Mutex<String>,
    }

    impl StepClock {
        pub fn new(now_ms: i64, offset: UtcOffset, zone: &str) -> StepClock {
            StepClock {
                now_ms: AtomicI64::new(now_ms),
                offset_secs: AtomicI32::new(offset.seconds()),
                zone: Mutex::new(zone.to_owned()),
            }
        }

        /// Moves the instant by `ms` (negative moves it back).
        pub fn step_ms(&self, ms: i64) {
            self.now_ms.fetch_add(ms, Ordering::SeqCst);
        }

        pub fn set_now_ms(&self, now_ms: i64) {
            self.now_ms.store(now_ms, Ordering::SeqCst);
        }

        pub fn set_utc_offset(&self, offset: UtcOffset) {
            self.offset_secs.store(offset.seconds(), Ordering::SeqCst);
        }

        pub fn set_zone(&self, zone: &str) {
            *self.zone.lock().unwrap_or_else(PoisonError::into_inner) = zone.to_owned();
        }
    }

    impl Clock for StepClock {
        fn now_ms(&self) -> i64 {
            self.now_ms.load(Ordering::SeqCst)
        }

        fn utc_offset_at(&self, _at_ms: i64) -> UtcOffset {
            UtcOffset::from_seconds(self.offset_secs.load(Ordering::SeqCst)).unwrap_or(UtcOffset::UTC)
        }

        fn zone_id(&self) -> String {
            self.zone.lock().unwrap_or_else(PoisonError::into_inner).clone()
        }
    }
}
