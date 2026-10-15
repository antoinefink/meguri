//! U11: the synthetic collection generator. Generated collections are
//! reproducible from their seed, follow the profile they were made from,
//! keep ids historical, and the worst cases and UI recipes have their
//! shapes. `profile::measure` is checked first against a collection built
//! by hand, so the later comparisons rest on numbers computed independently
//! of the generator.
//!
//! The oracle checks of the unit (Check Database, deck-tree counts, render)
//! need oracle driver support and are not here.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::deck::{Deck, native_name};
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck};
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::rng::SeededRandom;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::{CollectionBuilder, OpenOptions, ReadRows, StampMode, Table, open_collection};
use meguri_testkit::generate::profile::{self, Histogram, Profile, VOCABULARY};
use meguri_testkit::generate::recipe::{DeckSpec, MediaSpec, Recipe, generate_recipe};
use meguri_testkit::generate::{self, Manifest, MediaMode, Request, WorstCase, media};
use serde_json::Value;

// ---------------------------------------------------------------- helpers

/// 2030-01-15T10:00:00Z, the generator's default instant.
const NOW_MS: i64 = 1_894_701_600_000;

fn utc(_: i64) -> i32 {
    0
}

fn shape() -> Profile {
    generate::user_shape().expect("profiles/user-shape.json loads")
}

fn run(request: &Request) -> (tempfile::TempDir, Manifest) {
    let dir = tempfile::tempdir().unwrap();
    let manifest = generate::generate(request, dir.path()).expect("generation succeeds");
    (dir, manifest)
}

fn measured(dir: &Path, offset_secs: i32) -> Profile {
    let offset = move |_: i64| offset_secs;
    profile::measure(
        &dir.join("collection.anki2"),
        Some(&dir.join("collection.media")),
        &offset,
    )
    .unwrap()
}

fn assert_close(what: &str, distance: Option<f64>, limit: f64) {
    let distance = distance.unwrap_or_else(|| panic!("{what}: histogram edges differ"));
    assert!(distance <= limit, "{what}: distance {distance:.4} > {limit}");
}

/// Shares of cards per note type, in note type order.
fn card_shares(p: &Profile) -> profile::Mix {
    p.notetypes
        .iter()
        .enumerate()
        .map(|(i, nt)| (format!("{i:03}"), nt.cards))
        .collect()
}

/// The `revlog` rows of a collection, per card: count and first id.
fn reviews_by_card(path: &Path) -> HashMap<i64, (u64, i64)> {
    let collection = open(path);
    let rows = collection.writer();
    let mut out: HashMap<i64, (u64, i64)> = HashMap::new();
    let mut after = None;
    loop {
        let page = rows.revlog_after(after, 50_000).unwrap();
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        for e in &page {
            let slot = out.entry(e.card_id.0).or_insert((0, e.id.0));
            slot.0 += 1;
            slot.1 = slot.1.min(e.id.0);
        }
    }
    out
}

fn all_cards(path: &Path) -> Vec<Card> {
    let collection = open(path);
    let rows = collection.writer();
    let mut out = Vec::new();
    let mut after = None;
    loop {
        let page = rows.cards_after(after, 50_000).unwrap();
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        out.extend(page);
    }
    out
}

fn all_notes(path: &Path) -> Vec<Note> {
    let collection = open(path);
    let rows = collection.writer();
    let mut out = Vec::new();
    let mut after = None;
    loop {
        let page = rows.notes_after(after, 50_000).unwrap();
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        out.extend(page);
    }
    out
}

fn open(path: &Path) -> meguri_storage::OpenedCollection {
    let clock = FixedClock::new(NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    open_collection(path, &OpenOptions::new(&clock, &random)).unwrap()
}

fn collection(dir: &Path) -> PathBuf {
    dir.join("collection.anki2")
}

// ------------------------------------------ measure on a hand-built file

const DAY: i64 = 86_400;
/// `col.crt` of the hand-built collection: 2030-01-01T04:00:00Z.
const CRT: i64 = 1_893_470_400;

fn field(ord: u32, name: &str) -> NotetypeField {
    NotetypeField {
        ord,
        name: name.to_owned(),
        config: ProtoBlob::new(FieldConfig::default()),
        prior_ord: None,
    }
}

fn template(q: &str, a: &str) -> NotetypeTemplate {
    let mut config = TemplateConfig::default();
    config.q_format = q.to_owned();
    config.a_format = a.to_owned();
    NotetypeTemplate {
        ord: 0,
        name: "T".to_owned(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        config: ProtoBlob::new(config),
        prior_ord: None,
    }
}

fn hand_notetype(id: i64, cloze: bool, css: &str, q: &str, a: &str) -> Notetype {
    let mut config = NotetypeConfig::default();
    config.kind = i32::from(cloze);
    config.css = css.to_owned();
    Notetype {
        id: NotetypeId(id),
        name: format!("nt{id}"),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        config: ProtoBlob::new(config),
        fields: vec![field(0, "A"), field(1, "B")],
        templates: vec![template(q, a)],
    }
}

fn hand_deck(id: i64, human: &str, preset: i64) -> Deck {
    let mut normal = NormalDeck::default();
    normal.config_id = preset;
    Deck {
        id: DeckId(id),
        name: native_name(human),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        common: ProtoBlob::new(DeckCommon::default()),
        kind: ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))),
    }
}

