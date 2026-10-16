//! One note with its cards, review log and media: drawn from the note's own
//! stream, so a note's rows depend only on the seed and its index.
//!
//! Review instants follow the profile's reviews-per-day and hour-of-day
//! histograms: a note's first reviewed card draws all its review days from
//! them, its note was created the profile's first-review gap before its
//! first review, and its other cards start their reviews that same kind of
//! gap after the note's creation. Notes with no reviewed card are created
//! on days drawn from the unreviewed-notes histogram.

use std::cmp::Ordering;
use std::collections::HashSet;

use meguri_model::ids::{CardId, NoteId, NotetypeId};
use meguri_model::note::{FIELD_SEPARATOR, Note};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_text::checksum::{note_checksum, sort_text_of};
use meguri_text::fold::compare_folded;

use super::cards::{CardInput, Due, Target, card_rows};
use super::decks::DeckPlan;
use super::media::{MediaFile, MediaKind, base64, png_of_size};
use super::notetypes::{GenNotetype, Kind};
use super::profile::{NotetypeShape, Profile};
use super::revlog::Fsrs;
use super::rng::{Rng, Stream};
use super::{AllIds, GenError, NoteRows, Timeline, WorstCase, draw_target};

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

    /// The tags of note `index` of `total`, in `unicase` order (F4.5,
    /// F6.6).
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
        for _ in 0..count {
            picked.push(rng.below(self.names.len() as u64) as usize);
        }
        picked.sort_unstable();
        picked.dedup();
        let tags: Vec<String> = picked.into_iter().map(|i| self.names[i].clone()).collect();
        unicase_sorted(tags)
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

/// Tags in `unicase` order (`text/unicode-primitives` U2), those equal
/// under it after the first dropped: the order Anki's tag clean-up leaves.
pub fn unicase_sorted(mut tags: Vec<String>) -> Vec<String> {
    tags.sort_by(|a, b| compare_folded(a, b));
    tags.dedup_by(|b, a| compare_folded(a, b) == Ordering::Equal);
    tags
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

    /// The last day answers may fall on: the day before the pinned day
    /// (F4.4).
    fn last_review_day(&self) -> i64 {
        self.timeline.today - 1
    }

    /// The start of the pinned day, epoch milliseconds: every answer is
    /// before it.
    fn pinned_day_start_ms(&self) -> i64 {
        (self.timeline.crt + self.timeline.today * 86_400) * 1000
    }

    /// An answer instant on a day from `min_day` to the last review day
    /// (F4.6); `None` when that range is empty. Such an instant is always
    /// before now.
    fn review_instant(&self, rng: &mut Rng, min_day: i64) -> Option<i64> {
        let max_day = self.last_review_day();
        if min_day > max_day {
            return None;
        }
        let revlog = &self.profile.revlog;
        let day = revlog.review_days.sample_within(rng, min_day, max_day + 1);
        let hour = revlog.hour_of_day.sample(rng).unwrap_or(12);
        self.timeline.instant(day, hour, rng)
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

    /// The creation instant of a note none of whose cards is reviewed: a
    /// day within `[0, today]`, an hour, drawn again while the instant is
    /// after now (F4.6). After 16 refusals (a day and hour mix that keeps
    /// landing after now on the pinned day) it takes an instant in the day
    /// before now, so generation always ends.
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
        (self.timeline.now_ms - 1000 - rng.below(86_400_000) as i64).max(self.timeline.crt_ms() + 1000)
    }
}

/// The bytes of the cloze deletions `{{c1::…}}` to `{{cN::…}}` with these
/// answer lengths, joined by single spaces.
fn deletions_bytes(answers: &[usize]) -> usize {
    let digits = |k: usize| k.to_string().len();
    let each: usize = answers
        .iter()
        .enumerate()
        .map(|(i, answer)| "{{c::}}".len() + digits(i + 1) + answer)
        .sum();
    each + answers.len().saturating_sub(1)
}

