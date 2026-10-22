//! Candidate-only P12: actual gate, copy preflight, integrity, preview facts,
//! and absence of managed metadata. Every SQLite handle uses a fixture copy.
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{
    OpenError, StorageError,
    package_inspection::{CandidateContext, CandidateError, CandidateFacts, inspect_candidate},
    space::{SpaceProvider, StatvfsSpace},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures/storage")
        .join(name)
}
fn candidate(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(fixture(name), &path).unwrap();
    (dir, path)
}
fn inspect(
    path: &Path,
    space: &dyn SpaceProvider,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<CandidateFacts, CandidateError> {
    inspect_candidate(
        path,
        &CandidateContext {
            clock: &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
            random: &SeededRandom::new(42),
            space,
            is_cancelled: cancel,
        },
    )
}
fn plain(path: &Path) -> meguri_storage::rusqlite::Connection {
    let conn = meguri_storage::rusqlite::Connection::open(path).unwrap();
    conn.create_collation("unicase", meguri_text::fold::compare_folded)
        .unwrap();
    conn
}
#[test]
fn schema18_preview_matches_actual_rows_and_preserves_scm_mod_without_managed_sidecar() {
    let (dir, path) = candidate("schema-18.anki2");
    let expected = {
        let conn = plain(&path);
        let counts: (i64, i64, i64) = conn
            .query_row(
                "SELECT (SELECT count(*) FROM notes),(SELECT count(*) FROM cards),(SELECT count(*) FROM revlog)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let stamps: (i64, i64) = conn
            .query_row("SELECT scm,mod FROM col", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        (counts, stamps)
    };
    fs::write(dir.path().join("meguri-sidecar.json"), b"not managed").unwrap();
    let facts = inspect(&path, &StatvfsSpace, &|| false).unwrap();
    assert_eq!(facts.ver_found, 18);
    assert_eq!(
        (facts.notes, facts.cards, facts.revlog),
        (
            u64::try_from(expected.0.0).unwrap(),
            u64::try_from(expected.0.1).unwrap(),
            u64::try_from(expected.0.2).unwrap()
        )
    );
    assert_eq!((facts.scm, facts.modified), expected.1);
    assert_eq!(facts.media_count, 0);
    assert_eq!(
        fs::read(dir.path().join("meguri-sidecar.json")).unwrap(),
        b"not managed"
    );
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        2,
        "connections and WAL siblings closed"
    );
}
#[test]
fn upgrades_all_supported_old_schemas_after_exact_copy_preflight_and_keeps_original() {
    for version in [11, 14, 15, 16, 17] {
        let name = format!("schema-{version}.anki2");
        let original = fs::read(fixture(&name)).unwrap();
        let (dir, path) = candidate(&name);
        struct Capacity {
            expected: u64,
            calls: AtomicUsize,
        }
        impl SpaceProvider for Capacity {
            fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let path = directory.join("collection.anki2");
                assert_eq!(fs::metadata(&path).unwrap().len(), self.expected);
                assert_eq!(
                    fs::read_dir(directory).unwrap().count(),
                    1,
                    "preflight precedes pre-upgrade copy"
                );
                Ok(u64::MAX)
            }
        }
        let space = Capacity {
            expected: original.len() as u64,
            calls: AtomicUsize::new(0),
        };
        let facts = inspect(&path, &space, &|| false).unwrap();
        assert_eq!(facts.ver_found, version);
        assert_eq!(space.calls.load(Ordering::SeqCst), 1);
        let conn = plain(&path);
        assert_eq!(
            conn.query_row("SELECT ver FROM col", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            18
        );
        drop(conn);
        assert!(
            dir.path()
                .join(format!("collection.anki2.pre-upgrade-v{version}"))
                .exists()
        );
        assert!(!dir.path().join("meguri-sidecar.json").exists());
        assert_eq!(fs::read(fixture(&name)).unwrap(), original);
    }
}
#[test]
fn low_space_refuses_upgrade_copy_before_writes_and_schema_refusals_precede_space() {
    struct Empty(AtomicUsize);
    impl SpaceProvider for Empty {
        fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(0)
        }
    }
    let space = Empty(AtomicUsize::new(0));
    let (dir, path) = candidate("schema-11.anki2");
    let before = fs::read(&path).unwrap();
    assert!(
        matches!(inspect(&path,&space,&||false),Err(CandidateError::InsufficientSpace { needed,available:0 }) if needed==before.len() as u64)
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(space.0.load(Ordering::SeqCst), 1);
    for (name, expected) in [
        ("ver-10-delete.anki2", OpenError::TooOld { version: 10 }),
        ("ver-12-wal.anki2", OpenError::TooNew { version: 12 }),
        ("ver-13-delete.anki2", OpenError::TooNew { version: 13 }),
        ("ver-19-wal.anki2", OpenError::TooNew { version: 19 }),
        ("v1-scheduler-18.anki2", OpenError::V1Scheduler),
        ("v1-scheduler-11.anki2", OpenError::V1Scheduler),
    ] {
        let (dir, path) = candidate(name);
        let before = fs::read(&path).unwrap();
        assert!(matches!(inspect(&path,&space,&||false),Err(CandidateError::Gate(gate)) if gate==expected));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    assert_eq!(space.0.load(Ordering::SeqCst), 1);
}
#[test]
fn expected_object_check_and_integrity_run_with_unicase() {
    let (dir, path) = candidate("schema-18.anki2");
    // A name index requiring unicase remains present in the valid candidate.
    inspect(&path, &StatvfsSpace, &|| false).unwrap();
    plain(&path).execute_batch("DROP TABLE cards").unwrap();
    assert!(
        matches!(inspect(&path,&StatvfsSpace,&||false),Err(CandidateError::Gate(OpenError::NotACollection { missing_tables })) if missing_tables.iter().any(|t|t=="cards"))
    );
    assert!(!dir.path().join("meguri-sidecar.json").exists());
}
#[test]
fn cancellation_at_each_gate_boundary_closes_connections_without_sidecar() {
    for threshold in 0..8 {
        let (dir, path) = candidate("schema-18.anki2");
        let calls = AtomicUsize::new(0);
        let cancel = || calls.fetch_add(1, Ordering::SeqCst) >= threshold;
        assert!(matches!(
            inspect(&path, &StatvfsSpace, &cancel),
            Err(CandidateError::Cancelled)
        ));
        assert!(!dir.path().join("meguri-sidecar.json").exists());
        // An immediate reopen proves no retained SQLite handle/transaction.
        inspect(&path, &StatvfsSpace, &|| false).unwrap();
    }
}

#[test]
fn candidate_probe_limit_is_inclusive_for_each_representation() {
    use meguri_storage::package_inspection::{
        probe_candidate,
        testing::{ProbeMutation, mutate_probe_candidate},
    };
    for version in [11, 14, 15, 16, 17, 18] {
        for blob in [false, true] {
            for bytes in [4 * 1024 * 1024, 4 * 1024 * 1024 + 1] {
                let (dir, path) = candidate(&format!("schema-{version}.anki2"));
                mutate_probe_candidate(&path, ProbeMutation::SchedulerValue { bytes, blob });
                let before = fs::read(&path).unwrap();
                let result = probe_candidate(&path);
                if bytes == 4 * 1024 * 1024 {
                    assert_eq!(result.unwrap(), version);
                } else {
                    assert!(
                        matches!(result, Err(CandidateError::Gate(OpenError::Corrupt { detail })) if detail == "candidate probe value byte limit")
                    );
                }
                assert_eq!(fs::read(&path).unwrap(), before);
                assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
            }
        }
    }
}

#[test]
fn full_gate_accepts_boundary_marker_and_ignores_large_unrelated_config() {
    use meguri_storage::package_inspection::testing::{ProbeMutation, mutate_probe_candidate};
    for version in [11, 18] {
        let (_dir, path) = candidate(&format!("schema-{version}.anki2"));
        mutate_probe_candidate(
            &path,
            ProbeMutation::SchedulerValue {
                bytes: 4 * 1024 * 1024,
                blob: false,
            },
        );
        assert_eq!(inspect(&path, &StatvfsSpace, &|| false).unwrap().ver_found, version);
    }
    let (_dir, path) = candidate("schema-18.anki2");
    mutate_probe_candidate(&path, ProbeMutation::UnrelatedConfigValue { bytes: 8 * 1024 * 1024 });
    assert_eq!(inspect(&path, &StatvfsSpace, &|| false).unwrap().ver_found, 18);
}

#[test]
fn large_real_candidate_is_refused_before_upgrade_preflight_or_writes() {
    struct NoPreflight;
    impl SpaceProvider for NoPreflight {
        fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
            panic!("oversized marker must fail before upgrade preflight")
        }
    }
    for version in [11, 18] {
        for blob in [false, true] {
            let (dir, path) = candidate(&format!("schema-{version}.anki2"));
            let conn = plain(&path);
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
            conn.execute(&sql, []).unwrap();
            drop(conn);
            let size = fs::metadata(&path).unwrap().len();
            assert!(size > 64 * 1024 * 1024);
            assert!(
                matches!(inspect(&path, &NoPreflight, &|| false), Err(CandidateError::Gate(OpenError::Corrupt { detail })) if detail == "candidate probe value byte limit")
            );
            assert_eq!(fs::metadata(&path).unwrap().len(), size);
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn candidate_filesystem_open_failure_keeps_io_classification() {
    use meguri_storage::package_inspection::probe_candidate;
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        probe_candidate(&dir.path().join("absent.anki2")),
        Err(CandidateError::Gate(OpenError::Io { .. }))
    ));
    assert!(matches!(
        probe_candidate(dir.path()),
        Err(CandidateError::Gate(OpenError::Io { .. }))
    ));
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}
