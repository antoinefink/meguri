//! The Meguri side of the timed operations (`specs/conformance/timed-operations.md`):
//! the catalogue, the protocol with stand-in operations, the image and the
//! summary, the import package, the sync stub and its raw zstd frame.
//! Expected values come from the spec's text, never from the code.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_testkit::timing::catalogue::{self, Operation};
use meguri_testkit::timing::package::{self, crc32};
use meguri_testkit::timing::protocol::{self, Check, ErrorKind, Outcome, RunContext, RunFailure, Stage};
use meguri_testkit::timing::run::{self, RunError, Session, TimingRequest};
use meguri_testkit::timing::stub::{self, SyncStub};
use meguri_testkit::timing::{MeguriBuild, TimedOperation, TimedRun};
use serde_json::{Value, json};

// ---------------------------------------------------------------- catalogue

#[test]
fn the_catalogue_is_t6_in_order() {
    let names: Vec<&str> = Operation::ALL.iter().map(|op| op.name()).collect();
    assert_eq!(
        names,
        [
            "collection.open",
            "decks.counts",
            "queue.build",
            "review.answer",
            "search.deck",
            "search.state",
            "search.tag",
            "search.text",
            "search.regex",
            "search.prop",
            "search.rated",
            "search.combined",
            "browser.sort_field",
            "browser.sort_due",
            "stats.graphs",
            "maintenance.check_db",
            "package.apkg_export",
            "package.colpkg_export",
            "package.apkg_import",
            "sync.full_upload",
            "sync.full_download",
            "fsrs.recompute_memory_states",
        ]
    );
    assert_eq!(
        Operation::SearchCombined.query(),
        Some("deck:current (is:new OR is:due) -is:suspended")
    );
    assert_eq!(Operation::SearchTag.query(), Some("tag:*::*"));
    assert_eq!(Operation::BrowserSortDue.sort_column(), Some("cardDue"));
    assert_eq!(Operation::CollectionOpen.query(), None);
}

#[test]
fn op_names_run_in_catalogue_order_once_and_an_unknown_name_is_refused() {
    let names = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        catalogue::select(&names(&["sync.full_upload", "collection.open", "sync.full_upload"])),
        Ok(vec![Operation::CollectionOpen, Operation::SyncFullUpload])
    );
    assert_eq!(catalogue::select(&[]).map(|ops| ops.len()), Ok(22));
    assert_eq!(
        catalogue::select(&names(&["collection.open", "Collection.Open"])),
        Err("Collection.Open".to_owned())
    );
}

// --------------------------------------------------------------- statistic

fn ok(samples_us: &[u64]) -> Outcome {
    Outcome::Ok {
        samples_us: samples_us.to_vec(),
        check: Check::new(),
    }
}

#[test]
fn the_median_is_the_lower_middle_sample_and_never_an_average() {
    assert_eq!(ok(&[5, 1, 4, 2, 3]).median_us(), Some(3));
    assert_eq!(ok(&[5, 1, 4, 2]).median_us(), Some(2));
    assert_eq!(ok(&[7]).median_us(), Some(7));
    assert_eq!(ok(&[5, 1, 4, 2]).min_us(), Some(1));
    assert_eq!(Outcome::Unsupported.median_us(), None);
}

#[test]
fn samples_are_milliseconds_rounded_to_the_microsecond_halves_up() {
    assert_eq!(protocol::micros(Duration::from_nanos(1_499)), 1);
    assert_eq!(protocol::micros(Duration::from_nanos(1_500)), 2);
    assert_eq!(protocol::micros(Duration::from_nanos(41_206_500)), 41_207);
    let mut check = Check::new();
    check.insert("cards".into(), json!(3));
    let json = Value::Object(
        Outcome::Ok {
            samples_us: vec![41_207, 40_883, 41_950, 40_512, 41_031],
            check,
        }
        .to_json(),
    );
    assert_eq!(
        json,
        json!({
            "status": "ok",
            "samples_ms": [41.207, 40.883, 41.95, 40.512, 41.031],
            "statistic": {"median_ms": 41.031, "min_ms": 40.512},
            "check": {"cards": 3},
        })
    );
}