fn hand_note(id: i64, mid: i64, fields: &[&str], tags: &str) -> Note {
    Note {
        id: NoteId(id),
        guid: format!("g{id}"),
        notetype_id: NotetypeId(mid),
        mtime: EpochSecs(id / 1000),
        usn: Usn(0),
        tags: tags.to_owned(),
        fields: fields.join("\u{1f}"),
        sort_field: fields[0].to_owned(),
        checksum: 0,
        flags: 0,
        data: String::new(),
    }
}

fn hand_card(id: i64, nid: i64, did: i64, ord: u16, card_type: CardType, queue: CardQueue, ivl: u32) -> Card {
    Card {
        id: CardId(id),
        note_id: NoteId(nid),
        deck_id: DeckId(did),
        ord,
        mtime: EpochSecs(id / 1000),
        usn: Usn(0),
        card_type,
        queue,
        due: 0,
        interval: ivl,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: CardData::default(),
    }
}

fn hand_review(id_ms: i64, cid: i64, button: u8, kind: RevlogKind, time: u32) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(id_ms),
        card_id: CardId(cid),
        usn: Usn(0),
        button,
        interval: 1,
        last_interval: 0,
        ease_factor: 2500,
        answer_millis: time,
        kind,
    }
}

#[test]
fn measure_counts_a_hand_built_collection() {
    let dir = tempfile::tempdir().unwrap();
    let path = collection(dir.path());
    let media_dir = dir.path().join("collection.media");
    std::fs::create_dir(&media_dir).unwrap();
    std::fs::write(media_dir.join("a.png"), vec![0u8; 100]).unwrap();
    std::fs::write(media_dir.join("b.mp3"), vec![0u8; 5000]).unwrap();

    let clock = FixedClock::new(CRT * 1000 + 30 * DAY * 1000, UtcOffset::UTC);
    let random = SeededRandom::new(3);
    let col = Col {
        version: 18,
        created: EpochSecs(CRT),
        modified: EpochMillis(CRT * 1000),
        schema_modified: EpochMillis(CRT * 1000),
        ..Col::default()
    };
    let config = vec![
        ConfigEntry {
            key: "schedVer".to_owned(),
            value: b"2".to_vec(),
            ..ConfigEntry::default()
        },
        ConfigEntry {
            key: "rollover".to_owned(),
            value: b"4".to_vec(),
            ..ConfigEntry::default()
        },
    ];
    let builder = CollectionBuilder::create(&path, &col, &config, &OpenOptions::new(&clock, &random)).unwrap();
    // Notes: n1 on day 2, n2 on day 3, n3 on day 9 (never reviewed).
    let (n1, n2, n3) = ((CRT + 2 * DAY) * 1000, (CRT + 3 * DAY) * 1000, (CRT + 9 * DAY) * 1000);
    {
        let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
        for preset in [1, 2] {
            scope
                .put(&DeckConfig {
                    id: DeckConfigId(preset),
                    name: format!("p{preset}"),
                    mtime: EpochSecs(CRT),
                    usn: Usn(0),
                    config: ProtoBlob::new(DeckConfigMessage::default()),
                })
                .unwrap();
        }
        scope.put(&hand_deck(1, "Default", 1)).unwrap();
        scope.put(&hand_deck(10, "Lang", 2)).unwrap();
        scope.put(&hand_deck(11, "Lang::Verbs", 2)).unwrap();
        scope
            .put_notetype(&hand_notetype(
                100,
                false,
                "body {}",
                "{{A}}<script>abc</script>",
                "{{B}}",
            ))
            .unwrap();
        scope
            .put_notetype(&hand_notetype(200, true, "", "{{cloze:A}}", "{{cloze:A}}{{B}}"))
            .unwrap();
        scope
            .put(&hand_note(
                n1,
                100,
                &["hello", "<img src=\"a.png\"> [sound:b.mp3]"],
                " x::y z ",
            ))
            .unwrap();
        scope
            .put(&hand_note(n2, 200, &["{{c1::a}} {{c2::b}}", ""], ""))
            .unwrap();
        scope.put(&hand_note(n3, 100, &["new one", "back"], " Z ")).unwrap();
        // c1 (n1): review; c2, c3 (n2): new, suspended review card; c4 (n3): new.
        scope
            .put(&hand_card(n1, n1, 11, 0, CardType::Review, CardQueue::Review, 10))
            .unwrap();
        scope
            .put(&hand_card(n2, n2, 10, 0, CardType::New, CardQueue::New, 0))
            .unwrap();
        scope
            .put(&hand_card(n2 + 1, n2, 10, 1, CardType::Review, CardQueue::Suspended, 3))
            .unwrap();
        scope
            .put(&hand_card(n3, n3, 1, 0, CardType::New, CardQueue::New, 0))
            .unwrap();
        // Reviews: c1 on day 2 at 07:00:30 and day 12 at 21:15; c3 on day 20 at 09:00.
        let r1 = (CRT + 2 * DAY + 3 * 3600 + 30) * 1000;
        let r2 = (CRT + 12 * DAY + 17 * 3600 + 15 * 60) * 1000;
        let r3 = (CRT + 20 * DAY + 5 * 3600) * 1000;
        scope.put(&hand_review(r1, n1, 3, RevlogKind::Learning, 5000)).unwrap();
        scope.put(&hand_review(r2, n1, 1, RevlogKind::Review, 12_000)).unwrap();
        scope.put(&hand_review(r3, n2 + 1, 3, RevlogKind::Review, 700)).unwrap();
        scope.commit().unwrap();
    }
    builder.finish().unwrap();

    let p = profile::measure(&path, Some(&media_dir), &utc).unwrap();
    let t = &p.totals;
    assert_eq!((t.notes, t.cards, t.revlog, t.notetypes), (3, 4, 3, 2));
    assert_eq!((t.decks, t.filtered_decks, t.presets), (3, 0, 2));
    // `x::y` and `z` (`Z` is the same tag).
    assert_eq!(t.tags, 2);
    assert_eq!((t.media_files, t.media_bytes), (2, 5100));
    // Last activity: the review on day 20.
    assert_eq!(t.span_days, 20);
    assert_eq!(p.rollover_hour, 4);

    let normal = &p.notetypes[0];
    assert_eq!((normal.kind.as_str(), normal.notes, normal.cards), ("normal", 2, 2));
    assert_eq!(normal.template_bytes, vec![[25, 5]]);
    assert_eq!((normal.script_bytes, normal.css_bytes), (3, 7));
    assert_eq!(normal.media_refs, vec![[0, 0], [1, 1]]);
    // Field 0: "hello" (5) and "new one" (7) are both in [4, 8).
    assert_eq!(normal.field_bytes[0].counts[normal.field_bytes[0].bucket(5)], 2);
    assert_eq!(normal.refs_per_note.counts[0..3], [1, 0, 1]);
    assert_eq!(normal.cards_per_note.counts[0], 2);
    let cloze = &p.notetypes[1];
    assert_eq!((cloze.kind.as_str(), cloze.notes, cloze.cards), ("cloze", 1, 2));
    assert_eq!(cloze.cards_per_note.counts[1], 1);

    assert_eq!(p.decks.depth.counts[0..2], [2, 1]);
    assert_eq!(p.decks.decks_per_preset, vec![2, 1]);
    // Cards per deck: Default 1, Lang 2, Lang::Verbs 1.
    assert_eq!(p.decks.cards_per_deck.counts[1..3], [2, 1]);
    assert_eq!(p.tags.depth.counts[0..2], [1, 1]);
    assert_eq!(p.tags.per_note.counts[0..3], [1, 1, 1]);
    assert_eq!(
        p.media.kinds,
        BTreeMap::from([("audio".to_owned(), 1), ("image".to_owned(), 1)])
    );

    let states: Vec<(&str, u64)> = p.cards.states.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(
        states,
        [
            ("buried", 0),
            ("learning", 0),
            ("new", 2),
            ("relearning", 0),
            ("review", 1),
            ("suspended", 1)
        ]
    );
    // Intervals 10 ([8, 16)) and 3 ([2, 4)).
    assert_eq!(p.cards.interval_days.counts[p.cards.interval_days.bucket(10)], 1);
    assert_eq!(p.cards.interval_days.counts[p.cards.interval_days.bucket(3)], 1);

    let r = &p.revlog;
    assert_eq!(r.per_card.counts[0..2], [1, 1]);
    assert_eq!(
        r.kinds,
        BTreeMap::from([("learning".to_owned(), 1), ("review".to_owned(), 2)])
    );
    assert_eq!(r.ratings["learning"], [0, 0, 0, 1, 0]);
    assert_eq!(r.ratings["review"], [0, 1, 0, 1, 0]);
    // Days 2, 12, 20 fall in the 7-day buckets 0, 1, 2.
    assert_eq!(r.review_days.counts[0..3], [1, 1, 1]);
    assert_eq!(r.review_days.edges[0..4], [0, 7, 14, 21]);
    // Hours (UTC): 7, 21, 9.
    for hour in [7, 21, 9] {
        assert_eq!(r.hour_of_day.counts[hour], 1, "hour {hour}");
    }
    assert_eq!(r.hour_of_day.total(), 3);
    // Gaps: c1 3 hours 30 s after n1 (10,830 s, in [2^13, 2^14), bucket
    // 14); c3 17 days 5 hours after n2 (1,486,800 s, in [2^20, 2^21)).
    assert_eq!(r.first_review_gap_secs.bucket(10_830), 14);
    assert_eq!(r.first_review_gap_secs.counts[14], 1);
    assert_eq!(r.first_review_gap_secs.bucket(1_486_800), 21);
    assert_eq!(r.first_review_gap_secs.counts[21], 1);
    assert_eq!(r.first_review_gap_secs.total(), 2);
    assert_eq!(r.answer_ms.counts[r.answer_ms.bucket(700)], 1);
    // Only n3 is unreviewed, created on day 9.
    let unreviewed = &p.notes.unreviewed_creation_days;
    assert_eq!((unreviewed.total(), unreviewed.counts[1]), (1, 1));
}

