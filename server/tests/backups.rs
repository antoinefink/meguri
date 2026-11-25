// SPDX-License-Identifier: MIT
//! Real account, loopback HTTP and durable private-store recovery regressions.
#![cfg(feature="test-hooks")]
mod support;
use support::*;
use serde_json::{Value,json};
use std::{io::{Read,Write},path::PathBuf};

async fn within<T>(future: impl std::future::Future<Output=T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(30),future).await.expect("bounded backup fixture")
}
async fn native(h: &Harness,family: &str,method: &str,key: &str,body: &[u8]) -> reqwest::Response {
    h.client.post(format!("{}/{family}/{method}",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"c":"synthetic-backup","s":"backup-round"}).to_string())
        .body(zstd::stream::encode_all(body,0).unwrap()).send().await.unwrap()
}
async fn decoded(response: reqwest::Response) -> Vec<u8> {
    assert_eq!(response.status(),200);
    zstd::stream::decode_all(response.bytes().await.unwrap().as_ref()).unwrap()
}
async fn media(h: &Harness,key: &str,method: &str,body: Value) -> Value {
    let bytes=decoded(native(h,"msync",method,key,&serde_json::to_vec(&body).unwrap()).await).await;
    let value:Value=serde_json::from_slice(&bytes).unwrap();assert_eq!(value["err"],"");value["data"].clone()
}
fn media_zip(entries: &[(&str,Option<&[u8]>)]) -> Vec<u8> {
    let mut archive=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut meta=Vec::new();
    for (i,(name,bytes)) in entries.iter().enumerate() {
        let member=bytes.map(|_|i.to_string());
        if let Some(bytes)=bytes {archive.start_file(member.as_ref().unwrap(),options).unwrap();archive.write_all(bytes).unwrap();}
        meta.push(json!([name,member]));
    }
    archive.start_file("_meta",options).unwrap();archive.write_all(&serde_json::to_vec(&meta).unwrap()).unwrap();
    archive.finish().unwrap().into_inner()
}
async fn upload_media(h: &Harness,key: &str,entries: &[(&str,Option<&[u8]>)]) {
    let bytes=decoded(native(h,"msync","uploadChanges",key,&media_zip(entries)).await).await;
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap()["err"],"");
}
fn collection(h: &Harness,stamp: i64,label: &str) -> Vec<u8> {
    use std::os::unix::fs::OpenOptionsExt;
    let path=h.temp.path().join(format!("source-{stamp}-{label}.anki2"));
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).unwrap();
    meguri_sync_server::collection::schema::create_blank(&path,123,stamp).unwrap();
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.execute("UPDATE col SET scm=?1,mod=?1,usn=1",[stamp]).unwrap();
    c.execute("INSERT INTO config VALUES('backup-proof',0,0,?1)",[label.as_bytes()]).unwrap();
    c.execute_batch("INSERT INTO notes VALUES(101,'backup-guid',1,123,0,'','Question'||char(31)||'Answer','Question',0,0,'opaque');
        INSERT INTO cards VALUES(201,101,1,0,123,0,2,2,321,45,2500,9,1,0,0,0,0,'{}');
        INSERT INTO revlog VALUES(301,201,0,3,45,30,2500,1200,1);
        CREATE TABLE private_extension(bytes BLOB);
        INSERT INTO private_extension VALUES(x'00ff01');
        CREATE TRIGGER preserved_but_disabled AFTER UPDATE ON col BEGIN DELETE FROM private_extension; END;").unwrap();
    meguri_sync_server::collection::schema::checkpoint(&c).unwrap();drop(c);
    std::fs::read(path).unwrap()
}
async fn upload(h: &Harness,key: &str,stamp: i64,label: &str) {
    assert_eq!(decoded(native(h,"sync","upload",key,&collection(h,stamp,label)).await).await,b"OK");
}
fn current(h: &Harness,id: &str) -> PathBuf {
    let account=h.path().join("state/accounts").join(id);
    let pointer=std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation=pointer.lines().nth(1).unwrap().strip_prefix("generation=").unwrap();
    account.join("generations").join(generation).join("collection.anki2")
}
fn stamps(h: &Harness,id: &str) -> (i64,i64,i64) {
    let c=meguri_sync_server::collection::schema::open(&current(h,id),false).unwrap();
    c.query_row("SELECT scm,mod,usn FROM col",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}
async fn history(h: &Harness,cookie: &str) -> Vec<Value> {
    let response=h.get("/api/me/backups",Some(cookie)).await;assert_eq!(response.status(),200);
    response.json::<Value>().await.unwrap()["data"].as_array().unwrap().clone()
}
async fn restore(h: &Harness,cookie: &str,id: &str) {
    let response=h.post(&format!("/api/me/backups/{id}/restore"),json!({}),Some(cookie)).await;
    let status=response.status();let body=response.json::<Value>().await.unwrap();
    assert_eq!(status,200,"restore response: {body}");assert_eq!(body,json!({"data":null}));
}
fn today() -> i64 {time::OffsetDateTime::now_utc().unix_timestamp()}
fn account_credentials(h: &Harness,id: &str) -> (String,i64,i64) {
    let c=rusqlite::Connection::open(h.database()).unwrap();
    c.query_row("SELECT password_phc,password_revision,generation FROM accounts WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()
}
#[tokio::test]
async fn restores_reviews_scheduling_and_new_changed_deleted_media_with_monotonic_cursors() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();
    let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    upload(&h,&key,100,"saved").await;
    upload_media(&h,&key,&[("changed.bin",Some(b"saved bytes")),("revived.bin",Some(b"restore deletion")),("same.bin",Some(b"same"))]).await;
    let before=stamps(&h,id);let credentials=account_credentials(&h,id);
    h.service.capture_backups_at(today()).await.unwrap();
    assert_eq!(stamps(&h,id),before,"daily capture must not stamp the collection");
    let points=history(&h,&cookie).await;assert_eq!(points.len(),1);
    let backup=points[0]["id"].as_str().unwrap();
    assert_eq!(points[0]["kind"],"daily");assert!(points[0]["size_bytes"].as_u64().unwrap()>0);
    upload(&h,&key,200,"later").await;
    upload_media(&h,&key,&[("changed.bin",Some(b"changed later")),("revived.bin",None),("new.bin",Some(b"new later"))]).await;
    let cursor=media(&h,&key,"begin",json!({"v":"fixture"})).await["usn"].as_i64().unwrap();
    let stale=h.service.validate_hostkey(&key).await.unwrap();
    restore(&h,&cookie,backup).await;
    assert_eq!(h.service.publish(stale,|_|Ok(())).await.unwrap_err().code,"authentication_failed");
    assert_eq!(account_credentials(&h,id),credentials);
    assert_eq!(h.get("/api/me",Some(&cookie)).await.status(),200);
    let first=stamps(&h,id);assert!(first.0>200 && first.1>200);
    let delta=media(&h,&key,"mediaChanges",json!({"lastUsn":cursor})).await;
    let rows=delta.as_array().unwrap();assert_eq!(rows.len(),3);
    let names=rows.iter().map(|row|row[0].as_str().unwrap()).collect::<Vec<_>>();
    assert_eq!(names,vec!["changed.bin","new.bin","revived.bin"]);
    for row in rows {assert!(row[1].as_i64().unwrap()>cursor);}
    assert_eq!(rows[1][2],"");assert_ne!(rows[0][2],"");assert_ne!(rows[2][2],"");
    let bytes=decoded(native(&h,"msync","downloadFiles",&key,br#"{"files":["changed.bin","revived.bin","same.bin"]}"#).await).await;
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    for (name,expected) in [("0",b"saved bytes".as_slice()),("1",b"restore deletion".as_slice()),("2",b"same".as_slice())] {
        let mut bytes=Vec::new();archive.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();assert_eq!(bytes,expected);
    }
    // Exercise native full download as a client would, then inspect exact rows.
    let bytes=decoded(native(&h,"sync","download",&key,b"{}").await).await;
    let path=h.temp.path().join("restored-download.anki2");std::fs::write(&path,bytes).unwrap();
    let c=rusqlite::Connection::open(path).unwrap();
    assert_eq!(c.query_row("SELECT val FROM config WHERE key='backup-proof'",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),b"saved");
    assert_eq!(c.query_row("SELECT due,ivl,reps FROM cards WHERE id=201",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).unwrap(),(321,45,9));
    assert_eq!(c.query_row("SELECT ease,ivl FROM revlog WHERE id=301",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?))).unwrap(),(3,45));
    assert_eq!(c.query_row("SELECT bytes FROM private_extension",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),vec![0,255,1]);drop(c);
    let prior=stamps(&h,id);restore(&h,&cookie,backup).await;let second=stamps(&h,id);
    assert!(second.0>prior.0 && second.1>prior.1);
    let post=media(&h,&key,"begin",json!({"v":"fixture"})).await["usn"].as_i64().unwrap();
    assert_eq!(post,cursor+3,"a duplicate restore leaves unchanged media USNs alone");
    for _ in 0..4 {restore(&h,&cookie,backup).await;}
    assert_eq!(history(&h,&cookie).await.iter().filter(|p|p["kind"]=="before_restore").count(),3);
    let safety=history(&h,&cookie).await.into_iter().find(|p|p["kind"]=="before_restore").unwrap();
    restore(&h,&cookie,safety["id"].as_str().unwrap()).await;
    assert_eq!(history(&h,&cookie).await.iter().filter(|p|p["kind"]=="before_restore").count(),3);
    let _=h.stop().await;
}
#[tokio::test]
async fn daily_idempotence_retention_boundaries_and_missed_days_use_real_account_points() {
    let h=Harness::new().await;h.setup().await;let cookie=h.login("root.admin",PASSWORD).await;
    // Start far enough ahead that the serving startup sweep cannot overlap days.
    let start=(today().div_euclid(86400)+10)*86400;
    for day in 0..410 {h.service.capture_backups_at(start+day*86400).await.unwrap();}
    let points=history(&h,&cookie).await;
    assert_eq!(points.iter().filter(|p|p["kind"]=="daily").count(),7);
    let dates=points.iter().filter(|p|p["kind"]=="daily").map(|p|p["created_at"].as_i64().unwrap()).collect::<Vec<_>>();
    assert_eq!(dates,(403..=409).rev().map(|day|start+day*86400).collect::<Vec<_>>());
    assert_eq!(points.iter().filter(|p|p["kind"]=="weekly").count(),4);
    let end_day=(start+409*86400).div_euclid(86400);
    let month=|at| {let d=time::OffsetDateTime::from_unix_timestamp(at).unwrap();d.year()*12+i32::from(u8::from(d.month()))};
    let expected_months=(0..410).filter_map(|day| {
        let at=start+day*86400;let date=at.div_euclid(86400);
        let age=end_day-date;let week=(end_day+3).div_euclid(7)-(date+3).div_euclid(7);
        let age_month=month(start+409*86400)-month(at);
        (age>=7 && !(1..=4).contains(&week) && (1..=12).contains(&age_month)).then(||month(at))
    }).collect::<std::collections::BTreeSet<_>>();
    let actual_months=points.iter().filter(|p|p["kind"]=="monthly").map(|p|month(p["created_at"].as_i64().unwrap())).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual_months,expected_months);
    assert_eq!(points.iter().filter(|p|p["kind"]=="monthly").count(),expected_months.len());
    assert!(points.windows(2).all(|p|p[0]["created_at"].as_i64().unwrap()>=p[1]["created_at"].as_i64().unwrap()));
    let before=points.clone();h.service.capture_backups_at(start+409*86400+500).await.unwrap();assert_eq!(history(&h,&cookie).await,before);
    // Identical collection payloads are shared across all retained dates.
    let c=rusqlite::Connection::open(h.database()).unwrap();
    assert_eq!(c.query_row("SELECT count(DISTINCT payload) FROM backup_points",[],|r|r.get::<_,i64>(0)).unwrap(),1);drop(c);
    h.service.capture_backups_at(start+450*86400).await.unwrap();
    let points=history(&h,&cookie).await;assert_eq!(points.iter().filter(|p|p["kind"]=="daily").count(),1);
    assert_eq!(points[0]["created_at"],start+450*86400);
    let _=h.stop().await;
}
#[tokio::test]
async fn daily_clock_samples_each_snapshot_pin_and_rechecks_its_utc_day_at_publication() {
    use std::sync::{Arc,atomic::{AtomicI64,Ordering}};
    let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    h.member(&admin,"clock.member").await;let member=h.login("clock.member",PASSWORD).await;
    let start=(today().div_euclid(86400)+10)*86400;
    let ticks=Arc::new(AtomicI64::new(start-1));
    let clock={let ticks=ticks.clone();move ||ticks.fetch_add(1,Ordering::SeqCst)};
    h.service.capture_backups_with_clock(clock.clone()).await.unwrap();
    let a=history(&h,&admin).await;let b=history(&h,&member).await;
    let mut stamps=vec![a[0]["created_at"].as_i64().unwrap(),b[0]["created_at"].as_i64().unwrap()];stamps.sort();
    // Per account: preflight, successful snapshot pin, then pruning. The first
    // account crosses midnight while acquiring its snapshot; the next is later.
    assert_eq!(stamps,vec![start,start+3]);
    // Revisit the absent previous day, then pin on the already captured day.
    // Publication must preserve the original point rather than replace it.
    ticks.store(start-1,Ordering::SeqCst);
    h.service.capture_backups_with_clock(clock).await.unwrap();
    assert_eq!(history(&h,&admin).await,a);assert_eq!(history(&h,&member).await,b);
    let _=h.stop().await;
}
#[tokio::test]
async fn browser_contract_is_account_scoped_and_strict_and_reset_delete_remove_history() {
    let h=Harness::new().await;let root=h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    let member=h.member(&admin,"backup.member").await;let cookie=h.login("backup.member",PASSWORD).await;
    error(h.get("/api/me/backups",None).await,401,"authentication_failed").await;
    h.service.capture_backups_at(today()).await.unwrap();
    let a=history(&h,&admin).await;let b=history(&h,&cookie).await;assert_eq!(a.len(),1);assert_eq!(b.len(),1);assert_ne!(a[0]["id"],b[0]["id"]);
    let route=format!("/api/me/backups/{}/restore",a[0]["id"].as_str().unwrap());
    error(h.post(&route,json!({}),Some(&cookie)).await,404,"route_not_found").await;
    error(h.post(&route,json!({}),None).await,401,"authentication_failed").await;
    error(h.post(&route,json!({"extra":1}),Some(&admin)).await,400,"invalid_request").await;
    error(h.client.post(format!("{}{route}",h.base)).header("cookie",&admin).json(&json!({})).send().await.unwrap(),403,"origin_rejected").await;
    error(h.client.post(format!("{}{route}",h.base)).header("cookie",&admin).header("origin",ORIGIN).header("content-type","text/plain").body("{}").send().await.unwrap(),415,"unsupported_content_type").await;
    error(h.client.post(format!("{}{route}",h.base)).header("cookie",&admin).header("origin",ORIGIN).header("content-type","application/json").body("{\"extra\":1,\"extra\":2}").send().await.unwrap(),400,"invalid_request").await;
    error(h.get("/api/me/backups?account=other",Some(&admin)).await,400,"invalid_request").await;
    let id=member["id"].as_str().unwrap();let root_id=root["id"].as_str().unwrap();
    let root_files=store_bytes(&h.path().join("state/accounts").join(root_id));
    assert_eq!(h.post(&format!("/api/admin/accounts/{id}/reset-data"),json!({}),Some(&admin)).await.status(),200);
    assert!(history(&h,&cookie).await.is_empty());
    assert!(!h.path().join("state/accounts").join(id).join("backups").exists());
    assert_eq!(store_bytes(&h.path().join("state/accounts").join(root_id)),root_files);
    h.service.capture_backups_at(today()).await.unwrap();
    assert_eq!(history(&h,&cookie).await.len(),1);
    assert_eq!(h.post(&format!("/api/admin/accounts/{id}/delete"),json!({}),Some(&admin)).await.status(),200);
    let c=rusqlite::Connection::open(h.database()).unwrap();
    for table in ["backup_points","backup_media","backup_restores"] {
        assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table} WHERE account_id=?1"),[id],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
    assert!(!h.path().join("state/accounts").join(id).exists());drop(c);
    let _=h.stop().await;
}
#[tokio::test]
async fn invalid_payload_and_budget_exhaustion_refuse_without_cutover_or_safety_point() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;
    h.service.capture_backups_at(today()).await.unwrap();let points=history(&h,&cookie).await;
    let backup=points[0]["id"].as_str().unwrap();let before=stamps(&h,id);
    let hold=h.service.temp.reserve(h.service.config.temp_bytes.get()-4096).unwrap();
    error(h.post(&format!("/api/me/backups/{backup}/restore"),json!({}),Some(&cookie)).await,503,"storage_unavailable").await;
    assert_eq!(history(&h,&cookie).await,points);assert_eq!(stamps(&h,id),before);drop(hold);
    let c=rusqlite::Connection::open(h.database()).unwrap();let payload:String=c.query_row("SELECT payload FROM backup_points WHERE account_id=?1 AND id=?2",rusqlite::params![id,backup],|r|r.get(0)).unwrap();drop(c);
    let path=h.path().join("state/accounts").join(id).join("backups").join(payload);
    let original=std::fs::read(&path).unwrap();let mut damaged=original.clone();damaged[100]^=1;std::fs::write(&path,damaged).unwrap();
    error(h.post(&format!("/api/me/backups/{backup}/restore"),json!({}),Some(&cookie)).await,503,"storage_unavailable").await;
    assert_eq!(history(&h,&cookie).await,points);assert_eq!(stamps(&h,id),before);
    std::fs::write(&path,original).unwrap();
    restore(&h,&cookie,backup).await;
    let _=h.stop().await;
}
#[tokio::test]
async fn persistent_capture_budget_failures_preserve_last_history_until_success() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap().to_owned();
    let cookie=h.login("root.admin",PASSWORD).await;
    h.service.capture_backups_at(today()).await.unwrap();
    let original=history(&h,&cookie).await;
    let hold=h.service.temp.reserve(h.service.config.temp_bytes.get()).unwrap();
    let later=today()+800*86400;
    for day in 0..3 {
        assert_eq!(h.service.capture_account_backup_at(id.clone(),later+day*86400).await.unwrap_err().code,"storage_unavailable");
        assert_eq!(history(&h,&cookie).await,original);
    }
    drop(hold);
    h.service.capture_account_backup_at(id,later+3*86400).await.unwrap();
    let points=history(&h,&cookie).await;
    assert_eq!(points.len(),1);assert_ne!(points[0]["id"],original[0]["id"]);
    restore(&h,&cookie,points[0]["id"].as_str().unwrap()).await;
    let _=h.stop().await;
}
#[tokio::test]
async fn pinned_daily_view_does_not_end_round_and_reset_fences_capture() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    let stamp=stamps(&h,id);
    for (method,body) in [("start",json!({"minUsn":0,"lnewer":false,"graves":null})),("applyChanges",json!({"changes":{"models":[],"decks":[[],[]],"tags":["uncommitted"]}}))] {
        decoded(native(&h,"sync",method,&key,&serde_json::to_vec(&body).unwrap()).await).await;
    }
    h.service.capture_backups_at(today()).await.unwrap();
    assert_eq!(stamps(&h,id),stamp);
    let c=rusqlite::Connection::open(h.database()).unwrap();
    let payload:String=c.query_row("SELECT payload FROM backup_points WHERE account_id=?1",[id],|r|r.get(0)).unwrap();drop(c);
    let saved=meguri_sync_server::collection::schema::open(&h.path().join("state/accounts").join(id).join("backups").join(payload),false).unwrap();
    assert_eq!(saved.query_row("SELECT count(*) FROM tags",[],|r|r.get::<_,i64>(0)).unwrap(),0,"uncommitted normal-round tags are absent");drop(saved);
    assert_eq!(native(&h,"sync","chunk",&key,b"{}").await.status(),200,"capture leaves the retained round admitted");
    decoded(native(&h,"sync","abort",&key,b"{}").await).await;
    let pause=h.service.hooks.arm("backup_snapshot_pinned");let s=h.service.clone();
    let pending=tokio::spawn(async move {s.capture_backups_at(today()+86400).await});
    within(pause.arrived()).await;
    assert_eq!(h.post(&format!("/api/admin/accounts/{id}/reset-data"),json!({}),Some(&cookie)).await.status(),200);
    pause.resume();within(pending).await.unwrap().unwrap();
    assert!(history(&h,&cookie).await.is_empty());
    let _=h.stop().await;
}
#[tokio::test]
async fn restore_rechecks_intervening_media_and_logout_and_serializes_with_daily_capture() {
    let h=Harness::new().await;h.setup().await;let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    h.service.capture_backups_at(today()).await.unwrap();let points=history(&h,&cookie).await;let id=points[0]["id"].as_str().unwrap();
    let pause=h.service.hooks.arm("backup_restore_before_commit");
    let a=h.service.browser_authority(secret(&cookie)).await.unwrap();let s=h.service.clone();let backup=id.to_owned();
    let pending=tokio::spawn(async move {s.restore_backup(a,backup).await});within(pause.arrived()).await;
    upload_media(&h,&key,&[("intervening.bin",Some(b"keep this"))]).await;
    pause.resume();assert_eq!(within(pending).await.unwrap().unwrap_err().code,"busy");
    assert_eq!(history(&h,&cookie).await,points);
    let pause=h.service.hooks.arm("backup_restore_before_commit");
    let a=h.service.browser_authority(secret(&cookie)).await.unwrap();let s=h.service.clone();let backup=id.to_owned();
    let pending=tokio::spawn(async move {s.restore_backup(a,backup).await});within(pause.arrived()).await;
    h.service.logout(secret(&cookie)).await.unwrap();pause.resume();assert_eq!(within(pending).await.unwrap().unwrap_err().code,"authentication_failed");
    let cookie=h.login("root.admin",PASSWORD).await;
    let pause=h.service.hooks.arm("backup_snapshot_pinned");let s=h.service.clone();
    let pending=tokio::spawn(async move {s.capture_backups_at(today()+86400).await});within(pause.arrived()).await;
    // Capture owns the per-account backup slot, so a concurrent restore refuses.
    error(h.post(&format!("/api/me/backups/{id}/restore"),json!({}),Some(&cookie)).await,409,"busy").await;
    pause.resume();within(pending).await.unwrap().unwrap();
    restore(&h,&cookie,id).await;
    let _=h.stop().await;
}

