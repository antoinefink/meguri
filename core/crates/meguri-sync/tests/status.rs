//! The `meta` status check, the sync status and the meta probe
//! (specs/sync/meta-status.md) against meguri-testkit's scripted stub: the
//! official server never answers `cont` false, a non-empty `msg` or an
//! undecodable `meta`, and a stub's clock is the test's to set (M9). The
//! `rig_*` tests make the same calls against the U13 rig's official server.
//!
//! Expected values come from the spec's tables, never from the code under
//! test.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::{HOST_KEY, base, block_on, job, job_with_key, options, random, rig, threads};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::clock::{FixedClock, StepClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::ids::{CardId, DeckId, NoteId};
use meguri_model::rng::SeededRandom;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::{CollectionBuilder, OpenOptions, StampMode, open_collection};
use meguri_sync::status::{self, MAX_CLOCK_OFFSET_SECS};
use meguri_sync::wire::{self, ServerMeta};
use meguri_sync::{
    CheckResult, Endpoint, ErrorKind, HostKey, Job, JobError, JobOptions, LocalRecord, Method, OfflineStatus, Payload,
    Proxy, StatusMemory, SyncStatus, login_blocking,
};
use meguri_testkit::sync_stub::{Recorded, Reply, SyncStub};

/// The device's clock in these tests: 2030-01-15T10:03:00Z.
const NOW_MS: i64 = 1_894_701_780_000;
const NOW_SECS: i64 = NOW_MS / 1000;

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

/// A `meta` answer (wire-format.md W3).
#[derive(Clone)]
struct Meta {
    modified: i64,
    schema: i64,
    usn: i32,
    ts: i64,
    msg: &'static str,
    cont: bool,
    empty: bool,
}

impl Meta {
    fn json(&self) -> String {
        format!(
            r#"{{"mod":{},"scm":{},"usn":{},"ts":{},"msg":{},"cont":{},"empty":{},"hostNum":0,"media_usn":7}}"#,
            self.modified,
            self.schema,
            self.usn,
            self.ts,
            serde_json::to_string(self.msg).unwrap(),
            self.cont,
            self.empty
        )
    }
}

/// The server's record in most tests: changed since the device's last sync,
/// same schema, its clock with the device's.
fn server() -> Meta {
    Meta {
        modified: NOW_MS - 60_000,
        schema: NOW_MS - 3_600_000,
        usn: 5,
        ts: NOW_SECS,
        msg: "",
        cont: true,
        empty: false,
    }
}

/// The device's record matching [`server`]'s schema, synced before both
/// changed.
fn local() -> LocalRecord {
    LocalRecord {
        modified: EpochMillis(NOW_MS - 120_000),
        schema_modified: EpochMillis(NOW_MS - 3_600_000),
        usn: Usn(4),
        last_sync: EpochMillis(NOW_MS - 120_000),
        has_cards: true,
    }
}

fn meta_stub(meta: &Meta) -> SyncStub {
    let body = meta.json();
    SyncStub::serve(move |_| Reply::ok(body.as_bytes()))
}

fn check(stub: &SyncStub, local: &LocalRecord, clock: &StepClock) -> Result<status::CheckOutcome, JobError> {
    let mut job = job(&options(&stub.url()));
    block_on(status::check(&mut job, local, clock))
}

#[track_caller]
fn assert_kind(result: Result<impl std::fmt::Debug, JobError>, kind: ErrorKind) -> JobError {
    let err = result.expect_err("the check fails");
    assert_eq!(err.kind, kind, "{err}");
    assert_eq!(err.method, Some(Method::Meta), "{err}");
    err
}

// --- M2, M3: the check ----------------------------------------------------------

