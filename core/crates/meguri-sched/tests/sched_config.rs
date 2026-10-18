use meguri_model::{
    clock::{FixedClock, UtcOffset},
    col::Col,
    config::ConfigEntry,
    rng::SeededRandom,
};
use meguri_sched::model::sched_config::{self, SchedConfig};
use meguri_storage::{CollectionBuilder, ConfigValue, OpenOptions, ReadRows, StampMode, Writer, open_collection};

fn clock() -> FixedClock {
    FixedClock::new(1_894_701_600_000, UtcOffset::UTC)
}
fn create(path: &std::path::Path) {
    let col = Col {
        version: 18,
        ..Col::default()
    };
    let config = ConfigEntry {
        key: "schedVer".into(),
        value: b"2".to_vec(),
        ..ConfigEntry::default()
    };
    CollectionBuilder::create(
        path,
        &col,
        &[config],
        &OpenOptions::new(&clock(), &SeededRandom::new(7)),
    )
    .unwrap()
    .finish()
    .unwrap();
}
fn store(writer: &Writer, key: &str, bytes: &str) {
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Raw, &clock, None).unwrap();
    scope
        .put(&ConfigEntry {
            key: key.into(),
            value: bytes.as_bytes().into(),
            ..ConfigEntry::default()
        })
        .unwrap();
    scope.commit().unwrap();
}

#[test]
fn every_key_accepts_only_its_shape_and_defaults_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    create(&path);
    let opened = open_collection(&path, &OpenOptions::new(&clock(), &SeededRandom::new(7))).unwrap();
    let writer = opened.writer();
    type Read = fn(&Writer) -> ConfigValue;
    macro_rules! boolean {
        ($key:literal, $get:ident, $default:expr) => {
            (
                $key,
                (|w| ConfigValue::Bool(SchedConfig::new(w).$get().unwrap())) as Read,
                ConfigValue::Bool($default),
                "true",
                ConfigValue::Bool(true),
                vec!["1", "0", "\"true\"", "null"],
            )
        };
    }
    macro_rules! unsigned {
        ($key:literal, $get:ident, $default:expr) => {
            (
                $key,
                (|w| ConfigValue::Integer(SchedConfig::new(w).$get().unwrap().into())) as Read,
                ConfigValue::Integer($default),
                "4294967295",
                ConfigValue::Integer(4294967295),
                vec!["-1", "4294967296", "5.0", "5e0", "\"5\""],
            )
        };
    }
    macro_rules! string {
        ($key:literal, $get:ident, $default:literal) => {
            (
                $key,
                (|w| ConfigValue::String(SchedConfig::new(w).$get().unwrap())) as Read,
                ConfigValue::from($default),
                "\"quoted \\\"value\\\"\"",
                ConfigValue::from("quoted \"value\""),
                vec!["1", "null", "[\"a\"]"],
            )
        };
    }
    let cases = vec![
        unsigned!("collapseTime", collapse_time, 1200),
        unsigned!("lastUnburied", last_unburied, 0),
        unsigned!("nextPos", next_position, 0),
        unsigned!("timeLim", time_limit, 0),
        boolean!("newCardsIgnoreReviewLimit", new_cards_ignore_review_limit, false),
        boolean!("applyAllParentLimits", apply_all_parent_limits, false),
        boolean!("loadBalancerEnabled", load_balancer_enabled, true),
        boolean!("fsrs", fsrs, false),
        boolean!("fsrsShortTermWithStepsEnabled", fsrs_short_term_with_steps, false),
        boolean!("fsrsHealthCheck", fsrs_health_check, true),
        boolean!("fsrsLegacyEvaluate", fsrs_legacy_evaluate, false),
        boolean!("restorePositionReviewer", restore_position_reviewer, true),
        boolean!("restorePositionBrowser", restore_position_browser, true),
        boolean!("resetCountsReviewer", reset_counts_reviewer, false),
        boolean!("resetCountsBrowser", reset_counts_browser, false),
        boolean!("randomOrderReposition", random_order_reposition, false),
        boolean!("shiftPositionOfExistingCards", shift_position_of_existing_cards, false),
        boolean!("sched2021", sched2021, false),
        string!("setDueReviewer", set_due_reviewer, "1"),
        string!("setDueBrowser", set_due_browser, "0"),
        string!("cardStateCustomizer", card_state_customizer, ""),
        (
            "lastFsrsOptimize",
            (|w| ConfigValue::Integer(SchedConfig::new(w).last_fsrs_optimize().unwrap().into())) as Read,
            ConfigValue::Integer(0),
            "-2147483648",
            ConfigValue::Integer(-2147483648),
            vec!["2147483648", "-2147483649", "1.0", "\"1\""],
        ),
    ];
    for (key, read, default, valid, expected, invalid) in cases {
        assert_eq!(read(writer), default, "missing {key}");
        store(writer, key, valid);
        assert_eq!(read(writer), expected, "valid {key}");
        for value in invalid.into_iter().chain(["not JSON", "[]", "{}"]) {
            store(writer, key, value);
            let before = writer.config_entry(key).unwrap();
            assert_eq!(read(writer), default, "{key}: {value}");
            assert_eq!(writer.config_entry(key).unwrap(), before, "read wrote {key}");
        }
    }
    for (key, values) in [
        (
            "rollover",
            vec![("23", 23), ("255", 23), ("256", 4), ("-1", 4), ("4.0", 4)],
        ),
        ("schedVer", vec![("2", 2), ("2.0", 1), ("3", 1), ("\"2\"", 1)]),
    ] {
        for (raw, expected) in values {
            store(writer, key, raw);
            let c = SchedConfig::new(writer);
            assert_eq!(
                if key == "rollover" {
                    c.rollover_hour().unwrap()
                } else {
                    c.scheduler_version().unwrap()
                },
                expected
            );
        }
    }
    for raw in ["-1440", "1440", "1.0", "null"] {
        store(writer, "creationOffset", raw);
        store(writer, "localOffset", raw);
        let c = SchedConfig::new(writer);
        assert_eq!(c.creation_offset().unwrap(), None);
        assert_eq!(c.local_offset().unwrap(), 0);
    }
    for n in [-1439, 0, 1439] {
        store(writer, "creationOffset", &n.to_string());
        store(writer, "localOffset", &n.to_string());
        let c = SchedConfig::new(writer);
        assert_eq!(c.creation_offset().unwrap(), Some(n));
        assert_eq!(c.local_offset().unwrap(), n);
    }
}

