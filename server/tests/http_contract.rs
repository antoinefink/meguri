// SPDX-License-Identifier: MIT
//! Ordinary, finite single-request transport boundaries. No connection/load driver.
mod support;
use reqwest::Response;
use serde_json::{Value, json};
use support::*;

fn headers(r: &Response) {
    for (name, expected) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        ("x-frame-options", "DENY"),
        ("x-content-type-options", "nosniff"),
    ] {
        assert_eq!(r.headers()[name], expected);
    }
    let csp = r.headers()["content-security-policy"].to_str().unwrap();
    for directive in [
        "default-src 'self'",
        "object-src 'none'",
        "base-uri 'none'",
        "frame-ancestors 'none'",
        "form-action 'self'",
    ] {
        assert!(csp.contains(directive));
    }
    assert!(!csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"));
    assert!(!r.headers().contains_key("access-control-allow-origin"));
    assert!(!r.headers().contains_key("access-control-allow-credentials"));
}
fn session_cookie(r: &Response, clear: bool) -> String {
    headers(r);
    let c = r.headers()["set-cookie"].to_str().unwrap();
    assert!(c.starts_with("__Host-meguri-session="));
    let attributes = c.split(';').map(str::trim).collect::<Vec<_>>();
    for a in [
        "Secure",
        "HttpOnly",
        "SameSite=Strict",
        "Path=/",
    ] {
        assert!(attributes.contains(&a));
    }
    assert!(!c.to_ascii_lowercase().contains("domain="));
    let age: i64 = attributes.iter().find_map(|a| a.strip_prefix("Max-Age="))
        .unwrap().parse().unwrap();
    if clear {
        assert_eq!(age, 0);
    } else {
        // Login's response may cross a UTC second after issuance.
        assert!((31_535_999..=31_536_000).contains(&age));
    }
    c.split(';').next().unwrap().into()
}
async fn failure(r: Response, status: u16, code: &str) {
    headers(&r);
    assert!(!r.headers().contains_key("set-cookie"));
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    error(r, status, code).await;
}
async fn raw(
    h: &Harness,
    path: &str,
    body: impl Into<reqwest::Body>,
    cookie: Option<&str>,
) -> Response {
    let mut request = h
        .client
        .post(format!("{}{path}", h.base))
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .body(body);
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    request.send().await.unwrap()
}
fn mutations(id: &str) -> Vec<(String, Value)> {
    vec![
        (
            "/api/setup".into(),
            json!({"token":"invalid","username":"test.user","password":PASSWORD}),
        ),
        (
            "/api/invitations/claim".into(),
            json!({"token":"invalid","username":"test.user","password":PASSWORD}),
        ),
        (
            "/api/login".into(),
            json!({"username":"unknown","password":PASSWORD}),
        ),
        ("/api/logout".into(), json!({})),
        (
            "/api/password".into(),
            json!({"current_password":"wrong","new_password":NEW_PASSWORD}),
        ),
        ("/api/admin/invitations".into(), json!({})),
        (format!("/api/admin/accounts/{id}/reset"), json!({})),
        (format!("/api/admin/accounts/{id}/disable"), json!({})),
        (
            "/api/resets/claim".into(),
            json!({"token":"invalid","new_password":NEW_PASSWORD}),
        ),
    ]
}

