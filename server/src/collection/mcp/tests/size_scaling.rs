// SPDX-License-Identifier: MIT
//! M7 opt-in proof: tools/mg sync-server-mcp-bench (integrator, Docker only).
//! No fixture bytes leave temporary scratch. Default tests never execute this.
//! The shared dev runner cannot promise two CPUs, 2 GiB, SSD, or cold OS caches.
//! Its JSON reports observations and gaps, never substitutes them for acceptance.
use super::*;
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};
use std::{fs, process::Command, time::{Instant, SystemTime, UNIX_EPOCH}};

const TEST: &str = "collection::mcp::tests::size_scaling::generated_sqlite_size_scaling";
const TARGET_ID: i64 = 1_700_000_000_000;
const SAMPLES: usize = 100;
const WARMUP: usize = 8;
const REPEATS: usize = 3;
const BATCH: i64 = 256;

// Keep the existing synthetic clock/admission seam. P4 uses exactly the pinned
// first-party generator, not a different random distribution or mock position.
struct SeededRuntime(i64);
impl WriteRuntime for SeededRuntime {
    fn permit_new_card(&self) -> Result<()> { Runtime(self.0).permit_new_card() }
    fn now_millis(&self) -> Result<i64> { Runtime(self.0).now_millis() }
    fn guid_random_u64(&self) -> Result<u64> { Ok(self.0 as u64) }
    fn random_position(&self, p: u32) -> Result<u32> {
        Ok(StdRng::seed_from_u64(u64::from(p)).random_range(1..p.max(1000)))
    }
}

fn creation() -> CreateInput {
    CreateInput { deck_id: "1".into(), note_type_id: "10".into(),
        fields: Fields(BTreeMap::from([
            ("Front".into(), "generated prompt a".into()),
            ("Back".into(), "generated answer".into()),
        ])), tags: None }
}
fn patch(revision: String, index: usize) -> EditInput {
    EditInput { note_id: TARGET_ID.to_string(), revision,
        fields: Some(Fields(BTreeMap::from([("Front".into(),
            if index % 2 == 0 { "generated prompt b" } else { "generated prompt a" }.into())]))),
        add_tags: None, remove_tags: None }
}
fn lookup(backend: &Backend, path: &Path, access: &Access) -> NoteResult {
    let mut found = backend.search_notes(path, access, exact_input()).unwrap();
    assert!(found.next_cursor.is_none());
    assert_eq!(found.notes.len(), 1);
    found.notes.remove(0)
}
fn exact_input() -> SearchInput {
    SearchInput { note_id: Some(TARGET_ID.to_string()), ..Default::default() }
}
fn scalar(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}
fn file_bytes(path: &Path) -> u64 { fs::metadata(path).unwrap().len() }
fn wal_frames(path: &Path, page_size: u64) -> u64 {
    let mut name = path.as_os_str().to_os_string(); name.push("-wal");
    let bytes = match fs::metadata(Path::new(&name)) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => panic!("WAL metadata: {e}"),
    };
    if bytes == 0 { return 0; }
    assert!(bytes >= 32 && (bytes - 32) % (page_size + 24) == 0);
    (bytes - 32) / (page_size + 24)
}

