//! Manual layout/error simulations, not F3 power-loss certification.
use super::*;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use std::fs;

const N: &str = "meguri-cloud.db.damaged-2026-10-01-00.00.00";

fn install(dir: &Path, wal: bool) -> MoveJournal {
    fs::write(dir.join(TRACKING_FILE), b"main evidence").unwrap();
    if wal {
        fs::write(dir.join("meguri-cloud.db-wal"), b"WAL evidence").unwrap();
    }
    let journal = MoveJournal {
        version: 1,
        destination: N.into(),
        main: identity(&dir.join(TRACKING_FILE)).unwrap().unwrap(),
        wal: identity(&dir.join("meguri-cloud.db-wal")).unwrap(),
    };
    fs::write(dir.join(format!("{N}.moving")), serde_json::to_vec(&journal).unwrap()).unwrap();
    journal
}

fn image(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut result: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (e.file_name().into_string().unwrap(), fs::read(e.path()).unwrap())
        })
        .collect();
    result.sort();
    result
}

#[test]
fn every_valid_initial_move_layout_including_no_wal_replays_exact_identities() {
    for wal in [false, true] {
        for main_moved in [false, true] {
            for wal_moved in [false, true] {
                if (wal_moved && !wal) || (wal && main_moved && !wal_moved) {
                    continue;
                }
                let dir = tempfile::tempdir().unwrap();
                let journal = install(dir.path(), wal);
                if main_moved {
                    fs::rename(
                        dir.path().join(TRACKING_FILE),
                        dir.path().join(format!("{N}.salvaging")),
                    )
                    .unwrap();
                }
                if wal_moved {
                    fs::rename(
                        dir.path().join("meguri-cloud.db-wal"),
                        dir.path().join(format!("{N}.salvaging-wal")),
                    )
                    .unwrap();
                }
                let directory = RecoveryDirectory::open(dir.path()).unwrap();
                replay_moves(&directory).unwrap();
                assert_eq!(
                    identity(&dir.path().join(format!("{N}.salvaging"))).unwrap(),
                    Some(journal.main)
                );
                assert_eq!(
                    identity(&dir.path().join(format!("{N}.salvaging-wal"))).unwrap(),
                    journal.wal
                );
                assert!(!dir.path().join(format!("{N}.moving")).exists());
                assert!(!dir.path().join(TRACKING_FILE).exists());
            }
        }
    }
}

#[test]
fn invalid_journal_or_any_main_wal_identity_collision_refuses_without_mutation() {
    for case in [
        "bad-json",
        "new-version",
        "traversal",
        "wrong-main",
        "wrong-wal",
        "missing-main",
        "missing-wal",
        "duplicate-main",
        "duplicate-wal",
        "unexpected-wal",
        "final-wal",
        "final-main",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = install(dir.path(), case != "unexpected-wal");
        match case {
            "bad-json" => {
                fs::write(dir.path().join(format!("{N}.moving")), b"broken").unwrap();
            }
            "new-version" => journal.version = 2,
            "traversal" => journal.destination = "../outside".into(),
            "wrong-main" => journal.main.inode += 1,
            "wrong-wal" => journal.wal.as_mut().unwrap().inode += 1,
            "missing-main" => fs::remove_file(dir.path().join(TRACKING_FILE)).unwrap(),
            "missing-wal" => fs::remove_file(dir.path().join("meguri-cloud.db-wal")).unwrap(),
            "duplicate-main" => {
                fs::hard_link(
                    dir.path().join(TRACKING_FILE),
                    dir.path().join(format!("{N}.salvaging")),
                )
                .unwrap();
            }
            "duplicate-wal" => {
                fs::hard_link(
                    dir.path().join("meguri-cloud.db-wal"),
                    dir.path().join(format!("{N}.salvaging-wal")),
                )
                .unwrap();
            }
            "unexpected-wal" => fs::write(dir.path().join("meguri-cloud.db-wal"), b"unexpected").unwrap(),
            "final-wal" => fs::write(dir.path().join(format!("{N}-wal")), b"collision").unwrap(),
            "final-main" => fs::write(dir.path().join(N), b"collision").unwrap(),
            _ => unreachable!(),
        }
        if case != "bad-json" {
            fs::write(
                dir.path().join(format!("{N}.moving")),
                serde_json::to_vec(&journal).unwrap(),
            )
            .unwrap();
        }
        let before = image(dir.path());
        assert!(
            replay_moves(&RecoveryDirectory::open(dir.path()).unwrap()).is_err(),
            "{case}"
        );
        assert_eq!(image(dir.path()), before, "{case}");
    }
}

