//! The error kinds of a failed sync job (specs/sync/errors.md): E3's class
//! and retry of every kind, E4's statuses and E5's transport categories
//! through meguri-testkit's scripted stub, one decode failure per wire
//! section (E6), `server_message` from `cont` false and from a media `err`,
//! and the local failures of E7 (E11).
//!
//! Expected values come from the spec's tables, never from the code under
//! test.

mod common;

use std::time::Duration;

use common::{HOST_KEY, block_on, job, options, random};
use meguri_engine::CancelToken;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::time::EpochMillis;
use meguri_model::usn::Usn;
use meguri_storage::{RowError, StorageError};
use meguri_sync::status::{self, LocalRecord};
use meguri_sync::wire::{self, SanityCounts};
use meguri_sync::{
    DbCheckOrigin, EndpointRule, ErrorClass, ErrorKind, HostKey, InvalidInput, Job, JobError, JobOptions, Method,
    Proxy, Retry, ServerKind, SyncError, decode_login_answer,
};
use meguri_testkit::sync_stub::{Part, Reply, ServerCert, SyncStub, TestRoot, compress};

const NOW_MS: i64 = 1_894_701_780_000;

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

fn local() -> LocalRecord {
    LocalRecord {
        modified: EpochMillis(NOW_MS - 120_000),
        schema_modified: EpochMillis(NOW_MS - 3_600_000),
        usn: Usn(4),
        last_sync: EpochMillis(NOW_MS - 120_000),
        has_cards: true,
    }
}

/// The status check of a job on `options`, as a failure.
fn check_fails(options: &JobOptions) -> JobError {
    let mut job = job(options);
    block_on(status::check(&mut job, &local(), &clock())).expect_err("the check fails")
}

// --- E2, E3 ------------------------------------------------------------------

#[test]
fn every_kind_has_the_id_class_and_retry_of_e3() {
    use ErrorClass::{Actionable, Silent, Transient};
    use Retry::{AfterTrigger, Auto, No};
    let counts = Some(SanityCounts::default());
    for (kind, id, class, retry) in [
        (ErrorKind::AuthFailed, "auth_failed", Actionable, No),
        (ErrorKind::Conflict, "conflict", Transient, Auto),
        (ErrorKind::ClientTooOld, "client_too_old", Actionable, No),
        (ErrorKind::ServerError { code: 503 }, "server_error", Transient, Auto),
        (
            ErrorKind::DbCheckRequired {
                origin: DbCheckOrigin::Server,
            },
            "db_check_required",
            Actionable,
            No,
        ),
        (ErrorKind::ProxyAuth, "proxy_auth", Actionable, No),
        (
            ErrorKind::UnexpectedStatus { code: 404 },
            "unexpected_status",
            Actionable,
            No,
        ),
        (ErrorKind::Offline, "offline", Transient, Auto),
        (ErrorKind::Network { head_received: true }, "network", Transient, Auto),
        (ErrorKind::Timeout, "timeout", Transient, Auto),
        (ErrorKind::Tls { self_hosted: true }, "tls", Actionable, No),
        (
            ErrorKind::RedirectRefused { code: 301 },
            "redirect_refused",
            Actionable,
            No,
        ),
        (ErrorKind::BadResponse, "bad_response", Transient, Auto),
        (
            ErrorKind::InvalidEndpoint {
                rule: EndpointRule::Empty,
            },
            "invalid_endpoint",
            Actionable,
            No,
        ),
        (
            ErrorKind::InvalidInput {
                field: InvalidInput::Password,
            },
            "invalid_input",
            Actionable,
            No,
        ),
        (ErrorKind::ProxyUnusable, "proxy_unusable", Actionable, No),
        (ErrorKind::EndpointRefused, "endpoint_refused", Actionable, No),
        (
            ErrorKind::ServerMessage { text: String::new() },
            "server_message",
            Actionable,
            No,
        ),
        (
            ErrorKind::ClockIncorrect { offset_secs: -360 },
            "clock_incorrect",
            Actionable,
            No,
        ),
        (
            ErrorKind::UploadTooLarge {
                size_bytes: 2,
                limit_bytes: 1,
            },
            "upload_too_large",
            Actionable,
            No,
        ),
        (
            ErrorKind::SanityCheckFailed {
                client: counts,
                server: None,
            },
            "sanity_check_failed",
            Actionable,
            No,
        ),
        (
            ErrorKind::NotetypeSchemaChanged {
                notetype_id: NotetypeId(1),
            },
            "notetype_schema_changed",
            Actionable,
            No,
        ),
        (
            ErrorKind::MissingNotetype {
                notetype_id: NotetypeId(1),
                note_id: NoteId(2),
            },
            "missing_notetype",
            Actionable,
            No,
        ),
        (ErrorKind::DownloadInvalid, "download_invalid", Actionable, No),
        (ErrorKind::MediaResyncFailed, "media_resync_failed", Actionable, No),
        (
            ErrorKind::DiskFull {
                needed_bytes: Some(1),
                available_bytes: None,
            },
            "disk_full",
            Actionable,
            No,
        ),
        (ErrorKind::LocalStorage, "local_storage", Actionable, No),
        (ErrorKind::SessionActive, "session_active", Silent, AfterTrigger),
        (ErrorKind::ExclusiveBusy, "exclusive_busy", Transient, Auto),
        (ErrorKind::Cancelled, "cancelled", Silent, No),
        (ErrorKind::Interrupted, "interrupted", Silent, AfterTrigger),
        (ErrorKind::Internal, "internal", Actionable, No),
    ] {
        assert_eq!((kind.id(), kind.class(), kind.retry()), (id, class, retry), "{kind:?}");
    }
    // E2's names.
    assert_eq!(
        [Transient.as_str(), Actionable.as_str(), Silent.as_str()],
        ["transient", "actionable", "silent"]
    );
    assert_eq!(
        [Auto.as_str(), AfterTrigger.as_str(), No.as_str()],
        ["auto", "after_trigger", "no"]
    );
}

