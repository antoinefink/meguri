//! Strict sidecar RMW through aliases, with independent F3/R6 members retained.
use meguri_storage::{
    StorageError,
    sidecar::{self, Sidecar},
};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::symlink};

#[test]
fn concurrent_alias_rmw_preserves_f3_r6_and_unknown_members() {
    let root = tempfile::tempdir_in(".").unwrap();
    let cwd = std::env::current_dir().unwrap();
    let directory = root.path().strip_prefix(&cwd).unwrap_or(root.path()).join("owned");
    fs::create_dir(&directory).unwrap();
    let absolute = cwd.join(&directory);
    let alias = root.path().join("alias");
    symlink(&absolute, &alias).unwrap();
    let path = directory.join("meguri-sidecar.json");
    let paths = [
        path.clone(),
        absolute.join("meguri-sidecar.json"),
        alias.join("meguri-sidecar.json"),
        directory.join(".").join("meguri-sidecar.json"),
    ];
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "clean_shutdown": false, "last_mod": 0, "last_scm": 7,
            "upgrade_in_progress": false, "futureF3": {"keep": [1, 2]},
            "deviceState": {"count": 0, "futureR6": {"keep": true}}
        }))
        .unwrap(),
    )
    .unwrap();
    std::thread::scope(|scope| {
        for path in &paths {
            scope.spawn(move || {
                for _ in 0..8 {
                    Sidecar::update(path, |state, known| {
                        assert!(known);
                        state.last_mod += 1;
                    })
                    .unwrap();
                    sidecar::update_document(path, |doc| {
                        let state = doc.get_mut("deviceState").unwrap().as_object_mut().unwrap();
                        let count = state["count"].as_u64().unwrap();
                        state.insert("count".into(), (count + 1).into());
                        Ok(())
                    })
                    .unwrap();
                }
            });
        }
    });
    let state = Sidecar::load(&path).unwrap();
    assert_eq!(state.last_mod, 32);
    assert_eq!(state.last_scm, 7);
    assert!(!state.clean_shutdown);
    assert!(!state.upgrade_in_progress);
    assert_eq!(state.extra["futureF3"], json!({"keep": [1, 2]}));
    assert_eq!(
        state.extra["deviceState"],
        json!({"count": 32, "futureR6": {"keep": true}})
    );
}

#[test]
fn missing_parent_defaults_only_for_reads_and_is_never_created() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("missing");
    let path = directory.join("meguri-sidecar.json");
    assert!(!Sidecar::load(&path).unwrap().clean_shutdown);
    let mut called = false;
    assert!(
        sidecar::update_document(&path, |_| {
            called = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!called);
    assert!(!directory.exists());
}

#[test]
fn unreadable_documents_and_callback_errors_leave_bytes_untouched() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("meguri-sidecar.json");
    fs::write(&path, b"unreadable JSON").unwrap();
    assert!(Sidecar::load(&path).is_err());
    let mut called = false;
    assert!(
        sidecar::update_document(&path, |_| {
            called = true;
            Ok(())
        })
        .is_err()
    );
    assert!(!called);
    assert_eq!(fs::read(&path).unwrap(), b"unreadable JSON");

    let bytes = br#"{"deviceState":{"futureR6":true},"futureF3":5}"#;
    fs::write(&path, bytes).unwrap();
    let expected = StorageError::Io {
        detail: "callback refused".into(),
    };
    let error = sidecar::update_document(&path, |doc| {
        doc.insert("futureF3".into(), Value::Null);
        Err::<(), _>(expected.clone())
    })
    .unwrap_err();
    assert_eq!(error, expected);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
