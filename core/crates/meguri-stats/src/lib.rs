//! Statistics: review aggregates, heatmap and streaks, graphs, the Statistics
//! page's request and its parts, Card Info and the widget snapshot.

pub mod card_info;
pub mod graphs;
pub mod history;
pub mod page;
pub mod parts;
pub mod text;

use meguri_engine::{Collection, EngineError};
use meguri_model::ids::CardId;
use meguri_sched::{
    model::sched_config::SchedConfig,
    timing::{DayInputs, sched_day},
};
use meguri_storage::{ReadRows, ReaderRole};

use card_info::{CardInfo, CardInfoError, ErrorKind, Input};

/// What a Card Info read gives: the snapshot, or C11's unavailable state.
#[derive(Debug)]
pub enum CardInfoOutcome {
    Found(Box<CardInfo>),
    Unavailable(ErrorKind),
}

/// Card Info for `card_id` (`stats/card-info` C1), on one snapshot of a
/// query reader, at the collection clock's instant. The day is the pure day
/// calculation (`sched/day-clock` D3, D4): the read never runs the day query,
/// which can write, and never writes anything else.
pub fn card_info(collection: &Collection, card_id: CardId) -> Result<CardInfoOutcome, EngineError> {
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let clock = collection.clock();
    let at = clock.now_ms();
    let config = SchedConfig::new(&*snapshot);
    let day = sched_day(&DayInputs {
        crt: snapshot.col()?.created.0,
        creation_offset: config.creation_offset()?,
        rollover_hour: config.rollover_hour()?,
        offset: clock.utc_offset_at(at),
        now: at.div_euclid(1000),
    });
    let input = Input {
        card_id,
        now: at.div_euclid(1000),
        days_elapsed: day.today,
        next_day_at: day.next_day_at,
    };
    match card_info::build(&*snapshot, clock, &input) {
        Ok(mut info) => {
            info.dependencies.generation = collection.generation().0;
            Ok(CardInfoOutcome::Found(Box::new(info)))
        }
        Err(CardInfoError::Unavailable(kind)) => Ok(CardInfoOutcome::Unavailable(kind)),
        Err(CardInfoError::Storage(error)) => Err(error.into()),
    }
}

/// C8's `card_info_debug_json`: the dump of the same snapshot, or the
/// unavailable kind.
pub fn card_info_debug_json(collection: &Collection, card_id: CardId) -> Result<Result<String, ErrorKind>, EngineError> {
    Ok(match card_info(collection, card_id)? {
        CardInfoOutcome::Found(info) => Ok(info.debug_json()),
        CardInfoOutcome::Unavailable(kind) => Err(kind),
    })
}
