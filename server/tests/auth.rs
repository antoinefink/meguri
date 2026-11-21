// SPDX-License-Identifier: MIT
mod support;
use meguri_sync_server::{Config, crypto};
use serde_json::{Value, json};
use support::*;

fn timestamp() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn session_expiry(db: &rusqlite::Connection) -> i64 {
    db.query_row(
        "SELECT expires_at FROM credentials WHERE kind='session'",
        [],
        |r| r.get(0),
    ).unwrap()
}

fn session_cookie(r: &reqwest::Response) -> (String, i64) {
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["cache-control"], "no-store");
    assert_eq!(r.headers().get_all("set-cookie").iter().count(), 1);
    let value = r.headers()["set-cookie"].to_str().unwrap();
    assert!(value.starts_with("__Host-meguri-session="));
    let attributes: Vec<_> = value.split(';').map(str::trim).collect();
    for attr in ["Secure", "HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(attributes.contains(&attr));
    }
    assert!(!value.to_ascii_lowercase().contains("domain="));
    let age = attributes.iter().find_map(|attr| attr.strip_prefix("Max-Age="))
        .unwrap().parse().unwrap();
    (attributes[0].to_owned(), age)
}

async fn identity_cookie(h: &Harness, cookie: &str, db: &rusqlite::Connection) -> i64 {
    let before = timestamp();
    let r = h.get("/api/me", Some(cookie)).await;
    let after = timestamp();
    let expires = session_expiry(db);
    let (returned_cookie, age) = session_cookie(&r);
    assert_eq!(returned_cookie, cookie);
    assert!((expires - after..=expires - before).contains(&age));
    assert_eq!(r.json::<Value>().await.unwrap()["data"]["server_url"], format!("{ORIGIN}/"));
    expires
}

async fn rejected_identity(h: &Harness, cookie: &str) {
    let r = h.get("/api/me", Some(cookie)).await;
    assert_eq!(r.headers()["cache-control"], "no-store");
    assert!(!r.headers().contains_key("set-cookie"));
    error(r, 401, "authentication_failed").await;
}

