//! Schema-11 JSON conversion (specs/model/schema11-json.md) in both
//! directions, with the model goldens as input-output pairs (S14): a blob read
//! next to a `legacy_json` observation gives a record and the object Anki
//! converts it to; a `legacy_update` patch applied to the object observed
//! before it gives an object and the record Anki saves from it.

mod common;

use common::{assert_same_json, golden, merge_patch, object, parse_json, rows, scenario, unhex};
use meguri_model::deck::{Deck, human_name, native_name};
use meguri_model::deck_config::DeckConfig;
use meguri_model::ids::{DeckConfigId, DeckId, NotetypeId};
use meguri_model::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use meguri_model::proto::ProtoBlob;
use meguri_model::schema11::col;
use meguri_model::schema11::{
    deck_config_from_json, deck_config_to_json, deck_from_json, deck_to_json, notetype_from_json, notetype_to_json,
};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_model::{ModelError, Schema11Item};
use serde_json::Value;

/// 2030-01-15T10:00:00Z, the scenarios' frozen clock, in seconds.
const CLOCK_SECS: i64 = 1_894_701_600;

fn int(value: &Value) -> i64 {
    value.as_i64().unwrap_or_else(|| panic!("{value} is not an integer"))
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_else(|| panic!("{value} is not a string"))
}

fn blob(value: &Value) -> Vec<u8> {
    unhex(text(value))
}

/// The blob literals (`X'…'`) of a scenario's SQL, in order.
fn blob_literals(sql: &str) -> Vec<Vec<u8>> {
    sql.split("X'")
        .skip(1)
        .map(|rest| unhex(&rest[..rest.find('\'').unwrap()]))
        .collect()
}

/// The SQL of step `i` of a scenario.
fn sql(scenario: &Value, i: usize) -> String {
    text(&scenario["steps"][i]["db_exec"]["sql"]).to_owned()
}

/// The patch of a `legacy_update` step.
fn patch(scenario: &Value, i: usize) -> &Value {
    let patch = &scenario["steps"][i]["legacy_update"]["patch"];
    assert!(patch.is_object(), "step {i} is not a legacy_update");
    patch
}

fn patched(object: &Value, patch: &Value) -> String {
    let mut object = object.clone();
    merge_patch(&mut object, patch);
    serde_json::to_string(&object).unwrap()
}

fn json_of(text: &str) -> Value {
    parse_json(text)
}

fn deck(id: i64, native: &str, mtime: i64, usn: i64, common: &[u8], kind: &[u8]) -> Deck {
    Deck {
        id: DeckId(id),
        name: native.to_owned(),
        mtime: EpochSecs(mtime),
        usn: Usn(usn as i32),
        common: ProtoBlob::decode(common).unwrap(),
        kind: ProtoBlob::decode(kind).unwrap(),
    }
}

fn preset(id: i64, name: &str, mtime: i64, usn: i64, config: &[u8]) -> DeckConfig {
    DeckConfig {
        id: DeckConfigId(id),
        name: name.to_owned(),
        mtime: EpochSecs(mtime),
        usn: Usn(usn as i32),
        config: ProtoBlob::decode(config).unwrap(),
    }
}

/// A note type from its blobs; the row values the reads leave out come from
/// the observed object.
fn notetype(object: &Value, config: &[u8], fields: &[(&str, Vec<u8>)], templates: &[(&str, Vec<u8>)]) -> Notetype {
    Notetype {
        id: NotetypeId(int(&object["id"])),
        name: text(&object["name"]).to_owned(),
        mtime: EpochSecs(int(&object["mod"])),
        usn: Usn(int(&object["usn"]) as i32),
        config: ProtoBlob::decode(config).unwrap(),
        fields: fields
            .iter()
            .enumerate()
            .map(|(ord, (name, config))| NotetypeField {
                ord: ord as u32,
                name: (*name).to_owned(),
                config: ProtoBlob::decode(config).unwrap(),
                prior_ord: None,
            })
            .collect(),
        templates: templates
            .iter()
            .enumerate()
            .map(|(ord, (name, config))| NotetypeTemplate {
                ord: ord as u32,
                name: (*name).to_owned(),
                mtime: EpochSecs(0),
                usn: Usn(0),
                config: ProtoBlob::decode(config).unwrap(),
                prior_ord: None,
            })
            .collect(),
    }
}

fn assert_notetype_blobs(notetype: &Notetype, config: &[u8], fields: &[Vec<u8>], templates: &[Vec<u8>], what: &str) {
    assert_eq!(
        common::hex(&notetype.config.to_bytes()),
        common::hex(config),
        "{what}: notetype config"
    );
    assert_eq!(notetype.fields.len(), fields.len(), "{what}: fields");
    for (field, bytes) in notetype.fields.iter().zip(fields) {
        assert_eq!(
            common::hex(&field.config.to_bytes()),
            common::hex(bytes),
            "{what}: field {}",
            field.name
        );
    }
    assert_eq!(notetype.templates.len(), templates.len(), "{what}: templates");
    for (template, bytes) in notetype.templates.iter().zip(templates) {
        assert_eq!(
            common::hex(&template.config.to_bytes()),
            common::hex(bytes),
            "{what}: template {}",
            template.name
        );
    }
}

fn assert_deck_blobs(deck: &Deck, common_bytes: &[u8], kind: &[u8], what: &str) {
    assert_eq!(
        common::hex(&deck.common.to_bytes()),
        common::hex(common_bytes),
        "{what}: common"
    );
    assert_eq!(common::hex(&deck.kind.to_bytes()), common::hex(kind), "{what}: kind");
}

