// SPDX-License-Identifier: MIT
use super::*;
use serde_json::Value;
use std::collections::BTreeMap;

const ORIGIN: &str = "https://oauth.example.test";
const CLIENT: &str = "https://chatgpt.com/oauth/client.json";
const REDIRECT: &str = "https://chatgpt.com/connector_platform_oauth_redirect";
fn params(items: &[(&str,&str)]) -> BTreeMap<String,String> {
    items.iter().map(|(k,v)| (k.to_string(),v.to_string())).collect()
}
fn request(write: bool, verifier: &str) -> Authorization {
    Authorization::parse(ORIGIN,&params(&[
        ("response_type","code"),("client_id",CLIENT),("redirect_uri",REDIRECT),
        ("resource",&format!("{ORIGIN}/mcp")),("scope",scopes(write)),
        ("code_challenge_method","S256"),("code_challenge",&policy::verifier(verifier).unwrap()),
        ("state","opaque + & = / ? # % 🦀"),
    ])).unwrap()
}
fn exchange(code: &str, verifier: &str) -> BTreeMap<String,String> {
    params(&[("grant_type","authorization_code"),("code",code),("client_id",CLIENT),
        ("resource",&format!("{ORIGIN}/mcp")),("code_verifier",verifier)])
}
fn refresh_request(token: &str) -> BTreeMap<String,String> {
    params(&[("grant_type","refresh_token"),("refresh_token",token),("resource",&format!("{ORIGIN}/mcp"))])
}
struct Fixture { _temp: tempfile::TempDir, s: Arc<Service>, a: BrowserAuthority, session: String }
impl Fixture {
    async fn new() -> Self {
        let temp=tempfile::tempdir().unwrap();
        let mut root=DataRoot::acquire(&temp.path().join("owner"),true).unwrap();
        root.init().unwrap();
        let setup=std::fs::read_to_string(root.setup_file()).unwrap().trim().to_owned();
        let s=Service::open(root,Config::new(ORIGIN,"127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
        let ip="127.0.0.1".parse().unwrap();
        s.claim(ip,"setup",setup,"generated.admin","generated password for oauth".into()).await.unwrap();
        let (_,session)=s.browser_login(ip,"generated.admin","generated password for oauth".into()).await.unwrap();
        let a=s.browser_authority(&session).await.unwrap();
        Self {_temp:temp,s,a,session}
    }
    async fn pending(&self, write: bool, verifier: &str) -> String {
        let url=self.s.oauth_begin(request(write,verifier)).await.unwrap();
        url.split_once("#request=").unwrap().1.to_owned()
    }
    async fn code(&self, write: bool, verifier: &str) -> String {
        let handle=self.pending(write,verifier).await;
        let read=self.s.oauth_consent_read(self.a.clone(),&handle).await.unwrap();
        assert_eq!(read.username,self.a.identity().username);
        assert_eq!(read.access_options,if write {vec!["read","read_write"]} else {vec!["read"]});
        let redirect=self.s.oauth_consent_decide(self.a.clone(),&handle,if write {"read_write"} else {"read"}).await.unwrap();
        let url=url::Url::parse(&redirect).unwrap();
        let p:BTreeMap<_,_>=url.query_pairs().into_owned().collect();
        assert_eq!(p["iss"],ORIGIN);
        assert_eq!(p["state"],"opaque + & = / ? # % 🦀");
        p["code"].clone()
    }
    async fn tokens(&self, write: bool) -> (String,Value) {
        let verifier=crypto::random_token().unwrap();
        let code=self.code(write,&verifier).await;
        let result=self.s.oauth_token(exchange(&code,&verifier)).await.unwrap();
        (verifier,serde_json::to_value(result).unwrap())
    }
}

#[test]
fn strict_policy_and_no_url_aliases_or_duplicate_parameters() {
    let verifier=crypto::random_token().unwrap();
    let r=request(true,&verifier);
    assert_eq!(policy::verifier(&verifier).unwrap(),r.challenge);
    for value in ["https://OAUTH.EXAMPLE.TEST/mcp","HTTPS://oauth.example.test/mcp"] {
        assert_eq!(resource(ORIGIN,value).unwrap(),format!("{ORIGIN}/mcp"));
    }
    for value in ["https://oauth.example.test:443/mcp","https://oauth.example.test/a/../mcp","https://oauth.example.test/%6dcp","https://oauth.example.test/mcp/","https://oauth.example.test/mcp?x","https://oauth.example.test/mcp#x","https://@oauth.example.test/mcp","https://oauth.example.test.evil/mcp","http://oauth.example.test/mcp"] {
        assert!(resource(ORIGIN,value).is_err(),"{value}");
    }
    assert!(resource("null","https://oauth.example.test/mcp").is_err());
    for value in ["collection:write","collection:read collection:read","collection:read  collection:write","openid",""] {
        assert!(policy::scope(value).is_err());
    }
    assert!(policy::scope("collection:write collection:read").unwrap());
    for bytes in [b"client_id=1&client_id=2".as_slice(),b"a=%",b"a=%zz",b"a=%ff",b"a=%00",b"state=1&%73tate=2",b"a=1&&b=2"] {
        assert!(form(bytes).is_err());
    }
    assert_eq!(form(b"state=a%2Bb+c%26d").unwrap()["state"],"a+b c&d");
    assert!(form(b"extension=one&extension=two").unwrap().is_empty());
    let mut extended = params(&[("response_type","code"),("client_id",CLIENT),("redirect_uri",REDIRECT),
        ("resource",&format!("{ORIGIN}/mcp")),("code_challenge_method","S256"),("code_challenge",&r.challenge)]);
    extended.insert("extension".into(),"ignored".into());
    assert!(Authorization::parse(ORIGIN,&extended).is_ok());
    for id in ["https://chatgpt.com.evil/oauth/client.json","https://chatgpt.com/oauth/client.json#x","https://user@chatgpt.com/oauth/client.json"] {
        assert!(client(id).is_err());
    }
    assert!(policy::verifier("short").is_err());
    assert!(policy::challenge("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").is_err());
}

#[tokio::test]
async fn consent_binds_exact_session_and_consumes_once_including_denial_and_expiry() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let h=f.pending(true,&verifier).await;
    assert_eq!(f.s.oauth_consent_decide(f.a.clone(),&h,"read").await.err().unwrap().code,"capability_invalid");
    let consent=f.s.oauth_consent_read(f.a.clone(),&h).await.unwrap();
    assert_eq!(consent.username,"generated.admin");
    let (_,other)=f.s.browser_login("127.0.0.1".parse().unwrap(),"generated.admin","generated password for oauth".into()).await.unwrap();
    let other=f.s.browser_authority(&other).await.unwrap();
    assert_eq!(f.s.oauth_consent_read(other.clone(),&h).await.err().unwrap().code,"capability_invalid");
    assert_eq!(f.s.oauth_consent_decide(other,&h,"read").await.err().unwrap().code,"capability_invalid");
    f.s.metadata.run(|c| {
        assert_eq!(c.query_row("SELECT count(*) FROM oauth_codes",[],|r|r.get::<_,i64>(0))?,0);
        assert_eq!(c.query_row("SELECT count(*) FROM oauth_grants",[],|r|r.get::<_,i64>(0))?,0);
        Ok(())
    }).await.unwrap();
    let (one,two)=tokio::join!(f.s.oauth_consent_decide(f.a.clone(),&h,"deny"),f.s.oauth_consent_decide(f.a.clone(),&h,"read"));
    assert_eq!(usize::from(one.is_ok())+usize::from(two.is_ok()),1);
    assert!(f.s.oauth_consent_read(f.a.clone(),&h).await.is_err());
    // Explicit deny has no code/grant; no GET or read approves anything.
    let deny=f.pending(false,&verifier).await;
    f.s.oauth_consent_read(f.a.clone(),&deny).await.unwrap();
    let url=f.s.oauth_consent_decide(f.a.clone(),&deny,"deny").await.unwrap();
    let query:BTreeMap<_,_>=url::Url::parse(&url).unwrap().query_pairs().into_owned().collect();
    assert_eq!(query["error"],"access_denied");
    assert!(!query.contains_key("code"));
    assert!(f.s.oauth_grants(f.a.clone()).await.unwrap().grants.is_empty());
    let expired=f.pending(false,&verifier).await;
    let hash=crypto::bearer(&expired).unwrap();
    f.s.metadata.run(move |c| {c.execute("UPDATE oauth_pending SET expires_at=?2 WHERE hash=?1",params![hash.as_slice(),now()])?;Ok(())}).await.unwrap();
    assert!(f.s.oauth_consent_read(f.a.clone(),&expired).await.is_err());
    let stale=f.pending(false,&verifier).await;
    f.s.oauth_consent_read(f.a.clone(),&stale).await.unwrap();
    f.s.logout(&f.session).await.unwrap();
    assert_eq!(f.s.oauth_consent_decide(f.a.clone(),&stale,"read").await.err().unwrap().code,"authentication_failed");
}

#[tokio::test]
async fn pkce_code_replays_are_bound_and_revoke_only_the_issued_grant() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let code=f.code(true,&verifier).await;
    let mut wrong=exchange(&code,&crypto::random_token().unwrap());
    assert_eq!(f.s.oauth_token(wrong.clone()).await.err().unwrap().code,"invalid_grant");
    let tokens=serde_json::to_value(f.s.oauth_token(exchange(&code,&verifier)).await.unwrap()).unwrap();
    let access=tokens["access_token"].as_str().unwrap();
    let a=f.s.mcp_authority(access,true).await.unwrap();
    assert!(a.can_write());
    assert_eq!(a.client_id(),CLIENT);
    // A different redirect and verifier cannot use replay as a revocation oracle.
    wrong=exchange(&code,&verifier);
    wrong.insert("redirect_uri".into(),format!("{REDIRECT}/"));
    assert_eq!(f.s.oauth_token(wrong).await.err().unwrap().code,"invalid_grant");
    f.s.mcp_recheck(a.clone(),true).await.unwrap();
    assert!(f.s.oauth_token(exchange(&code,&verifier)).await.is_err());
    assert_eq!(f.s.mcp_recheck(a,true).await.err().unwrap().code,"invalid_token");
    assert!(f.s.oauth_token(refresh_request(tokens["refresh_token"].as_str().unwrap())).await.is_err());
    // Omitted redirect is valid, supplied exact redirect is valid too.
    let next=f.code(false,&verifier).await;
    let mut p=exchange(&next,&verifier); p.insert("redirect_uri".into(),REDIRECT.into());
    let next=serde_json::to_value(f.s.oauth_token(p).await.unwrap()).unwrap();
    assert_eq!(next["scope"],READ);
    assert_eq!(f.s.mcp_authority(next["access_token"].as_str().unwrap(),true).await.err().unwrap().code,"insufficient_scope");
    assert!(f.s.mcp_authority(next["access_token"].as_str().unwrap(),false).await.is_ok());
    // OAuth credentials are rejected by both preexisting credential validators.
    assert!(f.s.browser_authority(access).await.is_err());
    assert!(f.s.validate_hostkey(access).await.is_err());
    assert!(f.s.mcp_authority(&f.session,false).await.is_err());
}