#[test]
fn validation_and_origin_are_exact() {
    assert_eq!(crypto::username(" \tA.B-c_1\r\n").unwrap(), "a.b-c_1");
    for name in ["ab", ".abc", "a/b", "ééé", "a b", "abc\0"] {
        assert!(crypto::username(name).is_err());
    }
    assert!(crypto::username(&"a".repeat(64)).is_ok());
    assert!(crypto::username(&"a".repeat(65)).is_err());
    assert!(crypto::new_password(" 1234567890 ").is_ok());
    assert!(crypto::new_password(&"é".repeat(512)).is_ok());
    assert!(crypto::new_password(&"é".repeat(513)).is_err());
    assert!(crypto::new_password("12345678901").is_err());
    assert!(crypto::new_password("abc\0abcdefghijk").is_err());
    for origin in [
        "http://example.test",
        "https://user@example.test",
        "https://@example.test",
        "https://example.test/",
        "https://example.test/path",
        "https://example.test?x",
        "https://example.test#x",
        "https://EXAMPLE.test",
        "https://example.test:443",
    ] {
        assert!(Config::new(origin, "127.0.0.1:0".parse().unwrap()).is_err());
    }
    assert!(Config::new("https://example.test:8443", "127.0.0.1:0".parse().unwrap()).is_ok());
}
#[tokio::test]
async fn actual_http_origin_precedes_body_work_security_headers_on_errors_no_spa() {
    let h = Harness::new().await;
    for origin in [None, Some("null"), Some("https://other.test")] {
        let mut r = h
            .client
            .post(format!("{}/api/setup", h.base))
            .header("content-type", "application/json")
            .body("not JSON");
        if let Some(o) = origin {
            r = r.header("origin", o);
        }
        let r = r.send().await.unwrap();
        for (k, v) in [
            ("cache-control", "no-store"),
            ("referrer-policy", "no-referrer"),
            ("x-frame-options", "DENY"),
            ("x-content-type-options", "nosniff"),
        ] {
            assert_eq!(r.headers()[k], v);
        }
        let csp = r.headers()["content-security-policy"].to_str().unwrap();
        assert!(!csp.contains("unsafe-inline"));
        assert!(!csp.contains("unsafe-eval"));
        error(r, 403, "origin_rejected").await;
    }
    let r = h
        .client
        .post(format!("{}/api/setup", h.base))
        .header("origin", ORIGIN)
        .header("origin", ORIGIN)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    error(r, 403, "origin_rejected").await;
    error(h.get("/api/unknown", None).await, 404, "route_not_found").await;
    error(
        h.get(&format!("/api/setup?token={}", h.setup_token), None)
            .await,
        400,
        "invalid_request",
    )
    .await;
    error(h.get("/api/setup", None).await, 404, "route_not_found").await;
    let native = h.get("/sync/", None).await;
    assert_eq!(native.status(), 404);
    assert_eq!(native.text().await.unwrap(), "route_not_found");
    h.setup().await;
}
#[tokio::test]
async fn exact_json_mime_size_and_members_are_enforced() {
    let h = Harness::new().await;
    let url = format!("{}/api/login", h.base);
    for mime in [
        "text/plain",
        "application/json; charset=latin1",
        "application/json; charset=utf-8; other=x",
    ] {
        error(
            h.client
                .post(&url)
                .header("origin", ORIGIN)
                .header("content-type", mime)
                .body("{}")
                .send()
                .await
                .unwrap(),
            415,
            "unsupported_content_type",
        )
        .await;
    }
    for body in [
        r#"{"username":"abc","username":"def","password":"not-a-secret"}"#,
        r#"{"username":"abc","password":"not-a-secret","extra":1}"#,
        r#"{"username":"abc"}"#,
        r#"{"username":null,"password":"not-a-secret"}"#,
    ] {
        error(
            h.client
                .post(&url)
                .header("origin", ORIGIN)
                .header("content-type", "application/json; charset=utf-8")
                .body(body)
                .send()
                .await
                .unwrap(),
            400,
            "invalid_request",
        )
        .await;
    }
    let body = format!("{}0{}", "[".repeat(17), "]".repeat(17));
    error(
        h.client
            .post(&url)
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .unwrap(),
        400,
        "invalid_request",
    )
    .await;
    error(
        h.client
            .post(&url)
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .body(vec![b'x'; 16385])
            .send()
            .await
            .unwrap(),
        413,
        "request_too_large",
    )
    .await;
    error(
        h.client
            .post(&url)
            .header("origin", ORIGIN)
            .header("content-type", "application/json")
            .body(vec![0xff])
            .send()
            .await
            .unwrap(),
        400,
        "invalid_request",
    )
    .await;
}
#[tokio::test]
async fn cookie_and_database_agree_on_year_lifetime_and_no_plaintext_bearers_in_sqlite() {
    let h = Harness::new().await;
    h.setup().await;
    let before = timestamp();
    let r = h
        .post(
            "/api/login",
            json!({"username":"root.admin","password":PASSWORD}),
            None,
        )
        .await;
    let after = timestamp();
    let (cookie, age) = session_cookie(&r);
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let expires = session_expiry(&db);
    // Contract values are independent of the implementation's TTL constant.
    assert!((before + 31_536_000..=after + 31_536_000).contains(&expires));
    assert!((expires - after..=expires - before).contains(&age));
    assert!(age > 0 && age <= 31_536_000);
    let host = h
        .service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let host_expiry: Option<i64> = db.query_row(
        "SELECT expires_at FROM credentials WHERE kind='hostkey'", [], |r| r.get(0),
    ).unwrap();
    assert_eq!(host_expiry, None);
    assert_eq!(identity_cookie(&h, &cookie, &db).await, expires);
    let phc: String = db
        .query_row("SELECT password_phc FROM accounts", [], |r| r.get(0))
        .unwrap();
    assert!(phc.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
    let mut q = db.prepare("SELECT hash FROM credentials").unwrap();
    for d in q.query_map([], |r| r.get::<_, Vec<u8>>(0)).unwrap() {
        let d = d.unwrap();
        assert_eq!(d.len(), 32);
        assert_ne!(d, host.as_bytes());
        assert_ne!(d, secret(&cookie).as_bytes());
    }
    db.execute(
        "UPDATE credentials SET expires_at=0 WHERE kind='session'",
        [],
    )
    .unwrap();
    rejected_identity(&h, &cookie).await;
    assert_eq!(session_expiry(&db), 0);
    assert!(h.service.validate_hostkey(&host).await.is_ok());
}

#[tokio::test]
async fn identity_before_daily_boundary_reads_without_a_writer_and_sets_remaining_cookie_age() {
    let h = Harness::new().await;
    h.setup().await;
    let cookie = h.login("root.admin", PASSWORD).await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let expires = timestamp() + 364 * 86400 + 3600;
    db.execute("UPDATE credentials SET expires_at=?1 WHERE kind='session'", [expires]).unwrap();
    let version: i64 = db.pragma_query_value(None, "data_version", |r| r.get(0)).unwrap();
    // A normal identity read must succeed even while another connection holds
    // the writer. This also catches an unnecessary no-op renewal transaction.
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert_eq!(identity_cookie(&h, &cookie, &db).await, expires);
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(db.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0)).unwrap(), version);
}

