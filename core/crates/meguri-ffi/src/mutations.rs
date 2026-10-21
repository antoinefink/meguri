//! Exact committed-operation metadata, shared by the typed U37 results.
use crate::changes::ChangeSet;
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MutationSummary {
    pub seq: u64,
    pub generation: u64,
    pub undo_name: Option<String>,
    pub changes: ChangeSet,
    pub past_cancel_point: bool,
}
impl MutationSummary {
    pub(crate) fn of<T>(result: &meguri_engine::OpResult<T>) -> Self {
        Self {
            seq: result.seq.0,
            generation: result.generation.0,
            undo_name: result.undo.map(|u| u.name.to_owned()),
            changes: result.changes.as_ref().into(),
            past_cancel_point: result.past_cancel_point,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CountMutation {
    pub summary: MutationSummary,
    pub count: u64,
}
impl From<meguri_engine::OpResult<usize>> for CountMutation {
    fn from(r: meguri_engine::OpResult<usize>) -> Self {
        Self {
            summary: MutationSummary::of(&r),
            count: r.value as u64,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct BoolMutation {
    pub summary: MutationSummary,
    pub written: bool,
}
impl From<meguri_engine::OpResult<bool>> for BoolMutation {
    fn from(r: meguri_engine::OpResult<bool>) -> Self {
        Self {
            summary: MutationSummary::of(&r),
            written: r.value,
        }
    }
}