#[test]
fn meta_sends_its_version_and_identity_and_nothing_else() {
    let stub = meta_stub(&server());
    check(&stub, &local(), &clock()).unwrap();
    let requests = stub.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].head.path(), "/sync/meta");
    let body = String::from_utf8(requests[0].body.clone().unwrap()).unwrap();
    assert_eq!(body, format!(r#"{{"v":11,"cv":"{}"}}"#, meguri_sync::client_identity()));
}

#[test]
fn the_comparison_follows_m3s_table() {
    let differing_schema = |empty: bool| Meta {
        schema: NOW_MS - 1_000,
        empty,
        ..server()
    };
    let no_cards = LocalRecord {
        has_cards: false,
        ..local()
    };
    for (meta, local, expected) in [
        // Equal `mod`: no changes, even with differing schema times.
        (
            Meta {
                modified: local().modified.0,
                ..differing_schema(false)
            },
            local(),
            CheckResult::NoChanges,
        ),
        // One-way, by who has cards.
        (
            differing_schema(false),
            local(),
            CheckResult::OneWay {
                upload: true,
                download: true,
            },
        ),
        (
            differing_schema(false),
            no_cards,
            CheckResult::OneWay {
                upload: false,
                download: true,
            },
        ),
        (
            differing_schema(true),
            local(),
            CheckResult::OneWay {
                upload: true,
                download: false,
            },
        ),
        (
            differing_schema(true),
            no_cards,
            CheckResult::OneWay {
                upload: true,
                download: true,
            },
        ),
        // Normal, `lnewer` by `mod`.
        (server(), local(), CheckResult::Normal { local_is_newer: false }),
        (
            Meta {
                modified: NOW_MS - 300_000,
                ..server()
            },
            local(),
            CheckResult::Normal { local_is_newer: true },
        ),
    ] {
        let outcome = check(&meta_stub(&meta), &local, &clock()).unwrap();
        assert_eq!(outcome.result, expected, "{}", meta.json());
        // M4: the server's record and the local usn.
        assert_eq!(outcome.server.modified, EpochMillis(meta.modified));
        assert_eq!(outcome.server.usn, Usn(5));
        assert_eq!(outcome.server.media_usn, 7);
        assert_eq!(outcome.local_usn, Usn(4));
        assert_eq!((outcome.server_message, outcome.new_endpoint), (None, None));
    }
}

#[test]
fn a_new_account_and_a_never_synced_collection_find_no_changes() {
    // M3: both `mod` 0.
    let meta = Meta {
        modified: 0,
        schema: NOW_MS,
        usn: 0,
        empty: true,
        ..server()
    };
    let new = LocalRecord {
        modified: EpochMillis(0),
        schema_modified: EpochMillis(NOW_MS - 5_000),
        usn: Usn(0),
        last_sync: EpochMillis(0),
        has_cards: false,
    };
    assert_eq!(
        check(&meta_stub(&meta), &new, &clock()).unwrap().result,
        CheckResult::NoChanges
    );
}

/// A new collection file with `col` as given, and one card when `card`.
fn collection(dir: &Path, col: &Col, card: bool) -> PathBuf {
    let path = dir.join("collection.anki2");
    let clock = FixedClock::new(NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(7);
    let config = [ConfigEntry {
        key: "schedVer".to_owned(),
        value: b"2".to_vec(),
        ..ConfigEntry::default()
    }];
    let builder = CollectionBuilder::create(&path, col, &config, &OpenOptions::new(&clock, &random)).unwrap();
    if card {
        let mut scope = builder.begin(StampMode::Raw, &clock).unwrap();
        scope
            .put(&Card {
                id: CardId(1),
                note_id: NoteId(1),
                deck_id: DeckId(1),
                ord: 0,
                mtime: EpochSecs(NOW_SECS),
                usn: Usn(0),
                card_type: CardType::New,
                queue: CardQueue::New,
                due: 1,
                interval: 0,
                ease_factor: 0,
                reps: 0,
                lapses: 0,
                remaining_steps: 0,
                original_due: 0,
                original_deck_id: DeckId(0),
                flags: 0,
                data: CardData::default(),
            })
            .unwrap();
        scope.commit().unwrap();
    }
    builder.finish().unwrap()
}

fn synced_col() -> Col {
    Col {
        version: Col::SCHEMA_VERSION,
        created: EpochSecs(NOW_SECS - 86_400),
        modified: local().modified,
        schema_modified: local().schema_modified,
        last_sync: local().last_sync,
        usn: local().usn,
        ..Col::default()
    }
}

#[test]
fn ae63_a_server_clock_360_s_behind_stops_the_check_and_changes_nothing() {
    // M2 step 3 and AE63: clock_incorrect with the offset; the job sends no
    // other request (the server's content cannot change) and the collection
    // file is as an open and close alone leaves it.
    let dir = tempfile::tempdir().unwrap();
    let path = collection(dir.path(), &synced_col(), true);
    let random = SeededRandom::new(7);
    let fixed = FixedClock::new(NOW_MS, UtcOffset::UTC);
    let opened = open_collection(&path, &OpenOptions::new(&fixed, &random)).unwrap();
    let before = LocalRecord::read(opened.writer()).unwrap();
    opened.close().unwrap();
    let untouched = std::fs::read(&path).unwrap();

    let opened = open_collection(&path, &OpenOptions::new(&fixed, &random)).unwrap();
    let record = LocalRecord::read(opened.writer()).unwrap();
    assert_eq!(record, before);
    assert_eq!(record.offline_status(), OfflineStatus::NothingPending);
    let stub = meta_stub(&Meta {
        ts: NOW_SECS - 360,
        ..server()
    });
    assert_kind(
        check(&stub, &record, &clock()),
        ErrorKind::ClockIncorrect { offset_secs: -360 },
    );
    let mut memory = StatusMemory::new();
    let mut status_job = job(&options(&stub.url()));
    let status = block_on(memory.status(&mut status_job, &record, &clock()));
    assert_kind(status, ErrorKind::ClockIncorrect { offset_secs: -360 });
    assert_eq!(LocalRecord::read(opened.writer()).unwrap(), before);
    opened.close().unwrap();

    assert_eq!(std::fs::read(&path).unwrap(), untouched);
    let paths: Vec<String> = stub.requests().iter().map(|r| r.head.path().to_owned()).collect();
    assert_eq!(paths, ["/sync/meta", "/sync/meta"]);
}

#[test]
fn exactly_300_s_either_way_passes_and_301_does_not() {
    // M2's table.
    assert_eq!(MAX_CLOCK_OFFSET_SECS, 300);
    for offset in [-300, 0, 300] {
        let stub = meta_stub(&Meta {
            ts: NOW_SECS + offset,
            ..server()
        });
        check(&stub, &local(), &clock()).unwrap_or_else(|err| panic!("{offset}: {err}"));
    }
    for offset in [-301, 301, -360] {
        let stub = meta_stub(&Meta {
            ts: NOW_SECS + offset,
            ..server()
        });
        assert_kind(
            check(&stub, &local(), &clock()),
            ErrorKind::ClockIncorrect { offset_secs: offset },
        );
    }
    // The device's whole second counts: 999 ms into it changes nothing.
    let late = clock();
    late.step_ms(999);
    let stub = meta_stub(&Meta {
        ts: NOW_SECS + 300,
        ..server()
    });
    check(&stub, &local(), &late).unwrap();
}

#[test]
fn cont_false_is_the_servers_message_and_nothing_else_is_checked_or_sent() {
    // M2 step 2 (R1514): the text exactly as received, possibly empty; the
    // clock is not checked after it.
    for msg in ["Please update your client.", "", "  <b>spaced</b>\n"] {
        let stub = meta_stub(&Meta {
            cont: false,
            msg,
            ts: NOW_SECS - 9_999,
            ..server()
        });
        assert_kind(
            check(&stub, &local(), &clock()),
            ErrorKind::ServerMessage { text: msg.to_owned() },
        );
        assert_eq!(stub.requests().len(), 1);
    }
}

#[test]
fn a_non_empty_msg_is_returned_verbatim_with_the_result() {
    // M4 (R1516): never sanitized or trimmed by the core.
    let stub = meta_stub(&Meta {
        msg: " <i>Server maintenance</i> at 3:00 ",
        ..server()
    });
    let outcome = check(&stub, &local(), &clock()).unwrap();
    assert_eq!(
        outcome.server_message.as_deref(),
        Some(" <i>Server maintenance</i> at 3:00 ")
    );
}

#[test]
fn an_answer_that_does_not_decode_is_bad_response() {
    // M8.
    for body in [r#"{"cont":true}"#, "not json", r#"{"mod":"1"}"#] {
        let body = body.to_owned();
        let stub = SyncStub::serve(move |_| Reply::ok(body.as_bytes()));
        assert_kind(check(&stub, &local(), &clock()), ErrorKind::BadResponse);
    }
}

#[test]
fn a_followed_redirect_comes_back_as_the_new_endpoint() {
    // M4: newEndpoint (transport.md T7: a self-hosted server may change its
    // port).
    let moved = meta_stub(&server());
    let target = moved.url();
    let first = SyncStub::serve(move |_| Reply::redirect(308, Some(target.as_str())));
    let outcome = check(&first, &local(), &clock()).unwrap();
    assert_eq!(
        outcome.new_endpoint.as_ref().map(Endpoint::as_str),
        Some(base(&moved.url()).as_str())
    );
    assert_eq!(moved.requests().len(), 1);
}

// --- M5, M6: the sync status -------------------------------------------------------

#[test]
fn the_offline_status_follows_m5s_table() {
    let at = |modified: i64, schema: i64, last_sync: i64| LocalRecord {
        modified: EpochMillis(modified),
        schema_modified: EpochMillis(schema),
        usn: Usn(1),
        last_sync: EpochMillis(last_sync),
        has_cards: true,
    };
    // `ls` 0 comes first, even with `scm` and `mod` past it.
    assert_eq!(at(10, 10, 0).offline_status(), OfflineStatus::Unknown);
    assert_eq!(at(5, 11, 10).offline_status(), OfflineStatus::OneWayNeeded);
    assert_eq!(at(12, 11, 10).offline_status(), OfflineStatus::OneWayNeeded);
    assert_eq!(at(11, 10, 10).offline_status(), OfflineStatus::PendingChanges);
    assert_eq!(at(10, 10, 10).offline_status(), OfflineStatus::NothingPending);
    assert_eq!(at(9, 8, 10).offline_status(), OfflineStatus::NothingPending);
}

/// A stub that counts its requests and answers `meta`.
fn counting(meta: Meta) -> (SyncStub, Arc<Mutex<usize>>) {
    let count = Arc::new(Mutex::new(0));
    let seen = Arc::clone(&count);
    let body = meta.json();
    let stub = SyncStub::serve(move |_| {
        *seen.lock().unwrap() += 1;
        Reply::ok(body.as_bytes())
    });
    (stub, count)
}

fn status_of(memory: &mut StatusMemory, url: &str, key: &str, local: &LocalRecord, clock: &StepClock) -> SyncStatus {
    let mut job = job_with_key(&options(url), key);
    block_on(memory.status(&mut job, local, clock)).unwrap().status
}

#[test]
fn local_stamps_answer_without_a_request() {
    // M6 step 1: one-way needed and local changes pending are the status;
    // the job is never sent. A port nothing listens on proves it.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    };
    let mut memory = StatusMemory::new();
    let pending = LocalRecord {
        modified: EpochMillis(NOW_MS),
        ..local()
    };
    assert_eq!(
        status_of(&mut memory, &closed, HOST_KEY, &pending, &clock()),
        SyncStatus::NormalSync
    );
    let schema = LocalRecord {
        schema_modified: EpochMillis(NOW_MS),
        ..pending
    };
    assert_eq!(
        status_of(&mut memory, &closed, HOST_KEY, &schema, &clock()),
        SyncStatus::FullSync
    );
}

#[test]
fn the_server_answers_when_the_stamps_cannot_and_its_answer_maps_to_a_status() {
    // M6 step 3: unknown (never synced) and nothing pending ask the server.
    for (meta, local, expected) in [
        (server(), local(), SyncStatus::NormalSync),
        (
            Meta {
                modified: local().modified.0,
                ..server()
            },
            local(),
            SyncStatus::NoChanges,
        ),
        (
            Meta {
                schema: NOW_MS,
                ..server()
            },
            local(),
            SyncStatus::FullSync,
        ),
        (
            Meta {
                modified: 0,
                ..server()
            },
            LocalRecord {
                modified: EpochMillis(0),
                last_sync: EpochMillis(0),
                ..local()
            },
            SyncStatus::NoChanges,
        ),
    ] {
        let (stub, count) = counting(meta);
        let mut memory = StatusMemory::new();
        assert_eq!(
            status_of(&mut memory, &stub.url(), HOST_KEY, &local, &clock()),
            expected
        );
        assert_eq!(*count.lock().unwrap(), 1);
    }
}

#[test]
fn a_remembered_answer_serves_299_s_and_not_300_s() {
    // M6.1 (oracle, sync/meta/status): stored at second T, it serves T + 299
    // and not T + 300.
    let (stub, count) = counting(server());
    let clock = clock();
    clock.step_ms(400); // inside the second T
    let mut memory = StatusMemory::new();
    let url = stub.url();
    assert_eq!(
        status_of(&mut memory, &url, HOST_KEY, &local(), &clock),
        SyncStatus::NormalSync
    );
    assert_eq!(*count.lock().unwrap(), 1);
    clock.set_now_ms(NOW_MS + 299_999);
    assert_eq!(
        status_of(&mut memory, &url, HOST_KEY, &local(), &clock),
        SyncStatus::NormalSync
    );
    assert_eq!(*count.lock().unwrap(), 1);
    clock.set_now_ms(NOW_MS + 300_000);
    assert_eq!(
        status_of(&mut memory, &url, HOST_KEY, &local(), &clock),
        SyncStatus::NormalSync
    );
    assert_eq!(*count.lock().unwrap(), 2);
}

#[test]
fn a_remembered_answer_belongs_to_its_base_and_host_key() {
    // M6.1: another base or key asks again and replaces it.
    let (stub, count) = counting(server());
    let (other, other_count) = counting(server());
    let clock = clock();
    let mut memory = StatusMemory::new();
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    assert_eq!(*count.lock().unwrap(), 1);
    status_of(&mut memory, &stub.url(), "another-key", &local(), &clock);
    assert_eq!(*count.lock().unwrap(), 2);
    status_of(&mut memory, &stub.url(), "another-key", &local(), &clock);
    assert_eq!(*count.lock().unwrap(), 2);
    status_of(&mut memory, &other.url(), "another-key", &local(), &clock);
    assert_eq!(*other_count.lock().unwrap(), 1);
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    assert_eq!(*count.lock().unwrap(), 3);
}

#[test]
fn an_answer_stored_in_the_future_has_expired() {
    // M6.1: the clock moved back.
    let (stub, count) = counting(server());
    let clock = clock();
    let mut memory = StatusMemory::new();
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    clock.step_ms(-1_000);
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    assert_eq!(*count.lock().unwrap(), 2);
}

#[test]
fn a_failed_check_leaves_the_remembered_answer_as_it_was() {
    // M6 step 3.
    let (stub, count) = counting(server());
    let failing = SyncStub::serve(|_| Reply::status(500));
    let clock = clock();
    let mut memory = StatusMemory::new();
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    let mut job = job(&options(&failing.url()));
    let err = block_on(memory.status(&mut job, &local(), &clock)).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ServerError { code: 500 });
    status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock);
    assert_eq!(*count.lock().unwrap(), 1);
}

