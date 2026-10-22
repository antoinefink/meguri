//! Overlay goldens (specs/conformance/float-determinism.md F7-F9): how the
//! runner applies them on macOS only, their freshness and checks in
//! goldens-check, and the `fp-libm-boundary` class with the examples of
//! F8.4. Fixtures are copies of committed scenarios and goldens in a
//! temporary tree; overlays are written here, or the committed ones copied.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor, Needs};
use meguri_testkit::observe::overlay::{self, Overlay};
use meguri_testkit::scenario::{self, LoadedScenario};
use meguri_testkit::suite::{GoldensCheck, Platform, Status, Suite};
use serde_json::{Value, json};

const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";
const MACOS_LOCK: &str = "869b29c42af493bd7b46960d5ee4e72e99ff92bfc8b2a10c190dc54a5513a244";
const SMOKE: &str = "smoke/fsrs-new-card";
const EDGES: &str = "fp-probe/next-states-edges";
/// smoke/fsrs-new-card's review card (sched/fsrs-transitions T6).
const SET: &str = "/steps/6/next_states/#c1";

fn repo_conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read_json(path: &Path) -> Value {
    meguri_testkit::observe::parse_json(&fs::read_to_string(path).unwrap()).unwrap()
}

/// A tree with the pin of both lanes and the committed deviations list,
/// which holds the `fp-libm-boundary` entry.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("scenarios")).unwrap();
    fs::create_dir_all(root.join("goldens")).unwrap();
    write(
        root,
        "oracle.json",
        &format!(
            "{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\", \"macos_lock_sha256\": \"{MACOS_LOCK}\"}}\n"
        ),
    );
    write(
        root,
        "deviations.yaml",
        &fs::read_to_string(repo_conformance().join("deviations.yaml")).unwrap(),
    );
    dir
}

/// Copies a committed scenario and its golden into the tree.
fn copy(root: &Path, rel: &str) {
    let scenarios = repo_conformance().join("scenarios");
    for suffix in [".yaml", ".json"] {
        let from = scenarios.join(format!("{rel}{suffix}"));
        if from.is_file() {
            write(
                root,
                &format!("scenarios/{rel}{suffix}"),
                &fs::read_to_string(from).unwrap(),
            );
        }
    }
    let golden = format!("goldens/{rel}.json");
    write(
        root,
        &golden,
        &fs::read_to_string(repo_conformance().join(&golden)).unwrap(),
    );
}

fn golden_observations(root: &Path, rel: &str) -> Value {
    read_json(&root.join(format!("goldens/{rel}.json")))["observations"].clone()
}

/// Writes a fresh overlay for a scenario of the tree with these entries.
fn write_overlay(root: &Path, rel: &str, entries: Value) {
    let golden = read_json(&root.join(format!("goldens/{rel}.json")));
    let overlay = json!({
        "entries": entries,
        "linux_observations_sha256": overlay::observations_sha256(&golden["observations"]).unwrap(),
        "oracle": {"anki_version": "26.9.3", "driver_sha256": "0".repeat(64), "lock_sha256": MACOS_LOCK, "platform": "macos-arm64"},
        "scenario_id": rel,
        "scenario_sha256": golden["scenario_sha256"],
    });
    write(
        root,
        &format!("goldens-apple/{rel}.json"),
        &format!("{}\n", serde_json::to_string_pretty(&overlay).unwrap()),
    );
}

/// The Good answer of T6 a day later on the Apple side, at a rounding
/// boundary, and its stability one ulp up.
fn good_flip() -> Value {
    json!([
        {"pointer": format!("{SET}/good/memory_state/stability"), "linux": 13.836124, "apple": 13.836125},
        {"pointer": format!("{SET}/good/scheduled_days"), "linux": 15, "apple": 16,
         "unrounded": {"linux": 15.499999046325684, "apple": 15.500000953674316}},
    ])
}

/// An executor that answers with fixed observations.
struct Canned(Value);