#[test]
fn an_error_entry_has_kind_stage_and_only_an_exception_name() {
    let error = Outcome::Error {
        kind: ErrorKind::NoInput,
        stage: Stage::Setup,
        exception: None,
    };
    assert_eq!(
        Value::Object(error.to_json()),
        json!({"status": "error", "error": {"kind": "no_input", "stage": "setup"}})
    );
    let raised = Outcome::Error {
        kind: ErrorKind::Failed,
        stage: Stage::Timed,
        exception: Some("EngineError::Unavailable".to_owned()),
    };
    assert_eq!(
        Value::Object(raised.to_json()),
        json!({"status": "error", "error": {"kind": "failed", "stage": "timed", "exception": "EngineError::Unavailable"}})
    );
    assert_eq!(
        Value::Object(Outcome::Unsupported.to_json()),
        json!({"status": "unsupported"})
    );
}

// ----------------------------------------------------------------- protocol

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2")
}

/// A collection directory holding a copy of the schema-18 fixture.
fn collection_dir(root: &Path) -> PathBuf {
    let dir = root.join("collection");
    fs::create_dir_all(&dir).unwrap();
    fs::copy(fixture(), dir.join("collection.anki2")).unwrap();
    dir
}

/// 2030-01-15T10:00:00Z, the generator's default instant.
const NOW: i64 = 1_894_701_600;

fn session(root: &Path) -> Session {
    Session::prepare(&collection_dir(root), &root.join("scratch"), NOW, "Europe/Paris").unwrap()
}

/// What a stand-in operation does in each part of a run.
#[derive(Default)]
struct Script {
    runs: AtomicU32,
    /// Sleep in setup and in the check, outside the interval.
    outside: Duration,
    /// Sleep in the timed part.
    inside: Duration,
    fail: Option<(Stage, RunFailure)>,
    fail_close: bool,
    panic_in_timed: bool,
    /// The check is the run's number, so it differs between runs.
    unstable: bool,
}

struct ScriptRun<'a> {
    script: &'a Script,
    context: &'a RunContext<'a>,
    number: u32,
}

impl Script {
    fn fails(&self, stage: Stage) -> Result<(), RunFailure> {
        match &self.fail {
            Some((at, failure)) if *at == stage => Err(failure.clone()),
            _ => Ok(()),
        }
    }
}

impl TimedOperation for Script {
    fn start<'a>(&'a self, run: &'a RunContext<'a>) -> Box<dyn TimedRun + 'a> {
        Box::new(ScriptRun {
            script: self,
            context: run,
            number: self.runs.fetch_add(1, Ordering::SeqCst),
        })
    }
}

impl TimedRun for ScriptRun<'_> {
    fn setup(&mut self) -> Result<(), RunFailure> {
        // Every run gets a fresh copy of the image in a fresh directory: no
        // other run's file is there, and the copy is the image byte for byte.
        let marker = self.context.dir.join("written-by-a-run");
        assert!(!marker.exists(), "run {} sees another run's file", self.number);
        fs::write(&marker, b"x").unwrap();
        assert_eq!(
            fs::read(self.context.collection).unwrap(),
            fs::read(self.context.session.image()).unwrap()
        );
        assert_ne!(self.context.collection, self.context.session.image());
        std::thread::sleep(self.script.outside);
        self.script.fails(Stage::Setup)
    }

    fn timed(&mut self) -> Result<(), RunFailure> {
        assert!(!self.script.panic_in_timed, "a stand-in panic");
        std::thread::sleep(self.script.inside);
        self.script.fails(Stage::Timed)
    }

    fn check(&mut self) -> Result<Check, RunFailure> {
        std::thread::sleep(self.script.outside);
        self.script.fails(Stage::Check)?;
        let mut check = Check::new();
        check.insert(
            "cards".into(),
            json!(if self.script.unstable { self.number } else { 42 }),
        );
        Ok(check)
    }

    fn close(self: Box<Self>) -> Result<(), RunFailure> {
        if self.script.fail_close {
            return Err(RunFailure::failed("CloseError"));
        }
        Ok(())
    }
}

