//! Deck config (preset) objects (`specs/model/schema11-json.md` S10).

use serde_json::value::RawValue;

use super::{ObjectReader, Result, other_from_members, other_members, truncate_u16, write_other};
use crate::deck_config::DeckConfig;
use crate::error::{ModelError, Schema11Item};
use crate::ids::DeckConfigId;
use crate::json::{self, Member, ObjectWriter};
use crate::proto::ProtoBlob;
use crate::proto::deck_config::DeckConfigMessage;
use crate::time::TimestampSecs;
use crate::usn::Usn;

const ITEM: Schema11Item = Schema11Item::DeckConfig;

/// The top-level keys, in output order. They are also the reserved keys
/// (S3), with `new`, `rev` and `lapse`.
const TOP_KEYS: [&str; 33] = [
    "id",
    "mod",
    "name",
    "usn",
    "maxTaken",
    "autoplay",
    "timer",
    "replayq",
    "new",
    "rev",
    "lapse",
    "dyn",
    "newMix",
    "newPerDayMinimum",
    "interdayLearningMix",
    "reviewOrder",
    "newSortOrder",
    "newGatherPriority",
    "buryInterdayLearning",
    "fsrsWeights",
    "fsrsParams5",
    "fsrsParams6",
    "desiredRetention",
    "ignoreRevlogsBeforeDate",
    "easyDaysPercentages",
    "stopTimerOnAnswer",
    "secondsToShowQuestion",
    "secondsToShowAnswer",
    "questionAction",
    "answerAction",
    "waitForAudio",
    "sm2Retention",
    "weightSearch",
];
const NEW_KEYS: [&str; 6] = ["bury", "delays", "initialFactor", "ints", "order", "perDay"];
const REV_KEYS: [&str; 6] = ["bury", "ease4", "hardFactor", "ivlFct", "maxIvl", "perDay"];
const LAPSE_KEYS: [&str; 5] = ["delays", "leechAction", "leechFails", "minInt", "mult"];

/// S10 output, with the `other` members placed as S10.5 says.
pub fn deck_config_to_json(preset: &DeckConfig) -> String {
    let c: &DeckConfigMessage = &preset.config;
    let other = other_members(c.other());
    let part = |name: &str| -> Vec<Member<'_>> {
        other
            .iter()
            .find(|(key, _)| key == name)
            .and_then(|(_, raw)| json::object(raw))
            .unwrap_or_default()
    };
    let mut out = ObjectWriter::new();
    out.int("id", preset.id.0);
    out.int("mod", preset.mtime.0);
    out.string("name", &preset.name);
    out.int("usn", preset.usn.0);
    out.int("maxTaken", c.cap_answer_time_to_secs as i32);
    out.bool("autoplay", !c.disable_autoplay);
    out.int("timer", u8::from(c.show_timer));
    out.bool("replayq", !c.skip_question_when_replaying_answer);

    let mut new = ObjectWriter::new();
    new.bool("bury", c.bury_new);
    new.f32_array("delays", &c.learn_steps);
    new.int("initialFactor", truncate_u16(c.initial_ease * 1000.0));
    let ints = new.key("ints");
    ints.push('[');
    json::push_int(ints, c.graduating_interval_good as u16);
    ints.push(',');
    json::push_int(ints, c.graduating_interval_easy as u16);
    ints.push_str(",0]");
    new.int("order", if c.new_card_insert_order == 1 { 0 } else { 1 });
    new.int("perDay", c.new_per_day);
    write_other(&mut new, &part("new"), &NEW_KEYS, &NEW_KEYS);
    out.raw("new", &new.finish());

    let mut rev = ObjectWriter::new();
    rev.bool("bury", c.bury_reviews);
    rev.f32("ease4", c.easy_multiplier);
    rev.f32("ivlFct", c.interval_multiplier);
    rev.int("maxIvl", c.maximum_review_interval);
    rev.int("perDay", c.reviews_per_day);
    rev.f32("hardFactor", c.hard_multiplier);
    write_other(&mut rev, &part("rev"), &REV_KEYS, &REV_KEYS);
    out.raw("rev", &rev.finish());

    let mut lapse = ObjectWriter::new();
    lapse.f32_array("delays", &c.relearn_steps);
    lapse.int("leechAction", if c.leech_action == 1 { 1 } else { 0 });
    lapse.int("leechFails", c.leech_threshold);
    lapse.int("minInt", c.minimum_lapse_interval);
    lapse.f32("mult", c.lapse_multiplier);
    write_other(&mut lapse, &part("lapse"), &LAPSE_KEYS, &LAPSE_KEYS);
    out.raw("lapse", &lapse.finish());

    out.bool("dyn", false);
    out.int("newMix", c.new_mix);
    out.int("newPerDayMinimum", c.new_per_day_minimum);
    out.int("interdayLearningMix", c.interday_learning_mix);
    out.int("reviewOrder", c.review_order);
    out.int("newSortOrder", c.new_card_sort_order);
    out.int("newGatherPriority", c.new_card_gather_priority);
    out.bool("buryInterdayLearning", c.bury_interday_learning);
    out.f32_array("fsrsWeights", c.fsrs_params_4());
    out.f32_array("fsrsParams5", c.fsrs_params_5());
    out.f32_array("fsrsParams6", c.fsrs_params_6());
    out.f32("desiredRetention", c.desired_retention);
    out.string("ignoreRevlogsBeforeDate", &c.ignore_revlogs_before_date);
    out.f32_array("easyDaysPercentages", &c.easy_days_percentages);
    out.bool("stopTimerOnAnswer", c.stop_timer_on_answer);
    out.f32("secondsToShowQuestion", c.seconds_to_show_question);
    out.f32("secondsToShowAnswer", c.seconds_to_show_answer);
    out.int("questionAction", if c.question_action == 1 { 1 } else { 0 });
    out.int(
        "answerAction",
        if (1..=4).contains(&c.answer_action) {
            c.answer_action
        } else {
            0
        },
    );
    out.bool("waitForAudio", c.wait_for_audio);
    out.f32("sm2Retention", c.historical_retention);
    out.string("weightSearch", &c.param_search);
    write_other(&mut out, &other, &TOP_KEYS, &TOP_KEYS);
    out.finish()
}