#[test]
fn a_sync_jobs_outcome_is_remembered_and_forget_clears_it() {
    // M6.1: right after a one-way download the status is "no changes"
    // without a request, although the server's `mod` differs.
    let (stub, count) = counting(server());
    let clock = clock();
    let mut memory = StatusMemory::new();
    let key = meguri_sync::HostKey::new(HOST_KEY);
    memory.remember(&base(&stub.url()), Some(&key), SyncStatus::NoChanges, &clock);
    assert_eq!(
        status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock),
        SyncStatus::NoChanges
    );
    assert_eq!(*count.lock().unwrap(), 0);
    memory.forget();
    assert_eq!(
        status_of(&mut memory, &stub.url(), HOST_KEY, &local(), &clock),
        SyncStatus::NormalSync
    );
    assert_eq!(*count.lock().unwrap(), 1);
}

#[test]
fn a_local_edit_turns_the_status_to_pending_changes_without_a_request() {
    // The plan's scenario on a real collection: synced, a status remembered,
    // then an edit moves `col.mod` past `ls`.
    let dir = tempfile::tempdir().unwrap();
    let path = collection(dir.path(), &synced_col(), true);
    let random = SeededRandom::new(7);
    let clock = clock();
    let opened = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let (stub, count) = counting(Meta {
        modified: local().modified.0,
        ..server()
    });
    let mut memory = StatusMemory::new();
    let record = LocalRecord::read(opened.writer()).unwrap();
    assert_eq!(
        status_of(&mut memory, &stub.url(), HOST_KEY, &record, &clock),
        SyncStatus::NoChanges
    );
    assert_eq!(*count.lock().unwrap(), 1);

    clock.step_ms(5_000);
    let mut scope = opened.writer().begin_write(StampMode::Local, &clock, None).unwrap();
    scope
        .put(&ConfigEntry {
            key: "curDeck".to_owned(),
            value: b"1".to_vec(),
            ..ConfigEntry::default()
        })
        .unwrap();
    scope.commit().unwrap();
    let record = LocalRecord::read(opened.writer()).unwrap();
    assert_eq!(record.modified, EpochMillis(NOW_MS + 5_000));
    assert_eq!(record.offline_status(), OfflineStatus::PendingChanges);
    assert_eq!(
        status_of(&mut memory, &stub.url(), HOST_KEY, &record, &clock),
        SyncStatus::NormalSync
    );
    assert_eq!(*count.lock().unwrap(), 1);
    opened.close().unwrap();
}

