//! The deck config blob (`specs/model/protobuf-records.md` P6.6, P8).

use super::{Decode, Encode, Reader, Result, UnknownFields, WireType, Writer, blob_message};
use crate::error::BlobColumn;

/// P6.6, `deck_config.config`. Numbers 7, 8 and 39 are reserved by Anki (7 and
/// 8 for later FSRS parameter lists) and kept as unknown fields.
///
/// The three FSRS parameter lists are separate and never merged (P8, R1486):
/// only `fsrs_params_6`, the list of the FSRS version Meguri runs (R696), has
/// a setter, and the optimizer (U47) is its only caller.
#[derive(Clone, Debug, Default)]
pub struct DeckConfigMessage {
    /// Learning steps in minutes.
    pub learn_steps: Vec<f32>,
    /// Relearning steps in minutes.
    pub relearn_steps: Vec<f32>,
    pub(crate) fsrs_params_4: Vec<f32>,
    /// Easy Days, one value per weekday.
    pub easy_days_percentages: Vec<f32>,
    pub(crate) fsrs_params_5: Vec<f32>,
    pub(crate) fsrs_params_6: Vec<f32>,
    pub new_per_day: u32,
    pub reviews_per_day: u32,
    /// Starting ease as a ratio (2.5).
    pub initial_ease: f32,
    pub easy_multiplier: f32,
    pub hard_multiplier: f32,
    pub lapse_multiplier: f32,
    pub interval_multiplier: f32,
    pub maximum_review_interval: u32,
    pub minimum_lapse_interval: u32,
    pub graduating_interval_good: u32,
    pub graduating_interval_easy: u32,
    /// 0 due order, 1 random.
    pub new_card_insert_order: i32,
    /// 0 suspend, 1 tag only.
    pub leech_action: i32,
    pub leech_threshold: u32,
    pub disable_autoplay: bool,
    pub cap_answer_time_to_secs: u32,
    pub show_timer: bool,
    pub skip_question_when_replaying_answer: bool,
    pub bury_new: bool,
    pub bury_reviews: bool,
    pub bury_interday_learning: bool,
    /// Review mix: 0 mix with reviews, 1 after reviews, 2 before reviews.
    pub new_mix: i32,
    /// Review mix, as `new_mix`.
    pub interday_learning_mix: i32,
    pub new_card_sort_order: i32,
    pub review_order: i32,
    pub new_card_gather_priority: i32,
    /// Unused by Anki; kept.
    pub new_per_day_minimum: u32,
    pub question_action: i32,
    pub desired_retention: f32,
    pub stop_timer_on_answer: bool,
    /// Retention assumed for missing history (schema-11 `sm2Retention`).
    pub historical_retention: f32,
    pub seconds_to_show_question: f32,
    pub seconds_to_show_answer: f32,
    pub answer_action: i32,
    pub wait_for_audio: bool,
    /// Optimizer search (schema-11 `weightSearch`).
    pub param_search: String,
    /// `YYYY-MM-DD` or empty.
    pub ignore_revlogs_before_date: String,
    pub(crate) other: Vec<u8>,
    pub(crate) unknown: UnknownFields,
}

impl DeckConfigMessage {
    /// sched/fsrs-transitions T13: expose only the support marker; callers
    /// never interpret opaque legacy or future protobuf payloads.
    pub fn has_later_fsrs_parameters(&self) -> bool {
        self.unknown
            .iter()
            .any(|field| matches!(field.number(), 7 | 8) && !field.is_empty_length_delimited())
            || crate::schema11::deck_config::has_later_fsrs_parameters(&self.other)
    }

    /// FSRS-4 parameters (schema-11 `fsrsWeights`); read-only.
    pub fn fsrs_params_4(&self) -> &[f32] {
        &self.fsrs_params_4
    }

    /// FSRS-5 parameters; read-only.
    pub fn fsrs_params_5(&self) -> &[f32] {
        &self.fsrs_params_5
    }

    /// FSRS-6 parameters, the list Meguri's optimizer writes (R696).
    pub fn fsrs_params_6(&self) -> &[f32] {
        &self.fsrs_params_6
    }

    /// Replaces the FSRS-6 list and nothing else (P8, R1486).
    pub fn set_fsrs_params_6(&mut self, params: Vec<f32>) {
        self.fsrs_params_6 = params;
    }

