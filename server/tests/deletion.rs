// SPDX-License-Identifier: MIT
//! Synthetic account deletion interruption and authority proofs.
#![cfg(feature = "test-hooks")]
mod support;
use support::*;
use serde_json::json;

#[cfg(feature = "test-hooks")]
async fn child_service(path: &std::path::Path, phase: &str, kill: bool, signal: &std::path::Path) -> (Child, String) {
    use std::{io::BufRead, process::{Command, Stdio}};
    let mut child = Child(Command::new(binary())
        .args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
        .env(if kill { "MEGURI_TEST_KILL_PHASE" } else { "MEGURI_TEST_FAIL_PHASE" }, phase).env("MEGURI_TEST_SIGNAL_FILE", signal)
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let stdout = child.0.stdout.take().unwrap();
    let line = within(tokio::task::spawn_blocking(move || {
        let mut line = String::new();
        std::io::BufReader::new(stdout).read_line(&mut line).unwrap();
        line
    })).await.unwrap();
    (child, format!("http://{}", line.trim().strip_prefix("listening ").unwrap()))
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn deletion_kill_boundaries_preserve_intent_and_restart_only_selected_cleanup() {
    for (phase, committed) in [
        ("delete_before_metadata_commit", false),
        ("delete_after_metadata_commit", true),
        ("delete_after_quarantine", true),
        ("delete_after_remove", true),
        ("delete_after_parent_sync", true),
        ("delete_before_journal_clear", true),
    ] {
        let h = Harness::new().await;
        h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let member = h.member(&admin, "deleted.member").await;
        let id = member["id"].as_str().unwrap();
        let cookie = h.login("deleted.member", PASSWORD).await;
        let account = h.path().join("state/accounts").join(id);
        let kept = h.path().join("state/accounts");
        // Include the ready siblings' immutable daily payloads in the exact
        // baseline; Harness has already joined the serving startup sweep.
        h.service.capture_backups_at(time::OffsetDateTime::now_utc().unix_timestamp()).await.unwrap();
        let before = store_bytes(&kept).into_iter().filter(|(path, _)| !path.starts_with(id)).collect::<std::collections::BTreeMap<_,_>>();
        let temp = h.stop().await;
        let owner = temp.path().join("owner");
        let signal = temp.path().join("delete-phase");
        let (mut child, base) = child_service(&owner, phase, true, &signal).await;
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let request = client.post(format!("{base}/api/admin/accounts/{id}/delete"))
            .header("origin", ORIGIN).header("cookie", &admin).json(&json!({}));
        let request = tokio::spawn(async move { request.send().await });
        wait_file(&signal, &mut child).await;
        assert_eq!(std::fs::read(&signal).unwrap(), phase.as_bytes());
        child.0.kill().unwrap(); child.0.wait().unwrap(); drop(child);
        assert!(within(request).await.unwrap().is_err());
        let db = rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM account_deletions WHERE id=?1", [id], |r| r.get::<_,i64>(0)).unwrap(), i64::from(committed));
        assert_eq!(db.query_row("SELECT count(*) FROM accounts WHERE id=?1", [id], |r| r.get::<_,i64>(0)).unwrap(), i64::from(!committed));
        if committed {
            // Pending IDs cannot be reallocated, and the reservation exception
            // does not permit deleting any unrelated account's proof.
            assert!(db.execute("INSERT INTO accounts(id,username,role,provisioning_state,enabled,password_phc,password_revision,generation) SELECT ?1,'reused.member','member','provisioning',1,password_phc,1,1 FROM accounts LIMIT 1", [id]).is_err());
        }
        assert!(db.execute("DELETE FROM provisioning_reservations", []).is_err());
        drop(db);
        let h = Harness::reopen(temp).await;
        if !committed {
            assert_eq!(h.get("/api/me", Some(&cookie)).await.status(), 200);
            assert_eq!(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&admin)).await.status(), 200);
        }
        assert!(!account.exists());
        assert!(!kept.join(format!(".delete-{id}")).exists());
        assert_eq!(store_bytes(&kept), before);
        error(h.get("/api/me", Some(&cookie)).await, 401, "authentication_failed").await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM account_deletions", [], |r| r.get::<_,i64>(0)).unwrap(), 0);
        drop(db);
        error(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&admin)).await, 404, "route_not_found").await;
        let _ = h.stop().await;
    }
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn closed_restore_admission_preserves_pending_files_until_explicit_reopening() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "pending.member").await;
    let id = member["id"].as_str().unwrap();
    let temp = h.stop().await;
    let owner = temp.path().join("owner");
    let signal = temp.path().join("delete-phase");
    let (mut child, base) = child_service(&owner, "delete_after_quarantine", true, &signal).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let request = client.post(format!("{base}/api/admin/accounts/{id}/delete"))
        .header("origin", ORIGIN).header("cookie", &admin).json(&json!({}));
    let request = tokio::spawn(async move { request.send().await });
    wait_file(&signal, &mut child).await;
    child.0.kill().unwrap(); child.0.wait().unwrap(); drop(child);
    assert!(within(request).await.unwrap().is_err());
    let quarantine = owner.join("state/accounts").join(format!(".delete-{id}"));
    let before = store_bytes(&quarantine);
    let db = rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
    db.execute("UPDATE service_state SET admission_open=0", []).unwrap(); drop(db);
    let h = Harness::reopen(temp).await;
    assert_eq!(store_bytes(&quarantine), before);
    error(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&admin)).await, 503, "admission_closed").await;
    let temp = h.stop().await;
    // Explicit stopped synthetic restore-admission review fixture.
    let db = rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
    db.execute("UPDATE service_state SET admission_open=1", []).unwrap(); drop(db);
    let h = Harness::reopen(temp).await;
    assert!(!quarantine.exists());
    assert_eq!(h.get("/api/me", Some(&admin)).await.status(), 200);
    let _ = h.stop().await;
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn deletion_revalidates_original_admin_after_wait_and_accepts_provisioning_members() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "disabled.member").await;
    let id = member["id"].as_str().unwrap();
    error(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({"unexpected":true}), Some(&admin)).await, 400, "invalid_request").await;
    assert_eq!(h.post(&format!("/api/admin/accounts/{id}/disable"), json!({}), Some(&admin)).await.status(), 200);
    let pause = h.service.hooks.arm("delete_with_barrier");
    let request = h.client.post(format!("{}/api/admin/accounts/{id}/delete", h.base))
        .header("origin", ORIGIN).header("cookie", &admin).json(&json!({}));
    let request = tokio::spawn(async move { request.send().await.unwrap() });
    within(pause.arrived()).await;
    assert_eq!(h.post("/api/password", json!({"current_password":PASSWORD,"new_password":NEW_PASSWORD}), Some(&admin)).await.status(), 200);
    let mut current_admin = h.login("root.admin", NEW_PASSWORD).await;
    pause.resume();
    error(within(request).await.unwrap(), 409, "stale_authority").await;
    assert!(h.path().join("state/accounts").join(id).exists());
    assert_eq!(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&current_admin)).await.status(), 200);
    for (phase, present) in [("claim_after_reserve", false), ("claim_after_stores", true)] {
        let invitation = h.invite(&current_admin).await;
        let pause = h.service.hooks.arm(phase);
        let request = h.client.post(format!("{}/api/invitations/claim", h.base)).header("origin", ORIGIN)
            .json(&json!({"token":invitation,"username":"provisioning.member","password":PASSWORD}));
        let claim = tokio::spawn(async move { request.send().await.unwrap() });
        within(pause.arrived()).await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        let id: String = db.query_row("SELECT id FROM accounts WHERE username='provisioning.member'", [], |r| r.get(0)).unwrap();
        drop(db);
        let directory = h.path().join("state/accounts").join(&id);
        assert_eq!(directory.exists(), present);
        if present {
            // The after-stores pause owns the barrier. Revoke the invitation's
            // issuer, then let publication refuse before deleting the blocked
            // provisioning row and its already-created stores.
            assert_eq!(h.post("/api/password", json!({"current_password":NEW_PASSWORD,"new_password":PASSWORD}), Some(&current_admin)).await.status(), 200);
            current_admin = h.login("root.admin", PASSWORD).await;
            pause.resume();
            error(within(claim).await.unwrap(), 403, "capability_invalid").await;
            assert_eq!(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&current_admin)).await.status(), 200);
        } else {
            assert_eq!(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&current_admin)).await.status(), 200);
            pause.resume();
            error(within(claim).await.unwrap(), 403, "capability_invalid").await;
        }
        assert!(!directory.exists());
    }
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    assert_eq!(h.get("/api/me", Some(&current_admin)).await.status(), 200);
    let _ = h.stop().await;
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn renamed_pending_deletion_is_retryable_without_poisoning_other_accounts_pins() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "retry.member").await;
    let id = member["id"].as_str().unwrap();
    let temp = h.stop().await;
    let owner = temp.path().join("owner");
    let signal = temp.path().join("unused-phase");
    // The no-replace rename succeeds, then the injected durability error
    // reaches the caller. Its directory pins must already have been evicted.
    let (mut child, base) = child_service(&owner, "owned_after_publish_rename", false, &signal).await;
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let delete = || client.post(format!("{base}/api/admin/accounts/{id}/delete"))
        .header("origin", ORIGIN).header("cookie", &admin).json(&json!({}));
    let response = within(delete().send()).await.unwrap();
    assert_eq!(response.status(), 503);
    assert!(!owner.join("state/accounts").join(id).exists());
    assert!(owner.join("state/accounts").join(format!(".delete-{id}")).is_dir());
    assert_eq!(within(client.get(format!("{base}/api/admin/accounts")).header("cookie", &admin).send()).await.unwrap().status(), 200);
    // The same current administrator can finish the exact pending ID. No new
    // rename is needed, so the still-armed fault is not bypassed or cleared.
    assert_eq!(within(delete().send()).await.unwrap().status(), 200);
    error(within(delete().send()).await.unwrap(), 404, "route_not_found").await;
    child.0.kill().unwrap(); child.0.wait().unwrap(); drop(child);
    let h = Harness::reopen(temp).await;
    assert_eq!(h.get("/api/me", Some(&admin)).await.status(), 200);
    let _ = h.stop().await;
}
