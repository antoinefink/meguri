//! The stored cutoff language, independent of local time (`fsrs-items#I1.3`).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDate;

impl std::fmt::Display for InvalidDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid ignore-before date")
    }
}

impl std::error::Error for InvalidDate {}

fn number(text: &mut &str, width: usize) -> Result<i64, InvalidDate> {
    *text = text.trim_start_matches(char::is_whitespace);
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > width {
        return Err(InvalidDate);
    }
    let value = text[..digits].parse().map_err(|_| InvalidDate)?;
    *text = &text[digits..];
    Ok(value)
}

fn separator(text: &mut &str) -> Result<(), InvalidDate> {
    *text = text.strip_prefix('-').ok_or(InvalidDate)?;
    Ok(())
}

fn leap(year: i64) -> bool {
    year % 400 == 0 || (year % 4 == 0 && year % 100 != 0)
}

/// UTC midnight in epoch milliseconds. Empty text means the epoch cutoff.
pub fn cutoff_millis(mut text: &str) -> Result<i64, InvalidDate> {
    if text.is_empty() {
        return Ok(0);
    }
    text = text.trim_start_matches(char::is_whitespace);
    let sign = match text.as_bytes().first() {
        Some(b'+') => {
            text = &text[1..];
            Some(1)
        }
        Some(b'-') => {
            text = &text[1..];
            Some(-1)
        }
        _ => None,
    };
    // Whitespace is accepted before the signed number, not between its sign
    // and digits. The remaining numbers have their own whitespace prefix.
    if sign.is_some() && !text.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Err(InvalidDate);
    }
    let year = number(&mut text, if sign.is_some() { usize::MAX } else { 4 })? * sign.unwrap_or(1);
    separator(&mut text)?;
    let month = number(&mut text, 2)?;
    separator(&mut text)?;
    let day = number(&mut text, 2)?;
    if !text.is_empty() || !(-262_143..=262_142).contains(&year) || !(1..=12).contains(&month) {
        return Err(InvalidDate);
    }
    let lengths = [31, 28 + i64::from(leap(year)), 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=lengths[month as usize - 1]).contains(&day) {
        return Err(InvalidDate);
    }
    let before_year = year - 1;
    let leap_days = before_year.div_euclid(4) - before_year.div_euclid(100) + before_year.div_euclid(400);
    // There are 477 leap days through 1969. Euclidean division also counts
    // correctly across year zero and the negative Gregorian years.
    let days = 365 * (year - 1970) + leap_days - 477 + lengths[..month as usize - 1].iter().sum::<i64>() + day - 1;
    Ok(days * 86_400_000)
}
