//! T1 of actor scenarios in the parties executor (specs/conformance/
//! sync-ops.md S8.3, S10.3, S10.4): the replay, in which the oracle plays
//! every actor, and the mixed run, in which Meguri's executor plays the
//! Meguri actors. The oracle side is a stand-in worker
//! (tests/support/parties_worker.py) that speaks the interactive protocol,
//! plays the actors it is given with fixed outputs and checks the caller's
//! requests; the goldens below are written from those fixed outputs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use meguri_model::rng::RandomSource;
use meguri_testkit::exec::sync_guard::ANKIWEB_HOST;
use meguri_testkit::exec::{ExecError, MeguriExecutor, OpResult, OracleExecutor, Registry, Session};
use meguri_testkit::report::Report;
use meguri_testkit::scenario::{ObservationRequest, Setup, Step, SyncKind, SyncSpec};
use meguri_testkit::suite::{LiveRun, OraclePin, Outcome, Status, Suite};
use serde_json::{Value, json};

const LOCK: &str = "232216513595610b19dc4e6a419bfdb4cd449aa313320882ffc766eec17a8dd7";

/// 2030-01-15T10:00:00Z.
const START: i64 = 1_894_701_600;

fn repo_conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// A tree with the pin, no deviations, and these scenarios (id, text,
/// observations of the golden).
fn tree(scenarios: &[(&str, &str, Value)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "oracle.json",
        &format!("{{\"anki_version\": \"26.9.3\", \"lock_sha256\": \"{LOCK}\"}}\n"),
    );
    write(root, "deviations.yaml", "deviations: []\n");
    for (id, text, observations) in scenarios {
        let rel = format!("scenarios/{id}.yaml");
        write(root, &rel, text);
        let loaded = meguri_testkit::scenario::load(&root.join(&rel)).unwrap();
        let golden = json!({
            "scenario_id": id, "scenario_sha256": loaded.sha256, "tz": "UTC",
            "oracle": {"anki_version": "26.9.3", "lock_sha256": LOCK, "driver_sha256": "b".repeat(64)},
            "clock": {"start": "2030-01-15T10:00:00", "mode": "frozen"},
            "observations": observations,
        });
        write(
            root,
            &format!("goldens/{id}.json"),
            &serde_json::to_string_pretty(&golden).unwrap(),
        );
    }
    dir
}

/// The stand-in worker, logging every request to `log`, with extra
/// environment (`NAME=value`).
fn oracle(log: &Path, env: &[String]) -> OracleExecutor {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/parties_worker.py");
    let mut command = vec![
        "env".to_owned(),
        "TZ={tz}".to_owned(),
        format!("LOCK={LOCK}"),
        format!("LOG={}", log.display()),
    ];
    command.extend(env.iter().cloned());
    command.extend([
        "python3".to_owned(),
        "-I".to_owned(),
        "-B".to_owned(),
        script.display().to_string(),
    ]);
    OracleExecutor::new(command, 2).expecting(OraclePin {
        anki_version: "26.9.3".to_owned(),
        lock_sha256: LOCK.to_owned(),
    })
}

