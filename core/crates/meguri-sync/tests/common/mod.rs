//! Helpers the test binaries share: the conformance tree, the sync runtime,
//! jobs against meguri-testkit's scripted stub (transport.md T13), answers
//! fetched through it, and the U13 rig.

// Each test binary uses its own part of this module.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use meguri_engine::{CancelToken, ThreadRegistry};
use meguri_model::rng::SeededRandom;
use meguri_sync::transport::runtime;
use meguri_sync::{Endpoint, HostKey, Job, JobOptions, Method, Payload, ServerKind, SyncError};
use meguri_testkit::sync_stub::{Reply, SyncStub};
use serde_json::Value;

/// The host key every test job sends.
pub const HOST_KEY: &str = "0123456789abcdef0123456789abcdef01234567";

pub fn conformance_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// A JSON file of the conformance tree.
pub fn conformance_json(relative: &str) -> Value {
    let path = conformance_dir().join(relative);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// The exchanges of a recording (sync-ops.md S9), one JSON object each.
pub fn exchanges(scenario: &str) -> Vec<Value> {
    let path = conformance_dir().join(format!("recordings/scenarios/{scenario}/exchanges.jsonl"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("{}: {err}", path.display()))
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

pub fn random() -> SeededRandom {
    SeededRandom::new(106)
}

/// The process's thread registry, as the engine's would be.
pub fn threads() -> &'static ThreadRegistry {
    static THREADS: OnceLock<ThreadRegistry> = OnceLock::new();
    THREADS.get_or_init(ThreadRegistry::new)
}

pub fn base(url: &str) -> Endpoint {
    Endpoint::parse(url).unwrap_or_else(|rule| panic!("{url}: {rule:?}"))
}

pub fn options(url: &str) -> JobOptions {
    JobOptions::new(base(url), ServerKind::SelfHosted)
}

pub fn job_with_key(options: &JobOptions, key: &str) -> Job {
    Job::new(options, Some(HostKey::new(key)), &random()).expect("the job starts")
}

pub fn job(options: &JobOptions) -> Job {
    job_with_key(options, HOST_KEY)
}

/// Runs async sync work on the sync runtime.
pub fn block_on<T>(work: impl Future<Output = T>) -> T {
    let cancel: Arc<CancelToken> = CancelToken::new();
    runtime::block_on(threads(), &cancel, async { Ok::<T, SyncError>(work.await) }).expect("the runtime runs")
}

/// `payload` as the stub serves it to one `method` request (200, zstd,
/// `anki-original-size`), fetched through a job: the bytes a decoder gets.
pub fn served(method: Method, payload: &[u8]) -> Vec<u8> {
    let reply = Reply::ok(payload);
    let stub = SyncStub::serve(move |_| reply.clone());
    let mut job = job(&options(&stub.url()));
    block_on(async {
        match job.request(method, Payload::empty()).await {
            Ok(body) => body.bytes().await,
            Err(err) => Err(err),
        }
    })
    .expect("the stub's answer arrives")
}

/// Compact JSON bytes of a value.
pub fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

/// Bytes read as JSON.
pub fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or_else(|err| panic!("{err}: {}", String::from_utf8_lossy(bytes)))
}

// --- The U13 rig (transport.md T14) -------------------------------------------
//
// Anki's own sync server, as `tools/mg sync-rig up` runs it on this Mac's
// loopback. `rig_*` tests are ignored by default; with the rig up:
//   tools/mg core-test -- --run-ignored only -E 'package(meguri-sync) & test(/^rig_/)'

/// The rig and one of its test accounts.
pub struct Rig {
    pub base: String,
    pub user: String,
    pub password: String,
}

/// Where the rig listens, from the latest `tools/mg sync-rig` result in this
/// checkout, and the password of its first test user, from the rig's env
/// files (docker/syncserver.env overrides the example, as compose reads
/// them). Inside the dev container, the Mac's loopback is the Docker host,
/// `host.docker.internal`.
pub fn rig() -> Rig {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let result = root.join("build/mg/sync-rig/latest/result.json");
    let result: Value = serde_json::from_slice(&std::fs::read(&result).expect("tools/mg sync-rig up first"))
        .expect("a tools/mg result is JSON");
    let details = &result["details"];
    assert_eq!(
        details["running"],
        Value::Bool(true),
        "the rig is down: tools/mg sync-rig up"
    );
    let endpoint = details["endpoints"]["syncserver"]
        .as_str()
        .expect("the rig reports its endpoint");
    let port: u16 = endpoint
        .trim_end_matches('/')
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .expect("the endpoint has a port");
    let user = details["users"][0]
        .as_str()
        .expect("the rig reports its test users")
        .to_owned();
    let mut password = None;
    for file in ["docker/syncserver.env.example", "docker/syncserver.env"] {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            continue;
        };
        for line in text.lines() {
            if let Some((_, account)) = line.split_once('=')
                && line.starts_with("SYNC_USER")
                && let Some((name, secret)) = account.split_once(':')
                && name == user
            {
                password = Some(secret.to_owned());
            }
        }
    }
    let host = std::net::ToSocketAddrs::to_socket_addrs(&("host.docker.internal", port))
        .ok()
        .and_then(|mut addresses| addresses.next())
        .unwrap_or_else(|| std::net::SocketAddr::from(([127, 0, 0, 1], port)));
    Rig {
        base: format!("http://{host}/"),
        user,
        password: password.expect("the rig's env files hold the user's password"),
    }
}
