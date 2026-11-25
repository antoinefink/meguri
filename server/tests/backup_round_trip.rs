// SPDX-License-Identifier: MIT
//! Synthetic recovery through the production scheduler and public HTTP routes.
mod support;

use meguri_sync_server::{Config, DataRoot, Service, collection::schema, httpapi};
use reqwest::Client;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};
use std::{collections::BTreeMap, io::{Read, Write}, path::Path, time::Duration};
use support::*;

async fn native(client: &Client, base: &str, family: &str, method: &str, key: &str, bytes: &[u8]) -> reqwest::Response {
    client.post(format!("{base}/{family}/{method}"))
        .header("anki-sync", json!({"v":11,"k":key,"c":"synthetic-backup","s":""}).to_string())
        .body(zstd::stream::encode_all(bytes, 0).unwrap())
        .send().await.unwrap()
}

async fn decoded(response: reqwest::Response) -> Vec<u8> {
    assert_eq!(response.status(), 200);
    let length: usize = response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let result = zstd::stream::decode_all(response.bytes().await.unwrap().as_ref()).unwrap();
    assert_eq!(result.len(), length);
    result
}

async fn media(client: &Client, base: &str, key: &str, method: &str, bytes: &[u8]) -> Value {
    let bytes = decoded(native(client, base, "msync", method, key, bytes).await).await;
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result["err"], "");
    result.as_object().unwrap().get("data").unwrap().clone()
}

fn media_batch(items: &[(&str, Option<&[u8]>)]) -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut metadata = Vec::new();
    for (index, (name, bytes)) in items.iter().enumerate() {
        let member = index.to_string();
        if let Some(bytes) = bytes {
            archive.start_file(&member, options).unwrap();
            archive.write_all(bytes).unwrap();
            metadata.push(json!([name, member]));
        } else {
            metadata.push(json!([name, null]));
        }
    }
    archive.start_file("_meta", options).unwrap();
    archive.write_all(&serde_json::to_vec(&metadata).unwrap()).unwrap();
    archive.finish().unwrap().into_inner()
}

fn collection(path: &Path, changed: bool) -> Vec<u8> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(path).unwrap();
    schema::create_blank(path, 123, 456).unwrap();
    let c = schema::open(path, true).unwrap();
    c.execute_batch("BEGIN;
        INSERT INTO notetypes VALUES(1,'Recovery fixture',123,0,x'');
        INSERT INTO fields VALUES(1,0,'Prompt',x'');
        INSERT INTO fields VALUES(1,1,'Response',x'');
        INSERT INTO templates VALUES(1,0,'Recovery card',123,0,x'');
        INSERT INTO decks VALUES(1,'Recovery deck',123,0,x'0801',x'0a020801');
        INSERT INTO notes VALUES(101,'backup-fixture',1,123,0,' recovery ','Before'||char(31)||'Original answer','Before',0,0,'');
        INSERT INTO cards VALUES(201,101,1,0,123,0,2,2,42,17,2500,4,1,0,0,0,0,'');
        INSERT INTO revlog VALUES(301,201,0,3,17,8,2500,12000,1);
        COMMIT;").unwrap();
    if changed {
        c.execute_batch("UPDATE notes SET flds='After'||char(31)||'Changed answer',sfld='After' WHERE id=101;
            UPDATE cards SET due=90,ivl=45 WHERE id=201;
            INSERT INTO notes VALUES(102,'later-fixture',1,124,0,'','Later note','Later note',0,0,'');
            INSERT INTO revlog VALUES(302,201,0,4,45,17,2500,9000,1);").unwrap();
    }
    schema::checkpoint(&c).unwrap();
    drop(c);
    std::fs::read(path).unwrap()
}

// Compare complete user rows rather than SQLite bytes: download/restore may
// legitimately advance collection sync stamps.
fn study_rows(path: &Path) -> BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    let c = rusqlite::Connection::open(path).unwrap();
    meguri_sync_server::collection::unicase::register(&c).unwrap();
    assert_eq!(c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)).unwrap(), "ok");
    let mut result = BTreeMap::new();
    for table in ["notes", "cards", "revlog"] {
        let mut query = c.prepare(&format!("SELECT * FROM {table} ORDER BY id")).unwrap();
        let columns = query.column_count();
        let rows = query.query_map([], |row| {
            (0..columns).map(|column| row.get(column)).collect::<rusqlite::Result<Vec<_>>>()
        }).unwrap().map(|row| row.unwrap()).collect();
        result.insert(table.to_owned(), rows);
    }
    result
}

