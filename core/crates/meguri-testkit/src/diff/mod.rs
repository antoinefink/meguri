//! Comparing observations: JSON-pointer differences, the first divergent step,
//! set comparison and deviation transforms (observations.md O3, O7; dsl.md D13).

pub mod pointer;
pub mod transforms;

use std::collections::BTreeSet;

use serde_json::{Number, Value};

use crate::observe;
use crate::observe::overlay;
use crate::suite::{Deviation, Transform};

/// One difference; `None` is a member or item present on one side only.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    pub pointer: String,
    pub expected: Option<Value>,
    pub actual: Option<Value>,
}

/// How a value is shown in reports: `<absent>` for a missing member.
pub fn shown(value: &Option<Value>) -> Value {
    value.clone().unwrap_or_else(|| Value::String("<absent>".to_owned()))
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CompareOptions<'a> {
    /// `compare.set` pointer patterns.
    pub set: &'a [String],
    pub deviations: &'a [Deviation],
    /// The scenario's overlay entries, which the `apple_overlay` deviation
    /// puts into expected: only in T0 on macOS (float-determinism.md F9).
    pub overlay: &'a [overlay::Entry],
}

#[derive(Debug, Clone, Default)]
pub struct Comparison {
    /// In pointer order.
    pub differences: Vec<Difference>,
    /// Ids of the deviations whose pointer matched in the expected value.
    pub deviations_applied: Vec<String>,
    /// How many overlay entries the `apple_overlay` deviation applied.
    pub overlay_applied: usize,
}

/// Compares a golden's observations (expected) with an executor's (actual):
/// deviation transforms on expected first (the `apple_overlay` entry before
/// the others, dsl.md D13), then set comparison, then the member-by-member
/// walk with the declared tolerances.
pub fn compare(scenario_id: &str, expected: &Value, actual: &Value, options: &CompareOptions<'_>) -> Comparison {
    let mut expected = expected.clone();
    let mut actual = actual.clone();
    let mut applied = Vec::new();
    let mut overlay_applied = 0;
    let selected = || {
        options
            .deviations
            .iter()
            .filter(|d| d.scenarios.iter().any(|p| pointer::glob(p, scenario_id)))
    };
    for deviation in selected().filter(|d| matches!(d.transform, Transform::AppleOverlay(_))) {
        let n = overlay::apply(options.overlay, &pointer::parse(&deviation.pointer), &mut expected);
        if n > 0 {
            overlay_applied += n;
            applied.push(deviation.id.clone());
        }
    }
    for deviation in selected().filter(|d| !matches!(d.transform, Transform::AppleOverlay(_))) {
        if transforms::apply(deviation, &mut expected) {
            applied.push(deviation.id.clone());
        }
    }
    for pattern in options.set {
        let pattern = pointer::parse(pattern);
        for side in [&mut expected, &mut actual] {
            pointer::for_each_match(side, &pattern, &mut |value| {
                if let Value::Array(items) = value {
                    items.sort_by_cached_key(set_key);
                }
            });
        }
    }
    let mut differences = Vec::new();
    walk(&expected, &actual, &mut Vec::new(), &mut differences, true);
    Comparison {
        differences,
        deviations_applied: applied,
        overlay_applied,
    }
}

/// The order of a `compare.set` array: JCS text first (O7), then serde_json's
/// text, which tells 1 from 1.0 where JCS writes both as `1`. Equal keys mean
/// equal values, so two equal multisets always sort alike.
fn set_key(item: &Value) -> (String, String) {
    let text = item.to_string();
    (observe::jcs(item).unwrap_or_else(|_| text.clone()), text)
}

/// Every difference, with no tolerance, set comparison or deviation: what
/// changed between two recordings.
pub fn exact_differences(expected: &Value, actual: &Value) -> Vec<Difference> {
    let mut differences = Vec::new();
    walk(expected, actual, &mut Vec::new(), &mut differences, false);
    differences
}

fn walk(expected: &Value, actual: &Value, path: &mut Vec<String>, out: &mut Vec<Difference>, tolerant: bool) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let keys: BTreeSet<&String> = e.keys().chain(a.keys()).collect();
            for key in keys {
                path.push(key.clone());
                match (e.get(key), a.get(key)) {
                    (Some(x), Some(y)) => walk(x, y, path, out, tolerant),
                    (x, y) => out.push(difference(path, x, y)),
                }
                path.pop();
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            for i in 0..e.len().max(a.len()) {
                path.push(i.to_string());
                match (e.get(i), a.get(i)) {
                    (Some(x), Some(y)) => walk(x, y, path, out, tolerant),
                    (x, y) => out.push(difference(path, x, y)),
                }
                path.pop();
            }
        }
        _ => {
            if !(expected == actual || (tolerant && equal(expected, actual, path))) {
                out.push(difference(path, Some(expected), Some(actual)));
            }
        }
    }
}

fn difference(path: &[String], expected: Option<&Value>, actual: Option<&Value>) -> Difference {
    Difference {
        pointer: pointer::render(path),
        expected: expected.cloned(),
        actual: actual.cloned(),
    }
}

/// Exact equality, kind included, except where a tolerance is declared and both
/// sides are fractions (O7).
fn equal(expected: &Value, actual: &Value, path: &[String]) -> bool {
    match (expected, actual) {
        (Value::Number(e), Value::Number(a)) => e == a || within_tolerance(e, a, path),
        _ => expected == actual,
    }
}

fn within_tolerance(e: &Number, a: &Number, path: &[String]) -> bool {
    let Some(tolerance) = observe::tolerance(path) else {
        return false;
    };
    if !(e.is_f64() && a.is_f64()) {
        return false;
    }
    match (e.as_f64(), a.as_f64()) {
        (Some(e), Some(a)) => (a - e).abs() <= tolerance * e.abs().max(a.abs()),
        _ => false,
    }
}

/// The smallest step index among the differences (`/steps/<i>/...`).
pub fn first_divergence(differences: &[Difference]) -> Option<usize> {
    differences
        .iter()
        .filter_map(|d| {
            let mut segments = d.pointer.split('/').skip(1);
            match (segments.next(), segments.next()) {
                (Some("steps"), Some(i)) => i.parse().ok(),
                _ => None,
            }
        })
        .min()
}
