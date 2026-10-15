//! Overlay goldens (specs/conformance/float-determinism.md F7-F9): the values
//! where the macOS native oracle differs from a Linux golden, the
//! `fp-libm-boundary` class those differences must belong to, and how the
//! macOS runner puts them into the expected observations. goldens-check, the
//! runner and the probe report share these checks.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::diff::pointer;
use crate::scenario::{FormatError, FsrsNextRequest, ObservationArgs, ObservationKind, Scenario, Step};

/// The overlay directory of a conformance tree.
pub const DIR: &str = "goldens-apple";

/// The `platform` of every overlay (F6.2).
pub const PLATFORM: &str = "macos-arm64";

/// Relative tolerance of the two unrounded intervals of a day entry and of
/// their distance to a half (F8.2 rules 2 and 4), and of a difficulty entry
/// (F8.3).
pub const CLASS_TOLERANCE: f64 = 1e-5;

/// Relative tolerance of a stability entry (F8.3).
pub const STABILITY_TOLERANCE: f64 = 1e-4;

/// Absolute floor of a stability entry's tolerance, in days (F8.3).
pub const STABILITY_FLOOR_DAYS: f64 = 1e-5;

/// How close a recorded unrounded interval must be to the recomputed one
/// (F8.2 rule 3).
pub const UNROUNDED_TOLERANCE: f64 = 1e-9;

/// The `n` FSRS divides by, 0.9 as an `f32` (F8.2 rule 3).
const FSRS_N: f64 = 0.9f32 as f64;

/// An overlay file (overlay.schema.json).
#[derive(Debug, Clone, PartialEq)]
pub struct Overlay {
    pub scenario_id: String,
    pub scenario_sha256: String,
    pub linux_observations_sha256: String,
    pub oracle: OverlayOracle,
    pub entries: Vec<Entry>,
}

/// The Apple run that recorded an overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayOracle {
    pub anki_version: String,
    /// sha256 of oracle/macos/requirements-macos.lock.
    pub lock_sha256: String,
    pub driver_sha256: String,
    pub platform: String,
}

/// One differing scalar (F7.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// RFC 6901 pointer into the observations.
    pub pointer: String,
    pub linux: Value,
    pub apple: Value,
    /// Day entries only: the rating's unrounded interval on each side.
    pub unrounded: Option<Unrounded>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unrounded {
    pub linux: f64,
    pub apple: f64,
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `^/steps/(0|[1-9][0-9]*)(/[^/]*)+$`.
fn pointer_shape(text: &str) -> bool {
    let Some((index, _)) = text.strip_prefix("/steps/").and_then(|rest| rest.split_once('/')) else {
        return false;
    };
    index == "0" || (!index.is_empty() && !index.starts_with('0') && index.bytes().all(|b| b.is_ascii_digit()))
}

fn members<'a>(
    value: &'a Value,
    what: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let map = value.as_object().ok_or_else(|| format!("{what} is not an object"))?;
    if let Some(missing) = required.iter().find(|k| !map.contains_key(**k)) {
        return Err(format!("{what} has no {missing}"));
    }
    if let Some(extra) = map
        .keys()
        .find(|k| !required.contains(&k.as_str()) && !optional.contains(&k.as_str()))
    {
        return Err(format!("{what} has an unknown member {extra:?}"));
    }
    Ok(map)
}

fn string<'a>(map: &'a Map<String, Value>, key: &str, what: &str) -> Result<&'a str, String> {
    map[key].as_str().ok_or_else(|| format!("{what}.{key} is not a string"))
}

fn sha(map: &Map<String, Value>, key: &str, what: &str) -> Result<String, String> {
    let text = string(map, key, what)?;
    if is_sha256(text) {
        Ok(text.to_owned())
    } else {
        Err(format!("{what}.{key} is not a lower-case hex sha256"))
    }
}

impl Overlay {
    /// Reads an overlay with overlay.schema.json's rules, every float exactly
    /// (`observe::parse_json`).
    pub fn parse(text: &str) -> Result<Overlay, String> {
        let value = super::parse_json(text).map_err(|e| e.to_string())?;
        Overlay::from_value(&value)
    }

