//! One card's knowledge (`stats/history-reads` H3.1): its chance of recall
//! at each kept sample instant, from an FSRS replay of only its rows at or
//! before that instant with its home preset as it is today, and its memory
//! state over every row, from which each request computes today's point.
//!
//! - The replay is `meguri_sched`'s memory items (`sched/fsrs-items` I3 to
//!   I5) on the pinned crate: one `historical_memory_states` call per
//!   history start, read at each prefix of the rows (C6.3's states are the
//!   ones `memory` gives for each prefix); on a crate error each prefix of
//!   that start is replayed with `memory`, so the first failing instant is
//!   exact.
//! - A chance of recall is kept as a whole number of 2^-40 cards: sums are
//!   exact integers, so a sum kept through any sequence of changes equals a
//!   cold one bit for bit, and the rounding of each card is under 1e-12.

use std::collections::HashMap;

use fsrs::FSRS;
use meguri_engine::activity::civil;
use meguri_model::ids::{CardId, RevlogId};
use meguri_model::revlog::RevlogEntry;
use meguri_sched::memory::ignore_before;
use meguri_sched::memory::items::{History, HistoryForm, is_graded};
use meguri_sched::model::preset::PresetView;
use meguri_sched::states::fsrs_glue::FsrsCache;

use super::replay::{DAY, Grid};
use crate::card_info::curve::recall;
use crate::graphs::context::Context;
use crate::graphs::rows::StatsRow;

/// 2^40: one card's whole chance of recall.
pub(crate) const SCALE: f64 = 1_099_511_627_776.0;

/// A chance of recall kept in [0, 1] (below 0 counts 0, above 1 counts 1,
/// NaN counts 0: H3.1), in 2^-40 cards.
pub(crate) fn quantize(recall: f64) -> i64 {
    let recall = if recall.is_nan() { 0.0 } else { recall.clamp(0.0, 1.0) };
    (recall * SCALE).round() as i64
}

/// A sum of quantized recall probabilities, in cards.
pub(crate) fn in_cards(sum: i128) -> f64 {
    sum as f64 / SCALE
}

/// Daily samples kept past the frame's latest day, so a request up to this
/// many rollovers later needs no rebuild (H14.3's new days).
pub(crate) const HORIZON: i64 = 92;
/// H3.2: the ends of days −365 to −1.
const DAILY: i64 = 365;

/// Where the kept samples sit (H3.2, H14.1): the latest daily sample's day
/// on the component's grid, and the date of each day (H1, AH3's `T0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    /// The day before today when the frame was set.
    pub latest: i64,
    /// Day `k` of the grid falls on `k + anchor` days after 1970-01-01.
    pub anchor: i64,
}

impl Frame {
    /// The frame a request with `context` reads.
    pub(crate) fn of(context: &Context) -> Frame {
        let today = context.next_day_at.div_euclid(DAY);
        let date = meguri_engine::activity::today_day_number(
            context.next_day_at,
            context.rollover_hour,
            i64::from(context.utc_offset_secs),
        );
        Frame {
            latest: today - 1,
            anchor: date - today,
        }
    }

    /// Whether samples kept on `self` hold every sample a request on
    /// `wanted` reads.
    pub(crate) fn serves(self, wanted: Frame) -> bool {
        self.anchor == wanted.anchor && self.latest <= wanted.latest && wanted.latest <= self.latest + HORIZON
    }

    /// Whether `day` is the last day of a calendar month (H1's dates).
    pub(crate) fn is_month_end(self, day: i64) -> bool {
        civil(day + 1 + self.anchor).day == 1
    }

    /// The request's first daily sample.
    pub(crate) fn first_daily(self) -> i64 {
        self.latest - (DAILY - 1)
    }

    /// Every day whose end the component keeps a sample at, ascending: the
    /// month ends before the daily samples (rows have `id` ≥ 0, so none is
    /// on a day before 0), then every day to [`HORIZON`] days past the
    /// latest.
    pub(crate) fn kept_days(self) -> Vec<i64> {
        let first = self.first_daily();
        let mut days: Vec<i64> = (0..first.max(0)).filter(|day| self.is_month_end(*day)).collect();
        days.extend(first.max(0)..=self.latest + HORIZON);
        days
    }