/// The requests the worker received, parsed.
fn requests(log: &Path) -> Vec<Value> {
    fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn t1(suite: &Suite, oracle: &OracleExecutor, meguri: &MeguriExecutor, edge: Option<&str>) -> Report {
    let live = LiveRun {
        oracle,
        meguri,
        handoff_root: None,
        meguri_edge: edge,
    };
    suite.run_live(&[], &live, "test")
}

fn only(report: &Report) -> Outcome {
    assert_eq!(report.outcomes.len(), 1, "{}", report.render_text());
    report.outcomes[0].clone()
}

fn ok(result: &str) -> Value {
    json!({"sync": {"result": result}})
}

/// The committed two-party scenario and what the stand-in observes playing
/// it: every sync succeeds, normal syncs find no changes.
fn two_party() -> (String, Value) {
    let text = fs::read_to_string(repo_conformance().join("scenarios/smoke/sync/two-party.yaml")).unwrap();
    let converged =
        json!({"converge": {"differences": {"client_a": [], "client_c": []}, "dump": {"col": "converged"}}});
    let steps = json!([
        {}, ok("ok"), ok("no_changes"), ok("ok"), {}, {}, {"card": "oracle"},
        ok("ok"), ok("no_changes"), ok("ok"), ok("no_changes"), {"card": "oracle"}, converged,
    ]);
    (text, json!({"steps": steps}))
}

#[test]
fn the_replay_runs_every_step_on_one_worker_session_and_passes() {
    let (text, golden) = two_party();
    let dir = tree(&[("smoke/sync/two-party", &text, golden)]);
    let log = dir.path().join("requests.jsonl");
    let report = t1(
        &Suite::open(dir.path()).unwrap(),
        &oracle(&log, &[]),
        &MeguriExecutor::new(),
        None,
    );
    let outcome = only(&report);
    assert_eq!(outcome.status, Status::Pass, "{}", report.render_text());
    assert_eq!(outcome.repro, "tools/mg conformance --live smoke/sync/two-party");

    // hello, begin with no caller actors, one bare step per scenario step,
    // end: the worker played every actor (sync-ops.md S10.4).
    let cmds: Vec<Value> = requests(&log).iter().map(|r| r["cmd"].clone()).collect();
    let mut expected = vec![json!("hello"), json!("begin")];
    expected.extend(std::iter::repeat_n(json!("step"), 13));
    expected.push(json!("end"));
    assert_eq!(cmds, expected);
    let all = requests(&log);
    assert!(all[1].get("caller_actors").is_none() && all[1].get("ids").is_none());
    assert_eq!(all[1]["scenario"]["id"], "smoke/sync/two-party");
    assert!(all[2..15].iter().all(|r| *r == json!({"cmd": "step"})), "{all:?}");
}

#[test]
fn every_reported_endpoint_passes_the_guard_before_the_first_step() {
    let (text, golden) = two_party();
    let refused = format!("http://sync.{ANKIWEB_HOST}/");
    for (name, value) in [
        ("ACTORS", refused.clone()),
        ("CONVERGE", refused.clone()),
        ("UPSTREAM", format!("sync.{ANKIWEB_HOST}:443")),
        ("RECORDER", "203.0.113.9:8080".to_owned()),
    ] {
        let dir = tree(&[("smoke/sync/two-party", &text, golden.clone())]);
        let log = dir.path().join("requests.jsonl");
        let report = t1(
            &Suite::open(dir.path()).unwrap(),
            &oracle(&log, &[format!("{name}={value}")]),
            &MeguriExecutor::new(),
            None,
        );
        let outcome = only(&report);
        assert_eq!(outcome.status, Status::Error, "{name}: {}", report.render_text());
        assert!(
            outcome.message.contains("endpoint_refused") && outcome.message.contains("R1876"),
            "{name}: {}",
            outcome.message
        );
        // The session began and ended; no step ran, so nothing connected.
        let cmds: Vec<Value> = requests(&log).iter().map(|r| r["cmd"].clone()).collect();
        assert_eq!(cmds, [json!("hello"), json!("begin"), json!("end")], "{name}");
    }
}

#[test]
fn a_begin_that_leaves_out_an_endpoint_ends_the_session_before_the_first_step() {
    // protocol.json `sync_info` requires all four endpoints, `upstream` and
    // `recorder` as null when the run has no fault proxy or recorder. One the
    // worker leaves out would never pass the guard (sync-ops.md S8.3).
    let (text, golden) = two_party();
    for name in ["actors", "converge", "upstream", "recorder"] {
        let dir = tree(&[("smoke/sync/two-party", &text, golden.clone())]);
        let log = dir.path().join("requests.jsonl");
        let report = t1(
            &Suite::open(dir.path()).unwrap(),
            &oracle(&log, &[format!("OMIT={name}")]),
            &MeguriExecutor::new(),
            None,
        );
        let outcome = only(&report);
        assert_eq!(outcome.status, Status::Error, "{name}: {}", report.render_text());
        assert!(
            outcome.message.contains("protocol_error") && outcome.message.contains(&format!("`{name}`")),
            "{name}: {}",
            outcome.message
        );
        let cmds: Vec<Value> = requests(&log).iter().map(|r| r["cmd"].clone()).collect();
        assert_eq!(cmds, [json!("hello"), json!("begin"), json!("end")], "{name}");
    }
}

#[test]
fn a_worker_that_cannot_start_the_server_is_an_error_not_a_failure() {
    let (text, golden) = two_party();
    let dir = tree(&[("smoke/sync/two-party", &text, golden)]);
    let log = dir.path().join("requests.jsonl");
    let report = t1(
        &Suite::open(dir.path()).unwrap(),
        &oracle(&log, &["FAIL_BEGIN=sync_unavailable".to_owned()]),
        &MeguriExecutor::new(),
        None,
    );
    let outcome = only(&report);
    assert_eq!(outcome.status, Status::Error);
    assert!(outcome.message.contains("sync_unavailable"), "{}", outcome.message);
    assert!(outcome.fails_t1());
}

/// client_a and a Meguri actor: client_a logs in and adds a note, then the
/// Meguri actor logs in, adds a note, observes, and both converge.
const MIXED: &str = r#"id: sync/mixed
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
actors: [client_a, meguri]
setup:
  notes: [{notetype: Pair, fields: [sun, sol]}]
steps:
  - advance: 1m
  - sync: {kind: login}
  - add_note: {notetype: Pair, fields: [moon, luna]}
  - actor: meguri
  - sync: {kind: login}
  - add_note: {notetype: Pair, fields: [star, estrella]}
  - observe: [timing]
  - converge: {}
"#;

/// What the oracle observes playing both actors of MIXED.
fn mixed_golden() -> Value {
    json!({"steps": [
        {}, ok("ok"), {"add_note": {"note": "#n2", "cards": ["#c2"]}}, {},
        ok("ok"), {"add_note": {"note": "#n3", "cards": ["#c3"]}}, {"timing": "oracle"},
        {"converge": {"differences": {"client_a": [], "meguri": []}, "dump": {"col": "converged"}}},
    ]})
}

/// What the Meguri adapters below saw, in order.
static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn seen(line: String) {
    SEEN.lock().unwrap().push(line);
}

fn take_seen() -> Vec<String> {
    std::mem::take(&mut *SEEN.lock().unwrap())
}

/// Tests that read SEEN run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn seat_of(session: &Session<'_>) -> String {
    let seat = session.actor.as_ref().expect("a Meguri actor has a seat");
    format!("{} as {} via {:?}", seat.name, seat.account.username, seat.endpoint)
}

