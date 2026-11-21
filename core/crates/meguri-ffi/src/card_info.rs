//! Card Info over FFI (`stats/card-info` C1 to C11, U92): one blocking
//! query-lane read per call. The texts are the core's (C12); the view adds
//! only its String Catalog labels, keyed by `CardInfoRow::key`.
use crate::{callback, error::CoreError, runtime};
use meguri_model::{card_data::MemoryState, ids::CardId, revlog::RevlogKind};
use meguri_stats::{
    CardInfoOutcome,
    card_info::{self, CardInfo, ErrorKind, RangeKind},
};

/// C11's kinds; the view shows its unavailable state for each (C12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardInfoUnavailable {
    NotFound,
    UnreadableRecord,
    MissingNote,
    MissingNotetype,
    MissingTemplate,
    MissingDeck,
    MissingHomeDeck,
    FilteredHomeDeck,
    MissingPreset,
    InvalidCutoff,
    FsrsFailed,
}

impl From<ErrorKind> for CardInfoUnavailable {
    fn from(kind: ErrorKind) -> Self {
        match kind {
            ErrorKind::NotFound => Self::NotFound,
            ErrorKind::UnreadableRecord => Self::UnreadableRecord,
            ErrorKind::MissingNote => Self::MissingNote,
            ErrorKind::MissingNotetype => Self::MissingNotetype,
            ErrorKind::MissingTemplate => Self::MissingTemplate,
            ErrorKind::MissingDeck => Self::MissingDeck,
            ErrorKind::MissingHomeDeck => Self::MissingHomeDeck,
            ErrorKind::FilteredHomeDeck => Self::FilteredHomeDeck,
            ErrorKind::MissingPreset => Self::MissingPreset,
            ErrorKind::InvalidCutoff => Self::InvalidCutoff,
            ErrorKind::FsrsFailed => Self::FsrsFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoMemoryState {
    pub stability: f32,
    pub difficulty: f32,
}

fn memory(state: Option<MemoryState>) -> Option<CardInfoMemoryState> {
    state.map(|state| CardInfoMemoryState {
        stability: state.stability,
        difficulty: state.difficulty,
    })
}

/// C2's kinds of a review-log row; U222 colours them (R1426).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardInfoReviewKind {
    Learn,
    Review,
    Relearn,
    Filtered,
    Manual,
    Rescheduled,
}

impl From<RevlogKind> for CardInfoReviewKind {
    fn from(kind: RevlogKind) -> Self {
        match kind {
            RevlogKind::Learning => Self::Learn,
            RevlogKind::Review => Self::Review,
            RevlogKind::Relearning => Self::Relearn,
            RevlogKind::Filtered => Self::Filtered,
            RevlogKind::Manual => Self::Manual,
            RevlogKind::Rescheduled => Self::Rescheduled,
        }
    }
}

/// C3's raw values (the oracle's names, C13).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoRaw {
    pub card_id: i64,
    pub note_id: i64,
    pub added: i64,
    pub card_type: String,
    pub notetype: String,
    pub deck: String,
    pub original_deck: Option<String>,
    pub preset: String,
    pub interval: u32,
    pub ease: u16,
    pub reviews: u32,
    pub lapses: u32,
    pub custom_data: String,
    pub desired_retention: Option<f32>,
    pub fsrs_params: Vec<f32>,
    pub first_review: Option<i64>,
    pub latest_review: Option<i64>,
    pub due_date: Option<i64>,
    pub due_position: Option<i32>,
    pub memory_state: Option<CardInfoMemoryState>,
    /// `stored` or `computed` (C3.4), absent without a memory state.
    pub memory_state_source: Option<String>,
    pub fsrs_retrievability: Option<f32>,
    pub average_secs: f32,
    pub total_secs: f32,
}

/// A C4 row: the String Catalog key `cardInfo.row.<key>` and the value text.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardInfoRow {
    pub key: String,
    pub value: String,
}

/// A history entry (C6.1) with its column texts (C6.2), newest first.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoHistoryEntry {
    pub revlog_id: i64,
    pub instant: i64,
    pub kind: CardInfoReviewKind,
    pub rating: u8,
    pub interval_secs: i64,
    pub last_interval_secs: i64,
    pub factor: u32,
    pub taken_secs: f32,
    pub memory_state: Option<CardInfoMemoryState>,
    pub date_text: String,
    pub date_time_text: String,
    pub kind_text: String,
    pub rating_text: String,
    pub interval_text: String,
    pub ease_text: String,
    pub time_text: String,
    pub stability_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardInfoCurveRangeKind {
    FirstWeek,
    FirstMonth,
    FirstYear,
    AllTime,
}

