//! U15: the anonymized clone on synthetic collections built here
//! (`specs/fixtures/anonymizer.md` A20, R1941, R1915). Every collection is
//! made up; expected names, hashes, shapes and placeholder bytes come from
//! the spec (A11.1, A12.1, A13, A19, A10.3), never from the code under test.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::deck::{Deck, native_name};
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{DeckConfigId, DeckId, NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckCommon, DeckKind, DeckKindMessage, FilteredDeck, NormalDeck, SearchTerm};
use meguri_model::proto::deck_config::DeckConfigMessage;
use meguri_model::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
use meguri_model::rng::SeededRandom;
use meguri_model::tag::Tag;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_storage::{CollectionBuilder, OpenOptions, StampMode};
use meguri_testkit::anonymize::{self, AnonymizeError, Hooks, Report, Salt, Status};
use meguri_text::{checksum, media_refs, normalize};
use regex::Regex;
use sha2::{Digest, Sha256};

// ------------------------------------------------------------------ helpers

const CRT: i64 = 1_893_470_400;
const BASIC: i64 = 100;
const CLOZE: i64 = 101;
const OCCLUSION: i64 = 102;

/// A19's test salt: the bytes 00 01 ... 1f.
fn salt() -> Salt {
    Salt::new(std::array::from_fn(|i| i as u8))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn normal_deck(id: i64, human: &str, description: &str) -> Deck {
    let mut normal = NormalDeck::default();
    normal.config_id = 1;
    normal.description = description.to_owned();
    deck(id, human, DeckKind::Normal(normal))
}

fn filtered_deck(id: i64, human: &str, search: &str) -> Deck {
    let mut filtered = FilteredDeck::default();
    filtered.reschedule = true;
    filtered.search_terms = vec![SearchTerm::new(search.to_owned(), 100, 1)];
    deck(id, human, DeckKind::Filtered(filtered))
}

fn deck(id: i64, human: &str, kind: DeckKind) -> Deck {
    Deck {
        id: DeckId(id),
        name: native_name(human),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        common: ProtoBlob::new(DeckCommon::default()),
        kind: ProtoBlob::new(DeckKindMessage::new(kind)),
    }
}

fn notetype(id: i64, name: &str, fields: &[&str], stock_kind: i32) -> Notetype {
    let mut template = TemplateConfig::default();
    template.q_format = format!("{{{{{}}}}}", fields[0]);
    template.a_format = format!("{{{{{}}}}}", fields[fields.len() - 1]);
    let mut config = NotetypeConfig::default();
    config.original_stock_kind = stock_kind;
    config.kind = i32::from(stock_kind == 5 || stock_kind == 6);
    Notetype {
        id: NotetypeId(id),
        name: name.to_owned(),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        config: ProtoBlob::new(config),
        fields: fields
            .iter()
            .enumerate()
            .map(|(ord, field)| NotetypeField {
                ord: ord as u32,
                name: (*field).to_owned(),
                config: ProtoBlob::new(FieldConfig::default()),
                prior_ord: None,
            })
            .collect(),
        templates: vec![NotetypeTemplate {
            ord: 0,
            name: "Card 1".to_owned(),
            mtime: EpochSecs(0),
            usn: Usn(0),
            config: ProtoBlob::new(template),
            prior_ord: None,
        }],
    }
}

fn note(id: i64, notetype: i64, fields: &[&str], tags: &str) -> Note {
    Note {
        id: NoteId(id),
        guid: format!("g{id}"),
        notetype_id: NotetypeId(notetype),
        mtime: EpochSecs(CRT),
        usn: Usn(0),
        tags: tags.to_owned(),
        fields: fields.join("\u{1f}"),
        sort_field: String::new(),
        checksum: 0,
        flags: 0,
        data: String::new(),
    }
}

fn tag(name: &str) -> Tag {
    Tag {
        name: name.to_owned(),
        usn: Usn(0),
        collapsed: false,
        config: None,
    }
}

fn config(key: &str, json: &str) -> ConfigEntry {
    ConfigEntry {
        key: key.to_owned(),
        value: json.as_bytes().to_vec(),
        ..ConfigEntry::default()
    }
}

/// What a synthetic collection holds.
struct Spec {
    decks: Vec<Deck>,
    notes: Vec<Note>,
    tags: Vec<&'static str>,
    config: Vec<ConfigEntry>,
    media: Vec<(String, Vec<u8>)>,
    /// The default preset's FSRS optimizer search (`param_search`).
    preset_search: &'static str,
}

impl Spec {
    fn new() -> Spec {
        Spec {
            decks: vec![normal_deck(1, "Default", "")],
            notes: Vec::new(),
            tags: Vec::new(),
            config: Vec::new(),
            media: Vec::new(),
            preset_search: "",
        }
    }
}

/// Builds `spec` in `dir`: rows through meguri-storage's builder, cards,
/// review history and graves through raw SQL, then ANALYZE.
fn build(dir: &Path, spec: Spec) {
    fs::create_dir_all(dir).unwrap();
    let clock = FixedClock::new(CRT * 1000, UtcOffset::UTC);
    let random = SeededRandom::new(7);
    let col = Col {
        version: 18,
        created: EpochSecs(CRT),
        modified: EpochMillis(CRT * 1000),
        schema_modified: EpochMillis(CRT * 1000),
        ..Col::default()
    };
    let mut entries = vec![config("schedVer", "2"), config("curDeck", "1")];
    entries.extend(spec.config);
    let builder = CollectionBuilder::create(
        &dir.join("collection.anki2"),
        &col,
        &entries,
        &OpenOptions::new(&clock, &random),
    )
    .unwrap();
    {
        let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
        let mut preset = DeckConfigMessage::default();
        preset.param_search = spec.preset_search.to_owned();
        scope
            .put(&DeckConfig {
                id: DeckConfigId(1),
                name: "Default".to_owned(),
                mtime: EpochSecs(CRT),
                usn: Usn(0),
                config: ProtoBlob::new(preset),
            })
            .unwrap();
        for d in &spec.decks {
            scope.put(d).unwrap();
        }
        scope
            .put_notetype(&notetype(BASIC, "Basic", &["Front", "Back"], 1))
            .unwrap();
        scope
            .put_notetype(&notetype(CLOZE, "Cloze", &["Text", "Back Extra"], 5))
            .unwrap();
        scope
            .put_notetype(&notetype(
                OCCLUSION,
                "Image Occlusion",
                &["Occlusion", "Image", "Header", "Comments"],
                6,
            ))
            .unwrap();
        for t in &spec.tags {
            scope.put(&tag(t)).unwrap();
        }
        for n in &spec.notes {
            scope.put(n).unwrap();
        }
        scope.commit().unwrap();
    }
    builder.finish().unwrap();
    let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
    for (i, n) in spec.notes.iter().enumerate() {
        let did = if i % 2 == 0 { 1 } else { spec.decks.last().unwrap().id.0 };
        db.execute(
            "INSERT INTO cards VALUES (?1, ?2, ?3, 0, ?4, 0, 2, 2, 400, 30, 2500, 5, 1, 0, 0, 0, 0, '{\"s\":12.5,\"d\":5.1}')",
            &[
                RawValue::Integer(n.id.0 + 7),
                RawValue::Integer(n.id.0),
                RawValue::Integer(did),
                RawValue::Integer(CRT),
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO revlog VALUES (?1, ?2, 0, 3, 30, 10, 2500, 6000, 1)",
            &[RawValue::Integer(CRT * 1000 + i as i64), RawValue::Integer(n.id.0 + 7)],
        )
        .unwrap();
    }
    db.execute("INSERT INTO graves VALUES (12345, 0, -1)", &[]).unwrap();
    db.execute_batch("ANALYZE").unwrap();
    db.close().unwrap();
    if !spec.media.is_empty() {
        let media = dir.join("collection.media");
        fs::create_dir_all(&media).unwrap();
        for (name, bytes) in &spec.media {
            fs::write(media.join(name), bytes).unwrap();
        }
    }
}

/// A 3 × 2 PNG that is not a placeholder: a valid IHDR, then other bytes.
fn source_png() -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x03\0\0\0\x02\x08\x02\0\0\0".to_vec();
    png.resize(300, 0x5a);
    png
}

/// A 3 × 2 JPEG head: SOI, an APP0 segment, SOF0 (height 2, width 3).
fn source_jpeg() -> Vec<u8> {
    let mut jpeg = vec![
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x06, b'J', b'F', b'I', b'F', 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x02, 0x00,
        0x03, 0x03,
    ];
    jpeg.resize(200, 0x11);
    jpeg
}

/// The notes of the A19 tables, by id.
const N_HUND: i64 = CRT * 1000 + 1;
const N_ENTITIES: i64 = CRT * 1000 + 2;
const N_ATTRIBUTES: i64 = CRT * 1000 + 3;
const N_CLOZE: i64 = CRT * 1000 + 4;
const N_OCCLUSION: i64 = CRT * 1000 + 5;
const N_FURIGANA: i64 = CRT * 1000 + 6;
const N_MEDIA: i64 = CRT * 1000 + 7;
const D_JAPANESE: i64 = 1_700_000_000_003;
const D_FILTERED: i64 = 1_700_000_000_099;

const A19_REFERENCES: &str = "<img src=\"Photo.JPG\"> <img src=\"a&amp;b.png\"> [sound:notes.tar.gz] [sound:README] \
                              <img src=\"日本.png\"> <img src=\"a b.jpg\"> <img src=\"_font.woff2\">";

/// The A20 collection: A19's fields and media, A11.1's decks, A12's tags,
/// a filtered deck and A13.5's config keys. `kept_names` adds the places
/// that keep a deck name on purpose (V4's review items).
fn standard(dir: &Path, kept_names: bool) {
    let mut spec = Spec::new();
    spec.decks.extend([
        normal_deck(1_700_000_000_001, "german", ""),
        normal_deck(D_JAPANESE, "Japanese", "<b>Hund</b> und [sound:dog.mp3]"),
        normal_deck(1_700_000_000_004, "Japanese::Audio", ""),
        normal_deck(1_700_000_000_005, "Japanese::Grammar", ""),
        filtered_deck(D_FILTERED, "zz filter", "deck:Japanese -tag:jp*"),
    ]);
    spec.notes = vec![
        note(
            N_HUND,
            BASIC,
            &["<b>Hund</b> {{c1::der Hund::animal}} [sound:dog.mp3]", "dog.mp3"],
            " jp::n5 ",
        ),
        note(
            N_ENTITIES,
            BASIC,
            &["Tom &amp; Jerry&nbsp;<br>", "caf&eacute;"],
            " JP::N5 leech ",
        ),
        note(
            N_ATTRIBUTES,
            BASIC,
            &[
                "<span style=\"color: rgb(255, 0, 0);\" title=\"Note\">Rot</span>",
                "<a href=\"https://example.org/x\">x</a>",
            ],
            " jp ",
        ),
        note(
            N_CLOZE,
            CLOZE,
            &["{{c1,2::a}} {{c1::x {{c2::y}} z}} {{c3::", "{{x1::y}}"],
            "jp  JP",
        ),
        note(
            N_OCCLUSION,
            OCCLUSION,
            &[
                "{{c1::image-occlusion:rect:left=.25:top=.1:width=.2:height=.1:oi=1}} \
                 {{c2::image-occlusion:text:left=.5:top=.5:text=Leber\\:Herz:scale=1}}",
                "<img src=\"occl.png\">",
                "<img src=\"occl2.jpg\">",
                "Leber links",
            ],
            "",
        ),
        note(
            N_FURIGANA,
            BASIC,
            &["日本[にほん]は 東京[とうきょう]", "出[だ;k1]す"],
            " Straße german::verb ",
        ),
        note(N_MEDIA, BASIC, &["Cat", A19_REFERENCES], " STRASSE spanish::verb "),
    ];
    spec.tags = vec!["jp::n5", "jp", "leech", "Straße", "german::verb", "spanish::verb"];
    spec.config = vec![
        config("defaultSearchText", "\"deck:Japanese tag:jp::n5 is:due Hund\""),
        config(
            "savedFilters",
            r#"{"My Japanese":"deck:*a*","broken":"\"open","nowhere":"-deck:Nowhere","regex":"tag:re:^jp front:*Hund*"}"#,
        ),
        config("flagLabels", r#"{"1":"Wichtig","2":"später"}"#),
    ];
    if kept_names {
        // A key outside A13.5 that names a deck, and a preset's optimizer
        // search (A4): copied, and V4 lists them.
        spec.config.push(config("addonDecks", "\"Japanese\""));
        spec.preset_search = "deck:Japanese";
    }
    spec.media = vec![
        ("dog.mp3".into(), vec![3u8; 1234]),
        ("photo.jpg".into(), vec![4u8; 500]),
        ("a&b.png".into(), vec![5u8; 40]),
        ("notes.tar.gz".into(), vec![6u8; 64]),
        ("README".into(), b"read me".to_vec()),
        ("日本.png".into(), vec![7u8; 10]),
        ("a b.jpg".into(), vec![8u8; 11]),
        ("_font.woff2".into(), b"font bytes, kept as they are".to_vec()),
        ("Cat".into(), vec![9u8; 3]),
        ("occl.png".into(), source_png()),
        ("occl2.jpg".into(), source_jpeg()),
    ];
    build(dir, spec);
}

struct Clone {
    dir: PathBuf,
    report: Report,
}

fn run(source: &Path, out: &Path) -> Clone {
    let report = anonymize::anonymize(source, out, &salt()).unwrap();
    Clone {
        dir: out.to_owned(),
        report,
    }
}

impl Clone {
    fn db(&self) -> PlainDatabase {
        PlainDatabase::open(&self.dir.join("collection.anki2")).unwrap()
    }

    fn fields(&self, id: i64) -> Vec<String> {
        let rows = self
            .db()
            .rows("SELECT flds FROM notes WHERE id = ?1", &[RawValue::Integer(id)])
            .unwrap();
        text(&rows[0][0]).split('\u{1f}').map(str::to_owned).collect()
    }

    fn report_json(&self) -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(self.dir.join("anonymize.json")).unwrap()).unwrap()
    }

    fn media(&self) -> BTreeSet<String> {
        fs::read_dir(self.dir.join("collection.media"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect()
    }
}

fn text(value: &RawValue) -> String {
    match value {
        RawValue::Text(b) | RawValue::Blob(b) => String::from_utf8(b.clone()).unwrap(),
        other => panic!("not text: {other:?}"),
    }
}

fn assert_shape(actual: &str, pattern: &str) {
    let re = Regex::new(&format!("^(?:{pattern})$")).unwrap();
    assert!(re.is_match(actual), "{actual:?} does not have the shape {pattern}");
}

fn standard_clone(kept_names: bool) -> (tempfile::TempDir, Clone) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    standard(&source, kept_names);
    let clone = run(&source, &dir.path().join("clone"));
    (dir, clone)
}

// ---------------------------------------------------- A19's field and runs

#[test]
fn the_a19_field_keeps_its_shape_and_its_media_file() {
    let (_dir, clone) = standard_clone(false);
    let fields = clone.fields(N_HUND);
    assert_shape(
        &fields[0],
        r"<b>[A-Z][a-z]{3}</b> \{\{c1::[a-z]{3} [A-Z][a-z]{3}::[a-z]{6}\}\} \[sound:aae169c91f46c422\.mp3\]",
    );
    assert_eq!(fields[1], "aae169c91f46c422.mp3");
    let media = clone.dir.join("collection.media").join("aae169c91f46c422.mp3");
    assert_eq!(fs::metadata(&media).unwrap().len(), 1234);
    assert_ne!(
        fs::read(&media).unwrap(),
        vec![3u8; 1234],
        "the sound's bytes are replaced"
    );

    // sfld and csum as meguri-text computes them for the clone field.
    let row = clone
        .db()
        .rows(
            "SELECT sfld, csum FROM notes WHERE id = ?1",
            &[RawValue::Integer(N_HUND)],
        )
        .unwrap()
        .remove(0);
    let saved = normalize::field_as_saved(&fields[0], true);
    assert_eq!(row[0], RawValue::text(&checksum::sort_text(&saved)));
    assert_eq!(row[1], RawValue::Integer(i64::from(checksum::note_checksum(&saved))));
}

#[test]
fn the_same_source_and_salt_give_a_byte_identical_clone() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    standard(&source, false);
    let a = run(&source, &dir.path().join("a"));
    let b = run(&source, &dir.path().join("b"));
    for name in ["collection.anki2", "anonymize.json"] {
        assert_eq!(
            fs::read(a.dir.join(name)).unwrap(),
            fs::read(b.dir.join(name)).unwrap(),
            "{name}"
        );
    }
    assert_eq!(a.media(), b.media());
    for name in a.media() {
        let path = |c: &Clone| c.dir.join("collection.media").join(&name);
        assert_eq!(fs::read(path(&a)).unwrap(), fs::read(path(&b)).unwrap(), "{name}");
    }
    // Another salt: other text, same structure.
    let other = anonymize::anonymize(&source, &dir.path().join("c"), &Salt::new([9; 32])).unwrap();
    assert_eq!(other.json["notes"], a.report.json["notes"]);
    let c_fields = Clone {
        dir: dir.path().join("c"),
        report: other,
    }
    .fields(N_HUND);
    assert_ne!(c_fields[0], a.fields(N_HUND)[0]);
}

