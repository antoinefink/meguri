// SPDX-License-Identifier: MIT
//! Generated accounts, real bridge and loopback HTTP only. Boundary mutations
//! simulate invalidation/clock expiry; they are not lifecycle barrier proofs.
use super::*;
use crate::media;
use crate::mcp_transport::{self as transport, Access, AuthorizationRequest, Authorizer, BoxFuture};
use axum::response::Response;
use meguri_model::proto::{ProtoBlob, notetype::{FieldConfig, NotetypeConfig, TemplateConfig}};
use serde_json::json;
use sha2::Digest as _;
use std::collections::BTreeMap;

const ORIGIN: &str = "https://mcp-generated.example.test";
struct Fixture { _temp: tempfile::TempDir, service: Arc<Service>, token: String, path: PathBuf }
impl Fixture {
    async fn new(write: bool) -> Self {
        let temp=tempfile::tempdir().unwrap();
        let mut root=DataRoot::acquire(&temp.path().join("owner"),true).unwrap();root.init().unwrap();
        let setup=std::fs::read_to_string(root.setup_file()).unwrap().trim().to_owned();
        let service=Service::open(root,crate::Config::new(ORIGIN,"127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let ip="127.0.0.1".parse().unwrap();
        service.claim(ip,"setup",setup,"generated.admin","generated MCP password".into()).await.unwrap();
        let (_,session)=service.browser_login(ip,"generated.admin","generated MCP password".into()).await.unwrap();
        let browser=service.browser_authority(&session).await.unwrap();
        let verifier=crypto::random_token().unwrap();
        let challenge=base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, sha2::Sha256::digest(verifier.as_bytes()));
        let p:BTreeMap<String,String>=[("response_type","code".into()),("client_id","https://chatgpt.com/oauth/client.json".into()),
            ("redirect_uri","https://chatgpt.com/connector_platform_oauth_redirect".into()),("resource",format!("{ORIGIN}/mcp")),
            ("scope",oauth::scopes(write).into()),("code_challenge_method","S256".into()),("code_challenge",challenge)]
            .into_iter().map(|(k,v)|(k.into(),v)).collect();
        let redirect=service.oauth_begin(oauth::Authorization::parse(ORIGIN,&p).unwrap()).await.unwrap();
        let handle=redirect.split_once("#request=").unwrap().1;
        service.oauth_consent_read(browser.clone(),handle).await.unwrap();
        let redirect=service.oauth_consent_decide(browser,handle,if write {"read_write"} else {"read"}).await.unwrap();
        let url=url::Url::parse(&redirect).unwrap();
        let code=url.query_pairs().find(|(k,_)|k=="code").unwrap().1.into_owned();
        let p=[("grant_type","authorization_code".into()),("client_id","https://chatgpt.com/oauth/client.json".into()),
            ("code",code),("code_verifier",verifier),("resource",format!("{ORIGIN}/mcp"))]
            .into_iter().map(|(k,v)|(k.into(),v)).collect();
        let token=serde_json::to_value(service.oauth_token(p).await.unwrap()).unwrap()["access_token"].as_str().unwrap().to_owned();
        let a=service.mcp_authority(&token,false).await.unwrap();
        let path=service.metadata.owner().collection_path(a.account_id()).unwrap();
        let db=crate::collection::schema::open(&path,true).unwrap();
        db.execute("INSERT INTO notetypes VALUES(10,'Generated',0,0,?1)",[ProtoBlob::new(NotetypeConfig::default()).to_bytes().into_owned()]).unwrap();
        db.execute("INSERT INTO fields VALUES(10,0,'Front',?1)",[ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned()]).unwrap();
        let mut template=TemplateConfig::default();template.q_format="{{Front}}".into();
        db.execute("INSERT INTO templates VALUES(10,0,'Generated',0,0,?1)",[ProtoBlob::new(template).to_bytes().into_owned()]).unwrap();
        db.execute_batch("INSERT INTO notes VALUES(1,'generated',10,0,0,'','original',0,0,0,''); INSERT INTO cards VALUES(1,1,1,0,0,0,0,0,1,0,0,0,0,0,0,0,0,'{}')").unwrap();
        Self {_temp:temp,service,token,path}
    }
    fn snapshot(&self) -> (String,i64,i64,i64,i64,i64) {
        let db=crate::collection::schema::open(&self.path,false).unwrap();
        db.query_row("SELECT flds,notes.mod,notes.usn,col.mod,col.usn,(SELECT count(*) FROM cards) FROM notes CROSS JOIN col WHERE notes.id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).unwrap()
    }
}

struct Admission {
    bridge: Arc<McpService>,
    invalidate: Option<&'static str>,
    scope_probe: bool,
}
impl Authorizer<oauth::Authority> for Admission {
    fn authorize<'a>(&'a self, mut request: AuthorizationRequest<'a>) -> BoxFuture<'a,std::result::Result<oauth::Authority,Response>> {
        Box::pin(async move {
            // The scope-only probe deliberately supplies read authority to a
            // write dispatch, testing its defense after the HTTP seam.
            if self.scope_probe { request.required=Access::Read; }
            let authority=self.bridge.authorize(request).await?;
            if let Some(sql)=self.invalidate {
                self.bridge.service.metadata.run(move |c| { c.execute(sql,[])?;Ok(()) }).await.unwrap();
            }
            Ok(authority)
        })
    }
}

#[tokio::test]
async fn late_oauth_failures_challenge_without_releasing_stale_reads_or_committing() {
    for (sql,completion,scope_probe) in [
        (Some("UPDATE oauth_grants SET revoked=1"),false,false),
        (Some("UPDATE oauth_tokens SET expires_at=1"),false,false),
        (Some("UPDATE oauth_tokens SET expires_at=1"),true,false),
        (None,false,true),
    ] {
        for write in [false,true] {
            if scope_probe && !write {continue;}
            let f=Fixture::new(!scope_probe).await;
            let bridge=McpService::new(f.service.clone());
            let authority=f.service.mcp_authority(&f.token,false).await.unwrap();
            let read=bridge.search_notes(authority,SearchInput {note_id:Some("1".into()),..Default::default()}).await.unwrap();
            assert_eq!(read.notes[0].fields["Front"],"original");
            let revision=read.notes[0].revision.clone();
            let before=f.snapshot();
            let bridge=if completion {
                let database=f.service.metadata.owner().database().to_path_buf();
                bridge.with_test_probe(Arc::new(move |boundary| {
                    if boundary==TestBoundary::BeforeCompletion {
                        let db=storage::open(database.clone(), false).unwrap();
                        db.execute(sql.unwrap(),[]).unwrap();
                    }
                }))
            } else {bridge};
            let bridge=Arc::new(bridge);
            let admission=Arc::new(Admission {bridge:bridge.clone(),invalidate:if completion {None} else {sql},scope_probe});
            let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address=listener.local_addr().unwrap();
            let config=crate::Config::new(ORIGIN,address).unwrap();
            let (router,control)=transport::router(&config,admission,bridge);
            let (stop,stopped)=tokio::sync::oneshot::channel();
            let server=tokio::spawn(async move {axum::serve(listener,router).with_graceful_shutdown(async {let _=stopped.await;}).await.unwrap();});
            let (name,arguments)=if write {("edit_note",json!({"note_id":"1","revision":revision,"fields":{"Front":"changed"}}))}
                else {("search_notes",json!({"note_id":"1"}))};
            let response=reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(5)).build().unwrap()
                .post(format!("http://{address}/mcp")).bearer_auth(&f.token).header("mcp-protocol-version","2025-11-25")
                .json(&json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":arguments}})).send().await.unwrap();
            assert_eq!(response.status(),if scope_probe {403} else {401});
            let challenge=response.headers()["www-authenticate"].to_str().unwrap();
            assert!(challenge.contains(if scope_probe {"insufficient_scope"} else {"invalid_token"}));
            assert!(challenge.contains(&format!("{ORIGIN}/.well-known/oauth-protected-resource/mcp")));
            let body=response.text().await.unwrap();
            assert!(body.is_empty(),"must not release a tool result: {body}");
            assert_eq!(f.snapshot(),before);
            control.close();stop.send(()).unwrap();server.await.unwrap();control.idle().await;
        }
    }
}

fn generated_audio() -> Vec<u8> {
    // Two complete MPEG-1 layer III frames with fixture-owned payloads.
    let mut bytes=vec![0;834];
    for at in [0,417] { bytes[at..at+4].copy_from_slice(&[0xff,0xfb,0x90,0]); }
    bytes
}
fn media_type(f: &Fixture) {
    use meguri_model::proto::{deck::{DeckCommon,DeckKind,DeckKindMessage,NormalDeck},deck_config::DeckConfigMessage};
    let db=crate::collection::schema::open(&f.path,true).unwrap();
    let common=ProtoBlob::new(DeckCommon::default()).to_bytes().into_owned();
    let mut normal=NormalDeck::default();normal.config_id=1;
    let kind=ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))).to_bytes().into_owned();
    db.execute("INSERT OR REPLACE INTO decks VALUES(1,'Generated deck',0,0,?1,?2)",rusqlite::params![common,kind]).unwrap();
    db.execute("INSERT OR REPLACE INTO deck_config VALUES(1,'Generated preset',0,0,?1)",[ProtoBlob::new(DeckConfigMessage::default()).to_bytes().into_owned()]).unwrap();
    db.execute("INSERT INTO fields VALUES(10,1,'Back',?1)",[ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned()]).unwrap();
    db.execute("UPDATE notes SET flds='original'||char(31)||'kept' WHERE id=1",[]).unwrap();
}
async fn mapped(bridge: &McpService, f: &Fixture, name: &str, args: serde_json::Value) -> std::result::Result<transport::ToolOutput,transport::DispatchFailure> {
    use transport::Backend as _;
    let authority=f.service.mcp_authority(&f.token,true).await.unwrap();
    bridge.dispatch(authority,transport::inputs::parse(name,args.as_object().unwrap().clone()).unwrap()).await
}
fn media_counts(f: &Fixture) -> (i64,i64,usize) {
    let root=f.service.metadata.owner();
    let db=storage::inspect(root.database()).unwrap();
    let usn=db.query_row("SELECT media_usn FROM accounts",[],|r|r.get(0)).unwrap();
    let rows=db.query_row("SELECT count(*) FROM media_names",[],|r|r.get(0)).unwrap();
    let a: String=db.query_row("SELECT id FROM accounts",[],|r|r.get(0)).unwrap();
    let durable=std::fs::read_dir(root.stores(&a).unwrap().media).unwrap().filter(|e|e.as_ref().unwrap().file_name().to_string_lossy().starts_with("blob-")).count();
    (usn,rows,durable)
}
#[tokio::test]
async fn direct_mapping_reuses_bytes_and_stale_or_invalid_notes_leave_no_durable_blobs() {
    let f=Fixture::new(true).await;media_type(&f);
    let mut bridge=McpService::new(f.service.clone());
    let fetched=Arc::new(std::sync::atomic::AtomicUsize::new(0));let count=fetched.clone();
    bridge.direct_fixture=Some(Arc::new(move |url| {
        assert_eq!(url,"https://files.oaiusercontent.com/generated?secret=synthetic");
        count.fetch_add(1,std::sync::atomic::Ordering::SeqCst);Ok(generated_audio())
    }));
    let files=json!([{"download_url":"https://files.oaiusercontent.com/generated?secret=synthetic","file_id":"fixture-secret","file_name":"untrusted.png","mime_type":"image/png"}]);
    let created=mapped(&bridge,&f,"create_note",json!({"deck_id":"1","note_type_id":"10","fields":{"Front":"question"},"files":files,"file_fields":["Back"]})).await;
    let transport::ToolOutput::Changed {note_id,revision}=created.ok().unwrap() else {panic!()};
    let a=f.service.mcp_authority(&f.token,true).await.unwrap();
    let read=bridge.search_notes(a,SearchInput {note_id:Some(note_id.clone()),..Default::default()}).await.unwrap();
    assert_eq!(read.notes[0].fields["Front"],"question");
    let reference=read.notes[0].fields["Back"].clone();
    assert!(reference.starts_with("[sound:meguri-"));assert!(reference.ends_with(".mp3]"));
    assert_eq!(media_counts(&f),(1,1,1));
    // Supplied replacements are the base text. Omitted fields keep stored text.
    let edited=mapped(&bridge,&f,"edit_note",json!({"note_id":note_id,"revision":revision,"fields":{"Back":"answer"},"files":files,"file_fields":["Back"]})).await;
    let transport::ToolOutput::Changed {revision:new_revision,..}=edited.ok().unwrap() else {panic!()};
    assert_ne!(new_revision,revision);assert_eq!(media_counts(&f),(1,1,1));
    let noop=mapped(&bridge,&f,"edit_note",json!({"note_id":note_id,"revision":new_revision,"files":files,"file_fields":["Back"]})).await;
    let transport::ToolOutput::Changed {revision:unchanged,..}=noop.ok().unwrap() else {panic!()};
    assert_eq!(unchanged,new_revision);assert_eq!(media_counts(&f),(1,1,1));
    let stale=mapped(&bridge,&f,"edit_note",json!({"note_id":note_id,"revision":revision,"files":files,"file_fields":["Front"]})).await;
    assert!(matches!(stale,Err(transport::DispatchFailure::Tool(transport::ToolFailure::Conflict))));
    // Different valid bytes need a new temporary, but invalid fields cannot publish it.
    bridge.direct_fixture=Some(Arc::new(|_| {let mut bytes=generated_audio();bytes[50]=1;Ok(bytes)}));
    let invalid=mapped(&bridge,&f,"create_note",json!({"deck_id":"1","note_type_id":"10","fields":{},"files":files,"file_fields":["Unknown"]})).await;
    assert!(matches!(invalid,Err(transport::DispatchFailure::Tool(transport::ToolFailure::InvalidInput))));
    let stale=mapped(&bridge,&f,"edit_note",json!({"note_id":note_id,"revision":revision,"files":files,"file_fields":["Front"]})).await;
    assert!(matches!(stale,Err(transport::DispatchFailure::Tool(transport::ToolFailure::Conflict))));
    assert_eq!(media_counts(&f),(1,1,1));
    // A no-op note can still need its previously referenced media installed.
    bridge.direct_fixture=Some(Arc::new(|_| Ok(generated_audio())));
    let db=storage::open(f.service.metadata.owner().database(),false).unwrap();
    db.execute("UPDATE media_names SET blob=NULL",[]).unwrap();drop(db);
    let again=mapped(&bridge,&f,"edit_note",json!({"note_id":note_id,"revision":new_revision,"files":files,"file_fields":["Back"]})).await;
    let transport::ToolOutput::Changed { revision:installed,.. }=again.ok().unwrap() else {panic!()};
    assert_eq!(installed,new_revision);
    assert_eq!(media_counts(&f),(2,1,2));
    assert_eq!(fetched.load(std::sync::atomic::Ordering::SeqCst),4);
}
#[tokio::test]
async fn direct_refusals_and_late_revocation_or_expiry_never_publish() {
    for mutation in ["UPDATE oauth_grants SET revoked=1","UPDATE oauth_tokens SET expires_at=1"] {
        let f=Fixture::new(true).await;
        let mut bridge=McpService::new(f.service.clone());
        let database=f.service.metadata.owner().database();
        // Grant and token changes occur after the request pin, during acquisition.
        bridge.direct_fixture=Some(Arc::new(move |_| {
            let db=storage::open(database.clone(),false).unwrap();db.execute(mutation,[]).unwrap();Ok(generated_audio())
        }));
        let before=f.snapshot();
        let result=mapped(&bridge,&f,"edit_note",json!({"note_id":"1","revision":"stale","files":[{"download_url":"https://files.oaiusercontent.com/generated","file_id":"f"}],"file_fields":["Front"]})).await;
        assert!(matches!(result,Err(transport::DispatchFailure::Challenge(_))));
        assert_eq!(f.snapshot(),before);assert_eq!(media_counts(&f),(0,0,0));
    }
    let f=Fixture::new(true).await;
    let mut bridge=McpService::new(f.service.clone());
    bridge.direct_fixture=Some(Arc::new(|_| panic!("a refused URL must not reach the fixture")));
    for url in ["http://files.oaiusercontent.com/generated","https://untrusted.example.test/generated","https://files.oaiusercontent.com/generated#fragment"] {
        let result=mapped(&bridge,&f,"edit_note",json!({"note_id":"1","revision":"stale","files":[{"download_url":url,"file_id":"f"}],"file_fields":["Front"]})).await;
        assert!(matches!(result,Err(transport::DispatchFailure::Tool(transport::ToolFailure::AttachmentUnavailable))));
    }
    assert_eq!(media_counts(&f),(0,0,0));
}