async fn download_rows(h: &Harness, key: &str, label: &str) -> BTreeMap<String, Vec<Vec<rusqlite::types::Value>>> {
    let bytes = decoded(native(&h.client, &h.base, "sync", "download", key, b"{}").await).await;
    let path = h.temp.path().join(format!("{label}.anki2"));
    std::fs::write(&path, bytes).unwrap();
    study_rows(&path)
}

async fn download_media(h: &Harness, key: &str, names: &[&str]) -> BTreeMap<String, Vec<u8>> {
    let request = serde_json::to_vec(&json!({"files":names})).unwrap();
    let bytes = decoded(native(&h.client, &h.base, "msync", "downloadFiles", key, &request).await).await;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(), names.len() + 1);
    let metadata: BTreeMap<String, String> = serde_json::from_reader(archive.by_name("_meta").unwrap()).unwrap();
    assert_eq!(metadata.len(), names.len());
    let mut result = BTreeMap::new();
    for (index, name) in names.iter().enumerate() {
        let member = index.to_string();
        assert_eq!(metadata[&member], *name);
        let mut bytes = Vec::new();
        archive.by_name(&member).unwrap().read_to_end(&mut bytes).unwrap();
        result.insert((*name).to_owned(), bytes);
    }
    result
}

async fn backups(h: &Harness, cookie: &str) -> Vec<Value> {
    let response = h.get("/api/me/backups", Some(cookie)).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    body["data"].as_array().expect("backup history is an array").clone()
}

