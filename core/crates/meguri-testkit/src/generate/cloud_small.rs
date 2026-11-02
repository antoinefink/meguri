//! Direct, bounded Q4.2 CloudKit base. No recipe or preparation mutation.
//! Only this request pins 2025; legacy generator requests keep their clock.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId};
use meguri_model::note::{FIELD_SEPARATOR, Note};
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::notetype::{CardRequirement, FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{OpenOptions, ReadRows, Table};
use meguri_text::checksum::{note_checksum, sort_text_of};
use serde::Serialize;

use super::cards::{CardInput, Target, card_rows};
use super::media::{MediaFile, MediaKind};
use super::profile::{Histogram, RevlogShape};
use super::revlog::Fsrs;
use super::rng::{Rng, Stream};
use super::{
    AllIds, Counts, GENERATOR_VERSION, GenError, Manifest, MediaMode, NoteRows, Output, Timeline, base_config,
    closing_config,
};

pub const NOW_MS: i64 = 1_735_689_600_000;
pub const PINNED_DAY_START_MS: i64 = 1_735_617_600_000;
const DAY_MS: i64 = 86_400_000;
const NOTES: u64 = 20;
const MEDIA_FILES: u64 = 5;
const MEDIA_BYTES: u64 = 10_240;
const CSS: &str = ".cloud-qa { font-family: system-ui; padding: 1em; }\n.cloud-answer { margin-top: 1em; }";

/// The full fixed request is retained in the normal CLI generation receipt.
/// Only the seed varies; callers cannot mutate this base's shape or clock.
#[derive(Clone, Debug, Serialize)]
pub struct Request {
    seed: u64,
    now_ms: i64,
    utc_offset_secs: i32,
    rollover_hour: u8,
    notes: u64,
    cards: u64,
    media: MediaMode,
    media_files: u64,
    media_file_bytes: u64,
    reviews_per_card: u8,
    review_days_before_pinned: [u8; 3],
    answer_seconds_after_day_start: u32,
    answer_ordinal_seconds: bool,
}

impl Request {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            now_ms: NOW_MS,
            utc_offset_secs: 0,
            rollover_hour: 4,
            notes: NOTES,
            cards: NOTES,
            media: MediaMode::Files,
            media_files: MEDIA_FILES,
            media_file_bytes: MEDIA_BYTES,
            reviews_per_card: 3,
            review_days_before_pinned: [3, 2, 1],
            answer_seconds_after_day_start: 8 * 3600,
            answer_ordinal_seconds: true,
        }
    }

    /// Synthetic content identity, derived entirely from the request seed.
    pub fn marker(&self) -> String {
        format!("mg-cloud-small-{:016x}", self.seed)
    }

    /// Cache identity; counts and logical hash come from the actual output.
    pub fn manifest(&self) -> Manifest {
        Manifest {
            generator_version: GENERATOR_VERSION,
            kind: "cloud-small".to_owned(),
            name: "cloud-small".to_owned(),
            seed: self.seed,
            cards_requested: NOTES,
            now_ms: NOW_MS,
            utc_offset_secs: 0,
            profile_sha256: None,
            worst_case_sha256: None,
            media: MediaMode::Files,
            counts: Counts::default(),
            dump_sha256: String::new(),
        }
    }
}

fn qa(id: NotetypeId, timeline: &Timeline, rng: &mut Rng) -> Notetype {
    let mut config = NotetypeConfig::default();
    config.css = CSS.to_owned();
    let mut requirement = CardRequirement::default();
    requirement.kind = 1;
    requirement.field_ords = vec![0];
    config.reqs = vec![requirement];
    let fields = ["Q", "A"]
        .into_iter()
        .enumerate()
        .map(|(ord, name)| {
            let mut config = FieldConfig::default();
            config.font_name = "Arial".to_owned();
            config.font_size = 20;
            config.id = Some(rng.u64() as i64);
            NotetypeField {
                ord: ord as u32,
                name: name.to_owned(),
                config: ProtoBlob::new(config),
                prior_ord: None,
            }
        })
        .collect();
    let mut template = TemplateConfig::default();
    template.id = Some(rng.u64() as i64);
    template.q_format = "<section class=\"cloud-qa\">{{Q}}</section>".to_owned();
    template.a_format = "<section class=\"cloud-qa\">{{Q}}<div class=\"cloud-answer\">{{A}}</div></section>".to_owned();
    Notetype {
        id,
        name: "QA".to_owned(),
        mtime: EpochSecs(timeline.crt),
        usn: Usn::PENDING,
        config: ProtoBlob::new(config),
        fields,
        templates: vec![NotetypeTemplate {
            ord: 0,
            name: "QA".to_owned(),
            mtime: EpochSecs(0),
            usn: Usn(0),
            config: ProtoBlob::new(template),
            prior_ord: None,
        }],
    }
}

