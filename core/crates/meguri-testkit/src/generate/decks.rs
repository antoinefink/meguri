//! Presets and the deck tree. The default deck and the default preset have
//! id 1 (`ids-usn-mtime` I1); every preset is the new-collection default of
//! `model/protobuf-records` P6.6 with sibling burying on (R1804). Deck names
//! are Meguri's own.

use meguri_model::deck::{Deck, native_name};
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{DeckConfigId, DeckId};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck};
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::profile::Profile;
use super::rng::Rng;
use super::{DESIRED_RETENTION, Ids, Timeline, WorstCase};

/// The preset values of P6.6's default-preset example, sibling burying on.
pub fn default_preset_message() -> DeckConfigMessage {
    let mut m = DeckConfigMessage::default();
    m.learn_steps = vec![1.0, 10.0];
    m.relearn_steps = vec![10.0];
    m.easy_days_percentages = vec![1.0; 7];
    m.new_per_day = 20;
    m.reviews_per_day = 200;
    m.initial_ease = 2.5;
    m.easy_multiplier = 1.3;
    m.hard_multiplier = 1.2;
    m.interval_multiplier = 1.0;
    m.maximum_review_interval = 36_500;
    m.minimum_lapse_interval = 1;
    m.graduating_interval_good = 1;
    m.graduating_interval_easy = 4;
    m.leech_action = 1;
    m.leech_threshold = 8;
    m.cap_answer_time_to_secs = 60;
    m.desired_retention = DESIRED_RETENTION;
    m.historical_retention = 0.9;
    m.wait_for_audio = true;
    m.bury_new = true;
    m.bury_reviews = true;
    m.bury_interday_learning = true;
    m
}

pub fn preset(id: DeckConfigId, name: &str, message: DeckConfigMessage, timeline: &Timeline) -> DeckConfig {
    DeckConfig {
        id,
        name: name.to_owned(),
        mtime: EpochSecs(timeline.crt),
        usn: Usn::PENDING,
        config: ProtoBlob::new(message),
    }
}

/// The profile's presets: `Default` (id 1), then `Preset 2`, `Preset 3`...
/// In the due worst case the second preset carries its review limit.
pub fn presets(profile: &Profile, worst: Option<&WorstCase>, timeline: &Timeline, ids: &mut Ids) -> Vec<DeckConfig> {
    let count = profile.totals.presets.max(1);
    (0..count)
        .map(|i| {
            let mut message = default_preset_message();
            let id = if i == 0 {
                ids.claim(1)
            } else {
                ids.claim(timeline.crt_ms() + i as i64)
            };
            if i == 1
                && let Some(due) = worst.and_then(|w| w.due_deck.as_ref())
            {
                message.reviews_per_day = due.review_limit;
            }
            let name = if i == 0 {
                "Default".to_owned()
            } else {
                format!("Preset {}", i + 1)
            };
            preset(DeckConfigId(id), &name, message, timeline)
        })
        .collect()
}

pub fn deck(id: DeckId, human_name: &str, preset: DeckConfigId, timeline: &Timeline) -> Deck {
    let mut common = DeckCommon::default();
    if id.0 == 1 {
        common.study_collapsed = true;
        common.browser_collapsed = true;
    }
    let mut normal = NormalDeck::default();
    normal.config_id = preset.0;
    Deck {
        id,
        name: native_name(human_name),
        mtime: EpochSecs(timeline.crt),
        usn: Usn::PENDING,
        common: ProtoBlob::new(common),
        kind: ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))),
    }
}

#[derive(Debug)]
pub struct GenDeck {
    pub record: Deck,
    /// Relative share of notes placed in it.
    pub weight: u64,
}

#[derive(Debug)]
pub struct DeckPlan {
    pub decks: Vec<GenDeck>,
    weights: Vec<u64>,
}

impl DeckPlan {
    pub fn new(decks: Vec<GenDeck>) -> DeckPlan {
        let mut weights: Vec<u64> = decks.iter().map(|d| d.weight).collect();
        if weights.iter().all(|&w| w == 0) {
            weights = vec![1; decks.len()];
        }
        DeckPlan { decks, weights }
    }

    /// A deck for a note, by weight.
    pub fn pick(&self, rng: &mut Rng) -> DeckId {
        let index = rng.weighted(&self.weights).unwrap_or(0);
        self.decks[index].record.id
    }
}

/// The deck tree: the default deck, then decks by the profile's depth
/// histogram, each under a deck one level up, with presets by the
/// profile's decks-per-preset counts and note shares by its cards-per-deck
/// histogram. The due worst case has one deck besides the default deck,
/// which holds every note.
pub fn decks(
    profile: &Profile,
    worst: Option<&WorstCase>,
    presets: &[DeckConfig],
    timeline: &Timeline,
    ids: &mut Ids,
    rng: &mut Rng,
) -> DeckPlan {
    let default_preset = presets[0].id;
    let mut out = vec![GenDeck {
        record: deck(DeckId(ids.claim(1)), "Default", default_preset, timeline),
        weight: 0,
    }];
    if worst.is_some_and(|w| w.due_deck.is_some()) {
        let preset = presets.get(1).map_or(default_preset, |p| p.id);
        out.push(GenDeck {
            record: deck(DeckId(ids.claim(timeline.crt_ms())), "Due", preset, timeline),
            weight: 1,
        });
        return DeckPlan::new(out);
    }
    out[0].weight = profile.decks.cards_per_deck.sample(rng).unwrap_or(0).max(0) as u64;
    let count = worst.and_then(|w| w.decks).unwrap_or(profile.totals.decks).max(1);
    let preset_weights: Vec<u64> = (0..presets.len())
        .map(|i| profile.decks.decks_per_preset.get(i).copied().unwrap_or(1).max(1))
        .collect();
    // Human names by depth: index 0 holds the top-level decks.
    let mut by_depth: Vec<Vec<String>> = vec![Vec::new()];
    for n in 1..count {
        // A deck at depth d needs a parent at depth d - 1 (index d - 2).
        let mut depth = profile.decks.depth.sample(rng).unwrap_or(1).max(1) as usize;
        while depth > 1 && by_depth.get(depth - 2).is_none_or(Vec::is_empty) {
            depth -= 1;
        }
        let name = if depth == 1 {
            format!("Deck {n}")
        } else {
            let parents = &by_depth[depth - 2];
            let parent = &parents[rng.below(parents.len() as u64) as usize];
            format!("{parent}::Part {n}")
        };
        let level = name.matches("::").count();
        while by_depth.len() <= level {
            by_depth.push(Vec::new());
        }
        by_depth[level].push(name.clone());
        let preset = presets[rng.weighted(&preset_weights).unwrap_or(0)].id;
        let id = DeckId(ids.claim(timeline.crt_ms() + n as i64));
        out.push(GenDeck {
            record: deck(id, &name, preset, timeline),
            weight: profile.decks.cards_per_deck.sample(rng).unwrap_or(1).max(0) as u64,
        });
    }
    DeckPlan::new(out)
}