/// S10 input. The record's blob is new (no original bytes).
pub fn deck_config_from_json(text: &str) -> Result<DeckConfig> {
    let raw = json::parse_value(text).ok_or_else(|| ModelError::invalid_json(ITEM, ""))?;
    deck_config_from_raw(raw)
}

pub(crate) fn deck_config_from_raw(raw: &RawValue) -> Result<DeckConfig> {
    let mut r = ObjectReader::new(raw, ITEM, "")?;
    let id = r.strict("id", json::i64_or_string, None)?;
    let mtime = r.strict("mod", json::i64_or_string, None)?;
    let name = r.strict("name", json::string, None)?;
    let usn = r.strict("usn", json::i32_value, None)?;
    let max_taken = r.strict("maxTaken", json::i32_value, None)?;
    let autoplay = r.strict("autoplay", json::boolean, None)?;
    let timer = r.lenient("timer", json::u8_value, None)?;
    let replay_question = r.strict("replayq", json::boolean, Some(false))?;
    let new = r.strict("new", |raw| Some(NewPart::read(raw)), None)?;
    let rev = r.strict("rev", |raw| Some(RevPart::read(raw)), None)?;
    let lapse = r.strict("lapse", |raw| Some(LapsePart::read(raw)), None)?;
    r.lenient("dyn", json::boolean, Some(false))?;
    let new_mix = r.strict("newMix", json::i32_value, Some(0))?;
    let new_per_day_minimum = r.strict("newPerDayMinimum", json::u32_value, Some(0))?;
    let interday_learning_mix = r.strict("interdayLearningMix", json::i32_value, Some(0))?;
    let review_order = r.strict("reviewOrder", json::i32_value, Some(0))?;
    let new_card_sort_order = r.strict("newSortOrder", json::i32_value, Some(0))?;
    let new_card_gather_priority = r.strict("newGatherPriority", json::i32_value, Some(0))?;
    let bury_interday_learning = r.strict("buryInterdayLearning", json::boolean, Some(false))?;
    let fsrs_params_4 = r.strict("fsrsWeights", json::f32_array, Some(Vec::new()))?;
    let fsrs_params_5 = r.strict("fsrsParams5", json::f32_array, Some(Vec::new()))?;
    let fsrs_params_6 = r.strict("fsrsParams6", json::f32_array, Some(Vec::new()))?;
    let desired_retention = r.strict("desiredRetention", json::f32_value, Some(0.0))?;
    let ignore_revlogs_before_date = r.strict("ignoreRevlogsBeforeDate", json::string, Some(String::new()))?;
    let easy_days_percentages = r.strict("easyDaysPercentages", json::f32_array, Some(Vec::new()))?;
    let stop_timer_on_answer = r.strict("stopTimerOnAnswer", json::boolean, Some(false))?;
    let seconds_to_show_question = r.strict("secondsToShowQuestion", json::f32_value, Some(0.0))?;
    let seconds_to_show_answer = r.strict("secondsToShowAnswer", json::f32_value, Some(0.0))?;
    let question_action = r.strict(
        "questionAction",
        |v| json::i32_value(v).filter(|a| matches!(a, 0 | 1)),
        Some(0),
    )?;
    let answer_action = r.strict(
        "answerAction",
        |v| json::i32_value(v).filter(|a| (0..=4).contains(a)),
        Some(0),
    )?;
    let wait_for_audio = r.strict("waitForAudio", json::boolean, Some(true))?;
    let historical_retention = r.strict("sm2Retention", json::f32_value, Some(0.0))?;
    let param_search = r.strict("weightSearch", json::string, Some(String::new()))?;

    // S10.5 input: unknown top-level members, then the parts' extras.
    let mut extras = ObjectWriter::new();
    for (key, raw) in r.unread() {
        extras.raw(&key, raw.get());
    }
    for (key, part) in [("new", &new.extras), ("rev", &rev.extras), ("lapse", &lapse.extras)] {
        if !part.is_empty() {
            extras.raw(key, &String::from_utf8_lossy(&other_from_members(part)));
        }
    }
    let other = if extras.is_empty() {
        Vec::new()
    } else {
        extras.finish().into_bytes()
    };

    let config = DeckConfigMessage {
        learn_steps: new.delays,
        relearn_steps: lapse.delays,
        fsrs_params_4,
        easy_days_percentages,
        fsrs_params_5,
        fsrs_params_6,
        new_per_day: new.per_day,
        reviews_per_day: rev.per_day,
        initial_ease: f32::from(new.initial_factor) / 1000.0,
        easy_multiplier: rev.ease4,
        hard_multiplier: rev.hard_factor,
        lapse_multiplier: lapse.mult,
        interval_multiplier: rev.interval_factor,
        maximum_review_interval: rev.max_interval,
        minimum_lapse_interval: lapse.min_interval,
        graduating_interval_good: new.ints.0.into(),
        graduating_interval_easy: new.ints.1.into(),
        // `order` 0 is random (insert order 1), 1 is due order (0).
        new_card_insert_order: if new.order == 0 { 1 } else { 0 },
        leech_action: lapse.leech_action,
        leech_threshold: lapse.leech_fails,
        disable_autoplay: !autoplay,
        cap_answer_time_to_secs: u32::try_from(max_taken).unwrap_or(0),
        show_timer: timer != 0,
        skip_question_when_replaying_answer: !replay_question,
        bury_new: new.bury,
        bury_reviews: rev.bury,
        bury_interday_learning,
        new_mix,
        interday_learning_mix,
        new_card_sort_order,
        review_order,
        new_card_gather_priority,
        new_per_day_minimum,
        question_action,
        desired_retention,
        stop_timer_on_answer,
        historical_retention,
        seconds_to_show_question,
        seconds_to_show_answer,
        answer_action,
        wait_for_audio,
        param_search,
        ignore_revlogs_before_date,
        other,
        ..DeckConfigMessage::default()
    };
    Ok(DeckConfig {
        id: DeckConfigId(id),
        name,
        mtime: TimestampSecs(mtime),
        usn: Usn(usn),
        config: ProtoBlob::new(config),
    })
}