async fn child_service(path: &std::path::Path,phase: &str,kill: bool,signal: &std::path::Path) -> (Child,String) {
    use std::{io::BufRead,process::{Command,Stdio}};
    let mut child=Child(Command::new(binary()).args(["serve",path.to_str().unwrap(),ORIGIN,"127.0.0.1:0"])
        .env(if kill {"MEGURI_TEST_KILL_PHASE"} else {"MEGURI_TEST_FAIL_PHASE"},phase).env("MEGURI_TEST_SIGNAL_FILE",signal)
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let stdout=child.0.stdout.take().unwrap();
    let line=within(tokio::task::spawn_blocking(move || {let mut line=String::new();std::io::BufReader::new(stdout).read_line(&mut line).unwrap();line})).await.unwrap();
    (child,format!("http://{}",line.trim().strip_prefix("listening ").unwrap()))
}
#[tokio::test]
async fn interrupted_restore_completes_before_admission_at_each_durable_commit_boundary() {
    for (phase,committed,partial) in [("backup_restore_before_intent",false,None),("backup_restore_after_intent",true,None),("backup_restore_after_pointer",true,None),("backup_restore_before_complete",true,None),("backup_restore_after_intent",true,Some(0)),("backup_restore_after_intent",true,Some(19))] {
        let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;
        let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
        let other=h.member(&cookie,"recovery.member").await;let other_id=other["id"].as_str().unwrap();
        let other_cookie=h.login("recovery.member",PASSWORD).await;
        let (_,other_key)=h.service.native_login(IP,"recovery.member",PASSWORD.into()).await.unwrap();
        upload(&h,&other_key,300,"isolated").await;upload_media(&h,&other_key,&[("point.bin",Some(b"isolated media"))]).await;
        upload(&h,&key,100,"original").await;upload_media(&h,&key,&[("point.bin",Some(b"original media"))]).await;
        h.service.capture_backups_at(today()).await.unwrap();let points=history(&h,&cookie).await;let backup=points[0]["id"].as_str().unwrap();
        upload(&h,&key,200,"changed").await;upload_media(&h,&key,&[("point.bin",Some(b"changed media")),("after.bin",Some(b"after"))]).await;
        let cursor=media(&h,&key,"begin",json!({"v":"fixture"})).await["usn"].as_i64().unwrap();
        let temp=h.stop().await;let owner=temp.path().join("owner");let signal=temp.path().join("restore-boundary");
        let (mut child,base)=child_service(&owner,phase,true,&signal).await;
        let client=reqwest::Client::builder().no_proxy().build().unwrap();
        let request=client.post(format!("{base}/api/me/backups/{backup}/restore")).header("cookie",&cookie).header("origin",ORIGIN).json(&json!({}));
        let pending=tokio::spawn(async move {request.send().await});
        wait_file(&signal,&mut child).await;child.0.kill().unwrap();child.0.wait().unwrap();drop(child);
        assert!(within(pending).await.unwrap().is_err());
        let c=rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM backup_restores",[],|r|r.get::<_,i64>(0)).unwrap(),i64::from(committed));
        let scratch=partial.map(|length| {
            use std::os::unix::fs::OpenOptionsExt;
            let generation:String=c.query_row("SELECT generation FROM backup_restores WHERE account_id=?1",[id],|r|r.get(0)).unwrap();
            let bytes=format!("meguri-collection-v1\ngeneration={generation}\n");
            let path=owner.join("state/accounts").join(id).join(format!("backup-restore-pointer-{generation}"));
            let mut stage=std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).unwrap();
            stage.write_all(&bytes.as_bytes()[..length]).unwrap();stage.sync_all().unwrap();
            assert_eq!(std::fs::read(&path).unwrap().len(),length);
            path
        });drop(c);
        let other_directory=owner.join("state/accounts").join(other_id);
        let other_pointer=std::fs::read(other_directory.join("current-collection")).unwrap();
        let h=Harness::reopen(temp).await;
        if let Some(path)=scratch {assert!(!path.exists(),"recovery consumes its only journal-bound scratch file");}
        assert_eq!(std::fs::read(other_directory.join("current-collection")).unwrap(),other_pointer);
        let other_collection=meguri_sync_server::collection::schema::open(&current(&h,other_id),false).unwrap();
        assert_eq!(other_collection.query_row("SELECT val FROM config WHERE key='backup-proof'",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),b"isolated");drop(other_collection);
        assert_eq!(h.get("/api/me",Some(&other_cookie)).await.status(),200);
        let c=rusqlite::Connection::open(h.database()).unwrap();
        let other_blob:String=c.query_row("SELECT blob FROM media_names WHERE account_id=?1 AND name='point.bin'",[other_id],|r|r.get(0)).unwrap();drop(c);
        assert_eq!(std::fs::read(other_directory.join("media").join(other_blob)).unwrap(),b"isolated media");
        let c=meguri_sync_server::collection::schema::open(&current(&h,id),false).unwrap();
        let value:Vec<u8>=c.query_row("SELECT val FROM config WHERE key='backup-proof'",[],|r|r.get(0)).unwrap();
        assert_eq!(value,if committed {b"original".as_slice()} else {b"changed".as_slice()});drop(c);
        assert_eq!(h.get("/api/me",Some(&cookie)).await.status(),200);
        let changes=media(&h,&key,"mediaChanges",json!({"lastUsn":cursor})).await;
        assert_eq!(changes.as_array().unwrap().len(),if committed {2} else {0});
        let c=rusqlite::Connection::open(h.database()).unwrap();assert_eq!(c.query_row("SELECT count(*) FROM backup_restores",[],|r|r.get::<_,i64>(0)).unwrap(),0);drop(c);
        // A second startup must not reapply the diff or increment its cursor.
        let usn=media(&h,&key,"begin",json!({"v":"fixture"})).await["usn"].clone();
        let h=Harness::reopen(h.stop().await).await;
        assert_eq!(media(&h,&key,"begin",json!({"v":"fixture"})).await["usn"],usn);
        let _=h.stop().await;
    }
}
#[tokio::test]
async fn failed_cutover_keeps_account_fenced_until_startup_and_pre_restore_point_recovers_later_state() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    upload(&h,&key,100,"saved-failure").await;h.service.capture_backups_at(today()).await.unwrap();let points=history(&h,&cookie).await;let backup=points[0]["id"].as_str().unwrap();
    upload(&h,&key,200,"live-failure").await;
    let temp=h.stop().await;let owner=temp.path().join("owner");
    let (mut child,base)=child_service(&owner,"backup_restore_after_pointer",false,&temp.path().join("unused")).await;
    let client=reqwest::Client::builder().no_proxy().build().unwrap();
    error(client.post(format!("{base}/api/me/backups/{backup}/restore")).header("cookie",&cookie).header("origin",ORIGIN).json(&json!({})).send().await.unwrap(),503,"provision_unavailable").await;
    let native=client.post(format!("{base}/sync/meta")).header("anki-sync",json!({"v":11,"k":key,"c":"fixture","s":""}).to_string())
        .body(zstd::stream::encode_all(br#"{"v":11}"#.as_slice(),0).unwrap()).send().await.unwrap();assert_ne!(native.status(),200);
    let storage:Value=client.get(format!("{base}/api/me/storage")).header("cookie",&cookie).send().await.unwrap().json().await.unwrap();assert!(storage["data"].is_null());
    for action in ["reset-data","delete"] {
        error(client.post(format!("{base}/api/admin/accounts/{id}/{action}")).header("cookie",&cookie).header("origin",ORIGIN).json(&json!({})).send().await.unwrap(),503,"storage_unavailable").await;
    }
    child.0.kill().unwrap();child.0.wait().unwrap();drop(child);
    let h=Harness::reopen(temp).await;
    let safety=history(&h,&cookie).await.into_iter().find(|p|p["kind"]=="before_restore").unwrap();
    restore(&h,&cookie,safety["id"].as_str().unwrap()).await;
    let c=meguri_sync_server::collection::schema::open(&current(&h,id),false).unwrap();
    assert_eq!(c.query_row("SELECT val FROM config WHERE key='backup-proof'",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),b"live-failure");drop(c);
    let _=h.stop().await;
}
#[tokio::test]
async fn startup_serve_catches_up_accounts_without_history_requests_and_isolates_failed_captures() {
    let h=Harness::new().await;let root=h.setup().await;let admin=h.login("root.admin",PASSWORD).await;let member=h.member(&admin,"scheduled.member").await;
    assert_eq!(h.post(&format!("/api/admin/accounts/{}/disable",member["id"].as_str().unwrap()),json!({}),Some(&admin)).await.status(),200);
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    upload_media(&h,&key,&[("broken.bin",Some(b"original valid bytes"))]).await;
    // Corrupt bytes of one account without changing its blob's length, so
    // startup shape validation succeeds and capture's strong verification fails.
    let c=rusqlite::Connection::open(h.database()).unwrap();let blob:String=c.query_row("SELECT blob FROM media_blobs WHERE account_id=?1",[root["id"].as_str().unwrap()],|r|r.get(0)).unwrap();drop(c);
    let path=h.path().join("state/accounts").join(root["id"].as_str().unwrap()).join("media").join(blob);
    let bytes=std::fs::read(&path).unwrap();std::fs::write(&path,vec![0;bytes.len()]).unwrap();
    let temp=h.stop().await;
    let c=rusqlite::Connection::open(temp.path().join("owner/state/accounts.sqlite")).unwrap();
    c.pragma_update(None,"foreign_keys",true).unwrap();
    c.execute("DELETE FROM backup_points",[]).unwrap();drop(c);
    let h=Harness::reopen(temp).await;
    let member_id=member["id"].as_str().unwrap().to_owned();
    within(async {
        loop {
            let c=rusqlite::Connection::open(h.database()).unwrap();
            let count:i64=c.query_row("SELECT count(*) FROM backup_points WHERE account_id=?1",[&member_id],|r|r.get(0)).unwrap();drop(c);
            if count==1 {break;}tokio::task::yield_now().await;
        }
    }).await;
    // No GET /api/me/backups has been issued for either account.
    let c=rusqlite::Connection::open(h.database()).unwrap();
    assert_eq!(c.query_row("SELECT count(*) FROM backup_points WHERE account_id=?1",[&member_id],|r|r.get::<_,i64>(0)).unwrap(),1);drop(c);
    std::fs::write(&path,bytes).unwrap();h.service.capture_backups_at(today()).await.unwrap();
    assert_eq!(history(&h,&admin).await.len(),1);
    let _=h.stop().await;
}

#[tokio::test]
async fn restore_refuses_unstamped_collection_commits_and_exhausted_stamps_and_media_cursors() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    upload(&h,&key,100,"lineage").await;h.service.capture_backups_at(today()).await.unwrap();let points=history(&h,&cookie).await;let backup=points[0]["id"].as_str().unwrap();
    let pause=h.service.hooks.arm("backup_restore_before_commit");let s=h.service.clone();let a=s.browser_authority(secret(&cookie)).await.unwrap();let selected=backup.to_owned();
    let pending=tokio::spawn(async move {s.restore_backup(a,selected).await});within(pause.arrived()).await;
    let path=current(&h,id);let a=h.service.validate_hostkey(&key).await.unwrap();
    h.service.publish(a,move |_| {
        let c=meguri_sync_server::collection::schema::open(&path,true)?;
        c.execute("UPDATE config SET val=x'ff' WHERE key='backup-proof'",[])?;
        Ok(())
    }).await.unwrap();
    pause.resume();assert_eq!(within(pending).await.unwrap().unwrap_err().code,"busy");
    assert_eq!(history(&h,&cookie).await,points);
    // Cursor exhaustion is rejected before an intent, safety metadata or pointer.
    upload_media(&h,&key,&[("cursor.bin",Some(b"remove on restore"))]).await;
    let c=rusqlite::Connection::open(h.database()).unwrap();
    c.execute("UPDATE accounts SET media_usn=2147483647 WHERE id=?1",[id]).unwrap();drop(c);
    let before=std::fs::read(h.path().join("state/accounts").join(id).join("current-collection")).unwrap();
    error(h.post(&format!("/api/me/backups/{backup}/restore"),json!({}),Some(&cookie)).await,503,"storage_unavailable").await;
    assert_eq!(std::fs::read(h.path().join("state/accounts").join(id).join("current-collection")).unwrap(),before);
    assert_eq!(history(&h,&cookie).await,points);
    let c=meguri_sync_server::collection::schema::open(&current(&h,id),true).unwrap();
    c.execute("UPDATE col SET mod=9223372036854775807",[]).unwrap();drop(c);
    error(h.post(&format!("/api/me/backups/{backup}/restore"),json!({}),Some(&cookie)).await,503,"storage_unavailable").await;
    assert_eq!(history(&h,&cookie).await,points);
    let _=h.stop().await;
}
#[tokio::test]
async fn already_validated_native_collection_and_media_uploads_cannot_commit_after_restore() {
    let h=Harness::new().await;h.setup().await;let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    h.service.capture_backups_at(today()).await.unwrap();let points=history(&h,&cookie).await;let backup=points[0]["id"].as_str().unwrap();
    let collection_pause=h.service.hooks.arm("collection_after_validation");
    let media_pause=h.service.hooks.arm("media_before_publication");
    let request=h.client.post(format!("{}/sync/upload",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"c":"fixture","s":"stale"}).to_string())
        .body(zstd::stream::encode_all(collection(&h,300,"late-upload").as_slice(),0).unwrap());
    let collection=tokio::spawn(async move {request.send().await.unwrap()});within(collection_pause.arrived()).await;
    let request=h.client.post(format!("{}/msync/uploadChanges",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"c":"fixture","s":"stale"}).to_string())
        .body(zstd::stream::encode_all(media_zip(&[("late.bin",Some(b"late bytes"))]).as_slice(),0).unwrap());
    let media=tokio::spawn(async move {request.send().await.unwrap()});within(media_pause.arrived()).await;
    restore(&h,&cookie,backup).await;
    collection_pause.resume();media_pause.resume();
    assert_ne!(within(collection).await.unwrap().status(),200);assert_ne!(within(media).await.unwrap().status(),200);
    let _=h.stop().await;
}

