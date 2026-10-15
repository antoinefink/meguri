//! One note with its cards, review log and media: drawn from the note's own
//! stream, so a note's rows depend only on the seed and its index.
//!
//! Review instants follow the profile's reviews-per-day and hour-of-day
//! histograms: a note's first reviewed card draws all its review days from
//! them, its note was created the profile's first-review gap before its
//! first review, and its other cards start their reviews that same kind of
//! gap after the note's creation. Notes with no reviewed card are created
//! on days drawn from the unreviewed-notes histogram.

use std::collections::HashSet;

use meguri_model::ids::{CardId, NoteId, NotetypeId};
use meguri_model::note::{FIELD_SEPARATOR, Note};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_text::checksum::{note_checksum, sort_text_of};

use super::cards::{CardInput, Due, Target, card_rows};
use super::decks::DeckPlan;
use super::media::{MediaFile, MediaKind, base64, png_of_size};
use super::notetypes::{GenNotetype, Kind};
use super::profile::Profile;
use super::revlog::Fsrs;
use super::rng::{Rng, Stream};
use super::{AllIds, NoteRows, Timeline, WorstCase, draw_target};

pub struct NoteContext<'a> {
    pub profile: &'a Profile,
    pub worst: Option<&'a WorstCase>,
    pub timeline: &'a Timeline,
    pub types: &'a [GenNotetype],
    pub decks: &'a DeckPlan,
    pub tags: &'a TagPool,
    pub fsrs: &'a Fsrs,
    pub seed: u64,
    pub total_notes: u64,
}

/// The tags notes draw from.
#[derive(Debug)]
pub struct TagPool {
    names: Vec<String>,
    /// Every tag must appear on some note (the tag-tree worst case).
    cover_all: bool,
    per_note: super::Histogram,
}

const TAG_LEVELS: [&str; 6] = ["topic", "area", "unit", "part", "item", "leaf"];

impl TagPool {
    pub fn build(profile: &Profile, worst: Option<&WorstCase>, scale: f64, rng: &mut Rng) -> TagPool {
        if let Some(nodes) = worst.and_then(|w| w.tag_nodes) {
            // A tree with ten children per node: node i's children are
            // 10i + 10 to 10i + 19, the first ten nodes are the roots.
            let mut names: Vec<String> = Vec::with_capacity(nodes as usize);
            for i in 0..nodes as usize {
                let name = if i < 10 {
                    format!("node{i}")
                } else {
                    format!("{}::node{i}", names[(i - 10) / 10])
                };
                names.push(name);
            }
            return TagPool {
                names,
                cover_all: true,
                per_note: profile.tags.per_note.clone(),
            };
        }
        let distinct = (profile.totals.tags as f64 * scale).round() as usize;
        let distinct = if profile.totals.tags > 0 { distinct.max(1) } else { 0 };
        let width = (distinct as f64).sqrt().ceil().max(2.0) as u64;
        let mut seen = HashSet::new();
        let mut names = Vec::with_capacity(distinct);
        while names.len() < distinct {
            let depth = profile
                .tags
                .depth
                .sample(rng)
                .unwrap_or(1)
                .clamp(1, TAG_LEVELS.len() as i64) as usize;
            let mut name = (0..depth)
                .map(|level| format!("{}{}", TAG_LEVELS[level], rng.below(width)))
                .collect::<Vec<_>>()
                .join("::");
            if !seen.insert(name.to_lowercase()) {
                name = format!("{name}_{}", names.len());
                seen.insert(name.to_lowercase());
            }
            names.push(name);
        }
        TagPool {
            names,
            cover_all: false,
            per_note: profile.tags.per_note.clone(),
        }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// The tags of note `index` of `total`, sorted.
    pub fn for_note(&self, index: u64, total: u64, rng: &mut Rng) -> Vec<String> {
        if self.names.is_empty() {
            return Vec::new();
        }
        let mut picked: Vec<usize> = Vec::new();
        if self.cover_all {
            let mut i = index as usize;
            while i < self.names.len() {
                picked.push(i);
                i += total.max(1) as usize;
            }
        }
        let count = self.per_note.sample(rng).unwrap_or(0).max(0) as usize;
        for _ in 0..count.min(self.names.len()) {
            picked.push(rng.below(self.names.len() as u64) as usize);
        }
        picked.sort_unstable();
        picked.dedup();
        let mut tags: Vec<String> = picked.into_iter().map(|i| self.names[i].clone()).collect();
        tags.sort();
        tags
    }
}

/// The note type of every note, in generation order: each note type's
/// share of the profile's notes, scaled, shuffled.
pub fn assign_notetypes(profile: &Profile, types: &[GenNotetype], cards: u64, scale: f64, rng: &mut Rng) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::new();
    for (i, nt) in types.iter().enumerate() {
        let notes = profile.notetypes[nt.shape].notes as f64 * scale;
        let notes = if profile.notetypes[nt.shape].notes > 0 {
            (notes.round() as u64).max(1)
        } else {
            0
        };
        out.extend(std::iter::repeat_n(i as u16, notes as usize));
    }
    if out.is_empty() {
        out = vec![0; cards.max(1) as usize];
    }
    for i in (1..out.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        out.swap(i, j);
    }
    out
}

