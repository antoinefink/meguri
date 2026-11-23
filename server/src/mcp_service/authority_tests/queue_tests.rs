// SPDX-License-Identifier: MIT
//! Real OAuth fixture and production HTTP transport/collection bridge, no outbound I/O.
use super::*;
use std::future::Future;

async fn within<T>(future: impl Future<Output=T>) -> T {
    tokio::time::timeout(Duration::from_secs(5),future).await.expect("bounded fixture wait")
}

#[tokio::test]
async fn ten_active_http_calls_queue_and_complete_reads_catalogs_and_independent_creates() {
    let f=Fixture::new(true).await;
    media_type(&f);
    let authority=f.service.mcp_authority(&f.token,true).await.unwrap();
    let gate=f.service.mcp_publication_barrier(&authority).await.unwrap();
    let (arrived,arrival)=tokio::sync::oneshot::channel();
    let (release,released)=std::sync::mpsc::sync_channel(1);
    let arrived=std::sync::Mutex::new(Some(arrived));
    let released=std::sync::Mutex::new(released);
    let bridge=Arc::new(McpService::new(f.service.clone()).with_test_probe(Arc::new(move |at| {
        if at==TestBoundary::Admitted {
            if let Some(arrived)=arrived.lock().unwrap().take() {
                arrived.send(()).unwrap();
                released.lock().unwrap().recv_timeout(Duration::from_secs(10)).expect("release first HTTP call");
            }
        }
    })));
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address=listener.local_addr().unwrap();
    let config=crate::Config::new(ORIGIN,address).unwrap();
    let (router,control)=transport::router(&config,bridge.clone(),bridge);
    let (stop,stopped)=tokio::sync::oneshot::channel();
    let server=tokio::spawn(async move {
        axum::serve(listener,router).with_graceful_shutdown(async {let _=stopped.await;}).await.unwrap();
    });
    let client=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(15)).build().unwrap();
    let call=|id,name,arguments| {
        let request=client.post(format!("http://{address}/mcp")).bearer_auth(&f.token)
            .header("mcp-protocol-version","2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}}));
        tokio::spawn(async move {
            let response=request.send().await.unwrap();
            assert_eq!(response.status(),200);
            response.json::<serde_json::Value>().await.unwrap()
        })
    };
    let first=call(0,"search_notes",json!({"query":"original"}));
    within(arrival).await.unwrap();
    let mut calls=vec![(0,first)];
    // Each signal comes from polling this account's real MCP mutex Pending.
    // The first owns both locks throughout: all ten HTTP calls are now active,
    // with nine queued (not merely spawned or already returning busy).
    for id in 1..10 {
        let (name,args)=match id%3 {
            0 => ("create_note",json!({"deck_id":"1","note_type_id":"10","fields":{"Front":format!("generated burst {id}")}})),
            1 => ("search_notes",json!({"query":"original"})),
            _ => ("list_decks_and_note_types",json!({})),
        };
        let mut job=call(id,name,args);
        within(async {
            tokio::select! {
                result=&mut job => panic!("call {id} completed before queue release: {result:?}"),
                _=gate.mcp_waiter_pending() => {},
            }
        }).await;
        calls.push((id,job));
    }
    assert_eq!(calls.len(),10);
    assert!(calls.iter().all(|(_,job)|!job.is_finished()),"all ten HTTP calls must remain active");
    release.send(()).unwrap();
    let mut created=Vec::new();
    for (id,job) in calls {
        let wire=within(job).await.unwrap();
        assert_eq!(wire["id"],id);
        assert_eq!(wire["result"]["isError"],false,"{wire}");
        let data=&wire["result"]["structuredContent"];
        let text=wire["result"]["content"][0]["text"].as_str().unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(text).unwrap(),*data);
        match id%3 {
            0 if id!=0 => {
                let note_id=data["note_id"].as_str().unwrap().parse::<i64>().unwrap();
                assert!(!data["revision"].as_str().unwrap().is_empty());
                created.push((id,note_id));
            }
            2 => {
                assert_eq!(data["decks"][0]["deck_id"],"1");
                assert_eq!(data["note_types"][0]["note_type_id"],"10");
            }
            _ => {
                assert_eq!(data["notes"].as_array().unwrap().len(),1);
                assert_eq!(data["notes"][0]["note_id"],"1");
                assert_eq!(data["notes"][0]["fields"]["Front"],"original");
                assert!(data["next_cursor"].is_null());
            }
        }
    }
    // A separate SQLite handle proves every independent write was persisted,
    // with its generated card; result success alone is insufficient.
    let db=crate::collection::schema::open(&f.path,false).unwrap();
    assert_eq!(created.len(),3);
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM notes",[],|r|r.get(0)).unwrap(),4);
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM cards",[],|r|r.get(0)).unwrap(),4);
    for (id,note_id) in created {
        let fields:String=db.query_row("SELECT flds FROM notes WHERE id=?1",[note_id],|r|r.get(0)).unwrap();
        assert_eq!(fields,format!("generated burst {id}\u{1f}"));
        assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM cards WHERE nid=?1",[note_id],|r|r.get(0)).unwrap(),1);
    }
    control.close();stop.send(()).unwrap();within(server).await.unwrap();within(control.idle()).await;
}

