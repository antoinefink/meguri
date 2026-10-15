//! The config table (`storage/config-table`; R323, R1466, R1472, R1474):
//! values written only when they change (C4) in compact form (C5), read the
//! way Anki reads them (C3), changed member by member (C6), keys Meguri does
//! not use kept byte for byte (C7), key families removed (C10). The scenario
//! replays compare stored rows with the oracle's goldens, deviations
//! applied.

mod common;

use common::{NOW_MS, card, conformance_dir, deck, empty_collection, exec, expected_steps, note, open, revlog, tag};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::NoteId;
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReadRows, StampMode, Written};
use serde_json::{Value, json};

const HOUR_MS: i64 = 3_600_000;

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

fn rows(writer: &meguri_storage::Writer, sql: &str) -> Value {
    Value::Array(
        exec(writer, sql)
            .rows
            .iter()
            .map(|row| {
                Value::Array(
                    row.iter()
                        .map(|v| match v {
                            SqlValue::Null => Value::Null,
                            SqlValue::Integer(i) => json!(i),
                            SqlValue::Real(f) => json!(f),
                            SqlValue::Text(t) => json!(t),
                            SqlValue::Blob(_) => panic!("read blobs as text"),
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

/// A scenario step's argument, as written in the scenario file.
fn scenario_step(id: &str, index: usize, op: &str) -> Value {
    let path = conformance_dir().join(format!("scenarios/{id}.yaml"));
    let loaded = meguri_testkit::scenario::load(&path).unwrap();
    loaded.value["steps"][index][op].clone()
}

/// A scenario's JSON value as a config value: numbers keep their kind.
fn config_value(value: &Value) -> ConfigValue {
    match value {
        Value::Null => ConfigValue::Null,
        Value::Bool(b) => ConfigValue::Bool(*b),
        Value::Number(n) if n.is_f64() => ConfigValue::float(n.as_f64().unwrap()).unwrap(),
        Value::Number(n) => n
            .as_i64()
            .map_or_else(|| ConfigValue::from(n.as_u64().unwrap()), ConfigValue::from),
        Value::String(s) => ConfigValue::String(s.clone()),
        Value::Array(items) => ConfigValue::Array(items.iter().map(config_value).collect()),
        Value::Object(map) => ConfigValue::Object(map.iter().map(|(k, v)| (k.clone(), config_value(v))).collect()),
    }
}

fn sql_param(value: &Value) -> SqlValue {
    match value {
        Value::String(s) => SqlValue::Text(s.clone()),
        Value::Number(n) => SqlValue::Integer(n.as_i64().unwrap()),
        other => panic!("{other}"),
    }
}

fn set(writer: &meguri_storage::Writer, clock: &StepClock, key: &str, value: &ConfigValue) -> Written {
    let mut scope = writer.begin_write(StampMode::Local, clock, None).unwrap();
    let written = scope.set_config(key, value).unwrap();
    scope.commit().unwrap();
    written
}

#[test]
fn rewriting_a_value_keeps_its_bytes_and_a_change_is_written_compact() {
    let id = "storage/config/rewrite-same-value";
    let steps = expected_steps(id);
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let insert = scenario_step(id, 0, "db_exec");
    writer
        .execute_raw(
            insert["sql"].as_str().unwrap(),
            &insert["params"]
                .as_array()
                .unwrap()
                .iter()
                .map(sql_param)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let select = "SELECT usn, mtime_secs, CAST(val AS TEXT), (SELECT mod FROM col) FROM config WHERE KEY = 'heatmap'";
    assert_eq!(rows(writer, select), steps[1]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    let same = config_value(&scenario_step(id, 3, "set_config")["value"]);
    assert_eq!(set(writer, &clock, "heatmap", &same), Written::Unchanged);
    assert_eq!(rows(writer, select), steps[4]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    let changed = config_value(&scenario_step(id, 6, "set_config")["value"]);
    assert_eq!(set(writer, &clock, "heatmap", &changed), Written::Updated);
    assert_eq!(rows(writer, select), steps[7]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    let reordered = config_value(&scenario_step(id, 9, "set_config")["value"]);
    assert_eq!(set(writer, &clock, "heatmap", &reordered), Written::Unchanged);
    assert_eq!(rows(writer, select), steps[10]["db_exec"]["rows"]);

    let probe = scenario_step(id, 11, "set_config");
    set(
        writer,
        &clock,
        probe["key"].as_str().unwrap(),
        &config_value(&probe["value"]),
    );
    assert_eq!(
        rows(
            writer,
            "SELECT usn, CAST(val AS TEXT), typeof(val) FROM config WHERE KEY = 'encodingProbe'"
        ),
        steps[12]["db_exec"]["rows"]
    );
}

#[test]
fn setting_the_current_deck_is_a_change_for_sync_and_rewriting_it_is_none() {
    let steps = expected_steps("storage/config/current-deck");
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    // The new collection's curDeck (the golden's first row).
    exec(
        writer,
        "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('curDeck', 0, 0, CAST('1' AS BLOB))",
    );
    let select = "SELECT usn, mtime_secs, CAST(val AS TEXT), (SELECT mod FROM col) FROM config WHERE KEY = 'curDeck'";
    assert_eq!(rows(writer, select), steps[0]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    assert_eq!(
        set(writer, &clock, "curDeck", &ConfigValue::from(1i64)),
        Written::Unchanged
    );
    assert_eq!(rows(writer, select), steps[3]["db_exec"]["rows"]);
    let inbox = steps[6]["db_exec"]["rows"][0][0].as_i64().unwrap();
    assert_eq!(
        set(writer, &clock, "curDeck", &ConfigValue::from(inbox)),
        Written::Updated
    );
    assert_eq!(rows(writer, select), steps[5]["db_exec"]["rows"]);
}

#[test]
fn a_value_stored_with_other_spacing_is_kept_when_written_again() {
    let steps = expected_steps("storage/config/other-bytes-kept");
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let deck_id = 1_894_701_600_000i64;
    exec(
        writer,
        &format!(
            "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('_nt_5_lastDeck', 3, 1700000000, CAST(' {deck_id} ' AS BLOB))"
        ),
    );
    clock.step_ms(HOUR_MS);
    assert_eq!(
        set(writer, &clock, "_nt_5_lastDeck", &ConfigValue::from(deck_id)),
        Written::Unchanged
    );
    let select =
        format!("SELECT usn, mtime_secs, CAST(val AS TEXT) = ' {deck_id} ' FROM config WHERE KEY = '_nt_5_lastDeck'");
    assert_eq!(rows(writer, &select), steps[4]["db_exec"]["rows"]);
}

/// C7 as a property: the scenario's unknown keys, with their layouts,
/// through 1,000 random operations that write notes, cards, review-log
/// rows, graves, tags, decks and other config keys.
#[test]
fn unknown_keys_survive_a_thousand_random_operations_byte_for_byte() {
    let id = "storage/config/unknown-keys-survive";
    let steps = expected_steps(id);
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    exec(writer, "PRAGMA synchronous = OFF");
    let clock = clock();
    let insert = scenario_step(id, 0, "db_exec");
    writer
        .execute_raw(
            insert["sql"].as_str().unwrap(),
            &insert["params"]
                .as_array()
                .unwrap()
                .iter()
                .map(sql_param)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let select = "SELECT KEY, usn, mtime_secs, CAST(val AS TEXT) FROM config \
                  WHERE KEY IN ('onigiri_sidebar_collapsed', '_nt_123_lastDeck', 'cardStateCustomizer') ORDER BY KEY";
    assert_eq!(rows(writer, select), steps[1]["db_exec"]["rows"]);
    let unknown_bytes = exec(
        writer,
        "SELECT hex(val), typeof(val) FROM config WHERE KEY IN ('onigiri_sidebar_collapsed', '_nt_123_lastDeck', 'cardStateCustomizer') ORDER BY KEY",
    );

    let random = SeededRandom::new(0x636f_6e66);
    let mut notes: Vec<NoteId> = Vec::new();
    for op in 0..1000u64 {
        clock.step_ms(61_000);
        let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
        match random.next_u64() % 9 {
            0 | 1 => {
                let id = scope.add_note(&note(1, &[&format!("n{op}"), "x"])).unwrap();
                scope.add_card(&card(id, 0)).unwrap();
                scope.set_config("curModel", &ConfigValue::from(1i64)).unwrap();
                scope.set_config("_nt_1_lastDeck", &ConfigValue::from(1i64)).unwrap();
                scope
                    .set_config("_deck_1_lastNotetype", &ConfigValue::from(1i64))
                    .unwrap();
                notes.push(id);
            }
            2 if !notes.is_empty() => {
                let id = notes[(random.next_u64() as usize) % notes.len()];
                let mut stored = scope.note(id).unwrap().unwrap();
                stored.fields = format!("e{op}\u{1f}x");
                stored.tags = " sky ".to_owned();
                scope.put(&stored).unwrap();
                scope.put(&tag("sky")).unwrap();
            }
            3 if !notes.is_empty() => {
                let id = notes.swap_remove((random.next_u64() as usize) % notes.len());
                scope.remove_note_and_cards(id).unwrap();
            }
            4 if !notes.is_empty() => {
                let id = notes[(random.next_u64() as usize) % notes.len()];
                if let Some(mut stored) = scope.cards_of_note(id).unwrap().pop() {
                    stored.reps += 1;
                    scope.put(&stored).unwrap();
                    scope.add_revlog(&revlog(stored.id, 2500)).unwrap();
                }
            }
            5 => {
                scope.set_config("curDeck", &ConfigValue::from(op as i64)).unwrap();
                scope
                    .set_config("lastUnburied", &ConfigValue::from(op as i64 / 7))
                    .unwrap();
            }
            6 => {
                scope
                    .set_config_members("meguriExample", &[("count", ConfigValue::from(op as i64))])
                    .unwrap();
            }
            7 => {
                scope.add_deck(&deck(&format!("deck{op}"))).unwrap();
            }
            _ => {
                // A note type's family whose id shares a prefix with the
                // unknown `_nt_123_` key.
                scope.set_config("_nt_12_lastDeck", &ConfigValue::from(1i64)).unwrap();
                scope.remove_config_family("_nt_12_").unwrap();
                scope.remove_config_family("_nt_1_").unwrap();
            }
        }
        scope.commit().unwrap();
    }
    let last = steps.len() - 1;
    assert_eq!(rows(writer, select), steps[last]["db_exec"]["rows"]);
    assert_eq!(
        exec(
            writer,
            "SELECT hex(val), typeof(val) FROM config WHERE KEY IN ('onigiri_sidebar_collapsed', '_nt_123_lastDeck', 'cardStateCustomizer') ORDER BY KEY",
        ),
        unknown_bytes
    );
}

/// C5's table.
#[test]
fn values_are_written_in_the_compact_form() {
    let object = ConfigValue::Object(vec![
        ("weeks".into(), ConfigValue::from(52i64)),
        ("mode".into(), ConfigValue::from("year")),
        ("streak".into(), ConfigValue::Bool(true)),
    ]);
    let array = ConfigValue::Array(vec![
        ConfigValue::from(1i64),
        ConfigValue::float(2.5).unwrap(),
        ConfigValue::from("Été"),
        ConfigValue::Object(vec![
            ("b".into(), ConfigValue::Array(vec![])),
            ("a".into(), ConfigValue::Object(vec![])),
        ]),
    ]);
    let sorted = ConfigValue::Object(
        [("b", 1), ("a1", 4), ("_x", 3), ("B", 2), ("a", 5)]
            .iter()
            .map(|(k, v)| ((*k).to_owned(), ConfigValue::from(*v as i64)))
            .collect(),
    );
    let cases: [(ConfigValue, &str); 8] = [
        (object, r#"{"mode":"year","streak":true,"weeks":52}"#),
        (array, r#"[1,2.5,"Été",{"a":{},"b":[]}]"#),
        (ConfigValue::from(1_894_701_600_000i64), "1894701600000"),
        (ConfigValue::float(1.0).unwrap(), "1.0"),
        (ConfigValue::float(0.000001).unwrap(), "1e-6"),
        (ConfigValue::from("a/b\u{1}\"\n"), r#""a/b\u0001\"\n""#),
        (sorted, r#"{"B":2,"_x":3,"a":5,"a1":4,"b":1}"#),
        (
            ConfigValue::String("\u{8}\t\u{c}\r\u{1b}\u{7f}\\".into()),
            "\"\\b\\t\\f\\r\\u001b\u{7f}\\\\\"",
        ),
    ];
    for (value, bytes) in cases {
        assert_eq!(value.compact(), bytes);
    }
    let floats = [
        (0.00001, "0.00001"),
        (0.85, "0.85"),
        (100.0, "100.0"),
        (1e15, "1000000000000000.0"),
        (1e16, "1e16"),
        (2.5e20, "2.5e20"),
        (1.2345678901234568e17, "1.2345678901234568e17"),
        (1.5e-7, "1.5e-7"),
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (-3.25, "-3.25"),
    ];
    for (value, bytes) in floats {
        assert_eq!(ConfigValue::float(value).unwrap().compact(), bytes, "{value}");
    }
    assert!(ConfigValue::float(f64::NAN).is_none());
}

/// C3's number reading, repeated names and the shapes a typed read takes.
#[test]
fn values_read_as_anki_reads_them() {
    let read = |text: &str| ConfigValue::parse(text.as_bytes());
    assert_eq!(read("18446744073709551615"), Some(ConfigValue::from(u64::MAX)));
    assert_eq!(read("-9223372036854775808"), Some(ConfigValue::from(i64::MIN)));
    assert_eq!(read("18446744073709551616"), ConfigValue::float(1.8446744073709552e19));
    assert_eq!(read("-9223372036854775809").unwrap().compact(), "-9.223372036854776e18");
    assert_eq!(read("-0").unwrap().compact(), "-0.0");
    assert_eq!(read("1E2"), ConfigValue::float(100.0));
    // The nearest float, not serde_json's approximation.
    assert_eq!(
        read("123456789012345678901").unwrap().compact(),
        "1.2345678901234568e20"
    );
    let repeated = read(r#"{"a": 1, "b": 2, "a": 3}"#).unwrap();
    assert_eq!(repeated.get("a").and_then(ConfigValue::as_i64), Some(3));
    assert!(repeated.has_repeated_names());
    assert_eq!(repeated.compact(), r#"{"a":3,"b":2}"#);
    assert_eq!(read("1.0").unwrap().as_i64(), None);
    assert_eq!(read("1").unwrap().as_f64(), Some(1.0));
}

/// C3 through the table: missing keys, TEXT values and invalid JSON read
/// as missing; C4: a TEXT value is never the same value, an integer is
/// never a float, a value holding repeated names is rewritten; C6 and C10.
#[test]
fn the_config_api_reads_leniently_and_writes_only_changes() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    exec(
        writer,
        "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES \
         ('text', 0, 0, '1'), ('broken', 0, 0, CAST('{\"a\":' AS BLOB)), ('one', 0, 0, CAST('1' AS BLOB)), \
         ('twice', 0, 0, CAST('{\"a\":1,\"a\":1}' AS BLOB)), \
         ('object', 0, 0, CAST('{\"z\": 2.0, \"keep\": {\"n\": 1}, \"count\": 1}' AS BLOB)), \
         ('_nt_7_lastDeck', 0, 0, CAST('1' AS BLOB)), ('_nt_7_x', 0, 0, CAST('2' AS BLOB)), ('_nt_77_x', 0, 0, CAST('3' AS BLOB))",
    );
    assert_eq!(writer.config_value("missing").unwrap(), None);
    assert_eq!(writer.config_value("text").unwrap(), None);
    assert_eq!(writer.config_value("broken").unwrap(), None);
    assert_eq!(writer.config_value("one").unwrap(), Some(ConfigValue::from(1i64)));
    let stored = writer.config_entry("text").unwrap().unwrap();
    assert!(!stored.is_blob);
    assert_eq!(stored.entry.value, b"1");

    clock.step_ms(HOUR_MS);
    let one = ConfigValue::from(1i64);
    assert_eq!(set(writer, &clock, "text", &one), Written::Updated);
    assert!(writer.config_entry("text").unwrap().unwrap().is_blob);
    assert_eq!(
        set(writer, &clock, "one", &ConfigValue::float(1.0).unwrap()),
        Written::Updated
    );
    assert_eq!(
        set(
            writer,
            &clock,
            "twice",
            &ConfigValue::Object(vec![("a".into(), one.clone())])
        ),
        Written::Updated
    );
    assert_eq!(
        common::text(&exec(
            writer,
            "SELECT CAST(val AS TEXT) FROM config WHERE KEY = 'twice'"
        )),
        r#"{"a":1}"#
    );

    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    scope
        .set_config_members("object", &[("count", ConfigValue::from(2i64))])
        .unwrap();
    assert_eq!(scope.remove_config_family("_nt_7_").unwrap(), 2);
    assert_eq!(scope.remove_config("missing").unwrap(), Written::Unchanged);
    scope.commit().unwrap();
    assert_eq!(
        common::text(&exec(
            writer,
            "SELECT CAST(val AS TEXT) FROM config WHERE KEY = 'object'"
        )),
        r#"{"count":2,"keep":{"n":1},"z":2.0}"#
    );
    assert_eq!(
        writer.config_keys().unwrap(),
        ["_nt_77_x", "broken", "object", "one", "schedVer", "text", "twice"]
    );
}