    /// Legacy JSON keys (P7), opaque bytes.
    pub fn other(&self) -> &[u8] {
        &self.other
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for DeckConfigMessage {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => r.floats(wire, &mut self.learn_steps)?,
            2 => r.floats(wire, &mut self.relearn_steps)?,
            3 => r.floats(wire, &mut self.fsrs_params_4)?,
            4 => r.floats(wire, &mut self.easy_days_percentages)?,
            5 => r.floats(wire, &mut self.fsrs_params_5)?,
            6 => r.floats(wire, &mut self.fsrs_params_6)?,
            9 => self.new_per_day = r.uint32(wire)?,
            10 => self.reviews_per_day = r.uint32(wire)?,
            11 => self.initial_ease = r.float(wire)?,
            12 => self.easy_multiplier = r.float(wire)?,
            13 => self.hard_multiplier = r.float(wire)?,
            14 => self.lapse_multiplier = r.float(wire)?,
            15 => self.interval_multiplier = r.float(wire)?,
            16 => self.maximum_review_interval = r.uint32(wire)?,
            17 => self.minimum_lapse_interval = r.uint32(wire)?,
            18 => self.graduating_interval_good = r.uint32(wire)?,
            19 => self.graduating_interval_easy = r.uint32(wire)?,
            20 => self.new_card_insert_order = r.int32(wire)?,
            21 => self.leech_action = r.int32(wire)?,
            22 => self.leech_threshold = r.uint32(wire)?,
            23 => self.disable_autoplay = r.bool(wire)?,
            24 => self.cap_answer_time_to_secs = r.uint32(wire)?,
            25 => self.show_timer = r.bool(wire)?,
            26 => self.skip_question_when_replaying_answer = r.bool(wire)?,
            27 => self.bury_new = r.bool(wire)?,
            28 => self.bury_reviews = r.bool(wire)?,
            29 => self.bury_interday_learning = r.bool(wire)?,
            30 => self.new_mix = r.int32(wire)?,
            31 => self.interday_learning_mix = r.int32(wire)?,
            32 => self.new_card_sort_order = r.int32(wire)?,
            33 => self.review_order = r.int32(wire)?,
            34 => self.new_card_gather_priority = r.int32(wire)?,
            35 => self.new_per_day_minimum = r.uint32(wire)?,
            36 => self.question_action = r.int32(wire)?,
            37 => self.desired_retention = r.float(wire)?,
            38 => self.stop_timer_on_answer = r.bool(wire)?,
            40 => self.historical_retention = r.float(wire)?,
            41 => self.seconds_to_show_question = r.float(wire)?,
            42 => self.seconds_to_show_answer = r.float(wire)?,
            43 => self.answer_action = r.int32(wire)?,
            44 => self.wait_for_audio = r.bool(wire)?,
            45 => self.param_search = r.string(wire)?,
            46 => self.ignore_revlogs_before_date = r.string(wire)?,
            255 => self.other = r.bytes(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for DeckConfigMessage {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.floats(1, &self.learn_steps);
        w.floats(2, &self.relearn_steps);
        w.floats(3, &self.fsrs_params_4);
        w.floats(4, &self.easy_days_percentages);
        w.floats(5, &self.fsrs_params_5);
        w.floats(6, &self.fsrs_params_6);
        w.uint32(9, self.new_per_day);
        w.uint32(10, self.reviews_per_day);
        w.float(11, self.initial_ease);
        w.float(12, self.easy_multiplier);
        w.float(13, self.hard_multiplier);
        w.float(14, self.lapse_multiplier);
        w.float(15, self.interval_multiplier);
        w.uint32(16, self.maximum_review_interval);
        w.uint32(17, self.minimum_lapse_interval);
        w.uint32(18, self.graduating_interval_good);
        w.uint32(19, self.graduating_interval_easy);
        w.int32(20, self.new_card_insert_order);
        w.int32(21, self.leech_action);
        w.uint32(22, self.leech_threshold);
        w.bool(23, self.disable_autoplay);
        w.uint32(24, self.cap_answer_time_to_secs);
        w.bool(25, self.show_timer);
        w.bool(26, self.skip_question_when_replaying_answer);
        w.bool(27, self.bury_new);
        w.bool(28, self.bury_reviews);
        w.bool(29, self.bury_interday_learning);
        w.int32(30, self.new_mix);
        w.int32(31, self.interday_learning_mix);
        w.int32(32, self.new_card_sort_order);
        w.int32(33, self.review_order);
        w.int32(34, self.new_card_gather_priority);
        w.uint32(35, self.new_per_day_minimum);
        w.int32(36, self.question_action);
        w.float(37, self.desired_retention);
        w.bool(38, self.stop_timer_on_answer);
        w.float(40, self.historical_retention);
        w.float(41, self.seconds_to_show_question);
        w.float(42, self.seconds_to_show_answer);
        w.int32(43, self.answer_action);
        w.bool(44, self.wait_for_audio);
        w.string(45, &self.param_search);
        w.string(46, &self.ignore_revlogs_before_date);
        w.bytes(255, &self.other);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

blob_message!(DeckConfigMessage, BlobColumn::DeckConfig);