#[tokio::test]
async fn compiled_navigation_and_assets_use_exact_get_head_routes_without_claiming_setup() {
    let h = Harness::new().await;
    let mut shell = None;
    for path in meguri_sync_server::webassets::NAVIGATION {
        let response = h.get(path, None).await;
        headers(&response);
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "text/html; charset=utf-8");
        assert!(!response.headers().contains_key("set-cookie"));
        let length = response.headers()["content-length"].clone();
        let html = response.text().await.unwrap();
        assert!(!html.contains(&h.setup_token));
        assert_eq!(length.to_str().unwrap().parse::<usize>().unwrap(), html.len());
        if let Some(expected) = &shell { assert_eq!(&html, expected); }
        else { shell = Some(html); }
        let head = h.client.head(format!("{}{path}", h.base)).send().await.unwrap();
        headers(&head);
        assert_eq!(head.status(), 200);
        assert_eq!(head.headers()["content-length"], length);
        assert_eq!(head.headers()["content-type"], "text/html; charset=utf-8");
        assert!(head.bytes().await.unwrap().is_empty());
    }
    let shell = shell.unwrap();
    let paths: Vec<_> = shell.split('"').filter(|value| value.starts_with("/assets/")).collect();
    assert!(paths.iter().any(|path| path.ends_with(".js")));
    assert!(paths.iter().any(|path| path.ends_with(".css")));
    assert!(paths.iter().any(|path| path.ends_with(".png")));
    for path in paths {
        let response = h.get(path, None).await;
        headers(&response);
        assert_eq!(response.status(), 200);
        let mime = match path.rsplit_once('.').unwrap().1 {
            "js" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            _ => "image/png",
        };
        assert_eq!(response.headers()["content-type"], mime);
        let length = response.headers()["content-length"].clone();
        let bytes = response.bytes().await.unwrap();
        assert!(!bytes.is_empty());
        assert_ne!(bytes.as_ref(), shell.as_bytes());
        assert_eq!(length.to_str().unwrap().parse::<usize>().unwrap(), bytes.len());
        let head = h.client.head(format!("{}{path}", h.base)).send().await.unwrap();
        headers(&head);
        assert_eq!(head.status(), 200);
        assert_eq!(head.headers()["content-type"], mime);
        assert_eq!(head.headers()["content-length"], length);
        assert!(head.bytes().await.unwrap().is_empty());
        failure(h.get(&format!("{path}?x=1"), None).await, 404, "route_not_found").await;
    }
    for path in ["/unknown", "/index.html", "/accounts/", "/assets/main.js", "/assets/missing-abcdefgh.js", "/setup?token=invalid", "/api/unknown"] {
        failure(h.get(path, None).await, 404, "route_not_found").await;
    }
    for method in [reqwest::Method::POST, reqwest::Method::OPTIONS] {
        failure(h.client.request(method, format!("{}/setup", h.base)).send().await.unwrap(), 404, "route_not_found").await;
    }
    assert_eq!(h.get("/api/status", None).await.json::<Value>().await.unwrap()["data"]["setup_required"], true);
    h.setup().await;
    h.stop().await;
}

#[tokio::test]
async fn every_mutation_requires_an_object_without_mutation() {
    let h = Harness::new().await;
    for (path, object) in mutations("valid-member-shaped-target") {
        // Positional derived-struct forms must never redeem capabilities or
        // perform even an empty mutation. Include an array with all field values.
        let positional = Value::Array(object.as_object().unwrap().values().cloned().collect());
        for root in [
            json!([]),
            positional,
            json!(null),
            json!(true),
            json!(3),
            json!("{}"),
        ] {
            failure(h.post(&path, root, None).await, 400, "invalid_request").await;
        }
        // Unknown and duplicate known members must still use the original bytes.
        let mut extra = object.clone();
        extra
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(1));
        failure(h.post(&path, extra, None).await, 400, "invalid_request").await;
        if let Some((name, value)) = object.as_object().unwrap().iter().next() {
            let serialized = serde_json::to_string(&object).unwrap();
            let duplicate = format!(
                "{{{}:{},{}",
                serde_json::to_string(name).unwrap(),
                value,
                &serialized[1..]
            );
            failure(
                raw(&h, &path, duplicate, None).await,
                400,
                "invalid_request",
            )
            .await;
        }
    }
    let r = h.get("/api/status", None).await;
    headers(&r);
    assert_eq!(
        r.json::<Value>().await.unwrap()["data"]["setup_required"],
        true
    );
    let r = h
        .post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"root.admin","password":PASSWORD}),
            None,
        )
        .await;
    headers(&r);
    assert_eq!(r.status(), 201);
    assert!(!r.headers().contains_key("set-cookie"));
    h.stop().await; // Original setup token remains usable after every rejection.
}

