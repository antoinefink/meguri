// SPDX-License-Identifier: MIT
#![cfg(feature = "test-hooks")]
mod support;
use meguri_sync_server::{Result, accounts::RoundRetirer};
use serde_json::json;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};
use support::*;

#[tokio::test]
async fn old_browser_password_proof_cannot_issue_after_reset() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let reset = h.reset_link(&admin, member["id"].as_str().unwrap()).await;
    let pause = h.service.hooks.arm("login_after_proof");
    let client = h.client.clone();
    let base = h.base.clone();
    let login = tokio::spawn(async move {
        client
            .post(format!("{base}/api/login"))
            .header("origin", ORIGIN)
            .json(&json!({"username":"member","password":PASSWORD}))
            .send()
            .await
            .unwrap()
    });
    pause.arrived().await;
    assert_eq!(
        h.post(
            "/api/resets/claim",
            json!({"token":reset,"new_password":NEW_PASSWORD}),
            None
        )
        .await
        .status(),
        200
    );
    pause.resume();
    let r = login.await.unwrap();
    assert!(!r.headers().contains_key("set-cookie"));
    error(r, 401, "authentication_failed").await;
    h.login("member", NEW_PASSWORD).await;
}
#[tokio::test]
async fn old_native_proof_cannot_issue_after_disable() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let pause = h.service.hooks.arm("login_after_proof");
    let s = h.service.clone();
    let login = tokio::spawn(async move { s.native_login(IP, "member", PASSWORD.into()).await });
    pause.arrived().await;
    assert_eq!(
        h.post(
            &format!(
                "/api/admin/accounts/{}/disable",
                member["id"].as_str().unwrap()
            ),
            json!({}),
            Some(&admin)
        )
        .await
        .status(),
        200
    );
    pause.resume();
    assert_eq!(
        login.await.unwrap().unwrap_err().code,
        "authentication_failed"
    );
}
#[tokio::test]
async fn stale_admin_cannot_commit_either_capability_creation() {
    for reset_action in [false, true] {
        let h = Harness::new().await;
        let identity = h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let (h, _) = h.existing_second_admin(&admin).await;
        let reset = h.reset_link(&admin, identity["id"].as_str().unwrap()).await;
        let pause = h.service.hooks.arm("admin_before_commit");
        let client = h.client.clone();
        let base = h.base.clone();
        let original = admin.clone();
        let route = if reset_action {
            format!(
                "/api/admin/accounts/{}/reset",
                identity["id"].as_str().unwrap()
            )
        } else {
            "/api/admin/invitations".into()
        };
        let action = tokio::spawn(async move {
            client
                .post(format!("{base}{route}"))
                .header("origin", ORIGIN)
                .header("cookie", original)
                .json(&json!({}))
                .send()
                .await
                .unwrap()
        });
        pause.arrived().await;
        assert_eq!(
            h.post(
                "/api/resets/claim",
                json!({"token":reset,"new_password":NEW_PASSWORD}),
                None
            )
            .await
            .status(),
            200
        );
        pause.resume();
        error(action.await.unwrap(), 409, "stale_authority").await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM capabilities WHERE state='available'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}
#[tokio::test]
async fn competing_password_replacements_use_original_session_and_revision() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let pause = h.service.hooks.arm("password_after_proof");
    let client = h.client.clone();
    let base = h.base.clone();
    let original = admin.clone();
    let first = tokio::spawn(async move {
        client
            .post(format!("{base}/api/password"))
            .header("origin", ORIGIN)
            .header("cookie", original)
            .json(
                &json!({"current_password":PASSWORD,"new_password":"losing replacement password"}),
            )
            .send()
            .await
            .unwrap()
    });
    pause.arrived().await;
    assert_eq!(
        h.post(
            "/api/password",
            json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD}),
            Some(&admin)
        )
        .await
        .status(),
        200
    );
    pause.resume();
    error(first.await.unwrap(), 401, "authentication_failed").await;
    h.login("root.admin", NEW_PASSWORD).await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    assert_eq!(
        db.query_row("SELECT password_revision FROM accounts", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
}
#[tokio::test]
async fn reset_hash_cannot_bypass_target_generation_change() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let reset = h.reset_link(&admin, member["id"].as_str().unwrap()).await;
    let pause = h.service.hooks.arm("reset_after_hash");
    let client = h.client.clone();
    let base = h.base.clone();
    let claim = tokio::spawn(async move {
        client
            .post(format!("{base}/api/resets/claim"))
            .header("origin", ORIGIN)
            .json(&json!({"token":reset,"new_password":NEW_PASSWORD}))
            .send()
            .await
            .unwrap()
    });
    pause.arrived().await;
    assert_eq!(
        h.post(
            &format!(
                "/api/admin/accounts/{}/disable",
                member["id"].as_str().unwrap()
            ),
            json!({}),
            Some(&admin)
        )
        .await
        .status(),
        200
    );
    pause.resume();
    error(claim.await.unwrap(), 403, "capability_invalid").await;
}

