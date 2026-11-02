//! Q4.2/J2.3 real generated-row evidence. Expected history and FSRS values
//! are checked independently of the generator's card_rows implementation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use fsrs::{FSRS, FSRSItem, FSRSReview};
use meguri_model::card::{CardQueue, CardType};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::deck::human_name;
use meguri_model::ids::DeckId;
use meguri_model::proto::deck::DeckKind;
use meguri_model::revlog::RevlogKind;
use meguri_model::rng::SeededRandom;
use meguri_storage::{OpenOptions, ReadRows, Record, Table};
use meguri_testkit::generate::cloud_small::{self, Request};
use meguri_testkit::generate::{self, Manifest, MediaMode};

const NOW: i64 = 1_735_689_600_000;
const DAY_START: i64 = 1_735_617_600_000;
const FIRST_NOON: i64 = 1_735_387_200_000; // December 28, 2024, noon UTC
const DAY: i64 = 86_400_000;

fn media(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir.join("collection.media"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[test]
fn small_is_the_complete_marked_base_with_computed_learning_histories() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("base");
    let request = Request::new(73);
    let manifest = cloud_small::generate(&request, &dir).unwrap();
    let request_json = serde_json::to_value(&request).unwrap();
    assert_eq!(request_json["seed"], 73);
    assert_eq!(request_json["now_ms"], NOW);
    assert_eq!(request_json["utc_offset_secs"], 0);
    assert_eq!(request_json["rollover_hour"], 4);
    assert_eq!(request_json["media"], "files");
    assert_eq!(request_json["media_file_bytes"], 10240);
    assert_eq!(manifest.kind, "cloud-small");
    assert_eq!(manifest.name, "cloud-small");
    assert_eq!(manifest.seed, 73);
    assert_eq!(manifest.cards_requested, 20);
    assert_eq!(manifest.now_ms, NOW);
    assert_eq!(manifest.utc_offset_secs, 0);
    assert_eq!(manifest.media, MediaMode::Files);
    assert_eq!(manifest.profile_sha256, None);
    assert_eq!(manifest.worst_case_sha256, None);
    assert_eq!(Manifest::load(&dir).unwrap(), manifest);
    assert_eq!(
        generate::dump::logical_sha256(&dir.join("collection.anki2")).unwrap(),
        manifest.dump_sha256
    );
    let observed = cloud_small::readback(&dir).unwrap();
    assert_eq!(observed, manifest.counts);
    assert_eq!((observed.notes, observed.cards, observed.revlog), (20, 20, 60));
    assert_eq!(
        (observed.notetypes, observed.decks, observed.presets, observed.tags),
        (1, 2, 1, 20)
    );
    assert_eq!(
        (observed.media_files, observed.media_refs, observed.media_bytes),
        (5, 5, 51200)
    );

    let clock = FixedClock::new(NOW, UtcOffset::UTC);
    let rng = SeededRandom::new(73);
    let collection =
        meguri_storage::open_collection(&dir.join("collection.anki2"), &OpenOptions::new(&clock, &rng)).unwrap();
    let rows = collection.writer();
    let col = rows.col().unwrap();
    assert_eq!(col.created.0, DAY_START / 1000 - 4 * 86400);
    assert_eq!(col.modified.0, NOW);
    for (key, expected) in [
        ("rollover", "4"),
        ("creationOffset", "0"),
        ("nextPos", "21"),
        ("fsrs", "true"),
    ] {
        let entry = rows.config_entry(key).unwrap().unwrap();
        assert_eq!(entry.entry.value, expected.as_bytes());
    }
    let marker = request.marker();
    let decks = rows.deck_rows().unwrap();
    assert_eq!(decks.len(), 2);
    let parent = decks.iter().find(|d| d.id == DeckId(1)).unwrap();
    let child = decks.iter().find(|d| d.id != DeckId(1)).unwrap();
    assert!(human_name(&parent.name) == format!("{marker}-D1"));
    assert!(human_name(&child.name) == format!("{marker}-D1::{marker}-D2"));
    for deck in &decks {
        assert!(matches!(&deck.kind.kind, DeckKind::Normal(n) if n.config_id == 1));
        assert_eq!(deck.common.new_studied, 0);
        assert_eq!(deck.common.review_studied, 0);
        assert_eq!(deck.common.learning_studied, 0);
        assert_eq!(deck.common.milliseconds_studied, 0);
    }
    let presets = rows.deck_config_rows().unwrap();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].to_cells(), meguri_engine::deck_config::defaults().to_cells());
    assert_eq!(rows.row_page(Table::Notetypes, None).unwrap().len(), 1);
    let notes = rows.notes_after(None, 100).unwrap();
    let cards = rows.cards_after(None, 100).unwrap();
    let history = rows.revlog_after(None, 100).unwrap();
    assert_eq!((notes.len(), cards.len(), history.len()), (20, 20, 60));
    let nt = rows.notetype(notes[0].notetype_id).unwrap().unwrap();
    assert_eq!(nt.name, "QA");
    assert_eq!(
        nt.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["Q", "A"]
    );
    assert_eq!(nt.templates.len(), 1);
    let template = &nt.templates[0];
    assert_eq!(template.ord, 0);
    assert!(template.config.q_format.contains("{{Q}}"));
    assert!(template.config.a_format.contains("{{A}}"));
    assert!(template.config.q_format.contains("cloud-qa"));
    assert!(nt.config.css.contains(".cloud-qa"));
    assert!(nt.config.css.contains(".cloud-answer"));
    assert_eq!(nt.config.original_stock_kind, 0);

    let files = media(&dir);
    assert_eq!(files.len(), 5);
    assert!(files.values().all(|bytes| bytes.len() == 10240));
    assert_eq!(
        files.values().collect::<BTreeSet<_>>().len(),
        5,
        "distinct actual bytes"
    );
    let mut field_values = BTreeSet::new();
    let mut tags = BTreeSet::new();
    let mut references = BTreeSet::new();
    let mut history_ids = BTreeSet::new();
    let mut buckets = BTreeMap::new();
    let fsrs = FSRS::new(&[]).unwrap();
    let expected = fsrs
        .historical_memory_states(
            FSRSItem {
                reviews: vec![
                    FSRSReview { rating: 1, delta_t: 0 },
                    FSRSReview { rating: 1, delta_t: 1 },
                    FSRSReview { rating: 1, delta_t: 1 },
                ],
            },
            None,
        )
        .unwrap();
    for (i, note) in notes.iter().enumerate() {
        assert_eq!(note.notetype_id, nt.id);
        let values: Vec<_> = note.field_values().collect();
        assert_eq!(values.len(), 2);
        for (side, value) in ["Q", "A"].iter().zip(&values) {
            assert!(value.starts_with(&format!("{marker}-note-{i:02}-{side}")));
            assert!(field_values.insert(*value));
        }
        let tag = note.tags.trim();
        assert!(tag == format!("{marker}-tag-{i:02}"));
        assert!(tags.insert(tag));
        let refs: u64 = values
            .iter()
            .map(|v| {
                let (images, sounds) = generate::profile::count_refs(v);
                images + sounds
            })
            .sum();
        assert_eq!(refs, u64::from(i < 5));
        if i < 5 {
            let filename = format!("{marker}-media-{i:02}.png");
            assert!(values[1].ends_with(&format!("<img src=\"{filename}\">")));
            assert!(files.contains_key(&filename));
            assert!(references.insert(filename));
        }
        let note_cards: Vec<_> = cards.iter().filter(|c| c.note_id == note.id).collect();
        assert_eq!(note_cards.len(), 1);
        let card = note_cards[0];
        assert_eq!(card.deck_id, if i % 2 == 0 { parent.id } else { child.id });
        assert_eq!(card.ord, 0);
        assert_eq!((card.card_type, card.queue), (CardType::Learning, CardQueue::Learning));
        assert_eq!((card.remaining_steps, card.reps, card.lapses), (2, 3, 0));
        assert_eq!(
            (
                card.interval,
                card.ease_factor,
                card.original_due,
                card.original_deck_id.0,
                card.flags
            ),
            (0, 0, 0, 0, 0)
        );
        let entries: Vec<_> = history.iter().filter(|r| r.card_id == card.id).collect();
        assert_eq!(entries.len(), 3);
        for (day, entry) in entries.iter().enumerate() {
            let instant = FIRST_NOON + day as i64 * DAY + i as i64 * 1000;
            assert_eq!(entry.id.0, instant);
            assert!(entry.id.0 > note.id.0 && entry.id.0 > card.id.0);
            assert!(entry.id.0 < DAY_START);
            assert!(history_ids.insert(entry.id));
            let bucket = (entry.id.0 - (DAY_START - 3 * DAY)).div_euclid(DAY);
            *buckets.entry(bucket).or_insert(0) += 1;
            assert_eq!(
                (entry.button, entry.kind, entry.interval),
                (1, RevlogKind::Learning, -60)
            );
            assert_eq!(entry.last_interval, if day == 0 { 0 } else { -60 });
            assert!(entry.answer_millis <= 60_000);
            let n: f32 = (expected[day].difficulty - 1.0) / 9.0;
            let s: f32 = n + 0.1;
            assert_eq!(entry.ease_factor, (s * 1000.0).round() as u32);
        }
        let last = (FIRST_NOON + 2 * DAY) / 1000 + i as i64;
        assert_eq!(i64::from(card.due), last + 60);
        assert_eq!(card.mtime.0, last);
        assert_eq!(card.data.last_review_secs, Some(last));
        assert_eq!(card.data.new_position, Some(i as u32 + 1));
        assert_eq!(card.data.desired_retention, Some(0.9));
        assert_eq!(card.data.decay, Some(0.154));
        let memory = card.data.memory_state.unwrap();
        assert!((memory.stability - expected[2].stability).abs() <= 1e-4 + expected[2].stability * 1e-6);
        assert!((memory.difficulty - expected[2].difficulty).abs() <= 1e-3);
    }
    assert!(references == files.keys().cloned().collect());
    assert_eq!(history_ids.len(), 60);
    assert_eq!(buckets, BTreeMap::from([(0, 20), (1, 20), (2, 20)]));
    assert_eq!(tags.len(), rows.tag_rows().unwrap().len());
    collection.close().unwrap();
}

