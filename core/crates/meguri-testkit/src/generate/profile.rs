//! The shape model: aggregate histograms of a collection (R1804, R1940).
//!
//! A [`Profile`] holds only counts and numeric histograms: no text, names
//! or ids of the collection it describes. `profiles/user-shape.json` is the
//! committed profile the generator follows; [`measure`] computes one from
//! any collection file, which is how `meguri-cli profile-extract` builds it
//! from a copy of the private collection and how the tests check that a
//! generated collection follows its profile.
//!
//! Every string a profile holds is a fixed key or a value from
//! [`VOCABULARY`]; `tests/generate_validity.rs` checks an extracted profile
//! against it.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use meguri_model::card::{CardQueue, CardType};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::{DeckConfigId, DeckId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_model::proto::deck::DeckKind;
use meguri_model::revlog::RevlogKind;
use meguri_model::rng::SeededRandom;
use meguri_storage::{OpenOptions, ReadRows, Table, open_collection};
use serde::{Deserialize, Serialize};

use super::GenError;
use super::rng::Rng;

/// The profile format this code reads and writes.
pub const FORMAT: u32 = 1;

/// Every string value a profile may hold (besides its fixed keys).
pub const VOCABULARY: &[&str] = &[
    "provisional",
    "extracted",
    "measured",
    "normal",
    "cloze",
    "new",
    "learning",
    "review",
    "relearning",
    "suspended",
    "buried",
    "filtered",
    "manual",
    "rescheduled",
    "image",
    "audio",
    "video",
    "other",
];

/// The card states a profile counts: the card's queue, and its type for
/// the learning queues.
pub const CARD_STATES: [&str; 6] = ["new", "learning", "review", "relearning", "suspended", "buried"];

/// Review-log kinds by their `type` value (`model/row-records` R5).
pub const REVLOG_KINDS: [&str; 6] = ["learning", "review", "relearning", "filtered", "manual", "rescheduled"];

/// Width of a `review_days` and `creation_days` bucket, in days.
pub const DAY_BUCKET: i64 = 7;

/// Where a profile's numbers come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provenance {
    /// Written from the plan's published aggregates until an extraction
    /// replaces it.
    Provisional,
    /// `profile-extract` on a copy of the user's collection.
    Extracted,
    /// [`measure`] on any other collection (a generated one).
    Measured,
}

/// A histogram over integers: bucket `i` holds the values from `edges[i]`
/// up to but not including `edges[i + 1]`. A value below the first edge
/// counts in the first bucket, one at or past the last edge in the last.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Histogram {
    pub edges: Vec<i64>,
    pub counts: Vec<u64>,
}

impl Histogram {
    /// An empty histogram with these edges (at least two, increasing).
    pub fn with_edges(edges: Vec<i64>) -> Histogram {
        let buckets = edges.len().saturating_sub(1);
        Histogram {
            edges,
            counts: vec![0; buckets],
        }
    }

    /// Edges 0, 1, 2, 4, ... 2^`max_exp`, then 2^`max_exp` + 1 closing the
    /// last bucket.
    pub fn log2(max_exp: u32) -> Histogram {
        let mut edges = vec![0];
        edges.extend((0..=max_exp).map(|e| 1i64 << e));
        edges.push((1i64 << max_exp) + 1);
        Histogram::with_edges(edges)
    }

    /// Edges `start`, `start + step`, ... up to the first edge past `end`.
    pub fn linear(start: i64, end: i64, step: i64) -> Histogram {
        let mut edges = Vec::new();
        let mut edge = start;
        while edge <= end {
            edges.push(edge);
            edge += step;
        }
        edges.push(edge);
        Histogram::with_edges(edges)
    }

    /// The bucket a value counts in.
    pub fn bucket(&self, value: i64) -> usize {
        let last = self.counts.len().saturating_sub(1);
        match self.edges.binary_search(&value) {
            Ok(i) => i.min(last),
            Err(0) => 0,
            Err(i) => (i - 1).min(last),
        }
    }

    pub fn add(&mut self, value: i64) {
        self.add_n(value, 1);
    }

