// SPDX-License-Identifier: MIT
//! Synthetic, real-HTTP reset isolation, credential continuity and epoch fences.
mod support;
use support::*;
use serde_json::{Value,json};
use std::io::Write;

async fn native(h: &Harness, family: &str, method: &str, key: &str, bytes: &[u8]) -> reqwest::Response {
    h.client.post(format!("{}/{family}/{method}",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"c":"synthetic-reset","s":"reset-round"}).to_string())
        .body(zstd::stream::encode_all(bytes,0).unwrap()).send().await.unwrap()
}
async fn decoded(response: reqwest::Response) -> Vec<u8> {
    assert_eq!(response.status(),200);
    zstd::stream::decode_all(response.bytes().await.unwrap().as_ref()).unwrap()
}
fn media() -> Vec<u8> {
    let mut archive=zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options=zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive.start_file("0",options).unwrap(); archive.write_all(b"synthetic reset media").unwrap();
    archive.start_file("_meta",options).unwrap(); archive.write_all(br#"[["reset.bin","0"]]"#).unwrap();
    archive.finish().unwrap().into_inner()
}
fn collection(h: &Harness) -> Vec<u8> {
    use std::os::unix::fs::OpenOptionsExt;
    let path=h.temp.path().join("reset-source.anki2");
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
    meguri_sync_server::collection::schema::create_blank(&path,123,456).unwrap();
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.execute("INSERT INTO config VALUES('synthetic-reset-payload',0,0,?1)",[vec![37u8;32768]]).unwrap();
    meguri_sync_server::collection::schema::checkpoint(&c).unwrap(); drop(c);
    std::fs::read(path).unwrap()
}
async fn reset(h: &Harness, admin: &str, id: &str) -> Value {
    let response=h.post(&format!("/api/admin/accounts/{id}/reset-data"),json!({}),Some(admin)).await;
    assert_eq!(response.status(),200);
    response.json::<Value>().await.unwrap()["data"].clone()
}
fn credentials(c: &rusqlite::Connection, id: &str) -> Vec<rusqlite::types::Value> {
    c.query_row("SELECT username,role,enabled,password_phc,password_revision,generation FROM accounts WHERE id=?1",[id],|row| {
        ["username","role","enabled","password_phc","password_revision","generation"]
            .into_iter().map(|column|row.get(column)).collect()
    }).unwrap()
}
#[tokio::test]
async fn occupied_staging_budget_refuses_reset_without_retiring_live_data() {
    let h=Harness::new().await;
    let account=h.setup().await; let admin=h.login("root.admin",PASSWORD).await;
    let id=account["id"].as_str().unwrap();
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    let authority=h.service.validate_hostkey(&key).await.unwrap();
    let upload=collection(&h);
    assert_eq!(decoded(native(&h,"sync","upload",&key,&upload).await).await,b"OK");
    let result:Value=serde_json::from_slice(&decoded(native(&h,"msync","uploadChanges",&key,&media()).await).await).unwrap();
    assert_eq!(result["data"],json!([1,1]));
    let before=store_bytes(&h.path().join("state/accounts"));
    let held=h.service.temp.reserve(h.service.config.temp_bytes.get()-4096).unwrap();
    error(h.post(&format!("/api/admin/accounts/{id}/reset-data"),json!({}),Some(&admin)).await,503,"storage_unavailable").await;
    assert_eq!(store_bytes(&h.path().join("state/accounts")),before);
    let c=rusqlite::Connection::open(h.database()).unwrap();
    for table in ["account_resets","account_data_epochs"] {
        assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
    drop(c);
    h.service.publish(authority,|_|Ok(())).await.unwrap();
    assert_eq!(h.get("/api/me",Some(&admin)).await.status(),200);
    drop(held);
    let downloaded=h.temp.path().join("retained-reset.anki2");
    std::fs::write(&downloaded,decoded(native(&h,"sync","download",&key,b"{}").await).await).unwrap();
    let c=rusqlite::Connection::open(downloaded).unwrap();
    assert_eq!(c.query_row("SELECT val FROM config WHERE key='synthetic-reset-payload'",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),vec![37u8;32768]);
    drop(c);
    let bytes=decoded(native(&h,"msync","downloadFiles",&key,br#"{"files":["reset.bin"]}"#).await).await;
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut media_bytes=Vec::new();
    std::io::Read::read_to_end(&mut archive.by_name("0").unwrap(),&mut media_bytes).unwrap();
    assert_eq!(media_bytes,b"synthetic reset media");
    let result=reset(&h,&admin,id).await;
    assert_eq!(result["storage"]["media_bytes"],0);
    assert_eq!(h.get("/api/me",Some(&admin)).await.status(),200);
}
#[tokio::test]
async fn resets_ready_members_disabled_members_and_sole_admin_without_replacing_credentials() {
    for self_admin in [false,true] {
        let h=Harness::new().await;
        let administrator=h.setup().await;
        let admin=h.login("root.admin",PASSWORD).await;
        let member=h.member(&admin,"reset.member").await;
        let target=if self_admin {&administrator} else {&member};
        let id=target["id"].as_str().unwrap(); let username=target["username"].as_str().unwrap();
        let cookie=h.login(username,PASSWORD).await;
        let (_,key)=h.service.native_login(IP,username,PASSWORD.into()).await.unwrap();
        let stale=h.service.validate_hostkey(&key).await.unwrap();
        let upload=collection(&h);
        assert_eq!(decoded(native(&h,"sync","upload",&key,&upload).await).await,b"OK");
        let result:Value=serde_json::from_slice(&decoded(native(&h,"msync","uploadChanges",&key,&media()).await).await).unwrap();
        assert_eq!(result["data"],json!([1,1]));
        let stores=h.path().join("state/accounts"); let account=stores.join(id);
        let other=store_bytes(&stores).into_iter().filter(|(path,_)|!path.starts_with(id)).collect::<std::collections::BTreeMap<_,_>>();
        assert!(std::fs::read_dir(account.join("generations")).unwrap().count()>1);
        let original={
            let c=rusqlite::Connection::open(h.database()).unwrap();
            credentials(&c,id)
        };
        let result=reset(&h,&admin,id).await;
        assert_eq!(result["id"],id); assert_eq!(result["role"],target["role"]); assert_eq!(result["enabled"],true);
        let bytes=result["storage"]["collection_bytes"].as_u64().unwrap();
        assert!(bytes>0); assert_eq!(result["storage"]["media_bytes"],0); assert_eq!(result["storage"]["total_bytes"],bytes);
        assert_eq!(std::fs::read_dir(account.join("generations")).unwrap().count(),1);
        assert_eq!(std::fs::read_dir(account.join("media")).unwrap().count(),0);
        let path=account.join("generations/initial/collection.anki2");
        let c=meguri_sync_server::collection::schema::open(&path,false).unwrap();
        for table in ["notes","cards","revlog","notetypes","config"] {
            assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get::<_,i64>(0)).unwrap(),0);
        }
        let pages:i64=c.pragma_query_value(None,"page_count",|r|r.get(0)).unwrap();
        let size:i64=c.pragma_query_value(None,"page_size",|r|r.get(0)).unwrap();
        assert_eq!(bytes,(pages*size) as u64); drop(c);
        assert_eq!(store_bytes(&stores).into_iter().filter(|(path,_)|!path.starts_with(id)).collect::<std::collections::BTreeMap<_,_>>(),other);
        assert_eq!(h.get("/api/me",Some(&cookie)).await.status(),200);
        h.login(username,PASSWORD).await;
        assert_eq!(h.service.validate_hostkey(&key).await.unwrap().account_id(),id);
        assert_eq!(h.service.publish(stale,|_|Ok(())).await.unwrap_err().code,"authentication_failed");
        let c=rusqlite::Connection::open(h.database()).unwrap();
        let current=credentials(&c,id);
        assert_eq!(current,original);
        assert_eq!(c.query_row("SELECT count(*) FROM account_resets",[],|r|r.get::<_,i64>(0)).unwrap(),0); drop(c);
        if !self_admin {
            assert_eq!(h.post(&format!("/api/admin/accounts/{id}/disable"),json!({}),Some(&admin)).await.status(),200);
            let result=reset(&h,&admin,id).await;
            assert_eq!(result["enabled"],false); assert_eq!(result["storage"]["media_bytes"],0);
        }
        let h=Harness::reopen(h.stop().await).await;
        assert_eq!(h.get("/api/me",Some(&admin)).await.status(),200);
        let _=h.stop().await;
    }
}
#[tokio::test]
async fn reset_requires_admin_exact_origin_and_strict_empty_json_without_target_leaks() {
    let h=Harness::new().await; let administrator=h.setup().await;
    let admin=h.login("root.admin",PASSWORD).await; let member=h.member(&admin,"reset.member").await;
    let cookie=h.login("reset.member",PASSWORD).await;
    let route=format!("/api/admin/accounts/{}/reset-data",administrator["id"].as_str().unwrap());
    let before=store_bytes(&h.path().join("state/accounts"));
    error(h.post(&route,json!({}),None).await,401,"authentication_failed").await;
    error(h.post(&route,json!({}),Some(&cookie)).await,403,"permission_denied").await;
    error(h.post("/api/admin/accounts/absent/reset-data",json!({}),Some(&cookie)).await,403,"permission_denied").await;
    error(h.post("/api/admin/accounts/absent/reset-data",json!({}),Some(&admin)).await,404,"route_not_found").await;
    error(h.post(&route,json!({"extra":true}),Some(&admin)).await,400,"invalid_request").await;
    error(h.client.post(format!("{}{route}",h.base)).header("cookie",&admin).json(&json!({})).send().await.unwrap(),403,"origin_rejected").await;
    error(h.client.post(format!("{}{route}",h.base)).header("origin",ORIGIN).header("cookie",&admin).header("content-type","text/plain").body("{}").send().await.unwrap(),415,"unsupported_content_type").await;
    assert_eq!(store_bytes(&h.path().join("state/accounts")),before);
    assert_eq!(member["role"],"member");
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn old_validated_collection_upload_cannot_publish_after_data_reset() {
    let h=Harness::new().await; let account=h.setup().await; let admin=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    let bytes=collection(&h); let pause=h.service.hooks.arm("collection_after_validation");
    let client=h.client.clone(); let base=h.base.clone();
    let upload=tokio::spawn(async move {
        client.post(format!("{base}/sync/upload"))
            .header("anki-sync",json!({"v":11,"k":key,"c":"synthetic-reset","s":""}).to_string())
            .body(zstd::stream::encode_all(bytes.as_slice(),0).unwrap()).send().await.unwrap()
    });
    within(pause.arrived()).await;
    let result=reset(&h,&admin,account["id"].as_str().unwrap()).await;
    pause.resume(); assert_eq!(within(upload).await.unwrap().status(),403);
    let current=h.get("/api/me/storage",Some(&admin)).await.json::<Value>().await.unwrap();
    assert_eq!(current["data"],result["storage"]);
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn reset_rechecks_original_admin_and_rejects_queued_stale_reset_without_second_purge() {
    let h=Harness::new().await; let account=h.setup().await; let admin=h.login("root.admin",PASSWORD).await;
    let id=account["id"].as_str().unwrap();
    let pause=h.service.hooks.arm("reset_data_before_commit");
    let authority=h.service.browser_authority(secret(&admin)).await.unwrap();
    let s=h.service.clone(); let target=id.to_owned();
    let stale=tokio::spawn(async move {s.reset_data(authority,target).await});
    within(pause.arrived()).await;
    reset(&h,&admin,id).await;
    pause.resume(); assert_eq!(within(stale).await.unwrap().unwrap_err().code,"storage_unavailable");
    let c=rusqlite::Connection::open(h.database()).unwrap();
    assert_eq!(c.query_row("SELECT epoch FROM account_data_epochs WHERE account_id=?1",[id],|r|r.get::<_,i64>(0)).unwrap(),1); drop(c);
    let pause=h.service.hooks.arm("reset_data_before_commit");
    let authority=h.service.browser_authority(secret(&admin)).await.unwrap(); let s=h.service.clone(); let target=id.to_owned();
    let stale=tokio::spawn(async move {s.reset_data(authority,target).await});
    within(pause.arrived()).await;
    h.service.logout(secret(&admin)).await.unwrap();
    pause.resume(); assert_eq!(within(stale).await.unwrap().unwrap_err().code,"stale_authority");
}

#[cfg(feature="test-hooks")]
async fn child_service(path: &std::path::Path, phase: &str, kill: bool, signal: &std::path::Path) -> (Child,String) {
    use std::{io::BufRead,process::{Command,Stdio}};
    let mut child=Child(Command::new(binary()).args(["serve",path.to_str().unwrap(),ORIGIN,"127.0.0.1:0"])
        .env(if kill {"MEGURI_TEST_KILL_PHASE"} else {"MEGURI_TEST_FAIL_PHASE"},phase).env("MEGURI_TEST_SIGNAL_FILE",signal)
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let stdout=child.0.stdout.take().unwrap();
    let line=within(tokio::task::spawn_blocking(move || {let mut line=String::new();std::io::BufReader::new(stdout).read_line(&mut line).unwrap();line})).await.unwrap();
    (child,format!("http://{}",line.trim().strip_prefix("listening ").unwrap()))
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn reset_crash_boundaries_recover_before_ready_validation_and_never_purge_recreated_store() {
    for (phase,committed) in [
        ("reset_data_before_metadata_commit",false),
        ("reset_data_after_metadata_commit",true),
        ("delete_after_quarantine",true),
        ("reset_data_after_purge",true),
        ("reset_data_before_provision",true),
        ("stores_during_stage_write",true),
        ("reset_data_after_provision",true),
        ("reset_data_before_journal_clear",true),
    ] {
        let h=Harness::new().await; h.setup().await; let admin=h.login("root.admin",PASSWORD).await;
        let member=h.member(&admin,"reset.member").await; let id=member["id"].as_str().unwrap();
        let cookie=h.login("reset.member",PASSWORD).await;
        let (_,key)=h.service.native_login(IP,"reset.member",PASSWORD.into()).await.unwrap();
        let account=h.path().join("state/accounts").join(id);
        let other=store_bytes(&h.path().join("state/accounts")).into_iter().filter(|(p,_)|!p.starts_with(id)).collect::<std::collections::BTreeMap<_,_>>();
        let temp=h.stop().await; let owner=temp.path().join("owner"); let signal=temp.path().join("reset-phase");
        let (mut child,base)=child_service(&owner,phase,true,&signal).await;
        let client=reqwest::Client::builder().no_proxy().build().unwrap();
        let request=client.post(format!("{base}/api/admin/accounts/{id}/reset-data")).header("origin",ORIGIN).header("cookie",&admin).json(&json!({}));
        let request=tokio::spawn(async move {request.send().await});
        wait_file(&signal,&mut child).await; child.0.kill().unwrap(); child.0.wait().unwrap(); drop(child);
        assert!(within(request).await.unwrap().is_err());
        let c=rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM account_resets WHERE id=?1",[id],|r|r.get::<_,i64>(0)).unwrap(),i64::from(committed));
        let recreated=phase=="reset_data_after_provision" || phase=="reset_data_before_journal_clear";
        if recreated {assert_eq!(c.query_row("SELECT phase FROM account_resets WHERE id=?1",[id],|r|r.get::<_,String>(0)).unwrap(),"recreate");}
        drop(c);
        // Recreate-phase recovery must not run recursive removal even once.
        // Preserve this newly introduced private file through every restart.
        if recreated {
            use std::os::unix::fs::OpenOptionsExt;
            let mut f=std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(account.join("recreated-proof")).unwrap();
            f.write_all(b"new generation must survive recovery").unwrap(); f.sync_all().unwrap();
        }
        let h=Harness::reopen(temp).await;
        assert_eq!(h.get("/api/me",Some(&cookie)).await.status(),200);
        assert!(h.service.validate_hostkey(&key).await.is_ok());
        assert!(account.join("generations/initial/collection.anki2").exists());
        if recreated {assert_eq!(std::fs::read(account.join("recreated-proof")).unwrap(),b"new generation must survive recovery");}
        assert_eq!(store_bytes(&h.path().join("state/accounts")).into_iter().filter(|(p,_)|!p.starts_with(id)).collect::<std::collections::BTreeMap<_,_>>(),other);
        let c=rusqlite::Connection::open(h.database()).unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM account_resets",[],|r|r.get::<_,i64>(0)).unwrap(),0); drop(c);
        let _=h.stop().await;
    }
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn failed_pending_reset_closes_target_data_and_delete_but_preserves_signin_and_closed_restore() {
    let h=Harness::new().await; h.setup().await; let admin=h.login("root.admin",PASSWORD).await;
    let member=h.member(&admin,"reset.member").await; let id=member["id"].as_str().unwrap();
    let cookie=h.login("reset.member",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"reset.member",PASSWORD.into()).await.unwrap();
    let temp=h.stop().await; let owner=temp.path().join("owner");
    let (mut child,base)=child_service(&owner,"reset_data_after_metadata_commit",false,&temp.path().join("unused")).await;
    let client=reqwest::Client::builder().no_proxy().build().unwrap();
    let post=|action:&str|client.post(format!("{base}/api/admin/accounts/{id}/{action}")).header("origin",ORIGIN).header("cookie",&admin).json(&json!({}));
    error(post("reset-data").send().await.unwrap(),503,"provision_unavailable").await;
    error(post("delete").send().await.unwrap(),503,"storage_unavailable").await;
    assert_eq!(client.get(format!("{base}/api/me")).header("cookie",&cookie).send().await.unwrap().status(),200);
    let own=client.get(format!("{base}/api/me/storage")).header("cookie",&cookie).send().await.unwrap().json::<Value>().await.unwrap();
    assert_eq!(own["data"],Value::Null);
    let native=client.post(format!("{base}/sync/meta")).header("anki-sync",json!({"v":11,"k":key,"c":"synthetic","s":""}).to_string()).body(zstd::stream::encode_all(br#"{"v":11}"#.as_slice(),0).unwrap()).send().await.unwrap();
    assert_ne!(native.status(),200);
    child.0.kill().unwrap(); child.0.wait().unwrap(); drop(child);
    let before=store_bytes(&owner.join("state/accounts"));
    let c=rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();c.execute("UPDATE service_state SET admission_open=0",[]).unwrap();drop(c);
    let h=Harness::reopen(temp).await;
    assert_eq!(store_bytes(&owner.join("state/accounts")),before);
    let c=rusqlite::Connection::open(h.database()).unwrap();assert_eq!(c.query_row("SELECT count(*) FROM account_resets",[],|r|r.get::<_,i64>(0)).unwrap(),1);drop(c);
    let temp=h.stop().await;
    let c=rusqlite::Connection::open(owner.join("state/accounts.sqlite")).unwrap();c.execute("UPDATE service_state SET admission_open=1",[]).unwrap();drop(c);
    let h=Harness::reopen(temp).await; assert!(h.service.validate_hostkey(&key).await.is_ok());
    assert_eq!(h.get("/api/me",Some(&cookie)).await.status(),200);
    let _=h.stop().await;
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn earlier_media_upload_cannot_republish_names_or_blobs_after_reset() {
    let h=Harness::new().await; let account=h.setup().await; let admin=h.login("root.admin",PASSWORD).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    let pause=h.service.hooks.arm("media_before_publication");
    let client=h.client.clone(); let base=h.base.clone(); let old_key=key.clone();
    let upload=tokio::spawn(async move {
        client.post(format!("{base}/msync/uploadChanges"))
            .header("anki-sync",json!({"v":11,"k":old_key,"c":"synthetic-reset","s":""}).to_string())
            .body(zstd::stream::encode_all(media().as_slice(),0).unwrap()).send().await.unwrap()
    });
    within(pause.arrived()).await;
    reset(&h,&admin,account["id"].as_str().unwrap()).await;
    pause.resume(); assert_ne!(within(upload).await.unwrap().status(),200);
    let directory=h.path().join("state/accounts").join(account["id"].as_str().unwrap()).join("media");
    assert_eq!(std::fs::read_dir(directory).unwrap().count(),0);
    let c=rusqlite::Connection::open(h.database()).unwrap();
    for table in ["media_names","media_blobs"] {
        assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table} WHERE account_id=?1"),[account["id"].as_str().unwrap()],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
    drop(c);
    assert!(h.service.validate_hostkey(&key).await.is_ok());
}
