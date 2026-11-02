//! `meguri-cli generate`: synthetic collections (U11, R1804, R1805, R1963).
//!
//! ```text
//! meguri-cli generate cloud-small --out DIR [--seed N] [--cache]
//! meguri-cli generate <size> --out DIR [options]           10k, 100k, 150k, 500k, 1m or a card count
//! meguri-cli generate worst-case <name|path.json> --out DIR [options]
//! meguri-cli generate recipe <name|path.yaml> --out DIR [--seed N] [--cache]
//!
//! options: --seed N (default 1)  --profile PATH (default the user shape)
//!          --cards N (worst cases)  --now RFC3339 (default 2030-01-15T10:00:00Z)
//!          --media references|files  --cache (reuse DIR when its manifest matches)
//! ```
//!
//! A recipe is generated again every time, `--cache` or not: its manifest
//! does not identify the recipe's text, so an edited recipe would match an
//! old directory, and a recipe takes well under a second. A worst case's
//! manifest does identify its definition (`worst_case_sha256`).
//!
//! A run holds DIR's lock (`.<name>.lock` beside it) from the cache check
//! to the end, so runs into one directory take turns, and the output is
//! published whole (`generate::OutputLock`, `generate::generate`).
//!
//! Prints `{dir, cached, elapsed_ms, peak_rss_kb, manifest}` as JSON, plus
//! `large_field_notes` for a worst case with large fields: the ids of the
//! notes holding one, which `tools/mg fixtures-check` renders in the oracle
//! (F11). Exit 0 on success, 1 when generation fails, 2 for a usage error.

use std::path::{Path, PathBuf};

use meguri_testkit::generate::recipe::{Recipe, generate_recipe};
use meguri_testkit::generate::{
    self, DEFAULT_NOW_MS, GENERATOR_VERSION, Manifest, MediaMode, OutputLock, Profile, Request, WorstCase, parse_size,
};
use meguri_testkit::wallclock::Stopwatch;
use serde_json::json;

enum What {
    CloudSmall,
    Size(u64),
    WorstCase(String),
    Recipe(String),
}

struct Options {
    what: What,
    out: PathBuf,
    seed: u64,
    profile: Option<PathBuf>,
    cards: Option<u64>,
    now_ms: i64,
    media: MediaMode,
    cache: bool,
}

fn usage(message: &str) -> i32 {
    eprintln!("meguri-cli generate: {message}");
    eprintln!(
        "usage: meguri-cli generate cloud-small|<size>|worst-case <name|path.json>|recipe <name|path.yaml> --out DIR \
         [--seed N] [--profile PATH] [--cards N] [--now RFC3339] [--media references|files] [--cache]"
    );
    2
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut rest = args.iter();
    let what = match rest.next().map(String::as_str) {
        Some("cloud-small") => What::CloudSmall,
        Some("worst-case") => What::WorstCase(rest.next().ok_or("worst-case needs a name")?.clone()),
        Some("recipe") => What::Recipe(rest.next().ok_or("recipe needs a name")?.clone()),
        Some(size) => What::Size(parse_size(size).ok_or_else(|| format!("not a size: {size}"))?),
        None => return Err("nothing to generate".to_owned()),
    };
    let cloud_small = matches!(&what, What::CloudSmall);
    let mut options = Options {
        what,
        out: PathBuf::new(),
        seed: 1,
        profile: None,
        cards: None,
        now_ms: if cloud_small {
            generate::cloud_small::NOW_MS
        } else {
            DEFAULT_NOW_MS
        },
        media: if cloud_small {
            MediaMode::Files
        } else {
            MediaMode::References
        },
        cache: false,
    };
    while let Some(flag) = rest.next() {
        let mut value = || rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--out" => options.out = PathBuf::from(value()?),
            "--seed" => options.seed = value()?.parse().map_err(|_| "--seed takes an integer")?,
            "--profile" => {
                if cloud_small {
                    return Err("cloud-small has a fixed shape; --profile is incompatible".to_owned());
                }
                options.profile = Some(PathBuf::from(value()?));
            }
            "--cards" => {
                let cards = parse_size(&value()?).ok_or("--cards takes a size")?;
                if cloud_small && cards != 20 {
                    return Err("cloud-small requires exactly 20 cards".to_owned());
                }
                options.cards = Some(cards);
            }
            "--now" => {
                let text = value()?;
                let ts: jiff::Timestamp = text.parse().map_err(|e| format!("--now {text}: {e}"))?;
                options.now_ms = ts.as_millisecond();
                if cloud_small && options.now_ms != generate::cloud_small::NOW_MS {
                    return Err("cloud-small requires --now 2025-01-01T00:00:00Z".to_owned());
                }
            }
            "--media" => {
                options.media = match value()?.as_str() {
                    "references" => MediaMode::References,
                    "files" => MediaMode::Files,
                    other => return Err(format!("--media is references or files, not {other}")),
                };
                if cloud_small && options.media != MediaMode::Files {
                    return Err("cloud-small requires --media files".to_owned());
                }
            }
            "--cache" => options.cache = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if options.out.as_os_str().is_empty() {
        return Err("--out is required".to_owned());
    }
    Ok(options)
}

/// The peak resident set size of this process in KiB, where the system
/// reports it (Linux).
pub fn peak_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
}