// ------------------------------------------------------- the user shape

#[test]
fn the_user_shape_has_the_planned_constants() {
    let p = shape();
    assert_eq!(p.totals.presets, 9, "R1804: 9 presets");
    let mean = p.totals.revlog as f64 / p.totals.cards as f64;
    assert!((7.5..8.5).contains(&mean), "about 8 reviews per card, got {mean:.2}");
    let text = std::fs::read_to_string(generate::profiles_dir().join("user-shape.json")).unwrap();
    assert_eq!(text, p.to_json(), "user-shape.json is in its committed form");
    let notes: u64 = p.notetypes.iter().map(|n| n.notes).sum();
    let cards: u64 = p.notetypes.iter().map(|n| n.cards).sum();
    assert_eq!((notes, cards), (p.totals.notes, p.totals.cards));
}

// ------------------------------------------------- reproducibility, 10k

#[test]
fn ten_thousand_cards_are_reproducible_and_follow_the_profile() {
    let p = shape();
    let mut request = Request::new(p.clone(), 10_000, 1);
    let (dir_a, a) = run(&request);
    let (_dir_b, b) = run(&request);
    request.seed = 2;
    let (dir_c, c) = run(&request);
    assert_eq!(a.dump_sha256, b.dump_sha256, "same seed and profile, same rows");
    assert_ne!(a.dump_sha256, c.dump_sha256, "another seed, other rows");

    let ma = measured(dir_a.path(), p.utc_offset_secs);
    let mc = measured(dir_c.path(), p.utc_offset_secs);
    // Another seed: the same shape within sampling tolerance.
    assert_close(
        "seeds: cards per note type",
        Some(profile::mix_distance(&card_shares(&ma), &card_shares(&mc))),
        0.03,
    );
    assert_close(
        "seeds: reviews by day",
        ma.revlog.review_days.distance(&mc.revlog.review_days),
        0.03,
    );
    assert_close(
        "seeds: hour of day",
        ma.revlog.hour_of_day.distance(&mc.revlog.hour_of_day),
        0.03,
    );
    assert_close(
        "seeds: reviews per card",
        ma.revlog.per_card.distance(&mc.revlog.per_card),
        0.03,
    );

    // The profile's aggregates, within 5%.
    assert!(
        (a.counts.cards as f64 - 10_000.0).abs() <= 500.0,
        "about 10k cards, got {}",
        a.counts.cards
    );
    assert_close(
        "cards per note type",
        Some(profile::mix_distance(&card_shares(&ma), &card_shares(&p))),
        0.05,
    );
    let mean = |x: &Profile| x.totals.revlog as f64 / x.totals.cards as f64;
    let relative = (mean(&ma) - mean(&p)).abs() / mean(&p);
    assert!(relative <= 0.05, "reviews per card {:.3} vs {:.3}", mean(&ma), mean(&p));
    assert_eq!(ma.totals.presets, 9);
    assert_close(
        "card states",
        Some(profile::mix_distance(&ma.cards.states, &p.cards.states)),
        0.05,
    );

    // FSRS and the load balancer on, sibling burying on in every preset.
    let collection = open(&collection(dir_a.path()));
    let rows = collection.writer();
    for key in ["fsrs", "loadBalancerEnabled"] {
        let value = rows
            .config_value(key)
            .unwrap()
            .unwrap_or_else(|| panic!("{key} is set"));
        assert_eq!(value.compact(), "true", "{key}");
    }
    let presets = rows.ids_after(Table::DeckConfig, None, 100).unwrap();
    assert_eq!(presets.len(), 9);
    for id in presets {
        let preset = rows.deck_config(DeckConfigId(id)).unwrap().unwrap();
        let c = &preset.config;
        assert!(
            c.bury_new && c.bury_reviews && c.bury_interday_learning,
            "{}: sibling burying",
            preset.name
        );
    }
}

