// SPDX-License-Identifier: MIT
//! Synthetic protocol client for the exact packaged service; never a server hook.
use serde_json::{Value, json};
use std::{io::{Cursor, Read, Write}, time::Duration};

struct Client { http: reqwest::Client, key: String }
impl Client {
    async fn raw(&self, family: &str, method: &str, bytes: &[u8]) -> Vec<u8> {
        let response = self.http.post(format!("http://127.0.0.1:8080/{family}/{method}"))
            .header("anki-sync", json!({"v":11,"k":self.key,"c":"packaging-smoke","s":"packaging"}).to_string())
            .body(zstd::stream::encode_all(bytes, 0).unwrap()).send().await.unwrap();
        assert_eq!(response.status(), 200, "native {family}/{method}");
        assert!(response.headers().get("content-encoding").is_none());
        let size: usize = response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
        let bytes = zstd::stream::decode_all(response.bytes().await.unwrap().as_ref()).unwrap();
        assert_eq!(bytes.len(), size);
        bytes
    }
    async fn json(&self, family: &str, method: &str, value: Value) -> Value {
        let value: Value = serde_json::from_slice(&self.raw(family, method, &serde_json::to_vec(&value).unwrap()).await).unwrap();
        if family == "msync" { assert_eq!(value["err"], ""); value["data"].clone() } else { value }
    }
}
fn database(path: &str) -> rusqlite::Connection {
    let db = rusqlite::Connection::open(path).unwrap();
    db.create_collation("unicase", |a,b| unicase::UniCase::new(a).cmp(&unicase::UniCase::new(b))).unwrap();
    db
}
fn verify_collection(path: &str) {
    let db = database(path);
    assert_eq!(db.query_row("PRAGMA integrity_check", [], |r| r.get::<_,String>(0)).unwrap(), "ok");
    assert_eq!(db.query_row("SELECT guid,flds,data FROM notes WHERE id=101", [], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).unwrap(),
        ("package-guid".into(), "Synthetic front\u{1f}Synthetic back".into(), "package note".into()));
    assert_eq!(db.query_row("SELECT nid,reps FROM cards WHERE id=102", [], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap(), (101,1));
    assert_eq!(db.query_row("SELECT cid,ease FROM revlog WHERE id=103", [], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap(), (102,3));
}

// Runner admission owns the fresh synthetic container, its runtime network
// namespace, the fixture's account binding, and the pre-UI one-card/zero-history
// baseline. Invoke only after the real app journey and ordinary Sign Out.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedReviewIds { card_id: i64, note_id: i64 }

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedReviewFixture {
    run_id: String,
    username: String,
    password: String,
    account_id: String,
}

impl Client {
    // Separate from raw(): this phase must not panic or expose response data.
    async fn owned_raw(&self, method: &str, body: &[u8]) -> Result<Vec<u8>, ()> {
        let (url, limit) = match method {
            "hostKey" => ("http://127.0.0.1:8080/sync/hostKey", 4096usize),
            "download" => ("http://127.0.0.1:8080/sync/download", 8 * 1024 * 1024),
            _ => return Err(()),
        };
        let mut response = self.http.post(url)
            .header("anki-sync", json!({"v":11,"k":self.key,"c":"packaging-smoke","s":"packaging"}).to_string())
            .body(zstd::stream::encode_all(body, 0).map_err(|_| ())?)
            .send().await.map_err(|_| ())?;
        if response.status() != 200 || response.headers().contains_key("content-encoding") {
            return Err(());
        }
        let mut sizes = response.headers().get_all("anki-original-size").iter();
        let size = sizes.next().ok_or(())?.to_str().map_err(|_| ())?;
        if sizes.next().is_some() || size.is_empty() || !size.bytes().all(|b| b.is_ascii_digit()) {
            return Err(());
        }
        let size: usize = size.parse().map_err(|_| ())?;
        if size == 0 || size > limit { return Err(()); }
        let encoded_limit = limit + 65536;
        if response.content_length().is_some_and(|n| n > encoded_limit as u64) { return Err(()); }
        let mut encoded = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
            if chunk.len() > encoded_limit - encoded.len() { return Err(()); }
            encoded.extend_from_slice(&chunk);
        }
        let mut decoder = zstd::stream::read::Decoder::new(Cursor::new(encoded)).map_err(|_| ())?;
        decoder.window_log_max(23).map_err(|_| ())?;
        let mut decoded = Vec::new();
        decoder.take(size as u64 + 1).read_to_end(&mut decoded).map_err(|_| ())?;
        if decoded.len() != size { return Err(()); }
        Ok(decoded)
    }
}

async fn owned_review_readback() -> Result<(), ()> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

    let mut input = Vec::new();
    std::io::stdin().lock().take(257).read_to_end(&mut input).map_err(|_| ())?;
    if input.len() > 256 { return Err(()); }
    let ids: OwnedReviewIds = serde_json::from_slice(&input).map_err(|_| ())?;
    if ids.card_id <= 0 || ids.note_id <= 0 { return Err(()); }

    let directory = std::fs::symlink_metadata("/tmp/native-route").map_err(|_| ())?;
    if !directory.is_dir() || directory.uid() != 10001 || directory.mode() & 0o7777 != 0o700 {
        return Err(());
    }
    let fixture = std::fs::OpenOptions::new().read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open("/tmp/native-route/journey.json").map_err(|_| ())?;
    let metadata = fixture.metadata().map_err(|_| ())?;
    if !metadata.is_file() || metadata.uid() != 10001 || metadata.mode() & 0o7777 != 0o600
        || metadata.nlink() != 1 || metadata.len() > 4096 {
        return Err(());
    }
    let mut bytes = Vec::new();
    fixture.take(4097).read_to_end(&mut bytes).map_err(|_| ())?;
    if bytes.len() > 4096 { return Err(()); }
    let fixture: OwnedReviewFixture = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let suffix = fixture.username.strip_prefix("owned.").ok_or(())?;
    if fixture.run_id.is_empty() || fixture.run_id.len() > 100
        || !fixture.run_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        || suffix.len() != 24 || !suffix.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(());
    }
    // Account IDs and generated passwords use the server's canonical 32-byte
    // base64url format. hostKey does not echo identity; admission binds the ID.
    meguri_sync_server::crypto::bearer(&fixture.account_id).map_err(|_| ())?;
    meguri_sync_server::crypto::bearer(&fixture.password).map_err(|_| ())?;
    let mut client = Client {
        http: reqwest::Client::builder().no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30)).build().map_err(|_| ())?,
        key: String::new(),
    };
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct HostKey { key: String }
    let login = serde_json::to_vec(&json!({"u":fixture.username,"p":fixture.password})).map_err(|_| ())?;
    let host: HostKey = serde_json::from_slice(&client.owned_raw("hostKey", &login).await?).map_err(|_| ())?;
    meguri_sync_server::crypto::bearer(&host.key).map_err(|_| ())?;
    client.key = host.key;
    // Only the committed native download is inspected, never a mounted live DB.
    let downloaded = client.owned_raw("download", b"{}").await?;
    let mut snapshot = tempfile::Builder::new().prefix("owned-review-").suffix(".anki2")
        .tempfile_in("/tmp/native-route").map_err(|_| ())?;
    let verified = (|| -> Result<(), ()> {
        let metadata = snapshot.as_file().metadata().map_err(|_| ())?;
        if !metadata.is_file() || metadata.uid() != 10001 || metadata.mode() & 0o7777 != 0o600
            || metadata.nlink() != 1 { return Err(()); }
        snapshot.write_all(&downloaded).map_err(|_| ())?;
        snapshot.as_file().sync_all().map_err(|_| ())?;
        // Immutable read-only mode prevents SQLite from creating sidecar files,
        // including for a WAL-format main file. Disable executable schema just
        // as the server's collection opener does, and register its collation.
        let mut uri = url::Url::from_file_path(snapshot.path()).map_err(|_| ())?;
        uri.query_pairs_mut().append_pair("immutable", "1");
        use rusqlite::{OpenFlags, config::DbConfig};
        let db = rusqlite::Connection::open_with_flags(uri.as_str(), OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW | OpenFlags::SQLITE_OPEN_URI)
            .map_err(|_| ())?;
        for option in [DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA,
            DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW] {
            db.set_db_config(option, false).map_err(|_| ())?;
        }
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true).map_err(|_| ())?;
        db.pragma_update(None, "temp_store", 2).map_err(|_| ())?;
        db.pragma_update(None, "ignore_check_constraints", true).map_err(|_| ())?;
        db.create_collation("unicase", |a,b| unicase::UniCase::new(a).cmp(&unicase::UniCase::new(b))).map_err(|_| ())?;
        meguri_sync_server::collection::schema::validate(&db).map_err(|_| ())?;
        let counts: (i64,i64,i64) = db.query_row(
            "SELECT (SELECT count(*) FROM cards),(SELECT count(*) FROM notes),(SELECT count(*) FROM revlog)",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_| ())?;
        if counts != (1,1,1) { return Err(()); }
        let card: (i64,i64,i64) = db.query_row("SELECT id,nid,reps FROM cards", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_| ())?;
        let note: i64 = db.query_row("SELECT id FROM notes", [], |r| r.get(0)).map_err(|_| ())?;
        let review: (i64,i64) = db.query_row("SELECT cid,ease FROM revlog", [],
            |r| Ok((r.get(0)?,r.get(1)?))).map_err(|_| ())?;
        if card != (ids.card_id,ids.note_id,1) || note != ids.note_id || review != (ids.card_id,3) {
            return Err(());
        }
        db.close().map_err(|_| ())?;
        Ok(())
    })();
    // Explicit cleanup is part of success; never unlink another fixture file.
    let cleaned = snapshot.close().map_err(|_| ());
    verified.and(cleaned)
}

