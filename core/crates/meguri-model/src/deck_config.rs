//! Deck config (preset) records (`specs/model/row-records.md` R7).

use crate::ids::DeckConfigId;
use crate::proto::ProtoBlob;
use crate::proto::deck_config::DeckConfigMessage;
use crate::time::EpochSecs;
use crate::usn::Usn;

/// A `deck_config` row with its blob.
#[derive(Clone, Debug)]
pub struct DeckConfig {
    pub id: DeckConfigId,
    pub name: String,
    pub mtime: EpochSecs,
    pub usn: Usn,
    pub config: ProtoBlob<DeckConfigMessage>,
}

impl DeckConfigMessage {
    /// Factory preset values, distinct from protobuf's absent-field defaults
    /// (sched/presets V2). Older parameter lists remain empty.
    pub fn builtin_defaults() -> Self {
        Self {
            learn_steps: vec![1.0, 10.0],
            relearn_steps: vec![10.0],
            easy_days_percentages: vec![1.0; 7],
            new_per_day: 20,
            reviews_per_day: 200,
            initial_ease: 2.5,
            easy_multiplier: 1.3,
            hard_multiplier: 1.2,
            interval_multiplier: 1.0,
            maximum_review_interval: 36500,
            minimum_lapse_interval: 1,
            graduating_interval_good: 1,
            graduating_interval_easy: 4,
            leech_action: 1,
            leech_threshold: 8,
            cap_answer_time_to_secs: 60,
            desired_retention: 0.9,
            historical_retention: 0.9,
            wait_for_audio: true,
            ..Self::default()
        }
    }

    /// V3: replace invalid bounded values for use, without mutating storage.
    /// Unknown enum codes, unbounded fields and opaque fields stay as stored.
    pub fn for_use(&self) -> Self {
        let mut value = self.clone();
        let defaults = Self::builtin_defaults();
        macro_rules! bound {
            ($field:ident, $low:expr, $high:expr) => {
                if !($low..=$high).contains(&value.$field) {
                    value.$field = defaults.$field;
                }
            };
        }
        bound!(new_per_day, 0, 9999);
        bound!(reviews_per_day, 0, 9999);
        bound!(initial_ease, 1.31, 5.0);
        bound!(easy_multiplier, 1.0, 5.0);
        bound!(hard_multiplier, 0.5, 1.3);
        bound!(lapse_multiplier, 0.0, 1.0);
        bound!(interval_multiplier, 0.5, 2.0);
        bound!(maximum_review_interval, 1, 36500);
        bound!(minimum_lapse_interval, 1, 36500);
        bound!(graduating_interval_good, 1, 36500);
        bound!(graduating_interval_easy, 1, 36500);
        bound!(leech_threshold, 1, 9999);
        bound!(cap_answer_time_to_secs, 1, 9999);
        bound!(new_per_day_minimum, 0, 9999);
        bound!(desired_retention, 0.7, 0.99);
        bound!(historical_retention, 0.7, 0.97);
        value
    }

    /// V8/V9: edits may not replace older parameter lists or opaque bytes.
    pub fn same_read_only_fields(&self, other: &Self) -> bool {
        let same_floats =
            |a: &[f32], b: &[f32]| a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.to_bits() == b.to_bits());
        same_floats(self.fsrs_params_4(), other.fsrs_params_4())
            && same_floats(self.fsrs_params_5(), other.fsrs_params_5())
            && self.other() == other.other()
            && self.unknown_fields() == other.unknown_fields()
    }
}

impl DeckConfig {
    /// A read-only bounded copy for legacy output or other normal callers;
    /// the pure schema-11 codec itself deliberately stays unbounded.
    pub fn for_use(&self) -> Self {
        let mut value = self.clone();
        *value.config = self.config.for_use();
        value
    }
}