// ----------------------------------------------------- temporal, 100k

#[test]
fn hundred_thousand_cards_follow_the_temporal_shape_and_keep_ids_historical() {
    let p = shape();
    let (dir, manifest) = run(&Request::new(p.clone(), 100_000, 7));
    let m = measured(dir.path(), p.utc_offset_secs);
    assert_close(
        "reviews by day",
        m.revlog.review_days.distance(&p.revlog.review_days),
        0.05,
    );
    assert_close(
        "hour of day",
        m.revlog.hour_of_day.distance(&p.revlog.hour_of_day),
        0.05,
    );
    assert_close(
        "note creation to first review",
        m.revlog.first_review_gap_secs.distance(&p.revlog.first_review_gap_secs),
        0.05,
    );

    let path = collection(dir.path());
    let first_reviews = reviews_by_card(&path);
    let cards = all_cards(&path);
    assert_eq!(cards.len() as u64, manifest.counts.cards);
    let mut reviewed = 0;
    for card in &cards {
        assert!(card.id.0 <= NOW_MS, "card {} is in the future", card.id);
        assert!(card.note_id.0 <= card.id.0, "card {} is older than its note", card.id);
        if let Some(&(count, first)) = first_reviews.get(&card.id.0) {
            reviewed += 1;
            assert!(
                card.id.0 < first,
                "card {} is not older than its first review {first}",
                card.id
            );
            assert_eq!(u64::from(card.reps), count, "card {}: reps", card.id);
        }
    }
    assert!(reviewed > cards.len() / 2);
    let last_review = first_reviews.values().map(|&(_, first)| first).max().unwrap();
    assert!(last_review <= NOW_MS);
    let latest = open(&path)
        .writer()
        .revlog_after(None, usize::MAX)
        .unwrap()
        .last()
        .unwrap()
        .id
        .0;
    assert!(latest <= NOW_MS, "a review in the future: {latest}");
}

