//! Canonical Gregorian wall names, independent of a timezone database (B2/B3).
use super::backup_name;
use crate::CollectionPackageWriteError;
use meguri_model::{clock::Clock, time::EpochMillis};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WallTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}
impl WallTime {
    /// Whole local days since the Unix epoch. Year zero uses Gregorian rules.
    pub fn day_number(self) -> i64 {
        let year = i64::from(self.year);
        let prior = year - 1;
        let mut day = 365 * (year - 1970) + prior.div_euclid(4) - prior.div_euclid(100) + prior.div_euclid(400) - 477;
        for month in 1..self.month {
            day += i64::from(month_days(self.year, month));
        }
        day + i64::from(self.day) - 1
    }
    pub fn wall_seconds(self) -> i64 {
        self.day_number() * 86_400 + i64::from(self.hour) * 3600 + i64::from(self.minute) * 60 + i64::from(self.second)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupKind {
    Backup,
    ServerCopy,
    ServerCopyUnverified,
}

fn month_days(year: u16, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u8::from(year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100))),
        _ => 0,
    }
}

/// Parse only the complete, exact-width B2 spelling; never inspect contents.
pub fn parse_name(name: &str) -> Option<(BackupKind, WallTime)> {
    let (kind, stamp) = if let Some(stamp) = name.strip_prefix("backup-").and_then(|s| s.strip_suffix(".colpkg")) {
        (BackupKind::Backup, stamp)
    } else if let Some(stamp) = name
        .strip_prefix("server-copy-")
        .and_then(|s| s.strip_suffix(".colpkg"))
    {
        (BackupKind::ServerCopy, stamp)
    } else {
        (
            BackupKind::ServerCopyUnverified,
            name.strip_prefix("server-copy-")?.strip_suffix(".unverified.anki2")?,
        )
    };
    let b = stamp.as_bytes();
    if b.len() != 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'-' || b[13] != b'.' || b[16] != b'.' {
        return None;
    }
    fn number(b: &[u8]) -> Option<u16> {
        b.iter()
            .try_fold(0u16, |n, b| b.is_ascii_digit().then(|| n * 10 + u16::from(b - b'0')))
    }
    let wall = WallTime {
        year: number(&b[..4])?,
        month: u8::try_from(number(&b[5..7])?).ok()?,
        day: u8::try_from(number(&b[8..10])?).ok()?,
        hour: u8::try_from(number(&b[11..13])?).ok()?,
        minute: u8::try_from(number(&b[14..16])?).ok()?,
        second: u8::try_from(number(&b[17..19])?).ok()?,
    };
    (wall.month >= 1
        && wall.month <= 12
        && wall.day >= 1
        && wall.day <= month_days(wall.year, wall.month)
        && wall.hour <= 23
        && wall.minute <= 59
        && wall.second <= 59)
        .then_some((kind, wall))
}

pub fn name_at(instant: EpochMillis, clock: &dyn Clock) -> Result<String, CollectionPackageWriteError> {
    let wall = instant
        .0
        .div_euclid(1000)
        .checked_add(i64::from(clock.utc_offset_at(instant.0).seconds()))
        .ok_or_else(|| crate::corrupt("backup date overflow"))?;
    backup_name(wall)
}

pub fn today(instant: EpochMillis, clock: &dyn Clock) -> Result<i64, CollectionPackageWriteError> {
    let name = name_at(instant, clock)?;
    Ok(parse_name(&name)
        .expect("writer produces a canonical name")
        .1
        .day_number())
}

pub fn name_from_wall_seconds(seconds: i64) -> Result<String, CollectionPackageWriteError> {
    backup_name(seconds)
}
