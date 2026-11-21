//! Card Info's value writers (`stats/card-info` C5): dates, time spans,
//! percents and integers, plus the JavaScript number form some of them use.
//! Every text is English and is shown as returned (C12).

pub mod custom_data;

use meguri_model::clock::Clock;

/// The shortest decimal that reads back as `x` (finite, non-negative): its
/// significant digits and the position of the decimal point, so that `x` is
/// `0.<digits> × 10^point`. Zero is `("0", 1)`.
fn shortest(x: f64) -> (Vec<u8>, i32) {
    let text = format!("{:e}", x);
    let (mantissa, exponent) = text.split_once('e').expect("LowerExp writes an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp writes an integer exponent");
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    (digits, exponent + 1)
}

/// JavaScript's number-to-string conversion (shortest round trip; `1e+21`,
/// `1e-7`, `Infinity`; negative zero is `0`).
pub fn js_number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if x == 0.0 {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let (digits, n) = shortest(x.abs());
    let k = digits.len() as i32;
    let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).expect("ASCII digits");
    let body = if k <= n && n <= 21 {
        format!("{}{}", text(&digits), "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", text(&digits[..n as usize]), text(&digits[n as usize..]))
    } else if -6 < n && n <= 0 {
        format!("0.{}{}", "0".repeat((-n) as usize), text(&digits))
    } else {
        let e = n - 1;
        let rest = if k > 1 {
            format!(".{}", text(&digits[1..]))
        } else {
            String::new()
        };
        format!(
            "{}{}e{}{}",
            text(&digits[..1]),
            rest,
            if e > 0 { "+" } else { "-" },
            e.abs()
        )
    };
    format!("{sign}{body}")
}

/// `stats/studied-today#T5.2` for a finite, non-negative value: the shortest
/// decimal, rounded to two fraction digits with halves away from zero,
/// trailing zeros dropped, the integer part grouped with `,`.
pub fn stats_number(magnitude: f64) -> String {
    let (digits, point) = shortest(magnitude);
    let (mut whole, mut fraction): (Vec<u8>, Vec<u8>) = if point <= 0 {
        (vec![b'0'], std::iter::repeat_n(b'0', (-point) as usize).chain(digits).collect())
    } else if point as usize >= digits.len() {
        let mut whole = digits;
        whole.resize(point as usize, b'0');
        (whole, Vec::new())
    } else {
        let (whole, fraction) = digits.split_at(point as usize);
        (whole.to_vec(), fraction.to_vec())
    };
    if fraction.len() > 2 {
        let up = fraction[2] >= b'5';
        fraction.truncate(2);
        if up {
            // Add one unit in the last kept place, carrying leftwards.
            let mut carry = true;
            for digit in fraction.iter_mut().rev().chain(whole.iter_mut().rev()) {
                if *digit == b'9' {
                    *digit = b'0';
                } else {
                    *digit += 1;
                    carry = false;
                    break;
                }
            }
            if carry {
                whole.insert(0, b'1');
            }
        }
    }
    while fraction.last() == Some(&b'0') {
        fraction.pop();
    }
    let mut out = String::new();
    for (index, digit) in whole.iter().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(*digit as char);
    }
    if !fraction.is_empty() {
        out.push('.');
        out.extend(fraction.iter().map(|d| *d as char));
    }
    out
}

/// The unit of a time span (C5.2 step 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unit {
    Seconds,
    Minutes,
    Hours,
    Days,
    Months,
    Years,
}

impl Unit {
    fn of(seconds: f64, cap_days: bool) -> Unit {
        let size = seconds.abs();
        let unit = if size < 60.0 {
            Unit::Seconds
        } else if size < 3_600.0 {
            Unit::Minutes
        } else if size < 86_400.0 {
            Unit::Hours
        } else if size < 2_628_000.0 {
            Unit::Days
        } else if size < 31_536_000.0 {
            Unit::Months
        } else {
            Unit::Years
        };
        if cap_days && matches!(unit, Unit::Months | Unit::Years) {
            Unit::Days
        } else {
            unit
        }
    }

    fn seconds(self) -> f64 {
        match self {
            Unit::Seconds => 1.0,
            Unit::Minutes => 60.0,
            Unit::Hours => 3_600.0,
            Unit::Days => 86_400.0,
            Unit::Months => 2_628_000.0,
            Unit::Years => 31_536_000.0,
        }
    }

    fn names(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Unit::Seconds => ("second", "seconds", "s"),
            Unit::Minutes => ("minute", "minutes", "m"),
            Unit::Hours => ("hour", "hours", "h"),
            Unit::Days => ("day", "days", "d"),
            Unit::Months => ("month", "months", "mo"),
            Unit::Years => ("year", "years", "y"),
        }
    }
}