#[tokio::test]
async fn upload_expiry_is_generic_and_never_publishes_media() {
    let f=Fixture::new(true).await;
    let bridge=McpService::new(f.service.clone());
    let a=f.service.mcp_authority(&f.token,true).await.unwrap();
    let (id,_,_)=bridge.prepare_media_upload(a).await.unwrap();
    f.service.mcp_uploads.expire_fixture(&id);
    let result=mapped(&bridge,&f,"edit_note",json!({"note_id":"1","revision":"irrelevant","uploads":[{"upload_id":id,"field":"Front"}]})).await;
    assert!(matches!(result,Err(transport::DispatchFailure::Tool(transport::ToolFailure::AttachmentUnavailable))));
    assert_eq!(media_counts(&f),(0,0,0));
}
#[tokio::test]
async fn occupied_generated_media_names_compare_actual_bytes_before_coordination() {
    use std::io::Write;
    let f=Fixture::new(true).await;
    let root=f.service.metadata.owner();let a=f.service.mcp_authority(&f.token,true).await.unwrap();
    let value=Arc::new(attachments::Validated::new(generated_audio()).unwrap());
    // Deliberately occupy the generated name through ordinary media storage with
    // different valid bytes and matching metadata for those different bytes.
    let mut other=generated_audio();other[50]=1;
    let mut temp=media::blobs::Temporary::create(&root,a.account_id(),f.service.temp.clone()).unwrap();temp.write_all(&other).unwrap();
    let content=media::blobs::Content {blob:temp.publish().unwrap(),size:other.len() as u64,
        sha1:media::blobs::hex(&sha1::Sha1::digest(&other)),sha256:sha2::Sha256::digest(&other).to_vec()};
    let mut db=storage::open(root.database(),false).unwrap();let tx=db.transaction().unwrap();
    media::index::apply(&tx,a.account_id(),&[(value.name.clone(),Some(content))],&[true]).unwrap();tx.commit().unwrap();drop(db);
    let result=media::prepare_insert(&root,a.account_id(),vec![value],f.service.temp.clone());
    assert!(matches!(result,Err(Error {code:"attachment_conflict",..})));assert_eq!(media_counts(&f),(1,1,1));
}
#[cfg(feature="test-hooks")]
#[tokio::test]
async fn media_blob_durability_failure_rolls_back_note_before_metadata_commit() {
    let f=Fixture::new(true).await;media_type(&f);
    let original=f.service.mcp_authority(&f.token,true).await.unwrap();
    let root=f.service.metadata.owner();
    let value=Arc::new(attachments::Validated::new(generated_audio()).unwrap());
    let mut prepared=media::prepare_insert(&root,original.account_id(),vec![value],f.service.temp.clone()).unwrap();
    let barrier=f.service.mcp_publication_barrier(&original).await.unwrap();let guard=barrier.try_enter().unwrap();
    let before=f.snapshot();
    let path=f.path.clone();let sessions=f.service.normal_sessions.clone();let backend=Backend::new(f.service.mcp_key);
    let result=f.service.metadata.files(move |_| {
        let pin=root.pin_collection(&path)?;
        let publication=path.parent().unwrap().file_name().unwrap().to_str().unwrap().to_owned();
        let context=Context {account:original.account_id().to_owned(),publication};
        let held=Arc::new(HeldPublication {root:root.clone(),path:path.clone(),_pin:pin,_barrier:barrier,_guard:guard});
        let authority=CollectionAuthority {original,context,held,sessions,write:true,probe:None};
        let mut install=||crate::test_hooks::storage_failure("media_before_blob_durability",||media::install_insert(&root,&authority.original,&mut prepared));
        backend.create_note_with_media(&path,&authority,CreateInput {deck_id:"1".into(),note_type_id:"10".into(),
            fields:mcp::Fields(BTreeMap::from([("Front".into(),"new question".into())])),tags:None},&Runtime,&[],Some(&mut install))
    }).await;
    assert!(result.is_err());assert_eq!(f.snapshot(),before);assert_eq!(media_counts(&f),(0,0,0));
    let db=crate::collection::schema::open(&f.path,false).unwrap();
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM notes",[],|r|r.get(0)).unwrap(),1);
}
