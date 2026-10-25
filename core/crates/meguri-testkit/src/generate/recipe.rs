//! Named UI fixtures (R1963): small collections described by a recipe in
//! `fixtures/recipes/<name>.yaml`, at a pinned instant, with decks whose
//! card counts by state are given exactly. The app's fixture launch option
//! loads the output directory (`collection.anki2` and `collection.media/`).
//!
//! A recipe is YAML in the scenarios' subset (read with
//! `scenario::parse_yaml`):
//!
//! ```yaml
//! recipe: typical
//! description: what it is for
//! now: "2030-01-15T10:00:00+01:00"   # the pinned instant and UTC offset
//! rollover_hour: 4                    # optional, default 4
//! learn_ahead_secs: 1200              # optional, default 1200
//! presets:                            # optional; Default (id 1) always exists
//!   - {name: Light, new_per_day: 10, reviews_per_day: 50}
//! decks:                              # parents of `A::B` are made when missing
//!   - name: "Languages::日本語"
//!     preset: Light                   # optional, default Default
//!     notetype: basic                 # basic | reversed | cloze | broken
//!     new: 20                         # cards per state, all optional
//!     learning: 3                     # due now
//!     due: 45                         # review cards due today or earlier
//!     later: 100                      # review cards due after today
//!     suspended: 2
//!     buried: 1
//!     tags: [jp, "jp::vocab"]
//!     fields: ["{n} front", "back {n}"]   # {n}: the note's number in the deck
//!     media: {images: 1, sounds: 1, missing: 1}  # references per note
//! ```
//!
//! Cards of a `reversed` note share their state, so its counts are even.
//! `broken` is a note type whose back template names a field it lacks.
//! `missing` references name files that are not written.

use std::path::{Path, PathBuf};

use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId};
use meguri_model::note::{FIELD_SEPARATOR, Note};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_text::checksum::{note_checksum, sort_text_of};
use serde::Deserialize;
use serde_json::Value;

use super::cards::{CardInput, Due, Target, card_rows};
use super::decks::{deck, default_preset_message, preset};
use super::media::{MediaFile, MediaKind};
use super::notes::{unicase_sorted, words};
use super::notetypes::{Kind, base_templates, field_names, notetype};
use super::profile::{Histogram, NotetypeShape, RevlogShape};
use super::revlog::Fsrs;
use super::rng::{Rng, Stream};
use super::{
    AllIds, GENERATOR_VERSION, GenError, LATEST_NOW_MS, MAX_NOTES, Manifest, MediaMode, NoteRows, Output, Timeline,
    base_config, closing_config,
};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub recipe: String,
    pub description: String,
    pub now: String,
    #[serde(default)]
    pub rollover_hour: Option<u8>,
    #[serde(default = "default_learn_ahead_secs")]
    pub learn_ahead_secs: u32,
    #[serde(default)]
    pub presets: Vec<PresetSpec>,
    #[serde(default)]
    pub decks: Vec<DeckSpec>,
}

fn default_learn_ahead_secs() -> u32 {
    1200
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetSpec {
    pub name: String,
    #[serde(default)]
    pub new_per_day: Option<u32>,
    #[serde(default)]
    pub reviews_per_day: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeckSpec {
    pub name: String,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub notetype: Option<String>,
    #[serde(default)]
    pub new: u32,
    #[serde(default)]
    pub learning: u32,
    #[serde(default)]
    pub due: u32,
    #[serde(default)]
    pub later: u32,
    #[serde(default)]
    pub suspended: u32,
    #[serde(default)]
    pub buried: u32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub media: Option<MediaSpec>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaSpec {
    #[serde(default)]
    pub images: u32,
    #[serde(default)]
    pub sounds: u32,
    #[serde(default)]
    pub missing: u32,
}

/// The recipe note types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecipeType {
    Basic,
    Reversed,
    Cloze,
    Broken,
}

impl RecipeType {
    const ALL: [RecipeType; 4] = [
        RecipeType::Basic,
        RecipeType::Reversed,
        RecipeType::Cloze,
        RecipeType::Broken,
    ];

    fn parse(name: Option<&str>) -> Result<RecipeType, GenError> {
        match name.unwrap_or("basic") {
            "basic" => Ok(RecipeType::Basic),
            "reversed" => Ok(RecipeType::Reversed),
            "cloze" => Ok(RecipeType::Cloze),
            "broken" => Ok(RecipeType::Broken),
            other => Err(GenError::Recipe(format!("unknown note type {other:?}"))),
        }
    }

    fn name(self) -> &'static str {
        match self {
            RecipeType::Basic => "Meguri Basic",
            RecipeType::Reversed => "Meguri Basic and Reverse",
            RecipeType::Cloze => "Meguri Cloze",
            RecipeType::Broken => "Meguri Broken Template",
        }
    }

    fn cards_per_note(self) -> u32 {
        if self == RecipeType::Reversed { 2 } else { 1 }
    }

    fn shape(self) -> NotetypeShape {
        let (kind, templates) = match self {
            RecipeType::Cloze => ("cloze", 1),
            RecipeType::Reversed => ("normal", 2),
            _ => ("normal", 1),
        };
        NotetypeShape {
            kind: kind.to_owned(),
            stock_kind: 0,
            fields: 2,
            templates,
            notes: 0,
            cards: 0,
            css_bytes: 0,
            template_bytes: vec![[0, 0]; templates as usize],
            script_bytes: 0,
            cards_per_note: Histogram::linear(1, 1, 1),
            field_bytes: vec![Histogram::linear(0, 0, 1); 2],
            media_refs: vec![[0, 0]; 2],
            refs_per_note: Histogram::linear(0, 0, 1),
        }
    }
}

/// `core/crates/meguri-testkit/fixtures/recipes`.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("recipes")
}

