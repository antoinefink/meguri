//! `meguri-cli profile-extract`: the shape of a collection as aggregate
//! histograms (U11, R1940; `fixtures/synthetic-collections` F2.6). It
//! copies the collection (and its `-wal`) to a temporary directory and
//! reads the copy; the media folder is read for file sizes and extensions
//! only. The output holds counts and numeric histograms, no text, names or
//! ids.
//!
//! ```text
//! meguri-cli profile-extract <collection> --media-dir <dir> --tz <IANA zone> --out <file>
//! ```
//!
//! The private collection is extracted only by a role allowed to read
//! `local-data/`, never by an implementer.

use std::path::PathBuf;

use meguri_testkit::generate::profile;

fn usage(message: &str) -> i32 {
    eprintln!("meguri-cli profile-extract: {message}");
    eprintln!("usage: meguri-cli profile-extract <collection> --media-dir <dir> --tz <IANA zone> --out <file>");
    2
}

pub fn main(args: &[String]) -> i32 {
    let mut collection: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut media_dir: Option<PathBuf> = None;
    let mut tz: Option<jiff::tz::TimeZone> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if !arg.starts_with("--") {
            if collection.is_some() {
                return usage(&format!("one collection, not also {arg}"));
            }
            collection = Some(PathBuf::from(arg));
            continue;
        }
        let Some(value) = rest.next() else {
            return usage(&format!("{arg} needs a value"));
        };
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value)),
            "--media-dir" => media_dir = Some(PathBuf::from(value)),
            "--tz" => match jiff::tz::TimeZone::get(value) {
                Ok(zone) => tz = Some(zone),
                Err(e) => return usage(&format!("--tz {value}: {e}")),
            },
            other => return usage(&format!("unknown option {other}")),
        }
    }
    let (Some(collection), Some(media_dir), Some(tz), Some(out)) = (collection, media_dir, tz, out) else {
        return usage("<collection>, --media-dir, --tz and --out are required");
    };
    let offset_at = |ms: i64| -> i32 {
        jiff::Timestamp::from_millisecond(ms)
            .map(|ts| tz.to_offset(ts).seconds())
            .unwrap_or(0)
    };
    let extracted = match profile::extract(&collection, Some(&media_dir), &offset_at) {
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