#[tokio::test]
async fn interrupted_capture_never_publishes_partial_history_and_startup_reclaims_its_files() {
    for phase in ["backup_before_payload","backup_after_payload","backup_before_publish"] {
        let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap().to_owned();let cookie=h.login("root.admin",PASSWORD).await;
        let before=stamps(&h,&id);let temp=h.stop().await;let owner=temp.path().join("owner");
        let c=rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
        c.pragma_update(None,"foreign_keys",true).unwrap();c.execute("DELETE FROM backup_points",[]).unwrap();drop(c);
        let signal=temp.path().join("capture-boundary");
        let (mut child,_)=child_service(&owner,phase,true,&signal).await;
        wait_file(&signal,&mut child).await;child.0.kill().unwrap();child.0.wait().unwrap();drop(child);
        let c=rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM backup_points",[],|r|r.get::<_,i64>(0)).unwrap(),0);drop(c);
        let h=Harness::reopen(temp).await;
        // Join any active startup sweep, then make the same day's capture once.
        within(async {
            loop {
                let c=rusqlite::Connection::open(h.database()).unwrap();
                let n:i64=c.query_row("SELECT count(*) FROM backup_points",[],|r|r.get(0)).unwrap();drop(c);
                if n==1 {break;}tokio::task::yield_now().await;
            }
        }).await;
        h.service.capture_backups_at(today()).await.unwrap();
        assert_eq!(history(&h,&cookie).await.len(),1);assert_eq!(stamps(&h,&id),before);
        // stop() joins the sweep's cleanup, not just its metadata publication.
        let temp=h.stop().await;
        assert_eq!(std::fs::read_dir(temp.path().join("owner/state/accounts").join(id).join("backups")).unwrap().count(),1);
    }
}

