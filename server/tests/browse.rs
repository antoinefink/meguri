// SPDX-License-Identifier: MIT
//! W12 through real browser authentication, loopback HTTP and native sync.
#![cfg(feature="test-hooks")]
mod support;
use support::*;
use meguri_sync_server::collection::schema;
use meguri_model::proto::{ProtoBlob, deck::{DeckCommon,DeckKind,DeckKindMessage,NormalDeck},deck_config::DeckConfigMessage,notetype::{FieldConfig,NotetypeConfig,TemplateConfig}};
use rusqlite::params;
use serde_json::{Value,json};
use std::path::PathBuf;
fn current(h:&Harness,id:&str)->PathBuf {
    let account=h.path().join("state/accounts").join(id);
    let pointer=std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation=pointer.lines().find_map(|s|s.strip_prefix("generation=")).unwrap();
    account.join("generations").join(generation).join("collection.anki2")
}
fn seed(h:&Harness,id:&str) {
    let mut db=schema::open(&current(h,id),true).unwrap(); let tx=db.transaction().unwrap();
    let common=ProtoBlob::new(DeckCommon::default()).to_bytes().into_owned();
    let mut normal=NormalDeck::default();normal.config_id=1;
    let kind=ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))).to_bytes().into_owned();
    for (did,name) in [(1,"Spanish"),(2,"Other")] {tx.execute("INSERT INTO decks VALUES(?1,?2,0,0,?3,?4)",params![did,name,common,kind]).unwrap();}
    let mut config=DeckConfigMessage::default();config.new_card_insert_order=1;
    tx.execute("INSERT INTO deck_config VALUES(1,'Ordered',0,0,?1)",[ProtoBlob::new(config).to_bytes().into_owned()]).unwrap();
    for (mid,name,fields,count) in [(10,"Vocabulary",vec!["Spanish","English","Example"],1),(20,"Two directions",vec!["Spanish","English","Example"],2),(30,"Other type",vec!["Front"],1)] {
        tx.execute("INSERT INTO notetypes VALUES(?1,?2,0,0,?3)",params![mid,name,ProtoBlob::new(NotetypeConfig::default()).to_bytes().into_owned()]).unwrap();
        for (ord,name) in fields.iter().enumerate() {tx.execute("INSERT INTO fields VALUES(?1,?2,?3,?4)",params![mid,ord as i64,name,ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned()]).unwrap();}
        for ord in 0..count {
            let mut config=TemplateConfig::default();config.q_format=format!("{{{{{}}}}}",fields[ord]);config.a_format="{{Spanish}} {{English}}".into();
            tx.execute("INSERT INTO templates VALUES(?1,?2,?3,0,0,?4)",params![mid,ord,if ord==0 {"Forward"} else {"Reverse"},ProtoBlob::new(config).to_bytes().into_owned()]).unwrap();
        }
    }
    tx.execute("INSERT INTO config VALUES('nextPos',0,0,?1)",[b"1".as_slice()]).unwrap();tx.commit().unwrap();
}
async fn request(h:&Harness,cookie:&str,id:&str,action:&str,body:Value)->reqwest::Response {
    h.client.post(format!("{}/api/me/browse/{action}",h.base)).header("origin",ORIGIN).header("cookie",cookie)
        .header("X-Meguri-Expected-Account",id).json(&body).send().await.unwrap()
}
async fn data(h:&Harness,cookie:&str,id:&str,action:&str,body:Value)->Value {
    let response=request(h,cookie,id,action,body).await;
    let status=response.status(); assert_eq!(response.headers()["cache-control"],"no-store");assert_eq!(response.headers()["connection"],"close");
    let body=response.json::<Value>().await.unwrap();assert_eq!(status,200,"{body}");assert_eq!(body["data"]["account_id"],id);body["data"].clone()
}
async fn generation(h:&Harness,cookie:&str,id:&str)->Value {data(h,cookie,id,"catalog",json!({})).await["generation"].clone()}
async fn create(h:&Harness,cookie:&str,id:&str,mid:&str,title:&str,example:&str)->Value {
    let generation=generation(h,cookie,id).await;
    data(h,cookie,id,"create",json!({"generation":generation,"deck_id":"1","note_type_id":mid,"fields":{"Spanish":title,"English":"<b>untouched</b>","Example":example},"tags":["initial"]})).await
}
async fn page(h:&Harness,cookie:&str,id:&str,body:Value)->Value {data(h,cookie,id,"page",body).await}
async fn detail(h:&Harness,cookie:&str,id:&str,card:&Value)->Value {data(h,cookie,id,"detail",json!({"card_id":card["card_id"]})).await}
async fn native(h:&Harness,key:&str,method:&str,body:Value)->Value {
    let response=h.client.post(format!("{}/sync/{method}",h.base)).header("anki-sync",json!({"v":11,"k":key,"c":"synthetic-browse","s":"browse001"}).to_string())
        .body(zstd::stream::encode_all(serde_json::to_vec(&body).unwrap().as_slice(),0).unwrap()).send().await.unwrap();
    let status=response.status();let bytes=response.bytes().await.unwrap();assert_eq!(status,200,"{method}");
    serde_json::from_slice(&zstd::stream::decode_all(bytes.as_ref()).unwrap()).unwrap()
}
async fn round(h:&Harness,key:&str,min:&Value,notes:u32,cards:u32,incoming:Option<Value>)->(Value,Value,Value) {
    let meta=native(h,key,"meta",json!({"v":11,"cv":"synthetic"})).await;
    let graves=native(h,key,"start",json!({"minUsn":min,"lnewer":false,"graves":null})).await;
    native(h,key,"applyChanges",json!({"changes":{"models":[],"decks":[[],[]],"tags":[]}})).await;
    let chunk=native(h,key,"chunk",json!({})).await;assert_eq!(chunk["done"],true);
    assert_eq!(native(h,key,"applyChunk",json!({"chunk":incoming.unwrap_or(json!({"done":true}))})).await,Value::Null);
    let counts=json!([[0,0,0],cards,notes,0,0,3,2,1]);
    assert_eq!(native(h,key,"sanityCheck2",json!({"client":counts})).await,json!({"status":"ok","c":counts,"s":counts}));
    assert!(native(h,key,"finish",json!({})).await.as_i64().unwrap()>0);
    (meta,graves,chunk)
}
#[tokio::test]
async fn field_examples_visible_text_sparse_card_keysets_and_exact_identities() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;seed(&h,id);
    for (title,example) in [("<b>el viaje</b>","Quiero aprovechar el viaje para visitar a mi hermana."),("aprovechar","Quiero aprovechar el día."),("la oportunidad","Hay que aprovechar la oportunidad.")] {create(&h,&cookie,id,"10",title,example).await;}
    let first=page(&h,&cookie,id,json!({"query":"APROVECHAR","limit":1})).await;
    assert_eq!(first["cards"][0]["title"],"el viaje");assert_eq!(first["cards"][0]["field"],"Example");
    assert_eq!(first["cards"][0]["excerpt"].as_array().unwrap().iter().filter(|f|f["matched"]==true).map(|f|f["text"].as_str().unwrap()).collect::<Vec<_>>(),["aprovechar"]);
    let mut cursor=first["next_cursor"].clone();let mut ids=vec![first["cards"][0]["card_id"].clone()];
    while !cursor.is_null(){let p=page(&h,&cookie,id,json!({"query":"APROVECHAR","limit":1,"cursor":cursor})).await;ids.extend(p["cards"].as_array().unwrap().iter().map(|c|c["card_id"].clone()));cursor=p["next_cursor"].clone();}
    assert_eq!(ids.len(),3);let scoped=page(&h,&cookie,id,json!({"query":"aprovechar","field":"Spanish","deck_id":"1"})).await;assert_eq!(scoped["cards"].as_array().unwrap().len(),1);assert_eq!(scoped["cards"][0]["title"],"aprovechar");
    assert!(page(&h,&cookie,id,json!({"query":"aprovechar","field":"Absent"})).await["cards"].as_array().unwrap().is_empty());
    create(&h,&cookie,id,"10","<img src='misleading.png'><script>var markup='<audio>misleading';</script><span hidden>misleading</span><span style='display:none'>misleading</span>Visible &amp; safe","[sound:misleading.mp3] <span title='misleading'>Shown</span>").await;
    assert!(page(&h,&cookie,id,json!({"query":"misleading"})).await["cards"].as_array().unwrap().is_empty());
    let literal=create(&h,&cookie,id,"10","[a.b] % _","literal").await;
    assert_eq!(page(&h,&cookie,id,json!({"query":"[A.B] % _"})).await["cards"].as_array().unwrap().len(),1);
    // Sparse deck matches beyond the 200-card inspection bound. Each empty
    // page carries progress instead of falsely announcing end-of-results.
    let db=schema::open(&current(&h,id),true).unwrap();
    let nid=literal["note_id"].as_str().unwrap().parse::<i64>().unwrap();
    for i in 1..=405 {db.execute("INSERT INTO cards SELECT ?1,nid,2,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data FROM cards WHERE nid=?2 LIMIT 1",params![i,nid]).unwrap();}
    db.execute("UPDATE cards SET did=1 WHERE id=405",[]).unwrap();
    let sparse=page(&h,&cookie,id,json!({"deck_id":"1","query":"[a.b]"})).await;assert_eq!(sparse["cards"],json!([]));assert!(sparse["next_cursor"].is_string());
    let next=page(&h,&cookie,id,json!({"deck_id":"1","query":"[a.b]","cursor":sparse["next_cursor"]})).await;assert_eq!(next["cards"],json!([]));assert!(next["next_cursor"].is_string());
    let last=page(&h,&cookie,id,json!({"deck_id":"1","query":"[a.b]","cursor":next["next_cursor"]})).await;assert!(last["cards"].as_array().unwrap().iter().any(|c|c["card_id"]=="405"));
    db.execute("INSERT INTO cards SELECT 9007199254740993,nid,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data FROM cards WHERE id=405",[]).unwrap();
    let exact=data(&h,&cookie,id,"detail",json!({"card_id":"9007199254740993"})).await;assert_eq!(exact["card"]["card_id"],"9007199254740993");drop(db);h.stop().await;
}
#[tokio::test]
async fn writes_siblings_stale_delete_and_current_usn_graves_reach_incremental_sync() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;seed(&h,id);
    let key=native(&h,"","hostKey",json!({"u":"root.admin","p":PASSWORD})).await["key"].as_str().unwrap().to_owned();
    let created=create(&h,&cookie,id,"20","el viaje","Quiero aprovechar el viaje para visitar a mi hermana.").await;
    let p=page(&h,&cookie,id,json!({"query":"aprovechar"})).await;let cards=p["cards"].as_array().unwrap();assert_eq!(cards.len(),2);assert_ne!(cards[0]["card_id"],cards[1]["card_id"]);
    let d=detail(&h,&cookie,id,&cards[0]).await;assert_eq!(d["sibling_ids"].as_array().unwrap().len(),2);assert_eq!(d["card"]["template"],"Forward");assert_eq!(detail(&h,&cookie,id,&cards[1]).await["card"]["template"],"Reverse");
    let (base,g,chunk)=round(&h,&key,&json!(0),1,2,None).await;assert_eq!(g,json!({"cards":[],"notes":[],"decks":[]}));assert_eq!(chunk["cards"].as_array().unwrap().len(),2);assert_eq!(chunk["notes"][0][0].to_string(),created["note_id"].as_str().unwrap());
    let fresh=detail(&h,&cookie,id,&cards[0]).await;
    let edited=data(&h,&cookie,id,"edit",json!({"note_id":created["note_id"],"revision":fresh["note"]["revision"],"fields":{"Example":"Quiero aprovechar el viaje para ver a mi hermana."},"add_tags":["corrected"]})).await;
    let read=detail(&h,&cookie,id,&cards[1]).await;assert_eq!(read["note"]["fields"]["English"],"<b>untouched</b>");assert_eq!(read["note"]["revision"],edited["revision"]);
    error(request(&h,&cookie,id,"edit",json!({"note_id":created["note_id"],"revision":fresh["note"]["revision"],"fields":{"Example":"stale draft"}})).await,409,"browse_conflict").await;
    let (second,_,chunk)=round(&h,&key,&base["usn"],1,2,None).await;assert!(chunk["notes"][0][6].as_str().unwrap().contains("para ver"));assert_eq!(chunk["notes"][0][5]," corrected initial ");
    // A device correction in a real committing round invalidates an open web draft.
    let open=detail(&h,&cookie,id,&cards[0]).await;
    let mut device=chunk["notes"][0].as_array().unwrap().clone();device[3]=json!(i64::MAX/2);device[4]=json!(-1);device[6]=json!("el viaje\u{1f}<b>untouched</b>\u{1f}Device correction");
    round(&h,&key,&second["usn"],1,2,Some(json!({"done":true,"notes":[device]}))).await;
    error(request(&h,&cookie,id,"edit",json!({"note_id":created["note_id"],"revision":open["note"]["revision"],"fields":{"Example":"web draft"}})).await,409,"browse_conflict").await;
    let d=detail(&h,&cookie,id,&cards[0]).await;assert_eq!(d["note"]["fields"]["Example"],"Device correction");
    // Keep a stale grave for a reused identity: deletion must advance its USN.
    let db=schema::open(&current(&h,id),true).unwrap();let cid=cards[0]["card_id"].as_str().unwrap().parse::<i64>().unwrap();
    db.execute("INSERT INTO graves VALUES(?1,0,0)",[cid]).unwrap();
    let nid=created["note_id"].as_str().unwrap().parse::<i64>().unwrap();
    db.execute("INSERT INTO graves VALUES(?1,1,0)",[nid]).unwrap();
    let old_usn:i64=db.query_row("SELECT usn FROM col",[],|r|r.get(0)).unwrap();
    // The same count with different sibling identities must invalidate confirmation.
    let other=cards[1]["card_id"].as_str().unwrap().parse::<i64>().unwrap();db.execute("UPDATE cards SET id=id+100 WHERE id=?1",[other]).unwrap();
    error(request(&h,&cookie,id,"delete",json!({"note_id":d["note"]["note_id"],"revision":d["note"]["revision"],"confirmation":d["confirmation"]})).await,409,"browse_conflict").await;
    let d=detail(&h,&cookie,id,&cards[0]).await;
    data(&h,&cookie,id,"delete",json!({"note_id":d["note"]["note_id"],"revision":d["note"]["revision"],"confirmation":d["confirmation"]})).await;
    assert_eq!(db.query_row("SELECT usn FROM graves WHERE oid=?1 AND type=0",[cid],|r|r.get::<_,i64>(0)).unwrap(),old_usn);
    assert_eq!(db.query_row("SELECT usn FROM graves WHERE oid=?1 AND type=1",[nid],|r|r.get::<_,i64>(0)).unwrap(),old_usn);
    let (_,graves,chunk)=round(&h,&key,&json!(old_usn),0,0,None).await;
    assert_eq!(graves["cards"].as_array().unwrap().len(),2);assert_eq!(graves["notes"][0].to_string(),created["note_id"].as_str().unwrap());assert!(chunk["notes"].as_array().unwrap().is_empty());
    assert!(page(&h,&cookie,id,json!({})).await["cards"].as_array().unwrap().is_empty());drop(db);h.stop().await;
}
#[tokio::test]
async fn strict_account_binding_epoch_publication_and_session_revalidation() {
    let h=Harness::new().await;let root=h.setup().await;let id=root["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;seed(&h,id);
    let member=h.member(&cookie,"browse.member").await;let other=member["id"].as_str().unwrap();let member_cookie=h.login("browse.member",PASSWORD).await;
    let created=create(&h,&cookie,id,"10","Private synthetic","example").await;
    for (action,body) in [
        ("catalog",json!({})), ("page",json!({})), ("detail",json!({"card_id":"1"})),
        ("create",json!({"generation":"g","deck_id":"1","note_type_id":"10","fields":{}})),
        ("edit",json!({"note_id":"1","revision":"r","fields":{}})),
        ("delete",json!({"note_id":"1","revision":"r","confirmation":"c"})),
    ] {
        error(h.post(&format!("/api/me/browse/{action}"),body.clone(),None).await,401,"authentication_failed").await;
        error(request(&h,&member_cookie,id,action,body).await,409,"stale_authority").await;
    }
    assert!(page(&h,&member_cookie,other,json!({})).await["cards"].as_array().unwrap().is_empty());
    let card=page(&h,&cookie,id,json!({})).await["cards"][0].clone();
    error(request(&h,&member_cookie,other,"detail",json!({"card_id":card["card_id"]})).await,404,"route_not_found").await;
    for header in ["bad".to_owned(),format!("{id},{id}")] {
        let response=h.client.post(format!("{}/api/me/browse/page",h.base)).header("origin",ORIGIN).header("cookie",&cookie).header("X-Meguri-Expected-Account",header).json(&json!({})).send().await.unwrap();error(response,400,"invalid_request").await;
    }
    let duplicate=h.client.post(format!("{}/api/me/browse/page",h.base)).header("origin",ORIGIN).header("cookie",&cookie).header("X-Meguri-Expected-Account",id).header("X-Meguri-Expected-Account",id).json(&json!({})).send().await.unwrap();error(duplicate,400,"invalid_request").await;
    error(h.post("/api/me/browse/page?account=x",json!({}),Some(&cookie)).await,400,"invalid_request").await;
    let raw=h.client.post(format!("{}/api/me/browse/page",h.base)).header("origin",ORIGIN).header("cookie",&cookie).header("content-type","application/json").body("{\"query\":\"a\",\"query\":\"b\"}").send().await.unwrap();error(raw,400,"invalid_request").await;
    let old_generation=generation(&h,&cookie,id).await;
    let stale_create=json!({"generation":old_generation,"deck_id":"1","note_type_id":"10","fields":{"Spanish":"Must not appear"}});
    let path=current(&h,id);schema::checkpoint(&schema::open(&path,true).unwrap()).unwrap();let bytes=std::fs::read(&path).unwrap();
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    let upload=h.client.post(format!("{}/sync/upload",h.base)).header("anki-sync",json!({"v":11,"k":key,"c":"synthetic","s":"replace"}).to_string()).body(zstd::stream::encode_all(bytes.as_slice(),0).unwrap()).send().await.unwrap();assert_eq!(upload.status(),200);assert_eq!(zstd::stream::decode_all(upload.bytes().await.unwrap().as_ref()).unwrap(),b"OK");
    error(request(&h,&cookie,id,"create",stale_create).await,409,"browse_conflict").await;
    let member_generation=generation(&h,&member_cookie,other).await;
    h.post(&format!("/api/admin/accounts/{other}/reset-data"),json!({}),Some(&cookie)).await.error_for_status().unwrap();
    error(request(&h,&member_cookie,other,"create",json!({"generation":member_generation,"deck_id":"1","note_type_id":"10","fields":{"Spanish":"stale epoch"}})).await,409,"browse_conflict").await;
    // Expire this synthetic session after SQL work and before COMMIT. The
    // final synchronous authority read must see durable expiry and roll back.
    let d=detail(&h,&cookie,id,&card).await;let pause=h.service.hooks.arm("browse_before_completion");
    let action=request(&h,&cookie,id,"edit",json!({"note_id":created["note_id"],"revision":d["note"]["revision"],"fields":{"Example":"must roll back"}}));
    let expire=async {
        pause.arrived().await;
        let c=rusqlite::Connection::open(h.database()).unwrap();
        c.execute("UPDATE credentials SET expires_at=0 WHERE account_id=?1 AND kind='session'",[id]).unwrap();
        pause.resume();
    };
    let (response,())=tokio::join!(action,expire);error(response,401,"authentication_failed").await;
    let cookie=h.login("root.admin",PASSWORD).await;assert_eq!(detail(&h,&cookie,id,&card).await["note"]["fields"]["Example"],"example");
    h.stop().await;
}

#[tokio::test]
async fn continuation_binding_and_active_native_writer_refuse_without_retiring_sync() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;seed(&h,id);
    create(&h,&cookie,id,"20","siblings","literal [a.b]").await;
    let p=page(&h,&cookie,id,json!({"limit":1})).await;
    error(request(&h,&cookie,id,"page",json!({"limit":1,"query":"other","cursor":p["next_cursor"]})).await,409,"browse_conflict").await;
    let d=detail(&h,&cookie,id,&p["cards"][0]).await;
    data(&h,&cookie,id,"edit",json!({"note_id":d["note"]["note_id"],"revision":d["note"]["revision"],"fields":{"Example":"changed"}})).await;
    error(request(&h,&cookie,id,"page",json!({"limit":1,"cursor":p["next_cursor"]})).await,409,"browse_conflict").await;
    error(h.client.post(format!("{}/api/me/browse/page",h.base)).header("cookie",&cookie).json(&json!({})).send().await.unwrap(),403,"origin_rejected").await;
    error(h.client.post(format!("{}/api/me/browse/page",h.base)).header("cookie",&cookie).header("origin",ORIGIN).header("content-type","text/plain").body("{}").send().await.unwrap(),415,"unsupported_content_type").await;
    error(request(&h,&cookie,id,"catalog",json!({"account_id":id})).await,400,"invalid_request").await;
    let key=native(&h,"","hostKey",json!({"u":"root.admin","p":PASSWORD})).await["key"].as_str().unwrap().to_owned();
    native(&h,&key,"start",json!({"minUsn":0,"lnewer":false,"graves":null})).await;
    let generation=generation(&h,&cookie,id).await;
    error(request(&h,&cookie,id,"create",json!({"generation":generation,"deck_id":"1","note_type_id":"10","fields":{"Spanish":"busy"}})).await,409,"busy").await;
    assert_eq!(page(&h,&cookie,id,json!({})).await["cards"].as_array().unwrap().len(),2);
    // The retained native round is still healthy and explicitly aborts.
    assert_eq!(native(&h,&key,"abort",json!({})).await,Value::Null);
    create(&h,&cookie,id,"10","after abort","works").await;
    h.stop().await;
}
#[tokio::test]
async fn sign_out_serializes_with_browser_collection_commit() {
    let h=Harness::new().await;let account=h.setup().await;let id=account["id"].as_str().unwrap();let cookie=h.login("root.admin",PASSWORD).await;seed(&h,id);
    create(&h,&cookie,id,"10","Sign out ordering","before").await;
    let p=page(&h,&cookie,id,json!({})).await;let d=detail(&h,&cookie,id,&p["cards"][0]).await;
    let (_,key)=h.service.native_login(IP,"root.admin",PASSWORD.into()).await.unwrap();
    let a=h.service.validate_hostkey(&key).await.unwrap();let barrier=h.service.publication_barrier(&a).await.unwrap();
    let pause=h.service.hooks.arm("browse_before_completion");
    let write=request(&h,&cookie,id,"edit",json!({"note_id":d["note"]["note_id"],"revision":d["note"]["revision"],"fields":{"Example":"committed before sign-out"}}));
    let logout=async {
        pause.arrived().await;
        let service=h.service.clone();let token=cookie.split_once('=').unwrap().1.to_owned();
        let action=tokio::spawn(async move {service.logout(&token).await});
        tokio::time::timeout(std::time::Duration::from_secs(10),barrier.browser_commit_waiter_pending()).await.unwrap();
        pause.resume();action.await.unwrap().unwrap();
    };
    let (response,())=tokio::join!(write,logout);assert_eq!(response.status(),200);
    error(request(&h,&cookie,id,"page",json!({})).await,401,"authentication_failed").await;
    let cookie=h.login("root.admin",PASSWORD).await;assert_eq!(detail(&h,&cookie,id,&p["cards"][0]).await["note"]["fields"]["Example"],"committed before sign-out");
    h.stop().await;
}