impl Executor for Canned {
    fn name(&self) -> &str {
        "canned"
    }
    fn missing(&self, _needs: &Needs) -> Needs {
        Needs::default()
    }
    fn run(&self, _scenario: &LoadedScenario) -> Result<Value, ExecError> {
        Ok(self.0.clone())
    }
}

fn with_values(mut observations: Value, entries: &Value, side: &str) -> Value {
    for entry in entries.as_array().unwrap() {
        *observations.pointer_mut(entry["pointer"].as_str().unwrap()).unwrap() = entry[side].clone();
    }
    observations
}

fn suite(root: &Path, platform: Platform) -> Suite {
    Suite::open(root).unwrap().on_platform(platform)
}

#[test]
fn overlay_applies_on_macos_only() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    write_overlay(root, SMOKE, good_flip());
    let linux = golden_observations(root, SMOKE);
    let apple = Canned(with_values(linux.clone(), &good_flip(), "apple"));
    let anki_on_linux = Canned(linux);

    // macOS: the overlay's values are expected.
    let report = suite(root, Platform::MacOs).run(&[], &apple, "native");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Pass, "{}", outcome.render_text());
    assert_eq!(outcome.overlay_applied, Some(2));
    assert_eq!(outcome.deviations_applied, ["fp-libm-boundary"]);
    let json = report.to_json();
    assert_eq!(json["totals"]["overlay_applied"], 2);
    assert_eq!(json["totals"]["overlay_scenarios"], 1);
    assert_eq!(json["scenarios"][0]["overlay_applied"], 2);
    assert!(report.render_text().contains("overlays: 2 values in 1 scenarios\n"));
    let report = suite(root, Platform::MacOs).run(&[], &anki_on_linux, "native");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Fail);
    let pointers: Vec<&str> = outcome.differences.iter().map(|d| d.pointer.as_str()).collect();
    // The stability differs by one ulp, inside the O7 tolerance.
    assert_eq!(pointers, [format!("{SET}/good/scheduled_days")]);
    assert_eq!(outcome.differences[0].expected, Some(json!(16)));

    // Linux: the golden's values, and no trace of the overlay.
    let report = suite(root, Platform::Linux).run(&[], &anki_on_linux, "docker");
    let outcome = &report.outcomes[0];
    assert_eq!(outcome.status, Status::Pass, "{}", outcome.render_text());
    assert_eq!(outcome.overlay_applied, None);
    assert!(outcome.deviations_applied.is_empty());
    let json = report.to_json();
    assert!(json["totals"].get("overlay_applied").is_none());
    assert!(json["scenarios"][0].get("overlay_applied").is_none());
    assert!(!report.render_text().contains("overlays:"));
    let outcome = &suite(root, Platform::Linux).run(&[], &apple, "docker").outcomes[0];
    assert_eq!(outcome.status, Status::Fail);
    assert_eq!(outcome.differences[0].expected, Some(json!(15)));
}

#[test]
fn the_linux_lane_never_reads_overlays_even_broken_ones() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    write(root, &format!("goldens-apple/{SMOKE}.json"), "{not json");
    let anki = Canned(golden_observations(root, SMOKE));
    assert_eq!(
        suite(root, Platform::Linux).run(&[], &anki, "docker").outcomes[0].status,
        Status::Pass
    );
    let outcome = &suite(root, Platform::MacOs).run(&[], &anki, "native").outcomes[0];
    assert_eq!(outcome.status, Status::Invalid);
    assert_eq!(
        outcome.repro,
        format!("tools/mg oracle record --platform apple {SMOKE}")
    );
    // goldens-check reads them on every platform.
    let fresh = suite(root, Platform::Linux)
        .goldens_check(&GoldensCheck::default())
        .unwrap();
    assert_eq!(kinds(&fresh), ["invalid_overlay"]);
}