#[tokio::test]
async fn concurrent_code_and_refresh_use_is_transactional_and_refresh_replay_is_bound() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let code=f.code(true,&verifier).await;
    let (one,two)=tokio::join!(f.s.oauth_token(exchange(&code,&verifier)),f.s.oauth_token(exchange(&code,&verifier)));
    assert_eq!(usize::from(one.is_ok())+usize::from(two.is_ok()),1);
    let winner=serde_json::to_value(one.or(two).unwrap()).unwrap();
    // The second valid code use revokes the winning grant before responding.
    assert!(f.s.mcp_authority(winner["access_token"].as_str().unwrap(),false).await.is_err());
    let (_,tokens)=f.tokens(true).await;
    let refresh=tokens["refresh_token"].as_str().unwrap();
    let rotated=serde_json::to_value(f.s.oauth_token(refresh_request(refresh)).await.unwrap()).unwrap();
    assert_ne!(tokens["refresh_token"],rotated["refresh_token"]);
    let mut wrong=refresh_request(refresh);
    wrong.insert("client_id".into(),"https://claude.ai/oauth/mcp-oauth-client-metadata".into());
    assert!(f.s.oauth_token(wrong).await.is_err());
    f.s.mcp_authority(rotated["access_token"].as_str().unwrap(),true).await.unwrap();
    let mut wrong=refresh_request(refresh);
    wrong.insert("resource".into(),"https://other.example.test/mcp".into());
    assert!(f.s.oauth_token(wrong).await.is_err());
    f.s.mcp_authority(rotated["access_token"].as_str().unwrap(),true).await.unwrap();
    assert!(f.s.oauth_token(refresh_request(refresh)).await.is_err());
    assert!(f.s.mcp_authority(rotated["access_token"].as_str().unwrap(),false).await.is_err());
    assert!(f.s.oauth_token(refresh_request(rotated["refresh_token"].as_str().unwrap())).await.is_err());
    let (_,tokens)=f.tokens(false).await;
    let refresh=tokens["refresh_token"].as_str().unwrap();
    let (one,two)=tokio::join!(f.s.oauth_token(refresh_request(refresh)),f.s.oauth_token(refresh_request(refresh)));
    assert_eq!(usize::from(one.is_ok())+usize::from(two.is_ok()),1);
    let winner=serde_json::to_value(one.or(two).unwrap()).unwrap();
    assert!(f.s.mcp_authority(winner["access_token"].as_str().unwrap(),false).await.is_err());
}