/// The answer lengths of the cloze deletions of a field drawn at `len`
/// bytes (F4.8): the drawn lengths, or 1 byte each when the deletions they
/// make, joined by single spaces, are longer than `len`.
pub fn answer_bytes(drawn: Vec<usize>, len: usize) -> Vec<usize> {
    if deletions_bytes(&drawn) > len {
        vec![1; drawn.len()]
    } else {
        drawn
    }
}

/// The cloze deletions of a cloze note's first field drawn at `len` bytes
/// (F4.8): `{{c1::…}}` to `{{cN::…}}` for its `cards` cards, each answer
/// words of 3 to 14 bytes, or of 1 byte when they do not fit (`answer_bytes`).
pub fn cloze_deletions(cards: usize, len: usize, rng: &mut Rng) -> Vec<String> {
    let drawn = (0..cards).map(|_| 3 + rng.below(12) as usize).collect();
    answer_bytes(drawn, len)
        .into_iter()
        .enumerate()
        .map(|(i, answer)| format!("{{{{c{}::{}}}}}", i + 1, words(answer, rng)))
        .collect()
}

/// A field's stored text as F4.8 builds it from its drawn length: the
/// cloze deletions, then words, then the media references, joined by
/// single spaces. The markup (deletions and references) counts inside
/// `len`; the words fill what it leaves.
#[derive(Clone, Debug)]
pub struct FieldText {
    len: usize,
    deletions: Vec<String>,
    references: Vec<String>,
}

impl FieldText {
    /// A field drawn at `len` bytes, without markup yet.
    pub fn new(len: usize) -> FieldText {
        FieldText::with_deletions(len, Vec::new())
    }

    /// A cloze first field drawn at `len` bytes, holding `deletions`.
    pub fn with_deletions(len: usize, deletions: Vec<String>) -> FieldText {
        FieldText {
            len,
            deletions,
            references: Vec::new(),
        }
    }

    /// Bytes of the markup joined by single spaces.
    fn markup_bytes(&self) -> usize {
        let pieces = self.deletions.iter().chain(&self.references);
        let (count, bytes) = pieces.fold((0usize, 0usize), |(n, b), p| (n + 1, b + p.len()));
        bytes + count.saturating_sub(1)
    }

    fn has_markup(&self) -> bool {
        !self.deletions.is_empty() || !self.references.is_empty()
    }

    /// Whether `reference` fits: the markup so far, a separating space
    /// when there is markup, and the reference take at most the drawn
    /// length together.
    pub fn fits(&self, reference: &str) -> bool {
        self.markup_bytes() + usize::from(self.has_markup()) + reference.len() <= self.len
    }

    /// Adds a reference after those already there, fitting or not (the
    /// caller decides, `fits`).
    pub fn push_reference(&mut self, reference: String) {
        self.references.push(reference);
    }

    /// The stored text. Without markup it is `len` bytes of words. With
    /// markup the words take `len` minus the markup minus one separating
    /// space; when fewer than 2 bytes are left the field is the markup
    /// alone, which may be one byte short of `len` or longer than it.
    pub fn finish(self, rng: &mut Rng) -> String {
        if !self.has_markup() {
            return words(self.len, rng);
        }
        let left = self.len.saturating_sub(self.markup_bytes());
        let mut pieces = self.deletions;
        if left >= 2 {
            pieces.push(words(left - 1, rng));
        }
        pieces.extend(self.references);
        pieces.join(" ")
    }
}

/// The drawn length a field needs before it takes a reference whose first
/// is of `kind` (F4.8): the longest reference 0 can be, with a 5-digit hex
/// note index (positions stay below 1,000,000, F6.6).
pub fn reference_threshold(kind: MediaKind) -> usize {
    match kind {
        MediaKind::Image => 25,
        MediaKind::Audio => 21,
    }
}

