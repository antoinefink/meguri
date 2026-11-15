// SPDX-License-Identifier: MIT
mod support;
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
use support::*;

fn recover(path: &Path, id: &str, input: &[u8], reenable: bool) -> Output {
    let mut command = Command::new(binary());
    command.arg("recover-admin").arg(path).arg(ORIGIN).arg(id);
    if reenable {
        command.arg("--reenable");
    }
    let mut child = Child(
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    child.0.stdin.take().unwrap().write_all(input).unwrap();
    // Finite input is closed before waiting. No terminal/workstation input.
    finish(child)
}
fn finish(mut child: Child) -> Output {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "bounded recovery subprocess completion"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    use std::io::Read;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .0
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
    child
        .0
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut stderr)
        .unwrap();
    Output {
        status,
        stdout,
        stderr,
    }
}

fn metadata_state(path: &Path) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    let db = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
    [
        "accounts",
        "credentials",
        "capabilities",
        "provisioning_reservations",
        "setup",
        "service_state",
    ]
    .into_iter()
    .map(|table| {
        let mut q = db
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = q.column_count();
        q.query_map([], |r| {
            (0..columns)
                .map(|i| r.get(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
    })
    .collect()
}
fn credential(path: &Path, id: &str) -> (String, i64, i64, bool) {
    rusqlite::Connection::open(path.join("state/accounts.sqlite"))
        .unwrap()
        .query_row(
            "SELECT password_phc,password_revision,generation,enabled FROM accounts WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap()
}

#[tokio::test]
async fn protected_stdin_rejects_oversized_and_nul_without_hash_or_data_change() {
    let h = Harness::new().await;
    let first = h.setup().await;
    let id = first["id"].as_str().unwrap();
    let admin = h.login("root.admin", PASSWORD).await;
    h.invite(&admin).await;
    h.service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap();
    let stores = h.path().join("state/accounts");
    std::fs::write(
        stores.join(id).join("media/sentinel"),
        b"finite stdin fixture\0\xff",
    )
    .unwrap();
    let bytes = store_bytes(&stores);
    let temp = h.stop().await;
    let path = temp.path().join("owner");
    let before = metadata_state(&path);
    for input in [
        [vec![b'x'; 1024], b"\r\nadditional".to_vec()].concat(),
        [vec![b'x'; 1024], b"\nadditional".to_vec()].concat(),
        vec![b'x'; 1025],
        [vec![b'x'; 1025], b"\r\n".to_vec()].concat(),
        b"valid password\0hidden".to_vec(),
        vec![0xff; 12],
        b"too short\n".to_vec(),
    ] {
        let out = recover(&path, id, &input, false);
        assert!(!out.status.success());
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .contains("validation_failed")
        );
        assert!(out.stdout.is_empty());
        assert_eq!(metadata_state(&path), before);
        assert_eq!(store_bytes(&stores), bytes);
    }
}

#[tokio::test]
async fn protected_stdin_preserves_exact_password_bytes_and_one_transport_newline() {
    let h = Harness::new().await;
    let first = h.setup().await;
    let id = first["id"].as_str().unwrap();
    let temp = h.stop().await;
    let path = temp.path().join("owner");
    let stores = store_bytes(&path.join("state/accounts"));
    for (password, ending) in [
        ("x".repeat(1024), "\r\n"),
        ("é".repeat(512), "\n"),
        ("  exact spaces and unicode é  ".into(), ""),
        ("password ending with CR\r".into(), ""),
        ("password ending with LF\n".into(), "\n"),
    ] {
        let before = credential(&path, id);
        let input = format!("{password}{ending}");
        let out = recover(&path, id, input.as_bytes(), false);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let after = credential(&path, id);
        assert_eq!((after.1, after.2), (before.1 + 1, before.2 + 1));
        assert!(meguri_sync_server::crypto::verify(&password, &after.0));
        assert!(!meguri_sync_server::crypto::verify(
            "different replacement password",
            &after.0
        ));
        assert_eq!(store_bytes(&path.join("state/accounts")), stores);
    }
}

#[tokio::test]
async fn recovery_selects_existing_admin_revokes_related_authority_and_optionally_enables() {
    let h = Harness::new().await;
    let first = h.setup().await;
    let id = first["id"].as_str().unwrap();
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "member").await;
    let (h, second) = h.existing_second_admin(&admin).await;
    let second_cookie = h.login("second.admin", PASSWORD).await;
    let target_reset = h.reset_link(&second_cookie, id).await;
    let issued = h.invite(&admin).await;
    let session = h.login("root.admin", PASSWORD).await;
    let host = h
        .service
        .native_login(IP, "root.admin", PASSWORD.into())
        .await
        .unwrap()
        .1;
    let stores = h.path().join("state/accounts");
    std::fs::write(
        stores.join(id).join("media/sentinel"),
        b"recovery media\0\xff",
    )
    .unwrap();
    std::fs::write(
        stores.join(id).join("generations/substrate-v1/sentinel"),
        b"recovery collection\0\xfe",
    )
    .unwrap();
    let bytes = store_bytes(&stores);
    let temp = h.stop().await;
    let path = temp.path().join("owner");
    let before = metadata_state(&path);
    for invalid in [
        "not-an-id".to_owned(),
        meguri_sync_server::crypto::random_token().unwrap(),
        member["id"].as_str().unwrap().to_owned(),
    ] {
        let out = recover(&path, &invalid, NEW_PASSWORD.as_bytes(), true);
        assert!(!out.status.success());
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .contains("permission_denied")
        );
        assert_eq!(metadata_state(&path), before);
        assert_eq!(store_bytes(&stores), bytes);
    }
    let initial = credential(&path, id);
    assert!(
        recover(&path, id, NEW_PASSWORD.as_bytes(), false)
            .status
            .success()
    );
    let current = credential(&path, id);
    assert_eq!(
        (current.1, current.2, current.3),
        (initial.1 + 1, initial.2 + 1, true)
    );
    assert!(meguri_sync_server::crypto::verify(NEW_PASSWORD, &current.0));
    assert!(!meguri_sync_server::crypto::verify(PASSWORD, &current.0));
    let db = rusqlite::Connection::open(path.join("state/accounts.sqlite")).unwrap();
    for token in [&target_reset, &issued] {
        assert_eq!(
            db.query_row(
                "SELECT state FROM capabilities WHERE hash=?1",
                [meguri_sync_server::crypto::bearer(token)
                    .unwrap()
                    .as_slice()],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "revoked"
        );
    }
    for token in [secret(&admin), secret(&session), &host] {
        assert!(
            db.query_row(
                "SELECT revoked FROM credentials WHERE hash=?1",
                [meguri_sync_server::crypto::bearer(token)
                    .unwrap()
                    .as_slice()],
                |r| r.get::<_, bool>(0)
            )
            .unwrap()
        );
    }
    assert!(
        !db.query_row(
            "SELECT revoked FROM credentials WHERE hash=?1",
            [meguri_sync_server::crypto::bearer(secret(&second_cookie))
                .unwrap()
                .as_slice()],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert_eq!(credential(&path, second["id"].as_str().unwrap()).2, 1);
    db.execute(
        "UPDATE accounts SET enabled=0,generation=generation+1 WHERE id=?1",
        [id],
    )
    .unwrap();
    db.execute("UPDATE service_state SET admission_open=0", [])
        .unwrap();
    let disabled = credential(&path, id);
    drop(db);
    assert!(
        recover(&path, id, PASSWORD.as_bytes(), false)
            .status
            .success()
    );
    assert!(!credential(&path, id).3);
    assert!(
        recover(&path, id, NEW_PASSWORD.as_bytes(), true)
            .status
            .success()
    );
    let enabled = credential(&path, id);
    assert_eq!(
        (enabled.1, enabled.2, enabled.3),
        (disabled.1 + 2, disabled.2 + 2, true)
    );
    assert!(meguri_sync_server::crypto::verify(NEW_PASSWORD, &enabled.0));
    assert_eq!(store_bytes(&stores), bytes);
    let h = Harness::reopen(temp).await;
    assert!(!h.service.admission_open().await.unwrap());
}

#[tokio::test]
async fn protected_stdin_refuses_echoed_private_fixture_terminal_and_public_file() {
    use std::{
        fs::File,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::PermissionsExt,
        },
    };
    let h = Harness::new().await;
    let first = h.setup().await;
    let id = first["id"].as_str().unwrap();
    let temp = h.stop().await;
    let path = temp.path().join("owner");
    let before = metadata_state(&path);
    let input = temp.path().join("input");
    std::fs::write(&input, NEW_PASSWORD).unwrap();
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o644)).unwrap();
    let out = Command::new(binary())
        .arg("recover-admin")
        .arg(&path)
        .arg(ORIGIN)
        .arg(id)
        .stdin(File::open(&input).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = finish(Child(out));
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("permission_denied")
    );
    assert_eq!(metadata_state(&path), before);
    // Own a new pseudoterminal pair; never open /dev/tty or the workstation tty.
    // SAFETY: check all descriptor/results, use initialized buffers, retain the
    // owned master until the child exits, and give only the slave to the child.
    let master_fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    assert!(master_fd >= 0);
    let master = unsafe { File::from_raw_fd(master_fd) };
    assert_eq!(unsafe { libc::grantpt(master.as_raw_fd()) }, 0);
    assert_eq!(unsafe { libc::unlockpt(master.as_raw_fd()) }, 0);
    let mut name = [0 as libc::c_char; 128];
    assert_eq!(
        unsafe { libc::ptsname_r(master.as_raw_fd(), name.as_mut_ptr(), name.len()) },
        0
    );
    let slave_fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        )
    };
    assert!(slave_fd >= 0);
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    let mut term = std::mem::MaybeUninit::<libc::termios>::uninit();
    assert_eq!(
        unsafe { libc::tcgetattr(slave.as_raw_fd(), term.as_mut_ptr()) },
        0
    );
    let mut term = unsafe { term.assume_init() };
    term.c_lflag |= libc::ECHO;
    assert_eq!(
        unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &term) },
        0
    );
    let out = Command::new(binary())
        .arg("recover-admin")
        .arg(&path)
        .arg(ORIGIN)
        .arg(id)
        .stdin(slave)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = finish(Child(out));
    assert!(!out.status.success());
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("permission_denied")
    );
    assert_eq!(metadata_state(&path), before);
    // The documented protected regular-file path also preserves exact input.
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o600)).unwrap();
    let out = Command::new(binary())
        .arg("recover-admin")
        .arg(&path)
        .arg(ORIGIN)
        .arg(id)
        .stdin(File::open(&input).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = finish(Child(out));
    assert!(out.status.success());
    assert!(meguri_sync_server::crypto::verify(
        NEW_PASSWORD,
        &credential(&path, id).0
    ));
}
