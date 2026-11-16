// SPDX-License-Identifier: MIT
#![allow(dead_code)]
use meguri_sync_server::{Config, DataRoot, Service, httpapi};
use reqwest::{Client, Response};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;
use tokio::{sync::oneshot, task::JoinHandle};
pub const ORIGIN: &str = "https://sync.example.test";
pub const PASSWORD: &str = "exact initial password";
pub const NEW_PASSWORD: &str = "replacement password 2";
pub const IP: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
pub struct Harness {
    pub temp: TempDir,
    pub service: Arc<Service>,
    pub client: Client,
    pub base: String,
    pub setup_token: String,
    shutdown: oneshot::Sender<()>,
    server: JoinHandle<std::io::Result<()>>,
}
impl Harness {
    pub async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("owner");
        let root = tokio::task::spawn_blocking(move || {
            let mut root = DataRoot::acquire(&path, true).unwrap();
            root.init().unwrap();
            root
        })
        .await
        .unwrap();
        Self::start(temp, root).await
    }
    pub async fn reopen(temp: TempDir) -> Self {
        let config = Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap();
        Self::reopen_with_config(temp, config).await
    }
    pub async fn reopen_with_config(temp: TempDir, config: Config) -> Self {
        let path = temp.path().join("owner");
        let root = tokio::task::spawn_blocking(move || DataRoot::acquire(&path, false).unwrap())
            .await
            .unwrap();
        Self::start_with_config(temp, root, config).await
    }
    async fn start(temp: TempDir, root: DataRoot) -> Self {
        let config = Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap();
        Self::start_with_config(temp, root, config).await
    }
    async fn start_with_config(temp: TempDir, root: DataRoot, config: Config) -> Self {
        let setup_token = std::fs::read_to_string(root.setup_file())
            .unwrap_or_default()
            .trim()
            .into();
        let service = Service::open(root, config).await.unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown, rx) = oneshot::channel();
        let serving = service.clone();
        let server = tokio::spawn(async move {
            httpapi::serve(listener, serving, async {
                let _ = rx.await;
            })
            .await
        });
        Self {
            temp,
            service,
            base,
            setup_token,
            shutdown,
            server,
            client: Client::builder().no_proxy().build().unwrap(),
        }
    }
    pub fn path(&self) -> PathBuf {
        self.temp.path().join("owner")
    }
    pub fn database(&self) -> PathBuf {
        self.path().join("state/accounts.sqlite")
    }
    pub async fn stop(self) -> TempDir {
        let Self {
            temp,
            service,
            shutdown,
            server,
            ..
        } = self;
        let _ = shutdown.send(());
        server.await.unwrap().unwrap();
        drop(service);
        temp
    }
    pub async fn post(&self, path: &str, body: Value, cookie: Option<&str>) -> Response {
        let mut r = self
            .client
            .post(format!("{}{path}", self.base))
            .header("origin", ORIGIN)
            .json(&body);
        if let Some(c) = cookie {
            r = r.header("cookie", c);
        }
        r.send().await.unwrap()
    }
    pub async fn get(&self, path: &str, cookie: Option<&str>) -> Response {
        let mut r = self.client.get(format!("{}{path}", self.base));
        if let Some(c) = cookie {
            r = r.header("cookie", c);
        }
        r.send().await.unwrap()
    }
    pub async fn setup(&self) -> Value {
        let r = self
            .post(
                "/api/setup",
                json!({"token":self.setup_token,"username":" Root.Admin ","password":PASSWORD}),
                None,
            )
            .await;
        assert_eq!(r.status(), 201);
        r.json::<Value>().await.unwrap()["data"].clone()
    }
    pub async fn login(&self, name: &str, password: &str) -> String {
        let r = self
            .post(
                "/api/login",
                json!({"username":name,"password":password}),
                None,
            )
            .await;
        assert_eq!(r.status(), 200);
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .into()
    }
    pub async fn invite(&self, admin: &str) -> String {
        let r = self
            .post("/api/admin/invitations", json!({}), Some(admin))
            .await;
        assert_eq!(r.status(), 201);
        let b = r.json::<Value>().await.unwrap();
        token(&b["data"]["url"])
    }
    pub async fn member(&self, admin: &str, name: &str) -> Value {
        let token = self.invite(admin).await;
        let r = self
            .post(
                "/api/invitations/claim",
                json!({"token":token,"username":name,"password":PASSWORD}),
                None,
            )
            .await;
        assert_eq!(r.status(), 201);
        r.json::<Value>().await.unwrap()["data"].clone()
    }
    /// Controlled stopped fixture for an existing ready administrator. Preserve
    /// its actual invitation reservation, stores and original password proof.
    /// This is synthetic restored metadata, not an administrator workflow.
    pub async fn existing_second_admin(self, admin: &str) -> (Self, Value) {
        let mut identity = self.member(admin, "second.admin").await;
        let temp = self.stop().await;
        let root = DataRoot::acquire(&temp.path().join("owner"), false).unwrap();
        let db = rusqlite::Connection::open(root.database()).unwrap();
        db.execute_batch("BEGIN IMMEDIATE; DROP TRIGGER identity_immutable;")
            .unwrap();
        db.execute(
            "UPDATE accounts SET role='admin' WHERE id=?1 AND provisioning_state='ready'",
            [identity["id"].as_str().unwrap()],
        )
        .unwrap();
        db.execute_batch("CREATE TRIGGER identity_immutable BEFORE UPDATE OF id,username,role ON accounts BEGIN SELECT RAISE(ABORT,'immutable identity'); END; COMMIT;").unwrap();
        drop(db);
        drop(root);
        identity["role"] = json!("admin");
        (Self::reopen(temp).await, identity)
    }
    pub async fn reset_link(&self, admin: &str, id: &str) -> String {
        let r = self
            .post(
                &format!("/api/admin/accounts/{id}/reset"),
                json!({}),
                Some(admin),
            )
            .await;
        assert_eq!(r.status(), 201);
        token(&r.json::<Value>().await.unwrap()["data"]["url"])
    }
}
pub fn token(value: &Value) -> String {
    value
        .as_str()
        .unwrap()
        .split_once("#token=")
        .unwrap()
        .1
        .into()
}
pub fn secret(cookie: &str) -> &str {
    cookie.split_once('=').unwrap().1
}
pub async fn error(r: Response, status: u16, code: &str) {
    assert_eq!(r.status().as_u16(), status);
    assert_eq!(r.json::<Value>().await.unwrap()["error"]["code"], code);
}
pub fn binary() -> PathBuf {
    std::env::var_os("MEGURI_SERVER_TEST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_meguri-sync-server")))
}
pub struct Child(pub std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
/// Bounded polling of explicit child phase evidence, never a timing-based
/// concurrency assertion. Child phase does not continue until SIGKILL.
pub async fn wait_file(path: &std::path::Path, child: &mut Child) {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            if path.is_file() {
                break;
            }
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "child exited before requested phase"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("child phase evidence");
}

/// Actual bytes of every synthetic account store file, including binary media.
pub fn store_bytes(path: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    fn visit(
        base: &std::path::Path,
        path: &std::path::Path,
        out: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(base, &entry.path(), out);
            } else {
                out.insert(
                    entry.path().strip_prefix(base).unwrap().to_owned(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    visit(path, path, &mut out);
    out
}

pub async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(15), future)
        .await
        .expect("bounded fixture progress")
}