#[test]
fn meguris_own_fsrs_next_run_takes_the_committed_overlay_on_macos() {
    let dir = tree();
    let root = dir.path();
    copy(root, EDGES);
    let committed = repo_conformance().join(format!("goldens-apple/{EDGES}.json"));
    write(
        root,
        &format!("goldens-apple/{EDGES}.json"),
        &fs::read_to_string(&committed).unwrap(),
    );
    let meguri = MeguriExecutor::new();
    let entries = Overlay::parse(&fs::read_to_string(&committed).unwrap())
        .unwrap()
        .entries
        .len();
    let outcome = &suite(root, Platform::MacOs).run(&[], &meguri, "native").outcomes[0];
    assert_eq!(outcome.status, Status::Pass, "{}", outcome.render_text());
    assert_eq!(outcome.overlay_applied, Some(entries));
    let outcome = &suite(root, Platform::Linux).run(&[], &meguri, "docker").outcomes[0];
    assert_eq!(outcome.status, Status::Pass, "{}", outcome.render_text());
    assert_eq!(outcome.overlay_applied, None);
}

#[test]
fn every_committed_overlay_parses_and_is_fresh_against_its_golden() {
    // The recorder hashes the golden's observations in Python; equal hashes
    // here mean both JCS writers agree. Every entry is in the class (F11).
    let suite = Suite::open(&repo_conformance()).unwrap();
    let ids: Vec<&str> = suite.overlay_ids().collect();
    assert!(!ids.is_empty());
    let fresh = suite.goldens_check(&GoldensCheck::default()).unwrap();
    let broken: Vec<_> = fresh
        .problems
        .iter()
        .filter(|p| {
            matches!(
                p.kind.as_str(),
                "invalid_overlay" | "stale_overlay" | "orphan_overlay" | "overlay_entry" | "outside_class"
            )
        })
        .collect();
    assert!(broken.is_empty(), "{broken:?}");
    assert_eq!(fresh.overlays.len(), ids.len());
    for id in ids {
        assert!(suite.oracle_sides(id).is_ok(), "{id}");
    }
}

fn kinds(fresh: &meguri_testkit::suite::Freshness) -> Vec<&str> {
    fresh.problems.iter().map(|p| p.kind.as_str()).collect()
}

#[test]
fn an_overlay_entry_three_days_apart_is_rejected_by_goldens_check() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    write_overlay(
        root,
        SMOKE,
        json!([{"pointer": format!("{SET}/good/scheduled_days"), "linux": 15, "apple": 18,
                "unrounded": {"linux": 15.499999046325684, "apple": 15.500000953674316}}]),
    );
    let fresh = suite(root, Platform::Linux)
        .goldens_check(&GoldensCheck::default())
        .unwrap();
    assert_eq!(kinds(&fresh), ["outside_class"]);
    assert!(
        fresh.problems[0].message.contains("3 apart"),
        "{}",
        fresh.problems[0].message
    );
    assert_eq!(fresh.overlays.len(), 1);
    assert_eq!(
        (fresh.overlays[0].day_entries, fresh.overlays[0].memory_entries),
        (1, 0)
    );
    // On macOS the runner refuses the scenario with the same check.
    let anki = Canned(golden_observations(root, SMOKE));
    let outcome = &suite(root, Platform::MacOs).run(&[], &anki, "native").outcomes[0];
    assert_eq!(outcome.status, Status::Invalid);
    assert!(outcome.message.contains("3 apart"), "{}", outcome.message);
}