fn history_shape() -> RevlogShape {
    let mut answer_ms = Histogram::with_edges(vec![8000, 8001]);
    answer_ms.counts = vec![1];
    RevlogShape {
        per_card: Histogram::linear(3, 3, 1),
        kinds: Default::default(),
        ratings: BTreeMap::from([("learning".to_owned(), [0, 1, 0, 0, 0])]),
        answer_ms,
        review_days: Histogram::linear(1, 3, 1),
        hour_of_day: Histogram::linear(12, 12, 1),
        first_review_gap_secs: Histogram::linear(1, 1, 1),
    }
}

fn media_file(request: &Request, index: u64) -> MediaFile {
    MediaFile {
        name: format!("{}-media-{index:02}.png", request.marker()),
        bytes: MEDIA_BYTES,
        kind: MediaKind::Image,
        seed: request.seed,
        index,
    }
}

/// Generates through the shared staging/builder/publication path. A caller
/// with overlapping runs holds `OutputLock`, just as for other generators.
pub fn generate(request: &Request, dir: &Path) -> Result<Manifest, GenError> {
    // Four days leave a creation day before the three exact answer days.
    let timeline = Timeline::new(NOW_MS, 0, 4, 4);
    let mut ids = AllIds::default();
    let mut structure = Rng::stream(request.seed, Stream::Structure, 0);
    let nt_id = NotetypeId(
        ids.notetypes
            .claim(timeline.crt_ms() + structure.below(3_600_000) as i64),
    );
    let nt = qa(nt_id, &timeline, &mut structure);
    let marker = request.marker();
    let parent = DeckId(ids.decks.claim(1));
    let child = DeckId(ids.decks.claim(timeline.crt_ms() + structure.below(3_600_000) as i64));
    let mut out = Output::create(dir, &timeline, &base_config(&timeline), MediaMode::Files, false)?;
    let preset = meguri_engine::deck_config::defaults();
    out.scope(|scope| {
        scope.put(&preset)?;
        // Keep/rename id 1. Deleting it would reset its name (D8).
        scope.put(&super::decks::deck(
            parent,
            &format!("{marker}-D1"),
            DeckConfigId(1),
            &timeline,
            false,
        ))?;
        scope.put(&super::decks::deck(
            child,
            &format!("{marker}-D1::{marker}-D2"),
            DeckConfigId(1),
            &timeline,
            false,
        ))?;
        scope.put_notetype(&nt)?;
        Ok(())
    })?;
    out.counts.presets = 1;
    out.counts.decks = 2;
    out.counts.notetypes = 1;
    let fsrs = Fsrs::new();
    let shape = history_shape();
    for index in 0..NOTES {
        let mut rng = Rng::stream(request.seed, Stream::Note, index);
        let created = timeline.crt_ms() + index as i64 * 1000 + rng.below(1000) as i64;
        let note_id = NoteId(ids.notes.claim(created));
        let media: Vec<MediaFile> = if index < MEDIA_FILES {
            vec![media_file(request, index)]
        } else {
            Vec::new()
        };
        let q = format!("{marker}-note-{index:02}-Q");
        let a = format!(
            "{marker}-note-{index:02}-A{}",
            media.first().map(MediaFile::reference).unwrap_or_default()
        );
        let values = [q, a];
        let tag = format!("{marker}-tag-{index:02}");
        let note = Note {
            id: note_id,
            guid: meguri_text::base91::encode(rng.u64()),
            notetype_id: nt_id,
            mtime: EpochSecs(created.div_euclid(1000)),
            usn: Usn::PENDING,
            tags: format!(" {tag} "),
            fields: values.join(&FIELD_SEPARATOR.to_string()),
            sort_field: sort_text_of(&values, 0).into_owned(),
            checksum: note_checksum(&values[0]),
            flags: 0,
            data: String::new(),
        };
        // Compose exact instants locally; Timeline::instant stays unchanged
        // for legacy requests. Each card/day pair is a distinct ms id.
        let reviews = [3, 2, 1].map(|days| PINNED_DAY_START_MS - days * DAY_MS + (8 * 3600 + index as i64) * 1000);
        let input = CardInput {
            id: CardId(ids.cards.claim(created)),
            note_id,
            deck_id: if index % 2 == 0 { parent } else { child },
            ord: 0,
            position: index as u32 + 1,
            created_ms: created,
            target: Target::Learning,
            reviews: &reviews,
        };
        let (card, revlog) = card_rows(&shape, &timeline, &fsrs, &input, &mut ids.revlog, &mut rng)?;
        out.push(NoteRows {
            note: Some(note),
            cards: vec![card],
            revlog,
            media,
            tags: vec![tag],
        })?;
    }
    out.finish(&closing_config(&timeline, nt_id.0, NOTES), |counts| Manifest {
        counts,
        ..request.manifest()
    })
}

