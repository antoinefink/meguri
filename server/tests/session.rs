// SPDX-License-Identifier: MIT
//! Original loopback protocol fixtures. These are not reference-client lanes.
mod support;
use support::*;
use serde_json::{Value,json};
use rusqlite::{Connection,OpenFlags};

async fn request(h:&Harness,method:&str,key:&str,session:&str,bytes:&[u8])->reqwest::Response {
    h.client.post(format!("{}/sync/{method}",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"s":session,"c":"synthetic"}).to_string())
        .body(zstd::stream::encode_all(bytes,0).unwrap()).send().await.unwrap()
}
async fn native(h:&Harness,method:&str,key:&str,session:&str,value:Value)->reqwest::Response {
    request(h,method,key,session,&serde_json::to_vec(&value).unwrap()).await
}
async fn decoded(r:reqwest::Response)->Vec<u8> {
    assert_eq!(r.status(),200);
    let expected:usize=r.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
    let bytes=zstd::stream::decode_all(r.bytes().await.unwrap().as_ref()).unwrap();
    assert_eq!(bytes.len(),expected);bytes
}
async fn call(h:&Harness,method:&str,key:&str,session:&str,value:Value)->Value {
    serde_json::from_slice(&decoded(native(h,method,key,session,value).await).await).unwrap()
}
async fn login(h:&Harness,name:&str)->String {
    call(h,"hostKey","","auth",json!({"u":name,"p":PASSWORD})).await["key"].as_str().unwrap().into()
}
fn db(h:&Harness,id:&str)->Connection {
    let account=h.path().join("state/accounts").join(id);
    let manifest=std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation=manifest.lines().find_map(|l|l.strip_prefix("generation=")).unwrap();
    let c=Connection::open_with_flags(account.join("generations").join(generation).join("collection.anki2"),OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    meguri_sync_server::collection::unicase::register(&c).unwrap();c
}
fn count(h:&Harness,id:&str,table:&str)->i64 {db(h,id).query_row(&format!("SELECT count(*) FROM {table}"),[],|r|r.get(0)).unwrap()}
fn stamps(h:&Harness,id:&str)->(i64,i32,i64) {db(h,id).query_row("SELECT mod,usn,ls FROM col",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap()}
fn changes(tags:Value)->Value {json!({"changes":{"models":[],"decks":[[],[]],"tags":tags}})}
async fn start(h:&Harness,key:&str,s:&str,min:i32) {call(h,"start",key,s,json!({"minUsn":min,"lnewer":false,"graves":null})).await;}
async fn receive(h:&Harness,key:&str,s:&str)->Vec<Value> {
    let mut all=vec![];
    loop {let c=call(h,"chunk",key,s,json!({"_pad":null})).await;let done=c["done"]==true;all.push(c);if done {return all;}}
}
async fn ready(h:&Harness,key:&str,s:&str,counts:Value) {
    let result=call(h,"sanityCheck2",key,s,json!({"client":counts})).await;assert_eq!(result["status"],"ok");
}
async fn finish(h:&Harness,key:&str,s:&str)->i64 {call(h,"finish",key,s,json!({"_pad":null})).await.as_i64().unwrap()}
fn model()->Value {
    // Independently authored synthetic content, no borrowed stock type/assets.
    json!({"id":10,"name":"Authored","type":0,"mod":10,"usn":0,"sortf":0,"did":null,"css":".authored { color: inherit; }",
        "flds":[{"name":"Prompt","ord":0,"sticky":false,"rtl":false,"font":"","size":12}],
        "tmpls":[{"name":"Authored template","ord":0,"qfmt":"{{Prompt}}","afmt":"{{Prompt}}","bqfmt":"","bafmt":"","did":null,"bfont":"","bsize":0}]})
}
fn note(id:i64,mtime:i64,usn:i32,text:&str)->Value {json!([id,"authored-guid",10,mtime,usn," tag\u{3000}two ",text,"","",0,""])}
fn card(id:i64,usn:i32)->Value {json!([id,id,1,0,20,usn,0,0,1,0,0,0,0,0,0,0,0,"{}"])}
fn review(id:i64,usn:i32,ease:u8)->Value {json!([id,1,usn,ease,-60,0,2500,1000,1])}
async fn with_model(h:&Harness,key:&str,s:&str,min:i32) {
    start(h,key,s,min).await;
    call(h,"applyChanges",key,s,json!({"changes":{"models":[model()],"decks":[[],[]],"tags":[]}})).await;
    receive(h,key,s).await;
}

#[tokio::test]
async fn session_wrong_key_isolated_invalid_order_and_decode_retire_only_owner() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    let original=stamps(&h,id);
    for mode in 0..3 {
        start(&h,&key,"owner",0).await;
        call(&h,"applyChanges",&key,"owner",changes(json!(["unpublished"]))).await;
        assert_eq!(native(&h,"abort",&key,"other",json!({})).await.status(),409);
        assert_eq!(native(&h,"finish",&key,"other",json!({})).await.status(),409);
        assert_eq!(count(&h,id,"tags"),0);
        let refused=match mode {
            0=>native(&h,"finish",&key,"owner",json!({})).await,
            1=>request(&h,"chunk",&key,"owner",br#"{"_pad":null,"unfinished":}"#).await,
            _=>request(&h,"applyChunk",&key,"owner",br#"{"chunk":{"done":true},"chunk":{"done":true}}"#).await,
        };
        assert_eq!(refused.status(),if mode==0 || mode==2 {409} else {400});
        assert_eq!(native(&h,"chunk",&key,"owner",json!({})).await.status(),409);
        assert_eq!(stamps(&h,id),original);assert_eq!(count(&h,id,"tags"),0);
    }
    h.stop().await;
}

#[tokio::test]
async fn session_restart_replacement_and_bad_sanity_roll_back_actual_transaction() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap().to_owned();let key=login(&h,"root.admin").await;
    let before=stamps(&h,&id);
    start(&h,&key,"same",0).await;
    call(&h,"applyChanges",&key,"same",changes(json!(["discard-first"]))).await;
    start(&h,&key,"same",0).await;
    call(&h,"applyChanges",&key,"same",changes(json!(["discard-second"]))).await;
    start(&h,&key,"new",0).await;
    assert_eq!(native(&h,"abort",&key,"same",json!({})).await.status(),409);
    call(&h,"applyChanges",&key,"new",changes(json!(["discard-bad-sanity"]))).await;
    receive(&h,&key,"new").await;
    call(&h,"applyChunk",&key,"new",json!({"chunk":{"done":true,"revlog":[review(1,0,2)]}})).await;
    let bad=call(&h,"sanityCheck2",&key,"new",json!({"client":[[0,0,0],0,0,0,0,0,0,0]})).await;
    assert_eq!(bad["status"],"bad");assert_eq!(bad["s"],json!([[0,0,0],0,0,1,0,0,0,0]));
    assert_eq!(native(&h,"finish",&key,"new",json!({})).await.status(),409);
    assert_eq!(stamps(&h,&id),before);assert_eq!(count(&h,&id,"tags"),0);assert_eq!(count(&h,&id,"revlog"),0);
    start(&h,&key,"shutdown",0).await;
    call(&h,"applyChanges",&key,"shutdown",changes(json!(["discard-restart"]))).await;
    let temp=h.stop().await;let h=Harness::reopen(temp).await;
    assert_eq!(native(&h,"finish",&key,"shutdown",json!({})).await.status(),409);
    assert_eq!(stamps(&h,&id),before);assert_eq!(count(&h,&id,"tags"),0);
    h.stop().await;
}

#[tokio::test]
async fn session_250_and_502_tuple_boundaries_and_251_graves_preserve_orphan_notes() {
    for total in [125,251] {
        let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
        with_model(&h,&key,"insert",0).await;
        let mut objects=vec![];
        for n in 1..=total {objects.push((note(n,20,0,"authored"),card(n,0)));}
        for group in objects.chunks(125) {
            call(&h,"applyChunk",&key,"insert",json!({"chunk":{"done":false,"notes":group.iter().map(|p|p.0.clone()).collect::<Vec<_>>(),"cards":group.iter().map(|p|p.1.clone()).collect::<Vec<_>>()}})).await;
        }
        call(&h,"applyChunk",&key,"insert",json!({"chunk":{"done":true}})).await;
        assert_eq!(count(&h,id,"notes"),0);
        ready(&h,&key,"insert",json!([[4,5,6],total,total,0,999,1,0,0])).await;
        finish(&h,&key,"insert").await;
        assert_eq!(count(&h,id,"notes"),total);assert_eq!(count(&h,id,"cards"),total);
        start(&h,&key,"read",0).await;call(&h,"applyChanges",&key,"read",changes(json!([]))).await;
        let received=receive(&h,&key,"read").await;
        let lengths:Vec<_>=received.iter().map(|c|c["notes"].as_array().unwrap().len()+c["cards"].as_array().unwrap().len()+c["revlog"].as_array().unwrap().len()).collect();
        assert_eq!(lengths,if total==125 {vec![250,0]} else {vec![250,250,2]});
        let ids:std::collections::BTreeSet<_>=received.iter().flat_map(|c|c["notes"].as_array().unwrap().iter().map(|r|r[0].as_i64().unwrap())).collect();
        assert_eq!(ids.len(),total as usize);
        call(&h,"abort",&key,"read",json!({})).await;
        if total==251 {
            start(&h,&key,"delete",1).await;
            for group in (1..=total).collect::<Vec<_>>().chunks(250) {
                call(&h,"applyGraves",&key,"delete",json!({"chunk":{"cards":group,"notes":[],"decks":[]}})).await;
            }
            call(&h,"applyChanges",&key,"delete",changes(json!([]))).await;receive(&h,&key,"delete").await;
            call(&h,"applyChunk",&key,"delete",json!({"chunk":{"done":true}})).await;
            ready(&h,&key,"delete",json!([[0,0,0],0,total,0,0,1,0,0])).await;finish(&h,&key,"delete").await;
            assert_eq!(count(&h,id,"cards"),0);assert_eq!(count(&h,id,"notes"),251);assert_eq!(count(&h,id,"graves"),251);
        }
        h.stop().await;
    }
}

#[tokio::test]
async fn session_ordered_note_and_review_occurrences_keep_first_review_and_derive_fields() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    with_model(&h,&key,"ordered",0).await;
    call(&h,"applyChunk",&key,"ordered",json!({"chunk":{"done":false,
        "notes":[note(1,30,-1,"first"),note(1,10,-1,"a\u{1}b"),note(2,30,0,"pending-first")],
        "revlog":[review(1,0,2),review(1,0,4)]}})).await;
    call(&h,"applyChunk",&key,"ordered",json!({"chunk":{"done":true,
        "notes":[note(1,5,-1,"e\u{301}"),note(2,10,-1,"blocked")],"revlog":[review(1,0,3)]}})).await;
    ready(&h,&key,"ordered",json!([[0,0,0],0,2,1,0,1,0,0])).await;
    let stamp=finish(&h,&key,"ordered").await;
    assert_eq!(stamps(&h,id),(stamp,1,stamp));
    let c=db(&h,id);
    // The tuple type fixes each independently observed SQLite column's range.
    type StoredNoteProof=(i64,i32,String,String,u32,i64,String,String);
    assert_eq!(c.query_row("SELECT mod,usn,flds,sfld,csum,flags,data,tags FROM notes WHERE id=1",[],|r| -> rusqlite::Result<StoredNoteProof> {
        Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))
    }).unwrap(),
        (5,-1,"e\u{301}".into(),"e\u{301}".into(),2118106353,0,String::new()," tag two ".into()));
    assert_eq!(c.query_row("SELECT flds FROM notes WHERE id=2",[],|r|r.get::<_,String>(0)).unwrap(),"pending-first");
    assert_eq!(c.query_row("SELECT count(*),ease FROM revlog",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,u8>(1)?))).unwrap(),(1,2));drop(c);
    // Ordinary replay after a finished round. The process test below separately
    // loses the actual finish acknowledgement at the committed boundary.
    assert_eq!(native(&h,"finish",&key,"ordered",json!({})).await.status(),409);
    start(&h,&key,"retry",0).await;call(&h,"applyChanges",&key,"retry",changes(json!([]))).await;receive(&h,&key,"retry").await;
    call(&h,"applyChunk",&key,"retry",json!({"chunk":{"done":true,"revlog":[review(1,1,4)]}})).await;
    ready(&h,&key,"retry",json!([[0,0,0],0,2,1,0,1,0,0])).await;finish(&h,&key,"retry").await;
    assert_eq!(count(&h,id,"revlog"),1);assert_eq!(db(&h,id).query_row("SELECT ease FROM revlog",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    h.stop().await;
}

#[tokio::test]
async fn session_accepted_invalid_note_cannot_be_hidden_by_later_occurrence() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    with_model(&h,&key,"invalid",0).await;
    assert_eq!(native(&h,"applyChunk",&key,"invalid",json!({"chunk":{"done":true,"notes":[note(1,30,-1,"two\u{1f}fields"),note(1,10,-1,"one")]}})).await.status(),400);
    assert_eq!(count(&h,id,"notetypes"),0);assert_eq!(count(&h,id,"notes"),0);
    assert_eq!(native(&h,"finish",&key,"invalid",json!({})).await.status(),409);
    h.stop().await;
}