/// The two spellings of C5.2 step 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanForm {
    /// `<amount> <unit>`, singular for `1` and `-1`.
    Long,
    /// `<amount><suffix>`, the history's Time column.
    Short,
}

/// Rounds to an integer with halves toward positive infinity, keeping the
/// sign of a negative amount that rounds to zero (C5.2 step 3).
fn round_half_up(amount: f64) -> f64 {
    if !amount.is_finite() {
        return amount;
    }
    let floor = amount.floor();
    let rounded = if amount - floor >= 0.5 { floor + 1.0 } else { floor };
    if rounded == 0.0 && amount.is_sign_negative() {
        -0.0
    } else {
        rounded
    }
}

/// A time span of `seconds` (C5.2). `rounded` is step 3's rounded form;
/// `cap_days` makes days the largest unit (C4.2 step 2).
pub fn time_span(seconds: f64, form: SpanForm, rounded: bool, cap_days: bool) -> String {
    let unit = Unit::of(seconds, cap_days);
    let mut amount = seconds / unit.seconds();
    if rounded && matches!(unit, Unit::Seconds | Unit::Minutes | Unit::Hours | Unit::Days) {
        amount = round_half_up(amount);
    }
    let magnitude = if amount.is_infinite() {
        "∞".to_owned()
    } else if amount.is_nan() {
        "NaN".to_owned()
    } else {
        stats_number(amount.abs())
    };
    let sign = if amount.is_sign_negative() && !amount.is_nan() {
        "-"
    } else {
        ""
    };
    let (singular, plural, suffix) = unit.names();
    match form {
        SpanForm::Long => {
            let name = if magnitude == "1" { singular } else { plural };
            format!("{sign}{magnitude} {name}")
        }
        SpanForm::Short => format!("{sign}{magnitude}{suffix}"),
    }
}

/// C5.4's non-finite and huge values; `None` for the ordinary ones.
fn special_percent(x: f64) -> Option<String> {
    if x.is_nan() || x.is_infinite() || x.abs() >= 1e21 {
        Some(format!("{}%", js_number(x)))
    } else {
        None
    }
}

/// A whole percent of `x` (C5.4): the integer nearest `|x|`, halves away
/// from zero, with `-` when `x < 0`.
pub fn whole_percent(x: f64) -> String {
    if let Some(text) = special_percent(x) {
        return text;
    }
    let sign = if x < 0.0 { "-" } else { "" };
    format!("{sign}{:.0}%", x.abs().round())
}

/// A percent with two fraction digits always written (C5.4), from the
/// exact binary value, halves away from zero.
pub fn two_decimal_percent(x: f64) -> String {
    if let Some(text) = special_percent(x) {
        return text;
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let magnitude = x.abs();
    // A binary value lies exactly halfway between two hundredths only when
    // it is an odd number of eighths (x.125, x.375, x.625, x.875): those
    // round up here, where the formatter would round them to even.
    let eighths = magnitude * 8.0;
    let body = if eighths.fract() == 0.0 && (eighths as u128) % 2 == 1 {
        let hundredths = ((eighths as u128) * 25).div_ceil(2);
        format!("{}.{:02}", hundredths / 100, hundredths % 100)
    } else {
        format!("{magnitude:.2}")
    };
    format!("{sign}{body}%")
}

/// The largest instant a JavaScript date holds (C5.1, out of range).
const JS_DATE_LIMIT: i64 = 8_640_000_000_000;

/// The proleptic Gregorian date of a day count from 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The local calendar date and minute of the day of `instant` (C2).
fn local(instant: i64, clock: &dyn Clock) -> (i64, u32, u32, i64) {
    let offset = i64::from(clock.utc_offset_at(instant.saturating_mul(1000)).seconds());
    let local = instant + offset;
    let (year, month, day) = civil(local.div_euclid(86_400));
    (year, month, day, local.rem_euclid(86_400) / 60)
}

/// `YYYY-MM-DD` of the local date of `instant` (C5.1).
pub fn date(instant: i64, clock: &dyn Clock) -> String {
    if !(-JS_DATE_LIMIT..=JS_DATE_LIMIT).contains(&instant) {
        return "NaN-aN-aN".into();
    }
    let (year, month, day, _) = local(instant, clock);
    format!("{year}-{month:02}-{day:02}")
}

/// `<date> @ HH:MM` of `instant` (C5.1), seconds dropped.
pub fn date_time(instant: i64, clock: &dyn Clock) -> String {
    if !(-JS_DATE_LIMIT..=JS_DATE_LIMIT).contains(&instant) {
        return "NaN-aN-aN @ aN:aN".into();
    }
    let (year, month, day, minute) = local(instant, clock);
    format!("{year}-{month:02}-{day:02} @ {:02}:{:02}", minute / 60, minute % 60)
}