#[tokio::test]
async fn typed_login_exact_byte_boundaries_preserve_existing_passwords() {
    let h = Harness::new().await;
    h.setup().await;
    let prefix = r#"{"username":"root.admin","password":""#;
    let suffix = "\"}";
    for length in [16383, 16384, 16385] {
        // Include leading/trailing spaces: login must verify supplied bytes even
        // though these restored passwords exceed current creation policy.
        let password = format!(" {} ", "x".repeat(length - prefix.len() - suffix.len() - 2));
        let body = format!("{prefix}{password}{suffix}");
        assert_eq!(body.len(), length);
        if length <= 16384 {
            let password_for_hash = password.clone();
            let hash = tokio::task::spawn_blocking(move || {
                meguri_sync_server::crypto::hash(&password_for_hash).unwrap()
            })
            .await
            .unwrap();
            let db = rusqlite::Connection::open(h.database()).unwrap();
            db.execute(
                "UPDATE accounts SET password_phc=?1,password_revision=password_revision+1",
                [hash],
            )
            .unwrap();
            drop(db);
            let r = raw(&h, "/api/login", body, None).await;
            assert_eq!(r.status(), 200);
            session_cookie(&r, false);
            assert_eq!(
                r.json::<Value>().await.unwrap()["data"]["username"],
                "root.admin"
            );
        } else {
            failure(
                raw(&h, "/api/login", body, None).await,
                413,
                "request_too_large",
            )
            .await;
        }
    }
    h.stop().await;
}

#[tokio::test]
async fn utf8_typed_fields_depth_and_content_type_errors_are_json() {
    let h = Harness::new().await;
    for body in [
        vec![0xff],
        b"{} trailing".to_vec(),
        b"{".to_vec(),
        br#"{"username":3,"password":"x"}"#.to_vec(),
        br#"{"username":"abc"}"#.to_vec(),
        br#"{"username":"abc","password":null}"#.to_vec(),
        format!(
            "{{\"username\":{},\"password\":\"x\"}}",
            format!("{}0{}", "[".repeat(16), "]".repeat(16))
        )
        .into_bytes(),
    ] {
        failure(
            raw(&h, "/api/login", body, None).await,
            400,
            "invalid_request",
        )
        .await;
    }
    for mime in [
        "text/plain",
        "application/json; charset=latin1",
        "application/json; charset=utf-8; x=y",
    ] {
        let r = h
            .client
            .post(format!("{}/api/login", h.base))
            .header("origin", ORIGIN)
            .header("content-type", mime)
            .body("{}")
            .send()
            .await
            .unwrap();
        failure(r, 415, "unsupported_content_type").await;
    }
    let r = h
        .client
        .post(format!("{}/api/login", h.base))
        .header("origin", ORIGIN)
        .header("content-type", "application/json")
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    failure(r, 415, "unsupported_content_type").await;
    let r = h
        .client
        .post(format!("{}/api/login", h.base))
        .header("origin", ORIGIN)
        .body("{}")
        .send()
        .await
        .unwrap();
    failure(r, 415, "unsupported_content_type").await;
    // Brackets/escapes inside strings are data, not containers.
    let r = h
        .client
        .post(format!("{}/api/login", h.base))
        .header("origin", ORIGIN)
        .header("content-type", "Application/JSON; Charset=\"UTF-8\"")
        .body(r#"{"username":"unknown","password":"[[[\\\"{}]]]"}"#)
        .send()
        .await
        .unwrap();
    failure(r, 401, "authentication_failed").await;
    h.stop().await;
}

#[tokio::test]
async fn unsafe_origin_precedes_all_forms_for_anonymous_and_authenticated_callers() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "test.member").await;
    let member_cookie = h.login("test.member", PASSWORD).await;
    for cookie in [None, Some(admin.as_str()), Some(member_cookie.as_str())] {
        for (path, _) in mutations(member["id"].as_str().unwrap()) {
            for origins in [
                vec![],
                vec!["null"],
                vec!["https://other.test"],
                vec!["malformed"],
                vec![ORIGIN, ORIGIN],
                vec![ORIGIN, "https://other.test"],
            ] {
                let mut r = h
                    .client
                    .post(format!("{}{path}", h.base))
                    .header("host", "spoof.invalid")
                    .header("forwarded", "host=spoof.invalid;proto=http;for=192.0.2.1")
                    .header("x-forwarded-host", "spoof.invalid")
                    .header("x-forwarded-proto", "https")
                    .header("content-type", "text/plain")
                    .body("invalid JSON");
                for origin in origins {
                    r = r.header("origin", origin);
                }
                if let Some(cookie) = cookie {
                    r = r.header("cookie", cookie);
                }
                failure(r.send().await.unwrap(), 403, "origin_rejected").await;
            }
        }
    }
    // The same member target remains enabled and its administrator can still act.
    let r = h.get("/api/me", Some(&member_cookie)).await;
    assert_eq!(r.status(), 200);
    let reset = h
        .post(
            &format!(
                "/api/admin/accounts/{}/reset",
                member["id"].as_str().unwrap()
            ),
            json!({}),
            Some(&admin),
        )
        .await;
    headers(&reset);
    assert_eq!(reset.status(), 201);
    assert!(
        reset.json::<Value>().await.unwrap()["data"]["url"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{ORIGIN}/reset#token="))
    );
    h.stop().await;
}