impl Recipe {
    pub fn parse(text: &str) -> Result<Recipe, GenError> {
        let value: Value = crate::scenario::parse_yaml(text).map_err(|e| GenError::Recipe(e.to_string()))?;
        serde_json::from_value(value).map_err(|e| GenError::Recipe(e.to_string()))
    }

    pub fn load(path: &Path) -> Result<Recipe, GenError> {
        let text = std::fs::read_to_string(path).map_err(|e| GenError::io(path, e))?;
        Recipe::parse(&text)
    }

    /// The committed recipe `name`.
    pub fn named(name: &str) -> Result<Recipe, GenError> {
        Recipe::load(&dir().join(format!("{name}.yaml")))
    }

    pub fn names() -> Result<Vec<String>, GenError> {
        let dir = dir();
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| GenError::io(&dir, e))? {
            let path = entry.map_err(|e| GenError::io(&dir, e))?.path();
            if path.extension().is_some_and(|e| e == "yaml")
                && let Some(stem) = path.file_stem()
            {
                names.push(stem.to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    /// The pinned instant in epoch milliseconds and its UTC offset.
    pub fn instant(&self) -> Result<(i64, i32), GenError> {
        let ts: jiff::Timestamp = self
            .now
            .parse()
            .map_err(|e| GenError::Recipe(format!("now {:?}: {e}", self.now)))?;
        // The offset as written: the text after the seconds.
        let offset =
            parse_offset(&self.now).ok_or_else(|| GenError::Recipe(format!("now {:?} needs an offset", self.now)))?;
        Ok((ts.as_millisecond(), offset))
    }
}

/// `Z`, `+HH:MM` or `-HH:MM` at the end of an RFC 3339 instant, in seconds.
fn parse_offset(text: &str) -> Option<i32> {
    if text.ends_with('Z') || text.ends_with('z') {
        return Some(0);
    }
    let tail = &text[text.len().checked_sub(6)?..];
    let sign = match tail.as_bytes()[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hours: i32 = tail[1..3].parse().ok()?;
    let minutes: i32 = tail[4..6].parse().ok()?;
    (tail.as_bytes()[3] == b':').then_some(sign * (hours * 3600 + minutes * 60))
}

/// Buttons and answer times for recipe histories.
fn revlog_shape() -> RevlogShape {
    let mut ratings = std::collections::BTreeMap::new();
    ratings.insert("learning".to_owned(), [0, 10, 5, 80, 5]);
    ratings.insert("review".to_owned(), [0, 8, 4, 83, 5]);
    ratings.insert("relearning".to_owned(), [0, 10, 5, 83, 2]);
    let mut answer_ms = Histogram::with_edges(vec![1000, 3000, 6000, 12_000, 30_000]);
    answer_ms.counts = vec![20, 40, 30, 10];
    RevlogShape {
        per_card: Histogram::linear(1, 1, 1),
        kinds: Default::default(),
        ratings,
        answer_ms,
        review_days: Histogram::linear(0, 0, 1),
        hour_of_day: Histogram::linear(0, 23, 1),
        first_review_gap_secs: Histogram::linear(0, 0, 1),
    }
}

/// Days of history before the pinned day (F9.2).
pub const SPAN_DAYS: u64 = 120;

/// Generates the recipe into `dir`.
pub fn generate_recipe(recipe: &Recipe, seed: u64, dir: &Path) -> Result<Manifest, GenError> {
    let (now_ms, offset) = recipe.instant()?;
    if now_ms >= LATEST_NOW_MS {
        return Err(GenError::Recipe(format!(
            "now {} is too late: every due must fit i32 (F4.3)",
            recipe.now
        )));
    }
    let rollover = recipe.rollover_hour.unwrap_or(4);
    if rollover > 23 {
        return Err(GenError::Recipe(format!("rollover_hour {rollover} is not 0 to 23")));
    }
    let mut notes = 0u64;
    for spec in &recipe.decks {
        let per_note = RecipeType::parse(spec.notetype.as_deref())?.cards_per_note();
        let cards = [
            spec.new,
            spec.learning,
            spec.due,
            spec.later,
            spec.suspended,
            spec.buried,
        ];
        notes += cards.iter().map(|&n| u64::from(n / per_note)).sum::<u64>();
    }
    if notes > MAX_NOTES {
        return Err(GenError::Recipe(format!(
            "{notes} notes: positions must stay below 1,000,000, so at most {MAX_NOTES} notes (F6.6)"
        )));
    }
    let timeline = Timeline::new(now_ms, offset, rollover, SPAN_DAYS);
    // Recipe decks and tags are expanded: UI fixtures show their whole tree.
    let mut config = base_config(&timeline);
    for entry in &mut config {
        if entry.key == "collapseTime" {
            entry.value = recipe.learn_ahead_secs.to_string().into_bytes();
        }
    }
    let mut out = Output::create(dir, &timeline, &config, MediaMode::Files, false)?;
    let mut ids = AllIds::default();
    let mut structure = Rng::stream(seed, Stream::Recipe, 0);

    // Presets.
    let mut presets = vec![preset(
        DeckConfigId(ids.presets.claim(1)),
        "Default",
        default_preset_message(),
        &timeline,
    )];
    for (i, spec) in recipe.presets.iter().enumerate() {
        let mut message = default_preset_message();
        if let Some(n) = spec.new_per_day {
            message.new_per_day = n;
        }
        if let Some(n) = spec.reviews_per_day {
            message.reviews_per_day = n;
        }
        let id = DeckConfigId(ids.presets.claim(timeline.crt_ms() + i as i64 + 1));
        presets.push(preset(id, &spec.name, message, &timeline));
    }
    let preset_id = |name: Option<&str>| -> Result<DeckConfigId, GenError> {
        match name {
            None => Ok(DeckConfigId(1)),
            Some(name) => presets
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.id)
                .ok_or_else(|| GenError::Recipe(format!("unknown preset {name:?}"))),
        }
    };

    // Decks, parents first.
    let mut deck_ids: Vec<(String, DeckId)> = vec![("Default".to_owned(), DeckId(ids.decks.claim(1)))];
    let mut deck_rows = vec![deck(DeckId(1), "Default", DeckConfigId(1), &timeline, false)];
    let mut spec_decks = Vec::new();
    for spec in &recipe.decks {
        let parts: Vec<&str> = spec.name.split("::").collect();
        for depth in 1..=parts.len() {
            let name = parts[..depth].join("::");
            // Deck names are unique under unicase (F6.4).
            if deck_ids
                .iter()
                .any(|(n, _)| meguri_text::fold::compare_folded(n, &name) == std::cmp::Ordering::Equal)
            {
                if depth == parts.len() {
                    return Err(GenError::Recipe(format!(
                        "deck {name:?} is listed twice or after its child"
                    )));
                }
                continue;
            }
            let own = depth == parts.len();
            let preset = if own {
                preset_id(spec.preset.as_deref())?
            } else {
                DeckConfigId(1)
            };
            let id = DeckId(ids.decks.claim(timeline.crt_ms() + deck_ids.len() as i64));
            deck_rows.push(deck(id, &name, preset, &timeline, false));
            deck_ids.push((name, id));
        }
        spec_decks.push(deck_ids.last().expect("just added").1);
    }

    // Note types, in a fixed order, for those the recipe uses; ids by their
    // place in that order (F4.7).
    let mut types = Vec::new();
    for kind in RecipeType::ALL.iter() {
        if !recipe
            .decks
            .iter()
            .any(|d| RecipeType::parse(d.notetype.as_deref()).ok() == Some(*kind))
        {
            continue;
        }
        let id = NotetypeId(ids.notetypes.claim(timeline.crt_ms() + types.len() as i64));
        let mut record = notetype(id, kind.name(), &kind.shape(), None, &timeline, &mut structure);
        if *kind == RecipeType::Broken {
            let names = field_names(Kind::Normal, 2);
            let (front, _) = base_templates(Kind::Normal, &names, 0);
            let config = &mut record.templates[0].config;
            config.q_format = front;
            config.a_format = "{{FrontSide}}\n<hr class=\"mg-rule\">\n{{Missing Field}}".to_owned();
        }
        types.push((*kind, record));
    }
    if types.is_empty() {
        let id = NotetypeId(ids.notetypes.claim(timeline.crt_ms()));
        types.push((
            RecipeType::Basic,
            notetype(
                id,
                RecipeType::Basic.name(),
                &RecipeType::Basic.shape(),
                None,
                &timeline,
                &mut structure,
            ),
        ));
    }
    out.counts.presets = presets.len() as u64;
    out.counts.decks = deck_rows.len() as u64;
    out.counts.notetypes = types.len() as u64;
    out.scope(|scope| {
        for p in &presets {
            scope.put(p)?;
        }
        for d in &deck_rows {
            scope.put(d)?;
        }
        for (_, nt) in &types {
            scope.put_notetype(nt)?;
        }
        Ok(())
    })?;

    // Notes.
    let shape = revlog_shape();
    let fsrs = Fsrs::new();
    let mut position = 0u32;
    let mut note_index = 0u64;
    for (spec, &deck_id) in recipe.decks.iter().zip(&spec_decks) {
        let kind = RecipeType::parse(spec.notetype.as_deref())?;
        let per_note = kind.cards_per_note();
        let states = [
            (spec.new, Target::New),
            (spec.learning, Target::Learning),
            (spec.due, Target::Review(Due::Today)),
            (spec.later, Target::Review(Due::Later)),
            (spec.suspended, Target::Suspended),
            (spec.buried, Target::Buried),
        ];
        let (_, record) = types.iter().find(|(k, _)| *k == kind).expect("note type made above");
        let mut n_in_deck = 0u32;
        for (count, target) in states {
            if count % per_note != 0 {
                return Err(GenError::Recipe(format!(
                    "deck {:?}: counts of a {} deck must be multiples of {per_note}",
                    spec.name,
                    kind.name()
                )));
            }
            for _ in 0..count / per_note {
                n_in_deck += 1;
                position += 1;
                let mut rng = Rng::stream(seed, Stream::Recipe, note_index + 1);
                let rows = recipe_note(
                    &timeline,
                    &shape,
                    &fsrs,
                    &mut ids,
                    &mut rng,
                    RecipeNote {
                        spec,
                        kind,
                        notetype: record.id,
                        deck: deck_id,
                        target,
                        number: n_in_deck,
                        position,
                        index: note_index,
                        seed,
                    },
                )?;
                out.push(rows)?;
                note_index += 1;
            }
        }
    }

    let closing = closing_config(&timeline, types[0].1.id.0, u64::from(position));
    let name = recipe.recipe.clone();
    out.finish(&closing, |counts| Manifest {
        generator_version: GENERATOR_VERSION,
        kind: "recipe".to_owned(),
        name,
        seed,
        cards_requested: counts.cards,
        now_ms,
        utc_offset_secs: offset,
        profile_sha256: None,
        worst_case_sha256: None,
        media: MediaMode::Files,
        counts,
        dump_sha256: String::new(),
    })
}

struct RecipeNote<'a> {
    spec: &'a DeckSpec,
    kind: RecipeType,
    notetype: NotetypeId,
    deck: DeckId,
    target: Target,
    /// The note's number in its deck, from 1.
    number: u32,
    position: u32,
    index: u64,
    seed: u64,
}

/// Answer instants for a recipe card in `target` state, increasing, all
/// before the pinned day (F9.2, F4.4): a learning card has one answer in
/// the 24 hours before the pinned day, so it is due now; a due card's last
/// answer is 1 to 20 days before today; a later card's (and a suspended or
/// buried one's) 1 to 3 days before today. Earlier answers come a week (6
/// days for later cards) apart.
fn recipe_reviews(timeline: &Timeline, target: Target, rng: &mut Rng) -> Vec<i64> {
    let at = |day: i64, rng: &mut Rng| {
        (timeline.crt + day * 86_400 + 3600 * (2 + rng.below(14) as i64)) * 1000 + rng.below(3_600_000) as i64
    };
    let pinned_day_start = (timeline.crt + timeline.today * 86_400) * 1000;
    let count = 2 + rng.below(6) as i64;
    match target {
        Target::New => Vec::new(),
        Target::Learning => vec![pinned_day_start - 86_400_000 + rng.below(86_400_000) as i64],
        Target::Review(Due::Today) => {
            let last = timeline.today - 1 - rng.below(20) as i64;
            (0..count).map(|i| at(last - (count - 1 - i) * 7, rng)).collect()
        }
        _ => {
            let last = timeline.today - 1 - rng.below(3) as i64;
            (0..count).map(|i| at(last - (count - 1 - i) * 6, rng)).collect()
        }
    }
}

fn recipe_note(
    timeline: &Timeline,
    shape: &RevlogShape,
    fsrs: &Fsrs,
    ids: &mut AllIds,
    rng: &mut Rng,
    n: RecipeNote<'_>,
) -> Result<NoteRows, GenError> {
    let reviews = recipe_reviews(timeline, n.target, rng);
    let created = match reviews.first() {
        Some(&first) => first - 86_400_000 - rng.below(86_400_000) as i64,
        None => timeline.now_ms - 86_400_000 * (1 + rng.below(30) as i64),
    };
    // The note id is the creation instant (F4.7), moved up when taken.
    let note_id = NoteId(ids.notes.claim(created));
    let created = note_id.0;

    // Field text and tags are NFC (F6.6), as Anki would save them.
    let mut values: Vec<String> = (0..2)
        .map(|f| match n.spec.fields.get(f) {
            Some(template) => meguri_text::normalize::nfc(&template.replace("{n}", &n.number.to_string())).into_owned(),
            None => words(6 + rng.below(20) as usize, rng),
        })
        .collect();
    if n.kind == RecipeType::Cloze {
        values[0] = format!("{{{{c1::{}}}}} {}", words(5, rng), values[0]);
    }
    let mut rows_media = Vec::new();
    if let Some(m) = &n.spec.media {
        let refs = [
            (m.images, MediaKind::Image, true),
            (m.sounds, MediaKind::Audio, true),
            (m.missing, MediaKind::Image, false),
        ];
        let mut k = 0;
        for (count, kind, written) in refs {
            for _ in 0..count {
                let ext = if kind == MediaKind::Image { "png" } else { "mp3" };
                let prefix = if written { "mg" } else { "mg-missing" };
                let file = MediaFile {
                    name: format!("{prefix}{:x}-{k}.{ext}", n.index),
                    bytes: 2048 + rng.below(30_000),
                    kind,
                    seed: n.seed,
                    index: n.index * 1024 + k,
                };
                values[1].push(' ');
                values[1].push_str(&file.reference());
                if written {
                    rows_media.push(file);
                }
                k += 1;
            }
        }
    }
    let tags = unicase_sorted(
        n.spec
            .tags
            .iter()
            .map(|t| meguri_text::normalize::nfc(t).into_owned())
            .collect(),
    );
    let note = Note {
        id: note_id,
        guid: meguri_text::base91::encode(rng.u64()),
        notetype_id: n.notetype,
        mtime: EpochSecs(created.div_euclid(1000)),
        usn: Usn::PENDING,
        tags: if tags.is_empty() {
            String::new()
        } else {
            format!(" {} ", tags.join(" "))
        },
        fields: values.join(&FIELD_SEPARATOR.to_string()),
        sort_field: sort_text_of(&values, 0).into_owned(),
        checksum: note_checksum(&values[0]),
        flags: 0,
        data: String::new(),
    };
    let mut rows = NoteRows {
        note: Some(note),
        media: rows_media,
        tags,
        ..NoteRows::default()
    };
    for ord in 0..n.kind.cards_per_note() {
        let card_id = CardId(ids.cards.claim(created + i64::from(ord)));
        let input = CardInput {
            id: card_id,
            note_id,
            deck_id: n.deck,
            ord: ord as u16,
            position: n.position,
            created_ms: created,
            target: n.target,
            reviews: &reviews,
        };
        let (card, entries) = card_rows(shape, timeline, fsrs, &input, &mut ids.revlog, rng)?;
        rows.cards.push(card);
        rows.revlog.extend(entries);
    }
    Ok(rows)
}