fn sync(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Sync(spec) = step else { unreachable!() };
    seen(format!(
        "{} {} at {}",
        spec.kind.as_str(),
        seat_of(session),
        session.clock.now_secs()
    ));
    let result = if spec.kind == SyncKind::Normal {
        "no_changes"
    } else {
        "ok"
    };
    Ok(OpResult::Output(json!({"result": result})))
}

/// Meguri getting a sync result wrong.
fn sync_wrongly(_: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    Ok(OpResult::Output(json!({"result": "full_sync"})))
}

fn add_note(session: &mut Session<'_>, _: &Step) -> Result<OpResult, ExecError> {
    seen(format!(
        "add_note after notes {:?} cards {:?}",
        session.notes, session.cards
    ));
    session.notes.push(9_000 + session.notes.len() as i64 + 1);
    session.cards.push(9_500 + session.cards.len() as i64 + 1);
    Ok(OpResult::Output(json!({
        "note": format!("#n{}", session.notes.len()),
        "cards": [format!("#c{}", session.cards.len())],
    })))
}

fn timing(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    seen(format!("timing at {}", session.clock.now_secs()));
    Ok(json!("oracle"))
}

fn setup_notes(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    for _ in setup.notes.iter().flatten() {
        session.notes.push(5_001);
        session.cards.push(6_001);
    }
    seen(format!("setup on {}", session.random.next_u64()));
    Ok(())
}

/// A stand-in for the new collection each Meguri actor starts from
/// (sync-ops.md S2.2, dsl.md D8).
fn new_collection(session: &mut Session<'_>) -> Result<(), ExecError> {
    seen(format!("new collection at {}", session.clock.now_secs()));
    Ok(())
}

fn dump(session: &mut Session<'_>) -> Result<Value, ExecError> {
    seen(format!("dump {}", seat_of(session)));
    Ok(json!({"col": "converged"}))
}

fn dump_diverged(_: &mut Session<'_>) -> Result<Value, ExecError> {
    Ok(json!({"col": "diverged"}))
}

/// Meguri's executor with the adapters MIXED's Meguri actor needs, stand-ins
/// for the storage area's among them.
fn meguri_actor(sync_op: meguri_testkit::exec::OpFn, raw_dump: meguri_testkit::exec::RawDumpFn) -> MeguriExecutor {
    let mut registry: Registry = MeguriExecutor::area_registry(&["clock", "text"]);
    registry.actor(raw_dump);
    registry.new_collection(new_collection);
    registry.op_rejecting("sync", &SyncSpec::REJECTIONS, sync_op);
    registry.op("add_note", add_note);
    registry.observation("timing", timing);
    registry.observation("db_dump", |_, _| Ok(json!({})));
    registry.setup("notes", setup_notes);
    MeguriExecutor::with(registry)
}

