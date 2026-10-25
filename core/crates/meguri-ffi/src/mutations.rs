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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_preserves_all_diagnostics_and_operation_metadata() {
        let changes = crate::changes::sample();
        let result = meguri_engine::OpResult {
            value: (),
            seq: meguri_engine::Seq(101),
            generation: meguri_engine::Generation(7),
            changes: std::sync::Arc::new(changes),
            undo: None,
            past_cancel_point: false,
            timing: Default::default(),
        };
        let summary = MutationSummary::of(&result);
        assert_eq!((summary.seq, summary.generation), (101, 7));
        assert_eq!(summary.changes.skipped_incoming_cards, [71, 72]);
        assert_eq!(summary.changes.skipped_incoming_notes, [71, 73]);
        assert_eq!(summary.changes.skipped_incoming_decks, [71, 74]);
        assert_eq!(summary.changes, ChangeSet::from(result.changes.as_ref()));
    }

    #[test]
    fn default_model_conversion_keeps_diagnostics_empty() {
        let model = meguri_model::changes::ChangeSet::empty(meguri_model::changes::OpSource::Maintenance);
        let changes = ChangeSet::from(&model);
        assert!(changes.skipped_incoming_cards.is_empty());
        assert!(changes.skipped_incoming_notes.is_empty());
        assert!(changes.skipped_incoming_decks.is_empty());
    }
}