#[test]
fn a19_source_texts_get_their_clone_shapes() {
    let (_dir, clone) = standard_clone(false);
    let entities = clone.fields(N_ENTITIES);
    assert_shape(&entities[0], "[A-Z][a-z]{2} &amp; [A-Z][a-z]{4}&nbsp;<br>");
    assert_shape(&entities[1], "[a-z]{3}[\u{DF}-\u{F6}\u{F8}-\u{FF}]");

    let attributes = clone.fields(N_ATTRIBUTES);
    assert_shape(
        &attributes[0],
        r#"<span style="color: rgb\(255, 0, 0\);" title="[A-Z][a-z]{3}">[A-Z][a-z]{2}</span>"#,
    );
    assert_shape(
        &attributes[1],
        r#"<a href="https://[a-z]{7}\.[a-z]{3}/[a-z]">[a-z]</a>"#,
    );

    let cloze = clone.fields(N_CLOZE);
    assert_shape(
        &cloze[0],
        r"\{\{c1,2::[a-z]\}\} \{\{c1::[a-z] \{\{c2::[a-z]\}\} [a-z]\}\} \{\{c3::",
    );
    // Not an opener shape: G1 keeps the `c` out, and the digit is replaced.
    assert_shape(&cloze[1], r"\{\{[abd-z][0-9]::[a-z]\}\}");

    let occlusion = clone.fields(N_OCCLUSION);
    assert_shape(
        &occlusion[0],
        r"\{\{c1::image-occlusion:rect:left=\.25:top=\.1:width=\.2:height=\.1:oi=1\}\} \{\{c2::image-occlusion:text:left=\.5:top=\.5:text=[A-Z][a-z]{4}\\:[A-Z][a-z]{3}:scale=1\}\}",
    );
    // A10.2: no field of an image-occlusion note is kept whole; the
    // comments are text.
    assert_shape(&occlusion[3], "[A-Z][a-z]{4} [a-z]{5}");

    let han = "[\u{4E00}-\u{9FA5}]";
    let hira = "[\u{3041}-\u{3096}]";
    let furigana = clone.fields(N_FURIGANA);
    assert_shape(
        &furigana[0],
        &format!(r"{han}{{2}}\[{hira}{{3}}\]{hira} {han}{{2}}\[{hira}{{5}}\]"),
    );
    assert_shape(&furigana[1], &format!(r"{han}\[{hira};k1\]{hira}"));

    // `Cat` has no extension: text. A field holding a file name: its clone.
    let media = clone.fields(N_MEDIA);
    assert_shape(&media[0], "[A-Z][a-z]{2}");
}

