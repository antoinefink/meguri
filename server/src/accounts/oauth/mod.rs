// SPDX-License-Identifier: MIT
//! Collection OAuth authority; no collection operations or native credentials.
mod policy;
mod tokens;
#[cfg(test)]
mod tests;
pub(crate) use policy::{Authorization, BOTH, READ, callback, client, direct_file_client, form, required, resource, scopes};
use super::*;
use rusqlite::TransactionBehavior;
use serde::{Deserialize, Serialize};

const PENDING_SECONDS: i64 = 600;
const CODE_SECONDS: i64 = 60;
const ACCESS_SECONDS: i64 = 900;
const FAMILY_SECONDS: i64 = 30 * 86400;
const RECORD_LIMIT: i64 = 10_000;
const TOKEN_LIMIT: i64 = 100_000;

fn secret_hash(value: &str, failure: &'static str) -> Result<[u8;32]> {
    crypto::bearer(value).map_err(|_| Error::new(failure))
}
fn timestamp(value: i64) -> Result<String> {
    time::OffsetDateTime::from_unix_timestamp(value)
        .map_err(|_| Error::new("storage_unavailable"))?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| Error::new("storage_unavailable"))
}
/// Must be called only in a short writer. Pruning never discards live replay
/// correlations, and global caps bound retained state across process restarts.
fn prune(c: &Connection) -> Result<()> {
    let t = now();
    c.execute("DELETE FROM oauth_pending WHERE expires_at<=?1", [t])?;
    c.execute("DELETE FROM oauth_codes WHERE expires_at<=?1", [t])?;
    c.execute("DELETE FROM oauth_grants WHERE expires_at<=?1", [t])?;
    c.execute("DELETE FROM oauth_tokens WHERE kind='access' AND expires_at<=?1", [t])?;
    Ok(())
}
fn room(c: &Connection, table: &'static str, limit: i64, additional: i64) -> Result<()> {
    // Table names are internal constants, never protocol data.
    let count: i64 = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
    if count + additional > limit { return Err(Error::limited(60)); }
    Ok(())
}
#[derive(Serialize)]
pub(crate) struct ConsentPreview { client_name: &'static str }
#[derive(Serialize)]
pub(crate) struct Consent {
    username: String,
    client_name: &'static str,
    redirect_host: String,
    access_options: Vec<&'static str>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadRequest { pub request: String }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionRequest { pub request: String, pub decision: String }
#[derive(Serialize)]
pub(crate) struct GrantSummary {
    id: String,
    client_name: &'static str,
    access: &'static str,
    created_at: String,
}
#[derive(Serialize)]
pub(crate) struct GrantList {
    account_id: String,
    grants: Vec<GrantSummary>,
}
#[derive(Serialize)]
pub(crate) struct RevokeResult { account_id: String }
fn pending(c: &Connection, hash: &[u8;32], a: &BrowserAuthority) -> Result<Authorization> {
    let row: Option<(String,Option<Vec<u8>>,Option<String>,Option<i64>)> = c.query_row(
        "SELECT request,session,account_id,generation FROM oauth_pending WHERE hash=?1 AND expires_at>?2",
        params![hash.as_slice(),now()], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let (json, session, id, generation) = row.ok_or(Error::new("capability_invalid"))?;
    // A decision cannot be the first binding read.
    if !session.is_some_and(|s| crypto::same_digest(&s,&a.hash))
        || id.as_deref() != Some(a.account.identity.id.as_str()) || generation != Some(a.account.generation)
    { return Err(Error::new("capability_invalid")); }
    serde_json::from_str(&json).map_err(|_| Error::new("storage_unavailable"))
}
impl Service {
    /// Anonymous display only. This read never binds, consumes or approves a request.
    pub(crate) async fn oauth_consent_preview(&self, handle: &str) -> Result<ConsentPreview> {
        let hash = secret_hash(handle,"capability_invalid")?;
        self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            let json: String = tx.query_row(
                "SELECT p.request FROM oauth_pending p WHERE p.hash=?1 AND p.expires_at>?2
                 AND (p.session IS NULL OR EXISTS (
                   SELECT 1 FROM credentials c JOIN accounts a ON a.id=c.account_id
                   WHERE c.hash=p.session AND c.kind='session' AND c.revoked=0 AND c.expires_at>?2
                   AND c.account_id=p.account_id AND c.generation=p.generation
                   AND a.generation=p.generation AND a.enabled=1 AND a.provisioning_state='ready'))",
                params![hash.as_slice(),now()], |r| r.get(0))
                .optional()?.ok_or(Error::new("capability_invalid"))?;
            let request: Authorization = serde_json::from_str(&json).map_err(|_| Error::new("storage_unavailable"))?;
            let identity = client(&request.client).map_err(|_| Error::new("capability_invalid"))?;
            Ok(ConsentPreview { client_name: identity.name })
        }).await
    }
    pub(crate) async fn oauth_begin(&self, request: Authorization) -> Result<String> {
        let handle = crypto::random_token()?;
        let hash = crypto::bearer(&handle)?;
        let json = serde_json::to_string(&request).map_err(|_| Error::new("internal_error"))?;
        self.metadata.run(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            admission(&tx)?;
            prune(&tx)?;
            room(&tx,"oauth_pending",RECORD_LIMIT,1)?;
            tx.execute("INSERT INTO oauth_pending(hash,request,expires_at) VALUES(?1,?2,?3)", params![hash.as_slice(),json,now()+PENDING_SECONDS])?;
            tx.commit()?;
            Ok(())
        }).await?;
        Ok(format!("{}/oauth/consent#request={handle}",self.config.origin()))
    }
    pub(crate) async fn oauth_consent_read(&self, authority: BrowserAuthority, handle: &str) -> Result<Consent> {
        let hash = secret_hash(handle,"capability_invalid")?;
        self.metadata.run(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            admission(&tx)?;
            let account = check_browser(&tx,&authority,"authentication_failed")?;
            tx.execute("UPDATE oauth_pending SET session=?2,account_id=?3,generation=?4 WHERE hash=?1 AND session IS NULL AND expires_at>?5",
                params![hash.as_slice(),authority.hash.as_slice(),account.identity.id,account.generation,now()])?;
            let request = pending(&tx,&hash,&authority)?;
            let client = client(&request.client)?;
            let result = Consent { username:account.identity.username,client_name:client.name,
                redirect_host:url::Url::parse(client.redirect).expect("reviewed URI").host_str().expect("reviewed host").into(),
                access_options:if request.write { vec!["read","read_write"] } else { vec!["read"] } };
            tx.commit()?;
            Ok(result)
        }).await
    }
    pub(crate) async fn oauth_consent_decide(&self, authority: BrowserAuthority, handle: &str, decision: &str) -> Result<String> {
        let hash = secret_hash(handle,"capability_invalid")?;
        let decision = decision.to_owned();
        let code = crypto::random_token()?;
        let code_hash = crypto::bearer(&code)?;
        let origin = self.config.origin().to_owned();
        self.metadata.run(move |c| {
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            admission(&tx)?;
            let account = check_browser(&tx,&authority,"authentication_failed")?;
            let mut request = pending(&tx,&hash,&authority)?;
            if decision != "deny" && decision != "read" && !(decision == "read_write" && request.write) {
                return Err(Error::new("invalid_request"));
            }
            let redirect = if decision == "deny" {
                request.callback(&origin,"error","access_denied")
            } else {
                prune(&tx)?;
                room(&tx,"oauth_codes",RECORD_LIMIT,1)?;
                request.write = decision == "read_write";
                let json = serde_json::to_string(&request).map_err(|_| Error::new("internal_error"))?;
                tx.execute("INSERT INTO oauth_codes(hash,account_id,generation,request,expires_at) VALUES(?1,?2,?3,?4,?5)",
                    params![code_hash.as_slice(),account.identity.id,account.generation,json,now()+CODE_SECONDS])?;
                request.callback(&origin,"code",&code)
            };
            tx.execute("DELETE FROM oauth_pending WHERE hash=?1",[hash.as_slice()])?;
            tx.commit()?;
            Ok(redirect)
        }).await
    }
    pub(crate) async fn oauth_grants(&self, authority: BrowserAuthority) -> Result<GrantList> {
        self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            let account = check_browser(&tx,&authority,"authentication_failed")?;
            let mut q = tx.prepare("SELECT id,client,write_scope,created_at FROM oauth_grants WHERE account_id=?1 AND generation=?2 AND revoked=0 AND expires_at>?3 ORDER BY created_at,id")?;
            let mut rows = q.query(params![account.identity.id,account.generation,now()])?;
            let mut grants = Vec::new();
            while let Some(row) = rows.next()? {
                let client_id: String = row.get(1)?;
                grants.push(GrantSummary { id:row.get(0)?,client_name:client(&client_id)?.name,
                    access:if row.get::<_,bool>(2)? {"read_write"} else {"read"},created_at:timestamp(row.get(3)?)? });
            }
            Ok(GrantList { account_id:account.identity.id,grants })
        }).await
    }
    pub(crate) async fn oauth_revoke(&self, authority: BrowserAuthority, grant: String) -> Result<RevokeResult> {
        // Resolve only the requester before taking its barrier. Even absent or
        // foreign IDs revalidate that original authority in the committing writer.
        let target = self.metadata.run({
            let a = authority.clone();
            move |c| {
                admission(c)?;
                let account = check_browser(c,&a,"authentication_failed")?;
                Ok(account.identity.id)
            }
        }).await?;
        let gate = self.barriers.get(&target)?;
        let guard = gate.enter().await;
        self.pause("oauth_revoke_with_barrier").await;
        self.metadata.run(move |c| {
            let _publication = (gate,guard);
            let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
            admission(&tx)?;
            let account = check_browser(&tx,&authority,"authentication_failed")?;
            tx.execute("UPDATE oauth_grants SET revoked=1 WHERE id=?1 AND account_id=?2",params![grant,account.identity.id])?;
            tx.commit()?;
            Ok(RevokeResult { account_id:account.identity.id })
        }).await
    }
}