#[tokio::test]
async fn identity_upgrades_legacy_and_rolls_daily_from_now_with_repeat_cookie_and_persistence() {
    let h = Harness::new().await;
    h.setup().await;
    let cookie = h.login("root.admin", PASSWORD).await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let mut renewed = 0;
    for remaining in [86400, 364 * 86400] {
        let old_expiry = timestamp() + remaining;
        db.execute("UPDATE credentials SET expires_at=?1 WHERE kind='session'", [old_expiry]).unwrap();
        // Other callers of Service::me must retain read-only semantics.
        let authority = h.service.browser_authority(secret(&cookie)).await.unwrap();
        assert_eq!(h.service.me(authority).await.unwrap().username, "root.admin");
        assert_eq!(session_expiry(&db), old_expiry);
        let before = timestamp();
        renewed = identity_cookie(&h, &cookie, &db).await;
        let after = timestamp();
        assert!((before + 31_536_000..=after + 31_536_000).contains(&renewed));
        assert!(renewed > old_expiry);
        // Even if the renewal response was lost, the original cookie gets the
        // authoritative remaining age again without a further extension.
        assert_eq!(identity_cookie(&h, &cookie, &db).await, renewed);
        assert_eq!(db.query_row("SELECT count(*) FROM credentials", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }
    drop(db);
    let h = Harness::reopen(h.stop().await).await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    assert_eq!(identity_cookie(&h, &cookie, &db).await, renewed);
}

#[tokio::test]
async fn identity_never_renews_expired_revoked_disabled_or_stale_generation_sessions() {
    let h = Harness::new().await;
    h.setup().await;
    let cookie = h.login("root.admin", PASSWORD).await;
    let host = h.service.native_login(IP, "root.admin", PASSWORD.into()).await.unwrap().1;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    for expires in [0, timestamp() - 1, timestamp()] {
        db.execute("UPDATE credentials SET expires_at=?1 WHERE kind='session'", [expires]).unwrap();
        rejected_identity(&h, &cookie).await;
        assert_eq!(session_expiry(&db), expires);
    }
    let expires = timestamp() + 3600;
    db.execute("UPDATE credentials SET expires_at=?1 WHERE kind='session'", [expires]).unwrap();
    db.execute("UPDATE accounts SET enabled=0", []).unwrap();
    rejected_identity(&h, &cookie).await;
    assert_eq!(session_expiry(&db), expires);
    db.execute("UPDATE accounts SET enabled=1", []).unwrap();
    db.execute("UPDATE credentials SET generation=generation+1 WHERE kind='session'", []).unwrap();
    rejected_identity(&h, &cookie).await;
    assert_eq!(session_expiry(&db), expires);
    db.execute("UPDATE credentials SET generation=generation-1 WHERE kind='session'", []).unwrap();
    let r = h.post("/api/logout", json!({}), Some(&cookie)).await;
    assert_eq!(r.status(), 200);
    assert!(r.headers()["set-cookie"].to_str().unwrap().contains("Max-Age=0"));
    rejected_identity(&h, &cookie).await;
    assert_eq!(session_expiry(&db), expires);
    assert!(h.service.validate_hostkey(&host).await.is_ok());
}
#[tokio::test]
async fn login_verifies_original_password_despite_current_creation_policy() {
    let h = Harness::new().await;
    h.setup().await;
    let phc = tokio::task::spawn_blocking(|| crypto::hash("old").unwrap())
        .await
        .unwrap();
    let db = rusqlite::Connection::open(h.database()).unwrap();
    db.execute(
        "UPDATE accounts SET password_phc=?1,password_revision=password_revision+1",
        [phc],
    )
    .unwrap();
    drop(db);
    h.login("root.admin", "old").await;
    error(
        h.post(
            "/api/login",
            json!({"username":"unknown","password":"old"}),
            None,
        )
        .await,
        401,
        "authentication_failed",
    )
    .await;
    error(
        h.post(
            "/api/login",
            json!({"username":"root.admin","password":"OLD"}),
            None,
        )
        .await,
        401,
        "authentication_failed",
    )
    .await;
}
#[tokio::test]
async fn untrusted_spoof_does_not_change_ip_budget_and_names_are_canonical() {
    let h = Harness::new().await;
    for n in 0..10 {
        let r = h
            .client
            .post(format!("{}/api/login", h.base))
            .header("origin", ORIGIN)
            .header("x-forwarded-for", format!("192.0.2.{n}"))
            .header("x-forwarded-proto", "https")
            .json(&json!({"username":if n%2==0 {" Guessed "}else{"GUESSED"},"password":"a"}))
            .send()
            .await
            .unwrap();
        error(r, 401, "authentication_failed").await;
    }
    let r = h
        .post(
            "/api/login",
            json!({"username":"guessed","password":"a"}),
            None,
        )
        .await;
    assert!(r.headers().contains_key("retry-after"));
    error(r, 429, "rate_limited").await;
    for n in 0..10 {
        error(
            h.post(
                "/api/login",
                json!({"username":format!("other{n}"),"password":"a"}),
                None,
            )
            .await,
            401,
            "authentication_failed",
        )
        .await;
    }
    error(
        h.post(
            "/api/login",
            json!({"username":"more","password":"a"}),
            None,
        )
        .await,
        429,
        "rate_limited",
    )
    .await;
}
#[tokio::test]
async fn peer_trust_uses_socket_identity_and_never_changes_url() {
    let h = Harness::new().await;
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("x-forwarded-for", "192.0.2.2".parse().unwrap());
    headers.insert("x-forwarded-proto", "https".parse().unwrap());
    let socket = "127.0.0.1:1234".parse().unwrap();
    assert_eq!(
        meguri_sync_server::httpapi::effective_peer(&headers, socket, &h.service),
        (IP, false)
    );
    // Independent config plus service instance, still owned by a real root.
    let temp = h.stop().await;
    let root = meguri_sync_server::DataRoot::acquire(&temp.path().join("owner"), false).unwrap();
    let mut config = Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap();
    config.trusted_peers.insert(IP);
    let service = meguri_sync_server::Service::open(root, config)
        .await
        .unwrap();
    assert_eq!(
        meguri_sync_server::httpapi::effective_peer(&headers, socket, &service),
        ("192.0.2.2".parse().unwrap(), true)
    );
    assert_eq!(service.config.server_url(), format!("{ORIGIN}/"));
    headers.append("x-forwarded-for", "192.0.2.3".parse().unwrap());
    assert_eq!(
        meguri_sync_server::httpapi::effective_peer(&headers, socket, &service).0,
        IP
    );
}
