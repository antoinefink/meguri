// SPDX-License-Identifier: MIT
mod support;
use serde_json::{Value, json};
use support::*;

#[tokio::test]
async fn actual_http_setup_invite_login_admin_me_logout_and_restart() {
    let h = Harness::new().await;
    let status = h
        .get("/api/status", None)
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(status["data"]["setup_required"], true);
    let admin_identity = h.setup().await;
    assert_eq!(admin_identity["username"], "root.admin");
    let admin = h.login("ROOT.ADMIN", PASSWORD).await;
    let member = h.member(&admin, "member.one").await;
    let member_cookie = h.login(" MEMBER.ONE ", PASSWORD).await;
    let other_member_session = h.login("member.one", PASSWORD).await;
    let member_host = h
        .service
        .native_login(IP, "member.one", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let me = h
        .get("/api/me", Some(&member_cookie))
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(me["data"]["server_url"], format!("{ORIGIN}/"));
    assert_eq!(me["data"]["id"], member["id"]);
    error(
        h.get("/api/admin/accounts", Some(&member_cookie)).await,
        403,
        "permission_denied",
    )
    .await;
    error(
        h.post("/api/admin/invitations", json!({}), Some(&member_cookie))
            .await,
        403,
        "permission_denied",
    )
    .await;
    let list = h
        .get("/api/admin/accounts", Some(&admin))
        .await
        .json::<Value>()
        .await
        .unwrap();
    let accounts = list["data"]["accounts"].as_array().unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0]["username"], "member.one");
    assert_eq!(accounts[1]["role"], "admin");
    for a in accounts {
        assert_eq!(a.as_object().unwrap().len(), 6);
        let storage = &a["storage"];
        assert_eq!(storage.as_object().unwrap().len(), 3);
        assert!(storage["collection_bytes"].as_u64().unwrap() > 0);
        assert_eq!(storage["media_bytes"], 0);
        assert_eq!(storage["total_bytes"], storage["collection_bytes"]);
    }
    let native = h
        .service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap()
        .1;
    assert!(h.service.validate_hostkey(&native).await.is_ok());
    error(
        h.get("/api/me", Some(&format!("__Host-meguri-session={native}")))
            .await,
        401,
        "authentication_failed",
    )
    .await;
    assert!(h.service.validate_hostkey(secret(&admin)).await.is_err());
    let r = h.post("/api/logout", json!({}), Some(&member_cookie)).await;
    assert_eq!(r.status(), 200);
    assert!(
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(
        h.post("/api/logout", json!({}), Some(&member_cookie))
            .await
            .status(),
        200
    );
    error(
        h.get("/api/me", Some(&member_cookie)).await,
        401,
        "authentication_failed",
    )
    .await;
    assert_eq!(h.get("/api/me", Some(&admin)).await.status(), 200);
    assert_eq!(
        h.get("/api/me", Some(&other_member_session)).await.status(),
        200
    );
    assert!(h.service.validate_hostkey(&member_host).await.is_ok());
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let revoked: Vec<(Vec<u8>, bool)> = db
        .prepare("SELECT hash,revoked FROM credentials WHERE account_id=?1")
        .unwrap()
        .query_map([member["id"].as_str().unwrap()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(revoked.len(), 3);
    for (hash, revoked) in revoked {
        assert_eq!(
            revoked,
            hash.as_slice()
                == meguri_sync_server::crypto::bearer(secret(&member_cookie))
                    .unwrap()
                    .as_slice()
        );
    }
    drop(db);
    let h = Harness::reopen(h.stop().await).await;
    assert_eq!(
        h.get("/api/status", None)
            .await
            .json::<Value>()
            .await
            .unwrap()["data"]["setup_required"],
        false
    );
    assert_eq!(h.get("/api/me", Some(&admin)).await.status(), 200);
    assert!(h.service.validate_hostkey(&native).await.is_ok());
}
#[tokio::test]
async fn reset_replaces_links_revokes_both_credential_kinds_and_retains_disabled_data() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let id = member["id"].as_str().unwrap();
    let member_cookie = h.login("member", PASSWORD).await;
    let host = h
        .service
        .native_login(IP, "member", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let sentinel = h.path().join("state/accounts").join(id).join("media/kept");
    std::fs::write(&sentinel, b"private synthetic media").unwrap();
    let old = h.reset_link(&admin, id).await;
    let current = h.reset_link(&admin, id).await;
    error(
        h.post(
            "/api/resets/claim",
            json!({"token":old,"new_password":NEW_PASSWORD}),
            None,
        )
        .await,
        403,
        "capability_invalid",
    )
    .await;
    assert_eq!(
        h.post(
            "/api/resets/claim",
            json!({"token":current,"new_password":NEW_PASSWORD}),
            None
        )
        .await
        .status(),
        200
    );
    error(
        h.get("/api/me", Some(&member_cookie)).await,
        401,
        "authentication_failed",
    )
    .await;
    assert!(h.service.validate_hostkey(&host).await.is_err());
    let cookie = h.login("member", NEW_PASSWORD).await;
    assert_eq!(
        h.post(
            &format!("/api/admin/accounts/{id}/disable"),
            json!({}),
            Some(&admin)
        )
        .await
        .status(),
        200
    );
    error(
        h.get("/api/me", Some(&cookie)).await,
        401,
        "authentication_failed",
    )
    .await;
    let reset = h.reset_link(&admin, id).await;
    assert_eq!(
        h.post(
            "/api/resets/claim",
            json!({"token":reset,"new_password":PASSWORD}),
            None
        )
        .await
        .status(),
        200
    );
    error(
        h.post(
            "/api/login",
            json!({"username":"member","password":PASSWORD}),
            None,
        )
        .await,
        401,
        "authentication_failed",
    )
    .await;
    assert_eq!(
        std::fs::read(&sentinel).unwrap(),
        b"private synthetic media"
    );
    let list = h
        .get("/api/admin/accounts", Some(&admin))
        .await
        .json::<Value>()
        .await
        .unwrap();
    let m = &list["data"]["accounts"][0];
    assert_eq!(m["enabled"], false);
    assert_eq!(m["role"], "member");
    assert_eq!(m["id"], id);
    assert!(m["storage"]["collection_bytes"].as_u64().unwrap() > 0);
    assert_eq!(m["storage"]["media_bytes"], 0); // Unreferenced sentinel is excluded.
}
#[tokio::test]
async fn sole_admin_password_change_revocation_and_logout_idempotence() {
    let h = Harness::new().await;
    let a = h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    error(
        h.post(
            &format!("/api/admin/accounts/{}/disable", a["id"].as_str().unwrap()),
            json!({}),
            Some(&admin),
        )
        .await,
        409,
        "sole_administrator",
    )
    .await;
    let before = account_state(&h, a["id"].as_str().unwrap());
    let stores = store_bytes(&h.path().join("state/accounts"));
    error(
        h.post(
            &format!("/api/admin/accounts/{}/reset", a["id"].as_str().unwrap()),
            json!({}),
            Some(&admin),
        )
        .await,
        409,
        "sole_administrator",
    )
    .await;
    assert_eq!(account_state(&h, a["id"].as_str().unwrap()), before);
    assert_eq!(store_bytes(&h.path().join("state/accounts")), stores);
    let pending = h.invite(&admin).await;
    let host = h
        .service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let r = h
        .post(
            "/api/password",
            json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD}),
            Some(&admin),
        )
        .await;
    assert_eq!(r.status(), 200);
    assert!(
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert!(h.service.validate_hostkey(&host).await.is_err());
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":pending,"username":"lost","password":PASSWORD}),
            None,
        )
        .await,
        403,
        "capability_invalid",
    )
    .await;
    error(
        h.get("/api/me", Some(&admin)).await,
        401,
        "authentication_failed",
    )
    .await;
    assert_eq!(
        h.post("/api/logout", json!({}), Some(&admin))
            .await
            .status(),
        200
    );
    h.login("root.admin", NEW_PASSWORD).await;
}
#[tokio::test]
async fn local_recovery_preserves_data_and_never_reopens_restore_admission() {
    let h = Harness::new().await;
    let a = h.setup().await;
    let id = a["id"].as_str().unwrap().to_owned();
    let cookie = h.login("root.admin", PASSWORD).await;
    let pending = h.invite(&cookie).await;
    let host = h
        .service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let stores = h.path().join("state/accounts");
    std::fs::write(
        stores.join(&id).join("media/sentinel"),
        b"synthetic recovery bytes\0\xff",
    )
    .unwrap();
    std::fs::write(
        stores.join(&id).join("generations/initial/sentinel"),
        b"synthetic collection bytes\0\xfe",
    )
    .unwrap();
    let bytes = store_bytes(&stores);
    let before = account_state(&h, &id);
    h.service.close_admission().await.unwrap();
    error(h.get("/api/status", None).await, 503, "admission_closed").await;
    let temp = h.stop().await;
    let path = temp.path().join("owner");
    let root = meguri_sync_server::DataRoot::acquire(&path, false).unwrap();
    meguri_sync_server::Service::recover_admin(root, id.clone(), NEW_PASSWORD.into(), true)
        .await
        .unwrap();
    let h = Harness::reopen(temp).await;
    assert!(!h.service.admission_open().await.unwrap());
    let after = account_state(&h, &id);
    assert_eq!((after.1, after.2), (before.1 + 1, before.2 + 1));
    assert_ne!(after.0, before.0);
    assert!(meguri_sync_server::crypto::verify(NEW_PASSWORD, &after.0));
    assert!(!meguri_sync_server::crypto::verify(PASSWORD, &after.0));
    assert_eq!(store_bytes(&stores), bytes);
    assert!(h.service.validate_hostkey(&host).await.is_err());
    error(
        h.post(
            "/api/login",
            json!({"username":"root.admin","password":NEW_PASSWORD}),
            None,
        )
        .await,
        503,
        "admission_closed",
    )
    .await;
    let temp = h.stop().await;
    let db = rusqlite::Connection::open(temp.path().join("owner/state/accounts.sqlite")).unwrap();
    let revoked: bool = db
        .query_row(
            "SELECT count(*)=0 FROM credentials WHERE revoked=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(revoked);
    let digest = meguri_sync_server::crypto::bearer(&pending).unwrap();
    let state: String = db
        .query_row(
            "SELECT state FROM capabilities WHERE hash=?1",
            [digest.as_slice()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "revoked");
}

fn account_state(h: &Harness, id: &str) -> (String, i64, i64) {
    rusqlite::Connection::open(h.database())
        .unwrap()
        .query_row(
            "SELECT password_phc,password_revision,generation FROM accounts WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

#[tokio::test]
async fn self_reset_requires_another_enabled_ready_admin_at_both_commits() {
    for ineligible in [false, true] {
        let h = Harness::new().await;
        let first = h.setup().await;
        let id = first["id"].as_str().unwrap();
        let admin = h.login("root.admin", PASSWORD).await;
        let (h, second) = h.existing_second_admin(&admin).await;
        let link = h.reset_link(&admin, id).await;
        let before = account_state(&h, id);
        let bytes = store_bytes(&h.path().join("state/accounts"));
        if ineligible {
            assert_eq!(
                h.post(
                    &format!(
                        "/api/admin/accounts/{}/disable",
                        second["id"].as_str().unwrap()
                    ),
                    json!({}),
                    Some(&admin)
                )
                .await
                .status(),
                200
            );
        }
        let r = h
            .post(
                "/api/resets/claim",
                json!({"token":link,"new_password":NEW_PASSWORD}),
                None,
            )
            .await;
        if ineligible {
            error(r, 409, "sole_administrator").await;
            assert_eq!(account_state(&h, id), before);
            assert_eq!(h.get("/api/me", Some(&admin)).await.status(), 200);
            let db = rusqlite::Connection::open(h.database()).unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT state FROM capabilities WHERE hash=?1",
                    [meguri_sync_server::crypto::bearer(&link)
                        .unwrap()
                        .as_slice()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "available"
            );
        } else {
            assert_eq!(r.status(), 200);
            h.login("root.admin", NEW_PASSWORD).await;
            assert_eq!(account_state(&h, id).2, before.2 + 1);
        }
        assert_eq!(store_bytes(&h.path().join("state/accounts")), bytes);
    }
}

#[tokio::test]
async fn capability_and_session_lifetimes_are_independent_and_exclusive() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let link = h.reset_link(&admin, member["id"].as_str().unwrap()).await;
    let invite = h.invite(&admin).await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    for (token, ttl) in [(&link, 1800), (&invite, 7 * 86400)] {
        let expiry: i64 = db
            .query_row(
                "SELECT expires_at FROM capabilities WHERE hash=?1",
                [meguri_sync_server::crypto::bearer(token)
                    .unwrap()
                    .as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        assert!((now + ttl - 10..=now + ttl).contains(&expiry));
    }
    // Signing out only the issuing session leaves generation-bound links valid.
    assert_eq!(
        h.post("/api/logout", json!({}), Some(&admin))
            .await
            .status(),
        200
    );
    assert_eq!(
        h.post(
            "/api/resets/claim",
            json!({"token":link,"new_password":NEW_PASSWORD}),
            None
        )
        .await
        .status(),
        200
    );
    error(
        h.post(
            "/api/resets/claim",
            json!({"token":link,"new_password":PASSWORD}),
            None,
        )
        .await,
        403,
        "capability_invalid",
    )
    .await;
    // Exclusive boundary at the current second: no sleep or live load.
    db.execute(
        "UPDATE capabilities SET expires_at=?1 WHERE hash=?2",
        rusqlite::params![
            time::OffsetDateTime::now_utc().unix_timestamp(),
            meguri_sync_server::crypto::bearer(&invite)
                .unwrap()
                .as_slice()
        ],
    )
    .unwrap();
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"expired","password":PASSWORD}),
            None,
        )
        .await,
        403,
        "capability_invalid",
    )
    .await;
    let replacement = h.login("root.admin", PASSWORD).await;
    assert_ne!(replacement, admin);
    assert_eq!(
        h.post("/api/logout", json!({}), Some(&admin))
            .await
            .status(),
        200
    );
    assert_eq!(h.get("/api/me", Some(&replacement)).await.status(), 200);
}

#[tokio::test]
async fn admin_storage_reads_committed_sqlite_pages_and_preserves_unmeasurable_rows() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let id = member["id"].as_str().unwrap();
    let path = h.path().join("state/accounts").join(id).join("generations/initial/collection.anki2");
    let c = meguri_sync_server::collection::schema::open(&path, true).unwrap();
    c.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    c.execute_batch("CREATE TABLE storage_fixture(value BLOB); INSERT INTO storage_fixture VALUES(zeroblob(200000));").unwrap();
    let pages: i64 = c.pragma_query_value(None, "page_count", |r| r.get(0)).unwrap();
    let size: i64 = c.pragma_query_value(None, "page_size", |r| r.get(0)).unwrap();
    let expected = u64::try_from(pages * size).unwrap();
    assert!(expected > std::fs::metadata(&path).unwrap().len(), "committed WAL pages must be counted");
    c.execute_batch("BEGIN; INSERT INTO storage_fixture VALUES(zeroblob(200000));").unwrap();
    let response = h.get("/api/admin/accounts", Some(&admin)).await;
    assert_eq!(response.status(), 200);
    let list = response.json::<Value>().await.unwrap();
    assert_eq!(list["data"]["accounts"][0]["storage"], json!({
        "collection_bytes": expected, "media_bytes": 0, "total_bytes": expected
    }));
    c.execute_batch("ROLLBACK;").unwrap();
    drop(c);
    // A bad active manifest affects only that row, not list authority/actions.
    let manifest = h.path().join("state/accounts").join(id).join("current-collection");
    let saved = std::fs::read(&manifest).unwrap();
    std::fs::write(&manifest, b"invalid synthetic manifest").unwrap();
    let response = h.get("/api/admin/accounts", Some(&admin)).await;
    assert_eq!(response.status(), 200);
    let list = response.json::<Value>().await.unwrap();
    assert_eq!(list["data"]["accounts"][0]["id"], id);
    assert!(list["data"]["accounts"][0]["storage"].is_null());
    assert!(list["data"]["accounts"][1]["storage"].is_object());
    error(h.get("/api/admin/accounts", None).await, 401, "authentication_failed").await;
    std::fs::write(&manifest, saved).unwrap();
    h.reset_link(&admin, id).await;
    h.stop().await;
}