#[tokio::test]
async fn session_password_reset_and_disable_close_unpublished_member_round() {
    for disable in [false,true] {
        let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;let member=h.member(&admin,"member").await;let id=member["id"].as_str().unwrap();let key=login(&h,"member").await;
        let before=stamps(&h,id);
        start(&h,&key,"revoked",0).await;call(&h,"applyChanges",&key,"revoked",changes(json!(["revoked-tag"]))).await;
        receive(&h,&key,"revoked").await;call(&h,"applyChunk",&key,"revoked",json!({"chunk":{"done":true,"revlog":[review(1,0,2)]}})).await;
        ready(&h,&key,"revoked",json!([[0,0,0],0,0,1,0,0,0,0])).await;
        let response=if disable {h.post(&format!("/api/admin/accounts/{id}/disable"),json!({}),Some(&admin)).await} else {
            let token=h.reset_link(&admin,id).await;h.post("/api/resets/claim",json!({"token":token,"new_password":NEW_PASSWORD}),None).await
        };
        assert_eq!(response.status(),200);
        assert_eq!(native(&h,"finish",&key,"revoked",json!({})).await.status(),403);
        assert_eq!(stamps(&h,id),before);assert_eq!(count(&h,id,"tags"),0);assert_eq!(count(&h,id,"revlog"),0);
        h.stop().await;
    }
}

fn deck(mtime:i64,name:&str)->Value {
    json!({"id":20,"name":name,"mod":mtime,"usn":0,"dyn":0,"conf":1,"desc":"authored deck","collapsed":true,"browserCollapsed":true,
        "newToday":[0,0],"revToday":[0,0],"lrnToday":[0,0],"timeToday":[0,0],"extendNew":0,"extendRev":0})
}
#[tokio::test]
async fn session_observed_deck_and_note_conflicts_use_prior_response_and_inclusive_cursor() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    start(&h,&key,"seed",0).await;
    call(&h,"applyChanges",&key,"seed",json!({"changes":{"models":[model()],"decks":[[deck(20,"Initial")],[]],"tags":[]}})).await;
    receive(&h,&key,"seed").await;
    call(&h,"applyChunk",&key,"seed",json!({"chunk":{"done":true,"notes":[note(1,20,0,"server"),note(2,20,-1,"below")],"revlog":[review(2,1,2)]}})).await;
    ready(&h,&key,"seed",json!([[0,0,0],0,2,1,0,1,1,0])).await;finish(&h,&key,"seed").await;
    // Existing-deck matrix is D7's selected object case; response precedes merge.
    let mut expected="Initial";
    for (mtime,name,accepted) in [(10,"Older",false),(20,"Equal",true),(30,"Newer",true)] {
        start(&h,&key,"deck",0).await;
        let response=call(&h,"applyChanges",&key,"deck",json!({"changes":{"models":[],"decks":[[deck(mtime,name)],[]],"tags":[]}})).await;
        assert_eq!(response["decks"][0][0]["name"],expected);
        receive(&h,&key,"deck").await;call(&h,"applyChunk",&key,"deck",json!({"chunk":{"done":true}})).await;
        ready(&h,&key,"deck",json!([[0,0,0],0,2,1,0,1,1,0])).await;finish(&h,&key,"deck").await;
        if accepted {expected=name;}
        assert_eq!(db(&h,id).query_row("SELECT name FROM decks WHERE id=20",[],|r|r.get::<_,String>(0)).unwrap(),expected);
    }
    // Equal/below/above USNs remain distinct, including values > JS exact range.
    start(&h,&key,"cursor",0).await;call(&h,"applyChanges",&key,"cursor",changes(json!([]))).await;
    let outgoing=receive(&h,&key,"cursor").await;
    assert_eq!(outgoing.iter().map(|v|v["notes"].as_array().unwrap().len()).sum::<usize>(),1);
    assert_eq!(outgoing.iter().map(|v|v["revlog"].as_array().unwrap().len()).sum::<usize>(),1);
    call(&h,"applyChunk",&key,"cursor",json!({"chunk":{"done":true,"notes":[note(1,20,0,"equal"),note(1,10,0,"older"),note(2,1,-1,"nonpending-older"),note(9007199254740993,1,0,"exact-id")]}})).await;
    ready(&h,&key,"cursor",json!([[0,0,0],0,3,1,0,1,1,0])).await;finish(&h,&key,"cursor").await;
    assert_eq!(db(&h,id).query_row("SELECT flds FROM notes WHERE id=1",[],|r|r.get::<_,String>(0)).unwrap(),"server");
    assert_eq!(db(&h,id).query_row("SELECT flds FROM notes WHERE id=2",[],|r|r.get::<_,String>(0)).unwrap(),"nonpending-older");
    start(&h,&key,"newer-note",0).await;call(&h,"applyChanges",&key,"newer-note",changes(json!([]))).await;
    let rows=receive(&h,&key,"newer-note").await;
    assert!(rows.iter().any(|chunk|chunk["notes"].as_array().unwrap().iter().any(|row|row[0].as_i64()==Some(9007199254740993))));
    call(&h,"applyChunk",&key,"newer-note",json!({"chunk":{"done":true,"notes":[note(1,30,0,"newer")]}})).await;
    ready(&h,&key,"newer-note",json!([[0,0,0],0,3,1,0,1,1,0])).await;finish(&h,&key,"newer-note").await;
    assert_eq!(db(&h,id).query_row("SELECT flds FROM notes WHERE id=1",[],|r|r.get::<_,String>(0)).unwrap(),"newer");
    h.stop().await;
}

#[cfg(feature="test-hooks")]
fn pending(h:&Harness,method:&str,key:&str,s:&str,bytes:Vec<u8>)->tokio::task::JoinHandle<reqwest::Response> {
    let client=h.client.clone();let url=format!("{}/sync/{method}",h.base);
    let header=json!({"v":11,"k":key,"s":s,"c":"synthetic"}).to_string();
    tokio::spawn(async move {client.post(url).header("anki-sync",header).body(zstd::stream::encode_all(bytes.as_slice(),0).unwrap()).send().await.unwrap()})
}
fn full_fixture(h:&Harness)->Vec<u8> {
    use std::os::unix::fs::OpenOptionsExt;
    let path=h.temp.path().join("replacement.anki2");
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
    meguri_sync_server::collection::schema::create_blank(&path,123,456).unwrap();
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.execute("INSERT INTO tags VALUES('full-generation',0,1,NULL)",[]).unwrap();
    meguri_sync_server::collection::schema::checkpoint(&c).unwrap();drop(c);std::fs::read(path).unwrap()
}
fn manifest(h:&Harness,id:&str)->String {std::fs::read_to_string(h.path().join("state/accounts").join(id).join("current-collection")).unwrap()}