#[test]
fn a_stale_or_orphaned_overlay_fails_goldens_check_and_is_stale_on_macos() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    write_overlay(root, SMOKE, good_flip());
    let anki = Canned(with_values(golden_observations(root, SMOKE), &good_flip(), "apple"));
    let stale = |root: &Path| {
        let fresh = suite(root, Platform::Linux)
            .goldens_check(&GoldensCheck::default())
            .unwrap();
        let outcome = suite(root, Platform::MacOs).run(&[], &anki, "native").outcomes[0].clone();
        (
            kinds(&fresh).into_iter().map(str::to_owned).collect::<Vec<_>>(),
            outcome,
        )
    };
    let (problems, outcome) = stale(root);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(outcome.status, Status::Pass);

    // The Linux golden's observations changed: re-recorded with another value.
    let golden_path = root.join(format!("goldens/{SMOKE}.json"));
    let mut golden = read_json(&golden_path);
    golden["observations"]["steps"][3]["card"]["#c1"]["due"] = json!(99);
    fs::write(&golden_path, serde_json::to_string_pretty(&golden).unwrap()).unwrap();
    let (problems, outcome) = stale(root);
    assert_eq!(problems, ["stale_overlay"]);
    assert_eq!(outcome.status, Status::Stale);
    assert!(
        outcome.message.contains("linux_observations_sha256"),
        "{}",
        outcome.message
    );
    assert_eq!(
        outcome.repro,
        format!("tools/mg oracle record --platform apple {SMOKE}")
    );
    write_overlay(root, SMOKE, good_flip());

    // Another macOS lock, then no macOS pin at all.
    write(
        root,
        "oracle.json",
        &format!(
            "{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\", \"macos_lock_sha256\": \"{}\"}}\n",
            "1".repeat(64)
        ),
    );
    let (problems, outcome) = stale(root);
    assert_eq!(problems, ["stale_overlay"]);
    assert!(outcome.message.contains("macOS lock"), "{}", outcome.message);
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}\n"),
    );
    let (problems, outcome) = stale(root);
    assert_eq!(problems, ["stale_overlay"]);
    assert!(outcome.message.contains("no macos_lock_sha256"), "{}", outcome.message);

    // Without its scenario, the overlay is orphaned.
    fs::remove_file(root.join(format!("scenarios/{SMOKE}.yaml"))).unwrap();
    let fresh = suite(root, Platform::Linux)
        .goldens_check(&GoldensCheck::default())
        .unwrap();
    assert!(kinds(&fresh).contains(&"orphan_overlay"), "{:?}", kinds(&fresh));
}

#[test]
fn overlays_that_break_the_schema_or_the_entry_rules_are_rejected() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    let path = root.join(format!("goldens-apple/{SMOKE}.json"));
    let schema_breaks = [
        ("platform", json!("linux-arm64")),
        ("entries", json!([])),
        ("extra", json!(1)),
        ("scenario_sha256", json!("ABC")),
    ];
    for (key, value) in schema_breaks {
        write_overlay(root, SMOKE, good_flip());
        let mut text = read_json(&path);
        if key == "platform" {
            text["oracle"]["platform"] = value;
        } else {
            text[key] = value;
        }
        fs::write(&path, serde_json::to_string(&text).unwrap()).unwrap();
        let fresh = suite(root, Platform::Linux)
            .goldens_check(&GoldensCheck::default())
            .unwrap();
        assert_eq!(kinds(&fresh), ["invalid_overlay"], "{key}");
    }
    let entry_breaks = [
        // Not where the golden has a scalar.
        json!([{"pointer": format!("{SET}/good"), "linux": 15, "apple": 16}]),
        // Not the golden's value.
        json!([{"pointer": format!("{SET}/good/scheduled_days"), "linux": 14, "apple": 15,
                "unrounded": {"linux": 14.5, "apple": 14.5}}]),
        // No difference.
        json!([{"pointer": format!("{SET}/good/memory_state/stability"), "linux": 13.836124, "apple": 13.836124}]),
        // Twice, then out of order.
        json!([{"pointer": format!("{SET}/good/memory_state/stability"), "linux": 13.836124, "apple": 13.836125},
               {"pointer": format!("{SET}/good/memory_state/stability"), "linux": 13.836124, "apple": 13.836125}]),
        json!([{"pointer": format!("{SET}/hard/memory_state/stability"), "linux": 9.240417, "apple": 9.240418},
               {"pointer": format!("{SET}/good/memory_state/stability"), "linux": 13.836124, "apple": 13.836125}]),
    ];
    for entries in entry_breaks {
        write_overlay(root, SMOKE, entries.clone());
        let fresh = suite(root, Platform::Linux)
            .goldens_check(&GoldensCheck::default())
            .unwrap();
        assert_eq!(kinds(&fresh), ["overlay_entry"], "{entries}");
        let anki = Canned(golden_observations(root, SMOKE));
        assert_eq!(
            suite(root, Platform::MacOs).run(&[], &anki, "native").outcomes[0].status,
            Status::Invalid,
            "{entries}"
        );
    }
}