#[test]
fn malformed_owned_names_symlinks_and_destination_collisions_never_move_source() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(TRACKING_FILE), b"evidence").unwrap();
    fs::write(dir.path().join(N), b"kept forever").unwrap();
    let before = image(dir.path());
    let clock = FixedClock::new(1_790_812_800_000, UtcOffset::UTC);
    // Explicit collision with the actual clock-derived name.
    let chosen = damaged_name(&clock).unwrap();
    fs::write(dir.path().join(format!("{chosen}.salvaging")), b"collision").unwrap();
    let before_collision = image(dir.path());
    assert!(move_damaged(&RecoveryDirectory::open(dir.path()).unwrap(), &clock).is_err());
    assert_eq!(image(dir.path()), before_collision);
    assert!(before.iter().any(|(name, _)| name == TRACKING_FILE));
    assert!(!is_source_name("meguri-cloud.db.damaged-2026-99-01-00.00.00.salvaging"));
    assert!(!is_source_name(
        "../meguri-cloud.db.damaged-2026-10-01-00.00.00.salvaging"
    ));
}

#[test]
fn replay_sync_failures_never_advance_and_repeat_observed_barriers_after_relaunch() {
    // Each validated main/WAL/journal full sync and the directory barrier.
    // Rebuild layouts for every injection; visible locations are retained as
    // evidence, then replayed twice to model another restart, not real power.
    for wal in [false, true] {
        for moved in [false, true] {
            for fail_at in 1..=if wal { 4 } else { 3 } {
                let dir = tempfile::tempdir().unwrap();
                install(dir.path(), wal);
                if moved {
                    if wal {
                        fs::rename(
                            dir.path().join("meguri-cloud.db-wal"),
                            dir.path().join(format!("{N}.salvaging-wal")),
                        )
                        .unwrap();
                    }
                    fs::rename(
                        dir.path().join(TRACKING_FILE),
                        dir.path().join(format!("{N}.salvaging")),
                    )
                    .unwrap();
                }
                let before = image(dir.path());
                let directory = RecoveryDirectory::open(dir.path()).unwrap();
                directory.fail_sync_at.set(Some(fail_at));
                assert!(replay_moves(&directory).is_err());
                assert_eq!(image(dir.path()), before);
                let directory = RecoveryDirectory::open(dir.path()).unwrap();
                replay_moves(&directory).unwrap();
                assert!(directory.sync_count.get() >= if wal { 4 } else { 3 });
                replay_moves(&RecoveryDirectory::open(dir.path()).unwrap()).unwrap();
                assert!(!dir.path().join(format!("{N}.moving")).exists());
            }
        }
    }
}

#[test]
fn absent_journal_directory_sync_failure_prevents_orphan_cleanup_and_new_current() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(format!("{N}.salvaging-wal")), b"orphan").unwrap();
    let directory = RecoveryDirectory::open(dir.path()).unwrap();
    directory.fail_sync_at.set(Some(1));
    let before = image(dir.path());
    assert!(replay_moves(&directory).is_err());
    assert_eq!(image(dir.path()), before);
    assert!(!dir.path().join(TRACKING_FILE).exists());
}

