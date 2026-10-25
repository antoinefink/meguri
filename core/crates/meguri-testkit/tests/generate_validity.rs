//! U11: the synthetic collection generator. Generated collections are
//! reproducible from their seed, follow the profile they were made from,
//! keep ids historical, and the worst cases and UI recipes have their
//! shapes. `profile::measure` is checked first against a collection built
//! by hand, so the later comparisons rest on numbers computed independently
//! of the generator.
//!
//! The histories are replayed through the answer table written out from
//! the spec (`fixtures/synthetic-collections` F5) and fsrs itself, so the
//! card and review-log columns are checked against values the generator did
//! not compute. The oracle checks of the unit (F11 T1: Check Database,
//! deck-list counts, render) run in `tools/mg fixtures-check`.

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
use meguri_storage::raw::SqlValue;
use meguri_storage::{CollectionBuilder, OpenOptions, ReadRows, StampMode, Table, open_collection};
use meguri_testkit::generate::profile::{self, Histogram, Profile, VOCABULARY};
use meguri_testkit::generate::recipe::{DeckSpec, MediaSpec, Recipe, generate_recipe};
use meguri_testkit::generate::{self, GenError, Manifest, MediaMode, Request, WorstCase, media};
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

/// Measured for a comparison with `reference` (F2.5): the day histograms
/// get the reference's span.
fn measured_against(dir: &Path, reference: &Profile) -> Profile {
    let offset_secs = reference.utc_offset_secs;
    let offset = move |_: i64| offset_secs;
    profile::measure_against(
        &dir.join("collection.anki2"),
        Some(&dir.join("collection.media")),
        &offset,
        reference,
    )
    .unwrap()
}

/// Every row of one raw query.
fn query(path: &Path, sql: &str) -> Vec<Vec<SqlValue>> {
    open(path).writer().execute_raw(sql, &[]).unwrap().rows
}

fn int(value: &SqlValue) -> i64 {
    match value {
        SqlValue::Integer(v) => *v,
        other => panic!("not an integer: {other:?}"),
    }
}

fn text(value: &SqlValue) -> String {
    match value {
        SqlValue::Text(v) => v.clone(),
        SqlValue::Blob(v) => String::from_utf8(v.clone()).unwrap(),
        other => panic!("not text: {other:?}"),
    }
}

fn all_revlog(path: &Path) -> Vec<RevlogEntry> {
    let collection = open(path);
    let rows = collection.writer();
    let mut out = Vec::new();
    let mut after = None;
    loop {
        let page = rows.revlog_after(after, 50_000).unwrap();
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        out.extend(page);
    }
    out
}

fn assert_close(what: &str, distance: Option<f64>, limit: f64) {
    let distance = distance.unwrap_or_else(|| panic!("{what}: histogram edges differ"));
    assert!(distance <= limit, "{what}: distance {distance:.4} > {limit}");
}

/// Total variation distance between two weightings of the same buckets:
/// half the sum of the share differences (F3). A weighting whose total is
/// 0 has every share 0.
fn variation_distance(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len(), "the same buckets");
    let total = |w: &[f64]| w.iter().sum::<f64>();
    let share = |x: f64, t: f64| if t == 0.0 { 0.0 } else { x / t };
    let (ta, tb) = (total(a), total(b));
    a.iter()
        .zip(b)
        .map(|(&x, &y)| (share(x, ta) - share(y, tb)).abs())
        .sum::<f64>()
        / 2.0
}

fn as_f64(counts: &[u64]) -> Vec<f64> {
    counts.iter().map(|&c| c as f64).collect()
}

/// F3.1: the lengths a generated field is compared with: its profile
/// `field_bytes`, with a normal note type's 0-byte first fields counted in
/// the bucket holding 1 (F4.8 writes them as one letter).
fn expected_field_bytes(shape: &profile::NotetypeShape, field: usize) -> Vec<f64> {
    let lengths = &shape.field_bytes[field];
    let mut counts = as_f64(&lengths.counts);
    if field == 0 && shape.kind == "normal" {
        let one = lengths
            .edges
            .windows(2)
            .position(|w| w[0] <= 1 && 1 < w[1])
            .expect("a bucket holds 1");
        counts[one] += std::mem::take(&mut counts[0]);
    }
    counts
}

/// F3.1: generated field lengths follow the profile, pooled over every
/// field of every note type (each field's shares weighted by its note
/// type's notes) and per field of every note type with at least 2,000
/// generated notes. Returns how many fields were compared one by one.
fn assert_field_lengths(what: &str, measured: &Profile, p: &Profile) -> usize {
    assert_eq!(measured.notetypes.len(), p.notetypes.len(), "{what}: note types");
    let buckets = p.notetypes[0].field_bytes[0].counts.len();
    let (mut pooled_measured, mut pooled_expected) = (vec![0.0; buckets], vec![0.0; buckets]);
    let mut per_field = Vec::new();
    for (i, (generated, shape)) in measured.notetypes.iter().zip(&p.notetypes).enumerate() {
        assert_eq!(generated.fields, shape.fields, "{what}: note type {i} fields");
        for f in 0..shape.fields as usize {
            assert_eq!(generated.field_bytes[f].edges, shape.field_bytes[f].edges);
            let got = as_f64(&generated.field_bytes[f].counts);
            let expected = expected_field_bytes(shape, f);
            let weigh = |slots: &mut [f64], counts: &[f64], notes: u64| {
                let total: f64 = counts.iter().sum();
                if total > 0.0 {
                    for (slot, c) in slots.iter_mut().zip(counts) {
                        *slot += c / total * notes as f64;
                    }
                }
            };
            weigh(&mut pooled_measured, &got, generated.notes);
            weigh(&mut pooled_expected, &expected, shape.notes);
            if generated.notes >= 2000 {
                per_field.push((i, f, variation_distance(&got, &expected)));
            }
        }
    }
    let pooled = variation_distance(&pooled_measured, &pooled_expected);
    let worst = per_field
        .iter()
        .fold((0, 0, 0.0f64), |w, &d| if d.2 > w.2 { d } else { w });
    eprintln!(
        "{what}: field lengths pooled {pooled:.4}; {} fields compared, the farthest note type {} field {} at {:.4}",
        per_field.len(),
        worst.0,
        worst.1,
        worst.2
    );
    assert!(
        pooled <= 0.05,
        "{what}: pooled field lengths at distance {pooled:.4} > 0.05"
    );
    for (i, f, d) in &per_field {
        assert!(
            *d <= 0.05,
            "{what}: note type {i} field {f} lengths at distance {d:.4} > 0.05"
        );
    }
    per_field.len()
}

/// References per note of a whole profile (F3.2's mean): every field's
/// `media_refs`, images and sounds, of every note type, over
/// `totals.notes`.
fn references_per_note(x: &Profile) -> f64 {
    let refs: u64 = x
        .notetypes
        .iter()
        .flat_map(|nt| &nt.media_refs)
        .map(|[images, sounds]| images + sounds)
        .sum();
    refs as f64 / x.totals.notes as f64
}