/// The class check of one overlay of smoke/fsrs-new-card: the reason for
/// each entry outside it, `None` for entries in it.
fn class_of(entries: Value) -> Vec<Option<String>> {
    class_in(SMOKE, entries, false)
}

/// The class check of one overlay of the committed scenario `rel`. With
/// `golden_holds_linux`, the Linux golden first gets each entry's `linux`
/// value at its pointer, so that an example of F8.4 needs no golden with
/// that very value: F8.3 reads only the two values and the pointer.
fn class_in(rel: &str, entries: Value, golden_holds_linux: bool) -> Vec<Option<String>> {
    let dir = tree();
    let root = dir.path();
    copy(root, rel);
    if golden_holds_linux {
        let path = root.join(format!("goldens/{rel}.json"));
        let mut golden = read_json(&path);
        for entry in entries.as_array().unwrap() {
            *golden["observations"]
                .pointer_mut(entry["pointer"].as_str().unwrap())
                .unwrap() = entry["linux"].clone();
        }
        fs::write(&path, serde_json::to_string_pretty(&golden).unwrap()).unwrap();
    }
    write_overlay(root, rel, entries);
    let checked = Overlay::parse(&fs::read_to_string(root.join(format!("goldens-apple/{rel}.json"))).unwrap()).unwrap();
    let path = [".yaml", ".json"]
        .map(|suffix| root.join(format!("scenarios/{rel}{suffix}")))
        .into_iter()
        .find(|p| p.is_file())
        .unwrap();
    let loaded = scenario::load(&path).unwrap();
    let problems = overlay::check_entries(&checked, &golden_observations(root, rel), &loaded.scenario);
    checked
        .entries
        .iter()
        .map(|e| {
            problems
                .iter()
                .find(|p| p.pointer == e.pointer)
                .map(|p| format!("{}{}", if p.outside_class { "" } else { "entry: " }, p.message))
        })
        .collect()
}

fn day(rating: &str, linux: i64, apple: i64, u_linux: f64, u_apple: f64) -> Value {
    json!({"pointer": format!("{SET}/{rating}/scheduled_days"), "linux": linux, "apple": apple,
           "unrounded": {"linux": u_linux, "apple": u_apple}})
}

