// SPDX-License-Identifier: MIT
use super::*;
use axum::body::{Bytes, to_bytes};
use http_body::{Body as HttpBody, Frame};
use std::{
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};
use tokio::sync::oneshot;

async fn fixture(trusted: bool) -> (tempfile::TempDir, Arc<Service>) {
    let temp = tempfile::tempdir().unwrap();
    let mut root = crate::DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
    root.init().unwrap();
    let mut config =
        crate::Config::new("https://example.test", "127.0.0.1:0".parse().unwrap()).unwrap();
    if trusted {
        config.trusted_peers.insert("127.0.0.1".parse().unwrap());
    }
    let service = Service::open(root, config).await.unwrap();
    (temp, service)
}

#[tokio::test]
async fn ticket_errors_hide_credentials_but_preserve_operational_failures() {
    for code in ["authentication_failed", "capability_invalid", "invalid_token"] {
        let response = api_error(Error::new(code), true);
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(response.headers()["www-authenticate"], "Bearer");
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(body.as_ref(), br#"{"error":{"code":"authentication_failed"}}"#);
    }
    for code in ["busy", "storage_unavailable", "invalid_collection", "admission_closed"] {
        let error = Error::new(code);
        let status = error.status();
        let response = api_error(error, true);
        assert_eq!(response.status(), status);
        assert!(!response.headers().contains_key("www-authenticate"));
        let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value, serde_json::json!({"error":{"code":code}}));
    }
    // Other browser routes retain their established error semantics.
    assert_eq!(api_error(Error::new("capability_invalid"), false).status(), StatusCode::FORBIDDEN);
    assert_eq!(api_error(Error::new("conflict"), false).status(), Error::new("conflict").status());
    let response = api_error(Error::new("conflict"), true);
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(!response.headers().contains_key("www-authenticate"));
    let body = axum::body::to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(body.as_ref(), br#"{"error":{"code":"conflict"}}"#);
}
fn request(path: &str, body: Body) -> Request {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(path)
        .header("origin", "https://example.test")
        .header("content-type", "application/json")
        .body(body)
        .unwrap();
    request
        .extensions_mut()
        .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
    request
}
async fn code(response: Response, expected: StatusCode, code: &str) {
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let bytes = to_bytes(response.into_body(), 16384).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["error"]["code"], code);
}