#[test]
fn a_warm_up_and_repeat_timed_runs_each_on_a_fresh_copy() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    let script = Script::default();
    let outcome = protocol::time_operation(&script, &session, 3);
    assert_eq!(
        script.runs.load(Ordering::SeqCst),
        4,
        "one warm-up and three timed runs"
    );
    let Outcome::Ok { samples_us, check } = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(samples_us.len(), 3, "the warm-up's sample is discarded");
    assert_eq!(Value::Object(check), json!({"cards": 42}));
    for index in 0..4 {
        assert!(!session.run_dir(index).exists(), "run {index}'s directory is deleted");
    }
}

#[test]
fn timing_measures_only_the_timed_part() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    let script = Script {
        outside: Duration::from_millis(400),
        inside: Duration::from_millis(5),
        ..Script::default()
    };
    let Outcome::Ok { samples_us, .. } = protocol::time_operation(&script, &session, 2) else {
        panic!("not ok");
    };
    for sample in samples_us {
        assert!(
            (5_000..400_000).contains(&sample),
            "{sample} µs: the 5 ms timed part only, not the 400 ms setup or check"
        );
    }
}

#[test]
fn a_check_that_changes_between_runs_is_unstable() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    let script = Script {
        unstable: true,
        ..Script::default()
    };
    assert_eq!(
        protocol::time_operation(&script, &session, 5),
        Outcome::Error {
            kind: ErrorKind::Unstable,
            stage: Stage::Check,
            exception: None
        }
    );
    assert_eq!(
        script.runs.load(Ordering::SeqCst),
        2,
        "the first differing run ends the operation"
    );
}

#[test]
fn a_failing_call_ends_the_operation_at_its_stage() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    for (stage, failure) in [
        (Stage::Setup, RunFailure::no_input()),
        (Stage::Setup, RunFailure::failed("SetupError")),
        (Stage::Timed, RunFailure::failed("TimedError")),
        (Stage::Check, RunFailure::failed("CheckError")),
    ] {
        let script = Script {
            fail: Some((stage, failure.clone())),
            ..Script::default()
        };
        assert_eq!(
            protocol::time_operation(&script, &session, 5),
            Outcome::Error {
                kind: failure.kind,
                stage,
                exception: failure.exception.clone()
            }
        );
        assert_eq!(
            script.runs.load(Ordering::SeqCst),
            1,
            "{stage:?}: the first failing run ends it"
        );
        assert!(
            !session.run_dir(0).exists(),
            "{stage:?}: the failed run's directory is deleted"
        );
    }
    let closing = Script {
        fail_close: true,
        ..Script::default()
    };
    assert_eq!(
        protocol::time_operation(&closing, &session, 5),
        Outcome::Error {
            kind: ErrorKind::Failed,
            stage: Stage::Check,
            exception: Some("CloseError".to_owned())
        }
    );
    let panicking = Script {
        panic_in_timed: true,
        ..Script::default()
    };
    assert_eq!(
        protocol::time_operation(&panicking, &session, 5),
        Outcome::Error {
            kind: ErrorKind::Failed,
            stage: Stage::Timed,
            exception: Some("panic".to_owned())
        }
    );
}

// ---------------------------------------------------- image, summary, run