/// Generate ordinary schema-18 rows, not padding tables, sparse files or blobs.
/// At most 256 notes/cards and 1024 history rows per transaction. The large
/// fixture has the same schema, templates, payload sizes and insertion order.
fn fixture(target: u64) -> (tempfile::TempDir, PathBuf, Value) {
    let Fixture { _temp, path, mut db, backend, access } = Fixture::new();
    let mut preset = DeckConfigMessage::default(); preset.new_card_insert_order = 1;
    db.execute("UPDATE deck_config SET config=?1 WHERE id=1",
        [ProtoBlob::new(preset).to_bytes().into_owned()]).unwrap();
    backend.create_note(&path, &access, creation(), &SeededRuntime(TARGET_ID)).unwrap();
    db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    let stored = format!("unrelated {}\u{1f}{}\u{1f}", "q".repeat(1536), "a".repeat(1024));
    let derived = crate::collection::notes::derive_saved_values(&stored, 0);
    let mut count = 0i64;
    loop {
        let tx = db.transaction().unwrap();
        {
            let mut notes = tx.prepare("INSERT INTO notes VALUES(?1,?2,10,1700000000,0,'',?3,?4,?5,0,'')").unwrap();
            let mut cards = tx.prepare("INSERT INTO cards VALUES(?1,?1,1,0,1700000000,0,2,2,100,10,2500,4,0,0,0,0,0,'{}')").unwrap();
            let mut history = tx.prepare("INSERT INTO revlog VALUES(?1,?2,0,3,10,9,2500,1000,1)").unwrap();
            for id in 1000 + count..1000 + count + BATCH {
                notes.execute(params![id, format!("generated-{id}"), stored, derived.sort_field, derived.checksum]).unwrap();
                cards.execute([id]).unwrap();
                for review in 0..4 { history.execute(params![1_600_000_000_000i64 + id * 4 + review, id]).unwrap(); }
            }
        }
        tx.commit().unwrap(); count += BATCH;
        // A page-count query sees committed WAL pages without checkpointing.
        let logical = scalar(&db, "PRAGMA page_count") as u64 * scalar(&db, "PRAGMA page_size") as u64;
        if logical >= target { break; }
        assert!(count < 200_000, "bounded fixture generator");
    }
    // All whole-file validation and checkpoint work precedes every measurement.
    crate::collection::schema::validate(&db).unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM notes"), count + 1);
    assert_eq!(scalar(&db, "SELECT count(*) FROM cards"), count + 1);
    assert_eq!(scalar(&db, "SELECT count(*) FROM revlog"), count * 4);
    assert_eq!(scalar(&db, "SELECT count(*) FROM cards c LEFT JOIN notes n ON c.nid=n.id WHERE n.id IS NULL"), 0);
    assert_eq!(scalar(&db, "SELECT count(*) FROM revlog r LEFT JOIN cards c ON r.cid=c.id WHERE c.id IS NULL"), 0);
    crate::collection::schema::checkpoint(&db).unwrap();
    let bytes = file_bytes(&path);
    assert!((target..target + 3_000_000).contains(&bytes));
    let schema: Vec<(String, String)> = db.prepare(
        "SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name"
    ).unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
        .collect::<std::result::Result<_, _>>().unwrap();
    let info = json!({"target_bytes": target, "main_bytes": bytes,
        "unrelated_notes": count, "cards": count + 1, "review_rows": count * 4,
        "batch_notes": BATCH, "reviews_per_unrelated_card": 4,
        "schema": schema, "validated_before_measurement": true,
        "checkpointed_before_measurement": true});
    drop(db);
    (_temp, path, info)
}

