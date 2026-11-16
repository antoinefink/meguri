// SPDX-License-Identifier: MIT
//! Finite own-service fixtures. No external paths, network or disk-full claims.
use super::*;
use crate::crypto;
use rusqlite::params;
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

// Same public synthetic PHC as the permanent crypto compatibility test.
const PHC: &str = "$argon2id$v=19$m=19456,t=2,p=1$AAECAwQFBgcICQoLDA0ODw$krbozlfmG/YbBoj1y9KHSlgK7HHNhVNfzFmZ8bEcWH4";
fn budget() -> crate::limits::TempBudget {
    crate::limits::TempBudget::new(std::num::NonZeroU64::new(collection::INITIAL_TEMP_BYTES + 4096).unwrap())
}
fn initialized() -> (tempfile::TempDir, DataRoot) {
    let temp = tempfile::tempdir().unwrap();
    let mut root = DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
    root.init().unwrap();
    (temp, root)
}
fn private_sentinel(path: &Path) {
    fs::write(path, b"synthetic data: preserve exactly\0\xff").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn reserve(root: &DataRoot, kind: &str, issuer: Option<&str>) -> String {
    let id = crypto::random_token().unwrap();
    let mut c = open(root.database(), false).unwrap();
    let tx = c.transaction().unwrap();
    let hash: Vec<u8> = if kind == "setup" {
        tx.query_row("SELECT capability FROM setup", [], |r| r.get(0))
            .unwrap()
    } else {
        let h = crypto::digest(id.as_bytes()).to_vec();
        tx.execute("INSERT INTO capabilities(hash,kind,issuer,issuer_generation,expires_at,state) VALUES(?1,'invite',?2,1,unixepoch()+3600,'available')",params![h,issuer]).unwrap();
        h
    };
    let role = if kind == "setup" { "admin" } else { "member" };
    let username = if kind == "setup" { "admin" } else { "member" };
    tx.execute(
        "INSERT INTO accounts VALUES(?1,?2,?3,'provisioning',1,?4,1,1)",
        params![id, username, role, PHC],
    )
    .unwrap();
    tx.execute(
        "UPDATE capabilities SET state='provisioning',reserved_id=?2 WHERE hash=?1",
        params![hash, id],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO provisioning_reservations VALUES(?1,?2,?3,?4,?5)",
        params![
            hash,
            id,
            username,
            role,
            crypto::digest(PHC.as_bytes()).as_slice()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    id
}
fn ready(root: &DataRoot, id: &str, setup: bool) {
    root.provision(id, &budget()).unwrap();
    let mut c = open(root.database(), false).unwrap();
    let tx = c.transaction().unwrap();
    tx.execute(
        "UPDATE accounts SET provisioning_state='ready' WHERE id=?1",
        [id],
    )
    .unwrap();
    tx.execute(
        "UPDATE capabilities SET state='consumed' WHERE reserved_id=?1",
        [id],
    )
    .unwrap();
    if setup {
        tx.execute("UPDATE setup SET phase='closed'", []).unwrap();
    }
    tx.commit().unwrap();
}
fn metadata_snapshot(root: &DataRoot) -> Vec<u8> {
    // Close writers first; compare the real main database bytes on refusal.
    fs::read(root.database()).unwrap()
}

#[cfg(feature = "test-hooks")]
#[test]
fn early_initialization_phase_errors_require_explicit_local_repair() {
    for phase in [
        "init_after_state",
        "init_after_empty_database",
        "init_before_schema_commit",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        assert!(crate::test_hooks::storage_failure(phase, || root.init()).is_err());
        assert!(root.check_initialized().is_err());
        let sentinel = path.join("state/operator-sentinel");
        private_sentinel(&sentinel);
        let bytes = fs::read(&sentinel).unwrap();
        let lock_inode = fs::metadata(path.join("owner.lock")).unwrap();
        drop(root);
        let root = DataRoot::acquire(&path, false).unwrap();
        assert!(root.check_initialized().is_err());
        root.rotate_setup().unwrap();
        root.check_initialized().unwrap();
        assert_eq!(fs::read(&sentinel).unwrap(), bytes);
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(path.join("owner.lock")).unwrap().ino(),
            lock_inode.ino()
        );
        let c = inspect(root.database()).unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM setup", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
#[test]
fn unknown_initialization_bytes_are_preserved_and_not_repaired() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner");
    let mut root = DataRoot::acquire(&path, true).unwrap();
    fs::create_dir(root.state()).unwrap();
    fs::set_permissions(root.state(), fs::Permissions::from_mode(0o700)).unwrap();
    let sentinel = root.state().join("accounts.sqlite");
    private_sentinel(&sentinel);
    let bytes = fs::read(&sentinel).unwrap();
    assert!(root.init().is_err());
    assert!(root.rotate_setup().is_err());
    assert!(root.check_initialized().is_err());
    assert_eq!(fs::read(sentinel).unwrap(), bytes);
}
#[cfg(feature = "test-hooks")]
#[test]
fn private_marker_phase_errors_resume_same_reservation_on_restart() {
    for phase in [
        "stores_after_stage_create",
        "stores_during_stage_write",
        "stores_before_stage_sync",
    ] {
        let (temp, root) = initialized();
        let id = reserve(&root, "setup", None);
        let before = metadata_snapshot(&root);
        assert!(
            crate::test_hooks::storage_failure(phase, || root.provision(&id, &budget())).is_err()
        );
        let stores = root.stores(&id).unwrap();
        assert!(!stores.current_manifest.exists());
        let sentinel = stores.media.join("operator-sentinel");
        private_sentinel(&sentinel);
        let bytes = fs::read(&sentinel).unwrap();
        let staging_sentinel = stores.directory.join(".collection-staging/unrelated");
        private_sentinel(&staging_sentinel);
        drop(root);
        let root = DataRoot::acquire(&temp.path().join("owner"), false).unwrap();
        root.check_initialized().unwrap();
        root.provision(&id, &budget()).unwrap();
        root.validate_stores(&id).unwrap();
        assert_eq!(metadata_snapshot(&root), before);
        assert_eq!(fs::read(&sentinel).unwrap(), bytes);
        assert_eq!(fs::read(&staging_sentinel).unwrap(), bytes);
        let c = inspect(root.database()).unwrap();
        let claim: (String, String, String) = c
            .query_row("SELECT id,username,password_phc FROM accounts", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(claim, (id, "admin".into(), PHC.into()));
    }
}
#[test]
fn complete_store_bindings_reject_changed_intermediate_directory_identities() {
    for ancestor in [
        "state/accounts",
        "generations",
        "media",
        "generations/initial",
    ] {
        let (_temp, root) = initialized();
        let id = reserve(&root, "setup", None);
        root.provision(&id, &budget()).unwrap();
        let stores = root.stores(&id).unwrap();
        let changed = if ancestor == "state/accounts" {
            root.state().join("accounts")
        } else {
            stores.directory.join(ancestor)
        };
        // Controlled invalid fixture stays entirely inside the fresh owned root.
        // No links, outside targets or concurrent path replacement sequence.
        fs::rename(&changed, changed.with_extension("retained")).unwrap();
        fs::create_dir(&changed).unwrap();
        fs::set_permissions(&changed, fs::Permissions::from_mode(0o700)).unwrap();
        let sentinel = changed.join("unrelated");
        private_sentinel(&sentinel);
        assert!(root.validate_stores(&id).is_err());
        assert!(root.provision(&id, &budget()).is_err());
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"synthetic data: preserve exactly\0\xff"
        );
        let c = inspect(root.database()).unwrap();
        assert_eq!(
            c.query_row("SELECT provisioning_state FROM accounts", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "provisioning"
        );
    }
}
#[test]
fn valid_ready_and_blocked_metadata_survive_read_only_startup_checks() {
    let (temp, root) = initialized();
    let admin = reserve(&root, "setup", None);
    ready(&root, &admin, true);
    let member = reserve(&root, "invite", Some(&admin));
    let sentinel = root.stores(&admin).unwrap().media.join("sentinel");
    private_sentinel(&sentinel);
    for alteration in [
        "UPDATE capabilities SET expires_at=unixepoch()-1 WHERE kind='invite'",
        "UPDATE capabilities SET state='revoked' WHERE kind='invite'",
        "UPDATE accounts SET enabled=0,generation=generation+1 WHERE role='admin'",
    ] {
        let c = open(root.database(), false).unwrap();
        c.execute(alteration, []).unwrap();
        drop(c);
        let bytes = metadata_snapshot(&root);
        root.check_initialized().unwrap();
        assert_eq!(metadata_snapshot(&root), bytes);
    }
    drop(root);
    let root = DataRoot::acquire(&temp.path().join("owner"), false).unwrap();
    root.check_initialized().unwrap();
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"synthetic data: preserve exactly\0\xff"
    );
    let c = inspect(root.database()).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT provisioning_state FROM accounts WHERE id=?1",
            [member],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "provisioning"
    );
}
#[test]
fn contradictory_sqlite_metadata_refuses_without_mutation_or_publication() {
    for sql in [
        "UPDATE setup SET capability=(SELECT hash FROM capabilities WHERE kind='invite')",
        "DELETE FROM setup",
        "UPDATE setup SET phase='closed'",
        "UPDATE accounts SET password_phc='invalid-phc' WHERE role='member'",
        "UPDATE accounts SET password_phc=replace(password_phc,'krbo','arbo') WHERE role='member'",
        "UPDATE capabilities SET issuer_generation=999 WHERE kind='invite'",
        "UPDATE capabilities SET target=issuer,target_generation=1 WHERE kind='invite'",
        "UPDATE capabilities SET state='available' WHERE kind='invite'",
        "UPDATE capabilities SET reserved_id=NULL WHERE kind='invite'",
        "DELETE FROM service_state",
        "DROP TRIGGER identity_immutable; UPDATE accounts SET username='Noncanonical' WHERE role='member'",
        "DROP TRIGGER identity_immutable; UPDATE accounts SET role='admin' WHERE role='member'",
        "UPDATE accounts SET provisioning_state='ready' WHERE role='member'",
    ] {
        let (_temp, root) = initialized();
        let admin = reserve(&root, "setup", None);
        ready(&root, &admin, true);
        let member = reserve(&root, "invite", Some(&admin));
        let c = open(root.database(), false).unwrap();
        // The first phase='closed' case needs an actually contradictory consumed
        // association, so undo just that capability's lifecycle in this fixture.
        if sql == "UPDATE setup SET phase='closed'" {
            c.execute(
                "UPDATE capabilities SET state='provisioning' WHERE kind='setup'",
                [],
            )
            .unwrap();
        }
        c.execute_batch(sql).unwrap();
        drop(c);
        let bytes = metadata_snapshot(&root);
        assert!(root.check_initialized().is_err(), "{sql}");
        assert_eq!(metadata_snapshot(&root), bytes, "{sql}");
        assert!(!root.stores(&member).unwrap().directory.exists());
    }
}

fn config() -> crate::Config {
    crate::Config::new("https://example.test", "127.0.0.1:0".parse().unwrap()).unwrap()
}
const PASSWORD: &str = "synthetic-password-2026";
const REPLACEMENT: &str = "synthetic-replacement-2026";

#[tokio::test]
async fn revision_only_rehash_reopens_with_password_and_existing_session() {
    let (temp, root) = initialized();
    let path = temp.path().join("owner");
    let token = fs::read_to_string(root.setup_file()).unwrap();
    let service = crate::Service::open(root, config()).await.unwrap();
    let ip = "127.0.0.1".parse().unwrap();
    let identity = service
        .claim(ip, "setup", token.trim().into(), "admin", PASSWORD.into())
        .await
        .unwrap();
    let (_, session) = service
        .browser_login(ip, "admin", PASSWORD.into())
        .await
        .unwrap();
    drop(service);

    // Stopped-service compatibility fixture only: a fresh salt for the same
    // password advances hash revision without revoking credential generation.
    let rehashed = tokio::task::spawn_blocking(|| crypto::hash(PASSWORD).unwrap())
        .await
        .unwrap();
    let c = open(path.join("state/accounts.sqlite"), false).unwrap();
    assert_eq!(
        c.execute(
            "UPDATE accounts SET password_phc=?1,password_revision=password_revision+1 WHERE id=?2",
            params![rehashed, identity.id],
        )
        .unwrap(),
        1
    );
    let counters: (i64, i64) = c
        .query_row(
            "SELECT password_revision,generation FROM accounts WHERE id=?1",
            [&identity.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counters, (2, 1));
    drop(c);

    let root = DataRoot::acquire(&path, false).unwrap();
    root.check_initialized().unwrap();
    let service = crate::Service::open(root, config()).await.unwrap();
    let authority = service.browser_authority(&session).await.unwrap();
    assert_eq!(service.me(authority).await.unwrap(), identity);
    let (logged_in, fresh_session) = service
        .browser_login(ip, "admin", PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(logged_in, identity);
    let authority = service.browser_authority(&fresh_session).await.unwrap();
    assert_eq!(service.me(authority).await.unwrap(), identity);
}

fn deferred_commit_failure(root_path: &Path, trigger: &str) {
    let c = rusqlite::Connection::open(root_path.join("state/accounts.sqlite")).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE fixture_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE fixture_commit_guard(parent INTEGER REFERENCES fixture_parent(id) DEFERRABLE INITIALLY DEFERRED);").unwrap();
    c.execute_batch(trigger).unwrap();
}
#[tokio::test]
async fn sqlite_commit_constraint_error_does_not_acknowledge_provisioning() {
    let (temp, root) = initialized();
    let path = temp.path().join("owner");
    let token = fs::read_to_string(root.setup_file()).unwrap();
    let service = crate::Service::open(root, config()).await.unwrap();
    deferred_commit_failure(
        &path,
        "CREATE TRIGGER fixture_fail_ready AFTER UPDATE OF provisioning_state ON accounts WHEN new.provisioning_state='ready' BEGIN INSERT INTO fixture_commit_guard VALUES(7); END;",
    );
    let ip = "127.0.0.1".parse().unwrap();
    assert!(
        service
            .claim(ip, "setup", token.trim().into(), "admin", PASSWORD.into())
            .await
            .is_err()
    );
    let c = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
    let (id,state,cap): (String,String,String) = c.query_row("SELECT a.id,a.provisioning_state,k.state FROM accounts a JOIN capabilities k ON k.reserved_id=a.id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
    assert_eq!(state, "provisioning");
    assert_eq!(cap, "provisioning");
    assert!(
        service
            .browser_login(ip, "admin", PASSWORD.into())
            .await
            .is_err()
    );
    let sentinel = path.join("state/accounts").join(&id).join("media/sentinel");
    private_sentinel(&sentinel);
    c.execute_batch("DROP TRIGGER fixture_fail_ready; DROP TABLE fixture_commit_guard; DROP TABLE fixture_parent;").unwrap();
    drop(c);
    drop(service);
    let root = DataRoot::acquire(&path, false).unwrap();
    let service = crate::Service::open(root, config()).await.unwrap();
    assert_eq!(
        service
            .browser_login(ip, "admin", PASSWORD.into())
            .await
            .unwrap()
            .0
            .id,
        id
    );
    assert!(!service.status().await.unwrap());
    assert_eq!(
        fs::read(sentinel).unwrap(),
        b"synthetic data: preserve exactly\0\xff"
    );
}
#[tokio::test]
async fn sqlite_commit_constraint_error_does_not_acknowledge_credentials() {
    let (temp, root) = initialized();
    let path = temp.path().join("owner");
    let admin = reserve(&root, "setup", None);
    ready(&root, &admin, true);
    let service = crate::Service::open(root, config()).await.unwrap();
    deferred_commit_failure(
        &path,
        "CREATE TRIGGER fixture_fail_credentials AFTER INSERT ON credentials BEGIN INSERT INTO fixture_commit_guard VALUES(7); END;",
    );
    let ip = "127.0.0.1".parse().unwrap();
    assert!(
        service
            .browser_login(ip, "admin", "compatibility-password-2026".into())
            .await
            .is_err()
    );
    assert!(
        service
            .native_login(ip, "admin", "compatibility-password-2026".into())
            .await
            .is_err()
    );
    let c = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
    assert_eq!(
        c.query_row("SELECT count(*) FROM credentials", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    c.execute_batch("DROP TRIGGER fixture_fail_credentials; DROP TABLE fixture_commit_guard; DROP TABLE fixture_parent;").unwrap();
    drop(c);
    // The fixture PHC uses the permanent crypto test's synthetic password.
    // Retry below uses that exact proof and confirms normal publication remains.
    assert!(
        service
            .browser_login(ip, "admin", "compatibility-password-2026".into())
            .await
            .is_ok()
    );
}
#[tokio::test]
async fn ordinary_restart_and_local_admin_recovery_preserve_synthetic_stores() {
    let (temp, root) = initialized();
    let path = temp.path().join("owner");
    let admin = reserve(&root, "setup", None);
    ready(&root, &admin, true);
    let stores = root.stores(&admin).unwrap();
    let sentinels = [
        stores.media.join("blob"),
        stores.generations.join("initial/data"),
        stores.directory.join("unrelated"),
    ];
    for path in &sentinels {
        private_sentinel(path);
    }
    drop(root);
    let service = crate::Service::open(DataRoot::acquire(&path, false).unwrap(), config())
        .await
        .unwrap();
    assert!(!service.status().await.unwrap());
    drop(service);
    crate::Service::recover_admin(
        DataRoot::acquire(&path, false).unwrap(),
        admin.clone(),
        REPLACEMENT.into(),
        true,
    )
    .await
    .unwrap();
    let service = crate::Service::open(DataRoot::acquire(&path, false).unwrap(), config())
        .await
        .unwrap();
    let ip = "127.0.0.1".parse().unwrap();
    assert_eq!(
        service
            .browser_login(ip, "admin", REPLACEMENT.into())
            .await
            .unwrap()
            .0
            .id,
        admin
    );
    assert!(
        service
            .browser_login(ip, "admin", "compatibility-password-2026".into())
            .await
            .is_err()
    );
    for path in &sentinels {
        assert_eq!(
            fs::read(path).unwrap(),
            b"synthetic data: preserve exactly\0\xff"
        );
    }
}

#[test]
fn unexpected_pending_marker_bytes_remain_untouched() {
    let (_temp, root) = initialized();
    let id = reserve(&root, "setup", None);
    root.provision(&id, &budget()).unwrap();
    let stores = root.stores(&id).unwrap();
    fs::remove_file(&stores.current_manifest).unwrap();
    let pending = stores.directory.join(".collection-staging/manifest.pending");
    private_sentinel(&pending);
    assert!(root.provision(&id, &budget()).is_err());
    assert!(!stores.current_manifest.exists());
    assert_eq!(
        fs::read(pending).unwrap(),
        b"synthetic data: preserve exactly\0\xff"
    );
}
#[tokio::test]
async fn startup_checks_all_ready_state_before_resuming_any_reservation() {
    let (_temp, root) = initialized();
    let admin = reserve(&root, "setup", None);
    ready(&root, &admin, true);
    let member = reserve(&root, "invite", Some(&admin));
    let media = root.stores(&admin).unwrap().media;
    let sentinel = media.join("sentinel");
    private_sentinel(&sentinel);
    // Structurally valid SQLite, semantically impossible first-admin lifecycle.
    let c = open(root.database(), false).unwrap();
    c.execute(
        "UPDATE capabilities SET state='provisioning' WHERE kind='setup'",
        [],
    )
    .unwrap();
    drop(c);
    let before = metadata_snapshot(&root);
    let database = root.database();
    let member_stores = root.stores(&member).unwrap();
    assert!(crate::Service::open(root, config()).await.is_err());
    assert_eq!(fs::read(database).unwrap(), before);
    assert!(!member_stores.directory.exists());
    assert_eq!(
        fs::read(sentinel).unwrap(),
        b"synthetic data: preserve exactly\0\xff"
    );
}

#[cfg(feature = "test-hooks")]
#[test]
fn root_parent_sync_precedes_lock_and_initialization_and_failure_is_propagated() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner");
    // Injection at the parent sync boundary refuses acquisition before any
    // initialization. This is ordering/error evidence, not power-loss proof.
    let error =
        crate::test_hooks::storage_failure("root_parent_sync", || DataRoot::acquire(&path, true))
            .err()
            .unwrap();
    assert_eq!(error.code, "provision_unavailable");
    assert!(path.is_dir());
    assert!(!path.join("owner.lock").exists());
    assert!(!path.join(".initialization").exists());
    // Retrying an already-created root must still reach the parent sync.
    assert!(
        crate::test_hooks::storage_failure("root_after_parent_sync", || {
            DataRoot::acquire(&path, false)
        })
        .is_err()
    );
    assert!(!path.join("owner.lock").exists());
    let mut root = DataRoot::acquire(&path, true).unwrap();
    assert!(DataRoot::acquire(&path, false).is_err());
    root.init().unwrap();
    root.check_initialized().unwrap();
    assert!(DataRoot::acquire(&temp.path().join("missing/owner"), true).is_err());
    assert!(!temp.path().join("missing").exists());
}

#[cfg(feature = "test-hooks")]
#[test]
fn early_intent_errors_are_closed_until_explicit_repair_and_preserve_sentinels() {
    for phase in [
        "init_after_intent_directory",
        "init_after_intent_create",
        "init_during_intent_write",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        let sentinel = path.join("unrelated");
        private_sentinel(&sentinel);
        assert!(crate::test_hooks::storage_failure(phase, || root.init()).is_err());
        assert!(root.check_initialized().is_err());
        assert!(root.init().is_err());
        let inode = fs::metadata(path.join("owner.lock")).unwrap();
        drop(root);
        let root = DataRoot::acquire(&path, false).unwrap();
        assert!(root.check_initialized().is_err());
        root.rotate_setup().unwrap();
        root.check_initialized().unwrap();
        assert_eq!(
            fs::read(&sentinel).unwrap(),
            b"synthetic data: preserve exactly\0\xff"
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(path.join("owner.lock")).unwrap().ino(),
            inode.ino()
        );
        let c = inspect(root.database()).unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM setup", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            c.query_row(
                "SELECT count(*) FROM capabilities WHERE kind='setup' AND state='available'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
}

#[cfg(feature = "test-hooks")]
#[test]
fn partial_intent_never_grants_ownership_of_unknown_files_or_existing_state() {
    for unknown in [
        ".initialization/unrelated",
        "state/accounts.sqlite",
        ".initialization/intent",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        assert!(
            crate::test_hooks::storage_failure("init_during_intent_write", || root.init()).is_err()
        );
        if unknown.starts_with("state/") {
            fs::create_dir(root.state()).unwrap();
            fs::set_permissions(root.state(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        let sentinel = path.join(unknown);
        private_sentinel(&sentinel);
        assert!(root.rotate_setup().is_err());
        assert!(root.check_initialized().is_err());
        assert_eq!(
            fs::read(sentinel).unwrap(),
            b"synthetic data: preserve exactly\0\xff"
        );
    }
    // A missing early intent cannot authorize mutation of a real reservation.
    let (_temp, root) = initialized();
    let id = reserve(&root, "setup", None);
    let before = metadata_snapshot(&root);
    fs::remove_file(
        root.state()
            .parent()
            .unwrap()
            .join(".initialization/intent"),
    )
    .unwrap();
    assert!(root.rotate_setup().is_err());
    assert_eq!(metadata_snapshot(&root), before);
    let c = inspect(root.database()).unwrap();
    assert_eq!(
        c.query_row("SELECT id FROM accounts", [], |r| r.get::<_, String>(0))
            .unwrap(),
        id
    );
}

#[cfg(feature = "test-hooks")]
#[test]
fn staging_budget_covers_payload_and_releases_on_each_error_and_adoption() {
    for phase in [
        "stores_after_stage_create",
        "stores_during_stage_write",
        "stores_before_stage_sync",
    ] {
        let (_temp, root) = initialized();
        let id = reserve(&root, "setup", None);
        let budget = budget();
        let before = metadata_snapshot(&root);
        assert!(
            crate::test_hooks::storage_failure(phase, || root.provision(&id, &budget)).is_err()
        );
        assert_eq!(budget.used(), 0);
        let stores = root.stores(&id).unwrap();
        let files = staging_bytes(&stores.directory);
        let held = budget.reserve(collection::INITIAL_TEMP_BYTES + 4096).unwrap();
        assert!(root.provision(&id, &budget).is_err());
        assert_eq!(staging_bytes(&stores.directory), files);
        assert!(!stores.current_manifest.exists());
        assert_eq!(metadata_snapshot(&root), before);
        drop(held);
        root.provision(&id, &budget).unwrap();
        assert_eq!(budget.used(), 0);
        assert_eq!(metadata_snapshot(&root), before);
        root.validate_stores(&id).unwrap();
    }
}
fn staging_bytes(path: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    let mut files = std::collections::BTreeMap::new();
    let staging = path.join(".collection-staging");
    if staging.exists() {
        for entry in fs::read_dir(staging).unwrap() {
            let entry = entry.unwrap();
            files.insert(entry.file_name().into(), fs::read(entry.path()).unwrap());
        }
    }
    files
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn startup_resume_reserves_staging_budget_before_extending_existing_bytes() {
    let (temp, root) = initialized();
    let id = reserve(&root, "setup", None);
    assert!(
        crate::test_hooks::storage_failure("stores_during_stage_write", || root
            .provision(&id, &budget()))
        .is_err()
    );
    let stores = root.stores(&id).unwrap();
    let sentinel = stores.media.join("sentinel");
    private_sentinel(&sentinel);
    let before = metadata_snapshot(&root);
    let staged = staging_bytes(&stores.directory);
    let mut small = config();
    small.temp_bytes = std::num::NonZeroU64::new(1).unwrap();
    assert!(crate::Service::open(root, small).await.is_err());
    assert_eq!(
        fs::read(temp.path().join("owner/state/accounts.sqlite")).unwrap(),
        before
    );
    assert_eq!(staging_bytes(&stores.directory), staged);
    assert!(!stores.current_manifest.exists());
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"synthetic data: preserve exactly\0\xff"
    );
    let root = DataRoot::acquire(&temp.path().join("owner"), false).unwrap();
    let service = crate::Service::open(root, config()).await.unwrap();
    assert_eq!(service.temp.used(), 0);
    assert!(!service.status().await.unwrap());
    assert!(stores.current_manifest.is_file());
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"synthetic data: preserve exactly\0\xff"
    );
    let c = inspect(temp.path().join("owner/state/accounts.sqlite")).unwrap();
    let account: (String, String, String, String) = c
        .query_row(
            "SELECT id,username,password_phc,provisioning_state FROM accounts",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(account, (id, "admin".into(), PHC.into(), "ready".into()));
    assert_eq!(
        c.query_row("SELECT count(*) FROM setup", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn staging_payload_requires_its_full_byte_reservation_before_any_write() {
    let (_temp, root) = initialized();
    let id = reserve(&root, "setup", None);
    root.provision(&id, &budget()).unwrap();
    let stores = root.stores(&id).unwrap();
    // Derive the finite payload from actual service-created files, rather than
    // assume a physical disk capacity or generate a disk-full condition.
    let payload = fs::metadata(stores.directory.join(".collection-staging/owner"))
        .unwrap()
        .len()
        + fs::metadata(&stores.current_manifest).unwrap().len()
        + collection::INITIAL_TEMP_BYTES;
    let (_other_temp, other) = initialized();
    let other_id = reserve(&other, "setup", None);
    let before = metadata_snapshot(&other);
    let too_small = crate::limits::TempBudget::new(std::num::NonZeroU64::new(payload - 1).unwrap());
    assert!(other.provision(&other_id, &too_small).is_err());
    assert_eq!(too_small.used(), 0);
    assert!(!other.stores(&other_id).unwrap().directory.exists());
    assert_eq!(metadata_snapshot(&other), before);
    let exact = crate::limits::TempBudget::new(std::num::NonZeroU64::new(payload).unwrap());
    other.provision(&other_id, &exact).unwrap();
    assert_eq!(exact.used(), 0);
    other.validate_stores(&other_id).unwrap();
    assert_eq!(metadata_snapshot(&other), before);
}

#[cfg(feature="test-hooks")]
#[test]
fn collection_pointer_faults_reopen_only_complete_generations_without_touching_media() {
    for (phase,changed) in [("collection_before_manifest",false),("collection_after_manifest_rename",true),("collection_after_manifest_sync",true)] {
        let (temp,root)=initialized();let id=reserve(&root,"setup",None);ready(&root,&id,true);
        let stores=root.stores(&id).unwrap();let before=fs::read(&stores.current_manifest).unwrap();
        let sentinel=stores.media.join("sentinel");private_sentinel(&sentinel);
        let stage=root.stage_collection(&id).unwrap();
        crate::collection::schema::create_blank(&stage.path,123,456).unwrap();
        assert!(crate::test_hooks::storage_failure(phase,||stage.activate()).is_err());
        assert_eq!(fs::read(&stores.current_manifest).unwrap()!=before,changed);
        drop(root);
        let root=DataRoot::acquire(&temp.path().join("owner"),false).unwrap();root.check_initialized().unwrap();
        root.recover_collection_stages(&id).unwrap();
        let path=root.collection_path(&id).unwrap();let c=crate::collection::schema::open(&path,false).unwrap();
        crate::collection::schema::validate(&c).unwrap();
        if changed {assert_eq!(c.query_row("SELECT scm FROM col",[],|r|r.get::<_,i64>(0)).unwrap(),456);}
        assert_eq!(fs::read(sentinel).unwrap(),b"synthetic data: preserve exactly\0\xff");
    }
}

#[cfg(feature="test-hooks")]
#[test]
fn activation_reports_uncertainty_only_at_pointer_replacement() {
    for (phase,notified) in [("collection_before_manifest",false),("collection_after_manifest_rename",true),("collection_after_manifest_sync",true)] {
        let (_temp,root)=initialized();let id=reserve(&root,"setup",None);ready(&root,&id,true);
        let old=root.collection_path(&id).unwrap();let stage=root.stage_collection(&id).unwrap();let candidate=stage.path.clone();
        crate::collection::schema::create_blank(&candidate,123,456).unwrap();
        let boundary=std::cell::Cell::new(false);
        let result=crate::test_hooks::storage_failure(phase,||stage.activate_notifying(||boundary.set(true)));
        assert!(result.is_err());assert_eq!(boundary.get(),notified);
        assert!(old.exists());assert_eq!(candidate.exists(),notified);
        // A fault after rename is never returned as an acknowledged success.
    }
}