const SYLLABLES: [&str; 24] = [
    "ka", "ri", "mo", "su", "te", "no", "wa", "zu", "pe", "lo", "an", "en", "is", "or", "ul", "vi", "do", "ge", "ha",
    "mi", "ne", "ra", "si", "to",
];

/// About `len` bytes of words, never more.
pub fn words(len: usize, rng: &mut Rng) -> String {
    let mut out = String::with_capacity(len + 8);
    while out.len() < len {
        if !out.is_empty() {
            out.push(' ');
        }
        for _ in 0..1 + rng.below(3) {
            out.push_str(SYLLABLES[rng.below(SYLLABLES.len() as u64) as usize]);
        }
    }
    out.truncate(len);
    out
}

impl NoteContext<'_> {
    fn due_mode(&self) -> bool {
        self.worst.is_some_and(|w| w.due_deck.is_some())
    }

    /// The last day reviews may fall on.
    fn last_review_day(&self) -> i64 {
        if self.due_mode() {
            self.timeline.today - 1
        } else {
            self.timeline.today
        }
    }

    /// A review instant on a day from `min_day` to the last review day.
    fn review_instant(&self, rng: &mut Rng, min_day: i64) -> Option<i64> {
        let max_day = self.last_review_day();
        if min_day > max_day {
            return None;
        }
        let revlog = &self.profile.revlog;
        for _ in 0..16 {
            let day = revlog.review_days.sample_within(rng, min_day, max_day + 1);
            let hour = revlog.hour_of_day.sample(rng).unwrap_or(12);
            if let Some(ms) = self.timeline.instant(day, hour, rng) {
                return Some(ms);
            }
        }
        None
    }

    fn reviews_per_card(&self, rng: &mut Rng) -> usize {
        self.profile.revlog.per_card.sample(rng).unwrap_or(1).max(1) as usize
    }

    /// The creation instant of a note whose first review is at `first`.
    fn created_before(&self, rng: &mut Rng, first: i64) -> i64 {
        let floor = self.timeline.crt_ms() + 1000;
        let room = (first - floor) / 1000;
        if room < 2 {
            return floor.min(first - 1);
        }
        let gap = self.profile.revlog.first_review_gap_secs.sample_within(rng, 1, room);
        first - gap * 1000 - rng.below(1000) as i64
    }

    /// The creation instant of a note none of whose cards is reviewed.
    fn created_unreviewed(&self, rng: &mut Rng) -> i64 {
        let today = self.timeline.today;
        for _ in 0..16 {
            let day = self
                .profile
                .notes
                .unreviewed_creation_days
                .sample_within(rng, 0, today + 1);
            let hour = self.profile.revlog.hour_of_day.sample(rng).unwrap_or(12);
            if let Some(ms) = self.timeline.instant(day, hour, rng) {
                return ms.max(self.timeline.crt_ms() + 1000);
            }
        }
        self.timeline.now_ms - 1000 - rng.below(86_400_000) as i64
    }
}