#[test]
fn the_class_follows_the_examples_of_f8_4() {
    // u lies on 15.5.
    assert_eq!(
        class_of(json!([day("good", 15, 16, 15.499999046325684, 15.500000953674316)])),
        [None]
    );
    // Three days apart.
    assert!(
        class_of(json!([day("good", 15, 18, 15.499999046325684, 15.500000953674316)]))[0]
            .as_deref()
            .is_some_and(|m| m.contains("3 apart"))
    );
    // B(15.2) is about {15.2, 12.705, 17.695}: no half nearby.
    assert!(
        class_of(json!([day("good", 15, 16, 15.2, 15.2)]))[0]
            .as_deref()
            .is_some_and(|m| m.contains("rule 4"))
    );
    // Δ(14.972222) is 2.472222, so u - Δ is 12.5.
    assert_eq!(class_of(json!([day("good", 15, 16, 14.972222, 14.972222)])), [None]);
    // Easy with no boundary of its own follows Good in the class, and is
    // outside it alone.
    let easy = day("easy", 26, 27, 23.9, 23.9);
    assert_eq!(
        class_of(json!([
            easy,
            day("good", 15, 16, 15.499999046325684, 15.500000953674316)
        ])),
        [None, None]
    );
    assert!(class_of(json!([easy]))[0].is_some());
    // Hard carries Good, but not Easy over a Good outside the class.
    assert_eq!(
        class_of(json!([
            day("good", 15, 16, 15.2, 15.2),
            day("hard", 10, 11, 10.5, 10.5)
        ])),
        [None, None]
    );
    assert_eq!(
        class_of(json!([easy, day("good", 15, 17, 15.5, 15.5)]))
            .iter()
            .filter(|r| r.is_some())
            .count(),
        2
    );
    // Memory: relative 7e-8 is in. The stability bound is the larger of
    // relative 1e-4 and 1e-5 days; the hard stability here is 9.240417.
    let memory = |rating: &str, linux: Value, apple: Value| json!({"pointer": format!("{SET}/{rating}/memory_state/stability"), "linux": linux, "apple": apple});
    assert_eq!(
        class_of(json!([memory("good", json!(13.836124), json!(13.836125))])),
        [None]
    );
    // Relative 4.1e-5 was outside the class as first written, and is in it
    // now; relative 2.3e-4 (2.1e-3 apart) is outside.
    assert_eq!(
        class_of(json!([memory("hard", json!(9.240417), json!(9.2408))])),
        [None]
    );
    assert!(
        class_of(json!([memory("hard", json!(9.240417), json!(9.2425))]))[0]
            .as_deref()
            .is_some_and(|m| m.contains("stability") && m.contains("1e-4") && m.contains("1e-5 days")),
        "{:?}",
        class_of(json!([memory("hard", json!(9.240417), json!(9.2425))]))
    );
    // Relearning parts count too; `current` is stored data, never a next
    // state, and card rows are not next states either.
    let again_s = json!({"pointer": format!("{SET}/again/learning/memory_state/stability"), "linux": 0.6369821, "apple": 0.6369822});
    let again_days = json!({"pointer": format!("{SET}/again/review/scheduled_days"), "linux": 1, "apple": 2,
                            "unrounded": {"linux": 1.4999999, "apple": 1.5000001}});
    let current = json!({"pointer": format!("{SET}/current/memory_state/stability"), "linux": 2.3065, "apple": 2.3066});
    let card = json!({"pointer": "/steps/6/card/#c1/data/s", "linux": 2.3065, "apple": 2.3066});
    // Entries in pointer order: card < next_states, learning < review.
    let classes = class_of(json!([card, again_s, again_days, current]));
    assert_eq!(classes[1..3], [None, None]);
    for outside in [&classes[0], &classes[3]] {
        assert!(outside.as_deref().is_some_and(|m| m.contains("F8.1")), "{classes:?}");
    }
    // A day entry needs its unrounded intervals, within relative 1e-5.
    let bare = json!({"pointer": format!("{SET}/good/scheduled_days"), "linux": 15, "apple": 16});
    assert!(
        class_of(json!([bare]))[0]
            .as_deref()
            .is_some_and(|m| m.contains("rule 2"))
    );
    assert!(
        class_of(json!([day("good", 15, 16, 15.4995, 15.5005)]))[0]
            .as_deref()
            .is_some_and(|m| m.contains("rule 2"))
    );
}

/// One memory entry of F8.4 at `pointer` of scenario `rel`, its class.
fn memory_class(rel: &str, pointer: &str, linux: Value, apple: Value, golden_holds_linux: bool) -> Option<String> {
    let entry = json!([{"pointer": pointer, "linux": linux, "apple": apple}]);
    class_in(rel, entry, golden_holds_linux).remove(0)
}

