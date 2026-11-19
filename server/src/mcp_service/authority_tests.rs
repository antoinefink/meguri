// SPDX-License-Identifier: MIT
//! Generated accounts, real bridge and loopback HTTP only. Boundary mutations
//! simulate invalidation/clock expiry; they are not lifecycle barrier proofs.
use super::*;
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