/// S10.1 to S10.3: a part that is not an object, misses a required key or has
/// a strict key with a wrong value takes its defaults as a whole, extras
/// included.
fn read_part<'a, T>(
    raw: &'a RawValue,
    read: impl FnOnce(&mut ObjectReader<'a>) -> Result<T>,
) -> Option<(T, Vec<Member<'a>>)> {
    let mut r = ObjectReader::new(raw, ITEM, "").ok()?;
    let part = read(&mut r).ok()?;
    Some((part, r.unread()))
}

/// S10.1.
struct NewPart<'a> {
    bury: bool,
    delays: Vec<f32>,
    initial_factor: u16,
    ints: (u16, u16),
    order: u8,
    per_day: u32,
    extras: Vec<Member<'a>>,
}

impl<'a> NewPart<'a> {
    fn read(raw: &'a RawValue) -> NewPart<'a> {
        read_part(raw, |r| {
            Ok(NewPart {
                bury: r.strict("bury", json::boolean, Some(false))?,
                delays: r.lenient("delays", json::f32_array, None)?,
                initial_factor: r.strict("initialFactor", json::u16_value, None)?,
                ints: r.strict("ints", |v| Some(intervals(v)), None)?,
                order: r.strict(
                    "order",
                    |v| Some(json::u8_value(v).filter(|o| *o <= 1).unwrap_or(1)),
                    None,
                )?,
                per_day: r.lenient("perDay", json::u32_value, None)?,
                extras: Vec::new(),
            })
        })
        .map(|(part, extras)| NewPart { extras, ..part })
        .unwrap_or(NewPart {
            bury: false,
            delays: vec![1.0, 10.0],
            initial_factor: 2500,
            ints: (1, 4),
            order: 1,
            per_day: 20,
            extras: Vec::new(),
        })
    }
}

/// S10.4: at least two elements, all `u16`; else `[1, 4]`.
fn intervals(raw: &RawValue) -> (u16, u16) {
    json::array(raw)
        .and_then(|items| items.into_iter().map(json::u16_value).collect::<Option<Vec<u16>>>())
        .and_then(|ints| match ints.as_slice() {
            [good, easy, ..] => Some((*good, *easy)),
            _ => None,
        })
        .unwrap_or((1, 4))
}

/// S10.2.
struct RevPart<'a> {
    bury: bool,
    ease4: f32,
    interval_factor: f32,
    max_interval: u32,
    per_day: u32,
    hard_factor: f32,
    extras: Vec<Member<'a>>,
}