#[tokio::test]
async fn expiration_generation_and_account_scoped_revocation() {
    let f=Fixture::new().await;
    let (_,one)=f.tokens(false).await;
    let (_,two)=f.tokens(true).await;
    let first=f.s.mcp_authority(one["access_token"].as_str().unwrap(),false).await.unwrap();
    let second=f.s.mcp_authority(two["access_token"].as_str().unwrap(),true).await.unwrap();
    let revoked=f.s.oauth_revoke(f.a.clone(),first.grant_id().into()).await.unwrap();
    assert_eq!(revoked.account_id,f.a.identity().id);
    assert!(f.s.mcp_recheck(first,false).await.is_err());
    f.s.mcp_recheck(second.clone(),true).await.unwrap();
    let listing=f.s.oauth_grants(f.a.clone()).await.unwrap();
    assert_eq!(listing.account_id,f.a.identity().id);
    assert_eq!(listing.grants.len(),1);
    let absent=f.s.oauth_revoke(f.a.clone(),crypto::random_token().unwrap()).await.unwrap();
    assert_eq!(absent.account_id,f.a.identity().id);
    let hash=crypto::bearer(two["access_token"].as_str().unwrap()).unwrap();
    f.s.metadata.run(move |c| {c.execute("UPDATE oauth_tokens SET expires_at=?2 WHERE hash=?1",params![hash.as_slice(),now()])?;Ok(())}).await.unwrap();
    assert!(f.s.mcp_recheck(second,true).await.is_err());
    // Access expiry leaves an unexpired refresh usable.
    let rotated=serde_json::to_value(f.s.oauth_token(refresh_request(two["refresh_token"].as_str().unwrap())).await.unwrap()).unwrap();
    let verifier=crypto::random_token().unwrap();
    let outstanding=f.code(false,&verifier).await;
    let id=f.a.identity().id.clone();
    f.s.metadata.run(move |c| {
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE accounts SET generation=generation+1 WHERE id=?1",[&id])?;
        revoke(&tx,&id)?;
        tx.commit()?;Ok(())
    }).await.unwrap();
    assert!(f.s.mcp_authority(rotated["access_token"].as_str().unwrap(),false).await.is_err());
    assert!(f.s.oauth_token(refresh_request(rotated["refresh_token"].as_str().unwrap())).await.is_err());
    assert!(f.s.oauth_token(exchange(&outstanding,&verifier)).await.is_err());
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn absent_grant_revoke_revalidates_original_session_after_barrier_admission() {
    let f=Fixture::new().await;
    let pause=f.s.hooks.arm("oauth_revoke_with_barrier");
    let s=f.s.clone();let a=f.a.clone();
    let revoke=tokio::spawn(async move {s.oauth_revoke(a,"absent".into()).await});
    tokio::time::timeout(std::time::Duration::from_secs(5),pause.arrived()).await.unwrap();
    // Browser logout needs no collection barrier. The committing recheck must
    // reject it even though the grant lookup could never change a row.
    f.s.logout(&f.session).await.unwrap();
    pause.resume();
    let result=tokio::time::timeout(std::time::Duration::from_secs(5),revoke).await.unwrap().unwrap();
    assert_eq!(result.err().unwrap().code,"authentication_failed");
}

#[tokio::test]
async fn expiry_is_exclusive_and_family_expiration_never_slides() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let code=f.code(false,&verifier).await;
    let hash=crypto::bearer(&code).unwrap();
    f.s.metadata.run(move |c| {c.execute("UPDATE oauth_codes SET expires_at=?2 WHERE hash=?1",params![hash.as_slice(),now()])?;Ok(())}).await.unwrap();
    assert!(f.s.oauth_token(exchange(&code,&verifier)).await.is_err());
    let (_,tokens)=f.tokens(true).await;
    let authority=f.s.mcp_authority(tokens["access_token"].as_str().unwrap(),false).await.unwrap();
    let grant=authority.grant_id().to_owned();
    let before=f.s.metadata.run({let grant=grant.clone();move |c| Ok(c.query_row("SELECT expires_at FROM oauth_grants WHERE id=?1",[grant],|r|r.get::<_,i64>(0))?)}).await.unwrap();
    f.s.oauth_token(refresh_request(tokens["refresh_token"].as_str().unwrap())).await.unwrap();
    f.s.metadata.run(move |c| {
        let after:i64=c.query_row("SELECT expires_at FROM oauth_grants WHERE id=?1",[&grant],|r|r.get(0))?;
        assert_eq!(before,after);
        // Preserve schema constraints while expiring the entire family.
        c.execute("UPDATE oauth_grants SET created_at=?2-2592000,expires_at=?2 WHERE id=?1",params![grant,now()])?;
        Ok(())
    }).await.unwrap();
    assert!(f.s.mcp_recheck(authority,false).await.is_err());
    assert!(f.s.oauth_token(refresh_request(tokens["refresh_token"].as_str().unwrap())).await.is_err());
}