#[tokio::test]
async fn session_full_upload_retires_round_and_new_round_commits_selected_generation() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    let before=manifest(&h,id);
    start(&h,&key,"old",0).await;call(&h,"applyChanges",&key,"old",changes(json!(["unpublished-old"]))).await;
    let upload=full_fixture(&h);
    assert_eq!(decoded(request(&h,"upload",&key,"full",&upload).await).await,b"OK");
    assert_ne!(manifest(&h,id),before);
    assert_eq!(native(&h,"finish",&key,"old",json!({})).await.status(),409);
    assert_eq!(count(&h,id,"tags"),1);
    start(&h,&key,"new",0).await;call(&h,"applyChanges",&key,"new",changes(json!(["committed-new"]))).await;receive(&h,&key,"new").await;
    call(&h,"applyChunk",&key,"new",json!({"chunk":{"done":true}})).await;ready(&h,&key,"new",json!([[0,0,0],0,0,0,0,0,0,0])).await;
    let stamp=finish(&h,&key,"new").await;assert_eq!(stamps(&h,id),(stamp,1,stamp));assert_eq!(count(&h,id,"tags"),2);
    h.stop().await;
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn session_pinned_stamped_download_is_immutable_across_both_publication_orders() {
    use std::os::unix::fs::OpenOptionsExt;
    for upload_first in [false,true] {
        let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
        start(&h,&key,"retired",0).await;call(&h,"applyChanges",&key,"retired",changes(json!(["discard-download"]))).await;
        let upload=full_fixture(&h);let original=manifest(&h,id);
        let pause=h.service.hooks.arm("collection_snapshot_pinned");
        let download=pending(&h,"download",&key,"download",b"{\"_pad\":null}".to_vec());within(pause.arrived()).await;
        let pinned=stamps(&h,id);assert_eq!(pinned.1,1);assert_eq!(count(&h,id,"tags"),0);
        assert_eq!(native(&h,"finish",&key,"retired",json!({})).await.status(),409);
        if upload_first {assert_eq!(decoded(request(&h,"upload",&key,"full",&upload).await).await,b"OK");}
        start(&h,&key,"normal",if upload_first {0} else {1}).await;
        call(&h,"applyChanges",&key,"normal",changes(json!(["acknowledged-normal"]))).await;receive(&h,&key,"normal").await;
        call(&h,"applyChunk",&key,"normal",json!({"chunk":{"done":true}})).await;ready(&h,&key,"normal",json!([[0,0,0],0,0,0,0,0,0,0])).await;finish(&h,&key,"normal").await;
        if !upload_first {assert_eq!(decoded(request(&h,"upload",&key,"full",&upload).await).await,b"OK");}
        assert_ne!(manifest(&h,id),original);assert_eq!(count(&h,id,"tags"),if upload_first {2} else {1});
        pause.resume();let bytes=decoded(within(download).await.unwrap()).await;
        let path=h.temp.path().join("pinned.anki2");
        std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
        std::fs::write(&path,bytes).unwrap();
        let c=meguri_sync_server::collection::schema::open(&path,false).unwrap();
        assert_eq!(c.query_row("SELECT mod,usn,ls FROM col",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i32>(1)?,r.get::<_,i64>(2)?))).unwrap(),pinned);
        assert_eq!(c.query_row("SELECT count(*) FROM tags",[],|r|r.get::<_,i64>(0)).unwrap(),0);drop(c);
        h.stop().await;
    }
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn session_finish_and_revocation_serialize_in_both_barrier_orders() {
    for finish_first in [false,true] {
        let h=Harness::new().await;h.setup().await;let admin=h.login("root.admin",PASSWORD).await;let member=h.member(&admin,"member").await;let id=member["id"].as_str().unwrap().to_owned();let key=login(&h,"member").await;
        start(&h,&key,"round",0).await;call(&h,"applyChanges",&key,"round",changes(json!(["normal-publication"]))).await;receive(&h,&key,"round").await;
        call(&h,"applyChunk",&key,"round",json!({"chunk":{"done":true,"revlog":[review(1,0,2)]}})).await;ready(&h,&key,"round",json!([[0,0,0],0,0,1,0,0,0,0])).await;
        let authority=h.service.validate_hostkey(&key).await.unwrap();let gate=h.service.publication_barrier(&authority).await.unwrap();
        let pause=h.service.hooks.arm(if finish_first {"collection_with_barrier"} else {"disable_with_barrier"});
        let actor=h.service.browser_authority(secret(&admin)).await.unwrap();let service=h.service.clone();let target=id.clone();
        let (finishing,disabling)=if finish_first {
            let finishing=pending(&h,"finish",&key,"round",b"{}".to_vec());within(pause.arrived()).await;
            // No shared writer is held while the account barrier is paused.
            Connection::open(h.database()).unwrap().execute_batch("BEGIN IMMEDIATE;ROLLBACK").unwrap();
            let disabling=tokio::spawn(async move {service.disable(actor,target).await});within(gate.waiter_pending()).await;
            (finishing,disabling)
        } else {
            let disabling=tokio::spawn(async move {service.disable(actor,target).await});within(pause.arrived()).await;
            let finishing=pending(&h,"finish",&key,"round",b"{}".to_vec());within(gate.waiter_pending()).await;
            (finishing,disabling)
        };
        pause.resume();let response=within(finishing).await.unwrap();within(disabling).await.unwrap().unwrap();
        assert_eq!(response.status(),if finish_first {200} else {403});
        assert_eq!(count(&h,&id,"revlog"),i64::from(finish_first));assert_eq!(count(&h,&id,"tags"),i64::from(finish_first));
        assert_eq!(native(&h,"finish",&key,"round",json!({})).await.status(),403);
        h.stop().await;
    }
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn session_process_death_before_commit_and_lost_finish_reply_recover_by_ordinary_retry() {
    use std::{io::BufRead,process::{Command,Stdio}};
    for (phase,committed) in [("normal_before_commit",false),("normal_after_commit",true)] {
        let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap().to_owned();let key=login(&h,"root.admin").await;
        let before=stamps(&h,&id);let temp=h.stop().await;let path=temp.path().join("owner");let signal=temp.path().join("normal-phase");
        let mut child=Child(Command::new(binary()).args(["serve",path.to_str().unwrap(),ORIGIN,"127.0.0.1:0"])
            .env("MEGURI_TEST_KILL_PHASE",phase).env("MEGURI_TEST_SIGNAL_FILE",&signal).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
        let stdout=child.0.stdout.take().unwrap();
        let line=within(tokio::task::spawn_blocking(move || {let mut line=String::new();std::io::BufReader::new(stdout).read_line(&mut line).unwrap();line})).await.unwrap();
        let base=format!("http://{}",line.trim().strip_prefix("listening ").unwrap());
        let client=reqwest::Client::builder().no_proxy().build().unwrap();let header=json!({"v":11,"k":key,"s":"crash","c":"synthetic"}).to_string();
        for (method,body) in [
            ("start",json!({"minUsn":0,"lnewer":false,"graves":null})),
            ("applyChanges",changes(json!(["crash-tag"]))),
            ("chunk",json!({"_pad":null})),
            ("applyChunk",json!({"chunk":{"done":true,"revlog":[review(1,0,2)]}})),
            ("sanityCheck2",json!({"client":[[0,0,0],0,0,1,0,0,0,0]})),
        ] {
            decoded(within(client.post(format!("{base}/sync/{method}")).header("anki-sync",&header)
                .body(zstd::stream::encode_all(serde_json::to_vec(&body).unwrap().as_slice(),0).unwrap()).send()).await.unwrap()).await;
        }
        let finishing=tokio::spawn(async move {client.post(format!("{base}/sync/finish")).header("anki-sync",header).body(zstd::stream::encode_all(b"{}".as_slice(),0).unwrap()).send().await});
        wait_file(&signal,&mut child).await;
        child.0.kill().unwrap();child.0.wait().unwrap();assert!(within(finishing).await.unwrap().is_err());drop(child);
        let h=Harness::reopen(temp).await;
        assert_eq!(count(&h,&id,"tags"),i64::from(committed));assert_eq!(count(&h,&id,"revlog"),i64::from(committed));
        if committed {let after=stamps(&h,&id);assert_eq!(after.1,before.1+1);assert_eq!(after.0,after.2);} else {assert_eq!(stamps(&h,&id),before);}
        assert_eq!(native(&h,"finish",&key,"crash",json!({})).await.status(),409);
        start(&h,&key,"retry",0).await;call(&h,"applyChanges",&key,"retry",changes(json!(["crash-tag"]))).await;receive(&h,&key,"retry").await;
        call(&h,"applyChunk",&key,"retry",json!({"chunk":{"done":true,"revlog":[review(1,0,2)]}})).await;
        ready(&h,&key,"retry",json!([[0,0,0],0,0,1,0,0,0,0])).await;finish(&h,&key,"retry").await;
        assert_eq!(count(&h,&id,"revlog"),1);assert_eq!(count(&h,&id,"tags"),1);
        h.stop().await;
    }
}

#[tokio::test]
async fn session_unobserved_mixed_config_forms_refuse_without_partial_commit() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    let before=stamps(&h,id);
    // Paired omitted/null and paired replacement are observed. These mixed
    // optional forms remain a local refusal policy, not a Desktop error claim.
    // Retain the three original conf-only scenarios and cover the reverse mix.
    for optional in [json!({"conf":null}),json!({"conf":{}}),json!({"conf":{"authored":true}}),
        json!({"crt":123}),json!({"conf":null,"crt":123}),json!({"conf":{},"crt":null})] {
        start(&h,&key,"config",0).await;
        call(&h,"applyGraves",&key,"config",json!({"chunk":{"cards":[99],"notes":[],"decks":[]}})).await;
        let mut request=changes(json!(["must-roll-back"]));
        for (name,value) in optional.as_object().unwrap() {request["changes"][name]=value.clone();}
        let response=native(&h,"applyChanges",&key,"config",request).await;
        assert_eq!(response.status(),400);assert_eq!(response.text().await.unwrap(),"unsupported_normal_case");
        assert_eq!(count(&h,id,"tags"),0);assert_eq!(count(&h,id,"config"),0);
        assert_eq!(count(&h,id,"graves"),0);assert_eq!(stamps(&h,id),before);
        assert_eq!(native(&h,"finish",&key,"config",json!({})).await.status(),409);
    }
    // The former same-round card resurrection scenario is retained and
    // strengthened in matrix_graves_then_same_id_occurrences_restore_rows.
    h.stop().await;
}

#[tokio::test]
async fn session_newer_compatible_deck_edit_preserves_stored_unknown_protobuf_bytes() {
    use std::os::unix::fs::OpenOptionsExt;
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    // Independent schema-18 fixture: storage/schema-18#S2, model/row-records#R7.
    // P2 keys are (number << 3) | wire type. P6.4 common fields 1/2
    // are true; unknown field 8 is varint 150 (40 96 01).
    let common=b"\x08\x01\x10\x01\x40\x96\x01";
    // P6.5 normal member: config 1, description "authored deck", then
    // unknown field 11 (wire 2), length 3, payload [0, 255, 127]. The
    // normal payload is 22 bytes; both unknown fields are valid under P2.
    let kind=b"\x0a\x16\x08\x01\x22\x0dauthored deck\x5a\x03\x00\xff\x7f";
    let path=h.temp.path().join("opaque-deck.anki2");
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
    meguri_sync_server::collection::schema::create_blank(&path,123,456).unwrap();
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();
    c.execute("INSERT INTO decks VALUES(20,'Opaque before',20,0,?1,?2)",rusqlite::params![common.as_slice(),kind.as_slice()]).unwrap();
    meguri_sync_server::collection::schema::checkpoint(&c).unwrap();drop(c);
    let upload=std::fs::read(&path).unwrap();
    assert_eq!(decoded(request(&h,"upload",&key,"opaque-upload",&upload).await).await,b"OK");
    let c=db(&h,id);
    let stored=c.query_row("SELECT common,kind FROM decks WHERE id=20",[],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?))).unwrap();
    assert_eq!(stored,(common.to_vec(),kind.to_vec()));drop(c);

    // D7 observes the prior response and acceptance of a newer deck name.
    // C6/KTD6 and P4/P5 independently require stored-byte preservation;
    // S3/S8 do not give unknown binary fields a schema-11 JSON channel.
    start(&h,&key,"opaque-edit",0).await;
    let response=native(&h,"applyChanges",&key,"opaque-edit",json!({"changes":{"models":[],"decks":[[deck(30,"Opaque after")],[]],"tags":[]}})).await;
    let status=response.status();
    if status!=200 {
        let body=response.text().await.unwrap();
        panic!("compatible deck edit requires HTTP 200, got {status}: {body}");
    }
    let response:Value=serde_json::from_slice(&decoded(response).await).unwrap();
    assert_eq!(response["decks"][0][0]["name"],"Opaque before");
    receive(&h,&key,"opaque-edit").await;
    call(&h,"applyChunk",&key,"opaque-edit",json!({"chunk":{"done":true}})).await;
    ready(&h,&key,"opaque-edit",json!([[0,0,0],0,0,0,0,0,1,0])).await;
    finish(&h,&key,"opaque-edit").await;

    // A separate read-only connection resolves the actual published database.
    // Exact blob equality includes the independent unknown values above;
    // the known edit is a row-column change, so P5 keeps both original blobs.
    let c=db(&h,id);
    let stored=c.query_row("SELECT name,mtime_secs,common,kind FROM decks WHERE id=20",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?))).unwrap();
    assert_eq!(stored,("Opaque after".into(),30,common.to_vec(),kind.to_vec()));drop(c);
    h.stop().await;
}

