use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::Step;

pub(super) fn register(registry: &mut Registry) {
    registry.op("optimize", optimize);
}
fn optimize(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Optimize(_) = step else {
        return Err(ExecError::new("invalid_scenario", "expected optimize"));
    };
    let collection = session.engine_collection()?;
    meguri_engine::maintenance::optimize::optimize(&collection, &meguri_engine::CancelToken::new())
        .map_err(|error| ExecError::new("engine_error", error.to_string()))?;
    Ok(OpResult::Done)
}