#[tokio::test]
async fn anonymous_and_member_authorization_matrix_uses_valid_member_targets() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "test.member").await;
    let member_cookie = h.login("test.member", PASSWORD).await;
    let id = member["id"].as_str().unwrap();
    for (cookie, expected) in [(None, 401), (Some(member_cookie.as_str()), 403)] {
        let code = if cookie.is_none() {
            "authentication_failed"
        } else {
            "permission_denied"
        };
        failure(h.get("/api/admin/accounts", cookie).await, expected, code).await;
        for path in [
            "/api/admin/invitations".into(),
            format!("/api/admin/accounts/{id}/reset"),
            format!("/api/admin/accounts/{id}/disable"),
        ] {
            failure(h.post(&path, json!({}), cookie).await, expected, code).await;
        }
    }
    for (path, body) in mutations(id) {
        if path.contains("/admin/") {
            continue;
        }
        let (status, code) = match path.as_str() {
            "/api/setup" | "/api/invitations/claim" | "/api/resets/claim" => {
                (403, "capability_invalid")
            }
            _ => (401, "authentication_failed"),
        };
        failure(h.post(&path, body.clone(), None).await, status, code).await;
        if matches!(
            path.as_str(),
            "/api/setup" | "/api/invitations/claim" | "/api/resets/claim" | "/api/login"
        ) {
            failure(
                h.post(&path, body, Some(&member_cookie)).await,
                status,
                code,
            )
            .await;
        }
    }
    failure(h.get("/api/me", None).await, 401, "authentication_failed").await;
    for path in ["/api/status", "/api/me"] {
        let r = h.get(path, Some(&member_cookie)).await;
        headers(&r);
        assert_eq!(r.status(), 200);
    }
    let r = h.get("/api/admin/accounts", Some(&admin)).await;
    headers(&r);
    assert_eq!(r.status(), 200);
    let r = h
        .post(
            "/api/password",
            json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD}),
            Some(&member_cookie),
        )
        .await;
    assert_eq!(r.status(), 200);
    session_cookie(&r, true);
    assert!(r.json::<Value>().await.unwrap()["data"].is_null());
    let member_cookie = h.login("test.member", NEW_PASSWORD).await;
    let r = h.post("/api/logout", json!({}), Some(&member_cookie)).await;
    assert_eq!(r.status(), 200);
    session_cookie(&r, true);
    assert!(r.json::<Value>().await.unwrap()["data"].is_null());
    // Administrator success targets a member, never the protected sole admin.
    let r = h
        .post(
            &format!("/api/admin/accounts/{id}/disable"),
            json!({}),
            Some(&admin),
        )
        .await;
    headers(&r);
    assert_eq!(r.status(), 200);
    h.stop().await;
}