#[tokio::test]
async fn cleanup_keeps_one_gate_and_serializes_restore_reset_and_deletion_after_caller_cancellation() {
    struct ResumeOnDrop(std::sync::Arc<meguri_sync_server::test_hooks::Pause>);
    impl Drop for ResumeOnDrop {fn drop(&mut self) {self.0.resume();}}
    for (phase,delete) in [("backup_cleanup",false),("backup_cleanup",true),("backup_restore_cleanup",false),("backup_restore_cleanup",true)] {
        let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
        let member=h.member(&admin,"cleanup.member").await;let id=member["id"].as_str().unwrap().to_owned();
        let cookie=h.login("cleanup.member",PASSWORD).await;
        let (_,key)=h.service.native_login(IP,"cleanup.member",PASSWORD.into()).await.unwrap();
        h.service.capture_backups_at(today()).await.unwrap();
        let points=history(&h,&cookie).await;let selected=points[0]["id"].as_str().unwrap().to_owned();
        // Pause this member's work directly rather than an earlier account
        // visited by a sweep. Always release the blocking hook on fixture panic.
        let pause=h.service.hooks.arm(phase);
        let _release=ResumeOnDrop(pause.clone());
        let s=h.service.clone();let backup=selected.clone();
        let a=s.browser_authority(secret(&cookie)).await.unwrap();
        let pending=if phase=="backup_cleanup" {
            // Cancel the actual daily future while its blocking cleanup runs.
            let member_id=id.clone();
            tokio::spawn(async move {s.capture_account_backup_at(member_id,today()).await})
        } else {tokio::spawn(async move {s.restore_backup(a,backup).await})};
        within(pause.arrived()).await;
        pending.abort();assert!(pending.await.unwrap_err().is_cancelled());
        let a=h.service.browser_authority(secret(&cookie)).await.unwrap();
        assert_eq!(within(h.service.restore_backup(a,selected)).await.unwrap_err().code,"busy");
        // Resolve the barrier after the worker reached cleanup. A Weak-map gate
        // dropped before reclamation would silently create a different lock.
        let a=h.service.validate_hostkey(&key).await.unwrap();
        let gate=h.service.publication_barrier(&a).await.unwrap();
        let s=h.service.clone();let target=id.clone();let actor=s.browser_authority(secret(&admin)).await.unwrap();
        let administration=tokio::spawn(async move {
            if delete {s.delete_member(actor,target).await}
            else {s.reset_data(actor,target).await.map(|_|())}
        });
        within(gate.waiter_pending()).await;
        assert!(!administration.is_finished());
        pause.resume();within(administration).await.unwrap().unwrap();
        let c=rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM backup_points WHERE account_id=?1",[&id],|r|r.get::<_,i64>(0)).unwrap(),0);
        drop(c);drop(gate);
        let _=h.stop().await;
    }
}