    /// The day ends a request on this frame samples (H3.2), ascending:
    /// the end of each day from −365 to −1 and, with `months`, before them
    /// the end of each month back to the month of `oldest` (a day).
    pub(crate) fn sample_days(self, months: Option<i64>) -> Vec<i64> {
        let first = self.first_daily();
        let mut days = Vec::new();
        if let Some(oldest) = months {
            let mut day = oldest.max(0);
            // The month of `oldest` begins on or before it: its end is the
            // first month end from `oldest` on.
            while day < first {
                if self.is_month_end(day) {
                    days.push(day);
                }
                day += 1;
            }
        }
        days.extend(first.max(0)..=self.latest);
        days
    }
}

/// What H3.1 reads of a home preset as it is today: its parameter list and
/// decay (`sched/presets` V6), its cutoff (`sched/fsrs-items` I1.3) and its
/// historical retention (I1.4).
#[derive(Clone, Debug)]
pub(crate) struct PresetInputs {
    pub id: i64,
    pub params: Vec<f32>,
    /// The crate takes the list (`sched` T1/T13, as answers check it).
    pub usable: bool,
    pub decay: f32,
    /// I1.3's `T` in epoch milliseconds, or `None` for a stored text the
    /// reader refuses.
    pub cutoff: Option<i64>,
    pub retention: f32,
}

impl PresetInputs {
    pub(crate) fn of(id: i64, view: &PresetView) -> PresetInputs {
        PresetInputs {
            id,
            params: view.parameters().to_vec(),
            usable: FsrsCache::default().model(view).is_ok(),
            decay: view.decay(),
            cutoff: ignore_before::cutoff_millis(&view.fields().ignore_revlogs_before_date).ok(),
            retention: view.fields().historical_retention,
        }
    }

    /// The same values, bit for bit: a replay with either gives the same.
    pub(crate) fn same(&self, other: &PresetInputs) -> bool {
        let bits = |values: &[f32]| values.iter().map(|v| v.to_bits()).collect::<Vec<_>>();
        self.id == other.id
            && bits(&self.params) == bits(&other.params)
            && self.usable == other.usable
            && self.decay.to_bits() == other.decay.to_bits()
            && self.cutoff == other.cutoff
            && self.retention.to_bits() == other.retention.to_bits()
    }
}

/// A home preset ready for replays.
#[derive(Clone, Debug)]
pub(crate) struct Preset {
    pub inputs: PresetInputs,
    /// `None` when the crate refuses the list or the cutoff text is
    /// refused: every replay with the preset fails.
    model: Option<FSRS>,
    decay: f64,
}

impl Preset {
    pub(crate) fn new(inputs: PresetInputs) -> Preset {
        let model = if inputs.usable && inputs.cutoff.is_some() {
            FSRS::new(&inputs.params).ok()
        } else {
            None
        };
        Preset {
            decay: f64::from(inputs.decay),
            model,
            inputs,
        }
    }

    pub(crate) fn decay(&self) -> f64 {
        self.decay
    }
}

/// A card's memory at one cut of its rows (H3.1 steps 1 and 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Memory {
    /// The rows give no item: not studied there.
    None,
    /// The item's memory state and the instant `L` (epoch seconds) of its
    /// newest kept row.
    State { stability: f32, difficulty: f32, last: i64 },
    /// The crate refused the item (or the preset): left out, `failed`.
    Failed,
}

impl Memory {
    pub(crate) fn code(self) -> i64 {
        match self {
            Memory::None => 0,
            Memory::State { .. } => 1,
            Memory::Failed => 2,
        }
    }

    /// The card's quantized chance of recall at `instant` (epoch seconds),
    /// with `decay` (H3.1 step 3), when it has a state.
    pub(crate) fn recall_at(self, instant: i64, decay: f64) -> Option<i64> {
        match self {
            Memory::State { stability, last, .. } => {
                let days = (instant - last) as f64 / 86_400.0;
                Some(quantize(recall(days, f64::from(stability), decay)))
            }
            _ => None,
        }
    }
}

/// One card's knowledge (H14.1): what it adds at each kept sample day, its
/// memory over every row, with and without its preset's cutoff (today's
/// point reads the first when now is at or after the cutoff instant), and
/// the first instant its replay fails at.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Knowledge {
    /// Each kept sample day it is studied on, with its quantized chance.
    pub points: Vec<(i64, i64)>,
    pub cut: Memory,
    pub free: Memory,
    /// H3.1's crate error: from this whole second on, the card is out of
    /// every sample and in `failed`. It depends only on the card's rows and
    /// preset, never on which samples a store keeps.
    pub failed_at: Option<i64>,
}