/// `s(m)` of F4.8: the share of a field's drawn lengths at `min` bytes or
/// more, each bucket counting the fraction of its integers (drawn
/// uniformly, F2.2) that are at least `min`.
pub fn length_share(lengths: &super::Histogram, min: i64) -> f64 {
    let total = lengths.total();
    if total == 0 {
        return 0.0;
    }
    let reaching: f64 = lengths
        .counts
        .iter()
        .zip(lengths.edges.windows(2))
        .map(|(&count, edge)| {
            let (lo, hi) = (edge[0], edge[1]);
            let fraction = if lo >= min {
                1.0
            } else if hi <= min {
                0.0
            } else {
                (hi - min) as f64 / (hi - lo) as f64
            };
            count as f64 * fraction
        })
        .sum();
    reaching / total as f64
}

/// References per note a field takes when its drawn length reaches the
/// threshold of its first reference's kind (F4.8): `r = (images + sounds)
/// / (notes × q)`, `q` the share of drawn lengths reaching the threshold,
/// each kind weighted by its references. 0 for a field without
/// references, of a shape without notes, or whose `q` is 0.
pub fn reference_rate(shape: &NotetypeShape, field: usize) -> f64 {
    let [images, sounds] = shape.media_refs.get(field).copied().unwrap_or([0, 0]);
    let Some(lengths) = shape.field_bytes.get(field) else {
        return 0.0;
    };
    if images + sounds == 0 || shape.notes == 0 {
        return 0.0;
    }
    let reaching = |kind| length_share(lengths, reference_threshold(kind) as i64);
    let q = (images as f64 * reaching(MediaKind::Image) + sounds as f64 * reaching(MediaKind::Audio))
        / (images + sounds) as f64;
    if q == 0.0 {
        return 0.0;
    }
    (images + sounds) as f64 / (shape.notes as f64 * q)
}

/// Media file `n` of note `index` (F4.8): `mg<index in hex>-<n>.png` or
/// `.mp3`, its bytes from the run's seed.
pub fn note_media(seed: u64, index: u64, n: u64, kind: MediaKind, bytes: u64) -> MediaFile {
    let ext = match kind {
        MediaKind::Image => "png",
        MediaKind::Audio => "mp3",
    };
    MediaFile {
        name: format!("mg{index:x}-{n}.{ext}"),
        bytes,
        kind,
        seed,
        index: index * 1024 + n,
    }
}