/// The field values of a note: lengths from the note type's field
/// histograms, clozes in the first field of a cloze note type, media
/// references by the note type's per-field counts, and the worst cases'
/// large fields and inline images.
fn fields(
    ctx: &NoteContext<'_>,
    nt: &GenNotetype,
    index: u64,
    clozes: usize,
    rng: &mut Rng,
) -> (Vec<String>, Vec<MediaFile>) {
    let shape = &ctx.profile.notetypes[nt.shape];
    let count = nt.fields();
    let mut values: Vec<String> = (0..count)
        .map(|f| {
            let len = shape.field_bytes[f].sample(rng).unwrap_or(0).max(0) as usize;
            let len = if f == 0 { len.max(4) } else { len };
            words(len, rng)
        })
        .collect();
    if nt.kind == Kind::Cloze {
        let mut text = String::new();
        for n in 1..=clozes {
            text.push_str(&format!("{{{{c{n}::{}}}}} ", words(3 + rng.below(12) as usize, rng)));
        }
        text.push_str(&values[0]);
        values[0] = text;
    }

    // Media references.
    let mut media = Vec::new();
    let refs = match ctx.worst.and_then(|w| w.media_files.as_ref()) {
        Some(files) => {
            let total = ctx.total_notes.max(1);
            files.count / total + u64::from(index < files.count % total)
        }
        None => shape.refs_per_note.sample(rng).unwrap_or(0).max(0) as u64,
    };
    let weights: Vec<u64> = shape.media_refs.iter().map(|[i, s]| i + s).collect();
    for n in 0..refs {
        let field = rng.weighted(&weights).unwrap_or(count - 1);
        let [images, sounds] = shape.media_refs.get(field).copied().unwrap_or([1, 1]);
        let image = if images + sounds == 0 {
            n % 2 == 0
        } else {
            rng.below(images + sounds) < images
        };
        let bytes = match ctx.worst.and_then(|w| w.media_files.as_ref()) {
            Some(files) => files.bytes.sample(rng),
            None => ctx.profile.media.file_bytes.sample(rng),
        }
        .unwrap_or(4096)
        .max(128) as u64;
        let (kind, ext) = if image {
            (MediaKind::Image, "png")
        } else {
            (MediaKind::Audio, "mp3")
        };
        let file = MediaFile {
            name: format!("mg{index:x}-{n}.{ext}"),
            bytes,
            kind,
            seed: ctx.seed,
            index: index * 1024 + n,
        };
        let value = &mut values[field];
        if !value.is_empty() {
            value.push(' ');
        }
        value.push_str(&file.reference());
        media.push(file);
    }

    // Worst cases.
    if let Some(large) = ctx.worst.and_then(|w| w.large_fields.as_ref())
        && index < large.notes
    {
        let field = usize::from(count > 1);
        values[field] = words(large.bytes as usize, rng);
    }
    if let Some(inline) = ctx.worst.and_then(|w| w.inline_images.as_ref())
        && index < inline.notes
    {
        for _ in 0..inline.per_note {
            let png = png_of_size(inline.image_bytes as usize, rng);
            values[0].push_str(&format!("<img src=\"data:image/png;base64,{}\">", base64(&png)));
        }
    }
    (values, media)
}