#[test]
fn repeated_generation_matches_manifest_logical_dump_and_every_media_byte() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    let other = temp.path().join("other");
    let request = Request::new(91);
    let a = cloud_small::generate(&request, &first).unwrap();
    let b = cloud_small::generate(&request, &second).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        std::fs::read(first.join("manifest.json")).unwrap(),
        std::fs::read(second.join("manifest.json")).unwrap()
    );
    assert_eq!(
        generate::dump::logical_sha256(&first.join("collection.anki2")).unwrap(),
        a.dump_sha256
    );
    assert_eq!(
        generate::dump::logical_sha256(&second.join("collection.anki2")).unwrap(),
        b.dump_sha256
    );
    assert!(media(&first) == media(&second), "same-seed media bytes");
    let c = cloud_small::generate(&Request::new(92), &other).unwrap();
    assert_ne!(a.dump_sha256, c.dump_sha256);
    assert!(media(&first) != media(&other), "different-seed media bytes");
    // Replacement uses the same atomic path; no old media survive.
    assert_eq!(cloud_small::generate(&request, &other).unwrap(), a);
    assert!(media(&first) == media(&other), "replacement media bytes");
    // Real readback refuses missing files instead of repeating requested counts.
    let file = std::fs::read_dir(other.join("collection.media"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::remove_file(file).unwrap();
    assert!(cloud_small::readback(&other).is_err());
}

#[test]
fn legacy_requests_keep_the_2030_clock_and_generation_version() {
    let request = generate::Request::new(generate::user_shape().unwrap(), 20, 91);
    assert_eq!(request.now_ms, 1_894_701_600_000);
    assert_eq!(generate::GENERATOR_VERSION, 5);
    assert_eq!(request.media, MediaMode::References);
    let temp = tempfile::tempdir().unwrap();
    let manifest = generate::generate(&request, &temp.path().join("legacy")).unwrap();
    assert_eq!(manifest.now_ms, request.now_ms);
    assert_eq!(manifest.kind, "profile");
    assert_eq!(manifest.media, MediaMode::References);
}
