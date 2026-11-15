// SPDX-License-Identifier: MIT
mod support;
use meguri_sync_server::{Config, DataRoot, Service};
use serde_json::{Value, json};
use std::{
    io::BufRead,
    os::unix::fs::{MetadataExt, PermissionsExt},
    process::{Command, Stdio},
};
use support::*;

#[tokio::test]
async fn invalid_and_conflicting_invites_remain_available() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let invite = h.invite(&admin).await;
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"..","password":PASSWORD}),
            None,
        )
        .await,
        422,
        "validation_failed",
    )
    .await;
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"valid","password":"short"}),
            None,
        )
        .await,
        422,
        "validation_failed",
    )
    .await;
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"ROOT.ADMIN","password":PASSWORD}),
            None,
        )
        .await,
        409,
        "username_unavailable",
    )
    .await;
    let r = h
        .post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"valid","password":PASSWORD}),
            None,
        )
        .await;
    assert_eq!(r.status(), 201);
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":invite,"username":"another","password":PASSWORD}),
            None,
        )
        .await,
        403,
        "capability_invalid",
    )
    .await;
}
#[tokio::test]
async fn simultaneous_invites_same_name_leave_losing_capability_available() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let a = h.invite(&admin).await;
    let b = h.invite(&admin).await;
    let (ra, rb) = tokio::join!(
        h.post(
            "/api/invitations/claim",
            json!({"token":a,"username":"shared","password":PASSWORD}),
            None
        ),
        h.post(
            "/api/invitations/claim",
            json!({"token":b,"username":"shared","password":PASSWORD}),
            None
        )
    );
    let losing = if ra.status() == 201 {
        assert_eq!(rb.status(), 409);
        b
    } else {
        assert_eq!(ra.status(), 409);
        assert_eq!(rb.status(), 201);
        a
    };
    assert_eq!(
        h.post(
            "/api/invitations/claim",
            json!({"token":losing,"username":"separate","password":PASSWORD}),
            None
        )
        .await
        .status(),
        201
    );
}
#[tokio::test]
async fn permissions_lock_aliases_and_explicit_init() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner");
    assert!(DataRoot::acquire(&path, false).is_err());
    let mut root = DataRoot::acquire(&path, true).unwrap();
    assert!(root.check_initialized().is_err());
    root.init().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o700);
    assert_eq!(
        std::fs::metadata(root.setup_file()).unwrap().mode() & 0o777,
        0o600
    );
    let lock_path = path.join("owner.lock");
    let inode = std::fs::metadata(&lock_path).unwrap().ino();
    let result = Command::new(binary())
        .args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!result.success());
    assert_eq!(std::fs::metadata(&lock_path).unwrap().ino(), inode);
    // Replacing the lock pathname alone still cannot bypass the directory lock.
    std::fs::rename(&lock_path, path.join("retained-owner-lock")).unwrap();
    std::fs::write(&lock_path, b"").unwrap();
    std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(root.validate().is_err());
    let result = Command::new(binary())
        .args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!result.success());
    drop(root);
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(DataRoot::acquire(&alias, false).is_err());
    let hard = temp.path().join("hardlock");
    std::fs::hard_link(&lock_path, hard).unwrap();
    assert!(DataRoot::acquire(&path, false).is_err());
}
#[tokio::test]
async fn local_setup_rotation_invalidates_old_secret_without_exposing_one() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner");
    let mut root = DataRoot::acquire(&path, true).unwrap();
    root.init().unwrap();
    let old = std::fs::read_to_string(root.setup_file()).unwrap();
    root.rotate_setup().unwrap();
    let current = std::fs::read_to_string(root.setup_file()).unwrap();
    assert_ne!(old, current);
    let service = Service::open(
        root,
        Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        service
            .claim(IP, "setup", old.trim().into(), "admin", PASSWORD.into())
            .await
            .unwrap_err()
            .code,
        "capability_invalid"
    );
    service
        .claim(IP, "setup", current.trim().into(), "admin", PASSWORD.into())
        .await
        .unwrap();
    drop(service);
    let root = DataRoot::acquire(&path, false).unwrap();
    assert!(root.rotate_setup().is_err());
}
#[tokio::test]
async fn sqlite_policies_and_temp_budget_apply_to_real_files() {
    let h = Harness::new().await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let mode: String = db
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let guard = h.service.temp.reserve(1024).unwrap();
    let path = h.temp.path().join("staged");
    std::fs::write(&path, vec![0; 1024]).unwrap();
    assert_eq!(h.service.temp.used(), 1024);
    assert!(
        h.service
            .temp
            .reserve(h.service.config.temp_bytes.get())
            .is_err()
    );
    std::fs::remove_file(path).unwrap();
    drop(guard);
    assert_eq!(h.service.temp.used(), 0);
    let mut permits = Vec::new();
    for _ in 0..16 {
        permits.push(h.service.collections.acquire().unwrap());
    }
    assert!(h.service.collections.acquire().is_err());
    drop(permits);
    assert!(h.service.collections.acquire().is_ok());
}
#[tokio::test]
async fn expired_capability_does_not_hash_or_reserve_and_boundaries_are_exclusive() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let token = h.invite(&admin).await;
    let digest = meguri_sync_server::crypto::bearer(&token).unwrap();
    let db = rusqlite::Connection::open(h.database()).unwrap();
    db.execute(
        "UPDATE capabilities SET expires_at=unixepoch() WHERE hash=?1",
        [digest.as_slice()],
    )
    .unwrap();
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":token,"username":"expired","password":PASSWORD}),
            None,
        )
        .await,
        403,
        "capability_invalid",
    )
    .await;
    let count: i64 = db
        .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn same_setup_reservation_different_proof_cannot_replace_it() {
    let h = Harness::new().await;
    let pause = h.service.hooks.arm("claim_after_reserve");
    let client = h.client.clone();
    let base = h.base.clone();
    let token = h.setup_token.clone();
    let first = tokio::spawn(async move {
        client
            .post(format!("{base}/api/setup"))
            .header("origin", ORIGIN)
            .json(&json!({"token":token,"username":"admin","password":PASSWORD}))
            .send()
            .await
            .unwrap()
    });
    pause.arrived().await;
    assert_eq!(
        h.get("/api/status", None)
            .await
            .json::<Value>()
            .await
            .unwrap()["data"]["setup_required"],
        true
    );
    error(
        h.post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"admin","password":NEW_PASSWORD}),
            None,
        )
        .await,
        409,
        "claim_in_progress",
    )
    .await;
    error(
        h.post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"different","password":PASSWORD}),
            None,
        )
        .await,
        409,
        "claim_in_progress",
    )
    .await;
    let winner = h
        .post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"ADMIN","password":PASSWORD}),
            None,
        )
        .await;
    assert_eq!(winner.status(), 201);
    let identity = winner.json::<Value>().await.unwrap()["data"].clone();
    pause.resume();
    error(first.await.unwrap(), 403, "capability_invalid").await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let (count, id): (i64, String) = db
        .query_row("SELECT count(*),id FROM accounts", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(id, identity["id"]);
}
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn filesystem_failure_retains_reservation_and_restart_resumes_only_it() {
    let h = Harness::new().await;
    let pause = h.service.hooks.arm("claim_after_reserve");
    let client = h.client.clone();
    let base = h.base.clone();
    let token = h.setup_token.clone();
    let first = tokio::spawn(async move {
        client
            .post(format!("{base}/api/setup"))
            .header("origin", ORIGIN)
            .json(&json!({"token":token,"username":"admin","password":PASSWORD}))
            .send()
            .await
            .unwrap()
    });
    pause.arrived().await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let id: String = db
        .query_row("SELECT id FROM accounts", [], |r| r.get(0))
        .unwrap();
    drop(db);
    let account = h.path().join("state/accounts").join(&id);
    std::fs::create_dir(&account).unwrap();
    std::fs::set_permissions(&account, std::fs::Permissions::from_mode(0o700)).unwrap();
    let blocker = account.join("media");
    std::fs::write(&blocker, b"not a directory").unwrap();
    pause.resume();
    error(first.await.unwrap(), 503, "provision_unavailable").await;
    error(
        h.post(
            "/api/login",
            json!({"username":"admin","password":PASSWORD}),
            None,
        )
        .await,
        401,
        "authentication_failed",
    )
    .await;
    error(
        h.post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"admin","password":NEW_PASSWORD}),
            None,
        )
        .await,
        409,
        "claim_in_progress",
    )
    .await;
    std::fs::remove_file(blocker).unwrap();
    let h = Harness::reopen(h.stop().await).await;
    let cookie = h.login("admin", PASSWORD).await;
    assert_eq!(
        h.get("/api/me", Some(&cookie))
            .await
            .json::<Value>()
            .await
            .unwrap()["data"]["id"],
        id
    );
}
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn revoked_reserved_invitation_remains_blocked_and_username_reserved() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let token = h.invite(&admin).await;
    let pause = h.service.hooks.arm("claim_after_reserve");
    let client = h.client.clone();
    let base = h.base.clone();
    let first = tokio::spawn(async move {
        client
            .post(format!("{base}/api/invitations/claim"))
            .header("origin", ORIGIN)
            .json(&json!({"token":token,"username":"reserved","password":PASSWORD}))
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
    error(first.await.unwrap(), 403, "capability_invalid").await;
    let h = Harness::reopen(h.stop().await).await;
    let admin = h.login("root.admin", NEW_PASSWORD).await;
    let fresh = h.invite(&admin).await;
    error(
        h.post(
            "/api/invitations/claim",
            json!({"token":fresh,"username":"reserved","password":PASSWORD}),
            None,
        )
        .await,
        409,
        "username_unavailable",
    )
    .await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let state: String = db
        .query_row(
            "SELECT provisioning_state FROM accounts WHERE username='reserved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "provisioning");
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn process_kill_at_durable_provisioning_phases_recovers_one_first_admin() {
    // Added store-write checkpoints include incomplete writes. Signals establish
    // process progress; SIGKILL is process-interruption evidence, not power loss.
    for phase in [
        "after_reserve",
        "stores_after_stage_create",
        "stores_during_stage_write",
        "stores_before_stage_sync",
        "after_stores",
        "before_ready",
        "after_ready",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        root.init().unwrap();
        let token = std::fs::read_to_string(root.setup_file())
            .unwrap()
            .trim()
            .to_owned();
        let old_token = token.clone();
        let lock_inode = std::fs::metadata(path.join("owner.lock")).unwrap().ino();
        drop(root);
        let signal = temp.path().join("phase");
        let mut child = Child(
            Command::new(binary())
                .args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
                .env("MEGURI_TEST_KILL_PHASE", phase)
                .env("MEGURI_TEST_SIGNAL_FILE", &signal)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let line = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            tokio::task::spawn_blocking(move || {
                let mut line = String::new();
                std::io::BufReader::new(stdout)
                    .read_line(&mut line)
                    .unwrap();
                line
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let addr = line.trim().strip_prefix("listening ").unwrap().to_owned();
        let request = tokio::spawn(async move {
            reqwest::Client::builder()
                .no_proxy()
                .build()
                .unwrap()
                .post(format!("http://{addr}/api/setup"))
                .header("origin", ORIGIN)
                .json(&json!({"token":token,"username":"admin","password":PASSWORD}))
                .send()
                .await
        });
        wait_file(&signal, &mut child).await;
        let db = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
        let reserved: (String, String, String) = db
            .query_row("SELECT id,username,password_phc FROM accounts", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        let reservation: (Vec<u8>, String, Vec<u8>) = db
            .query_row(
                "SELECT capability,account_id,credential_proof FROM provisioning_reservations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        drop(db);
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let _ = request.await.unwrap();
        drop(child);
        let root = DataRoot::acquire(&path, false).unwrap();
        assert_eq!(
            std::fs::metadata(path.join("owner.lock")).unwrap().ino(),
            lock_inode
        );
        let mut sentinels = vec![path.join("operator-sentinel")];
        let account = path.join("state/accounts").join(&reserved.0);
        for directory in [account.join("media"), account.join(".substrate-staging")] {
            if directory.is_dir() {
                sentinels.push(directory.join("unrelated-sentinel"));
            }
        }
        for sentinel in &sentinels {
            std::fs::write(sentinel, b"preserved process-interruption fixture\0\xff").unwrap();
            std::fs::set_permissions(sentinel, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        if phase != "after_ready" {
            let db = rusqlite::Connection::open(root.database()).unwrap();
            assert_eq!(
                db.query_row("SELECT provisioning_state FROM accounts", [], |r| r
                    .get::<_, String>(0))
                    .unwrap(),
                "provisioning"
            );
            assert_eq!(
                db.query_row("SELECT count(*) FROM credentials", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
                0
            );
        }
        let service = Service::open(
            root,
            Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap(),
        )
        .await
        .unwrap();
        assert!(!service.status().await.unwrap());
        assert_eq!(
            service
                .claim(IP, "setup", old_token, "another", PASSWORD.into())
                .await
                .unwrap_err()
                .code,
            "capability_invalid"
        );
        let (identity, _) = service
            .browser_login(IP, "admin", PASSWORD.into())
            .await
            .unwrap();
        assert_eq!(identity.id, reserved.0);
        for sentinel in &sentinels {
            assert_eq!(
                std::fs::read(sentinel).unwrap(),
                b"preserved process-interruption fixture\0\xff"
            );
        }
        assert_eq!(
            std::fs::metadata(path.join("owner.lock")).unwrap().ino(),
            lock_inode
        );
        let db = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
        let after: (String, String, String) = db
            .query_row("SELECT id,username,password_phc FROM accounts", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(after, reserved);
        let after_reservation: (Vec<u8>, String, Vec<u8>) = db
            .query_row(
                "SELECT capability,account_id,credential_proof FROM provisioning_reservations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(after_reservation, reservation);
        assert_eq!(
            db.query_row("SELECT count(*) FROM setup", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM accounts WHERE role='admin'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT phase FROM setup", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "closed"
        );
    }
}
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn incomplete_init_requires_explicit_repair_after_process_kill() {
    // These explicitly signalled own-child interruptions do not model power loss.
    for phase in [
        "init_after_intent_directory",
        "init_after_intent_create",
        "init_during_intent_write",
        "init_after_state",
        "init_after_empty_database",
        "init_before_schema_commit",
        "init_after_database",
        "init_after_token",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let signal = temp.path().join("phase");
        let mut child = Child(
            Command::new(binary())
                .args(["init", path.to_str().unwrap(), ORIGIN])
                .env("MEGURI_TEST_KILL_PHASE", phase)
                .env("MEGURI_TEST_SIGNAL_FILE", &signal)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        wait_file(&signal, &mut child).await;
        let child_lock_inode = std::fs::metadata(path.join("owner.lock")).unwrap().ino();
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        drop(child);
        let root = DataRoot::acquire(&path, false).unwrap();
        assert!(root.check_initialized().is_err());
        let inode = std::fs::metadata(path.join("owner.lock")).unwrap().ino();
        assert_eq!(inode, child_lock_inode);
        let sentinel = path.join("operator-sentinel");
        std::fs::write(&sentinel, b"preserve during explicit repair\0\xff").unwrap();
        std::fs::set_permissions(&sentinel, std::fs::Permissions::from_mode(0o600)).unwrap();
        // Admission refusal must precede the explicit local repair, not silently
        // repair on Service::open. Reacquire the same stable lock afterwards.
        assert!(
            Service::open(
                root,
                Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap()
            )
            .await
            .is_err()
        );
        let root = DataRoot::acquire(&path, false).unwrap();
        root.rotate_setup().unwrap();
        root.check_initialized().unwrap();
        assert_eq!(
            std::fs::metadata(path.join("owner.lock")).unwrap().ino(),
            inode
        );
        assert_eq!(
            std::fs::read(sentinel).unwrap(),
            b"preserve during explicit repair\0\xff"
        );
        let db = rusqlite::Connection::open(root.database()).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM setup", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(db);
        let token = std::fs::read_to_string(root.setup_file()).unwrap();
        let service = Service::open(
            root,
            Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap(),
        )
        .await
        .unwrap();
        let identity = service
            .claim(IP, "setup", token.trim().into(), "admin", PASSWORD.into())
            .await
            .unwrap();
        let db = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
        assert_eq!(
            db.query_row(
                "SELECT id FROM accounts WHERE role='admin' AND provisioning_state='ready'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            identity.id
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM setup WHERE phase='closed'", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn expired_reserved_invitation_does_not_resume_on_startup() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let token = h.invite(&admin).await;
    let digest = meguri_sync_server::crypto::bearer(&token).unwrap();
    let pause = h.service.hooks.arm("claim_after_reserve");
    let client = h.client.clone();
    let base = h.base.clone();
    let first = tokio::spawn(async move {
        client
            .post(format!("{base}/api/invitations/claim"))
            .header("origin", ORIGIN)
            .json(&json!({"token":token,"username":"reserved","password":PASSWORD}))
            .send()
            .await
            .unwrap()
    });
    pause.arrived().await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    db.execute(
        "UPDATE capabilities SET expires_at=unixepoch() WHERE hash=?1",
        [digest.as_slice()],
    )
    .unwrap();
    drop(db);
    pause.resume();
    error(first.await.unwrap(), 403, "capability_invalid").await;
    let h = Harness::reopen(h.stop().await).await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let state: String = db
        .query_row(
            "SELECT provisioning_state FROM accounts WHERE username='reserved'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(state, "provisioning");
    error(
        h.post(
            "/api/login",
            json!({"username":"reserved","password":PASSWORD}),
            None,
        )
        .await,
        401,
        "authentication_failed",
    )
    .await;
}
#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn injected_unlink_failure_cannot_revive_consumed_setup_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner");
    let mut root = DataRoot::acquire(&path, true).unwrap();
    root.init().unwrap();
    let token = std::fs::read_to_string(root.setup_file())
        .unwrap()
        .trim()
        .to_owned();
    drop(root);
    let mut child = Child(
        Command::new(binary())
            .args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
            .env("MEGURI_TEST_FAIL_PHASE", "before_setup_unlink")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let line = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::task::spawn_blocking(move || {
            let mut s = String::new();
            std::io::BufReader::new(stdout).read_line(&mut s).unwrap();
            s
        }),
    )
    .await
    .unwrap()
    .unwrap();
    let base = format!("http://{}", line.trim().strip_prefix("listening ").unwrap());
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let response = client
        .post(format!("{base}/api/setup"))
        .header("origin", ORIGIN)
        .json(&json!({"token":token,"username":"admin","password":PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 201);
    assert!(path.join("state/setup-token").is_file());
    error(
        client
            .post(format!("{base}/api/setup"))
            .header("origin", ORIGIN)
            .json(&json!({"token":token,"username":"again","password":PASSWORD}))
            .send()
            .await
            .unwrap(),
        403,
        "capability_invalid",
    )
    .await;
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(child);
    let root = DataRoot::acquire(&path, false).unwrap();
    let service = Service::open(
        root,
        Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap(),
    )
    .await
    .unwrap();
    assert!(!service.status().await.unwrap());
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn both_initial_claim_readers_precede_one_exact_reservation() {
    for same_cap in [true, false] {
        for different_proof in [true, false] {
            let h = Harness::new().await;
            h.setup().await;
            let admin = h.login("root.admin", PASSWORD).await;
            let a = h.invite(&admin).await;
            let b = if same_cap {
                a.clone()
            } else {
                h.invite(&admin).await
            };
            let first_read = h.service.hooks.arm("claim_before_reservation");
            let s = h.service.clone();
            let token = a.clone();
            let first = tokio::spawn(async move {
                s.claim(IP, "invite", token, "shared", PASSWORD.into())
                    .await
            });
            within(first_read.arrived()).await;
            // Re-arm the existing one-shot phase only after reader A is parked.
            let second_read = h.service.hooks.arm("claim_before_reservation");
            let s = h.service.clone();
            let token = b.clone();
            let proof = if different_proof {
                NEW_PASSWORD
            } else {
                PASSWORD
            };
            let second =
                tokio::spawn(
                    async move { s.claim(IP, "invite", token, "shared", proof.into()).await },
                );
            within(second_read.arrived()).await;
            let db = rusqlite::Connection::open(h.database()).unwrap();
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM accounts WHERE username='shared'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            let reserved = h.service.hooks.arm("claim_after_reserve");
            first_read.resume();
            within(reserved.arrived()).await;
            let exact: (String, String) = db
                .query_row(
                    "SELECT id,password_phc FROM accounts WHERE username='shared'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            let association: (Vec<u8>, String, Vec<u8>) = db.query_row(
                "SELECT capability,account_id,credential_proof FROM provisioning_reservations WHERE username='shared'", [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
            assert_eq!(
                association.0,
                meguri_sync_server::crypto::bearer(&a).unwrap().to_vec()
            );
            assert_eq!(association.1, exact.0);
            assert_eq!(
                association.2,
                meguri_sync_server::crypto::digest(exact.1.as_bytes()).to_vec()
            );
            second_read.resume();
            if same_cap && !different_proof {
                let identity = within(second).await.unwrap().unwrap();
                assert_eq!(identity.id, exact.0);
                reserved.resume();
                assert_eq!(
                    within(first).await.unwrap().unwrap_err().code,
                    "capability_invalid"
                );
            } else {
                let expected = if same_cap {
                    "claim_in_progress"
                } else {
                    "username_unavailable"
                };
                assert_eq!(within(second).await.unwrap().unwrap_err().code, expected);
                reserved.resume();
                assert_eq!(within(first).await.unwrap().unwrap().id, exact.0);
            }
            let after: (String, String) = db
                .query_row(
                    "SELECT id,password_phc FROM accounts WHERE username='shared'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap();
            assert_eq!(after, exact);
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM provisioning_reservations WHERE username='shared'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
            // Even the different-cap losing password cannot authenticate the
            // winner, and its capability is still available for its own claim.
            if different_proof {
                assert_eq!(
                    h.service
                        .browser_login(IP, "shared", NEW_PASSWORD.into())
                        .await
                        .unwrap_err()
                        .code,
                    "authentication_failed"
                );
            }
            if !same_cap {
                let hash = meguri_sync_server::crypto::bearer(&b).unwrap();
                let available: (String, Option<String>) = db
                    .query_row(
                        "SELECT state,reserved_id FROM capabilities WHERE hash=?1",
                        [hash.as_slice()],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(available, ("available".into(), None));
                let own = h
                    .service
                    .claim(IP, "invite", b, "separate", proof.into())
                    .await
                    .unwrap();
                assert_ne!(own.id, exact.0);
            }
        }
    }
}

#[tokio::test]
async fn ordinary_claim_refuses_exhausted_staging_budget_then_retries_exact_identity() {
    let h = Harness::new().await;
    let held = h
        .service
        .temp
        .reserve(h.service.config.temp_bytes.get())
        .unwrap();
    // One ordinary HTTP claim; exhaustion is injected by an in-process guard.
    error(
        h.post(
            "/api/setup",
            json!({"token":h.setup_token,"username":"admin","password":PASSWORD}),
            None,
        )
        .await,
        503,
        "provision_unavailable",
    )
    .await;
    let db = rusqlite::Connection::open(h.database()).unwrap();
    let exact: (String, String, String) = db
        .query_row("SELECT id,username,password_phc FROM accounts", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    let reservation: (Vec<u8>, String, Vec<u8>) = db
        .query_row(
            "SELECT capability,account_id,credential_proof FROM provisioning_reservations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert!(!h.path().join("state/accounts").join(&exact.0).exists());
    assert_eq!(
        db.query_row("SELECT provisioning_state FROM accounts", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "provisioning"
    );
    assert_eq!(h.service.temp.used(), h.service.config.temp_bytes.get());
    drop(held);
    let identity = h
        .service
        .claim(IP, "setup", h.setup_token.clone(), "admin", PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(identity.id, exact.0);
    assert_eq!(h.service.temp.used(), 0);
    let after: (String, String, String) = db
        .query_row("SELECT id,username,password_phc FROM accounts", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(after, exact);
    let after_reservation: (Vec<u8>, String, Vec<u8>) = db
        .query_row(
            "SELECT capability,account_id,credential_proof FROM provisioning_reservations",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(after_reservation, reservation);
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn final_invitation_authority_is_checked_after_durable_stores_before_ready() {
    for expire in [true, false] {
        let h = Harness::new().await;
        h.setup().await;
        let admin = h.login("root.admin", PASSWORD).await;
        let token = h.invite(&admin).await;
        let hash = meguri_sync_server::crypto::bearer(&token).unwrap();
        let pause = h.service.hooks.arm("claim_after_stores");
        let s = h.service.clone();
        let claim = tokio::spawn(async move {
            s.claim(IP, "invite", token, "reserved", PASSWORD.into())
                .await
        });
        within(pause.arrived()).await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        let exact: (String, String) = db
            .query_row(
                "SELECT id,password_phc FROM accounts WHERE username='reserved'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let stores = h.path().join("state/accounts").join(&exact.0);
        assert!(stores.join("current-substrate").is_file());
        let before = store_bytes(&stores);
        let reservation: (Vec<u8>, String, Vec<u8>) = db.query_row(
            "SELECT capability,account_id,credential_proof FROM provisioning_reservations WHERE username='reserved'", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        if expire {
            // Controlled UTC-boundary fixture, not a wall-clock sleep.
            db.execute(
                "UPDATE capabilities SET expires_at=unixepoch() WHERE hash=?1",
                [hash.as_slice()],
            )
            .unwrap();
        } else {
            let actor = h.service.browser_authority(secret(&admin)).await.unwrap();
            h.service
                .change_password(IP, actor, PASSWORD.into(), NEW_PASSWORD.into())
                .await
                .unwrap();
            assert!(h.service.browser_authority(secret(&admin)).await.is_err());
        }
        pause.resume();
        assert_eq!(
            within(claim).await.unwrap().unwrap_err().code,
            "capability_invalid"
        );
        let after: (String, String, String) = db
            .query_row(
                "SELECT id,password_phc,provisioning_state FROM accounts WHERE username='reserved'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(after, (exact.0.clone(), exact.1, "provisioning".into()));
        let after_reservation: (Vec<u8>, String, Vec<u8>) = db.query_row(
            "SELECT capability,account_id,credential_proof FROM provisioning_reservations WHERE username='reserved'", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(after_reservation, reservation);
        assert_eq!(store_bytes(&stores), before);
        assert_eq!(
            h.service
                .browser_login(IP, "reserved", PASSWORD.into())
                .await
                .unwrap_err()
                .code,
            "authentication_failed"
        );
        let admin = if expire {
            admin
        } else {
            h.login("root.admin", NEW_PASSWORD).await
        };
        let fresh = h.invite(&admin).await;
        assert_eq!(
            h.service
                .claim(IP, "invite", fresh, "reserved", PASSWORD.into())
                .await
                .unwrap_err()
                .code,
            "username_unavailable"
        );
        drop(db);
        let h = Harness::reopen(h.stop().await).await;
        let db = rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(db.query_row("SELECT id FROM accounts WHERE username='reserved' AND provisioning_state='provisioning'", [], |r| r.get::<_, String>(0)).unwrap(), exact.0);
        assert_eq!(store_bytes(&stores), before);
    }
}
