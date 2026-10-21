//! Collection-wide due lists and the W3–W9 chooser. The owner assigns build
//! identity and updates this value only after a successful answer commit.
use crate::states::{fuzz, interval_limits::FuzzedDayChooser};
use fuzz_rand::{
    SeedableRng,
    distr::{Distribution, weighted::WeightedIndex},
    rngs::StdRng,
};
use meguri_model::{
    clock::Clock,
    ids::{CardId, DeckConfigId, NoteId},
};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub card: CardId,
    pub note: NoteId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasyDay {
    Normal,
    Reduced,
    Minimum,
}
impl EasyDay {
    fn load(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Reduced => 0.5,
            Self::Minimum => 0.0001,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadBalancer {
    pub today: u32,
    pub next_day_at: i64,
    days: BTreeMap<DeckConfigId, [Vec<Entry>; 99]>,
    easy: BTreeMap<DeckConfigId, [EasyDay; 99]>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidEasyDays;
impl std::fmt::Display for InvalidEasyDays {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Easy Days must contain zero or seven values")
    }
}
impl std::error::Error for InvalidEasyDays {}

impl LoadBalancer {
    pub fn new(
        today: u32,
        next_day_at: i64,
        clock: &dyn Clock,
        presets: impl IntoIterator<Item = (DeckConfigId, Vec<f32>)>,
        rows: impl IntoIterator<Item = (DeckConfigId, u32, Entry)>,
    ) -> Result<Self, InvalidEasyDays> {
        let mut easy = BTreeMap::new();
        for (preset, values) in presets {
            if !values.is_empty() && values.len() != 7 {
                return Err(InvalidEasyDays);
            }
            let classes = std::array::from_fn(|day| {
                let at = next_day_at.wrapping_add((day as i64 - 1) * 86400);
                let local = at.wrapping_add(i64::from(clock.utc_offset_at(at.saturating_mul(1000)).seconds()));
                let weekday = (local.div_euclid(86400) + 3).rem_euclid(7) as usize;
                match values.get(weekday).copied().unwrap_or(1.0) {
                    1.0 => EasyDay::Normal,
                    0.0 => EasyDay::Minimum,
                    _ => EasyDay::Reduced,
                }
            });
            easy.insert(preset, classes);
        }
        let mut days: BTreeMap<_, [Vec<Entry>; 99]> = BTreeMap::new();
        for (preset, day, entry) in rows {
            if day < 99 {
                days.entry(preset)
                    .or_insert_with(|| std::array::from_fn(|_| Vec::new()))[day as usize]
                    .push(entry);
            }
        }
        Ok(Self {
            today,
            next_day_at,
            days,
            easy,
        })
    }
    pub fn entries(&self, preset: DeckConfigId, day: u32) -> &[Entry] {
        self.days
            .get(&preset)
            .and_then(|v| v.get(day as usize))
            .map_or(&[], |v| v.as_slice())
    }
    pub fn chooser(&self, preset: DeckConfigId, note: Option<NoteId>, seed: u64) -> Chooser<'_> {
        Chooser {
            cache: self,
            preset,
            note,
            seed,
        }
    }
    pub fn append(&mut self, preset: DeckConfigId, day: u32, entry: Entry) {
        if let Some(days) = self.days.get_mut(&preset)
            && let Some(entries) = days.get_mut(day as usize)
        {
            entries.push(entry);
        }
    }
    /// W9 removes one per day, including entries present at the build.
    pub fn remove_answer(&mut self, card: CardId) {
        for days in self.days.values_mut() {
            for entries in days {
                if let Some(at) = entries.iter().position(|e| e.card == card) {
                    entries.remove(at);
                }
            }
        }
    }
    pub fn sibling_factors(&self, note: Option<NoteId>, lower: u32, upper: u32) -> Vec<f32> {
        let mut factors = vec![1.0; (upper - lower + 1) as usize];
        let Some(note) = note else { return factors };
        for day in 0..99usize {
            if !self.days.values().any(|days| days[day].iter().any(|e| e.note == note)) {
                continue;
            }
            for (index, candidate) in (lower..=upper).enumerate() {
                let distance = (candidate as i64 - day as i64).unsigned_abs();
                if distance <= 5 {
                    factors[index] *= [0.000001, 0.2, 0.4, 0.6, 0.8, 1.0][distance as usize];
                }
            }
        }
        factors
    }
}
pub struct Chooser<'a> {
    cache: &'a LoadBalancer,
    preset: DeckConfigId,
    note: Option<NoteId>,
    seed: u64,
}
impl FuzzedDayChooser for Chooser<'_> {
    fn choose(&mut self, interval: f32, minimum: u32, maximum: u32) -> Option<u32> {
        if interval as u32 > 90 || minimum > 90 || !interval.is_finite() {
            return None;
        }
        let days = self.cache.days.get(&self.preset)?;
        let easy = self.cache.easy.get(&self.preset)?;
        let (lower, upper) = fuzz::range(interval, minimum, maximum);
        if lower < 1 || upper > 98 {
            return None;
        }
        let counts = (lower..=upper)
            .map(|d| days[d as usize].len() as u32)
            .collect::<Vec<_>>();
        let classes = (lower..=upper).map(|d| easy[d as usize]).collect::<Vec<_>>();
        let e = easy_factors(&counts, &classes);
        let s = self.cache.sibling_factors(self.note, lower, upper);
        let weights = (lower..=upper)
            .enumerate()
            .map(|(i, d)| weight(counts[i], d, s[i], e[i]))
            .collect::<Vec<_>>();
        draw(&weights, self.seed).map(|i| lower + i as u32)
    }
}
pub fn easy_factors(counts: &[u32], classes: &[EasyDay]) -> Vec<f32> {
    let total: u64 = counts.iter().map(|n| u64::from(*n)).sum();
    let load = classes.iter().fold(0.0f32, |sum, c| sum + c.load());
    counts
        .iter()
        .zip(classes)
        .map(|(&n, &class)| match class {
            EasyDay::Normal => 1.0,
            EasyDay::Minimum => 0.0001,
            EasyDay::Reduced => {
                if n as f32 / 0.5 > (total - u64::from(n)) as f32 / (load - 0.5) {
                    0.0001
                } else {
                    1.0
                }
            }
        })
        .collect()
}
/// W7 is deliberately separate from FSRS arithmetic: powf is specified here.
pub fn weight(count: u32, day: u32, sibling: f32, easy: f32) -> f32 {
    if count == 0 {
        return 1.0;
    }
    let a = (1.0f32 / count as f32).powf(2.15f32);
    let q = 1.0f32 / day as f32;
    let b = q * (q * q);
    ((a * b) * sibling) * easy
}
pub fn draw(weights: &[f32], seed: u64) -> Option<usize> {
    if weights.iter().any(|w| !w.is_finite()) {
        return None;
    }
    let draw = WeightedIndex::new(weights).ok()?;
    Some(draw.sample(&mut StdRng::seed_from_u64(seed)))
}