/// This authority cannot be converted to native sync authority. The adapter
/// derives account/storage selection only from account_id(), never tool input.
#[derive(Clone)]
pub(crate) struct Authority {
    id: String,
    generation: i64,
    data_epoch: i64,
    grant: String,
    client: String,
    resource: String,
    write: bool,
    hash: [u8;32],
    expires: i64,
}
impl Authority {
    pub(crate) fn account_id(&self) -> &str { &self.id }
    pub(crate) fn grant_id(&self) -> &str { &self.grant }
    pub(crate) fn client_id(&self) -> &str { &self.client }
    pub(crate) fn can_write(&self) -> bool { self.write }
    pub(crate) fn same_upload_binding(&self, other: &Self) -> bool {
        self.id == other.id && self.generation == other.generation && self.data_epoch == other.data_epoch
            && self.grant == other.grant && self.client == other.client && self.resource == other.resource && self.write && other.write
    }
}
fn resolve_authority(c: &Connection, hash: [u8;32]) -> Result<Authority> {
    let mut authority = c.query_row("SELECT g.account_id,g.generation,g.id,g.client,g.resource,g.write_scope,t.expires_at FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN accounts a ON a.id=g.account_id WHERE t.hash=?1 AND t.kind='access' AND t.consumed=0 AND t.expires_at>?2 AND g.expires_at>?2 AND g.revoked=0 AND a.enabled=1 AND a.provisioning_state='ready' AND a.generation=g.generation",
        params![hash.as_slice(),now()],|r| Ok(Authority {data_epoch:0,id:r.get(0)?,generation:r.get(1)?,grant:r.get(2)?,client:r.get(3)?,resource:r.get(4)?,write:r.get(5)?,expires:r.get(6)?,hash}))
        .optional()?.ok_or(Error::new("invalid_token"))?;
    authority.data_epoch = crate::storage::reset::admitted_epoch(c, &authority.id)?;
    Ok(authority)
}
/// Synchronous seam when a collection worker already has a metadata read. This
/// does not acquire a publication barrier. Own that account's barrier before
/// the final write check and retain it through durable collection commit.
pub(crate) fn check_authority(c: &Connection, original: &Authority, write: bool) -> Result<()> {
    admission(c)?;
    let a = resolve_authority(c,original.hash)?;
    if a.data_epoch != original.data_epoch || a.id != original.id || a.generation != original.generation || a.grant != original.grant
        || a.client != original.client || a.resource != original.resource || a.write != original.write || a.expires != original.expires
    { return Err(Error::new("invalid_token")); }
    if write && !a.write { return Err(Error::new("insufficient_scope")); }
    Ok(())
}
/// Pending browser uploads survive access-token rotation, but never their grant,
/// credential generation, reset epoch, or grant expiry. No bearer is substituted.
pub(crate) fn check_upload_binding(c: &Connection, original: &Authority) -> Result<()> {
    admission(c)?;
    let valid: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM oauth_grants g JOIN accounts a ON a.id=g.account_id WHERE g.id=?1 AND g.account_id=?2 AND g.generation=?3 AND g.client=?4 AND g.resource=?5 AND g.write_scope=1 AND g.revoked=0 AND g.expires_at>?6 AND a.generation=g.generation AND a.enabled=1 AND a.provisioning_state='ready')",
        params![original.grant, original.id, original.generation, original.client, original.resource, now()], |r| r.get(0))?;
    if !valid || crate::storage::reset::admitted_epoch(c, &original.id)? != original.data_epoch { return Err(Error::new("capability_invalid")); }
    Ok(())
}
impl Service {
    pub(crate) async fn mcp_authority(&self, token: &str, write: bool) -> Result<Authority> {
        let hash = secret_hash(token,"invalid_token")?;
        let resource = format!("{}/mcp",self.config.origin());
        self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            let a = resolve_authority(&tx,hash)?;
            if a.resource != resource { return Err(Error::new("invalid_token")); }
            if write && !a.write { return Err(Error::new("insufficient_scope")); }
            Ok(a)
        }).await
    }
    /// Recheck the original bearer, never substitute a newly resolved grant.
    /// Call at collection admission and immediately before commit; for reads,
    /// check original publication plus this authority before releasing results.
    pub(crate) async fn mcp_recheck(&self, authority: Authority, write: bool) -> Result<()> {
        self.metadata.run(move |c| {
            let tx = c.transaction()?;
            check_authority(&tx,&authority,write)
        }).await
    }
    /// Resolve/check, release metadata, then return the shared account barrier.
    /// Move the Arc and its guard into the blocking collection worker so that
    /// cancellation cannot release ownership before its final check/commit.
    pub(crate) async fn mcp_publication_barrier(&self, authority: &Authority) -> Result<Arc<PublicationBarrier>> {
        self.mcp_recheck(authority.clone(),false).await?;
        self.barriers.get(authority.account_id())
    }
}