// ------------------------------------------------------ profile extract

#[test]
fn an_extracted_profile_holds_no_text_names_or_ids() {
    let private_deck = "Zebrafinch Secrets::Quokka Vault";
    let recipe = Recipe {
        recipe: "private-like".to_owned(),
        description: "distinctive text to look for".to_owned(),
        now: "2030-01-15T10:00:00+01:00".to_owned(),
        rollover_hour: None,
        presets: Vec::new(),
        decks: vec![DeckSpec {
            name: private_deck.to_owned(),
            new: 4,
            due: 6,
            later: 5,
            tags: vec!["hidden::marker".to_owned(), "Wombatique".to_owned()],
            fields: vec!["Xylophonic {n} whisper".to_owned(), "Brontide answer {n}".to_owned()],
            media: Some(MediaSpec {
                images: 1,
                sounds: 0,
                missing: 0,
            }),
            ..DeckSpec::default()
        }],
    };
    let dir = tempfile::tempdir().unwrap();
    generate_recipe(&recipe, 5, dir.path()).unwrap();
    let source = collection(dir.path());
    let before = std::fs::read(&source).unwrap();

    let extracted = profile::extract(&source, Some(&dir.path().join("collection.media")), &utc).unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), before, "the source is only read");
    assert_eq!(extracted.provenance, profile::Provenance::Extracted);
    assert_eq!(extracted.totals.cards, 15);
    let text = extracted.to_json();

    // Every private string the source holds: deck names, tags, note type
    // and field names, field words, media names, ids.
    let mut private: Vec<String> = vec![
        "Zebrafinch".into(),
        "Quokka".into(),
        "Secrets".into(),
        "hidden".into(),
        "marker".into(),
        "Wombatique".into(),
        "Xylophonic".into(),
        "whisper".into(),
        "Brontide".into(),
        "Meguri".into(),
        "Front".into(),
        ".png".into(),
    ];
    let notes = all_notes(&source);
    private.extend(notes.iter().map(|n| n.id.0.to_string()));
    private.extend(notes.iter().map(|n| n.guid.clone()));
    for word in &private {
        assert!(
            !text.to_lowercase().contains(&word.to_lowercase()),
            "the profile holds {word:?}"
        );
    }
    // Strings appear only as fixed keys and vocabulary values.
    fn check(value: &Value) {
        match value {
            Value::String(s) => assert!(VOCABULARY.contains(&s.as_str()), "string value {s:?}"),
            Value::Array(items) => items.iter().for_each(check),
            Value::Object(map) => {
                for (key, item) in map {
                    let fixed = key
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                        && (key.len() <= 24);
                    assert!(fixed, "key {key:?}");
                    check(item);
                }
            }
            _ => {}
        }
    }
    check(&serde_json::from_str::<Value>(&text).unwrap());
}

// ----------------------------------------------------------- worst cases

fn worst(name: &str, cards: Option<u64>) -> (tempfile::TempDir, Manifest, WorstCase) {
    let case = WorstCase::named(name).unwrap();
    let mut request = Request::new(shape(), cards.unwrap_or(case.cards), 11);
    request.worst_case = Some(case.clone());
    let (dir, manifest) = run(&request);
    (dir, manifest, case)
}

