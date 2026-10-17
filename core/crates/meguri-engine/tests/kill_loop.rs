//! Process kills, independent of Rust unwinding or SQLite close handlers.
mod common;
use meguri_engine::{Engine, EngineConfig};
use meguri_storage::{ReadRows, ReaderRole, integrity::integrity_check, sidecar::Sidecar};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Default)]
struct Pending {
    collection: std::sync::OnceLock<meguri_engine::Collection>,
}
impl meguri_engine::SuspendParticipant for Pending {
    fn name(&self) -> &str {
        "kill-test pending ops"
    }
    fn flush(&self) -> Result<(), meguri_engine::EngineError> {
        for i in 0..5 {
            self.collection.get().unwrap().run(
                meguri_engine::OpSpec::named("pending", meguri_engine::OpSource::UserUndoable),
                move |ctx| {
                    ctx.scope()
                        .add_note(&common::note(&[&format!("pending-{i}"), "back"]))?;
                    Ok(())
                },
            )?;
        }
        Ok(())
    }
}

fn child(path: &Path, mode: &str) -> Child {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "kill_worker", "--nocapture"])
        .env("MEGURI_KILL_PATH", path)
        .env("MEGURI_KILL_MODE", mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(lines.read_line(&mut line).unwrap() > 0, "worker stopped");
        if line.starts_with("KILL_READY") {
            break;
        }
    }
    child.stdout = Some(lines.into_inner());
    child
}
fn kill(child: &mut Child) {
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
}

#[test]
fn kill_worker() {
    let Some(path) = std::env::var_os("MEGURI_KILL_PATH") else {
        return;
    };
    let path = Path::new(&path);
    let mode = std::env::var("MEGURI_KILL_MODE").unwrap();
    let clock = std::sync::Arc::new(meguri_model::clock::StepClock::new(
        common::NOW_MS,
        meguri_model::clock::UtcOffset::UTC,
        "UTC",
    ));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let pending = std::sync::Arc::new(Pending::default());
    let mut config = common::open_config(&clock);
    if mode == "flushed" {
        config = config.ports(meguri_engine::Ports::new().suspend_participant(pending.clone()));
    }
    let collection = engine.open_raw(path, config).unwrap();
    pending.collection.set(collection.clone()).unwrap();
    match mode.as_str() {
        "answer" => {
            let (_, cid) = common::steps::setup_note(&collection, "kill answer");
            common::steps::answer(&collection, None, cid).unwrap();
        }
        "suspended" => {
            engine.suspend(Duration::from_secs(2)).unwrap();
        }
        "resumed" => {
            engine.suspend(Duration::from_secs(2)).unwrap();
            engine.resume().unwrap();
        }
        "flushed" => {
            engine.flush_for_background().unwrap();
            common::add_note_with_card(
                &collection,
                meguri_engine::OpSource::UserUndoable,
                &["following", "edit"],
            );
        }
        "half-replace" => {
            let mut file =
                meguri_storage::atomic_file::AtomicFile::new(path, meguri_storage::atomic_file::Purpose::Stage)
                    .unwrap();
            file.file_mut().write_all(&[0x55; 32768]).unwrap();
            // Exactly half a synthetic 64 KiB transfer, with no publish.
            std::mem::forget(file);
        }
        "lock-owner" => {}
        _ => panic!("unknown kill mode"),
    }
    println!("KILL_READY");
    std::io::stdout().flush().unwrap();
    if mode == "lock-owner" {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
        common::add_note_with_card(
            &collection,
            meguri_engine::OpSource::UserUndoable,
            &["owner still writing", "back"],
        );
        engine.close().unwrap();
        return;
    }
    loop {
        std::thread::park();
    }
}

#[test]
fn a_second_engine_process_is_refused_and_the_first_keeps_writing() {
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let mut owner = child(&path, "lock-owner");
    let clock = std::sync::Arc::new(meguri_model::clock::StepClock::new(
        common::NOW_MS,
        meguri_model::clock::UtcOffset::UTC,
        "UTC",
    ));
    let second = Engine::new(EngineConfig::default()).unwrap();
    let error = second.open_raw(&path, common::open_config(&clock)).unwrap_err();
    assert!(error.to_string().contains("open in another program"));
    owner.stdin.as_mut().unwrap().write_all(b"continue\n").unwrap();
    assert!(owner.wait().unwrap().success());
    let h = common::Harness::open_path(dir, path);
    assert_eq!(
        common::query_int(
            &h.collection,
            "SELECT count(*) FROM notes WHERE flds = 'owner still writing' || char(31) || 'back'"
        ),
        1
    );
    h.engine.close().unwrap();
    assert!(integrity_check(&h.path).unwrap().is_ok());
}