#[test]
fn a_mixed_run_plays_the_meguri_actor_on_meguris_executor() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    take_seen();
    let dir = tree(&[("sync/mixed", MIXED, mixed_golden())]);
    let log = dir.path().join("requests.jsonl");
    let meguri = meguri_actor(sync, dump);
    let edge = "http://127.0.0.1:41234/";
    let report = t1(
        &Suite::open(dir.path()).unwrap(),
        &oracle(&log, &[]),
        &meguri,
        Some(edge),
    );
    let outcome = only(&report);
    assert_eq!(outcome.status, Status::Pass, "{}", report.render_text());

    let all = requests(&log);
    let begins: Vec<&Value> = all.iter().filter(|r| r["cmd"] == "begin").collect();
    assert_eq!(begins.len(), 2, "the replay, then the mixed run");
    assert!(begins[0].get("caller_actors").is_none());
    assert_eq!(begins[1]["caller_actors"], json!(["meguri"]));
    // client_a runs the setup, so the caller hands over no ids.
    assert!(begins[1].get("ids").is_none());
    let mixed: Vec<&Value> = all
        .iter()
        .skip_while(|r| *r != begins[1])
        .filter(|r| r["cmd"] == "step")
        .collect();
    assert_eq!(mixed.len(), 8);
    // Global steps and client_a's steps run on the worker.
    for i in [0, 1, 2, 3] {
        assert_eq!(*mixed[i], json!({"cmd": "step"}), "step {i}");
    }
    // The Meguri actor's steps come done, with the ids they created.
    assert_eq!(mixed[4]["done"], json!({"output": ok("ok"), "notes": [], "cards": []}));
    assert_eq!(
        mixed[5]["done"],
        json!({"output": {"add_note": {"note": "#n3", "cards": ["#c3"]}}, "notes": [9003], "cards": [9503]})
    );
    assert_eq!(mixed[6]["done"]["output"], json!({"timing": "oracle"}));
    // converge carries the Meguri actor's raw dump.
    assert_eq!(
        *mixed[7],
        json!({"cmd": "step", "dumps": {"meguri": {"col": "converged"}}})
    );

    // Meguri's actor started from a new collection at clock.start, logged
    // in at its seat (the reported account, the edge), on the clock the
    // global advance moved, and added its note after client_a's, with the
    // ids of the setup and of client_a's note.
    assert_eq!(
        take_seen(),
        [
            format!("new collection at {START}"),
            format!("login meguri as user1 via Some({edge:?}) at {}", START + 60),
            "add_note after notes [1001, 1002] cards [2001, 2002]".to_owned(),
            format!("timing at {}", START + 60),
            format!("dump meguri as user1 via Some({edge:?})"),
        ]
    );
}

#[test]
fn a_meguri_first_actor_runs_the_setup_and_hands_its_ids_to_the_worker() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    take_seen();
    let text = r#"id: sync/meguri-first
clock: {tz: UTC, start: "2030-01-15T10:00:00"}
actors: [meguri, client_b]
setup:
  notes: [{notetype: Pair, fields: [sun, sol]}]
steps:
  - sync: {kind: login}
  - actor: client_b
  - add_note: {notetype: Pair, fields: [moon, luna]}
  - converge: {}
"#;
    let golden = json!({"steps": [
        ok("ok"), {}, {"add_note": {"note": "#n2", "cards": ["#c2"]}},
        {"converge": {"differences": {"meguri": [], "client_b": []}, "dump": {"col": "converged"}}},
    ]});
    let dir = tree(&[("sync/meguri-first", text, golden)]);
    let log = dir.path().join("requests.jsonl");
    let meguri = meguri_actor(sync, dump);
    let report = t1(&Suite::open(dir.path()).unwrap(), &oracle(&log, &[]), &meguri, None);
    assert_eq!(only(&report).status, Status::Pass, "{}", report.render_text());
    let begins: Vec<Value> = requests(&log).into_iter().filter(|r| r["cmd"] == "begin").collect();
    assert_eq!(begins[1]["ids"], json!({"notes": [5001], "cards": [6001]}));
    // The setup ran on the Meguri actor's new collection, seeded for that
    // actor (sync-ops.md S2.5), and the actor synced with no edge
    // configured.
    let loaded = meguri_testkit::scenario::load(&dir.path().join("scenarios/sync/meguri-first.yaml")).unwrap();
    let first = meguri_model::rng::SeededRandom::new(meguri_testkit::exec::actor_seed(&loaded, "meguri")).next_u64();
    let seen = take_seen();
    assert_eq!(seen[0], format!("new collection at {START}"));
    assert_eq!(seen[1], format!("setup on {first}"));
    assert_eq!(seen[2], format!("login meguri as user1 via None at {START}"));
}