fn assert_refused(result: Result<impl std::fmt::Debug, ModelError>, item: Schema11Item, what: &str) {
    match result {
        Err(ModelError::InvalidJson { item: refused, .. }) => assert_eq!(refused, item, "{what}"),
        other => panic!("{what}: expected a refusal, got {other:?}"),
    }
}

/// A preset saved through `legacy_update` is a local change (model-ops L2).
fn saved_preset(object_text: &str) -> DeckConfig {
    let mut preset = deck_config_from_json(object_text).unwrap();
    preset.usn = Usn::PENDING;
    preset.mtime = EpochSecs(CLOCK_SECS);
    preset
}

#[test]
fn preset_blobs_and_objects_convert_both_ways() {
    let g = golden("model/proto/preset-bytes");
    for row in rows(&g, 1) {
        let name = text(&row[1]);
        let record = preset(int(&row[0]), name, int(&row[2]), int(&row[3]), &blob(&row[4]));
        let expected = object(&g, 2, "presets", name);
        assert_same_json(&json_of(&deck_config_to_json(&record)), expected, name);

        let back = deck_config_from_json(&expected.to_string()).unwrap();
        assert_eq!(back.id, record.id);
        assert_eq!(back.name, name);
        assert_eq!((back.mtime, back.usn), (record.mtime, record.usn));
        assert_eq!(
            common::hex(&back.config.to_bytes()),
            text(&row[4]),
            "{name}: object to blob"
        );
    }
}

#[test]
fn spec_example_default_preset_object() {
    // specs/model/schema11-json.md S13, the new collection's Default preset.
    let g = golden("model/proto/preset-bytes");
    let row = rows(&g, 1).iter().find(|row| row[1] == "Default").unwrap();
    let record = preset(1, "Default", 0, 0, &blob(&row[4]));
    let expected = r#"{"id":1,"mod":0,"name":"Default","usn":0,"maxTaken":60,"autoplay":true,"timer":0,"replayq":true,"new":{"bury":false,"delays":[1.0,10.0],"initialFactor":2500,"ints":[1,4,0],"order":1,"perDay":20},"rev":{"bury":false,"ease4":1.3,"ivlFct":1.0,"maxIvl":36500,"perDay":200,"hardFactor":1.2},"lapse":{"delays":[10.0],"leechAction":1,"leechFails":8,"minInt":1,"mult":0.0},"dyn":false,"newMix":0,"newPerDayMinimum":0,"interdayLearningMix":0,"reviewOrder":0,"newSortOrder":0,"newGatherPriority":0,"buryInterdayLearning":false,"fsrsWeights":[],"fsrsParams5":[],"fsrsParams6":[],"desiredRetention":0.9,"ignoreRevlogsBeforeDate":"","easyDaysPercentages":[1.0,1.0,1.0,1.0,1.0,1.0,1.0],"stopTimerOnAnswer":false,"secondsToShowQuestion":0.0,"secondsToShowAnswer":0.0,"questionAction":0,"answerAction":0,"waitForAudio":true,"sm2Retention":0.9,"weightSearch":""}"#;
    assert_eq!(deck_config_to_json(&record), expected);
}

#[test]
fn normal_deck_blobs_and_objects_convert_both_ways() {
    let g = golden("model/proto/deck-normal-bytes");
    for row in rows(&g, 1) {
        let native = text(&row[1]);
        let record = deck(
            int(&row[0]),
            native,
            int(&row[2]),
            int(&row[3]),
            &blob(&row[4]),
            &blob(&row[5]),
        );
        let expected = object(&g, 2, "decks", &human_name(native));
        assert_same_json(&json_of(&deck_to_json(&record)), expected, native);

        let back = deck_from_json(&expected.to_string()).unwrap();
        assert_eq!((back.id, back.name.as_str()), (record.id, native));
        assert_eq!((back.mtime, back.usn), (record.mtime, record.usn));
        assert_deck_blobs(&back, &blob(&row[4]), &blob(&row[5]), native);
    }
}

/// specs/model/schema11-json.md S13, the new collection's Default deck.
const DEFAULT_DECK_OBJECT: &str = r#"{"id":1,"mod":0,"name":"Default","usn":0,"lrnToday":[0,0],"revToday":[0,0],"newToday":[0,0],"timeToday":[0,0],"collapsed":true,"browserCollapsed":true,"desc":"","dyn":0,"conf":1,"extendNew":0,"extendRev":0,"reviewLimit":null,"newLimit":null,"reviewLimitToday":null,"newLimitToday":null,"desiredRetention":null}"#;

/// The Default deck's `common` and `kind` blobs in a new oracle collection.
fn default_deck_blobs() -> (Vec<u8>, Vec<u8>) {
    let g = golden("model/proto/deck-normal-bytes");
    let row = rows(&g, 1).iter().find(|row| row[1] == "Default").unwrap();
    (blob(&row[4]), blob(&row[5]))
}

#[test]
fn spec_example_default_deck_object() {
    let (common_bytes, kind) = default_deck_blobs();
    let record = deck(1, "Default", 0, 0, &common_bytes, &kind);
    assert_eq!(deck_to_json(&record), DEFAULT_DECK_OBJECT);
}