#[test]
fn the_committed_worst_cases_are_the_r1805_set() {
    let names = WorstCase::names().unwrap();
    assert_eq!(
        names,
        [
            "decks-2000",
            "due-500k",
            "field-1mb",
            "inline-images",
            "media-30k",
            "tags-10k",
            "template-5mb"
        ]
    );
    assert_eq!(WorstCase::named("due-500k").unwrap().cards, 500_000);
    assert_eq!(WorstCase::named("decks-2000").unwrap().decks, Some(2000));
    assert_eq!(WorstCase::named("tags-10k").unwrap().tag_nodes, Some(10_000));
    assert_eq!(
        WorstCase::named("template-5mb").unwrap().template_bytes,
        Some(5 * 1024 * 1024)
    );
    assert_eq!(
        WorstCase::named("media-30k").unwrap().media_files.unwrap().count,
        30_000
    );
    assert_eq!(
        WorstCase::named("field-1mb").unwrap().large_fields.unwrap().bytes,
        1024 * 1024
    );
}

#[test]
fn worst_case_two_thousand_decks() {
    let (dir, manifest, _) = worst("decks-2000", Some(3000));
    let m = measured(dir.path(), 0);
    assert_eq!((m.totals.decks, manifest.counts.decks), (2000, 2000));
    assert!(m.decks.depth.counts[1] > 0, "nested decks too");
}

#[test]
fn worst_case_ten_thousand_tag_nodes() {
    let (dir, _, _) = worst("tags-10k", Some(4000));
    let mut tags: HashSet<String> = HashSet::new();
    for note in all_notes(&collection(dir.path())) {
        tags.extend(note.tags.split_whitespace().map(str::to_owned));
    }
    assert_eq!(tags.len(), 10_000, "every node of the tree is a tag on a note");
    for tag in &tags {
        if let Some((parent, _)) = tag.rsplit_once("::") {
            assert!(tags.contains(parent), "{tag} has no parent node");
        }
    }
    assert!(tags.iter().any(|t| t.matches("::").count() >= 3), "a deep hierarchy");
}

#[test]
fn worst_case_five_megabyte_template() {
    let (dir, _, case) = worst("template-5mb", Some(2000));
    let collection = open(&collection(dir.path()));
    let rows = collection.writer();
    let first = rows.ids_after(Table::Notetypes, None, 1).unwrap()[0];
    let nt = rows.notetype(NotetypeId(first)).unwrap().unwrap();
    let size = nt.templates[0].config.q_format.len() as u64;
    assert!(size >= case.template_bytes.unwrap(), "front template of {size} bytes");
}

#[test]
fn worst_case_one_megabyte_fields() {
    let (dir, _, case) = worst("field-1mb", Some(2000));
    let wanted = case.large_fields.unwrap();
    let large = all_notes(&collection(dir.path()))
        .iter()
        .filter(|n| n.field_values().any(|f| f.len() as u64 >= wanted.bytes))
        .count() as u64;
    assert_eq!(large, wanted.notes);
}

/// Checks a PNG independently of the generator's encoder: signature, chunk
/// CRCs, IHDR, IEND last, and the zlib stream's stored blocks, length and
/// Adler-32 against the image size.
fn assert_valid_png(bytes: &[u8]) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = !0u32;
        for &b in data {
            c ^= u32::from(b);
            for _ in 0..8 {
                c = (c >> 1) ^ (0xEDB8_8320 & (c & 1).wrapping_neg());
            }
        }
        !c
    }
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut at = 8;
    let (mut width, mut height, mut idat, mut kinds) = (0, 0, Vec::new(), Vec::new());
    while at < bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = &bytes[at + 4..at + 8];
        let data = &bytes[at + 8..at + 8 + len];
        let stored = u32::from_be_bytes(bytes[at + 8 + len..at + 12 + len].try_into().unwrap());
        assert_eq!(
            stored,
            crc(&bytes[at + 4..at + 8 + len]),
            "CRC of {}",
            String::from_utf8_lossy(kind)
        );
        match kind {
            b"IHDR" => {
                width = u32::from_be_bytes(data[0..4].try_into().unwrap()) as usize;
                height = u32::from_be_bytes(data[4..8].try_into().unwrap()) as usize;
                assert_eq!(&data[8..13], &[8, 2, 0, 0, 0]);
            }
            b"IDAT" => idat.extend_from_slice(data),
            _ => {}
        }
        kinds.push(kind.to_vec());
        at += 12 + len;
    }
    assert_eq!(at, bytes.len());
    assert_eq!(kinds.first().map(Vec::as_slice), Some(&b"IHDR"[..]));
    assert_eq!(kinds.last().map(Vec::as_slice), Some(&b"IEND"[..]));
    // zlib: header, stored blocks, Adler-32.
    assert_eq!((u16::from(idat[0]) << 8 | u16::from(idat[1])) % 31, 0);
    let mut raw = Vec::new();
    let mut i = 2;
    loop {
        let last = idat[i] & 1 == 1;
        assert_eq!(idat[i] & 6, 0, "stored block");
        let len = u16::from_le_bytes([idat[i + 1], idat[i + 2]]);
        let nlen = u16::from_le_bytes([idat[i + 3], idat[i + 4]]);
        assert_eq!(len, !nlen);
        raw.extend_from_slice(&idat[i + 5..i + 5 + usize::from(len)]);
        i += 5 + usize::from(len);
        if last {
            break;
        }
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    assert_eq!(&idat[i..i + 4], &((b << 16) | a).to_be_bytes());
    assert_eq!(raw.len(), height * (1 + 3 * width));
    assert!(raw.chunks(1 + 3 * width).all(|row| row[0] <= 4), "filter bytes");
}