    pub fn from_value(value: &Value) -> Result<Overlay, String> {
        let map = members(
            value,
            "the overlay",
            &[
                "scenario_id",
                "scenario_sha256",
                "linux_observations_sha256",
                "oracle",
                "entries",
            ],
            &[],
        )?;
        let scenario_id = string(map, "scenario_id", "the overlay")?;
        if scenario_id.is_empty() {
            return Err("scenario_id is empty".to_owned());
        }
        let oracle = members(
            &map["oracle"],
            "oracle",
            &["anki_version", "lock_sha256", "driver_sha256", "platform"],
            &[],
        )?;
        let platform = string(oracle, "platform", "oracle")?;
        if platform != PLATFORM {
            return Err(format!("oracle.platform is {platform:?}, not {PLATFORM:?}"));
        }
        let items = map["entries"]
            .as_array()
            .filter(|e| !e.is_empty())
            .ok_or("entries is not a list of at least one entry")?;
        let mut entries = Vec::with_capacity(items.len());
        for (i, item) in items.iter().enumerate() {
            let what = format!("entries/{i}");
            let e = members(item, &what, &["pointer", "linux", "apple"], &["unrounded"])?;
            let pointer = string(e, "pointer", &what)?;
            if !pointer_shape(pointer) {
                return Err(format!(
                    "{what}.pointer {pointer:?} is not a pointer into /steps/<i>/..."
                ));
            }
            for side in ["linux", "apple"] {
                if e[side].is_array() || e[side].is_object() {
                    return Err(format!("{what}.{side} is not a scalar"));
                }
            }
            let unrounded = match e.get("unrounded") {
                None => None,
                Some(u) => {
                    let u = members(u, &format!("{what}.unrounded"), &["linux", "apple"], &[])?;
                    let side = |key: &str| {
                        u[key]
                            .as_f64()
                            .filter(|x| *x > 0.0)
                            .ok_or_else(|| format!("{what}.unrounded.{key} is not a number above 0"))
                    };
                    Some(Unrounded {
                        linux: side("linux")?,
                        apple: side("apple")?,
                    })
                }
            };
            entries.push(Entry {
                pointer: pointer.to_owned(),
                linux: e["linux"].clone(),
                apple: e["apple"].clone(),
                unrounded,
            });
        }
        Ok(Overlay {
            scenario_id: scenario_id.to_owned(),
            scenario_sha256: sha(map, "scenario_sha256", "the overlay")?,
            linux_observations_sha256: sha(map, "linux_observations_sha256", "the overlay")?,
            oracle: OverlayOracle {
                anki_version: string(oracle, "anki_version", "oracle")?.to_owned(),
                lock_sha256: sha(oracle, "lock_sha256", "oracle")?,
                driver_sha256: sha(oracle, "driver_sha256", "oracle")?,
                platform: platform.to_owned(),
            },
            entries,
        })
    }

    /// Entries per kind: (day, memory, outside the class's pointers).
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut counts = (0, 0, 0);
        for entry in &self.entries {
            match target(&entry.pointer).map(|t| t.kind) {
                Some(Kind::Day) => counts.0 += 1,
                Some(Kind::Memory(_)) => counts.1 += 1,
                None => counts.2 += 1,
            }
        }
        counts
    }
}