#[test]
fn typed_writes_keep_equal_bytes_stamps_and_unowned_keys() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    create(&path);
    let opened = open_collection(&path, &OpenOptions::new(&clock(), &SeededRandom::new(7))).unwrap();
    let writer = opened.writer();
    store(writer, "collapseTime", " 1200 ");
    store(writer, "fsrsHealthCheck", "  false ");
    store(writer, "addon", "{\"x\": [1, 2]}");
    let before = writer.config_entry("collapseTime").unwrap();
    let readonly = writer.config_entry("fsrsHealthCheck").unwrap();
    let addon = writer.config_entry("addon").unwrap();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    assert!(!sched_config::set_collapse_time(&mut scope, 1200).unwrap().changed());
    sched_config::set_due_browser(&mut scope, "3\"d").unwrap();
    sched_config::set_last_fsrs_optimize(&mut scope, -23).unwrap();
    scope.commit().unwrap();
    assert_eq!(writer.config_entry("collapseTime").unwrap(), before);
    assert_eq!(writer.config_entry("fsrsHealthCheck").unwrap(), readonly);
    assert_eq!(writer.config_entry("addon").unwrap(), addon);
    assert_eq!(
        writer.config_entry("setDueBrowser").unwrap().unwrap().entry.value,
        br#""3\"d""#
    );
    assert_eq!(SchedConfig::new(writer).last_fsrs_optimize().unwrap(), -23);
}

#[test]
fn every_owned_setter_uses_compact_json_and_suppresses_equal_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    create(&path);
    let opened = open_collection(&path, &OpenOptions::new(&clock(), &SeededRandom::new(7))).unwrap();
    let writer = opened.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    macro_rules! check {
        ($set:ident, $value:expr, $key:literal, $bytes:literal) => {
            sched_config::$set(&mut scope, $value).unwrap();
            let stored = scope.config_entry($key).unwrap().unwrap();
            assert_eq!(stored.entry.value, $bytes.as_bytes());
            assert_eq!(stored.entry.usn.0, -1);
            assert!(!sched_config::$set(&mut scope, $value).unwrap().changed());
            assert_eq!(scope.config_entry($key).unwrap().unwrap(), stored);
        };
    }
    check!(set_collapse_time, 300, "collapseTime", "300");
    check!(set_rollover_hour, 22, "rollover", "22");
    check!(set_time_limit, 600, "timeLim", "600");
    check!(
        set_new_cards_ignore_review_limit,
        true,
        "newCardsIgnoreReviewLimit",
        "true"
    );
    check!(set_apply_all_parent_limits, true, "applyAllParentLimits", "true");
    check!(set_load_balancer_enabled, false, "loadBalancerEnabled", "false");
    check!(set_fsrs, true, "fsrs", "true");
    check!(set_last_fsrs_optimize, 23, "lastFsrsOptimize", "23");
    check!(set_due_reviewer, "2", "setDueReviewer", "\"2\"");
    check!(set_due_browser, "3", "setDueBrowser", "\"3\"");
    check!(set_restore_position_reviewer, false, "restorePositionReviewer", "false");
    check!(set_restore_position_browser, false, "restorePositionBrowser", "false");
    check!(set_reset_counts_reviewer, true, "resetCountsReviewer", "true");
    check!(set_reset_counts_browser, true, "resetCountsBrowser", "true");
    check!(set_sched2021, true, "sched2021", "true");
    scope.commit().unwrap();
}