    pub fn add_n(&mut self, value: i64, n: u64) {
        if self.counts.is_empty() {
            return;
        }
        let bucket = self.bucket(value);
        self.counts[bucket] += n;
    }

    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// Each bucket's share of the total; all zero for an empty histogram.
    pub fn shares(&self) -> Vec<f64> {
        let total = self.total();
        self.counts
            .iter()
            .map(|&c| if total == 0 { 0.0 } else { c as f64 / total as f64 })
            .collect()
    }

    /// Total variation distance between the two distributions: half the
    /// sum of the share differences, from 0 (equal) to 1 (disjoint). `None`
    /// when the edges differ.
    pub fn distance(&self, other: &Histogram) -> Option<f64> {
        if self.edges != other.edges {
            return None;
        }
        Some(
            self.shares()
                .iter()
                .zip(other.shares())
                .map(|(a, b)| (a - b).abs())
                .sum::<f64>()
                / 2.0,
        )
    }

    /// Draws a value: a bucket by its count, then an integer uniformly in
    /// it. `None` when every count is zero.
    pub fn sample(&self, rng: &mut Rng) -> Option<i64> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let mut pick = rng.below(total);
        for (i, &count) in self.counts.iter().enumerate() {
            if pick < count {
                let (lo, hi) = (self.edges[i], self.edges[i + 1]);
                return Some(lo + rng.below((hi - lo).max(1) as u64) as i64);
            }
            pick -= count;
        }
        None
    }

    /// Draws a value no smaller than `min` and below `max`, by rejection,
    /// falling back to a uniform draw in that range.
    pub fn sample_within(&self, rng: &mut Rng, min: i64, max: i64) -> i64 {
        if max <= min {
            return min;
        }
        for _ in 0..64 {
            if let Some(value) = self.sample(rng)
                && value >= min
                && value < max
            {
                return value;
            }
        }
        min + rng.below((max - min) as u64) as i64
    }
}

/// Counts by category name.
pub type Mix = BTreeMap<String, u64>;

/// Draws a category of a mix by its count.
pub fn sample_mix<'m>(mix: &'m Mix, rng: &mut Rng) -> Option<&'m str> {
    let total: u64 = mix.values().sum();
    if total == 0 {
        return None;
    }
    let mut pick = rng.below(total);
    for (name, &count) in mix {
        if pick < count {
            return Some(name);
        }
        pick -= count;
    }
    None
}