/// Actual bounded readback for the normal CLI/MG receipt, including cache
/// hits. Counts are obtained from persisted rows/references and file bytes,
/// never copied from the request or the manifest's declared counts.
pub fn readback(dir: &Path) -> Result<Counts, GenError> {
    let clock = FixedClock::new(NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(0);
    let collection = meguri_storage::open_collection(&dir.join("collection.anki2"), &OpenOptions::new(&clock, &random))
        .map_err(GenError::open)?;
    let rows = collection.writer();
    let notes = rows.notes_after(None, 21).map_err(GenError::storage)?;
    let cards = rows.cards_after(None, 21).map_err(GenError::storage)?;
    let revlog = rows.revlog_after(None, 61).map_err(GenError::storage)?;
    let mut counts = Counts {
        notes: notes.len() as u64,
        cards: cards.len() as u64,
        revlog: revlog.len() as u64,
        notetypes: rows.row_page(Table::Notetypes, None).map_err(GenError::storage)?.len() as u64,
        decks: rows.deck_rows().map_err(GenError::storage)?.len() as u64,
        presets: rows.deck_config_rows().map_err(GenError::storage)?.len() as u64,
        tags: rows.tag_rows().map_err(GenError::storage)?.len() as u64,
        ..Counts::default()
    };
    let mut referenced = BTreeSet::new();
    for note in &notes {
        for field in note.field_values() {
            let (images, sounds) = super::profile::count_refs(field);
            counts.media_refs += images + sounds;
            // This bounded base writes only one simple PNG reference per
            // selected note; reject another reference form or duplicates.
            if let Some((_, tail)) = field.split_once("<img src=\"") {
                let Some((name, _)) = tail.split_once("\">") else {
                    return Err(GenError::Request("cloud-small media reference readback".to_owned()));
                };
                if !referenced.insert(name.to_owned()) {
                    return Err(GenError::Request("cloud-small duplicate media reference".to_owned()));
                }
            }
        }
    }
    let media_dir = dir.join("collection.media");
    let request = Request::new(Manifest::load(dir)?.seed);
    let expected: BTreeMap<_, _> = (0..MEDIA_FILES)
        .map(|index| {
            let file = media_file(&request, index);
            (file.name.clone(), file)
        })
        .collect();
    for entry in std::fs::read_dir(&media_dir).map_err(|e| GenError::io(&media_dir, e))? {
        let entry = entry.map_err(|e| GenError::io(&media_dir, e))?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|e| GenError::io(&path, e))?;
        if !metadata.is_file()
            || metadata.len() != MEDIA_BYTES
            || !referenced.remove(entry.file_name().to_string_lossy().as_ref())
        {
            return Err(GenError::Request("cloud-small media file readback".to_owned()));
        }
        let name = entry.file_name();
        let Some(file) = expected.get(name.to_string_lossy().as_ref()) else {
            return Err(GenError::Request("cloud-small media name readback".to_owned()));
        };
        let contents = std::fs::read(&path).map_err(|e| GenError::io(&path, e))?;
        if contents != file.contents() {
            return Err(GenError::Request("cloud-small media content readback".to_owned()));
        }
        counts.media_files += 1;
        counts.media_bytes += metadata.len();
    }
    collection.close().map_err(GenError::storage)?;
    if !referenced.is_empty()
        || counts.notes != NOTES
        || counts.cards != NOTES
        || counts.revlog != 60
        || counts.notetypes != 1
        || counts.decks != 2
        || counts.presets != 1
        || counts.tags != NOTES
        || counts.media_refs != MEDIA_FILES
        || counts.media_files != MEDIA_FILES
        || counts.media_bytes != MEDIA_FILES * MEDIA_BYTES
    {
        return Err(GenError::Request("cloud-small row/media count readback".to_owned()));
    }
    Ok(counts)
}