/// The field values of a note (F4.8): each field's whole stored length
/// drawn from the note type's field histograms, cloze deletions in a cloze
/// note type's first field, media references by field in fields drawn long
/// enough for them and where they fit (or, in the `media_files` worst
/// case, the exact count, always written), and
/// the worst cases' large fields and inline images.
fn fields(
    ctx: &NoteContext<'_>,
    nt: &GenNotetype,
    index: u64,
    clozes: usize,
    rng: &mut Rng,
) -> (Vec<String>, Vec<MediaFile>) {
    let shape = &ctx.profile.notetypes[nt.shape];
    let count = nt.fields();
    let media_files = ctx.worst.and_then(|w| w.media_files.as_ref());
    let large = ctx
        .worst
        .and_then(|w| w.large_fields.as_ref())
        .filter(|large| index < large.notes);
    let large_field = usize::from(count > 1);
    let refs_of = |field: usize| shape.media_refs.get(field).copied().unwrap_or([0, 0]);

    // The `media_files` worst case: the note's references by field, drawn
    // up front so that they are numbered in field order.
    let mut assigned: Vec<Vec<MediaKind>> = vec![Vec::new(); count];
    if let Some(files) = media_files {
        let total = ctx.total_notes.max(1);
        let refs = files.count / total + u64::from(index < files.count % total);
        let weights: Vec<u64> = (0..count).map(|f| refs_of(f).iter().sum()).collect();
        for k in 0..refs {
            let field = rng.weighted(&weights).unwrap_or(count - 1);
            let [images, sounds] = refs_of(field);
            let image = if images + sounds == 0 {
                k % 2 == 0
            } else {
                rng.below(images + sounds) < images
            };
            assigned[field].push(if image { MediaKind::Image } else { MediaKind::Audio });
        }
    }
    let file_bytes = |rng: &mut Rng| {
        match media_files {
            Some(files) => files.bytes.sample(rng),
            None => ctx.profile.media.file_bytes.sample(rng),
        }
        .unwrap_or(4096)
        .max(128) as u64
    };

    let mut values = Vec::with_capacity(count);
    let mut media: Vec<MediaFile> = Vec::new();
    for (field, kinds) in assigned.iter().enumerate() {
        let drawn = shape.field_bytes[field].sample(rng).unwrap_or(0).max(0) as usize;
        // A normal front is never empty (F7).
        let len = if field == 0 && nt.kind == Kind::Normal {
            drawn.max(1)
        } else {
            drawn
        };
        if let Some(large) = large
            && field == large_field
        {
            // Exactly `bytes` bytes of words, without references.
            values.push(words(large.bytes as usize, rng));
            continue;
        }
        let mut text = if field == 0 && nt.kind == Kind::Cloze {
            FieldText::with_deletions(len, cloze_deletions(clozes, len, rng))
        } else {
            FieldText::new(len)
        };
        if media_files.is_some() {
            for &kind in kinds {
                let file = note_media(ctx.seed, index, media.len() as u64, kind, file_bytes(rng));
                text.push_reference(file.reference());
                media.push(file);
            }
        } else {
            let rate = reference_rate(shape, field);
            let [images, sounds] = refs_of(field);
            let kind = |rng: &mut Rng| {
                if rng.below(images + sounds) < images {
                    MediaKind::Image
                } else {
                    MediaKind::Audio
                }
            };
            // The first reference's kind decides whether the field is long
            // enough to take references at all.
            let first = (rate > 0.0).then(|| kind(rng));
            if let Some(first) = first
                && len >= reference_threshold(first)
            {
                let whole = rate.floor();
                let draws = whole as u64 + u64::from(rng.chance(rate - whole));
                for k in 0..draws {
                    let kind = if k == 0 { first } else { kind(rng) };
                    // Named as the next written reference; one that does
                    // not fit is not written and has no file.
                    let mut file = note_media(ctx.seed, index, media.len() as u64, kind, 0);
                    let reference = file.reference();
                    if text.fits(&reference) {
                        file.bytes = file_bytes(rng);
                        text.push_reference(reference);
                        media.push(file);
                    }
                }
            }
        }
        values.push(text.finish(rng));
    }

    // The inline-images worst case: after the first field's text, beyond
    // its drawn length.
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
pub fn generate_note(ctx: &NoteContext<'_>, index: u64, nt: u16, ids: &mut AllIds) -> Result<NoteRows, GenError> {
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

    // The note id is the creation instant (F4.7), moved up when taken.
    let note_id = NoteId(ids.notes.claim(created));
    let created = note_id.0;
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
        )?;
        rows.cards.push(card);
        rows.revlog.extend(entries);
    }
    Ok(rows)
}

/// Answer instants of a card whose note was created at `created`, not its
/// note's lead card (F4.6): the first one a first-review gap after the
/// creation, within the seconds left before the pinned day, then the
/// others drawn on days from that first day on, keeping those after the
/// first. Empty when no first answer fits.
fn sibling_reviews(ctx: &NoteContext<'_>, rng: &mut Rng, created: i64) -> Vec<i64> {
    let limit = ctx.pinned_day_start_ms();
    let room = (limit - created) / 1000;
    if room < 2 {
        return Vec::new();
    }
    let gap = ctx.profile.revlog.first_review_gap_secs.sample_within(rng, 1, room);
    let first = created + gap * 1000 + rng.below(1000) as i64;
    if first >= limit {
        return Vec::new();
    }
    let mut out = vec![first];
    let first_day = ctx.timeline.day_of(first);
    for _ in 1..ctx.reviews_per_card(rng) {
        if let Some(ms) = ctx.review_instant(rng, first_day)
            && ms > first
        {
            out.push(ms);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}
