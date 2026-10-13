//! The T0/T1 run report (dsl.md D12): text for people, JSON for tools/mg and
//! agents.

use serde_json::{Map, Value, json};

use crate::diff;
use crate::suite::{OraclePin, Outcome, SHOWN_DIFFERENCES, Status};

#[derive(Debug, Clone)]
pub struct Report {
    pub executor: String,
    pub lane: String,
    pub oracle_pin: OraclePin,
    pub outcomes: Vec<Outcome>,
    pub wall_ms: u64,
}

impl Report {
    /// Outcomes that fail T0.
    pub fn failures(&self) -> Vec<&Outcome> {
        self.outcomes.iter().filter(|o| o.fails_t0()).collect()
    }

    /// Scenario count per status, every status listed.
    pub fn totals(&self) -> Map<String, Value> {
        Status::ALL
            .iter()
            .map(|s| {
                (
                    s.as_str().to_owned(),
                    json!(self.outcomes.iter().filter(|o| o.status == *s).count()),
                )
            })
            .collect()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "runner": "meguri-testkit",
            "executor": self.executor,
            "lane": self.lane,
            "oracle_pin": {"anki_version": self.oracle_pin.anki_version, "lock_sha256": self.oracle_pin.lock_sha256},
            "wall_ms": self.wall_ms,
            "totals": self.totals(),
            "failed": self.failures().len(),
            "scenarios": self.outcomes.iter().map(outcome_json).collect::<Vec<_>>(),
        })
    }

    /// Every scenario that is not a pass, then a summary line.
    pub fn render_text(&self) -> String {
        let mut text = String::new();
        for outcome in self.outcomes.iter().filter(|o| o.status != Status::Pass) {
            text.push_str(&outcome.render_text());
        }
        let counts: Vec<String> = self
            .totals()
            .into_iter()
            .filter(|(_, n)| n.as_u64() != Some(0))
            .map(|(status, n)| format!("{n} {status}"))
            .collect();
        text.push_str(&format!(
            "{} scenario{} on {} ({}): {}; {} fail{} T0\n",
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
        ));
        text
    }
}

fn outcome_json(o: &Outcome) -> Value {
    let mut out = json!({
        "scenario": o.scenario,
        "path": o.path.display().to_string(),
        "area": o.area,
        "active": o.active,
        "status": o.status.as_str(),
        "fails_t0": o.fails_t0(),
        "message": o.message,
        "repro": o.repro,
    });
    let map = out.as_object_mut().expect("an object");
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
    out
}
