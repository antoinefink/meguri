// SPDX-License-Identifier: MIT
mod support;
use support::*;
use serde_json::{Value,json};
use std::io::Write;

fn zip(name:&str,bytes:&[u8])->Vec<u8> {
    let mut archive=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("0",options).unwrap();archive.write_all(bytes).unwrap();
    archive.start_file("_meta",options).unwrap();archive.write_all(&serde_json::to_vec(&json!([[name,"0"]])).unwrap()).unwrap();
    archive.finish().unwrap().into_inner()
}
fn request(client:&reqwest::Client,base:&str,family:&str,method:&str,key:&str,bytes:&[u8])->reqwest::RequestBuilder {
    client.post(format!("{base}/{family}/{method}")).header("anki-sync",json!({"v":11,"k":key,"c":"synthetic-media-recovery","s":"media-proof"}).to_string())
        .body(zstd::stream::encode_all(bytes,0).unwrap())
}
async fn decoded(r:reqwest::Response)->Vec<u8> {
    assert_eq!(r.status(),200);
    let size:usize=r.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let bytes=zstd::stream::decode_all(r.bytes().await.unwrap().as_ref()).unwrap();assert_eq!(bytes.len(),size);bytes
}
async fn login(h:&Harness,name:&str)->String {
    let r=request(&h.client,&h.base,"sync","hostKey","",&serde_json::to_vec(&json!({"u":name,"p":PASSWORD})).unwrap()).send().await.unwrap();
    serde_json::from_slice::<Value>(&decoded(r).await).unwrap()["key"].as_str().unwrap().into()
}
async fn media(h:&Harness,key:&str,method:&str,bytes:&[u8])->Value {
    let r=request(&h.client,&h.base,"msync",method,key,bytes).send().await.unwrap();
    let envelope:Value=serde_json::from_slice(&decoded(r).await).unwrap();assert_eq!(envelope["err"],"");envelope["data"].clone()
}
async fn collection(h:&Harness,key:&str,method:&str,value:Value)->Value {
    let r=request(&h.client,&h.base,"sync",method,key,&serde_json::to_vec(&value).unwrap()).send().await.unwrap();
    serde_json::from_slice(&decoded(r).await).unwrap()
}
async fn cursor(h:&Harness,key:&str)->i64 {media(h,key,"begin",br#"{"v":"synthetic"}"#).await["usn"].as_i64().unwrap()}
async fn downloaded(h:&Harness,key:&str,name:&str)->Vec<u8> {
    use std::io::Read;
    let r=request(&h.client,&h.base,"msync","downloadFiles",key,&serde_json::to_vec(&json!({"files":[name]})).unwrap()).send().await.unwrap();
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(decoded(r).await)).unwrap();
    let mut content=Vec::new();archive.by_name("0").unwrap().read_to_end(&mut content).unwrap();content
}
fn persistent(h:&Harness,id:&str)->(i64,Vec<(String,i64,Option<String>)>) {
    let c=rusqlite::Connection::open(h.database()).unwrap();
    let usn=c.query_row("SELECT media_usn FROM accounts WHERE id=?1",[id],|r|r.get(0)).unwrap();
    let mut q=c.prepare("SELECT name,usn,blob FROM media_names WHERE account_id=?1 ORDER BY usn").unwrap();
    let rows=q.query_map([id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap().map(|r|r.unwrap()).collect();(usn,rows)
}
async fn ready_to_finish(h:&Harness,key:&str) {
    collection(h,key,"start",json!({"minUsn":0,"lnewer":false,"graves":null})).await;
    collection(h,key,"applyChanges",json!({"changes":{"models":[],"decks":[[],[]],"tags":[]}})).await;
    assert_eq!(collection(h,key,"chunk",json!({"_pad":null})).await["done"],true);
    collection(h,key,"applyChunk",json!({"chunk":{"done":true}})).await;
    assert_eq!(collection(h,key,"sanityCheck2",json!({"client":[[0,0,0],0,0,0,0,0,0,0]})).await["status"],"ok");
}

#[tokio::test]
async fn idle_normal_round_allows_media_and_another_account_then_finishes() {
    let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    h.member(&admin,"media.member").await;
    let root=login(&h,"root.admin").await;let member=login(&h,"media.member").await;
    within(ready_to_finish(&h,&root)).await;
    assert_eq!(within(media(&h,&root,"uploadChanges",&zip("root.bin",b"root media"))).await,json!([1,1]));
    assert_eq!(within(media(&h,&member,"uploadChanges",&zip("member.bin",b"member media"))).await,json!([1,1]));
    assert_eq!(within(downloaded(&h,&root,"root.bin")).await,b"root media");
    assert!(within(collection(&h,&root,"finish",json!({}))).await.as_i64().unwrap()>0);
    assert_eq!(cursor(&h,&root).await,1);assert_eq!(cursor(&h,&member).await,1);
    let _=h.stop().await;
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn media_and_normal_finish_both_barrier_orders_preserve_independent_durable_state() {
    for media_first in [true,false] {
        let h=Harness::new().await;let identity=h.setup().await;let key=login(&h,"root.admin").await;
        ready_to_finish(&h,&key).await;
        let authority=h.service.validate_hostkey(&key).await.unwrap();let gate=h.service.publication_barrier(&authority).await.unwrap();
        let pause=h.service.hooks.arm(if media_first {"media_with_barrier"} else {"collection_with_barrier"});
        let upload=request(&h.client,&h.base,"msync","uploadChanges",&key,&zip("overlap",b"independent media"));
        let finish=request(&h.client,&h.base,"sync","finish",&key,b"{}");
        let (first,second)=if media_first {(upload,finish)} else {(finish,upload)};
        let first=tokio::spawn(async move {first.send().await.unwrap()});within(pause.arrived()).await;
        let second=tokio::spawn(async move {second.send().await.unwrap()});within(gate.waiter_pending()).await;
        // No shared writer is held while either operation waits for the gate.
        let db=rusqlite::Connection::open(h.database()).unwrap();db.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();drop(db);
        pause.resume();assert_eq!(within(first).await.unwrap().status(),200);assert_eq!(within(second).await.unwrap().status(),200);
        assert_eq!(persistent(&h,identity["id"].as_str().unwrap()).0,1);
        assert_eq!(downloaded(&h,&key,"overlap").await,b"independent media");
        let meta=collection(&h,&key,"meta",json!({"v":11,"cv":"synthetic"})).await;assert_eq!(meta["usn"],1);assert_eq!(meta["media_usn"],1);
        let temp=h.stop().await;let h=Harness::reopen(temp).await;assert_eq!(downloaded(&h,&key,"overlap").await,b"independent media");let _=h.stop().await;
    }
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn media_reset_disable_both_linearization_orders_recheck_exact_generation() {
    for reset in [false,true] {for media_first in [false,true] {
        let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
        let member=h.member(&admin,"media.member").await;let id=member["id"].as_str().unwrap();
        let key=login(&h,"media.member").await;let other=login(&h,"root.admin").await;
        ready_to_finish(&h,&key).await;
        let token=if reset {Some(h.reset_link(&admin,id).await)} else {None};
        let authority=h.service.validate_hostkey(&key).await.unwrap();let gate=h.service.publication_barrier(&authority).await.unwrap();
        let revocation=if let Some(token)=token {
            h.client.post(format!("{}/api/resets/claim",h.base)).header("origin",ORIGIN).json(&json!({"token":token,"new_password":NEW_PASSWORD}))
        } else {
            h.client.post(format!("{}/api/admin/accounts/{id}/disable",h.base)).header("origin",ORIGIN).header("cookie",&admin).json(&json!({}))
        };
        let upload=request(&h.client,&h.base,"msync","uploadChanges",&key,&zip("overlap",b"authorized before revoke"));
        let pause=h.service.hooks.arm(if media_first {"media_with_barrier"} else if reset {"reset_with_barrier"} else {"disable_with_barrier"});
        let (first,second)=if media_first {(upload,revocation)} else {(revocation,upload)};
        let first=tokio::spawn(async move {first.send().await.unwrap()});within(pause.arrived()).await;
        let second=tokio::spawn(async move {second.send().await.unwrap()});within(gate.waiter_pending()).await;
        // Another real account progresses while this account's gate is held.
        assert_eq!(within(media(&h,&other,"uploadChanges",&zip("other",b"other account progress"))).await,json!([1,1]));
        pause.resume();let first=within(first).await.unwrap();let second=within(second).await.unwrap();
        assert_eq!(first.status(),200);assert_eq!(second.status(),if media_first {200} else {403});
        let stored=persistent(&h,id);assert_eq!(stored.0,i64::from(media_first));assert_eq!(stored.1.len(),usize::from(media_first));
        if media_first {
            let blob=stored.1[0].2.as_ref().unwrap();
            assert_eq!(std::fs::read(h.path().join("state/accounts").join(id).join("media").join(blob)).unwrap(),b"authorized before revoke");
        }
        assert_eq!(request(&h.client,&h.base,"msync","begin",&key,br#"{"v":"synthetic"}"#).send().await.unwrap().status(),403);
        assert_eq!(request(&h.client,&h.base,"sync","finish",&key,b"{}").send().await.unwrap().status(),403);
        // The collection round was retired before revocation acknowledged.
        let temp=h.stop().await;let h=Harness::reopen(temp).await;assert_eq!(persistent(&h,id),stored);
        if reset {let replacement=request(&h.client,&h.base,"sync","hostKey","",&serde_json::to_vec(&json!({"u":"media.member","p":NEW_PASSWORD})).unwrap()).send().await.unwrap();
            let replacement:Value=serde_json::from_slice(&decoded(replacement).await).unwrap();let replacement=replacement["key"].as_str().unwrap();
            assert_eq!(cursor(&h,replacement).await,i64::from(media_first));if media_first {assert_eq!(downloaded(&h,replacement,"overlap").await,b"authorized before revoke");}
        }
        let _=h.stop().await;
    }}
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn pinned_download_survives_replacement_without_blocking_media_publication() {
    let h=Harness::new().await;h.setup().await;let key=login(&h,"root.admin").await;
    assert_eq!(media(&h,&key,"uploadChanges",&zip("pinned",b"old pinned bytes")).await,json!([1,1]));
    let pause=h.service.hooks.arm("media_download_pinned");
    let download=request(&h.client,&h.base,"msync","downloadFiles",&key,br#"{"files":["pinned"]}"#);
    let download=tokio::spawn(async move {download.send().await.unwrap()});within(pause.arrived()).await;
    assert_eq!(within(media(&h,&key,"uploadChanges",&zip("pinned",b"replacement"))).await,json!([1,2]));
    pause.resume();let bytes=decoded(within(download).await.unwrap()).await;
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();let mut old=Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name("0").unwrap(),&mut old).unwrap();assert_eq!(old,b"old pinned bytes");
    assert_eq!(downloaded(&h,&key,"pinned").await,b"replacement");let _=h.stop().await;
}

#[cfg(feature="test-hooks")]
async fn child_service(path:&std::path::Path,phase:&str,kill:bool,signal:&std::path::Path)->(Child,String) {
    child_service_with_backup_gate(path,phase,kill,signal,None).await
}
#[cfg(feature="test-hooks")]
async fn child_service_with_backup_gate(path:&std::path::Path,phase:&str,kill:bool,signal:&std::path::Path,backup_gate:Option<&std::path::Path>)->(Child,String) {
    use std::{io::BufRead,process::{Command,Stdio}};
    let mut command=Command::new(binary());
    command.args(["serve",path.to_str().unwrap(),ORIGIN,"127.0.0.1:0"])
        .env(if kill {"MEGURI_TEST_KILL_PHASE"} else {"MEGURI_TEST_FAIL_PHASE"},phase)
        .env("MEGURI_TEST_SIGNAL_FILE",signal).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(gate)=backup_gate {command.env("MEGURI_TEST_BACKUP_START_SIGNAL",gate);}
    let mut child=Child(command.spawn().unwrap());
    let stdout=child.0.stdout.take().unwrap();
    let line=within(tokio::task::spawn_blocking(move || {let mut line=String::new();std::io::BufReader::new(stdout).read_line(&mut line).unwrap();line})).await.unwrap();
    let address=line.trim().strip_prefix("listening ").unwrap();let base=format!("http://{address}");(child,base)
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn process_kill_before_blob_after_blob_and_after_metadata_preserves_exact_commit_boundary() {
    for (phase,committed) in [("media_before_blob_durability",false),("media_after_blob_durability",false),("media_after_metadata_commit",true)] {
        let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
        assert_eq!(media(&h,&key,"uploadChanges",&zip("baseline",b"retained baseline")).await,json!([1,1]));
        let account=h.path().join("state/accounts").join(id);
        let manifest=std::fs::read(account.join("current-collection")).unwrap();
        let temp=h.stop().await;let signal=temp.path().join("media-phase");
        let (mut child,base)=child_service(&temp.path().join("owner"),phase,true,&signal).await;
        let client=reqwest::Client::builder().no_proxy().build().unwrap();
        let upload=request(&client,&base,"msync","uploadChanges",&key,&zip("new",b"durable new bytes"));
        let upload=tokio::spawn(async move {upload.send().await});
        wait_file(&signal,&mut child).await;child.0.kill().unwrap();child.0.wait().unwrap();
        assert!(within(upload).await.unwrap().is_err());drop(child);
        let h=Harness::reopen(temp).await;
        assert_eq!(cursor(&h,&key).await,if committed {2} else {1});
        assert_eq!(downloaded(&h,&key,"baseline").await,b"retained baseline");
        assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),manifest);
        assert_eq!(persistent(&h,id).1.len(),if committed {2} else {1});
        if committed {assert_eq!(downloaded(&h,&key,"new").await,b"durable new bytes");}
        else {assert_eq!(request(&h.client,&h.base,"msync","downloadFiles",&key,br#"{"files":["new"]}"#).send().await.unwrap().status(),409);}
        // A lost acknowledgement replays into the same final cursor and bytes.
        assert_eq!(media(&h,&key,"uploadChanges",&zip("new",b"durable new bytes")).await,json!([1,2]));
        assert_eq!(downloaded(&h,&key,"new").await,b"durable new bytes");
        assert_eq!(h.service.temp.used(),0);
        let temp=h.stop().await;let h=Harness::reopen(temp).await;
        assert_eq!(cursor(&h,&key).await,2);assert_eq!(downloaded(&h,&key,"new").await,b"durable new bytes");let _=h.stop().await;
        // SIGKILL/restart coverage; no claim of power-loss emulation.
    }
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn injected_disk_full_blob_publication_and_metadata_failures_leave_no_committed_effects() {
    for phase in ["media_stage_write","media_before_blob_durability","media_after_blob_durability","media_before_metadata_commit"] {
        let h=Harness::new().await;let identity=h.setup().await;let key=login(&h,"root.admin").await;
        assert_eq!(media(&h,&key,"uploadChanges",&zip("baseline",b"preserve")).await,json!([1,1]));
        let before=persistent(&h,identity["id"].as_str().unwrap());
        let temp=h.stop().await;let signal=temp.path().join("unused-phase");
        let (mut child,base)=child_service(&temp.path().join("owner"),phase,false,&signal).await;
        let client=reqwest::Client::builder().no_proxy().build().unwrap();
        let response=within(request(&client,&base,"msync","uploadChanges",&key,&zip("baseline",b"must roll back")).send()).await.unwrap();
        assert_eq!(response.status(),503);drop(response);
        child.0.kill().unwrap();child.0.wait().unwrap();drop(child);
        let h=Harness::reopen(temp).await;
        assert_eq!(persistent(&h,identity["id"].as_str().unwrap()),before);
        assert_eq!(downloaded(&h,&key,"baseline").await,b"preserve");
        // Offline cleanup removes only unreferenced generated candidates.
        let directory=h.path().join("state/accounts").join(identity["id"].as_str().unwrap()).join("media");
        let mut files=std::fs::read_dir(directory).unwrap().map(|r|r.unwrap().file_name()).collect::<Vec<_>>();files.sort();
        assert_eq!(files.len(),1);assert!(files[0].to_str().unwrap().starts_with("blob-"));
        assert_eq!(h.service.temp.used(),0);let _=h.stop().await;
    }
}

#[tokio::test]
async fn accounts_never_resolve_another_accounts_names_or_sha1_blob_identity() {
    let h=Harness::new().await;let root_id=h.setup().await;let admin=h.login("root.admin",PASSWORD).await;
    let member_id=h.member(&admin,"media.member").await;let root=login(&h,"root.admin").await;let member=login(&h,"media.member").await;
    let bytes=b"same protocol digest in separate accounts";
    assert_eq!(media(&h,&root,"uploadChanges",&zip("root-only",bytes)).await,json!([1,1]));
    assert_eq!(request(&h.client,&h.base,"msync","downloadFiles",&member,br#"{"files":["root-only"]}"#).send().await.unwrap().status(),409);
    assert_eq!(media(&h,&member,"uploadChanges",&zip("member-only",bytes)).await,json!([1,1]));
    let a=persistent(&h,root_id["id"].as_str().unwrap());let b=persistent(&h,member_id["id"].as_str().unwrap());
    assert_ne!(a.1[0].2,b.1[0].2);assert_eq!(a.0,1);assert_eq!(b.0,1);
    assert_eq!(downloaded(&h,&root,"root-only").await,bytes);assert_eq!(downloaded(&h,&member,"member-only").await,bytes);
    let _=h.stop().await;
}

#[tokio::test]
async fn offline_v1_migration_preserves_existing_credentials_collections_and_unknown_media() {
    let h=Harness::new().await;let identity=h.setup().await;let key=login(&h,"root.admin").await;
    let account=h.path().join("state/accounts").join(identity["id"].as_str().unwrap());
    let manifest=std::fs::read(account.join("current-collection")).unwrap();
    let temp=h.stop().await;let path=temp.path().join("owner");
    let root=meguri_sync_server::DataRoot::acquire(&path,false).unwrap();
    // Restore the pre-media metadata shape in this stopped synthetic fixture.
    let c=rusqlite::Connection::open(root.database()).unwrap();
    c.execute_batch("DROP TABLE backup_restores; DROP TABLE backup_media; DROP TABLE backup_points;
DROP TABLE account_resets; DROP TABLE account_data_epochs;
DROP TABLE oauth_tokens; DROP TABLE oauth_codes; DROP TABLE oauth_pending; DROP TABLE oauth_grants;
DROP TRIGGER deletion_id_reserved; DROP TRIGGER reservation_retained; DROP TABLE account_deletions;
CREATE TRIGGER reservation_retained BEFORE DELETE ON provisioning_reservations BEGIN SELECT RAISE(ABORT,'retained reservation'); END;
DROP TABLE media_names; DROP TABLE media_blobs; ALTER TABLE accounts DROP COLUMN media_usn; PRAGMA user_version=1;").unwrap();drop(c);
    let unknown=account.join("media/operator-kept");std::fs::write(&unknown,b"unknown preserved bytes").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&unknown,std::fs::Permissions::from_mode(0o600)).unwrap();drop(root);
    let h=Harness::reopen(temp).await;assert_eq!(cursor(&h,&key).await,0);
    assert_eq!(std::fs::read(account.join("current-collection")).unwrap(),manifest);assert_eq!(std::fs::read(&unknown).unwrap(),b"unknown preserved bytes");
    assert_eq!(media(&h,&key,"uploadChanges",&zip("migrated",b"new bytes")).await,json!([1,1]));
    let temp=h.stop().await;let h=Harness::reopen(temp).await;assert_eq!(downloaded(&h,&key,"migrated").await,b"new bytes");let _=h.stop().await;
}

#[tokio::test]
async fn interrupted_zstd_body_and_exhausted_temp_budget_preserve_committed_media() {
    let h=Harness::new().await;let identity=h.setup().await;let key=login(&h,"root.admin").await;
    assert_eq!(media(&h,&key,"uploadChanges",&zip("baseline",b"preserve")).await,json!([1,1]));
    let before=persistent(&h,identity["id"].as_str().unwrap());
    let archive=zip("baseline",&vec![0x7f;1024*1024]);let mut compressed=zstd::stream::encode_all(archive.as_slice(),0).unwrap();compressed.truncate(compressed.len()/2);
    let response=h.client.post(format!("{}/msync/uploadChanges",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"c":"synthetic","s":""}).to_string()).body(compressed).send().await.unwrap();
    assert_eq!(response.status(),400);assert_eq!(persistent(&h,identity["id"].as_str().unwrap()),before);assert_eq!(h.service.temp.used(),0);
    let hold=h.service.temp.reserve(h.service.config.temp_bytes.get()-4096).unwrap();
    let response=request(&h.client,&h.base,"msync","uploadChanges",&key,&archive).send().await.unwrap();assert_eq!(response.status(),503);drop(hold);
    assert_eq!(h.service.temp.used(),0);assert_eq!(persistent(&h,identity["id"].as_str().unwrap()),before);
    let temp=h.stop().await;let h=Harness::reopen(temp).await;assert_eq!(downloaded(&h,&key,"baseline").await,b"preserve");let _=h.stop().await;
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn process_kill_before_media_migration_commit_preserves_v1_and_restarts_migrate_once() {
    use std::{io::Read, os::unix::{fs::PermissionsExt, process::ExitStatusExt}, process::{Command, Stdio}};
    let h = Harness::new().await;
    let identity = h.setup().await;
    let id = identity["id"].as_str().unwrap();
    let key = login(&h, "root.admin").await;
    let temp = h.stop().await;
    let path = temp.path().join("owner");
    let root = meguri_sync_server::DataRoot::acquire(&path, false).unwrap();
    // Restore only this stopped synthetic fixture's pre-media schema. Its real
    // credentials, provisioning proofs, collection and unknown media survive.
    let db = rusqlite::Connection::open(root.database()).unwrap();
    db.execute_batch("DROP TABLE backup_restores; DROP TABLE backup_media; DROP TABLE backup_points;
DROP TABLE account_resets; DROP TABLE account_data_epochs;
DROP TABLE oauth_tokens; DROP TABLE oauth_codes; DROP TABLE oauth_pending; DROP TABLE oauth_grants;
DROP TRIGGER deletion_id_reserved; DROP TRIGGER reservation_retained; DROP TABLE account_deletions;
CREATE TRIGGER reservation_retained BEFORE DELETE ON provisioning_reservations BEGIN SELECT RAISE(ABORT,'retained reservation'); END;
DROP TABLE media_names; DROP TABLE media_blobs; ALTER TABLE accounts DROP COLUMN media_usn; PRAGMA user_version=1;").unwrap();
    drop(db);
    let account = path.join("state/accounts").join(id);
    let unknown = account.join("media/operator-kept");
    std::fs::write(&unknown, b"preserve unknown media\0\xff").unwrap();
    std::fs::set_permissions(&unknown, std::fs::Permissions::from_mode(0o600)).unwrap();
    root.check_initialized().unwrap();
    let original_metadata = std::fs::read(root.database()).unwrap();
    let original_stores = store_bytes(&path.join("state/accounts"));
    drop(root);

    // Startup hits this hook before binding/listening, so the upload-oriented
    // child_service helper cannot wait for a listening line here. Spawn the
    // actual service binary directly and await its durable phase signal.
    let signal = temp.path().join("migration-before-commit");
    let mut child = Child(Command::new(binary())
        .args(["serve", path.to_str().unwrap(), ORIGIN, "127.0.0.1:0"])
        .env("MEGURI_TEST_KILL_PHASE", "media_migration_before_commit")
        .env("MEGURI_TEST_SIGNAL_FILE", &signal)
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap());
    wait_file(&signal, &mut child).await;
    assert_eq!(std::fs::read(&signal).unwrap(), b"media_migration_before_commit");
    child.0.kill().unwrap();
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    drop(child);

    // Inspect the actual killed process's original root before any successful
    // migration. Read-only SQLite recovery must expose the complete v1 state.
    let root = meguri_sync_server::DataRoot::acquire(&path, false).unwrap();
    root.check_initialized().unwrap();
    let db = rusqlite::Connection::open_with_flags(root.database(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT count(*) FROM sqlite_schema WHERE name IN ('media_names','media_blobs')", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(db.query_row("SELECT count(*) FROM pragma_table_info('accounts') WHERE name='media_usn'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    drop(db);
    assert_eq!(std::fs::read(root.database()).unwrap(), original_metadata);
    assert_eq!(store_bytes(&path.join("state/accounts")), original_stores);
    drop(root);

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let unused = temp.path().join("unused-migration-phase");
    // Keep the real scheduler enrolled but hold its first capture separately
    // from migration. Every original file remains subject to exact comparison;
    // no backup directories or unknown additions are excluded.
    let backup_gate = temp.path().join("migration-backup-gate");
    let (mut child, base) = child_service_with_backup_gate(&path, "unused_migration_phase", false, &unused, Some(&backup_gate)).await;
    wait_file(&backup_gate, &mut child).await;
    assert_eq!(std::fs::read(&backup_gate).unwrap(), b"backup_before_initial_sweep");
    let response = request(&client, &base, "msync", "begin", &key, br#"{"v":"synthetic-migration"}"#).send().await.unwrap();
    let begin: Value = serde_json::from_slice(&decoded(response).await).unwrap();
    assert_eq!(begin["err"], "");
    assert_eq!(begin["data"]["usn"], 0);
    assert_eq!(store_bytes(&path.join("state/accounts")), original_stores);
    let upload = zip("migrated", b"real post-migration bytes\0\xff");
    let response = request(&client, &base, "msync", "uploadChanges", &key, &upload).send().await.unwrap();
    let result: Value = serde_json::from_slice(&decoded(response).await).unwrap();
    assert_eq!(result["err"], "");
    assert_eq!(result["data"], json!([1,1]));
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(child);
    let root = meguri_sync_server::DataRoot::acquire(&path, false).unwrap();
    root.check_initialized().unwrap();
    let db = rusqlite::Connection::open_with_flags(root.database(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    assert_eq!(db.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0)).unwrap(), 6);
    assert_eq!(db.query_row("SELECT count(*) FROM pragma_table_info('accounts') WHERE name='media_usn'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(db.query_row("SELECT media_usn FROM accounts WHERE id=?1", [id], |r| r.get::<_, i64>(0)).unwrap(), 1);
    drop(db);
    let migrated_stores = store_bytes(&path.join("state/accounts"));
    drop(root);

    // A second real-binary restart leaves the same hook armed. Reaching HTTP
    // proves it did not run the migration transaction again; no mock outcome.
    let second_signal = temp.path().join("unexpected-second-migration");
    let second_backup_gate = temp.path().join("second-migration-backup-gate");
    let (mut child, base) = child_service_with_backup_gate(&path, "media_migration_before_commit", true, &second_signal, Some(&second_backup_gate)).await;
    wait_file(&second_backup_gate, &mut child).await;
    assert_eq!(std::fs::read(&second_backup_gate).unwrap(), b"backup_before_initial_sweep");
    let response = request(&client, &base, "msync", "begin", &key, br#"{"v":"synthetic-migration"}"#).send().await.unwrap();
    let begin: Value = serde_json::from_slice(&decoded(response).await).unwrap();
    assert_eq!(begin["err"], "");
    assert_eq!(begin["data"]["usn"], 1);
    let response = request(&client, &base, "msync", "downloadFiles", &key, br#"{"files":["migrated"]}"#).send().await.unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(decoded(response).await)).unwrap();
    let mut content = Vec::new();
    archive.by_name("0").unwrap().read_to_end(&mut content).unwrap();
    assert_eq!(content, b"real post-migration bytes\0\xff");
    assert!(!second_signal.exists());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    drop(child);
    let root = meguri_sync_server::DataRoot::acquire(&path, false).unwrap();
    root.check_initialized().unwrap();
    assert_eq!(store_bytes(&path.join("state/accounts")), migrated_stores);
    assert_eq!(std::fs::read(&unknown).unwrap(), b"preserve unknown media\0\xff");
    // SIGKILL at the existing migration hook, not power-loss emulation.
}

#[cfg(feature = "test-hooks")]
#[tokio::test]
async fn deletion_retires_native_round_without_waiting_for_opened_media_snapshot() {
    let h = Harness::new().await;
    h.setup().await;
    let admin = h.login("root.admin", PASSWORD).await;
    let member = h.member(&admin, "snapshot.member").await;
    let id = member["id"].as_str().unwrap();
    let key = login(&h, "snapshot.member").await;
    assert_eq!(media(&h, &key, "uploadChanges", &zip("snapshot", b"prior snapshot")).await, json!([1,1]));
    ready_to_finish(&h, &key).await;
    let pause = h.service.hooks.arm("media_download_opened");
    let download = request(&h.client, &h.base, "msync", "downloadFiles", &key, br#"{"files":["snapshot"]}"#);
    let download = tokio::spawn(async move { download.send().await.unwrap() });
    within(pause.arrived()).await;
    let response = within(h.post(&format!("/api/admin/accounts/{id}/delete"), json!({}), Some(&admin))).await;
    assert_eq!(response.status(), 200);
    assert!(!h.path().join("state/accounts").join(id).exists());
    pause.resume();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(decoded(within(download).await.unwrap()).await)).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name("0").unwrap(), &mut bytes).unwrap();
    assert_eq!(bytes, b"prior snapshot");
    for (family, method, bytes) in [("sync", "finish", b"{}".as_slice()), ("msync", "begin", br#"{"v":"synthetic"}"#.as_slice())] {
        assert_eq!(request(&h.client, &h.base, family, method, &key, bytes).send().await.unwrap().status(), 403);
    }
    let _ = h.stop().await;
}

fn exact_download_zip(bytes:Vec<u8>,name:&str,expected:&[u8]) {
    use std::io::Read;
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(archive.len(),2);
    let mut meta=String::new();archive.by_name("_meta").unwrap().read_to_string(&mut meta).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&meta).unwrap(),json!({"0":name}));
    let mut content=Vec::new();archive.by_name("0").unwrap().read_to_end(&mut content).unwrap();
    assert_eq!(content,expected);
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn advertised_media_overlap_on_one_account_allows_collection_meta() {
    let h=Harness::new().await;h.setup().await;let key=login(&h,"root.admin").await;
    let fixtures:Vec<_>=(0..4).map(|i|(format!("overlap-{i}.bin"),format!("exact synthetic media {i}").into_bytes())).collect();
    for (i,(name,bytes)) in fixtures.iter().enumerate() {
        assert_eq!(media(&h,&key,"uploadChanges",&zip(name,bytes)).await,json!([1,i+1]));
    }
    let begin=within(request(&h.client,&h.base,"msync","begin",&key,br#"{"v":"synthetic"}"#).send()).await.unwrap();
    assert_eq!(begin.headers().get_all("meguri-media-download-concurrency").iter().count(),1);
    assert_eq!(begin.headers()["meguri-media-download-concurrency"],"4");
    assert_eq!(serde_json::from_slice::<Value>(&decoded(begin).await).unwrap(),json!({"data":{"usn":4,"sk":""},"err":""}));
    let refused=request(&h.client,&h.base,"msync","begin","invalid-host-key",br#"{"v":"synthetic"}"#).send().await.unwrap();
    assert_eq!(refused.status(),403);assert!(!refused.headers().contains_key("meguri-media-download-concurrency"));drop(refused);
    let mut pauses=Vec::new();let mut downloads=Vec::new();
    for (name,_) in &fixtures {
        // The hook is single-use. Rearming after each arrival proves all four
        // disjoint same-account requests reach the pin boundary concurrently.
        let pause=h.service.hooks.arm("media_download_pinned");
        let download=request(&h.client,&h.base,"msync","downloadFiles",&key,&serde_json::to_vec(&json!({"files":[name]})).unwrap());
        downloads.push(tokio::spawn(async move {download.send().await.unwrap()}));
        within(pause.arrived()).await;pauses.push(pause);
    }
    assert!(downloads.iter().all(|d|!d.is_finished()));
    let meta=within(collection(&h,&key,"meta",json!({"v":11,"cv":"synthetic"}))).await;
    assert_eq!(meta["media_usn"],4);
    assert!(downloads.iter().all(|d|!d.is_finished()));
    for pause in pauses {pause.resume();}
    for (download,(name,expected)) in downloads.into_iter().zip(&fixtures) {
        exact_download_zip(within(decoded(within(download).await.unwrap())).await,name,expected);
    }
    assert_eq!(h.service.temp.used(),0);let _=h.stop().await;
}

// Explicit opt-in loopback probe; no live endpoint, settings change, or timing
// assertion. Compare whole-run wall time, including validation of every byte.
#[tokio::test]
#[ignore="opt-in synthetic serial/four-way media throughput probe"]
async fn synthetic_media_serial_vs_four_way_throughput_probe() {
    let h=Harness::new().await;h.setup().await;let key=login(&h,"root.admin").await;
    let mut fixtures=Vec::new();
    for size in [64*1024,4*1024*1024] {
        for i in 0..8 {
            let name=format!("probe-{size}-{i}.bin");
            let bytes:Vec<u8>=(0..size).map(|j|((j*31+j/251+i*17)%256) as u8).collect();
            media(&h,&key,"uploadChanges",&zip(&name,&bytes)).await;
            fixtures.push((name,bytes));
        }
    }
    for concurrency in [1,4] {
        let started=std::time::Instant::now();let mut count=0;let mut total=0;
        for wave in fixtures.chunks(concurrency) {
            let mut downloads=Vec::new();
            for (name,expected) in wave {
                let download=request(&h.client,&h.base,"msync","downloadFiles",&key,&serde_json::to_vec(&json!({"files":[name]})).unwrap());
                let name=name.clone();let expected=expected.clone();
                downloads.push(tokio::spawn(async move {
                    let bytes=decoded(download.send().await.unwrap()).await;
                    exact_download_zip(bytes,&name,&expected);expected.len()
                }));
            }
            for download in downloads {total+=within(download).await.unwrap();count+=1;}
        }
        eprintln!("synthetic media concurrency={concurrency} count={count} bytes={total} wall={:?}",started.elapsed());
        assert_eq!(count,fixtures.len());assert_eq!(total,fixtures.iter().map(|(_,b)|b.len()).sum::<usize>());
    }
    let _=h.stop().await;
}
