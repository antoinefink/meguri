mod common;
use meguri_storage::{OpenError, lockfile::CollectionLock};
use std::process::Command;

#[test]
fn child_lock_probe() {
    let Some(path) = std::env::var_os("MEGURI_LOCK_PROBE") else {
        return;
    };
    assert!(matches!(
        CollectionLock::acquire(std::path::Path::new(&path)),
        Err(OpenError::OpenInAnotherProgram)
    ));
}

#[test]
fn second_process_refused_and_original_owner_unaffected() {
    let dir = tempfile::tempdir().unwrap();
    let first = CollectionLock::acquire(dir.path()).unwrap();
    assert!(matches!(
        CollectionLock::acquire(dir.path()),
        Err(OpenError::OpenInAnotherProgram)
    ));
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_lock_probe"])
        .env("MEGURI_LOCK_PROBE", dir.path())
        .output()
        .unwrap();
    assert!(child.status.success(), "{}", String::from_utf8_lossy(&child.stderr));
    assert!(CollectionLock::acquire(dir.path()).is_err());
    drop(first);
    let _second = CollectionLock::acquire(dir.path()).unwrap();
}

#[test]
fn sqlite_exclusive_owner_is_refused_without_disturbing_it() {
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let db = common::plain(&path);
    let previous = db.query_row("SELECT mod FROM col", [], |r| r.get::<_, i64>(0)).unwrap();
    db.execute_batch("PRAGMA locking_mode = EXCLUSIVE; BEGIN EXCLUSIVE; UPDATE col SET mod = 45;")
        .unwrap();
    let err = common::open(&path).unwrap_err();
    assert!(err.to_string().contains("open in another program"), "{err}");
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        db.query_row("SELECT mod FROM col", [], |r| r.get::<_, i64>(0)).unwrap(),
        previous
    );
}
