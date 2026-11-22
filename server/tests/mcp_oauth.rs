// SPDX-License-Identifier: MIT
//! Generated accounts and loopback HTTP only. Redirects are inspected, never
//! followed to any hosted client or configured public HTTPS origin.
mod support;
use support::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use meguri_sync_server::crypto;
use reqwest::{Client, redirect::Policy};
use serde_json::{Value,json};
use std::collections::BTreeMap;

const CLIENT: &str="https://chatgpt.com/oauth/client.json";
const CALLBACK: &str="https://chatgpt.com/connector_platform_oauth_redirect";
fn local_client() -> Client {Client::builder().no_proxy().redirect(Policy::none()).build().unwrap()}
fn authorization(verifier: &str, write: bool) -> BTreeMap<String,String> {
    [("response_type","code".into()),("client_id",CLIENT.into()),("redirect_uri",CALLBACK.into()),
        ("resource",format!("{ORIGIN}/mcp")),("scope",if write {"collection:read collection:write"} else {"collection:read"}.into()),
        ("code_challenge_method","S256".into()),("code_challenge",URL_SAFE_NO_PAD.encode(crypto::digest(verifier.as_bytes()))),
        ("state","an opaque state + & = / 🦀".into())].into_iter().map(|(k,v)|(k.into(),v)).collect()
}
fn encoded(p: &BTreeMap<String,String>) -> String {
    url::form_urlencoded::Serializer::new(String::new()).extend_pairs(p.iter()).finish()
}
async fn begin(h: &Harness, c: &Client, verifier: &str) -> String {
    let p=authorization(verifier,true);
    let response=c.get(format!("{}/oauth/authorize?{}",h.base,encoded(&p))).send().await.unwrap();
    assert_eq!(response.status(),302);
    assert_eq!(response.headers()["cache-control"],"no-store");
    assert_eq!(response.headers()["referrer-policy"],"no-referrer");
    let location=response.headers()["location"].to_str().unwrap();
    let (page,handle)=location.split_once("#request=").unwrap();
    assert_eq!(page,format!("{ORIGIN}/oauth/consent"));
    handle.into()
}
async fn token(h: &Harness, c: &Client, code: &str, verifier: &str) -> reqwest::Response {
    let p:BTreeMap<String,String>=[("grant_type","authorization_code".into()),("client_id",CLIENT.into()),("code",code.into()),("code_verifier",verifier.into()),("resource",format!("{ORIGIN}/mcp"))]
        .into_iter().map(|(k,v)|(k.into(),v)).collect();
    c.post(format!("{}/oauth/token",h.base)).header("content-type","application/x-www-form-urlencoded").body(encoded(&p)).send().await.unwrap()
}