#[test]
fn the_mixed_run_fails_where_meguri_differs_from_the_golden() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (meguri, step, pointer) in [
        (meguri_actor(sync_wrongly, dump), 4, "/steps/4/sync/result"),
        (
            meguri_actor(sync, dump_diverged),
            7,
            "/steps/7/converge/differences/meguri/0",
        ),
    ] {
        take_seen();
        let dir = tree(&[("sync/mixed", MIXED, mixed_golden())]);
        let log = dir.path().join("requests.jsonl");
        let report = t1(&Suite::open(dir.path()).unwrap(), &oracle(&log, &[]), &meguri, None);
        let outcome = only(&report);
        assert_eq!(outcome.status, Status::Fail, "{}", report.render_text());
        assert!(outcome.message.starts_with("the mixed run: "), "{}", outcome.message);
        let first = outcome.first_divergence.unwrap();
        assert_eq!((first.step, first.pointer.as_str()), (step, pointer));
    }
}

#[test]
fn a_refused_meguri_edge_stops_the_mixed_run_before_its_first_step() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    take_seen();
    let dir = tree(&[("sync/mixed", MIXED, mixed_golden())]);
    let log = dir.path().join("requests.jsonl");
    let meguri = meguri_actor(sync, dump);
    let edge = format!("https://{ANKIWEB_HOST}/");
    let report = t1(
        &Suite::open(dir.path()).unwrap(),
        &oracle(&log, &[]),
        &meguri,
        Some(&edge),
    );
    let outcome = only(&report);
    // The replay plays no Meguri actor and passes; the mixed run is refused.
    assert_eq!(outcome.status, Status::Error, "{}", report.render_text());
    assert!(
        outcome.message.starts_with("mixed run: endpoint_refused") && outcome.message.contains("meguri edge"),
        "{}",
        outcome.message
    );
    let cmds: Vec<Value> = requests(&log).iter().map(|r| r["cmd"].clone()).collect();
    assert_eq!(
        cmds.iter().filter(|c| **c == "step").count(),
        8,
        "only the replay's steps ran"
    );
    assert_eq!(cmds.last(), Some(&json!("end")));
    // Meguri's actor got its new collection and ran nothing.
    assert_eq!(take_seen(), [format!("new collection at {START}")]);
}

#[test]
fn a_meguri_actor_scenario_is_pending_in_t1_while_meguri_cannot_play_actors() {
    let dir = tree(&[("sync/mixed", MIXED, mixed_golden())]);
    let log = dir.path().join("requests.jsonl");
    let report = t1(
        &Suite::open(dir.path()).unwrap(),
        &oracle(&log, &[]),
        &MeguriExecutor::new(),
        None,
    );
    let outcome = only(&report);
    // The replay passed; Meguri lacks the actor op and the sync op.
    assert_eq!(outcome.status, Status::Pending, "{}", report.render_text());
    let missing = outcome.missing.clone().unwrap();
    assert!(
        missing.ops.contains("actor") && missing.ops.contains("sync"),
        "{missing:?}"
    );
    assert!(!outcome.fails_t1(), "an inactive pending scenario does not fail T1");
    let begins = requests(&log).iter().filter(|r| r["cmd"] == "begin").count();
    assert_eq!(begins, 1, "only the replay ran");

    // The oracle drifting from the golden fails T1 before any of that.
    let mut drifted = mixed_golden();
    drifted["steps"][1] = ok("no_changes");
    let dir = tree(&[("sync/mixed", MIXED, drifted)]);
    let log = dir.path().join("requests.jsonl");
    let report = t1(
        &Suite::open(dir.path()).unwrap(),
        &oracle(&log, &[]),
        &MeguriExecutor::new(),
        None,
    );
    let outcome = only(&report);
    assert_eq!(outcome.status, Status::Fail);
    assert!(outcome.message.contains("no longer reproduces"), "{}", outcome.message);
}