#[tokio::test]
async fn configured_origin_and_urls_ignore_host_forwarding_native_is_independent() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    for path in ["/api/me", "/api/status"] {
        let r = h
            .client
            .get(format!("{}{path}", h.base))
            .header("cookie", &admin)
            .header("host", "spoof.invalid")
            .header("forwarded", "host=spoof.invalid;proto=http")
            .header("x-forwarded-host", "spoof.invalid")
            .send()
            .await
            .unwrap();
        headers(&r);
        assert_eq!(r.status(), 200);
        let value = r.json::<Value>().await.unwrap();
        if path == "/api/me" {
            assert_eq!(value["data"]["server_url"], format!("{ORIGIN}/"));
        }
    }
    let r = h
        .client
        .post(format!("{}/api/admin/invitations", h.base))
        .header("origin", ORIGIN)
        .header("cookie", &admin)
        .header("host", "spoof.invalid")
        .header("x-forwarded-host", "spoof.invalid")
        .header("forwarded", "host=spoof.invalid;proto=http")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    headers(&r);
    assert_eq!(r.status(), 201);
    assert!(
        r.json::<Value>().await.unwrap()["data"]["url"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{ORIGIN}/invite#token="))
    );
    for path in ["/sync/", "/msync/", "/sync/login", "/msync/begin"] {
        for origin in [None, Some("null")] {
            let mut r = h
                .client
                .post(format!("{}{path}", h.base))
                .header("cookie", "__Host-meguri-session=invalid")
                .body("native placeholder");
            if let Some(origin) = origin {
                r = r.header("origin", origin);
            }
            let r = r.send().await.unwrap();
            let registered=path=="/msync/begin";
            assert_eq!(r.status(), if registered {400} else {404});
            headers(&r);
            assert_eq!(r.text().await.unwrap(), if registered {"invalid_request"} else {"route_not_found"});
        }
    }
    for path in ["/api/unknown", "/api/missing.js"] {
        failure(h.get(path, None).await, 404, "route_not_found").await;
    }
    h.service.close_admission().await.unwrap();
    failure(
        h.post(
            "/api/login",
            json!({"username":"root.admin","password":PASSWORD}),
            None,
        )
        .await,
        503,
        "admission_closed",
    )
    .await;
    failure(h.get("/health", None).await, 503, "admission_closed").await;
    h.stop().await;
}

#[tokio::test]
async fn capability_success_objects_have_security_headers_without_automatic_cookies() {
    let h = Harness::new().await;
    let r = h
        .post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"root.admin","password":PASSWORD}),
            None,
        )
        .await;
    headers(&r);
    assert_eq!(r.status(), 201);
    assert!(!r.headers().contains_key("set-cookie"));
    let admin = h.login("root.admin", PASSWORD).await;
    let invite = h.invite(&admin).await;
    let r = h
        .post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"test.member","password":PASSWORD}),
            None,
        )
        .await;
    headers(&r);
    assert_eq!(r.status(), 201);
    assert!(!r.headers().contains_key("set-cookie"));
    let id = r.json::<Value>().await.unwrap()["data"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let reset = h.reset_link(&admin, &id).await;
    let r = h
        .post(
            "/api/resets/claim",
            json!({"token":reset,"new_password":NEW_PASSWORD}),
            None,
        )
        .await;
    headers(&r);
    assert_eq!(r.status(), 200);
    assert!(!r.headers().contains_key("set-cookie"));
    assert!(r.json::<Value>().await.unwrap()["data"].is_null());
    h.login("test.member", NEW_PASSWORD).await;
    h.stop().await;
}