#[test]
fn completed_delete_and_rename_halfpairs_replay_without_salvaging_again() {
    for rename in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        if rename {
            fs::write(
                dir.path().join(format!("{N}.salvaging")),
                b"unreadable main already copied",
            )
            .unwrap();
            fs::write(dir.path().join(format!("{N}-wal")), b"kept WAL").unwrap();
        } else {
            fs::write(dir.path().join(format!("{N}.salvaging-wal")), b"completed-delete WAL").unwrap();
        }
        let random = SeededRandom::new(55);
        let outcome = recover(
            dir.path(),
            super::super::BuildScope {
                container: super::super::ContainerId::new("test").unwrap(),
                environment: super::super::Environment::Development,
            },
            &FixedClock::new(0, UtcOffset::UTC),
            &random,
        )
        .unwrap();
        let store = match outcome {
            OpenOutcome::Missing(s) | OpenOutcome::Opened(s) => s,
            _ => panic!(),
        };
        assert!(store.conflict_copies().unwrap().is_empty());
        assert!(store.salvage_bindings().unwrap().is_empty());
        assert_eq!(
            store.retained_damaged_sources().unwrap(),
            if rename { vec![N.to_owned()] } else { vec![] }
        );
        assert!(!dir.path().join(format!("{N}.salvaging-wal")).exists());
        assert!(!dir.path().join(format!("{N}.salvaging")).exists());
    }
}

#[test]
fn failed_visible_rename_replay_barrier_runs_again_before_journal_removal() {
    use meguri_model::faultpoint::{self, Fault};
    for wal in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), wal);
        let directory = RecoveryDirectory::open(dir.path()).unwrap();
        faultpoint::arm(
            "storage.recovery_move.before_directory_sync",
            Fault::IoError(std::io::ErrorKind::Other),
        );
        let result = replay_moves(&directory);
        faultpoint::disarm("storage.recovery_move.before_directory_sync");
        assert!(result.is_err());
        assert!(dir.path().join(format!("{N}.moving")).exists());
        let visible = image(dir.path());
        // A second process fails its replay barrier at the directory sync;
        // even the already visible rename must not remove its journal.
        let relaunched = RecoveryDirectory::open(dir.path()).unwrap();
        relaunched.fail_sync_at.set(Some(if wal { 4 } else { 3 }));
        assert!(replay_moves(&relaunched).is_err());
        assert_eq!(image(dir.path()), visible);
        replay_moves(&RecoveryDirectory::open(dir.path()).unwrap()).unwrap();
        assert!(dir.path().join(format!("{N}.salvaging")).exists());
    }
}

#[test]
fn failed_journal_unlink_sync_is_reestablished_before_any_later_work() {
    let dir = tempfile::tempdir().unwrap();
    install(dir.path(), false);
    fs::rename(
        dir.path().join(TRACKING_FILE),
        dir.path().join(format!("{N}.salvaging")),
    )
    .unwrap();
    let directory = RecoveryDirectory::open(dir.path()).unwrap();
    // Three replay barrier syncs, then the journal-unlink directory sync.
    directory.fail_sync_at.set(Some(4));
    assert!(replay_moves(&directory).is_err());
    assert!(!dir.path().join(format!("{N}.moving")).exists());
    let visible = image(dir.path());
    let relaunched = RecoveryDirectory::open(dir.path()).unwrap();
    relaunched.fail_sync_at.set(Some(1));
    assert!(replay_moves(&relaunched).is_err());
    assert_eq!(image(dir.path()), visible);
    replay_moves(&RecoveryDirectory::open(dir.path()).unwrap()).unwrap();
}

