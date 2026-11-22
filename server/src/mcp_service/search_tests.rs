// SPDX-License-Identifier: MIT
//! Real OAuth/collection bridge; generated data and deterministic worker fences.
use super::*;
use crate::collection::mcp::BuildBoundary;
use rusqlite::params;
use std::sync::Mutex;
use tokio::sync::oneshot;

fn sparse(f: &Fixture) {
    let mut c = crate::collection::schema::open(&f.path, true).unwrap();
    let tx = c.transaction().unwrap();
    tx.execute_batch("WITH RECURSIVE n(id) AS (VALUES(2) UNION ALL SELECT id+1 FROM n WHERE id<451)
        INSERT INTO notes SELECT n.id,printf('generated-%d',n.id),mid,mod,usn,' unrelated ','ordinary',sfld,csum,flags,data
        FROM n CROSS JOIN notes WHERE notes.id=1;").unwrap();
    for (id, tags, text) in [
        (1000, " Straße ", "ςſK é猫🦀 [a.b] deck:1"),
        (1001, " STRASSE ", "ΣSK é猫🦀 [a.b] deck:1"),
        (1002, " other ", "ΣSK é猫🦀 [axb] deck:1"),
    ] {
        tx.execute("INSERT INTO notes SELECT ?1,printf('generated-%d',?1),mid,mod,usn,?2,?3,sfld,csum,flags,data FROM notes WHERE id=1",
            params![id,tags,text]).unwrap();
        tx.execute("INSERT INTO cards SELECT ?1,?1,2,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,0,flags,data FROM cards WHERE id=1", [id]).unwrap();
    }
    tx.commit().unwrap();
}
fn ids(result: &SearchResult) -> Vec<&str> {
    result.notes.iter().map(|n| n.note_id.as_str()).collect()
}
async fn search(bridge: &McpService, f: &Fixture, input: SearchInput) -> Result<SearchResult> {
    let a = f.service.mcp_authority(&f.token, false).await.unwrap();
    bridge.search_notes(a, input).await
}
// Assert a real cache hit under the same service authority and page transaction
// as search_notes; a failed build/cold driver must not satisfy an index proof.
async fn cached_search(bridge: &McpService, f: &Fixture, input: SearchInput) -> SearchResult {
    let cache=f.service.mcp_search_cache.clone();
    let original=f.service.mcp_authority(&f.token,false).await.unwrap();
    bridge.execute(original,false,move |backend,path,a| {
        let deck=input.deck_id.as_deref().map(|id|id.parse::<i64>().unwrap());
        let tag=input.tag.clone();let query=input.query.clone();
        backend.search_notes_with_index(path,a,input,|c| {
            let index=cache.get_or_schedule(c,a.original.account_id(),a.original.data_epoch(),&a.held.root,path)
                .expect("completed service-owned index must be present");
            assert!(index.candidates(deck,tag.as_deref(),query.as_deref()).is_some(),"requested facet must be complete");
            Some(index)
        })
    }).await.unwrap()
}
async fn finished(completion: oneshot::Receiver<()>) {
    tokio::time::timeout(Duration::from_secs(10), completion).await
        .expect("bounded build must finish").unwrap();
}
fn completion(f: &Fixture) -> oneshot::Receiver<()> {
    let (send, recv) = oneshot::channel();
    let send = Mutex::new(Some(send));
    f.service.mcp_search_cache.test_probe(Arc::new(move |boundary| {
        if boundary == BuildBoundary::Completed {
            if let Some(send) = send.lock().unwrap().take() { let _ = send.send(()); }
        }
    }));
    recv
}
async fn warm(bridge: &McpService, f: &Fixture) {
    let done = completion(f);
    search(bridge, f, SearchInput { tag:Some("not-present".into()), ..Default::default() }).await.unwrap();
    finished(done).await;
    assert_eq!(f.service.mcp_search_cache.test_is_building(), Some(false));
}

// The same native wire path/framing as the loopback protocol fixtures, with
// generated rows encoded using the repository's native record types.
struct Native {
    client: reqwest::Client,
    base: String,
    key: String,
    stop: oneshot::Sender<()>,
    server: tokio::task::JoinHandle<std::io::Result<()>>,
}
impl Native {
    async fn open(f: &Fixture) -> Self {
        let (_,key)=f.service.native_login("127.0.0.1".parse().unwrap(),
            "generated.admin","generated MCP password".into()).await.unwrap();
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let (stop,stopped)=oneshot::channel();
        let service=f.service.clone();
        let server=tokio::spawn(async move {
            crate::httpapi::serve(listener,service,async {let _=stopped.await;}).await
        });
        Self {client:reqwest::Client::builder().no_proxy().build().unwrap(),base,key,stop,server}
    }
    async fn exchange(&self, method: &str, body: serde_json::Value) -> serde_json::Value {
        let response=self.client.post(format!("{}/sync/{method}",self.base))
            .header("content-type","application/octet-stream")
            .header("anki-sync",json!({"v":11,"k":self.key,"c":"synthetic","s":"search-cache"}).to_string())
            .body(zstd::stream::encode_all(serde_json::to_vec(&body).unwrap().as_slice(),0).unwrap())
            .timeout(Duration::from_secs(15)).send().await.unwrap();
        assert_eq!(response.status(),200,"/sync/{method}");
        let size:usize=response.headers()["anki-original-size"].to_str().unwrap().parse().unwrap();
        let bytes=zstd::stream::decode_all(response.bytes().await.unwrap().as_ref()).unwrap();
        assert_eq!(bytes.len(),size);
        serde_json::from_slice(&bytes).unwrap()
    }
    async fn close(self) {
        self.stop.send(()).unwrap();self.server.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn native_incremental_commit_invalidates_warm_postings_and_continues_old_cursor_on_same_publication() {
    use crate::collection::{chunks::{Card,Note},sanity};
    let f=Fixture::new(false).await;
    sparse(&f);
    let bridge=McpService::new(f.service.clone());
    warm(&bridge,&f).await;
    let request=||SearchInput {tag:Some("strasse".into()),limit:Some(1),..Default::default()};
    let first=cached_search(&bridge,&f,request()).await;
    assert_eq!(ids(&first),["1000"]);
    let old_cursor=first.next_cursor.unwrap();
    // These are empty postings in the warmed key, so stale hits would lose the
    // incoming low-ID tag/text/home-deck memberships entirely.
    for input in [
        SearchInput {tag:Some("native-tag".into()),..Default::default()},
        SearchInput {query:Some("native literal".into()),..Default::default()},
        SearchInput {deck_id:Some("3".into()),..Default::default()},
    ] { assert!(search(&bridge,&f,input).await.unwrap().notes.is_empty()); }
    let original=f.service.mcp_authority(&f.token,false).await.unwrap();
    let stamp=f.snapshot();
    let native=Native::open(&f).await;
    let meta=native.exchange("meta",json!({"v":11,"cv":"synthetic"})).await;
    assert_eq!(native.exchange("start",json!({"minUsn":meta["usn"],"lnewer":false,"graves":null})).await,
        json!({"cards":[],"decks":[],"notes":[]}));
    native.exchange("applyChanges",json!({"changes":{"models":[],"decks":[[],[]],"tags":["native-tag","strasse"]}})).await;
    let mut notes=Vec::<Note>::new();let mut cards=Vec::<Card>::new();
    let mut done=false;
    // 454 notes and four cards require two chunks; bound the drain explicitly.
    for _ in 0..3 {
        let chunk=native.exchange("chunk",json!({})).await;
        notes.extend(serde_json::from_value::<Vec<Note>>(chunk["notes"].clone()).unwrap());
        cards.extend(serde_json::from_value::<Vec<Card>>(chunk["cards"].clone()).unwrap());
        if chunk["done"]==true {done=true;break;}
    }
    assert!(done);assert_eq!(notes.len(),454);assert_eq!(cards.len(),4);
    let mut low=notes.iter().find(|n|n.0==1).unwrap().clone();
    low.3+=1;low.5="native-tag".into();low.6="native literal".into();
    let mut high=notes.iter().find(|n|n.0==1002).unwrap().clone();
    high.3+=1;high.5="STRASSE".into();high.6="native literal".into();
    let mut card=cards.into_iter().find(|c|c.0==1).unwrap();
    card.2=3;card.4.0+=1;
    assert_eq!(native.exchange("applyChunk",json!({"chunk":{"done":true,"notes":[low,high],"cards":[card]}})).await,serde_json::Value::Null);
    // Incoming rows and stamps remain uncommitted until the actual finish.
    assert_eq!(f.snapshot(),stamp);
    let counts={
        let db=crate::collection::schema::open(&f.path,false).unwrap();
        sanity::counts(&db).unwrap()
    };
    assert_eq!(native.exchange("sanityCheck2",json!({"client":counts})).await["status"],"ok");
    let finish=native.exchange("finish",json!({})).await;
    assert_eq!(f.snapshot().3,finish.as_i64().unwrap());
    assert_eq!(f.snapshot().4,stamp.4+1);
    assert_eq!(f.snapshot().0,"native literal");
    let current=f.service.mcp_authority(&f.token,false).await.unwrap();
    assert_eq!(current.data_epoch(),original.data_epoch());
    assert_eq!(f.service.metadata.owner().collection_path(current.account_id()).unwrap(),f.path);

    let completion=completion(&f);
    let fresh=search(&bridge,&f,SearchInput {tag:Some("native-tag".into()),..Default::default()}).await.unwrap();
    assert_eq!(ids(&fresh),["1"]);
    finished(completion).await;
    for (input,expected) in [
        (SearchInput {tag:Some("native-tag".into()),..Default::default()},vec!["1"]),
        (SearchInput {query:Some("native literal".into()),..Default::default()},vec!["1","1002"]),
        (SearchInput {deck_id:Some("3".into()),..Default::default()},vec!["1"]),
    ] {
        let fresh=cached_search(&bridge,&f,input).await;
        assert_eq!(ids(&fresh),expected);
        assert!(fresh.next_cursor.is_none());
    }
    let next=search(&bridge,&f,SearchInput {cursor:Some(old_cursor),..request()}).await.unwrap();
    assert_eq!(ids(&next),["1001"]);
    let last=search(&bridge,&f,SearchInput {cursor:next.next_cursor,..request()}).await.unwrap();
    assert_eq!(ids(&last),["1002"]);
    assert!(last.next_cursor.is_none());
    native.close().await;
}

#[tokio::test]
async fn service_byte_fit_continues_across_old_key_cold_and_new_index_without_skipping() {
    for deck in [None,Some("2")] {
        let f=Fixture::new(true).await;
        sparse(&f);
        let text="\"".repeat(120000);
        {
            let db=crate::collection::schema::open(&f.path,true).unwrap();
            for id in 1003..1006 {
                db.execute("INSERT INTO notes SELECT ?1,printf('generated-%d',?1),mid,mod,usn,tags,flds,sfld,csum,flags,data FROM notes WHERE id=1000",[id]).unwrap();
                db.execute("INSERT INTO cards SELECT ?1,?1,did,ord,mod,usn,type,queue,due,ivl,factor,reps,lapses,left,odue,odid,flags,data FROM cards WHERE id=1000",[id]).unwrap();
            }
            db.execute("UPDATE notes SET flds=?1,tags=' byte-fit ' WHERE id BETWEEN 1000 AND 1005",[&text]).unwrap();
        }
        let bridge=McpService::new(f.service.clone());
        warm(&bridge,&f).await;
        let request=||SearchInput {deck_id:deck.map(str::to_owned),tag:Some("byte-fit".into()),..Default::default()};
        let first=cached_search(&bridge,&f,request()).await;
        assert_eq!(ids(&first),["1000","1001"]);
        assert!(first.next_cursor.is_some());
        assert!(Backend::result_json(&first).unwrap().len()<=mcp::MAX_RESULT_BYTES);
        let revision=search(&bridge,&f,SearchInput {note_id:Some("1".into()),..Default::default()}).await.unwrap().notes.remove(0).revision;
        let stamp=f.snapshot();
        let a=f.service.mcp_authority(&f.token,true).await.unwrap();
        bridge.edit_note(a,EditInput {note_id:"1".into(),revision,
            fields:Some(mcp::Fields(BTreeMap::from([("Front".into(),"new content stamp".into())]))),
            add_tags:None,remove_tags:None}).await.unwrap();
        assert_ne!((f.snapshot().3,f.snapshot().4),(stamp.3,stamp.4));

        let (started,start)=oneshot::channel();let (ended,end)=oneshot::channel();
        let (release,released)=std::sync::mpsc::sync_channel(1);
        let started=Mutex::new(Some(started));let ended=Mutex::new(Some(ended));
        let released=Mutex::new(released);
        f.service.mcp_search_cache.test_probe(Arc::new(move |boundary| match boundary {
            BuildBoundary::Scanning=>{
                started.lock().unwrap().take().unwrap().send(()).unwrap();
                released.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
            }
            BuildBoundary::Completed=>{let _=ended.lock().unwrap().take().unwrap().send(());}
        }));
        // The old-key cursor resumes via cold deck SQL or global tag filtering;
        // the new-key worker cannot publish until after this page returns.
        let middle=search(&bridge,&f,SearchInput {cursor:first.next_cursor.clone(),..request()}).await.unwrap();
        finished(start).await;
        assert_eq!(f.service.mcp_search_cache.test_is_building(),Some(true));
        assert_eq!(ids(&middle),["1002","1003"]);
        assert!(middle.next_cursor.is_some());
        assert!(Backend::result_json(&middle).unwrap().len()<=mcp::MAX_RESULT_BYTES);
        release.send(()).unwrap();finished(end).await;
        // Replay the identical old cursor through the new service-owned index:
        // stable records and position, including the previously omitted match.
        let replay=cached_search(&bridge,&f,SearchInput {cursor:first.next_cursor.clone(),..request()}).await;
        assert_eq!(replay.notes,middle.notes);
        assert_eq!(replay.next_cursor,middle.next_cursor);
        let last=cached_search(&bridge,&f,SearchInput {cursor:middle.next_cursor.clone(),..request()}).await;
        assert_eq!(ids(&last),["1004","1005"]);
        assert!(last.next_cursor.is_none());
        assert!(Backend::result_json(&last).unwrap().len()<=mcp::MAX_RESULT_BYTES);
        for page in [&first,&middle,&last] {
            assert!(page.notes.iter().all(|n|n.fields["Front"]==text));
        }
    }
}

#[tokio::test]
async fn cached_service_sparse_first_pages_match_production_unicode_literals_and_all_deck_combinations() {
    let f = Fixture::new(true).await;
    sparse(&f);
    let bridge = McpService::new(f.service.clone());
    warm(&bridge, &f).await;
    // Each hit is past 451 unrelated IDs. Widths one, two and three use
    // Unicode regex equivalence; punctuation and operators remain literal.
    for (tag, query, expected) in [
        (Some("strasse"), None, vec!["1000","1001"]),
        (None, Some("Σ"), vec!["1000","1001","1002"]),
        (None, Some("SK"), vec!["1000","1001","1002"]),
        (None, Some("ΣSK"), vec!["1000","1001","1002"]),
        (None, Some("É猫🦀"), vec!["1000","1001","1002"]),
        (None, Some("[A.B] DECK:1"), vec!["1000","1001"]),
        (Some("STRASSE"), Some("[A.B]"), vec!["1000","1001"]),
    ] {
        let result = search(&bridge, &f, SearchInput {
            tag:tag.map(str::to_owned), query:query.map(str::to_owned), ..Default::default()
        }).await.unwrap();
        assert_eq!(ids(&result), expected);
        assert!(result.next_cursor.is_none());
    }
    for (tag, query, expected) in [
        (None,None,vec!["1000","1001","1002"]),
        (Some("strasse"),None,vec!["1000","1001"]),
        (None,Some("[a.b]"),vec!["1000","1001"]),
        (Some("strasse"),Some("[a.b]"),vec!["1000","1001"]),
    ] {
        let result = search(&bridge, &f, SearchInput { deck_id:Some("2".into()),
            tag:tag.map(str::to_owned), query:query.map(str::to_owned), ..Default::default()
        }).await.unwrap();
        assert_eq!(ids(&result), expected);
        assert!(result.next_cursor.is_none());
    }
    let exact = search(&bridge, &f, SearchInput { note_id:Some("1".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&exact), ["1"]);
}

#[tokio::test]
async fn committed_mcp_edit_rejects_stale_postings_and_keeps_cursor_continuity() {
    let f = Fixture::new(true).await;
    sparse(&f);
    let bridge = McpService::new(f.service.clone());
    warm(&bridge, &f).await;
    let first = search(&bridge, &f, SearchInput { tag:Some("strasse".into()), limit:Some(1), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&first), ["1000"]);
    let revision = search(&bridge, &f, SearchInput { note_id:Some("1".into()), ..Default::default() }).await.unwrap().notes.remove(0).revision;
    let stamp = f.snapshot();
    let a = f.service.mcp_authority(&f.token, true).await.unwrap();
    bridge.edit_note(a, EditInput { note_id:"1".into(), revision,
        fields:Some(mcp::Fields(BTreeMap::from([("Front".into(),"new literal match".into())]))),
        add_tags:Some(vec!["new-tag".into()]), remove_tags:None,
    }).await.unwrap();
    assert_ne!((f.snapshot().3,f.snapshot().4), (stamp.3,stamp.4));
    let done = completion(&f);
    // An old posting would claim absence and silently lose this new low ID.
    let fresh = search(&bridge, &f, SearchInput { tag:Some("new-tag".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&fresh), ["1"]);
    finished(done).await;
    let fresh = search(&bridge, &f, SearchInput { query:Some("new literal match".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&fresh), ["1"]);
    // Force indexed -> fallback -> indexed across content stamps, preserving
    // the original note-ID cursor and its query shape throughout.
    f.service.mcp_search_cache.invalidate(f.service.mcp_authority(&f.token,false).await.unwrap().account_id());
    let done = completion(&f);
    let next = search(&bridge, &f, SearchInput { tag:Some("strasse".into()), limit:Some(1), cursor:first.next_cursor, ..Default::default() }).await.unwrap();
    assert_eq!(ids(&next), ["1001"]);
    finished(done).await;
    let next = search(&bridge, &f, SearchInput { tag:Some("strasse".into()), limit:Some(1), cursor:next.next_cursor, ..Default::default() }).await.unwrap();
    assert!(next.notes.is_empty());
    assert!(next.next_cursor.is_none());
}

#[tokio::test]
async fn builder_scan_holds_only_snapshot_pin_and_root_not_barrier_or_metadata_slot() {
    let f = Fixture::new(false).await;
    sparse(&f);
    let root = f.service.metadata.owner();
    let baseline = Arc::strong_count(&root);
    let bridge = McpService::new(f.service.clone());
    let a = f.service.mcp_authority(&f.token,false).await.unwrap();
    let gate = f.service.mcp_publication_barrier(&a).await.unwrap();
    let (started, start) = oneshot::channel();
    let (ended, end) = oneshot::channel();
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let started = Mutex::new(Some(started));
    let ended = Mutex::new(Some(ended));
    let released = Mutex::new(released);
    f.service.mcp_search_cache.test_probe(Arc::new(move |boundary| match boundary {
        BuildBoundary::Scanning => {
            started.lock().unwrap().take().unwrap().send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
        }
        BuildBoundary::Completed => { let _ = ended.lock().unwrap().take().unwrap().send(()); }
    }));
    let result = search(&bridge, &f, SearchInput { tag:Some("strasse".into()), ..Default::default() }).await.unwrap();
    finished(start).await;
    assert!(result.notes.is_empty());
    assert!(result.next_cursor.is_some());
    let held = gate.try_enter().expect("scan must release the request publication barrier");
    let slots = f.service.metadata.test_hold_capacity();
    assert_eq!(slots.len(), 8, "builder must not acquire a metadata/files permit");
    assert_eq!(Arc::strong_count(&root), baseline + 1, "only snapshot owns an extra root");
    drop(slots);
    drop(held);
    release.send(()).unwrap();
    finished(end).await;
    assert_eq!(Arc::strong_count(&root), baseline, "completed cache retains no root or SQLite reader");
    let hit = search(&bridge, &f, SearchInput { tag:Some("strasse".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&hit), ["1000","1001"]);
}

#[tokio::test]
async fn reset_reusing_initial_path_and_stamps_fences_inflight_index_cursors_and_revisions() {
    let f = Fixture::new(true).await;
    sparse(&f);
    let bridge = McpService::new(f.service.clone());
    let old_authority = f.service.mcp_authority(&f.token,false).await.unwrap();
    let old_note = search(&bridge, &f, SearchInput { note_id:Some("1001".into()), ..Default::default() }).await.unwrap().notes.remove(0);
    let (started, start) = oneshot::channel();
    let (ended, end) = oneshot::channel();
    let (release, released) = std::sync::mpsc::sync_channel(1);
    let started = Mutex::new(Some(started));
    let ended = Mutex::new(Some(ended));
    let released = Mutex::new(released);
    f.service.mcp_search_cache.test_probe(Arc::new(move |boundary| match boundary {
        BuildBoundary::Scanning => {
            started.lock().unwrap().take().unwrap().send(()).unwrap();
            released.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();
        }
        BuildBoundary::Completed => { let _ = ended.lock().unwrap().take().unwrap().send(()); }
    }));
    let old_page = search(&bridge, &f, SearchInput { tag:Some("strasse".into()), ..Default::default() }).await.unwrap();
    assert!(old_page.notes.is_empty());
    let old_cursor = old_page.next_cursor.unwrap();
    finished(start).await;
    let stamp = f.snapshot();
    let (_, session) = f.service.browser_login("127.0.0.1".parse().unwrap(),"generated.admin","generated MCP password".into()).await.unwrap();
    let browser = f.service.browser_authority(&session).await.unwrap();
    // This renames the account directory while the index-only pin is held.
    f.service.reset_data(browser, old_authority.account_id().into()).await.unwrap();
    let a = f.service.mcp_authority(&f.token,false).await.unwrap();
    assert_ne!(a.data_epoch(), old_authority.data_epoch());
    assert_eq!(f.service.metadata.owner().collection_path(a.account_id()).unwrap(), f.path);
    {
        let db = crate::collection::schema::open(&f.path,true).unwrap();
        db.execute("INSERT INTO notetypes VALUES(10,'Generated',0,0,?1)",[ProtoBlob::new(NotetypeConfig::default()).to_bytes().into_owned()]).unwrap();
        db.execute("INSERT INTO fields VALUES(10,0,'Front',?1)",[ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned()]).unwrap();
        let mut template=TemplateConfig::default();template.q_format="{{Front}}".into();
        db.execute("INSERT INTO templates VALUES(10,0,'Generated',0,0,?1)",[ProtoBlob::new(template).to_bytes().into_owned()]).unwrap();
        db.execute_batch("INSERT INTO notes VALUES(1,'generated',10,0,0,'','original',0,0,0,'');
            INSERT INTO cards VALUES(1,1,1,0,0,0,0,0,1,0,0,0,0,0,0,0,0,'{}');").unwrap();
        db.execute("UPDATE col SET mod=?1,usn=?2 WHERE id=1",params![stamp.3,stamp.4]).unwrap();
    }
    sparse(&f);
    {
        let db = crate::collection::schema::open(&f.path,true).unwrap();
        db.execute("UPDATE notes SET flds='after reset',tags=' reset-tag ' WHERE id=1",[]).unwrap();
    }
    assert_eq!((f.snapshot().3,f.snapshot().4), (stamp.3,stamp.4));
    assert_eq!(search(&bridge, &f, SearchInput { tag:Some("strasse".into()), cursor:Some(old_cursor), ..Default::default() }).await.unwrap_err().code, "conflict");
    let new_note = search(&bridge, &f, SearchInput { note_id:Some("1001".into()), ..Default::default() }).await.unwrap().notes.remove(0);
    assert_eq!(new_note.fields, old_note.fields);
    assert_ne!(new_note.revision, old_note.revision, "epoch must bind identical persisted state");
    let a = f.service.mcp_authority(&f.token,true).await.unwrap();
    assert_eq!(bridge.edit_note(a, EditInput { note_id:"1001".into(),revision:old_note.revision,
        fields:Some(mcp::Fields(BTreeMap::from([("Front".into(),"unexpected".into())]))),
        add_tags:None,remove_tags:None }).await.unwrap_err().code,"conflict");
    let fresh = search(&bridge, &f, SearchInput { tag:Some("reset-tag".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&fresh), ["1"]);
    // The old job cannot fill a new epoch's reservation, even with an equal
    // pathname and stamp. It keeps its slot only until its open reader closes.
    release.send(()).unwrap();
    finished(end).await;
    warm(&bridge,&f).await;
    let fresh = search(&bridge, &f, SearchInput { query:Some("after reset".into()), ..Default::default() }).await.unwrap();
    assert_eq!(ids(&fresh), ["1"]);
}

#[tokio::test]
async fn replacement_with_equal_stamps_rejects_old_cursor_and_postings() {
    let mut f = Fixture::new(false).await;
    sparse(&f);
    let bridge = McpService::new(f.service.clone());
    warm(&bridge,&f).await;
    let first = search(&bridge,&f,SearchInput {tag:Some("strasse".into()),limit:Some(1),..Default::default()}).await.unwrap();
    let a = f.service.mcp_authority(&f.token,false).await.unwrap();
    let root = f.service.metadata.owner();
    let gate = f.service.mcp_publication_barrier(&a).await.unwrap();
    {
        let _guard = gate.enter().await;
        let generation = root.stage_collection(a.account_id()).unwrap();
        {
            let db = crate::collection::schema::open(&f.path,false).unwrap();
            crate::collection::schema::snapshot(&db,&generation.path).unwrap();
            let replacement = crate::collection::schema::open(&generation.path,true).unwrap();
            replacement.execute("UPDATE notes SET flds='replacement-only',tags=' replacement-tag ' WHERE id=1",[]).unwrap();
        }
        generation.activate().unwrap();
        f.path = root.collection_path(a.account_id()).unwrap();
    }
    assert_eq!(search(&bridge,&f,SearchInput {tag:Some("strasse".into()),limit:Some(1),cursor:first.next_cursor,..Default::default()}).await.unwrap_err().code,"conflict");
    let done = completion(&f);
    let result = search(&bridge,&f,SearchInput {tag:Some("replacement-tag".into()),..Default::default()}).await.unwrap();
    assert_eq!(ids(&result), ["1"]);
    finished(done).await;
    let result = search(&bridge,&f,SearchInput {query:Some("replacement-only".into()),..Default::default()}).await.unwrap();
    assert_eq!(ids(&result), ["1"]);
}

#[tokio::test]
async fn unavailable_text_or_tag_facet_uses_original_scan_and_other_usable_facets() {
    for missing_tag in [true,false] {
        let f = Fixture::new(false).await;
        sparse(&f);
        {
            let db = crate::collection::schema::open(&f.path,true).unwrap();
            if missing_tag {
                db.execute("UPDATE notes SET tags=?1 WHERE id=451",["x".repeat(16*1024+1)]).unwrap();
            } else {
                db.execute("UPDATE notes SET flds=?1 WHERE id=451",["x".repeat(32*1024+1)]).unwrap();
            }
        }
        let bridge = McpService::new(f.service.clone());
        warm(&bridge,&f).await;
        let input = || if missing_tag {
            SearchInput {tag:Some("strasse".into()),..Default::default()}
        } else {
            SearchInput {query:Some("[a.b]".into()),..Default::default()}
        };
        let first = search(&bridge,&f,input()).await.unwrap();
        assert!(first.notes.is_empty());
        assert!(first.next_cursor.is_some());
        let mut cursor=first.next_cursor;
        let mut seen=Vec::new();
        for _ in 0..4 {
            let page=search(&bridge,&f,SearchInput {cursor,..input()}).await.unwrap();
            seen.extend(page.notes.into_iter().map(|n|n.note_id));
            cursor=page.next_cursor;
            if cursor.is_none() { break; }
        }
        assert!(cursor.is_none());
        assert_eq!(seen,["1000","1001"]);
        // Available deck membership still wins over the unavailable facet.
        let page=search(&bridge,&f,SearchInput {deck_id:Some("2".into()),..input()}).await.unwrap();
        assert_eq!(ids(&page),["1000","1001"]);
        let page=search(&bridge,&f,SearchInput {tag:Some("strasse".into()),query:Some("[a.b]".into()),..Default::default()}).await.unwrap();
        assert_eq!(ids(&page),["1000","1001"]);
    }
}

#[tokio::test]
async fn indexed_deck_combination_empty_progress_continues_through_cold_driver() {
    let f=Fixture::new(false).await;
    sparse(&f);
    {
        let db=crate::collection::schema::open(&f.path,true).unwrap();
        db.execute_batch("INSERT INTO cards SELECT notes.id,notes.id,2,c.ord,c.mod,c.usn,c.type,c.queue,c.due,c.ivl,c.factor,c.reps,c.lapses,c.left,c.odue,0,c.flags,c.data
            FROM notes CROSS JOIN cards c WHERE notes.id BETWEEN 2 AND 202 AND c.id=1;").unwrap();
    }
    let bridge=McpService::new(f.service.clone());
    warm(&bridge,&f).await;
    let input=||SearchInput {deck_id:Some("2".into()),tag:Some("strasse".into()),query:Some("[a.b]".into()),..Default::default()};
    let first=search(&bridge,&f,input()).await.unwrap();
    assert!(first.notes.is_empty());
    assert!(first.next_cursor.is_some());
    let a=f.service.mcp_authority(&f.token,false).await.unwrap();
    f.service.mcp_search_cache.invalidate(a.account_id());
    let done=completion(&f);
    let next=search(&bridge,&f,SearchInput {cursor:first.next_cursor,..input()}).await.unwrap();
    assert_eq!(ids(&next),["1000","1001"]);
    assert!(next.next_cursor.is_none());
    finished(done).await;
}