// Small independently encoded P2 length-delimited fields for synthetic fixtures.
fn preservation_message(number:u8,payload:&[u8])->Vec<u8> {
    assert!(number>0 && number<16 && payload.len()<128);
    let mut out=vec![(number<<3)|2,payload.len() as u8];out.extend_from_slice(payload);out
}
async fn preservation_upload(h:&Harness,key:&str,name:&str,seed:impl FnOnce(&Connection)) {
    use std::os::unix::fs::OpenOptionsExt;
    let path=h.temp.path().join(name);
    std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
    meguri_sync_server::collection::schema::create_blank(&path,123,456).unwrap();
    let c=meguri_sync_server::collection::schema::open(&path,true).unwrap();seed(&c);
    meguri_sync_server::collection::schema::checkpoint(&c).unwrap();drop(c);
    assert_eq!(decoded(request(h,"upload",key,"preservation-upload",&std::fs::read(path).unwrap()).await).await,b"OK");
}
async fn preservation_finish(h:&Harness,key:&str,s:&str,models:u32,decks:u32) {
    receive(h,key,s).await;call(h,"applyChunk",key,s,json!({"chunk":{"done":true}})).await;
    ready(h,key,s,json!([[0,0,0],0,0,0,0,models,decks,0])).await;finish(h,key,s).await;
}

#[tokio::test]
async fn session_known_deck_blob_edits_preserve_unknowns_and_unedited_lossy_values() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    // P6.4 common unknown field 8; both optional day limits carry unknown
    // field 3, the normal message unknown 11, and its envelope unknown 3.
    let common=b"\x08\x01\x10\x01\x40\x96\x01";
    let review=b"\x08\x05\x10\x02\x1a\x03\x00\xff\x7f";
    let new=b"\x08\x07\x10\x02\x18\x96\x01";
    let mut normal=b"\x08\x01\x22\x0dauthored deck".to_vec();
    normal.extend(preservation_message(8,review));normal.extend(preservation_message(9,new));
    // P6.5.1 optional float .857 has a lossy schema-11 integer percentage.
    normal.push(0x55);normal.extend_from_slice(&0.857f32.to_le_bytes());
    normal.extend_from_slice(b"\x5a\x03\x00\xff\x7f");
    let mut kind=preservation_message(1,&normal);kind.extend_from_slice(b"\x18\x96\x01");
    preservation_upload(&h,&key,"known-deck.anki2",|c| {
        c.execute("INSERT INTO decks VALUES(20,'Known before',20,0,?1,?2)",rusqlite::params![common.as_slice(),&kind]).unwrap();
    }).await;
    let mut edited=deck(30,"Known after");
    edited["collapsed"]=json!(false);edited["desc"]=json!("edited description");
    edited["reviewLimitToday"]=json!({"limit":9,"today":2});edited["newLimitToday"]=json!({"limit":11,"today":3});
    edited["desiredRetention"]=json!(85);
    start(&h,&key,"known-deck",0).await;
    let prior=call(&h,"applyChanges",&key,"known-deck",json!({"changes":{"models":[],"decks":[[edited.clone()],[]],"tags":[]}})).await;
    assert_eq!(prior["decks"][0][0]["desc"],"authored deck");
    preservation_finish(&h,&key,"known-deck",0,1).await;
    let mut expected_normal=b"\x08\x01".to_vec();expected_normal.extend(preservation_message(4,b"edited description"));
    expected_normal.extend(preservation_message(8,b"\x08\x09\x10\x02\x1a\x03\x00\xff\x7f"));
    expected_normal.extend(preservation_message(9,b"\x08\x0b\x10\x03\x18\x96\x01"));
    expected_normal.push(0x55);expected_normal.extend_from_slice(&0.857f32.to_le_bytes());expected_normal.extend_from_slice(b"\x5a\x03\x00\xff\x7f");
    let mut expected_kind=preservation_message(1,&expected_normal);expected_kind.extend_from_slice(b"\x18\x96\x01");
    let c=db(&h,id);
    let stored=c.query_row("SELECT name,common,kind FROM decks WHERE id=20",[],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,Vec<u8>>(2)?))).unwrap();
    assert_eq!(stored,("Known after".into(),b"\x10\x01\x40\x96\x01".to_vec(),expected_kind.clone()));drop(c);

    // S3 adds a JSON member to common.other (field 255). Its setter is not
    // public; this exercises the adapter's codec-only reconstruction seam.
    edited["mod"]=json!(40);edited["zz_authored"]=json!(7);
    start(&h,&key,"known-other",0).await;
    call(&h,"applyChanges",&key,"known-other",json!({"changes":{"models":[],"decks":[[edited],[]],"tags":[]}})).await;
    preservation_finish(&h,&key,"known-other",0,1).await;
    let other=br#"{"zz_authored":7}"#;
    let mut expected_common=b"\x10\x01\x40\x96\x01\xfa\x0f".to_vec();
    expected_common.push(u8::try_from(other.len()).unwrap());expected_common.extend_from_slice(other);
    let c=db(&h,id);
    let stored=c.query_row("SELECT common,kind FROM decks WHERE id=20",[],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?))).unwrap();
    assert_eq!(stored,(expected_common,expected_kind));drop(c);h.stop().await;
}

#[tokio::test]
async fn session_filtered_deck_known_edits_preserve_envelope_member_and_search_unknowns() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    // P6.5.3/.4: an unknown search-term field 4 and filtered field 8.
    let mut term=preservation_message(1,b"authored");term.extend_from_slice(b"\x10\x0a\x22\x03\x00\xff\x7f");
    let mut filtered=b"\x08\x01".to_vec();filtered.extend(preservation_message(2,&term));filtered.extend_from_slice(b"\x28\x05\x40\x96\x01");
    let mut kind=preservation_message(2,&filtered);kind.extend_from_slice(b"\x18\x96\x01");
    // Intentionally noncanonical common order and an explicit false. No common
    // edit means exact original bytes, even while kind is canonically rewritten.
    let common=b"\x40\x96\x01\x10\x01\x08\x00";
    preservation_upload(&h,&key,"filtered-deck.anki2",|c| {
        c.execute("INSERT INTO decks VALUES(20,'Filtered authored',20,0,?1,?2)",rusqlite::params![common.as_slice(),&kind]).unwrap();
    }).await;
    let edited=json!({"id":20,"name":"Filtered authored","mod":30,"usn":0,"dyn":1,"collapsed":false,"browserCollapsed":true,
        "newToday":[0,0],"revToday":[0,0],"lrnToday":[0,0],"timeToday":[0,0],"resched":true,
        "terms":[["edited",12,1]],"previewHardSecs":8});
    start(&h,&key,"filtered-edit",0).await;
    call(&h,"applyChanges",&key,"filtered-edit",json!({"changes":{"models":[],"decks":[[edited],[]],"tags":[]}})).await;
    preservation_finish(&h,&key,"filtered-edit",0,1).await;
    let mut term=preservation_message(1,b"edited");term.extend_from_slice(b"\x10\x0c\x18\x01\x22\x03\x00\xff\x7f");
    let mut filtered=b"\x08\x01".to_vec();filtered.extend(preservation_message(2,&term));filtered.extend_from_slice(b"\x28\x08\x40\x96\x01");
    let mut expected=preservation_message(2,&filtered);expected.extend_from_slice(b"\x18\x96\x01");
    let c=db(&h,id);
    let stored=c.query_row("SELECT common,kind FROM decks WHERE id=20",[],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?))).unwrap();
    assert_eq!(stored,(common.to_vec(),expected));drop(c);h.stop().await;
}

#[tokio::test]
async fn session_css_edit_preserves_unknown_notetype_requirement_field_and_template_bytes() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    // P6.1 unknown 11, and unknown 4 inside one card requirement. Field and
    // template configs have their own unknown 12 and 9 respectively.
    let config=b"\x1a\x03old\x42\x08\x10\x01\x1a\x01\x00\x20\x96\x01\x58\x96\x01";
    let field=b"\x60\x96\x01\x20\x0c\x08\x00";
    let template=b"\x48\x96\x01\x0a\x0a{{Prompt}}";
    preservation_upload(&h,&key,"css-preservation.anki2",|c| {
        c.execute("INSERT INTO notetypes VALUES(10,'Authored',10,0,?1)",[config.as_slice()]).unwrap();
        c.execute("INSERT INTO fields VALUES(10,0,'Prompt',?1)",[field.as_slice()]).unwrap();
        c.execute("INSERT INTO templates VALUES(10,0,'Authored template',0,0,?1)",[template.as_slice()]).unwrap();
    }).await;
    let mut edited=model();edited["mod"]=json!(30);edited["css"]=json!(".authored { color: blue; }");
    edited["tmpls"][0]["afmt"]=json!("");edited["req"]=json!([[0,"any",[0]]]);
    start(&h,&key,"css-preserve",0).await;
    let prior=call(&h,"applyChanges",&key,"css-preserve",json!({"changes":{"models":[edited.clone()],"decks":[[],[]],"tags":[]}})).await;
    assert_eq!(prior["models"][0]["css"],"old");
    preservation_finish(&h,&key,"css-preserve",1,0).await;
    let mut expected=preservation_message(3,b".authored { color: blue; }");expected.extend_from_slice(b"\x42\x08\x10\x01\x1a\x01\x00\x20\x96\x01\x58\x96\x01");
    let c=db(&h,id);
    assert_eq!(c.query_row("SELECT config FROM notetypes WHERE id=10",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),expected);
    assert_eq!(c.query_row("SELECT config FROM fields WHERE ntid=10",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),field);
    assert_eq!(c.query_row("SELECT config FROM templates WHERE ntid=10",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),template);drop(c);
    // The independent schema-preservation policy still rejects count changes
    // and rolls back the round; this makes no vendor error/timing claim.
    for shape_key in ["flds","tmpls"] {
        let mut changed=edited.clone();changed["mod"]=json!(40);changed[shape_key]=json!([]);
        start(&h,&key,"shape-refused",0).await;
        assert_eq!(native(&h,"applyChanges",&key,"shape-refused",json!({"changes":{"models":[changed],"decks":[[],[]],"tags":["must-not-commit"]}})).await.status(),400);
        assert_eq!(native(&h,"finish",&key,"shape-refused",json!({})).await.status(),409);
        assert_eq!(count(&h,id,"tags"),0);
        assert_eq!(db(&h,id).query_row("SELECT config FROM notetypes WHERE id=10",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),expected);
    }
    h.stop().await;
}