#[test]
fn the_local_record_reads_the_col_row_and_whether_a_card_exists() {
    // M1.
    let dir = tempfile::tempdir().unwrap();
    let col = synced_col();
    for card in [false, true] {
        let dir = dir.path().join(card.to_string());
        std::fs::create_dir(&dir).unwrap();
        let path = collection(&dir, &col, card);
        let random = SeededRandom::new(7);
        let fixed = FixedClock::new(NOW_MS, UtcOffset::UTC);
        let opened = open_collection(&path, &OpenOptions::new(&fixed, &random)).unwrap();
        assert_eq!(
            LocalRecord::read(opened.writer()).unwrap(),
            LocalRecord {
                has_cards: card,
                ..local()
            }
        );
        opened.close().unwrap();
    }
}

// --- M7: the meta probe -------------------------------------------------------------

#[test]
fn the_probe_returns_the_servers_record_and_fails_as_the_check_does() {
    let stub = meta_stub(&server());
    let mut probing = job(&options(&stub.url()));
    let probe = block_on(status::probe(&mut probing, &clock())).unwrap();
    assert_eq!(probe.server.modified, EpochMillis(server().modified));
    assert_eq!(probe.server.schema_modified, EpochMillis(server().schema));
    assert_eq!(probe.server.usn, Usn(5));
    assert_eq!(probe.server.media_usn, 7);
    assert_eq!(probe.server.server_time, EpochSecs(NOW_SECS));
    assert_eq!(probe.new_endpoint, None);
    for (meta, kind) in [
        (
            Meta {
                cont: false,
                msg: "no",
                ..server()
            },
            ErrorKind::ServerMessage { text: "no".to_owned() },
        ),
        (
            Meta {
                ts: NOW_SECS + 301,
                ..server()
            },
            ErrorKind::ClockIncorrect { offset_secs: 301 },
        ),
    ] {
        let stub = meta_stub(&meta);
        let mut job = job(&options(&stub.url()));
        assert_kind(block_on(status::probe(&mut job, &clock())), kind);
    }
}