impl Knowledge {
    pub(crate) const NONE: Knowledge = Knowledge {
        points: Vec::new(),
        cut: Memory::None,
        free: Memory::None,
        failed_at: None,
    };

    /// The memory today's point uses at `now_ms`, for a preset whose
    /// cutoff is `cutoff`.
    pub(crate) fn now(cut: Memory, free: Memory, failed_at: Option<i64>, cutoff: Option<i64>, now_ms: i64) -> Memory {
        if failed_at.is_some_and(|failed| failed <= now_ms.div_euclid(1_000)) {
            return Memory::Failed;
        }
        match cutoff {
            Some(cutoff) if now_ms < cutoff => free,
            _ => cut,
        }
    }
}

/// The review-log row as the memory items read it.
fn entry(row: &StatsRow, card: i64) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(row.id),
        card_id: CardId(card),
        usn: Default::default(),
        button: row.ease,
        interval: row.ivl,
        last_interval: row.last_ivl,
        ease_factor: row.factor,
        answer_millis: row.time,
        kind: row.kind,
    }
}

/// A row's instant, whole seconds (H1's `floor(id / 1,000)`).
fn second(row: &RevlogEntry) -> i64 {
    row.id.0.div_euclid(1_000)
}

/// One cut of the rows: the first `rows` rows, with or without the cutoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Cut {
    rows: usize,
    with_cutoff: bool,
}

/// What a cut selects (I3): where its history starts and the kept rows it
/// holds, with the instant of the newest.
#[derive(Clone, Copy, Debug)]
struct Selected {
    start: usize,
    complete: bool,
    kept: usize,
    last: i64,
}

/// H3.1 for one existing, unsuspended card with readable rows sorted by
/// `id`, on the kept sample `days` (ascending); `preset` is its home preset,
/// `None` when it has none (a missing or filtered home deck). I4's days are
/// counted back from `reference`: [`Grid::reference`] for kept samples,
/// whose rows are all before any request's `N`; a request's `N` for today's
/// point of a card with a row at or after it.
pub(crate) fn knowledge(
    grid: Grid,
    days: &[i64],
    card: i64,
    rows: &[StatsRow],
    preset: Option<&Preset>,
    reference: i64,
) -> Knowledge {
    let entries: Vec<RevlogEntry> = rows.iter().map(|row| entry(row, card)).collect();
    // No graded row gives no item at any cut (I3), whatever the preset.
    if !entries.iter().any(is_graded) {
        return Knowledge::NONE;
    }
    let cutoff = preset.and_then(|preset| preset.inputs.cutoff);
    Replay {
        entries: &entries,
        // No model (a refused list or cutoff, no home preset): every cut
        // that gives an item fails.
        model: preset.and_then(|preset| preset.model.as_ref()),
        retention: preset.map_or(0.9, |preset| preset.inputs.retention),
        cutoff: cutoff.unwrap_or(i64::MIN),
        // A cutoff before every row cuts nothing: both forms are the same.
        cuts_nothing: cutoff.is_none_or(|cutoff| entries.first().is_some_and(|first| cutoff < first.id.0)),
        reference,
    }
    .run(grid, days, preset.map_or(0.0, Preset::decay))
}

struct Replay<'a> {
    entries: &'a [RevlogEntry],
    model: Option<&'a FSRS>,
    retention: f32,
    cutoff: i64,
    cuts_nothing: bool,
    reference: i64,
}