/// The rows of note `index`, of note type `types[nt]`.
pub fn generate_note(ctx: &NoteContext<'_>, index: u64, nt: u16, ids: &mut AllIds) -> NoteRows {
    let mut rng = Rng::stream(ctx.seed, Stream::Note, index);
    let rng = &mut rng;
    let notetype = &ctx.types[usize::from(nt)];
    let shape = &ctx.profile.notetypes[notetype.shape];
    let deck = ctx.decks.pick(rng);
    let card_count = match notetype.kind {
        Kind::Normal => notetype.templates(),
        Kind::Cloze => shape.cards_per_note.sample_within(rng, 1, 65) as usize,
    };
    let mut targets: Vec<Target> = (0..card_count)
        .map(|_| {
            if ctx.due_mode() {
                Target::Review(Due::Today)
            } else {
                draw_target(ctx.profile, rng)
            }
        })
        .collect();

    // Review instants, then the note's creation.
    let mut reviews: Vec<Vec<i64>> = vec![Vec::new(); card_count];
    let created = match targets.iter().position(|t| !t.is_new()) {
        Some(lead) => {
            let mut lead_reviews: Vec<i64> = (0..ctx.reviews_per_card(rng))
                .filter_map(|_| ctx.review_instant(rng, 0))
                .collect();
            lead_reviews.sort_unstable();
            lead_reviews.dedup();
            match lead_reviews.first().copied() {
                Some(first) => {
                    let created = ctx.created_before(rng, first);
                    reviews[lead] = lead_reviews;
                    for (ord, target) in targets.iter_mut().enumerate() {
                        if ord == lead || target.is_new() {
                            continue;
                        }
                        reviews[ord] = sibling_reviews(ctx, rng, created);
                        if reviews[ord].is_empty() {
                            *target = Target::New;
                        }
                    }
                    created
                }
                None => {
                    targets.iter_mut().for_each(|t| *t = Target::New);
                    ctx.created_unreviewed(rng)
                }
            }
        }
        None => ctx.created_unreviewed(rng),
    };

    let note_id = NoteId(ids.notes.claim(created));
    let (values, media) = fields(ctx, notetype, index, card_count, rng);
    let tags = ctx.tags.for_note(index, ctx.total_notes, rng);
    let note = Note {
        id: note_id,
        guid: meguri_text::base91::encode(rng.u64()),
        notetype_id: NotetypeId(notetype.record.id.0),
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
    let position = u32::try_from(index + 1).unwrap_or(u32::MAX);
    let mut rows = NoteRows {
        note: Some(note),
        media,
        tags,
        ..NoteRows::default()
    };
    for (ord, target) in targets.iter().enumerate() {
        let card_id = CardId(ids.cards.claim(created + ord as i64));
        let input = CardInput {
            id: card_id,
            note_id,
            deck_id: deck,
            ord: ord as u16,
            position,
            created_ms: created,
            target: *target,
            reviews: &reviews[ord],
        };
        let (card, entries) = card_rows(
            &ctx.profile.revlog,
            ctx.timeline,
            ctx.fsrs,
            &input,
            &mut ids.revlog,
            rng,
        );
        rows.cards.push(card);
        rows.revlog.extend(entries);
    }
    rows
}

/// Review instants of a card whose note was created at `created`, not its
/// first reviewed card: the first one a first-review gap after the
/// creation, the others on later review days. Empty when no first review
/// fits before the last review day ends.
fn sibling_reviews(ctx: &NoteContext<'_>, rng: &mut Rng, created: i64) -> Vec<i64> {
    let limit = (ctx.timeline.crt + (ctx.last_review_day() + 1) * 86_400) * 1000;
    let limit = limit.min(ctx.timeline.now_ms);
    let room = (limit - created) / 1000;
    if room < 2 {
        return Vec::new();
    }
    let gap = ctx.profile.revlog.first_review_gap_secs.sample_within(rng, 1, room);
    let first = created + gap * 1000 + rng.below(1000) as i64;
    if first > limit {
        return Vec::new();
    }
    let mut out = vec![first];
    let first_day = ctx.timeline.day_of(first);
    for _ in 1..ctx.reviews_per_card(rng) {
        for _ in 0..8 {
            if let Some(ms) = ctx.review_instant(rng, first_day)
                && ms > first
            {
                out.push(ms);
                break;
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}