// Finite Desktop matrix authority: parent-approved immutable neutral JSON
// conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-normal-matrix.json, SHA-256
// 8a4e17346939516eda8b5b650203444693fb32e2173f4d6741242b4089da04eb.
// Its historical unreviewed header is superseded by parent approval. These
// independent fixtures do not execute/import that input or claim SDK traffic.
fn matrix_rows(h:&Harness,id:&str,sql:&str)->Value {
    use rusqlite::types::Value as Sql;
    let c=db(h,id);let mut statement=c.prepare(sql).unwrap();let columns=statement.column_count();
    let rows=statement.query_map([],|r| {
        (0..columns).map(|i|Ok(match r.get::<_,Sql>(i)? {
            Sql::Null=>Value::Null,Sql::Integer(n)=>json!(n),Sql::Real(n)=>json!(n),
            Sql::Text(s)=>json!(s),Sql::Blob(b)=>json!(b),
        })).collect::<rusqlite::Result<Vec<Value>>>()
    }).unwrap().map(|r|r.unwrap()).collect::<Vec<_>>();json!(rows)
}
fn matrix_card(id:i64,mtime:i64,usn:i32,due:i64,data:&str)->Value {
    json!([id,id,1,0,mtime,usn,0,0,due,0,0,0,0,0,0,0,0,data])
}
fn matrix_model()->Value {
    let mut m=model();m["name"]=json!("Cedar stored model");m["mod"]=json!(2000000000);m["css"]=json!("");
    m["flds"]=json!([
        {"name":"Prompt","ord":0,"sticky":false,"rtl":false,"font":"Arial","size":20},
        {"name":"Reply","ord":1,"sticky":false,"rtl":false,"font":"Arial","size":20}]);
    m["tmpls"][0]["afmt"]=json!("{{Reply}}");m
}
fn matrix_preset(mtime:i64,usn:i32,name:&str,limit:u32)->Value {
    // model/schema11-json#S10: full valid new part; rev/lapse take defaults.
    json!({"id":30,"name":name,"mod":mtime,"usn":usn,"maxTaken":60,"autoplay":true,"timer":0,"replayq":true,
        "new":{"bury":false,"delays":[1.0,10.0],"initialFactor":2500,"ints":[1,4,0],"order":1,"perDay":limit},
        "rev":{},"lapse":{}})
}
fn matrix_seed_model(c:&Connection,id:i64,name:&str,usn:i32) {
    c.execute("INSERT INTO notetypes VALUES(?1,?2,2000000000,?3,x'')",rusqlite::params![id,name,usn]).unwrap();
    // Independent P6.2/P6.3: Arial, 20pt; question/answer template strings.
    c.execute("INSERT INTO fields VALUES(?1,0,'Prompt',x'1a05417269616c2014')",[id]).unwrap();
    c.execute("INSERT INTO fields VALUES(?1,1,'Reply',x'1a05417269616c2014')",[id]).unwrap();
    let mut template=preservation_message(1,b"{{Prompt}}");template.extend(preservation_message(2,b"{{Reply}}"));
    c.execute("INSERT INTO templates VALUES(?1,0,'Authored template',0,0,?2)",rusqlite::params![id,template]).unwrap();
}
// Independent finite P2 reader for the known scalar/string fields asserted
// below. It inspects persisted blobs without using production model codecs.
fn matrix_proto_field(bytes:&[u8],number:u64)->Vec<u8> {
    fn varint(bytes:&[u8],at:&mut usize)->u64 {
        let mut out=0;
        for shift in (0..70).step_by(7) {
            let byte=bytes[*at];*at+=1;out|=u64::from(byte&127)<<shift;
            if byte&128==0 {return out;}
        }
        panic!("invalid synthetic protobuf");
    }
    let mut at=0;let mut found=vec![];
    while at<bytes.len() {
        let key=varint(bytes,&mut at);
        let value=match key&7 {
            0=>{let begin=at;varint(bytes,&mut at);bytes[begin..at].to_vec()},
            1=>{let begin=at;at+=8;bytes[begin..at].to_vec()},
            2=>{let len=usize::try_from(varint(bytes,&mut at)).unwrap();let begin=at;at+=len;bytes[begin..at].to_vec()},
            5=>{let begin=at;at+=4;bytes[begin..at].to_vec()},
            _=>panic!("unexpected wire type in synthetic fixture"),
        };
        if key>>3==number {found=value;}
    }
    found
}
async fn matrix_end(h:&Harness,key:&str,s:&str,counts:Value) {
    receive(h,key,s).await;call(h,"applyChunk",key,s,json!({"chunk":{"done":true}})).await;
    ready(h,key,s,counts).await;finish(h,key,s).await;
}
async fn matrix_start(h:&Harness,key:&str,s:&str,min:i32,newer:bool) {
    call(h,"start",key,s,json!({"minUsn":min,"lnewer":newer,"graves":null})).await;
}
fn matrix_now()->i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()).unwrap()
}

#[tokio::test]
async fn matrix_cards_pending_nonpending_and_accepted_data_normalization() {
    // Authority: card_samples[0..4], including the directly measured USN-0 seed.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"matrix-cards.anki2",|c| {
        c.execute("UPDATE col SET usn=4",[]).unwrap();
        for (card_id,usn) in [(1,1),(2,1),(3,1),(4,0)] {
            c.execute("INSERT INTO cards VALUES(?1,?1,1,0,2000000000,?2,0,0,17,0,0,0,0,0,0,0,0,'{}')",rusqlite::params![card_id,usn]).unwrap();
        }
    }).await;
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM cards WHERE id=4"),json!([[4,4,1,0,2000000000i64,0,0,0,17,0,0,0,0,0,0,0,0,"{}"]]));
    for (cid,in_mod,in_usn,out_mod,out_usn,out_due) in [
        (1,1999999999i64,4,2000000000i64,1,17),(2,2000000000,7,2000000000,1,17),
        (3,2000000001,10,2000000001,10,23),(4,1999999999,14,1999999999,14,23)] {
        start(&h,&key,"card-matrix",1).await;call(&h,"applyChanges",&key,"card-matrix",changes(json!([]))).await;
        receive(&h,&key,"card-matrix").await;
        call(&h,"applyChunk",&key,"card-matrix",json!({"chunk":{"done":true,"cards":[matrix_card(cid,in_mod,in_usn,23,"{}")]}})).await;
        ready(&h,&key,"card-matrix",json!([[0,0,0],4,0,0,0,0,0,0])).await;finish(&h,&key,"card-matrix").await;
        assert_eq!(matrix_rows(&h,id,&format!("SELECT * FROM cards WHERE id={cid}")),json!([[cid,cid,1,0,out_mod,out_usn,0,0,out_due,0,0,0,0,0,0,0,0,"{}"]]));
    }
    for (mtime,usn,due,input) in [(2000000000i64,16,5,r#"{"pos":5,"zz":7}"#),(2000000001,18,29,r#"{"pos":5}"#)] {
        start(&h,&key,"card-data",1).await;call(&h,"applyChanges",&key,"card-data",changes(json!([]))).await;receive(&h,&key,"card-data").await;
        call(&h,"applyChunk",&key,"card-data",json!({"chunk":{"done":true,"cards":[matrix_card(5,mtime,usn,due,input)]}})).await;
        ready(&h,&key,"card-data",json!([[0,0,0],5,0,0,0,0,0,0])).await;finish(&h,&key,"card-data").await;
        assert_eq!(matrix_rows(&h,id,"SELECT * FROM cards WHERE id=5"),json!([[5,5,1,0,mtime,usn,0,0,due,0,0,0,0,0,0,0,0,r#"{"pos":5}"#]]));
    }
    // The first request drops zz. No preexisting stored-zz claim is made.
    h.stop().await;
}

#[tokio::test]
async fn matrix_same_shape_notetype_and_preset_mods_respond_before_merge() {
    // Authority: object_samples.model/preset older/equal/newer, same structure.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"matrix-objects.anki2",|c| {
        matrix_seed_model(c,10,"Cedar stored model",0);
        c.execute("INSERT INTO deck_config VALUES(30,'Cedar stored preset',2000000000,0,x'4814')",[]).unwrap();
    }).await;
    for (mtime,usn,mname,pname,limit,prior_m,prior_p,prior_mod,prior_usn,prior_limit,out_mod,out_usn,out_limit) in [
        (1999999999i64,4,"Cedar older model","Cedar older preset",30,"Cedar stored model","Cedar stored preset",2000000000i64,0,20,2000000000i64,0,20),
        (2000000000,7,"Cedar equal model","Cedar equal preset",31,"Cedar stored model","Cedar stored preset",2000000000,0,20,2000000000,7,31),
        (2000000001,10,"Cedar newer model","Cedar newer preset",32,"Cedar equal model","Cedar equal preset",2000000000,7,31,2000000001,10,32)] {
        let mut m=matrix_model();m["name"]=json!(mname);m["mod"]=json!(mtime);m["usn"]=json!(usn);m["css"]=json!(".card { color: #456789; }");
        start(&h,&key,"objects",0).await;
        let response=call(&h,"applyChanges",&key,"objects",json!({"changes":{"models":[m],"decks":[[],[matrix_preset(mtime,usn,pname,limit)]],"tags":[]}})).await;
        assert_eq!(response["models"].as_array().unwrap().len(),1);assert_eq!(response["decks"][1].as_array().unwrap().len(),1);
        let old=&response["models"][0];let preset=&response["decks"][1][0];
        assert_eq!(json!([old["id"],old["name"],old["mod"],old["usn"],old["css"]]),json!([10,prior_m,prior_mod,prior_usn,if prior_usn==0 {""} else {".card { color: #456789; }"}]));
        assert_eq!(json!([preset["id"],preset["name"],preset["mod"],preset["usn"],preset["new"]["perDay"]]),json!([30,prior_p,prior_mod,prior_usn,prior_limit]));
        matrix_end(&h,&key,"objects",json!([[0,0,0],0,0,0,0,1,0,1])).await;
        let accepted=usn!=4;
        assert_eq!(matrix_rows(&h,id,"SELECT id,name,mtime_secs,usn FROM notetypes"),json!([[10,if accepted {mname} else {"Cedar stored model"},out_mod,out_usn]]));
        assert_eq!(matrix_rows(&h,id,"SELECT id,name,mtime_secs,usn FROM deck_config"),json!([[30,if accepted {pname} else {"Cedar stored preset"},out_mod,out_usn]]));
        let c=db(&h,id);
        let css:Vec<u8>=c.query_row("SELECT config FROM notetypes WHERE id=10",[],|r|r.get(0)).unwrap();
        assert_eq!(matrix_proto_field(&css,3),if accepted {b".card { color: #456789; }".as_slice()} else {b"".as_slice()});
        let config:Vec<u8>=c.query_row("SELECT config FROM deck_config WHERE id=30",[],|r|r.get(0)).unwrap();
        assert_eq!(matrix_proto_field(&config,9),vec![out_limit]);drop(c);
        assert_eq!(matrix_rows(&h,id,"SELECT ntid,ord,name FROM fields ORDER BY ord"),json!([[10,0,"Prompt"],[10,1,"Reply"]]));
        assert_eq!(matrix_rows(&h,id,"SELECT ntid,ord,name FROM templates"),json!([[10,0,"Authored template"]]));
    }
    h.stop().await;
}

#[tokio::test]
async fn matrix_ascii_repeated_plus_names_and_existing_tag_state() {
    // Authority: ascii_names, selected ASCII only; merge-time stamps are clocks.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"matrix-names.anki2",|c| {
        c.execute("UPDATE col SET usn=5",[]).unwrap();
        for (did,ntid,name,usn) in [(810001,820001,"Alpha",3),(810002,820002,"alpha+",4)] {
            c.execute("INSERT INTO decks VALUES(?1,?2,2000000000,?3,x'08011001',x'0a020801')",rusqlite::params![did,name,usn]).unwrap();
            matrix_seed_model(c,ntid,name,usn);
        }
        c.execute("INSERT INTO tags VALUES('CedarTag',0,0,NULL)",[]).unwrap();
    }).await;
    assert_eq!(stamps(&h,id).1,5);
    let mut m=matrix_model();m["id"]=json!(820003);m["name"]=json!("alpha");m["usn"]=json!(5);
    let mut d=deck(2000000000,"alpha");d["id"]=json!(810003);d["usn"]=json!(5);
    let lower=matrix_now();start(&h,&key,"names",0).await;
    let response=call(&h,"applyChanges",&key,"names",json!({"changes":{"models":[m],"decks":[[d],[]],"tags":["cedartag"]}})).await;
    assert_eq!(response["models"].as_array().unwrap().len(),2);assert_eq!(response["decks"][0].as_array().unwrap().len(),2);
    assert_eq!(response["tags"],json!(["CedarTag"]));
    matrix_end(&h,&key,"names",json!([[0,0,0],0,0,0,0,3,3,0])).await;let upper=matrix_now();
    for table in ["decks","notetypes"] {
        let first=if table=="decks" {810001} else {820001};
        assert_eq!(matrix_rows(&h,id,&format!("SELECT id,name,mtime_secs,usn FROM {table} WHERE id<{} ORDER BY id",first+2)),json!([[first,"Alpha",2000000000i64,3],[first+1,"alpha+",2000000000i64,4]]));
        assert_eq!(matrix_rows(&h,id,&format!("SELECT id,name,usn FROM {table} WHERE id={}",first+2)),json!([[first+2,"alpha++",5]]));
        let stamp:i64=db(&h,id).query_row(&format!("SELECT mtime_secs FROM {table} WHERE id={}",first+2),[],|r|r.get(0)).unwrap();
        assert!((lower..=upper).contains(&stamp),"rename must use merge-time seconds");
    }
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM tags"),json!([["CedarTag",0,0,null]]));
    h.stop().await;
}