/// Whether `dir` already holds what was asked for.
fn cached(dir: &Path, wanted: &Manifest) -> Option<Manifest> {
    let have = Manifest::load(dir).ok()?;
    let same = have.generator_version == GENERATOR_VERSION
        && have.kind == wanted.kind
        && have.name == wanted.name
        && have.seed == wanted.seed
        && have.cards_requested == wanted.cards_requested
        && have.now_ms == wanted.now_ms
        && have.utc_offset_secs == wanted.utc_offset_secs
        && have.profile_sha256 == wanted.profile_sha256
        && have.worst_case_sha256 == wanted.worst_case_sha256
        && have.media == wanted.media
        && dir.join("collection.anki2").is_file();
    same.then_some(have)
}

/// A committed worst case by name, or a worst-case file (`.json`).
fn worst_case(name: &str) -> Result<WorstCase, String> {
    let path = Path::new(name);
    let loaded = if path.extension().is_some_and(|e| e == "json") {
        WorstCase::load(path)
    } else {
        WorstCase::named(name)
    };
    loaded.map_err(|e| format!("worst case {name}: {e}"))
}

fn recipe(name: &str) -> Result<Recipe, String> {
    let path = Path::new(name);
    let loaded = if path.extension().is_some_and(|e| e == "yaml") {
        Recipe::load(path)
    } else {
        Recipe::named(name)
    };
    loaded.map_err(|e| format!("recipe {name}: {e}"))
}