// --- Against the U13 rig ---------------------------------------------------------------
//
// The check, the status and the probe on the official server's own `meta`
// answers (M9 lists what only the stub can show). The actor scenarios of
// conformance/scenarios/sync/meta/ wait for Meguri's executor to sync (U117).
// With the rig up:
//   tools/mg core-test -- --run-ignored only -E 'package(meguri-sync) & test(/^rig_/)'

/// A login to the rig through a recording proxy, which shows every request
/// sent after it.
fn rig_login() -> (JobOptions, HostKey, SyncStub) {
    let rig = rig();
    let proxy = rig.recording_proxy();
    let options = rig.options().proxy(Proxy::new(proxy.url()));
    let key = login_blocking(threads(), &options, &random(), &rig.user, &rig.password).unwrap();
    (options, key, proxy)
}

fn rig_job(options: &JobOptions, key: &HostKey) -> Job {
    Job::new(options, Some(key.clone()), &random()).unwrap()
}

/// The device's clock: the server's own `ts` from a plain `meta` request
/// (the core reads no system clock, R1875), moved `ahead_ms` forward, and
/// the server's record from that answer.
fn rig_clock(options: &JobOptions, key: &HostKey, ahead_ms: i64) -> (StepClock, ServerMeta) {
    let mut job = rig_job(options, key);
    let body = block_on(async {
        let request = Payload::Json(wire::encode_meta_request());
        job.request(Method::Meta, request).await?.bytes().await
    })
    .unwrap();
    let server = wire::decode_meta(&body).unwrap();
    let clock = StepClock::new(server.server_time.0 * 1000 + ahead_ms, UtcOffset::UTC, "UTC");
    (clock, server)
}