/// F3.2: generated references per note follow the profile, pooled over
/// every note type (the profile's shares weighted by its notes, against
/// the generated counts summed), per note type with at least 2,000
/// generated notes, and as a mean. Returns how many note types were
/// compared one by one.
fn assert_references_per_note(what: &str, measured: &Profile, p: &Profile) -> usize {
    assert_eq!(measured.notetypes.len(), p.notetypes.len(), "{what}: note types");
    let buckets = p.notetypes[0].refs_per_note.counts.len();
    let (mut pooled_measured, mut pooled_expected) = (vec![0.0; buckets], vec![0.0; buckets]);
    let mut per_type = Vec::new();
    for (i, (generated, shape)) in measured.notetypes.iter().zip(&p.notetypes).enumerate() {
        assert_eq!(generated.refs_per_note.edges, shape.refs_per_note.edges);
        for (slot, &c) in pooled_measured.iter_mut().zip(&generated.refs_per_note.counts) {
            *slot += c as f64;
        }
        for (slot, share) in pooled_expected.iter_mut().zip(shape.refs_per_note.shares()) {
            *slot += share * shape.notes as f64;
        }
        if generated.notes >= 2000 {
            let d = generated.refs_per_note.distance(&shape.refs_per_note).unwrap();
            per_type.push((i, d));
        }
    }
    let pooled = variation_distance(&pooled_measured, &pooled_expected);
    let (got, want) = (references_per_note(measured), references_per_note(p));
    let relative = (got - want).abs() / want;
    let worst = per_type.iter().fold((0, 0.0f64), |w, &d| if d.1 > w.1 { d } else { w });
    eprintln!(
        "{what}: references per note pooled {pooled:.4}, mean {got:.4} vs {want:.4} ({relative:.4}); {} note types compared, the farthest note type {} at {:.4}",
        per_type.len(),
        worst.0,
        worst.1
    );
    assert!(
        pooled <= 0.05,
        "{what}: pooled references per note at distance {pooled:.4} > 0.05"
    );
    assert!(
        relative <= 0.05,
        "{what}: references per note {got:.4} vs the profile's {want:.4}, {relative:.4} > 5%"
    );
    for (i, d) in &per_type {
        assert!(
            *d <= 0.05,
            "{what}: note type {i} references per note at distance {d:.4} > 0.05"
        );
    }
    per_type.len()
}

/// A media reference in a field: `<img src="name">` or `[sound:name]`.
#[derive(Debug)]
struct FieldRef {
    start: usize,
    end: usize,
    name: String,
    image: bool,
}