impl From<RangeKind> for CardInfoCurveRangeKind {
    fn from(kind: RangeKind) -> Self {
        match kind {
            RangeKind::FirstWeek => Self::FirstWeek,
            RangeKind::FirstMonth => Self::FirstMonth,
            RangeKind::FirstYear => Self::FirstYear,
            RangeKind::AllTime => Self::AllTime,
        }
    }
}

/// One point the view draws (C7.5), with its tooltip texts (C7.6). The
/// tooltip's date is the view's own formatting.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoCurvePoint {
    pub instant: f64,
    pub days_since_first: f64,
    pub days_since_review: f64,
    /// Percent.
    pub retrievability: f64,
    /// Days.
    pub stability: f64,
    /// Drawn dashed (after now).
    pub future: bool,
    pub elapsed_text: String,
    pub retrievability_text: String,
    pub stability_text: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoCurveRange {
    pub kind: CardInfoCurveRangeKind,
    pub span_days: f64,
    pub points: Vec<CardInfoCurvePoint>,
    pub y_min: f64,
    /// Whether the desired-retention line is above `y_min` and drawn.
    pub shows_retention: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoCurve {
    /// Offered ranges, First Week first; one range means no picker.
    pub ranges: Vec<CardInfoCurveRange>,
    pub default_range: CardInfoCurveRangeKind,
    pub desired_retention: f32,
    pub desired_retention_text: String,
}

/// What the snapshot depends on (C10), for the change watch (U103).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CardInfoDependencies {
    pub card_id: i64,
    pub note_id: i64,
    pub notetype_id: i64,
    pub deck_id: i64,
    pub home_deck_id: i64,
    pub preset_id: i64,
    pub revlog_ids: Vec<i64>,
    /// Refresh at local midnight (C10.2).
    pub due_from_day_number: bool,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CardInfoSnapshot {
    pub raw: CardInfoRaw,
    pub rows: Vec<CardInfoRow>,
    pub history: Vec<CardInfoHistoryEntry>,
    /// `None`: no chart (C7.1).
    pub curve: Option<CardInfoCurve>,
    pub dependencies: CardInfoDependencies,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum CardInfoResult {
    Found { snapshot: CardInfoSnapshot },
    Unavailable { kind: CardInfoUnavailable },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardInfoDebugResult {
    Found { json: String },
    Unavailable { kind: CardInfoUnavailable },
}

impl From<&CardInfo> for CardInfoSnapshot {
    fn from(info: &CardInfo) -> Self {
        let raw = &info.raw;
        let dependencies = &info.dependencies;
        CardInfoSnapshot {
            raw: CardInfoRaw {
                card_id: raw.card_id.0,
                note_id: raw.note_id.0,
                added: raw.added,
                card_type: raw.card_type.clone(),
                notetype: raw.notetype.clone(),
                deck: raw.deck.clone(),
                original_deck: raw.original_deck.clone(),
                preset: raw.preset.clone(),
                interval: raw.interval,
                ease: raw.ease,
                reviews: raw.reviews,
                lapses: raw.lapses,
                custom_data: raw.custom_data.clone(),
                desired_retention: raw.desired_retention,
                fsrs_params: raw.fsrs_params.clone(),
                first_review: raw.first_review,
                latest_review: raw.latest_review,
                due_date: raw.due_date,
                due_position: raw.due_position,
                memory_state: memory(raw.memory_state),
                memory_state_source: raw.memory_state_source.map(|source| source.as_str().to_owned()),
                fsrs_retrievability: raw.fsrs_retrievability,
                average_secs: raw.average_secs,
                total_secs: raw.total_secs,
            },
            rows: info
                .rows
                .iter()
                .map(|row| CardInfoRow {
                    key: row.key.as_str().to_owned(),
                    value: row.value.clone(),
                })
                .collect(),
            history: info
                .history
                .iter()
                .map(|entry| CardInfoHistoryEntry {
                    revlog_id: entry.revlog_id.0,
                    instant: entry.instant,
                    kind: entry.kind.into(),
                    rating: entry.rating,
                    interval_secs: entry.interval_secs,
                    last_interval_secs: entry.last_interval_secs,
                    factor: entry.factor,
                    taken_secs: entry.taken_secs,
                    memory_state: memory(entry.memory_state),
                    date_text: entry.texts.date.clone(),
                    date_time_text: entry.texts.date_time.clone(),
                    kind_text: entry.texts.kind.clone(),
                    rating_text: entry.texts.rating.clone(),
                    interval_text: entry.texts.interval.clone(),
                    ease_text: entry.texts.ease.clone(),
                    time_text: entry.texts.time.clone(),
                    stability_text: entry.texts.stability.clone(),
                })
                .collect(),
            curve: info.curve.as_ref().map(|curve| CardInfoCurve {
                ranges: curve
                    .ranges
                    .iter()
                    .map(|range| CardInfoCurveRange {
                        kind: range.kind.into(),
                        span_days: range.span_days,
                        points: range
                            .points
                            .iter()
                            .map(|point| CardInfoCurvePoint {
                                instant: point.instant,
                                days_since_first: point.days_since_first,
                                days_since_review: point.days_since_review,
                                retrievability: point.retrievability,
                                stability: point.stability,
                                future: point.future,
                                elapsed_text: point.elapsed_text.clone(),
                                retrievability_text: point.retrievability_text.clone(),
                                stability_text: point.stability_text.clone(),
                            })
                            .collect(),
                        y_min: range.y_min,
                        shows_retention: range.shows_retention,
                    })
                    .collect(),
                default_range: curve.default_range.into(),
                desired_retention: curve.desired_retention,
                desired_retention_text: curve.desired_retention_text.clone(),
            }),
            dependencies: CardInfoDependencies {
                card_id: dependencies.card.0,
                note_id: dependencies.note.0,
                notetype_id: dependencies.notetype.0,
                deck_id: dependencies.deck.0,
                home_deck_id: dependencies.home_deck.0,
                preset_id: dependencies.preset.0,
                revlog_ids: dependencies.revlog.iter().map(|id| id.0).collect(),
                due_from_day_number: dependencies.due_from_day_number,
                generation: dependencies.generation,
            },
        }
    }
}

/// Card Info for one card (C1). Previous Card Info calls it with the
/// previous card's id (C9).
#[uniffi::export]
pub fn card_info(card_id: i64) -> Result<CardInfoResult, CoreError> {
    callback::enter("card_info");
    let collection = runtime::engine()?.collection()?;
    Ok(match meguri_stats::card_info(&collection, CardId(card_id))? {
        CardInfoOutcome::Found(info) => CardInfoResult::Found {
            snapshot: (&*info).into(),
        },
        CardInfoOutcome::Unavailable(kind) => CardInfoResult::Unavailable { kind: kind.into() },
    })
}

/// C8: the debug dump for Copy Debug Info.
#[uniffi::export]
pub fn card_info_debug_json(card_id: i64) -> Result<CardInfoDebugResult, CoreError> {
    callback::enter("card_info_debug_json");
    let collection = runtime::engine()?.collection()?;
    Ok(match meguri_stats::card_info_debug_json(&collection, CardId(card_id))? {
        Ok(json) => CardInfoDebugResult::Found { json },
        Err(kind) => CardInfoDebugResult::Unavailable { kind: kind.into() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_unavailable_kind_maps_to_its_own_case() {
        let kinds = [
            ErrorKind::NotFound,
            ErrorKind::UnreadableRecord,
            ErrorKind::MissingNote,
            ErrorKind::MissingNotetype,
            ErrorKind::MissingTemplate,
            ErrorKind::MissingDeck,
            ErrorKind::MissingHomeDeck,
            ErrorKind::FilteredHomeDeck,
            ErrorKind::MissingPreset,
            ErrorKind::InvalidCutoff,
            ErrorKind::FsrsFailed,
        ];
        let mapped: std::collections::BTreeSet<String> = kinds
            .iter()
            .map(|kind| format!("{:?}", CardInfoUnavailable::from(*kind)))
            .collect();
        assert_eq!(mapped.len(), kinds.len());
        assert_eq!(CardInfoUnavailable::from(ErrorKind::FsrsFailed), CardInfoUnavailable::FsrsFailed);
        assert_eq!(CardInfoReviewKind::from(RevlogKind::Relearning), CardInfoReviewKind::Relearn);
        assert_eq!(
            CardInfoCurveRangeKind::from(card_info::RangeKind::AllTime),
            CardInfoCurveRangeKind::AllTime
        );
    }
}