#[test]
fn five_flushed_pending_ops_and_following_edit_survive_sigkill() {
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let mut worker = child(&path, "flushed");
    kill(&mut worker);
    let h = common::Harness::open_path(dir, path);
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes WHERE flds LIKE 'pending-%'"),
        5
    );
    assert_eq!(
        common::query_int(
            &h.collection,
            "SELECT count(*) FROM notes WHERE flds = 'following' || char(31) || 'edit'"
        ),
        1
    );
    h.engine.close().unwrap();
    assert!(integrity_check(&h.path).unwrap().is_ok());
}

#[test]
fn one_thousand_answers_survive_sigkill_ten_ms_after_commit() {
    for iteration in 0..1000 {
        let (dir, path) = common::copy_fixture("schema-18.anki2");
        let mut worker = child(&path, "answer");
        std::thread::sleep(Duration::from_millis(10));
        kill(&mut worker);
        let h = common::Harness::open_path(dir, path);
        assert_eq!(
            common::query_int(&h.collection, "SELECT count(*) FROM revlog WHERE time = 4000"),
            1,
            "iteration {iteration}"
        );
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let mut found = 0;
        for card in reader.cards_after(None, 100).unwrap() {
            if reader
                .note(card.note_id)
                .unwrap()
                .is_some_and(|n| n.fields == "kill answer\u{1f}back")
            {
                assert_eq!(card.reps, 1);
                found += 1;
            }
        }
        assert_eq!(found, 1, "iteration {iteration}");
        drop(reader);
        h.engine.close().unwrap();
        assert!(integrity_check(&h.path).unwrap().is_ok(), "iteration {iteration}");
    }
}

#[test]
fn sidecar_distinguishes_suspended_kill_and_resumed_kill() {
    for (mode, clean) in [("suspended", true), ("resumed", false)] {
        let (dir, path) = common::copy_fixture("schema-18.anki2");
        let mut worker = child(&path, mode);
        kill(&mut worker);
        assert_eq!(
            Sidecar::load(&dir.path().join("meguri-sidecar.json"))
                .unwrap()
                .clean_shutdown,
            clean
        );
        let h = common::Harness::open_path(dir, path);
        assert_eq!(h.engine.previous_clean_shutdown(), Some(clean));
        h.engine.close().unwrap();
        assert!(
            Sidecar::load(&h.dir.path().join("meguri-sidecar.json"))
                .unwrap()
                .clean_shutdown
        );
    }
}

#[test]
fn half_written_replacement_killed_keeps_previous_collection_and_launch_sweeps_temp() {
    let fixture = meguri_testkit::bench::fixtures::profile_collection("150k").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(fixture.dir.join("collection.anki2"), &path).unwrap();
    let h = common::Harness::open_path(dir, path);
    let (_, cid) = common::steps::setup_note(&h.collection, "prior review");
    common::steps::answer(&h.collection, None, cid).unwrap();
    h.engine.close().unwrap();
    let before = std::fs::read(&h.path).unwrap();
    let mut worker = child(&h.path, "half-replace");
    kill(&mut worker);
    assert_eq!(std::fs::read(&h.path).unwrap(), before);
    assert!(std::fs::read_dir(h.dir.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".meguri-tmp-stage-")
    }));
    let collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(
        common::query_int(
            &collection,
            "SELECT count(*) FROM revlog WHERE cid = (SELECT c.id FROM cards c JOIN notes n ON n.id = c.nid WHERE n.flds = 'prior review' || char(31) || 'back') AND time = 4000"
        ),
        1
    );
    h.engine.close().unwrap();
    assert!(integrity_check(&h.path).unwrap().is_ok());
    assert!(
        !std::fs::read_dir(h.dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".meguri-tmp-"))
    );
}