async fn scheduled_backups(h: &Harness, cookie: &str) -> Vec<Value> {
    within(async {
        loop {
            let points = backups(h, cookie).await;
            if !points.is_empty() { return points; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await
}

#[tokio::test]
async fn startup_scheduler_captures_idle_account_and_cookie_restore_recovers_downloaded_study_and_media() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("before.anki2");
        let before = collection(&source, false);
        let expected = study_rows(&source);
        let after_path = temp.path().join("after.anki2");
        let after = collection(&after_path, true);
        let changed = study_rows(&after_path);
        assert_ne!(expected, changed);
        let original_media: &[(&str, Option<&[u8]>)] = &[
            ("changed.bin", Some(b"original media\0\xff")),
            ("removed.bin", Some(b"media to recover\x80")),
        ];

        let path = temp.path().join("owner");
        let mut root = DataRoot::acquire(&path, true).unwrap();
        root.init().unwrap();
        let setup = std::fs::read_to_string(root.setup_file()).unwrap();
        let service = Service::open(root, Config::new(ORIGIN, "127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        service.claim(IP, "setup", setup.trim().into(), "root.admin", PASSWORD.into()).await.unwrap();
        let (_, token) = service.browser_login(IP, "root.admin", PASSWORD.into()).await.unwrap();
        let authority = service.browser_authority(&token).await.unwrap();
        let invitation = service.invitation(authority).await.unwrap();
        let invitation = invitation.url.split_once("#token=").unwrap().1.to_owned();
        service.claim(IP, "invite", invitation, "idle.member", PASSWORD.into()).await.unwrap();
        let (_, key) = service.native_login(IP, "idle.member", PASSWORD.into()).await.unwrap();

        // Mount the real routes without serve's scheduler. Both accounts are
        // ready and all payloads committed before the first scheduled sweep.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let router = httpapi::router(service.clone());
        let (shutdown, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router.into_make_service_with_connect_info::<std::net::SocketAddr>())
                .with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
        });
        let client = Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap();
        assert_eq!(decoded(native(&client, &base, "sync", "upload", &key, &before).await).await, b"OK");
        assert_eq!(media(&client, &base, &key, "uploadChanges", &media_batch(original_media)).await, json!([2,2]));
        shutdown.send(()).unwrap();
        server.await.unwrap();
        drop(client);
        drop(service);

        // This restart uses the production serve lifecycle and actual scheduler.
        // The target has no browser session until after startup.
        let h = Harness::reopen(temp).await;
        let admin = h.login("root.admin", PASSWORD).await;
        // Observe the sweep through the other account first, keeping the target
        // idle. Pace requests and bound the entire journey; never spin or retry
        // a failed HTTP response.
        scheduled_backups(&h, &admin).await;
        let cookie = h.login("idle.member", PASSWORD).await;
        let points = scheduled_backups(&h, &cookie).await;
        let point = &points[0];
        for field in ["id", "kind", "created_at", "size_bytes"] {
            assert!(point.get(field).is_some(), "missing public backup field {field}");
        }
        let id = point["id"].as_str().unwrap();
        assert!(!id.is_empty());
        assert!(!point["kind"].as_str().unwrap().is_empty());
        assert!(!point["created_at"].as_str().unwrap().is_empty());
        assert!(point["size_bytes"].as_u64().unwrap() > 0);
        let restore = format!("/api/me/backups/{id}/restore");

        let login = serde_json::to_vec(&json!({"u":"idle.member","p":PASSWORD})).unwrap();
        let login = decoded(native(&h.client, &h.base, "sync", "hostKey", "", &login).await).await;
        let key = serde_json::from_slice::<Value>(&login).unwrap()["key"].as_str().unwrap().to_owned();
        assert_eq!(download_rows(&h, &key, "seeded").await, expected);
        assert_eq!(decoded(native(&h.client, &h.base, "sync", "upload", &key, &after).await).await, b"OK");
        let mutations: &[(&str, Option<&[u8]>)] = &[
            ("changed.bin", Some(b"replacement media")),
            ("removed.bin", None),
            ("added.bin", Some(b"later media")),
        ];
        let result = media(&h.client, &h.base, &key, "uploadChanges", &media_batch(mutations)).await;
        assert_eq!(result[0], 3);
        let cursor = result[1].as_i64().unwrap();
        assert_eq!(download_rows(&h, &key, "mutated").await, changed);
        assert_eq!(download_media(&h, &key, &["changed.bin", "added.bin"]).await,
            BTreeMap::from([("changed.bin".into(), b"replacement media".to_vec()), ("added.bin".into(), b"later media".to_vec())]));

        // Even the administrator cannot restore another account's opaque ID.
        let denied = h.post(&restore, json!({}), Some(&admin)).await;
        assert!(matches!(denied.status().as_u16(), 403 | 404));
        assert_eq!(download_rows(&h, &key, "denied").await, changed);
        let response = h.post(&restore, json!({}), Some(&cookie)).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.json::<Value>().await.unwrap(), json!({"data":null}));
        assert_eq!(download_rows(&h, &key, "restored").await, expected);
        let expected_media = original_media.iter().map(|(name, bytes)| ((*name).to_owned(), bytes.unwrap().to_vec())).collect::<BTreeMap<_, _>>();
        assert_eq!(download_media(&h, &key, &["changed.bin", "removed.bin"]).await, expected_media);
        assert_eq!(native(&h.client, &h.base, "msync", "downloadFiles", &key, br#"{"files":["added.bin"]}"#).await.status(), 409);
        let changes = media(&h.client, &h.base, &key, "mediaChanges", &serde_json::to_vec(&json!({"lastUsn":cursor})).unwrap()).await;
        let changes = changes.as_array().unwrap();
        for (name, bytes) in original_media {
            let row = changes.iter().find(|row| row[0] == *name).expect("restored media must reconcile after the old cursor");
            assert!(row[1].as_i64().unwrap() > cursor);
            let digest = Sha1::digest(bytes.unwrap()).iter().map(|byte| format!("{byte:02x}")).collect::<String>();
            assert_eq!(row[2], digest);
        }
        let removed = changes.iter().find(|row| row[0] == "added.bin").expect("post-backup addition must reconcile as deleted");
        assert!(removed[1].as_i64().unwrap() > cursor);
        assert_eq!(removed[2], "");

        let h = Harness::reopen(h.stop().await).await;
        assert_eq!(h.get("/api/me", Some(&cookie)).await.status(), 200);
        assert_eq!(download_rows(&h, &key, "cold-restored").await, expected);
        assert_eq!(download_media(&h, &key, &["changed.bin", "removed.bin"]).await, expected_media);
        let _ = h.stop().await;
    }).await.expect("bounded public backup recovery journey");
}