#[cfg(feature="test-hooks")]
#[tokio::test]
async fn replay_and_revoke_serialize_with_final_authority_check_and_durable_work() {
    let f=Fixture::new().await;
    // Model the future adapter's final-check/commit scope with a durable marker
    // in generated metadata, never claim collection-operation race coverage.
    f.s.metadata.run(|c| {c.execute("CREATE TABLE oauth_test_commits(id TEXT)",[])?;Ok(())}).await.unwrap();
    for replay in [false,true] {
        let (_,tokens)=f.tokens(true).await;
        let a=f.s.mcp_authority(tokens["access_token"].as_str().unwrap(),true).await.unwrap();
        let refresh=tokens["refresh_token"].as_str().unwrap().to_owned();
        if replay {f.s.oauth_token(refresh_request(&refresh)).await.unwrap();}
        let gate=f.s.mcp_publication_barrier(&a).await.unwrap();
        let held=gate.enter().await;
        let s=f.s.clone(); let browser=f.a.clone(); let grant=a.grant_id().to_owned();
        // Start invalidation while final write authority owns the barrier.
        let invalidation=tokio::spawn(async move {
            if replay { assert!(s.oauth_token(refresh_request(&refresh)).await.is_err()); }
            else {s.oauth_revoke(browser,grant).await.unwrap();}
        });
        gate.waiter_pending().await;
        assert!(!invalidation.is_finished());
        let checked=a.clone();
        f.s.metadata.run(move |c| {
            check_authority(c,&checked,true)?;
            c.execute("INSERT INTO oauth_test_commits VALUES(?1)",[checked.grant_id()])?;
            Ok(())
        }).await.unwrap();
        drop(held);
        invalidation.await.unwrap();
        assert!(f.s.mcp_recheck(a.clone(),true).await.is_err());
        f.s.metadata.run(move |c| {
            assert_eq!(c.query_row("SELECT count(*) FROM oauth_test_commits WHERE id=?1",[a.grant_id()],|r|r.get::<_,i64>(0))?,1);
            Ok(())
        }).await.unwrap();
    }
    // Reverse order: invalidation completes while work waits for its barrier;
    // the next final check fails and no marker is written.
    let (_,tokens)=f.tokens(true).await;
    let a=f.s.mcp_authority(tokens["access_token"].as_str().unwrap(),true).await.unwrap();
    let gate=f.s.mcp_publication_barrier(&a).await.unwrap();
    f.s.oauth_revoke(f.a.clone(),a.grant_id().into()).await.unwrap();
    let held=gate.enter().await;
    let checked=a.clone();
    assert!(f.s.metadata.run(move |c| {
        check_authority(c,&checked,true)?;
        c.execute("INSERT INTO oauth_test_commits VALUES(?1)",[checked.grant_id()])?;Ok(())
    }).await.is_err());
    drop(held);
}

