// SPDX-License-Identifier: MIT
use super::provision::{read_cap, valid_cap};
use super::*;
#[derive(Serialize)]
pub struct CapabilityLink {
    pub url: String,
    pub expires_at: String,
}
impl Service {
    pub async fn accounts(&self, authority: BrowserAuthority) -> Result<Vec<AccountSummary>> {
        let actor = authority.clone();
        let rows = self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            check_admin(&tx, &actor, "authentication_failed")?;
            let mut query = tx.prepare("SELECT id,username,role,enabled,provisioning_state FROM accounts ORDER BY username,id")?;
            let accounts = query.query_map([], |r| Ok(AccountSummary {
                identity: Identity { id: r.get(0)?, username: r.get(1)?, role: r.get(2)? },
                enabled: r.get(3)?, provisioning_state: r.get(4)?, storage: None,
            }))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(accounts.into_iter().map(|account| {
                let media = if account.provisioning_state == "ready" {
                    crate::media::index::storage_bytes(&tx, &account.identity.id).ok()
                } else { None };
                (account, media)
            }).collect::<Vec<_>>())
        }).await?;
        let mut accounts = Vec::with_capacity(rows.len());
        for (mut account, media) in rows {
            if let Some(media_bytes) = media {
                account.storage = self.collection_storage(account.identity.id.clone(), media_bytes).await;
            }
            accounts.push(account);
        }
        self.pause("accounts_before_return").await;
        // Slow filesystem reads must not return data under revoked authority.
        self.metadata.run(move |c| {
            admission(c)?;
            check_admin(c, &authority, "authentication_failed")?;
            Ok(())
        }).await?;
        Ok(accounts)
    }
    pub async fn own_storage(&self, authority: BrowserAuthority) -> Result<Option<AccountStorage>> {
        let actor = authority.clone();
        let (id, media) = self.metadata.run(move |c| {
            let tx = c.transaction()?;
            admission(&tx)?;
            let account = check_browser(&tx, &actor, "authentication_failed")?;
            let id = account.identity.id;
            let media = crate::media::index::storage_bytes(&tx, &id).ok();
            Ok((id, media))
        }).await?;
        // The metadata read is released before acquiring publication ownership.
        let storage = match media {
            Some(bytes) => self.collection_storage(id, bytes).await,
            None => None,
        };
        self.pause("own_storage_before_return").await;
        // Revalidate the same session even when measurement was unavailable.
        self.me(authority).await?;
        Ok(storage)
    }
    async fn collection_storage(&self, id: String, media_bytes: u64) -> Option<AccountStorage> {
        // No metadata writer or other account barrier is held here.
        // Move the guard into the blocking job so cancellation cannot
        // release publication ownership before the snapshot closes.
        let gate = self.barriers.get(&id).ok()?;
        let guard = gate.enter().await;
        self.metadata.files(move |root| {
            let _publication = (gate, guard);
            let path = root.collection_path(&id)?;
            let mut c = crate::collection::schema::open(&path, false)?;
            let tx = c.transaction()?;
            // Establish one read snapshot before reading both PRAGMAs.
            tx.query_row("SELECT count(*) FROM sqlite_schema", [], |r| r.get::<_, i64>(0))?;
            let pages: i64 = tx.pragma_query_value(None, "page_count", |r| r.get(0))?;
            let page_size: i64 = tx.pragma_query_value(None, "page_size", |r| r.get(0))?;
            let pages = u64::try_from(pages).map_err(|_| Error::new("storage_unavailable"))?;
            let page_size = u64::try_from(page_size).map_err(|_| Error::new("storage_unavailable"))?;
            let collection_bytes = pages.checked_mul(page_size)
                .filter(|n| *n > 0).ok_or(Error::new("storage_unavailable"))?;
            let total_bytes = collection_bytes.checked_add(media_bytes)
                .ok_or(Error::new("storage_unavailable"))?;
            Ok(AccountStorage { collection_bytes, media_bytes, total_bytes })
        }).await.ok()
    }
    pub async fn invitation(&self, authority: BrowserAuthority) -> Result<CapabilityLink> {
        self.create_capability(authority, None).await
    }
    pub async fn reset_link(
        &self,
        authority: BrowserAuthority,
        target: String,
    ) -> Result<CapabilityLink> {
        self.create_capability(authority, Some(target)).await
    }
    async fn create_capability(
        &self,
        authority: BrowserAuthority,
        target: Option<String>,
    ) -> Result<CapabilityLink> {
        // Capture original session at the HTTP boundary. A fresh session found
        // after this point cannot substitute for the one that authorized work.
        self.pause("admin_before_commit").await;
        let secret = crypto::random_token()?;
        let hash = crypto::bearer(&secret)?;
        let reset = target.is_some();
        let expires=self.metadata.run(move |c| {
            let tx=c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?; admission(&tx)?;
            let actor=check_admin(&tx,&authority,"stale_authority")?;
            let (kind,expires,target_generation)=if let Some(ref id)=target {
                let a=read_account(&tx,id)?.filter(|a|a.state=="ready").ok_or(Error::new("route_not_found"))?;
                if actor.identity.id == *id { another_admin(&tx, id)?; }
                tx.execute("UPDATE capabilities SET state='revoked' WHERE kind='reset' AND target=?1 AND state IN ('available','provisioning')",[id])?;
                ("reset",now()+1800,Some(a.generation))
            } else { ("invite",now()+7*86400,None) };
            tx.execute("INSERT INTO capabilities(hash,kind,issuer,issuer_generation,target,target_generation,expires_at,state) VALUES(?1,?2,?3,?4,?5,?6,?7,'available')",
                params![hash.as_slice(),kind,actor.identity.id,actor.generation,target,target_generation,expires])?;
            tx.commit()?; Ok(expires)
        }).await?;
        // Test-only response checkpoint after the writer has been released.
        #[cfg(feature = "test-hooks")]
        self.pause("admin_after_commit").await;
        let expires_at = time::OffsetDateTime::from_unix_timestamp(expires)
            .map_err(|_| Error::new("internal_error"))?
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| Error::new("internal_error"))?;
        Ok(CapabilityLink {
            url: format!(
                "{}/{}#token={secret}",
                self.config.origin(),
                if reset { "reset" } else { "invite" }
            ),
            expires_at,
        })
    }
    pub async fn reset_claim(&self, ip: IpAddr, token: String, password: String) -> Result<()> {
        self.attempts.capability(ip, &token)?;
        let hash = crypto::bearer(&token)?;
        let cap = self
            .metadata
            .run(move |c| {
                admission(c)?;
                let cap = read_cap(c, &hash)?;
                valid_cap(c, &cap, "reset")?;
                Ok(cap)
            })
            .await?;
        crypto::new_password(&password).map_err(|_| Error::field("new_password"))?;
        let phc = self.hashes.run(move || crypto::hash(&password)).await?;
        self.pause("reset_after_hash").await;
        let id = cap.target.ok_or(Error::new("capability_invalid"))?;
        let gate = self.barriers.get(&id)?;
        let guard = gate.enter().await;
        self.pause("reset_with_barrier").await;
        self.metadata
            .run(move |c| {
                let _guard = guard;
                let _gate = gate.clone();
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                let cap = read_cap(&tx, &hash)?;
                valid_cap(&tx, &cap, "reset")?;
                if cap.issuer.as_deref() == Some(id.as_str()) {
                    another_admin(&tx, &id)?;
                }
                gate.retire()?;
                replace_password(&tx, &id, &phc)?;
                // replace_password invalidates this and all other target resets;
                // record this particular redemption as consumed atomically.
                tx.execute(
                    "UPDATE capabilities SET state='consumed' WHERE hash=?1",
                    [hash.as_slice()],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await
    }
    pub async fn disable(&self, authority: BrowserAuthority, id: String) -> Result<()> {
        // Resolve first: never populate the gate map from guessed IDs.
        self.metadata
            .run({
                let id = id.clone();
                let actor = authority.clone();
                move |c| {
                    admission(c)?;
                    check_admin(c, &actor, "stale_authority")?;
                    read_account(c, &id)?.ok_or(Error::new("route_not_found"))?;
                    Ok(())
                }
            })
            .await?;
        self.pause("disable_before_commit").await;
        let gate = self.barriers.get(&id)?;
        let guard = gate.enter().await;
        self.pause("disable_with_barrier").await;
        self.metadata
            .run(move |c| {
                let _guard = guard;
                let _gate = gate.clone();
                let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
                admission(&tx)?;
                check_admin(&tx, &authority, "stale_authority")?;
                let target = read_account(&tx, &id)?.ok_or(Error::new("route_not_found"))?;
                if target.usable() && target.identity.role == "admin" {
                    another_admin(&tx, &id)?;
                }
                gate.retire()?;
                tx.execute(
                    "UPDATE accounts SET enabled=0,generation=generation+1 WHERE id=?1",
                    [&id],
                )?;
                revoke(&tx, &id)?;
                tx.commit()?;
                Ok(())
            })
            .await
    }
}

/// Caller owns the committing writer; eligibility cannot change before commit.
fn another_admin(c: &Connection, id: &str) -> Result<()> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE id<>?1 AND role='admin' AND enabled=1 AND provisioning_state='ready')",
        [id], |r| r.get(0))?;
    if !exists {
        return Err(Error::new("sole_administrator"));
    }
    Ok(())
}
