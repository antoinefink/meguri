mod common;
use meguri_model::{
    clock::{Clock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{
    OpenError, OpenOptions, ReadRows, StorageError,
    sidecar::{self, Sidecar},
    verified_replace::{self as replace, ReplaceError, ReplacementStage},
};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug)]
struct CountingClock(AtomicUsize);
impl Clock for CountingClock {
    fn now_ms(&self) -> i64 {
        common::NOW_MS + self.0.fetch_add(1, Ordering::SeqCst) as i64
    }
    fn utc_offset_at(&self, _: i64) -> UtcOffset {
        UtcOffset::UTC
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}

#[test]
fn package_stamps_once_then_caller_final_wal_changes_become_self_contained() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let live_bytes = fs::read(&live).unwrap();
    let stage = ReplacementStage::new(dir.path()).unwrap();
    fs::copy(&live, stage.path()).unwrap();
    let original = common::open(&stage.path()).unwrap();
    let original_col = original.writer().col().unwrap();
    let original_note = original.writer().notes_after(None, 1).unwrap().remove(0);
    original.close().unwrap();
    let clock = CountingClock(AtomicUsize::new(0));
    let random = SeededRandom::new(991);
    replace::prepare(
        &stage,
        &live,
        &OpenOptions::new(&clock, &random),
        &clock,
        true,
        |scope| {
            let col = scope.col()?;
            assert_eq!(col.modified.0, common::NOW_MS);
            assert_eq!(col.schema_modified, col.modified);
            assert_eq!(scope.now().0, common::NOW_MS);
            let mut note = scope.note(original_note.id)?.unwrap();
            note.fields = "caller transaction in WAL".into();
            scope.put(&note)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(clock.0.load(Ordering::SeqCst), 1);
    assert_eq!(fs::read(&live).unwrap(), live_bytes);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!stage.path().with_extension(format!("anki2{suffix}")).exists());
    }
    assert!(
        meguri_storage::integrity::integrity_check(&stage.path())
            .unwrap()
            .is_ok()
    );
    let opened = common::open(&stage.path()).unwrap();
    let mut expected = original_col;
    expected.modified.0 = common::NOW_MS;
    expected.schema_modified.0 = common::NOW_MS;
    assert_eq!(opened.writer().col().unwrap(), expected);
    let mut expected_note = original_note;
    expected_note.fields = "caller transaction in WAL".into();
    assert_eq!(opened.writer().note(expected_note.id).unwrap().unwrap(), expected_note);
    opened.close().unwrap();
}

#[test]
fn caller_error_rolls_back_stamps_and_changes_without_live_writes() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let stage = ReplacementStage::new(dir.path()).unwrap();
    fs::copy(&live, stage.path()).unwrap();
    let original = common::open(&stage.path()).unwrap();
    let col = original.writer().col().unwrap();
    original.close().unwrap();
    let old = fs::read(&live).unwrap();
    let error = StorageError::Io {
        detail: "caller refused".into(),
    };
    let result = replace::prepare(
        &stage,
        &live,
        &OpenOptions::new(&common::clock(), &SeededRandom::new(2)),
        &common::clock(),
        true,
        |_| Err(error.clone()),
    );
    assert_eq!(result, Err(ReplaceError::CallerChangeFailed(error)));
    assert_eq!(fs::read(&live).unwrap(), old);
    let reopened = common::open(&stage.path()).unwrap();
    assert_eq!(reopened.writer().col().unwrap(), col);
    reopened.close().unwrap();
}