#[test]
fn filtered_deck_converts_and_saves_lenient_values() {
    let id = "model/proto/deck-filtered-bytes";
    let (s, g) = (scenario(id), golden(id));
    let inserted = blob_literals(&sql(&s, 0));
    let record = deck(1_500_000_000_000, "Cram", 1_500_000_000, 0, &inserted[0], &inserted[1]);
    let before = object(&g, 1, "decks", "Cram");
    assert_same_json(&json_of(&deck_to_json(&record)), before, "inserted filtered deck");

    let saved = deck_from_json(&patched(before, patch(&s, 2))).unwrap();
    let row = &rows(&g, 3)[0];
    assert_eq!((saved.id.0, saved.name.as_str()), (int(&row[0]), text(&row[1])));
    assert_eq!((saved.mtime.0, i64::from(saved.usn.0)), (int(&row[2]), int(&row[3])));
    assert_deck_blobs(&saved, &blob(&row[4]), &blob(&row[5]), "saved filtered deck");
    assert_same_json(
        &json_of(&deck_to_json(&saved)),
        object(&g, 4, "decks", "Cram"),
        "after the save",
    );
}

#[test]
fn notetype_blobs_and_objects_convert_both_ways() {
    let g = golden("model/proto/notetype-bytes");
    for row in rows(&g, 2) {
        let name = text(&row[0]);
        let expected = object(&g, 5, "notetypes", name);
        let fields: Vec<(&str, Vec<u8>)> = rows(&g, 3)
            .iter()
            .filter(|f| f[0] == name)
            .map(|f| (text(&f[2]), blob(&f[3])))
            .collect();
        let templates: Vec<(&str, Vec<u8>)> = rows(&g, 4)
            .iter()
            .filter(|t| t[0] == name)
            .map(|t| (text(&t[2]), blob(&t[5])))
            .collect();
        let record = notetype(expected, &blob(&row[3]), &fields, &templates);
        assert_eq!((record.mtime.0, i64::from(record.usn.0)), (int(&row[1]), int(&row[2])));
        assert_same_json(&json_of(&notetype_to_json(&record)), expected, name);

        let back = notetype_from_json(&expected.to_string()).unwrap();
        assert_eq!((back.id, back.name.as_str()), (record.id, name));
        assert_eq!((back.mtime, back.usn), (record.mtime, record.usn));
        let field_blobs: Vec<Vec<u8>> = fields.iter().map(|(_, b)| b.clone()).collect();
        let template_blobs: Vec<Vec<u8>> = templates.iter().map(|(_, b)| b.clone()).collect();
        assert_notetype_blobs(&back, &blob(&row[3]), &field_blobs, &template_blobs, name);
        let names: Vec<&str> = back.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, fields.iter().map(|(n, _)| *n).collect::<Vec<_>>());
        // The object's `ord` is carried as the previous ordinal.
        assert!(back.fields.iter().all(|f| f.prior_ord == Some(f.ord)));
        assert!(back.templates.iter().all(|t| (t.mtime.0, t.usn.0) == (0, 0)));
    }
}

#[test]
fn notetype_values_use_lenient_and_bool_any_readers() {
    let id = "model/schema11/notetype-values";
    let (s, g) = (scenario(id), golden(id));
    let observed = object(&g, 4, "notetypes", "Pair");
    // Saving the step-0 patch again over what it produced gives the same
    // record: every key it sets is read again from the patch.
    let saved = notetype_from_json(&patched(observed, patch(&s, 0))).unwrap();
    let fields: Vec<Vec<u8>> = rows(&g, 2).iter().map(|f| blob(&f[2])).collect();
    let templates: Vec<Vec<u8>> = rows(&g, 3).iter().map(|t| blob(&t[2])).collect();
    assert_notetype_blobs(&saved, &blob(&rows(&g, 1)[0][0]), &fields, &templates, "saved");

    let named_fields: Vec<(&str, Vec<u8>)> = rows(&g, 2).iter().map(|f| (text(&f[1]), blob(&f[2]))).collect();
    let named_templates: Vec<(&str, Vec<u8>)> = rows(&g, 3).iter().map(|t| (text(&t[1]), blob(&t[2]))).collect();
    let record = notetype(observed, &blob(&rows(&g, 1)[0][0]), &named_fields, &named_templates);
    assert_same_json(&json_of(&notetype_to_json(&record)), observed, "record to object");
}

#[test]
fn notetype_other_drops_reserved_keys() {
    let id = "model/schema11/notetype-other-reserved";
    let (s, g) = (scenario(id), golden(id));
    let config = blob_literals(&sql(&s, 0)).remove(0);
    let front = blob_literals(&sql(&s, 1)).remove(0);
    let back = blob_literals(&sql(&s, 2)).remove(0);
    let forward = blob_literals(&sql(&s, 3)).remove(0);
    let observed = object(&g, 4, "notetypes", "Pair");
    let record = notetype(
        observed,
        &config,
        &[("Front", front), ("Back", back)],
        &[("Forward", forward)],
    );
    assert_same_json(&json_of(&notetype_to_json(&record)), observed, "record to object");

    let saved = notetype_from_json(&patched(observed, patch(&s, 5))).unwrap();
    let fields: Vec<Vec<u8>> = rows(&g, 7).iter().map(|f| blob(&f[1])).collect();
    let templates: Vec<Vec<u8>> = rows(&g, 8).iter().map(|t| blob(&t[1])).collect();
    assert_notetype_blobs(&saved, &blob(&rows(&g, 6)[0][0]), &fields, &templates, "saved");
}