#[test]
fn memory_entries_take_the_stability_or_difficulty_bound_of_f8_3() {
    let corpus = |n: u32| format!("fp-probe/corpus/next-states-{n:03}");
    let stability = format!("{SET}/again/learning/memory_state/stability");
    let difficulty = format!("{SET}/again/learning/memory_state/difficulty");
    let inside = |rel: &str, pointer: &str, linux: Value, apple: Value, patch: bool| {
        let class = memory_class(rel, pointer, linux.clone(), apple.clone(), patch);
        assert_eq!(class, None, "{pointer} {linux} {apple}");
    };
    let outside = |rel: &str, pointer: &str, linux: Value, apple: Value, why: &[&str]| {
        let class = memory_class(rel, pointer, linux.clone(), apple.clone(), true);
        assert!(
            class
                .as_deref()
                .is_some_and(|m| !m.starts_with("entry:") && why.iter().all(|w| m.contains(w))),
            "{pointer} {linux} {apple}: {class:?}"
        );
    };

    // The committed corpus entries of F8.4, against their own goldens: an
    // again stability 3.9e-7 apart (relative 9.4e-5) is under the 1e-5
    // floor; an easy stability at relative 1.04e-5 is under 1e-4.
    inside(
        &corpus(29),
        "/steps/0/fsrs_next/rows/820/1",
        json!(0.0041170507),
        json!(0.0041174362),
        false,
    );
    inside(
        &corpus(89),
        "/steps/0/fsrs_next/rows/977/10",
        json!(59.598206),
        json!(59.598827),
        false,
    );
    // The column decides: the same values in the difficulty column next to
    // them are outside, since difficulty keeps relative 1e-5.
    outside(
        &corpus(29),
        "/steps/0/fsrs_next/rows/820/2",
        json!(0.0041170507),
        json!(0.0041174362),
        &["difficulty", "1e-5"],
    );
    outside(
        &corpus(89),
        "/steps/0/fsrs_next/rows/977/11",
        json!(59.598206),
        json!(59.598827),
        &["difficulty", "1e-5"],
    );
    // fsrs_next columns 4 and 7 are stabilities, 5 and 8 difficulties.
    inside(
        EDGES,
        "/steps/0/fsrs_next/rows/4/7",
        json!(13.836124),
        json!(13.836125),
        false,
    );
    inside(
        EDGES,
        "/steps/0/fsrs_next/rows/4/4",
        json!(9.240417),
        json!(9.2408),
        false,
    );
    outside(
        EDGES,
        "/steps/0/fsrs_next/rows/4/5",
        json!(4.7481427),
        json!(4.7484),
        &["difficulty"],
    );
    inside(
        EDGES,
        "/steps/0/fsrs_next/rows/4/8",
        json!(2.1041174),
        json!(2.1041175),
        false,
    );

    // The stability rows of F8.4, by value.
    inside(SMOKE, &stability, json!(0.0011), json!(0.0011005), true);
    inside(SMOKE, &stability, json!(2.3065), json!(2.3066), true);
    outside(SMOKE, &stability, json!(2.3065), json!(2.3075), &["stability", "1e-4"]);
    outside(
        SMOKE,
        &stability,
        json!(0.0041),
        json!(0.0042),
        &["stability", "1e-5 days"],
    );
    outside(SMOKE, &stability, json!(2), json!(2.0000001), &["fraction"]);
    // A stability under review/ takes the same bound.
    inside(
        SMOKE,
        &format!("{SET}/again/review/memory_state/stability"),
        json!(2.3065),
        json!(2.3066),
        true,
    );

    // The difficulty rows of F8.4: relative 5e-8 is in, 4.5e-5 is out.
    inside(SMOKE, &difficulty, json!(7.3921676), json!(7.392168), false);
    outside(
        SMOKE,
        &difficulty,
        json!(7.3921676),
        json!(7.3925),
        &["difficulty", "1e-5"],
    );
    outside(
        SMOKE,
        &format!("{SET}/good/memory_state/difficulty"),
        json!(2.1041174),
        json!(2.1042),
        &["difficulty"],
    );
}