/// Total variation distance between two mixes over the union of their
/// categories.
pub fn mix_distance(a: &Mix, b: &Mix) -> f64 {
    let (ta, tb) = (a.values().sum::<u64>() as f64, b.values().sum::<u64>() as f64);
    let share = |m: &Mix, t: f64, k: &str| {
        if t == 0.0 {
            0.0
        } else {
            *m.get(k).unwrap_or(&0) as f64 / t
        }
    };
    let keys: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    keys.iter()
        .map(|k| (share(a, ta, k) - share(b, tb, k)).abs())
        .sum::<f64>()
        / 2.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub format: u32,
    pub provenance: Provenance,
    /// The UTC offset the hours of day were read in (and the generator
    /// writes them in), seconds east of UTC.
    pub utc_offset_secs: i32,
    /// The collection's day rollover hour (config `rollover`).
    pub rollover_hour: u8,
    pub totals: Totals,
    pub notetypes: Vec<NotetypeShape>,
    pub decks: DeckShape,
    pub tags: TagShape,
    pub media: MediaShape,
    pub cards: CardShape,
    pub revlog: RevlogShape,
    pub notes: NoteShape,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Totals {
    pub notes: u64,
    pub cards: u64,
    pub revlog: u64,
    pub notetypes: u64,
    /// Normal decks, the default deck included.
    pub decks: u64,
    pub filtered_decks: u64,
    pub presets: u64,
    /// Distinct tags on notes.
    pub tags: u64,
    pub media_files: u64,
    pub media_bytes: u64,
    /// Days from the day of `col.crt` to the day of the last note or review.
    pub span_days: u64,
}

/// One note type, without its name or its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotetypeShape {
    /// `normal` or `cloze`.
    pub kind: String,
    /// The stored `original_stock_kind` (P6.1).
    pub stock_kind: i32,
    pub fields: u64,
    pub templates: u64,
    pub notes: u64,
    pub cards: u64,
    pub css_bytes: u64,
    /// Front and back template bytes, per template.
    pub template_bytes: Vec<[u64; 2]>,
    /// Bytes inside `<script>` elements, all templates together.
    pub script_bytes: u64,
    pub cards_per_note: Histogram,
    /// Field value bytes, per field.
    pub field_bytes: Vec<Histogram>,
    /// Local media references per field: images (media tags) and sounds.
    pub media_refs: Vec<[u64; 2]>,
    pub refs_per_note: Histogram,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckShape {
    /// Normal decks by depth (1 = top level).
    pub depth: Histogram,
    /// Cards per normal deck (home deck), every normal deck counted.
    pub cards_per_deck: Histogram,
    /// Decks per preset, largest first, one entry per preset.
    pub decks_per_preset: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagShape {
    /// Distinct tags by their number of `::` levels.
    pub depth: Histogram,
    /// Tags per note.
    pub per_note: Histogram,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaShape {
    /// File sizes in the media folder.
    pub file_bytes: Histogram,
    /// Files by kind of extension.
    pub kinds: Mix,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CardShape {
    /// Cards per state (`CARD_STATES`).
    pub states: Mix,
    /// `ivl` of review-type cards, days.
    pub interval_days: Histogram,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevlogShape {
    /// Review-log rows per card, over cards with at least one.
    pub per_card: Histogram,
    /// Rows per kind (`REVLOG_KINDS`).
    pub kinds: Mix,
    /// Rows per button (index 0 to 4, `ease`), per kind.
    pub ratings: BTreeMap<String, [u64; 5]>,
    /// `time`, milliseconds.
    pub answer_ms: Histogram,
    /// Reviews by day number (days since the day of `col.crt`), in
    /// `DAY_BUCKET`-day buckets: the reviews-per-day curve.
    pub review_days: Histogram,
    /// Reviews by local hour of day, 24 buckets.
    pub hour_of_day: Histogram,
    /// Seconds from a card's note creation (note id) to its first review.
    pub first_review_gap_secs: Histogram,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteShape {
    /// Creation day numbers of notes none of whose cards has a review, in
    /// `DAY_BUCKET`-day buckets.
    pub unreviewed_creation_days: Histogram,
}

impl Profile {
    pub fn from_json(text: &str) -> Result<Profile, GenError> {
        let profile: Profile = serde_json::from_str(text).map_err(|e| GenError::Profile(e.to_string()))?;
        profile.check()?;
        Ok(profile)
    }

    pub fn load(path: &Path) -> Result<Profile, GenError> {
        let text = std::fs::read_to_string(path).map_err(|e| GenError::io(path, e))?;
        Profile::from_json(&text)
    }

    /// The committed form: the goldens' layout (sorted keys, two-space
    /// indents, arrays of numbers on one line) with a final newline.
    pub fn to_json(&self) -> String {
        crate::observe::golden_text(&serde_json::to_value(self).expect("a profile serializes"))
    }

    /// The SHA-256 of the committed form, which names generated fixtures.
    pub fn sha256(&self) -> String {
        use sha2::{Digest, Sha256};
        super::hex(&Sha256::digest(self.to_json().as_bytes()))
    }

    /// Structural checks the generator relies on.
    pub fn check(&self) -> Result<(), GenError> {
        let bad = |what: String| Err(GenError::Profile(what));
        if self.format != FORMAT {
            return bad(format!("format {} (this generator reads {FORMAT})", self.format));
        }
        if self.rollover_hour > 23 || UtcOffset::from_seconds(self.utc_offset_secs).is_none() {
            return bad("rollover hour or UTC offset out of range".to_owned());
        }
        let mut histograms: Vec<(&str, &Histogram)> = vec![
            ("decks.depth", &self.decks.depth),
            ("decks.cards_per_deck", &self.decks.cards_per_deck),
            ("tags.depth", &self.tags.depth),
            ("tags.per_note", &self.tags.per_note),
            ("media.file_bytes", &self.media.file_bytes),
            ("cards.interval_days", &self.cards.interval_days),
            ("revlog.per_card", &self.revlog.per_card),
            ("revlog.answer_ms", &self.revlog.answer_ms),
            ("revlog.review_days", &self.revlog.review_days),
            ("revlog.hour_of_day", &self.revlog.hour_of_day),
            ("revlog.first_review_gap_secs", &self.revlog.first_review_gap_secs),
            ("notes.unreviewed_creation_days", &self.notes.unreviewed_creation_days),
        ];
        for nt in &self.notetypes {
            if nt.kind != "normal" && nt.kind != "cloze" {
                return bad(format!("note type kind {:?}", nt.kind));
            }
            if nt.fields == 0 || nt.templates == 0 {
                return bad("a note type without fields or templates".to_owned());
            }
            if nt.field_bytes.len() as u64 != nt.fields || nt.media_refs.len() as u64 != nt.fields {
                return bad("field_bytes and media_refs need one entry per field".to_owned());
            }
            if nt.template_bytes.len() as u64 != nt.templates {
                return bad("template_bytes needs one entry per template".to_owned());
            }
            histograms.push(("cards_per_note", &nt.cards_per_note));
            histograms.push(("refs_per_note", &nt.refs_per_note));
            histograms.extend(nt.field_bytes.iter().map(|h| ("field_bytes", h)));
        }
        for (name, h) in histograms {
            if h.edges.len() < 2 || h.counts.len() + 1 != h.edges.len() || h.edges.windows(2).any(|w| w[0] >= w[1]) {
                return bad(format!(
                    "{name}: edges must increase and number one more than the counts"
                ));
            }
        }
        if self.revlog.hour_of_day.counts.len() != 24 {
            return bad("revlog.hour_of_day needs 24 buckets".to_owned());
        }
        if self.notetypes.is_empty() {
            return bad("no note types".to_owned());
        }
        Ok(())
    }
}

/// The local UTC offset at an instant, seconds east of UTC.
pub type OffsetAt<'a> = &'a dyn Fn(i64) -> i32;

/// Builds the profile of a collection file. `media_dir` is its media
/// folder, read for file sizes only; `offset_at` gives the local offset at
/// an instant in epoch milliseconds (the time zone the hours of day are
/// read in). The file is opened through the open gate, which may upgrade
/// it: pass a copy (`extract` does).
pub fn measure(path: &Path, media_dir: Option<&Path>, offset_at: OffsetAt<'_>) -> Result<Profile, GenError> {
    let clock = FixedClock::new(0, UtcOffset::UTC);
    let random = SeededRandom::new(0);
    let collection = open_collection(path, &OpenOptions::new(&clock, &random)).map_err(GenError::open)?;
    let rows = collection.writer();
    let col = rows.col().map_err(GenError::storage)?;
    let crt = col.created.0;
    let rollover_hour = rows
        .config_value("rollover")
        .map_err(GenError::storage)?
        .and_then(|v| v.as_i64())
        .filter(|h| (0..24).contains(h))
        .unwrap_or(4) as u8;

    // Note types.
    let mut notetypes: Vec<Notetype> = Vec::new();
    for id in rows
        .ids_after(Table::Notetypes, None, usize::MAX)
        .map_err(GenError::storage)?
    {
        if let Some(nt) = rows.notetype(NotetypeId(id)).map_err(GenError::storage)? {
            notetypes.push(nt);
        }
    }
    let nt_index: HashMap<i64, usize> = notetypes.iter().enumerate().map(|(i, nt)| (nt.id.0, i)).collect();
    let mut shapes: Vec<NotetypeShape> = notetypes.iter().map(notetype_shape).collect();

    // Decks and presets.
    let mut presets: Vec<i64> = rows
        .ids_after(Table::DeckConfig, None, usize::MAX)
        .map_err(GenError::storage)?;
    presets.sort_unstable();
    let mut deck_depth = Histogram::linear(1, 8, 1);
    let mut deck_preset: HashMap<i64, i64> = HashMap::new();
    let mut filtered_decks = 0;
    for id in rows
        .ids_after(Table::Decks, None, usize::MAX)
        .map_err(GenError::storage)?
    {
        let Some(deck) = rows.deck(DeckId(id)).map_err(GenError::storage)? else {
            continue;
        };
        match &deck.kind.kind {
            DeckKind::Normal(normal) => {
                deck_depth.add(deck.name.split(meguri_model::note::FIELD_SEPARATOR).count() as i64);
                deck_preset.insert(id, normal.config_id);
            }
            DeckKind::Filtered(_) => filtered_decks += 1,
        }
    }
    let mut per_preset: HashMap<i64, u64> = presets.iter().map(|&p| (p, 0)).collect();
    for config in deck_preset.values() {
        if rows
            .deck_config(DeckConfigId(*config))
            .map_err(GenError::storage)?
            .is_some()
        {
            *per_preset.entry(*config).or_default() += 1;
        }
    }
    let mut decks_per_preset: Vec<u64> = per_preset.values().copied().collect();
    decks_per_preset.sort_unstable_by(|a, b| b.cmp(a));

    // Notes: creation, fields, media references, tags.
    let mut note_info: HashMap<i64, (usize, i64)> = HashMap::new();
    let mut distinct_tags: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut tag_depth = Histogram::linear(1, 8, 1);
    let mut tags_per_note = Histogram::linear(0, 16, 1);
    let mut note_count = 0u64;
    let mut after = None;
    loop {
        let page = rows.notes_after(after, 5000).map_err(GenError::storage)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for note in &page {
            note_count += 1;
            let Some(&index) = nt_index.get(&note.notetype_id.0) else {
                continue;
            };
            note_info.insert(note.id.0, (index, note.id.0));
            let shape = &mut shapes[index];
            shape.notes += 1;
            let mut refs = 0;
            for (ord, value) in note.field_values().enumerate() {
                if let Some(h) = shape.field_bytes.get_mut(ord) {
                    h.add(value.len() as i64);
                }
                let (images, sounds) = count_refs(value);
                if let Some(slot) = shape.media_refs.get_mut(ord) {
                    slot[0] += images;
                    slot[1] += sounds;
                }
                refs += images + sounds;
            }
            shape.refs_per_note.add(refs as i64);
            let tags: Vec<&str> = note.tags.split_whitespace().collect();
            tags_per_note.add(tags.len() as i64);
            for tag in tags {
                let folded = tag.to_lowercase();
                if distinct_tags.insert(folded) {
                    tag_depth.add(tag.split("::").count() as i64);
                }
            }
        }
    }

    // Review log: per card counts and first review, kinds, buttons, times.
    let mut per_card: HashMap<i64, (u64, i64)> = HashMap::new();
    let mut kinds = Mix::new();
    let mut ratings: BTreeMap<String, [u64; 5]> = BTreeMap::new();
    let mut answer_ms = Histogram::log2(20);
    let mut review_ms: Vec<i64> = Vec::new();
    let mut revlog_count = 0u64;
    let mut last_activity_ms = i64::MIN;
    let mut after = None;
    loop {
        let page = rows.revlog_after(after, 20_000).map_err(GenError::storage)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for entry in &page {
            revlog_count += 1;
            let slot = per_card.entry(entry.card_id.0).or_insert((0, entry.id.0));
            slot.0 += 1;
            slot.1 = slot.1.min(entry.id.0);
            let kind = kind_name(entry.kind);
            *kinds.entry(kind.to_owned()).or_default() += 1;
            ratings.entry(kind.to_owned()).or_insert([0; 5])[usize::from(entry.button.min(4))] += 1;
            answer_ms.add(i64::from(entry.answer_millis));
            review_ms.push(entry.id.0);
            last_activity_ms = last_activity_ms.max(entry.id.0);
        }
    }

    // Cards: states, intervals, per note type and per deck counts, gaps.
    let mut states: Mix = CARD_STATES.iter().map(|s| ((*s).to_owned(), 0)).collect();
    let mut interval_days = Histogram::log2(15);
    let mut deck_cards: HashMap<i64, u64> = deck_preset.keys().map(|&d| (d, 0)).collect();
    let mut cards_of_note: HashMap<i64, (u64, bool)> = HashMap::new();
    let mut gaps = Histogram::log2(32);
    let mut per_card_hist = Histogram::linear(1, 64, 1);
    let mut card_count = 0u64;
    let mut after = None;
    loop {
        let page = rows.cards_after(after, 5000).map_err(GenError::storage)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for card in &page {
            card_count += 1;
            *states
                .entry(state_name(card.card_type, card.queue).to_owned())
                .or_default() += 1;
            if card.card_type == CardType::Review {
                interval_days.add(i64::from(card.interval));
            }
            let home = if card.original_deck_id.0 != 0 {
                card.original_deck_id.0
            } else {
                card.deck_id.0
            };
            if let Some(count) = deck_cards.get_mut(&home) {
                *count += 1;
            }
            let reviewed = per_card.get(&card.id.0);
            let entry = cards_of_note.entry(card.note_id.0).or_insert((0, false));
            entry.0 += 1;
            entry.1 |= reviewed.is_some();
            if let Some(&(count, first_ms)) = reviewed {
                per_card_hist.add(count as i64);
                gaps.add(((first_ms - card.note_id.0).max(0)) / 1000);
            }
        }
    }
    for (note_id, (count, _)) in &cards_of_note {
        if let Some(&(index, _)) = note_info.get(note_id) {
            shapes[index].cards += count;
            shapes[index].cards_per_note.add(*count as i64);
        }
    }
    let mut cards_per_deck = Histogram::log2(20);
    for count in deck_cards.values() {
        cards_per_deck.add(*count as i64);
    }

    // Time: the span, reviews by day and hour, unreviewed notes by day.
    for &(_, id) in note_info.values() {
        last_activity_ms = last_activity_ms.max(id);
    }
    let day_of = |ms: i64| (ms.div_euclid(1000) - crt).div_euclid(86_400);
    let span_days = if last_activity_ms == i64::MIN {
        0
    } else {
        day_of(last_activity_ms).max(0)
    };
    let mut review_days = Histogram::linear(0, span_days, DAY_BUCKET);
    let mut hour_of_day = Histogram::linear(0, 23, 1);
    for &ms in &review_ms {
        review_days.add(day_of(ms));
        let local = ms.div_euclid(1000) + i64::from(offset_at(ms));
        hour_of_day.add(local.rem_euclid(86_400) / 3600);
    }
    let mut unreviewed = Histogram::linear(0, span_days, DAY_BUCKET);
    for (note_id, (_, reviewed)) in &cards_of_note {
        if !reviewed {
            unreviewed.add(day_of(*note_id));
        }
    }

    let (media, media_files, media_bytes) = media_shape(media_dir)?;
    Ok(Profile {
        format: FORMAT,
        provenance: Provenance::Measured,
        utc_offset_secs: offset_at(last_activity_ms.max(0)),
        rollover_hour,
        totals: Totals {
            notes: note_count,
            cards: card_count,
            revlog: revlog_count,
            notetypes: shapes.len() as u64,
            decks: deck_preset.len() as u64,
            filtered_decks,
            presets: presets.len() as u64,
            tags: distinct_tags.len() as u64,
            media_files,
            media_bytes,
            span_days: span_days as u64,
        },
        notetypes: shapes,
        decks: DeckShape {
            depth: deck_depth,
            cards_per_deck,
            decks_per_preset,
        },
        tags: TagShape {
            depth: tag_depth,
            per_note: tags_per_note,
        },
        media,
        cards: CardShape { states, interval_days },
        revlog: RevlogShape {
            per_card: per_card_hist,
            kinds,
            ratings,
            answer_ms,
            review_days,
            hour_of_day,
            first_review_gap_secs: gaps,
        },
        notes: NoteShape {
            unreviewed_creation_days: unreviewed,
        },
    })
}

/// [`measure`] on a temporary copy of `source` (and its `-wal`, when there
/// is one): the source file is only read.
pub fn extract(source: &Path, media_dir: Option<&Path>, offset_at: OffsetAt<'_>) -> Result<Profile, GenError> {
    let dir = tempfile::tempdir().map_err(|e| GenError::io(source, e))?;
    let copy = dir.path().join("collection.anki2");
    std::fs::copy(source, &copy).map_err(|e| GenError::io(source, e))?;
    let wal = super::sibling(source, "-wal");
    if wal.exists() {
        std::fs::copy(&wal, super::sibling(&copy, "-wal")).map_err(|e| GenError::io(&wal, e))?;
    }
    let mut profile = measure(&copy, media_dir, offset_at)?;
    profile.provenance = Provenance::Extracted;
    Ok(profile)
}

fn notetype_shape(nt: &Notetype) -> NotetypeShape {
    let fields = nt.fields.len();
    let mut script_bytes = 0;
    let template_bytes = nt
        .templates
        .iter()
        .map(|t| {
            script_bytes += script_len(&t.config.q_format) + script_len(&t.config.a_format);
            [t.config.q_format.len() as u64, t.config.a_format.len() as u64]
        })
        .collect();
    NotetypeShape {
        kind: if nt.config.kind == 1 { "cloze" } else { "normal" }.to_owned(),
        stock_kind: nt.config.original_stock_kind,
        fields: fields as u64,
        templates: nt.templates.len() as u64,
        notes: 0,
        cards: 0,
        css_bytes: nt.config.css.len() as u64,
        template_bytes,
        script_bytes,
        cards_per_note: Histogram::linear(1, 64, 1),
        field_bytes: (0..fields).map(|_| Histogram::log2(24)).collect(),
        media_refs: vec![[0, 0]; fields],
        refs_per_note: Histogram::linear(0, 64, 1),
    }
}

/// Bytes between `<script...>` and `</script>`, case-insensitively.
fn script_len(text: &str) -> u64 {
    let lower = text.to_ascii_lowercase();
    let mut total = 0;
    let mut rest = lower.as_str();
    while let Some(open) = rest.find("<script") {
        let after_open = &rest[open..];
        let Some(close_tag) = after_open.find('>') else { break };
        let body = &after_open[close_tag + 1..];
        match body.find("</script") {
            Some(end) => {
                total += end as u64;
                rest = &body[end..];
            }
            None => {
                total += body.len() as u64;
                break;
            }
        }
    }
    total
}

/// Local media references in a field: media tags (images and other
/// elements) and sound tags. Remote and `data:` names do not count.
pub fn count_refs(field: &str) -> (u64, u64) {
    let local = |name: &str| !meguri_text::media_refs::is_remote(name) && !name.trim_start().starts_with("data:");
    let images = meguri_text::media_refs::media_tags(field)
        .filter(|t| local(t.raw_name))
        .count() as u64;
    let sounds = meguri_text::media_refs::sound_tags(field)
        .filter(|t| local(t.raw_name))
        .count() as u64;
    (images, sounds)
}

fn kind_name(kind: RevlogKind) -> &'static str {
    REVLOG_KINDS[kind.stored() as usize]
}

fn state_name(card_type: CardType, queue: CardQueue) -> &'static str {
    match queue {
        CardQueue::New => "new",
        CardQueue::Review => "review",
        CardQueue::Suspended => "suspended",
        CardQueue::BuriedByScheduler | CardQueue::BuriedByUser => "buried",
        CardQueue::Learning | CardQueue::InterdayLearning | CardQueue::Preview => {
            if card_type == CardType::Relearning {
                "relearning"
            } else {
                "learning"
            }
        }
    }
}

/// The kind of a media file by its extension.
pub fn media_kind(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tif" | "tiff" | "avif" | "heic" => "image",
        "mp3" | "ogg" | "oga" | "opus" | "wav" | "m4a" | "aac" | "flac" | "spx" => "audio",
        "mp4" | "webm" | "mov" | "mkv" | "ogv" | "m4v" | "avi" | "mpg" | "mpeg" => "video",
        _ => "other",
    }
}

fn media_shape(dir: Option<&Path>) -> Result<(MediaShape, u64, u64), GenError> {
    let mut file_bytes = Histogram::log2(30);
    let mut kinds = Mix::new();
    let (mut files, mut bytes) = (0u64, 0u64);
    if let Some(dir) = dir
        && dir.is_dir()
    {
        for entry in std::fs::read_dir(dir).map_err(|e| GenError::io(dir, e))? {
            let entry = entry.map_err(|e| GenError::io(dir, e))?;
            let meta = entry.metadata().map_err(|e| GenError::io(&entry.path(), e))?;
            if !meta.is_file() {
                continue;
            }
            files += 1;
            bytes += meta.len();
            file_bytes.add(meta.len() as i64);
            let name = entry.file_name();
            *kinds.entry(media_kind(&name.to_string_lossy()).to_owned()).or_default() += 1;
        }
    }
    Ok((MediaShape { file_bytes, kinds }, files, bytes))
}