#[derive(Clone, Copy)]
struct Process { read_bytes: u64, rss_kib: u64, hwm_kib: u64 }
fn field(text: &str, key: &str) -> u64 {
    text.lines().find_map(|line| line.strip_prefix(key))
        .and_then(|value| value.split_whitespace().next()).unwrap().parse().unwrap()
}
fn process() -> Process {
    // Actual kernel counters for this process, not SQLite page-count estimates.
    let io = fs::read_to_string("/proc/self/io").expect("Linux process I/O counters required");
    let status = fs::read_to_string("/proc/self/status").expect("Linux RSS/HWM required");
    Process { read_bytes: field(&io, "read_bytes:"), rss_kib: field(&status, "VmRSS:"),
        hwm_kib: field(&status, "VmHWM:") }
}
fn optional(path: &str) -> Option<String> { fs::read_to_string(path).ok().map(|s| s.trim().into()) }
fn environment(path: &Path) -> Value {
    // statfs identifies tmpfs/overlay/etc.; it cannot prove physical SSD backing.
    let name = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: name is NUL-terminated and stat points to writable statfs storage.
    assert_eq!(unsafe { libc::statfs(name.as_ptr(), stat.as_mut_ptr()) }, 0);
    // SAFETY: successful statfs initialized the structure above.
    let fs_type = unsafe { stat.assume_init() }.f_type;
    let status = fs::read_to_string("/proc/self/status").unwrap();
    json!({"named_environment": "MG shared docker:dev, release, generated schema-18",
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "kernel": optional("/proc/sys/kernel/osrelease"),
        "available_parallelism": std::thread::available_parallelism().map(|n| n.get()).ok(),
        "cpus_allowed_list": status.lines().find_map(|l| l.strip_prefix("Cpus_allowed_list:")).map(str::trim),
        "cgroup_v2_cpu_max": optional("/sys/fs/cgroup/cpu.max"),
        "cgroup_v2_memory_max": optional("/sys/fs/cgroup/memory.max"),
        "cgroup_v2_cpuset_effective": optional("/sys/fs/cgroup/cpuset.cpus.effective"),
        "process_limits": optional("/proc/self/limits"),
        "filesystem_type_magic": format!("{fs_type:x}"),
        "sqlite_version": rusqlite::version(), "profile": "release",
        "gaps": ["No dedicated CPU or memory cap imposed by this shared MG job; visible cgroup/affinity constraints are observations, parent constraints may be hidden",
            "Physical local SSD backing is not established by statfs",
            "OS cache is uncontrolled and primed by generation/validation and previous repeats; no cold-OS-cache claim",
            "Busy measures the native retained BEGIN IMMEDIATE SQLite writer seam; synthetic Authority excludes service/session admission, OAuth and transport",
            "read_bytes counts storage reads charged to the process; cache hits and tmpfs can yield zero; WAL frames count appended pages, not physical I/O"]})
}

fn sample<T>(path: &Path, page: u64, label: &str, operation: impl FnOnce() -> T) -> (T, Value) {
    let before = process(); let frames = wal_frames(path, page);
    let start = Instant::now(); let value = operation(); let elapsed = start.elapsed();
    // Counter reads, assertions, JSON, WAL inspection and payload preparation
    // are outside the API interval. read_bytes deltas still include bookkeeping.
    let after = process(); let end_frames = wal_frames(path, page);
    assert!(end_frames >= frames, "unexpected checkpoint/WAL reset within samples");
    (value, json!({"operation": label, "latency_ms": elapsed.as_secs_f64() * 1000.,
        "read_bytes": after.read_bytes.checked_sub(before.read_bytes).unwrap(),
        "wal_page_frames": end_frames - frames,
        "rss_before_kib": before.rss_kib, "rss_after_kib": after.rss_kib,
        "rss_delta_kib": after.rss_kib as i64 - before.rss_kib as i64,
        "hwm_before_kib": before.hwm_kib, "hwm_after_kib": after.hwm_kib,
        "hwm_increment_kib": after.hwm_kib.saturating_sub(before.hwm_kib)}))
}
fn percentile(values: &[f64], percent: usize) -> f64 {
    let mut values = values.to_vec(); values.sort_by(f64::total_cmp);
    values[(values.len() * percent).div_ceil(100) - 1]
}
fn summary(samples: &[Value], label: &str) -> Value {
    let selected: Vec<_> = samples.iter().filter(|v| v["operation"] == label).collect();
    let metric = |key: &str| selected.iter().map(|v| v[key].as_f64().unwrap()).collect::<Vec<_>>();
    let latency = metric("latency_ms"); let wal = metric("wal_page_frames"); let reads = metric("read_bytes");
    json!({"operation": label, "samples": selected.len(),
        "latency_ms": {"p50": percentile(&latency, 50), "p95": percentile(&latency, 95), "max": percentile(&latency, 100)},
        "wal_page_frames": {"p50": percentile(&wal, 50), "p95": percentile(&wal, 95)},
        "read_bytes": {"p50": percentile(&reads, 50), "p95": percentile(&reads, 95)},
        "rss_peak_kib": percentile(&metric("rss_after_kib"), 100),
        "hwm_peak_kib": percentile(&metric("hwm_after_kib"), 100)})
}