#[test]
fn parsed_container_depth_has_meaningful_boundaries_and_strings_do_not_nest() {
    for depth in [1, 15, 16, 17] {
        let mut value = serde_json::json!("[\\\"{}]");
        for _ in 0..depth {
            value = serde_json::json!({"child": value});
        }
        assert_eq!(container_depth(&value), depth);
        let bytes = serde_json::to_vec(&value).unwrap();
        assert_eq!(parse::<serde_json::Value>(&bytes).is_ok(), depth <= 16);
    }
    let string: Login = parse(br#"{"username":"abc","password":"[[[\\\"{}]]]"}"#).unwrap();
    assert!(string.password.contains('['));
    assert!(parse::<Empty>(b"{}").is_ok());
    assert!(parse::<Empty>(b"[]").is_err());
    assert!(parse::<Login>(br#"["abc","password"]"#).is_err());
    assert!(parse::<Login>(br#"{"username":"abc","username":"abc","password":"x"}"#).is_err());
}

struct ControlledBody {
    polled: Option<oneshot::Sender<()>>,
    dropped: Arc<AtomicUsize>,
}
impl Drop for ControlledBody {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl HttpBody for ControlledBody {
    type Data = Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
        if let Some(signal) = self.polled.take() {
            signal.send(()).unwrap();
        }
        Poll::Pending
    }
}

#[tokio::test]
async fn finite_admission_rejection_never_polls_and_status_health_remain_available() {
    let (_temp, service) = fixture(false).await;
    let held = (0..ingress::CAPACITY)
        .map(|_| service.browser_ingress.enter().unwrap())
        .collect::<Vec<_>>();
    let (signal, mut polled) = oneshot::channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let response = handle(
        State(service.clone()),
        request(
            "/api/login",
            Body::new(ControlledBody {
                polled: Some(signal),
                dropped: dropped.clone(),
            }),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()["retry-after"], "1");
    assert_eq!(response.headers()["connection"], "close");
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert!(matches!(
        polled.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    code(response, StatusCode::TOO_MANY_REQUESTS, "rate_limited").await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    for path in ["/api/status", "/health"] {
        let response = handle(
            State(service.clone()),
            Request::builder().uri(path).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);
    }
    drop(held);
    service.browser_ingress.close();
    code(
        handle(State(service), request("/api/logout", Body::from("{}"))).await,
        StatusCode::SERVICE_UNAVAILABLE,
        "admission_closed",
    )
    .await;
}

#[tokio::test]
async fn caller_cancellation_keeps_worker_slot_and_root_until_shutdown_ends_pending_body() {
    let (_temp, service) = fixture(false).await;
    let ingress = service.browser_ingress.clone();
    let root_owner = Arc::downgrade(&service);
    let (entered, observed) = oneshot::channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let req = request(
        "/api/login",
        Body::new(ControlledBody {
            polled: Some(entered),
            dropped: dropped.clone(),
        }),
    );
    let caller = tokio::spawn(handle(State(service.clone()), req));
    observed.await.unwrap(); // Actual body poll establishes admission and read.
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    drop(service);
    let held = (0..ingress::CAPACITY - 1)
        .map(|_| ingress.enter().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ingress.enter().unwrap_err().code, "rate_limited");
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert!(root_owner.upgrade().is_some());
    drop(held);
    ingress.close(); // Same signal sent before axum graceful shutdown.
    ingress.idle().await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(root_owner.upgrade().is_none());
}

#[tokio::test]
async fn early_origin_error_retains_unread_receiver_and_root_through_response_lifetime() {
    let (_temp, service) = fixture(false).await;
    let weak = Arc::downgrade(&service);
    let (signal, mut observed) = oneshot::channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut req = request(
        "/api/setup",
        Body::new(ControlledBody {
            polled: Some(signal),
            dropped: dropped.clone(),
        }),
    );
    req.headers_mut().remove("origin");
    let response = handle(State(service.clone()), req).await;
    drop(service);
    assert!(weak.upgrade().is_some());
    assert_eq!(response.headers()["connection"], "close");
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    assert!(matches!(
        observed.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    code(response, StatusCode::FORBIDDEN, "origin_rejected").await;
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn machine_upload_authentication_rejection_never_reads_the_body() {
    let (_temp,service)=fixture(false).await;
    let (signal,mut observed)=oneshot::channel();
    let dropped=Arc::new(AtomicUsize::new(0));
    let request=Request::builder().method(Method::POST).uri("/api/mcp/media-upload")
        .header("authorization",format!("Bearer {}","A".repeat(43)))
        .header("content-type","application/octet-stream")
        .extension(ConnectInfo("127.0.0.1:12345".parse::<SocketAddr>().unwrap()))
        .body(Body::new(ControlledBody {polled:Some(signal),dropped:dropped.clone()})).unwrap();
    let response=handle(State(service),request).await;
    assert_eq!(response.headers()["www-authenticate"],"Bearer");
    assert_eq!(response.headers()["connection"],"close");
    assert_eq!(dropped.load(Ordering::SeqCst),0);
    assert!(matches!(observed.try_recv(),Err(oneshot::error::TryRecvError::Empty)));
    code(response,StatusCode::UNAUTHORIZED,"authentication_failed").await;
    assert_eq!(dropped.load(Ordering::SeqCst),1);
}

#[tokio::test]
async fn single_http_boundary_for_fixture_capacity_and_effective_peer_budget() {
    for (trusted, limited_ip, expected) in [
        (false, "127.0.0.1", 429), // Socket wins over a spoofed forwarded address.
        (false, "192.0.2.1", 401), // Spoofed address's budget is ignored.
        (true, "192.0.2.1", 429),  // Trusted forwarded address is charged.
        (true, "127.0.0.1", 401),  // Trusted socket's budget is not substituted.
    ] {
        let (_temp, service) = fixture(trusted).await;
        // Establish the bucket deterministically in process. Only one ordinary
        // HTTP request tests the decision; no repeated network exhaustion.
        let limited_ip = limited_ip.parse().unwrap();
        for n in 0..20 {
            // Distinct synthetic names fill only the intended IP bucket. The
            // submitted username remains untouched until the one HTTP request.
            service
                .attempts
                .username(limited_ip, &format!("fixture{n}"))
                .unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let owner = service.clone();
        let server = tokio::spawn(serve(listener, owner, async {
            let _ = stopped.await;
        }));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let response = client
            .post(format!("http://{address}/api/login"))
            .header("origin", "https://example.test")
            .header("host", "spoof.invalid")
            .header("x-forwarded-host", "spoof.invalid")
            .header("forwarded", "for=198.51.100.1;host=spoof.invalid")
            .header("x-forwarded-for", "192.0.2.1")
            .header("x-forwarded-proto", "https")
            .json(&serde_json::json!({"username":" GUESSED ","password":"x"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(
            response.headers().contains_key("retry-after"),
            expected == 429
        );
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
            if expected == 429 {
                "rate_limited"
            } else {
                "authentication_failed"
            }
        );
        let response=client.post(format!("http://{address}/api/mcp/media-upload"))
            .bearer_auth("A".repeat(43)).header("content-type","application/octet-stream")
            .header("x-forwarded-for","192.0.2.1").header("x-forwarded-proto","https")
            .body("unread synthetic bytes").send().await.unwrap();
        assert_eq!(response.status().as_u16(),expected);
        assert_eq!(response.headers()["connection"],"close");
        assert_eq!(response.headers()["cache-control"],"no-store");
        assert_eq!(response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
            if expected==429 {"rate_limited"} else {"authentication_failed"});
        // This final request uses held fixture permits, not connections.
        let held = (0..ingress::CAPACITY)
            .map(|_| service.browser_ingress.enter().unwrap())
            .collect::<Vec<_>>();
        let response = client
            .post(format!("http://{address}/api/logout"))
            .header("origin", "https://example.test")
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 429);
        assert_eq!(response.headers()["retry-after"], "1");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
            "rate_limited"
        );
        drop(held);
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn completed_body_and_error_keep_admission_until_response_is_dropped() {
    let (_temp, service) = fixture(false).await;
    let response = handle(
        State(service.clone()),
        request("/api/logout", Body::from("{}")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let held = (0..ingress::CAPACITY - 1)
        .map(|_| service.browser_ingress.enter().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        service.browser_ingress.enter().unwrap_err().code,
        "rate_limited"
    );
    code(response, StatusCode::UNAUTHORIZED, "authentication_failed").await;
    drop(service.browser_ingress.enter().unwrap());
    drop(held);
    let held = (0..ingress::CAPACITY)
        .map(|_| service.browser_ingress.enter().unwrap())
        .collect::<Vec<_>>();
    drop(held);
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn backup_scheduler_stops_before_new_work_and_joins_an_active_sweep() {
    let (temp, service) = fixture(false).await;
    let token = std::fs::read_to_string(temp.path().join("owner/state/setup-token")).unwrap();
    service.claim("127.0.0.1".parse().unwrap(), "setup", token.trim().into(), "scheduler.member", "synthetic password 123".into()).await.unwrap();
    let (stop, stopped) = oneshot::channel();
    stop.send(()).unwrap();
    backup_sweeps(service.clone(), stopped).await;
    let count = service.metadata.run(|c| {
        Ok(c.query_row("SELECT count(*) FROM backup_points", [], |r| r.get::<_, i64>(0))?)
    }).await.unwrap();
    assert_eq!(count, 0, "shutdown already began: no initial capture may start");

    let pause = service.hooks.arm("backup_snapshot_pinned");
    let (stop, stopped) = oneshot::channel();
    let running = tokio::spawn(backup_sweeps(service.clone(), stopped));
    tokio::time::timeout(Duration::from_secs(30), pause.arrived()).await.unwrap();
    stop.send(()).unwrap();
    assert!(!running.is_finished(), "shutdown must join the already-running capture");
    pause.resume();
    tokio::time::timeout(Duration::from_secs(30), running).await.unwrap().unwrap();
    let count = service.metadata.run(|c| {
        Ok(c.query_row("SELECT count(*) FROM backup_points", [], |r| r.get::<_, i64>(0))?)
    }).await.unwrap();
    assert_eq!(count, 1);
}

#[test]
fn expected_account_header_is_optional_for_legacy_callers_but_unambiguous() {
    let mut headers = HeaderMap::new();
    assert_eq!(expected_account(&headers).unwrap(), None);
    let account = "A".repeat(43); // canonical unpadded base64url of 32 zero bytes
    headers.insert("x-meguri-expected-account", account.parse().unwrap());
    assert_eq!(expected_account(&headers).unwrap().as_deref(), Some(account.as_str()));
    headers.append("x-meguri-expected-account", account.parse().unwrap());
    assert_eq!(expected_account(&headers).unwrap_err().code, "invalid_request");
    headers.remove("x-meguri-expected-account");
    for invalid in ["", "account-a", "account-a,account-b", &format!("{account},{account}"),
        &format!("{account}="), &format!("{}B", "A".repeat(42))] {
        headers.insert("x-meguri-expected-account", invalid.parse().unwrap());
        let error = expected_account(&headers).unwrap_err();
        assert_eq!(error.code, "invalid_request");
        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
    }
}

#[cfg(feature = "test-hooks")]
async fn backup_fixture() -> (tempfile::TempDir, Arc<Service>, String, String, String) {
    let (temp, service) = fixture(false).await;
    let token = std::fs::read_to_string(temp.path().join("owner/state/setup-token")).unwrap();
    let ip = "127.0.0.1".parse().unwrap();
    service.claim(ip, "setup", token.trim().into(), "restore.member", "synthetic password 123".into()).await.unwrap();
    let (identity, secret, _) = service.browser_login_session(ip, "restore.member", "synthetic password 123".into()).await.unwrap();
    service.capture_backups_at(crate::accounts::now()).await.unwrap();
    let id = identity.id;
    let history = service.backups(service.browser_authority(&secret).await.unwrap(), Some(id.clone())).await.unwrap();
    let backup = serde_json::to_value(history).unwrap()["backups"][0]["id"].as_str().unwrap().to_owned();
    (temp, service, format!("{COOKIE}={secret}"), id, backup)
}

#[cfg(feature = "test-hooks")]
fn backup_request(cookie: &str, account: &str, backup: Option<&str>) -> Request {
    let mut req = match backup {
        Some(backup) => request(&format!("/api/me/backups/{backup}/restore"), Body::from("{}")),
        None => Request::builder().uri("/api/me/backups").body(Body::empty()).unwrap(),
    };
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    req.headers_mut().insert("x-meguri-expected-account", account.parse().unwrap());
    req
}

#[cfg(feature = "test-hooks")]
async fn backup_json(response: Response) -> serde_json::Value {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["connection"], "close");
    serde_json::from_slice(&to_bytes(response.into_body(), 16384).await.unwrap()).unwrap()
}

#[cfg(feature = "test-hooks")]
async fn backup_live_state(service: &Service, id: &str) -> (std::path::PathBuf, i64, i64, i64) {
    let id = id.to_owned();
    service.metadata.files(move |root| {
        let path = root.collection_path(&id)?;
        let c = crate::collection::schema::open(&path, false)?;
        let (scm, modified, usn) = c.query_row("SELECT scm,mod,usn FROM col WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok((path, scm, modified, usn))
    }).await.unwrap()
}

#[cfg(feature = "test-hooks")]
struct ResumeBackupPause(Arc<crate::test_hooks::Pause>);
#[cfg(feature = "test-hooks")]
impl Drop for ResumeBackupPause {
    fn drop(&mut self) {self.0.resume();}
}

#[cfg(feature = "test-hooks")]
struct CompleteRestoreBody {
    frame: Option<Bytes>,
    dropped: Arc<AtomicUsize>,
}
#[cfg(feature = "test-hooks")]
impl Drop for CompleteRestoreBody {
    fn drop(&mut self) {self.dropped.fetch_add(1, Ordering::SeqCst);}
}
#[cfg(feature = "test-hooks")]
impl HttpBody for CompleteRestoreBody {
    type Data = Bytes;
    type Error = std::io::Error;
    fn poll_frame(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<std::result::Result<Frame<Bytes>, Self::Error>>> {
        Poll::Ready(self.frame.take().map(|bytes| Ok(Frame::data(bytes))))
    }
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn cancelled_http_restore_before_lease_cannot_mutate_after_settled_history() {
    let (_temp, service, cookie, id, backup) = backup_fixture().await;
    let before = backup_json(handle(State(service.clone()), backup_request(&cookie, &id, None)).await).await;
    let state = backup_live_state(&service, &id).await;
    let pause = service.hooks.arm("backup_restore_before_admission");
    let _release = ResumeBackupPause(pause.clone());
    // This pause is reached only after the actual route has read strict {},
    // checked Origin, authenticated the cookie and matched the expected account.
    let dropped = Arc::new(AtomicUsize::new(0));
    let req = backup_request(&cookie, &id, Some(&backup)).map(|_| Body::new(CompleteRestoreBody {
        frame: Some(Bytes::from_static(b"{}")), dropped: dropped.clone(),
    }));
    let caller = tokio::spawn(handle(State(service.clone()), req));
    tokio::time::timeout(Duration::from_secs(30), pause.arrived()).await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert_eq!(dropped.load(Ordering::SeqCst), 0, "the blocked worker retains the complete incoming receiver");
    let root_owner = Arc::downgrade(&service);
    drop(service);
    let service = root_owner.upgrade().expect("the blocked worker retains service/root ownership");
    assert_eq!(backup_json(handle(State(service.clone()), backup_request(&cookie, &id, None)).await).await, before);
    // Cancellation must not release the blocked outer worker's ingress lease.
    let held = (0..ingress::CAPACITY - 1).map(|_| service.browser_ingress.enter().unwrap()).collect::<Vec<_>>();
    assert_eq!(service.browser_ingress.enter().unwrap_err().code, "rate_limited");
    drop(held);
    pause.resume();
    // Join the actual orphaned worker, rather than inferring non-mutation from
    // a delay or polling history while a late admission might still be pending.
    tokio::time::timeout(Duration::from_secs(30), service.browser_ingress.idle()).await.unwrap();
    assert_eq!(backup_json(handle(State(service.clone()), backup_request(&cookie, &id, None)).await).await, before);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert_eq!(backup_live_state(&service, &id).await, state);
    drop(service);
    assert!(root_owner.upgrade().is_none());
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn cancelled_http_restore_after_lease_stays_busy_through_committed_intent_and_finishes() {
    for phase in ["backup_restore_preparing", "backup_restore_intent_committed", "backup_restore_cleanup"] {
        let (_temp, service, cookie, id, backup) = backup_fixture().await;
        let before = backup_json(handle(State(service.clone()), backup_request(&cookie, &id, None)).await).await;
        let state = backup_live_state(&service, &id).await;
        let pause = service.hooks.arm(phase);
        let _release = ResumeBackupPause(pause.clone());
        let caller = tokio::spawn(handle(State(service.clone()), backup_request(&cookie, &id, Some(&backup))));
        tokio::time::timeout(Duration::from_secs(30), pause.arrived()).await.unwrap();
        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        for selected in [None, Some(backup.as_str())] {
            code(handle(State(service.clone()), backup_request(&cookie, &id, selected)).await, StatusCode::CONFLICT, "busy").await;
            // Busy never masks a mismatching expected account or invalid cookie.
            let mismatch = backup_request(&cookie, &crate::crypto::random_token().unwrap(), selected);
            code(handle(State(service.clone()), mismatch).await, StatusCode::CONFLICT, "stale_authority").await;
            let mismatch = backup_request(&format!("{COOKIE}={}", crate::crypto::random_token().unwrap()), &id, selected);
            code(handle(State(service.clone()), mismatch).await, StatusCode::UNAUTHORIZED, "authentication_failed").await;
        }
        if phase == "backup_restore_intent_committed" {
            let count = service.metadata.run(|c| Ok(c.query_row("SELECT count(*) FROM backup_restores", [], |r| r.get::<_, i64>(0))?)).await.unwrap();
            assert_eq!(count, 1, "409 must take precedence while durable intent and the live lease coexist");
        }
        pause.resume();
        tokio::time::timeout(Duration::from_secs(30), service.browser_ingress.idle()).await.unwrap();
        let after = backup_json(handle(State(service.clone()), backup_request(&cookie, &id, None)).await).await;
        assert_eq!(after["data"]["backups"].as_array().unwrap().len(), before["data"]["backups"].as_array().unwrap().len() + 1);
        assert_eq!(after["data"]["backups"].as_array().unwrap().iter().filter(|p| p["kind"] == "before_restore").count(), 1);
        let restored = backup_live_state(&service, &id).await;
        assert_ne!(restored.0, state.0);
        assert!(restored.1 > state.1 && restored.2 > state.2);
    }
}
