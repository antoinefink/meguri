// SPDX-License-Identifier: MIT
use super::*;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub(crate) struct TokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_in: i64,
    refresh_token: String,
    scope: &'static str,
}
struct Pair {
    access: String,
    refresh: String,
    access_hash: [u8;32],
    refresh_hash: [u8;32],
}
impl Pair {
    fn new() -> Result<Self> {
        let access = crypto::random_token()?;
        let refresh = crypto::random_token()?;
        Ok(Self { access_hash:crypto::bearer(&access)?,refresh_hash:crypto::bearer(&refresh)?,access,refresh })
    }
    fn insert(&self, c: &Connection, grant: &str, expires: i64) -> Result<i64> {
        room(c,"oauth_tokens",TOKEN_LIMIT,2)?;
        let access_expiry = (now()+ACCESS_SECONDS).min(expires);
        c.execute("INSERT INTO oauth_tokens(hash,grant_id,kind,expires_at) VALUES(?1,?2,'access',?3)",params![self.access_hash.as_slice(),grant,access_expiry])?;
        c.execute("INSERT INTO oauth_tokens(hash,grant_id,kind,expires_at) VALUES(?1,?2,'refresh',?3)",params![self.refresh_hash.as_slice(),grant,expires])?;
        Ok(access_expiry)
    }
    fn response(self, write: bool, access_expiry: i64) -> TokenResponse {
        TokenResponse { access_token:self.access,token_type:"Bearer",expires_in:(access_expiry-now()).max(0),refresh_token:self.refresh,scope:scopes(write) }
    }
}
struct Code {
    account: String,
    generation: i64,
    request: Authorization,
    grant: Option<String>,
}
fn code(c: &Connection, hash: &[u8;32]) -> Result<Code> {
    let row: Option<(String,i64,String,Option<String>)> = c.query_row(
        "SELECT account_id,generation,request,grant_id FROM oauth_codes WHERE hash=?1 AND expires_at>?2",
        params![hash.as_slice(),now()],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let (account,generation,json,grant) = row.ok_or(Error::new("invalid_grant"))?;
    let request = serde_json::from_str(&json).map_err(|_| Error::new("storage_unavailable"))?;
    Ok(Code {account,generation,request,grant})
}
fn account_current(c: &Connection, id: &str, generation: i64) -> Result<()> {
    read_account(c,id)?.filter(|a| a.usable() && a.generation == generation).ok_or(Error::new("invalid_grant"))?;
    Ok(())
}
fn code_binding(code: &Code, client: &str, resource: &str, challenge: &str, redirect: Option<&str>) -> Result<()> {
    if code.request.client != client || code.request.resource != resource
        || !crypto::same_digest(code.request.challenge.as_bytes(),challenge.as_bytes())
        || redirect.is_some_and(|r| r != code.request.redirect)
    { return Err(Error::new("invalid_grant")); }
    Ok(())
}
struct Refresh {
    account: String,
    generation: i64,
    grant: String,
    client: String,
    resource: String,
    write: bool,
    expires: i64,
    consumed: bool,
}
fn refresh(c: &Connection, hash: &[u8;32]) -> Result<Refresh> {
    c.query_row("SELECT g.account_id,g.generation,g.id,g.client,g.resource,g.write_scope,g.expires_at,t.consumed FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE t.hash=?1 AND t.kind='refresh' AND t.expires_at>?2 AND g.expires_at>?2 AND g.revoked=0",
        params![hash.as_slice(),now()],|r| Ok(Refresh {account:r.get(0)?,generation:r.get(1)?,grant:r.get(2)?,client:r.get(3)?,resource:r.get(4)?,write:r.get(5)?,expires:r.get(6)?,consumed:r.get(7)?}))
        .optional()?.ok_or(Error::new("invalid_grant"))
}
fn refresh_binding(r: &Refresh, client: Option<&str>, resource: &str) -> Result<()> {
    if client.is_some_and(|c| c != r.client) || resource != r.resource { return Err(Error::new("invalid_grant")); }
    Ok(())
}
impl Service {
    pub(crate) async fn oauth_token(&self, p: BTreeMap<String,String>) -> Result<TokenResponse> {
        match required(&p,"grant_type")? {
            "authorization_code" => self.oauth_exchange(p).await,
            "refresh_token" => self.oauth_refresh(p).await,
            _ => Err(Error::new("unsupported_grant_type")),
        }
    }
    async fn oauth_exchange(&self, p: BTreeMap<String,String>) -> Result<TokenResponse> {
        let hash = secret_hash(required(&p,"code")?,"invalid_grant")?;
        let client_id = required(&p,"client_id")?.to_owned();
        client(&client_id)?;
        let resource = resource(self.config.origin(),required(&p,"resource")?)?;
        let challenge = policy::verifier(required(&p,"code_verifier")?)?;
        let redirect = p.get("redirect_uri").cloned();
        // Resolve target without retaining metadata while taking its barrier.
        let target = self.metadata.run({
            let client_id=client_id.clone(); let resource=resource.clone();
            let challenge=challenge.clone(); let redirect=redirect.clone();
            move |c| {
                admission(c)?;
                let row=code(c,&hash)?;
                code_binding(&row,&client_id,&resource,&challenge,redirect.as_deref())?;
                account_current(c,&row.account,row.generation)?;
                Ok(row.account)
            }
        }).await?;
        let pair = Pair::new()?;
        let grant_id = crypto::random_token()?;
        let gate = self.barriers.get(&target)?;
        let guard = gate.enter().await;
        self.pause("oauth_code_with_barrier").await;
        let result = self.metadata.run(move |c| {
            let _publication = (gate,guard);
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            admission(&tx)?;
            let row=code(&tx,&hash)?;
            code_binding(&row,&client_id,&resource,&challenge,redirect.as_deref())?;
            account_current(&tx,&row.account,row.generation)?;
            if row.account != target { return Err(Error::new("invalid_grant")); }
            if let Some(grant) = row.grant {
                // Commit revocation before reporting invalid_grant. Returning an
                // error from this scope would roll back replay invalidation.
                tx.execute("UPDATE oauth_grants SET revoked=1 WHERE id=?1 AND account_id=?2",params![grant,target])?;
                tx.commit()?;
                return Ok(None);
            }
            prune(&tx)?;
            room(&tx,"oauth_grants",RECORD_LIMIT,1)?;
            let t=now();
            let expires=t+FAMILY_SECONDS;
            tx.execute("INSERT INTO oauth_grants(id,account_id,generation,client,resource,write_scope,created_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![grant_id,row.account,row.generation,row.request.client,row.request.resource,row.request.write,t,expires])?;
            let access_expiry=pair.insert(&tx,&grant_id,expires)?;
            tx.execute("UPDATE oauth_codes SET grant_id=?2 WHERE hash=?1",params![hash.as_slice(),grant_id])?;
            tx.commit()?;
            Ok(Some(pair.response(row.request.write,access_expiry)))
        }).await?;
        result.ok_or(Error::new("invalid_grant"))
    }
    async fn oauth_refresh(&self, p: BTreeMap<String,String>) -> Result<TokenResponse> {
        let hash=secret_hash(required(&p,"refresh_token")?,"invalid_grant")?;
        let resource=resource(self.config.origin(),required(&p,"resource")?)?;
        let client_id=p.get("client_id").cloned();
        let requested_scope=p.get("scope").cloned();
        let target=self.metadata.run({
            let client_id=client_id.clone(); let resource=resource.clone();
            move |c| {
                admission(c)?;
                let row=refresh(c,&hash)?;
                refresh_binding(&row,client_id.as_deref(),&resource)?;
                account_current(c,&row.account,row.generation)?;
                Ok(row.account)
            }
        }).await?;
        let pair=Pair::new()?;
        let gate=self.barriers.get(&target)?;
        let guard=gate.enter().await;
        self.pause("oauth_refresh_with_barrier").await;
        let result=self.metadata.run(move |c| {
            let _publication=(gate,guard);
            let tx=c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            admission(&tx)?;
            let row=refresh(&tx,&hash)?;
            refresh_binding(&row,client_id.as_deref(),&resource)?;
            account_current(&tx,&row.account,row.generation)?;
            if row.account != target { return Err(Error::new("invalid_grant")); }
            if row.consumed {
                tx.execute("UPDATE oauth_grants SET revoked=1 WHERE id=?1 AND account_id=?2",params![row.grant,target])?;
                tx.commit()?;
                return Ok(None);
            }
            // The grant's consent is immutable. A refresh cannot alter it.
            let requested_scope=requested_scope.as_deref().map(policy::scope).transpose()?;
            if requested_scope.is_some_and(|w| w != row.write) { return Err(Error::new("invalid_scope")); }
            prune(&tx)?;
            let access_expiry=pair.insert(&tx,&row.grant,row.expires)?;
            tx.execute("UPDATE oauth_tokens SET consumed=1 WHERE hash=?1",[hash.as_slice()])?;
            tx.commit()?;
            Ok(Some(pair.response(row.write,access_expiry)))
        }).await?;
        result.ok_or(Error::new("invalid_grant"))
    }
}
