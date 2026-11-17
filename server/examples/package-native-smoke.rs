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
#[tokio::main]
async fn main() {
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