#[test]
fn generated_pngs_are_valid_and_sized() {
    for size in [10usize, 80, 1000, 4096, 70_000, 300_000] {
        let mut rng = generate::rng::Rng::stream(3, generate::rng::Stream::Media, size as u64);
        let png = media::png_of_size(size, &mut rng);
        assert_valid_png(&png);
        if size >= 100 {
            assert_eq!(png.len(), size);
        }
    }
}

fn base64_decode(text: &str) -> Vec<u8> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => panic!("not base64: {c}"),
    };
    let clean: Vec<u8> = text.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for group in clean.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in group.iter().enumerate() {
            n |= u32::from(value(c)) << (18 - 6 * i);
        }
        out.extend_from_slice(&n.to_be_bytes()[1..group.len()]);
    }
    out
}

#[test]
fn worst_case_inline_base64_images() {
    let (dir, _, case) = worst("inline-images", Some(1500));
    let inline = case.inline_images.unwrap();
    let mut images = 0;
    for note in all_notes(&collection(dir.path())) {
        for part in note.fields.split("data:image/png;base64,").skip(1) {
            let encoded = part.split('"').next().unwrap();
            assert_valid_png(&base64_decode(encoded));
            images += 1;
        }
    }
    assert_eq!(images, inline.notes * inline.per_note);
}

#[test]
fn worst_case_thirty_thousand_media_files_in_one_folder() {
    let (dir, manifest, case) = worst("media-30k", None);
    let wanted = case.media_files.unwrap().count;
    let media_dir = dir.path().join("collection.media");
    let files: HashSet<String> = std::fs::read_dir(&media_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len() as u64, wanted);
    assert_eq!(manifest.counts.media_files, wanted);
    let mut referenced = 0;
    for note in all_notes(&collection(dir.path())) {
        for f in note.field_values() {
            for name in meguri_text::media_refs::local_references(f) {
                assert!(files.contains(&name), "{name} is referenced but missing");
                referenced += 1;
            }
        }
    }
    assert_eq!(referenced, wanted);
    let png = files.iter().find(|f| f.ends_with(".png")).unwrap();
    assert_valid_png(&std::fs::read(media_dir.join(png)).unwrap());
}

#[test]
fn worst_case_one_deck_all_due() {
    // The 500k case at a smaller size; `tools/mg fixtures due-500k` makes
    // the full one.
    let (dir, manifest, case) = worst("due-500k", Some(5000));
    let path = collection(dir.path());
    let collection = open(&path);
    let rows = collection.writer();
    let deck_ids = rows.ids_after(Table::Decks, None, 10).unwrap();
    assert_eq!(deck_ids.len(), 2, "the default deck and the due deck");
    let due_deck = rows
        .deck(DeckId(*deck_ids.iter().find(|&&d| d != 1).unwrap()))
        .unwrap()
        .unwrap();
    let DeckKind::Normal(normal) = &due_deck.kind.kind else {
        panic!("a normal deck")
    };
    let preset = rows.deck_config(DeckConfigId(normal.config_id)).unwrap().unwrap();
    assert_eq!(preset.config.reviews_per_day, case.due_deck.unwrap().review_limit);
    let today = (NOW_MS / 1000 - rows.col().unwrap().created.0).div_euclid(DAY);
    drop(collection);
    let cards = all_cards(&path);
    assert_eq!(cards.len() as u64, manifest.counts.cards);
    for card in &cards {
        assert_eq!(card.deck_id, due_deck.id);
        assert_eq!((card.card_type, card.queue), (CardType::Review, CardQueue::Review));
        assert!(
            i64::from(card.due) <= today,
            "card {} due {} after today {today}",
            card.id,
            card.due
        );
    }
}

// --------------------------------------------------------------- recipes