#[tokio::test]
async fn mcp_queue_timeout_is_typed_and_cancelled_waiters_release_their_place() {
    use transport::Backend as _;
    let f=Fixture::new(true).await;
    media_type(&f);
    let bridge=McpService::new(f.service.clone());
    let authority=f.service.mcp_authority(&f.token,true).await.unwrap();
    let gate=f.service.mcp_publication_barrier(&authority).await.unwrap();
    let held=gate.enter_mcp().await;
    let before=f.snapshot();
    let call=transport::inputs::parse("create_note",json!({"deck_id":"1","note_type_id":"10","fields":{"Front":"must not commit"}}).as_object().unwrap().clone()).unwrap();
    let queued=bridge.dispatch(authority.clone(),call);
    tokio::pin!(queued);
    within(async {
        tokio::select! {
            _=&mut queued => panic!("create must join the MCP queue"),
            _=gate.mcp_waiter_pending() => {},
        }
    }).await;
    drop(f.service.metadata.test_hold_capacity());
    assert!(gate.try_enter().is_some(),"waiting owns no publication guard");
    let result=tokio::time::timeout(Duration::from_secs(8),&mut queued).await.unwrap();
    assert!(matches!(result,Err(transport::DispatchFailure::Diagnosed {
        failure:transport::ToolFailure::Busy,detail:Detail::McpQueue
    })));
    assert_eq!(Detail::McpQueue.label(),"mcp_queue");
    assert_eq!(f.snapshot(),before);
    assert_eq!(crate::collection::schema::open(&f.path,false).unwrap()
        .query_row::<i64,_,_>("SELECT count(*) FROM notes",[],|r|r.get(0)).unwrap(),1);
    {
        let cancelled=bridge.search_notes(authority.clone(),SearchInput {note_id:Some("1".into()),..Default::default()});
        tokio::pin!(cancelled);
        within(async {
            tokio::select! {
                _=&mut cancelled => panic!("read must join the MCP queue"),
                _=gate.mcp_waiter_pending() => {},
            }
        }).await;
        // Drop the actual future, not just a Pin reference.
    }
    drop(held);
    let read=within(bridge.search_notes(authority,SearchInput {note_id:Some("1".into()),..Default::default()})).await.unwrap();
    assert_eq!(read.notes[0].fields["Front"],"original");
}

#[tokio::test]
async fn cancelled_mutation_keeps_both_guards_until_its_worker_commits() {
    let f=Fixture::new(true).await;
    let authority=f.service.mcp_authority(&f.token,true).await.unwrap();
    let gate=f.service.mcp_publication_barrier(&authority).await.unwrap();
    let read=McpService::new(f.service.clone()).search_notes(authority.clone(),SearchInput {note_id:Some("1".into()),..Default::default()}).await.unwrap();
    let revision=read.notes[0].revision.clone();
    let (arrived,arrival)=tokio::sync::oneshot::channel();
    let (release,released)=std::sync::mpsc::sync_channel(1);
    let arrived=std::sync::Mutex::new(Some(arrived));
    let released=std::sync::Mutex::new(released);
    let bridge=McpService::new(f.service.clone()).with_test_probe(Arc::new(move |at| {
        if at==TestBoundary::Admitted {
            arrived.lock().unwrap().take().unwrap().send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(10)).expect("release mutation");
        }
    }));
    let original=authority.clone();
    let write=tokio::spawn(async move {
        bridge.edit_note(original,EditInput {note_id:"1".into(),revision,
            fields:Some(mcp::Fields(BTreeMap::from([("Front".into(),"committed after cancellation".into())]))),
            add_tags:None,remove_tags:None}).await
    });
    within(arrival).await.unwrap();
    write.abort();assert!(within(write).await.unwrap_err().is_cancelled());
    assert!(gate.try_enter().is_none(),"cancelled caller cannot release publication");
    let reader=McpService::new(f.service.clone());
    let queued=reader.search_notes(authority,SearchInput {note_id:Some("1".into()),..Default::default()});
    tokio::pin!(queued);
    within(async {
        tokio::select! {
            _=&mut queued => panic!("cancelled mutation must still own the MCP queue"),
            _=gate.mcp_waiter_pending() => {},
        }
    }).await;
    release.send(()).unwrap();
    let read=within(&mut queued).await.unwrap();
    assert_eq!(read.notes[0].fields["Front"],"committed after cancellation");
    assert_eq!(f.snapshot().0,"committed after cancellation");
}