fn matrix_seed_config(c:&Connection) {
    c.execute("UPDATE col SET crt=1794715200,usn=4",[]).unwrap();
    c.execute("INSERT INTO config VALUES('cedar_keep',3,1794786162,x'37')",[]).unwrap();
    c.execute("INSERT INTO config VALUES('cedar_old',3,1794786162,?1)",[br#"{"payload":"old"}"#.as_slice()]).unwrap();
}
fn matrix_config(h:&Harness,id:&str)->Value {
    matrix_rows(h,id,"SELECT key,usn,mtime_secs,CAST(val AS TEXT),typeof(val) FROM config ORDER BY key")
}

#[tokio::test]
async fn matrix_paired_omitted_and_null_config_crt_preserve_values_and_stamps() {
    // Authority: configuration.config-optional.samples, including raw row stamps.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"matrix-optional.anki2",matrix_seed_config).await;
    let expected=json!([["cedar_keep",3,1794786162,"7","blob"],["cedar_old",3,1794786162,r#"{"payload":"old"}"#,"blob"]]);
    assert_eq!(matrix_config(&h,id),expected);
    for nulls in [false,true] {
        matrix_start(&h,&key,"optional",0,true).await;let mut request=changes(json!([]));
        if nulls {request["changes"]["conf"]=Value::Null;request["changes"]["crt"]=Value::Null;}
        let response=call(&h,"applyChanges",&key,"optional",request).await;
        assert!(response.get("conf").is_none());assert!(response.get("crt").is_none());
        matrix_end(&h,&key,"optional",json!([[0,0,0],0,0,0,0,0,0,0])).await;
        assert_eq!(matrix_config(&h,id),expected);
        assert_eq!(matrix_rows(&h,id,"SELECT crt FROM col"),json!([[1794715200]]));
    }
    h.stop().await;
}

#[tokio::test]
async fn matrix_config_full_replacement_outgoing_literal_zero_usn_keeps_mtime() {
    // Authority: configuration.config-replacement-outgoing raw_served and
    // native_server_final_rows. Sent row USN is literal 0, never col.usn.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"matrix-replacement.anki2",matrix_seed_config).await;
    assert_eq!(stamps(&h,id).1,4);let lower=matrix_now();
    matrix_start(&h,&key,"replace-config",0,true).await;
    let response=call(&h,"applyChanges",&key,"replace-config",json!({"changes":{"models":[],"decks":[[],[]],"tags":[],
        "conf":{"cedar_keep":9,"cedar_new":["cedar",4]},"crt":1794715201}})).await;
    assert!(response.get("conf").is_none());assert!(response.get("crt").is_none());
    matrix_end(&h,&key,"replace-config",json!([[0,0,0],0,0,0,0,0,0,0])).await;let upper=matrix_now();
    let mtime:i64=db(&h,id).query_row("SELECT mtime_secs FROM config WHERE key='cedar_keep'",[],|r|r.get(0)).unwrap();
    assert!((lower..=upper).contains(&mtime));
    assert_eq!(matrix_config(&h,id),json!([["cedar_keep",4,mtime,"9","blob"],["cedar_new",4,mtime,r#"["cedar",4]"#,"blob"]]));
    assert_eq!(matrix_rows(&h,id,"SELECT crt FROM col"),json!([[1794715201]]));
    start(&h,&key,"send-config",0).await;
    let response=call(&h,"applyChanges",&key,"send-config",changes(json!([]))).await;
    assert_eq!(response["conf"],json!({"cedar_keep":9,"cedar_new":["cedar",4]}));assert_eq!(response["crt"],1794715201);
    matrix_end(&h,&key,"send-config",json!([[0,0,0],0,0,0,0,0,0,0])).await;
    assert_eq!(matrix_config(&h,id),json!([["cedar_keep",0,mtime,"9","blob"],["cedar_new",0,mtime,r#"["cedar",4]"#,"blob"]]));
    assert_eq!(matrix_rows(&h,id,"SELECT crt FROM col"),json!([[1794715201]]));
    h.stop().await;
}

async fn matrix_grave_seed(h:&Harness,key:&str,name:&str) {
    preservation_upload(h,key,name,|c| {
        c.execute("UPDATE col SET usn=4",[]).unwrap();matrix_seed_model(c,10,"Cedar grave model",0);
        c.execute("INSERT INTO decks VALUES(830001,'Cedar grave deck',2000000000,0,x'08011001',x'0a020801')",[]).unwrap();
        c.execute("INSERT INTO cards VALUES(1,1,830001,0,1794786164,0,0,0,1,0,0,0,0,0,0,0,0,'{}')",[]).unwrap();
        c.execute("INSERT INTO cards VALUES(2,2,1,0,1794786164,0,0,0,2,0,0,0,0,0,0,0,0,'{}')",[]).unwrap();
        c.execute("INSERT INTO notes VALUES(1,'synthetic-guid-0',10,1794786164,0,'',?1,'Cedar prompt 0',3197265419,0,'')",["Cedar prompt 0\u{1f}Cedar reply 0"]).unwrap();
        c.execute("INSERT INTO notes VALUES(2,'synthetic-guid-1',10,1794786164,0,'',?1,'Cedar prompt 1',390943721,0,'')",["Cedar prompt 1\u{1f}Cedar reply 1"]).unwrap();
    }).await;
}

#[tokio::test]
async fn matrix_deck_grave_relocates_card_without_changing_mod_or_usn() {
    // Authority: graves.raw_deck_grave_only. This supersedes the old blanket
    // no-related-row-change wording: only did changes on the relocated card.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    matrix_grave_seed(&h,&key,"matrix-deck-grave.anki2").await;
    start(&h,&key,"deck-grave",0).await;
    call(&h,"applyGraves",&key,"deck-grave",json!({"chunk":{"cards":[],"notes":[],"decks":[830001]}})).await;
    call(&h,"applyChanges",&key,"deck-grave",changes(json!([]))).await;
    matrix_end(&h,&key,"deck-grave",json!([[0,0,0],2,2,0,0,1,0,0])).await;
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM cards ORDER BY id"),json!([
        [1,1,1,0,1794786164,0,0,0,1,0,0,0,0,0,0,0,0,"{}"],
        [2,2,1,0,1794786164,0,0,0,2,0,0,0,0,0,0,0,0,"{}"]]));
    assert_eq!(count(&h,id,"decks"),0);
    assert_eq!(matrix_rows(&h,id,"SELECT oid,type,usn FROM graves"),json!([[830001,2,4]]));
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM notes ORDER BY id"),json!([
        [1,"synthetic-guid-0",10,1794786164,0,"","Cedar prompt 0\u{1f}Cedar reply 0","Cedar prompt 0",3197265419u32,0,""],
        [2,"synthetic-guid-1",10,1794786164,0,"","Cedar prompt 1\u{1f}Cedar reply 1","Cedar prompt 1",390943721,0,""]]));
    h.stop().await;
}

#[tokio::test]
async fn matrix_graves_then_same_id_occurrences_restore_rows() {
    // Authority: graves.raw_same_round_occurrences, exact note derived fields;
    // old deck grave keeps USN 4 while new card/note graves receive round USN 7.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    // Preserve the original absent-card grave + card(1,0) scenario. The observed
    // acceptance replaces only its obsolete HTTP-400/rollback expectation.
    start(&h,&key,"grave",0).await;
    call(&h,"applyGraves",&key,"grave",json!({"chunk":{"cards":[1],"notes":[],"decks":[]}})).await;
    call(&h,"applyChanges",&key,"grave",changes(json!([]))).await;receive(&h,&key,"grave").await;
    call(&h,"applyChunk",&key,"grave",json!({"chunk":{"done":true,"cards":[card(1,0)]}})).await;
    assert_eq!(count(&h,id,"cards"),0);assert_eq!(count(&h,id,"graves"),0);
    ready(&h,&key,"grave",json!([[0,0,0],1,0,0,0,0,0,0])).await;finish(&h,&key,"grave").await;
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM cards"),json!([[1,1,1,0,20,0,0,0,1,0,0,0,0,0,0,0,0,"{}"]]));
    assert_eq!(matrix_rows(&h,id,"SELECT oid,type,usn FROM graves"),json!([[1,0,0]]));

    matrix_grave_seed(&h,&key,"matrix-restore.anki2").await;
    // Commit the earlier deck grave, then two harmless rounds to reach U=7.
    start(&h,&key,"earlier-deck",0).await;
    call(&h,"applyGraves",&key,"earlier-deck",json!({"chunk":{"cards":[],"notes":[],"decks":[830001]}})).await;
    call(&h,"applyChanges",&key,"earlier-deck",changes(json!([]))).await;
    matrix_end(&h,&key,"earlier-deck",json!([[0,0,0],2,2,0,0,1,0,0])).await;
    for s in ["advance-one","advance-two"] {
        start(&h,&key,s,0).await;call(&h,"applyChanges",&key,s,changes(json!([]))).await;
        matrix_end(&h,&key,s,json!([[0,0,0],2,2,0,0,1,0,0])).await;
    }
    assert_eq!(stamps(&h,id).1,7);
    start(&h,&key,"restore",0).await;
    call(&h,"applyGraves",&key,"restore",json!({"chunk":{"cards":[1],"notes":[2],"decks":[830001]}})).await;
    let mut d=deck(2000000002,"Cedar grave deck");d["id"]=json!(830001);d["usn"]=json!(7);d["desc"]=json!("");
    call(&h,"applyChanges",&key,"restore",json!({"changes":{"models":[],"decks":[[d],[]],"tags":[]}})).await;
    receive(&h,&key,"restore").await;
    let mut restored=matrix_card(1,2000000001,7,1,"{}");restored[2]=json!(830001);
    call(&h,"applyChunk",&key,"restore",json!({"chunk":{"done":true,"cards":[restored],
        "notes":[[2,"synthetic-guid-1",10,2000000001i64,7,"","Cedar prompt 1\u{1f}Cedar reply 1","","",0,""]]}})).await;
    ready(&h,&key,"restore",json!([[0,0,0],2,2,0,0,1,1,0])).await;finish(&h,&key,"restore").await;
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM cards ORDER BY id"),json!([
        [1,1,830001,0,2000000001i64,7,0,0,1,0,0,0,0,0,0,0,0,"{}"],
        [2,2,1,0,1794786164,0,0,0,2,0,0,0,0,0,0,0,0,"{}"]]));
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM notes ORDER BY id"),json!([
        [1,"synthetic-guid-0",10,1794786164,0,"","Cedar prompt 0\u{1f}Cedar reply 0","Cedar prompt 0",3197265419u32,0,""],
        [2,"synthetic-guid-1",10,2000000001i64,7,"","Cedar prompt 1\u{1f}Cedar reply 1","Cedar prompt 1",390943721,0,""]]));
    assert_eq!(matrix_rows(&h,id,"SELECT * FROM decks"),json!([[830001,"Cedar grave deck",2000000002i64,7,[8,1,16,1],[10,2,8,1]]]));
    assert_eq!(matrix_rows(&h,id,"SELECT oid,type,usn FROM graves ORDER BY type"),json!([[1,0,7],[2,1,7],[830001,2,4]]));
    h.stop().await;
}

#[tokio::test]
async fn session_ordered_cards_change_pending_state_and_one_review_stays_incremental() {
    // Card acceptance is the approved matrix; sequential occurrence evaluation
    // is normal-sync N6.5. This is an HTTP/storage proof, not an SDK lane.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"ordered-cards.anki2",|c| {
        c.execute("UPDATE col SET usn=2",[]).unwrap();
        for (cid,usn) in [(1,1),(2,0)] {
            c.execute("INSERT INTO cards VALUES(?1,?1,1,0,20,?2,0,0,17,0,0,0,0,0,0,0,0,'{}')",rusqlite::params![cid,usn]).unwrap();
        }
    }).await;
    start(&h,&key,"ordered-cards",1).await;call(&h,"applyChanges",&key,"ordered-cards",changes(json!([]))).await;
    let outgoing=receive(&h,&key,"ordered-cards").await;
    assert_eq!(outgoing[0]["cards"],json!([matrix_card(1,20,1,17,"{}")]));
    call(&h,"applyChunk",&key,"ordered-cards",json!({"chunk":{"done":false,
        "cards":[matrix_card(1,30,0,31,"{}"),matrix_card(1,10,0,11,"{}"),
            matrix_card(2,10,1,21,"{}"),matrix_card(2,5,0,99,"{}")],"revlog":[review(55,2,2),review(55,2,4)]}})).await;
    call(&h,"applyChunk",&key,"ordered-cards",json!({"chunk":{"done":true,
        "cards":[matrix_card(1,5,0,6,"{}"),matrix_card(2,10,0,99,"{}")],"revlog":[review(55,2,3)]}})).await;
    assert_eq!(matrix_rows(&h,id,"SELECT id,mod,usn,due FROM cards ORDER BY id"),json!([[1,20,1,17],[2,20,0,17]]));
    ready(&h,&key,"ordered-cards",json!([[0,0,0],2,0,1,0,0,0,0])).await;finish(&h,&key,"ordered-cards").await;
    assert_eq!(matrix_rows(&h,id,"SELECT id,mod,usn,due FROM cards ORDER BY id"),json!([[1,5,0,6],[2,10,1,21]]));
    assert_eq!(matrix_rows(&h,id,"SELECT id,ease FROM revlog"),json!([[55,2]]));

    start(&h,&key,"one-review",3).await;let response=call(&h,"applyChanges",&key,"one-review",changes(json!([]))).await;
    assert_eq!(response["models"],json!([]));assert_eq!(response["decks"],json!([[],[]]));
    let outgoing=receive(&h,&key,"one-review").await;
    assert_eq!(outgoing,vec![json!({"done":true,"cards":[],"notes":[],"revlog":[]})]);
    call(&h,"applyChunk",&key,"one-review",json!({"chunk":{"done":true,
        "cards":[matrix_card(1,6,3,29,r#"{"pos":5,"cd":"{\"user\": 1}","zz":7}"#)],"revlog":[review(56,3,3)]}})).await;
    ready(&h,&key,"one-review",json!([[0,0,0],2,0,2,0,0,0,0])).await;finish(&h,&key,"one-review").await;
    assert_eq!(matrix_rows(&h,id,"SELECT id,mod,usn,due,data FROM cards WHERE id=1"),json!([[1,6,3,29,r#"{"pos":5,"cd":"{\"user\": 1}"}"#]]));
    assert_eq!(matrix_rows(&h,id,"SELECT id,ease FROM revlog ORDER BY id"),json!([[55,2],[56,3]]));
    h.stop().await;
}

#[tokio::test]
async fn session_same_shape_known_edits_retain_unknowns_at_each_notetype_level_and_preset() {
    // Independent preservation policy from protobuf-records P2/P4/P5/P7;
    // compatible known-field editing uses public codecs, never MPL SQL logic.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    let config=b"\x1a\x03old\x42\x08\x10\x01\x1a\x01\x00\x20\x96\x01\x58\x96\x01\xfa\x0f\x02\xff\xff";
    let field=b"\x60\x96\x01\x20\x0c\x08\x00\xfa\x0f\x02\xff\xff";
    let template=b"\x48\x96\x01\x0a\x0a{{Prompt}}\xfa\x0f\x02\xff\xff";
    let preset=b"\x3a\x04\x00\x00\x80\x40\x48\x14\xfa\x0f\x02\xff\xff";
    preservation_upload(&h,&key,"known-edit-opaque.anki2",|c| {
        c.execute("INSERT INTO notetypes VALUES(10,'Authored',10,0,?1)",[config.as_slice()]).unwrap();
        c.execute("INSERT INTO fields VALUES(10,0,'Prompt',?1)",[field.as_slice()]).unwrap();
        c.execute("INSERT INTO templates VALUES(10,0,'Authored template',0,0,?1)",[template.as_slice()]).unwrap();
        c.execute("INSERT INTO deck_config VALUES(30,'Authored preset',10,0,?1)",[preset.as_slice()]).unwrap();
    }).await;
    let mut edited=model();edited["mod"]=json!(30);edited["name"]=json!("Renamed type");edited["css"]=json!("authored css");
    edited["req"]=json!([[0,"all",[0]]]);edited["flds"][0]["name"]=json!("Edited field");edited["flds"][0]["size"]=json!(18);
    edited["tmpls"][0]["qfmt"]=json!("<b>{{Prompt}}</b>");edited["tmpls"][0]["afmt"]=json!("");
    start(&h,&key,"known-edit-opaque",0).await;
    call(&h,"applyChanges",&key,"known-edit-opaque",json!({"changes":{"models":[edited],
        "decks":[[],[matrix_preset(30,0,"Edited preset",31)]],"tags":[]}})).await;
    matrix_end(&h,&key,"known-edit-opaque",json!([[0,0,0],0,0,0,0,1,0,1])).await;
    let c=db(&h,id);
    for (table,where_clause,unknown_number,unknown_value) in [
        ("notetypes","id=10",11,vec![0x96,0x01]),("fields","ntid=10",12,vec![0x96,0x01]),
        ("templates","ntid=10",9,vec![0x96,0x01]),("deck_config","id=30",7,vec![0,0,0x80,0x40]),
    ] {
        let bytes:Vec<u8>=c.query_row(&format!("SELECT config FROM {table} WHERE {where_clause}"),[],|r|r.get(0)).unwrap();
        assert_eq!(matrix_proto_field(&bytes,unknown_number),unknown_value);
        assert_eq!(matrix_proto_field(&bytes,255),vec![0xff,0xff]);
        match table {
            "notetypes"=> {
                assert_eq!(matrix_proto_field(&bytes,3),b"authored css");
                let requirement=matrix_proto_field(&bytes,8);
                assert_eq!(matrix_proto_field(&requirement,2),vec![2]);
                assert_eq!(matrix_proto_field(&requirement,4),vec![0x96,0x01]);
            },
            "fields"=>assert_eq!(matrix_proto_field(&bytes,4),vec![18]),
            "templates"=>assert_eq!(matrix_proto_field(&bytes,1),b"<b>{{Prompt}}</b>"),
            "deck_config"=>assert_eq!(matrix_proto_field(&bytes,9),vec![31]),
            _=>unreachable!(),
        }
    }
    drop(c);
    assert_eq!(matrix_rows(&h,id,"SELECT name FROM notetypes"),json!([["Renamed type"]]));
    assert_eq!(matrix_rows(&h,id,"SELECT name FROM fields"),json!([["Edited field"]]));
    h.stop().await;
}

#[tokio::test]
async fn session_config_compacts_exact_tokens_and_outgoing_stamp_reset_rolls_back_on_abort() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"config-exact.anki2",matrix_seed_config).await;
    matrix_start(&h,&key,"exact-config",0,true).await;
    let body=br#"{"changes":{"models":[],"decks":[[],[]],"tags":[],
        "conf":{"payload": { "text" : "a b\"c\\d", "n" : 9007199254740993,
            "list" : [ 1, { "x" : " y " } ] } },"crt":123}}"#;
    decoded(request(&h,"applyChanges",&key,"exact-config",body).await).await;
    matrix_end(&h,&key,"exact-config",json!([[0,0,0],0,0,0,0,0,0,0])).await;
    let before=matrix_config(&h,id);
    assert_eq!(before.as_array().unwrap().len(),1);
    assert_eq!(before[0][0],"payload");assert_eq!(before[0][1],4);assert_eq!(before[0][4],"blob");
    assert_eq!(before[0][3],r#"{"text":"a b\"c\\d","n":9007199254740993,"list":[1,{"x":" y "}]}"#);
    let stamps_before=stamps(&h,id);
    start(&h,&key,"send-abort",0).await;
    let response=call(&h,"applyChanges",&key,"send-abort",changes(json!([]))).await;
    assert_eq!(response["conf"]["payload"]["n"],json!(9007199254740993i64));assert_eq!(response["crt"],123);
    call(&h,"abort",&key,"send-abort",json!({})).await;
    assert_eq!(matrix_config(&h,id),before);assert_eq!(stamps(&h,id),stamps_before);
    assert_eq!(matrix_rows(&h,id,"SELECT crt FROM col"),json!([[123]]));
    h.stop().await;
}

#[tokio::test]
async fn session_preset_column_edit_keeps_noncanonical_blob_and_implicit_negative_zero() {
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    // P3/P5: explicit -0.0 in field 11 and out-of-order unknown field 7.
    let config=b"\x5d\x00\x00\x00\x80\x3a\x04\x00\x00\x80\x40\x48\x14\xfa\x0f\x02\xff\xff";
    preservation_upload(&h,&key,"preset-noop.anki2",|c| {
        c.execute("INSERT INTO deck_config VALUES(30,'Authored preset',10,0,?1)",[config.as_slice()]).unwrap();
    }).await;
    start(&h,&key,"read-preset",0).await;
    let prior=call(&h,"applyChanges",&key,"read-preset",changes(json!([]))).await;
    let mut incoming=prior["decks"][1][0].clone();
    call(&h,"abort",&key,"read-preset",json!({})).await;
    incoming["name"]=json!("Renamed preset");incoming["mod"]=json!(30);
    start(&h,&key,"rename-preset",0).await;
    call(&h,"applyChanges",&key,"rename-preset",json!({"changes":{"models":[],"decks":[[],[incoming]],"tags":[]}})).await;
    matrix_end(&h,&key,"rename-preset",json!([[0,0,0],0,0,0,0,0,0,1])).await;
    assert_eq!(matrix_rows(&h,id,"SELECT name,mtime_secs FROM deck_config"),json!([["Renamed preset",30]]));
    assert_eq!(db(&h,id).query_row("SELECT config FROM deck_config",[],|r|r.get::<_,Vec<u8>>(0)).unwrap(),config);
    h.stop().await;
}

#[tokio::test]
async fn session_pending_card_and_note_graves_remove_outgoing_tuples_and_persist() {
    // sync/server-collection C2/C3, normal-sync N4.3: a grave beats a row
    // already pending at start. No later incoming tuple restores either ID;
    // matrix_graves_then_same_id_occurrences_restore_rows covers that distinction.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    matrix_grave_seed(&h,&key,"pending-row-graves.anki2").await;
    // Both rows have usn=0, selected inclusively by minUsn=0.
    assert_eq!(matrix_rows(&h,id,"SELECT id,usn FROM cards ORDER BY id"),json!([[1,0],[2,0]]));
    assert_eq!(matrix_rows(&h,id,"SELECT id,usn FROM notes ORDER BY id"),json!([[1,0],[2,0]]));
    start(&h,&key,"pending-row-graves",0).await;
    call(&h,"applyGraves",&key,"pending-row-graves",json!({"chunk":{"cards":[1],"notes":[2],"decks":[]}})).await;
    call(&h,"applyChanges",&key,"pending-row-graves",changes(json!([]))).await;
    let outgoing=receive(&h,&key,"pending-row-graves").await;
    call(&h,"applyChunk",&key,"pending-row-graves",json!({"chunk":{"done":true}})).await;
    // Counts describe the live rows, independently of the collected HTTP tuples.
    ready(&h,&key,"pending-row-graves",json!([[0,0,0],1,1,0,0,1,1,0])).await;
    let stamp=finish(&h,&key,"pending-row-graves").await;
    assert_eq!(stamps(&h,id),(stamp,5,stamp));
    assert_eq!(matrix_rows(&h,id,"SELECT id,nid,did,mod,usn FROM cards ORDER BY id"),json!([[2,2,1,1794786164,0]]));
    assert_eq!(matrix_rows(&h,id,"SELECT id,mod,usn,flds FROM notes ORDER BY id"),json!([[1,1794786164,0,"Cedar prompt 0\u{1f}Cedar reply 0"]]));
    assert_eq!(matrix_rows(&h,id,"SELECT oid,type,usn FROM graves ORDER BY type,oid"),json!([[1,0,4],[2,1,4]]));

    let cards:Vec<_>=outgoing.iter().flat_map(|c|c["cards"].as_array().unwrap().iter().map(|r|r[0].as_i64().unwrap())).collect();
    let notes:Vec<_>=outgoing.iter().flat_map(|c|c["notes"].as_array().unwrap().iter().map(|r|r[0].as_i64().unwrap())).collect();
    assert_eq!(cards,vec![2],"outgoing HTTP chunks must omit card 1 deleted by its grave");
    assert_eq!(notes,vec![1],"outgoing HTTP chunks must omit note 2 deleted by its grave");
    h.stop().await;
}

#[tokio::test]
async fn session_pending_deck_grave_sends_relocated_card_with_original_stamps() {
    // sync/server-collection C2/C3 plus approved matrix deck relocation:
    // the selected outgoing card must reflect did=1 after the deck grave,
    // while mod/usn stay unchanged. This adds HTTP evidence to the final-row proof.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    matrix_grave_seed(&h,&key,"pending-deck-grave.anki2").await;
    assert_eq!(matrix_rows(&h,id,"SELECT id,did,mod,usn FROM cards WHERE id=1"),json!([[1,830001,1794786164,0]]));
    start(&h,&key,"pending-deck-grave",0).await;
    call(&h,"applyGraves",&key,"pending-deck-grave",json!({"chunk":{"cards":[],"notes":[],"decks":[830001]}})).await;
    call(&h,"applyChanges",&key,"pending-deck-grave",changes(json!([]))).await;
    let outgoing=receive(&h,&key,"pending-deck-grave").await;
    call(&h,"applyChunk",&key,"pending-deck-grave",json!({"chunk":{"done":true}})).await;
    ready(&h,&key,"pending-deck-grave",json!([[0,0,0],2,2,0,0,1,0,0])).await;
    let stamp=finish(&h,&key,"pending-deck-grave").await;
    assert_eq!(stamps(&h,id),(stamp,5,stamp));
    assert_eq!(matrix_rows(&h,id,"SELECT id,did,mod,usn FROM cards ORDER BY id"),json!([[1,1,1794786164,0],[2,1,1794786164,0]]));
    assert_eq!(count(&h,id,"decks"),0);
    assert_eq!(matrix_rows(&h,id,"SELECT oid,type,usn FROM graves"),json!([[830001,2,4]]));

    let cards:Vec<_>=outgoing.iter().flat_map(|c|c["cards"].as_array().unwrap().iter().cloned()).collect();
    assert_eq!(cards,vec![matrix_card(1,1794786164,0,1,"{}"),matrix_card(2,1794786164,0,2,"{}")],
        "outgoing HTTP card must use relocated did=1 and unchanged mod/usn, never the old deck ID");
    h.stop().await;
}

#[tokio::test]
async fn session_uploaded_real_review_intervals_truncate_on_wire_without_storage_rewrite() {
    // model/row-records R5, sync/wire-format W10: SQLite integer conversion
    // interprets these within-range REALs for output; stored REALs must survive.
    let h=Harness::new().await;let identity=h.setup().await;let id=identity["id"].as_str().unwrap();let key=login(&h,"root.admin").await;
    preservation_upload(&h,&key,"real-review-intervals.anki2",|c| {
        c.execute("UPDATE col SET usn=4",[]).unwrap();
        matrix_seed_model(c,10,"Authored review model",0);
        c.execute("INSERT INTO decks VALUES(20,'Authored review deck',2000000000,0,x'08011001',x'0a020801')",[]).unwrap();
        c.execute("INSERT INTO notes VALUES(1,'authored-review-guid',10,1794786164,0,'',?1,'Cedar prompt 0',3197265419,0,'')",["Cedar prompt 0\u{1f}Cedar reply 0"]).unwrap();
        c.execute("INSERT INTO cards VALUES(1,1,20,0,1794786164,0,0,0,1,0,0,0,0,0,0,0,0,'{}')",[]).unwrap();
        c.execute("INSERT INTO revlog VALUES(1794786164000,1,4,3,60.5,12.75,2500,1000,1)",[]).unwrap();
    }).await;
    let stored="SELECT id,cid,usn,ease,typeof(ivl),ivl,typeof(lastIvl),lastIvl,factor,time,type FROM revlog ORDER BY id";
    let expected=json!([[1794786164000i64,1,4,3,"real",60.5,"real",12.75,2500,1000,1]]);
    assert_eq!(matrix_rows(&h,id,stored),expected);
    start(&h,&key,"real-review-intervals",4).await;
    call(&h,"applyChanges",&key,"real-review-intervals",changes(json!([]))).await;
    // Intended pre-fix red: the actual outgoing chunk returns HTTP 503
    // storage_unavailable when these REAL columns are read directly as i32.
    let outgoing=receive(&h,&key,"real-review-intervals").await;
    let reviews:Vec<_>=outgoing.iter().flat_map(|c|c["revlog"].as_array().unwrap().iter().cloned()).collect();
    assert_eq!(reviews,vec![json!([1794786164000i64,1,4,3,60,12,2500,1000,1])]);
    call(&h,"applyChunk",&key,"real-review-intervals",json!({"chunk":{"done":true}})).await;
    ready(&h,&key,"real-review-intervals",json!([[0,0,0],1,1,1,0,1,1,0])).await;
    let stamp=finish(&h,&key,"real-review-intervals").await;
    assert_eq!(stamps(&h,id),(stamp,5,stamp));
    assert_eq!(matrix_rows(&h,id,stored),expected);
    h.stop().await;
}
