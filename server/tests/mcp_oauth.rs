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
        .send().await.unwrap();
    assert_eq!(r.status(),200);
    serde_json::from_slice(&zstd::stream::decode_all(r.bytes().await.unwrap().as_ref()).unwrap()).unwrap()
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
    assert_eq!(list.json::<Value>().await.unwrap()["result"]["tools"].as_array().unwrap().len(),4);
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
