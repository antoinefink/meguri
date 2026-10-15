//! `meguri-cli profile-extract`: the shape of a collection as aggregate
//! histograms (U11, R1940). It copies the collection (and its `-wal`) to a
//! temporary directory and reads the copy; the media folder is read for
//! file sizes and extensions only. The output holds counts and numeric
//! histograms, no text, names or ids.
//!
//! ```text
//! meguri-cli profile-extract --collection PATH --out FILE
//!     [--media-dir DIR] [--tz IANA | --utc-offset ±HH:MM]
//! ```
//!
//! The private collection is extracted only by a role allowed to read
//! `local-data/`, never by an implementer.

use std::path::PathBuf;

use meguri_testkit::generate::profile;

fn usage(message: &str) -> i32 {
    eprintln!("meguri-cli profile-extract: {message}");
    eprintln!(
        "usage: meguri-cli profile-extract --collection PATH --out FILE [--media-dir DIR] [--tz IANA | --utc-offset ±HH:MM]"
    );
    2
}

/// `±HH:MM` in seconds.
fn parse_offset(text: &str) -> Option<i32> {
    let (sign, rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':')?;
    let seconds = h.parse::<i32>().ok()? * 3600 + m.parse::<i32>().ok()? * 60;
    (seconds < 86_400).then_some(sign * seconds)
}

pub fn main(args: &[String]) -> i32 {
    let mut collection: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut media_dir: Option<PathBuf> = None;
    let mut tz: Option<jiff::tz::TimeZone> = None;
    let mut fixed: Option<i32> = None;
    let mut rest = args.iter();
    while let Some(flag) = rest.next() {
        let Some(value) = rest.next() else {
            return usage(&format!("{flag} needs a value"));
        };
        match flag.as_str() {
            "--collection" => collection = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--media-dir" => media_dir = Some(PathBuf::from(value)),
            "--tz" => match jiff::tz::TimeZone::get(value) {
                Ok(zone) => tz = Some(zone),
                Err(e) => return usage(&format!("--tz {value}: {e}")),
            },
            "--utc-offset" => match parse_offset(value) {
                Some(seconds) => fixed = Some(seconds),
                None => return usage(&format!("--utc-offset {value}: expected ±HH:MM")),
            },
            other => return usage(&format!("unknown option {other}")),
        }
    }
    let (Some(collection), Some(out)) = (collection, out) else {
        return usage("--collection and --out are required");
    };
    let offset_at = |ms: i64| -> i32 {
        match (&tz, fixed) {
            (Some(zone), _) => jiff::Timestamp::from_millisecond(ms)
                .map(|ts| zone.to_offset(ts).seconds())
                .unwrap_or(0),
            (None, Some(seconds)) => seconds,
            (None, None) => 0,
        }
    };
    let extracted = match profile::extract(&collection, media_dir.as_deref(), &offset_at) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("meguri-cli profile-extract: {e}");
            return 1;
        }
    };
    if let Err(e) = std::fs::write(&out, extracted.to_json()) {
        eprintln!("meguri-cli profile-extract: {}: {e}", out.display());
        return 1;
    }
    let summary = serde_json::json!({
        "out": out.display().to_string(),
        "totals": extracted.totals,
        "sha256": extracted.sha256(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("a summary serializes")
    );
    0
}