#[tokio::test]
async fn credentials_and_replay_survive_restart_without_plaintext_storage() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let code=f.code(true,&verifier).await;
    let tokens=serde_json::to_value(f.s.oauth_token(exchange(&code,&verifier)).await.unwrap()).unwrap();
    let access=tokens["access_token"].as_str().unwrap().to_owned();
    let refresh=tokens["refresh_token"].as_str().unwrap().to_owned();
    let code_hash=crypto::bearer(&code).unwrap();
    let access_hash=crypto::bearer(&access).unwrap();
    let refresh_hash=crypto::bearer(&refresh).unwrap();
    f.s.metadata.run(move |c| {
        let stored:Vec<u8>=c.query_row("SELECT hash FROM oauth_codes WHERE hash=?1",[code_hash.as_slice()],|r|r.get(0))?;
        assert_eq!(stored,code_hash);
        for hash in [access_hash,refresh_hash] {
            let stored:Vec<u8>=c.query_row("SELECT hash FROM oauth_tokens WHERE hash=?1",[hash.as_slice()],|r|r.get(0))?;
            assert_eq!(stored,hash);
        }
        Ok(())
    }).await.unwrap();
    f.s.logout(&f.session).await.unwrap();
    f.s.mcp_authority(&access,true).await.unwrap();
    let Fixture {_temp,s,a,session:_}=f;
    drop(a); drop(s);
    let root=DataRoot::acquire(&_temp.path().join("owner"),false).unwrap();
    let s=Service::open(root,Config::new(ORIGIN,"127.0.0.1:0".parse().unwrap()).unwrap()).await.unwrap();
    s.mcp_authority(&access,true).await.unwrap();
    assert!(s.oauth_token(exchange(&code,&verifier)).await.is_err());
    assert!(s.mcp_authority(&access,false).await.is_err());
    assert!(s.oauth_token(refresh_request(&refresh)).await.is_err());
}

