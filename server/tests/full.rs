// SPDX-License-Identifier: MIT
mod support;
use serde_json::{Value, json};
use support::*;

async fn native(h: &Harness, method: &str, key: &str, bytes: &[u8]) -> reqwest::Response {
    h.client.post(format!("{}/sync/{method}", h.base))
        .header("anki-sync", json!({"v":11,"k":key,"c":"synthetic","s":"local"}).to_string())
        .body(zstd::stream::encode_all(bytes, 0).unwrap())
        .send().await.unwrap()
}
async fn decoded(response: reqwest::Response) -> Vec<u8> {
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("content-encoding").is_none());
    let length: usize = response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let bytes = response.bytes().await.unwrap();
    let result = zstd::stream::decode_all(bytes.as_ref()).unwrap();
    assert_eq!(result.len(), length);
    result
}
async fn login(h: &Harness) -> String {
    let bytes = decoded(native(h, "hostKey", "", &serde_json::to_vec(&json!({"u":"root.admin","p":PASSWORD})).unwrap()).await).await;
    serde_json::from_slice::<Value>(&bytes).unwrap()["key"].as_str().unwrap().into()
}

#[tokio::test]
async fn authenticated_fresh_account_serves_valid_blank_schema_and_read_only_meta() {
    let h = Harness::new().await;
    h.setup().await;
    let key = login(&h).await;
    let request = br#"{"v":11,"cv":"synthetic"}"#;
    let first: Value = serde_json::from_slice(&decoded(native(&h, "meta", &key, request).await).await).unwrap();
    assert_eq!(first["mod"], 0);
    assert_eq!(first["usn"], 0);
    assert_eq!(first["empty"], true);
    assert_eq!(first["cont"], true);
    assert_eq!(first["hostNum"], 0);
    let second: Value = serde_json::from_slice(&decoded(native(&h, "meta", &key, request).await).await).unwrap();
    for name in ["mod", "scm", "usn"] { assert_eq!(first[name], second[name]); }
    let bytes = decoded(native(&h, "download", &key, br#"{"_pad":null}"#).await).await;
    let path = h.temp.path().join("blank.anki2");
    std::fs::write(&path, bytes).unwrap();
    let db = rusqlite::Connection::open(path).unwrap();
    db.create_collation("unicase", |a,b| unicase::UniCase::new(a).cmp(&unicase::UniCase::new(b))).unwrap();
    assert_eq!(db.query_row("PRAGMA integrity_check", [], |r| r.get::<_,String>(0)).unwrap(), "ok");
    assert_eq!(db.query_row("SELECT ver,usn,ls FROM col", [], |r| Ok((r.get::<_,i64>(0)?, r.get::<_,i32>(1)?, r.get::<_,i64>(2)?))).unwrap(), (18,1,0));
    for table in ["notes", "cards", "revlog", "graves", "notetypes"] {
        assert_eq!(db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_,i64>(0)).unwrap(),0);
    }
    assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('accounts','credentials','capabilities')", [], |r| r.get::<_,i64>(0)).unwrap(),0);
}

fn fixture(h:&Harness)->std::path::PathBuf {
    use std::os::unix::fs::OpenOptionsExt;
    let path=h.temp.path().join("synthetic.anki2");
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
    meguri_sync_server::collection::schema::create_blank(&path,123,456).unwrap();
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.execute_batch("BEGIN;
        UPDATE col SET mod=9007199254740993,scm=9007199254740993,ls=9007199254740993,usn=1;
        INSERT INTO notetypes VALUES(9007199254740993,'Synthetic',123,0,x'0800fa0f02ffff');
        INSERT INTO fields VALUES(9007199254740993,0,'Front',x'');
        INSERT INTO fields VALUES(9007199254740993,1,'Back',x'');
        INSERT INTO templates VALUES(9007199254740993,0,'Card',123,0,x'0a0973796e746865746963');
        INSERT INTO deck_config VALUES(1,'Preset',123,0,x'42003a004814');
        INSERT INTO decks VALUES(1,'Home',123,0,x'0801',x'0a020801');
        INSERT INTO decks VALUES(2,'Filtered',123,0,x'0801',x'12020801');
        INSERT INTO notes VALUES(9007199254740995,'synthetic-guid',9007199254740993,123,0,' tag ','<b>私</b>'||char(31)||'raw', '私',1270766077,7,'opaque note data');
        INSERT INTO cards VALUES(9007199254740997,9007199254740995,2,0,123,0,2,2,123,15,2500,3,1,0,50,1,7,'{ \"unknown\" : 1.10 }');
        INSERT INTO revlog VALUES(9007199254740999,9007199254740997,0,3,15,14,2500,15000,1);
        INSERT INTO tags VALUES('Straße',0,1,x'00ff');
        INSERT INTO graves VALUES(9223372036854775807,0,0);
        INSERT INTO config VALUES('opaque',-1,123,x'7b202276223a2039303037313939323534373430393933207d');
        INSERT INTO config VALUES('text-storage',-1,123,'unchanged non-blob value');
        CREATE TABLE addon_bytes(value BLOB);
        INSERT INTO addon_bytes VALUES(x'00ff00');
        CREATE VIEW addon_view AS SELECT value FROM addon_bytes;
        CREATE TRIGGER addon_stamp AFTER UPDATE ON col BEGIN DELETE FROM addon_bytes; END;
        COMMIT;") .unwrap();
    meguri_sync_server::collection::schema::checkpoint(&c).unwrap();drop(c);
    path
}
fn logical(path:&std::path::Path)->std::collections::BTreeMap<String,Vec<Vec<rusqlite::types::Value>>> {
    let c=rusqlite::Connection::open(path).unwrap();
    meguri_sync_server::collection::unicase::register(&c).unwrap();
    let mut out=std::collections::BTreeMap::new();
    for table in meguri_sync_server::collection::schema::TABLES.iter().copied().chain(["addon_bytes"]) {
        if table=="col" {continue;}
        let sql=if table=="addon_bytes" {"SELECT value FROM addon_bytes ORDER BY value".into()} else {format!("SELECT * FROM {table} ORDER BY 1,2")};
        let mut q=c.prepare(&sql).unwrap();
        let n=q.column_count();
        let rows=q.query_map([],|r|(0..n).map(|i|r.get(i)).collect::<rusqlite::Result<Vec<_>>>()).unwrap().map(|r|r.unwrap()).collect();
        out.insert(table.into(),rows);
    }
    out
}
fn objects(path:&std::path::Path)->Vec<(String,String,String,Option<String>)> {
    let c=rusqlite::Connection::open(path).unwrap();
    c.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name").unwrap()
        .query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap().map(|r|r.unwrap()).collect()
}
#[tokio::test]
async fn upload_download_preserves_all_user_rows_unknown_blobs_schema_and_exact_meta() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    let path=fixture(&h);let expected=logical(&path);let schema=objects(&path);
    let bytes=std::fs::read(&path).unwrap();
    assert_eq!(decoded(native(&h,"upload",&key,&bytes).await).await,b"OK");
    let meta:Value=serde_json::from_slice(&decoded(native(&h,"meta",&key,br#"{"v":11,"cv":"x"}"#).await).await).unwrap();
    assert_eq!(meta["mod"].as_i64(),Some(9007199254740993));
    assert_eq!(meta["scm"].as_i64(),Some(9007199254740993));
    assert_eq!(meta["empty"],false);
    for (n,usn) in [(1,2),(2,3)] {
        let bytes=decoded(native(&h,"download",&key,b"{}").await).await;
        let download=h.temp.path().join(format!("download-{n}.anki2"));std::fs::write(&download,&bytes).unwrap();
        assert_eq!(logical(&download),expected);
        assert_eq!(objects(&download),schema);
        let c=rusqlite::Connection::open(&download).unwrap();
        assert_eq!(c.query_row("SELECT crt,scm,ls,usn FROM col",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i32>(3)?))).unwrap(),(123,9007199254740993,9007199254740993,usn));
        assert!(!download.with_extension("anki2-wal").exists());
    }
    let temp=h.stop().await;let h=Harness::reopen(temp).await;
    let key=login(&h).await;
    assert_eq!(native(&h,"meta",&key,br#"{"v":11,"cv":"x"}"#).await.status(),200);
}
#[tokio::test]
async fn corrupt_unsupported_truncated_and_budget_rejections_keep_committed_generation() {
    let h=Harness::new().await;let identity=h.setup().await;let key=login(&h).await;
    let path=fixture(&h);let bytes=std::fs::read(&path).unwrap();
    assert_eq!(decoded(native(&h,"upload",&key,&bytes).await).await,b"OK");
    let account=h.path().join("state/accounts").join(identity["id"].as_str().unwrap());
    let pointer=std::fs::read(account.join("current-collection")).unwrap();
    let before=store_bytes(&account);
    for input in [b"broken SQLite".to_vec(),bytes[..100].to_vec()] {
        assert_eq!(native(&h,"upload",&key,&input).await.status(),400);
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),pointer);
        assert_eq!(store_bytes(&account),before);
        assert_eq!(h.service.temp.used(),0);
    }
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.execute("UPDATE col SET ver=19",[]).unwrap();meguri_sync_server::collection::schema::checkpoint(&c).unwrap();drop(c);
    assert_eq!(native(&h,"upload",&key,&std::fs::read(&path).unwrap()).await.status(),400);
    assert_eq!(store_bytes(&account),before);
    let held=h.service.temp.reserve(h.service.config.temp_bytes.get()).unwrap();
    assert_eq!(native(&h,"upload",&key,&bytes).await.status(),503);
    assert_eq!(store_bytes(&account),before);drop(held);
    assert_eq!(h.service.temp.used(),0);
}
#[tokio::test]
async fn accounts_never_select_each_others_collection() {
    let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    h.member(&admin,"member").await;
    let key=login(&h).await;let path=fixture(&h);
    assert_eq!(decoded(native(&h,"upload",&key,&std::fs::read(path).unwrap()).await).await,b"OK");
    let member=h.service.native_login(IP,"member",PASSWORD.into()).await.unwrap().1;
    let meta:Value=serde_json::from_slice(&decoded(native(&h,"meta",&member,br#"{"v":11,"cv":"x"}"#).await).await).unwrap();
    assert_eq!(meta["mod"],0);assert_eq!(meta["empty"],true);
}

#[cfg(feature="test-hooks")]
fn pending_native(h:&Harness,method:&str,key:&str,bytes:&[u8])->tokio::task::JoinHandle<reqwest::Response> {
    let url=format!("{}/sync/{method}",h.base);let client=h.client.clone();
    let header=json!({"v":11,"k":key,"c":"synthetic","s":"local"}).to_string();
    let body=zstd::stream::encode_all(bytes,0).unwrap();
    tokio::spawn(async move {client.post(url).header("anki-sync",header).body(body).send().await.unwrap()})
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn download_pins_committed_wal_pages_before_concurrent_replacement() {
    let h=Harness::new().await;h.setup().await;let key=login(&h).await;
    let authority=h.service.validate_hostkey(&key).await.unwrap();let stores=h.service.account_stores(&authority).await.unwrap();
    let path=stores.generations.join("initial/collection.anki2");
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.pragma_update(None,"wal_autocheckpoint",0).unwrap();
    c.execute("INSERT INTO config VALUES('wal-only',0,0,?1)",[b"9007199254740993".as_slice()]).unwrap();
    assert!(path.with_extension("anki2-wal").metadata().unwrap().len()>0);
    let pause=h.service.hooks.arm("collection_snapshot_pinned");
    let download=pending_native(&h,"download",&key,b"{}");within(pause.arrived()).await;
    let replacement=fixture(&h);
    assert_eq!(decoded(native(&h,"upload",&key,&std::fs::read(replacement).unwrap()).await).await,b"OK");
    pause.resume();let bytes=decoded(within(download).await.unwrap()).await;
    let path=h.temp.path().join("wal-download.anki2");std::fs::write(&path,bytes).unwrap();
    let downloaded=rusqlite::Connection::open(&path).unwrap();
    assert_eq!(downloaded.query_row("SELECT val FROM config WHERE KEY='wal-only'",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),b"9007199254740993");
    assert_eq!(downloaded.query_row("SELECT count(*) FROM notes",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    drop(c);
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn revocation_after_validation_prevents_actual_manifest_publication() {
    let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    let member=h.member(&admin,"member").await;
    let key=h.service.native_login(IP,"member",PASSWORD.into()).await.unwrap().1;
    let account=h.path().join("state/accounts").join(member["id"].as_str().unwrap());
    let before=std::fs::read(account.join("current-collection")).unwrap();
    let fixture=fixture(&h);let pause=h.service.hooks.arm("collection_after_validation");
    let upload=pending_native(&h,"upload",&key,&std::fs::read(fixture).unwrap());within(pause.arrived()).await;
    assert_eq!(h.post(&format!("/api/admin/accounts/{}/disable",member["id"].as_str().unwrap()),json!({}),Some(&admin)).await.status(),200);
    pause.resume();assert_eq!(within(upload).await.unwrap().status(),403);
    assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),before);
    assert_eq!(h.service.temp.used(),0);
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn publication_owns_account_before_revoker_and_never_holds_shared_writer_while_waiting() {
    let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    let member=h.member(&admin,"member").await;
    let key=h.service.native_login(IP,"member",PASSWORD.into()).await.unwrap().1;
    let authority=h.service.validate_hostkey(&key).await.unwrap();let gate=h.service.publication_barrier(&authority).await.unwrap();
    let fixture=fixture(&h);let pause=h.service.hooks.arm("collection_with_barrier");
    let upload=pending_native(&h,"upload",&key,&std::fs::read(fixture).unwrap());within(pause.arrived()).await;
    // This explicit metadata writer succeeds while collection work is paused
    // under its barrier: the collection path has not acquired a shared writer.
    let c=rusqlite::Connection::open(h.database()).unwrap();c.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();drop(c);
    let actor=h.service.browser_authority(secret(&admin)).await.unwrap();let service=h.service.clone();let id=member["id"].as_str().unwrap().to_owned();
    let disable=tokio::spawn(async move {service.disable(actor,id).await});within(gate.waiter_pending()).await;
    pause.resume();assert_eq!(decoded(within(upload).await.unwrap()).await,b"OK");within(disable).await.unwrap().unwrap();
    assert!(h.service.validate_hostkey(&key).await.is_err());
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn process_interruption_at_collection_publication_resolves_old_or_new_complete_file() {
    use std::{io::BufRead,process::{Command,Stdio}};
    for (phase,changed) in [("collection_before_stage_sync",false),("collection_before_manifest",false),("collection_after_manifest_rename",true),("collection_after_manifest_sync",true)] {
        let h=Harness::new().await;let identity=h.setup().await;let key=login(&h).await;
        let fixture=fixture(&h);let upload=std::fs::read(&fixture).unwrap();let expected=logical(&fixture);
        let account=h.path().join("state/accounts").join(identity["id"].as_str().unwrap());
        let pointer=std::fs::read(account.join("current-collection")).unwrap();
        let sentinel=account.join("media/sentinel");std::fs::write(&sentinel,b"independent media\0\xff").unwrap();
        let temp=h.stop().await;let path=temp.path().join("owner");let signal=temp.path().join("collection-phase");
        let mut child=Child(Command::new(binary()).args(["serve",path.to_str().unwrap(),ORIGIN,"127.0.0.1:0"])
            .env("MEGURI_TEST_KILL_PHASE",phase).env("MEGURI_TEST_SIGNAL_FILE",&signal)
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
        let stdout=child.0.stdout.take().unwrap();
        let line=within(tokio::task::spawn_blocking(move || {let mut line=String::new();std::io::BufReader::new(stdout).read_line(&mut line).unwrap();line})).await.unwrap();
        let address=line.trim().strip_prefix("listening ").unwrap().to_owned();
        let header=json!({"v":11,"k":key,"c":"synthetic","s":"local"}).to_string();
        let request=tokio::spawn(async move {reqwest::Client::builder().no_proxy().build().unwrap().post(format!("http://{address}/sync/upload")).header("anki-sync",header).body(zstd::stream::encode_all(upload.as_slice(),0).unwrap()).send().await});
        wait_file(&signal,&mut child).await;child.0.kill().unwrap();child.0.wait().unwrap();
        assert!(within(request).await.unwrap().is_err());drop(child);
        let h=Harness::reopen(temp).await;
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap()!=pointer,changed);
        let authority=h.service.validate_hostkey(&key).await.unwrap();let stores=h.service.account_stores(&authority).await.unwrap();
        let text=std::fs::read_to_string(&stores.current_manifest).unwrap();let generation=text.lines().nth(1).unwrap().strip_prefix("generation=").unwrap();
        let current=stores.generations.join(generation).join("collection.anki2");
        if changed {assert_eq!(logical(&current),expected);} else {
            let c=meguri_sync_server::collection::schema::open(&current,false).unwrap();assert_eq!(c.query_row("SELECT count(*) FROM notes",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        }
        assert_eq!(std::fs::read(&sentinel).unwrap(),b"independent media\0\xff");
        assert_eq!(h.service.temp.used(),0);
        // An absent acknowledgement is uncertain even when publication survived.
        // Retry the same authoritative upload through the ordinary public API.
        // This explicitly chooses recovery authority; meta alone cannot prove
        // whether the interrupted request committed.
        assert_eq!(decoded(native(&h,"upload",&key,&std::fs::read(&fixture).unwrap()).await).await,b"OK");
        let recovered=h.temp.path().join("recovered.anki2");
        std::fs::write(&recovered,decoded(native(&h,"download",&key,b"{}").await).await).unwrap();
        assert_eq!(logical(&recovered),expected);
        assert_eq!(objects(&recovered),objects(&fixture));
        assert_eq!(std::fs::read(&sentinel).unwrap(),b"independent media\0\xff");
        assert_eq!(h.service.temp.used(),0);
        // Process-interruption evidence only; no power-loss claim.
    }
}