fn worker(path: &Path) -> Value {
    let entry = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64();
    let spawn: f64 = std::env::var("MEGURI_MCP_BENCH_SPAWN").unwrap().parse().unwrap();
    let baseline = process(); let backend = Backend::new([7; 32]); let access = Access::new();
    // Parent reports the schema page size. Do not open SQLite to learn it before
    // the first API call; that would contaminate the first-open observation.
    let page: u64 = std::env::var("MEGURI_MCP_BENCH_PAGE").unwrap().parse().unwrap();
    let input = exact_input();
    let (mut found, first_read) = sample(path, page, "first_open_exact_lookup", || backend.search_notes(path, &access, input).unwrap());
    assert!(found.next_cursor.is_none()); assert_eq!(found.notes.len(), 1);
    let first = found.notes.remove(0);
    assert_eq!(first.fields["Front"], "generated prompt a");
    let start = Instant::now();
    let db = crate::collection::schema::open_mcp(path, true).unwrap();
    let keeper_ms = start.elapsed().as_secs_f64() * 1000.;
    assert_eq!(scalar(&db, "PRAGMA synchronous"), 2);
    assert_eq!(scalar(&db, "PRAGMA wal_autocheckpoint"), 0);
    assert_eq!(scalar(&db, "PRAGMA page_size") as u64, page);
    // Retain this handle to make appended WAL frames monotonic; no checkpoint
    // or reader transaction is held during point operations.
    let mut samples = vec![first_read];
    let mut revision = first.revision;
    let mut first_create = None; let mut first_edit = None;
    let mut last_created = String::new();
    let repeat: i64 = std::env::var("MEGURI_MCP_BENCH_REPEAT").unwrap().parse().unwrap();
    for index in 0..1 + WARMUP + SAMPLES {
        let phase = if index == 0 { "first" } else if index <= WARMUP { "warmup" } else { "warm" };
        let input = exact_input();
        let (found, read) = sample(path, page, &format!("{phase}_exact_lookup"), || backend.search_notes(path, &access, input).unwrap());
        assert!(found.next_cursor.is_none()); assert_eq!(found.notes.len(), 1);
        assert_eq!(read["wal_page_frames"], 0);
        let input = patch(revision.clone(), index);
        let runtime = SeededRuntime(1_800_000_000_000 + repeat * 10_000 + index as i64);
        let (changed, edit) = sample(path, page, &format!("{phase}_one_field_edit"), ||
            backend.edit_note(path, &access, input, &runtime).unwrap());
        assert_ne!(changed.revision, revision); revision = changed.revision;
        assert!(edit["wal_page_frames"].as_u64().unwrap() > 0);
        let input = creation();
        let (created, create) = sample(path, page, &format!("{phase}_create_note_and_card"), ||
            backend.create_note(path, &access, input, &runtime).unwrap());
        assert_eq!(scalar(&db, &format!("SELECT count(*) FROM cards WHERE nid={}", created.note_id)), 1);
        assert!(create["wal_page_frames"].as_u64().unwrap() > 0);
        last_created = created.note_id;
        if index == 0 { first_create = Some(create.clone()); first_edit = Some(edit.clone()); }
        if phase == "warm" { samples.extend([read, edit, create]); }
    }
    // The native normal-sync protocol retains this exact SQLite writer kind
    // between requests. Do not replace it with a mock busy flag or retire it.
    let before = lookup(&backend, path, &access); let stamp = scalar(&db, "SELECT mod FROM col");
    db.execute_batch("BEGIN IMMEDIATE; UPDATE col SET dty=1 WHERE id=1").unwrap();
    for index in 0..SAMPLES {
        let input = patch(revision.clone(), index);
        let (error, measurement) = sample(path, page, "retained_sqlite_writer_busy", ||
            backend.edit_note(path, &access, input, &SeededRuntime(1_900_000_000_000)).unwrap_err());
        assert_eq!(error.code, "busy"); assert_eq!(error.retry_after, Some(1));
        assert!(!db.is_autocommit()); assert_eq!(measurement["wal_page_frames"], 0);
        samples.push(measurement);
    }
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(lookup(&backend, path, &access), before);
    assert_eq!(scalar(&db, "SELECT mod FROM col"), stamp);
    // After release the same revision permits a real changed, durable edit.
    let recovery = patch(revision, 1 + WARMUP + SAMPLES);
    backend.edit_note(path, &access, recovery, &SeededRuntime(1_900_000_000_000)).unwrap();
    // Text search is one bounded page, explicitly not a complete collection scan.
    let query = SearchInput { query: Some("absent-generated-needle".into()), ..Default::default() };
    let (search, text) = sample(path, page, "bounded_text_search_page", || backend.search_notes(path, &access, query).unwrap());
    assert!(search.notes.is_empty());
    let frames_before_checkpoint = wal_frames(path, page);
    let checkpoint_start = Instant::now(); crate::collection::schema::checkpoint(&db).unwrap();
    let checkpoint_ms = checkpoint_start.elapsed().as_secs_f64() * 1000.;
    drop(db);
    // Separate cold SQLite-handle reopen verifies persisted fields/card state;
    // the OS cache remains uncontrolled/warm. No byte copy or hash is timed.
    let input = exact_input();
    let (mut found, reopen) = sample(path, page, "reopen_after_checkpoint", || backend.search_notes(path, &access, input).unwrap());
    assert_eq!(found.notes.len(), 1); let reopened = found.notes.remove(0);
    assert_eq!(reopened.fields["Front"], "generated prompt a");
    let verify = crate::collection::schema::open_mcp(path, false).unwrap();
    assert_eq!(scalar(&verify, &format!("SELECT count(*) FROM cards c JOIN notes n ON c.nid=n.id WHERE n.id={last_created}")), 1);
    drop(verify);
    let labels = ["warm_exact_lookup", "warm_one_field_edit", "warm_create_note_and_card", "retained_sqlite_writer_busy"];
    let summaries: Vec<_> = labels.iter().map(|label| summary(&samples, label)).collect();
    let idle_ok = summaries[..3].iter().all(|v| v["latency_ms"]["p95"].as_f64().unwrap() <= 150.);
    let busy_ok = summaries[3]["latency_ms"]["max"].as_f64().unwrap() <= 250.;
    let end = process();
    json!({"environment": environment(path), "spawn_to_test_entry_ms": (entry-spawn)*1000.,
        "startup_clock": "wall clock, includes libtest setup; not monotonic cross-process timing",
        "keeper_open_ms": keeper_ms, "first_create": first_create, "first_edit": first_edit,
        "baseline_rss_kib": baseline.rss_kib, "baseline_hwm_kib": baseline.hwm_kib,
        "worker_hwm_kib": end.hwm_kib, "incremental_worker_hwm_kib": end.hwm_kib.saturating_sub(baseline.hwm_kib),
        "warmup_iterations": WARMUP, "summaries": summaries, "raw_samples": samples,
        "bounded_text_search": {"measurement": text, "has_next_cursor": search.next_cursor.is_some(), "complete_search": false},
        "checkpoint": {"latency_ms": checkpoint_ms, "preceding_wal_frames": frames_before_checkpoint},
        "reopen_after_checkpoint": reopen, "main_bytes_after": file_bytes(path),
        "observed_idle_p95_within_150ms": idle_ok, "observed_busy_max_within_250ms": busy_ok,
        "acceptance_claim": false})
}