/// F7.1: the lower-case hex SHA-256 of the JCS text of a golden's
/// observations.
pub fn observations_sha256(observations: &Value) -> Result<String, FormatError> {
    let text = super::jcs(observations)?;
    Ok(Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// F7.3: why the overlay is stale, or `None` when it is fresh.
/// `macos_lock_sha256` is `conformance/oracle.json`'s, `None` when it has
/// none.
pub fn staleness(
    overlay: &Overlay,
    scenario_sha256: &str,
    linux_observations_sha256: &str,
    anki_version: &str,
    macos_lock_sha256: Option<&str>,
) -> Option<String> {
    let short = |h: &str| h.get(..12).unwrap_or(h).to_owned();
    if overlay.scenario_sha256 != scenario_sha256 {
        return Some("the scenario changed since the overlay was recorded (scenario_sha256 differs)".to_owned());
    }
    if overlay.linux_observations_sha256 != linux_observations_sha256 {
        return Some(
            "the Linux golden's observations changed since the overlay was recorded (linux_observations_sha256 differs)"
                .to_owned(),
        );
    }
    if overlay.oracle.anki_version != anki_version {
        return Some(format!(
            "the overlay was recorded with Anki {}, the pin is {anki_version}",
            overlay.oracle.anki_version
        ));
    }
    match macos_lock_sha256 {
        None => Some("conformance/oracle.json has no macos_lock_sha256, so no overlay is fresh".to_owned()),
        Some(lock) if lock != overlay.oracle.lock_sha256 => Some(format!(
            "the overlay was recorded with macOS lock {}, the pin is {} (conformance/oracle.json macos_lock_sha256)",
            short(&overlay.oracle.lock_sha256),
            short(lock)
        )),
        Some(_) => None,
    }
}

/// What is wrong with one entry (F7.4, F8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryProblem {
    pub pointer: String,
    /// True for an entry outside the class of F8; false for the other
    /// checks of F7.4.
    pub outside_class: bool,
    pub message: String,
}

/// The F7.4 entry checks and the class of F8, against the Linux golden's
/// observations and the scenario: every problem, those of F7.4 first (in
/// entry order), then the entries outside the class. An entry that fails
/// F7.4 is not checked against the class.
pub fn check_entries(overlay: &Overlay, observations: &Value, scenario: &Scenario) -> Vec<EntryProblem> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    let mut previous: Option<&str> = None;
    let mut sound = Vec::with_capacity(overlay.entries.len());
    for entry in &overlay.entries {
        let problem = |message: String| EntryProblem {
            pointer: entry.pointer.clone(),
            outside_class: false,
            message,
        };
        let before = problems.len();
        match observations.pointer(&entry.pointer) {
            Some(Value::Array(_) | Value::Object(_)) | None => problems.push(problem(
                "the pointer does not lead to a scalar of the Linux golden".to_owned(),
            )),
            Some(value) if *value != entry.linux => problems.push(problem(format!(
                "linux is {}, the Linux golden has {value}",
                entry.linux
            ))),
            Some(_) => {}
        }
        if entry.apple == entry.linux {
            problems.push(problem("apple equals linux".to_owned()));
        }
        if !seen.insert(entry.pointer.as_str()) {
            problems.push(problem("the pointer appears twice".to_owned()));
        } else if let Some(previous) = previous
            && pointer_order(observations, previous, &entry.pointer) != Ordering::Less
        {
            problems.push(problem(format!("out of pointer order after {previous}")));
        }
        previous = Some(&entry.pointer);
        sound.push(problems.len() == before);
    }
    let class = Class::new(overlay, observations, scenario);
    for (entry, sound) in overlay.entries.iter().zip(sound) {
        if !sound {
            continue;
        }
        if let Err(why) = class.check(entry) {
            problems.push(EntryProblem {
                pointer: entry.pointer.clone(),
                outside_class: true,
                message: why,
            });
        }
    }
    problems
}

/// Pointer order (observations.md O7): members by key, items by index, read
/// from the shape of `observations`.
fn pointer_order(observations: &Value, a: &str, b: &str) -> Ordering {
    let (a, b) = (pointer::parse(a), pointer::parse(b));
    let mut node = Some(observations);
    for (x, y) in a.iter().zip(&b) {
        if x != y {
            return match node {
                Some(Value::Array(_)) => match (x.parse::<usize>(), y.parse::<usize>()) {
                    (Ok(i), Ok(j)) => i.cmp(&j),
                    _ => x.cmp(y),
                },
                _ => x.cmp(y),
            };
        }
        node = node.and_then(|n| match n {
            Value::Array(items) => x.parse::<usize>().ok().and_then(|i| items.get(i)),
            Value::Object(map) => map.get(x),
            _ => None,
        });
    }
    a.len().cmp(&b.len())
}

/// Puts each entry's `apple` value at its pointer in `expected` when the
/// pointer lies at or under a match of `pattern` (dsl.md D13
/// `apple_overlay`); the number of entries applied.
pub fn apply(entries: &[Entry], pattern: &[String], expected: &mut Value) -> usize {
    let mut applied = 0;
    for entry in entries {
        let segments = pointer::parse(&entry.pointer);
        let under = pattern.len() <= segments.len() && pattern.iter().zip(&segments).all(|(p, s)| p == "*" || p == s);
        if !under {
            continue;
        }
        if let Some(slot) = expected.pointer_mut(&entry.pointer) {
            *slot = entry.apple.clone();
            applied += 1;
        }
    }
    applied
}

