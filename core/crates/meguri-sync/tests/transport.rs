//! The sync transport, endpoints and login (specs/sync/transport.md,
//! specs/sync/login-endpoints.md) against meguri-testkit's scripted stub
//! (T13), and against the U13 rig (`rig_*`, ignored unless the rig is up).
//!
//! Expected values come from the specs' tables and the stub's own records,
//! never from the code under test. No AnkiWeb host appears here: the
//! AnkiWeb rules run under the `aw.test` stand-in (T12), and the guard's
//! refusals build the host from `endpoint_guard::ANKIWEB_HOST` (U19).

mod common;

use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;

use bytes::Bytes;
use futures_core::Stream;
use meguri_engine::monotonic;
use meguri_engine::{CancelToken, Progress, ProgressListener, ProgressSink, Qos, ThreadInfo, ThreadRegistry};
use meguri_model::rng::SeededRandom;
use meguri_sync::endpoint_guard::ANKIWEB_HOST;
use meguri_sync::transport::runtime;
use meguri_sync::{
    Endpoint, EndpointRule, HostKey, Job, JobOptions, Method, NetworkCause, Payload, ProtocolError, Proxy,
    RedirectRefusal, ResponseBody, SERVER_CHOICES, ServerKind, SyncError, ankiweb, client_identity, follow_redirect,
    login_blocking,
};
use meguri_testkit::sync_stub::{
    Part, ProxyRules, ReadBody, Recorded, Reply, ServerCert, SyncStub, TestRoot, compress, trickle,
};
use serde_json::{Value, json};

/// The stand-in for the AnkiWeb names (T12), the same in every test of this
/// process.
const STAND_IN_SUFFIX: &str = "aw.test";

/// The stall timeout of the stall tests.
const STALL: Duration = Duration::from_secs(2);

fn random() -> SeededRandom {
    SeededRandom::new(105)
}

/// The process's thread registry, as the engine's would be.
fn threads() -> &'static ThreadRegistry {
    static THREADS: OnceLock<ThreadRegistry> = OnceLock::new();
    THREADS.get_or_init(ThreadRegistry::new)
}

fn base(url: &str) -> Endpoint {
    Endpoint::parse(url).unwrap_or_else(|rule| panic!("{url}: {rule:?}"))
}

fn options(url: &str) -> JobOptions {
    JobOptions::new(base(url), ServerKind::SelfHosted)
}

fn new_job(options: &JobOptions) -> Job {
    Job::new(options, Some(HostKey::new("0123456789abcdef")), &random()).expect("the job starts")
}

/// Runs one request to its end on the sync runtime and returns the whole
/// body.
fn request(job: &mut Job, method: Method, payload: Payload) -> Result<Vec<u8>, SyncError> {
    let cancel = CancelToken::new();
    runtime::block_on(threads(), &cancel, async {
        job.request(method, payload).await?.bytes().await
    })
}

