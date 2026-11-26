//! B2/B3/B5/B6/B7 expected sets and generated-folder invariants.
use meguri_engine::{OpSource, OpSpec, UndoScope, backup_limits};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    time::EpochMillis,
};
use meguri_package::backup::{
    BackupLimits, listing,
    naming::{self, BackupKind},
    retention,
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
use meguri_testkit::normal_support::Fixture;
use proptest::prelude::*;
use std::{collections::BTreeSet, fs};

fn names(stamps: &[&str]) -> Vec<String> {
    stamps.iter().map(|s| format!("backup-{s}.colpkg")).collect()
}
fn day(stamp: &str) -> i64 {
    naming::parse_name(&format!("backup-{stamp}.colpkg"))
        .unwrap()
        .1
        .day_number()
}
fn limits(daily: u32, weekly: u32, monthly: u32) -> BackupLimits {
    BackupLimits {
        daily,
        weekly,
        monthly,
        minimum_interval_mins: 30,
    }
}
fn set(items: Vec<String>) -> BTreeSet<String> {
    items.into_iter().collect()
}

#[test]
fn period_boundaries_follow_the_last_retained_anchor() {
    let input = names(&[
        "2030-01-15-10.00.00",
        "2030-01-14-09.00.00",
        "2030-01-12-20.00.00",
        "2030-01-12-08.00.00",
        "2030-01-08-12.00.00",
        "2030-01-07-12.00.00",
        "2030-01-06-12.00.00",
        "2030-01-02-12.00.00",
        "2029-12-20-12.00.00",
        "2029-12-02-12.00.00",
        "2029-06-01-12.00.00",
    ]);
    let result = retention::plan(day("2030-01-15-00.00.00"), &input, limits(1, 1, 1), &BTreeSet::new());
    assert_eq!(
        result.keep,
        names(&[
            "2030-01-15-10.00.00",
            "2030-01-14-09.00.00",
            "2030-01-12-20.00.00",
            "2030-01-06-12.00.00",
            "2029-12-20-12.00.00"
        ])
    );
    assert_eq!(
        result.delete,
        names(&[
            "2030-01-12-08.00.00",
            "2030-01-08-12.00.00",
            "2030-01-07-12.00.00",
            "2030-01-02-12.00.00",
            "2029-12-02-12.00.00",
            "2029-06-01-12.00.00"
        ])
    );
    let protected = set(names(&["2030-01-12-08.00.00"]));
    let kept = retention::plan(day("2030-01-15-00.00.00"), &input, limits(1, 1, 1), &protected);
    assert!(kept.keep.contains(protected.first().unwrap()));
    assert!(kept.keep.contains(&"backup-2030-01-06-12.00.00.colpkg".into()));
    assert!(
        retention::plan(day("2030-01-15-00.00.00"), &kept.keep, limits(1, 1, 1), &protected)
            .delete
            .is_empty()
    );
}

#[test]
fn monday_weeks_calendar_months_zero_limits_and_sparse_folders() {
    let input = names(&[
        "2030-01-14-23.00.00",
        "2030-01-13-23.00.00",
        "2030-01-07-23.00.00",
        "2030-01-06-23.00.00",
        "2029-12-31-23.00.00",
        "2029-12-30-23.00.00",
    ]);
    assert_eq!(
        retention::plan(day("2030-01-20-00.00.00"), &input, limits(0, 2, 1), &BTreeSet::new()).keep,
        names(&["2030-01-14-23.00.00", "2030-01-13-23.00.00", "2029-12-31-23.00.00"])
    );
    let sparse = names(&["2029-10-01-00.00.00", "2029-05-01-00.00.00", "2028-12-31-23.59.59"]);
    assert_eq!(
        retention::plan(
            day("2030-01-20-00.00.00"),
            &sparse,
            BackupLimits::default(),
            &BTreeSet::new()
        )
        .keep,
        sparse
    );
    let future = names(&[
        "2031-01-01-00.00.00",
        "2030-01-20-00.00.00",
        "2030-01-19-00.00.00",
        "2030-01-18-23.59.59",
    ]);
    assert_eq!(
        retention::plan(day("2030-01-20-00.00.00"), &future, limits(0, 0, 0), &BTreeSet::new()).delete,
        names(&["2030-01-18-23.59.59"])
    );
}

#[test]
fn noncanonical_names_are_foreign() {
    for name in [
        "backup-2029-02-29-12.00.00.colpkg",
        "backup-2029-06-01-24.00.00.colpkg",
        "backup-02029-06-01-12.00.00.colpkg",
        "backup-2029-06-01T12.00.00.colpkg",
        "backup-2029-06-01.colpkg",
        "Backup-2029-06-01-12.00.00.colpkg",
        "x-backup-2029-06-01-12.00.00.colpkg",
        "backup-2029-06-01-12.00.00.colpkg.bak",
        "backup-2029-06-01-12.00.00.apkg",
        "backup-2029-6-1-12.00.00.colpkg",
        "backup-2029-06-01-12.0.0.colpkg",
        "backup-29-06-01-12.00.00.colpkg",
        "backup-+2029-06-01-12.00.00.colpkg",
        "backup--2029-06-01-12.00.00.colpkg",
        "backup-2029-06-01- 12.00.00.colpkg",
        "backup-2029-06-01-12.00.60.colpkg",
        "backup-２０２９-06-01-12.00.00.colpkg",
        "backup-2100-02-29-00.00.00.colpkg",
        "backup-2029-00-01-00.00.00.colpkg",
        "backup-2029-01-00-00.00.00.colpkg",
        "corrupt-2029-06-01-12.00.00.anki2",
        "corrupt-2029-06-01-12.00.00.anki2-wal",
    ] {
        assert!(naming::parse_name(name).is_none(), "{name}");
    }
    for stamp in ["0000-02-29-00.00.00", "2000-02-29-00.00.00", "9999-12-31-23.59.59"] {
        let name = format!("backup-{stamp}.colpkg");
        let wall = naming::parse_name(&name).unwrap().1;
        assert_eq!(naming::name_from_wall_seconds(wall.wall_seconds()).unwrap(), name);
    }
}
#[test]
fn dst_gap_name_is_a_backup() {
    assert_eq!(
        naming::parse_name("backup-2029-03-11-02.30.00.colpkg").unwrap().0,
        BackupKind::Backup
    );
}
#[test]
fn offsets_and_negative_fractional_instants_use_local_wall_dates() {
    for (offset, expected) in [(9 * 3600, "2030-01-16-05.00.00"), (-5 * 3600, "2030-01-15-15.00.00")] {
        let clock = FixedClock::new(1_894_737_600_999, UtcOffset::from_seconds(offset).unwrap());
        assert_eq!(
            naming::name_at(EpochMillis(1_894_737_600_999), &clock).unwrap(),
            format!("backup-{expected}.colpkg")
        );
        assert_eq!(
            naming::today(EpochMillis(1_894_737_600_999), &clock).unwrap(),
            day(expected)
        );
    }
    let clock = FixedClock::new(-1, UtcOffset::UTC);
    assert_eq!(
        naming::name_at(EpochMillis(-1), &clock).unwrap(),
        "backup-1969-12-31-23.59.59.colpkg"
    );
}

#[test]
fn ae69_400_days_hourly_keeps_the_explicit_66_file_set_and_recovery_copies() {
    let folder = tempfile::tempdir().unwrap();
    let start = day("2028-12-11-09.00.00") * 86_400 + 9 * 3600;
    let end = day("2030-01-15-09.00.00") * 86_400 + 9 * 3600;
    for second in (start..=end).step_by(3600) {
        fs::write(folder.path().join(naming::name_from_wall_seconds(second).unwrap()), []).unwrap();
    }
    fs::write(folder.path().join("backup-2030-01-15-10.00.00.colpkg"), b"new file").unwrap();
    let recovery = [
        "server-copy-2029-01-01-00.00.00.colpkg",
        "server-copy-2029-01-01-00.00.00.unverified.anki2",
        "corrupt-2029-01-01-00.00.00.anki2",
        "corrupt-2029-01-01-00.00.00.anki2-wal",
    ];
    for name in recovery {
        fs::write(folder.path().join(name), b"recovery bytes").unwrap();
    }
    let mut expected = BTreeSet::new();
    for hour in 0..24 {
        expected.insert(format!("backup-2030-01-14-{hour:02}.00.00.colpkg"));
    }
    for hour in 0..=10 {
        expected.insert(format!("backup-2030-01-15-{hour:02}.00.00.colpkg"));
    }
    for date in 2..=13 {
        expected.insert(format!("backup-2030-01-{date:02}-23.00.00.colpkg"));
    }
    expected.extend(names(&[
        "2029-12-30-23.00.00",
        "2029-12-23-23.00.00",
        "2029-12-16-23.00.00",
        "2029-12-09-23.00.00",
        "2029-12-02-23.00.00",
        "2029-11-25-23.00.00",
        "2029-11-18-23.00.00",
        "2029-11-11-23.00.00",
        "2029-11-04-23.00.00",
        "2029-10-28-23.00.00",
        "2029-09-30-23.00.00",
        "2029-08-31-23.00.00",
        "2029-07-31-23.00.00",
        "2029-06-30-23.00.00",
        "2029-05-31-23.00.00",
        "2029-04-30-23.00.00",
        "2029-03-31-23.00.00",
        "2029-02-28-23.00.00",
        "2029-01-31-23.00.00",
    ]));
    assert_eq!(expected.len(), 66);
    let report = retention::thin(
        folder.path(),
        day("2030-01-15-00.00.00"),
        BackupLimits::default(),
        &BTreeSet::new(),
    );
    assert!(report.failures.is_empty());
    let listed = listing::list(folder.path()).unwrap();
    assert_eq!(
        set(listed
            .iter()
            .filter(|e| e.kind == BackupKind::Backup)
            .map(|e| e.name.clone())
            .collect()),
        expected
    );
    assert!(
        listed
            .iter()
            .any(|e| e.name == recovery[0] && e.kind == BackupKind::ServerCopy)
    );
    assert!(
        listed
            .iter()
            .any(|e| e.name == recovery[1] && e.kind == BackupKind::ServerCopyUnverified)
    );
    for name in recovery {
        assert_eq!(fs::read(folder.path().join(name)).unwrap(), b"recovery bytes");
    }
    assert!(
        retention::thin(
            folder.path(),
            day("2030-01-15-00.00.00"),
            BackupLimits::default(),
            &BTreeSet::new()
        )
        .deleted_names
        .is_empty()
    );
}

#[test]
fn listing_is_passive_ordered_and_never_follows_links_or_staging() {
    let folder = tempfile::tempdir().unwrap();
    for name in [
        "server-copy-2030-01-15-10.00.00.unverified.anki2",
        "server-copy-2030-01-15-10.00.00.colpkg",
        "backup-2030-01-15-10.00.00.colpkg",
        "backup-2030-01-16-00.00.00.colpkg",
        "foreign.txt",
        ".meguri-tmp-backup-a",
    ] {
        fs::write(folder.path().join(name), b"damaged contents are not read").unwrap();
    }
    for name in [".meguri-stage-backup-a", "backup-2029-01-01-00.00.00.colpkg"] {
        fs::create_dir(folder.path().join(name)).unwrap();
    }
    std::os::unix::fs::symlink("foreign.txt", folder.path().join("backup-2029-01-02-00.00.00.colpkg")).unwrap();
    let expected: Vec<String> = [
        "backup-2030-01-16-00.00.00.colpkg",
        "backup-2030-01-15-10.00.00.colpkg",
        "server-copy-2030-01-15-10.00.00.colpkg",
        "server-copy-2030-01-15-10.00.00.unverified.anki2",
    ]
    .map(String::from)
    .into();
    assert_eq!(
        listing::list(folder.path())
            .unwrap()
            .iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>(),
        expected
    );
    let report = retention::thin(
        folder.path(),
        day("2030-01-15-00.00.00"),
        limits(0, 0, 0),
        &BTreeSet::new(),
    );
    assert!(report.deleted_names.is_empty());
    assert_eq!(fs::read_dir(folder.path()).unwrap().count(), 9);
    assert!(
        folder
            .path()
            .join("backup-2029-01-02-00.00.00.colpkg")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(listing::list(&folder.path().join("missing")).unwrap().is_empty());
    assert!(listing::list(&folder.path().join("foreign.txt")).is_err());
}

#[test]
fn limits_read_is_exact_all_or_nothing_and_unknown_members_are_allowed() {
    for text in [
        "{}",
        "[5,4,3]",
        "[5,4,3,60,1]",
        "null",
        "\"x\"",
        "bad JSON",
        "{\"daily\":5}",
        "{\"daily\":5,\"daily\":5,\"weekly\":4,\"monthly\":3,\"minimum_interval_mins\":60}",
        "{\"Daily\":5,\"weekly\":4,\"monthly\":3,\"minimum_interval_mins\":60}",
        "{\"daily\":5,\"weekly\":4,\"monthly\":3,\"minimumIntervalMins\":60}",
    ] {
        assert_eq!(
            BackupLimits::from_stored(ConfigValue::parse(text.as_bytes()).as_ref()),
            BackupLimits::default(),
            "{text}"
        );
    }
    for bad in ["5.0", "1e1", "-0", "-1", "4294967296", "\"5\"", "null", "true"] {
        let text = format!("{{\"daily\":{bad},\"weekly\":4,\"monthly\":3,\"minimum_interval_mins\":60}}");
        assert_eq!(
            BackupLimits::from_stored(ConfigValue::parse(text.as_bytes()).as_ref()),
            BackupLimits::default(),
            "{bad}"
        );
    }
    for text in [
        "[5,4,3,60]",
        "{\"daily\":5,\"weekly\":4,\"monthly\":3,\"minimum_interval_mins\":60,\"extra\":[1,{\"a\":2}]}",
    ] {
        assert_eq!(
            BackupLimits::from_stored(ConfigValue::parse(text.as_bytes()).as_ref()),
            BackupLimits {
                daily: 5,
                weekly: 4,
                monthly: 3,
                minimum_interval_mins: 60
            }
        );
    }
    assert_eq!(
        BackupLimits::from_stored(ConfigValue::parse(b"[4294967295,0,0,0]").as_ref()).daily,
        u32::MAX
    );
}

#[test]
fn limits_write_keeps_other_members_and_equal_values_keep_bytes_stamps_and_history() {
    let f = Fixture::new(0);
    let c = &f.collection;
    c.run(OpSpec::named("seed opaque config", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("foreignKey", &ConfigValue::from("untouched"))?;
        ctx.scope().set_config(
            "backups",
            &ConfigValue::parse(
                br#"{"extra":[1,1.0,{"nested":true}],"daily":8,"weekly":7,"monthly":6,"minimum_interval_mins":5}"#,
            )
            .unwrap(),
        )?;
        Ok(())
    })
    .unwrap();
    let foreign = c.reader(ReaderRole::Query).unwrap().config_entry("foreignKey").unwrap();
    c.run(
        OpSpec::named("history sentinel", OpSource::UserUndoable).undo_step("Sentinel", Default::default()),
        |ctx| {
            ctx.scope().set_config("testKey", &true.into())?;
            Ok(())
        },
    )
    .unwrap();
    let requested = BackupLimits {
        daily: 3,
        weekly: 2,
        monthly: 1,
        minimum_interval_mins: 45,
    };
    f.clock.step_ms(3_600_000);
    let result = backup_limits::save(c, requested).unwrap();
    assert_eq!(result.changes.config_keys.len(), 1);
    assert_eq!(result.changes.config_keys[0].key, "backups");
    assert!(result.value);
    assert!(result.undo.is_none());
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap().name(), None);
    let reader = c.reader(ReaderRole::Query).unwrap();
    let stored = reader.config_entry("backups").unwrap().unwrap();
    assert!(stored.is_blob);
    assert_eq!(
        stored.entry.value,
        br#"{"daily":3,"weekly":2,"monthly":1,"minimum_interval_mins":45,"extra":[1,1.0,{"nested":true}]}"#
    );
    assert_eq!(stored.entry.usn.0, -1);
    assert_eq!(
        stored.entry.mtime.0,
        (meguri_testkit::normal_support::NOW + 3_600_000) / 1000
    );
    assert_eq!(reader.config_entry("foreignKey").unwrap(), foreign);
    drop(reader);
    c.run(
        OpSpec::named("history sentinel", OpSource::UserUndoable).undo_step("Sentinel", Default::default()),
        |ctx| {
            ctx.scope().set_config("testKey", &false.into())?;
            Ok(())
        },
    )
    .unwrap();
    c.run(
        OpSpec::named("redo sentinel", OpSource::UserUndoable).undo_step("Redo Sentinel", Default::default()),
        |ctx| {
            ctx.scope().set_config("testKey2", &true.into())?;
            Ok(())
        },
    )
    .unwrap();
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(c.peek_redo(UndoScope::Global).unwrap().name(), Some("Redo Sentinel"));
    let reader = c.reader(ReaderRole::Query).unwrap();
    let before = reader.config_entry("backups").unwrap();
    let modified = reader.col().unwrap().modified;
    drop(reader);
    f.clock.step_ms(3_600_000);
    let result = backup_limits::save(c, requested).unwrap();
    assert!(!result.value);
    assert!(result.changes.is_empty());
    let reader = c.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.config_entry("backups").unwrap(), before);
    assert_eq!(reader.col().unwrap().modified, modified);
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap().name(), Some("Sentinel"));
    assert_eq!(c.peek_redo(UndoScope::Global).unwrap().name(), Some("Redo Sentinel"));
    drop(reader);
    f.host.close().unwrap();
}