/// Called under the account lifecycle writer (and its publication barrier).
/// Legacy v1-v3 stopped recovery has no OAuth records to invalidate.
pub(crate) fn revoke_account(c: &Connection, id: &str) -> Result<()> {
    let version: i64 = c.pragma_query_value(None,"user_version",|r| r.get(0))?;
    if version < 4 { return Ok(()); }
    c.execute("UPDATE oauth_grants SET revoked=1 WHERE account_id=?1",[id])?;
    c.execute("DELETE FROM oauth_codes WHERE account_id=?1",[id])?;
    c.execute("DELETE FROM oauth_pending WHERE account_id=?1",[id])?;
    Ok(())
}

/// Fail closed on unsupported restored records before serving or migration.
pub(crate) fn validate(c: &Connection) -> Result<()> {
    let bad = || Error::new("storage_unavailable");
    for table in ["oauth_pending","oauth_codes"] {
        let mut q = c.prepare(&format!("SELECT request FROM {table}"))?;
        let mut rows = q.query([])?;
        while let Some(row) = rows.next()? {
            let json: String = row.get(0)?;
            let request: Authorization = serde_json::from_str(&json).map_err(|_| bad())?;
            let identity = client(&request.client).map_err(|_| bad())?;
            let url = url::Url::parse(&request.resource).map_err(|_| bad())?;
            if identity.redirect != request.redirect || resource(&url.origin().ascii_serialization(),&request.resource).is_err()
                || policy::challenge(&request.challenge).is_err() { return Err(bad()); }
        }
    }
    let mut q = c.prepare("SELECT id,client,resource FROM oauth_grants")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let identity: String = row.get(1)?;
        let target: String = row.get(2)?;
        let url = url::Url::parse(&target).map_err(|_| bad())?;
        if crypto::bearer(&id).is_err() || client(&identity).is_err()
            || resource(&url.origin().ascii_serialization(),&target).is_err() { return Err(bad()); }
    }
    if c.prepare("SELECT 1 FROM oauth_grants g JOIN accounts a ON a.id=g.account_id WHERE g.generation>a.generation OR (g.revoked=0 AND (g.generation<>a.generation OR a.enabled<>1 OR a.provisioning_state<>'ready'))")?.exists([])? {
        return Err(bad());
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum BearerFailure { Missing, Invalid, InsufficientScope }
/// Both HTTP challenges and any hosted-client tool-level challenge must reuse
/// this server-built value. No request strings are interpolated in the header.
pub(crate) struct BearerChallenge {
    pub status: axum::http::StatusCode,
    pub value: String,
}
impl Service {
    pub(crate) fn mcp_challenge(&self, failure: BearerFailure, write: bool) -> BearerChallenge {
        use axum::http::StatusCode;
        let (status,error,scope) = match failure {
            BearerFailure::Missing => (StatusCode::UNAUTHORIZED,"",BOTH),
            BearerFailure::Invalid => (StatusCode::UNAUTHORIZED,", error=\"invalid_token\", error_description=\"Credential unavailable\"",BOTH),
            BearerFailure::InsufficientScope => (StatusCode::FORBIDDEN,", error=\"insufficient_scope\", error_description=\"Additional collection permission required\"",scopes(write)),
        };
        BearerChallenge { status,value:format!("Bearer resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", scope=\"{scope}\"{error}",self.config.origin()) }
    }
}
