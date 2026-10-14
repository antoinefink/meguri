//! The scenario clock (dsl.md D5, D7.1): frozen at `clock.start`, moved only by
//! `advance` and `set_clock`, with the zone's offsets from jiff's bundled IANA
//! database, so macOS and the Linux container agree. It is the [`Clock`]
//! Meguri's executor gives the core (R1875). The clock ops' adapters live here
//! too.

use std::sync::atomic::{AtomicI64, Ordering};

use jiff::Timestamp;
use jiff::civil::DateTime;
use jiff::tz::{AmbiguousOffset, TimeZone};
use meguri_model::clock::{Clock, UtcOffset};

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{DateTimeText, Duration, FormatError, Step};

#[derive(Debug)]
pub struct ScenarioClock {
    now_secs: AtomicI64,
    zone: TimeZone,
}

impl ScenarioClock {
    /// A clock in the IANA zone `tz` stopped at `start`.
    pub fn new(tz: &str, start: &str) -> Result<ScenarioClock, FormatError> {
        let zone = jiff::tz::db()
            .get(tz)
            .map_err(|_| FormatError::new(format!("unknown time zone {tz:?}")))?;
        let now = instant(&zone, start)?;
        Ok(ScenarioClock {
            now_secs: AtomicI64::new(now),
            zone,
        })
    }

    /// Epoch seconds now.
    pub fn now_secs(&self) -> i64 {
        self.now_secs.load(Ordering::SeqCst)
    }

    /// Moves forward by whole seconds; a day is 86,400 s.
    pub fn advance(&self, by: Duration) {
        self.now_secs.fetch_add(by.seconds(), Ordering::SeqCst);
    }

    /// Jumps to an instant in epoch seconds (a handoff's clock).
    pub fn set_secs(&self, secs: i64) {
        self.now_secs.store(secs, Ordering::SeqCst);
    }

    /// Jumps to a date-time: wall time in the zone, or with an offset.
    pub fn set(&self, text: &str) -> Result<(), FormatError> {
        let now = instant(&self.zone, text)?;
        self.now_secs.store(now, Ordering::SeqCst);
        Ok(())
    }
}

/// Epoch seconds of a scenario date-time: a wall time inside a DST gap is an
/// error, and a repeated one means its first occurrence.
fn instant(zone: &TimeZone, text: &str) -> Result<i64, FormatError> {
    let checked: DateTimeText = text.parse()?;
    let text = checked.as_str();
    let invalid = || FormatError::new(format!("{text:?} is not a valid date-time"));
    if text.len() > 19 {
        let ts: Timestamp = text.parse().map_err(|_| invalid())?;
        return Ok(ts.as_second());
    }
    let wall: DateTime = text.parse().map_err(|_| invalid())?;
    let ambiguous = zone.to_ambiguous_timestamp(wall);
    if let AmbiguousOffset::Gap { .. } = ambiguous.offset() {
        let name = zone.iana_name().unwrap_or("the scenario zone");
        return Err(FormatError::new(format!("wall time {text:?} does not exist in {name}")));
    }
    Ok(ambiguous.earlier().map_err(|_| invalid())?.as_second())
}

impl Clock for ScenarioClock {
    fn now_ms(&self) -> i64 {
        self.now_secs() * 1000
    }

    fn utc_offset_at(&self, at_ms: i64) -> UtcOffset {
        let offset = Timestamp::from_millisecond(at_ms)
            .map(|ts| self.zone.to_offset(ts).seconds())
            .unwrap_or(0);
        UtcOffset::from_seconds(offset).unwrap_or(UtcOffset::UTC)
    }

    fn zone_id(&self) -> String {
        self.zone.iana_name().unwrap_or("UTC").to_owned()
    }
}

/// Registers `advance` and `set_clock`.
pub(super) fn register(registry: &mut Registry) {
    registry.op("advance", advance);
    registry.op("set_clock", set_clock);
}

fn advance(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    if let Step::Advance(by) = step {
        session.clock.advance(*by);
    }
    Ok(OpResult::Done)
}

fn set_clock(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    if let Step::SetClock(at) = step {
        session
            .clock
            .set(at.as_str())
            .map_err(|e| ExecError::new("invalid_scenario", e.to_string()))?;
    }
    Ok(OpResult::Done)
}
