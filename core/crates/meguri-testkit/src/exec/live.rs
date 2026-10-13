//! Live chains (dsl.md D9): a live scenario's segments run on Meguri and the
//! oracle in turn, each side handing the collection to the other through a
//! directory both see. T1 compares the chain's observations with the golden,
//! which the oracle recorded running every segment itself.

use std::path::Path;

use serde_json::{Value, json};

use super::{ExecError, Handoff, SegmentExecutor, SegmentRequest};
use crate::scenario::{LoadedScenario, Side};

pub struct Chain<'a> {
    pub meguri: &'a dyn SegmentExecutor,
    pub oracle: &'a dyn SegmentExecutor,
    /// The handoff directory as this process sees it; oracle workers mount it.
    pub root: &'a Path,
}

/// Where the session crosses at a handoff step, relative to the handoff
/// directory (dsl.md D9.2). `@` is outside the id alphabet (D1), so no
/// scenario's handoff directory lies inside or above another's: T1 runs
/// `smoke/rt` and `smoke/rt/1` at the same time.
pub fn handoff_dir(scenario_id: &str, step: usize) -> String {
    format!("{scenario_id}@{step}")
}

impl Chain<'_> {
    /// The chain's observations, `{"steps": [...]}`: every segment's outputs
    /// at their step indices and `{}` for each handoff step.
    pub fn run(&self, loaded: &LoadedScenario) -> Result<Value, ExecError> {
        let scenario = &loaded.scenario;
        let mut outputs = vec![Value::Null; scenario.steps.len()];
        let mut resume: Option<Handoff> = None;
        for segment in scenario.segments() {
            let side = match segment.side {
                Side::Meguri => self.meguri,
                Side::Oracle => self.oracle,
            };
            let on_side = |e: ExecError| ExecError {
                detail: format!("{} side: {}", segment.side.as_str(), e.detail),
                ..e
            };
            let hand_off_to = segment.handoff.map(|step| handoff_dir(&scenario.id, step));
            if let Some(rel) = &hand_off_to {
                let dir = self.root.join(rel);
                let io = |e: std::io::Error| ExecError::new("handoff_error", format!("{}: {e}", dir.display()));
                if dir.exists() {
                    std::fs::remove_dir_all(&dir).map_err(io)?;
                }
                if let Some(parent) = dir.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
            }
            let request = SegmentRequest {
                resume: resume.take(),
                steps: segment.steps.clone(),
                hand_off_to,
                root: self.root.to_path_buf(),
            };
            let out = side.segment(loaded, &request).map_err(on_side)?;
            if out.steps.len() != segment.steps.len() {
                return Err(on_side(ExecError::new(
                    "protocol_error",
                    format!("{} outputs for {} steps", out.steps.len(), segment.steps.len()),
                )));
            }
            for (index, output) in segment.steps.clone().zip(out.steps) {
                outputs[index] = output;
            }
            if let Some(step) = segment.handoff {
                outputs[step] = json!({});
                let handoff = out.handoff.ok_or_else(|| {
                    on_side(ExecError::new("protocol_error", "the segment handed over no state")).at_step(step)
                })?;
                resume = Some(handoff);
            }
        }
        Ok(json!({"steps": outputs}))
    }
}
