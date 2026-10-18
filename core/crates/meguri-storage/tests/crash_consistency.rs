mod common;
use meguri_storage::{ReaderRole, integrity::integrity_check};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn crash_copy_worker() {
    let Some(path) = std::env::var_os("MEGURI_CRASH_COPY") else {
        return;
    };
    let path = std::path::Path::new(&path);
    let opened = common::open(path).unwrap();
    let count: u32 = std::env::var("MEGURI_CRASH_COUNT").unwrap().parse().unwrap();
    use meguri_model::faultpoint::{self, Fault};
    use meguri_storage::ReadRows;
    let clock = common::clock();
    let note = opened.writer().notes_after(None, 1).unwrap().remove(0);
    let mut seed = 0x7135_u64;
    for _ in 0..count {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let mut scope = opened
            .writer()
            .begin_write(meguri_storage::StampMode::Raw, &clock, None)
            .unwrap();
        let mut changed = note.clone();
        changed.fields = format!("copy-{seed}");
        changed.sort_field = changed.fields.clone();
        scope.put(&changed).unwrap();
        let point = if seed & 1 == 0 {
            "storage.commit.before"
        } else {
            "storage.commit.after"
        };
        faultpoint::arm(point, Fault::Callback(capture_boundary));
        scope.commit().unwrap();
        faultpoint::disarm_all();
    }
}

