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
        // This second single request uses held fixture permits, not connections.
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

#[test]
fn download_form_is_one_exact_opaque_field() {
    let token = crate::crypto::random_token().unwrap();
    assert_eq!(download_form(format!("ticket={token}").as_bytes()).unwrap(), token);
    for form in [String::new(), token.clone(), format!("ticket={token}&ticket={token}"),
        format!("ticket={token}&other=1"), format!("ticket=%41{}", &token[1..]),
        format!("ticket={token}+"), format!("Ticket={token}"), format!("ticket={token}\n")] {
        assert!(download_form(form.as_bytes()).is_err());
    }
}