fn count(path: &Path, table: &str) -> i64 {
    let db = PlainDatabase::open(path).unwrap();
    let rows = db.rows(&format!("SELECT count(*) FROM {table}"), &[]).unwrap();
    db.close().unwrap();
    match rows[0][0] {
        RawValue::Integer(n) => n,
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn the_image_folds_the_log_in_and_leaves_the_source_as_it_was() {
    let root = tempfile::tempdir().unwrap();
    // A collection whose log holds rows the main file does not: three
    // graves written in WAL mode with checkpoints off, copied with its -wal
    // while the writer is still open.
    let work = root.path().join("work");
    fs::create_dir_all(&work).unwrap();
    fs::copy(fixture(), work.join("collection.anki2")).unwrap();
    let graves_before = count(&work.join("collection.anki2"), "graves");
    let db = PlainDatabase::open(&work.join("collection.anki2")).unwrap();
    db.rows("PRAGMA journal_mode=WAL", &[]).unwrap();
    db.execute_batch("PRAGMA wal_autocheckpoint=0").unwrap();
    for oid in 1..=3 {
        db.execute(
            "INSERT INTO graves (oid, type, usn) VALUES (?1, 0, -1)",
            &[RawValue::Integer(9_000 + oid)],
        )
        .unwrap();
    }
    let dir = root.path().join("source");
    fs::create_dir_all(&dir).unwrap();
    fs::copy(work.join("collection.anki2"), dir.join("collection.anki2")).unwrap();
    fs::copy(work.join("collection.anki2-wal"), dir.join("collection.anki2-wal")).unwrap();
    db.close().unwrap();
    assert!(fs::metadata(dir.join("collection.anki2-wal")).unwrap().len() > 0);
    let before = (
        fs::read(dir.join("collection.anki2")).unwrap(),
        fs::read(dir.join("collection.anki2-wal")).unwrap(),
    );

    let scratch = root.path().join("scratch");
    let session = Session::prepare(&dir, &scratch, NOW, "UTC").unwrap();
    let image = session.image().to_path_buf();
    assert!(
        !image.with_file_name("collection.anki2-wal").exists(),
        "no -wal beside the image"
    );
    // The image alone, copied without any log, holds the three graves.
    let alone = root.path().join("alone.anki2");
    fs::copy(&image, &alone).unwrap();
    assert_eq!(count(&alone, "graves"), graves_before + 3);
    assert_eq!(
        session.summary().unwrap().graves,
        u64::try_from(graves_before + 3).unwrap()
    );

    drop(session);
    assert_eq!(
        fs::read_dir(&scratch).unwrap().count(),
        0,
        "the scratch directory is deleted"
    );
    let after = (
        fs::read(dir.join("collection.anki2")).unwrap(),
        fs::read(dir.join("collection.anki2-wal")).unwrap(),
    );
    assert!(before == after, "<dir> is never written");
}

#[test]
fn the_summary_counts_the_rows_of_the_eight_tables() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    let summary = session.summary().unwrap();
    let source = fixture();
    let expected = [
        ("notes", summary.notes),
        ("cards", summary.cards),
        ("revlog", summary.revlog),
        ("notetypes", summary.notetypes),
        ("decks", summary.decks),
        ("deck_config", summary.presets),
        ("tags", summary.tags),
        ("graves", summary.graves),
    ];
    for (table, counted) in expected {
        assert_eq!(i64::try_from(counted).unwrap(), count(&source, table), "{table}");
    }
    assert!(summary.notetypes > 0 && summary.decks > 0 && summary.presets > 0);
}

#[test]
fn a_file_that_is_not_a_collection_fails_the_open() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("junk");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("collection.anki2"), b"this is not an SQLite file, only text").unwrap();
    let request = TimingRequest {
        dir,
        now: Some("2030-01-15T10:00:00Z".into()),
        tz: Some("UTC".into()),
        scratch_root: root.path().join("scratch"),
        ..TimingRequest::default()
    };
    let error = run::run(&request, &|_| None, build()).unwrap_err();
    assert_eq!(error.kind(), "open_failed", "{error:?}");
    assert_eq!(fs::read_dir(root.path().join("scratch")).unwrap().count(), 0);
}

fn build() -> MeguriBuild {
    MeguriBuild {
        commit: None,
        profile: "release".to_owned(),
    }
}