#[tokio::test]
async fn anonymous_preview_is_name_only_read_only_and_rejects_consumed_or_expired_handles() {
    let h=Harness::new().await;
    let c=local_client();
    h.setup().await;
    let cookie=h.login("root.admin",PASSWORD).await;
    let verifier=crypto::random_token().unwrap();
    let handle=begin(&h,&c,&verifier).await;
    let snapshot=|| {
        let db=rusqlite::Connection::open(h.database()).unwrap();
        let pending:(String,i64,Option<Vec<u8>>,Option<String>,Option<i64>)=db.query_row(
            "SELECT request,expires_at,session,account_id,generation FROM oauth_pending WHERE hash=?1",
            [crypto::digest(handle.as_bytes()).as_slice()],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
        let counts:Vec<i64>=["oauth_pending","oauth_codes","oauth_grants","oauth_tokens"].iter().map(|table|
            db.query_row(&format!("SELECT count(*) FROM {table}"),[],|r| r.get(0)).unwrap()).collect();
        (pending,counts)
    };
    let before=snapshot();
    assert_eq!((before.0.2.clone(),before.0.3.clone(),before.0.4),(None,None,None));
    // Cookies (even malformed ones) neither authorize nor bind this display-only route.
    for cookie in [None,Some("__Host-meguri-session=invalid")] {
        let response=h.post("/api/mcp/authorization/preview",json!({"request":handle}),cookie).await;
        assert_eq!(response.status(),200);
        assert_eq!(response.headers()["cache-control"],"no-store");
        assert_eq!(response.headers()["referrer-policy"],"no-referrer");
        assert!(response.headers().get("set-cookie").is_none());
        assert_eq!(response.json::<Value>().await.unwrap(),json!({"data":{"client_name":"ChatGPT"}}));
        assert_eq!(snapshot(),before);
    }
    assert_eq!(h.post("/api/mcp/authorization/read",json!({"request":handle}),Some(&cookie)).await.status(),200);
    let bound=snapshot();
    assert!(bound.0.2.is_some());
    assert_eq!(h.post("/api/mcp/authorization/preview",json!({"request":handle}),None).await.status(),200);
    assert_eq!(snapshot(),bound);
    assert_eq!(h.post("/api/mcp/authorization/decide",json!({"request":handle,"decision":"deny"}),Some(&cookie)).await.status(),200);
    let consumed=h.post("/api/mcp/authorization/preview",json!({"request":handle}),None).await;
    assert_eq!(consumed.status(),403);
    let failure=consumed.json::<Value>().await.unwrap();
    assert_eq!(failure,json!({"error":{"code":"capability_invalid"}}));
    let expired=begin(&h,&c,&verifier).await;
    let db=rusqlite::Connection::open(h.database()).unwrap();
    db.execute("UPDATE oauth_pending SET expires_at=0 WHERE hash=?1",[crypto::digest(expired.as_bytes()).as_slice()]).unwrap();
    for handle in [expired,crypto::random_token().unwrap(),String::new(),"bad".into()] {
        let response=h.post("/api/mcp/authorization/preview",json!({"request":handle}),None).await;
        assert_eq!(response.status(),403);
        assert_eq!(response.json::<Value>().await.unwrap(),failure);
    }
    let binding:Option<Vec<u8>>=db.query_row("SELECT session FROM oauth_pending WHERE expires_at=0",[],|r|r.get(0)).unwrap();
    assert!(binding.is_none());
    for table in ["oauth_codes","oauth_grants","oauth_tokens"] {
        assert_eq!(db.query_row::<i64,_,_>(&format!("SELECT count(*) FROM {table}"),[],|r|r.get(0)).unwrap(),0);
    }
    drop(db);
    let stale=begin(&h,&c,&verifier).await;
    assert_eq!(h.post("/api/mcp/authorization/read",json!({"request":stale}),Some(&cookie)).await.status(),200);
    assert_eq!(h.post("/api/logout",json!({}),Some(&cookie)).await.status(),200);
    let response=h.post("/api/mcp/authorization/preview",json!({"request":stale}),None).await;
    assert_eq!(response.status(),403);
    assert_eq!(response.json::<Value>().await.unwrap(),failure);
    h.stop().await;
}

#[tokio::test]
async fn anonymous_preview_uses_browser_origin_schema_and_attempt_limits() {
    let h=Harness::new().await;
    let c=local_client();
    let handle=begin(&h,&c,&crypto::random_token().unwrap()).await;
    let path=format!("{}/api/mcp/authorization/preview",h.base);
    for origin in [None,Some("null"),Some("https://untrusted.example.test")] {
        let mut request=c.post(&path).json(&json!({"request":handle}));
        if let Some(value)=origin { request=request.header("origin",value); }
        assert_eq!(request.send().await.unwrap().status(),403);
    }
    let repeated=c.post(&path).header("origin",ORIGIN).header("origin",ORIGIN).json(&json!({"request":handle})).send().await.unwrap();
    assert_eq!(repeated.status(),403);
    for body in [format!(r#"{{"request":"{handle}","request":"{handle}"}}"#),format!(r#"{{"request":"{handle}","client_name":"Untrusted"}}"#),"{}".into()] {
        assert_eq!(c.post(&path).header("origin",ORIGIN).header("content-type","application/json").body(body).send().await.unwrap().status(),400);
    }
    assert_eq!(c.post(&path).header("origin",ORIGIN).header("content-type","text/plain").body("{}").send().await.unwrap().status(),415);
    assert_eq!(c.post(&path).header("origin",ORIGIN).header("content-type","application/json").body(" ".repeat(16*1024+1)).send().await.unwrap().status(),413);
    assert_eq!(h.post("/api/mcp/authorization/preview?client_name=Untrusted",json!({"request":handle}),None).await.status(),400);
    assert_eq!(h.get("/api/mcp/authorization/preview",None).await.status(),404);
    for _ in 0..10 {
        assert_eq!(h.post("/api/mcp/authorization/preview",json!({"request":handle}),None).await.status(),200);
    }
    let exhausted=h.post("/api/mcp/authorization/preview",json!({"request":handle}),None).await;
    assert_eq!(exhausted.status(),429);
    assert!(exhausted.headers().contains_key("retry-after"));
    assert_eq!(exhausted.headers()["cache-control"],"no-store");
    h.stop().await;
}

#[tokio::test]
async fn discovery_authorize_consent_token_listing_and_revoke_over_http() {
    let h=Harness::new().await;
    let c=local_client();
    let resource=c.get(format!("{}/.well-known/oauth-protected-resource/mcp",h.base)).send().await.unwrap();
    assert_eq!(resource.status(),200);
    let resource:Value=resource.json().await.unwrap();
    assert_eq!(resource["resource"],format!("{ORIGIN}/mcp"));
    assert_eq!(resource["authorization_servers"],json!([ORIGIN]));
    let issuer:Value=c.get(format!("{}/.well-known/oauth-authorization-server",h.base)).send().await.unwrap().json().await.unwrap();
    assert_eq!(issuer["issuer"],ORIGIN);
    assert_eq!(issuer["code_challenge_methods_supported"],json!(["S256"]));
    assert_eq!(issuer["token_endpoint_auth_methods_supported"],json!(["none"]));
    assert_eq!(issuer["authorization_response_iss_parameter_supported"],true);
    let verifier=crypto::random_token().unwrap();
    // Signed-out entry creates only a pending request, then normal sign-in.
    let handle=begin(&h,&c,&verifier).await;
    assert_eq!(h.post("/api/mcp/authorization/read",json!({"request":handle}),None).await.status(),401);
    let identity=h.setup().await;
    let cookie=h.login("root.admin",PASSWORD).await;
    let read=h.post("/api/mcp/authorization/read",json!({"request":handle}),Some(&cookie)).await;
    assert_eq!(read.status(),200);
    let read:Value=read.json().await.unwrap();
    assert_eq!(read["data"],json!({"username":"root.admin","client_name":"ChatGPT","redirect_host":"chatgpt.com","access_options":["read","read_write"]}));
    let decision=h.post("/api/mcp/authorization/decide",json!({"request":handle,"decision":"read_write"}),Some(&cookie)).await;
    assert_eq!(decision.status(),200);
    let decision:Value=decision.json().await.unwrap();
    let redirect=url::Url::parse(decision["data"]["redirect_to"].as_str().unwrap()).unwrap();
    let p:BTreeMap<_,_>=redirect.query_pairs().into_owned().collect();
    assert_eq!(redirect.host_str(),Some("chatgpt.com"));
    assert_eq!(redirect.path(),"/connector_platform_oauth_redirect");
    assert_eq!(p["iss"],ORIGIN);
    assert_eq!(p["state"],"an opaque state + & = / 🦀");
    let response=token(&h,&c,&p["code"],&verifier).await;
    assert_eq!(response.status(),200);
    assert_eq!(response.headers()["cache-control"],"no-store");
    let tokens:Value=response.json().await.unwrap();
    assert_eq!(tokens["scope"],"collection:read collection:write");
    assert_eq!(tokens["token_type"],"Bearer");
    assert!(tokens["expires_in"].as_i64().is_some_and(|v| v>0 && v<=900));
    let access=tokens["access_token"].as_str().unwrap();
    assert!(h.service.browser_authority(access).await.is_err());
    assert!(h.service.validate_hostkey(access).await.is_err());
    let listing:Value=h.get("/api/me/mcp-grants",Some(&cookie)).await.json().await.unwrap();
    assert_eq!(listing["data"]["account_id"],identity["id"]);
    let grant=listing["data"]["grants"][0]["id"].as_str().unwrap();
    assert_eq!(listing["data"]["grants"][0]["client_name"],"ChatGPT");
    assert_eq!(listing["data"]["grants"][0]["access"],"read_write");
    assert!(!listing.to_string().contains(access));
    let revoked=h.post(&format!("/api/me/mcp-grants/{grant}/revoke"),json!({}),Some(&cookie)).await;
    assert_eq!(revoked.status(),200);
    assert_eq!(revoked.json::<Value>().await.unwrap(),json!({"data":{"account_id":identity["id"]}}));
    let listing:Value=h.get("/api/me/mcp-grants",Some(&cookie)).await.json().await.unwrap();
    assert_eq!(listing["data"]["grants"],json!([]));
    let refresh:BTreeMap<String,String>=[("grant_type","refresh_token".into()),("refresh_token",tokens["refresh_token"].as_str().unwrap().into()),("resource",format!("{ORIGIN}/mcp"))]
        .into_iter().map(|(k,v)|(k.into(),v)).collect();
    let response=c.post(format!("{}/oauth/token",h.base)).header("content-type","application/x-www-form-urlencoded").body(encoded(&refresh)).send().await.unwrap();
    assert_eq!(response.status(),400);
    assert_eq!(response.json::<Value>().await.unwrap(),json!({"error":"invalid_grant"}));
    h.stop().await;
}

#[tokio::test]
async fn approved_error_redirects_and_local_rejection_never_follow_request_urls() {
    let h=Harness::new().await;
    let c=local_client();
    let verifier=crypto::random_token().unwrap();
    let good=authorization(&verifier,true);
    for (field,value) in [("client_id","https://chatgpt.com.evil/oauth/client.json"),("redirect_uri","https://chatgpt.com/connector_platform_oauth_redirect/"),("redirect_uri","https://user@chatgpt.com/connector_platform_oauth_redirect"),("redirect_uri","https://chatgpt.com/connector_platform_oauth_redirect#x")] {
        let mut p=good.clone(); p.insert(field.into(),value.into());
        let r=c.get(format!("{}/oauth/authorize?{}",h.base,encoded(&p))).send().await.unwrap();
        assert_eq!(r.status(),400);
        assert!(r.headers().get("location").is_none());
        assert!(!r.text().await.unwrap().contains(value));
    }
    for (field,value,error) in [("code_challenge_method","plain","invalid_request"),("scope","collection:write","invalid_scope"),("resource","https://sync.example.test/mcp/","invalid_target"),("response_type","token","unsupported_response_type")] {
        let mut p=good.clone();p.insert(field.into(),value.into());
        let r=c.get(format!("{}/oauth/authorize?{}",h.base,encoded(&p))).send().await.unwrap();
        assert_eq!(r.status(),302);
        let url=url::Url::parse(r.headers()["location"].to_str().unwrap()).unwrap();
        let p:BTreeMap<_,_>=url.query_pairs().into_owned().collect();
        assert_eq!(p["error"],error);
        assert_eq!(p["iss"],ORIGIN);
    }
    let duplicate=c.get(format!("{}/oauth/authorize?{}&client_id={}",h.base,encoded(&good),CLIENT)).send().await.unwrap();
    assert_eq!(duplicate.status(),400);
    assert!(duplicate.headers().get("location").is_none());
    let malformed=c.get(format!("{}/oauth/authorize?{}&state=%zz",h.base,encoded(&good))).send().await.unwrap();
    assert_eq!(malformed.status(),400);
    let r=c.post(format!("{}/oauth/token",h.base)).header("content-type","application/x-www-form-urlencoded").body("grant_type=refresh_token&grant_type=authorization_code").send().await.unwrap();
    assert_eq!(r.status(),400);
    assert_eq!(r.json::<Value>().await.unwrap(),json!({"error":"invalid_request"}));
    let r=c.post(format!("{}/oauth/token",h.base)).header("origin","https://elsewhere.test").header("content-type","application/x-www-form-urlencoded").body("grant_type=refresh_token").send().await.unwrap();
    assert_eq!(r.status(),403);
    h.stop().await;
}

#[tokio::test]
async fn browser_origin_schema_and_cross_account_grant_ownership() {
    let h=Harness::new().await;
    let c=local_client();
    h.setup().await;
    let cookie=h.login("root.admin",PASSWORD).await;
    let member_identity=h.member(&cookie,"generated.member").await;
    let member=h.login("generated.member",PASSWORD).await;
    let verifier=crypto::random_token().unwrap();
    let handle=begin(&h,&c,&verifier).await;
    for path in ["/api/mcp/authorization/read","/api/mcp/authorization/decide"] {
        let response=c.post(format!("{}{path}",h.base)).header("cookie",&cookie).json(&json!({"request":handle,"decision":"read"})).send().await.unwrap();
        assert_eq!(response.status(),403);
    }
    let r=h.post("/api/mcp/authorization/read",json!({"request":handle}),Some(&cookie)).await;
    assert_eq!(r.status(),200);
    assert_eq!(h.post("/api/mcp/authorization/read",json!({"request":handle}),Some(&member)).await.status(),403);
    let r=h.post("/api/mcp/authorization/decide",json!({"request":handle,"decision":"read"}),Some(&cookie)).await;
    assert_eq!(r.status(),200);
    let body:Value=r.json().await.unwrap();
    let url=url::Url::parse(body["data"]["redirect_to"].as_str().unwrap()).unwrap();
    let p:BTreeMap<_,_>=url.query_pairs().into_owned().collect();
    assert_eq!(token(&h,&c,&p["code"],&verifier).await.status(),200);
    let grants:Value=h.get("/api/me/mcp-grants",Some(&cookie)).await.json().await.unwrap();
    let grant=grants["data"]["grants"][0]["id"].as_str().unwrap();
    let foreign=h.post(&format!("/api/me/mcp-grants/{grant}/revoke"),json!({}),Some(&member)).await;
    let absent=h.post("/api/me/mcp-grants/absent/revoke",json!({}),Some(&member)).await;
    assert_eq!(foreign.status(),absent.status());
    let expected=json!({"data":{"account_id":member_identity["id"]}});
    assert_eq!(foreign.json::<Value>().await.unwrap(),expected);
    assert_eq!(absent.json::<Value>().await.unwrap(),expected);
    let member_grants:Value=h.get("/api/me/mcp-grants",Some(&member)).await.json().await.unwrap();
    assert_eq!(member_grants,json!({"data":{"account_id":member_identity["id"],"grants":[]}}));
    let grants:Value=h.get("/api/me/mcp-grants",Some(&cookie)).await.json().await.unwrap();
    assert_eq!(grants["data"]["grants"].as_array().unwrap().len(),1);
    assert_eq!(h.get("/oauth/consent/",None).await.status(),404);
    assert_eq!(h.get("/api/mcp/authorization/read",Some(&cookie)).await.status(),404);
    h.stop().await;
}

#[tokio::test]
async fn consent_read_identifies_bound_account_and_session_switch_never_redirects() {
    let h=Harness::new().await;
    let c=local_client();
    h.setup().await;
    let admin=h.login("root.admin",PASSWORD).await;
    h.member(&admin,"generated.member").await;
    let member=h.login("generated.member",PASSWORD).await;
    let verifier=crypto::random_token().unwrap();
    // A screen may have looked up the old identity before the cookie changed.
    let old:Value=h.get("/api/me",Some(&admin)).await.json().await.unwrap();
    assert_eq!(old["data"]["username"],"root.admin");
    let handle=begin(&h,&c,&verifier).await;
    let read=h.post("/api/mcp/authorization/read",json!({"request":handle}),Some(&member)).await;
    assert_eq!(read.status(),200);
    assert_eq!(read.json::<Value>().await.unwrap()["data"]["username"],"generated.member");
    // Neither another account nor a new session for the same account owns the
    // pending request. Both Allow and Deny fail locally without any callback.
    let replacement=h.login("generated.member",PASSWORD).await;
    for cookie in [&admin,&replacement] {
        for decision in ["read","deny"] {
            let response=c.post(format!("{}/api/mcp/authorization/decide",h.base))
                .header("origin",ORIGIN).header("cookie",cookie)
                .json(&json!({"request":handle,"decision":decision})).send().await.unwrap();
            assert_eq!(response.status(),403);
            assert!(response.headers().get("location").is_none());
            assert_eq!(response.json::<Value>().await.unwrap(),json!({"error":{"code":"capability_invalid"}}));
        }
        let listing:Value=h.get("/api/me/mcp-grants",Some(cookie)).await.json().await.unwrap();
        assert_eq!(listing["data"]["grants"],json!([]));
    }
    h.stop().await;
}

async fn grant(h: &Harness, c: &Client, cookie: &str, write: bool, claude: bool) -> Value {
    let verifier=crypto::random_token().unwrap();
    let mut p=authorization(&verifier,true);
    let client=if claude {"https://claude.ai/oauth/mcp-oauth-client-metadata"} else {CLIENT};
    p.insert("client_id".into(),client.into());
    if claude {p.insert("redirect_uri".into(),"https://claude.ai/api/mcp/auth_callback".into());}
    // Unknown extensions, including repeated ones, do not change authority.
    let r=c.get(format!("{}/oauth/authorize?{}&extension=one&extension=two",h.base,encoded(&p))).send().await.unwrap();
    assert_eq!(r.status(),302);
    let handle=r.headers()["location"].to_str().unwrap().split_once("#request=").unwrap().1.to_owned();
    assert_eq!(h.post("/api/mcp/authorization/read",json!({"request":handle}),Some(cookie)).await.status(),200);
    let r=h.post("/api/mcp/authorization/decide",json!({"request":handle,"decision":if write {"read_write"} else {"read"}}),Some(cookie)).await;
    assert_eq!(r.status(),200);
    let decision:Value=r.json().await.unwrap();
    let redirect=url::Url::parse(decision["data"]["redirect_to"].as_str().unwrap()).unwrap();
    let p:BTreeMap<_,_>=redirect.query_pairs().into_owned().collect();
    let exchange:BTreeMap<String,String>=[("grant_type","authorization_code".into()),("client_id",client.into()),
        ("code",p["code"].clone()),("code_verifier",verifier),("resource",format!("{ORIGIN}/mcp")),("extension","ignored".into())]
        .into_iter().map(|(k,v)|(k.into(),v)).collect();
    let r=c.post(format!("{}/oauth/token",h.base)).header("content-type","application/x-www-form-urlencoded")
        .body(encoded(&exchange)).send().await.unwrap();
    assert_eq!(r.status(),200);
    r.json().await.unwrap()
}
fn mcp_request(h: &Harness, c: &Client, token: &str, method: &str, params: Value) -> reqwest::RequestBuilder {
    c.post(format!("{}/mcp",h.base)).bearer_auth(token).header("mcp-protocol-version","2025-11-25")
        .json(&json!({"jsonrpc":"2.0","id":"generated-request","method":method,"params":params}))
}
async fn native_exchange(h: &Harness, c: &Client, method: &str, key: &str, body: Value) -> Value {
    let r=c.post(format!("{}/sync/{method}",h.base)).header("content-type","application/octet-stream")
        .header("anki-sync",json!({"v":11,"k":key,"c":"synthetic","s":"mcpmount"}).to_string())
        .body(zstd::stream::encode_all(serde_json::to_vec(&body).unwrap().as_slice(),0).unwrap())
        .timeout(std::time::Duration::from_secs(15))
        .send().await.unwrap();
    assert_eq!(r.status(),200,"/sync/{method}");
    serde_json::from_slice(&zstd::stream::decode_all(r.bytes().await.unwrap().as_ref()).unwrap()).unwrap()
}

async fn note_write_tool(h: &Harness, c: &Client, access: &str, name: &str, arguments: Value) -> Value {
    let response=mcp_request(h,c,access,"tools/call",json!({"name":name,"arguments":arguments}))
        .timeout(std::time::Duration::from_secs(15)).send().await.unwrap();
    assert_eq!(response.status(),200);
    let body:Value=response.json().await.unwrap();
    assert_eq!(body["result"]["isError"],false,"{body}");
    let text:Value=serde_json::from_str(body["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["result"]["structuredContent"],text);
    text
}

// Complete W6–W12 receiver round for the generated one-note fixture below.
// Return admission meta: a native client retains this USN for its next round.
async fn note_write_round(h: &Harness, c: &Client, key: &str, min_usn: &Value, cards: u32) -> (Value,Value,Value) {
    let meta=native_exchange(h,c,"meta",key,json!({"v":11,"cv":"synthetic"})).await;
    assert_eq!(native_exchange(h,c,"start",key,json!({"minUsn":min_usn,"lnewer":false,"graves":null})).await,
        json!({"cards":[],"decks":[],"notes":[]}));
    let changes=native_exchange(h,c,"applyChanges",key,json!({"changes":{"models":[],"decks":[[],[]],"tags":[]}})).await;
    // This fixture has at most three outgoing rows, so its first chunk ends the stream.
    let chunk=native_exchange(h,c,"chunk",key,json!({})).await;
    assert_eq!(chunk["done"],true);
    assert_eq!(chunk["revlog"],json!([]));
    assert_eq!(native_exchange(h,c,"applyChunk",key,json!({"chunk":{"done":true}})).await,Value::Null);
    let counts=json!([[0,0,0],cards,1,0,0,1,1,1]);
    let sanity=native_exchange(h,c,"sanityCheck2",key,json!({"client":counts})).await;
    assert_eq!(sanity,json!({"status":"ok","c":counts,"s":counts}));
    let finish=native_exchange(h,c,"finish",key,json!({})).await;
    assert!(finish.as_i64().unwrap()>=meta["mod"].as_i64().unwrap());
    let after=native_exchange(h,c,"meta",key,json!({"v":11,"cv":"synthetic"})).await;
    assert_eq!(after["mod"],finish);
    assert_eq!(after["usn"].as_i64().unwrap(),meta["usn"].as_i64().unwrap()+1);
    (meta,changes,chunk)
}

#[tokio::test]
async fn production_mcp_create_and_card_generating_edit_reach_native_incremental_sync() {
    use meguri_model::proto::{ProtoBlob, deck::{DeckCommon,DeckKind,DeckKindMessage,NormalDeck},
        deck_config::DeckConfigMessage, notetype::{FieldConfig,NotetypeConfig,TemplateConfig}};
    use meguri_sync_server::collection::schema;
    use rusqlite::{Connection,params,types::ValueRef};

    // Read committed rows independently. Only W9's derived-column placeholders
    // differ from storage; the cards are compared in full W8 column order.
    fn rows(db: &Connection, sql: &str) -> Vec<Value> {
        let mut q=db.prepare(sql).unwrap();
        let columns=q.column_count();
        q.query_map([],|r| {
            Ok(Value::Array((0..columns).map(|i|match r.get_ref(i).unwrap() {
                ValueRef::Integer(n)=>json!(n),
                ValueRef::Text(s)=>json!(std::str::from_utf8(s).unwrap()),
                other=>panic!("unexpected fixture column: {other:?}"),
            }).collect()))
        }).unwrap().map(|r|r.unwrap()).collect()
    }
    fn stamps(db: &Connection) -> (i64,i64,i64,i64,i64) {
        db.query_row("SELECT mod,usn,crt,scm,ls FROM col",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap()
    }
    const NOTES: &str="SELECT id,guid,mid,mod,usn,tags,flds,'','',0,'' FROM notes ORDER BY id";
    const CARDS: &str="SELECT * FROM cards ORDER BY ord";

    let h=Harness::new().await;
    let c=local_client();
    let identity=h.setup().await;
    let cookie=h.login("root.admin",PASSWORD).await;
    let tokens=grant(&h,&c,&cookie,true,false).await;
    let access=tokens["access_token"].as_str().unwrap();
    let account=h.path().join("state/accounts").join(identity["id"].as_str().unwrap());
    let pointer=std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation=pointer.lines().find_map(|line|line.strip_prefix("generation=")).unwrap();
    let path=account.join("generations").join(generation).join("collection.anki2");
    // First-party protobuf fixtures, as in collection/mcp/tests.rs. No backend
    // replacement, test runtime or alternate MCP mount enables these writes.
    {
        let mut db=schema::open(&path,true).unwrap();
        let tx=db.transaction().unwrap();
        let common=ProtoBlob::new(DeckCommon::default()).to_bytes().into_owned();
        let mut normal=NormalDeck::default();normal.config_id=1;
        let kind=ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))).to_bytes().into_owned();
        tx.execute("INSERT INTO decks VALUES(1,'Generated deck',0,0,?1,?2)",params![common,kind]).unwrap();
        let mut preset=DeckConfigMessage::default();preset.new_card_insert_order=1;
        tx.execute("INSERT INTO deck_config VALUES(1,'Generated preset',0,0,?1)",[ProtoBlob::new(preset).to_bytes().into_owned()]).unwrap();
        tx.execute("INSERT INTO notetypes VALUES(10,'Generated type',0,0,?1)",[ProtoBlob::new(NotetypeConfig::default()).to_bytes().into_owned()]).unwrap();
        for (ord,name) in [(0,"Front"),(1,"Back"),(2,"Reverse")] {
            tx.execute("INSERT INTO fields VALUES(10,?1,?2,?3)",params![ord,name,ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned()]).unwrap();
        }
        for (ord,front) in [(0,"{{Front}}"),(1,"{{#Reverse}}{{Back}}{{/Reverse}} ")] {
            let mut template=TemplateConfig::default();template.q_format=front.into();
            tx.execute("INSERT INTO templates VALUES(10,?1,?2,0,0,?3)",params![ord,format!("Generated {ord}"),ProtoBlob::new(template).to_bytes().into_owned()]).unwrap();
        }
        tx.execute("INSERT INTO config VALUES('nextPos',0,0,?1)",[b"1".as_slice()]).unwrap();
        tx.commit().unwrap();
    }
    let db=schema::open(&path,false).unwrap();
    let key=native_exchange(&h,&c,"hostKey","",json!({"u":"root.admin","p":PASSWORD})).await["key"].as_str().unwrap().to_owned();
    let baseline=native_exchange(&h,&c,"meta",&key,json!({"v":11,"cv":"synthetic"})).await;
    let before=stamps(&db);
    assert_eq!(baseline["usn"],before.1);
    assert_eq!(baseline["mod"],before.0);
    let created=note_write_tool(&h,&c,access,"create_note",json!({"deck_id":"1","note_type_id":"10",
        "fields":{"Front":"cafe\u{301}","Back":"generated answer"},"tags":["generated"]})).await;
    let note_id=created["note_id"].as_str().unwrap().parse::<i64>().unwrap();
    let first_notes=rows(&db,NOTES);
    let first_cards=rows(&db,CARDS);
    assert_eq!(first_notes.len(),1);
    assert_eq!(first_cards.len(),1);
    assert_eq!(first_notes[0][0],note_id);
    assert_eq!(first_notes[0][2],10);
    assert_eq!(first_notes[0][3],note_id/1000);
    assert_eq!(first_notes[0][4],baseline["usn"]);
    assert_eq!(first_notes[0][5]," generated ");
    assert_eq!(first_notes[0][6],"café\u{1f}generated answer\u{1f}");
    let card_id=first_cards[0][0].as_i64().unwrap();
    assert!(card_id>0);
    assert_eq!(first_cards[0],json!([card_id,note_id,1,0,first_notes[0][3],baseline["usn"],0,0,825,0,0,0,0,0,0,0,0,"{}"]));
    let created_stamps=stamps(&db);
    assert!(created_stamps.0>before.0);
    assert_eq!(created_stamps.1,before.1+1);
    assert_eq!((created_stamps.2,created_stamps.3,created_stamps.4),(before.2,before.3,before.4));

    // The write stamped rows with baseline U, not the now-incremented col.usn.
    // Start inclusively at U to prove the normal receiver actually gets them.
    let (first_meta,changes,chunk)=note_write_round(&h,&c,&key,&baseline["usn"],1).await;
    assert_eq!(chunk["notes"],json!(first_notes));
    assert_eq!(chunk["cards"],json!(first_cards));
    assert_eq!(changes["models"][0]["id"],10);
    assert_eq!(changes["models"].as_array().unwrap().len(),1);
    assert_eq!(changes["decks"][0].as_array().unwrap().len(),1);
    assert_eq!(changes["decks"][1].as_array().unwrap().len(),1);
    assert_eq!(changes["decks"][0][0]["id"],1);
    assert_eq!(changes["decks"][1][0]["id"],1);
    assert_eq!(changes["tags"],json!(["generated"]));
    assert_eq!(changes["conf"]["nextPos"],2);

    let before_edit=stamps(&db);
    let edited=note_write_tool(&h,&c,access,"edit_note",json!({"note_id":created["note_id"],"revision":created["revision"],
        "fields":{"Reverse":"yes"},"add_tags":["expanded"],"remove_tags":["generated"]})).await;
    assert_eq!(edited["note_id"],created["note_id"]);
    assert_ne!(edited["revision"],created["revision"]);
    let edited_notes=rows(&db,NOTES);
    let edited_cards=rows(&db,CARDS);
    assert_eq!(edited_notes.len(),1);
    assert_eq!(edited_notes[0][0],note_id);
    assert_eq!(edited_notes[0][1],first_notes[0][1]);
    assert_eq!(edited_notes[0][4],before_edit.1);
    assert_eq!(edited_notes[0][5]," expanded ");
    assert_eq!(edited_notes[0][6],"café\u{1f}generated answer\u{1f}yes");
    assert_eq!(edited_cards.len(),2);
    assert_eq!(edited_cards[0],first_cards[0]);
    let new_card_id=edited_cards[1][0].as_i64().unwrap();
    assert_ne!(new_card_id,card_id);
    assert_eq!(edited_cards[1],json!([new_card_id,note_id,1,1,edited_notes[0][3],before_edit.1,0,0,825,0,0,0,0,0,0,0,0,"{}"]));
    let edited_stamps=stamps(&db);
    assert!(edited_stamps.0>before_edit.0);
    assert_eq!(edited_stamps.1,before_edit.1+1);
    assert_eq!((edited_stamps.2,edited_stamps.3,edited_stamps.4),(before_edit.2,before_edit.3,before_edit.4));
    let (_,changes,chunk)=note_write_round(&h,&c,&key,&first_meta["usn"],2).await;
    assert_eq!(chunk["notes"],json!(edited_notes));
    assert_eq!(chunk["cards"],json!([edited_cards[1]]));
    assert_eq!(changes["models"],json!([]));
    assert_eq!(changes["decks"],json!([[],[]]));
    assert_eq!(changes["tags"],json!(["expanded"]));
    assert_eq!(changes["conf"]["nextPos"],2);
    assert_eq!(rows(&db,CARDS),edited_cards);
    assert_eq!(rows(&db,NOTES),edited_notes);
    let finished=stamps(&db);
    assert_eq!(finished.1,edited_stamps.1+1);
    assert!(finished.0>=edited_stamps.0);
    assert_eq!(finished.4,finished.0);
    drop(db);
    h.stop().await;
}

#[tokio::test]
async fn mounted_mcp_uses_real_oauth_collection_and_preserves_native_and_browser_routes() {
    let h=Harness::new().await;
    let c=local_client();
    let identity=h.setup().await;
    let cookie=h.login("root.admin",PASSWORD).await;
    let tokens=grant(&h,&c,&cookie,true,false).await;
    let access=tokens["access_token"].as_str().unwrap();
    let list=mcp_request(&h,&c,access,"tools/list",json!({})).send().await.unwrap();
    assert_eq!(list.status(),200);
    assert_eq!(list.headers()["x-frame-options"],"DENY");
    assert_eq!(list.headers()["x-content-type-options"],"nosniff");
    assert_eq!(list.headers()["cache-control"],"no-store");
    assert_eq!(list.json::<Value>().await.unwrap()["result"]["tools"].as_array().unwrap().len(),5);
    for (tool,args) in [("list_decks_and_note_types",json!({})),("search_notes",json!({"note_id":"1"}))] {
        let r=mcp_request(&h,&c,access,"tools/call",json!({"name":tool,"arguments":args})).send().await.unwrap();
        assert_eq!(r.status(),200);
        let result:Value=r.json().await.unwrap();
        assert_eq!(result["result"]["isError"],false);
        let text:Value=serde_json::from_str(result["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(result["result"]["structuredContent"],text);
        if tool=="search_notes" {assert_eq!(text["notes"],json!([]));}
        else {assert!(text["decks"].is_array());assert!(text["note_types"].is_array());}
    }
    let get=c.get(format!("{}/mcp",h.base)).send().await.unwrap();
    assert_eq!(get.status(),405);
    assert_eq!(get.headers()["allow"],"POST");
    assert_eq!(h.get("/api/me",Some(&cookie)).await.json::<Value>().await.unwrap()["data"]["id"],identity["id"]);
    for path in ["/", "/connection", "/oauth/consent"] {
        let r=c.get(format!("{}{path}",h.base)).send().await.unwrap();
        assert_eq!(r.status(),200,"SPA mount {path}");
        assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/html"));
    }
    let call=json!({"name":"edit_note","arguments":{"note_id":"1","revision":"generated","fields":{}}});
    for claude in [false,true] {
        let read=grant(&h,&c,&cookie,false,claude).await;
        let r=mcp_request(&h,&c,read["access_token"].as_str().unwrap(),"tools/call",call.clone()).send().await.unwrap();
        if claude {
            assert_eq!(r.status(),403);
            assert!(r.headers()["www-authenticate"].to_str().unwrap().contains("insufficient_scope"));
        } else {
            assert_eq!(r.status(),200);
            let result:Value=r.json().await.unwrap();
            assert_eq!(result["result"]["isError"],true);
            assert!(result["result"]["_meta"]["mcp/www_authenticate"][0].as_str().unwrap().contains("scope=\"collection:read collection:write\""));
        }
    }
    // A real retained native writer owns BEGIN IMMEDIATE between HTTP calls.
    // MCP must refuse before treating the missing note/revision or touching data.
    let key=native_exchange(&h,&c,"hostKey","",json!({"u":"root.admin","p":PASSWORD})).await["key"].as_str().unwrap().to_owned();
    let meta=native_exchange(&h,&c,"meta",&key,json!({"v":11,"cv":"synthetic"})).await;
    native_exchange(&h,&c,"start",&key,json!({"minUsn":meta["usn"],"lnewer":false,"graves":null})).await;
    let account=h.path().join("state/accounts").join(identity["id"].as_str().unwrap());
    let pointer=std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation=pointer.lines().find_map(|line|line.strip_prefix("generation=")).unwrap();
    let db=rusqlite::Connection::open_with_flags(account.join("generations").join(generation).join("collection.anki2"),rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let state=|| db.query_row("SELECT mod,usn,(SELECT count(*) FROM notes),(SELECT count(*) FROM cards) FROM col",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?))).unwrap();
    let before=state();
    let busy=mcp_request(&h,&c,access,"tools/call",call).send().await.unwrap();
    assert_eq!(busy.status(),200);
    let busy:Value=busy.json().await.unwrap();
    assert_eq!(busy["result"]["structuredContent"]["error"]["code"],"busy");
    assert_eq!(busy["result"]["structuredContent"]["error"]["write_committed"],false);
    assert_eq!(state(),before);
    assert_eq!(native_exchange(&h,&c,"abort",&key,json!({})).await,Value::Null);
    assert_eq!(state(),before);
    drop(db);
    let grants:Value=h.get("/api/me/mcp-grants",Some(&cookie)).await.json().await.unwrap();
    let grant=grants["data"]["grants"].as_array().unwrap().iter().find(|g|g["access"]=="read_write").unwrap()["id"].as_str().unwrap();
    assert_eq!(h.post(&format!("/api/me/mcp-grants/{grant}/revoke"),json!({}),Some(&cookie)).await.status(),200);
    let r=mcp_request(&h,&c,access,"tools/list",json!({})).send().await.unwrap();
    assert_eq!(r.status(),401);
    assert!(r.headers()["www-authenticate"].to_str().unwrap().contains("invalid_token"));
    assert!(!r.text().await.unwrap().contains(access));
    // Invalid and ambiguous credentials never trigger tool-level authorization.
    for authorization in ["Bearer invalid", "Basic invalid", "Bearer Bearer invalid"] {
        let r=c.post(format!("{}/mcp",h.base)).header("authorization",authorization)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).send().await.unwrap();
        assert_eq!(r.status(),401);
        assert!(r.headers()["www-authenticate"].to_str().unwrap().contains("invalid_token"));
        assert!(!r.text().await.unwrap().contains("mcp/www_authenticate"));
    }
    // Browser cookie authority never substitutes for a bearer.
    let r=c.post(format!("{}/mcp",h.base)).header("cookie",&cookie)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).send().await.unwrap();
    assert_eq!(r.status(),401);
    h.stop().await;
}

fn attachment_collection(h: &Harness, id: &str) -> std::path::PathBuf {
    use meguri_model::proto::{ProtoBlob,deck::{DeckCommon,DeckKind,DeckKindMessage,NormalDeck},deck_config::DeckConfigMessage,
        notetype::{FieldConfig,NotetypeConfig,TemplateConfig}};
    let account=h.path().join("state/accounts").join(id);
    let pointer=std::fs::read_to_string(account.join("current-collection")).unwrap();
    let generation=pointer.lines().find_map(|line|line.strip_prefix("generation=")).unwrap();
    let path=account.join("generations").join(generation).join("collection.anki2");
    let mut db=meguri_sync_server::collection::schema::open(&path,true).unwrap();let tx=db.transaction().unwrap();
    let mut normal=NormalDeck::default();normal.config_id=1;
    tx.execute("INSERT INTO decks VALUES(1,'Generated deck',0,0,?1,?2)",rusqlite::params![ProtoBlob::new(DeckCommon::default()).to_bytes().into_owned(),ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))).to_bytes().into_owned()]).unwrap();
    let mut preset=DeckConfigMessage::default();preset.new_card_insert_order=1;
    tx.execute("INSERT INTO deck_config VALUES(1,'Generated preset',0,0,?1)",[ProtoBlob::new(preset).to_bytes().into_owned()]).unwrap();
    tx.execute("INSERT INTO notetypes VALUES(10,'Generated type',0,0,?1)",[ProtoBlob::new(NotetypeConfig::default()).to_bytes().into_owned()]).unwrap();
    for (ord,name) in [(0,"Front"),(1,"Back")] {tx.execute("INSERT INTO fields VALUES(10,?1,?2,?3)",rusqlite::params![ord,name,ProtoBlob::new(FieldConfig::default()).to_bytes().into_owned()]).unwrap();}
    let mut template=TemplateConfig::default();template.q_format="{{Front}}".into();
    tx.execute("INSERT INTO templates VALUES(10,0,'Generated',0,0,?1)",[ProtoBlob::new(template).to_bytes().into_owned()]).unwrap();
    tx.execute("INSERT INTO config VALUES('nextPos',0,0,?1)",[b"1".as_slice()]).unwrap();tx.commit().unwrap();drop(db);path
}
async fn stage_attachment(h: &Harness, cookie: &str, id: &str, bytes: Vec<u8>) -> reqwest::Response {
    h.client.post(format!("{}/api/mcp/media-upload",h.base)).header("origin",ORIGIN).header("cookie",cookie)
        .header("x-meguri-upload",id).header("content-type","application/octet-stream").body(bytes).send().await.unwrap()
}
async fn attachment_media(h: &Harness, key: &str, method: &str, request: Value) -> Vec<u8> {
    let response=h.client.post(format!("{}/msync/{method}",h.base))
        .header("anki-sync",json!({"v":11,"k":key,"c":"synthetic-attachments","s":""}).to_string())
        .body(zstd::stream::encode_all(serde_json::to_vec(&request).unwrap().as_slice(),0).unwrap()).send().await.unwrap();
    assert_eq!(response.status(),200);
    zstd::stream::decode_all(response.bytes().await.unwrap().as_ref()).unwrap()
}
async fn attachment_media_json(h: &Harness, key: &str, method: &str, request: Value) -> Value {
    let result:Value=serde_json::from_slice(&attachment_media(h,key,method,request).await).unwrap();
    assert_eq!(result["err"],"");result["data"].clone()
}
async fn attachment_download(h: &Harness, key: &str, names: &[&str]) -> BTreeMap<String,Vec<u8>> {
    use std::io::Read;
    let bytes=attachment_media(h,key,"downloadFiles",json!({"files":names})).await;
    let mut archive=zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mapping:BTreeMap<String,String>={let file=archive.by_name("_meta").unwrap();serde_json::from_reader(file).unwrap()};
    let mut files=BTreeMap::new();
    for (member,name) in mapping {let mut bytes=Vec::new();archive.by_name(&member).unwrap().read_to_end(&mut bytes).unwrap();files.insert(name,bytes);}
    assert_eq!(files.len(),names.len());files
}
async fn attachment_failure(h: &Harness,c: &Client,access: &str,name: &str,args: Value,code: &str) {
    let response=mcp_request(h,c,access,"tools/call",json!({"name":name,"arguments":args})).send().await.unwrap();
    assert_eq!(response.status(),200);let body: Value=response.json().await.unwrap();
    assert_eq!(body["result"]["isError"],true);assert_eq!(body["result"]["structuredContent"]["error"]["code"],code);
}
#[tokio::test]
async fn mcp_browser_attachments_reach_native_notes_and_media_and_reuse_after_refresh() {
    let h=Harness::new().await;let c=local_client();let identity=h.setup().await;let cookie=h.login("root.admin",PASSWORD).await;
    let path=attachment_collection(&h,identity["id"].as_str().unwrap());
    let tokens=grant(&h,&c,&cookie,true,false).await;let access=tokens["access_token"].as_str().unwrap();
    let key=native_exchange(&h,&c,"hostKey","",json!({"u":"root.admin","p":PASSWORD})).await["key"].as_str().unwrap().to_owned();
    let before=native_exchange(&h,&c,"meta",&key,json!({"v":11,"cv":"synthetic"})).await;
    let picture=note_write_tool(&h,&c,access,"prepare_media_upload",json!({})).await;
    let audio=note_write_tool(&h,&c,access,"prepare_media_upload",json!({})).await;
    assert_eq!(picture["upload_url"],format!("{ORIGIN}/mcp/upload#upload={}",picture["upload_id"].as_str().unwrap()));
    assert_eq!(h.get("/mcp/upload",Some(&cookie)).await.status(),200);
    let png=attachment_png();let mp3=attachment_mp3();
    assert_eq!(stage_attachment(&h,&cookie,picture["upload_id"].as_str().unwrap(),png.clone()).await.status(),200);
    assert_eq!(stage_attachment(&h,&cookie,audio["upload_id"].as_str().unwrap(),mp3.clone()).await.status(),200);
    let metadata=rusqlite::Connection::open(h.database()).unwrap();
    let counts=||metadata.query_row("SELECT media_usn,(SELECT count(*) FROM media_names),(SELECT count(*) FROM media_blobs) FROM accounts WHERE id=?1",[identity["id"].as_str().unwrap()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?))).unwrap();
    assert_eq!(counts(),(0,0,0),"browser upload only stages bytes");
    let uploads=json!([{"upload_id":picture["upload_id"],"field":"Front"},{"upload_id":audio["upload_id"],"field":"Back"}]);
    let created=note_write_tool(&h,&c,access,"create_note",json!({"deck_id":"1","note_type_id":"10","fields":{"Front":"question","Back":"answer"},"uploads":uploads})).await;
    let read=note_write_tool(&h,&c,access,"search_notes",json!({"note_id":created["note_id"]})).await;
    let fields=&read["notes"][0]["fields"];
    let png_name=format!("meguri-{}.png",crypto::digest(&png).iter().map(|b|format!("{b:02x}")).collect::<String>());
    let mp3_name=format!("meguri-{}.mp3",crypto::digest(&mp3).iter().map(|b|format!("{b:02x}")).collect::<String>());
    assert_eq!(fields["Front"],format!("question<br><img src=\"{png_name}\">"));assert_eq!(fields["Back"],format!("answer<br>[sound:{mp3_name}]"));
    let (_,_,chunk)=note_write_round(&h,&c,&key,&before["usn"],1).await;
    assert_eq!(chunk["notes"][0][6],format!("{}\u{1f}{}",fields["Front"].as_str().unwrap(),fields["Back"].as_str().unwrap()));
    assert_eq!(counts(),(2,2,2));
    let listing=attachment_media_json(&h,&key,"mediaChanges",json!({"lastUsn":0})).await;
    assert_eq!(listing.as_array().unwrap().len(),2);
    let delivered=attachment_download(&h,&key,&[&png_name,&mp3_name]).await;
    assert_eq!(delivered[&png_name],png);assert_eq!(delivered[&mp3_name],mp3);
    assert_eq!(attachment_media_json(&h,&key,"mediaSanity",json!({"local":2})).await,"OK");
    // No-op attachment-only patch preserves the revision and media USNs.
    let noop=note_write_tool(&h,&c,access,"edit_note",json!({"note_id":created["note_id"],"revision":created["revision"],"uploads":uploads})).await;
    assert_eq!(noop,created);assert_eq!(counts(),(2,2,2));
    let edited=note_write_tool(&h,&c,access,"edit_note",json!({"note_id":created["note_id"],"revision":created["revision"],"fields":{"Front":"replacement"},"uploads":uploads})).await;
    assert_ne!(edited["revision"],created["revision"]);
    attachment_failure(&h,&c,access,"edit_note",json!({"note_id":created["note_id"],"revision":created["revision"],"uploads":uploads}),"conflict").await;
    assert_eq!(counts(),(2,2,2));
    let exchange:BTreeMap<String,String>=[("grant_type","refresh_token".into()),("client_id",CLIENT.into()),("refresh_token",tokens["refresh_token"].as_str().unwrap().into()),("resource",format!("{ORIGIN}/mcp"))].into_iter().map(|(k,v)|(k.into(),v)).collect();
    let rotated:Value=c.post(format!("{}/oauth/token",h.base)).header("content-type","application/x-www-form-urlencoded").body(encoded(&exchange)).send().await.unwrap().json().await.unwrap();
    note_write_tool(&h,&c,rotated["access_token"].as_str().unwrap(),"create_note",json!({"deck_id":"1","note_type_id":"10","fields":{"Front":"second"},"uploads":uploads})).await;
    assert_eq!(counts(),(2,2,2));
    assert_eq!(stage_attachment(&h,&cookie,picture["upload_id"].as_str().unwrap(),attachment_png()).await.status(),403);
    let db=meguri_sync_server::collection::schema::open(&path,false).unwrap();
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM notes",[],|r|r.get(0)).unwrap(),2);drop(db);drop(metadata);h.stop().await;
}
#[tokio::test]
async fn mcp_uploads_deny_other_accounts_grants_revocation_reset_and_unsupported_direct_clients() {
    let h=Harness::new().await;let c=local_client();let identity=h.setup().await;let cookie=h.login("root.admin",PASSWORD).await;
    attachment_collection(&h,identity["id"].as_str().unwrap());
    h.member(&cookie,"generated.other").await;let other=h.login("generated.other",PASSWORD).await;
    let first=grant(&h,&c,&cookie,true,false).await;let access=first["access_token"].as_str().unwrap();
    let upload=note_write_tool(&h,&c,access,"prepare_media_upload",json!({})).await;let id=upload["upload_id"].as_str().unwrap();
    assert_eq!(stage_attachment(&h,&other,id,attachment_png()).await.status(),403);
    assert_eq!(stage_attachment(&h,&cookie,id,b"unsupported synthetic svg".to_vec()).await.status(),400);
    assert_eq!(stage_attachment(&h,&cookie,id,attachment_png()).await.status(),200);
    let args=json!({"deck_id":"1","note_type_id":"10","fields":{},"uploads":[{"upload_id":id,"field":"Front"}]});
    let second=grant(&h,&c,&cookie,true,false).await;
    let other_grant=grant(&h,&c,&other,true,false).await;
    for token in [second["access_token"].as_str().unwrap(),other_grant["access_token"].as_str().unwrap()] {
        attachment_failure(&h,&c,token,"create_note",args.clone(),"attachment_unavailable").await;
    }
    let claude=grant(&h,&c,&cookie,true,true).await;
    attachment_failure(&h,&c,claude["access_token"].as_str().unwrap(),"create_note",json!({"deck_id":"1","note_type_id":"10","fields":{},"files":[{"download_url":"https://files.oaiusercontent.com/never-fetched","file_id":"fixture"}],"file_fields":["Front"]}),"attachment_unavailable").await;
    let pending=note_write_tool(&h,&c,access,"prepare_media_upload",json!({})).await;
    let grants=h.get("/api/me/mcp-grants",Some(&cookie)).await.json::<Value>().await.unwrap();
    // Revoke all of this generated account's grants through the real browser route.
    for grant in grants["data"]["grants"].as_array().unwrap() {
        assert_eq!(h.post(&format!("/api/me/mcp-grants/{}/revoke",grant["id"].as_str().unwrap()),json!({}),Some(&cookie)).await.status(),200);
    }
    assert_eq!(stage_attachment(&h,&cookie,pending["upload_id"].as_str().unwrap(),attachment_png()).await.status(),403);
    let refused=mcp_request(&h,&c,access,"tools/call",json!({"name":"create_note","arguments":args})).send().await.unwrap();assert_eq!(refused.status(),401);
    let fresh=grant(&h,&c,&cookie,true,false).await;
    let pending=note_write_tool(&h,&c,fresh["access_token"].as_str().unwrap(),"prepare_media_upload",json!({})).await;
    assert_eq!(h.post(&format!("/api/admin/accounts/{}/reset-data",identity["id"].as_str().unwrap()),json!({}),Some(&cookie)).await.status(),200);
    assert_eq!(stage_attachment(&h,&cookie,pending["upload_id"].as_str().unwrap(),attachment_png()).await.status(),403);
    let db=rusqlite::Connection::open(h.database()).unwrap();assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM media_names",[],|r|r.get(0)).unwrap(),0);drop(db);h.stop().await;
}

#[tokio::test]
async fn mcp_staged_uploads_are_discarded_on_server_restart() {
    let h=Harness::new().await;let c=local_client();let identity=h.setup().await;let cookie=h.login("root.admin",PASSWORD).await;
    attachment_collection(&h,identity["id"].as_str().unwrap());
    let tokens=grant(&h,&c,&cookie,true,false).await;let access=tokens["access_token"].as_str().unwrap();
    let pending=note_write_tool(&h,&c,access,"prepare_media_upload",json!({})).await;
    assert_eq!(stage_attachment(&h,&cookie,pending["upload_id"].as_str().unwrap(),attachment_png()).await.status(),200);
    let temp=h.stop().await;let h=Harness::reopen(temp).await;
    attachment_failure(&h,&c,access,"create_note",json!({"deck_id":"1","note_type_id":"10","fields":{},"uploads":[{"upload_id":pending["upload_id"],"field":"Front"}]}),"attachment_unavailable").await;
    let db=rusqlite::Connection::open(h.database()).unwrap();
    assert_eq!(db.query_row::<i64,_,_>("SELECT count(*) FROM media_blobs",[],|r|r.get(0)).unwrap(),0);drop(db);h.stop().await;
}