fn rig_paths(proxy: &SyncStub) -> Vec<String> {
    proxy
        .requests()
        .iter()
        .map(|r: &Recorded| r.head.path().to_owned())
        .collect()
}

/// `hostKey`, then `meta` `count` times.
fn login_then_metas(count: usize) -> Vec<String> {
    let mut paths = vec!["/sync/hostKey".to_owned()];
    paths.extend(std::iter::repeat_n("/sync/meta".to_owned(), count));
    paths
}

#[test]
#[ignore = "needs tools/mg sync-rig up"]
fn rig_the_check_and_the_probe_read_the_servers_own_record() {
    let (options, key, proxy) = rig_login();
    let (clock, server) = rig_clock(&options, &key, 0);
    // M2: the server answers `cont` true with no message.
    assert!(server.can_continue && server.message.is_empty(), "{server:?}");
    // M7: the probe returns the same record.
    let probe = block_on(status::probe(&mut rig_job(&options, &key), &clock)).unwrap();
    assert_eq!(
        (probe.server.modified, probe.server.schema_modified, probe.server.usn),
        (server.modified, server.schema_modified, server.usn)
    );
    assert_eq!(probe.new_endpoint, None);
    // M3, from the real record: equal `mod`; a newer local `mod`; a schema
    // that moved on a device without cards (one-way: download, and upload
    // only onto an empty server).
    let same = LocalRecord {
        modified: server.modified,
        schema_modified: server.schema_modified,
        usn: server.usn,
        last_sync: server.modified,
        has_cards: true,
    };
    let newer = EpochMillis(server.modified.0 + 1);
    for (local, expected) in [
        (same, CheckResult::NoChanges),
        (
            LocalRecord {
                modified: newer,
                ..same
            },
            CheckResult::Normal { local_is_newer: true },
        ),
        (
            LocalRecord {
                modified: newer,
                schema_modified: EpochMillis(server.schema_modified.0 + 1),
                has_cards: false,
                ..same
            },
            CheckResult::OneWay {
                upload: server.empty,
                download: true,
            },
        ),
    ] {
        let outcome = block_on(status::check(&mut rig_job(&options, &key), &local, &clock)).unwrap();
        assert_eq!(outcome.result, expected, "{local:?}");
        assert_eq!(outcome.server.modified, server.modified);
        assert_eq!(outcome.local_usn, server.usn);
        assert_eq!((outcome.server_message, outcome.new_endpoint), (None, None));
    }
    assert_eq!(rig_paths(&proxy), login_then_metas(5));
}