#[test]
fn attribute_values_after_whitespace_around_the_equals_sign_are_values() {
    // A6.1: an attribute's `=` and its value, in HTML's syntax, which allows
    // whitespace (spaces, tabs, line breaks) on either side of the `=`. The
    // value is then replaced or kept by A6.2 like any other; the whitespace
    // and the `=` are kept.
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let mut spec = Spec::new();
    spec.notes = vec![
        note(
            CRT * 1000 + 1,
            BASIC,
            &["<span title = \"Secret\">x</span>", "<span title\t=\t'Geheim'>x</span>"],
            "",
        ),
        note(
            CRT * 1000 + 2,
            BASIC,
            &[
                "<abbr\ntitle\n=\nVertraulich\n>x</abbr>",
                "<span hidden class = \"Merkmal\" alt  =  Kennwort data-x = >x</span>",
            ],
            "",
        ),
    ];
    build(&source, spec);
    let clone = run(&source, &dir.path().join("clone"));
    let first = clone.fields(CRT * 1000 + 1);
    assert_shape(&first[0], r#"<span title = "[A-Z][a-z]{5}">[a-z]</span>"#);
    assert_shape(&first[1], "<span title\t=\t'[A-Z][a-z]{5}'>[a-z]</span>");
    let second = clone.fields(CRT * 1000 + 2);
    assert_shape(&second[0], "<abbr\ntitle\n=\n[A-Z][a-z]{10}\n>[a-z]</abbr>");
    // `class` is kept (A6.2); `hidden` has no value; `alt` is replaced; an
    // `=` before the `>` gives an empty value.
    assert_shape(
        &second[1],
        r#"<span hidden class = "Merkmal" alt  =  [A-Z][a-z]{7} data-x = >[a-z]</span>"#,
    );
    for field in first.iter().chain(&second) {
        for word in ["Secret", "Geheim", "Vertraulich", "Kennwort"] {
            assert!(!field.contains(word), "{word} kept in {field:?}");
        }
    }
    assert_eq!(clone.report.json["checks"]["structure"], "pass");
}

#[test]
fn a19_media_names_hash_as_the_spec_lists() {
    let (_dir, clone) = standard_clone(false);
    let refs = media_refs::references(&clone.fields(N_MEDIA)[1]);
    assert_eq!(
        refs,
        [
            "8716100710f92e8a.jpg",
            "7c860c6a8a6650f4.png",
            "8c36fbb97b3ca668.png",
            "23abcb0818f98f68.jpg",
            "_font.woff2",
            "29d55ad821dec71d.gz",
            "d2ba4fd1b7428819",
        ]
    );
    let media = clone.media();
    for name in [
        "aae169c91f46c422.mp3",
        "8716100710f92e8a.jpg",
        "7c860c6a8a6650f4.png",
        "29d55ad821dec71d.gz",
        "d2ba4fd1b7428819",
        "8c36fbb97b3ca668.png",
        "23abcb0818f98f68.jpg",
        "_font.woff2",
    ] {
        assert!(media.contains(name), "{name} missing from {media:?}");
    }
    assert_eq!(media.len(), 11, "one clone file per source file: {media:?}");
    assert_eq!(
        fs::read(clone.dir.join("collection.media/_font.woff2")).unwrap(),
        b"font bytes, kept as they are"
    );
    for (name, size) in [
        ("8716100710f92e8a.jpg", 500),
        ("d2ba4fd1b7428819", 7),
        ("29d55ad821dec71d.gz", 64),
    ] {
        assert_eq!(
            fs::metadata(clone.dir.join("collection.media").join(name))
                .unwrap()
                .len(),
            size,
            "{name}"
        );
    }
    assert_eq!(
        clone.report.json["salt_sha256"],
        "630dcd2966c4336691125448bbb25b4ff412a49c732db2c8abc1b8581bd710dd"
    );
}

#[test]
fn image_occlusion_images_become_black_pngs_of_their_size() {
    let (_dir, clone) = standard_clone(false);
    let fields = clone.fields(N_OCCLUSION);
    // A10.3's 3 × 2 vector, padded with zeros to each source's size.
    for (field, source_len) in [(&fields[1], 300usize), (&fields[2], 200)] {
        let name = media_refs::references(field).remove(0);
        let bytes = fs::read(clone.dir.join("collection.media").join(&name)).unwrap();
        assert_eq!(bytes.len(), source_len, "{name}");
        assert_eq!(
            sha256_hex(&bytes[..67]),
            "d3bfdfaae80e2d8dfd859fb9f4d382a6368fde337ddea9dd7d79a6e32a143176",
            "{name}"
        );
        assert!(bytes[67..].iter().all(|b| *b == 0), "{name}");
    }
    let report = clone.report_json();
    assert_eq!(report["media"]["io_images"], 2);
    assert_eq!(report["media"]["io_images_larger"], 0);
    assert_eq!(report["checks"]["media"], "pass");
}

#[test]
fn guards_keep_random_text_from_forming_syntax() {
    // A5.4: hundreds of places where a free draw would sometimes make a
    // cloze opener (G1), a sound tag (G2) or an entity (G3). V1 fails the
    // run on any of them, so the run succeeding is the check; the texts
    // are checked too.
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    let mut spec = Spec::new();
    let openers = "{{x1::y}} ".repeat(300);
    let sounds = "[sounx:a.mp3] ".repeat(300);
    let entities = "&xx; &#x4q; ".repeat(300);
    spec.notes = vec![note(
        CRT * 1000 + 1,
        BASIC,
        &[&openers, &format!("{sounds}{entities}")],
        "",
    )];
    build(&source, spec);
    let clone = run(&source, &dir.path().join("clone"));
    let fields = clone.fields(CRT * 1000 + 1);
    assert!(!fields[0].contains("{{c"), "G1");
    assert_shape(&fields[0], r"(\{\{[abd-z][0-9]::[a-z]\}\} ){300}");
    assert!(!fields[1].contains("[sound:"), "G2");
    let entity = Regex::new(r"&(?:[A-Za-z0-9]+|#[0-9]+|#x[0-9a-fA-F]+);").unwrap();
    for found in entity.find_iter(&fields[1]) {
        assert!(
            meguri_text::entities::decode_text(found.as_str()) == found.as_str(),
            "G3: {} is an entity",
            found.as_str()
        );
    }
    assert_eq!(clone.report.json["fields"]["total"], 2);
}

// ------------------------------------------------------------- decks, tags

fn deck_names(clone: &Clone) -> BTreeMap<i64, String> {
    clone
        .db()
        .rows("SELECT id, name FROM decks", &[])
        .unwrap()
        .into_iter()
        .map(|r| match &r[0] {
            RawValue::Integer(id) => (*id, text(&r[1]).replace('\u{1f}', "::")),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn deck_names_become_ranks_in_sibling_order() {
    let (_dir, clone) = standard_clone(false);
    let names = deck_names(&clone);
    assert_eq!(names[&1], "d0");
    assert_eq!(names[&1_700_000_000_001], "d1");
    assert_eq!(names[&D_JAPANESE], "d2");
    assert_eq!(names[&1_700_000_000_004], "d2::d0");
    assert_eq!(names[&1_700_000_000_005], "d2::d1");
    assert_eq!(names[&D_FILTERED], "d3");

    // Eleven siblings, and a deck whose parent is missing.
    let dir = tempfile::tempdir().unwrap();
    let mut spec = Spec::new();
    for (i, c) in "abcdefghijk".chars().enumerate() {
        spec.decks.push(normal_deck(2_000 + i as i64, &format!("P::{c}"), ""));
    }
    spec.decks.push(normal_deck(3_000, "A::B", ""));
    spec.decks.push(normal_deck(3_001, "P", ""));
    let source = dir.path().join("source");
    build(&source, spec);
    let names = deck_names(&run(&source, &dir.path().join("clone")));
    assert_eq!(names[&3_000], "d0::d0");
    assert_eq!(names[&1], "d1");
    assert_eq!(names[&3_001], "d2");
    for i in 0..11 {
        assert_eq!(names[&(2_000 + i)], format!("d2::d{i:02}"));
    }
}

#[test]
fn tags_hash_per_component_and_note_tag_strings_are_canonical() {
    let (_dir, clone) = standard_clone(false);
    let db = clone.db();
    let note_tags = |id: i64| {
        text(
            &db.rows("SELECT tags FROM notes WHERE id = ?1", &[RawValue::Integer(id)])
                .unwrap()[0][0],
        )
    };
    assert_eq!(note_tags(N_HUND), " 97ed61c408f64a10::56fcd14408509d6b ");
    assert_eq!(note_tags(N_ENTITIES), " 97ed61c408f64a10::56fcd14408509d6b leech ");
    assert_eq!(note_tags(N_ATTRIBUTES), " 97ed61c408f64a10 ");
    // `jp  JP` was not canonical: one tag, spaced.
    assert_eq!(note_tags(N_CLOZE), " 97ed61c408f64a10 ");
    assert_eq!(note_tags(N_OCCLUSION), "");
    assert_eq!(
        note_tags(N_FURIGANA),
        " 4d6b561dad60f313 86dd12acb57c5398::87bee7bf6e40e099 "
    );
    assert_eq!(
        note_tags(N_MEDIA),
        " 4d6b561dad60f313 b347e7b99054d409::87bee7bf6e40e099 "
    );
    let table: BTreeSet<String> = db
        .rows("SELECT tag FROM tags", &[])
        .unwrap()
        .iter()
        .map(|r| text(&r[0]))
        .collect();
    let expected: BTreeSet<String> = [
        "97ed61c408f64a10::56fcd14408509d6b",
        "97ed61c408f64a10",
        "leech",
        "4d6b561dad60f313",
        "86dd12acb57c5398::87bee7bf6e40e099",
        "b347e7b99054d409::87bee7bf6e40e099",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(table, expected);
    assert_eq!(clone.report.json["tags"]["kept"], 1);
    assert_eq!(clone.report.json["tags"]["renamed"], 5);
}

// ---------------------------------------------------------------- searches

fn config_value(clone: &Clone, key: &str) -> serde_json::Value {
    let rows = clone
        .db()
        .rows("SELECT val FROM config WHERE KEY = ?1", &[RawValue::text(key)])
        .unwrap();
    serde_json::from_str(&text(&rows[0][0])).unwrap()
}

#[test]
fn searches_select_the_same_decks_and_tags() {
    let (_dir, clone) = standard_clone(false);
    // The filtered deck's search, read from its kind blob.
    let kind = clone
        .db()
        .rows("SELECT kind FROM decks WHERE id = ?1", &[RawValue::Integer(D_FILTERED)])
        .unwrap()
        .remove(0)
        .remove(0);
    let RawValue::Blob(kind) = kind else {
        panic!("kind is a blob")
    };
    let message = ProtoBlob::<DeckKindMessage>::decode(&kind).unwrap();
    let DeckKind::Filtered(filtered) = &message.kind else {
        panic!("a filtered deck")
    };
    assert_eq!(filtered.search_terms[0].search, "deck:d2 -tag:97ed61c408f64a10");
    assert_eq!(filtered.search_terms[0].limit, 100);
    assert!(filtered.reschedule);

    let default = config_value(&clone, "defaultSearchText");
    assert_shape(
        default.as_str().unwrap(),
        "deck:d2 tag:97ed61c408f64a10::56fcd14408509d6b is:due [A-Z][a-z]{3}",
    );

    let saved = config_value(&clone, "savedFilters");
    let saved = saved.as_object().unwrap();
    assert_eq!(saved.len(), 4);
    let values: BTreeSet<&str> = saved.values().map(|v| v.as_str().unwrap()).collect();
    assert!(values.contains("(deck:d0 OR deck:d1 OR deck:d2)"), "{values:?}");
    assert!(values.contains("-did:0"), "{values:?}");
    let unparsed: Vec<&&str> = values.iter().filter(|v| v.starts_with('"')).collect();
    assert_eq!(unparsed.len(), 1);
    assert_shape(unparsed[0], "\"[a-z]{4}");
    // An inexact tag term keeps its prefix; a field term keeps its key and
    // wildcards.
    assert!(
        values
            .iter()
            .any(|v| Regex::new(r"^tag:re:\^[a-z]{2} front:\*[A-Z][a-z]{3}\*$")
                .unwrap()
                .is_match(v)),
        "{values:?}"
    );
    let names: Vec<&String> = saved.keys().collect();
    assert!(
        names
            .iter()
            .any(|n| Regex::new("^[A-Z][a-z] [A-Z][a-z]{7}$").unwrap().is_match(n)),
        "{names:?}"
    );

    let labels = config_value(&clone, "flagLabels");
    assert_shape(labels["1"].as_str().unwrap(), "[A-Z][a-z]{6}");
    assert_shape(
        labels["2"].as_str().unwrap(),
        "[a-z]{2}[\u{DF}-\u{F6}\u{F8}-\u{FF}][a-z]{3}",
    );

    let report = clone.report_json();
    assert_eq!(report["searches"]["rewritten"], 6);
    assert_eq!(report["searches"]["unparsed"], 1);
    assert_eq!(report["searches"]["inexact"], 1);
    assert_eq!(report["decks"]["searches"], 1);
    assert_eq!(report["decks"]["descriptions"], 1);
}

#[test]
fn a_deck_description_is_rewritten_as_field_text() {
    let (_dir, clone) = standard_clone(false);
    let kind = clone
        .db()
        .rows("SELECT kind FROM decks WHERE id = ?1", &[RawValue::Integer(D_JAPANESE)])
        .unwrap()
        .remove(0)
        .remove(0);
    let RawValue::Blob(kind) = kind else {
        panic!("kind is a blob")
    };
    let message = ProtoBlob::<DeckKindMessage>::decode(&kind).unwrap();
    let DeckKind::Normal(normal) = &message.kind else {
        panic!("a normal deck")
    };
    assert_shape(
        &normal.description,
        r"<b>[A-Z][a-z]{3}</b> [a-z]{3} \[sound:aae169c91f46c422\.mp3\]",
    );
    assert_eq!(normal.config_id, 1);
}

// ------------------------------------------------------------------- V2

fn all_rows(db: &PlainDatabase, sql: &str) -> Vec<Vec<RawValue>> {
    db.rows(sql, &[]).unwrap()
}

#[test]
fn every_table_the_spec_keeps_is_unchanged_and_name_samples_are_gone() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    standard(&source, false);
    let clone = run(&source, &dir.path().join("clone"));
    let (src, out) = (
        PlainDatabase::open(&source.join("collection.anki2")).unwrap(),
        clone.db(),
    );
    for table in [
        "col",
        "cards",
        "revlog",
        "graves",
        "deck_config",
        "notetypes",
        "fields",
        "templates",
        "sqlite_stat1",
    ] {
        let sql = format!("SELECT * FROM {table} ORDER BY 1, 2");
        let rows = all_rows(&src, &sql);
        assert!(!rows.is_empty(), "{table} has rows");
        assert_eq!(rows, all_rows(&out, &sql), "{table}");
    }
    let kept_notes = "SELECT id, guid, mid, mod, usn, flags, data FROM notes ORDER BY id";
    assert_eq!(all_rows(&src, kept_notes), all_rows(&out, kept_notes));
    let kept_decks = "SELECT id, mtime_secs, usn, common FROM decks ORDER BY id";
    assert_eq!(all_rows(&src, kept_decks), all_rows(&out, kept_decks));
    let other_config =
        "SELECT * FROM config WHERE KEY NOT IN ('defaultSearchText', 'savedFilters', 'flagLabels') ORDER BY KEY";
    assert_eq!(all_rows(&src, other_config), all_rows(&out, other_config));
    let config_keys = "SELECT KEY, usn, mtime_secs FROM config ORDER BY KEY";
    assert_eq!(all_rows(&src, config_keys), all_rows(&out, config_keys));
    assert_eq!(
        all_rows(&src, "SELECT count(*) FROM tags"),
        all_rows(&out, "SELECT count(*) FROM tags")
    );

    // A14.2's indexes, each checked on the source first: a name that
    // matches no row would delete nothing and still pass the clone's check
    // (`tags` is the WITHOUT ROWID table's own name, not an autoindex).
    let mut before = 0;
    for idx in ["idx_decks_name", "ix_notes_csum", "tags"] {
        let count = format!("SELECT count(*) FROM sqlite_stat4 WHERE idx = '{idx}'");
        let RawValue::Integer(n) = all_rows(&src, &count)[0][0] else {
            panic!("{idx}")
        };
        assert!(n > 0, "ANALYZE sampled {idx} in the source");
        assert_eq!(all_rows(&out, &count)[0][0], RawValue::Integer(0), "{idx}");
        before += n;
    }
    let others = "SELECT * FROM sqlite_stat4 WHERE idx NOT IN ('idx_decks_name', 'ix_notes_csum', 'tags') ORDER BY 1, 2, 3, 4, 5, 6";
    let kept = all_rows(&src, others);
    assert!(!kept.is_empty(), "the source has other samples");
    assert_eq!(kept, all_rows(&out, others));
    assert_eq!(clone.report.json["stat4_rows_deleted"], before);
    assert_eq!(clone.report.json["checks"]["rows"], "pass");
    assert_eq!(clone.report.json["checks"]["structure"], "pass");

    // Nothing but the clone, its media folder and the report; no log.
    let entries: BTreeSet<String> = fs::read_dir(&clone.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        entries,
        ["anonymize.json", "collection.anki2", "collection.media"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
}

// ------------------------------------------------------------------- V4

#[test]
fn a_config_value_naming_a_deck_is_a_review_item() {
    let (_dir, clone) = standard_clone(true);
    assert_eq!(clone.report.leak, Status::Warn);
    let report = clone.report_json();
    assert_eq!(report["checks"]["leak"]["status"], "warn");
    let locations = report["checks"]["leak"]["locations"].as_array().unwrap();
    assert!(
        locations
            .iter()
            .any(|l| l["table"] == "config" && l["column"] == "val" && l["key"] == "addonDecks" && l["hits"] == 1),
        "{locations:?}"
    );
    // The copied value is unchanged.
    assert_eq!(config_value(&clone, "addonDecks"), "Japanese");
    // The report never holds the token.
    assert!(
        !fs::read_to_string(clone.dir.join("anonymize.json"))
            .unwrap()
            .contains("Japanese")
    );
}

fn leak_locations(report: &Report) -> Vec<serde_json::Value> {
    report.json["checks"]["leak"]["locations"].as_array().unwrap().clone()
}

#[test]
fn a_preset_search_naming_a_deck_is_a_review_item() {
    // deck_config is unchanged (A4) but scanned: the default preset's
    // param_search names the deck Japanese.
    let (_dir, clone) = standard_clone(true);
    assert_eq!(clone.report.leak, Status::Warn);
    let locations = leak_locations(&clone.report);
    assert!(
        locations
            .iter()
            .any(|l| l["table"] == "deck_config" && l["column"] == "config" && l["key"] == 1 && l["hits"] == 1),
        "{locations:?}"
    );
    let preset = clone
        .db()
        .rows("SELECT config FROM deck_config WHERE id = 1", &[])
        .unwrap()
        .remove(0)
        .remove(0);
    let RawValue::Blob(preset) = preset else {
        panic!("config is a blob")
    };
    assert_eq!(
        ProtoBlob::<DeckConfigMessage>::decode(&preset).unwrap().param_search,
        "deck:Japanese"
    );

    // Without it the preset is no location.
    let (_dir, plain) = standard_clone(false);
    let locations = leak_locations(&plain.report);
    assert!(!locations.iter().any(|l| l["table"] == "deck_config"), "{locations:?}");
}

#[test]
fn a_deck_name_left_in_place_fails_the_leak_scan() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    standard(&source, false);
    let hooks = Hooks {
        keep_deck_name: Some(D_JAPANESE),
        ..Hooks::default()
    };
    let report = anonymize::anonymize_with(&source, &dir.path().join("clone"), &salt(), &hooks).unwrap();
    assert_eq!(report.leak, Status::Fail);
    assert_eq!(report.status(), Status::Fail);
    let locations = leak_locations(&report);
    assert!(
        locations
            .iter()
            .any(|l| l["table"] == "decks" && l["column"] == "name" && l["key"] == D_JAPANESE),
        "{locations:?}"
    );
}

#[test]
fn a_tag_name_left_in_a_stat4_sample_fails_the_leak_scan() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    standard(&source, false);
    let in_samples = |report: &Report| {
        leak_locations(report)
            .iter()
            .any(|l| l["table"] == "sqlite_stat4" && l["column"] == "sample")
    };

    // A14.2 removes the tag samples: no sample holds a token. (Table and
    // index names such as `notes`, a run of the media name notes.tar.gz,
    // stay review items.)
    let clean = run(&source, &dir.path().join("clean"));
    assert_ne!(clean.report.leak, Status::Fail);
    assert!(!in_samples(&clean.report), "{:?}", leak_locations(&clean.report));

    // Left in place, the samples hold the source's tag names (Straße,
    // german::verb, ...).
    let hooks = Hooks {
        keep_tag_samples: true,
        ..Hooks::default()
    };
    let out = dir.path().join("clone");
    let report = anonymize::anonymize_with(&source, &out, &salt(), &hooks).unwrap();
    assert_eq!(report.leak, Status::Fail);
    assert_eq!(report.status(), Status::Fail);
    assert!(in_samples(&report), "{:?}", leak_locations(&report));
    let text = fs::read_to_string(out.join("anonymize.json")).unwrap();
    for token in ["Straße", "german", "spanish"] {
        assert!(!text.contains(token), "the report holds {token}");
    }
}

// ------------------------------------------------------------------ errors

fn small(dir: &Path) {
    let mut spec = Spec::new();
    spec.notes = vec![note(CRT * 1000 + 1, BASIC, &["Hund", "dog"], "")];
    build(dir, spec);
}

#[test]
fn errors_name_their_kind_and_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    small(&source);

    // out_not_empty: the existing file stays.
    let busy = dir.path().join("busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("keep.txt"), b"x").unwrap();
    let err = anonymize::anonymize(&source, &busy, &salt()).unwrap_err();
    assert_eq!(err.kind(), "out_not_empty");
    assert!(busy.join("keep.txt").exists());

    // usage: inside the source.
    let err = anonymize::anonymize(&source, &source.join("clone"), &salt()).unwrap_err();
    assert_eq!(err.kind(), "usage");
    assert!(!source.join("clone").exists());

    // no_collection.
    let empty = dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let err = anonymize::anonymize(&empty, &dir.path().join("o1"), &salt()).unwrap_err();
    assert_eq!(err.kind(), "no_collection");

    // invalid_utf8: names the note; the output directory it made is gone.
    let bad = dir.path().join("bad");
    small(&bad);
    let db = PlainDatabase::open(&bad.join("collection.anki2")).unwrap();
    db.execute(
        "UPDATE notes SET flds = ?1",
        &[RawValue::Text(vec![b'a', 0xFF, 0x1F, b'b'])],
    )
    .unwrap();
    db.close().unwrap();
    let out = dir.path().join("o2");
    let err = anonymize::anonymize(&bad, &out, &salt()).unwrap_err();
    assert!(
        matches!(err, AnonymizeError::InvalidUtf8 { note } if note == CRT * 1000 + 1),
        "{err:?}"
    );
    assert_eq!(err.kind(), "invalid_utf8");
    assert!(!out.exists());

    // An empty output directory that existed is emptied, not removed.
    let kept = dir.path().join("o3");
    fs::create_dir(&kept).unwrap();
    anonymize::anonymize(&bad, &kept, &salt()).unwrap_err();
    assert!(kept.is_dir());
    assert_eq!(fs::read_dir(&kept).unwrap().count(), 0);

    // unsupported_schema and open_failed.
    let old = dir.path().join("old");
    small(&old);
    let db = PlainDatabase::open(&old.join("collection.anki2")).unwrap();
    db.execute("UPDATE col SET ver = 11", &[]).unwrap();
    db.close().unwrap();
    assert_eq!(
        anonymize::anonymize(&old, &dir.path().join("o4"), &salt())
            .unwrap_err()
            .kind(),
        "unsupported_schema"
    );
    let junk = dir.path().join("junk");
    fs::create_dir(&junk).unwrap();
    fs::write(junk.join("collection.anki2"), vec![0x42u8; 4096]).unwrap();
    assert_eq!(
        anonymize::anonymize(&junk, &dir.path().join("o5"), &salt())
            .unwrap_err()
            .kind(),
        "open_failed"
    );
}

#[test]
fn salt_files_hold_64_lower_case_hex_digits() {
    let hex: String = (0u8..32).map(|b| format!("{b:02x}")).collect();
    assert_eq!(Salt::parse(hex.as_bytes()).unwrap(), salt());
    assert_eq!(Salt::parse(format!("{hex}\n").as_bytes()).unwrap(), salt());
    assert_eq!(salt().to_file_contents(), format!("{hex}\n"));
    for bad in [
        format!("{hex}\n\n"),
        hex.to_uppercase(),
        hex[..62].to_owned(),
        format!("{hex}0"),
        format!(" {hex}"),
        format!("{}g", &hex[..63]),
    ] {
        assert_eq!(Salt::parse(bad.as_bytes()).unwrap_err().kind(), "salt", "{bad:?}");
    }
}

#[test]
fn two_media_files_equal_under_folding_collide() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source");
    small(&source);
    let media = source.join("collection.media");
    fs::create_dir(&media).unwrap();
    fs::write(media.join("Dog.mp3"), b"a").unwrap();
    fs::write(media.join("dog.mp3"), b"b").unwrap();
    if fs::read_dir(&media).unwrap().count() < 2 {
        // A case-insensitive file system cannot hold both (A20 asks for a
        // case-sensitive one; tools/mg core-test runs on Linux).
        return;
    }
    let out = dir.path().join("clone");
    let err = anonymize::anonymize(&source, &out, &salt()).unwrap_err();
    assert_eq!(err.kind(), "media_name_collision");
    assert!(!out.exists());
}