#[tokio::main]
async fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("owned-review-readback")) {
        let result = if std::env::args_os().count() == 2 { owned_review_readback().await } else { Err(()) };
        if result.is_ok() && writeln!(std::io::stdout().lock(), "owned-review-verified").is_ok() { return; }
        let _ = writeln!(std::io::stderr().lock(), "owned-review-failed");
        std::process::exit(1);
    }
    let phase = std::env::args().nth(1).unwrap();
    assert!(phase == "seed" || phase == "check");
    let mut client = Client { http: reqwest::Client::builder().timeout(Duration::from_secs(30)).build().unwrap(), key: String::new() };
    client.key = client.json("sync", "hostKey", json!({"u":"package.admin","p":"Synthetic package password 2026"})).await["key"].as_str().filter(|s|!s.is_empty()).unwrap().into();
    let path = "/tmp/package.anki2";
    let media = b"Synthetic media bytes\0\xff\x80\r\n";
    if phase == "seed" {
        std::fs::write(path, client.raw("sync", "download", b"{}").await).unwrap();
        let db = database(path);
        db.execute_batch("BEGIN;
            UPDATE col SET mod=456,scm=456,ls=456;
            INSERT INTO notetypes VALUES(1,'Synthetic',123,0,x'0800');
            INSERT INTO fields VALUES(1,0,'Front',x'');
            INSERT INTO fields VALUES(1,1,'Back',x'');
            INSERT INTO templates VALUES(1,0,'Card',123,0,x'0a0973796e746865746963');
            INSERT INTO deck_config VALUES(1,'Preset',123,0,x'42003a004814');
            INSERT INTO decks VALUES(1,'Home',123,0,x'0801',x'0a020801');
            INSERT INTO notes VALUES(101,'package-guid',1,123,0,'','Synthetic front'||char(31)||'Synthetic back','Synthetic front',0,0,'package note');
            INSERT INTO cards VALUES(102,101,1,0,123,0,2,2,123,15,2500,1,0,0,0,0,0,'');
            INSERT INTO revlog VALUES(103,102,0,3,15,14,2500,15000,1);
            COMMIT; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        drop(db);
        assert_eq!(client.raw("sync", "upload", &std::fs::read(path).unwrap()).await, b"OK");
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("0", options).unwrap(); zip.write_all(media).unwrap();
        zip.start_file("_meta", options).unwrap(); zip.write_all(br#"[["package.bin","0"]]"#).unwrap();
        let bytes = client.raw("msync", "uploadChanges", &zip.finish().unwrap().into_inner()).await;
        let envelope: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(envelope["err"], ""); assert_eq!(envelope["data"], json!([1,1]));
    }
    // A new client reads actual content, never just the metadata's empty flag.
    std::fs::write(path, client.raw("sync", "download", b"{}").await).unwrap();
    verify_collection(path);
    let begin = client.json("msync", "begin", json!({"v":"packaging-smoke"})).await;
    assert_eq!(begin["usn"], 1);
    let changes = client.json("msync", "mediaChanges", json!({"lastUsn":0})).await;
    assert_eq!(changes.as_array().unwrap().len(), 1); assert_eq!(changes[0][0], "package.bin");
    let bytes = client.raw("msync", "downloadFiles", br#"{"files":["package.bin"]}"#).await;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut downloaded = Vec::new(); zip.by_name("0").unwrap().read_to_end(&mut downloaded).unwrap();
    assert_eq!(downloaded, media);
    let mut metadata = String::new(); zip.by_name("_meta").unwrap().read_to_string(&mut metadata).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&metadata).unwrap(), json!({"0":"package.bin"}));
    assert_eq!(client.json("msync", "mediaSanity", json!({"local":1})).await, "OK");
    println!("native collection, review and media bytes verified: {phase}");
}