fn ok_meta() -> Reply {
    Reply::ok(br#"{"cont":true}"#)
}

fn paths(requests: &[Recorded]) -> Vec<&str> {
    requests.iter().map(|r| r.head.path()).collect()
}

fn session_keys(requests: &[Recorded]) -> Vec<String> {
    requests
        .iter()
        .map(|r| {
            r.sync_member("s")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

/// Seconds since the process started, for timing assertions.
fn now() -> Duration {
    monotonic::now()
}

// --- Endpoints (login-endpoints.md L1-L3) ---------------------------------

#[test]
fn server_urls_normalise_as_l1_lists() {
    for (text, expected) in [
        ("http://h:8080", Ok("http://h:8080/")),
        ("https://h/anki", Ok("https://h/anki/")),
        ("https://h/anki/", Ok("https://h/anki/")),
        ("  http://192.168.1.10:8080  ", Ok("http://192.168.1.10:8080/")),
        (
            "HTTPS://Sync.Example.ORG:443/Anki",
            Ok("https://sync.example.org/Anki/"),
        ),
        ("http://[::1]:27701", Ok("http://[::1]:27701/")),
        ("https://h/a b", Ok("https://h/a%20b/")),
        ("ftp://h", Err(EndpointRule::UnsupportedScheme)),
        ("h:8080", Err(EndpointRule::UnsupportedScheme)),
        ("192.168.1.10:8080", Err(EndpointRule::Unparseable)),
        ("http://", Err(EndpointRule::Unparseable)),
        ("http://user:pw@h/", Err(EndpointRule::Credentials)),
        ("https://h/anki?x=1", Err(EndpointRule::QueryOrFragment)),
        ("https://h/?", Err(EndpointRule::QueryOrFragment)),
        ("https://h/#top", Err(EndpointRule::QueryOrFragment)),
        ("", Err(EndpointRule::Empty)),
        ("   ", Err(EndpointRule::Empty)),
    ] {
        let got = Endpoint::parse(text);
        assert_eq!(got.as_ref().map(Endpoint::as_str).map_err(|r| *r), expected, "{text:?}");
    }
}

#[test]
fn method_urls_append_the_method_path_to_the_base() {
    // L2's table, and T1's paths.
    let plain = base("http://h:8080/");
    assert_eq!(plain.method_url(Method::Meta), "http://h:8080/sync/meta");
    assert_eq!(plain.method_url(Method::MediaBegin), "http://h:8080/msync/begin");
    let sub = base("https://h/anki");
    assert_eq!(sub.method_url(Method::Meta), "https://h/anki/sync/meta");
    assert_eq!(sub.method_url(Method::MediaBegin), "https://h/anki/msync/begin");
    for (method, path) in [
        (Method::HostKey, "sync/hostKey"),
        (Method::Start, "sync/start"),
        (Method::ApplyGraves, "sync/applyGraves"),
        (Method::ApplyChanges, "sync/applyChanges"),
        (Method::Chunk, "sync/chunk"),
        (Method::ApplyChunk, "sync/applyChunk"),
        (Method::SanityCheck2, "sync/sanityCheck2"),
        (Method::Finish, "sync/finish"),
        (Method::Abort, "sync/abort"),
        (Method::Upload, "sync/upload"),
        (Method::Download, "sync/download"),
        (Method::MediaChanges, "msync/mediaChanges"),
        (Method::UploadChanges, "msync/uploadChanges"),
        (Method::DownloadFiles, "msync/downloadFiles"),
        (Method::MediaSanity, "msync/mediaSanity"),
    ] {
        assert_eq!(plain.method_url(method), format!("http://h:8080/{path}"));
    }
}

#[test]
fn with_the_build_switch_both_server_kinds_are_offered() {
    // L3: the service first, then the self-hosted server. Without the
    // switch, a compile-time assertion keeps the list to the self-hosted
    // server, and core-lint's scan build checks that no service name is
    // compiled in.
    assert_eq!(SERVER_CHOICES, [ServerKind::AnkiWeb, ServerKind::SelfHosted]);
}

#[test]
fn the_service_default_endpoint_is_a_base() {
    // The real default parses as a base; tests then use the stand-in.
    assert!(Endpoint::parse(ankiweb::DEFAULT_ENDPOINT).is_ok());
    ankiweb::use_stand_in(STAND_IN_SUFFIX, "https://sync.aw.test/").unwrap();
    assert_eq!(ankiweb::default_endpoint().as_str(), "https://sync.aw.test/");
}

// --- Redirect rules (transport.md T7) --------------------------------------

#[test]
fn redirect_targets_follow_t7s_table() {
    ankiweb::use_stand_in(STAND_IN_SUFFIX, "https://sync.aw.test/").unwrap();
    let service = base("https://sync.aw.test/");
    let follow = |current: &Endpoint, kind, location: &str| {
        follow_redirect(current, kind, location).map(|e| e.as_str().to_owned())
    };
    let ankiweb = ServerKind::AnkiWeb;
    let own = ServerKind::SelfHosted;

    // AE70's row with the stand-in suffix.
    assert_eq!(
        follow(&service, ankiweb, "https://sync20.aw.test/"),
        Ok("https://sync20.aw.test/".to_owned())
    );
    assert_eq!(
        follow(&service, ankiweb, "https://sync20.aw.test"),
        Ok("https://sync20.aw.test/".to_owned())
    );
    assert_eq!(
        follow(&service, ankiweb, "https://aw.test/"),
        Ok("https://aw.test/".to_owned())
    );
    assert_eq!(
        follow(&service, ankiweb, "http://sync20.aw.test/"),
        Err(RedirectRefusal::LostHttps)
    );
    for elsewhere in [
        "https://sync20.example.test/",
        "https://evilaw.test/",
        "https://sync20.aw.test.example.test/",
        "https://127.0.0.1/",
        "https://[::1]/",
    ] {
        assert_eq!(
            follow(&service, ankiweb, elsewhere),
            Err(RedirectRefusal::Host),
            "{elsewhere}"
        );
    }

    // Self-hosted rows: same host, any port, http may become https.
    assert_eq!(
        follow(&base("https://h.example/anki/"), own, "https://h.example:8443/v2"),
        Ok("https://h.example:8443/v2/".to_owned())
    );
    assert_eq!(
        follow(&base("https://h.example/"), own, "https://other.example/"),
        Err(RedirectRefusal::Host)
    );
    assert_eq!(
        follow(&base("http://192.168.1.5:8080/"), own, "https://192.168.1.5:8443/"),
        Ok("https://192.168.1.5:8443/".to_owned())
    );
    assert_eq!(
        follow(&base("https://h.example/"), own, "HTTPS://H.Example/v3"),
        Ok("https://h.example/v3/".to_owned())
    );
    assert_eq!(
        follow(&base("http://127.0.0.1:8080/"), own, "http://127.1:9090/"),
        Ok("http://127.0.0.1:9090/".to_owned()),
        "addresses compare as addresses"
    );
    assert_eq!(
        follow(&base("https://h.example/"), own, "/v2/"),
        Err(RedirectRefusal::Target(EndpointRule::Unparseable))
    );
    assert_eq!(
        follow(&base("https://h.example/"), own, "https://u:p@h.example/"),
        Err(RedirectRefusal::Target(EndpointRule::Credentials))
    );
    assert_eq!(
        follow(&base("https://h.example/"), own, "https://h.example/?a=1"),
        Err(RedirectRefusal::Target(EndpointRule::QueryOrFragment))
    );
}

// --- Requests and the header (T1-T5) ----------------------------------------

#[test]
fn every_request_carries_the_sync_header_and_a_compressed_body() {
    let stub = SyncStub::serve(|_| Reply::ok(b"null"));
    let options = options(&stub.url());
    // One source for the process, seeded as a test seeds it (T4): each job
    // draws fresh values from it.
    let random = random();
    let key = || Some(HostKey::new("0123456789abcdef"));
    let mut first = Job::new(&options, key(), &random).unwrap();
    assert_eq!(
        request(&mut first, Method::Meta, Payload::Json(br#"{"v":11}"#.to_vec())).unwrap(),
        b"null"
    );
    assert_eq!(request(&mut first, Method::Finish, Payload::empty()).unwrap(), b"null");
    let mut second = Job::new(&options, key(), &random).unwrap();
    assert_eq!(request(&mut second, Method::Abort, Payload::empty()).unwrap(), b"null");

    let requests = stub.requests();
    assert_eq!(paths(&requests), ["/sync/meta", "/sync/finish", "/sync/abort"]);
    for recorded in &requests {
        let head = &recorded.head;
        assert_eq!(head.method, "POST");
        assert_eq!(head.header("content-type"), Some("application/octet-stream"));
        for absent in ["accept-encoding", "authorization", "cookie"] {
            assert_eq!(head.header(absent), None, "{absent}");
        }
        // T2: exactly v, k, c, s, in that order, compact.
        let raw = head.header("anki-sync").unwrap();
        let s = recorded.sync_member("s").and_then(Value::as_str).unwrap();
        assert_eq!(
            raw,
            format!(
                "{{\"v\":11,\"k\":\"0123456789abcdef\",\"c\":\"{}\",\"s\":\"{s}\"}}",
                client_identity()
            )
        );
        assert_eq!(s.len(), 8);
        assert!(s.bytes().all(|b| b.is_ascii_alphanumeric()), "{s}");
        // T5: a standard zstd stream, sent chunked.
        assert!(recorded.chunked);
        assert!(recorded.complete);
    }
    assert_eq!(requests[0].body.as_deref(), Some(&br#"{"v":11}"#[..]));
    // Methods without arguments send `{}` (R1507).
    assert_eq!(requests[1].body.as_deref(), Some(&b"{}"[..]));
    assert_eq!(requests[2].body.as_deref(), Some(&b"{}"[..]));
    // T4: one key per job.
    let keys = session_keys(&requests);
    assert_eq!(keys[0], keys[1]);
    assert_ne!(keys[0], keys[2]);
}

#[test]
fn the_header_writes_every_character_outside_printable_ascii_as_an_escape() {
    let stub = SyncStub::serve(|_| Reply::ok(b"null"));
    let options = options(&stub.url());
    let key = "k\u{e9}y\"\\\u{1f600}\u{7f}";
    let mut job = Job::new(&options, Some(HostKey::new(key)), &random()).unwrap();
    request(&mut job, Method::Start, Payload::empty()).unwrap();
    let recorded = &stub.requests()[0];
    let raw = recorded.head.header("anki-sync").unwrap();
    assert!(raw.bytes().all(|b| (0x20..0x7f).contains(&b)), "{raw}");
    assert!(raw.contains(r#""k":"k\u00e9y\"\\\ud83d\ude00\u007f""#), "{raw}");
    // Read back as JSON, it is the key.
    assert_eq!(recorded.sync_member("k").and_then(Value::as_str), Some(key));
}

#[test]
fn a_job_draws_its_session_key_from_its_random_source() {
    let options = options("http://127.0.0.1:9/");
    let key = |random: &SeededRandom| Job::new(&options, None, random).unwrap().session_key().to_owned();
    // T4: the process's source is seeded once; each job draws fresh values,
    // so two jobs differ and a run with the same seed repeats its keys.
    let (run, rerun) = (SeededRandom::new(1), SeededRandom::new(1));
    let keys = [key(&run), key(&run), key(&run)];
    assert_eq!(keys, [key(&rerun), key(&rerun), key(&rerun)]);
    assert!(
        keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2],
        "{keys:?}"
    );
    assert!(
        keys.iter()
            .all(|k| k.len() == 8 && k.bytes().all(|b| b.is_ascii_alphanumeric()))
    );
}

#[test]
fn a_zero_stall_timeout_is_invalid_input() {
    let options = options("http://127.0.0.1:9/").stall_timeout(Duration::ZERO);
    assert_eq!(
        Job::new(&options, None, &random()).err(),
        Some(SyncError::InvalidInput(meguri_sync::InvalidInput::StallTimeout))
    );
}

#[test]
fn the_identity_names_meguri_its_version_build_and_platform() {
    // T3: meguri,<version> (<build>),<platform>.
    let identity = client_identity();
    let rest = identity.strip_prefix("meguri,").unwrap();
    let (version, rest) = rest.split_once(" (").unwrap();
    let (build, platform) = rest.split_once("),").unwrap();
    assert!(!version.is_empty());
    assert!(
        version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    );
    assert!(
        build == "dev" || (build.len() == 8 && build.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
        "{build}"
    );
    assert!(["macos", "ios", "linux"].contains(&platform), "{platform}");
    assert!(!identity.to_ascii_lowercase().contains("anki"), "{identity}");
}

// --- Login (login-endpoints.md L5, L6) --------------------------------------

#[test]
fn login_returns_the_host_key_and_sends_an_empty_k() {
    let stub = SyncStub::serve(|_| Reply::ok(br#"{"key":"4f2c0a","other":1}"#));
    let options = options(&stub.url());
    // L5: the core writes nothing to disk. The login runs from an empty
    // directory, which stays empty (nextest gives each test its own process).
    let scratch = std::env::temp_dir().join(format!("meguri-sync-login-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    std::env::set_current_dir(&scratch).unwrap();
    let key = login_blocking(
        threads(),
        &options,
        &random(),
        "  someone@example.test \t",
        " pass word ",
    )
    .unwrap();
    assert_eq!(key, HostKey::new("4f2c0a"));
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    std::fs::remove_dir(&scratch).unwrap();
    let requests = stub.requests();
    assert_eq!(paths(&requests), ["/sync/hostKey"]);
    assert_eq!(requests[0].sync_member("k"), Some(&json!("")));
    assert_eq!(requests[0].sync_member("c"), Some(&json!(client_identity())));
    // L5: the trimmed user name, the password as given, u then p.
    assert_eq!(
        requests[0].body.as_deref(),
        Some(&br#"{"u":"someone@example.test","p":" pass word "}"#[..])
    );
}

#[test]
fn an_empty_user_name_or_password_sends_nothing() {
    let stub = SyncStub::serve(|_| Reply::ok(br#"{"key":"k"}"#));
    let options = options(&stub.url());
    assert_eq!(
        login_blocking(threads(), &options, &random(), " \t", "pw"),
        Err(SyncError::InvalidInput(meguri_sync::InvalidInput::UserName))
    );
    assert_eq!(
        login_blocking(threads(), &options, &random(), "someone", ""),
        Err(SyncError::InvalidInput(meguri_sync::InvalidInput::Password))
    );
    assert!(stub.requests().is_empty());
}

#[test]
fn a_403_on_login_is_an_auth_failure_and_gives_no_key() {
    // The official server's answer (T14): text `403`, no original size.
    let stub = SyncStub::serve(|_| {
        Reply::status(403)
            .with_header("content-type", "text/plain; charset=utf-8")
            .with_body(vec![Part::Bytes(b"403".to_vec())])
    });
    let options = options(&stub.url());
    let err = login_blocking(threads(), &options, &random(), "someone", "wrong").unwrap_err();
    assert_eq!(
        err,
        SyncError::Status {
            code: 403,
            reason: "403".to_owned()
        }
    );
    assert!(err.is_auth_failure());
}

#[test]
fn a_login_answer_without_a_non_empty_key_is_a_protocol_error() {
    // L5, read by wire-format.md W1.2: a repeated `key` makes the object
    // invalid, and so does nesting past 127 levels, the outermost counting.
    let deep = format!(r#"{{"key":"k","x":{}{}}}"#, "[".repeat(127), "]".repeat(127));
    for answer in [
        &br#"{"nokey":"x"}"#[..],
        &br#"{"key":""}"#[..],
        &br#"{"key":5}"#[..],
        &br#"["key"]"#[..],
        &b"not json"[..],
        &br#"{"key":"first","key":"second"}"#[..],
        deep.as_bytes(),
    ] {
        let payload = answer.to_vec();
        let stub = SyncStub::serve(move |_| Reply::ok(&payload));
        let options = options(&stub.url());
        assert_eq!(
            login_blocking(threads(), &options, &random(), "someone", "pw"),
            Err(SyncError::Protocol(ProtocolError::BadLoginAnswer)),
            "{}",
            String::from_utf8_lossy(answer)
        );
    }
    // Exactly 127 levels read.
    let deepest = format!(r#"{{"key":"k","x":{}{}}}"#, "[".repeat(126), "]".repeat(126));
    let stub = SyncStub::serve(move |_| Reply::ok(deepest.as_bytes()));
    assert_eq!(
        login_blocking(threads(), &options(&stub.url()), &random(), "someone", "pw"),
        Ok(HostKey::new("k"))
    );
}

// --- Responses (T6) ---------------------------------------------------------

#[test]
fn a_success_without_an_original_size_is_refused_before_any_data() {
    let stub = SyncStub::serve(|_| Reply::ok(br#"{"cont":true}"#).without_header("anki-original-size"));
    let mut job = new_job(&options(&stub.url()));
    let cancel = CancelToken::new();
    let result = runtime::block_on(threads(), &cancel, job.request(Method::Meta, Payload::empty()));
    assert_eq!(
        result.err(),
        Some(SyncError::Protocol(ProtocolError::MissingOriginalSize))
    );
}

#[test]
fn a_malformed_or_repeated_original_size_is_refused() {
    for bad in [
        "",
        "-1",
        "+5",
        " 5x",
        "1e3",
        "0x10",
        "123456789012345678901",
        "18446744073709551616",
    ] {
        let value = bad.to_owned();
        let stub = SyncStub::serve(move |_| Reply::ok(b"null").with_header("anki-original-size", &value));
        let mut job = new_job(&options(&stub.url()));
        assert_eq!(
            request(&mut job, Method::Meta, Payload::empty()),
            Err(SyncError::Protocol(ProtocolError::BadOriginalSize)),
            "{bad:?}"
        );
    }
    let stub = SyncStub::serve(|_| Reply::ok(b"null").add_header("anki-original-size", "4"));
    let mut job = new_job(&options(&stub.url()));
    assert_eq!(
        request(&mut job, Method::Meta, Payload::empty()),
        Err(SyncError::Protocol(ProtocolError::BadOriginalSize))
    );
}

/// Every chunk a body hands out, and how it ended.
fn drain(body: &mut ResponseBody) -> (u64, Result<(), SyncError>) {
    let cancel = CancelToken::new();
    let mut delivered = 0u64;
    let end = runtime::block_on(threads(), &cancel, async {
        while let Some(piece) = body.chunk().await? {
            delivered += piece.len() as u64;
        }
        Ok(())
    });
    (delivered, end)
}

fn response(stub: &SyncStub, method: Method) -> Result<ResponseBody, SyncError> {
    let mut job = new_job(&options(&stub.url()));
    let cancel = CancelToken::new();
    runtime::block_on(threads(), &cancel, job.request(method, Payload::empty()))
}

#[test]
fn bytes_past_the_declared_size_are_refused_and_never_handed_out() {
    let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let stub = SyncStub::serve(move |_| Reply::ok(&payload).with_header("anki-original-size", "150000"));
    let mut body = response(&stub, Method::Download).unwrap();
    assert_eq!(body.declared_size(), 150_000);
    let (delivered, end) = drain(&mut body);
    assert_eq!(end, Err(SyncError::Protocol(ProtocolError::SizeExceeded)));
    assert!(delivered <= 150_000, "{delivered}");

    // A small declared size and 64 MiB of zeros in a few compressed KiB: the
    // excess is refused at once, not decompressed into memory (R1852).
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 3).unwrap();
    let zeros = vec![0u8; 1 << 20];
    for _ in 0..64 {
        io::Write::write_all(&mut encoder, &zeros).unwrap();
    }
    let bomb = encoder.finish().unwrap();
    let stub = SyncStub::serve(move |_| {
        Reply::status(200)
            .with_header("anki-original-size", "10")
            .with_body(vec![Part::Bytes(bomb.clone())])
    });
    let mut body = response(&stub, Method::Download).unwrap();
    let started = now();
    let (delivered, end) = drain(&mut body);
    assert_eq!(end, Err(SyncError::Protocol(ProtocolError::SizeExceeded)));
    assert!(delivered <= 10, "{delivered}");
    assert!(now() - started < Duration::from_secs(2), "{:?}", now() - started);
}

#[test]
fn a_body_that_ends_early_is_size_short() {
    let stub = SyncStub::serve(|_| Reply::ok(b"0123456789").with_header("anki-original-size", "11"));
    let mut body = response(&stub, Method::Download).unwrap();
    assert_eq!(drain(&mut body).1, Err(SyncError::Protocol(ProtocolError::SizeShort)));
    // The largest size the header can declare reserves nothing by itself
    // (R1852): the ten bytes arrive, then the body is short.
    let stub = SyncStub::serve(|_| Reply::ok(b"0123456789").with_header("anki-original-size", "18446744073709551615"));
    let mut body = response(&stub, Method::Download).unwrap();
    assert_eq!(body.declared_size(), u64::MAX);
    assert_eq!(
        drain(&mut body),
        (10, Err(SyncError::Protocol(ProtocolError::SizeShort)))
    );
}

#[test]
fn only_complete_zstd_frames_decompress() {
    let frame = compress(b"0123456789");
    for (name, body) in [
        ("not zstd", b"plain text, not zstd".to_vec()),
        ("truncated", frame[..frame.len() - 3].to_vec()),
        ("trailing garbage", [frame.clone(), b"xyz".to_vec()].concat()),
    ] {
        let stub = SyncStub::serve(move |_| {
            Reply::status(200)
                .with_header("anki-original-size", "10")
                .with_body(vec![Part::Bytes(body.clone())])
        });
        let mut body = response(&stub, Method::Download).unwrap();
        assert_eq!(
            drain(&mut body).1,
            Err(SyncError::Protocol(ProtocolError::BadCompression)),
            "{name}"
        );
    }
}

#[test]
fn several_frames_and_skippable_frames_make_one_body() {
    // A skippable frame: magic 0x184D2A50 little-endian, a 4-byte length, data.
    let skippable = [&[0x50, 0x2a, 0x4d, 0x18, 3, 0, 0, 0][..], b"abc"].concat();
    let body = [compress(b"first,"), skippable, compress(b"second")].concat();
    let stub = SyncStub::serve(move |_| {
        Reply::status(200)
            .with_header("anki-original-size", "12")
            .with_body(trickle(&body, 7, Duration::ZERO))
    });
    let mut job = new_job(&options(&stub.url()));
    assert_eq!(
        request(&mut job, Method::Download, Payload::empty()).unwrap(),
        b"first,second"
    );
}

#[test]
fn a_whole_body_decodes_to_its_declared_size_whatever_that_size() {
    // T6.2: complete frames that reach the declared size are the payload.
    // The sizes sit on and around multiples of 64 KiB, where a frame can end
    // exactly as a decoded piece fills, then once as two such frames.
    let pattern = |len: usize| (0..len).map(|i| (i % 251) as u8).collect::<Vec<u8>>();
    for size in [65_535, 65_536, 65_537, 131_072, 196_608] {
        let payload = pattern(size);
        let expected = payload.clone();
        let stub = SyncStub::serve(move |_| Reply::ok(&payload));
        let mut job = new_job(&options(&stub.url()));
        let got = request(&mut job, Method::Download, Payload::empty());
        assert!(got.as_ref() == Ok(&expected), "{size}: {:?}", got.map(|b| b.len()));
    }
    let half = pattern(65_536);
    let body = [compress(&half), compress(&half)].concat();
    let stub = SyncStub::serve(move |_| {
        Reply::status(200)
            .with_header("anki-original-size", "131072")
            .with_body(vec![Part::Bytes(body.clone())])
    });
    let mut job = new_job(&options(&stub.url()));
    assert_eq!(
        request(&mut job, Method::Download, Payload::empty()).unwrap(),
        [half.clone(), half].concat()
    );
}

#[test]
fn an_empty_body_is_the_empty_payload() {
    let stub = SyncStub::serve(|_| Reply::status(200).with_header("anki-original-size", "0"));
    let mut job = new_job(&options(&stub.url()));
    assert_eq!(request(&mut job, Method::Abort, Payload::empty()).unwrap(), b"");
    let stub = SyncStub::serve(|_| Reply::status(200).with_header("anki-original-size", "5"));
    let mut job = new_job(&options(&stub.url()));
    assert_eq!(
        request(&mut job, Method::Abort, Payload::empty()),
        Err(SyncError::Protocol(ProtocolError::SizeShort))
    );
}

#[test]
fn a_content_length_body_decodes_like_a_chunked_one() {
    let stub = SyncStub::serve(|_| ok_meta().content_length());
    let mut job = new_job(&options(&stub.url()));
    assert_eq!(
        request(&mut job, Method::Meta, Payload::empty()).unwrap(),
        br#"{"cont":true}"#
    );
}

#[test]
fn failed_statuses_carry_their_code_and_the_start_of_their_body() {
    for (status, body, reason) in [
        (500, "  internal\n".to_owned(), "internal".to_owned()),
        (409, String::new(), String::new()),
        (400, "<html>bad</html>".to_owned(), "<html>bad</html>".to_owned()),
        (502, "x".repeat(20_000), "x".repeat(16 * 1024)),
    ] {
        let text = body.clone();
        let stub =
            SyncStub::serve(move |_| Reply::status(status).with_body(vec![Part::Bytes(text.clone().into_bytes())]));
        let mut job = new_job(&options(&stub.url()));
        assert_eq!(
            request(&mut job, Method::Start, Payload::empty()),
            Err(SyncError::Status { code: status, reason }),
            "{status}"
        );
    }
}

#[test]
fn a_403_and_a_407_are_classified_by_status_not_as_protocol_errors() {
    for status in [403, 407] {
        let stub = SyncStub::serve(move |_| Reply::status(status));
        let mut job = new_job(&options(&stub.url()));
        let err = request(&mut job, Method::Meta, Payload::empty()).unwrap_err();
        assert_eq!(
            err,
            SyncError::Status {
                code: status,
                reason: String::new()
            }
        );
        assert_eq!(err.is_auth_failure(), status == 403);
    }
}

// --- Redirects through the job (T7, R1510, R1511, AE70) ----------------------

#[test]
fn ae70_meta_follows_a_308_on_the_service_and_the_job_stays_there() {
    let root = TestRoot::generate();
    let moved = SyncStub::serve_tls(&root.server_cert(&["sync20.aw.test"]), |_| ok_meta());
    let target = moved.url_for("sync20.aw.test");
    let location = target.clone();
    let first = SyncStub::serve_tls(&root.server_cert(&["sync.aw.test"]), move |_| {
        Reply::redirect(308, Some(location.as_str()))
    });
    ankiweb::use_stand_in(STAND_IN_SUFFIX, &first.url_for("sync.aw.test")).unwrap();
    let options = JobOptions::new(ankiweb::default_endpoint(), ServerKind::AnkiWeb)
        .pin_host("sync.aw.test", first.addr())
        .pin_host("sync20.aw.test", moved.addr())
        .trust_root(root.der());
    let mut job = new_job(&options);
    let meta = br#"{"v":11,"cv":"x"}"#.to_vec();
    assert_eq!(
        request(&mut job, Method::Meta, Payload::Json(meta.clone())).unwrap(),
        br#"{"cont":true}"#
    );
    request(&mut job, Method::Start, Payload::empty()).unwrap();
    request(&mut job, Method::MediaBegin, Payload::empty()).unwrap();

    assert_eq!(job.new_endpoint().map(Endpoint::as_str), Some(target.as_str()));
    assert_eq!(job.base().as_str(), target);
    let before = first.requests();
    let after = moved.requests();
    assert_eq!(paths(&before), ["/sync/meta"]);
    assert_eq!(paths(&after), ["/sync/meta", "/sync/start", "/msync/begin"]);
    // The repeated meta is the same request, with the same session key.
    assert_eq!(after[0].body.as_deref(), Some(meta.as_slice()));
    let keys = session_keys(&[before, after].concat());
    assert!(keys.iter().all(|k| *k == keys[0]), "{keys:?}");
}

/// A stub that must never be reached, and a script that redirects to it.
fn untouched() -> SyncStub {
    SyncStub::serve(|_| ok_meta())
}

#[track_caller]
fn refused(result: Result<Vec<u8>, SyncError>, code: u16, refusal: RedirectRefusal) {
    match result {
        Err(SyncError::Redirect(err)) => {
            assert_eq!((err.code, err.refusal), (code, refusal), "{err}");
        }
        other => panic!("expected a refused {code} ({refusal:?}), got {other:?}"),
    }
}

#[test]
fn a_308_from_https_to_http_fails_without_a_new_endpoint() {
    let root = TestRoot::generate();
    let plain = untouched();
    let location = plain.url_for("h.test");
    let secure = SyncStub::serve_tls(&root.server_cert(&["h.test"]), move |_| {
        Reply::redirect(308, Some(location.as_str()))
    });
    let options = options(&secure.url_for("h.test"))
        .pin_host("h.test", secure.addr())
        .trust_root(root.der());
    let mut job = new_job(&options);
    refused(
        request(&mut job, Method::Meta, Payload::empty()),
        308,
        RedirectRefusal::LostHttps,
    );
    assert_eq!(job.new_endpoint(), None);
    assert!(plain.requests().is_empty());
}

#[test]
fn a_self_hosted_server_may_not_redirect_to_another_host() {
    let other = untouched();
    let location = other.url_for("localhost");
    let stub = SyncStub::serve(move |_| Reply::redirect(308, Some(location.as_str())));
    let mut job = new_job(&options(&stub.url()));
    refused(
        request(&mut job, Method::Meta, Payload::empty()),
        308,
        RedirectRefusal::Host,
    );
    assert_eq!(job.new_endpoint(), None);
    assert!(other.requests().is_empty());
}

#[test]
fn a_self_hosted_server_may_move_to_another_port() {
    let moved = SyncStub::serve(|_| ok_meta());
    let location = moved.url();
    let stub = SyncStub::serve(move |_| Reply::redirect(308, Some(location.as_str())));
    let mut job = new_job(&options(&stub.url()));
    request(&mut job, Method::Meta, Payload::empty()).unwrap();
    assert_eq!(job.new_endpoint().map(Endpoint::as_str), Some(moved.url().as_str()));
}

#[test]
fn a_redirect_refused_after_a_followed_one_leaves_no_new_endpoint() {
    // The job moved and meta was answered there; a later refused redirect
    // stops the job, and its result has no newEndpoint (T7).
    let elsewhere = untouched();
    let away = elsewhere.url();
    for (method, refusal) in [
        (Method::Start, RedirectRefusal::NotMeta),
        (Method::Meta, RedirectRefusal::Repeated),
    ] {
        let away = away.clone();
        // The target answers its first meta, then redirects everything.
        let metas = AtomicU64::new(0);
        let moved = SyncStub::serve(move |head| {
            if head.path() == "/sync/meta" && metas.fetch_add(1, Ordering::SeqCst) == 0 {
                ok_meta()
            } else {
                Reply::redirect(308, Some(away.as_str()))
            }
        });
        let target = moved.url();
        let location = target.clone();
        let stub = SyncStub::serve(move |_| Reply::redirect(308, Some(location.as_str())));
        let mut job = new_job(&options(&stub.url()));
        request(&mut job, Method::Meta, Payload::empty()).unwrap();
        assert_eq!(job.new_endpoint().map(Endpoint::as_str), Some(target.as_str()));
        refused(request(&mut job, method, Payload::empty()), 308, refusal);
        assert_eq!(moved.requests().len(), 2, "the later {method:?} went to the target");
        assert_eq!(job.new_endpoint(), None, "after a refused {method:?}");
    }
    assert!(elsewhere.requests().is_empty());
}

#[test]
fn no_redirect_is_ever_followed_automatically() {
    // Only meta, only a 308, only once (T7, R1511).
    let target = untouched();
    let to = target.url();
    for (method, status, location, refusal) in [
        (Method::Start, 308, Some(to.clone()), RedirectRefusal::NotMeta),
        (Method::HostKey, 308, Some(to.clone()), RedirectRefusal::NotMeta),
        (Method::Download, 307, Some(to.clone()), RedirectRefusal::NotMeta),
        (Method::Meta, 301, Some(to.clone()), RedirectRefusal::NotPermanent),
        (Method::Meta, 307, Some(to.clone()), RedirectRefusal::NotPermanent),
        (Method::Meta, 302, Some(to.clone()), RedirectRefusal::NotPermanent),
        (Method::Meta, 308, None, RedirectRefusal::MissingLocation),
        (
            Method::Meta,
            308,
            Some("/v2/".to_owned()),
            RedirectRefusal::Target(EndpointRule::Unparseable),
        ),
    ] {
        let location = location.clone();
        let stub = SyncStub::serve(move |_| Reply::redirect(status, location.as_deref()));
        let mut job = new_job(&options(&stub.url()));
        refused(request(&mut job, method, Payload::empty()), status, refusal);
        assert_eq!(job.new_endpoint(), None);
        let err = request(&mut job, method, Payload::empty()).unwrap_err();
        assert_eq!(err.category(), "redirect");
    }
    assert!(target.requests().is_empty());

    // A 308 on the repeated meta is refused too, and the job then has no
    // new endpoint: not the first target either (T7).
    let second = SyncStub::serve(|_| Reply::redirect(308, Some("http://127.0.0.1:9/")));
    let to_second = second.url();
    let first = SyncStub::serve(move |_| Reply::redirect(308, Some(to_second.as_str())));
    let mut job = new_job(&options(&first.url()));
    refused(
        request(&mut job, Method::Meta, Payload::empty()),
        308,
        RedirectRefusal::Repeated,
    );
    assert_eq!(paths(&second.requests()), ["/sync/meta"]);
    assert_eq!(job.new_endpoint(), None);
}

#[test]
fn a_streamed_meta_is_not_sent_twice() {
    let moved = untouched();
    let location = moved.url();
    let stub = SyncStub::serve(move |_| Reply::redirect(308, Some(location.as_str())));
    let mut job = new_job(&options(&stub.url()));
    let payload = Payload::Stream {
        source: Box::pin(Timed::new(vec![(Duration::ZERO, Bytes::from_static(b"{}"))])),
        total: Some(2),
    };
    refused(
        request(&mut job, Method::Meta, payload),
        308,
        RedirectRefusal::NotResendable,
    );
    assert!(moved.requests().is_empty());
}

// --- Stalls and cancellation (T8, T9) ---------------------------------------

/// A body source that yields each chunk after its delay.
struct Timed {
    chunks: VecDeque<(Duration, Bytes)>,
    sleep: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl Timed {
    fn new(chunks: Vec<(Duration, Bytes)>) -> Timed {
        Timed {
            chunks: chunks.into(),
            sleep: None,
        }
    }
}

impl Stream for Timed {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<io::Result<Bytes>>> {
        loop {
            if let Some(sleep) = self.sleep.as_mut() {
                if sleep.as_mut().poll(cx).is_pending() {
                    return Poll::Pending;
                }
                self.sleep = None;
                let (_, chunk) = self.chunks.pop_front().expect("a chunk waits behind its sleep");
                return Poll::Ready(Some(Ok(chunk)));
            }
            let Some((delay, _)) = self.chunks.front() else {
                return Poll::Ready(None);
            };
            self.sleep = Some(Box::pin(tokio::time::sleep(*delay)));
        }
    }
}

/// `len` pseudo-random bytes, which zstd cannot shrink.
fn incompressible(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

/// `count` incompressible chunks of `size` bytes, each after `every`.
fn noise(count: usize, size: usize, every: Duration) -> Vec<(Duration, Bytes)> {
    incompressible(count * size)
        .chunks(size)
        .map(|chunk| (every, Bytes::copy_from_slice(chunk)))
        .collect()
}

fn stalling(url: &str) -> JobOptions {
    options(url).stall_timeout(STALL)
}

#[track_caller]
fn timed_out_within(started: Duration, result: Result<Vec<u8>, SyncError>, at_least: Duration, at_most: Duration) {
    let elapsed = now() - started;
    assert_eq!(result, Err(SyncError::Timeout));
    assert!(elapsed >= at_least && elapsed < at_most, "{elapsed:?}");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let stub = SyncStub::serve(|_| ok_meta().head_delay(Duration::from_secs(5)));
    let mut job = new_job(&stalling(&stub.url()));
    let started = now();
    let result = request(&mut job, Method::Meta, Payload::empty());
    timed_out_within(started, result, STALL, STALL + Duration::from_secs(1));
}

#[test]
fn a_response_that_stops_mid_body_times_out() {
    let payload = vec![7u8; 100_000];
    let stub = SyncStub::serve(move |_| {
        let body = compress(&payload);
        let (head, tail) = body.split_at(body.len() / 2);
        Reply::ok(&payload).with_body(vec![
            Part::Bytes(head.to_vec()),
            Part::Pause(Duration::from_secs(5)),
            Part::Bytes(tail.to_vec()),
        ])
    });
    let mut job = new_job(&stalling(&stub.url()));
    let started = now();
    let result = request(&mut job, Method::Download, Payload::empty());
    timed_out_within(started, result, STALL, STALL + Duration::from_secs(1));
}

#[test]
fn a_steady_trickle_never_times_out() {
    // 10 s of bytes every 0.5 s with a 2 s stall timeout: no total timeout.
    let payload = incompressible(40_000);
    let expected = payload.clone();
    let stub = SyncStub::serve(move |_| {
        let body = compress(&payload);
        Reply::ok(&payload).with_body(trickle(&body, 20, Duration::from_millis(500)))
    });
    let mut job = new_job(&stalling(&stub.url()));
    let started = now();
    assert_eq!(request(&mut job, Method::Download, Payload::empty()).unwrap(), expected);
    assert!(now() - started >= Duration::from_secs(9));
}

#[test]
fn an_upload_that_keeps_writing_never_times_out() {
    let stub = SyncStub::serve(|_| Reply::ok(b"OK"));
    let mut job = new_job(&stalling(&stub.url()));
    let chunks = noise(20, 4096, Duration::from_millis(500));
    let sent: Vec<u8> = chunks.iter().flat_map(|(_, c)| c.to_vec()).collect();
    let payload = Payload::Stream {
        source: Box::pin(Timed::new(chunks)),
        total: Some(sent.len() as u64),
    };
    let started = now();
    assert_eq!(request(&mut job, Method::Upload, payload).unwrap(), b"OK");
    assert!(now() - started >= Duration::from_secs(9));
    let recorded = &stub.requests()[0];
    assert!(recorded.chunked);
    assert_eq!(recorded.body.as_deref(), Some(sent.as_slice()));
}

#[test]
fn an_upload_whose_source_stops_times_out() {
    let stub = SyncStub::serve(|_| Reply::ok(b"OK"));
    let mut job = new_job(&stalling(&stub.url()));
    let mut chunks = noise(2, 4096, Duration::ZERO);
    chunks[1].0 = Duration::from_secs(3);
    let payload = Payload::Stream {
        source: Box::pin(Timed::new(chunks)),
        total: None,
    };
    let started = now();
    let result = request(&mut job, Method::Upload, payload);
    timed_out_within(started, result, STALL, STALL + Duration::from_secs(1));
}

#[test]
fn an_upload_the_server_stops_reading_times_out() {
    // The stub reads 1 MiB, then nothing for 6 s; the socket buffers fill
    // and no more bytes are handed over.
    let stub = SyncStub::serve(|_| {
        Reply::ok(b"OK").read(ReadBody::StallAfter {
            bytes: 1 << 20,
            pause: Duration::from_secs(6),
        })
    });
    let mut job = new_job(&stalling(&stub.url()));
    let payload = Payload::Stream {
        source: Box::pin(Timed::new(noise(256, 256 * 1024, Duration::ZERO))),
        total: None,
    };
    let started = now();
    let result = request(&mut job, Method::Upload, payload);
    assert_eq!(result, Err(SyncError::Timeout));
    assert!(now() - started < Duration::from_secs(6), "{:?}", now() - started);
}

#[test]
fn a_server_that_reads_everything_then_stays_silent_times_out() {
    let stub = SyncStub::serve(|_| Reply::ok(b"OK").head_delay(Duration::from_secs(3)));
    let mut job = new_job(&stalling(&stub.url()));
    let payload = Payload::Stream {
        source: Box::pin(Timed::new(noise(4, 64 * 1024, Duration::ZERO))),
        total: None,
    };
    let started = now();
    let result = request(&mut job, Method::Upload, payload);
    timed_out_within(started, result, STALL, STALL + Duration::from_secs(1));
    assert!(stub.requests()[0].complete);
}

#[test]
fn a_cancelled_token_ends_a_slow_request_within_200_ms() {
    let stub = SyncStub::serve(|_| ok_meta().head_delay(Duration::from_secs(10)));
    let options = options(&stub.url());
    let token = Arc::clone(options.cancel_token());
    let mut job = new_job(&options);
    let cancelled_at = Arc::new(AtomicU64::new(0));
    let at = Arc::clone(&cancelled_at);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        at.store(now().as_micros() as u64, Ordering::SeqCst);
        token.cancel();
    });
    let result = runtime::block_on(
        threads(),
        options.cancel_token(),
        job.request(Method::Meta, Payload::empty()),
    );
    let returned = now();
    canceller.join().unwrap();
    assert_eq!(result.err(), Some(SyncError::Cancelled));
    let after = returned - Duration::from_micros(cancelled_at.load(Ordering::SeqCst));
    assert!(after < Duration::from_millis(200), "{after:?}");
}

#[test]
fn a_cancelled_token_also_ends_a_body_being_read() {
    let payload = vec![1u8; 10_000];
    let stub = SyncStub::serve(move |_| {
        let body = compress(&payload);
        Reply::ok(&payload).with_body(vec![Part::Pause(Duration::from_secs(10)), Part::Bytes(body)])
    });
    let options = options(&stub.url());
    let token = Arc::clone(options.cancel_token());
    let mut job = new_job(&options);
    let mut body = runtime::block_on(
        threads(),
        &CancelToken::new(),
        job.request(Method::Download, Payload::empty()),
    )
    .unwrap();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        token.cancel();
    });
    let started = now();
    assert_eq!(drain(&mut body).1, Err(SyncError::Cancelled));
    assert!(now() - started < Duration::from_millis(800));
    canceller.join().unwrap();
}

#[test]
fn the_next_job_starts_a_new_runtime_after_a_shut_down() {
    // KTD14, C8: the app may drop the runtime while no job runs.
    let stub = SyncStub::serve(|_| ok_meta());
    request(&mut new_job(&options(&stub.url())), Method::Meta, Payload::empty()).unwrap();
    runtime::shut_down();
    request(&mut new_job(&options(&stub.url())), Method::Meta, Payload::empty()).unwrap();
    assert_eq!(stub.requests().len(), 2);
}

/// Polls `done` until it holds, for at most 5 s.
#[track_caller]
fn wait_until(what: &str, done: impl Fn() -> bool) {
    let deadline = now() + Duration::from_secs(5);
    while !done() {
        assert!(now() < deadline, "{what}: not within 5 s");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_runtime_threads_are_listed_in_the_thread_registry_until_shut_down() {
    // engine/jobs J6: every thread the core creates is listed, at its QoS
    // (user-initiated: the user started the sync, J2), busy only while it
    // works. The runtime has two workers (KTD14).
    let sync_threads = || -> Vec<ThreadInfo> {
        threads()
            .threads()
            .into_iter()
            .filter(|t| t.name == "meguri-sync")
            .collect()
    };
    assert!(sync_threads().is_empty());
    let stub = SyncStub::serve(|_| ok_meta());
    let mut job = new_job(&options(&stub.url()));
    let busy_in_a_task = runtime::block_on(threads(), &CancelToken::new(), async {
        job.request(Method::Meta, Payload::empty()).await?.bytes().await?;
        // A task runs on a worker, which is busy while it runs.
        Ok(tokio::spawn(async { threads().busy_threads() })
            .await
            .expect("the task runs"))
    })
    .unwrap();
    assert!(
        busy_in_a_task.iter().any(|t| t.name == "meguri-sync"),
        "{busy_in_a_task:?}"
    );
    wait_until("both workers are listed", || sync_threads().len() >= 2);
    assert!(
        sync_threads().iter().all(|t| t.qos == Qos::UserInitiated),
        "{:?}",
        sync_threads()
    );
    wait_until("parked workers are idle", || sync_threads().iter().all(|t| !t.busy));
    runtime::shut_down();
    wait_until("ended threads leave the list", || sync_threads().is_empty());
}

// --- Network, TLS and proxies (T10, T11) --------------------------------------

#[test]
fn a_refused_connection_is_a_network_error_before_the_head() {
    let address = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let mut job = new_job(&options(&format!("http://{address}/")));
    match request(&mut job, Method::Meta, Payload::empty()) {
        Err(SyncError::Network(err)) => {
            assert_eq!(err.cause, NetworkCause::Connect);
            assert!(!err.head_arrived);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_connection_closed_before_or_after_the_head_is_a_network_error() {
    let stub = SyncStub::serve(|_| Reply::hang_up());
    let mut job = new_job(&options(&stub.url()));
    match request(&mut job, Method::HostKey, Payload::empty()) {
        Err(SyncError::Network(err)) => assert!(!err.head_arrived, "{err}"),
        other => panic!("{other:?}"),
    }
    let payload = incompressible(50_000);
    let stub = SyncStub::serve(move |_| {
        let body = compress(&payload);
        Reply::ok(&payload).with_body(vec![Part::Bytes(body[..100].to_vec()), Part::Close])
    });
    let mut job = new_job(&options(&stub.url()));
    match request(&mut job, Method::Download, Payload::empty()) {
        Err(SyncError::Network(err)) => assert!(err.head_arrived, "{err}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_self_signed_certificate_fails_tls() {
    // The job trusts a test root; the stub's certificate is outside it.
    let root = TestRoot::generate();
    let stub = SyncStub::serve_tls(&ServerCert::self_signed(&["self.test"]), |_| ok_meta());
    let options = options(&stub.url_for("self.test"))
        .pin_host("self.test", stub.addr())
        .trust_root(root.der());
    let err = request(&mut new_job(&options), Method::Meta, Payload::empty()).unwrap_err();
    assert_eq!(err.category(), "tls", "{err}");
}

#[test]
fn a_certificate_for_another_name_fails_tls() {
    let root = TestRoot::generate();
    let stub = SyncStub::serve_tls(&root.server_cert(&["other.test"]), |_| ok_meta());
    let options = options(&stub.url_for("self.test"))
        .pin_host("self.test", stub.addr())
        .trust_root(root.der());
    let err = request(&mut new_job(&options), Method::Meta, Payload::empty()).unwrap_err();
    assert_eq!(err.category(), "tls", "{err}");
    // The same stub under its own name works.
    let options = self::options(&stub.url_for("other.test"))
        .pin_host("other.test", stub.addr())
        .trust_root(root.der());
    request(&mut new_job(&options), Method::Meta, Payload::empty()).unwrap();
}

#[test]
fn every_request_goes_through_the_jobs_proxy() {
    let server = SyncStub::serve(|_| ok_meta());
    let proxy = SyncStub::proxy(ProxyRules {
        credentials: Some(("proxy-user".to_owned(), "proxy-pass".to_owned())),
        routes: Vec::new(),
    });
    let options = options(&server.url()).proxy(Proxy::new(proxy.url()).with_credentials("proxy-user", "proxy-pass"));
    let mut job = new_job(&options);
    request(&mut job, Method::Meta, Payload::empty()).unwrap();
    request(&mut job, Method::Start, Payload::empty()).unwrap();
    let through = proxy.requests();
    assert_eq!(
        through.iter().map(|r| r.head.target.as_str()).collect::<Vec<_>>(),
        [server.url() + "sync/meta", server.url() + "sync/start"]
    );
    for recorded in &through {
        // RFC 7617 basic credentials for proxy-user:proxy-pass.
        assert_eq!(
            recorded.head.header("proxy-authorization"),
            Some("Basic cHJveHktdXNlcjpwcm94eS1wYXNz")
        );
    }
    assert_eq!(paths(&server.requests()), ["/sync/meta", "/sync/start"]);
}

#[test]
fn https_goes_through_the_proxy_in_a_connect_tunnel() {
    let root = TestRoot::generate();
    let server = SyncStub::serve_tls(&root.server_cert(&["tunnel.test"]), |_| ok_meta());
    let authority = format!("tunnel.test:{}", server.addr().port());
    let proxy = SyncStub::proxy(ProxyRules {
        credentials: None,
        routes: vec![(authority.clone(), server.addr())],
    });
    let options = options(&server.url_for("tunnel.test"))
        .proxy(Proxy::new(proxy.url()))
        .trust_root(root.der());
    request(&mut new_job(&options), Method::Meta, Payload::empty()).unwrap();
    let through = proxy.requests();
    assert_eq!(through.len(), 1);
    assert_eq!(
        (through[0].head.method.as_str(), through[0].head.target.as_str()),
        ("CONNECT", authority.as_str())
    );
    assert_eq!(paths(&server.requests()), ["/sync/meta"]);
}

#[test]
fn a_407_from_the_proxy_is_a_status_error_in_both_forms() {
    let root = TestRoot::generate();
    let plain = untouched();
    let secure = SyncStub::serve_tls(&root.server_cert(&["tunnel.test"]), |_| ok_meta());
    let proxy = SyncStub::proxy(ProxyRules {
        credentials: Some(("proxy-user".to_owned(), "proxy-pass".to_owned())),
        routes: vec![(format!("tunnel.test:{}", secure.addr().port()), secure.addr())],
    });
    // Absolute form: the 407 is the response.
    let options = options(&plain.url()).proxy(Proxy::new(proxy.url()).with_credentials("proxy-user", "wrong"));
    assert_eq!(
        request(&mut new_job(&options), Method::Meta, Payload::empty()),
        Err(SyncError::Status {
            code: 407,
            reason: String::new()
        })
    );
    // CONNECT: the 407 answers the tunnel.
    let options = self::options(&secure.url_for("tunnel.test"))
        .proxy(Proxy::new(proxy.url()))
        .trust_root(root.der());
    assert_eq!(
        request(&mut new_job(&options), Method::Meta, Payload::empty()),
        Err(SyncError::Status {
            code: 407,
            reason: String::new()
        })
    );
    assert!(plain.requests().is_empty() && secure.requests().is_empty());
}

#[test]
fn proxy_environment_variables_are_never_read() {
    // T10: only the job's proxy. A proxy named by the environment sees
    // nothing, with or without a job proxy.
    let ambient = SyncStub::proxy(ProxyRules::default());
    for name in [
        "http_proxy",
        "HTTP_PROXY",
        "https_proxy",
        "HTTPS_PROXY",
        "all_proxy",
        "ALL_PROXY",
    ] {
        // SAFETY: nextest runs each test in a process of its own, and no
        // thread of this test reads the environment while it is set (the
        // stub's threads and the runtime never do).
        unsafe { std::env::set_var(name, ambient.url()) };
    }
    let server = SyncStub::serve(|_| ok_meta());
    request(&mut new_job(&options(&server.url())), Method::Meta, Payload::empty()).unwrap();
    let chosen = SyncStub::proxy(ProxyRules::default());
    let through = options(&server.url()).proxy(Proxy::new(chosen.url()));
    request(&mut new_job(&through), Method::Meta, Payload::empty()).unwrap();
    assert!(ambient.requests().is_empty());
    assert_eq!(chosen.requests().len(), 1);
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn a_proxy_that_is_not_http_is_refused_before_anything_connects() {
    for url in [
        "socks5://127.0.0.1:1080",
        "https://127.0.0.1:3128",
        "not a url",
        "http://",
    ] {
        let options = options("http://127.0.0.1:9/").proxy(Proxy::new(url));
        match Job::new(&options, None, &random()) {
            Err(SyncError::InvalidProxy(_)) => {}
            other => panic!("{url}: {:?}", other.err()),
        }
    }
}

// --- The endpoint guard of test builds (T12, R1876) ---------------------------

#[test]
fn test_builds_refuse_every_endpoint_that_is_not_local() {
    let guarded = |result: Result<Vec<u8>, SyncError>| matches!(result, Err(SyncError::EndpointRefused(_)));
    for url in [format!("https://sync.{ANKIWEB_HOST}/"), "http://8.8.8.8/".to_owned()] {
        let mut job = new_job(&options(&url));
        assert!(guarded(request(&mut job, Method::Meta, Payload::empty())), "{url}");
    }
    // A proxy, and a pinned address, outside the local ranges.
    let options = self::options("http://127.0.0.1:9/").proxy(Proxy::new("http://8.8.8.8:3128"));
    assert!(matches!(
        Job::new(&options, None, &random()).err(),
        Some(SyncError::EndpointRefused(_))
    ));
    let options = self::options("https://far.test/").pin_host("far.test", "8.8.8.8:443".parse().unwrap());
    assert!(matches!(
        Job::new(&options, None, &random()).err(),
        Some(SyncError::EndpointRefused(_))
    ));
}

// --- Transfer progress (T15) --------------------------------------------------

static FAKE_CLOCK: AtomicU64 = AtomicU64::new(0);

/// A clock that moves a second per reading, so the sink delivers every
/// report.
fn fake_clock() -> Duration {
    Duration::from_secs(FAKE_CLOCK.fetch_add(1, Ordering::SeqCst))
}

#[derive(Default)]
struct Seen(Mutex<Vec<(u64, Option<u64>)>>);

impl ProgressListener for Seen {
    fn on_progress(&self, progress: &Progress) {
        self.0.lock().unwrap().push((progress.completed, progress.total));
    }
}

#[test]
fn transfers_report_uncompressed_bytes_against_their_totals() {
    let answer = vec![9u8; 70_000];
    let reply = answer.clone();
    let stub = SyncStub::serve(move |_| Reply::ok(&reply));
    let mut job = new_job(&options(&stub.url()));
    let sink = ProgressSink::with_clock(fake_clock);
    let seen = Arc::new(Seen::default());
    sink.set_listener(seen.clone());
    job.set_progress(sink);
    let upload = noise(3, 50_000, Duration::ZERO);
    let payload = Payload::Stream {
        source: Box::pin(Timed::new(upload)),
        total: Some(150_000),
    };
    assert_eq!(request(&mut job, Method::Upload, payload).unwrap(), answer);
    let seen = seen.0.lock().unwrap().clone();
    // The compressor took 50,000 raw bytes at a time; the response counts
    // decompressed bytes up to the declared size.
    for expected in [
        (50_000, Some(150_000)),
        (100_000, Some(150_000)),
        (150_000, Some(150_000)),
    ] {
        assert!(seen.contains(&expected), "{expected:?} in {seen:?}");
    }
    assert_eq!(seen.last(), Some(&(70_000, Some(70_000))));
}

// --- The U13 rig (T14) --------------------------------------------------------
//
// Anki's own sync server, as `tools/mg sync-rig up` runs it on this Mac's
// loopback. With the rig up:
//   tools/mg core-test -- --run-ignored only -E 'package(meguri-sync) & test(/^rig_/)'

use common::rig;

#[test]
fn local_rig_route_pins_loopback_under_a_reserved_name() {
    let server = SyncStub::serve(|_| Reply::ok(b"{}"));
    let (options, route) = common::rig_route(server.addr());
    assert!(route.is_none());
    let mut job = new_job(&options);
    assert_eq!(job.base().as_str(), server.url_for("sync-rig.test"));
    assert_eq!(request(&mut job, Method::Meta, Payload::empty()).unwrap(), b"{}");
    assert_eq!(paths(&server.requests()), ["/sync/meta"]);
}

#[test]
fn local_rig_route_keeps_recording_clients_on_the_explicit_upstream() {
    let server = SyncStub::serve(|_| Reply::ok(b"{}"));
    let (options, route) = common::rig_route(server.addr());
    assert!(route.is_none());
    let proxy = common::rig_proxy(server.addr());
    let options = options.proxy(Proxy::new(proxy.url()));
    let mut job = new_job(&options);
    assert_eq!(request(&mut job, Method::Meta, Payload::empty()).unwrap(), b"{}");
    assert_eq!(paths(&server.requests()), ["/sync/meta"]);
    let sent = proxy.requests();
    assert_eq!(paths(&sent), ["/sync/meta"]);
    assert_eq!(sent[0].head.target, format!("{}sync/meta", job.base().as_str()));
}

#[test]
fn local_rig_route_handles_the_orbstack_gateway_without_relaxing_the_guard() {
    let host = "0.250.250.254:27701".parse().unwrap();
    assert!(meguri_sync::endpoint_guard::check_endpoint("http://0.250.250.254:27701/").is_err());
    let (options, route) = common::rig_route(host);
    let proxy = route.expect("the synthetic gateway needs an explicit local proxy route");
    // Client construction checks pins/proxy without connecting to the rig.
    let job = new_job(&options);
    assert_eq!(job.base().as_str(), "http://sync-rig.test:27701/");
    assert!(proxy.requests().is_empty());
}

#[test]
#[should_panic(expected = "the Docker host must resolve to a supported local gateway")]
fn local_rig_route_refuses_other_zero_net_addresses() {
    let _ = common::rig_route("0.1.2.3:27701".parse().unwrap());
}

#[test]
#[ignore = "needs tools/mg sync-rig up"]
fn rig_login_returns_a_host_key_for_an_empty_k_and_writes_nothing() {
    let rig = rig();
    // A recording proxy in front of the rig shows what was sent.
    let proxy = rig.recording_proxy();
    let options = rig.options().proxy(Proxy::new(proxy.url()));
    let scratch = std::env::temp_dir().join(format!("meguri-sync-rig-login-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    std::env::set_current_dir(&scratch).unwrap();
    let key = login_blocking(threads(), &options, &random(), &rig.user, &rig.password).unwrap();
    // T14: the official server's key is 40 lowercase hex digits; L7 says not
    // to rely on it, so only its presence is the contract.
    assert!(!key.as_str().is_empty());
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    std::fs::remove_dir(&scratch).unwrap();
    let sent = proxy.requests();
    assert_eq!(paths(&sent), ["/sync/hostKey"]);
    assert_eq!(sent[0].sync_member("k"), Some(&json!("")));
    assert_eq!(sent[0].sync_member("c"), Some(&json!(client_identity())));
    // A second login gets a key too (login-endpoints.md L8, `login`).
    assert!(
        !login_blocking(threads(), &options, &random(), &rig.user, &rig.password)
            .unwrap()
            .as_str()
            .is_empty()
    );
}

#[test]
#[ignore = "needs tools/mg sync-rig up"]
fn rig_wrong_password_is_an_auth_failure() {
    let rig = rig();
    // sync-ops.md S4.5's derived wrong password, and a user the server does
    // not know (T14: both 403).
    let options = rig.options();
    for (user, password) in [
        (rig.user.clone(), format!("{}-wrong", rig.password)),
        (format!("{}-unknown", rig.user), rig.password.clone()),
    ] {
        let err = login_blocking(threads(), &options, &random(), &user, &password).unwrap_err();
        assert!(err.is_auth_failure(), "{err}");
        assert!(matches!(err, SyncError::Status { code: 403, .. }), "{err}");
    }
}

#[test]
#[ignore = "needs tools/mg sync-rig up"]
fn rig_accepts_the_meguri_identity_and_empty_bodies() {
    let rig = rig();
    let options = rig.options();
    let random = random();
    let key = login_blocking(threads(), &options, &random, &rig.user, &rig.password).unwrap();
    let mut job = Job::new(&options, Some(key), &random).unwrap();
    // T3: `cv` is the identity too; T14: the server answers `cont: true`.
    let meta = Payload::json(&json!({"v": 11, "cv": client_identity()})).unwrap();
    let answer: Value = serde_json::from_slice(&request(&mut job, Method::Meta, meta).unwrap()).unwrap();
    assert_eq!(answer.get("cont"), Some(&json!(true)), "{answer}");
    // T5, T14: an argument-less method sends `{}`; `abort` answers `null`.
    assert_eq!(request(&mut job, Method::Abort, Payload::empty()).unwrap(), b"null");
    // T14: an unknown host key is refused with 403 on any later request.
    let mut stranger = Job::new(&options, Some(HostKey::new("0".repeat(40))), &random).unwrap();
    let meta = Payload::json(&json!({"v": 11, "cv": client_identity()})).unwrap();
    let err = request(&mut stranger, Method::Meta, meta).unwrap_err();
    assert!(err.is_auth_failure(), "{err}");
}

#[test]
fn idle_dns_pool_threads_do_not_exhaust_the_suspend_budget() {
    let stub = SyncStub::serve(|_| ok_meta());
    let url = stub.url().replace("127.0.0.1", "localhost");
    request(&mut new_job(&options(&url)), Method::Meta, Payload::empty()).unwrap();
    let deadline = now() + Duration::from_millis(200);
    while !threads().busy_threads().is_empty() && now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(threads().busy_threads().is_empty(), "{:?}", threads().threads());
    assert!(
        threads()
            .threads()
            .iter()
            .all(|t| t.suspend_owner.as_deref() == Some("jobs"))
    );
}