/// The ratings of a next-state set, in the order their bounds build on each
/// other (sched/fsrs-transitions T5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rating {
    Again,
    Hard,
    Good,
    Easy,
}

impl Rating {
    fn from_name(name: &str) -> Option<Rating> {
        match name {
            "again" => Some(Rating::Again),
            "hard" => Some(Rating::Hard),
            "good" => Some(Rating::Good),
            "easy" => Some(Rating::Easy),
            _ => None,
        }
    }

    fn from_column(column: usize) -> Rating {
        [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy][column / 3]
    }

    /// The rating whose day entry in the class carries this one's (F8.2
    /// rule 4): Good follows Hard, Easy follows Good.
    fn follows(self) -> Option<Rating> {
        match self {
            Rating::Good => Some(Rating::Hard),
            Rating::Easy => Some(Rating::Good),
            Rating::Again | Rating::Hard => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Day,
    Memory(Member),
}

/// The member of a memory entry, which picks its bound (F8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    Stability,
    Difficulty,
}

impl Member {
    fn from_name(name: &str) -> Option<Member> {
        match name {
            "stability" => Some(Member::Stability),
            "difficulty" => Some(Member::Difficulty),
            _ => None,
        }
    }
}

/// Where an entry sits in the class's pointers (F8.1).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    /// The next-state set: `/steps/<i>/next_states/<#cN>` or
    /// `/steps/<i>/fsrs_next/rows/<k>`.
    set: String,
    step: usize,
    /// The `fsrs_next` row and column.
    row: Option<(usize, usize)>,
    rating: Rating,
    kind: Kind,
}

fn target(pointer: &str) -> Option<Target> {
    let segments = pointer::parse(pointer);
    let [steps, step, kind, rest @ ..] = segments.as_slice() else {
        return None;
    };
    if steps != "steps" {
        return None;
    }
    let step: usize = step.parse().ok()?;
    match (kind.as_str(), rest) {
        ("next_states", [_card, rating, tail @ ..]) => {
            let rating = Rating::from_name(rating)?;
            let tail: Vec<&str> = tail.iter().map(String::as_str).collect();
            let kind = match tail.as_slice() {
                ["scheduled_days"] | ["review", "scheduled_days"] => Kind::Day,
                ["memory_state", member] | ["learning" | "review", "memory_state", member] => {
                    Kind::Memory(Member::from_name(member)?)
                }
                _ => return None,
            };
            Some(Target {
                set: pointer::render(&segments[..4]),
                step,
                row: None,
                rating,
                kind,
            })
        }
        ("fsrs_next", [rows, row, column]) if rows == "rows" => {
            let row = (row == "0" || !row.starts_with('0'))
                .then(|| row.parse::<usize>().ok())
                .flatten()?;
            let column = super::fsrs_next_column(column)?;
            Some(Target {
                set: pointer::render(&segments[..5]),
                step,
                row: Some((row, column)),
                rating: Rating::from_column(column),
                kind: match column % 3 {
                    0 => Kind::Day,
                    1 => Kind::Memory(Member::Stability),
                    _ => Kind::Memory(Member::Difficulty),
                },
            })
        }
        _ => None,
    }
}

/// `sched/fuzz` Z3 in 64-bit float (F8.2 rule 4).
fn delta_f64(u: f64) -> f64 {
    if u < 2.5 {
        return 0.0;
    }
    let term = |start: f64, end: f64, factor: f64| factor * (u.min(end) - start).max(0.0);
    1.0 + term(2.5, 7.0, 0.15) + term(7.0, 20.0, 0.1) + term(20.0, f64::MAX, 0.05)
}

/// Whether some point of `{u, u - Δ(u), u + Δ(u)}` lies within
/// `1e-5 × max(1, p)` of a half-integer (F8.2 rule 4).
fn near_a_half(u: f64) -> bool {
    let delta = delta_f64(u);
    [u, u - delta, u + delta]
        .into_iter()
        .any(|p| (p - (p.floor() + 0.5)).abs() <= CLASS_TOLERANCE * p.max(1.0))
}

/// The decay the FSRS model uses for a parameter list (sched/fsrs-transitions
/// T2.1).
fn decay(params: &[f32]) -> Option<f32> {
    match params.len() {
        0 => Some(0.1542),
        17 | 19 => Some(0.5),
        21 => Some(params[20].clamp(0.1, 0.8)),
        _ => None,
    }
}