#[test]
fn errors_name_media_methods_with_their_prefix() {
    // E1: `meta`, `applyChanges`, `msync/begin`.
    let named = |method| JobError::bad_response(method, "x").method_name();
    assert_eq!(named(Method::Meta), Some("meta"));
    assert_eq!(named(Method::ApplyChanges), Some("applyChanges"));
    assert_eq!(named(Method::MediaBegin), Some("msync/begin"));
    assert_eq!(named(Method::DownloadFiles), Some("msync/downloadFiles"));
}

// --- E4: statuses ----------------------------------------------------------------

#[test]
fn statuses_map_to_kinds_by_their_code_alone() {
    // E4's table and E11's extra codes, each without `anki-original-size`
    // and with a body that must not matter or show; class and retry per E3.
    use ErrorClass::{Actionable, Transient};
    use Retry::{Auto, No};
    let server = |code| (ErrorKind::ServerError { code }, Transient, Auto);
    let unexpected = |code| (ErrorKind::UnexpectedStatus { code }, Actionable, No);
    for (code, (kind, class, retry)) in [
        (
            400,
            (
                ErrorKind::DbCheckRequired {
                    origin: DbCheckOrigin::Server,
                },
                Actionable,
                No,
            ),
        ),
        (401, unexpected(401)),
        (403, (ErrorKind::AuthFailed, Actionable, No)),
        (404, unexpected(404)),
        (407, (ErrorKind::ProxyAuth, Actionable, No)),
        (408, (ErrorKind::Timeout, Transient, Auto)),
        (409, (ErrorKind::Conflict, Transient, Auto)),
        (413, unexpected(413)),
        (429, server(429)),
        (500, server(500)),
        (501, (ErrorKind::ClientTooOld, Actionable, No)),
        (502, server(502)),
        (503, server(503)),
        (504, server(504)),
        (505, server(505)),
    ] {
        let stub = SyncStub::serve(move |_| {
            Reply::status(code).with_body(vec![Part::Bytes(b"<html>conflict SECRET-BODY</html>".to_vec())])
        });
        let err = check_fails(&options(&stub.url()));
        assert_eq!(
            (&err.kind, err.class(), err.retry()),
            (&kind, class, retry),
            "{code}: {err}"
        );
        assert_eq!(err.method, Some(Method::Meta), "{code}");
        assert!(!err.detail.contains("SECRET"), "{code}: {err}");
    }
}