impl<'a> RevPart<'a> {
    fn read(raw: &'a RawValue) -> RevPart<'a> {
        read_part(raw, |r| {
            Ok(RevPart {
                bury: r.strict("bury", json::boolean, Some(false))?,
                ease4: r.strict("ease4", json::f32_value, None)?,
                interval_factor: r.strict("ivlFct", json::f32_value, None)?,
                max_interval: r.strict("maxIvl", json::u32_value, None)?,
                per_day: r.lenient("perDay", json::u32_value, None)?,
                hard_factor: r.strict("hardFactor", json::f32_value, Some(1.2))?,
                extras: Vec::new(),
            })
        })
        .map(|(part, extras)| RevPart { extras, ..part })
        .unwrap_or(RevPart {
            bury: false,
            ease4: 1.3,
            interval_factor: 1.0,
            max_interval: 36_500,
            per_day: 200,
            hard_factor: 1.2,
            extras: Vec::new(),
        })
    }
}

/// S10.3.
struct LapsePart<'a> {
    delays: Vec<f32>,
    leech_action: i32,
    leech_fails: u32,
    min_interval: u32,
    mult: f32,
    extras: Vec<Member<'a>>,
}

impl<'a> LapsePart<'a> {
    fn read(raw: &'a RawValue) -> LapsePart<'a> {
        read_part(raw, |r| {
            Ok(LapsePart {
                delays: r.lenient("delays", json::f32_array, None)?,
                leech_action: r.strict(
                    "leechAction",
                    |v| Some(json::i32_value(v).filter(|a| matches!(a, 0 | 1)).unwrap_or(1)),
                    None,
                )?,
                leech_fails: r.strict("leechFails", json::u32_value, None)?,
                min_interval: r.strict("minInt", json::u32_value, None)?,
                mult: r.strict("mult", json::f32_value, None)?,
                extras: Vec::new(),
            })
        })
        .map(|(part, extras)| LapsePart { extras, ..part })
        .unwrap_or(LapsePart {
            delays: vec![10.0],
            leech_action: 1,
            leech_fails: 8,
            min_interval: 1,
            mult: 0.0,
            extras: Vec::new(),
        })
    }
}