#[test]
fn fsrs_next_day_entries_must_carry_the_recomputed_unrounded_intervals() {
    // Row 4 of next-states-edges: default parameters (decay 0.1542),
    // desired retention 0.9, Good stability 13.836124 (F3.5).
    let dir = tree();
    let root = dir.path();
    copy(root, EDGES);
    let n = f64::from(0.9f32);
    let w = f64::from(0.1542f32);
    let u = 13.836124f64 * (f64::from(0.9f32).powf(-1.0 / w) - 1.0) / (n.powf(-1.0 / w) - 1.0);
    let check = |u_recorded: f64| {
        let entry = json!([{"pointer": "/steps/0/fsrs_next/rows/4/6", "linux": 15, "apple": 16,
                            "unrounded": {"linux": u_recorded, "apple": u_recorded}}]);
        write_overlay(root, EDGES, entry);
        let fresh = suite(root, Platform::Linux)
            .goldens_check(&GoldensCheck::default())
            .unwrap();
        fresh
            .problems
            .iter()
            .map(|p| p.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    };
    // Recomputed right, the boundary rule decides: 13.84 has none nearby.
    let message = check(u);
    assert!(message.contains("rule 4") && !message.contains("rule 3"), "{message}");
    // Off by more than relative 1e-9, rule 3 rejects it.
    let message = check(u * (1.0 + 1e-8));
    assert!(message.contains("rule 3"), "{message}");
}

#[test]
fn goldens_check_compares_the_macos_pin_with_the_oracles_lock() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    let check = |macos: Option<Option<String>>| {
        let options = GoldensCheck {
            oracle_macos_lock_sha256: macos,
            ..GoldensCheck::default()
        };
        kinds(&Suite::open(root).unwrap().goldens_check(&options).unwrap())
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert!(check(None).is_empty());
    assert!(check(Some(Some(MACOS_LOCK.to_owned()))).is_empty());
    assert_eq!(check(Some(Some("1".repeat(64)))), ["pin_mismatch"]);
    assert_eq!(check(Some(None)), ["pin_mismatch"]);
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}\n"),
    );
    assert!(check(Some(None)).is_empty());
    assert_eq!(check(Some(Some(MACOS_LOCK.to_owned()))), ["pin_mismatch"]);
}

#[test]
fn apple_overlay_belongs_to_the_fp_libm_boundary_entry_only() {
    let dir = tree();
    let root = dir.path();
    let entry = |id: &str, on: bool| {
        format!(
            "deviations:\n  - id: {id}\n    requirement: R1889\n    summary: s\n    test: t\n    scenarios: [\"*\"]\n    pointer: /steps/*\n    transform: {{apple_overlay: {on}}}\n"
        )
    };
    write(root, "deviations.yaml", &entry("fp-libm-boundary", true));
    assert!(Suite::open(root).is_ok());
    for (id, on) in [("fp-libm-boundary", false), ("another", true)] {
        write(root, "deviations.yaml", &entry(id, on));
        let error = Suite::open(root).unwrap_err().to_string();
        assert!(error.contains("apple_overlay"), "{error}");
    }
}

#[test]
fn references_still_require_fresh_valid_overlays_in_the_native_lane() {
    let dir = tree();
    let root = dir.path();
    copy(root, SMOKE);
    write(
        root,
        "scenarios/smoke/area.yaml",
        "area: smoke\nreference:\n  fsrs-new-card:\n    reason: Scope example.\n    spec: conformance/dsl#D9.4\n",
    );
    write_overlay(root, SMOKE, good_flip());
    // Different observations would fail if Meguri were called.
    let executor = Canned(json!({"steps": []}));
    let native = suite(root, Platform::MacOs).run(&[], &executor, "native");
    assert_eq!(native.outcomes[0].status, Status::Reference, "{}", native.render_text());
    assert_eq!(native.totals()["pass"], 0);
    let path = root.join(format!("goldens-apple/{SMOKE}.json"));
    let mut overlay = read_json(&path);
    overlay["scenario_sha256"] = json!("0".repeat(64));
    write(root, &format!("goldens-apple/{SMOKE}.json"), &overlay.to_string());
    assert_eq!(
        suite(root, Platform::MacOs).run(&[], &executor, "native").outcomes[0].status,
        Status::Stale
    );
    assert!(
        !suite(root, Platform::Linux)
            .goldens_check(&GoldensCheck::default())
            .unwrap()
            .ok()
    );
    write(root, &format!("goldens-apple/{SMOKE}.json"), "{}");
    let native = suite(root, Platform::MacOs).run(&[], &executor, "native");
    assert_eq!(native.outcomes[0].status, Status::Invalid);
    assert!(native.outcomes[0].reference.is_some());
    assert!(
        !suite(root, Platform::Linux)
            .goldens_check(&GoldensCheck::default())
            .unwrap()
            .ok()
    );
}