#[test]
fn single_limit_patch_uses_current_writer_members_and_returns_stored_limits() {
    use backup_limits::BackupLimitPatch;
    let f = Fixture::new(0);
    let c = &f.collection;
    backup_limits::save(c, BackupLimits::default()).unwrap();
    let cached = backup_limits::read(&*c.reader(ReaderRole::Query).unwrap()).unwrap();
    c.run(
        OpSpec::named("remote backup settings", OpSource::SyncIncoming),
        |ctx| {
            ctx.scope().put_config_entry(&meguri_model::config::ConfigEntry {
                key: "backups".into(),
                usn: meguri_model::usn::Usn(7),
                mtime: meguri_model::time::EpochSecs(123),
                value: br#"{"extra":[1,1.0,{"nested":true}],"daily":4294967295,"weekly":10,"monthly":23,"minimum_interval_mins":4294967295}"#.to_vec(),
            })?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(cached.monthly, 9);
    // Occupy Query so the mutation must read through its actual writer scope.
    let reader = c.reader(ReaderRole::Query).unwrap();
    let expected = BackupLimits {
        daily: u32::MAX,
        weekly: 4,
        monthly: 23,
        minimum_interval_mins: u32::MAX,
    };
    let result = backup_limits::update(c, BackupLimitPatch::Weekly(4)).unwrap();
    assert_eq!(result.value, expected);
    assert_eq!(backup_limits::read(&*reader).unwrap(), expected);
    assert!(result.undo.is_none());
    assert_eq!(result.changes.config_keys.len(), 1);
    assert_eq!(result.changes.config_keys[0].key, "backups");
    let before = reader.config_entry("backups").unwrap();
    assert_eq!(
        before.as_ref().unwrap().entry.value,
        br#"{"daily":4294967295,"weekly":4,"monthly":23,"minimum_interval_mins":4294967295,"extra":[1,1.0,{"nested":true}]}"#
    );
    assert_eq!(before.as_ref().unwrap().entry.usn.0, -1);
    let modified = reader.col().unwrap().modified;
    f.clock.step_ms(3_600_000);
    let noop = backup_limits::update(c, BackupLimitPatch::Weekly(4)).unwrap();
    assert_eq!(noop.value, expected);
    assert!(noop.changes.is_empty());
    assert_eq!(reader.config_entry("backups").unwrap(), before);
    assert_eq!(reader.col().unwrap().modified, modified);
    drop(reader);
    f.host.close().unwrap();
}

#[test]
fn single_limit_patch_defaults_invalid_members_together_and_keeps_full_range() {
    use backup_limits::BackupLimitPatch;
    let f = Fixture::new(0);
    let c = &f.collection;
    for (stored, initial) in [
        (
            r#"{"daily":5,"weekly":-1,"monthly":3,"minimum_interval_mins":60,"extra":true}"#,
            BackupLimits::default(),
        ),
        (
            r#"{"daily":5,"daily":5,"weekly":4,"monthly":3,"minimum_interval_mins":60,"extra":true}"#,
            BackupLimits::default(),
        ),
        (
            "[4294967295,4,3,4294967295]",
            BackupLimits {
                daily: u32::MAX,
                weekly: 4,
                monthly: 3,
                minimum_interval_mins: u32::MAX,
            },
        ),
    ] {
        c.run(
            OpSpec::named("seed legacy backup limits", OpSource::Maintenance),
            move |ctx| {
                ctx.scope().set_config("backups", &ConfigValue::parse(stored.as_bytes()).unwrap())?;
                Ok(())
            },
        )
        .unwrap();
        let result = backup_limits::update(c, BackupLimitPatch::Weekly(0)).unwrap();
        assert_eq!(result.value, BackupLimits { weekly: 0, ..initial });
        for (patch, expected) in [
            (
                BackupLimitPatch::Daily(u32::MAX),
                BackupLimits { daily: u32::MAX, weekly: 0, ..initial },
            ),
            (
                BackupLimitPatch::Monthly(u32::MAX),
                BackupLimits { daily: u32::MAX, weekly: 0, monthly: u32::MAX, ..initial },
            ),
            (
                BackupLimitPatch::MinimumIntervalMins(0),
                BackupLimits { daily: u32::MAX, weekly: 0, monthly: u32::MAX, minimum_interval_mins: 0 },
            ),
        ] {
            assert_eq!(backup_limits::update(c, patch).unwrap().value, expected);
            assert_eq!(backup_limits::read(&*c.reader(ReaderRole::Query).unwrap()).unwrap(), expected);
        }
    }
    f.host.close().unwrap();
}

#[test]
fn blob_only_limits_reads_preserve_raw_bytes_and_row_stamps() {
    use meguri_storage::raw::SqlValue;
    use std::time::Duration;
    let f = Fixture::new(0);
    let valid = BackupLimits {
        daily: 5,
        weekly: 4,
        monthly: 3,
        minimum_interval_mins: 60,
    };
    for (value, expected) in [
        (SqlValue::Text("[5,4,3,60]".into()), BackupLimits::default()),
        (SqlValue::Blob(b"[5,4,3,60]".to_vec()), valid),
        (
            SqlValue::Blob(br#"{"daily":5,"weekly":4,"monthly":3,"minimum_interval_mins":60,"x":1,"x":2}"#.to_vec()),
            valid,
        ),
        (
            SqlValue::Blob(br#"{"daily":5,"daily":5,"weekly":4,"monthly":3,"minimum_interval_mins":60}"#.to_vec()),
            BackupLimits::default(),
        ),
    ] {
        f.collection
            .submit_maintenance(
                meguri_engine::executor::catalog::ANALYZE,
                Duration::from_millis(10),
                move |writer| {
                    Ok(writer.execute_raw(
                        "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES ('backups', 7, 123, ?)",
                        &[value],
                    )?)
                },
            )
            .wait()
            .unwrap();
        let reader = f.collection.reader(ReaderRole::Query).unwrap();
        let before = reader.config_entry("backups").unwrap();
        let modified = reader.col().unwrap().modified;
        assert_eq!(backup_limits::read(&*reader).unwrap(), expected);
        assert_eq!(reader.config_entry("backups").unwrap(), before);
        assert_eq!(reader.col().unwrap().modified, modified);
    }
    f.host.close().unwrap();
}

proptest! {
    #[test]
    fn generated_folders_are_idempotent_bounded_and_preserve_protected_and_recent(
        seconds in prop::collection::btree_set(0i64..(800 * 86_400), 0..600),
        daily in 0u32..20, weekly in 0u32..15, monthly in 0u32..12,
        protect in any::<bool>(),
    ) {
        let today = day("2030-01-15-00.00.00");
        let base = (today - 798) * 86_400;
        let mut input: Vec<_> = seconds.iter().map(|s| naming::name_from_wall_seconds(base + s).unwrap()).collect();
        input.push("foreign.txt".into());
        input.push("server-copy-2020-01-01-00.00.00.colpkg".into());
        let protected: BTreeSet<_> = if protect { input.first().cloned().into_iter().collect() } else { BTreeSet::new() };
        let limits = limits(daily, weekly, monthly);
        let plan = retention::plan(today, &input, limits, &protected);
        prop_assert!(retention::plan(today, &plan.keep, limits, &protected).delete.is_empty());
        for name in &plan.delete {
            prop_assert!(!protected.contains(name));
            let (kind, wall) = naming::parse_name(name).unwrap();
            prop_assert_eq!(kind, BackupKind::Backup);
            prop_assert!(wall.day_number() < today - 1);
        }
        let older = plan.keep.iter().filter(|n| !protected.contains(*n)
            && naming::parse_name(n).unwrap().1.day_number() < today - 1).count();
        prop_assert!(older <= (daily + weekly + monthly) as usize);
        prop_assert_eq!(plan.keep.len() + plan.delete.len(), seconds.len());
        for name in protected.iter().filter(|n| matches!(naming::parse_name(n), Some((BackupKind::Backup, _)))) {
            prop_assert!(plan.keep.contains(name));
        }
    }
}