#[test]
fn the_recipes_give_their_decks_and_counts() {
    let names = Recipe::names().unwrap();
    for wanted in ["empty", "typical", "overflow", "error-prone", "review-ready"] {
        assert!(names.iter().any(|n| n == wanted), "recipe {wanted}");
    }
    for name in names {
        let recipe = Recipe::named(&name).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let manifest = generate_recipe(&recipe, 1, dir.path()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let (now_ms, _) = recipe.instant().unwrap();
        let path = collection(dir.path());
        let collection = open(&path);
        let rows = collection.writer();
        let crt = rows.col().unwrap().created.0;
        let today = (now_ms / 1000 - crt).div_euclid(DAY);
        let mut decks: HashMap<i64, String> = HashMap::new();
        for id in rows.ids_after(Table::Decks, None, 100_000).unwrap() {
            decks.insert(id, rows.deck(DeckId(id)).unwrap().unwrap().human_name());
        }
        drop(collection);
        // Counts by deck name: new, learning due now, due, later, suspended, buried.
        let mut counts: HashMap<String, [u32; 6]> = HashMap::new();
        for card in all_cards(&path) {
            let slot = match (card.queue, card.card_type) {
                (CardQueue::New, _) => 0,
                (CardQueue::Learning, _) if i64::from(card.due) <= now_ms / 1000 => 1,
                (CardQueue::Review, _) if i64::from(card.due) <= today => 2,
                (CardQueue::Review, _) => 3,
                (CardQueue::Suspended, _) => 4,
                (CardQueue::BuriedByScheduler | CardQueue::BuriedByUser, _) => 5,
                other => panic!("{name}: unexpected card state {other:?}"),
            };
            counts.entry(decks[&card.deck_id.0].clone()).or_default()[slot] += 1;
        }
        let mut cards = 0;
        for spec in &recipe.decks {
            let wanted = [
                spec.new,
                spec.learning,
                spec.due,
                spec.later,
                spec.suspended,
                spec.buried,
            ];
            let got = counts.get(&spec.name).copied().unwrap_or_default();
            assert_eq!(got, wanted, "{name}: deck {:?}", spec.name);
            assert!(
                decks.values().any(|d| d == &spec.name),
                "{name}: deck {:?} exists",
                spec.name
            );
            cards += wanted.iter().sum::<u32>();
        }
        assert_eq!(manifest.counts.cards, u64::from(cards), "{name}: no other cards");
        // Media written are valid; missing ones are not written.
        for entry in std::fs::read_dir(dir.path().join("collection.media")).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "png") {
                assert_valid_png(&std::fs::read(&path).unwrap());
            }
            assert!(!path.to_string_lossy().contains("missing"));
        }
    }
}

#[test]
fn recipes_are_reproducible() {
    let recipe = Recipe::named("typical").unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let ma = generate_recipe(&recipe, 1, a.path()).unwrap();
    let mb = generate_recipe(&recipe, 1, b.path()).unwrap();
    assert_eq!(ma.dump_sha256, mb.dump_sha256);
    assert_eq!(ma, Manifest::load(a.path()).unwrap());
}

#[test]
fn media_files_mode_writes_every_reference() {
    let mut request = Request::new(shape(), 1500, 4);
    request.media = MediaMode::Files;
    let (dir, manifest) = run(&request);
    assert_eq!(manifest.counts.media_files, manifest.counts.media_refs);
    let m = measured(dir.path(), 0);
    assert_eq!(m.totals.media_files, manifest.counts.media_files);
}

// ------------------------------------------------------------ the budget

/// 1M cards in under 10 minutes with a peak resident size under 2 GB
/// (Linux: the process's VmHWM). Run with `--run-ignored only` in a
/// release build; `tools/mg fixtures 1m` measures the same in the CLI.
#[test]
#[ignore = "about 1M cards: run on demand (tools/mg fixtures 1m)"]
fn one_million_cards_within_budget() {
    let watch = meguri_testkit::wallclock::Stopwatch::start();
    let (_dir, manifest) = run(&Request::new(shape(), 1_000_000, 1));
    let secs = watch.elapsed_ms() / 1000;
    assert!(manifest.counts.cards >= 950_000);
    assert!(secs < 600, "took {secs} s");
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let kb: u64 = status
            .lines()
            .find_map(|l| l.strip_prefix("VmHWM:"))
            .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
            .unwrap();
        assert!(kb < 2 * 1024 * 1024, "peak {kb} kB");
    }
}

#[test]
fn histogram_sampling_follows_the_counts() {
    let mut h = Histogram::with_edges(vec![0, 10, 20, 40]);
    h.counts = vec![1, 0, 3];
    let mut rng = generate::rng::Rng::stream(1, generate::rng::Stream::Structure, 0);
    let mut drawn = Histogram::with_edges(vec![0, 10, 20, 40]);
    for _ in 0..40_000 {
        let v = h.sample(&mut rng).unwrap();
        assert!((0..10).contains(&v) || (20..40).contains(&v));
        drawn.add(v);
    }
    assert_close("sampling", h.distance(&drawn), 0.01);
}