impl Replay<'_> {
    fn history(&self, cut: Cut) -> Option<History<'_>> {
        let cutoff = if cut.with_cutoff { self.cutoff } else { i64::MIN };
        History::select(&self.entries[..cut.rows], cutoff, HistoryForm::Memory)
    }

    fn select(&self, cut: Cut) -> Option<Selected> {
        let history = self.history(cut)?;
        let kept: Vec<&RevlogEntry> = history.kept().collect();
        Some(Selected {
            start: cut.rows - history.len(),
            complete: history.complete,
            kept: kept.len(),
            last: second(kept.last()?),
        })
    }

    /// The cut at whole second `instant`: the rows at or before it, with the
    /// cutoff from the cutoff instant on.
    fn cut_at(&self, instant: i64) -> Cut {
        Cut {
            rows: self.entries.partition_point(|row| second(row) <= instant),
            with_cutoff: self.cuts_nothing || instant.saturating_mul(1_000) >= self.cutoff,
        }
    }

    /// The memory at every cut of the rows (each prefix, with and without
    /// the cutoff): one `historical_memory_states` call per history start,
    /// over its longest cut; on a crate error, one `memory` call per cut.
    fn states_at(&self, cuts: &[Cut]) -> HashMap<Cut, Memory> {
        let selected: Vec<Option<Selected>> = cuts.iter().map(|cut| self.select(*cut)).collect();
        let mut states_at: HashMap<Cut, Memory> = cuts.iter().map(|cut| (*cut, Memory::None)).collect();
        let Some(model) = self.model else {
            for (cut, found) in cuts.iter().zip(&selected) {
                if found.is_some() {
                    states_at.insert(*cut, Memory::Failed);
                }
            }
            return states_at;
        };
        // Each history start, with the longest cut that has it.
        let mut starts: Vec<((usize, bool), usize)> = Vec::new();
        for (at, found) in selected.iter().enumerate() {
            let Some(found) = found else { continue };
            let key = (found.start, found.complete);
            match starts.iter_mut().find(|(start, _)| *start == key) {
                Some((_, longest)) if cuts[*longest].rows < cuts[at].rows => *longest = at,
                Some(_) => {}
                None => starts.push((key, at)),
            }
        }
        let state = |state: meguri_model::card_data::MemoryState, found: &Selected| Memory::State {
            stability: state.stability,
            difficulty: state.difficulty,
            last: found.last,
        };
        for (key, longest) in starts {
            let states = self
                .history(cuts[longest])
                .map(|history| history.historical(model, self.reference, self.retention));
            for (at, found) in selected.iter().enumerate() {
                let Some(found) = found.filter(|found| (found.start, found.complete) == key) else { continue };
                let memory = match &states {
                    Some(Ok(states)) => states.get(found.kept - 1).map_or(Memory::Failed, |s| state(*s, &found)),
                    _ => match self.history(cuts[at]).map(|h| h.memory(model, self.reference, self.retention)) {
                        Some(Ok(memory)) => state(memory, &found),
                        _ => Memory::Failed,
                    },
                };
                states_at.insert(cuts[at], memory);
            }
        }
        states_at
    }

    fn run(&self, grid: Grid, days: &[i64], decay: f64) -> Knowledge {
        let all = self.entries.len();
        let forms: &[bool] = if self.cuts_nothing { &[true] } else { &[true, false] };
        let cuts: Vec<Cut> = (1..=all)
            .flat_map(|rows| forms.iter().map(move |with_cutoff| Cut { rows, with_cutoff: *with_cutoff }))
            .collect();
        let states_at = self.states_at(&cuts);
        let memory = |cut: Cut| states_at.get(&cut).copied().unwrap_or(Memory::None);
        // H3.1's first failing instant, whatever the samples: the cut
        // changes only at a row's second and at the cutoff instant.
        let mut changes: Vec<i64> = self.entries.iter().map(second).collect();
        if !self.cuts_nothing {
            changes.push(self.cutoff.div_euclid(1_000) + i64::from(self.cutoff.rem_euclid(1_000) != 0));
        }
        changes.sort_unstable();
        let failed_at = changes
            .into_iter()
            .find(|instant| matches!(memory(self.cut_at(*instant)), Memory::Failed));
        let first_day = grid.row_day(self.entries[0].id.0);
        let days = &days[days.partition_point(|day| *day < first_day)..];
        let mut points = Vec::with_capacity(days.len());
        for day in days {
            let instant = grid.end(*day);
            if failed_at.is_some_and(|failed| failed <= instant) {
                // Out of that sample and every later one.
                break;
            }
            if let Some(recall) = memory(self.cut_at(instant)).recall_at(instant, decay) {
                points.push((*day, recall));
            }
        }
        // Today's point reads every row (H1: "for now, every row").
        let cut = memory(Cut {
            rows: all,
            with_cutoff: true,
        });
        let free = if self.cuts_nothing {
            cut
        } else {
            memory(Cut {
                rows: all,
                with_cutoff: false,
            })
        };
        Knowledge {
            points,
            cut,
            free,
            failed_at,
        }
    }
}