/// F8.2 rule 3: the unrounded interval for stability `s` at desired
/// retention `r` and decay `w`, in 64-bit float.
fn unrounded_interval(s: f64, r: f64, w: f64) -> f64 {
    s * (r.powf(-1.0 / w) - 1.0) / (FSRS_N.powf(-1.0 / w) - 1.0)
}

fn relative(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs())
}

/// The class of F8 for the entries of one overlay.
struct Class<'a> {
    observations: &'a Value,
    scenario: &'a Scenario,
    /// Entries by pointer, for the Apple stability of a day entry.
    by_pointer: BTreeMap<&'a str, &'a Entry>,
    /// Day entries in the class, by set and rating (rule 4's second case).
    days_in_class: BTreeSet<(String, Rating)>,
}

impl<'a> Class<'a> {
    fn new(overlay: &'a Overlay, observations: &'a Value, scenario: &'a Scenario) -> Class<'a> {
        let mut class = Class {
            observations,
            scenario,
            by_pointer: overlay.entries.iter().map(|e| (e.pointer.as_str(), e)).collect(),
            days_in_class: BTreeSet::new(),
        };
        // Rule 4 lets Good follow Hard and Easy follow Good: settle the day
        // entries rating by rating.
        let mut days: Vec<(&Entry, Target)> = overlay
            .entries
            .iter()
            .filter_map(|e| target(&e.pointer).filter(|t| t.kind == Kind::Day).map(|t| (e, t)))
            .collect();
        days.sort_by_key(|(_, t)| t.rating);
        for (entry, t) in days {
            if class.day(entry, &t).is_ok() {
                class.days_in_class.insert((t.set.clone(), t.rating));
            }
        }
        class
    }

    fn check(&self, entry: &Entry) -> Result<(), String> {
        let Some(t) = target(&entry.pointer) else {
            return Err("not a next-state day or memory value (F8.1)".to_owned());
        };
        match t.kind {
            Kind::Memory(member) => memory(entry, member),
            Kind::Day if self.days_in_class.contains(&(t.set.clone(), t.rating)) => Ok(()),
            Kind::Day => self.day(entry, &t),
        }
    }

    /// F8.2.
    fn day(&self, entry: &Entry, t: &Target) -> Result<(), String> {
        let (Some(linux), Some(apple)) = (integer(&entry.linux), integer(&entry.apple)) else {
            return Err("a day entry's values are integers (F8.2 rule 1)".to_owned());
        };
        if linux.abs_diff(apple) != 1 {
            return Err(format!(
                "{linux} and {apple} days are {} apart, not 1 (F8.2 rule 1)",
                linux.abs_diff(apple)
            ));
        }
        let Some(u) = entry.unrounded else {
            return Err("a day entry needs its unrounded intervals (F8.2 rule 2)".to_owned());
        };
        if !(u.linux.is_finite() && u.apple.is_finite() && u.linux > 0.0 && u.apple > 0.0)
            || (u.apple - u.linux).abs() > CLASS_TOLERANCE * u.linux
        {
            return Err(format!(
                "the unrounded intervals {} and {} differ by more than relative 1e-5 (F8.2 rule 2)",
                u.linux, u.apple
            ));
        }
        if let Some((row, column)) = t.row {
            self.recomputed(t.step, row, column, u)?;
        }
        if near_a_half(u.linux) || near_a_half(u.apple) {
            return Ok(());
        }
        if t.rating
            .follows()
            .is_some_and(|before| self.days_in_class.contains(&(t.set.clone(), before)))
        {
            return Ok(());
        }
        Err(format!(
            "no rounding boundary explains it: no point of B({}) or B({}) is within 1e-5 of a half-integer, and no earlier rating's day entry in this set is in the class (F8.2 rule 4)",
            u.linux, u.apple
        ))
    }

    /// F8.2 rule 3 for an `fsrs_next` day entry: both unrounded intervals
    /// recomputed from the input, its parameter set and each side's
    /// stability of the same rating.
    fn recomputed(&self, step: usize, row: usize, column: usize, u: Unrounded) -> Result<(), String> {
        let request = fsrs_next_request(self.scenario, step)
            .ok_or_else(|| format!("step {step} has no fsrs_next request (F8.2 rule 3)"))?;
        let input = request
            .inputs
            .get(row)
            .ok_or_else(|| format!("the fsrs_next request has no input {row} (F8.2 rule 3)"))?;
        let w = request
            .param_sets
            .get(&input.params)
            .and_then(|p| decay(p))
            .ok_or_else(|| format!("no decay for parameter set {:?} (F8.2 rule 3)", input.params))?;
        let stability_pointer = format!("/steps/{step}/fsrs_next/rows/{row}/{}", column + 1);
        let linux_s = self
            .observations
            .pointer(&stability_pointer)
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("no stability at {stability_pointer} (F8.2 rule 3)"))?;
        let apple_s = match self.by_pointer.get(stability_pointer.as_str()) {
            Some(e) => e
                .apple
                .as_f64()
                .ok_or_else(|| format!("the Apple stability at {stability_pointer} is not a number"))?,
            None => linux_s,
        };
        let (r, w) = (f64::from(input.dr), f64::from(w));
        for (side, s, recorded) in [("linux", linux_s, u.linux), ("apple", apple_s, u.apple)] {
            let expected = unrounded_interval(s, r, w);
            if relative(recorded, expected) > UNROUNDED_TOLERANCE {
                return Err(format!(
                    "unrounded.{side} {recorded} is not the interval of stability {s} at retention {r} and decay {w}, {expected} (F8.2 rule 3)"
                ));
            }
        }
        Ok(())
    }
}