struct Retire(Arc<AtomicBool>);
impl RoundRetirer for Retire {
    fn retire(&self) -> Result<()> {
        self.0.store(true, Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn publication_first_commits_before_revocation_acknowledges_and_retires_round() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let host = h
        .service
        .native_login(IP, "member", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let authority = h.service.validate_hostkey(&host).await.unwrap();
    let gate = h.service.publication_barrier(&authority).await.unwrap();
    let retired = Arc::new(AtomicBool::new(false));
    {
        let _g = gate.enter().await;
        gate.set_round(Some(Arc::new(Retire(retired.clone()))))
            .unwrap();
    }
    let pause = h.service.hooks.arm("publish_with_barrier");
    let published = Arc::new(AtomicBool::new(false));
    let p = published.clone();
    let s = h.service.clone();
    let publish = tokio::spawn(async move {
        s.publish(authority, move |_| {
            p.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
    });
    within(pause.arrived()).await;
    // Publication owns its target barrier but has not even opened a metadata
    // connection. Counterpart authority lookup must progress independently.
    let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
    let s = h.service.clone();
    let id = member["id"].as_str().unwrap().to_owned();
    let p = published.clone();
    let r = retired.clone();
    let disable = tokio::spawn(async move {
        s.disable(actor, id).await.unwrap();
        assert!(p.load(Ordering::SeqCst));
        assert!(r.load(Ordering::SeqCst));
    });
    within(gate.waiter_pending()).await;
    assert!(!retired.load(Ordering::SeqCst));
    assert!(!published.load(Ordering::SeqCst));
    pause.resume();
    within(publish).await.unwrap().unwrap();
    within(disable).await.unwrap();
    assert!(h.service.validate_hostkey(&host).await.is_err());
}
#[tokio::test]
async fn revocation_first_prevents_old_publication_callback() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let host = h
        .service
        .native_login(IP, "member", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let authority = h.service.validate_hostkey(&host).await.unwrap();
    let gate = h.service.publication_barrier(&authority).await.unwrap();
    let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
    let pause = h.service.hooks.arm("disable_with_barrier");
    let s = h.service.clone();
    let id = member["id"].as_str().unwrap().to_owned();
    let disable = tokio::spawn(async move { s.disable(actor, id).await });
    within(pause.arrived()).await;
    let called = Arc::new(AtomicBool::new(false));
    let c = called.clone();
    let s = h.service.clone();
    let publish = tokio::spawn(async move {
        s.publish(authority, move |_| {
            c.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await
    });
    within(gate.waiter_pending()).await;
    assert!(!called.load(Ordering::SeqCst));
    pause.resume();
    within(disable).await.unwrap().unwrap();
    assert_eq!(
        within(publish).await.unwrap().unwrap_err().code,
        "authentication_failed"
    );
    assert!(!called.load(Ordering::SeqCst));
}
#[tokio::test]
async fn cancelled_http_future_cannot_release_a_running_publication_barrier() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let host = h
        .service
        .native_login(IP, "member", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let authority = h.service.validate_hostkey(&host).await.unwrap();
    let gate = h.service.publication_barrier(&authority).await.unwrap();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let s = h.service.clone();
    let r = release.clone();
    let job = tokio::spawn(async move {
        s.publish(authority, move |_| {
            tx.send(()).unwrap();
            let (m, c) = &*r;
            let mut go = m.lock().unwrap();
            while !*go {
                go = c.wait(go).unwrap();
            }
            Ok(())
        })
        .await
    });
    rx.await.unwrap();
    job.abort();
    let _ = job.await;
    let entered = Arc::new(AtomicBool::new(false));
    let flag = entered.clone();
    let observed = gate.clone();
    let waiting = tokio::spawn(async move {
        let _guard = gate.enter().await;
        flag.store(true, Ordering::SeqCst);
    });
    within(observed.waiter_pending()).await;
    assert!(!entered.load(Ordering::SeqCst));
    *release.0.lock().unwrap() = true;
    release.1.notify_all();
    waiting.await.unwrap();
    assert!(entered.load(Ordering::SeqCst));
}

#[tokio::test]
async fn concurrent_cross_account_admin_actions_do_not_nest_target_barriers() {
    for second_first in [false, true] {
        let h = Harness::new().await;
        let first = h.setup().await;
        let first_id = first["id"].as_str().unwrap().to_owned();
        let first_cookie = h.login("root.admin", PASSWORD).await;
        let (h, second) = h.existing_second_admin(&first_cookie).await;
        let id = second["id"].as_str().unwrap().to_owned();
        let second_cookie = h.login("second.admin", PASSWORD).await;
        let first_pause = h.service.hooks.arm("disable_with_barrier");
        let actor = h
            .service
            .browser_authority(secret(&first_cookie))
            .await
            .unwrap();
        let s = h.service.clone();
        let a = tokio::spawn(async move { s.disable(actor, id).await });
        within(first_pause.arrived()).await;
        let second_pause = h.service.hooks.arm("disable_with_barrier");
        let actor = h
            .service
            .browser_authority(secret(&second_cookie))
            .await
            .unwrap();
        let s = h.service.clone();
        let b = tokio::spawn(async move { s.disable(actor, first_id).await });
        within(second_pause.arrived()).await;
        // Both own different barriers before either owns the writer. Exercise
        // both completions, with no join-only assumption about contention.
        if second_first {
            second_pause.resume();
            within(b).await.unwrap().unwrap();
            first_pause.resume();
            assert_eq!(
                within(a).await.unwrap().unwrap_err().code,
                "stale_authority"
            );
        } else {
            first_pause.resume();
            within(a).await.unwrap().unwrap();
            second_pause.resume();
            assert_eq!(
                within(b).await.unwrap().unwrap_err().code,
                "stale_authority"
            );
        }
        let db = rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM accounts WHERE role='admin' AND enabled=1 AND provisioning_state='ready'", [], |r|r.get::<_,i64>(0)).unwrap(), 1);
    }
}

#[tokio::test]
async fn signed_out_original_admin_cannot_use_a_replacement_session_to_issue_links() {
    for reset in [false, true] {
        let h = Harness::new().await;
        h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let member = h.member(&admin, "member").await;
        let original = h.service.browser_authority(secret(&admin)).await.unwrap();
        let pause = h.service.hooks.arm("admin_before_commit");
        let s = h.service.clone();
        let id = member["id"].as_str().unwrap().to_owned();
        let job = tokio::spawn(async move {
            if reset {
                s.reset_link(original, id).await
            } else {
                s.invitation(original).await
            }
        });
        within(pause.arrived()).await;
        h.service.logout(secret(&admin)).await.unwrap();
        let replacement = h.login("root.admin", PASSWORD).await;
        pause.resume();
        assert_eq!(
            within(job).await.unwrap().err().unwrap().code,
            "stale_authority"
        );
        assert_eq!(h.get("/api/me", Some(&replacement)).await.status(), 200);
        let db = rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM capabilities WHERE state='available'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
}

#[tokio::test]
async fn retirement_requires_current_original_admin_inside_the_writer() {
    for stale in [
        "logout",
        "expired",
        "generation",
        "role",
        "disabled",
        "not_ready",
    ] {
        let h = Harness::new().await;
        let first = h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let member = h.member(&admin, "member").await;
        let host = h
            .service
            .native_login(IP, "member", PASSWORD.into())
            .await
            .unwrap()
            .1;
        let native = h.service.validate_hostkey(&host).await.unwrap();
        let gate = h.service.publication_barrier(&native).await.unwrap();
        let retired = Arc::new(AtomicBool::new(false));
        {
            let _g = gate.enter().await;
            gate.set_round(Some(Arc::new(Retire(retired.clone()))))
                .unwrap();
        }
        let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
        let pause = h.service.hooks.arm("disable_with_barrier");
        let s = h.service.clone();
        let id = member["id"].as_str().unwrap().to_owned();
        let action = tokio::spawn(async move { s.disable(actor, id).await });
        within(pause.arrived()).await;
        if stale == "logout" {
            h.service.logout(secret(&admin)).await.unwrap();
            h.login("root.admin", PASSWORD).await;
        } else {
            let db = rusqlite::Connection::open(h.database()).unwrap();
            match stale {
                "expired" => {
                    db.execute(
                        "UPDATE credentials SET expires_at=?1 WHERE kind='session'",
                        [time::OffsetDateTime::now_utc().unix_timestamp()],
                    )
                    .unwrap();
                }
                "generation" => {
                    db.execute(
                        "UPDATE accounts SET generation=generation+1 WHERE id=?1",
                        [first["id"].as_str().unwrap()],
                    )
                    .unwrap();
                }
                "role" => {
                    db.execute_batch("DROP TRIGGER identity_immutable; UPDATE accounts SET role='member' WHERE role='admin'; CREATE TRIGGER identity_immutable BEFORE UPDATE OF id,username,role ON accounts BEGIN SELECT RAISE(ABORT,'immutable identity'); END;").unwrap();
                }
                "disabled" => {
                    db.execute(
                        "UPDATE accounts SET enabled=0 WHERE id=?1",
                        [first["id"].as_str().unwrap()],
                    )
                    .unwrap();
                }
                "not_ready" => {
                    db.execute(
                        "UPDATE accounts SET provisioning_state='provisioning' WHERE id=?1",
                        [first["id"].as_str().unwrap()],
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let db = rusqlite::Connection::open(h.database()).unwrap();
        let before: (String,i64,i64,bool) = db.query_row("SELECT password_phc,password_revision,generation,enabled FROM accounts WHERE id=?1", [member["id"].as_str().unwrap()], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        pause.resume();
        let code = within(action).await.unwrap().unwrap_err().code;
        assert_eq!(
            code,
            if stale == "role" {
                "permission_denied"
            } else {
                "stale_authority"
            }
        );
        assert!(!retired.load(Ordering::SeqCst));
        let after: (String,i64,i64,bool) = db.query_row("SELECT password_phc,password_revision,generation,enabled FROM accounts WHERE id=?1", [member["id"].as_str().unwrap()], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap();
        assert_eq!(before, after);
        assert!(h.service.validate_hostkey(&host).await.is_ok());
    }
}

#[tokio::test]
async fn reset_final_authority_matrix_does_not_retire_or_mutate_target() {
    for stale in [
        "issuer_generation",
        "issuer_disabled",
        "issuer_role",
        "issuer_not_ready",
        "expired",
        "revoked",
        "consumed",
        "replaced",
        "target_generation",
        "target_not_ready",
    ] {
        let h = Harness::new().await;
        let first = h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let member = h.member(&admin, "member").await;
        let id = member["id"].as_str().unwrap();
        let host = h
            .service
            .native_login(IP, "member", PASSWORD.into())
            .await
            .unwrap()
            .1;
        let native = h.service.validate_hostkey(&host).await.unwrap();
        let gate = h.service.publication_barrier(&native).await.unwrap();
        let retired = Arc::new(AtomicBool::new(false));
        {
            let _g = gate.enter().await;
            gate.set_round(Some(Arc::new(Retire(retired.clone()))))
                .unwrap();
        }
        let token = h.reset_link(&admin, id).await;
        let digest = meguri_sync_server::crypto::bearer(&token).unwrap();
        let pause = h.service.hooks.arm("reset_with_barrier");
        let s = h.service.clone();
        let job = tokio::spawn(async move { s.reset_claim(IP, token, NEW_PASSWORD.into()).await });
        within(pause.arrived()).await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        let issuer = first["id"].as_str().unwrap();
        match stale {
            "issuer_generation" => {
                db.execute(
                    "UPDATE accounts SET generation=generation+1 WHERE id=?1",
                    [issuer],
                )
                .unwrap();
            }
            "issuer_disabled" => {
                db.execute("UPDATE accounts SET enabled=0 WHERE id=?1", [issuer])
                    .unwrap();
            }
            "issuer_role" => {
                db.execute_batch("DROP TRIGGER identity_immutable; UPDATE accounts SET role='member' WHERE role='admin'; CREATE TRIGGER identity_immutable BEFORE UPDATE OF id,username,role ON accounts BEGIN SELECT RAISE(ABORT,'immutable identity'); END;").unwrap();
            }
            "issuer_not_ready" => {
                db.execute(
                    "UPDATE accounts SET provisioning_state='provisioning' WHERE id=?1",
                    [issuer],
                )
                .unwrap();
            }
            "expired" => {
                db.execute(
                    "UPDATE capabilities SET expires_at=?1 WHERE hash=?2",
                    rusqlite::params![
                        time::OffsetDateTime::now_utc().unix_timestamp(),
                        digest.as_slice()
                    ],
                )
                .unwrap();
            }
            "revoked" => {
                db.execute(
                    "UPDATE capabilities SET state='revoked' WHERE hash=?1",
                    [digest.as_slice()],
                )
                .unwrap();
            }
            "consumed" => {
                db.execute(
                    "UPDATE capabilities SET state='consumed' WHERE hash=?1",
                    [digest.as_slice()],
                )
                .unwrap();
            }
            "replaced" => {
                h.reset_link(&admin, id).await;
            }
            "target_generation" => {
                db.execute(
                    "UPDATE accounts SET generation=generation+1 WHERE id=?1",
                    [id],
                )
                .unwrap();
            }
            "target_not_ready" => {
                db.execute(
                    "UPDATE accounts SET provisioning_state='provisioning' WHERE id=?1",
                    [id],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before: (String, i64, i64) = db
            .query_row(
                "SELECT password_phc,password_revision,generation FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        pause.resume();
        assert_eq!(
            within(job).await.unwrap().unwrap_err().code,
            "capability_invalid"
        );
        assert!(!retired.load(Ordering::SeqCst));
        let after: (String, i64, i64) = db
            .query_row(
                "SELECT password_phc,password_revision,generation FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(before, after);
    }
}

#[tokio::test]
async fn revision_only_change_invalidates_each_original_password_proof() {
    for operation in ["browser", "native", "password"] {
        let h = Harness::new().await;
        h.setup().await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        // Leave room for a revision-only update while retaining a valid shape.
        db.execute("UPDATE accounts SET generation=2", []).unwrap();
        let admin = h.login("root.admin", PASSWORD).await;
        let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
        let pause = h.service.hooks.arm(if operation == "password" {
            "password_after_proof"
        } else {
            "login_after_proof"
        });
        let s = h.service.clone();
        let job = tokio::spawn(async move {
            match operation {
                "password" => {
                    s.change_password(IP, actor, PASSWORD.into(), NEW_PASSWORD.into())
                        .await
                }
                "native" => s
                    .native_login(IP, "root.admin", PASSWORD.into())
                    .await
                    .map(|_| ()),
                _ => s
                    .browser_login(IP, "root.admin", PASSWORD.into())
                    .await
                    .map(|_| ()),
            }
        });
        within(pause.arrived()).await;
        let original: (String, i64) = db
            .query_row("SELECT password_phc,generation FROM accounts", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        db.execute(
            "UPDATE accounts SET password_revision=password_revision+1",
            [],
        )
        .unwrap();
        pause.resume();
        assert_eq!(
            within(job).await.unwrap().unwrap_err().code,
            "authentication_failed"
        );
        let after: (String, i64) = db
            .query_row("SELECT password_phc,generation FROM accounts", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(original, after);
        assert_eq!(
            db.query_row("SELECT count(*) FROM credentials", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[tokio::test]
async fn signed_out_password_submitter_cannot_retire_its_round() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let host = h
        .service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let native = h.service.validate_hostkey(&host).await.unwrap();
    let gate = h.service.publication_barrier(&native).await.unwrap();
    let retired = Arc::new(AtomicBool::new(false));
    {
        let _g = gate.enter().await;
        gate.set_round(Some(Arc::new(Retire(retired.clone()))))
            .unwrap();
    }
    let replacement = h.login("root.admin", PASSWORD).await;
    let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
    let pause = h.service.hooks.arm("password_with_barrier");
    let s = h.service.clone();
    let job = tokio::spawn(async move {
        s.change_password(IP, actor, PASSWORD.into(), NEW_PASSWORD.into())
            .await
    });
    within(pause.arrived()).await;
    h.service.logout(secret(&admin)).await.unwrap();
    pause.resume();
    assert_eq!(
        within(job).await.unwrap().unwrap_err().code,
        "authentication_failed"
    );
    assert!(!retired.load(Ordering::SeqCst));
    assert_eq!(h.get("/api/me", Some(&replacement)).await.status(), 200);
    assert!(h.service.validate_hostkey(&host).await.is_ok());
}

struct WriterRetire {
    database: std::path::PathBuf,
    retired: Arc<AtomicBool>,
}
impl RoundRetirer for WriterRetire {
    fn retire(&self) -> Result<()> {
        let db = rusqlite::Connection::open(&self.database).unwrap();
        db.busy_timeout(std::time::Duration::ZERO).unwrap();
        let error = db.execute_batch("BEGIN IMMEDIATE").unwrap_err();
        assert!(
            matches!(error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::DatabaseBusy)
        );
        self.retired.store(true, Ordering::SeqCst);
        Ok(())
    }
}
#[tokio::test]
async fn successful_retirement_already_owns_the_committing_writer() {
    for operation in ["disable", "reset", "password"] {
        let h = Harness::new().await;
        h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let member = h.member(&admin, "member").await;
        let cookie = h.login("member", PASSWORD).await;
        let host = h
            .service
            .native_login(IP, "member", PASSWORD.into())
            .await
            .unwrap()
            .1;
        let native = h.service.validate_hostkey(&host).await.unwrap();
        let gate = h.service.publication_barrier(&native).await.unwrap();
        let retired = Arc::new(AtomicBool::new(false));
        {
            let _g = gate.enter().await;
            gate.set_round(Some(Arc::new(WriterRetire {
                database: h.database(),
                retired: retired.clone(),
            })))
            .unwrap();
        }
        let id = member["id"].as_str().unwrap();
        match operation {
            "disable" => {
                let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
                h.service.disable(actor, id.into()).await.unwrap();
            }
            "reset" => {
                let token = h.reset_link(&admin, id).await;
                h.service
                    .reset_claim(IP, token, NEW_PASSWORD.into())
                    .await
                    .unwrap();
            }
            _ => {
                let actor = h.service.browser_authority(secret(&cookie)).await.unwrap();
                h.service
                    .change_password(IP, actor, PASSWORD.into(), NEW_PASSWORD.into())
                    .await
                    .unwrap();
            }
        }
        assert!(retired.load(Ordering::SeqCst));
        assert!(h.service.validate_hostkey(&host).await.is_err());
    }
}

#[tokio::test]
async fn self_reset_rechecks_second_admin_eligibility_after_slow_work() {
    for loss in ["disabled", "role", "not_ready"] {
        let h = Harness::new().await;
        let first = h.setup().await;
        let id = first["id"].as_str().unwrap();
        let admin = h.login("root.admin", PASSWORD).await;
        let (h, second) = h.existing_second_admin(&admin).await;
        let token = h.reset_link(&admin, id).await;
        let host = h
            .service
            .native_login(IP, "root.admin", PASSWORD.into())
            .await
            .unwrap()
            .1;
        let native = h.service.validate_hostkey(&host).await.unwrap();
        let gate = h.service.publication_barrier(&native).await.unwrap();
        let retired = Arc::new(AtomicBool::new(false));
        {
            let _g = gate.enter().await;
            gate.set_round(Some(Arc::new(Retire(retired.clone()))))
                .unwrap();
        }
        let pause = h.service.hooks.arm("reset_with_barrier");
        let s = h.service.clone();
        let job = tokio::spawn(async move { s.reset_claim(IP, token, NEW_PASSWORD.into()).await });
        within(pause.arrived()).await;
        let second_id = second["id"].as_str().unwrap();
        let db = rusqlite::Connection::open(h.database()).unwrap();
        match loss {
            "disabled" => {
                let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
                h.service.disable(actor, second_id.into()).await.unwrap();
            }
            "role" => {
                db.execute_batch("BEGIN IMMEDIATE; DROP TRIGGER identity_immutable;")
                    .unwrap();
                db.execute("UPDATE accounts SET role='member' WHERE id=?1", [second_id])
                    .unwrap();
                db.execute_batch("CREATE TRIGGER identity_immutable BEFORE UPDATE OF id,username,role ON accounts BEGIN SELECT RAISE(ABORT,'immutable identity'); END; COMMIT;").unwrap();
            }
            _ => {
                db.execute(
                    "UPDATE accounts SET provisioning_state='provisioning' WHERE id=?1",
                    [second_id],
                )
                .unwrap();
            }
        }
        let before: (String, i64, i64) = db
            .query_row(
                "SELECT password_phc,password_revision,generation FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        pause.resume();
        assert_eq!(
            within(job).await.unwrap().unwrap_err().code,
            "sole_administrator"
        );
        assert!(!retired.load(Ordering::SeqCst));
        let after: (String, i64, i64) = db
            .query_row(
                "SELECT password_phc,password_revision,generation FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(h.get("/api/me", Some(&admin)).await.status(), 200);
        assert!(h.service.validate_hostkey(&host).await.is_ok());
    }
}

#[tokio::test]
async fn acknowledged_credential_issuance_precedes_revocation_and_old_values_stop_working() {
    for native in [false, true] {
        for reset in [false, true] {
            let h = Harness::new().await;
            h.setup().await;
            let admin = h.login("root.admin", PASSWORD).await;
            let member = h.member(&admin, "member").await;
            let seed = h
                .service
                .native_login(IP, "member", PASSWORD.into())
                .await
                .unwrap()
                .1;
            let native_authority = h.service.validate_hostkey(&seed).await.unwrap();
            let gate = h
                .service
                .publication_barrier(&native_authority)
                .await
                .unwrap();
            let reset_token = if reset {
                Some(h.reset_link(&admin, member["id"].as_str().unwrap()).await)
            } else {
                None
            };
            let issuer = h.service.hooks.arm("login_with_barrier");
            let s = h.service.clone();
            let issue = tokio::spawn(async move {
                if native {
                    s.native_login(IP, "member", PASSWORD.into()).await
                } else {
                    s.browser_login(IP, "member", PASSWORD.into()).await
                }
            });
            within(issuer.arrived()).await;
            let revoke_pause = h.service.hooks.arm(if reset {
                "reset_with_barrier"
            } else {
                "disable_with_barrier"
            });
            let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
            let id = member["id"].as_str().unwrap().to_owned();
            let s = h.service.clone();
            let revoke = tokio::spawn(async move {
                if let Some(token) = reset_token {
                    s.reset_claim(IP, token, NEW_PASSWORD.into()).await
                } else {
                    s.disable(actor, id).await
                }
            });
            within(gate.waiter_pending()).await;
            issuer.resume();
            within(revoke_pause.arrived()).await;
            let (identity, issued) = within(issue).await.unwrap().unwrap();
            assert_eq!(identity.id, member["id"]);
            // The concurrent revoker now owns the barrier, but cannot commit
            // until this successful issuance has been acknowledged and observed.
            if native {
                assert!(h.service.validate_hostkey(&issued).await.is_ok());
            } else {
                assert!(h.service.browser_authority(&issued).await.is_ok());
            }
            let hash = meguri_sync_server::crypto::bearer(&issued).unwrap();
            let db = rusqlite::Connection::open(h.database()).unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT revoked FROM credentials WHERE hash=?1",
                    [hash.as_slice()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            revoke_pause.resume();
            within(revoke).await.unwrap().unwrap();
            assert!(h.service.validate_hostkey(&seed).await.is_err());
            // No older credential is usable after revocation acknowledges.
            assert!(h.service.browser_authority(&issued).await.is_err());
            assert!(h.service.validate_hostkey(&issued).await.is_err());
            assert_eq!(
                db.query_row(
                    "SELECT revoked FROM credentials WHERE hash=?1",
                    [hash.as_slice()],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
            let count: i64 = db
                .query_row("SELECT count(*) FROM credentials", [], |r| r.get(0))
                .unwrap();
            let stale = if native {
                h.service.native_login(IP, "member", PASSWORD.into()).await
            } else {
                h.service.browser_login(IP, "member", PASSWORD.into()).await
            };
            assert_eq!(stale.unwrap_err().code, "authentication_failed");
            assert_eq!(
                db.query_row("SELECT count(*) FROM credentials", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                count
            );
        }
    }
}

#[tokio::test]
async fn acknowledged_capability_issuance_precedes_issuer_revocation() {
    for reset in [false, true] {
        for disable in [false, true] {
            let h = Harness::new().await;
            let first = h.setup().await;
            let admin = h.login("root.admin", PASSWORD).await;
            let member = h.member(&admin, "member").await;
            let (h, _) = h.existing_second_admin(&admin).await;
            let original = h.service.browser_authority(secret(&admin)).await.unwrap();
            let second = h.login("second.admin", PASSWORD).await;
            let second_actor = h.service.browser_authority(secret(&second)).await.unwrap();
            let issued_pause = h.service.hooks.arm("admin_after_commit");
            let s = h.service.clone();
            let issuer = original.clone();
            let target = member["id"].as_str().unwrap().to_owned();
            let issue = tokio::spawn(async move {
                if reset {
                    s.reset_link(issuer, target).await
                } else {
                    s.invitation(issuer).await
                }
            });
            within(issued_pause.arrived()).await;
            let revoke_pause = h.service.hooks.arm(if disable {
                "disable_with_barrier"
            } else {
                "password_with_barrier"
            });
            let s = h.service.clone();
            let issuer = original.clone();
            let id = first["id"].as_str().unwrap().to_owned();
            let revoke = tokio::spawn(async move {
                if disable {
                    s.disable(second_actor, id).await
                } else {
                    s.change_password(IP, issuer, PASSWORD.into(), NEW_PASSWORD.into())
                        .await
                }
            });
            within(revoke_pause.arrived()).await;
            issued_pause.resume();
            let link = within(issue).await.unwrap().unwrap();
            let token = link.url.split_once("#token=").unwrap().1.to_owned();
            let hash = meguri_sync_server::crypto::bearer(&token).unwrap();
            let db = rusqlite::Connection::open(h.database()).unwrap();
            let issued: (String, String, i64) = db
                .query_row(
                    "SELECT state,issuer,issuer_generation FROM capabilities WHERE hash=?1",
                    [hash.as_slice()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            assert_eq!(
                issued,
                ("available".into(), first["id"].as_str().unwrap().into(), 1)
            );
            // Issuance has committed and acknowledged while the concurrent
            // revoker is paused outside its writer, before its acknowledgement.
            revoke_pause.resume();
            within(revoke).await.unwrap().unwrap();
            assert!(h.service.browser_authority(secret(&admin)).await.is_err());
            assert_eq!(
                db.query_row(
                    "SELECT state FROM capabilities WHERE hash=?1",
                    [hash.as_slice()],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "revoked"
            );
            let before: (String, i64) = db
                .query_row(
                    "SELECT password_phc,generation FROM accounts WHERE id=?1",
                    [member["id"].as_str().unwrap()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            if reset {
                assert_eq!(
                    h.service
                        .reset_claim(IP, token, NEW_PASSWORD.into())
                        .await
                        .unwrap_err()
                        .code,
                    "capability_invalid"
                );
            } else {
                assert_eq!(
                    h.service
                        .claim(IP, "invite", token, "unissued", PASSWORD.into())
                        .await
                        .unwrap_err()
                        .code,
                    "capability_invalid"
                );
            }
            let count: i64 = db
                .query_row("SELECT count(*) FROM capabilities", [], |r| r.get(0))
                .unwrap();
            let stale = if reset {
                h.service
                    .reset_link(original, member["id"].as_str().unwrap().into())
                    .await
            } else {
                h.service.invitation(original).await
            };
            assert_eq!(stale.err().unwrap().code, "stale_authority");
            assert_eq!(
                db.query_row("SELECT count(*) FROM capabilities", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                count
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM accounts WHERE username='unissued'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            let after: (String, i64) = db
                .query_row(
                    "SELECT password_phc,generation FROM accounts WHERE id=?1",
                    [member["id"].as_str().unwrap()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(after, before);
        }
    }
}

#[tokio::test]
async fn original_admin_disable_acknowledges_before_either_capability_can_commit() {
    for reset in [false, true] {
        let h = Harness::new().await;
        let first = h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let member = h.member(&admin, "member").await;
        let (h, _) = h.existing_second_admin(&admin).await;
        let second = h.login("second.admin", PASSWORD).await;
        let original = h.service.browser_authority(secret(&admin)).await.unwrap();
        let pause = h.service.hooks.arm("admin_before_commit");
        let s = h.service.clone();
        let id = member["id"].as_str().unwrap().to_owned();
        let issue = tokio::spawn(async move {
            if reset {
                s.reset_link(original, id).await
            } else {
                s.invitation(original).await
            }
        });
        within(pause.arrived()).await;
        let actor = h.service.browser_authority(secret(&second)).await.unwrap();
        h.service
            .disable(actor, first["id"].as_str().unwrap().into())
            .await
            .unwrap();
        assert!(h.service.browser_authority(secret(&admin)).await.is_err());
        let db = rusqlite::Connection::open(h.database()).unwrap();
        let before: i64 = db
            .query_row("SELECT count(*) FROM capabilities", [], |r| r.get(0))
            .unwrap();
        pause.resume();
        assert_eq!(
            within(issue).await.unwrap().err().unwrap().code,
            "stale_authority"
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM capabilities", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            before
        );
    }
}