#[test]
#[ignore = "needs tools/mg sync-rig up"]
fn rig_ae63_a_device_360_s_ahead_stops_the_check_and_nothing_changes() {
    // AE63, M2 step 3: the server's clock 360 s behind the device's. The
    // check and the status fail with clock_incorrect after `meta` alone; the
    // collection file and the server's record stay as they were.
    let (options, key, proxy) = rig_login();
    let (ahead, server) = rig_clock(&options, &key, 360_000);
    let dir = tempfile::tempdir().unwrap();
    let path = collection(dir.path(), &synced_col(), true);
    let random = SeededRandom::new(7);
    let fixed = FixedClock::new(NOW_MS, UtcOffset::UTC);
    open_collection(&path, &OpenOptions::new(&fixed, &random))
        .unwrap()
        .close()
        .unwrap();
    let untouched = std::fs::read(&path).unwrap();

    let opened = open_collection(&path, &OpenOptions::new(&fixed, &random)).unwrap();
    let record = LocalRecord::read(opened.writer()).unwrap();
    let check = block_on(status::check(&mut rig_job(&options, &key), &record, &ahead));
    let mut memory = StatusMemory::new();
    let status = block_on(memory.status(&mut rig_job(&options, &key), &record, &ahead));
    for err in [check.unwrap_err(), status.unwrap_err()] {
        assert_eq!(err.method, Some(Method::Meta), "{err}");
        let ErrorKind::ClockIncorrect { offset_secs } = err.kind else {
            panic!("{err}");
        };
        // The server's clock moves on between its answers.
        assert!((-360..=-355).contains(&offset_secs), "{err}");
    }
    opened.close().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), untouched);

    let (_, after) = rig_clock(&options, &key, 0);
    assert_eq!(
        (
            after.modified,
            after.schema_modified,
            after.usn,
            after.media_usn,
            after.empty
        ),
        (
            server.modified,
            server.schema_modified,
            server.usn,
            server.media_usn,
            server.empty
        )
    );
    assert_eq!(rig_paths(&proxy), login_then_metas(4));
}