#[test]
fn a_run_reports_unsupported_operations_without_running_them() {
    let root = tempfile::tempdir().unwrap();
    let script = Script::default();
    let request = TimingRequest {
        dir: collection_dir(root.path()),
        ops: vec!["search.deck".into(), "collection.open".into()],
        now: Some("2030-01-15T11:00:00+01:00".into()),
        tz: Some("Europe/Paris".into()),
        repeat: Some("2".into()),
        scratch_root: root.path().join("scratch"),
    };
    let side = |op: Operation| (op == Operation::CollectionOpen).then_some(&script as &dyn TimedOperation);
    let report = run::run(&request, &side, build()).unwrap();
    let json = report.to_json();
    assert_eq!(json["now"], json!(NOW));
    assert_eq!(json["tz"], json!("Europe/Paris"));
    assert_eq!(json["protocol"], json!({"warmup": 1, "repeat": 2}));
    assert_eq!(json["side"], json!("meguri"));
    assert_eq!(json["meguri"], json!({"commit": null, "profile": "release"}));
    let names: Vec<&Value> = json["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| &o["name"])
        .collect();
    assert_eq!(names, [&json!("collection.open"), &json!("search.deck")]);
    assert_eq!(json["operations"][0]["status"], json!("ok"));
    assert_eq!(json["operations"][0]["samples_ms"].as_array().unwrap().len(), 2);
    assert_eq!(
        json["operations"][1],
        json!({"name": "search.deck", "status": "unsupported"})
    );
    assert_eq!(script.runs.load(Ordering::SeqCst), 3);
    assert_eq!(fs::read_dir(root.path().join("scratch")).unwrap().count(), 0);
}

#[test]
fn usage_errors_name_what_is_wrong() {
    let root = tempfile::tempdir().unwrap();
    let dir = collection_dir(root.path());
    let request = |f: &dyn Fn(&mut TimingRequest)| {
        let mut request = TimingRequest {
            dir: dir.clone(),
            now: Some("2030-01-15T10:00:00Z".into()),
            tz: Some("UTC".into()),
            scratch_root: root.path().join("scratch"),
            ..TimingRequest::default()
        };
        f(&mut request);
        run::run(&request, &|_| None, build()).map(|_| ())
    };
    let usage = |result: Result<(), RunError>| match result {
        Err(RunError::Usage(_)) => {}
        other => panic!("{other:?}"),
    };
    usage(request(&|r| r.ops = vec!["collection.close".into()]));
    usage(request(&|r| r.repeat = Some("0".into())));
    usage(request(&|r| r.repeat = Some("101".into())));
    usage(request(&|r| r.repeat = Some("five".into())));
    usage(request(&|r| r.now = Some("2030-01-15T10:00:00".into())));
    usage(request(&|r| r.now = Some("2030-01-15T10:00:00.5Z".into())));
    usage(request(&|r| r.now = None));
    usage(request(&|r| r.tz = None));
    usage(request(&|r| r.tz = Some("Mars/Olympus".into())));
    assert!(request(&|r| r.repeat = Some("100".into())).is_ok());
    let missing = request(&|r| r.dir = root.path().join("nowhere"));
    assert!(matches!(missing, Err(RunError::NoCollection(_))), "{missing:?}");
}

#[test]
fn a_manifest_gives_now_and_the_zone_as_k1_derives_them() {
    let root = tempfile::tempdir().unwrap();
    let dir = collection_dir(root.path());
    let plan = |offset: i64| {
        fs::write(
            dir.join("manifest.json"),
            json!({"now_ms": 1_894_701_600_999i64, "utc_offset_secs": offset}).to_string(),
        )
        .unwrap();
        run::plan(&TimingRequest {
            dir: dir.clone(),
            ..TimingRequest::default()
        })
    };
    let east = plan(3600).unwrap();
    assert_eq!((east.now, east.tz.as_str(), east.repeat), (NOW, "Etc/GMT-1", 5));
    assert_eq!(plan(0).unwrap().tz, "UTC");
    assert_eq!(plan(-5 * 3600).unwrap().tz, "Etc/GMT+5");
    assert_eq!(plan(14 * 3600).unwrap().tz, "Etc/GMT-14");
    assert!(matches!(plan(5 * 3600 + 1800), Err(RunError::Usage(_))));
    assert!(matches!(plan(15 * 3600), Err(RunError::Usage(_))));
}

// ------------------------------------------------------------------ package

#[test]
fn crc32_is_zips() {
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn the_import_package_holds_the_image_then_an_empty_media_map_both_stored() {
    let root = tempfile::tempdir().unwrap();
    let image = root.path().join("image.anki2");
    let content: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
    fs::write(&image, &content).unwrap();
    let out = root.path().join("import.apkg");
    package::write_import_package(&image, &out).unwrap();
    let zip = fs::read(&out).unwrap();

    // The end record names two entries and where the central directory is.
    let end = zip.len() - 22;
    assert_eq!(u32_at(&zip, end), 0x0605_4b50);
    assert_eq!(u16_at(&zip, end + 10), 2);
    let mut central = u32_at(&zip, end + 16) as usize;
    let mut entries = Vec::new();
    for _ in 0..2 {
        assert_eq!(u32_at(&zip, central), 0x0201_4b50);
        let method = u16_at(&zip, central + 10);
        let crc = u32_at(&zip, central + 16);
        let size = u32_at(&zip, central + 20) as usize;
        assert_eq!(u32_at(&zip, central + 24) as usize, size, "stored: both sizes equal");
        let name_len = usize::from(u16_at(&zip, central + 28));
        let extra = usize::from(u16_at(&zip, central + 30));
        let comment = usize::from(u16_at(&zip, central + 32));
        let local = u32_at(&zip, central + 42) as usize;
        let name = String::from_utf8(zip[central + 46..central + 46 + name_len].to_vec()).unwrap();
        // The local header agrees and the data follows it.
        assert_eq!(u32_at(&zip, local), 0x0403_4b50);
        assert_eq!(u16_at(&zip, local + 8), 0);
        let data_at = local + 30 + usize::from(u16_at(&zip, local + 26)) + usize::from(u16_at(&zip, local + 28));
        let data = zip[data_at..data_at + size].to_vec();
        entries.push((name, method, crc, data));
        central += 46 + name_len + extra + comment;
    }
    assert_eq!(entries[0].0, "collection.anki2");
    assert_eq!(entries[0].1, 0, "stored without compression");
    assert_eq!(entries[0].3, content);
    assert_eq!(entries[0].2, crc32(&content));
    assert_eq!(entries[1].0, "media");
    assert_eq!(entries[1].1, 0);
    assert_eq!(entries[1].3, b"{}");
    assert_eq!(entries[1].2, crc32(b"{}"));
}

#[test]
fn a_session_builds_the_package_once_from_its_image() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    let first = session.package().unwrap().to_path_buf();
    let modified = fs::metadata(&first).unwrap().modified().unwrap();
    assert_eq!(session.package().unwrap(), first);
    assert_eq!(fs::metadata(&first).unwrap().modified().unwrap(), modified);
    let zip = fs::read(&first).unwrap();
    let image = fs::read(session.image()).unwrap();
    assert_eq!(&zip[30 + "collection.anki2".len()..][..image.len()], &image[..]);
}

// --------------------------------------------------------------------- stub

#[test]
fn the_raw_frame_of_ok_is_the_specs_example() {
    assert_eq!(
        stub::raw_zstd_frame(b"OK"),
        [0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x38, 0x11, 0x00, 0x00, 0x4F, 0x4B]
    );
    assert_eq!(
        stub::raw_zstd_frame(b""),
        [0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x38, 0x01, 0x00, 0x00]
    );
}

#[test]
fn the_raw_frame_cuts_its_content_into_128_kib_raw_blocks() {
    let content: Vec<u8> = (0..2 * 131_072 + 5).map(|i| (i % 7) as u8).collect();
    let frame = stub::raw_zstd_frame(&content);
    assert_eq!(frame.len(), 6 + 3 * 3 + content.len());
    // 8 × 131,072 = 0x100000; the last block: 1 + 8 × 5 = 41.
    assert_eq!(frame[6..9], [0x00, 0x00, 0x10]);
    assert_eq!(frame[9..9 + 131_072], content[..131_072]);
    let second = 9 + 131_072;
    assert_eq!(frame[second..second + 3], [0x00, 0x00, 0x10]);
    let third = second + 3 + 131_072;
    assert_eq!(frame[third..third + 3], [0x29, 0x00, 0x00]);
    assert_eq!(frame[third + 3..], content[2 * 131_072..]);

    // An exact multiple has no extra empty block.
    let exact = stub::raw_zstd_frame(&content[..131_072]);
    assert_eq!(exact.len(), 6 + 3 + 131_072);
    assert_eq!(exact[6..9], [0x01, 0x00, 0x10]);
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_response(reader: &mut BufReader<TcpStream>) -> Response {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let status = line.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut headers = Vec::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').unwrap();
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    let length: usize = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| v.parse().unwrap())
        .unwrap();
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    Response { status, headers, body }
}

#[test]
fn the_stub_answers_upload_and_download_on_one_connection_and_404_otherwise() {
    let image: Vec<u8> = (0..300_000u32).map(|i| (i % 253) as u8).collect();
    let stub = SyncStub::start(&image).unwrap();
    assert_eq!(stub.endpoint(), format!("http://127.0.0.1:{}/", stub.port()));
    let stream = TcpStream::connect(("127.0.0.1", stub.port())).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    // An upload with a body of known length, headers the stub ignores.
    let body = vec![7u8; 5_000];
    write!(
        writer,
        "POST /sync/upload HTTP/1.1\r\nHost: x\r\nanki-sync: {{\"v\":11}}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .unwrap();
    writer.write_all(&body).unwrap();
    let upload = read_response(&mut reader);
    assert_eq!(upload.status, 200);
    assert_eq!(upload.header("anki-original-size"), Some("2"));
    assert_eq!(upload.body, stub::raw_zstd_frame(b"OK"));

    // A download whose body comes chunked, on the same connection.
    writer
        .write_all(b"POST /sync/download HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n4;ext=1\r\nabcd\r\n3\r\nefg\r\n0\r\nX-Trailer: t\r\n\r\n")
        .unwrap();
    let download = read_response(&mut reader);
    assert_eq!(download.status, 200);
    assert_eq!(download.header("anki-original-size"), Some("300000"));
    assert_eq!(download.body, stub::raw_zstd_frame(&image));

    // Anything else is 404 without a body.
    writer.write_all(b"GET /sync/upload HTTP/1.1\r\n\r\n").unwrap();
    let get = read_response(&mut reader);
    assert_eq!((get.status, get.body.len()), (404, 0));
    writer
        .write_all(b"POST /sync/meta HTTP/1.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
        .unwrap();
    let other = read_response(&mut reader);
    assert_eq!((other.status, other.body.len()), (404, 0));
    // Connection: close ends the connection after the answer.
    let mut rest = Vec::new();
    assert_eq!(reader.read_to_end(&mut rest).unwrap(), 0);

    // Dropping the stub stops it: nothing listens on its port any more.
    let port = stub.port();
    let open = TcpStream::connect(("127.0.0.1", port)).unwrap();
    drop(stub);
    let mut gone = Vec::new();
    open.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    assert_eq!(
        (&open).read_to_end(&mut gone).unwrap_or(0),
        0,
        "open connections are closed"
    );
    assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[test]
fn a_sessions_stub_serves_the_image() {
    let root = tempfile::tempdir().unwrap();
    let session = session(root.path());
    let port = session.stub().unwrap().port();
    assert_eq!(session.stub().unwrap().port(), port, "started once");
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut writer = stream.try_clone().unwrap();
    writer
        .write_all(b"POST /sync/download HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
        .unwrap();
    let response = read_response(&mut BufReader::new(stream));
    assert_eq!(response.body, stub::raw_zstd_frame(&fs::read(session.image()).unwrap()));
}
