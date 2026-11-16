// SPDX-License-Identifier: MIT
mod support;

use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use support::*;

async fn native(h: &Harness, method: &str, key: &str, session: &str, body: Value) -> reqwest::Response {
    h.client
        .post(format!("{}/sync/{method}", h.base))
        .header("content-type", "application/octet-stream")
        .header(
            "anki-sync",
            json!({"v":11,"k":key,"c":"synthetic","s":session}).to_string(),
        )
        .body(zstd::stream::encode_all(serde_json::to_vec(&body).unwrap().as_slice(), 0).unwrap())
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .unwrap()
}

async fn exchange(h: &Harness, method: &str, key: &str, session: &str, body: Value) -> Value {
    let response = native(h, method, key, session, body).await;
    // sync/server-transport#T3: protocol compression, without HTTP content coding.
    assert_eq!(response.status(), 200, "/sync/{method} must succeed");
    assert!(response.headers().get("content-encoding").is_none());
    assert_eq!(response.headers().get_all("anki-original-size").iter().count(), 1);
    let length: usize = response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let bytes = response.bytes().await.unwrap();
    let decoded = zstd::stream::decode_all(bytes.as_ref()).unwrap();
    assert_eq!(decoded.len(), length);
    serde_json::from_slice(&decoded).unwrap()
}

async fn login(h: &Harness) -> String {
    exchange(h, "hostKey", "", "login001", json!({"u":"root.admin","p":PASSWORD}))
        .await["key"].as_str().filter(|key| !key.is_empty()).unwrap().to_owned()
}

#[derive(Clone, Debug, PartialEq)]
struct Committed {
    modification: i64,
    usn: i32,
    last_sync: i64,
    tables: BTreeMap<String, Vec<Vec<SqlValue>>>,
}

// Resolve the actual published generation and use a separate read-only SQLite
// connection, never the round's connection or a stamp-mutating full download.
fn committed(h: &Harness, id: &str) -> Committed {
    let account = h.path().join("state/accounts").join(id);
    let manifest = std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation = manifest.lines().find_map(|line| line.strip_prefix("generation=")).unwrap();
    let path = account.join("generations").join(generation).join("collection.anki2");
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    db.create_collation("unicase", |a, b| unicase::UniCase::new(a).cmp(&unicase::UniCase::new(b))).unwrap();
    db.execute_batch("BEGIN").unwrap();
    let (modification, usn, last_sync) = db.query_row("SELECT mod,usn,ls FROM col", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    }).unwrap();
    let mut tables = BTreeMap::new();
    for table in ["col", "notes", "cards", "revlog", "graves", "notetypes", "fields", "templates", "decks", "deck_config", "tags", "config"] {
        let sql = if table == "col" {
            // All remaining collection metadata must stay unchanged.
            "SELECT id,crt,scm,ver,dty,conf,models,decks,dconf,tags FROM col ORDER BY id".into()
        } else {
            format!("SELECT * FROM {table} ORDER BY 1,2")
        };
        let mut statement = db.prepare(&sql).unwrap();
        let columns = statement.column_count();
        let rows = statement.query_map([], |row| {
            (0..columns).map(|i| row.get(i)).collect::<rusqlite::Result<Vec<SqlValue>>>()
        }).unwrap().map(|row| row.unwrap()).collect();
        tables.insert(table.into(), rows);
    }
    Committed { modification, usn, last_sync, tables }
}

async fn start(h: &Harness, key: &str, session: &str, before: &Committed) {
    let meta = exchange(h, "meta", key, session, json!({"v":11,"cv":"synthetic"})).await;
    // sync/server-collection#C1: meta reports committed stamps without writing.
    assert_eq!(meta["mod"], before.modification);
    assert_eq!(meta["usn"], before.usn);
    let graves = exchange(h, "start", key, session,
        json!({"minUsn":before.usn,"lnewer":false,"graves":null})).await;
    // sync/wire-format#W4,W5.1: a fresh collection has no outgoing graves.
    assert_eq!(graves, json!({"cards":[],"decks":[],"notes":[]}));
}

