//! Read views never reset stored counters. The answer operation owns stamps,
//! ancestor traversal and persistence (sched/presets D1–D5).
use meguri_model::{
    card::CardQueue,
    proto::deck::{DayLimit, DeckCommon, NormalDeck},
};

pub struct DeckSched<'a> {
    normal: Option<&'a NormalDeck>,
    common: &'a DeckCommon,
}

impl<'a> DeckSched<'a> {
    pub fn new(normal: Option<&'a NormalDeck>, common: &'a DeckCommon) -> Self {
        Self { normal, common }
    }
    pub fn new_limit(&self, today: u32, preset: u32) -> Option<u32> {
        let normal = self.normal?;
        Some(limit(&normal.new_limit_today, normal.new_limit, today, preset))
    }
    pub fn review_limit(&self, today: u32, preset: u32) -> Option<u32> {
        let normal = self.normal?;
        Some(limit(&normal.review_limit_today, normal.review_limit, today, preset))
    }
    pub fn retention(&self, preset: f32) -> f32 {
        self.normal.and_then(|n| n.desired_retention).unwrap_or(preset)
    }
    pub fn studied(&self, today: u32) -> (i32, i32) {
        if self.common.last_day_studied == today {
            (self.common.new_studied, self.common.review_studied)
        } else {
            (0, 0)
        }
    }
    pub fn record_answer(common: &mut DeckCommon, today: u32, queue: CardQueue, milliseconds: u32, cap_seconds: u32) {
        if common.last_day_studied != today {
            common.last_day_studied = today;
            common.new_studied = 0;
            common.review_studied = 0;
            common.milliseconds_studied = 0;
        }
        match queue {
            CardQueue::New => common.new_studied = common.new_studied.wrapping_add(1),
            CardQueue::Review | CardQueue::InterdayLearning => {
                common.review_studied = common.review_studied.wrapping_add(1)
            }
            _ => {}
        }
        let time = milliseconds.min(cap_seconds.saturating_mul(1000));
        common.milliseconds_studied = common.milliseconds_studied.wrapping_add(time as i32);
    }
}

fn limit(today_limit: &Option<DayLimit>, permanent: Option<u32>, today: u32, preset: u32) -> u32 {
    today_limit
        .as_ref()
        .filter(|v| v.today == today)
        .map(|v| v.limit)
        .or(permanent)
        .unwrap_or(preset)
}

pub fn clear_today_limit(limit: &mut Option<DayLimit>, today: u32) {
    let Some(value) = limit else { return };
    if value.today == 0 || value.today > today {
        *limit = None;
    } else {
        value.today = value.today.min(today - 1);
    }
}