fn capture_boundary() {
    static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let i = COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    println!("CAPTURE {i}");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

#[test]
fn five_hundred_database_wal_copies_open_and_pass_integrity() {
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_copy_worker", "--nocapture"])
        .env("MEGURI_CRASH_COPY", &path)
        .env("MEGURI_CRASH_COUNT", "500")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    for i in 0..500 {
        let mut line = String::new();
        loop {
            line.clear();
            assert!(output.read_line(&mut line).unwrap() > 0);
            if line.starts_with("CAPTURE ") {
                break;
            }
        }
        assert_eq!(line.trim(), format!("CAPTURE {i}"));
        let copy_dir = dir.path().join(format!("copy-{i}"));
        std::fs::create_dir(&copy_dir).unwrap();
        let copy = copy_dir.join("collection.anki2");
        std::fs::copy(&path, &copy).unwrap();
        let wal = path.with_extension("anki2-wal");
        if wal.exists() {
            std::fs::copy(wal, copy.with_extension("anki2-wal")).unwrap();
        }
        let opened = common::open(&copy).unwrap();
        let reader = opened.open_reader(ReaderRole::Maintenance).unwrap();
        assert!(reader.quick_check().unwrap().is_ok(), "copy {i}");
        drop(reader);
        opened.close().unwrap();
        assert!(integrity_check(&copy).unwrap().is_ok(), "copy {i}");
        std::fs::remove_dir_all(copy_dir).unwrap();
        child.stdin.as_mut().unwrap().write_all(b"next\n").unwrap();
    }
    assert!(child.wait().unwrap().success());
}

#[test]
fn corrupted_page_is_reported_on_reader_while_writer_can_commit() {
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let opened = common::open(&path).unwrap();
    // Corrupt an index page the writer's unrelated col update never visits.
    let root = common::int(&common::exec(
        opened.writer(),
        "SELECT rootpage FROM sqlite_master WHERE name = 'ix_notes_csum'",
    ));
    let size = common::int(&common::exec(opened.writer(), "PRAGMA page_size"));
    use std::os::unix::fs::FileExt;
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all_at(&[0xff; 16], ((root - 1) * size) as u64).unwrap();
    // Closing an extra fd would drop SQLite's POSIX locks; hold it throughout.
    let reader = opened.open_reader(ReaderRole::Maintenance).unwrap();
    let (ready, start) = std::sync::mpsc::sync_channel(1);
    let scanner = std::thread::spawn(move || {
        ready.send(()).unwrap();
        for _ in 0..20 {
            assert!(!reader.quick_check().unwrap().is_ok());
        }
    });
    start.recv().unwrap();
    for _ in 0..20 {
        for sql in ["BEGIN IMMEDIATE", "UPDATE col SET mod = 123", "COMMIT"] {
            common::exec(opened.writer(), sql);
        }
    }
    scanner.join().unwrap();
    assert_eq!(common::int(&common::exec(opened.writer(), "SELECT mod FROM col")), 123);
    opened.close().unwrap();
    drop(file);
}

#[test]
fn commit_faults_leave_exactly_the_atomic_before_or_after_state() {
    use meguri_model::faultpoint::{self, Fault};
    use meguri_storage::ReadRows;
    for (point, committed) in [("storage.commit.before", false), ("storage.commit.after", true)] {
        let (_dir, path) = common::copy_fixture("schema-18.anki2");
        let opened = common::open(&path).unwrap();
        let original = opened.writer().notes_after(None, 1).unwrap().remove(0);
        let mut changed = original.clone();
        changed.fields = "fault-new".into();
        let clock = common::clock();
        let mut scope = opened
            .writer()
            .begin_write(meguri_storage::StampMode::Raw, &clock, None)
            .unwrap();
        scope.put(&changed).unwrap();
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        let result = scope.commit();
        if committed {
            assert!(
                result.unwrap().post_commit_error.is_some(),
                "post-commit failure must retain committed state"
            );
        } else {
            assert!(result.is_err(), "pre-commit failure must roll back");
        }
        faultpoint::disarm_all();
        assert!(opened.writer().is_autocommit());
        assert_eq!(
            opened.writer().note(original.id).unwrap().unwrap().fields,
            if committed { "fault-new" } else { &original.fields }
        );
        opened.close().unwrap();
        assert!(integrity_check(&path).unwrap().is_ok());
    }
}

#[test]
fn interrupted_managed_upgrade_keeps_unclean_intent_until_successful_reopen() {
    use meguri_model::{
        faultpoint::{self, Fault},
        rng::SeededRandom,
    };
    use meguri_storage::{OpenOptions, open_collection, sidecar::Sidecar};
    let (dir, path) = common::copy_fixture("schema-11.anki2");
    let sidecar_path = dir.path().join("meguri-sidecar.json");
    let clock = common::clock();
    let random = SeededRandom::new(7);
    let options = OpenOptions::new(&clock, &random).managed();
    faultpoint::arm(
        "storage.upgrade.in_transaction",
        Fault::IoError(std::io::ErrorKind::Other),
    );
    assert!(open_collection(&path, &options).is_err());
    faultpoint::disarm_all();
    let sidecar = Sidecar::load(&sidecar_path).unwrap();
    assert!(!sidecar.clean_shutdown);
    assert!(sidecar.upgrade_in_progress);
    let opened = open_collection(&path, &options).unwrap();
    assert!(!opened.previous_clean_shutdown);
    assert!(!Sidecar::load(&sidecar_path).unwrap().upgrade_in_progress);
    assert!(opened.close().unwrap().succeeded());
    assert!(Sidecar::load(&sidecar_path).unwrap().clean_shutdown);
    assert!(integrity_check(&path).unwrap().is_ok());
}

#[test]
fn incomplete_sidecar_never_claims_a_clean_shutdown() {
    use meguri_storage::sidecar::Sidecar;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meguri-sidecar.json");
    for bytes in [
        br#"{"clean_shutdown":true}"#.as_slice(),
        br#"{"clean_shutdown":true,"last_mod":7,"last_scm":8}"#.as_slice(),
        br#"{"clean_shutdown":true,"last_mod":"bad","last_scm":8,"upgrade_in_progress":false}"#.as_slice(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert!(!Sidecar::load(&path).unwrap().clean_shutdown);
    }
}

#[test]
fn zero_mod_and_scm_are_valid_external_change_baselines() {
    use meguri_model::rng::SeededRandom;
    use meguri_storage::{OpenOptions, open_collection};
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let db = common::plain(&path);
    db.execute_batch("UPDATE col SET mod = 0, scm = 0").unwrap();
    drop(db);
    let clock = common::clock();
    let random = SeededRandom::new(7);
    let options = OpenOptions::new(&clock, &random).managed();
    let opened = open_collection(&path, &options).unwrap();
    assert!(!opened.external_change, "no previous sidecar");
    opened.close().unwrap();
    let db = common::plain(&path);
    db.execute_batch("UPDATE col SET mod = 1").unwrap();
    drop(db);
    let opened = open_collection(&path, &options).unwrap();
    assert!(opened.external_change);
    opened.close().unwrap();
}

#[test]
fn quick_check_ignores_damage_in_attached_derived_store() {
    use meguri_storage::raw::{PlainDatabase, SqlValue};
    use std::os::unix::fs::FileExt;
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let derived = dir.path().join("meguri-derived.db");
    std::fs::File::create(&derived).unwrap();
    let db = PlainDatabase::open(&derived).unwrap();
    db.execute_batch("PRAGMA page_size=4096; CREATE TABLE cache(value TEXT); INSERT INTO cache VALUES ('cache')")
        .unwrap();
    drop(db);
    let file = std::fs::OpenOptions::new().write(true).open(&derived).unwrap();
    file.write_all_at(&[0xff; 16], 4096).unwrap();
    let opened = common::open(&path).unwrap();
    let mut reader = opened.open_reader(ReaderRole::Maintenance).unwrap();
    reader.attach_derived(&derived, 1).unwrap();
    let damaged = reader.execute_raw("PRAGMA derived.quick_check", &[]);
    assert!(damaged.is_err() || damaged.unwrap().rows != vec![vec![SqlValue::Text("ok".into())]]);
    assert!(reader.quick_check().unwrap().is_ok());
    drop(reader);
    opened.close().unwrap();
}

#[test]
fn counting_vfs_distinguishes_main_normal_sync_from_full_wal_sync() {
    use meguri_storage::counting_vfs::{CountingVfs, clock};
    use meguri_storage::rusqlite::{Connection, OpenFlags, ffi};
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let probe = CountingVfs::for_path(&path);
    let conn = Connection::open_with_flags(
        format!("file:{}?vfs=meguri-counting", path.display()),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    let before = clock::now();
    // Exercise the observer independently of sync_wal: the main file, NORMAL
    // flags. A hard-coded FULL flag or WAL filename in the observer fails.
    unsafe {
        let mut file = std::ptr::null_mut::<ffi::sqlite3_file>();
        assert_eq!(
            ffi::sqlite3_file_control(
                conn.handle(),
                c"main".as_ptr(),
                ffi::SQLITE_FCNTL_FILE_POINTER,
                std::ptr::from_mut(&mut file).cast()
            ),
            ffi::SQLITE_OK
        );
        assert_eq!(
            ((*(*file).pMethods).xSync.unwrap())(file, ffi::SQLITE_SYNC_NORMAL),
            ffi::SQLITE_OK
        );
    }
    let records = probe.records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].filename, path.to_string_lossy());
    assert_eq!(records[0].flags, 2); // SQLITE_SYNC_NORMAL
    assert_eq!(records[0].open_flags & 0x100, 0x100); // SQLITE_OPEN_MAIN_DB
    assert_eq!(records[0].result, 0);
    assert!(records[0].started >= before);
    assert!(records[0].completed >= records[0].started);
    assert!(records[0].completed <= clock::now());
    drop(conn);
    let opened = common::open(&path).unwrap();
    common::exec(opened.writer(), "UPDATE col SET mod = 42");
    assert!(opened.writer().sync_wal().unwrap());
    let records = probe.records();
    assert!(
        records.iter().any(|r| r.filename.ends_with("collection.anki2-wal")
            && r.open_flags & 0x80000 != 0
            && r.flags & 0x0f == 3)
    );
    opened.close().unwrap();
}
