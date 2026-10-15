//! `meguri-cli generate`: synthetic collections (U11, R1804, R1805, R1963).
//!
//! ```text
//! meguri-cli generate <size> --out DIR [options]           10k, 100k, 150k, 500k, 1m or a card count
//! meguri-cli generate worst-case <name> --out DIR [options]
//! meguri-cli generate recipe <name|path.yaml> --out DIR [--seed N] [--cache]
//!
//! options: --seed N (default 1)  --profile PATH (default the user shape)
//!          --cards N (worst cases)  --now RFC3339 (default 2030-01-15T10:00:00Z)
//!          --media references|files  --cache (reuse DIR when its manifest matches)
//! ```
//!
//! A recipe is generated again every time, `--cache` or not: its manifest
//! does not identify the recipe's text, so an edited recipe would match an
//! old directory, and a recipe takes well under a second.
//!
//! Prints `{dir, cached, elapsed_ms, peak_rss_kb, manifest}` as JSON, plus
//! `large_field_notes` for a worst case with large fields: the ids of the
//! notes holding one, which `tools/mg fixtures-check` renders in the oracle
//! (F11). Exit 0 on success, 1 when generation fails, 2 for a usage error.

use std::path::{Path, PathBuf};

use meguri_testkit::generate::recipe::{Recipe, generate_recipe};
use meguri_testkit::generate::{
    self, DEFAULT_NOW_MS, GENERATOR_VERSION, Manifest, MediaMode, Profile, Request, WorstCase, parse_size,
};
use meguri_testkit::wallclock::Stopwatch;
use serde_json::json;

enum What {
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
        "usage: meguri-cli generate <size>|worst-case <name>|recipe <name> --out DIR [--seed N] [--profile PATH] \
         [--cards N] [--now RFC3339] [--media references|files] [--cache]"
    );
    2
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut rest = args.iter();
    let what = match rest.next().map(String::as_str) {
        Some("worst-case") => What::WorstCase(rest.next().ok_or("worst-case needs a name")?.clone()),
        Some("recipe") => What::Recipe(rest.next().ok_or("recipe needs a name")?.clone()),
        Some(size) => What::Size(parse_size(size).ok_or_else(|| format!("not a size: {size}"))?),
        None => return Err("nothing to generate".to_owned()),
    };
    let mut options = Options {
        what,
        out: PathBuf::new(),
        seed: 1,
        profile: None,
        cards: None,
        now_ms: DEFAULT_NOW_MS,
        media: MediaMode::References,
        cache: false,
    };
    while let Some(flag) = rest.next() {
        let mut value = || rest.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--out" => options.out = PathBuf::from(value()?),
            "--seed" => options.seed = value()?.parse().map_err(|_| "--seed takes an integer")?,
            "--profile" => options.profile = Some(PathBuf::from(value()?)),
            "--cards" => options.cards = Some(parse_size(&value()?).ok_or("--cards takes a size")?),
            "--now" => {
                let text = value()?;
                let ts: jiff::Timestamp = text.parse().map_err(|e| format!("--now {text}: {e}"))?;
                options.now_ms = ts.as_millisecond();
            }
            "--media" => {
                options.media = match value()?.as_str() {
                    "references" => MediaMode::References,
                    "files" => MediaMode::Files,
                    other => return Err(format!("--media is references or files, not {other}")),
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
        && have.media == wanted.media
        && dir.join("collection.anki2").is_file();
    same.then_some(have)
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
                    let case = match WorstCase::named(name) {
                        Ok(case) => case,
                        Err(e) => return usage(&format!("worst case {name}: {e}")),
                    };
                    let cards = options.cards.unwrap_or(case.cards);
                    (Some(case), cards, "worst-case", name.clone())
                }
                What::Size(cards) => (None, *cards, "profile", cards.to_string()),
                What::Recipe(_) => unreachable!("handled above"),
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

fn report(dir: &Path, cached: bool, watch: &Stopwatch, manifest: &Manifest, large_fields: Option<u64>) -> i32 {
    let mut summary = json!({
        "dir": dir.display().to_string(),
        "cached": cached,
        "elapsed_ms": watch.elapsed_ms(),
        "peak_rss_kb": peak_rss_kb(),
        "manifest": manifest,
    });
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