#[test]
#[ignore = "opt-in generated release benchmark; tools/mg sync-server-mcp-bench"]
fn generated_sqlite_size_scaling() {
    assert!(!cfg!(debug_assertions), "benchmark requires --release");
    assert_eq!(std::env::consts::OS, "linux", "Linux process counters required");
    if let Some(path) = std::env::var_os("MEGURI_MCP_BENCH_FIXTURE") {
        let report = worker(Path::new(&path));
        fs::write(std::env::var_os("MEGURI_MCP_BENCH_WORKER_OUT").unwrap(), serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        return;
    }
    let out = PathBuf::from(std::env::var_os("MEGURI_MCP_BENCH_OUT").expect("run through named MG job"));
    let mut report = json!({"schema_version": 1, "complete": false,
        "scope": "first-party synchronous Backend APIs, synthetic Authority/seeded Runtime; no hosted services",
        "payload_bytes": {"lookup_note_id": TARGET_ID.to_string().len(),
            "creation_json": serde_json::to_vec(&creation()).unwrap().len(),
            "edit_field_value": "generated prompt a".len(), "creation_cards": 1},
        "method": "three fresh worker processes per size; fixed payloads; nearest-rank p50/p95; each API includes open/transaction/commit/close; setup, checkpoint, probes and payload construction excluded",
        "os_cache_state": "uncontrolled, primed by fixture generation/validation; never flushed",
        "sample_order": "lookup, alternating changed one-field edit, note/card creation; 1 first + 8 warmups + 100 retained warm iterations",
        "fixtures": [], "runs": []});
    for target in [20_000_000, 200_000_000] {
        let (_temp, path, info) = fixture(target);
        let page = {
            let c = crate::collection::schema::open_mcp(&path, false).unwrap(); scalar(&c, "PRAGMA page_size")
        };
        if let Some(previous) = report["fixtures"].as_array().unwrap().first() { assert_eq!(previous["schema"], info["schema"]); }
        report["fixtures"].as_array_mut().unwrap().push(info);
        for repeat in 0..REPEATS {
            // Restore exactly the measured target payload outside measurement;
            // unrelated rows and accumulated created rows remain explicit.
            let c = crate::collection::schema::open_mcp(&path, true).unwrap();
            let current = lookup(&Backend::new([7;32]), &path, &Access::new());
            if current.fields["Front"] != "generated prompt a" {
                Backend::new([7;32]).edit_note(&path, &Access::new(), patch(current.revision, 1), &SeededRuntime(1_900_000_000_001)).unwrap();
            }
            crate::collection::schema::checkpoint(&c).unwrap(); drop(c);
            let worker_out = _temp.path().join("worker.json");
            let bytes_before = file_bytes(&path);
            let start = Instant::now();
            let status = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", TEST, "--test-threads=1"])
                .env("MEGURI_MCP_BENCH_FIXTURE", &path)
                .env("MEGURI_MCP_BENCH_PAGE", page.to_string())
                .env("MEGURI_MCP_BENCH_REPEAT", repeat.to_string())
                .env("MEGURI_MCP_BENCH_WORKER_OUT", &worker_out)
                .env("MEGURI_MCP_BENCH_SPAWN", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64().to_string())
                .status().unwrap();
            assert!(status.success(), "benchmark worker failed; partial report retained");
            let mut run: Value = serde_json::from_slice(&fs::read(&worker_out).unwrap()).unwrap();
            run["target_bytes"] = json!(target); run["repeat"] = json!(repeat);
            run["main_bytes_before"] = json!(bytes_before);
            run["whole_worker_ms"] = json!(start.elapsed().as_secs_f64()*1000.);
            report["runs"].as_array_mut().unwrap().push(run);
            fs::write(&out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
    // Retain both raw repeat-level evidence and pooled, same-operation size
    // comparisons. Ratios are observations; cache hits can make I/O ratios undefined.
    let mut comparison = Vec::new();
    for label in ["warm_exact_lookup", "warm_one_field_edit", "warm_create_note_and_card", "retained_sqlite_writer_busy"] {
        let pooled = |target: u64| report["runs"].as_array().unwrap().iter()
            .filter(|run| run["target_bytes"] == target)
            .flat_map(|run| run["raw_samples"].as_array().unwrap().iter().cloned()).collect::<Vec<_>>();
        let small = summary(&pooled(20_000_000), label);
        let large = summary(&pooled(200_000_000), label);
        let ratio = |key: &str| {
            let denominator = small[key]["p95"].as_f64().unwrap();
            if denominator == 0. { None } else { Some(large[key]["p95"].as_f64().unwrap()/denominator) }
        };
        comparison.push(json!({"operation": label,
            "large_over_small_p95": {"latency_ms": ratio("latency_ms"),
                "wal_page_frames": ratio("wal_page_frames"), "read_bytes": ratio("read_bytes")},
            "small": small, "large": large}));
    }
    report["comparison"] = json!(comparison);
    report["complete"] = json!(true);
    fs::write(out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