#[tokio::test]
async fn password_replacement_invalidates_pending_codes_and_existing_grants() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let code=f.code(false,&verifier).await;
    let handle=f.pending(true,&verifier).await;
    f.s.oauth_consent_read(f.a.clone(),&handle).await.unwrap();
    let (_,tokens)=f.tokens(true).await;
    f.s.change_password("127.0.0.1".parse().unwrap(),f.a.clone(),"generated password for oauth".into(),"generated replacement password".into()).await.unwrap();
    assert!(f.s.oauth_consent_decide(f.a.clone(),&handle,"read").await.is_err());
    assert!(f.s.oauth_token(exchange(&code,&verifier)).await.is_err());
    assert!(f.s.mcp_authority(tokens["access_token"].as_str().unwrap(),false).await.is_err());
    assert!(f.s.oauth_token(refresh_request(tokens["refresh_token"].as_str().unwrap())).await.is_err());
    let (_,session)=f.s.browser_login("127.0.0.1".parse().unwrap(),"generated.admin","generated replacement password".into()).await.unwrap();
    let a=f.s.browser_authority(&session).await.unwrap();
    assert!(f.s.oauth_grants(a).await.unwrap().grants.is_empty());
    f.s.metadata.files(|r| r.check_initialized()).await.unwrap();
}