pub fn main(args: &[String]) -> i32 {
    let options = match parse(args) {
        Ok(options) => options,
        Err(message) => return usage(&message),
    };
    let watch = Stopwatch::start();
    if matches!(&options.what, What::CloudSmall) {
        return cloud_small(&options, &watch);
    }
    let profile = || -> Result<Profile, String> {
        match &options.profile {
            Some(path) => Profile::load(path),
            None => generate::user_shape(),
        }
        .map_err(|e| format!("profile: {e}"))
    };

    // What the manifest will say, for the cache check, and how to make it.
    let (wanted, request, recipe) = match &options.what {
        What::Recipe(name) => {
            let recipe = match recipe(name) {
                Ok(recipe) => recipe,
                Err(message) => return usage(&message),
            };
            (None, None, Some(recipe))
        }
        other => {
            let profile = match profile() {
                Ok(profile) => profile,
                Err(message) => return usage(&message),
            };
            let (worst, cards, kind, name) = match other {
                What::WorstCase(name) => {
                    let case = match worst_case(name) {
                        Ok(case) => case,
                        Err(message) => return usage(&message),
                    };
                    let cards = options.cards.unwrap_or(case.cards);
                    let name = case.name.clone();
                    (Some(case), cards, "worst-case", name)
                }
                What::Size(cards) => (None, *cards, "profile", cards.to_string()),
                What::CloudSmall | What::Recipe(_) => unreachable!("handled above"),
            };
            let mut request = Request::new(profile, cards, options.seed);
            request.now_ms = options.now_ms;
            request.media = options.media;
            let media = match &worst {
                Some(w) if w.media_files.is_some() => MediaMode::Files,
                _ => options.media,
            };
            request.worst_case = worst;
            let wanted = Manifest {
                generator_version: GENERATOR_VERSION,
                kind: kind.to_owned(),
                name,
                seed: options.seed,
                cards_requested: cards,
                now_ms: options.now_ms,
                utc_offset_secs: request.profile.utc_offset_secs,
                profile_sha256: Some(request.profile.sha256()),
                worst_case_sha256: request.worst_case.as_ref().map(WorstCase::sha256),
                media,
                counts: Default::default(),
                dump_sha256: String::new(),
            };
            (Some(wanted), Some(request), None)
        }
    };

    let large_fields = request
        .as_ref()
        .and_then(|r| r.worst_case.as_ref())
        .and_then(|w| w.large_fields.as_ref())
        .map(|large| large.bytes);
    // Held until this returns: the cache check, the generation and the
    // report's read of the output see one run's whole output.
    let _lock = match OutputLock::acquire(&options.out) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("meguri-cli generate: locking the output: {e}");
            return 1;
        }
    };
    if options.cache
        && let Some(have) = wanted.as_ref().and_then(|wanted| cached(&options.out, wanted))
    {
        return report(&options.out, true, &watch, &have, large_fields);
    }
    let result = match (request, recipe) {
        (Some(request), _) => generate::generate(&request, &options.out),
        (None, Some(recipe)) => generate_recipe(&recipe, options.seed, &options.out),
        (None, None) => unreachable!("one of them is set"),
    };
    match result {
        Ok(manifest) => report(&options.out, false, &watch, &manifest, large_fields),
        Err(e) => {
            eprintln!("meguri-cli generate: {e}");
            1
        }
    }
}

fn cloud_small(options: &Options, watch: &Stopwatch) -> i32 {
    let request = generate::cloud_small::Request::new(options.seed);
    let _lock = match OutputLock::acquire(&options.out) {
        Ok(lock) => lock,
        Err(e) => {
            eprintln!("meguri-cli generate: locking the output: {e}");
            return 1;
        }
    };
    if options.cache
        && let Some(have) = cached(&options.out, &request.manifest())
    {
        return report(&options.out, true, watch, &have, None);
    }
    match generate::cloud_small::generate(&request, &options.out) {
        Ok(manifest) => report(&options.out, false, watch, &manifest, None),
        Err(e) => {
            eprintln!("meguri-cli generate: {e}");
            1
        }
    }
}

fn report(dir: &Path, cached: bool, watch: &Stopwatch, manifest: &Manifest, large_fields: Option<u64>) -> i32 {
    let mut summary = json!({
        "dir": dir.display().to_string(),
        "cached": cached,
        "elapsed_ms": watch.elapsed_ms(),
        "peak_rss_kb": peak_rss_kb(),
        "manifest": manifest,
    });
    if manifest.kind == "cloud-small" {
        let observed = generate::cloud_small::readback(dir);
        let logical_hash = generate::dump::logical_sha256(&dir.join("collection.anki2"));
        match (observed, logical_hash) {
            (Ok(counts), Ok(hash)) if counts == manifest.counts && hash == manifest.dump_sha256 => {
                summary["request"] = json!(generate::cloud_small::Request::new(manifest.seed));
                summary["observed"] = json!({ "counts": counts, "dump_sha256": hash });
            }
            _ => {
                eprintln!("meguri-cli generate: cloud-small persisted output does not match its manifest");
                return 1;
            }
        }
    }
    if let Some(bytes) = large_fields {
        match generate::notes_with_field_of(&dir.join("collection.anki2"), bytes) {
            Ok(ids) => summary["large_field_notes"] = json!(ids),
            Err(e) => {
                eprintln!("meguri-cli generate: reading the large-field notes: {e}");
                return 1;
            }
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&summary).expect("a summary serializes")
    );
    0
}