#[test]
fn refused_truncated_and_old_candidate_leave_live_bytes_untouched_and_stages_owned() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let before = fs::read(&live).unwrap();
    for fixture in ["ver-10-delete.anki2", "ver-12-delete.anki2"] {
        let stage = ReplacementStage::new(dir.path()).unwrap();
        fs::copy(common::fixture(fixture), stage.path()).unwrap();
        let stage_path = stage.directory().to_owned();
        let result = replace::prepare(
            &stage,
            &live,
            &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
            &common::clock(),
            false,
            |_| panic!("refused file reached caller"),
        );
        assert!(matches!(result, Err(ReplaceError::StagingRefused(_))));
        assert_eq!(fs::read(&live).unwrap(), before);
        drop(stage);
        assert!(!stage_path.exists());
    }
    let stage = ReplacementStage::new(dir.path()).unwrap();
    fs::write(stage.path(), b"SQLite format 3\0truncated").unwrap();
    assert!(
        replace::prepare(
            &stage,
            &live,
            &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
            &common::clock(),
            false,
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(fs::read(&live).unwrap(), before);
}

#[test]
fn older_accepted_candidate_upgrades_only_in_stage() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let old = fs::read(&live).unwrap();
    let stage = ReplacementStage::new(dir.path()).unwrap();
    fs::copy(common::fixture("schema-11.anki2"), stage.path()).unwrap();
    replace::prepare(
        &stage,
        &live,
        &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
        &common::clock(),
        true,
        |_| Ok(()),
    )
    .unwrap();
    let opened = common::open(&stage.path()).unwrap();
    assert_eq!(opened.writer().col().unwrap().version, 18);
    opened.close().unwrap();
    assert_eq!(fs::read(&live).unwrap(), old);
}

#[test]
fn nonempty_live_wal_is_preserved_and_zero_wal_is_removed() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let wal = dir.path().join("collection.anki2-wal");
    fs::write(&wal, b"not empty").unwrap();
    assert_eq!(
        replace::remove_empty_live_sidecars(&live),
        Err(ReplaceError::LiveWalNotEmpty)
    );
    assert_eq!(fs::read(&wal).unwrap(), b"not empty");
    fs::write(&wal, b"").unwrap();
    replace::remove_empty_live_sidecars(&live).unwrap();
    assert!(!wal.exists());
}