// --- E5: transport categories ----------------------------------------------------

#[test]
fn a_refused_connection_and_a_broken_one_are_network_errors() {
    // E5: refused means the device has a route; `head_received` says
    // whether the status line had arrived.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}/", listener.local_addr().unwrap())
    };
    assert_eq!(
        check_fails(&options(&closed)).kind,
        ErrorKind::Network { head_received: false }
    );
    let stub = SyncStub::serve(|_| Reply::hang_up());
    assert_eq!(
        check_fails(&options(&stub.url())).kind,
        ErrorKind::Network { head_received: false }
    );
    let stub = SyncStub::serve(|_| {
        let body = compress(&[b'{'; 4000]);
        Reply::ok(&[b'{'; 4000]).with_body(vec![Part::Bytes(body[..body.len() / 2].to_vec()), Part::Close])
    });
    assert_eq!(
        check_fails(&options(&stub.url())).kind,
        ErrorKind::Network { head_received: true }
    );
}

#[test]
fn a_name_that_does_not_resolve_is_offline() {
    // E5: no route to the server. A reserved `.test` name nothing pins.
    let err = check_fails(&options("http://nowhere-u106.test:9/"));
    assert_eq!(err.kind, ErrorKind::Offline, "{err}");
}

#[test]
fn a_failed_handshake_is_tls_with_the_self_hosted_hint_by_server_kind() {
    // E5 and E3: the hint only for the user's own server (R1561).
    let root = TestRoot::generate();
    let stub = SyncStub::serve_tls(&ServerCert::self_signed(&["self.test"]), |_| Reply::ok(b"{}"));
    for (kind, self_hosted) in [(ServerKind::SelfHosted, true), (ServerKind::AnkiWeb, false)] {
        let options = JobOptions::new(common::base(&stub.url_for("self.test")), kind)
            .pin_host("self.test", stub.addr())
            .trust_root(root.der());
        assert_eq!(check_fails(&options).kind, ErrorKind::Tls { self_hosted });
    }
}

#[test]
fn a_stalled_answer_is_a_timeout() {
    let stub = SyncStub::serve(|_| Reply::ok(b"{}").head_delay(Duration::from_secs(3)));
    let options = options(&stub.url()).stall_timeout(Duration::from_secs(1));
    assert_eq!(check_fails(&options).kind, ErrorKind::Timeout);
}

#[test]
fn a_refused_redirect_carries_its_code() {
    let stub = SyncStub::serve(|_| Reply::redirect(301, Some("http://127.0.0.1:9/")));
    assert_eq!(
        check_fails(&options(&stub.url())).kind,
        ErrorKind::RedirectRefused { code: 301 }
    );
}

#[test]
fn a_refused_redirects_detail_holds_no_credentials_from_its_location() {
    // E1: `detail` never holds a URL with credentials. transport.md T7
    // refuses such a target; the `Location` text is the server's to choose.
    for (code, location) in [
        (308, "https://USER:SECRET@127.0.0.1:9/"),
        (301, "http://USER:SECRET@127.0.0.1:9/next?token=SECRET#SECRET"),
        (308, "//USER:SECRET@127.0.0.1:9/"),
        (307, "not a URL, USER:SECRET@"),
    ] {
        let stub = SyncStub::serve(move |_| Reply::redirect(code, Some(location)));
        let err = check_fails(&options(&stub.url()));
        assert_eq!(err.kind, ErrorKind::RedirectRefused { code }, "{location}");
        for secret in ["SECRET", "USER"] {
            assert!(!err.detail.contains(secret), "{err}");
            assert!(!err.to_string().contains(secret), "{err}");
        }
    }
}