#[test]
fn refused_notetype_objects() {
    let id = "model/schema11/notetype-rejections";
    let (s, g) = (scenario(id), golden(id));
    let accepted = object(&g, 13, "notetypes", "Pair");
    notetype_from_json(&accepted.to_string()).unwrap();
    for i in 1..=12 {
        assert_eq!(common::step(&g, i)["legacy_update"]["error"], "invalid_json");
        let result = notetype_from_json(&patched(accepted, patch(&s, i)));
        assert_refused(result, Schema11Item::Notetype, &format!("step {i}"));
    }
}

#[test]
fn deck_values_follow_the_day_counter_and_lenient_rules() {
    let id = "model/schema11/deck-values";
    let (s, g) = (scenario(id), golden(id));
    let first = object(&g, 2, "decks", "Words");
    let row = &rows(&g, 1)[0];
    let saved = deck_from_json(&patched(first, patch(&s, 0))).unwrap();
    assert_eq!(
        (saved.mtime.0, i64::from(saved.usn.0)),
        (int(&row[0]), int(&row[1])),
        "kept mod and usn"
    );
    assert_deck_blobs(&saved, &blob(&row[2]), &blob(&row[3]), "step 0");
    let record = deck(
        int(&first["id"]),
        "Words",
        int(&row[0]),
        int(&row[1]),
        &blob(&row[2]),
        &blob(&row[3]),
    );
    assert_same_json(&json_of(&deck_to_json(&record)), first, "step 2");

    let second = object(&g, 5, "decks", "Words");
    let saved = deck_from_json(&patched(first, patch(&s, 3))).unwrap();
    let row = &rows(&g, 4)[0];
    assert_deck_blobs(&saved, &blob(&row[2]), &blob(&row[3]), "step 3");
    assert_same_json(&json_of(&deck_to_json(&saved)), second, "step 5");

    let saved = deck_from_json(&patched(second, patch(&s, 6))).unwrap();
    assert_eq!(common::hex(&saved.kind.to_bytes()), text(&rows(&g, 7)[0][0]), "step 6");
    let saved = deck_from_json(&patched(&json_of(&deck_to_json(&saved)), patch(&s, 8))).unwrap();
    assert_eq!(common::hex(&saved.kind.to_bytes()), text(&rows(&g, 9)[0][0]), "step 8");
    assert_same_json(
        &json_of(&deck_to_json(&saved)),
        object(&g, 10, "decks", "Words"),
        "step 10",
    );
}

/// The common blob of a new deck in the oracle (deck-rejections reads it).
fn new_deck_common() -> Vec<u8> {
    blob(&rows(&golden("model/schema11/deck-rejections"), 11)[0][2])
}

fn appended(base: &[u8], sql: &str) -> Vec<u8> {
    let mut bytes = base.to_vec();
    bytes.extend(blob_literals(sql).remove(0));
    bytes
}