#[test]
#[ignore = "needs tools/mg sync-rig up"]
fn rig_the_status_asks_once_then_remembers_and_answers_local_changes_offline() {
    // M6: nothing pending locally, so the server answers (no changes: equal
    // `mod`); a second status within 300 s and a status after a local change
    // send nothing.
    let (options, key, proxy) = rig_login();
    let (clock, server) = rig_clock(&options, &key, 0);
    let synced = EpochMillis(server.modified.0.max(server.schema_modified.0).max(1));
    let local = LocalRecord {
        modified: server.modified,
        schema_modified: server.schema_modified,
        usn: server.usn,
        last_sync: synced,
        has_cards: true,
    };
    assert_eq!(local.offline_status(), OfflineStatus::NothingPending);
    let mut memory = StatusMemory::new();
    let status = |memory: &mut StatusMemory, local: &LocalRecord| {
        block_on(memory.status(&mut rig_job(&options, &key), local, &clock))
            .unwrap()
            .status
    };
    assert_eq!(status(&mut memory, &local), SyncStatus::NoChanges);
    assert_eq!(rig_paths(&proxy), login_then_metas(2));
    clock.step_ms(60_000);
    assert_eq!(status(&mut memory, &local), SyncStatus::NoChanges);
    let edited = LocalRecord {
        modified: EpochMillis(synced.0 + 1),
        ..local
    };
    assert_eq!(edited.offline_status(), OfflineStatus::PendingChanges);
    assert_eq!(status(&mut memory, &edited), SyncStatus::NormalSync);
    assert_eq!(rig_paths(&proxy), login_then_metas(2));
}