#[test]
fn a_success_that_breaks_the_transports_checks_is_bad_response() {
    // E6 via transport.md T6.2: no `anki-original-size`.
    let stub = SyncStub::serve(|_| Reply::ok(b"{}").without_header("anki-original-size"));
    let err = check_fails(&options(&stub.url()));
    assert_eq!((err.kind, err.method), (ErrorKind::BadResponse, Some(Method::Meta)));
}

#[test]
fn a_cancelled_job_is_cancelled() {
    let stub = SyncStub::serve(|_| Reply::ok(b"{}"));
    let token = CancelToken::new();
    token.cancel();
    let options = options(&stub.url()).cancel(token);
    assert_eq!(check_fails(&options).kind, ErrorKind::Cancelled);
}

#[test]
fn values_refused_before_any_request_keep_their_category() {
    // E5: invalid input, an unusable proxy, a URL the guard refuses (test
    // builds), and an endpoint that breaks L1.
    let start = |options: &JobOptions| {
        Job::new(options, Some(HostKey::new(HOST_KEY)), &random())
            .map(|_| ())
            .map_err(|err| JobError::from_transport(err, None, ServerKind::SelfHosted))
    };
    let err = start(&options("http://127.0.0.1:9/").stall_timeout(Duration::ZERO)).unwrap_err();
    assert_eq!(
        err.kind,
        ErrorKind::InvalidInput {
            field: InvalidInput::StallTimeout
        }
    );
    let err = start(&options("http://127.0.0.1:9/").proxy(Proxy::new("socks5://127.0.0.1:1080"))).unwrap_err();
    assert_eq!(err.kind, ErrorKind::ProxyUnusable);
    let err = check_fails(&options("http://8.8.8.8/"));
    assert_eq!((err.kind, err.method), (ErrorKind::EndpointRefused, Some(Method::Meta)));
    let rule = meguri_sync::Endpoint::parse("ftp://h").unwrap_err();
    let err = JobError::from_transport(SyncError::InvalidEndpoint(rule), None, ServerKind::SelfHosted);
    assert_eq!(
        err.kind,
        ErrorKind::InvalidEndpoint {
            rule: EndpointRule::UnsupportedScheme
        }
    );
}

// --- E6: decode failures, one per wire section ----------------------------------------