/// The references the generator writes, in field order.
fn references_in(field: &str) -> Vec<FieldRef> {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let img = field[at..].find("<img src=\"").map(|i| (at + i, true));
        let sound = field[at..].find("[sound:").map(|i| (at + i, false));
        let (start, image) = match (img, sound) {
            (Some(a), Some(b)) => {
                if a.0 < b.0 {
                    a
                } else {
                    b
                }
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        let (open, close) = if image {
            ("<img src=\"", "\">")
        } else {
            ("[sound:", "]")
        };
        let name_at = start + open.len();
        let name_end = name_at + field[name_at..].find(close).expect("a closed reference");
        out.push(FieldRef {
            start,
            end: name_end + close.len(),
            name: field[name_at..name_end].to_owned(),
            image,
        });
        at = name_end + close.len();
    }
    out
}

fn is_words(text: &str) -> bool {
    text.bytes().all(|b| b.is_ascii_lowercase() || b == b' ')
}

/// F4.8 on the stored notes of a profile collection (not the `media_files`
/// worst case): a normal front is never empty; a cloze first field starts
/// with its deletions c1 to cN (N its cards, answers of 3 to 14 bytes, or
/// all of 1 byte); references sit only in fields the profile gives
/// references, after the field's words, one space apart, and are numbered
/// from 0 across the note in field order; the rest is words. Returns the
/// references seen.
fn check_note_fields(path: &Path, p: &Profile) -> usize {
    let collection = open(path);
    let types = collection.writer().ids_after(Table::Notetypes, None, 1000).unwrap();
    drop(collection);
    let index_of: HashMap<i64, usize> = types.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut cards_of: HashMap<i64, usize> = HashMap::new();
    for card in all_cards(path) {
        *cards_of.entry(card.note_id.0).or_default() += 1;
    }
    let mut references = 0;
    for note in all_notes(path) {
        let id = note.id;
        let shape = &p.notetypes[index_of[&note.notetype_id.0]];
        let values: Vec<&str> = note.field_values().collect();
        assert_eq!(values.len() as u64, shape.fields, "note {id}");
        let mut numbers = Vec::new();
        let mut prefix: Option<String> = None;
        for (f, value) in values.iter().enumerate() {
            let shown: String = value.chars().take(160).collect();
            let at = format!("note {id}, field {f}: {shown:?}");
            let mut rest: &str = value;
            if f == 0 && shape.kind == "normal" {
                assert!(!value.is_empty(), "{at}: an empty front");
            }
            if f == 0 && shape.kind == "cloze" {
                let cards = cards_of[&id.0];
                let mut answers = Vec::new();
                for k in 1..=cards {
                    let open = format!("{{{{c{k}::");
                    rest = rest.strip_prefix(open.as_str()).unwrap_or_else(|| panic!("{at}: c{k}"));
                    let close = rest.find("}}").unwrap_or_else(|| panic!("{at}: c{k} closes"));
                    assert!(is_words(&rest[..close]), "{at}: answer {k}");
                    answers.push(close);
                    rest = &rest[close + 2..];
                    if k < cards {
                        rest = rest
                            .strip_prefix(' ')
                            .unwrap_or_else(|| panic!("{at}: a space after c{k}"));
                    }
                }
                assert!(
                    answers.iter().all(|&a| a == 1) || answers.iter().all(|a| (3..=14).contains(a)),
                    "{at}: answers {answers:?}"
                );
                assert!(!rest.contains("{{c"), "{at}: more deletions");
                rest = rest.strip_prefix(' ').unwrap_or(rest);
            }
            let refs = references_in(rest);
            if let Some(first) = refs.first() {
                let [images, sounds] = shape.media_refs[f];
                assert!(images + sounds > 0, "{at}: a reference in a field without references");
                let words = &rest[..first.start];
                assert!(
                    words.is_empty() || words.ends_with(' ') && is_words(words),
                    "{at}: words, then a space, before the references"
                );
                let tail: Vec<&str> = refs.iter().map(|r| &rest[r.start..r.end]).collect();
                assert_eq!(
                    &rest[first.start..],
                    tail.join(" "),
                    "{at}: references last, one space apart"
                );
            } else {
                assert!(is_words(rest), "{at}: words");
            }
            for r in &refs {
                let ext = if r.image { ".png" } else { ".mp3" };
                let stem = r
                    .name
                    .strip_suffix(ext)
                    .unwrap_or_else(|| panic!("{at}: {} ends {ext}", r.name));
                let (head, n) = stem.rsplit_once('-').unwrap_or_else(|| panic!("{at}: {}", r.name));
                let head = head.strip_prefix("mg").unwrap_or_else(|| panic!("{at}: {}", r.name));
                assert!(head.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
                assert_eq!(
                    prefix.get_or_insert_with(|| head.to_owned()),
                    head,
                    "{at}: one note index"
                );
                numbers.push(n.parse::<usize>().unwrap());
            }
        }
        assert_eq!(
            numbers,
            (0..numbers.len()).collect::<Vec<_>>(),
            "note {id}: reference numbers"
        );
        references += numbers.len();
    }
    references
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
    // Notes: n1 on day 2, n2 on day 3, n3 on day 9 (never reviewed); n4 on
    // day 5 of a note type that does not exist (totals only), with a card;
    // n5 on day 6, without cards (none of its cards has a row).
    let (n1, n2, n3) = ((CRT + 2 * DAY) * 1000, (CRT + 3 * DAY) * 1000, (CRT + 9 * DAY) * 1000);
    let (n4, n5) = ((CRT + 5 * DAY) * 1000, (CRT + 6 * DAY) * 1000);
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
                " x::y z STRASSE ",
            ))
            .unwrap();
        scope
            .put(&hand_note(n2, 200, &["{{c1::a}} {{c2::b}}", ""], ""))
            .unwrap();
        // `Z` is `z`, and `Straße` is `STRASSE` under unicase (lower-casing
        // alone would make them two tags).
        scope
            .put(&hand_note(n3, 100, &["new one", "back"], " Z Straße "))
            .unwrap();
        scope.put(&hand_note(n4, 999, &["lost", "type"], " orphan ")).unwrap();
        scope.put(&hand_note(n5, 100, &["solo", ""], "")).unwrap();
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
        scope
            .put(&hand_card(n4, n4, 1, 0, CardType::New, CardQueue::New, 0))
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
    assert_eq!((t.notes, t.cards, t.revlog, t.notetypes), (5, 5, 3, 2));
    assert_eq!((t.decks, t.filtered_decks, t.presets), (3, 0, 2));
    // `x::y`, `z` and `straße` (n4's `orphan` is on a note without a type).
    assert_eq!(t.tags, 3);
    assert_eq!((t.media_files, t.media_bytes), (2, 5100));
    // Last activity: the review on day 20.
    assert_eq!(t.span_days, 20);
    assert_eq!(p.rollover_hour, 4);

    let normal = &p.notetypes[0];
    assert_eq!((normal.kind.as_str(), normal.notes, normal.cards), ("normal", 3, 2));
    assert_eq!(normal.template_bytes, vec![[25, 5]]);
    assert_eq!((normal.script_bytes, normal.css_bytes), (3, 7));
    assert_eq!(normal.media_refs, vec![[0, 0], [1, 1]]);
    // Field 0: "hello" (5), "new one" (7) and "solo" (4) are in [4, 8).
    assert_eq!(normal.field_bytes[0].counts[normal.field_bytes[0].bucket(5)], 3);
    // References per note: n3 and n5 none, n1 two.
    assert_eq!(normal.refs_per_note.counts[0..3], [2, 0, 1]);
    // Cards per note: n1 and n3 one each; n5 has none and is not counted.
    assert_eq!(normal.cards_per_note.counts[0], 2);
    assert_eq!(normal.cards_per_note.total(), 2);
    let cloze = &p.notetypes[1];
    assert_eq!((cloze.kind.as_str(), cloze.notes, cloze.cards), ("cloze", 1, 2));
    assert_eq!(cloze.cards_per_note.counts[1], 1);

    assert_eq!(p.decks.depth.counts[0..2], [2, 1]);
    assert_eq!(p.decks.decks_per_preset, vec![2, 1]);
    // Cards per deck: Default 2 (c4, n4's card), Lang 2, Lang::Verbs 1.
    assert_eq!(p.decks.cards_per_deck.counts[1..3], [1, 2]);
    // Depth 1: `z`, `straße`; depth 2: `x::y`.
    assert_eq!(p.tags.depth.counts[0..2], [2, 1]);
    // Tags per note: n2 and n5 none, n3 two, n1 three (n4 not measured).
    assert_eq!(p.tags.per_note.counts[0..4], [2, 0, 1, 1]);
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
            ("new", 3),
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
    // Unreviewed: n5 (day 6) and n3 (day 9); not n4, whose type is missing.
    let unreviewed = &p.notes.unreviewed_creation_days;
    assert_eq!(unreviewed.counts[0..2], [1, 1]);
    assert_eq!(unreviewed.total(), 2);
    assert_eq!(p.utc_offset_secs, 0);

    // F2.5: against a reference whose span is 30 days, the day histograms
    // take its edges, 0 to 35 by 7, and keep their counts; the measured
    // span stays 20.
    let mut reference = p.clone();
    reference.totals.span_days = 30;
    let against = profile::measure_against(&path, Some(&media_dir), &utc, &reference).unwrap();
    assert_eq!(against.totals.span_days, 20);
    assert_eq!(against.revlog.review_days.edges, [0, 7, 14, 21, 28, 35]);
    assert_eq!(against.revlog.review_days.counts, [1, 1, 1, 0, 0]);
    assert_eq!(against.notes.unreviewed_creation_days.edges, [0, 7, 14, 21, 28, 35]);
    assert_eq!(against.notes.unreviewed_creation_days.counts, [1, 1, 0, 0, 0]);
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

    let ma = measured_against(dir_a.path(), &p);
    let mc = measured_against(dir_c.path(), &p);
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
    // F3.1 at 10k: only note types with at least 2,000 generated notes are
    // compared field by field.
    assert!(assert_field_lengths("10k", &ma, &p) > 0, "a field compared one by one");
    assert_field_lengths("10k, seed 2", &mc, &p);
    // F3.2 at 10k, both seeds.
    assert!(
        assert_references_per_note("10k", &ma, &p) > 0,
        "a note type compared one by one"
    );
    assert_references_per_note("10k, seed 2", &mc, &p);
    // F4.8: the stored fields' structure. The references this test's own
    // parser finds are the ones `measure` counted.
    let seen = check_note_fields(&collection(dir_a.path()), &p);
    assert!(seen > 0, "references written");
    let counted: u64 = ma
        .notetypes
        .iter()
        .flat_map(|nt| &nt.media_refs)
        .map(|r| r[0] + r[1])
        .sum();
    assert_eq!(seen as u64, counted, "references parsed and measured");

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
    let m = measured_against(dir.path(), &p);
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
    // F3.1 and F3.2 at 100k.
    assert!(assert_field_lengths("100k", &m, &p) > 0, "fields compared one by one");
    assert!(
        assert_references_per_note("100k", &m, &p) > 0,
        "note types compared one by one"
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
    // F4.4: no answer on the pinned day, which starts at the last rollover
    // (the profile's hour, in its offset) at or before now.
    let pinned_day_start = pinned_day_start_ms(NOW_MS, p.utc_offset_secs, p.rollover_hour);
    assert!(
        latest < pinned_day_start,
        "an answer on the pinned day: {latest} >= {pinned_day_start}"
    );
    // F4.7: no note id after now either.
    let notes = all_notes(&path);
    assert!(notes.iter().all(|n| n.id.0 <= NOW_MS));
}

/// The start of the day holding `now_ms`: its last local rollover at or
/// before it (F4.3), computed from the definition.
fn pinned_day_start_ms(now_ms: i64, offset_secs: i32, rollover_hour: u8) -> i64 {
    let local_ms = now_ms + i64::from(offset_secs) * 1000;
    let rollover_ms = i64::from(rollover_hour) * 3_600_000;
    let since = (local_ms - rollover_ms).rem_euclid(86_400_000);
    now_ms - since
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
        learn_ahead_secs: 1200,
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
    let (dir, manifest, case) = worst("field-1mb", Some(2000));
    let wanted = case.large_fields.unwrap();
    let path = collection(dir.path());
    let mut large: Vec<i64> = all_notes(&path)
        .iter()
        .filter(|n| n.field_values().any(|f| f.len() as u64 >= wanted.bytes))
        .map(|n| n.id.0)
        .collect();
    large.sort_unstable();
    assert_eq!(large.len() as u64, wanted.notes);
    // F4.8: a large field is exactly that many bytes of words, without the
    // references its note type would otherwise put there.
    for note in all_notes(&path) {
        for value in note.field_values().filter(|f| f.len() as u64 >= wanted.bytes) {
            assert_eq!(value.len() as u64, wanted.bytes, "note {}", note.id);
            assert!(is_words(value), "note {}: words only", note.id);
        }
    }
    // The notes the oracle check renders (meguri-cli generate's
    // large_field_notes), read without changing a row.
    assert_eq!(generate::notes_with_field_of(&path, wanted.bytes).unwrap(), large);
    assert_eq!(generate::dump::logical_sha256(&path).unwrap(), manifest.dump_sha256);
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
fn recipe_learn_ahead_only_changes_the_collection_setting() {
    let held = Recipe::named("local-review").unwrap();
    assert_eq!(held.learn_ahead_secs, 0, "F9.2 public learning must remain held");
    let mut early = held.clone();
    early.learn_ahead_secs = 1200;
    let held_dir = tempfile::tempdir().unwrap();
    let early_dir = tempfile::tempdir().unwrap();
    let held_manifest = generate_recipe(&held, 1, held_dir.path()).unwrap();
    generate_recipe(&early, 1, early_dir.path()).unwrap();
    let held_path = collection(held_dir.path());
    let early_path = collection(early_dir.path());
    assert_eq!(held_manifest.counts.cards, 4);
    assert_eq!(
        text(&query(&held_path, "SELECT val FROM config WHERE KEY = 'collapseTime'")[0][0]),
        "0"
    );
    assert_eq!(
        text(&query(&early_path, "SELECT val FROM config WHERE KEY = 'collapseTime'")[0][0]),
        "1200"
    );
    assert_eq!(
        query(&held_path, "SELECT * FROM cards ORDER BY id"),
        query(&early_path, "SELECT * FROM cards ORDER BY id")
    );
    assert_eq!(all_revlog(&held_path), all_revlog(&early_path));
    assert_eq!(
        query(&held_path, "SELECT * FROM deck_config ORDER BY id"),
        query(&early_path, "SELECT * FROM deck_config ORDER BY id")
    );
    assert_eq!(
        query(
            &held_path,
            "SELECT * FROM config WHERE KEY != 'collapseTime' ORDER BY KEY"
        ),
        query(
            &early_path,
            "SELECT * FROM config WHERE KEY != 'collapseTime' ORDER BY KEY"
        )
    );
    for name in Recipe::names()
        .unwrap()
        .into_iter()
        .filter(|name| name != "local-review")
    {
        assert_eq!(Recipe::named(&name).unwrap().learn_ahead_secs, 1200, "{name}");
    }
}

#[test]
fn recipe_learn_ahead_defaults_to_1200_and_requires_u32() {
    let base = "recipe: horizon\ndescription: horizon\nnow: '2030-01-15T10:00:00Z'\n";
    assert_eq!(Recipe::parse(base).unwrap().learn_ahead_secs, 1200);
    for value in ["0", "4294967295"] {
        assert_eq!(
            Recipe::parse(&format!("{base}learn_ahead_secs: {value}\n"))
                .unwrap()
                .learn_ahead_secs,
            value.parse::<u32>().unwrap()
        );
    }
    for value in ["-1", "4294967296", "1.5", "null", "'1200'"] {
        assert!(
            Recipe::parse(&format!("{base}learn_ahead_secs: {value}\n")).is_err(),
            "{value}"
        );
    }
}

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
        let (now_ms, offset) = recipe.instant().unwrap();
        assert_eq!(manifest.utc_offset_secs, offset, "{name}: the offset of now");
        let path = collection(dir.path());
        let collection = open(&path);
        let rows = collection.writer();
        let crt = rows.col().unwrap().created.0;
        let today = (now_ms / 1000 - crt).div_euclid(DAY);
        // F9.2: 120 days of history before the pinned day.
        assert_eq!(today, 120, "{name}");
        let pinned_day_start = (crt + today * DAY) * 1000;
        let mut decks: HashMap<i64, String> = HashMap::new();
        for id in rows.ids_after(Table::Decks, None, 100_000).unwrap() {
            decks.insert(id, rows.deck(DeckId(id)).unwrap().unwrap().human_name());
        }
        drop(collection);
        let mut answers: HashMap<i64, Vec<i64>> = HashMap::new();
        for entry in all_revlog(&path) {
            answers.entry(entry.card_id.0).or_default().push(entry.id.0);
        }
        let notes: HashMap<i64, Note> = all_notes(&path).into_iter().map(|n| (n.id.0, n)).collect();
        // Counts by deck name: new, learning due now, due, later, suspended,
        // buried; and F9.2's answer days.
        let mut counts: HashMap<String, [u32; 6]> = HashMap::new();
        for card in all_cards(&path) {
            let slot = match (card.queue, card.card_type) {
                (CardQueue::New, _) => 0,
                (CardQueue::Learning, CardType::Learning) if i64::from(card.due) <= now_ms / 1000 => 1,
                (CardQueue::InterdayLearning, CardType::Learning) if i64::from(card.due) <= today => 1,
                (CardQueue::Review, _) if i64::from(card.due) <= today => 2,
                (CardQueue::Review, _) => 3,
                (CardQueue::Suspended, _) => 4,
                (CardQueue::BuriedByUser, _) => 5,
                other => panic!("{name}: unexpected card state {other:?}"),
            };
            counts.entry(decks[&card.deck_id.0].clone()).or_default()[slot] += 1;
            let ids = answers.get(&card.id.0).cloned().unwrap_or_default();
            let created = card.note_id.0;
            assert!(notes.contains_key(&created), "{name}: card {} has a note", card.id);
            let Some(&last) = ids.iter().max() else {
                assert_eq!(slot, 0, "{name}: card {} has answers", card.id);
                // Created 1 to 30 days before now; a note id taken already
                // moves up a few milliseconds (F4.7).
                let before = now_ms - created;
                assert!(
                    (DAY * 1000 - 1000..=30 * DAY * 1000).contains(&before),
                    "{name}: new note {created} made {before} ms before now"
                );
                continue;
            };
            assert!(
                last < pinned_day_start,
                "{name}: card {} answered on the pinned day",
                card.id
            );
            let first = *ids.iter().min().unwrap();
            let gap = first - created;
            assert!(
                (DAY * 1000 - 1000..=2 * DAY * 1000).contains(&gap),
                "{name}: note {created} made {gap} ms before its first answer"
            );
            let last_day = (last / 1000 - crt).div_euclid(DAY);
            match slot {
                1 => {
                    assert_eq!(ids.len(), 1, "{name}: a learning card has one answer");
                    assert!(
                        pinned_day_start - last <= DAY * 1000,
                        "{name}: within 24 h of the pinned day"
                    );
                }
                2 => assert!(
                    (today - 20..=today - 1).contains(&last_day),
                    "{name}: due card's last day {last_day}"
                ),
                3 => assert!(
                    (today - 3..=today - 1).contains(&last_day),
                    "{name}: later card's last day {last_day}"
                ),
                _ => {}
            }
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

/// The entries next to `dir` that a run into `dir` made (`.<name>.…`),
/// its lock file aside.
fn run_leftovers(dir: &Path) -> Vec<String> {
    let prefix = format!(".{}.", dir.file_name().unwrap().to_string_lossy());
    std::fs::read_dir(dir.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&prefix) && !name.ends_with(".lock"))
        .collect()
}

/// A run that fails part-way leaves the directory's previous output whole:
/// the collection, its media and the manifest of one run, never a mix.
#[test]
fn a_failed_generation_leaves_the_previous_output() {
    let dir = tempfile::tempdir().unwrap();
    let good = generate_recipe(&Recipe::named("review-ready").unwrap(), 1, dir.path()).unwrap();
    let media: Vec<_> = std::fs::read_dir(dir.path().join("collection.media"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(!media.is_empty());
    // A deck listed twice is found once the collection is being written.
    let deck = |name: &str| DeckSpec {
        name: name.to_owned(),
        new: 1,
        ..DeckSpec::default()
    };
    let bad = Recipe {
        recipe: "twice".to_owned(),
        description: "fails part-way".to_owned(),
        now: "2030-01-15T10:00:00Z".to_owned(),
        rollover_hour: None,
        learn_ahead_secs: 1200,
        presets: Vec::new(),
        decks: vec![deck("A"), deck("A")],
    };
    assert!(generate_recipe(&bad, 1, dir.path()).is_err());
    assert_eq!(Manifest::load(dir.path()).unwrap(), good);
    assert_eq!(
        generate::dump::logical_sha256(&collection(dir.path())).unwrap(),
        good.dump_sha256
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("collection.media")).unwrap().count(),
        media.len()
    );
    assert_eq!(run_leftovers(dir.path()), Vec::<String>::new());
}

#[test]
fn media_files_mode_writes_every_reference() {
    let mut request = Request::new(shape(), 1500, 4);
    request.media = MediaMode::Files;
    let (dir, manifest) = run(&request);
    assert!(
        manifest.counts.media_refs > 0,
        "the profile's media fields hold references"
    );
    assert_eq!(manifest.counts.media_files, manifest.counts.media_refs);
    let m = measured(dir.path(), 0);
    assert_eq!(m.totals.media_files, manifest.counts.media_files);
    // Every written reference names a written file, and nothing else is
    // written (F4.8: a reference that does not fit has no file).
    let files: HashSet<String> = std::fs::read_dir(dir.path().join("collection.media"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let mut named = HashSet::new();
    for note in all_notes(&collection(dir.path())) {
        for value in note.field_values() {
            named.extend(references_in(value).into_iter().map(|r| r.name));
        }
    }
    assert_eq!(named, files);
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

/// F4.8's examples, for note index 7,978 (`1f2a`): the stored field for a
/// drawn length and its markup, with the bytes of the spec's table. The
/// markup counts inside the drawn length; words fill the rest.
#[test]
fn fields_follow_the_f4_8_examples() {
    use generate::notes::{FieldText, answer_bytes, cloze_deletions, note_media, reference_threshold};
    let mut rng = generate::rng::Rng::stream(5, generate::rng::Stream::Note, 7978);
    let rng = &mut rng;
    let image = note_media(5, 7978, 0, media::MediaKind::Image, 1000).reference();
    assert_eq!(image, "<img src=\"mg1f2a-0.png\">");
    assert_eq!(image.len(), 24);
    let sound = note_media(5, 7978, 1, media::MediaKind::Audio, 1000).reference();
    assert_eq!(sound, "[sound:mg1f2a-1.mp3]");

    // No markup: the drawn length in words. A normal second field drawn
    // at 0 is empty; a normal front drawn at 0 is built at 1 byte.
    assert_eq!(FieldText::new(0).finish(rng), "");
    let letter = FieldText::new(1).finish(rng);
    assert!(letter.len() == 1 && is_words(&letter) && letter != " ", "{letter:?}");

    // 40: 15 bytes of words, a space, the reference.
    let mut field = FieldText::new(40);
    assert!(field.fits(&image));
    field.push_reference(image.clone());
    let text = field.finish(rng);
    assert_eq!(text.len(), 40);
    let words = text.strip_suffix(&format!(" {image}")).unwrap();
    assert!(
        words.len() == 15 && is_words(words) && !words.starts_with(' '),
        "{text:?}"
    );

    // 25: the reference alone, one byte short (1 byte left).
    let mut field = FieldText::new(25);
    assert!(field.fits(&image));
    field.push_reference(image.clone());
    assert_eq!(field.finish(rng), image);

    // The thresholds are the longest reference 0 can be: the largest note
    // index is 999,998 (`f423e`), positions staying below 1,000,000.
    let longest_image = note_media(5, 999_998, 0, media::MediaKind::Image, 1000).reference();
    let longest_sound = note_media(5, 999_998, 0, media::MediaKind::Audio, 1000).reference();
    assert_eq!(longest_image, "<img src=\"mgf423e-0.png\">");
    assert_eq!(longest_sound, "[sound:mgf423e-0.mp3]");
    assert_eq!((longest_image.len(), longest_sound.len()), (25, 21));
    assert_eq!(reference_threshold(media::MediaKind::Image), 25);
    assert_eq!(reference_threshold(media::MediaKind::Audio), 21);

    // 20: below the image threshold, so no reference is drawn (it would
    // not fit either): 20 bytes of words.
    assert!(20 < reference_threshold(media::MediaKind::Image));
    let field = FieldText::new(20);
    assert!(!field.fits(&image));
    let text = field.finish(rng);
    assert!(text.len() == 20 && is_words(&text), "{text:?}");

    // A second reference fits only with its separating space: 24 + 1 + 20.
    let mut field = FieldText::new(45);
    field.push_reference(image.clone());
    assert!(field.fits(&sound));
    // 44: the sound does not fit and is not written: 19 bytes of words, a
    // space, the image.
    let mut field = FieldText::new(44);
    field.push_reference(image.clone());
    assert!(!field.fits(&sound));
    let text = field.finish(rng);
    assert_eq!(text.len(), 44);
    let words = text.strip_suffix(&format!(" {image}")).unwrap();
    assert!(
        words.len() == 19 && is_words(words) && !words.starts_with(' '),
        "{text:?}"
    );

    // A cloze first field, N = 2, answers drawn 5 and 8 bytes, at 60:
    // 13 + 1 + 16 bytes of deletions, a space, 29 bytes of words.
    assert_eq!(answer_bytes(vec![5, 8], 60), [5, 8]);
    let deletions = vec!["{{c1::kamor}}".to_owned(), "{{c2::sutenowa}}".to_owned()];
    assert_eq!((deletions[0].len(), deletions[1].len()), (13, 16));
    let text = FieldText::with_deletions(60, deletions).finish(rng);
    assert_eq!(text.len(), 60);
    let words = text.strip_prefix("{{c1::kamor}} {{c2::sutenowa}} ").unwrap();
    assert!(
        words.len() == 29 && is_words(words) && !words.starts_with(' '),
        "{text:?}"
    );

    // The same at 20: 30 bytes is longer, so every answer is 1 byte; 9 + 1
    // + 9 bytes, the deletions alone (1 byte left).
    assert_eq!(answer_bytes(vec![5, 8], 20), [1, 1]);
    let one_byte = vec!["{{c1::k}}".to_owned(), "{{c2::s}}".to_owned()];
    assert_eq!(
        FieldText::with_deletions(20, one_byte.clone()).finish(rng),
        "{{c1::k}} {{c2::s}}"
    );
    // A reference after them fits only in 19 + 1 + 24 bytes.
    assert!(FieldText::with_deletions(44, one_byte.clone()).fits(&image));
    assert!(!FieldText::with_deletions(43, one_byte).fits(&image));

    // A cloze first field, N = 3, at 10: 1-byte answers whatever was
    // drawn, 29 bytes, the deletions alone.
    for drawn in [[3, 3, 3], [14, 14, 14]] {
        assert_eq!(answer_bytes(drawn.to_vec(), 10), [1, 1, 1]);
    }
    let text = FieldText::with_deletions(10, cloze_deletions(3, 10, rng)).finish(rng);
    assert_eq!(text.len(), 29, "{text:?}");
    for (k, deletion) in text.split(' ').enumerate() {
        let answer = deletion
            .strip_prefix(&format!("{{{{c{}::", k + 1))
            .and_then(|d| d.strip_suffix("}}"))
            .unwrap_or_else(|| panic!("{text:?}"));
        assert!(answer.len() == 1 && is_words(answer) && answer != " ");
    }
    // Drawn answers of 3 to 14 bytes are kept when the deletions fit.
    for _ in 0..50 {
        for (k, deletion) in cloze_deletions(2, 60, rng).iter().enumerate() {
            let answer = &deletion[format!("{{{{c{}::", k + 1).len()..deletion.len() - 2];
            assert!((3..=14).contains(&answer.len()) && is_words(answer), "{deletion:?}");
        }
    }
    // `{{c10::..}}` takes its extra digit: ten 2-byte answers make 9 × 10
    // + 11 + 9 spaces = 110 bytes.
    assert_eq!(answer_bytes(vec![2; 10], 110), [2; 10]);
    assert_eq!(answer_bytes(vec![2; 10], 109), [1; 10]);
}

/// F4.8's worked example of a field's reference rate: 50 lengths in
/// `[16, 32)`, 50 in `[32, 64)`, `media_refs` `[30, 10]`, 100 notes. The
/// values are the spec's, worked out by hand.
#[test]
fn the_reference_rate_follows_the_f4_8_example() {
    use generate::notes::{length_share, reference_rate};
    let mut lengths = Histogram::log2(24);
    lengths.add_n(16, 50);
    lengths.add_n(32, 50);
    let close = |what: &str, got: f64, want: f64| {
        assert!((got - want).abs() < 1e-12, "{what}: {got} vs {want}");
    };
    close("s(25)", length_share(&lengths, 25), 0.71875);
    close("s(21)", length_share(&lengths, 21), 0.84375);
    // Bucket edges: all of `[32, 64)` is at 32 or more, none of `[16, 32)`.
    close("s(32)", length_share(&lengths, 32), 0.5);
    close("s(16)", length_share(&lengths, 16), 1.0);
    close("s(64)", length_share(&lengths, 64), 0.0);

    let mut shape = profile::NotetypeShape {
        kind: "normal".to_owned(),
        stock_kind: 0,
        fields: 2,
        templates: 1,
        notes: 100,
        cards: 100,
        css_bytes: 0,
        template_bytes: vec![[0, 0]],
        script_bytes: 0,
        cards_per_note: Histogram::linear(1, 64, 1),
        field_bytes: vec![lengths.clone(), lengths.clone()],
        media_refs: vec![[0, 0], [30, 10]],
        refs_per_note: Histogram::linear(0, 64, 1),
    };
    // q = (30 × 0.71875 + 10 × 0.84375) / 40 = 0.75; r = 40 / 75.
    close("r", reference_rate(&shape, 1), 40.0 / 75.0);
    close("a field without references", reference_rate(&shape, 0), 0.0);
    // q = 0: every length is below both thresholds.
    let mut short = Histogram::log2(24);
    short.add_n(10, 100);
    shape.field_bytes[1] = short;
    close("q = 0", reference_rate(&shape, 1), 0.0);
    // A shape without notes.
    shape.field_bytes[1] = lengths;
    shape.notes = 0;
    close("no notes", reference_rate(&shape, 1), 0.0);
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

// -------------------------------------------- histories and card columns

/// Where a card is before an answer: the rows of F5.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Step1,
    Step2,
    Review,
    Relearning,
}

/// F5.1, written out from the spec's table: the phase an answer leaves the
/// card in, and its delay in seconds (`None`: F days).
fn answer_table(phase: Phase, button: u8) -> (Phase, Option<i64>) {
    use Phase::*;
    match (phase, button) {
        (Step1, 1) => (Step1, Some(60)),
        (Step1, 2) => (Step1, Some(330)),
        (Step1, 3) => (Step2, Some(600)),
        (Step1, 4) => (Review, None),
        (Step2, 1) => (Step1, Some(60)),
        (Step2, 2) => (Step2, Some(600)),
        (Step2, 3 | 4) => (Review, None),
        (Review, 1) => (Relearning, Some(600)),
        (Review, 2..=4) => (Review, None),
        (Relearning, 1) => (Relearning, Some(600)),
        (Relearning, 2) => (Relearning, Some(900)),
        (Relearning, 3 | 4) => (Review, None),
        other => panic!("no button {other:?}"),
    }
}

/// F5.6: the FSRS difficulty mapped to `revlog.factor` in f32 steps.
fn expected_revlog_factor(difficulty: f32) -> u32 {
    let n: f32 = (difficulty - 1.0) / 9.0;
    let s: f32 = n + 0.1;
    (s * 1000.0).round() as u32
}

/// Replays every card's review log through F5.1 and fsrs (F5.3) and checks
/// the card's columns (F5.4, F5.5) and every review-log column (F5.6).
/// With `due_targets`, a review card's interval may be one of F5.5's due
/// targets instead of F days. Returns how many cards ended in each (type,
/// queue).
fn check_histories(path: &Path, now_ms: i64, due_targets: bool) -> BTreeMap<(i8, i8), usize> {
    let positions_by_note = true;
    let fsrs = fsrs::FSRS::new(&[]).unwrap();
    let interval = |stability: f32| -> i64 {
        f64::from(fsrs.next_interval(Some(stability), 0.9, 3).round().clamp(1.0, 36_500.0)) as i64
    };
    let crt = open(path).writer().col().unwrap().created.0;
    let day_of = |ms: i64| (ms.div_euclid(1000) - crt).div_euclid(DAY);
    let today = day_of(now_ms);
    let pinned_day_start = (crt + today * DAY) * 1000;
    let mut by_card: HashMap<i64, Vec<RevlogEntry>> = HashMap::new();
    for entry in all_revlog(path) {
        by_card.entry(entry.card_id.0).or_default().push(entry);
    }
    let mut seen: BTreeMap<(i8, i8), usize> = BTreeMap::new();
    let mut position_of_note: HashMap<i64, u32> = HashMap::new();
    for card in all_cards(path) {
        *seen.entry((card.card_type as i8, card.queue as i8)).or_default() += 1;
        let id = card.id;
        let rows = by_card.remove(&card.id.0).unwrap_or_default();
        let position = if rows.is_empty() {
            assert_eq!(
                (card.card_type, card.queue),
                (CardType::New, CardQueue::New),
                "card {id}"
            );
            assert_eq!((card.interval, card.ease_factor, card.reps, card.lapses), (0, 0, 0, 0));
            assert_eq!(card.remaining_steps, 0);
            assert_eq!(card.mtime.0, card.note_id.0.div_euclid(1000), "card {id}: mod");
            assert!(card.data.memory_state.is_none() && card.data.last_review_secs.is_none());
            card.due as u32
        } else {
            card.data.new_position.unwrap_or_else(|| panic!("card {id}: pos"))
        };
        if positions_by_note {
            let first = *position_of_note.entry(card.note_id.0).or_insert(position);
            assert_eq!(first, position, "card {id}: siblings share their note's position");
        }
        if rows.is_empty() {
            continue;
        }

        // fsrs's memory states over the answers (F5.3).
        let days: Vec<i64> = rows.iter().map(|r| day_of(r.id.0)).collect();
        let item = fsrs::FSRSItem {
            reviews: rows
                .iter()
                .zip(&days)
                .enumerate()
                .map(|(i, (r, &day))| fsrs::FSRSReview {
                    rating: u32::from(r.button),
                    delta_t: if i == 0 { 0 } else { (day - days[i - 1]) as u32 },
                })
                .collect(),
        };
        let states = fsrs.historical_memory_states(item, None).unwrap();

        let mut phase = Phase::Step1;
        let mut previous_ivl = 0;
        let mut previous_id = card.id.0;
        let mut ease = 0i64;
        let mut lapses = 0;
        let mut last_delay = None;
        for (i, r) in rows.iter().enumerate() {
            let at = format!("card {id}, answer {i}");
            assert!(r.id.0 > previous_id, "{at}: ids increase from the card's");
            assert!(r.id.0 < pinned_day_start, "{at}: on the pinned day (F4.4)");
            previous_id = r.id.0;
            let kind = match phase {
                Phase::Step1 | Phase::Step2 => RevlogKind::Learning,
                Phase::Review => RevlogKind::Review,
                Phase::Relearning => RevlogKind::Relearning,
            };
            assert_eq!(r.kind, kind, "{at}: kind");
            let (next, delay) = answer_table(phase, r.button);
            let next_day = (crt + (days[i] + 1) * DAY) * 1000;
            let ivl = match delay {
                Some(secs) if r.id.0 + secs * 1000 >= next_day => 1,
                Some(secs) => -secs,
                // The last answer of a review card: the card's interval,
                // checked below.
                None if i + 1 == rows.len() => i64::from(r.interval),
                None => interval(states[i].stability),
            };
            assert_eq!(i64::from(r.interval), ivl, "{at}: ivl");
            assert_eq!(i64::from(r.last_interval), previous_ivl, "{at}: lastIvl");
            assert_eq!(
                r.ease_factor,
                expected_revlog_factor(states[i].difficulty),
                "{at}: factor"
            );
            assert!(r.answer_millis <= 60_000, "{at}: time {}", r.answer_millis);
            // F5.4.
            match phase {
                Phase::Step1 | Phase::Step2 if next == Phase::Review => ease = 2500,
                Phase::Review => match r.button {
                    1 => ease = (ease - 200).max(1300),
                    2 => ease = (ease - 150).max(1300),
                    4 => ease += 150,
                    _ => {}
                },
                _ => {}
            }
            if phase == Phase::Review && r.button == 1 {
                lapses += 1;
            }
            previous_ivl = ivl;
            last_delay = delay.map(|secs| (secs, ivl == 1));
            phase = next;
        }

        let last = rows.last().unwrap();
        let last_secs = last.id.0.div_euclid(1000);
        let last_day = days[days.len() - 1];
        let state = states[states.len() - 1];
        let learning_queue = || match last_delay {
            Some((_, true)) => (CardQueue::InterdayLearning, last_day + 1),
            Some((secs, false)) => (CardQueue::Learning, last_secs + secs),
            None => panic!("card {id}: a learning phase after F days"),
        };
        match phase {
            Phase::Step1 | Phase::Step2 => {
                assert_eq!(card.card_type, CardType::Learning, "card {id}");
                let (queue, due) = learning_queue();
                assert_eq!((card.queue, i64::from(card.due)), (queue, due), "card {id}");
                assert_eq!(card.interval, 0, "card {id}");
                let left = if phase == Phase::Step1 { 2 } else { 1 };
                assert_eq!(card.remaining_steps, left, "card {id}: left");
            }
            Phase::Relearning => {
                assert_eq!(card.card_type, CardType::Relearning, "card {id}");
                let (queue, due) = learning_queue();
                assert_eq!((card.queue, i64::from(card.due)), (queue, due), "card {id}");
                assert_eq!(i64::from(card.interval), interval(state.stability), "card {id}: ivl");
                assert_eq!(card.remaining_steps, 1, "card {id}: left");
            }
            Phase::Review => {
                assert_eq!(card.card_type, CardType::Review, "card {id}");
                assert!(
                    matches!(
                        card.queue,
                        CardQueue::Review | CardQueue::Suspended | CardQueue::BuriedByUser
                    ),
                    "card {id}: queue {:?}",
                    card.queue
                );
                assert_eq!(
                    i64::from(card.interval),
                    i64::from(last.interval),
                    "card {id}: ivl is the last row's"
                );
                assert_eq!(
                    i64::from(card.due),
                    last_day + i64::from(card.interval),
                    "card {id}: due"
                );
                // F days, or a due target (F5.5).
                let f = interval(state.stability);
                let ivl = i64::from(card.interval);
                let due_today = (1i64).max(f.min(today - last_day));
                let due_later = f.max(today - last_day + 1).min(36_500);
                assert!(
                    ivl == f || (due_targets && (ivl == due_today || ivl == due_later)),
                    "card {id}: ivl {ivl}, F {f}, last day {last_day}, today {today}"
                );
                assert_eq!(card.remaining_steps, 0, "card {id}: left");
            }
        }
        assert_eq!(i64::from(card.ease_factor), ease, "card {id}: factor");
        assert_eq!((card.reps as usize, card.lapses), (rows.len(), lapses), "card {id}");
        assert_eq!(card.mtime.0, last_secs, "card {id}: mod");
        assert_eq!((card.original_due, card.original_deck_id.0, card.flags), (0, 0, 0));
        let data = &card.data;
        assert_eq!(data.last_review_secs, Some(last_secs), "card {id}: lrt");
        let memory = data.memory_state.unwrap();
        assert!(
            (memory.stability - state.stability).abs() <= 1e-4 + state.stability * 1e-6,
            "card {id}: s"
        );
        assert!((memory.difficulty - state.difficulty).abs() <= 1e-3, "card {id}: d");
        assert_eq!(data.desired_retention, Some(0.9));
        assert_eq!(data.decay, Some(0.154));
    }
    assert!(by_card.is_empty(), "review-log rows without a card");
    if positions_by_note {
        // F6.6: positions 1 to the note count, one per note.
        let mut positions: Vec<u32> = position_of_note.values().copied().collect();
        positions.sort_unstable();
        assert_eq!(positions, (1..=positions.len() as u32).collect::<Vec<_>>());
    }
    seen
}

#[test]
fn histories_follow_the_answer_table() {
    let p = shape();
    let (dir, manifest) = run(&Request::new(p.clone(), 4000, 21));
    let path = collection(dir.path());
    let seen = check_histories(&path, NOW_MS, false);
    // Every final state the table allows, buried by the user (queue -3).
    for (state, what) in [
        ((0, 0), "new"),
        ((1, 1), "learning"),
        ((3, 1), "relearning"),
        ((2, 2), "review"),
        ((2, -1), "suspended"),
        ((2, -3), "buried"),
    ] {
        assert!(seen.get(&state).copied().unwrap_or(0) > 0, "no {what} card: {seen:?}");
    }
    assert!(!seen.contains_key(&(2, -2)), "queue -2 needs a sibling answered today");
    let notes = query(&path, "SELECT count(*) FROM notes")[0][0].clone();
    assert_eq!(int(&notes) as u64, manifest.counts.notes);
}

#[test]
fn recipe_and_worst_case_histories_follow_the_answer_table() {
    for name in Recipe::names().unwrap() {
        let recipe = Recipe::named(&name).unwrap();
        let dir = tempfile::tempdir().unwrap();
        generate_recipe(&recipe, 3, dir.path()).unwrap();
        let (now_ms, _) = recipe.instant().unwrap();
        check_histories(&collection(dir.path()), now_ms, true);
    }
    let (dir, _, _) = worst("due-500k", Some(1500));
    check_histories(&collection(dir.path()), NOW_MS, true);
}

// ------------------------------------------------------ collection rows

/// The config rows of F6.2, as (key, JSON text).
fn expected_config(
    offset_secs: i32,
    rollover: u8,
    today: i64,
    cur_model: i64,
    notes: u64,
    learn_ahead: u32,
) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = [
        ("activeDecks", "[1]".to_owned()),
        ("addToCur", "true".to_owned()),
        ("collapseTime", learn_ahead.to_string()),
        ("creationOffset", (-offset_secs / 60).to_string()),
        ("curDeck", "1".to_owned()),
        ("curModel", cur_model.to_string()),
        ("dayLearnFirst", "false".to_owned()),
        ("dueCounts", "true".to_owned()),
        ("estTimes", "true".to_owned()),
        ("fsrs", "true".to_owned()),
        ("lastUnburied", today.to_string()),
        ("loadBalancerEnabled", "true".to_owned()),
        ("newSpread", "0".to_owned()),
        ("nextPos", (notes + 1).to_string()),
        ("rollover", rollover.to_string()),
        ("sched2021", "true".to_owned()),
        ("schedVer", "2".to_owned()),
        ("sortBackwards", "false".to_owned()),
        ("sortType", "\"noteFld\"".to_owned()),
        ("timeLim", "0".to_owned()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    rows.sort();
    rows
}

/// F6: the rows besides notes, cards and the review log.
fn check_collection_rows(path: &Path, now_ms: i64, offset_secs: i32, rollover: u8, recipe: bool, learn_ahead: u32) {
    let collection = open(path);
    let rows = collection.writer();
    let col = rows.col().unwrap();
    let crt = col.created.0;
    let today = (now_ms / 1000 - crt).div_euclid(DAY);
    // F6.1.
    assert_eq!(
        (
            col.modified.0,
            col.schema_modified.0,
            col.last_sync.0,
            col.usn.0,
            col.dirty
        ),
        (now_ms, crt * 1000, 0, 0, 0)
    );
    // crt is the pinned day's start minus `today` days, at the rollover.
    assert_eq!(
        (crt + today * DAY) * 1000,
        pinned_day_start_ms(now_ms, offset_secs, rollover)
    );
    let notetypes = rows.ids_after(Table::Notetypes, None, 1000).unwrap();
    let decks = rows.ids_after(Table::Decks, None, 100_000).unwrap();
    drop(collection);
    let notes = int(&query(path, "SELECT count(*) FROM notes")[0][0]) as u64;

    // F6.2: every key, its value, usn -1 and mtime crt.
    let config: Vec<(String, String)> = query(path, "SELECT KEY, usn, mtime_secs, val FROM config ORDER BY KEY")
        .iter()
        .map(|row| {
            assert_eq!((int(&row[1]), int(&row[2])), (-1, crt), "config {:?}", row[0]);
            (text(&row[0]), text(&row[3]))
        })
        .collect();
    assert_eq!(
        config,
        expected_config(offset_secs, rollover, today, notetypes[0], notes, learn_ahead)
    );

    let collection = open(path);
    let rows = collection.writer();
    // F6.4: collapsed as Anki creates decks, expanded in recipes; no
    // studied-today counters.
    for id in decks {
        let deck = rows.deck(DeckId(id)).unwrap().unwrap();
        let common = &deck.common;
        assert_eq!(
            (common.study_collapsed, common.browser_collapsed),
            (!recipe, !recipe),
            "deck {}",
            deck.human_name()
        );
        assert_eq!(
            (
                common.last_day_studied,
                common.new_studied,
                common.review_studied,
                common.learning_studied,
                common.milliseconds_studied
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!((deck.mtime.0, deck.usn.0), (crt, -1));
    }
    // F6.5.
    let mut field_ids = HashSet::new();
    for id in &notetypes {
        let nt = rows.notetype(NotetypeId(*id)).unwrap().unwrap();
        let c = &nt.config;
        assert_eq!((c.original_stock_kind, c.sort_field_idx), (0, 0), "{}", nt.name);
        assert!(c.latex_pre.is_empty() && c.latex_post.is_empty());
        let reqs: Vec<(u32, i32, Vec<u32>)> = c
            .reqs
            .iter()
            .map(|r| (r.card_ord, r.kind, r.field_ords.clone()))
            .collect();
        let wanted: Vec<(u32, i32, Vec<u32>)> = (0..nt.templates.len() as u32).map(|ord| (ord, 1, vec![0])).collect();
        assert_eq!(reqs, wanted, "{}: reqs", nt.name);
        assert_eq!((nt.mtime.0, nt.usn.0), (crt, -1));
        for field in &nt.fields {
            assert_eq!((field.config.font_name.as_str(), field.config.font_size), ("Arial", 20));
            assert!(field_ids.insert(field.config.id.unwrap()), "field ids are distinct");
        }
        for template in &nt.templates {
            assert_eq!((template.mtime.0, template.usn.0), (0, 0));
            assert!(template.config.id.is_some());
        }
    }
    drop(collection);
    // F6.7: one row per tag on notes, no parent rows, collapsed per kind.
    let mut on_notes: Vec<String> = Vec::new();
    for note in all_notes(path) {
        assert!(note.tags.is_empty() || (note.tags.starts_with(' ') && note.tags.ends_with(' ')));
        on_notes.extend(note.tags.split_whitespace().map(str::to_owned));
    }
    on_notes.sort_by_key(|t| t.to_lowercase());
    on_notes.dedup_by_key(|t| t.to_lowercase());
    let tag_rows = query(path, "SELECT tag, usn, collapsed, config FROM tags ORDER BY tag");
    let mut tags: Vec<String> = Vec::new();
    for row in &tag_rows {
        assert_eq!(
            (int(&row[1]), int(&row[2]), &row[3]),
            (-1, i64::from(!recipe), &SqlValue::Null)
        );
        tags.push(text(&row[0]));
    }
    tags.sort_by_key(|t| t.to_lowercase());
    assert_eq!(tags, on_notes, "tag rows are the tags on notes");
    // F6.8.
    assert_eq!(int(&query(path, "SELECT count(*) FROM graves")[0][0]), 0);
}

#[test]
fn collection_rows_are_those_of_a_new_collection() {
    let p = shape();
    let (dir, manifest) = run(&Request::new(p.clone(), 1500, 31));
    assert_eq!(manifest.generator_version, 5, "F9.2/F10.4");
    assert_eq!(manifest.utc_offset_secs, p.utc_offset_secs);
    check_collection_rows(
        &collection(dir.path()),
        NOW_MS,
        p.utc_offset_secs,
        p.rollover_hour,
        false,
        1200,
    );
    for name in Recipe::names().unwrap() {
        let recipe = Recipe::named(&name).unwrap();
        let dir = tempfile::tempdir().unwrap();
        generate_recipe(&recipe, 1, dir.path()).unwrap();
        let (now_ms, offset) = recipe.instant().unwrap();
        check_collection_rows(
            &collection(dir.path()),
            now_ms,
            offset,
            recipe.rollover_hour.unwrap_or(4),
            true,
            recipe.learn_ahead_secs,
        );
    }
    // A recipe at +09:00 writes creationOffset -540 (minutes west).
    let recipe = Recipe::named("error-prone").unwrap();
    assert!(recipe.now.ends_with("+09:00"));
    let dir = tempfile::tempdir().unwrap();
    let manifest = generate_recipe(&recipe, 1, dir.path()).unwrap();
    assert_eq!(manifest.utc_offset_secs, 32_400);
    let offset = query(
        &collection(dir.path()),
        "SELECT val FROM config WHERE KEY = 'creationOffset'",
    );
    assert_eq!(text(&offset[0][0]), "-540");
}

#[test]
fn note_tags_are_in_unicase_order() {
    let recipe = Recipe {
        recipe: "tags".to_owned(),
        description: "tag order".to_owned(),
        now: "2030-01-15T10:00:00Z".to_owned(),
        rollover_hour: None,
        learn_ahead_secs: 1200,
        presets: Vec::new(),
        decks: vec![DeckSpec {
            name: "Tags".to_owned(),
            new: 1,
            tags: vec![
                "beta".to_owned(),
                "Alpha".to_owned(),
                "_under".to_owned(),
                "Zeta".to_owned(),
                "alpha".to_owned(),
            ],
            ..DeckSpec::default()
        }],
    };
    let dir = tempfile::tempdir().unwrap();
    generate_recipe(&recipe, 1, dir.path()).unwrap();
    let notes = all_notes(&collection(dir.path()));
    // `_` (U+005F) sorts before letters once case is folded; `alpha` is
    // `Alpha`, which came first.
    assert_eq!(notes[0].tags, " _under Alpha beta Zeta ");
}

#[test]
fn requests_needing_a_million_positions_are_refused() {
    let p = shape();
    // About 0.87 notes per card: 1.2M cards need more than 999,999 notes.
    let cards = 1_200_000;
    assert!(p.totals.notes as f64 / p.totals.cards as f64 * cards as f64 > 999_999.0);
    let dir = tempfile::tempdir().unwrap();
    let watch = meguri_testkit::wallclock::Stopwatch::start();
    let result = generate::generate(&Request::new(p, cards, 1), dir.path());
    assert!(matches!(result, Err(GenError::Request(_))), "{result:?}");
    assert!(watch.elapsed_ms() < 60_000, "refused before generating");
    assert!(!collection(dir.path()).exists());

    // A recipe too: a million new notes.
    let recipe = Recipe {
        recipe: "huge".to_owned(),
        description: "too many notes".to_owned(),
        now: "2030-01-15T10:00:00Z".to_owned(),
        rollover_hour: None,
        learn_ahead_secs: 1200,
        presets: Vec::new(),
        decks: vec![DeckSpec {
            name: "All".to_owned(),
            new: 1_000_000,
            ..DeckSpec::default()
        }],
    };
    let result = generate_recipe(&recipe, 1, dir.path());
    assert!(matches!(result, Err(GenError::Recipe(_))), "{result:?}");
    assert!(!collection(dir.path()).exists());
}

#[test]
fn recipe_deck_names_are_unique_under_unicase() {
    // F6.4: `STRASSE` is `Straße` once case is folded (lower-casing alone
    // keeps them apart), so it is listed twice.
    let deck = |name: &str| DeckSpec {
        name: name.to_owned(),
        new: 1,
        ..DeckSpec::default()
    };
    let recipe = Recipe {
        recipe: "twice".to_owned(),
        description: "one deck under two spellings".to_owned(),
        now: "2030-01-15T10:00:00Z".to_owned(),
        rollover_hour: None,
        learn_ahead_secs: 1200,
        presets: Vec::new(),
        decks: vec![deck("Straße"), deck("STRASSE")],
    };
    let dir = tempfile::tempdir().unwrap();
    let error = generate_recipe(&recipe, 1, dir.path()).unwrap_err().to_string();
    assert!(error.contains("listed twice"), "{error}");
}

#[test]
fn an_fsrs_refusal_is_a_generation_error() {
    let fsrs = generate::revlog::Fsrs::new();
    assert!(matches!(fsrs.states(&[]), Err(GenError::Fsrs(_))));
    // A history the generator makes is never refused.
    assert_eq!(fsrs.states(&[(3, 0), (1, 2), (3, 2), (4, 9)]).unwrap().len(), 4);
}