/// F8.3: a stability may differ by relative 1e-4 or by 1e-5 days, whichever
/// is larger; a difficulty by relative 1e-5.
fn memory(entry: &Entry, member: Member) -> Result<(), String> {
    let fraction = |v: &Value| v.as_number().filter(|n| n.is_f64()).and_then(|n| n.as_f64());
    let (Some(linux), Some(apple)) = (fraction(&entry.linux), fraction(&entry.apple)) else {
        return Err("a memory entry's values are numbers with a fraction or an exponent (F8.3)".to_owned());
    };
    let apart = (apple - linux).abs();
    let m = linux.abs().max(apple.abs());
    let rel = apart / m;
    match member {
        Member::Stability if apart > (STABILITY_TOLERANCE * m).max(STABILITY_FLOOR_DAYS) => Err(format!(
            "a stability: {linux} and {apple} are {apart:.2e} days apart (relative {rel:.2e}), more than both relative 1e-4 and 1e-5 days (F8.3)"
        )),
        Member::Difficulty if apart > CLASS_TOLERANCE * m => Err(format!(
            "a difficulty: {linux} and {apple} differ by relative {rel:.2e}, more than 1e-5 (F8.3)"
        )),
        Member::Stability | Member::Difficulty => Ok(()),
    }
}

/// An integer JSON value (no fraction or exponent).
fn integer(value: &Value) -> Option<i64> {
    let n = value.as_number()?;
    n.as_i64().or_else(|| n.as_u64().and_then(|u| i64::try_from(u).ok()))
}

/// The `fsrs_next` request of a scenario step, if it has one.
fn fsrs_next_request(scenario: &Scenario, step: usize) -> Option<FsrsNextRequest> {
    let Some(Step::Observe(requests)) = scenario.steps.get(step) else {
        return None;
    };
    requests.iter().find_map(|r| match (&r.kind, &r.args) {
        (ObservationKind::FsrsNext, ObservationArgs::Mapping(map)) => FsrsNextRequest::from_mapping(map).ok(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointers_take_the_schema_shape() {
        for ok in [
            "/steps/0/fsrs_next/rows/417/6",
            "/steps/12/next_states/#c1/good/scheduled_days",
            "/steps/3/x/",
        ] {
            assert!(pointer_shape(ok), "{ok}");
        }
        for bad in [
            "/steps/01/x",
            "/steps/0",
            "/steps/",
            "steps/0/x",
            "/stepz/0/x",
            "/steps/a/x",
        ] {
            assert!(!pointer_shape(bad), "{bad}");
        }
    }

    #[test]
    fn the_delta_in_64_bit_float_matches_f8_4() {
        assert!((delta_f64(14.972222) - 2.472222).abs() < 1e-6);
        assert!((delta_f64(15.2) - 2.495).abs() < 1e-12);
        assert!(near_a_half(15.499999046325684));
        assert!(!near_a_half(15.2));
        assert!(near_a_half(14.972222));
    }
}