#[tokio::test]
async fn collection_grown_above_default_upload_limit_captures_reopens_and_restores() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap().to_owned();
    let cookie=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    // Simulate committed growth after initial upload, without allocating a
    // collection-sized request body or changing the server's upload config.
    let path=current(&h,&id);let a=h.service.validate_hostkey(&key).await.unwrap();
    h.service.publish(a,move |_| {
        let c=meguri_sync_server::collection::schema::open(&path,true)?;
        c.execute_batch("CREATE TABLE grown_collection(bytes BLOB);
            INSERT INTO grown_collection VALUES(zeroblob(315621376));")?;
        Ok(())
    }).await.unwrap();
    let at=today()+86400;
    h.service.capture_backups_at(at).await.unwrap();
    let points=history(&h,&cookie).await;
    let saved=points.iter().find(|p|p["created_at"]==at).unwrap();
    assert!(saved["size_bytes"].as_u64().unwrap()>314_572_800);
    let selected=saved["id"].as_str().unwrap().to_owned();
    // Startup size validation and restore point decoding accept the same size.
    // Harness joins today's real startup capture before this one-shot restore,
    // including cleanup and release of the account's backup-work lease.
    let h=Harness::reopen(h.stop().await).await;
    restore(&h,&cookie,&selected).await;
    let c=meguri_sync_server::collection::schema::open(&current(&h,&id),false).unwrap();
    assert_eq!(c.query_row("SELECT length(bytes) FROM grown_collection",[],|r|r.get::<_,i64>(0)).unwrap(),315_621_376);
    drop(c);let _=h.stop().await;
}