#[tokio::test]
async fn bounded_pending_state_prunes_expiry_and_refuses_live_overflow() {
    let f=Fixture::new().await;
    let verifier=crypto::random_token().unwrap();
    let json=serde_json::to_string(&request(false,&verifier)).unwrap();
    let digests:Vec<_>=(0..RECORD_LIMIT).map(|n| crypto::digest(&n.to_le_bytes())).collect();
    f.s.metadata.run(move |c| {
        let tx=c.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut q=tx.prepare("INSERT INTO oauth_pending(hash,request,expires_at) VALUES(?1,?2,?3)")?;
            for hash in digests {
                q.execute(params![hash.as_slice(),json,now()+600])?;
            }
        }
        tx.commit()?;Ok(())
    }).await.unwrap();
    assert_eq!(f.s.oauth_begin(request(false,&verifier)).await.err().unwrap().code,"rate_limited");
    f.s.metadata.run(|c| {c.execute("UPDATE oauth_pending SET expires_at=?1",[now()])?;Ok(())}).await.unwrap();
    f.s.oauth_begin(request(false,&verifier)).await.unwrap();
    f.s.metadata.run(|c| {
        assert_eq!(c.query_row("SELECT count(*) FROM oauth_pending",[],|r|r.get::<_,i64>(0))?,1);
        Ok(())
    }).await.unwrap();
    let missing=f.s.mcp_challenge(BearerFailure::Missing,false);
    assert_eq!(missing.status,axum::http::StatusCode::UNAUTHORIZED);
    assert!(missing.value.contains("scope=\"collection:read collection:write\""));
    assert!(missing.value.contains(&format!("resource_metadata=\"{ORIGIN}/.well-known/oauth-protected-resource/mcp\"")));
    assert!(!missing.value.contains("error="));
    let invalid=f.s.mcp_challenge(BearerFailure::Invalid,false);
    assert!(invalid.value.contains("error=\"invalid_token\""));
    let denied=f.s.mcp_challenge(BearerFailure::InsufficientScope,true);
    assert_eq!(denied.status,axum::http::StatusCode::FORBIDDEN);
    assert!(denied.value.contains("error=\"insufficient_scope\""));
    assert!(denied.value.contains("scope=\"collection:read collection:write\""));
}

#[cfg(feature="test-hooks")]
#[test]
fn oauth_migration_is_atomic_and_reentrant() {
    let temp=tempfile::tempdir().unwrap();
    let mut root=DataRoot::acquire(&temp.path().join("owner"),true).unwrap();
    root.init().unwrap();root.upgrade_media().unwrap();root.upgrade_deletions().unwrap();
    assert!(crate::test_hooks::storage_failure("oauth_migration_before_commit",||root.upgrade_oauth()).is_err());
    let c=crate::storage::open(root.database(),false).unwrap();
    assert_eq!(c.pragma_query_value::<i64,_>(None,"user_version",|r|r.get(0)).unwrap(),3);
    assert!(!c.prepare("SELECT 1 FROM sqlite_schema WHERE name LIKE 'oauth_%'").unwrap().exists([]).unwrap());
    drop(c);
    root.check_initialized().unwrap();
    root.upgrade_oauth().unwrap();root.upgrade_oauth().unwrap();root.check_initialized().unwrap();
}

#[tokio::test]
async fn reset_data_preserves_oauth_grants_and_tokens_but_fences_captured_authority() {
    let f=Fixture::new().await;
    let (_,tokens)=f.tokens(true).await;
    let token=tokens["access_token"].as_str().unwrap();
    let original=f.s.mcp_authority(token,true).await.unwrap();
    let grants=serde_json::to_value(f.s.oauth_grants(f.a.clone()).await.unwrap()).unwrap();
    let reset=f.s.reset_data(f.a.clone(),f.a.identity().id.clone()).await.unwrap();
    assert_eq!(reset.storage.unwrap().media_bytes,0);
    assert_eq!(f.s.mcp_recheck(original,true).await.unwrap_err().code,"invalid_token");
    let current=f.s.mcp_authority(token,true).await.unwrap();
    f.s.mcp_recheck(current,true).await.unwrap();
    assert_eq!(serde_json::to_value(f.s.oauth_grants(f.a.clone()).await.unwrap()).unwrap(),grants);
    f.s.oauth_token(refresh_request(tokens["refresh_token"].as_str().unwrap())).await.unwrap();
    assert_eq!(f.s.me(f.a.clone()).await.unwrap().id,f.a.identity().id);
}