#[test]
fn interrupted_journal_install_never_authorizes_a_source_move() {
    use meguri_model::faultpoint::{self, Fault};
    for point in [
        "storage.atomic.after_write",
        "storage.atomic.after_sync",
        "storage.atomic.after_rename",
    ] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(TRACKING_FILE), b"damaged evidence").unwrap();
        let clock = FixedClock::new(0, UtcOffset::UTC);
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        let result = move_damaged(&RecoveryDirectory::open(dir.path()).unwrap(), &clock);
        faultpoint::disarm(point);
        assert!(result.is_err());
        assert_eq!(fs::read(dir.path().join(TRACKING_FILE)).unwrap(), b"damaged evidence");
        let destination = damaged_name(&clock).unwrap();
        assert!(!dir.path().join(format!("{destination}.salvaging")).exists());
        if point == "storage.atomic.after_rename" {
            // Visible installation is not permission to skip journal/file
            // sync on replay. The failed barrier leaves the source intact.
            let relaunched = RecoveryDirectory::open(dir.path()).unwrap();
            relaunched.fail_sync_at.set(Some(1));
            let visible = image(dir.path());
            assert!(replay_moves(&relaunched).is_err());
            assert_eq!(image(dir.path()), visible);
            replay_moves(&RecoveryDirectory::open(dir.path()).unwrap()).unwrap();
        } else {
            assert!(!dir.path().join(format!("{destination}.moving")).exists());
        }
    }
}

#[test]
fn multiple_completed_journals_replay_but_shared_sources_refuse_as_a_set() {
    for shared in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let first = install(dir.path(), false);
        if !shared {
            fs::rename(
                dir.path().join(TRACKING_FILE),
                dir.path().join(format!("{N}.salvaging")),
            )
            .unwrap();
        }
        let second_name = "meguri-cloud.db.damaged-2026-10-02-00.00.00";
        let mut second = first.clone();
        second.destination = second_name.into();
        if !shared {
            fs::write(dir.path().join(format!("{second_name}.salvaging")), b"second main").unwrap();
            second.main = identity(&dir.path().join(format!("{second_name}.salvaging")))
                .unwrap()
                .unwrap();
        }
        fs::write(
            dir.path().join(format!("{second_name}.moving")),
            serde_json::to_vec(&second).unwrap(),
        )
        .unwrap();
        let before = image(dir.path());
        let result = replay_moves(&RecoveryDirectory::open(dir.path()).unwrap());
        if shared {
            assert!(result.is_err());
            assert_eq!(image(dir.path()), before);
        } else {
            result.unwrap();
            assert!(!dir.path().join(format!("{N}.moving")).exists());
            assert!(!dir.path().join(format!("{second_name}.moving")).exists());
        }
    }
}

#[test]
fn symlinked_main_wal_and_journals_are_refused_without_touching_targets() {
    use std::os::unix::fs::symlink;
    for suffix in ["main", "wal", "journal"] {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), true);
        let name = match suffix {
            "main" => TRACKING_FILE.to_owned(),
            "wal" => "meguri-cloud.db-wal".into(),
            _ => format!("{N}.moving"),
        };
        let target = dir.path().join("untouched-target");
        fs::rename(dir.path().join(&name), &target).unwrap();
        symlink(&target, dir.path().join(&name)).unwrap();
        let before = image(dir.path());
        assert!(replay_moves(&RecoveryDirectory::open(dir.path()).unwrap()).is_err());
        assert_eq!(image(dir.path()), before);
        assert!(
            fs::symlink_metadata(dir.path().join(name))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}

#[test]
fn owned_conflict_unlink_reuses_directory_sync_failure_injection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("asset-synthetic");
    fs::write(&path, b"synthetic staged bytes").unwrap();
    let directory = RecoveryDirectory::open(dir.path()).unwrap();
    let expected = directory.entry_identity("asset-synthetic").unwrap().unwrap();
    directory.fail_sync_at.set(Some(1));
    assert!(directory.remove_owned("asset-synthetic", expected).is_err());
    assert!(!path.exists(), "existing hook fails the directory barrier after unlink");
    let recovered = RecoveryDirectory::open(dir.path()).unwrap();
    recovered.remove_owned("asset-synthetic", expected).unwrap();
}