#[test]
fn deck_other_flattens_into_the_object() {
    let id = "model/schema11/deck-other-flatten";
    let (s, g) = (scenario(id), golden(id));
    let statement = sql(&s, 0);
    let literals = blob_literals(&statement);
    let common_bytes = appended(&new_deck_common(), &statement);
    let observed = object(&g, 1, "decks", "Words");
    let record = deck(
        int(&observed["id"]),
        "Words",
        int(&observed["mod"]),
        -1,
        &common_bytes,
        &literals[1],
    );
    assert_same_json(&json_of(&deck_to_json(&record)), observed, "record to object");

    // specs/model/schema11-json.md S3 example: converting back drops the
    // reserved extendNew member and keeps extend_new 3 from the record.
    let back = deck_from_json(&deck_to_json(&record)).unwrap();
    assert_eq!(back.common.other(), br#"{"browser_collapsed":true,"x_addon":[1]}"#);
    match &back.kind.kind {
        meguri_model::proto::deck::DeckKind::Normal(normal) => assert_eq!(normal.extend_new, 3),
        other => panic!("{other:?}"),
    }
}

#[test]
fn deck_other_round_trips() {
    let id = "model/schema11/deck-other-roundtrip";
    let (s, g) = (scenario(id), golden(id));
    let common_bytes = appended(&new_deck_common(), &sql(&s, 0));
    let kind = blob(&rows(&golden("model/schema11/deck-rejections"), 11)[0][3]);
    let observed = object(&g, 1, "decks", "Words");
    let record = deck(
        int(&observed["id"]),
        "Words",
        int(&observed["mod"]),
        -1,
        &common_bytes,
        &kind,
    );
    assert_same_json(&json_of(&deck_to_json(&record)), observed, "record to object");

    let saved = deck_from_json(&patched(observed, patch(&s, 2))).unwrap();
    let row = &rows(&g, 3)[0];
    assert_deck_blobs(&saved, &blob(&row[0]), &blob(&row[1]), "saved");
    assert_same_json(
        &json_of(&deck_to_json(&saved)),
        object(&g, 4, "decks", "Words"),
        "after the save",
    );
}

/// Field 255, `other` (specs/model/protobuf-records.md P7), as bytes to append
/// to a deck common blob: a later occurrence replaces the earlier one (P2).
fn other_field(other: &[u8]) -> Vec<u8> {
    let mut field = vec![0xfa, 0x0f];
    let mut len = other.len();
    while len >= 0x80 {
        field.push((len & 0x7f) as u8 | 0x80);
        len >>= 7;
    }
    field.push(len as u8);
    field.extend_from_slice(other);
    field
}

/// The Default deck whose `common.other` holds `other`.
fn default_deck_with_other(other: &str) -> Deck {
    let (mut common_bytes, kind) = default_deck_blobs();
    common_bytes.extend(other_field(other.as_bytes()));
    deck(1, "Default", 0, 0, &common_bytes, &kind)
}

/// The Default deck's object with `members` as its `other` members: after the
/// S7 keys, before the S8 keys.
fn default_deck_object_with(members: &str) -> String {
    DEFAULT_DECK_OBJECT.replacen(r#""dyn":0,"#, &format!(r#""dyn":0,{members},"#), 1)
}

#[test]
fn deck_other_without_reserved_keys_round_trips_byte_identical() {
    // S3 round trip: an `other` object with no reserved key and no key the
    // object writes comes back with the same bytes, its nested key order and
    // number text included (json-numbers N7). The collision-free counterpart
    // of the S3 example, whose reserved extendNew is dropped.
    let members = r#""browser_collapsed":true,"x_addon":[1,{"b":2,"a":1.10}]"#;
    let other = format!("{{{members}}}");
    let record = default_deck_with_other(&other);
    assert_eq!(record.common.other(), other.as_bytes());
    let object = deck_to_json(&record);
    assert_eq!(object, default_deck_object_with(members));
    let back = deck_from_json(&object).unwrap();
    assert_eq!(back.common.other(), other.as_bytes());
}

#[test]
fn deeply_nested_other_members_convert_in_one_pass() {
    // S3 in both directions with an `other` member nested 100,000 deep: its
    // text is kept exactly. At the bottom of the same nesting, a number
    // beyond the float range (json-numbers N10) or an unpaired surrogate
    // escape (N9) still makes the text invalid: output adds nothing and input
    // refuses the object.
    let members = format!(r#""x_deep":{}"#, common::deeply_nested("[1,\"x\"]"));
    let other = format!("{{{members}}}");
    let object = deck_to_json(&default_deck_with_other(&other));
    assert!(
        object == default_deck_object_with(&members),
        "deep member written as stored"
    );
    let back = deck_from_json(&object).unwrap();
    assert!(back.common.other() == other.as_bytes(), "deep member kept as read");

    for innermost in ["1e400", r#""\ud800""#] {
        let members = format!(r#""x_deep":{}"#, common::deeply_nested(innermost));
        let other = format!("{{{members}}}");
        assert!(
            deck_to_json(&default_deck_with_other(&other)) == DEFAULT_DECK_OBJECT,
            "{innermost}: other adds nothing"
        );
        assert_refused(
            deck_from_json(&default_deck_object_with(&members)),
            Schema11Item::Deck,
            innermost,
        );
    }
}

#[test]
fn refused_deck_objects() {
    let id = "model/schema11/deck-rejections";
    let (s, g) = (scenario(id), golden(id));
    let accepted = object(&g, 12, "decks", "Words");
    deck_from_json(&accepted.to_string()).unwrap();
    for i in 0..=10 {
        assert_eq!(common::step(&g, i)["legacy_update"]["error"], "invalid_json");
        let result = deck_from_json(&patched(accepted, patch(&s, i)));
        assert_refused(result, Schema11Item::Deck, &format!("step {i}"));
    }
}

#[test]
fn deck_names_are_stored_native() {
    let id = "model/schema11/deck-names";
    let (s, g) = (scenario(id), golden(id));
    let words = object(&g, 3, "decks", "A::B: x::blank");
    let renames = [(0, 2, 1), (1, 2, 2), (4, 6, 1), (5, 6, 2)];
    for (update, read, row) in renames {
        let saved = deck_from_json(&patched(words, patch(&s, update))).unwrap();
        let stored = &rows(&g, read)[row];
        assert_eq!(saved.name, text(&stored[0]), "step {update}");
        assert_eq!(common::hex(saved.name.as_bytes()), text(&stored[1]), "step {update}");
    }
    for (step, names) in [
        (3, ["A::B: x::blank", "a::b"]),
        (7, ["Café::日本語::😀 Emoji", "blank::Tail::blank"]),
    ] {
        for name in names {
            let observed = object(&g, step, "decks", name);
            assert_eq!(observed["name"], name);
            assert_eq!(human_name(&native_name(name)), name, "round trip of {name}");
        }
    }
}

#[test]
fn spec_deck_name_table() {
    // specs/model/schema11-json.md S11.
    let cases = [
        ("A::B", "A\u{1f}B", "A::B"),
        ("  A ::  :B: \u{7}x ::  ", "A\u{1f}B: x\u{1f}blank", "A::B: x::blank"),
        ("a:::b", "a\u{1f}b", "a::b"),
        ("::a", "blank\u{1f}a", "blank::a"),
        ("Cafe\u{301}", "Café", "Café"),
        ("日本語::😀 Emoji", "日本語\u{1f}😀 Emoji", "日本語::😀 Emoji"),
    ];
    for (human, native, shown) in cases {
        assert_eq!(native_name(human), native, "{human:?}");
        assert_eq!(human_name(native), shown, "{native:?}");
    }
}

#[test]
fn preset_values_follow_the_part_rules() {
    let id = "model/schema11/preset-values";
    let (s, g) = (scenario(id), golden(id));
    let first = object(&g, 2, "presets", "Daily");
    let row = &rows(&g, 1)[0];
    let saved = saved_preset(&patched(first, patch(&s, 0)));
    assert_eq!((saved.mtime.0, i64::from(saved.usn.0)), (int(&row[0]), int(&row[1])));
    assert_eq!(common::hex(&saved.config.to_bytes()), text(&row[2]), "step 0");
    let record = preset(int(&first["id"]), "Daily", int(&row[0]), int(&row[1]), &blob(&row[2]));
    assert_same_json(&json_of(&deck_config_to_json(&record)), first, "step 2");

    let second = object(&g, 5, "presets", "Daily");
    let saved = saved_preset(&patched(first, patch(&s, 3)));
    assert_eq!(
        common::hex(&saved.config.to_bytes()),
        text(&rows(&g, 4)[0][0]),
        "step 3"
    );
    assert_same_json(&json_of(&deck_config_to_json(&saved)), second, "step 5");

    let mut current = second.clone();
    for i in 6..=8 {
        let saved = saved_preset(&patched(&current, patch(&s, i)));
        current = json_of(&deck_config_to_json(&saved));
        if i == 8 {
            assert_eq!(
                common::hex(&saved.config.to_bytes()),
                text(&rows(&g, 9)[0][0]),
                "step 8"
            );
        }
    }
    assert_same_json(&current, object(&g, 10, "presets", "Daily"), "step 10");

    let saved = saved_preset(&patched(&current, patch(&s, 11)));
    assert_same_json(
        &json_of(&deck_config_to_json(&saved)),
        object(&g, 12, "presets", "Daily"),
        "step 12",
    );
}

/// The blob of the preset `{name: Daily, new_per_day: 15}` as the oracle
/// creates it (preset-rejections reads it unchanged).
fn daily_preset_blob() -> Vec<u8> {
    blob(&rows(&golden("model/schema11/preset-rejections"), 10)[0][2])
}

#[test]
fn preset_other_members_join_their_part_or_the_top() {
    let id = "model/schema11/preset-other";
    let (s, g) = (scenario(id), golden(id));
    let mut bytes = daily_preset_blob();
    for (update, observe) in [(0, 1), (2, 3), (4, 5), (6, 7)] {
        bytes = appended(&bytes, &sql(&s, update));
        let observed = object(&g, observe, "presets", "Daily");
        let record = preset(int(&observed["id"]), "Daily", int(&observed["mod"]), -1, &bytes);
        assert_same_json(
            &json_of(&deck_config_to_json(&record)),
            observed,
            &format!("step {observe}"),
        );
    }
}

#[test]
fn addon_preset_keys_survive_every_save() {
    let id = "model/schema11/preset-addon-roundtrip";
    let (s, g) = (scenario(id), golden(id));
    let default_row = rows(&golden("model/proto/preset-bytes"), 1)
        .iter()
        .find(|row| row[1] == "Default")
        .unwrap()
        .clone();
    let bytes = appended(&blob(&default_row[4]), &sql(&s, 0));
    let first = object(&g, 1, "presets", "Default");
    let record = preset(1, "Default", 0, 0, &bytes);
    assert_same_json(&json_of(&deck_config_to_json(&record)), first, "step 1");

    let saved = saved_preset(&patched(first, patch(&s, 2)));
    let second = object(&g, 3, "presets", "Default");
    assert_same_json(&json_of(&deck_config_to_json(&saved)), second, "step 3");

    let saved = saved_preset(&patched(second, patch(&s, 4)));
    assert_same_json(
        &json_of(&deck_config_to_json(&saved)),
        object(&g, 5, "presets", "Default"),
        "step 5",
    );

    // The model API's own FSRS-6 write keeps the add-on keys and the other
    // lists byte for byte (R1464, R1486).
    let stored = saved_preset(&patched(first, patch(&s, 2)))
        .config
        .to_bytes()
        .into_owned();
    let mut reread: ProtoBlob<meguri_model::proto::deck_config::DeckConfigMessage> =
        ProtoBlob::decode(&stored).unwrap();
    reread.set_fsrs_params_6(vec![0.5, 1.5]);
    let written = reread.to_bytes();
    let after = ProtoBlob::<meguri_model::proto::deck_config::DeckConfigMessage>::decode(&written).unwrap();
    assert_eq!(after.other(), reread.other());
    assert_eq!(after.fsrs_params_4(), [0.4, 0.6, 2.4, 5.8]);
    assert_eq!(after.fsrs_params_5(), [0.40255, 1.18385, 3.173, 15.69105]);
    assert_eq!(after.fsrs_params_6(), [0.5, 1.5]);
    let object_after = json_of(&deck_config_to_json(&preset(1, "Default", CLOCK_SECS, -1, &written)));
    for key in [
        "addon_balance",
        "addon_label",
        "addon_note",
        "addon_promotion",
        "addon_retire_after",
        "addon_tiers",
    ] {
        assert_eq!(object_after[key], second[key], "{key}");
    }
    assert_eq!(object_after["new"]["addon_new_cap"], 0);
    assert_eq!(object_after["lapse"]["addon_lapse_mode"], "off");
}

#[test]
fn preset_parts_that_take_their_defaults_drop_their_extras() {
    // S10.1 to S10.3 and S10.5 input rule 1: each part first keeps an extra
    // member in other, then takes its defaults and keeps nothing; a top-level
    // extra saved with a failing part stays.
    let id = "model/schema11/preset-part-defaults";
    let (s, g) = (scenario(id), golden(id));
    let first = object(&g, 4, "presets", "Daily");
    let record = preset(int(&first["id"]), "Daily", 0, 0, &daily_preset_blob());
    let mut current = json_of(&deck_config_to_json(&record));
    // Each legacy_update, with the step reading its blob back and the step
    // observing the object, when there is one.
    let saves = [
        (0, Some(1), None),
        (2, Some(3), Some(4)),
        (5, Some(6), None),
        (7, Some(8), Some(9)),
        (10, Some(11), None),
        (12, Some(13), Some(14)),
        (15, None, None),
        (16, Some(17), Some(18)),
    ];
    for (update, read, observed) in saves {
        let saved = saved_preset(&patched(&current, patch(&s, update)));
        if let Some(read) = read {
            assert_eq!(
                common::hex(&saved.config.to_bytes()),
                text(&rows(&g, read)[0][0]),
                "step {update}"
            );
        }
        current = json_of(&deck_config_to_json(&saved));
        if let Some(observed) = observed {
            assert_same_json(
                &current,
                object(&g, observed, "presets", "Daily"),
                &format!("step {observed}"),
            );
        }
    }
    // The spec example: an invalid `new` keeps nothing of its members.
    let saved = saved_preset(&patched(
        &current,
        &serde_json::json!({"new": {"initialFactor": 70000, "keepNew": 1, "keepNew2": 2}}),
    ));
    let object_after = json_of(&deck_config_to_json(&saved));
    assert_eq!(object_after["new"]["perDay"], 20);
    assert!(object_after["new"].get("keepNew").is_none());
}

#[test]
fn other_with_a_number_beyond_the_float_range_adds_nothing() {
    // json-numbers N10, S3 output rule 1: the whole other is invalid JSON.
    let id = "model/schema11/other-out-of-range";
    let (s, g) = (scenario(id), golden(id));
    let default_blob = blob(
        &rows(&golden("model/proto/preset-bytes"), 1)
            .iter()
            .find(|row| row[1] == "Default")
            .unwrap()[4],
    );
    for (step, name) in [(0, "Over"), (1, "Nested"), (2, "Edge"), (3, "Under"), (4, "Max")] {
        let observed = object(&g, 7, "presets", name);
        let bytes = appended(&default_blob, &sql(&s, step));
        let record = preset(int(&observed["id"]), name, int(&observed["mod"]), -1, &bytes);
        assert_same_json(&json_of(&deck_config_to_json(&record)), observed, name);
    }
    let kind = blob(&rows(&golden("model/schema11/deck-rejections"), 11)[0][3]);
    for (step, name) in [(5, "Words"), (6, "Kept")] {
        let observed = object(&g, 7, "decks", name);
        let common_bytes = appended(&new_deck_common(), &sql(&s, step));
        let record = deck(
            int(&observed["id"]),
            name,
            int(&observed["mod"]),
            -1,
            &common_bytes,
            &kind,
        );
        assert_same_json(&json_of(&deck_to_json(&record)), observed, name);
    }
}

#[test]
fn objects_with_a_number_beyond_the_float_range_are_refused() {
    // json-numbers N10: in a schema-11 object or a col column, such a number
    // makes the whole text invalid JSON, wherever it is.
    let with_member = |object: &Value, member: &str| {
        let text = object.to_string();
        format!("{},{member}}}", &text[..text.len() - 1])
    };
    let presets = golden("model/proto/preset-bytes");
    let preset_object = object(&presets, 2, "presets", "Default");
    for member in [
        r#""zz":1e400"#,
        r#""zz":{"a":[-1e400]}"#,
        r#""zz":1.7976931348623158e308"#,
    ] {
        let result = deck_config_from_json(&with_member(preset_object, member));
        assert_refused(result, Schema11Item::DeckConfig, member);
    }
    let text = with_member(preset_object, r#""zz":1.7976931348623157e308"#);
    assert!(deck_config_from_json(&text).is_ok());
    let text = with_member(preset_object, r#""zz":1e-400"#);
    assert!(deck_config_from_json(&text).is_ok());

    let decks = golden("model/schema11/deck-rejections");
    let deck_object = object(&decks, 12, "decks", "Words");
    assert_refused(
        deck_from_json(&with_member(deck_object, r#""zz":[1e400]"#)),
        Schema11Item::Deck,
        "deck",
    );
    let notetypes = golden("model/proto/notetype-bytes");
    let notetype_object = object(&notetypes, 5, "notetypes", "Pair");
    assert_refused(
        notetype_from_json(&with_member(notetype_object, r#""zz":1e400"#)),
        Schema11Item::Notetype,
        "notetype",
    );
    assert!(col::tags_from_json(r#"{"a":-1,"b":1e400}"#).is_err());
    assert!(col::conf_from_json(r#"{"nextPos":1e400}"#).is_err());
    assert!(col::conf_from_json(r#"{"nextPos":1e308}"#).is_ok());
}

#[test]
fn refused_preset_objects() {
    let id = "model/schema11/preset-rejections";
    let (s, g) = (scenario(id), golden(id));
    let accepted = object(&g, 11, "presets", "Daily");
    deck_config_from_json(&accepted.to_string()).unwrap();
    for i in 0..=9 {
        assert_eq!(common::step(&g, i)["legacy_update"]["error"], "invalid_json");
        let result = deck_config_from_json(&patched(accepted, patch(&s, i)));
        assert_refused(result, Schema11Item::DeckConfig, &format!("step {i}"));
    }
}

#[test]
fn col_columns_round_trip() {
    // S12: each column is an object keyed by id; output orders ids ascending.
    let g = golden("model/proto/notetype-bytes");
    let models = &common::step(&g, 5)["legacy_json"]["notetypes"];
    let mut by_id = serde_json::Map::new();
    for object in models.as_object().unwrap().values() {
        by_id.insert(object["id"].to_string(), object.clone());
    }
    let column = Value::Object(by_id).to_string();
    let notetypes = col::notetypes_from_json(&column).unwrap();
    assert_eq!(notetypes.len(), 2);
    let written = col::notetypes_to_json(&notetypes);
    assert_same_json(&json_of(&written), &json_of(&column), "models");
    let first_key = written[2..].split('"').next().unwrap().to_owned();
    assert_eq!(first_key, "1894701600006", "ids ascending");

    let decks = col::decks_to_json(&[]);
    assert_eq!(decks, "{}");
    assert!(col::decks_from_json("{}").unwrap().is_empty());

    // dconf: the member key is the preset's id.
    let presets_golden = golden("model/proto/preset-bytes");
    let preset_object = object(&presets_golden, 2, "presets", "Default");
    let dconf = format!(r#"{{"77":{preset_object}}}"#);
    let presets = col::deck_configs_from_json(&dconf).unwrap();
    assert_eq!(presets[0].id, DeckConfigId(77));
    let written = col::deck_configs_to_json(&presets);
    assert!(written.starts_with(r#"{"77":{"id":77,"#), "{written}");

    for bad in [
        r#"{"x1":{}}"#,
        r#"{"+1":{}}"#,
        r#"{"99999999999999999999":{}}"#,
        "[]",
        "",
    ] {
        assert!(col::notetypes_from_json(bad).is_err(), "{bad}");
    }
    let refused = col::deck_configs_from_json(r#"{"1":{"id":1}}"#).unwrap_err();
    assert_eq!(
        refused,
        ModelError::InvalidJson {
            item: Schema11Item::Column("dconf"),
            key: "1.mod".to_owned()
        }
    );

    let tags = vec![("zeta".to_owned(), Usn(-1)), ("alpha::beta".to_owned(), Usn(4))];
    let text = col::tags_to_json(&tags);
    assert_eq!(text, r#"{"zeta":-1,"alpha::beta":4}"#);
    assert_eq!(col::tags_from_json(&text).unwrap(), tags);
    assert!(col::tags_from_json(r#"{"a":"1"}"#).is_err());

    let conf = vec![
        meguri_model::config::ConfigEntry {
            key: "nextPos".to_owned(),
            value: b"5".to_vec(),
            ..Default::default()
        },
        meguri_model::config::ConfigEntry {
            key: "activeDecks".to_owned(),
            value: b"[1, 2]".to_vec(),
            ..Default::default()
        },
    ];
    let text = col::conf_to_json(&conf).unwrap();
    assert_eq!(text, r#"{"activeDecks":[1, 2],"nextPos":5}"#);
    assert_eq!(
        col::conf_from_json(&text).unwrap(),
        vec![
            ("activeDecks".to_owned(), b"[1, 2]".to_vec()),
            ("nextPos".to_owned(), b"5".to_vec())
        ]
    );
}

#[test]
fn readers_follow_json_numbers() {
    // json-numbers N2 and N6 through note type keys: `id` is int-or-string,
    // `sortf` a strict u16, field `sticky` bool-from-anything.
    let g = golden("model/proto/notetype-bytes");
    let base = object(&g, 5, "notetypes", "Pair");
    let accept = |patch: Value| notetype_from_json(&patched(base, &patch));
    assert_eq!(accept(serde_json::json!({"id": "+42"})).unwrap().id, NotetypeId(42));
    assert_eq!(accept(serde_json::json!({"id": "-0"})).unwrap().id, NotetypeId(0));
    for bad in [
        serde_json::json!({"id": " 5"}),
        serde_json::json!({"id": "5.0"}),
        serde_json::json!({"id": 5.0}),
        serde_json::json!({"id": "0x5"}),
        serde_json::json!({"id": ""}),
        serde_json::json!({"sortf": 65536}),
        serde_json::json!({"sortf": 1e0}),
    ] {
        assert!(accept(bad.clone()).is_err(), "{bad}");
    }
    let mut fields = base["flds"].clone();
    for (value, sticky) in [
        (serde_json::json!(true), Some(true)),
        (serde_json::json!(1), Some(true)),
        (serde_json::json!(1.0), Some(true)),
        (serde_json::json!(0.0), Some(false)),
        (serde_json::json!("TRUE"), Some(true)),
        (serde_json::json!("False"), Some(false)),
        (serde_json::json!("+1"), Some(true)),
        (serde_json::json!("1e0"), Some(true)),
        (serde_json::json!("0.0"), Some(false)),
        (serde_json::json!(2), None),
        (serde_json::json!(0.5), None),
        (serde_json::json!("yes"), None),
        (serde_json::json!(""), None),
        (serde_json::json!(" true"), None),
        (serde_json::json!("1 "), None),
        (serde_json::json!(null), None),
        (serde_json::json!([]), None),
    ] {
        fields[0]["sticky"] = value.clone();
        let result = accept(serde_json::json!({"flds": fields}));
        assert_eq!(result.ok().map(|n| n.fields[0].config.sticky), sticky, "sticky {value}");
    }
}