#[tokio::test]
async fn native_empty_normal_round_commits_finish_stamps_and_preserves_collection() {
    let h = Harness::new().await;
    let identity = h.setup().await;
    let id = identity["id"].as_str().unwrap();
    let key = login(&h).await;
    let before = committed(&h, id);
    let session = "empty001";
    start(&h, &key, session, &before).await;

    // sync/normal-sync#N3,N4.2: no pending graves means no applyGraves request.
    let changes = exchange(&h, "applyChanges", &key, session,
        json!({"changes":{"models":[],"decks":[[],[]],"tags":[]}})).await;
    // sync/wire-format#W6: required empty arrays; do not pin optional conf/crt.
    assert_eq!(changes["models"], json!([]));
    assert_eq!(changes["decks"], json!([[],[]]));
    assert_eq!(changes["tags"], json!([]));
    let chunk = exchange(&h, "chunk", &key, session, json!({"_pad":null})).await;
    // sync/wire-format#W7: empty terminal chunk; empty arrays may be omitted.
    assert_eq!(chunk["done"], true);
    for table in ["notes", "cards", "revlog"] {
        if let Some(rows) = chunk.get(table) { assert_eq!(rows, &json!([])); }
    }
    assert_eq!(exchange(&h, "applyChunk", &key, session,
        json!({"chunk":{"done":true}})).await, Value::Null);
    // sync/server-collection#C2 and server-lifecycle#L2: no partial publication.
    assert_eq!(committed(&h, id), before);
    let counts = json!([[0,0,0],0,0,0,0,0,0,0]);
    let sanity = exchange(&h, "sanityCheck2", &key, session, json!({"client":counts})).await;
    // sync/server-collection#C5 and sync/wire-format#W12: six equal counts.
    assert_eq!(sanity["status"], "ok");
    assert_eq!(sanity["c"], counts);
    assert_eq!(sanity["s"], counts);
    assert_eq!(committed(&h, id), before);

    let finish = exchange(&h, "finish", &key, session, json!({"_pad":null})).await;
    let time = finish.as_i64().expect("finish is a strict i64 timestamp");
    assert!(time > 0);
    // sync/server-collection#C5 (Meguri policy), with selected stamp evidence
    // in sync/server-desktop-current#D7: finish commits before acknowledgement.
    let mut expected = before;
    expected.modification = time;
    expected.last_sync = time;
    expected.usn += 1;
    assert_eq!(committed(&h, id), expected);
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    assert_eq!(committed(&h, id), expected);
    h.stop().await;
}

#[tokio::test]
async fn native_abort_rolls_back_received_tag_and_retires_round() {
    let h = Harness::new().await;
    let identity = h.setup().await;
    let id = identity["id"].as_str().unwrap();
    let key = login(&h).await;
    let before = committed(&h, id);
    let session = "abort001";
    start(&h, &key, session, &before).await;
    assert!(before.tables["tags"].is_empty());
    // sync/server-collection#C3: one new, noncolliding synthetic tag is received.
    // D7 observes tag exchange; this test's rollback/retirement is L2 policy.
    exchange(&h, "applyChanges", &key, session,
        json!({"changes":{"models":[],"decks":[[],[]],"tags":["round-only"]}})).await;
    assert_eq!(committed(&h, id), before);
    // sync/server-lifecycle#L2; sync/normal-sync#N10; sync/wire-format#W13.
    assert_eq!(exchange(&h, "abort", &key, session, json!({})).await, Value::Null);
    assert_eq!(committed(&h, id), before);
    assert_eq!(native(&h, "finish", &key, session, json!({"_pad":null})).await.status(), 409);
    assert_eq!(committed(&h, id), before);
    let temp = h.stop().await;
    let h = Harness::reopen(temp).await;
    assert_eq!(committed(&h, id), before);
    h.stop().await;
}