#[test]
fn concurrent_f3_and_prepared_metadata_updates_share_strict_rmw() {
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("meguri-sidecar.json"));
    fs::write(path.as_ref(), br#"{"clean_shutdown":true,"unknown":{"kept":7},"deviceState":{"future":"retained"},"lastBackup":{"filename":"saved.colpkg"}}"#).unwrap();
    let stale = Sidecar::load(path.as_ref()).unwrap();
    let mut workers = Vec::new();
    for index in 0..8 {
        let path = path.clone();
        workers.push(std::thread::spawn(move || {
            for _ in 0..25 {
                if index % 2 == 0 {
                    Sidecar::update(&path, |value, _| {
                        value.last_mod += 1;
                    })
                    .unwrap();
                } else {
                    sidecar::update_document(&path, |doc| {
                        let count = doc.get("counter").and_then(|v| v.as_u64()).unwrap_or(0);
                        doc.insert("counter".into(), (count + 1).into());
                        Ok(())
                    })
                    .unwrap();
                }
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    sidecar::read_document(&path, |doc| {
        let doc = doc.unwrap();
        assert_eq!(doc["last_mod"], 100);
        assert_eq!(doc["counter"], 100);
        assert_eq!(doc["unknown"]["kept"], 7);
        assert_eq!(doc["lastBackup"]["filename"], "saved.colpkg");
        Ok(())
    })
    .unwrap();
    sidecar::update_document(&path, |doc| {
        doc.insert("deviceState".into(), serde_json::json!({"future":"new"}));
        Ok(())
    })
    .unwrap();
    stale.save(&path).unwrap();
    sidecar::read_document(&path, |doc| {
        assert_eq!(doc.unwrap()["deviceState"]["future"], "new");
        Ok(())
    })
    .unwrap();
}

#[test]
fn unreadable_sidecar_is_never_overwritten_by_f3_or_other_updates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meguri-sidecar.json");
    for bytes in [b"{\"deviceState\":{\"intent\":".as_slice(), b"[]".as_slice()] {
        fs::write(&path, bytes).unwrap();
        assert!(Sidecar::default().save(&path).is_err());
        assert!(sidecar::update_document(&path, |_| Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn candidate_with_another_gate_connection_is_refused_before_mutation() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let stage = ReplacementStage::new(dir.path()).unwrap();
    fs::copy(&live, stage.path()).unwrap();
    let connection = common::open(&stage.path()).unwrap();
    assert_eq!(
        replace::prepare(
            &stage,
            &live,
            &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
            &common::clock(),
            true,
            |_| panic!("shared candidate reached transaction")
        ),
        Err(ReplaceError::StagingNotFinalised)
    );
    connection.close().unwrap();
}

#[test]
fn source_hardlink_cannot_turn_staging_validation_into_a_live_write() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let before = fs::read(&live).unwrap();
    let stage = ReplacementStage::new(dir.path()).unwrap();
    fs::hard_link(&live, stage.path()).unwrap();
    assert!(
        replace::prepare(
            &stage,
            &live,
            &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
            &common::clock(),
            true,
            |_| panic!("aliased candidate reached caller")
        )
        .is_err()
    );
    assert_eq!(fs::read(&live).unwrap(), before);
}

#[test]
fn replacement_refuses_oversized_scheduler_before_caller_or_live_changes() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let before = fs::read(&live).unwrap();
    for version in [11, 18] {
        for blob in [false, true] {
            let stage = ReplacementStage::new(dir.path()).unwrap();
            fs::copy(common::fixture(&format!("schema-{version}.anki2")), stage.path()).unwrap();
            let conn = common::plain(&stage.path());
            conn.create_collation("unicase", meguri_text::fold::compare_folded)
                .unwrap();
            let value = if blob {
                "zeroblob(67108864)"
            } else {
                "CAST(zeroblob(67108864) AS TEXT)"
            };
            let sql = if version == 11 {
                format!("UPDATE col SET conf = {value}")
            } else {
                format!("UPDATE config SET val = {value} WHERE key = 'schedVer'")
            };
            assert_eq!(conn.execute(&sql, []).unwrap(), 1);
            conn.close().unwrap();
            assert!(fs::metadata(stage.path()).unwrap().len() > 64 * 1024 * 1024);
            let candidate_before = common::sha256(&stage.path());
            let mut called = false;
            let result = replace::prepare(
                &stage,
                &live,
                &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
                &common::clock(),
                true,
                |_| {
                    called = true;
                    Ok(())
                },
            );
            assert!(
                matches!(result, Err(ReplaceError::StagingRefused(OpenError::Corrupt { detail })) if detail == "candidate probe value byte limit")
            );
            assert!(!called);
            assert_eq!(fs::read(&live).unwrap(), before);
            assert_eq!(common::sha256(&stage.path()), candidate_before);
            assert_eq!(fs::read_dir(stage.directory()).unwrap().count(), 1);
        }
    }
}

#[test]
fn replacement_refuses_huge_noninteger_version_before_caller_or_live_changes() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let before = fs::read(&live).unwrap();
    for blob in [false, true] {
        let stage = ReplacementStage::new(dir.path()).unwrap();
        fs::copy(&live, stage.path()).unwrap();
        let conn = common::plain(&stage.path());
        let value = if blob {
            "zeroblob(67108864)"
        } else {
            "CAST(zeroblob(67108864) AS TEXT)"
        };
        assert_eq!(conn.execute(&format!("UPDATE col SET ver = {value}"), []).unwrap(), 1);
        conn.close().unwrap();
        assert!(fs::metadata(stage.path()).unwrap().len() > 64 * 1024 * 1024);
        let candidate_before = common::sha256(&stage.path());
        let mut called = false;
        let result = replace::prepare(
            &stage,
            &live,
            &OpenOptions::new(&common::clock(), &SeededRandom::new(1)),
            &common::clock(),
            true,
            |_| {
                called = true;
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(ReplaceError::StagingRefused(OpenError::NotACollection { missing_tables })) if missing_tables.is_empty())
        );
        assert!(!called);
        assert_eq!(fs::read(&live).unwrap(), before);
        assert_eq!(common::sha256(&stage.path()), candidate_before);
        assert_eq!(fs::read_dir(stage.directory()).unwrap().count(), 1);
    }
}
