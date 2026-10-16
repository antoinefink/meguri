//! The T0/T1 run report (dsl.md D12): text for people, JSON for tools/mg and
//! agents.

use serde_json::{Map, Value, json};

use crate::diff;
use crate::suite::{OraclePin, Outcome, SHOWN_DIFFERENCES, Status};

/// Which tier a report belongs to; it decides what fails (dsl.md D10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Committed goldens on Meguri's executor.
    T0,
    /// Live oracle workers: golden replays and live chains.
    T1,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::T0 => "t0",
            Tier::T1 => "t1",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Report {
    pub tier: Tier,
    pub executor: String,
    pub lane: String,
    pub oracle_pin: OraclePin,
    pub outcomes: Vec<Outcome>,
    pub wall_ms: u64,
    /// T0 on macOS: totals and text count the overlay entries applied
    /// (float-determinism.md F9).
    pub overlays: bool,
}

impl Report {
    /// Outcomes that fail this report's tier.
    pub fn failures(&self) -> Vec<&Outcome> {
        self.outcomes.iter().filter(|o| o.fails(self.tier)).collect()
    }

    /// Scenario count per status, every status listed; on macOS T0 also
    /// `overlay_applied` (entries) and `overlay_scenarios` (scenarios with at
    /// least one).
    pub fn totals(&self) -> Map<String, Value> {
        let mut totals: Map<String, Value> = Status::ALL
            .iter()
            .map(|s| {
                (
                    s.as_str().to_owned(),
                    json!(self.outcomes.iter().filter(|o| o.status == *s).count()),
                )
            })
            .collect();
        if self.overlays {
            let (entries, scenarios) = self.overlay_totals();
            totals.insert("overlay_applied".to_owned(), json!(entries));
            totals.insert("overlay_scenarios".to_owned(), json!(scenarios));
        }
        totals
    }

    /// (overlay entries applied, scenarios with at least one).
    pub fn overlay_totals(&self) -> (usize, usize) {
        let applied = self.outcomes.iter().filter_map(|o| o.overlay_applied);
        (applied.clone().sum(), applied.filter(|n| *n > 0).count())
    }

    pub fn to_json(&self) -> Value {
        json!({
            "runner": "meguri-testkit",
            "tier": self.tier.as_str(),
            "executor": self.executor,
            "lane": self.lane,
            "oracle_pin": {"anki_version": self.oracle_pin.anki_version, "lock_sha256": self.oracle_pin.lock_sha256},
            "wall_ms": self.wall_ms,
            "totals": self.totals(),
            "failed": self.failures().len(),
            "scenarios": self.outcomes.iter().map(|o| outcome_json(o, self.tier)).collect::<Vec<_>>(),
        })
    }

    /// Every scenario that is not a pass, then a summary line.
    pub fn render_text(&self) -> String {
        let mut text = String::new();
        for outcome in self.outcomes.iter().filter(|o| o.status != Status::Pass) {
            text.push_str(&outcome.render_text());
        }
        if self.overlays {
            let (entries, scenarios) = self.overlay_totals();
            text.push_str(&format!("overlays: {entries} values in {scenarios} scenarios\n"));
        }
        let counts: Vec<String> = Status::ALL
            .iter()
            .map(|s| (s.as_str(), self.outcomes.iter().filter(|o| o.status == *s).count()))
            .filter(|(_, n)| *n != 0)
            .map(|(status, n)| format!("{n} {status}"))
            .collect();
        text.push_str(&format!(
            "{} scenario{} on {} ({}): {}; {} fail{} {}\n",
            self.outcomes.len(),
            if self.outcomes.len() == 1 { "" } else { "s" },
            self.executor,
            self.lane,
            if counts.is_empty() {
                "none".to_owned()
            } else {
                counts.join(", ")
            },
            self.failures().len(),
            if self.failures().len() == 1 { "s" } else { "" },
            self.tier.as_str().to_uppercase(),
        ));
        text
    }
}

fn outcome_json(o: &Outcome, tier: Tier) -> Value {
    let mut out = json!({
        "scenario": o.scenario,
        "path": o.path.display().to_string(),
        "area": o.area,
        "active": o.active,
        "live": o.live,
        "status": o.status.as_str(),
        "fails": o.fails(tier),
        "message": o.message,
        "repro": o.repro,
    });
    let map = out.as_object_mut().expect("an object");
    if let Some(sha) = &o.scenario_sha256 {
        map.insert("scenario_sha256".to_owned(), json!(sha));
    }
    if let Some(first) = &o.first_divergence {
        map.insert(
            "first_divergence".to_owned(),
            json!({"step": first.step, "pointer": first.pointer, "expected": first.expected, "actual": first.actual}),
        );
    }
    if !o.differences.is_empty() {
        map.insert("diff_count".to_owned(), json!(o.differences.len()));
        map.insert(
            "diffs".to_owned(),
            o.differences
                .iter()
                .take(SHOWN_DIFFERENCES)
                .map(|d| json!({"pointer": d.pointer, "expected": diff::shown(&d.expected), "actual": diff::shown(&d.actual)}))
                .collect(),
        );
    }
    if let Some(missing) = &o.missing {
        map.insert("missing".to_owned(), json!(missing));
    }
    if !o.deviations_applied.is_empty() {
        map.insert("deviations_applied".to_owned(), json!(o.deviations_applied));
    }
    if let Some(applied) = o.overlay_applied {
        map.insert("overlay_applied".to_owned(), json!(applied));
    }
    out
}