#[test]
fn every_answer_that_does_not_decode_is_bad_response_with_its_method() {
    // Each served by the stub and decoded as its method's answer; the detail
    // names the place and never quotes the body.
    let served = common::served;
    let secret = "SECRET-VALUE";
    let cases: Vec<(Method, Result<(), JobError>)> = vec![
        (
            Method::HostKey,
            decode_login_answer(&served(Method::HostKey, br#"{"key":""}"#))
                .map(|_| ())
                .map_err(|err| JobError::from_transport(err, Some(Method::HostKey), ServerKind::SelfHosted)),
        ),
        (
            Method::Meta,
            wire::decode_meta(&served(Method::Meta, format!(r#"{{"mod":"{secret}"}}"#).as_bytes())).map(|_| ()),
        ),
        (
            Method::Start,
            wire::decode_graves(&served(
                Method::Start,
                format!(r#"{{"cards":["{secret}"]}}"#).as_bytes(),
            ))
            .map(|_| ()),
        ),
        (
            Method::ApplyChanges,
            wire::decode_changes(&served(
                Method::ApplyChanges,
                format!(r#"{{"models":[{{"name":"{secret}"}}],"decks":[[],[]],"tags":[]}}"#).as_bytes(),
            ))
            .map(|_| ()),
        ),
        (
            Method::Chunk,
            wire::decode_chunk(&served(
                Method::Chunk,
                format!(r#"{{"done":true,"cards":[["{secret}"]]}}"#).as_bytes(),
            ))
            .map(|_| ()),
        ),
        (
            Method::SanityCheck2,
            wire::decode_sanity_answer(&served(
                Method::SanityCheck2,
                format!(r#"{{"status":"{secret}"}}"#).as_bytes(),
            ))
            .map(|_| ()),
        ),
        (
            Method::Finish,
            wire::decode_finish(&served(Method::Finish, format!(r#""{secret}""#).as_bytes())).map(|_| ()),
        ),
        (
            Method::MediaBegin,
            wire::decode_begin(&served(
                Method::MediaBegin,
                format!(r#"{{"data":"{secret}"}}"#).as_bytes(),
            ))
            .map(|_| ()),
        ),
        (
            Method::MediaChanges,
            wire::decode_media_changes(&served(Method::MediaChanges, b"[]")).map(|_| ()),
        ),
        (
            Method::UploadChanges,
            wire::decode_upload_changes(&served(Method::UploadChanges, br#"{"data":[1]}"#)).map(|_| ()),
        ),
        (
            Method::MediaSanity,
            wire::decode_media_sanity(&served(Method::MediaSanity, b"{}")).map(|_| ()),
        ),
        (
            Method::DownloadFiles,
            wire::decode_download_zip(&served(Method::DownloadFiles, secret.as_bytes())).map(|_| ()),
        ),
    ];
    for (method, result) in cases {
        let err = result.expect_err("the answer does not decode");
        assert_eq!(err.kind, ErrorKind::BadResponse, "{method:?}: {err}");
        assert_eq!(err.method, Some(method), "{err}");
        assert!(!err.detail.is_empty(), "{err}");
        assert!(!err.detail.contains(secret), "{err}");
    }
}

#[test]
fn server_messages_come_from_cont_false_and_from_a_media_err() {
    // E3 `server_message`: the text exactly as the server sent it.
    let meta = r#"{"mod":1,"scm":2,"usn":3,"ts":1894701780,"msg":"Upgrade, please.","cont":false,"media_usn":0}"#;
    let stub = SyncStub::serve(move |_| Reply::ok(meta.as_bytes()));
    let err = check_fails(&options(&stub.url()));
    assert_eq!(
        (err.kind, err.method),
        (
            ErrorKind::ServerMessage {
                text: "Upgrade, please.".to_owned()
            },
            Some(Method::Meta)
        )
    );
    let err = wire::decode_begin(&common::served(Method::MediaBegin, br#"{"err":"no media for you"}"#)).unwrap_err();
    assert_eq!(
        (err.kind, err.method),
        (
            ErrorKind::ServerMessage {
                text: "no media for you".to_owned()
            },
            Some(Method::MediaBegin)
        )
    );
}

// --- E7: local failures -----------------------------------------------------------------

#[test]
fn local_failures_map_per_e7() {
    let full = StorageError::Sqlite {
        code: 13,
        extended_code: 13,
        message: "database or disk is full".to_owned(),
    };
    assert_eq!(
        JobError::from_storage(&full).kind,
        ErrorKind::DiskFull {
            needed_bytes: None,
            available_bytes: None
        }
    );
    let row = StorageError::Row(RowError {
        table: "notes",
        column: "flds",
        key: "1".to_owned(),
        reason: "not text".to_owned(),
    });
    assert_eq!(
        JobError::from_storage(&row).kind,
        ErrorKind::DbCheckRequired {
            origin: DbCheckOrigin::Local
        }
    );
    for other in [
        StorageError::Io {
            detail: "permission denied".to_owned(),
        },
        StorageError::Sqlite {
            code: 10,
            extended_code: 10,
            message: "disk I/O error".to_owned(),
        },
    ] {
        assert_eq!(JobError::from_storage(&other).kind, ErrorKind::LocalStorage);
    }
}
